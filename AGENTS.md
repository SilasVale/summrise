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
invisible for a day. The measurements are in that commit's message, which is where a decision now lives.

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

**AND RUN THE COMMAND THE OTHER END RUNS.** A local check that is not CI's check is not the same check — `gateway/ui`
passed a local `tsc --noEmit` carrying six type errors, because the `ui` job runs `npm run build` instead.

| where | CI runs |
|---|---|
| `gateway/` | `npm run typecheck` · `npm test` · `npm run lint` · `npm run format:check` |
| `gateway/ui/` | `npm run build` (= `tsc -b && vite build && prune-stale-assets`) · `npm test` — **not** `tsc --noEmit`, which is the check that missed those six |
| `agent/resources/panel-react/` | `npm run build` · `npm test` |
| `agent/summrise-agent-npm/` | **no JavaScript suite any more, and no step of its own**: its packaging validations are `agent/tests/npm_package.rs`, reached by the `agent` job's `cargo test -p summrise-agent`. It has zero dependencies and no lockfile (`npm ci` there fails with `EUSAGE`), so `npm pack --dry-run --json` — which the Rust gate spawns — needs no install step |
| `agent/summrise-desktop-electron/` | `npm test` |
| `agent/` | `cargo fmt --all -- --check`, then the clippy and test commands the `agent` job names — **read them out of `ci.yml` rather than from here**: several crates are workspace members that are deliberately NOT default members, so a bare `cargo test` does not reach them, and a copy of the list in this file goes stale (this row named "59 cases" for a suite that had grown to 68) |

**AND A CHANGE UNDER `agent/scripts/` IS A CHANGE UNDER `gateway/`.** The console's Source Viewer serves a byte-for-byte
MIRROR of `agent/scripts/*.mjs` and their `lib/` (not `lib/sweep/`), so editing one of those files leaves
`gateway/public/code/files/instruments/` stale and turns `gateway`'s `code viewer: the instruments mirror matches
agent/scripts byte for byte` red — with an assertion that names the fix:

```bash
bash gateway/scripts/sync-code-viewer.sh && cd gateway && npm test
```

**READ THE EXIT CODE, NOT THE OUTPUT.** The suites do not share a reporter, and grepping for the wrong one returns
NOTHING — which looks exactly like a suite that passed silently:

| command | the line to look for |
|---|---|
| `cargo test` (either config) | `test result: ok. N passed; 0 failed` |
| `npm test` in `gateway/` (Node 24) | `ℹ pass N` — on Node 20 it prints `# pass N` instead |
| `npx vitest run` in `panel-react/` | `Tests  N passed` |

`exit 0` is the answer in every case; the line is a convenience.

**TWO RULES ABOUT WHAT NOTHING RUNS — READ THEM BEFORE YOU TRUST A GREEN OR A RED.**

  * **You changed a terminal backend, a file-relay path, a workflow step, the panel's wiring, the MCP surface, the evidence
    drawer or the browser/playwright door** — then `agent/scripts/e2e/e2e.js` has a section for it, **seven of its nine
    sections run in NO CI job and NO schedule**, and the rule is how to get the script onto a device and run it there.
  * **A CI job is red and you did not expect it to be** — a CANCELLED job (any push while a run is in flight) reports
    `conclusion: failure`, **indistinguishable from a real one in a count**; the rule is the log line that tells them apart.

**`main` ADVANCES ONLY BY MERGE** — four artifacts, and each one covers a way the others cannot.
`scripts/hooks/main-only-by-merge` is the rule, called by `pre-commit` **after** its "is this Summrise?" guard, so it
cannot see a fast-forward (which creates no commit and so never runs a hook) or `--no-verify`.
`agent/tests/main_shape.rs` is the CI gate: `main` must be at a commit with **two or more parents**, and it reads the
commit OBJECT rather than `git rev-list --parents`, because `actions/checkout@v4` gives the runner a **shallow** clone
whose grafted boundary commit answers zero parents for a commit that has two.
`scripts/test/main-only-by-merge.bash` proves the rule on a throwaway repo's real pre-commit hook;
`scripts/test/main-shape-shallow.bash` builds the shallow graft that no full clone can reproduce.

Work reaches `main` through a branch that was verified and reviewed, merged with **`--no-ff`** — the flag matters, because
a plain `git merge` fast-forwards when `main` has not moved. **A direct commit on `main` is refused by the hook and by the
gate**, and the hook lives in its own file precisely so it can be proven on a real repository (a rule inside `pre-commit`
sits below a guard that exits 0 in any repo that is not this one, so a fixture could never reach it).

### Which gates have been PROVEN to bite

**EVERY GATE CARRIES ITS OWN PROOF, IN ITS OWN HEADER**: a `MUTATION:` / `RESULT:` block naming the edit that must break
it, and what it said when it did. **Read that block when you change the gate** — a gate that cannot be broken is worse
than no gate. Three JSON fixtures cannot hold a comment (JSON has none), so their proofs are in
`agent/tests/fixtures/MUTATIONS.md` beside them.

**AND READ IT WHEN A FINDING SAYS SOMETHING IS UNUSED AND YOU ARE ABOUT TO DELETE IT** — that rule outlived the table it
was written for. **AND A GATE IS NAMED WHERE A PERSON CAN FIND IT, OR IT IS A GATE NOBODY CAN RUN BY HAND.**

## Pushing: wait for the run in flight

