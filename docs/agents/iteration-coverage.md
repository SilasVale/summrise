# Iteration coverage ledger

What has been looked at, by which round, and what is still open. Read the
newest entries of the round log in `agent/AGENTS.md` alongside this; keep the
table small (a row is a summary, not a report).

State: `unseen` · `partial` · `seen`. Round numbers refer to the round log.

Seeded 2026-09-14 at round 110.

## Current state

- Round log head: **round 114**; HEAD `3782802b` (round 113's journal).
- **HEAD WAS RED AND NOBODY KNEW**: `tests/module_map.rs` failed against BOTH guides because
  round 110 added `src/runstate.rs` without adding it to the module map, and round 111 shipped
  on top of that. Found by round 112's independent gate run, fixed there. The lesson is cheap to
  state and expensive to forget: `cargo test` also runs `tests/`, and one of those tests READS THE
  GUIDES — so adding a module is a map change, not only a code change.
- **d1 IS BACK AND CURRENT (1.2.364)** — `vale status`: RUNNING, `this device is current`, no
  update in flight; install `D:\Vale`, data `C:\ProgramData\Vale`. The multi-day dark period is
  over. Round 114 spent the live session verifying what rounds 110/111 could only pin: the boot
  task's revival contract (trigger/settings read from the live task, and the protective half
  proven BY EFFECT — a manual start while healthy is refused and the process count stays 1), the
  tunnel supervisor's spawn line, and the run journal. All read-only; see the round-114 log.
- **The restart-every-1-2-hours mystery is still OPEN, with the instrument now armed**: the run
  journal exists and is heartbeating, but its first boot line reads "no previous run on record"
  because the journal only exists since 1.2.363 — so the NEXT boot is the one that answers it.
  Do not read that line as "the previous run was clean".
- **NOT tested, deliberately**: reviving a DEAD agent (the boot task's other half). Killing the
  agent takes cloudflared with it (kill-on-close job), darkening the only device for up to five
  minutes, with a human recovery if the revival fails. Proposed as a maintenance window, not
  taken.

## Surfaces

| Surface | State | Rounds | Open |
|---|---|---|---|
| gateway `device-fetch` + `plugins/devices` | seen | 89, 90, 95 | F5 host allowlist is a family match — hardening only |
| gateway `store/` (KV cache, grants) | seen | 94 | grant single-use needs the device-side backstop (shipped) |
| gateway `auth` / `session` / `access` | seen | 61, 89 | — |
| gateway `plugins/admin` + `models-probe` + `model-route` | seen | 105 | panel cannot EDIT a declared facet; provider-model effort missing from the add row; no needs-setup hint |
| gateway `plugins/translate` (+vision) | seen | SOLID R117–R121 | — |
| gateway `mcp.ts` / `mcp-tools.ts` / `mcp-browser.ts` | seen | 78, 89 | — |
| gateway `reliability` / `upstream` / `channels` / `body-scan` / `http` | partial | 64, 89 | — |
| gateway console SPA (`gateway/ui`) | seen | 61, 91–93 | — |
| index worker | partial | 99 | F2, F3 |
| proxies (zen-go / zen-us / vrelay) | partial | 64 | — |
| extension (Vale Code Links) | unseen | — | — |
| agent `plugins/terminal` (26 tools) | partial | 88, SOLID | — |
| agent `plugins/memory` | seen | 99, 110 | F5 (sanitizer vs pre-fix bytes) |
| agent `plugins/runs` + `runs.rs` | seen | 110 | — |
| agent `plugins/update` | seen | 553–555, SOLID | layout-v2 migration + rollback still need a device run |
| agent `plugins/system` / `mcp_client` / `playwright` | partial | 89, SOLID R98 | — |
| agent `plugins/design` | unseen | — | — |
| agent `web/` (auth, panel grant, SSE, api) | seen | 88, 92 | grant single-use / audit record |
| agent `winmain.rs` boot path + `tunnel.rs` + `runstate.rs` | partial | 110, 112, 113 | the 5-minute repetition trigger and the supervisor fix are pins only — RUNTIME needs a device; tunnel F1/F2 are closed (113), leaving the runtime half |
| agent `paths.rs` + layout v2 | seen | 555 | — |
| agent CLI (`vale-agent-npm/bin/vale.js`) | seen | 78 | — |
| agent packaging + Electron shell (`vale-desktop-electron`) | partial | 555 | — |
| agent `deploy/` PowerShell (setup/update/installer/tasks) | partial | 555, 110 | — |
| agent panel SPA (`resources/panel-react`) | seen | 90–93 | — |
| ops `scripts/` (publish / installer / audit / cdn-from-ci) | partial | 102–110 | CDN-vs-GitHub reconcile proposal awaits sign-off; `npm publish` awaits `npm login` |
| ops CI (`ci.yml`, `release.yml`) | partial | 102–110 | — |
| docs (README / ARCHITECTURE / ADRs / this ledger) | partial | — | — |
| field device d1 | seen | 114 | current on 1.2.364; verified live (boot-task contract, tunnel supervisor, run journal). NOT tested: reviving a dead agent — proposed maintenance window. |

## Open items (each needs an owner round)

**Closed in round 112: the orphan `agent/src/tunnel.rs` change.** It had already been committed
as `86d84e5b` (round 111, paused, with no gate numbers recorded). Round 112 verified it: both
correct mutations of the seam fail its test — a best-effort write inside `install_tunnel_config`,
and a restart requested BEFORE the write. Mutating the CALLER's `if let Err` away stays green —
that gap was finding tunnel F2, closed in round 113 below.

**Closed in round 113: tunnel F1 + F2.** `update_remote_config` now RETURNS its verdict
(`RemoteConfig::Updated` / `Failed(reason)`) instead of returning `()` through eight bare
`return`s, and the operator's line is decided by a pure `tunnel_outcome(local, remote, hostname)`.
Four mutations are caught: the remote arm reporting plain success, the local arm reporting
success, the PARTIAL string dropping its cause, and — round 112's blind spot — the CALL SITE
inventing the verdict, which is pinned by a source check because that caller does network I/O and
no behavioural test can reach it. A remote config that did not land now reads
`PARTIAL (host): … NOT updated (<cause>) — a remote configuration OVERRIDES the local file, so
verify this tunnel before trusting it`.

**Closed in round 114: d1 recovery.** The device is up, current on 1.2.364, and round 114 used it
to verify the round-110/111 device fixes at runtime (boot-task trigger + IgnoreNew read from the
live task and the protective half proven by effect; the tunnel supervisor's spawn line; the run
journal heartbeating).

1. **CHARTER-1 — a conflict inside our own constitution (proposal, waits for the user).** The
   CHARTER's blast-radius row puts "published releases" in the propose column, while the goal
   objective says release cadence is the loop's and reserves sign-off for full rollout and
   deprecation windows. Read strictly, the CHARTER forbids what the objective grants. Proposed
   replacement for that cell: **"a release that replaces the current version for everyone, and
   closing a deprecation window"** — publishing under the cadence stays autonomous, replacing
   everyone's version does not. Until the user answers, no release ships (the CHARTER is stricter,
   and stricter wins).
