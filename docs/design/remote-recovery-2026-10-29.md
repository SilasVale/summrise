# When the agent stops answering, who brings it back?

> **Status: a decision for the owner, written after an incident, 2026-10-29.**

## What happened

At 02:26 UTC the device channel to the console went away on DESKTOP-9VH92BJ and did not come back
for the rest of the session (an hour observed, still down when the work stopped). The console itself
stayed healthy the whole time — `/health` answered 200 every minute — but every call that needed the
device answered `device reconnecting`.

The operator's machine was reachable at the console's port and at nothing else: SSH is closed on it,
the device panel binds loopback, and the console's own "Restart the device" action is a device tool,
so it needs the channel that was already down. The launcher is the only self-healing component, and a
launcher that has given up (or that died with its session) has nobody behind it.

**A product whose promise is "your AI operates a real machine" cannot have a machine that only a
person standing at it can bring back.**

## The options, and what each costs

1. **A console-side restart action.** The console already runs on the machine and is already
   reachable from outside; a button that runs `sc stop SummriseLaunch && sc start SummriseLaunch` on
   its own host would recover the common case (the launcher is alive; the app is not).
   - Needs: an authenticated, CSRF-checked POST (the console has both), an audit line (it has that),
     and the service-control rights the console task may not hold.
   - Risk: the console becomes a second actuator for the device it watches — a privilege boundary
     that today sits with the launcher alone. It must be limited to the console's *own* host; a
     console that could restart a device elsewhere would be a remote root.
   - It cannot help when the launcher itself is dead or the console's task is not running.

2. **A second watchdog.** A scheduled task that starts the launcher if it is not running. It needs no
   HTTP surface and no new privilege, and it survives the console being down.
   - Costs: another service on every machine, and a policy for "how many times, how often" that has
     to agree with the launcher's own three-strike rule.

3. **An out-of-band relay.** The tailnet already exists on this box (`docs/DERP.md` in the sibling
   repository), and the operator can reach the machine through it. That is a person, not the product.

## What was built (2026-10-29, after the machine came back)

Option 2, the watchdog, is in the installer: `SummriseRepair` runs **every five minutes** instead of
only at start, and `repair.cmd` does one more thing — when the service is registered but not running,
it starts it. The script's logic was run against a live Windows service before it shipped: a running
service exits 0 untouched; a stopped one (tested on BITS, which is safe to stop) is started and the
exit code is 0.

The generated `repair.cmd` was also installed by hand on all three machines, with the task recreated
at five minutes: DESKTOP-9VH92BJ (`C:\SummriseInstall`), DESKTOP-14RJCR8 (`C:\SummriseCanary`) and
DESKTOP-CPH4KB2, which had **no repair task at all** — its install came from 0.3.52. Every one of
them reports `PT5M` and `last result 0`.

Then the loop was proved end to end, on the least critical machine (DESKTOP-CPH4KB2):

- a detached script stopped `SummriseLaunch`;
- `/health` went from `online: 3` to `online: 2` within a minute;
- **2 minutes 31 seconds later it was back at `online: 3`**, with no one touching the machine;
- the task's own log says `2026/10/29 15:42:01.79 start exit 0` — the five-minute task found the
  service stopped and started it.

That is the incident of 02:26 replayed with the fix in place, and the outcome is the one the product
needs: the machine came back on its own.

A deliberate stop is undone within five minutes. Someone taking a machine down on purpose disables
the task first (`schtasks /change /tn SummriseRepair /disable`) and re-enables it afterwards; the
script says so in its own first lines, where the person reading it later will find it.

## What this note recommended

Option 2 first — a watchdog is smaller, has no authentication surface, and covers the case that
happened (the app was not running; the launcher was not there to notice). Option 1 second, gated to
the console's own host and audited as `console` / `agent restart`, because an operator already
expects a "restart" button in the console and already trusts it to do destructive things after
confirming.

Either way the incident adds one requirement to the product: **the console must be able to say "this
device stopped answering at 02:26" without the device**. Today it knows `last_seen` and prints it as
"51m ago"; that is one wording away from the fact an operator needs.

## What is waiting on the machine (written 04:12 UTC, 2026-10-29)

Three console changes are committed, CI-green, and **not deployed** — the deploy runs on the console
host through the device's channel, and that channel has been down since 02:26 UTC:

| commit | change | what to check after deploying |
| --- | --- | --- |
| `fbd3bb72` | `/health` answers `online` beside `devices` | `curl …/health` shows `"devices":3,"online":2` (or however many are up) |
| `433f82ab` | an offline row says `offline since <stamp>` instead of `51m ago` | the fleet row for a stopped device |
| `da77f2e0` | a device whose launcher gave up names `sc stop/start SummriseLaunch` | the fleet page above the table |

Deploy with `scripts/stage-console-deploy.bash <version>` and the downloaded `.cmd`, as
`docs/console-deploy.md` describes; `/health` must report the new version before anything else is
called verified.

The recovery itself, when someone is at the machine:

```
sc stop SummriseLaunch
sc start SummriseLaunch
```