**A PUSH SUPERSEDES AN IN-FLIGHT CI RUN.** GitHub cancels it, and the cancellation is reported as `conclusion: cancelled`
— which a count cannot tell from a real failure, and which leaves that commit with **no green CI to point at**. The rule
is enforced mechanically, in the one place a push cannot skip:

| artifact | what it is |
|---|---|
| `scripts/hooks/pre-push` | the hook. Installed as `.githooks/pre-push`, which `core.hooksPath` already points at |
| `scripts/hooks/ci-not-in-flight` | the check, its own file so it can be proven on fixtures |
| `scripts/test/ci-not-in-flight.bash` | the proof: 7 cases on saved API responses, **and it RUNS the hook the way git does** rather than grepping for the call site |

**AND THE RULE HAS A COST, SO SPEND IT IN BATCHES: EVERY MERGE IS A SERIALIZATION** (~7 minutes of CI, and this hook then
refuses the next push until it finishes). `ci.yml` triggers on `main` ONLY, so a branch costs nothing and the run happens
at the merge. Five fixes merged one at a time is five waits; the same five merged once is one. **BATCH THE MERGES.**

**AND RUN `scripts/test/all-gates.bash` BEFORE THE PUSH RATHER THAN GUESSING WHICH GATE WILL GO RED** — it runs every
gate `ci.yml` invokes, locally, in one command, and answers in minutes what the `design` job answers in tens of them.
Guessing by name from memory is how `production-host-check` was missed until after a commit.

**IT FAILS OPEN, DELIBERATELY.** No token, no answer, unparseable JSON — all exit 0, because blocking a push over a flaky
mirror is a worse failure than the one it prevents. `git push --no-verify` is the escape hatch, and the hook prints it.

**A JOB RECORD AND ITS STEPS CAN DISAGREE — READ `steps[].conclusion`, NOT THE JOB, AND BOTH DIRECTIONS HAPPEN.** A run
can say `success` while a job says `in_progress`, and it can say `failure` while a job's ONLY step is `in_progress` —
**the runner died in `Set up job` and nothing of yours ran**, which is not a red gate but a re-run
(`POST …/runs/<id>/rerun-failed-jobs`).

**AND SET `umask 022` BEFORE ANY GIT OPERATION THAT WRITES FILES ON THIS BOX — a merge, a checkout, a worktree add.** This
shell's umask is **0002**, and the rule is narrower than "a rewrite": **A FILE THAT IS CREATED INHERITS THE UMASK; A FILE
EDITED IN PLACE KEEPS ITS MODE.** What drifts is what git and the tools CREATE (a merge that adds or replaces a file, a
worktree add, a scratch file), not what a patch rewrites. `git status` says NOTHING about it (git compares only the
owner-execute bit) and `git diff` is empty, so the only thing that can see it is `publish-release.bash`'s whole-tree mode
check. It matters at release time because `npm pack` preserves worktree modes: the 1.2.348 pair differs by 3 bytes out of
17,774,080 with every sha256 matching. If a merge already happened, the gate prints the repair and it is one line:
`git ls-files -s | awk '$1=="100644"{print $4}' | xargs -r chmod 644` (and the same for `100755`/`755` — **the 0755 half
is invisible to the `100644` count**, which is why both are printed).

## Running several tracks at once

**ONE WORKING TREE IS ONE TRACK, AND A SUBAGENT OWNS THE TRACK IT RUNS IN.** Any `git checkout` or merge by anyone else
pulls the tree out from under it — sharing one checkout is what makes work serial, and serial is not a property of the
work. Give each track its own worktree and its own branch (`using-git-worktrees`):

    git worktree add ~/wt/<track> -b <branch> main         # after umask 022, and NOT under /tmp

**AND NOT UNDER `/tmp` — MEASURED 2026-10-09, WHEN THE BOX REBOOTED AND EVERY WORKTREE WENT WITH IT.** All
seven trees vanished mid-round (`git worktree list` afterwards called each one `prunable`), and the damage
split along exactly one line: **every COMMITTED thing survived** — the object store is shared, so each branch
still held its tip — while **every UNCOMMITTED tree was gone**, three tracks' worth, unrecoverable. So the
rule is not "avoid /tmp" but the sharper one: **a worktree is a working copy, and an hour of uncommitted work
in it can be deleted by the operating system.** Commit before leaving a track idle, and keep the tree where a
reboot cannot reach.

**AND THE CAUSE IS A REBOOT, NOT A SWEEPER — MEASURED RATHER THAN ASSUMED, BECAUSE THE FIRST VERSION OF THIS
PARAGRAPH SAID "A `/tmp` SWEEP" AND THAT SENDS THE NEXT READER HUNTING FOR SOMETHING THAT IS NOT THERE.**
`who -b` -> *system boot Oct 9 15:44*; `/tmp`'s own tmpfiles rule is `D /tmp 1777 root root 30d`, i.e. **30
DAYS**, so `systemd-tmpfiles-clean` (active, and it did run at 15:59 that day) cannot delete a file from the
same afternoon. One reboot explains both losses this session recorded — the worktrees, and the `/tmp/wasmopt`
that `build.sh index` needed — and it is also why a device arm and a docker socket changed under the work
mid-round. **Check `uptime` before theorising about what deleted a file.**

Symlink the dependency trees a worktree needs (`agent/resources/panel-react/node_modules`, `gateway/node_modules`, …).
**`.gitignore`'s `node_modules/` matches a DIRECTORY, not a symlink**, so `git add -A` there stages the LINK — and checks
like `agent/tests/console_assets.rs` refuse a dirty tree, which is how it surfaces. The fix is a clone-local `.git/info/exclude`
line for the bare name `node_modules`.

