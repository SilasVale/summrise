# The cutover verdict — what the week supports, and what it does not

> **Status: conclusion, 2026-10-22.** The design record now has a decision for every open row in the
> parity matrix, an observation report from a real machine, and one canary day of dense evidence. This
> document is the answer the operator asked for: *can we retire the old runtime?* It is **no**, with a
> named list of what would make it yes. It is also not "keep building": the building is done.

## The verdict

**Pilot, not deletion.** Two machines run the new runtime; both are enrolled in the console; the
update, repair, refusal and session paths have all been exercised on a device rather than in a test.
Three things are missing, and none of them is code:

1. **Time.** The gate asks for a week of *ordinary* use — the operator driving, not the agent. One day
   of evidence, however dense, is not that week.
2. **Hardware.** Serial is implemented and tested; it has never opened a port on a real machine,
   because no canary has one.
3. **CI.** The Windows lifecycle suite — the only place the real service, SCM and installer run
   together — has not executed since the account stopped starting jobs (`runner_id: 0`, zero steps).
   Local gates (895 tests, clippy, the Windows compile check) are green; that suite is not the same
   check.

## Every gate, its evidence, and what closes it

| Gate | Status | Evidence (where to look) | What closes it |
|---|---|---|---|
| one stable update | done | `state.json` `update.to=0.2.6 outcome=applied` on two machines | — |
| one canary update | done | same path, canary document | — |
| one update the device chose itself | done | 0.3.4 → 0.3.5 with no command after the restart; `lastVerdict:"updated to 0.3.5"` | — |
| one rollback | done | first canary update rolled back on a sharing violation; `swap.result.json` + the digest guard | — |
| one deleted-exe repair | done | `repair.log` `{"from":"previous","sha256":"db304dd5…"}`; the device served again | — |
| one PTY session | done | console: `terminal.create` → `powershell.exe` (the fallback), write/read | — |
| one SSH session | done | console: pinned host key, key auth, `echo SSH-OK` returned by the host | — |
| one browser session | done | Playwright on the device drove the console: login → grant → `/mcp` → state | — |
| one remote MCP call | done | two machines, grants minted on one, answers from both | — |
| one refused (revoked) release | done | exit 4, the message names version/digest/reason, `baseline/pending` never created | — |
| **one serial session** | **missing** | — | a machine with a COM port (or a USB-serial adapter) |
| **seven days of ordinary use** | **missing** | — | the operator uses the console for a week |
| **CI green on Windows** | **blocked** | every run since 11:06 UTC fails at job start | Actions billing/quota restored, then a re-run |
| rollback to `1.2.513` in ten minutes | untested | installer restores the shipped version in <2 min | a rehearsal on a machine with the old assets |

## What the design now guarantees (and what tested it)

| Guarantee | Tested by |
|---|---|
| only a signed document installs | the refused revoked release, and every update |
| a bad artifact cannot brick the device | the first canary update's rollback |
| a missing artifact self-heals | deleting `baseline/current` |
| authorization is not ambient | `-32002 approval_required` on `terminal.create` and `files.write` |
| the console cannot execute without a channel | the 30 s answer bound and the 504 that names the device |
| a device's identity is not rewritten by accident | the unclaimed-start fix, after a diagnostic run proved it |
| bytes travel through the audited path | text and binary transfers through the console |

## The three things only the operator can do

1. **GitHub → Settings → Billing → Actions** — restore the quota, then push anything; I re-run the
   device and installer jobs and check `steps`, not "did it trigger".
2. **One machine with a serial port** — the last gate row that needs hardware. A USB-serial adapter
   with TX/RX bridged is enough.
3. **A DNS name and a certificate** for the console when it leaves the tailnet — the terminator shape
   is in `2026-10-22-the-console-transport-decision.md`; four lines of Caddy.

## What I would do in the next week, in order

1. The operator drives the console for real: remote calls, grants, the audit view. A log of what they
   tried is worth more than another agent session.
2. The serial row, on the first machine that has a port.
3. The `1.2.513` rollback rehearsal, timed.
4. CI back, then the Windows lifecycle suite on every push again.

Deletion stays closed until 1–3 produce evidence. That is what a gate is for; the design's part of the
bargain is finished — the code, the decisions and the runbook exist, and the remaining distance is
operational.
