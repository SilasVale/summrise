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

**AND THE PER-DIRECTORY TABLE THAT STOOD HERE IS GONE ON PURPOSE**: it said in its own last row to *"read them out of
`ci.yml` rather than from here"*, because a copy of that list goes stale (it named "59 cases" for a suite that had grown
to 68). **`ci.yml` is the list.** What does not go stale is the rule it was there to carry:

**RUN THE COMMAND THE OTHER END RUNS.** A local check that is not CI's check is not the same check — `gateway/ui` passed
a local `tsc --noEmit` carrying six type errors, because the `ui` job runs `npm run build` instead.

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

**AND AFTER A DELETION THERE ARE FOUR INSTRUMENTS, EACH BLIND TO A DIFFERENT THING — MEASURED 2026-10-10, WHEN ONE
LANDING DELETED A PLUGIN FAMILY AND FIVE `read()` SITES WENT RED IN FOUR DIFFERENT TOOLS:**

| instrument | sees | blind to |
|---|---|---|
| the **import graph** | a module importing what you deleted | a ROUTE it registered; a test importing nothing you deleted |
| the **compiler** | what it typechecks | every `.mjs`, and every runtime-only path |
| the **route table** | a route whose only implementation you deleted | a test; a string a GENERATED file carries |
| **the suite** | **a test whose subject was deleted, because it RUNS it** | nothing static — but only what it is pointed at |

**Three can be green while the fourth is red, and that is not a gap in any of them**: one landing passed the import
graph, the compiler and the route table, and `npm test` failed 108 times — those tests imported nothing removed, they
drove the ported surface through the rollback configuration. **So run the suite, and read the route table.**

**AND WHEN YOU FIX SOMETHING, GREP FOR THE TEXT YOU JUST FIXED** — a fix applied at one of two call sites is invisible
to the compiler, the suite and the person who applied it. Two of the four CI-red commits on 2026-10-10 were that shape,
**both found by a grep rather than by a gate.**

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

**AND CHECK `uptime` BEFORE THEORISING ABOUT WHAT DELETED A FILE.** A `/tmp` sweeper was blamed once; `who -b` said
otherwise, and the measurement is in the commit that corrected it.

Symlink the dependency trees a worktree needs (`agent/resources/panel-react/node_modules`, `gateway/node_modules`, …).
**`.gitignore`'s `node_modules/` matches a DIRECTORY, not a symlink**, so `git add -A` there stages the LINK — and checks
like `agent/tests/console_assets.rs` refuse a dirty tree, which is how it surfaces. The fix is a clone-local `.git/info/exclude`
line for the bare name `node_modules`.

**AND START LONG WORK IN THE BACKGROUND — a CI run, an `all-gates` run, a build, a subagent — then do the next thing and
collect it when it finishes.** Polling a job in the foreground turns three tracks back into one. `main` still merges one
branch at a time (the ref is shared), but a merge is seconds, and the other tracks keep working through it.

**AND A READ-ONLY COMMAND CAN STALL THE WHOLE BOX — TWO OF THEM DID, ON 2026-10-09.** `du -sh` over four home directories ran
**1h34m** and `grep -rn underAA --include=* .` ran **45 minutes**, holding load at ~23 while every cargo gate in every
worktree crawled. **Check `ps` for long-running read-only commands before blaming the machine, and prefer the
`grep` TOOL over shell `grep -r`** — the repo-wide form with `--include=*` is

**AND A LONG GATE RUN HAS ONE SHAPE THAT WORKS HERE: run it in the FOREGROUND with a long timeout and let the
tool move it to a managed job when the timeout expires.** Three other shapes failed the same afternoon — `&` (the
child died silently), `| tail -40` (the job lived but its output was swallowed), and a second managed job whose
process had already gone. **No `&`, no pipe, and redirect to a FILE if the result must survive the job.**
**AND A MEASUREMENT BELONGS TO THE TREE IT WAS TAKEN ON — NAME THE BASE, OR IT WILL BE USED ON ANOTHER ONE.** On
2026-10-09 that mistake was made **three times in one session**, by a coordinator briefing agents and by an agent
reading a coordinator's brief:

