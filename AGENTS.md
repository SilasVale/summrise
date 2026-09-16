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

### Which gates have been PROVEN to bite

A gate that cannot fail is worse than no gate, and the only way to know is to break the thing it
guards and watch what happens. Every gate below was audited that way (rounds 65-68) — none of them is
assumed:

| gate | mutation that must fail it | result |
|---|---|---|
| `scripts/test/token-contract-check.mjs` | change a shared token's value on one side | exit 1 |
| `cargo test --features terminal,keyring spec_snapshot` | add a parameter inside a device tool's `properties` | exit 101, snapshot diff |
| `scripts/test/panel-audit-skip-check.mjs` | make the audit `exit(0)` on a skip | exit 1, names the distinction |
| `agent/tests/fixtures/approval-grants.json` | rename a member the panel mirror reads | both sides fail |
| `agent/tests/fixtures/session-row.json` | rename `idle_ms` to `idleMs` | device + panel fail |
| `agent/tests/fixtures/embedded-bridge.json` | rename `fwd` to `forward` | shell + panel fail |
| `scripts/test/panel-design-sweep.bash` | plant a defect per axis in a report | one check per axis |
| `scripts/test/release-lib.bash` | prune keeps 4 instead of 5 per minor | exit 1, actual/expected listed |
| `scripts/test/smoke-index.bash` | read the versioned installer instead of the versionless alias | exit 1 |
| `scripts/test/smoke-helpers.bash` | accept a truncated sha256 | exit 1, prints the offending value |
| `scripts/test/release-audit.bash` | stop recording mode drift | exit 1 |
| `scripts/test/publish-release.bash` | disable the stale-exe refusal | exit 1 — **after round 67 ADDED the case that does it** |
| `scripts/test/build-pins.bash` | bump rust-toolchain's channel alone | exit 1, names the workflow literal |
| `scripts/test/script-syntax.bash` | append an orphan `fi` to a shell script | exit 1, with file and line |
| `scripts/test/contrast-probe-check.mjs` | remove the probe's hex handling | exit 1, "both spellings must parse" |
| `scripts/test/e2e-only-check.mjs` | make the zero-selection guard exit 0 | exit 1, "reported success having run nothing" |
| `scripts/test/scan-dups-check.py` | stop recognising `*_test.rs` files | exit 1, names the file |
| `scripts/test/model-drift-check.mjs` | remove the normaliser's bracket-suffix strip | exit 1, prints the un-normalised id |

The whole RELEASE PATH is now proven, which is the part where a toothless guard ships a broken
release: the prune, the version.json writer, the installer-alias arm and the sha256 gate all fail
when their subject breaks. (Failure messages differ in usefulness: `release-lib` prints actual vs
expected and `smoke-helpers` prints the value it rejected, while `smoke-index` says only "an
advertised installer passes" — accurate, and less use to whoever hits it. Left alone deliberately:
a terse message is not a defect, and churning it buys nothing measurable.)

**Every gate in `scripts/test/` is now audited** (rounds 65-68). A new one should be added to this
table with the mutation that proves it — an unaudited gate is an assumption, and this table is where
that stops being invisible.

TWO THINGS THE AUDIT TAUGHT ABOUT AUDITING (round 67):
  * a gate that asserts a CLEAN WORKTREE rejects a mutation before it can prove anything — so
    `publish-release.bash` can only be mutation-tested by committing the mutation temporarily and
    resetting afterwards. My first attempt read its failure ("the refusal modified the tree") as a
    verdict about the refusal; it was the cleanliness check, and only the committed-mutation run
    showed the truth: the gate passed with the refusal disabled, i.e. the check had NO coverage;
  * a mutation can have SIDE EFFECTS. With the refusal disabled the script walked past it and packed
    a tgz, which the gate's own side-effect check caught. The artifacts were removed; the lesson is
    that "break the guard and see" can also break something, so look for what the run left behind.

THE METHOD HAS A TRAP, and it caught THREE mutations:

  * round 65: `probe_param` inserted at the top level of a tool's JSON instead of inside `properties`
    changed nothing the snapshot reads, so the gate "passed" and proved nothing about the gate;
  * round 68, twice: the zero-selection guard's MESSAGE was deleted while its `process.exit(2)`
    stayed, so the guard still fired and the gate rightly passed; and a "remove `.toLowerCase()`"
    edit matched nothing because the normaliser has no `toLowerCase`. Changing the narration — or
    changing nothing at all — is not changing the behaviour.

Every time the fix was the same: prove the mutation altered the thing under test before drawing a
conclusion about the guard. A "toothless gate" finding is a claim about the gate, and it is worth
exactly as much as the mutation behind it.

### An "untested surface" scan that found nothing (round 70)

Worth recording so it is not re-run: scanning the agent crate for `pub fn`s whose names never appear
in test code lists 65, and every one I checked is a GLUE wrapper around a tested core —
`retention_sweep` → `retention_sweep_in`, `close_abandoned_runs` → `runs::abandon_open_runs`. The
wrappers resolve real `DataDir` paths, so calling them from tests would mutate the developer's own
data; the files say so explicitly ("Deliberately NOT wired into `AppState::new`... those are
constructed by tests"). The apparent gap is deliberate testability design, and the scan's method was
also unreliable in the other direction: its `#[test]` extraction missed real call sites, so its count
is an upper bound, not a finding. Read the module before believing the count.

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