**AND START LONG WORK IN THE BACKGROUND — a CI run, an `all-gates` run, a build, a subagent — then do the next thing and
collect it when it finishes.** Polling a job in the foreground turns three tracks back into one. `main` still merges one
branch at a time (the ref is shared), but a merge is seconds, and the other tracks keep working through it.

**AND A READ-ONLY COMMAND CAN STALL THE WHOLE BOX — TWO OF THEM DID, ON 2026-10-09.** `du -sh` over four home
directories ran **1h34m**, and `grep -rn underAA --include=* .` ran **45 minutes**; both were left behind by
tracks that had moved on, and between them they held load at ~23 while a linker collected **5 CPU ticks in 15
seconds** and every cargo gate in every worktree crawled. Neither was killing anything (both are re-runnable),
so the cost was pure time — and **the first one was visible in a process listing for two rounds before anyone
acted on it.** Check `ps` for long-running read-only commands before blaming the machine, and prefer the `grep`
TOOL over shell `grep -r`: this file already said shell grep "crawls or hangs", and the repo-wide form with
`--include=*` is that hazard at box scale.

**AND A LONG GATE RUN HAS ONE SHAPE THAT WORKS HERE: run it in the FOREGROUND with a long timeout and let the
tool move it to a managed job when the timeout expires.** Three other shapes failed the same afternoon —
`&` (the child died silently), `| tail -40` (the job lived but its output was swallowed, and `ps` could not find
it), and a second managed job whose process had already gone. The tool's own fallback is the mechanism: no `&`,
no pipe, and the output streams while it runs.

**AND A WORKTREE SILENTLY LOSES EVERY HOOK, WHICH IS HOW A DIRECT COMMIT ON `main` REACHED CI ON 2026-10-09.** The
install above sets `core.hooksPath .githooks` — a RELATIVE path, resolved against the root of whichever worktree git is
running in. `.githooks/` is gitignored and machine-local, so it exists only in the main checkout: in a worktree the path
resolves to a directory that is not there, **git treats missing hooks as no hooks, and every commit lands unguarded** —
no emitter check, no main-only-by-merge. Nothing warns; the commit succeeds. **SET IT ABSOLUTE, ONCE, and every worktree
inherits it** (the config is shared; only the path was relative):

    git config --local core.hooksPath "$PWD/.githooks"        # run in the main checkout

The backstop is `agent/tests/main_shape.rs`, which reads the commit OBJECT of `main` and refuses a tip with fewer than
two parents — it caught this in CI on the next push, which is the gate working, and it is also why the repair is a MERGE
onto the bad tip rather than a history rewrite: a new merge commit gives `main` two parents without moving anything
already published.

**AND THE ABSOLUTE PATH HAS A COST, MEASURED 2026-10-09: A COMMIT IN ANY WORKTREE RUNS THE MAIN CHECKOUT'S HOOK.**
That was the fix — worktrees silently losing every hook — and the price is that the hook cannot match the branch
being committed. Main's copy still named `scripts/test/contrast-probe-check.mjs`, which another branch had
DELETED, so a merge was refused for a file that did not exist on the tree being merged. **Run the worktree's own
hook instead of bypassing it**, by giving the worktree its own `.githooks/` (gitignored, the same two links) and
committing with:

    git -c core.hooksPath="$PWD/.githooks" commit

**AND THE HOOK NEEDS A TOOLCHAIN IT CANNOT SEE.** Its emitters refuse with *"the sweep plan is Rust and there is
neither a cargo on PATH nor a built …/summrise-sweep-plan"* — the hook runs without `~/.cargo/bin`, so **put
`cargo` on PATH for the commit** (or build `summrise-sweep-plan` once). Measured the hard way: a commit was
bypassed with `--no-verify` and blamed on the worktree's dependency trees, which were not the cause and whose
symlinking changed nothing. **A cause that survives its own attempted fix is not a cause.**

## A status says what was CHECKED

**EVERY SENTENCE A SURFACE SHOWS ABOUT THE DEVICE'S STATE IS A CLAIM, AND A CLAIM NOBODY CHECKED IS A LIE THE READER
BELIEVES.** The operator once read `registered · tunnel: ok` three separate times while a remote client got **530**
(*"为什么还没有上线呢"* each time): the string came from `RemoteConfig::Updated` and **reported the STEP just taken in the
grammar of the OUTCOME the reader wanted**, and nothing on that path had asked whether anything could reach the device. It asks
now (`tunnel_health_via_api`): the API's own `status` and `conns_active`, with `healthy` and zero connectors counted as
NOT reachable, because the status alone is not the test.

**THE PATTERNS THAT ALREADY EXIST HERE, AND THEY ARE THE ONES TO COPY:**

| surface | what it says | why it is honest |
|---|---|---|
| the panel's liveness | `sseState` drives `connected` / `disconnected` | it is the SSE connection's OWN state, and `IconRail` carries the rule — **ONE MODEL, ONE PLACE**; a second copy is how two surfaces come to disagree about one device |
| the relay chip | `vitals.relay.connected` | the relay's fact, reported by the relay |
| `UpdateCard` | **`checked 12s ago`** | a TIMESTAMP, not a verdict — and its tests cover the degenerate cases (`56 years ago`, `497204h ago`) |
| the landing | **`No Windows installer is published for this release`** | it says what it knows instead of offering the `Setup.exe` alias, which serves the PREVIOUS installer after a tgz-only publish |

