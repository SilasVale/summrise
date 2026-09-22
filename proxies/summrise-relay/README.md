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

Then, in the agent's `config.yaml`:

```yaml
server:
  relay_url: https://relay.example.com     # or http://127.0.0.1:18990 on the same machine
  relay_token: <the same secret>
```

Restart the agent. `/api/status` reports `relay: {configured, connected, last_ok_ms, consecutive_failures, last_error}`, and
the panel's status strip shows "relay connected" when it is. The agent's own `device_token` still gates `/mcp`, so a relayed
caller needs BOTH secrets: a compromised relay does not by itself hand over the machine.

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
