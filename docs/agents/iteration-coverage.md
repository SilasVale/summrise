# Iteration coverage ledger

What has been looked at, by which round, and what is still open. Read the
newest entries of the round log in `agent/AGENTS.md` alongside this; keep the
table small (a row is a summary, not a report).

State: `unseen` · `partial` · `seen`. Round numbers refer to the round log.

Seeded 2026-09-14 at round 110.

## Current state

- Round log head: **round 113**; HEAD `23593d5f` (round 112's journal).
- **HEAD WAS RED AND NOBODY KNEW**: `tests/module_map.rs` failed against BOTH guides because
  round 110 added `src/runstate.rs` without adding it to the module map, and round 111 shipped
  on top of that. Found by round 112's independent gate run, fixed there. The lesson is cheap to
  state and expensive to forget: `cargo test` also runs `tests/`, and one of those tests READS THE
  GUIDES — so adding a module is a map change, not only a code change.
- **d1 is DARK (tunnel 530) and needs one human touch** — `vale tunnel start`, then
  `npm i -g --prefix (Split-Path (Get-Command vale).Source) https://agent.saisi.online/vale-agent/vale-agent-latest.tgz`,
  then `vale update`. 1.2.363 and 1.2.364 are published but on NO device, and four
  agent fixes are unverified at runtime until it is back. Restoring this channel is
  the highest-value work available (CHARTER: device regression is a gate).
- No device means: agent-side changes stop at `cargo test` + `clippy` + `xwin check` + static pins.

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
| field device d1 | BLOCKED | 110 | dark since 1.2.362; recovery needs one human action |

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

1. **d1 recovery** — one human action; then install 1.2.364 and verify the round-110/111 fixes at runtime (boot-task repetition trigger, tunnel supervisor paths, memory refusal paths, and now the tunnel card's verdicts).
2. **Agent restarted every 1–2 h before round 110** (cause unknown) — `runstate.rs` now answers this on the next occurrence; read it at the first boot after recovery.
3. **Round 105 leftovers** — panel facet editing, provider-model effort in the add row, no needs-setup/onboarding hint.
4. **Round 99 leftovers** — memory F5, index F2/F3.
5. **Panel F5** (host allowlist family match) — recorded as hardening only.
6. **ADR 0007 step 3** — `RELAY_ADMIN_CUTOVER` flag exists, default off; flipping it is a deprecation-window decision (propose).
7. **CDN ⇄ GitHub release reconcile** — member-wise comparison + exe provenance proposal awaits sign-off.

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
