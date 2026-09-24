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

**A NEWLY CREATED WORKER NEEDS ITS SECRETS AND ITS BUCKET'S CONTENTS — the cutover, not
follow-up work.** After creating or renaming a worker: `wrangler secret list --name <worker>`
must not be `[]`, and every object its routes read must be in the bucket it binds. The rename
that skipped both left the file relay's upload leg answering 401 and the playwright route 502,
invisible for a day. The measurements are in the ledger.

Panel-first: `panel.js` is embedded with `include_str!`, so a change under
`agent/resources/panel-react/` needs `npm run build` there (or `build.sh agent`, which does it).
The exe lands in `agent/target/x86_64-pc-windows-msvc/release/summrise-agent.exe`.

## Test

```bash
cd agent/resources/panel-react && npm test       # panel (vitest)
cd agent && cargo test                           # agent
cd agent && cargo test --features terminal,keyring
cd agent && cargo clippy --all-targets -- -D warnings        # also with --features terminal,keyring
cd gateway && npm test                           # gateway (own prettier gate)
```

Green tests are the bar for a release.

**AND RUN THE COMMAND THE OTHER END RUNS.** A local check that is not CI's check is not the same check —
`gateway/ui` passed a local `tsc --noEmit` carrying six type errors, because the `ui` job runs `npm run build`
instead. The story is in the ledger; the commands, by working directory:

| where | CI runs | and NOT |
|---|---|---|
| `gateway/` | `npm run typecheck` (= `tsc --noEmit`) · `npm test` · `npm run lint` (= `eslint src/`) · `npm run format:check` | — |
| `gateway/ui/` | `npm run build` (= `tsc -b && vite build && prune-stale-assets`) · `npm test` | **not** `tsc --noEmit`, which is the check that missed them |
| `agent/resources/panel-react/` | `npm run build` · `npm test` | — |
| `agent/summrise-agent-npm/` | `npm test` (= `node --test`, 55 cases) — **no install step**: the package has zero dependencies and no lockfile, so `npm ci` fails with `EUSAGE` | — |
| `agent/` | `cargo fmt --all -- --check` · `cargo clippy -p summrise-agent --all-targets -- -D warnings` · `cargo clippy -p summrise-agent --features terminal,keyring --all-targets -- -D warnings` · `cargo clippy -p summrise-agent-core --all-targets -- -D warnings` · `cargo test -p summrise-agent` · `cargo test -p summrise-agent --features terminal,keyring` · `cargo test -p summrise-agent-core` | — |
| `agent/summrise-desktop-electron/` | `npm test` | — |

All five were run by hand on the commit that added this table and all were green; before that, `gateway`'s lint and typecheck
and the agent's `fmt`/`clippy` had not been run by this loop at all, and the panel's `npm test`, not `npx vitest run`, is what
CI invokes.


**READ THE EXIT CODE, NOT THE OUTPUT.** The suites do not share a reporter, and grepping for the wrong
one returns NOTHING — which looks exactly like a suite that passed silently:

| command | the line to look for |
|---|---|
| `cargo test` (either config) | `test result: ok. N passed; 0 failed` |
| `npm test` in `gateway/` (Node 24) | `ℹ pass N` — on Node 20 it prints `# pass N` instead |
| `npx vitest run` in `panel-react/` | `Tests  N passed` |

`exit 0` is the answer in every case; the line is a convenience. (Two rounds were once spent reading a silent grep as
"the suite did not run" and re-running it another way. The reporter table is what prevents that; the story is in the ledger.)

### Which gates have been PROVEN to bite

**THE GATES THE STANDING OBJECTIVE ADDED ARE IN THE LEDGER, NOT HERE** (beginning with `contract-vocabulary-check`,
`one-derivation-check`, `session-row-check`, `wire-field-check`, `console-wire-field-check`, `gateway-device-field-check`,
`device-verdict-check`, `sweep-fixture-dupes-check` and `production-host-check`, plus the `harness-fixture-check` changes;
`stub-surface-check` and `ci-command-table-check` came later, `session-carry-detect-check` later
still, `device-version-rule-check` after that, and `workflow-shell-check` most recently). Each
carries the mutation that must fail it, and each was
written because the rule it holds had ALREADY cost a real defect. They live in `docs/agents/design-ledger.md` — section
"Which mutation must fail which gate" — rather than in this table (`inventory.md` §12 names the INSTRUMENTS and their
verdicts, not the mutations), because THIS is the file that gets truncated when it grows — the rule below,
applied to itself.

