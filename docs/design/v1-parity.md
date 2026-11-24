# v1 → v2 parity matrix

> The gate for v2.0: every row must be green on `next` before v2 is the default release. "v2 home" is
> where the behavior lives after the slices in `architecture-v2-2026-11-13.md` §5. "Acceptance" is the
> test or check that proves it, not a promise.

## Install, identity, reach

| Feature | v1 home | v2 home | Acceptance |
|---|---|---|---|
| installer builds four exes | `device/installer`, CI | same | `verify-release.sh` + a Windows install |
| launcher verifies, repairs, swaps against a signed release doc | `device/launch`, `device/release` | `the launcher`, `protocol::release` | launch test suite; install on a clean machine |
| self-update from the mirror | `device/device/src/update.rs` | `device/update` | update test; mirror fetch |
| enroll from the panel (loopback + boot token) | `device/device/src/http.rs` | `device/entries/panel` | panel enroll test |
| device registration + presence | `console/devices.rs` | `console/devices` | console test |
| operator session (password/session cookie) | `console/http.rs` | `console/http` | auth tests |
| tokens: mint, list, revoke, rotate (identity; no scopes after the operator's decision) | `console/tokens`, `audit` | `console/record` | token tests |
| device dials out; nothing listens inbound | `console/channels.rs` | `console/channel` | reconnect test |
| mesh / NAT traversal | `device/mesh` | `mesh` | mesh tests |

## The MCP door

| Feature | v1 home | v2 home | Acceptance |
|---|---|---|---|
| device MCP over HTTP (`/mcp`) | `device/mcp.rs` | `entries/mcp` | protocol test with the rmcp client |
| console proxy (`/mcp`, `/mcp/all`, `/mcp/:id`) | `console/http.rs` | `console/http` | proxy tests |
| `tools/list` + `tools/call` + JSON-RPC errors | `device/mcp.rs` | `dispatch` | conformance suite |
| tool catalogue page (`/tools`, live per device) | `console/http.rs` | `console/http` + registry | page test |
| destructive label (advice, not a gate) | `core/registry.rs` | `protocol` + registry | registry test |

## Capabilities (the hands)

| Feature | v1 home | v2 home | Acceptance |
|---|---|---|---|
| files read/write/mkdir/list, two roots | `device/capabilities/files.rs` | `capabilities/files` | capability tests |
| terminal: local PTY, SSH, serial; sessions | `capabilities/terminal.rs` | `capabilities/terminal` | session tests |
| browser (headless default, visible opt-in) | `capabilities/browser.rs` | `capabilities/browser` | browser tests |
| screen capture | `capabilities/screen.rs` | `capabilities/screen` | screen test |
| runs: one-shot exec + history | `capabilities/runs.rs` | `capabilities/runs` | run tests |
| host: facts, restart, net test | `capabilities/host.rs` | `capabilities/host` | host tests |
| memory: append/update/delete/search | `capabilities/memory.rs` | `capabilities/memory` | memory tests |
| plugins: install/inspect/list/start/stop/tools | `device/plugins`, `plugin-host` | `capabilities/plugins` + supervisor | plugin tests |
| forward / monitors | `capabilities/forward.rs` | `capabilities/forward` | forward tests |
| state.get (poisoned-aware) | `device/mcp.rs`, `store.rs` | `capabilities/state` | state tests |
| guide (new) | — | generated from `CapabilitySpec` | every tool appears once; unknown tool fails the build |

## The record

| Feature | v1 home | v2 home | Acceptance |
|---|---|---|---|
| `journal.read` (page, more, door carry-over) | `device/mcp.rs` | `device/record` | shaped answers, now surviving retention |
| `journal.get` (traces held/missing, bounded) | `device/mcp.rs` | `device/record` | join tests |
| `journal.day` (counts by door, bounded) | `device/mcp.rs` | `device/record` | day tests |
| `(epoch, seq)` cursor | parked branch | `protocol` + record | epoch tests |
| console audit: every proxied call, denial, poll | `console/audit.rs` | `console/record` | same rows, SQL-backed |
| audit list, export, source, day, join | `console/http.rs` | `console/http` | endpoint tests |
| denials log | `console/denials.rs` | `console/record` | denial test |
| Activity: list, live SSE, machine rows, join marks, day sentence, fleet | `console-page`, `web/` | `console/page` + `web/` | page tests + browser gate |
| record import from `events.jsonl` + `journal/` | — | `store` migration | idempotent import test; counts match |

## The brakes and the surface

| Feature | v1 home | v2 home | Acceptance |
|---|---|---|---|
| stop/kill a session or run | capabilities' `close` | `dispose` | dispose test |
| panel: local pages, terminal, state, update, enroll | `device/panel.rs` | `entries/panel` | panel tests + layout gate |
| shell: tray, WebView2, console fallback | `device/shell` | `shell` | shell check |
| console page: Home/Activity/Device, Connect sheet | `console/web` | `console/page` | browser gate |
| console deploy (`stage-console-deploy`) | scripts + docs | same | deploy smoke |
| architecture gate (`check-architecture.mjs`) | scripts | same, new table | `--self-test` + CI |
| `scripts/check.sh`, browser gate, layout gate | scripts | same | green |

## Deliberate non-goals

- Scopes, destructive consent, approval queue — removed by the operator's 2026-11-13 decision; the
  middleware seam stays.
- L2 event sourcing (the record as a replayable projection) — deferred.
- Electron client, plugin marketplace, sandbox — unchanged non-goals.
