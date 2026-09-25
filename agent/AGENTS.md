# Summrise Agent — build, test, release

## Build

```bash
./scripts/build.sh agent        # panel SPA (vite + vitest) then cargo xwin release — THE build
cargo xwin check --target x86_64-pc-windows-msvc --features terminal,keyring   # fast check
```

Panel-first: `panel.js` is embedded with `include_str!`, so any change under
`resources/panel-react/` needs `npm run build` there (or `build.sh agent`, which does it).
Exe lands in `target/x86_64-pc-windows-msvc/release/summrise-agent.exe`.

## Test

```bash
cd resources/panel-react && npm test             # panel (vitest)
cargo fmt --all -- --check                       # --check, because CI never runs the MUTATING form
cargo clippy -p summrise-agent --all-targets -- -D warnings
cargo clippy -p summrise-agent --features terminal,keyring --all-targets -- -D warnings
cargo clippy -p summrise-agent-core --all-targets -- -D warnings
cargo test -p summrise-agent
cargo test -p summrise-agent --features terminal,keyring
cargo test -p summrise-agent-core
cd ../gateway && npm test                        # gateway (own prettier gate)
```

**THESE ARE CI'S COMMANDS, NOT CONVENIENT ONES — and this file used to give the convenient ones.** It said
`cargo test` and `cargo clippy --all-targets` with no `-p`, which is a SUBSET of what CI runs (it never built
`summrise-agent-core`), and `cargo fmt --all`, which is the MUTATING form while CI runs `-- --check`. The root
AGENTS.md records where that class of gap has already bitten: `gateway/ui` passed a local `tsc --noEmit` carrying
six type errors, because the `ui` job runs `npm run build` instead. **A command that edits your tree is not a check.**

Green tests are the bar for a release.

## Release — npm is the only channel

```bash
# 1. bump summrise-agent-npm/package.json "version" to 1.2.N, then:
touch agent/src/lib.rs && ./scripts/build.sh agent
cp agent/target/x86_64-pc-windows-msvc/release/summrise-agent.exe agent/summrise-agent-npm/summrise-agent.exe
# 2. publish (pack + manifest + prune + deploy + smoke; it does NOT commit):
./scripts/publish-release.sh 1.2.N --npm
# 3. ONE commit that includes summrise-agent-npm/package.json and index/public/summrise-agent/version.json
git push origin main
#    AND NOW **WAIT** FOR THAT COMMIT'S CI TO GO GREEN BEFORE TAGGING ANYTHING. The SHA must be a pushed,
#    CI-green commit, and THE TAG MUST NOT MOVE ONTO DIFFERENT CONTENT — a moved tag demotes the release to a
#    DRAFT (invisible to the audit's GET /releases/tags/<tag>) and makes CI package a different artifact under
#    the same version number. release.yml's gate is FAIL-CLOSED on silence: zero check-runs means CI has not
#    reported yet (or never will) — WAIT, never pass. All three were paid for on 1.2.453; the transcripts are in
#    the ledger.
#
#    AND DO NOT PUSH ANYTHING WHILE A RELEASE COMMIT'S CI IS RUNNING. A push supersedes the run, GitHub cancels
#    it, and the tag then has no green CI to point at. That cost three runs in this repository (twice on
#    2026-09-23, once in round 45) — the cancellation is reported as `conclusion: failure`, INDISTINGUISHABLE
#    FROM A REAL FAILURE in a count. Release, then resume.
# 4. tag through the API (git push of tags times out here) — this triggers release.yml:
curl -sX POST -H "Authorization: Bearer $(cat ~/.github-token)" \
  https://api.github.com/repos/SilasVale/summrise/git/refs \
  -d "{\"ref\":\"refs/tags/v1.2.N\",\"sha\":\"$(git rev-parse HEAD)\"}"
# 5. audit CDN vs the GitHub asset, byte for byte:
./scripts/publish-release.sh --audit-only 1.2.N
```

On the device (PowerShell) — the `--prefix` matters: without it npm installs elsewhere, reports
success, and `summrise update` ships the old exe:

```powershell
npm i -g --prefix (Split-Path (Get-Command summrise).Source) https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz
summrise update
summrise status
```

Two things that cost a device restart when ignored: **never launch a second `summrise-agent.exe` from
an agent-hosted PTY** (it inherits the kill-on-close job and kills the running agent), and **never
kill/copy the exe inline over a PTY** — use the npm flow above.

## Layout

```
src/main.rs        server binary (argv[1] = config path); Windows service via SCM
src/lib.rs         crate root (DEFAULT_CONFIG_YAML embedded)
src/paths.rs       registry-first install_dir()/data_dir() + layout-v2 subdirs
src/state.rs       AppState (terminal_mgr, event_bus, plugin_registry, config)
src/web/           HTTP surface: auth + dispatch + api_* handlers (hand-rolled Tower)
src/metrics.rs     vitals + the 30 s sampler behind /api/vitals/history
src/monitor.rs     reachability: persisted host:port targets, 15 s probes (TCP or HTTP path)
src/tools/         TerminalManager + backends (pty/ssh/serial), serial pool, ssh client
src/plugins/       terminal, update, mcp_client, design, playwright, memory, system, runs, monitor
summrise-command-core/ Plugin/ToolDef/Config/EventBus/DeviceError (import as summrise_agent_core::)
summrise-agent-npm/    the npm package + the `summrise` CLI (bin/summrise.js)
resources/panel-react/  the panel SPA (React + vitest); resources/panel/ is its built output
```

Conventions that matter: features gate behind `terminal`/`keyring` with identical public paths
across configs; a new MCP tool is defined in its plugin's `tools.rs` (the registry caches it at
register time) and, to be callable from the console, must be registered in
`gateway/src/mcp-tools.ts` **and** matched by `isDeviceDirectTool()`; after adding or removing a
tool, run `SUMMRISE_REFRESH_SPEC=1 cargo test --features terminal,keyring spec_snapshot`.