**NO COUNT IS GIVEN HERE ON PURPOSE.** It said "ten" while the objective was at ten, and a round later there were twelve:
the same drift this file records for the sweep's check count ("the NUMBER is what drifts when a round adds a case without
updating this cell"). `numbered-claims-check.mjs` now holds the claim that a wired gate is a NAMED gate, which is the part
that can be checked; the count is left to whoever wants to count.


A gate that cannot fail is worse than no gate, and the only way to know is to break the thing it
guards and watch what happens. Every gate below was audited that way (rounds 65-68) — none of them is
assumed:

**THE TABLE THAT WAS HERE LIVES IN THE LEDGER NOW** — `docs/agents/design-ledger.md`, section
"Which mutation must fail which gate". It ran to 37 rows and 31 KB, which is 68% of this file: the
truncation this section warns about, applied to itself. `scripts/test/gate-mutations-check.mjs` automates
the mutations that can be automated and runs them on every push; the ledger holds the rest.

## The design ledger

THE LONG FORM LIVES IN `docs/agents/design-ledger.md` — one section per round: what was measured, what it cost, and
what was learned. It was split out in round 67 after this file grew past the workspace instruction budget and the
harness began TRUNCATING its tail (the Release and layout sections below were being dropped silently — an instruction
file that cannot be read whole is worse than a short one).

READ IT WHEN you are about to re-measure something, want the story behind a rule here, or wonder whether a failure has
happened before. The numbers for contrast, silhouettes, the loud axis, idle repaint, the press passes and the wire
contracts are all in there, with the mistakes that produced them.

What stays HERE: how to build, how to test, which mutation each gate must fail, how to commit, how to release, and
where the code lives. If a sentence does not change what you would DO, it belongs in the ledger — and a round that
learned something new adds a section THERE, not here.

## Committing

**A pre-commit hook runs the emitters** (`scripts/hooks/pre-commit`, round 225; rationale rewritten round 272).
It used to guard the backtick-in-a-template-literal accident — **and that class is gone**: all five emitters now hand
their payload to `agent/scripts/lib/sweep-bundle.mjs`, which resolves the payload's own requires and COMPILES what it
returns. (The incident count was quoted as 52 here, 34 in the hook and 38 in the operator's inbox; the three never
agreed, and they are history — the ledger holds the incidents.) What the hook still buys is the only end-to-end
assembly of all five artifacts in under a second: a payload module that does not parse, a require the assembler cannot
resolve, or an emitter that was renamed or deleted fails at the commit instead of in the design job.

**AND IT IS STILL NOT INSTALLED, WHICH HAS NOW COST A PUSH.** Round 93's commit carried a backtick in a comment,
the probe module stopped PARSING, and five of the ten CI jobs went red (ui, panel, gateway, design, pack-chain —
everything that imports it). The hook refuses that commit in under a second, and so does
`contrast-probe-check.mjs`, which CI runs at `ci.yml:506` — but nothing runs the hook unless the loop does it by
hand, because `core.hooksPath` is global. RUN IT BY HAND BEFORE EVERY COMMIT until the symlink below exists; the
story is in the ledger under "THE 46TH BACKTICK REACHED A COMMIT".

TWO THINGS ABOUT INSTALLING IT, both measured rather than assumed:

  * **`.git/hooks/pre-commit` will NOT run on this machine.** `core.hooksPath` is set globally in
    `~/.gitconfig` to `~/.config/git/hooks`, and git ignores the per-repo directory entirely when that is set.
    The first version of this hook was symlinked into `.git/hooks/` and a deliberately broken emitter was
    committed twice with it in place. Install it where the config actually looks:

        ln -sf "$PWD/scripts/hooks/pre-commit" ~/.config/git/hooks/pre-commit

    or set a repo-local path (which would SHADOW any global hooks, so read what is already there first):

        git config core.hooksPath scripts/hooks

  * **PROVE THE MUTATION, NOT THE HOOK.** The first attempt at proving it bit planted a backtick after
    `function browserScript() {` — inside the function body and OUTSIDE the template literal — so the emitter
    exited 0 and the test proved nothing about either. A trap only counts when it is inside the thing it traps.