**SO THE RULE IS NARROW, AND IT IS NOT "ADD MORE CAVEATS":** when a surface reports a state, name **the thing that was
observed** and **when**. A step that succeeded is not an outcome; a configuration that was written is not a service that
is up; a request that returned 200 is not a device that answered. **If nothing was checked, say that** — *"reachability
NOT VERIFIED"* is a better sentence than a confident `ok`, because the reader can act on it.

**AND DO NOT WEAKEN A CRITERION TO MATCH A MESSAGE.** The mirror image, from the same week: the design sweep reported
`acknowledged the press after 823ms — this feedback waited on the 824ms network round trip` for a button whose handler is
synchronous and whose popover has no network on it at all. **The judge was speculating about a cause its own row cannot
check.** The fix was the CRITERION (`:active` is only visible while the mouse is down, and every sample was taken after
the up), not the button.

## Measuring a surface from the DEVICE's browser

**THIS BOX HAS NO BROWSER OUT OF THE BOX — BUT IT CAN HAVE ONE WITHOUT ROOT**: the measured recipe is in
`agent/resources/panel-react/scripts/local-browser.mjs`, and it runs the real panel sweep HERE, against what this
checkout builds. **THE DEVICE IS STILL HOW THE LIVE SURFACES ARE SEEN** — it reaches the public hosts from
`browser_run_script`, so `page.evaluate` returns computed styles, box geometry and text.

**THREE WALLS, AND NONE OF THEM IS THE SURFACE:**

  * **`waitUntil: 'networkidle'` NEVER FIRES ON THE PANEL.** It holds an SSE connection open by design, so the network is
    never idle and `page.goto` times out after 45s. Use `domcontentloaded` for anything that talks to the agent.
  * **DO NOT DRIVE WINDOWS PATHS THROUGH `terminal_execute`.** Attempts come back mangled — `Get-ChildItem \etc` with the
    variable eaten, `dir "C:\Program Files (x86)\…"` answering *"文件名、目录名或卷标语法不正确"* — because the shell that
    receives the command is not the one the quoting was written for. **`browser_run_script` runs Node ON the device with
    the bundled runtime: read files with `fs`.** That is how the panel's own config and token are reachable.
  * **AND A DESIGN READING IS NOT A LAYOUT READING.** The console's login page was called "a 103px word in a lot of empty
    space" from a partial dump; the full measurement shows a logo and wordmark, a tagline, a vertically centred block and
    a form card centred with equal 162px margins. **It is well composed and needs no change** — the first conclusion was
    drawn from one element.

## Two languages, and which one goes where

**THE REPOSITORY IS WRITTEN IN ENGLISH**: commit messages, code comments, `AGENTS.md`, `CONTEXT.md`, the specs and plans.
That is a standing instruction from the operator and it is what every artifact here already does.

**TALKING TO THE OPERATOR IS IN THE OPERATOR'S LANGUAGE — Chinese.** These are different things, and the instruction was
read literally once as *"session language is English"*, which produced English replies to a Chinese question.

**PREFER RUST. A CHANGE TO A `.mjs`/`.cjs`/`.js` FILE NEEDS A REASON, NOT A HABIT** — the operator said it twice in one
session (*"我不太喜欢js，你一直改js"*), and the loop's own record agrees: rounds spent retrying a blocked device
measurement and editing sweep scripts were drift, not work.

**AND THE BOUNDARY IS NOT A MATTER OF OPINION ANY MORE — IT IS A GATE.** `agent/tests/js_boundary_inventory.rs`
classifies every tracked JS/TS file as **LOGIC** (a decision; must become Rust, counted and capped), **BOUNDARY** (a
platform call that decides nothing), **RENDERING** (places what it was given) or **GENERATED** (a build step produces it,
and the producer is checked to exist). The manifest is `agent/tests/fixtures/js-boundaries.txt` — per directory, with
named file exceptions and a written reason each — and **it is the answer to "may I write this in JS"**. Check it rather
than arguing from memory: the table this section used to carry had gone stale in three rows at once (the Workers' TS is
being ported to wasm, the CLI's decisions are Rust now, and the sweeps' driver was "the operator's call" and has been
called). Two rules in that file settle the cases that look like judgement calls: **a test belongs to the code it tests**,
and **a fixture is not a decision**.

## The vocabulary

**`CONTEXT.md` at the root is this project's GLOSSARY** — the words that mean something specific here (device, session, run,
goal, liveness, mark) and the words that do not, each with an `_Avoid_` list. Read it before naming anything in the panel,
the console or a tool description; two surfaces disagreeing about a word is how `Trajectory` and `Path` came to look like
two names for one screen. It holds TERMS ONLY — an invariant belongs in the gate that enforces it.

## Every gate CI runs

**A GATE NOBODY CAN NAME IS A GATE NOBODY RUNS BY HAND.** These are the gate scripts the workflow invokes. A gate is named where a person can find it, or it does not exist.

**AND THE RUST GATES RUN IN `cargo test`.** `agent/tests/*.rs` holds every gate that was migrated out of `scripts/test/`: no runner of its own — `cargo test -p summrise-agent`, already a CI job, is what runs them. One by hand: `cd agent && cargo test -p summrise-agent <name>`.

