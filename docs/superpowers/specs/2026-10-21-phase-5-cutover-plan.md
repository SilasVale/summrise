# Phase 5 — the cutover, and what evidence allows a deletion

> **Status: plan, not a decision to delete anything today.** Written 2026-10-21, after phases 1–4 and the
> console made the new runtime usable. Its job is to stop "phase 5" from becoming a mood: a deletion is
> allowed when a named piece of evidence exists, and not before.

## The principle

The old `agent/` tree is not deleted because the new one exists; it is deleted because a device **ran**
the new one, and the operator could get back. Every deletion below has a gate, and a gate is a command
whose output can be pasted into the ledger.

## The parity matrix (the part that is still missing)

| Capability | Old agent | New runtime | Gate |
|---|---|---|---|
| install / update / rollback / self-repair | scripts + tasks | `launch` + `release` | canary week |
| terminal (pty) | yes | `terminal.create {kind:"pty"}` | lane + canary |
| terminal (ssh) | yes | `terminal.create {kind:"ssh"}` (host-key pinned) | canary |
| browser | Playwright (Node) | `browser.*` (CDP, Rust) | canary |
| files / memory / monitor / runs | yes | yes | canary |
| serial | yes | `terminal.create {kind:"serial"}` | canary (loopback-plug smoke) |
| panel / desktop shell | Electron | **no** (browser-first; WebView2 deferred) | decide: port or drop |
| MCP keys / grants UI | console TS | **no UI** (CLI/API only) | console SPA |
| file transfer (R2 relay) | yes | unchanged (separate worker) | no change |
| remote access | cloudflared | console proxy only — the device has **no mesh client** yet | console deployed + canary |

One row is still a decision: **the desktop shell**. Serial is implemented and tested behind
`terminal.create {kind:"serial"}`; it needs the canary smoke row, not a port-or-drop decision. The
remote-access row names the console proxy alone because `summrise-device` does not link
`summrise-mesh`; the mesh stays out of this matrix until a device client exists.

## Gates

1. **Canary.** One real Windows device runs the new runtime for **7 days** with: one stable update, one
   canary update, one rollback, one deleted-exe repair, one PTY session, one browser session, one SSH
   session, one serial session, one remote MCP call through the console. Evidence: the device's
   `state.json` history, `<install>/repair.log`, `<data>/events.jsonl`, `swap.result.json`, and the
   console audit. There is no watchdog result file: the watchdog is a thread that exits after two
   failed probes, and its evidence is the supervisor restarting the app (a new pid/version in
   `/api/health`).
2. **Rollback in ten minutes.** From the canary, the operator can return to `1.2.513` (the last old
   agent) with the release assets on hand; measured, timed, written down. If this fails, nothing is
   deleted.
3. **The console is the operator's daily path** for at least a week: remote MCP calls, grants, audit
   review. If the operator still uses the old tunnel, the migration is not done.
4. **Data.** `state.json`, `known_hosts.json`, `memory.jsonl`, `runs.jsonl`, `monitors.json` and
   `events.jsonl` are device state; `<data>/token` is the root credential. The old `config.yaml` and
   its markers are **not** migrated: `--install-service` does not read them, so they are explicitly
   abandoned rather than silently carried.
5. **Release channel.** The new device ships through the signed `release.json`; the old npm channel is
   frozen (not deleted) and its last version is named in the ledger.

## The deletion, in order

| Step | What is removed | Precondition | Reversible by |
|---|---|---|---|
| D0 | new features land only in `device/` | now | reverting the policy |
| D1 | `agent/summrise-cli` install/update paths | gate 1 + 4 | the old release still installs |
| D2 | `agent/deploy/*.ps1`, the old watchdog | gate 1 + 2 | tag `v1.2.513` assets |
| D3 | `agent/src/tools`, `plugins`, `web` | parity matrix has no "no" row | the archived branch |
| D4 | `agent/` itself (and its CI jobs) | 30 days of fleet on the new runtime | the archived branch + tags |
| D5 | the repo split (`summrise-legacy`, private; new `summrise`) | D4 | the old repo still exists, renamed |

Every row is a separate commit and a separate decision. **A deletion that cannot name its reversal is
not ready.**

## What this plan refuses

- Deleting the old tree because the new one has more tests. Tests are evidence about code, not about a
  device.
- A "big bang" cutover of the fleet. One canary, then the operator's own machines, then anything else.
- Keeping both runtimes installed indefinitely. Two updaters on one device is the failure this project
  started with; the overlap window is 30 days, not forever.

## Where the canary actually stands (2026-10-22)

Written after the first working week. The matrix above was a plan; this is the ledger. A row is
`done` only when the evidence exists on a device, not when the code exists.

| Gate 1 item | Status | Evidence |
|---|---|---|
| one stable update | **done ×2** | `applied 0.2.6` on two machines, `/api/health` 0.2.6 |
| one canary update | **done** | `applied 0.2.6` from a canary document, then 0.3.0 |
| one update the device chose itself | **done** | `update.json` written, no command run; supervisor applied 0.3.0, `lastVerdict: updated to 0.3.0` |
| one rollback | **done** | the first canary update rolled back on a sharing violation; `swap.result.json` named the digest and the guard refused a retry until `--force-reinstall` |
| one deleted-exe repair | **not yet** | needs a canary session that deletes `baseline/current` |
| one PTY session | **not yet** | the lane drives PTYs; the canary has not |
| one browser session | **done** | Playwright on the device drove the console: login → grant → `/mcp` → `state.get` answered with the device's own state |
| one SSH session | **not yet** | |
| one serial session | **not yet** | needs hardware on the canary |
| one remote MCP call through the console | **done ×2** | grants minted on `desktop-9vh92bj`, the call answered for both machines over the tailnet |

What changed in the matrix itself:

- **`remote access`**: the device now links `summrise-mesh` (`--mesh-broker/--mesh-relay/--mesh-stun`),
  and the console channel carries a grant-verified frame today. The row's "no mesh client yet" is
  stale; the console is deployed on the tailnet and two machines are registered.
- **`panel / desktop shell`**: still `no`, and still the one open decision. The console page is the
  operator surface now; a desktop shell is a port-or-drop call nobody has made.
- **`file transfer`**: unchanged (separate worker) — and the device's relay upload currently answers
  404, so the canary moves files by HTTP from a host. That is a workaround, not parity.
- **installer**: the Windows setup is built and **self-tested in CI** (it installs itself on the
  runner, reads `/api/health`, uninstalls) and installs on real machines. It is not the old
  script-and-task path.

Gate 2 (rollback in ten minutes) has a measured piece: an installer re-run restores the shipped
version and rotates the previous generation, which took under two minutes on the canary. The
"return to `1.2.513`" half is untested.

Gate 3 (the console is the operator's daily path) is **not yet**: the operator has not used it for a
week; this session used it through a browser and the API.
