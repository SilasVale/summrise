# Summrise

[![CI](https://github.com/SilasVale/summrise/actions/workflows/ci.yml/badge.svg)](https://github.com/SilasVale/summrise/actions/workflows/ci.yml)

Summrise turns a Windows device into an **AI-controllable workspace** — terminal, SSH, serial and browser sessions exposed to AI through MCP, plus an Electron desktop shell and a device-local memory. One repository for the front door, the device agent and the download distribution.

```
Summrise Gate (front door, Cloudflare Worker) — console, BYOK AI gateway, /mcp proxy
        │
        ▼
Summrise Agent (Windows, Rust) — headless MCP server + /api/tools + panel
  └─ plugin registry: terminal / memory / system / mcp-client / playwright / update / design
        │  mcp-client bridges to a local browser MCP server (playwright)
        ▼
Summrise Desktop (Electron) — tray + native menu + CDP :9333 for AI-driven UI
Summrise Index (Cloudflare Worker) — npm tgz / download distribution
Satellites (not in the request path): satellite proxies (Cloudflare/VPS AI egress) + brand (static icons)
```

## Highlights

- **OSC 633 shell integration** (the VS Code approach): PowerShell prompts and command boundaries arrive as invisible sequences — clean terminal display, accurate exit codes, no wrapper text, no front-end filters.
- **Electron desktop shell** (TypeScript): live agent status in the tray, native menu (sessions + page navigation), CDP :9333 so AI can drive the same window the user watches, browser-window reuse + cap, hide-to-tray with a one-time notification.
- **Memory plugin**: device-local knowledge base shared across AI clients — 6 MCP tools (`memory_save/search/list/update/delete/export`), multi-word AND search, eager tombstone compaction.
- **56 MCP tools** on the device (by name prefix, `grep -oE '"name": *"[a-z_]+"' agent/spec-tools.json | sed 's/.*"\(.*\)"/\1/' | sed 's/_.*//' | sort | uniq -c`): terminal (23: PTY/SSH/serial open/write/close/execute/read/screen/history/background jobs/saved connections/secrets/env + legacy aliases), system (9: file list/read/write/stat/download/upload, process list/kill, net test), memory (6), mcp-client (4), monitor (4: list/add/remove/probe), playwright (2), runs (2: run_begin/run_end), update (1: agent_update), design (1: page_view). The count is what `agent/spec-tools.json` carries, regenerated from the live registry by `cargo test spec_snapshot` — this line said 49 and had no monitor or runs entry at all until it was measured.
- **Health endpoint**: `/api/status` reports version, uptime and live session count — consumed by the tray, the SPA status strip and AI health checks.
- **npm-only distribution**: one-command install/update, WMI-survives-the-kill swap, electron auto-restart on update.

## Quick start (Windows)

```powershell
npm.cmd i -g https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz   # or pin an exact version
summrise setup                 # pure local install (registry-first, no cloud needed)
summrise setup --reg-key <key> # optional: register the device with a Summrise Gate console
summrise update                # later: one-command update (exe + electron shell)
```

The install dir is registry-first (`HKLM\SOFTWARE\Summrise\Agent\InstallDir`); all path resolution goes through `agent/src/paths.rs`. The terminal panel is served by the agent at `/panel` (token entered once in the browser), and the Electron desktop shell loads `/desktop/`.

## Repository layout

| Directory | Project | Runtime | Description |
|---|---|---|---|
| `gateway/` | **Summrise Gate** | Cloudflare Worker | console (login/roles), BYOK AI gateway, `/mcp` proxy to devices, device registry |
| `agent/` | **Summrise Agent** | Windows (Rust) | headless MCP server + `/api/tools` + panel + Electron desktop shell (`summrise-desktop-electron/`) + npm distribution (`summrise-agent-npm/`) |
| `index/` | **Summrise Index** | Cloudflare Worker | download distribution (`summrise-dist`; hosts the npm tgz, see Quick start) |
| ~~`studio/`~~ | RETIRED 2026-09-06 | — | replaced by code-server (vscode.saisi.online, behind Access); the ADR that recorded it was pruned with the rest of `docs/adr/` — the retirement note here is now the only record |
| `proxies/` | **Satellite proxies** | Cloudflare Worker + Oracle VPS (vrelay) | zen-go / zen-us AI egress + api-relay (`./scripts/build.sh proxies|api-relay`) |
| `brand/` | **Brand assets** | static (satellite) | sunrise favicon / icon source (no build) |
| `scripts/` | build/release | shell | unified build/publish entry (`build.sh`, `publish-release.sh`) |
| `docs/` | docs | — | the operator's charter (`docs/CHARTER.md`), the agent inbox (`docs/agents/ideas.md`) and the round-by-round design ledger (`docs/agents/design-ledger.md`). The ADR and research trees this row used to name were pruned; the ledger carries what they recorded |

## Build & deploy

```bash
# Windows cross-compile of summrise-agent (needs cargo-xwin)
./scripts/build.sh agent             # + panel SPA rebuild (embedded at compile time)

# Deploy the workers (needs a Cloudflare API token)
./scripts/build.sh gateway|index     # wrangler deploy the worker
./scripts/build.sh proxies           # deploy satellite proxy workers (zen-go / zen-us)
./scripts/build.sh api-relay         # build+deploy the VPS api relay (vrelay @ Oracle box)
./scripts/build.sh deploy            # build agent + deploy gateway/index + 2 CF proxies (not api-relay)

# CDN-publish a release (pack + stage + version.json sha256 + last-5 prune
# + deploy; then push + tag vX to get the CI-built GitHub release)
./scripts/publish-release.sh 1.2.N
```

See `AGENTS.md` (build, tests, the proven-gate table, release) and `agent/AGENTS.md` (the Rust-side guide). `gateway/DEVICE-INTEGRATION.md` is a superseded 2026-08 design, kept for the device/wire history; the desktop core's design notes were pruned, and `docs/agents/design-ledger.md` is where those decisions were re-measured.

## Core design

- **Gateway plugin core (DSH-style)**: every `/api/*` route and `/mcp` lives in a plugin (`gateway/src/plugins/`: admin / auth / device-proxy / devices / mcp / model-route / translate / translate-vision on the shared registry) on a shared context; `index.ts` is a thin front door.
- **Device control, AI-first**: an AI client connects to `https://<console>/mcp` (gateway) or `https://<device>/mcp` (direct) with a bearer token and gets the device tool surface.
- **Terminal backends**: PTY (ConPTY on Windows, OSC 633 shell integration), SSH (keepalive 5s, bounded writes) and serial (auto-reconnect). Natural shell exits are detected (exit codes surface in `terminal_history`); the reader is pollable so `exit` never hangs the session.
- **Browser control via mcp-client**: the `mcp-client` plugin spawns the bundled `playwright-mcp` over stdio by default (stdin/stdout, no listening port) and forwards its tools; the Rust agent only bridges. `transport=http` (9229) remains for external servers only. The Electron shell also exposes CDP :9333 for driving the desktop UI itself.
- **Memory**: JSONL-backed knowledge base under the install dir, sanitized credentials, LRU caps, soft delete + compaction.
- **Console UI**: React + Vite (`gateway/ui`), built into `gateway/public`, dark mode, hash routing.

## License

MIT — see [LICENSE](LICENSE).