**FIVE OF THEM SPAWN A TOOLCHAIN, SO `cargo test -p summrise-agent` NOW NEEDS NODE — AND TWO `node_modules`.** `console_assets` and `console_smoke` build and render the console out of `gateway/ui`; `panel_sheet_freshness` rebuilds `agent/resources/panel-react` (an absent `node_modules` there declares `n/a` — round 191's rule — and the `agent` job installs it so CI measures rather than skips); `press_anchor` runs `node <sweep> --emit` and builds its own `summrise-sweep-plan` into a DEDICATED target directory, because `--emit` reaches the plan through `cargo run` and a nested cargo blocks on the build lock the outer `cargo test` holds (measured: 45s and still waiting; 32.9s with its own target dir). **AND `console_assets` REFUSES A DIRTY TREE**, so `cargo test -p summrise-agent` is red on a tree with uncommitted edits — run the one gate by name while editing, which is also why the worktree recipe above says a symlinked `node_modules` is EXCLUDED rather than staged.

**Each carries its own mutation proof in its header.** Read it when you change the gate.

`all-gates.bash`, `build-pins.bash`
`hook-finds-its-repo.bash`
`main-only-by-merge.bash`, `main-shape-shallow.bash`, `rust-byte-checks.bash`
`panel-design-sweep.bash`
`npm-test-floored.mjs`
`publish-release.bash`, `release-audit.bash`, `release-lib.bash`, `scan-dups-check.py`, `script-syntax.bash`
`smoke-helpers.bash`, `smoke-index.bash`
`sweep-judges.bash`

**AND FIVE MORE LEFT ON 2026-10-09 (landing 3), ALL BUT ONE OF `scripts/test/`'s JAVASCRIPT** —
`console-assets-check`, `console-smoke-check`, `panel-sheet-freshness-check` and `press-anchor-check` are
`agent/tests/console_assets.rs`, `console_smoke.rs`, `panel_sheet_freshness.rs` and `press_anchor.rs` now, and
`contrast-probe-check` is `contrast_probe.rs` (the rules) plus `contrast_probe_emitted.rs` (the artifact),
each ported with the Node gate still beside it and deleted only after the two agreed on one tree — the
differential is in the commit that landed each port. **`npm-test-floored.mjs` STAYS**, and the reason is the
rule rather than the count: only its DECISION moved (`agent/tests/npm_test_floor.rs`), and the `npm test`
spawn it runs is still JavaScript. `token-contract-check.mjs` moved 2026-09-30, and it is
`agent/tests/token_contract.rs` now.

## Where the long form lives

**THERE IS NO LEDGER.** The operator retired the process — one `##` section per round, what was measured and what it cost,
plus three sibling archives, 865 KB in all — because the loop's output had drifted into prose ABOUT the work rather than
the work. The measurement that settled it: **18 of the last 30 commits touched the ledger and nothing else.**

| where a thing goes now | what belongs there |
|---|---|
| **the commit message** | the measurement, the before/after, and the `VERIFIED:` line. The commit bodies here already carry them, and they are the record |
| **`CONTEXT.md`** | a WORD and what it means. Terms only |
| **the gate itself** | any invariant that can be checked — with its own mutation proof in its header. If a sentence can be enforced, enforce it instead of writing it down |
| **the spec or plan under `docs/superpowers/`** | a DESIGN and the decisions in it — including the ones that were wrong and were corrected, so the next reader does not re-derive them |

**`agent/tests/docs_budget.rs` HOLDS ALL OF THIS UP.** It enforces this file's 48,000-byte ceiling, refuses a narrative
`###` section growing back into it, and caps `CONTEXT.md` at 12,000 ("a glossary that grows into a rulebook has stopped
being a glossary"). It runs in the `agent` job via `cargo test -p summrise-agent`.

**THE CEILING IS A SIZE, NOT A RELEVANCE CHECK — so the file has to be pruned by hand.** A new rule costs bytes, and the
cheap move at the moment of an incident is to append; nothing ever removes. When this file needs room, prune it by its own
rule: **if a sentence does not change what you would DO, it does not belong here** — the incident it came from is already
in the commit history, which is where the record lives.

## Committing

**A pre-commit hook runs the emitters** (`scripts/hooks/pre-commit`). It used to guard the
backtick-in-a-template-literal accident, and **that class is gone**: all five emitters hand their payload to
`agent/scripts/lib/sweep-bundle.mjs`, which resolves the payload's own requires and COMPILES what it returns. What the
hook still buys is the only end-to-end assembly of all five artifacts in under a second — a payload module that does not
parse, a require the assembler cannot resolve, or an emitter that was renamed or deleted fails at the commit instead of
in the design job.

**IT IS INSTALLED, AND THE INSTALL IS NOT THE OBVIOUS ONE.** `.git/hooks/pre-commit` will NOT run on this machine:
`core.hooksPath` is set globally in `~/.gitconfig`, and git ignores the per-repo directory entirely when that is set.
What is installed is a repo-local `.githooks/` holding BOTH links plus a repo-local path pointing at it — explicit and
self-contained, and it does not depend on a file in the operator's home directory staying where it is:

    mkdir -p .githooks
    ln -sf ../scripts/hooks/pre-commit .githooks/pre-commit
    ln -sf ~/.config/git/hooks/post-commit .githooks/post-commit
    git config --local core.hooksPath .githooks

`.githooks/` is machine-local (one link points outside the repo), so it is gitignored; **the COMMAND is the artifact.**
Do NOT use `git config core.hooksPath scripts/hooks`: it shadows the global directory, which is not empty — it holds the
operator's `post-commit` (tokensave auto-sync), so this repo would silently stop syncing. (A symlink at
`~/.config/git/hooks/pre-commit` would also be safe: the hook's first act is `if [ ! -f
agent/scripts/panel-design-sweep.mjs ]; then exit 0; fi`, so it cannot block another repository's commits.)

**THE ONLY PROOF OF A CHECK IS WATCHING IT REFUSE SOMETHING.** A commit that SUCCEEDS looks exactly like a hook that ran
and passed: the hook once resolved its own repository from `$0` — which git sets to `.githooks/pre-commit`, one level
ABOVE this repository — so the `cd` succeeded in the wrong place, the guard found no emitter, and it exited 0 for every
commit. `scripts/test/hook-finds-its-repo.bash` invokes the hook the way git does and fails if it takes the inert path.
**AND PROVE THE MUTATION, NOT THE HOOK**: a trap only counts when it is inside the thing it traps — a planted backtick
in a function BODY, outside the template literal, exits 0 and proves nothing.

**AND DO NOT PUT A COMMAND WHOSE STATUS YOU NEED ON THE LEFT OF A PIPE.** "A pipeline exits with its LAST command's
status" is ordinary shell knowledge, and it has cost two pushes here: a hook run as `bash … >/dev/null 2>&1` threw its
exit code away, and `npm test 2>&1 | grep … | head -2` let a failing suite pass because `set -e` saw `head` succeed. READ
THE EXIT CODE, NOT THE OUTPUT was not disobeyed in either case — it was preserved and destroyed one step later:

```bash
npm test >/tmp/out 2>&1 || { echo FAILED; exit 1; }     # status kept
grep -E 'pass|fail' /tmp/out                            # output read afterwards
```

**AND THE LOOP FORM THAT LOOKS RIGHT IS THE ONE THAT FAILS — `cmd && echo ok || echo FAIL` THROWS THE STATUS AWAY.** The
`||` branch runs when the command fails, so the LINE SUCCEEDS either way and `set -e` never fires: a suite can be red,
print `FAIL`, and let the commit through. **Keep the status by making the failure EXIT, not by printing:**

```bash
if timeout 300 node "$g" >/dev/null 2>&1; then echo "ok   $g"; else echo "FAIL $g"; exit 1; fi
```

**AND WHEN THE TEXT YOU ARE WRITING IS FULL OF BACKTICKS, PUT IT THROUGH A QUOTED HEREDOC — NOT `python3 -c "…"`.**
`python3 -c "…"` is DOUBLE-quoted, so every backtick in the text is executed as COMMAND SUBSTITUTION before Python ever
sees it: a commit message naming `docs/agents/inventory.md` ran that path as a command, and the text was committed with
the substitution's output. `stderr` said so and the commit went out anyway. **BACKTICKS INSIDE DOUBLE QUOTES ARE NOT
LITERAL**, and this repository's prose is nothing but backticks, so the hazard is permanent. **AND THE DELIMITER IS A
HAZARD TOO**: a delimiter the body QUOTES as an example ends the heredoc in the middle of the text. Choose a string the
body cannot contain, and quote it:

    python3 - <<'AGENTS_R141_EOF'      # quoted, and a string the body cannot contain
    ...
    AGENTS_R141_EOF

**The quoting is on the DELIMITER, not the content**, and the NAME matters as much as the quoting.

**AND WHEN YOU COUNT SOMETHING, COUNT IT WITH THE TOOL THAT PRODUCES IT — THREE CONVENIENT COMMANDS ARE WRONG HERE**,
all in the same direction: the one that makes finished work look unfinished.

| do NOT count with | because | use |
|---|---|---|
| `grep -l <name> <dir>` | it counts FILES THAT MENTION a thing, not the thing (it said three gates read `panel.css`; five do) | the tool's own message, or `grep -c` of the count it prints |
| `grep -c '^| ' <table>` | it counts the header row and the separator too (a 44-row table read 46) | `awk '/^\| /{n++} END{print n-2}' <table>` |
| `git tag` | **this clone has almost no tags** — tags are made through the GitHub API and the mirror refuses `git fetch --tags` (HTTP 400), so `git tag` answers ZERO for every release made here | `GET /repos/SilasVale/summrise/git/refs/tags` |

**A summary is a claim set.** If a number is going into a commit message or a report, the command that produced it
belongs beside it — and a command that merely CORRELATES with the number is not that command.

## Release — npm is the only channel

```bash
# 1. bump agent/summrise-agent-npm/package.json "version" to 1.2.N, then:
touch agent/src/lib.rs && ./scripts/build.sh agent
cp agent/target/x86_64-pc-windows-msvc/release/summrise-{agent,launch}.exe agent/summrise-agent-npm/
mkdir -p agent/summrise-agent-npm/bin
cp agent/target/x86_64-pc-windows-msvc/release/summrise-cli.exe agent/summrise-agent-npm/bin/summrise.exe
#    THREE EXES, AND THE THIRD IS THE CLI. `bin/summrise.exe` is the npm `bin` (landing 4b) — the
#    command a device runs — and its version is COMPILED IN from package.json, so the bump above is
#    only in the binary if the build came after it. publish-release.sh dates the staged copy against
#    that manifest and refuses one older than it, which is the deadlock guard: a CLI older than the
#    release it manages tells the operator to install something npm cannot deliver.
#    The launcher is what SummriseDesktop and SummrisePlaywright run instead of the retired
#    .vbs wrappers, and it is COPIED on the device, never built there — a release without it ships tasks
#    that cannot start. publish-release.sh refuses a missing or stale staged copy and required-in-tgz.txt
#    refuses a tarball without any of the three, both BEFORE the upload.
#    ./scripts/publish-release.sh 1.2.N --dry-run — every gate, no credentials, changes nothing, seconds.
# 2. publish to BOTH channels (pack + manifest + prune + deploy + smoke; it does NOT commit):
./scripts/publish-release.sh 1.2.N --npm      # needs $NPM_TOKEN or ~/.npm-token
# 3. the release commit goes on a BRANCH and reaches main by merge, like everything else:
git checkout -b release/1.2.N
git add agent/summrise-agent-npm/package.json index/public/summrise-agent/version.json
git commit -m "release: 1.2.N"
git checkout main && git merge --no-ff release/1.2.N && git push origin main
# 4. WAIT for that commit's CI to go GREEN, then tag it through the API (pushing tags times out here):
curl -sX POST -H "Authorization: Bearer $(cat ~/.github-token)" \
  https://api.github.com/repos/SilasVale/summrise/git/refs \
  -d "{\"ref\":\"refs/tags/v1.2.N\",\"sha\":\"$(git rev-parse HEAD)\"}"
# 5. audit CDN vs the GitHub asset, byte for byte (it needs the asset, which is why step 4 comes first):
./scripts/publish-release.sh --audit-only 1.2.N
```

**THE TAG'S RULES, EACH ONE PAID FOR.** The SHA must be a PUSHED commit whose CI is GREEN — a local commit answers
"Object does not exist", and a superseded one fails `release.yml`'s own gate. The tag must NOT MOVE onto different
content: a move demotes the release to a DRAFT (invisible to the audit's `GET /releases/tags/<tag>`) and makes CI package
a different artifact under the same version number. **Do not push anything while a release commit's CI is running** — a
push cancels it, and the tag then has no green CI to point at. If CI must go green again for an already-published
version, put an EMPTY commit on the release commit: the tree is unchanged, so the asset matches what shipped.

**AND `ci.yml` TRIGGERS ON `main` ONLY, WHICH MAKES THE EMPTY COMMIT A TWO-STEP.** `release.yml`'s gate is FAIL-CLOSED
on silence ("zero check-runs + zero statuses means CI has not reported yet, or never will — WAIT, never pass"), so a
commit no workflow has ever seen can never be tagged, however green the tree is. The empty commit must be PUSHED first —
a dispatch names a ref the runner has to fetch — and then dispatched:

    gh api repos/$REPO/actions/workflows/ci.yml/dispatches -f ref=<branch>      # HTTP 204

**PUBLISH TO `latest`, NOT `alpha`.** The CDN's `-latest.tgz` alias moves on every release, so npm's `latest` must move
with it; a CLI older than the release it manages deadlocks the device, because the CLI's own guard tells the operator to
install a version `npm i -g` then cannot deliver. `--npm-tag alpha` remains for a deliberate prerelease channel.

**ON THE DEVICE (PowerShell) — AND THE `--prefix` MATTERS.** Without it npm installs elsewhere, reports success, and
`summrise update` ships the old exe:

```powershell
npm i -g --prefix (Split-Path (Get-Command summrise).Source) https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz
summrise update
summrise status
```

**THAT SUBSTITUTION FAILS SILENTLY ANYWHERE BUT POWERSHELL.** Run from a `cmd`-style shell — which is what a device
tool's terminal often is — `(Split-Path …)` is not evaluated: npm reads it as a PACKAGE NAME and fails with
`E404 … '(Get-Command@*'`. The obvious repair is worse: the `install dir:` that `summrise status` prints is NOT the npm
prefix, which is the directory the `summrise` command resolves from, so npm installs into the WRONG place and still
reports `added 1 package`. The form that needs no quoting, because the 8.3 short name has no spaces:

    (Get-Command summrise.cmd).Source                # -> D:\Program Files\nodejs\summrise.cmd
    #   NOT `where summrise` — it prints NOTHING in the agent-hosted PTY, which runs as SYSTEM with a cwd
    #   of C:\Windows\System32\config\systemprofile and a PATH holding no npm prefix. summrise IS installed.
    npm i -g --prefix D:\PROGRA~1\nodejs https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz
    summrise status   # `this CLI:` is the ONLY proof; npm prints "changed 1 package" either way

**A BARE `npm i -g summrise-agent` CAN INSTALL NOTHING WHILE REPORTING SUCCESS.** A stale `latest` resolved from npm's
cache or this box's mirror printed `changed 1 package` and left the OLD version in place. The URL form above has no
resolution step and is immune; an EXACT version is what `setup` uses for the same reason. Verify with `summrise status`,
never npm's exit code.

**`summrise update` ANSWERS 502 WHILE IT SWAPS THE EXE — that is the swap, not a failure**, and the dark window runs from
minutes to HOURS: measured, a tunnel answering 502 then 530/1033 for ~2 hours while `startup.log` showed the agent
starting normally with its new release marker. The retries total ~25 seconds, so the window is the tunnel with no
connector. **Read `startup.log` before calling a device broken.**

**THREE THINGS BITE A FIRST-TIME WINDOWS INSTALL:**

  1. **PowerShell REFUSES `summrise`** — it prefers npm's `.ps1` shim, which the default `Restricted` policy blocks
     (*"在此系统上禁止运行脚本"*). **Use `summrise.cmd <command>`**, which bypasses the policy and changes nothing on the
     machine (or `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`, once).
  2. **`setup` NEEDS AN ELEVATED SHELL AND SAYS SO BADLY** — the install dir defaults to `C:\Program Files\Summrise` and
     the registry key is `HKLM`, so a normal user's `setup` dies with `EPERM: … mkdir 'C:\Program Files\Summrise\scripts'`,
     a stack trace where the useful sentence is "run it as administrator".
  3. **THE FIRST `setup` CAN STALL at "reconciling desktop shortcut (retired-exe repair)"** — that step instantiates
     `WScript.Shell` over COM. It is cosmetic and `setup` is idempotent: **re-run it** (or Ctrl+C, which costs nothing).

**AND THE WHOLE INSTALL IS TWO COMMANDS** (the second one starts the agent):

    npm i -g summrise-agent@1.2.N        # exact version: `latest` can resolve stale and still report success
    summrise.cmd setup                   # components + registry + tasks + the agent

**THE npm PACKAGE CARRIES NO BOXED COMPONENTS — `setup` FETCHES AND VERIFIES THEM.** The package is ~6.7 MB: the exe,
the CLI, the desktop shell's *sources*. `cloudflared.exe` (54 MB), `summrise-playwright.zip` (31 MB) and the **electron
runtime** are served by the release host and staged into `<install>\components` by `resolveComponent()` (`curl -fsSL`, so
an HTTP error is a FAILURE and not a 404 page on disk). Both cloudflared and electron are STAGED IN R2, which is what
lets `index/components.json` pin a sha256 for each and `version.json` publish it: setup REFUSES a mismatch, warns when a
release carries no pin, and never touches GitHub — a device behind the GFW needs no mirror. An *upgrade* was never
affected (components live in `<install>\components` and survive); this bites at install and migration time. The migration
that came up local-only, and why it cost an hour, is in `docs/BRAND.md`.

**MEASURE THE PANEL THE DEVICE IS ACTUALLY RUNNING, not only the harness.** Every design sweep renders the HARNESS (a
stubbed device, this checkout's bundle). `agent/scripts/live-panel-probe.mjs` measures the live panel
(`127.0.0.1:18080`), which is the only way to see a rule no harness run can. It needs a browser and a running panel, so
it cannot be a CI job: run it on the device after a `summrise update`. A 38 KB script is EMITTED, never pasted —

    node agent/scripts/live-panel-probe.mjs --emit > index/public/summrise-agent/live-panel-probe.js
    ./scripts/build.sh index          # A COMMIT IS NOT A DEPLOY: a commit under index/public/ does not change
                                      # what the CDN serves, and the device fetches the PREVIOUS copy
    # the device then fetches it (system_file_download from
    # https://agent.saisi.online/summrise-agent/live-panel-probe.js) and runs it with browser_run_script.
    require('D:/Summrise/live-panel-probe.js');   # REQUIRING IT IS RUNNING IT: the emitted program reads the
                                                  # panel token from the device's config, walks both densities,
                                                  # prints JSON and exits with a verdict code. Nothing is passed in.

After ANY change to a served file, one command says whether the CDN has it:

    curl -s -o /tmp/cdn.js -w '%{size_download}\n' https://agent.saisi.online/summrise-agent/live-panel-probe.js
    cmp -s /tmp/cdn.js index/public/summrise-agent/live-panel-probe.js && echo served || echo STALE

**AND DO NOT `page.goto` THE PANEL FIRST**: the attached view is ONE page shared with the operator's screen, usually
already on the panel, so navigating it aborts (`net::ERR_ABORTED`); the program does its own navigation. **`ATTACHED=false`
IS NOT A SWITCH** — the helper attaches whenever the desktop CDP (`127.0.0.1:9333`) answers, so force headless by
pointing `SUMMRISE_CDP_ENDPOINT` at a dead port, and install `chrome-headless-shell` first
(`node node_modules/playwright-core/cli.js install chromium-headless-shell` — the boxed `playwright-core` does not ship
it and has no `npx playwright`).

**AND THE EMITTED FILE IN `index/public/` MUST NOT BE GITIGNORED: an ignored file is in no other checkout.** Workers
Assets uploads the directory WHOLESALE, `.gitignore` unread, so a worktree deploy REMOVES the missing files from the CDN
(`summrise-agent-latest.tgz` and `SummriseAgent-Setup.exe`: 404). **Deploy `index` from the main checkout**, or copy them
in; the post-deploy smoke catches it.

**TWO THINGS THAT COST A DEVICE RESTART WHEN IGNORED: never launch a second `summrise-agent.exe` from an agent-hosted
PTY** (it inherits the kill-on-close job and kills the running agent), and **never kill/copy the exe inline over a PTY** —
use the npm flow above.

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
agent/summrise-agent-npm/    the npm package + the `summrise` CLI (bin/summrise.exe, a Rust binary)
agent/resources/panel-react/  the panel SPA (React + vitest); resources/panel/ is its build output
```

Features gate behind `terminal`/`keyring` with identical public paths across configs. A new MCP
tool is defined in its plugin's `tools.rs` (the registry caches at register time) and, to be
callable from the console, must be registered in `gateway/src/mcp-tools.ts` **and** matched by
`isDeviceDirectTool()`; after adding or removing a tool run
`SUMMRISE_REFRESH_SPEC=1 cargo test --features terminal,keyring spec_snapshot`.

**MUTATION THAT MUST BREAK IT** (moved here from the ledger table, landing 4b): *add a parameter inside a device tool's
`properties`* → **exit 101, snapshot diff.** A snapshot that does not notice a new parameter is a snapshot that is not
pinning the surface.
