# Summrise

One repo, one front door: `gateway/` (Summrise Gate worker), `agent/` (Summrise Agent, Windows),
`index/` (dist + CDN worker), `proxies/` (satellite workers), `brand/`, `docs/`.
The operator's own rules are `docs/CHARTER.md`; their inbox is `docs/agents/ideas.md`.

## Build

```bash
./scripts/build.sh agent              # panel SPA (vite + vitest) then cargo xwin release
./scripts/build.sh gateway|index      # wrangler deploy the worker (needs CLOUDFLARE_API_TOKEN)
./scripts/build.sh proxies            # deploy the satellite proxy workers
./scripts/build.sh deploy             # agent + gateway + index + proxies

cargo xwin check --target x86_64-pc-windows-msvc --features terminal,keyring   # fast agent check
```

Panel-first: `panel.js` is embedded with `include_str!`, so a change under `agent/resources/panel-react/` needs
`npm run build` there (or `build.sh agent`). The exe lands in
`agent/target/x86_64-pc-windows-msvc/release/summrise-agent.exe`.

## Test

```bash
cd agent/resources/panel-react && npm test       # panel (vitest)
cd agent && cargo test                           # agent
cd agent && cargo test --features terminal,keyring
cd agent && cargo clippy --all-targets -- -D warnings        # also with --features terminal,keyring
cd gateway && npm test                           # gateway (own prettier gate)
```

Green tests are the bar for a release. **The per-directory list is `ci.yml` — read it there, not from a copy here.**

**READ THE EXIT CODE, NOT THE OUTPUT.** The suites do not share a reporter, and grepping for the wrong line returns
NOTHING, which looks exactly like a suite that passed silently: `cargo test` -> `test result: ok. N passed; 0 failed`;
`npm test` in `gateway/` -> `ℹ pass N` (Node 20 prints `# pass N`); `npx vitest run` in `panel-react/` -> `Tests N passed`.

**RUN THE COMMAND THE OTHER END RUNS** — a local check that is not CI's check is not the same check.

## Rules