* a brief said "branch from `main`" and quoted a cap of **94** and a fixture of **399 keys** — both measured on
  the integration branch, while `main` was at **107** and **330**. Following it would have deleted the console's
  TypeScript rollback on a tree where the Rust worker refuses two of those routes **by construction**, so the deletion
  would have succeeded and the routes would have died. **The agent stopped before the first `git rm` and sent the
  table instead** — which is the behaviour this rule asks for;
* the same brief pointed at a COMMIT (`main@64686ae8`) where it meant the REF, so "the CORS fix is not on `main`"
  was right about the commit and wrong about the branch;
* a rule deleted from the manifest because it was dead on the integration branch was deleted on `main` instead,
  where two files still matched it. **The inventory gate caught that one immediately.**

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

**AND DO NOT WEAKEN A CRITERION TO MATCH A MESSAGE.** The mirror image, measured once: a sweep reported *"acknowledged
the press after 823ms — this feedback waited on the 824ms network round trip"* for a button whose handler is synchronous
and whose popover has no network on it at all. **The judge was speculating about a cause its own row cannot check.** The
fix was the CRITERION (`:active` is visible only while the mouse is down, and every sample was taken after the up), not
the button.

## Measuring a surface from the DEVICE's browser

**THIS BOX HAS NO BROWSER OUT OF THE BOX — BUT IT CAN HAVE ONE WITHOUT ROOT**: the measured recipe is in
`agent/resources/panel-react/scripts/local-browser.mjs`, and it runs the real panel sweep HERE, against what this
checkout builds. **THE DEVICE IS STILL HOW THE LIVE SURFACES ARE SEEN** — it reaches the public hosts from
`browser_run_script`, so `page.evaluate` returns computed styles, box geometry and text.

**THREE WALLS, AND NONE OF THEM IS THE SURFACE:**

  * **`waitUntil: 'networkidle'` NEVER FIRES ON THE PANEL.** It holds an SSE connection open by design, so
    `page.goto` times out after 45s. Use `domcontentloaded` for anything that talks to the agent.
  * **DO NOT DRIVE WINDOWS PATHS THROUGH `terminal_execute`.** Attempts come back mangled — `Get-ChildItem \etc` with the
    variable eaten, `dir "C:\Program Files (x86)\…"` answering *"文件名、目录名或卷标语法不正确"* — because the shell that
    receives the command is not the one the quoting was written for. **`browser_run_script` runs Node ON the device:
    read files with `fs`.** That is how the panel's own config and token are reachable.
  * **AND A DESIGN READING IS NOT A LAYOUT READING.** The console's login page was called "a 103px word in a lot of empty
    space" from a partial dump; the full measurement shows a composed page with equal 162px margins. **The first
    conclusion was drawn from one element.**

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

**SIX OF THEM SPAWN A TOOLCHAIN, SO `cargo test -p summrise-agent` NEEDS NODE — AND THREE `node_modules`.** `console_assets`
and `console_smoke` build the console out of `gateway/ui`; `panel_sheet_freshness` rebuilds `agent/resources/panel-react`
(an absent `node_modules` there declares `n/a` — round 191's rule — and the `agent` job installs it so CI measures rather
than skips); `press_anchor` runs `node <sweep> --emit` and builds its own `summrise-sweep-plan` into a DEDICATED target
directory, because `--emit` reaches the plan through `cargo run` and a nested cargo blocks on the build lock the outer
`cargo test` holds (32.9s with its own target dir, 45s and waiting without). The sixth is `npm_test_floor`, which runs
`npm test` in every package whose runner is `node --test` — `gateway`'s among them, which is why the `agent` job installs
`gateway/node_modules` (**measured: 92 packages, 24s; 602 tests, 34s; the job is 6.4 min**). **AND `console_assets`
REFUSES A DIRTY TREE**, so `cargo test -p summrise-agent` is red on a tree with uncommitted edits — run the one gate by
name while editing, which is why a symlinked `node_modules` is EXCLUDED rather than staged.

**Each carries its own mutation proof in its header.** Read it when you change the gate.

