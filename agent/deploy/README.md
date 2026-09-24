# Summrise Agent deploy

Windows deployment notes for the summrise-agent device agent. The install and
update channel is **npm-only** — but that is the AGENT's channel, not the whole story: the
NSIS installer is **NOT retired** — but it is OPT-IN, and that is the state to hold in mind: `scripts/publish-release.sh` builds it only under `--with-installer` (default `WITH_INSTALLER=0`), and the default path PRUNES any staged `SummriseAgent-Setup-*.exe` (`retire_installers`). That is why the manifest carries no `installer` field and the landing shows no button right now — a PUBLICATION STATE, not a retirement. When it IS built it ships the desktop task and the one-click install, and it is served as an asset. Only
`setup.ps1` is retired, in `deploy/retired/`. `scripts/build.sh`'s comment carries the history of a
comment that called the installer retired for long enough that a reader would have believed it.

## Install / update

```powershell
npm i -g summrise-agent          # or: npm i -g <your tgz URL>
summrise setup                   # pure local install (registry-first)
summrise setup --reg-key <key>   # optional: register with a Summrise Gate console
summrise update                  # one-command update
```

- Install dir: registry-first (`HKLM\SOFTWARE\Summrise\Agent\InstallDir`); all
  path resolution goes through `src/paths.rs`.
- The `SummriseAgent` boot task runs the agent as SYSTEM (no execution-time
  limit, restart-on-failure ×8, 5-min repetition watchdog).
- Optional public access: a Cloudflare Tunnel (`cloudflared`) exposes the
  device at `<device-host>` — the status page, `/panel/` terminal UI, `/mcp`
  endpoint and `/api/*` tools.

## Files

- `fix-tunnel.ps1` — repairs a legacy `summrise-command-<device>` tunnel/ingress
  to `summrise-agent-<device>` (idempotent, runs on agent start)
- `cloudflared-config.example.yml` — example tunnel ingress config
- `claude-mcp.example.json` — example Claude Code MCP registration
- `retired/` — `setup.ps1` (superseded by npm). The NSIS installer is NOT here: see the note at the top.

## Architecture

```
<device> ──Cloudflare Tunnel──► summrise-agent:18080 ──► MCP / panel / desktop
```

## Build / release

`./scripts/build.sh agent` cross-compiles `summrise-agent.exe`;
`./scripts/publish-release.sh 1.2.N` (from the repo root) does the CDN
release (pack + stage + version.json sha256 + last-5 prune + deploy).
