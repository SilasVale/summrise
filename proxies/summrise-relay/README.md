# summrise-relay

An agent that sits behind NAT dials OUT to this process, and this process makes the agent's own `/mcp` and `/panel/` reachable
to whoever can reach the relay. Nothing on the agent side listens for inbound connections, which is the shape every comparable
product uses — Portainer Edge Agent, Microsoft dev tunnels ("No inbound connections are required"), Home Assistant Cloud via
SniTun, the GitLab and GitHub runners. `docs/research/remote-mcp-access.md` is the survey behind that claim.

## Run it

```bash
node relay.mjs --listen 127.0.0.1:18990 --token <a long secret> [--device d1]
```

`--token` is required; without it every agent is refused, which is the only safe default. `--listen` defaults to
`127.0.0.1:18990` and the process WARNS if you point it anywhere else, because it holds no identity beyond that token and does
not terminate TLS — put Caddy, nginx or a tunnel in front of it before real credentials cross it (RFC 6750 requires TLS for
bearer tokens, and nothing endorses "public URL + static token" as a complete posture).

Then, on the device — **the CLI writes the config for you** (it edits only those keys and keeps every comment):

```bash
summrise connect https://relay.example.com --token <the same secret> --name workshop-pc
summrise restart
```

`--name` is optional; without it the device polls the relay's DEFAULT device, which the console cannot address
per device. `summrise disconnect` removes the three keys again, so a relay host that moves is a config change
rather than a trap.

By hand, the same thing (the CLI's `connect` writes exactly this):

```yaml
server:
  relay_url: https://relay.example.com     # or http://127.0.0.1:18990 on the same machine
  relay_token: <the same secret>
  relay_device: workshop-pc                # optional; the name the console dials
```

Restart the agent. `/api/status` reports `relay: {configured, connected, last_ok_ms, consecutive_failures, last_error}`, and
the panel's status strip shows "relay connected" when it is. The agent's own `device_token` still gates `/mcp`, so a relayed
caller needs BOTH secrets: a compromised relay does not by itself hand over the machine.

## ONE RELAY, MANY DEVICES

A relay carries a FLEET now. A device names itself when it polls, and a client addresses it by that name:

```yaml
# config.yaml — the device's own name, and the name clients use
server:
  relay_url: https://relay.example.com
  relay_token: <the same secret>
  relay_device: workshop-pc
```

```
GET  /agent/pull?device=workshop-pc     the agent's poll (an absent ?device= is the DEFAULT device)
ANY  /d/workshop-pc/mcp                 a client reaches THAT device's own surface at /mcp
ANY  /d/workshop-pc/panel/state         …and its panel, unchanged: the prefix is ADDRESSING, not payload
GET  /healthz                           per-device summary: {device, agentConnectedMsAgo, waiting, devices}
```

**The un-prefixed form still targets the default device** (`--device`, `d1`), and an agent that sends no
`?device=` is that same device — so a single-device deployment behaves exactly as it always did, and the fleet
path is opt-in per device. Two devices on one relay cannot see each other's requests: the queue is filtered by
device before a job is handed out, and an answer is matched by the job's globally unique id.

**`relay_device` IS THE WHOLE SWITCH, AND IT IS USED TWICE**: the agent polls with that name AND declares it to
the console when it self-registers (`POST /api/devices/self-register`), which is what makes the console dial
`/d/<that name>/…` instead of the device's tunnel hostname. **One value, two uses** — so the name the relay sees
and the name the console dials cannot drift apart. A device with `relay_url` but NO `relay_device` polls the
relay's default device and stays tunnel-addressed in the console, because "the default device" is not something a
per-device address can express. Turning the relay off (removing `relay_url`, or the name) is reflected on the
next registration: the console's record loses its transport and goes back to the tunnel.

## START IT IN A WAY THAT SURVIVES THE AGENT (learned the hard way)

A relay started from a terminal **the agent hosts** inherits Windows' kill-on-close job object and dies the next time the agent
restarts — which an update swap does by design. That happened twice while this was being brought up, and each time the agent
correctly reported `connected: false` while the relay really was gone.

Start it outside that job, the way the CLI launches its own update swap:

```powershell
$cmd = '"C:\path\to\node.exe" "C:\path\to\relay.mjs" --listen 127.0.0.1:18990 --token <secret>'
Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{ CommandLine = $cmd }
```

(a child parented by WmiPrvSE survives its caller), or register it as a service. Redirect its output to a file either way: a
relay whose stdout is a terminal loses its logs and can be killed by that terminal closing.

### The deployment that was VERIFIED end to end (2026-09-22)

A scheduled task under SYSTEM, with restart-on-failure and no execution time limit. This exact command registered it, the
relay came up, the agent reconnected within a second, and the panel's status strip read "relay connected":

```powershell
$a = New-ScheduledTaskAction -Execute "<path>\node.exe" -Argument '"<path>\summrise-relay.mjs" --listen 127.0.0.1:18990 --token <secret>'
$t = New-ScheduledTaskTrigger -AtStartup
$s = New-ScheduledTaskSettingsSet -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) `
     -ExecutionTimeLimit ([TimeSpan]::Zero) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
$p = New-ScheduledTaskPrincipal -UserId "SYSTEM" -LogonType ServiceAccount -RunLevel Highest
Register-ScheduledTask -TaskName "SummriseRelay" -Action $a -Trigger $t -Settings $s -Principal $p -Force
Start-ScheduledTask -TaskName "SummriseRelay"
```

`-Principal SYSTEM` IS REQUIRED: without it, `Register-ScheduledTask` fails with "no mapping between account names and security
IDs" when it is run from a service account, and the task is never created. `RestartCount`/`RestartInterval` are the supervision
the section above says a bare `node` process cannot provide for itself.

## What it is not

One agent per relay (first token wins), no TLS, no per-client identity, no rate limiting of its own. It is a pipe with a shared
secret, sized for one device and its owner. For a fleet, run one relay per device or put a real identity layer in front.
