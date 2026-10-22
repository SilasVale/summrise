# The canary observation report — one machine, one week, and what it proved

> **Status: evidence, 2026-10-22.** This is the ledger the phase-5 cutover plan asks for, filled from a
> real machine rather than from test output. It says what happened, what broke, what was fixed, and
> what is still missing before the old runtime is retired. It does not claim the canary week is over:
> the clock has not run out (see "What is still open").

## The machine

`desktop-9vh92bj` — Windows, the operator's own box, not a spare. It ran the old agent on 18080
throughout; the new runtime has its own port (18090), its own service (`SummriseLaunch`) and its own
data directory (`C:\ProgramData\Summrise`). The two runtimes never shared a process, a port or a file.

The console (`summrise-console`) runs on the same machine on 19000, as a scheduled task, and both
canary machines are enrolled. The device dials the console; nothing dials the device.

## The timeline, from the machine's own files

| When (UTC) | What | Evidence on disk |
|---|---|---|
| 0.2.x → 0.2.6 | two real updates through the signed document, both machines | `/api/health` 0.2.6 |
| 13:40 | **0.3.0 applied by the device itself** (source file, no command) | `state.json` `"update":{"to":"0.3.0","outcome":"applied"}` |
| 13:47 | `baseline/current\summrise-device.exe` deleted; restart; the supervisor booted `previous` and rebuilt `current` | `repair.log` `{"from":"previous","sha256":"db304dd5…"}` |
| 14:39 | a release the revocation list withdraws was refused | exit 4, `baseline/pending` never created |
| 15:16 | **0.3.5 applied by the device's schedule** (0.3.4 running, source at 0.3.5) | `state.json` `"lastVerdict":"updated to 0.3.5"`, `swap.result.json` |
| 16:07 | a PTY session through the console | `events.jsonl` `terminal.create/write/read` ok |
| 16:31 | a file written and read through the console | `events.jsonl` `files.write/files.read` ok |
| 17:45 | an SSH session to a host, command executed | `events.jsonl` `terminal.create/write/read/close` ok |

The current document at the end of the week:

```json
{"schema":1,
 "install":{"dir":"C:\\SummriseInstall","version":"0.3.5","channel":"canary"},
 "baseline":{"version":"0.3.5","sha256":"cac66fdbc5f6…"},
 "service":{"state":"running","pid":11468,"startedAt":"2026-10-22T15:16:25Z"},
 "lastVerdict":{"ts":"2026-10-22T15:16:26Z","outcome":"ok","detail":"updated to 0.3.5"},
 "update":{"phase":"idle","to":"0.3.5","outcome":"applied"},
 "rolledBack":[],"repairOffset":136}
```

## The safety properties, as observed

| Property | Observed |
|---|---|
| only a signed document installs | every update carried a release-key signature; a document naming a revoked digest was refused (exit 4) before `baseline/pending` existed |
| a bad artifact cannot brick the device | the first canary update rolled back on a sharing violation, recorded the digest, and refused a retry until an operator named it |
| a missing artifact self-heals | `baseline/current` deleted → the previous generation booted and rebuilt `current`; the device served again |
| authorization is not ambient | `terminal.create` and `files.write` are destructive: without `allow_destructive` the device answered `-32002 approval_required` and wrote `decision:"ask"` to the audit |
| the audit is on the device | `events.jsonl` records every console call: tool, decision, outcome, transport |
| the console cannot execute on a device it does not have a channel to | requests wait, then fail; the answer bound is 30 s and the failure names the device |

## What broke, and what it cost to find

Every one of these was found by running the thing, and each is fixed with a test that would have
caught it:

1. **The installer embedded the phase-1 placeholder key.** CI built the binaries without the
   compile-time keys; the install "succeeded" and then refused its own seed. Fixed by deriving the
   public keys from the seeds in CI (`summrise-release pubkey`), and the workflow now installs its own
   installer on the runner before publishing it.
2. **The installer could hang for ever** on a port held by another program: the health gate looped
   without a bound. Fixed by ten attempts and a refusal; the setup now says which port and why.
3. **The install log was never written** by the section on one machine, while the init trace was. Both
   halves now append to `%TEMP%\summrise-setup.log`.
4. **`sc stop` does not mean the process is gone**; replacing a running image failed. The installer
   waits (bounded) for the launcher process to exit.
5. **A staged release was never swapped in**: the scheduler verified and staged, then did nothing,
   because the one-shot CLI writes `swap.request.json` and the scheduler did not. Fixed in 0.3.4; the
   0.3.4 → 0.3.5 update is the first one the device chose entirely on its own.
6. **The console waited forever for a device's answer.** An operator's call behind a wedged device
   never returned. Fixed with a 30 s bound and a 504 that says the device may still be working.
7. **A key written by Windows' own `ssh-keygen` was refused** (CRLF). Fixed by normalising line
   endings in the loader.
8. **`pwsh` was assumed.** A Windows device without PowerShell 7 could not open a terminal at all; the
   default now falls back through `powershell.exe` and `cmd.exe`.
9. **An unclaimed start rewrote the device's identity** in `state.json` (a diagnostic run turned 0.3.0
   canary into 0.1.0 stable with an empty digest). Fixed: a start that claims no release refreshes
   only its liveness.
10. **A boot from the previous generation was silent** — the gate asked for a repair record and there
    was none. Fixed: the record names the generation that was booted.

## What the week did **not** prove

- **Serial.** The machine has no serial port and no USB-serial adapter. The capability exists and is
  covered by tests; it has not run against hardware.
- **Seven days.** This is one day of evidence, gathered densely. The plan's gate is a week of ordinary
  use, and a week of ordinary use is what it says: the operator, not the agent, driving.
- **The operator's daily path.** The console was driven by this session (through HTTP and a browser),
  not by the operator for a week.
- **Rollback to the old agent.** The gate wants a measured return to `1.2.513`; the installer restores
  the shipped version in under two minutes, but the old agent's return is untested.
- **A second machine of a different shape.** The second canary is the same Windows build.

## The recommendation

**Do not delete anything yet, and do not call the canary concluded.** What the evidence supports is a
pilot: keep the new runtime on these two machines, have the operator use the console for a week, run
one serial session on a machine that has a port, and get CI running again (it has been failing at job
start on the account, not on the code). The deletion gates in the cutover plan stay closed until the
missing rows above have answers — which is exactly what the plan says a gate is for.