**`main` ADVANCES ONLY BY MERGE**, with `--no-ff`, through a branch that was verified and reviewed. A direct commit on
`main` is refused by `scripts/hooks/main-only-by-merge` and by `agent/tests/main_shape.rs` (which reads the commit
OBJECT, because CI's clone is shallow).

**DO NOT PUSH WHILE A CI RUN IS IN FLIGHT** — the push cancels it and leaves that commit with no green run to point at.
The hook is `scripts/hooks/ci-not-in-flight`. **So batch the merges**: every merge is a serialization.

**SET `umask 022` BEFORE ANY GIT OPERATION THAT WRITES FILES** (a merge, a checkout, a worktree add). A file CREATED
inherits the umask; a file edited in place keeps its mode. `git status` cannot see it; `publish-release.bash`'s
whole-tree check can, and it prints the one-line repair.

**A WORKTREE PER TRACK, REMOVED WHEN ITS BRANCH LANDS** (`git worktree remove ~/wt/<track>`), and never under `/tmp` —
the OS deletes those, taking every uncommitted file with it. Commit before leaving a track idle. Each worktree carries
its own `agent/target`: `rm -rf` it for a track you are keeping.

**PREFER RUST.** A change to a `.js`/`.mjs`/`.cjs` file needs a reason, not a habit. **The boundary is a gate**:
`agent/tests/js_boundary_inventory.rs` classifies every tracked JS/TS file as LOGIC / BOUNDARY / RENDERING / GENERATED
against `agent/tests/fixtures/js-boundaries.txt`, and caps the LOGIC count. **A test belongs to the code it tests; a
fixture is not a decision.**

**A CHANGE UNDER `agent/scripts/` IS A CHANGE UNDER `gateway/`**: the console serves a byte-for-byte mirror, so run
`bash gateway/scripts/sync-code-viewer.sh && cd gateway && npm test`.

**EVERY GATE CARRIES ITS OWN `MUTATION:` / `RESULT:` PROOF IN ITS HEADER.** Read it when you change the gate; a gate that
cannot be broken is worse than no gate. **A gate is named where a person can find it.**

**AND WHEN YOU FIX SOMETHING, GREP FOR THE TEXT YOU JUST FIXED** — a fix at one of two call sites is invisible to the
compiler, the suite and the person who applied it.

**A MEASUREMENT BELONGS TO THE TREE IT WAS TAKEN ON — NAME THE BASE.** A number without its tree is a number about
somebody else's tree. **AND A COUNT HAS TWO EDGES**: it catches a regression and it HIDES a deletion — when a number
improves, ask what was removed. Match a test's name in the SOURCE, never as a reporter renders it.

**AFTER A DELETION, FOUR INSTRUMENTS, EACH BLIND TO SOMETHING**: the import graph (blind to a route, and to a test that
imports nothing you deleted), the compiler (blind to every `.mjs`), the route table (blind to a test), and the suite
(blind to nothing static, but only sees what it is pointed at). Three can be green while the fourth is red. **Run the
suite, and read the route table.**

**A STATUS SAYS WHAT WAS CHECKED.** When a surface reports a state, name the thing observed and when. A step that
succeeded is not an outcome; a request that returned 200 is not a device that answered. If nothing was checked, say that.

**THE REPOSITORY IS WRITTEN IN ENGLISH** — commits, comments, docs. **TALKING TO THE OPERATOR IS IN CHINESE.**

## Committing

A pre-commit hook (`scripts/hooks/pre-commit`) runs the five emitters, which is the only end-to-end assembly of all five
artifacts in under a second. It is installed as a repo-local `.githooks/` — **the COMMAND is the artifact**:

    mkdir -p .githooks
    ln -sf ../scripts/hooks/pre-commit .githooks/pre-commit
    ln -sf ~/.config/git/hooks/post-commit .githooks/post-commit
    git config --local core.hooksPath .githooks

**IN A WORKTREE, USE THE WORKTREE'S OWN HOOK** — `git -c core.hooksPath="$PWD/.githooks" commit` — and **put `cargo` on
PATH for the commit**, because the emitters need a toolchain they cannot see.

**DO NOT PUT A COMMAND WHOSE STATUS YOU NEED ON THE LEFT OF A PIPE**, and never `cmd && echo ok || echo FAIL` — the `||`
branch runs either way, so the line succeeds and `set -e` never fires. Make the failure EXIT. Redirect to a file and read
it afterwards.

**TEXT FULL OF BACKTICKS GOES THROUGH A QUOTED HEREDOC**, never `python3 -c "…"` or `$(printf …)`: backticks inside
double quotes are command substitution. Quote the DELIMITER, and choose a name the body cannot contain.

**A SUMMARY IS A CLAIM SET.** If a number goes into a commit message or a report, the command that produced it belongs
beside it. Three convenient counts are wrong here: `grep -l` counts files that MENTION a thing; `grep -c '^| '` counts
the header and separator; `git tag` answers zero in this clone (use `GET /repos/SilasVale/summrise/git/refs/tags`).

## Release

**THE RUNBOOK IS `docs/RELEASE.md` — read it when you release.** `./scripts/publish-release.sh N --dry-run` runs every
gate without credentials and changes nothing. Three rules hold at all times: **the tag's SHA must be a pushed commit
whose CI is green and the tag must never move**; **do not push while a release commit's CI is running**; **publish to
`latest`, not `alpha`**.

### Which gates have been PROVEN to bite

**EVERY GATE CARRIES ITS OWN `MUTATION:` / `RESULT:` PROOF IN ITS HEADER** — the edit that must break it, and what it
said when it did. Read that block when you change a gate: a gate that cannot be broken is worse than no gate. Three JSON
fixtures cannot hold a comment, so their proofs are in `agent/tests/fixtures/MUTATIONS.md`.

## Every gate CI runs

`all-gates.bash` · `build-pins.bash` · `hook-finds-its-repo.bash` · `main-only-by-merge.bash` ·
`main-shape-shallow.bash` · `rust-byte-checks.bash` · `panel-design-sweep.bash` · `publish-release.bash` ·
`release-audit.bash` · `release-lib.bash` · `scan-dups-check.py` · `script-syntax.bash` · `smoke-helpers.bash` ·
`smoke-index.bash` · `sweep-judges.bash`

**AND THE RUST GATES RUN IN `cargo test`**: `agent/tests/*.rs`, reached by the `agent` job's
`cargo test -p summrise-agent`. One by hand: `cd agent && cargo test -p summrise-agent <name>`. Six of them spawn a
toolchain, so that command needs node and three `node_modules` (the `agent` job installs them); `console_assets` refuses
a dirty tree, so run the one gate by name while editing.

## The vocabulary

**`CONTEXT.md` at the root is this project's GLOSSARY** — the words that mean something specific here (device, session,
run, goal, liveness, mark) and the words that do not, each with an `_Avoid_` list. Read it before naming anything in the
panel, the console or a tool description. **TERMS ONLY** — an invariant belongs in the gate that enforces it.

## Where the long form lives

| where | what belongs there |
|---|---|
| **the commit message** | the measurement, the before/after, the `VERIFIED:` line — this is the record |
| **`CONTEXT.md`** | a WORD and what it means. Terms only |
| **the gate itself** | any invariant that can be checked, with its mutation proof in the header |
| **`docs/superpowers/`** | a design and its decisions, including the ones that were wrong |
| **`docs/RELEASE.md`** | the release runbook |

**`agent/tests/docs_budget.rs` holds this up**: this file's 48,000-byte ceiling, no narrative `###` sections, and
`CONTEXT.md` at 12,000. **Prune by the rule: if a sentence does not change what you would DO, it does not belong here** —
the incident it came from is in the commit history.

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
agent/src/plugins/       terminal, update, mcp_client, design, playwright, memory, system, runs, monitor
agent/summrise-command-core/ Plugin/ToolDef/Config/EventBus/DeviceError (summrise_agent_core::)
agent/summrise-agent-npm/    the npm package + the `summrise` CLI (bin/summrise.exe, a Rust binary)
agent/resources/panel-react/  the panel SPA (React + vitest); resources/panel/ is its build output
```

Features gate behind `terminal`/`keyring` with identical public paths across configs. A new MCP tool is defined in its
plugin's `tools.rs` and, to be callable from the console, registered in `gateway/src/mcp-tools.ts` **and** matched by
`isDeviceDirectTool()`; after adding or removing one run `SUMMRISE_REFRESH_SPEC=1 cargo test --features
terminal,keyring spec_snapshot` — a snapshot that does not notice a new parameter is not pinning the surface.