## Release — npm is the only channel

```bash
# 1. bump agent/summrise-agent-npm/package.json "version" to 1.2.N, then:
touch agent/src/lib.rs && ./scripts/build.sh agent
cp agent/target/x86_64-pc-windows-msvc/release/summrise-agent.exe agent/summrise-agent-npm/summrise-agent.exe
#    DRY RUN FIRST if unsure: ./scripts/publish-release.sh 1.2.N --dry-run — every gate, no
#    credentials, changes nothing, seconds.
# 2. publish to BOTH channels (pack + manifest + prune + deploy + smoke; it does NOT commit):
#    --npm needs $NPM_TOKEN or ~/.npm-token, and publishes to `latest` (see below for why not alpha)
./scripts/publish-release.sh 1.2.N --npm
# 3. ONE commit that includes agent/summrise-agent-npm/package.json and index/public/summrise-agent/version.json
git push origin main          # CI green on the pushed commit
# 4. tag through the API (git push of tags times out here) — this triggers release.yml.
#    THE SHA MUST BE A PUSHED, CI-GREEN COMMIT, AND THE TAG MUST NOT MOVE ONTO DIFFERENT
#    CONTENT. All three were paid for on 1.2.453: tagging a local commit answers "Object
#    does not exist"; tagging a commit whose CI was superseded fails release.yml's own
#    "Gate on tag-commit CI status"; and a tag MOVE (a) demotes the release to a DRAFT,
#    which is invisible to the GET /releases/tags/<tag> read the audit makes, and (b)
#    makes CI package a DIFFERENT artifact under the SAME version number, which the
#    dual-builder audit then refuses — correctly. So: push, WAIT for that commit's CI to
#    go green, then tag it; and if CI must go green again for an already-published
#    version, put an EMPTY commit on the release commit (`git commit --allow-empty`) —
#    the tree is unchanged, so the asset matches what shipped. Transcripts: the ledger.
#    AND DO NOT PUSH ANYTHING WHILE A RELEASE COMMIT'S CI IS RUNNING. A push supersedes it,
#    GitHub cancels the run, and the tag then has no green CI to point at — twice on
#    2026-09-23 (94cb06fd and fe29a24d), both times because the next piece of work was
#    pushed by hand before the tag existed. Release, then resume.
#
#    AND `ci.yml` TRIGGERS ON `main` ONLY, WHICH MAKES THE EMPTY COMMIT A TWO-STEP. `release.yml`'s
#    gate is FAIL-CLOSED on silence ("zero check-runs + zero statuses means CI has not reported yet
#    (or never will) — WAIT, never pass"), so a commit that no workflow has ever seen can never be
#    tagged, however green the tree is. Measured on 1.2.463: the empty commit went onto a BRANCH,
#    the branch was pushed, and nothing ran. The step that was missing:
#
#      gh api repos/$REPO/actions/workflows/ci.yml/dispatches -f ref=<branch>      # HTTP 204
#
#    `ci.yml` carries `workflow_dispatch:` for exactly this. After it, the run attaches its
#    check-runs to that SHA and the tag proceeds. The empty commit still has to be PUSHED first —
#    a dispatch names a ref the runner must be able to fetch.
curl -sX POST -H "Authorization: Bearer $(cat ~/.github-token)" \
  https://api.github.com/repos/SilasVale/summrise/git/refs \
  -d "{\"ref\":\"refs/tags/v1.2.N\",\"sha\":\"$(git rev-parse HEAD)\"}"
# 5. audit CDN vs the GitHub asset, byte for byte (needs the asset, which is why step 4 comes first):
./scripts/publish-release.sh --audit-only 1.2.N
```

**PUBLISH TO `latest`, NOT `alpha`.** The CDN's `-latest.tgz` alias moves on every release, so npm's
`latest` must move with it. 1.2.453 shipped to `alpha` first and deadlocked a real device:
`npm i -g summrise-agent` installed a CLI older than the release the agent was asked to take, and the
CLI's own guard ("this CLI is 1.2.452 and the release channel has 1.2.453 — install the new CLI
first") pointed at a command that could not deliver it. `--npm-tag alpha` remains for a deliberate
prerelease channel.