2. **Reviving a DEAD agent is untested** (the boot task's other half). Killing the agent takes
   cloudflared with it, so the only device goes dark for up to five minutes and needs a human if
   the revival fails. Proposed: a maintenance window — `Stop-Process vale-agent`, then confirm the
   task revives it within 5 minutes and the tunnel returns.
3. **Agent restarted every 1–2 h before round 110** (cause unknown) — the run journal is armed and
   answers it at the NEXT boot; its first line, "no previous run on record", is a gap in the
   instrument, not a clean bill of health.
4. **Round 105 leftovers** — panel facet editing, provider-model effort in the add row, no needs-setup/onboarding hint.
5. **Round 99 leftovers** — index F2/F3 (round 110 closed the code half of memory F5: the unread
   `load_failed` flag and the `true`-after-a-failed-append were both fixed, with the test forced
   through a REAL failure seam. A live test would mean corrupting the store on the only device,
   so it stays test-level by choice, not by omission).
6. **Panel F5** (host allowlist family match) — recorded as hardening only.
7. **ADR 0007 step 3** — `RELAY_ADMIN_CUTOVER` flag exists, default off; flipping it is a deprecation-window decision (propose).
8. **CDN ⇄ GitHub release reconcile** — member-wise comparison + exe provenance proposal awaits sign-off.

## Finding ID registry

`gateway F2` redirects followed without re-validation · `gateway F3` `deviceFetch`
skips the suffix allowlist at dial time (fixed round 95) · `panel F1` loopback
branch trusts a client Host header · `panel F2` `?grant=` not single-use over
eventual consistency, no device-side audit · `panel F3` grant route shape check
looser than the gateway's · `panel F5` host allowlist family match · `memory F5`
sanitizer gap on pre-fix bytes · `index F2/F3` (round 99) · `tunnel F1`
`update_remote_config` failed silently · `tunnel F2` the FAILED branch was unpinned
(both fixed round 113).

## ADR index

Adopted: 0001 plugin core · 0003 repo topology · 0004 panel grants · 0005
write-through config · 0006 retire studio · 0007 scoped relay token (step 3
pending) · 0008 install layout v2 · 0009 self-contained installer · 0010 two
products, one repo. Proposals: control-path, game-design, interactive-browser.
0002 is intentionally unused.