`all-gates.bash`, `build-pins.bash`
`hook-finds-its-repo.bash`
`main-only-by-merge.bash`, `main-shape-shallow.bash`, `rust-byte-checks.bash`
`panel-design-sweep.bash`
`publish-release.bash`, `release-audit.bash`, `release-lib.bash`, `scan-dups-check.py`, `script-syntax.bash`
`smoke-helpers.bash`, `smoke-index.bash`
`sweep-judges.bash`

**AND `scripts/test/` HOLDS NO LOGIC AT ALL, WHICH IS WHAT LANDING 3 FINISHED.** The six gates it held are
`agent/tests/`: `console_assets.rs` · `console_smoke.rs` · `panel_sheet_freshness.rs` · `press_anchor.rs` ·
`contrast_probe.rs` (+ `contrast_probe_emitted.rs` for the artifact) · `npm_test_floor.rs` · `token_contract.rs`.
**One BOUNDARY file remains there — `scripts/test/lib/emitted-pieces.mjs`, with its own rule and reason** — and
criterion ① is about LOGIC, not about a file extension. Each port landed with the Node gate still beside it and the
Node gate was deleted only after the two agreed on one tree; the differential is in the commit that landed each.


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

**AND THE LOOP FORM THAT LOOKS RIGHT IS THE ONE THAT FAILS — `cmd && echo ok || echo FAIL` THROWS THE STATUS AWAY.**
The `||` branch runs when the command fails, so the LINE SUCCEEDS either way and `set -e` never fires. **Keep the status
by making the failure EXIT, not by printing:**

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

**AND A COUNT HAS TWO EDGES: IT CATCHES A REGRESSION AND IT HIDES A DELETION.** Measured 2026-10-10, both ways in one
afternoon: a suite went **81 -> 48 failures** while a name match deleted the one test whose subject had no other witness,
and **nothing could see it — a deletion is not a failure, so the number gets BETTER.** Hours later the same instrument
caught three of my own edits, each by moving it the wrong way. **Read the failures, not the count, and when a number
improves ask what was removed.** And match a test's name in the SOURCE (`test("name", …)`), never as a reporter RENDERS
it: `✖ name (12.3ms)` is a rendering, and `->`, `/` or a wrapped body will not match the literal.

**AND WHEN YOU COUNT SOMETHING, COUNT IT WITH THE TOOL THAT PRODUCES IT — THREE CONVENIENT COMMANDS ARE WRONG HERE**,
all in the same direction: the one that makes finished work look unfinished.

| do NOT count with | because | use |
|---|---|---|
| `grep -l <name> <dir>` | it counts FILES THAT MENTION a thing, not the thing (it said three gates read `panel.css`; five do) | the tool's own message, or `grep -c` of the count it prints |
| `grep -c '^| ' <table>` | it counts the header row and the separator too (a 44-row table read 46) | `awk '/^\| /{n++} END{print n-2}' <table>` |
| `git tag` | **this clone has almost no tags** — tags are made through the GitHub API and the mirror refuses `git fetch --tags` (HTTP 400), so `git tag` answers ZERO for every release made here | `GET /repos/SilasVale/summrise/git/refs/tags` |

**A summary is a claim set.** If a number is going into a commit message or a report, the command that produced it
belongs beside it — and a command that merely CORRELATES with the number is not that command.

## Release

**THE RUNBOOK IS `docs/RELEASE.md` — read it when you release.** It was 11 KB of this file (23%), and a procedure that
is needed once a month does not belong in a document loaded every session. `./scripts/publish-release.sh N --dry-run`
runs every gate without credentials and is the first thing to run.

**THREE RULES THAT DO HOLD AT ALL TIMES**, because they are the ones that have cost releases:

* **the tag's SHA must be a PUSHED commit whose CI is GREEN**, and the tag must never move onto different content — a
  move demotes the release to a DRAFT and makes CI package a different artifact under the same version number;
* **do not push while a release commit's CI is running** (a push cancels it, and the tag then has no green run to point
  at); if CI must go green again for a published version, put an EMPTY commit on it, because `ci.yml` triggers on `main`
  only and `release.yml` is fail-closed on silence;
* **publish to `latest`, not `alpha`**: the CDN's `-latest.tgz` alias moves on every release, and a CLI older than the
  release it manages deadlocks the device.

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