On the device (PowerShell) — the `--prefix` matters: without it npm installs elsewhere, reports
success, and `summrise update` ships the old exe:

```powershell
npm i -g --prefix (Split-Path (Get-Command summrise).Source) https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz
summrise update
summrise status
```

**AND A BARE `npm i -g summrise-agent` CAN INSTALL NOTHING WHILE REPORTING SUCCESS.** A stale `latest`
resolved from npm's cache or this box's mirror printed `changed 1 package` and left the OLD version in
place. The URL above has no resolution step, so it is immune; an EXACT version (`summrise-agent@1.2.455`)
is what `setup` uses for the same reason. Verify with `summrise status` (`this CLI:`), never npm's exit code.

**AND MEASURE THE PANEL THE DEVICE IS ACTUALLY RUNNING, not only the harness.** Every design sweep renders the
HARNESS (a stubbed device, this checkout's bundle); nothing measured the live panel until
`agent/scripts/live-panel-probe.mjs` was pointed at `127.0.0.1:18080` on d1, where it immediately found the approval
gate's disarmed ring using the ink round 101 had replaced — in a rule no harness run could see. It needs a browser and
a running panel, so it cannot be a CI job: run it on the device after a `summrise update` (emit with `--emit`, hand the
script to the device's node or to `browser_run_script`).

**HOW TO HAND IT OVER, since a 38 KB script must not be pasted into anything:** emit it into the CDN's public dir
(`node agent/scripts/live-panel-probe.mjs --emit > index/public/summrise-agent/live-panel-probe.js`), deploy, and let
the DEVICE fetch it (`system_file_download` from `https://agent.saisi.online/summrise-agent/live-panel-probe.js`), then
`browser_run_script` with `require('D:/Summrise/live-panel-probe.js');`. It reads the panel token from the device's own
config and prints a JSON verdict. IT MUST NOT BE GITIGNORED: Workers Assets uploads the directory but HONOURS
`.gitignore`, so an ignored file is silently absent from the deploy — which is why the playwright zip was never a
static asset (its route reads R2) and why the probe is committed like the panel build and `bin/summrise.js` are.

Two things that cost a device restart when ignored: **never launch a second `summrise-agent.exe` from
an agent-hosted PTY** (it inherits the kill-on-close job and kills the running agent), and **never
kill/copy the exe inline over a PTY** — use the npm flow above.

**THE npm PACKAGE CARRIES NO BOXED COMPONENTS — `setup` FETCHES AND VERIFIES THEM.** The package is ~6.7 MB: the exe,
the CLI, the desktop shell's *sources*. `cloudflared.exe` (54 MB), `summrise-playwright.zip` (31 MB) and the **electron
runtime** the desktop shell launches are served by the release host and staged into `<install>\components` by
`resolveComponent()` (host route TODAY; the package arm is a seam) (`curl -fsSL`, so an HTTP error
is a FAILURE and not a 404 page on disk). Both cloudflared and electron are STAGED IN R2 now, which is what lets
`index/components.json` pin a sha256 for each and `version.json` publish it: setup REFUSES a mismatch, warns when a
release carries no pin, and never touches GitHub — a device behind the GFW needs no mirror. An *upgrade* was never
affected (components live in `<install>\components` and survive); this bites at install and migration time. The
migration that came up local-only, and why it cost an hour, is in `docs/BRAND.md`.

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
agent/summrise-command-core/ Plugin/ToolDef/Config/EventBus/DeviceError (summrise_agent_core::)
agent/summrise-agent-npm/    the npm package + the `summrise` CLI (bin/summrise.js)
agent/resources/panel-react/  the panel SPA (React + vitest); resources/panel/ is its build output
```

Features gate behind `terminal`/`keyring` with identical public paths across configs. A new MCP
tool is defined in its plugin's `tools.rs` (the registry caches at register time) and, to be
callable from the console, must be registered in `gateway/src/mcp-tools.ts` **and** matched by
`isDeviceDirectTool()`; after adding or removing a tool run
`SUMMRISE_REFRESH_SPEC=1 cargo test --features terminal,keyring spec_snapshot`.
