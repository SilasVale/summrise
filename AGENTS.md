# Vale — monorepo

One repo, one front door: `gateway/` (Vale Gate worker), `agent/` (Vale Agent, Windows),
`index/` (dist + CDN worker), `proxies/` (satellite workers), `extension/`, `brand/`, `docs/`.

## Build

```bash
./scripts/build.sh agent              # cross-compile vale-agent (needs cargo-xwin)
./scripts/build.sh gateway|index      # wrangler deploy the worker (needs CLOUDFLARE_API_TOKEN)
./scripts/build.sh proxies            # deploy the satellite proxy workers
./scripts/build.sh deploy             # agent + gateway + index + proxies
```

Per-subproject details live in their own guides: `agent/AGENTS.md` (build, test, release, layout),
`gateway/wrangler.jsonc` / `index/wrangler.jsonc`. Panel-first: `build.sh agent` rebuilds the panel
SPA before the exe (`panel.js` is embedded at compile time).

## Install / update

npm is the only channel: `vale setup` (pure local install), `vale update`, `vale rollback <ver>`,
`vale status`, `vale autostart on|off`, `vale tunnel …`. Install layout is registry-first
(`HKLM\SOFTWARE\Vale\Agent\{InstallDir,DataDir}`) and all path resolution goes through
`agent/src/paths.rs`. The Gateway is an optional Settings card (`POST /api/gateway/connect`).

## Commits

Conventional commits (`feat(agent): …`, `fix(panel): …`). Each commit leaves the tree green; the
record of what shipped lives in commit messages, not in a journal.
