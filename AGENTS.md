# Vale

One repo, one front door: `gateway/` (Vale Gate worker), `agent/` (Vale Agent, Windows),
`index/` (dist + CDN worker), `proxies/` (satellite workers), `extension/`, `brand/`, `docs/`.
The operator's own rules are `docs/CHARTER.md`; their inbox is `docs/agents/ideas.md`.

## Build

```bash
./scripts/build.sh agent              # panel SPA (vite + vitest) then cargo xwin release
./scripts/build.sh gateway|index      # wrangler deploy the worker (needs CLOUDFLARE_API_TOKEN)
./scripts/build.sh proxies            # deploy the satellite proxy workers
./scripts/build.sh deploy             # agent + gateway + index + proxies

cargo xwin check --target x86_64-pc-windows-msvc --features terminal,keyring   # fast agent check
```

Panel-first: `panel.js` is embedded with `include_str!`, so a change under
`agent/resources/panel-react/` needs `npm run build` there (or `build.sh agent`, which does it).
The exe lands in `agent/target/x86_64-pc-windows-msvc/release/vale-agent.exe`.

## Test

```bash
cd agent/resources/panel-react && npm test       # panel (vitest)
cd agent && cargo test                           # agent
cd agent && cargo test --features terminal,keyring
cd agent && cargo clippy --all-targets -- -D warnings        # also with --features terminal,keyring
cd gateway && npm test                           # gateway (own prettier gate)
```

Green tests are the bar for a release.

## Release — npm is the only channel

```bash
# 1. bump agent/vale-agent-npm/package.json "version" to 1.2.N, then:
touch agent/src/lib.rs && ./scripts/build.sh agent
cp agent/target/x86_64-pc-windows-msvc/release/vale-agent.exe agent/vale-agent-npm/vale-agent.exe
# 2. publish (pack + manifest + prune + deploy + smoke; it does NOT commit):
./scripts/publish-release.sh 1.2.N
# 3. ONE commit that includes agent/vale-agent-npm/package.json and index/public/vale-agent/version.json
git push origin main          # CI green on the pushed commit
# 4. tag through the API (git push of tags times out here) — this triggers release.yml:
curl -sX POST -H "Authorization: Bearer $(cat ~/.github-token)" \
  https://api.github.com/repos/SilasVale/vale/git/refs \
  -d "{\"ref\":\"refs/tags/v1.2.N\",\"sha\":\"$(git rev-parse HEAD)\"}"
# 5. audit CDN vs the GitHub asset, byte for byte:
./scripts/publish-release.sh --audit-only 1.2.N
```

On the device (PowerShell) — the `--prefix` matters: without it npm installs elsewhere, reports
success, and `vale update` ships the old exe:

```powershell
npm i -g --prefix (Split-Path (Get-Command vale).Source) https://agent.saisi.online/vale-agent/vale-agent-latest.tgz
vale update
vale status
```

Two things that cost a device restart when ignored: **never launch a second `vale-agent.exe` from
an agent-hosted PTY** (it inherits the kill-on-close job and kills the running agent), and **never
kill/copy the exe inline over a PTY** — use the npm flow above.

## Agent layout

```
agent/src/main.rs        server binary (argv[1] = config path); Windows service via SCM
agent/src/lib.rs         crate root (DEFAULT_CONFIG_YAML embedded)
agent/src/paths.rs       registry-first install_dir()/data_dir() + layout-v2 subdirs
agent/src/state.rs       AppState (terminal_mgr, event_bus, plugin_registry, config)
agent/src/web/           HTTP surface: auth + dispatch + api_* handlers (hand-rolled Tower)
agent/src/metrics.rs     vitals + the 30 s sampler behind /api/vitals/history
agent/src/monitor.rs     reachability: persisted host:port targets, 15 s probes (TCP or HTTP)
agent/src/tools/         TerminalManager + backends (pty/ssh/serial), serial pool, ssh client
agent/src/plugins/       terminal, update, mcp_client, design, playwright, memory, system,
                         runs, monitor
agent/vale-command-core/ Plugin/ToolDef/Config/EventBus/DeviceError (vale_agent_core::)
agent/vale-agent-npm/    the npm package + the `vale` CLI (bin/vale.js)
agent/resources/panel-react/  the panel SPA (React + vitest); resources/panel/ is its build output
```

Features gate behind `terminal`/`keyring` with identical public paths across configs. A new MCP
tool is defined in its plugin's `tools.rs` (the registry caches at register time) and, to be
callable from the console, must be registered in `gateway/src/mcp-tools.ts` **and** matched by
`isDeviceDirectTool()`; after adding or removing a tool run
`VALE_REFRESH_SPEC=1 cargo test --features terminal,keyring spec_snapshot`.
