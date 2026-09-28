# Summrise

[![CI](https://github.com/SilasVale/summrise/actions/workflows/ci.yml/badge.svg)](https://github.com/SilasVale/summrise/actions/workflows/ci.yml)

**A Windows machine you can hand to an AI.** Summrise runs a headless MCP server on a device and gives it a terminal
(PTY / SSH / serial), a real browser, a memory store and a file relay — reachable from a local panel, from a console over a
Cloudflare tunnel, or from any MCP client. One Rust binary, one npm package, no cloud account required for local use.

> **Install in two commands** · **Windows 10/11** · **MIT** · **Rust + TypeScript**

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

- **Terminal that behaves** — OSC 633 shell integration (the VS Code approach): prompts and command boundaries arrive as
  invisible sequences, so the display stays clean and exit codes stay true. PTY, SSH and serial behind one surface.
- **A real browser, driven** — Playwright over CDP, plus an Electron shell whose UI an AI can drive.
- **Memory that persists** — a device-local knowledge base shared across every AI client, with multi-word search.
- **Reachable when you need it** — local panel by default; a console and a Cloudflare tunnel when a remote client needs in.
  The tunnel is provisioned through the Cloudflare API, so a device running as SYSTEM needs no interactive login.
- **Two ways to install** — a self-contained `SummriseAgent-Setup.exe` (no Node, no npm, elevates itself) or the npm package.
- **Measured, not asserted** — the design of every surface is checked by gates that render it; the counts in this file are
  reproducible from commands beside them.

## Quick start (Windows)

**One file, no Node required** — [SummriseAgent-Setup.exe](https://agent.saisi.online/summrise-agent/SummriseAgent-Setup.exe).

**Or through npm:**

```powershell
npm.cmd i -g https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz   # or pin an exact version
summrise setup                 # install and start; the agent self-registers with the configured console on first start
summrise setup --reg-key <key> # only useful WITH --tunnel: it names the console that issues the tunnel token
summrise update                # later: swap the exe (run `npm i -g` FIRST when the CLI is behind — it refuses otherwise)
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
| `docs/` | docs | — | the operator's charter (`docs/CHARTER.md`), the agent inbox (`docs/agents/ideas.md`) and the two reference tables (`docs/agents/ledger-mutations.md`, `docs/agents/ledger-appendix.md`). The ADR, research and ledger trees this row used to name were pruned or retired; **the commit messages carry what they recorded**, which is where a decision lives now |

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

See `AGENTS.md` (build, tests, the proven-gate table, release) and `agent/AGENTS.md` (the Rust-side guide). `gateway/DEVICE-INTEGRATION.md` is a superseded 2026-08 design, kept for the device/wire history; and the desktop core's design notes were pruned. `CONTEXT.md` is the glossary; the decisions are in the commit that made them.

## Core design

- **Gateway plugin core (DSH-style)**: every `/api/*` route and `/mcp` lives in a plugin (`gateway/src/plugins/`: admin / auth / device-proxy / devices / mcp / model-route / translate / translate-vision on the shared registry) on a shared context; `index.ts` is a thin front door.
- **Device control, AI-first**: an AI client connects to `https://<console>/mcp` (gateway) or `https://<device>/mcp` (direct) with a bearer token and gets the device tool surface.
- **Terminal backends**: PTY (ConPTY on Windows, OSC 633 shell integration), SSH (keepalive 5s, bounded writes) and serial (auto-reconnect). Natural shell exits are detected (exit codes surface in `terminal_history`); the reader is pollable so `exit` never hangs the session.
- **Browser control via mcp-client**: the `mcp-client` plugin spawns the bundled `playwright-mcp` over stdio by default (stdin/stdout, no listening port) and forwards its tools; the Rust agent only bridges. `transport=http` (9229) remains for external servers only. The Electron shell also exposes CDP :9333 for driving the desktop UI itself.
- **Memory**: JSONL-backed knowledge base under the install dir, sanitized credentials, LRU caps, soft delete + compaction.
- **Console UI**: React + Vite (`gateway/ui`), built into `gateway/public`, dark mode, hash routing.

## License

MIT — see [LICENSE](LICENSE).
