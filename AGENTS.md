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

**AND RUN THE COMMAND THE OTHER END RUNS.** A local check that is not CI's check is not the same check —
`gateway/ui` passed a local `tsc --noEmit` carrying six type errors, because the `ui` job runs `npm run build`
instead. The commands, by working directory:

| where | CI runs | and NOT |
|---|---|---|
| `gateway/` | `npm run typecheck` (= `tsc --noEmit`) · `npm test` · `npm run lint` (= `eslint src/`) · `npm run format:check` | — |
| `gateway/ui/` | `npm run build` (= `tsc -b && vite build && prune-stale-assets`) · `npm test` | **not** `tsc --noEmit`, which is the check that missed them |
| `agent/resources/panel-react/` | `npm run build` · `npm test` | — |
| `agent/summrise-agent-npm/` | `npm test` (= `node --test`, 59 cases) — **no install step**: the package has zero dependencies and no lockfile, so `npm ci` fails with `EUSAGE` | — |
| `agent/` | `cargo fmt --all -- --check` · `cargo clippy -p summrise-agent --all-targets -- -D warnings` · `cargo clippy -p summrise-agent --features terminal,keyring --all-targets -- -D warnings` · `cargo clippy -p summrise-agent-core --all-targets -- -D warnings` · `cargo test -p summrise-agent` · `cargo test -p summrise-agent --features terminal,keyring` · `cargo test -p summrise-agent-core` | — |
| `agent/summrise-desktop-electron/` | `npm test` | — |

All five were run by hand on the commit that added this table and all were green; before that, `gateway`'s lint and typecheck
and the agent's `fmt`/`clippy` had not been run by this loop at all, and the panel's `npm test`, not `npx vitest run`, is what
CI invokes.

**AND A CHANGE UNDER `agent/scripts/` IS A CHANGE UNDER `gateway/`.** The console's Source Viewer serves a byte-for-byte
MIRROR of `agent/scripts/*.mjs` and their `lib/` (not `lib/sweep/`), so editing one of those files leaves
`gateway/public/code/files/instruments/` stale and turns `gateway`'s `code viewer: the instruments mirror matches
agent/scripts byte for byte` red — with an assertion that names the fix. Re-sync and commit the mirror:

```bash
bash gateway/scripts/sync-code-viewer.sh && cd gateway && npm test
```

Round 2 of the standing goal pushed a red `main` by running the design-sweep gates for a sweep change and not this one;
the gate was right and the local check was incomplete.


**READ THE EXIT CODE, NOT THE OUTPUT.** The suites do not share a reporter, and grepping for the wrong
one returns NOTHING — which looks exactly like a suite that passed silently:

| command | the line to look for |
|---|---|
| `cargo test` (either config) | `test result: ok. N passed; 0 failed` |
| `npm test` in `gateway/` (Node 24) | `ℹ pass N` — on Node 20 it prints `# pass N` instead |
| `npx vitest run` in `panel-react/` | `Tests  N passed` |

`exit 0` is the answer in every case; the line is a convenience. (Two rounds were once spent reading a silent grep as
"the suite did not run" and re-running it another way. The reporter table is what prevents that.)

**TWO RULES ABOUT WHAT NOTHING RUNS — READ THEM BEFORE YOU TRUST A GREEN OR A RED.**

  * **You changed a terminal backend, a file-relay path, a workflow step, the panel's wiring, the MCP surface, the evidence
    drawer or the browser/playwright door** — then `agent/scripts/e2e/e2e.js` has a section for it, **seven of its nine
    sections run in NO CI job and NO schedule**, and the rule is how to get the script onto a device and run it there.
  * **A CI job is red and you did not expect it to be** — a CANCELLED job (any push while a run is in flight) reports
    `conclusion: failure`, **indistinguishable from a real one in a count**; the rule is the log line that tells them apart.


**`main` ADVANCES ONLY BY MERGE** — four artifacts, and each one covers a way the others cannot:

| artifact | what it is | what it cannot see |
|---|---|---|
| `scripts/hooks/main-only-by-merge` | the rule, called by `scripts/hooks/pre-commit` **after** its "is this Summrise?" guard | a plain `git merge` that FAST-FORWARDS, which creates no commit and so never runs a hook; and `--no-verify` |
| `scripts/test/main-shape-check.mjs` | the CI gate: `main` must be at a commit with **two or more parents** | nothing — but it only runs in CI, after the push |
| `scripts/test/main-only-by-merge.bash` | the proof: installs the rule as a throwaway repo's real pre-commit hook and runs six cases through it | nothing outside that fixture |
| `scripts/test/main-shape-shallow.bash` | the fixture for **the clone CI actually gives**: a real `git clone --depth 1` of a repo whose HEAD is a real merge | the full-clone shape, which is the only one a developer sees |

**AND THE GATE FAILED ITS FIRST CI RUN WHILE PASSING EVERY LOCAL ONE**, which is why the fourth artifact exists.
`actions/checkout@v4` defaults to `fetch-depth: 1`, so the runner is **shallow**; git grafts the boundary commit and
`git rev-list --parents -n 1 HEAD` answers with the SHA alone — **zero parents** — for a commit that has two. The gate used
that and refused a `main` that WAS a merge. It reads the commit object now (`git cat-file -p HEAD`), whose `parent` lines are
there regardless of depth, and `main-shape-shallow.bash` builds the graft that no full clone can reproduce. It is this file's
own rule one level down — **run the command the other end runs** — and the gate was the other end.

Work reaches `main` through a branch that was verified and reviewed, merged with **`--no-ff`** — the flag matters, because
a plain `git merge` fast-forwards when `main` has not moved. **A direct commit on `main` is refused by the hook and by the
gate, and the hook lives in its own file precisely so it can be proven on a real repository** (a rule inside `pre-commit`
sits below a guard that exits 0 in any repo that is not this one, so a fixture could never reach it).

### Which gates have been PROVEN to bite

**EVERY GATE CARRIES ITS OWN PROOF, IN ITS OWN HEADER.** Until landing 4b they were rows in one 110 KB table; now each gate
opens with a `MUTATION:` / `RESULT:` block naming the edit that must break it and what it said when it did. **Read that block
when you change the gate** — a gate that cannot be broken is worse than no gate. Three JSON fixtures cannot hold a comment
(JSON has none), so their proofs are in `agent/tests/fixtures/MUTATIONS.md` beside them.

**AND READ IT WHEN A FINDING SAYS SOMETHING IS UNUSED AND YOU ARE ABOUT TO DELETE IT** — that rule outlived the table it was
written for.

**AND FOUR GATES WERE DOCUMENTED *ONLY* IN THE LEDGER, WHICH IS WHY THEY ARE NAMED HERE NOW.** Deleting it turned
`numbered-claims-check` red with `proxy-cors-parity-check`, `proxy-timeout-parity-check`, `panel-mock-spread-check` and
`custom-prop-check` — gates the census could not find anywhere an operator reads, because a round narrative was their only
mention. That is the ledger's own recorded failure mode arriving from the other side (*"they were reachable ONLY through
it"*), and the fix is the one this section already prescribes: **a gate is named where a person can find it, or it is a gate
nobody can run by hand.**

## Pushing: wait for the run in flight

**A PUSH SUPERSEDES AN IN-FLIGHT CI RUN.** GitHub cancels it, and the cancellation is reported as `conclusion: cancelled` —
which a count cannot tell from a real failure, and which leaves that commit with **no green CI to point at**. The rule has
been here for months and this repository paid for it four times; **three were this loop, twice in three rounds**, which is
what a rule that must be remembered costs.

So it is a mechanism now, in the one place a push cannot skip:

| artifact | what it is |
|---|---|
| `scripts/hooks/pre-push` | the hook. Installed as `.githooks/pre-push`, which `core.hooksPath` already points at |
| `scripts/hooks/ci-not-in-flight` | the check, its own file so it can be proven on fixtures |
| `scripts/test/ci-not-in-flight.bash` | the proof: 7 cases on saved API responses, **and it RUNS the hook the way git does** rather than grepping for the call site |

**IT FAILS OPEN, DELIBERATELY.** No token, no answer, unparseable JSON — all exit 0, because blocking a push over a flaky
mirror is a worse failure than the one it prevents, and this repository's git remote has been measured answering in 0s, 32s
and >90s for the same request. `git push --no-verify` is the escape hatch, and the hook prints it.

## Two languages, and which one goes where

**THE REPOSITORY IS WRITTEN IN ENGLISH**: commit messages, code comments, `AGENTS.md`, `CONTEXT.md`, the specs and plans.
That is a standing instruction from the operator (their inbox, row 7) and it is what every artifact here already does.

**TALKING TO THE OPERATOR IS IN THE OPERATOR'S LANGUAGE — Chinese.** These are different things, and the instruction was
read literally once as *"session language is English"*, which produced English replies to a Chinese question. The row in the
inbox is annotated now; this paragraph is the version an agent reads before it answers.

## The vocabulary

**`CONTEXT.md` at the root is this project's GLOSSARY** — the words that mean something specific here (device, session, run,
goal, liveness, mark) and the words that do not, each with an `_Avoid_` list. Read it before naming anything in the panel,
the console or a tool description; two surfaces disagreeing about a word is how `Trajectory` and `Path` came to look like
two names for one screen. It holds TERMS ONLY — an invariant belongs in the gate that enforces it.

## Every gate CI runs

**A GATE NOBODY CAN NAME IS A GATE NOBODY RUNS BY HAND.** These are the gate scripts the workflow invokes, 66 of them, listed because deleting the ledger proved they were documented *only* there: `numbered-claims-check` went from zero unnamed to **52** the moment the two reference tables went. A gate is named where a person can find it, or it does not exist.

**Every one of them carries its own mutation proof in its header** — the edit that must break it, and what it said when it did. Read that block when you change the gate.

`agents-snippet-check.mjs`, `all-gates.bash`, `build-pins.bash`, `chrome-stillness-check.mjs`, `ci-command-table-check.mjs`, `console-derivation-check.mjs`
`console-marks-check.mjs`, `console-smoke-check.mjs`, `console-wire-field-check.mjs`, `contract-vocabulary-check.mjs`, `contrast-probe-check.mjs`, `css-vars-check.mjs`
`custom-prop-check.mjs`, `device-verdict-check.mjs`, `device-version-rule-check.mjs`, `docs-budget-check.mjs`, `e2e-only-check.mjs`, `exports-check.mjs`
`feedback-check.mjs`, `gate-mutations-check.mjs`, `gateway-device-field-check.mjs`, `harness-fixture-check.mjs`, `hook-finds-its-repo.bash`, `http-route-header-check.mjs`
`landing-check.mjs`, `main-only-by-merge.bash`, `main-shape-check.mjs`, `main-shape-shallow.bash`, `mark-vocabulary-check.mjs`, `model-drift-check.mjs`
`motion-check.mjs`, `numbered-claims-check.mjs`, `one-derivation-check.mjs`, `panel-audit-skip-check.mjs`, `panel-design-sweep.bash`, `panel-mock-spread-check.mjs`
`npm-test-floored.mjs`, `console-assets-check.mjs`
`particles-check.mjs`, `powershell-structure-check.mjs`, `press-anchor-check.mjs`, `production-host-check.mjs`, `proxy-cors-parity-check.mjs`, `proxy-timeout-parity-check.mjs`
`publish-release.bash`, `release-audit.bash`, `release-lib.bash`, `retired-colours-check.mjs`, `scan-dups-check.py`, `script-syntax.bash`
`session-carry-detect-check.mjs`, `session-row-check.mjs`, `skill-frontmatter-check.mjs`, `smoke-helpers.bash`, `smoke-index.bash`, `spacing-scale-check.mjs`
`state-colour-check.mjs`, `stub-surface-check.mjs`, `stylesheet-hygiene.mjs`, `sweep-bundle-check.mjs`, `sweep-fixture-dupes-check.mjs`, `sweep-judges.bash`
`token-contract-check.mjs`, `wire-field-check.mjs`, `workflow-shell-check.mjs`, `workflow-yaml-check.mjs`

## Where the long form lives

**THERE IS NO LEDGER, AND THIS IS THE SECTION THAT USED TO DESCRIBE ONE.** Until landing 4a this file pointed at
`docs/agents/design-ledger.md` — one `##` section per round, what was measured and what it cost — plus three sibling
archives, 865 KB in all. **The operator retired that process**, because the loop's output had drifted into prose ABOUT the
work rather than the work, and the measurement that settled it is stark: **18 of the last 30 commits touched the ledger and
nothing else.**

| where a thing goes now | what belongs there |
|---|---|
| **the commit message** | the measurement, the before/after, and the `VERIFIED:` line. This repository's commit bodies already carry them — they are the strongest artifact here, and they are now the record |
| **`CONTEXT.md`** | a WORD and what it means. Terms only |
| **the gate itself** | any invariant that can be checked — including its own mutation proof, in its header. If a sentence can be enforced, enforce it instead of writing it down |

**`scripts/test/docs-budget-check.mjs` IS THE GATE THAT HOLDS ALL OF THIS UP** — renamed in landing 4a from
`ledger-budget-check`, because a gate called `ledger-budget-check` that no longer budgets a ledger is a name that lies. It
enforces this file's 48,000-byte ceiling, refuses a narrative `###` section growing back into it, caps `CONTEXT.md` at 12,000
bytes ("a glossary that grows into a rulebook has stopped being a glossary"). It is the whole of the budget now: the
instruction file and the glossary are all that is left of the long form.

**AND THE RULE THAT REPLACED THE PROTOCOL IS THE ONE THIS FILE HAS ALWAYS CARRIED**: if a sentence does not change what you
would DO, it does not belong in an instruction file — and it no longer has a ledger to hide in. What was 865 KB of narrative is now
`git log`, which had it in more detail all along.

## Committing

**A pre-commit hook runs the emitters** (`scripts/hooks/pre-commit`, round 225; rationale rewritten round 272).
It used to guard the backtick-in-a-template-literal accident — **and that class is gone**: all five emitters now hand
their payload to `agent/scripts/lib/sweep-bundle.mjs`, which resolves the payload's own requires and COMPILES what it
returns. (The incident count was quoted as 52 here, 34 in the hook and 38 in the operator's inbox; the three never
agreed, and they are history.) What the hook still buys is the only end-to-end
assembly of all five artifacts in under a second: a payload module that does not parse, a require the assembler cannot
resolve, or an emitter that was renamed or deleted fails at the commit instead of in the design job.

**IT IS INSTALLED NOW (round 118), AND BOTH COMMANDS THIS PARAGRAPH USED TO PRESCRIBE WERE WRONG ON THIS BOX.** Round 93's commit carried a backtick in a comment,
the probe module stopped PARSING, and five of the ten CI jobs went red (ui, panel, gateway, design, pack-chain —
everything that imports it). The hook refuses that commit in under a second, and so does
`contrast-probe-check.mjs`, which CI runs at `ci.yml:506` — but nothing ran the hook, because `core.hooksPath` is
global and both prescribed fixes for that were wrong here (below). **IT IS INSTALLED — AND UNTIL 2026-09-26 IT RAN NOTHING.** The paragraph after this
one records what is installed; what it never recorded is that the hook resolved its own repository from `$0`, which git sets to
`.githooks/pre-commit` — one level ABOVE this repository — so the `cd` succeeded in the wrong place, the guard found no
`agent/scripts/panel-design-sweep.mjs`, and it exited 0 on its fifth line **for every commit since round 118**. Not one emitter, not the
archive rule, nothing. **AND THE PROOF OFFERED FOR IT COULD NOT HAVE CAUGHT THAT**: "an empty commit and watching it run" — a commit
that SUCCEEDS looks exactly like a hook that ran and passed. **The only proof of a check is watching it REFUSE something.**
`scripts/test/hook-finds-its-repo.bash` now invokes the hook the way git does and fails if it takes the inert path; it caught the old
form on the first run.

TWO THINGS ABOUT INSTALLING IT, both measured rather than assumed:

  * **`.git/hooks/pre-commit` will NOT run on this machine** — `core.hooksPath` is set globally in `~/.gitconfig`
    to `~/.config/git/hooks`, and git ignores the per-repo directory entirely when that is set. **AND THE TWO FIXES
    THIS PARAGRAPH USED TO GIVE ARE BOTH WRONG HERE, which is why it went uninstalled for so long and why round 117
    pushed through a red gate with the hook run by hand and its exit code thrown away:**

        ln -sf "$PWD/scripts/hooks/pre-commit" ~/.config/git/hooks/pre-commit     # SAFE, and round 118 was WRONG about it

    Round 118 wrote here that this would make every other repo's commits fail, because this box has ~20 repositories
    and the hook runs summrise's own emitters. **IT WOULD NOT, AND THE HOOK SAYS SO ITSELF, twenty lines in** — its
    first act is:

        if [ ! -f agent/scripts/panel-design-sweep.mjs ]; then exit 0; fi

    with the reason spelled out above it: "a hook that blocked commits everywhere because it could not find a file
    would be a far worse outcome than the slips it exists to prevent", and the header says it is "designed to be
    symlinked into a global `core.hooksPath` (round 226)". **So the global install was always available, and the
    three rounds that went by with the hook uninstalled were three rounds of a false objection** — which is the same
    failure this ledger keeps recording: a claim about how a tool behaves, made without opening the tool. What
    remains a real objection is only the SECOND command:

        git config core.hooksPath scripts/hooks                                   # DON'T either

    shadows the global directory — which is not empty: it holds the operator's `post-commit` (tokensave auto-sync),
    so this repo would silently stop syncing.

    **WHAT IS ACTUALLY INSTALLED (round 118, and it is still the better of the two)**: a repo-local `.githooks/`
    holding BOTH links, and a repo-local path that points at it — better not because the global one is DANGEROUS but
    because it keeps this repository's hook set explicit and self-contained, and because it does not depend on a file
    in the operator's home directory staying where it is:

        mkdir -p .githooks
        ln -sf ../scripts/hooks/pre-commit .githooks/pre-commit
        ln -sf ~/.config/git/hooks/post-commit .githooks/post-commit
        git config --local core.hooksPath .githooks

    Proven by making an empty commit and watching it run. `.githooks/` is machine-local (one link points outside the
    repo), so it is gitignored; the COMMAND is the artifact, and this paragraph is where it lives.

  * **PROVE THE MUTATION, NOT THE HOOK.** The first attempt at proving it bit planted a backtick after
    `function browserScript() {` — inside the function body and OUTSIDE the template literal — so the emitter
    exited 0 and the test proved nothing about either. A trap only counts when it is inside the thing it traps.

**AND DO NOT PUT A COMMAND WHOSE STATUS YOU NEED ON THE LEFT OF A PIPE — IT HAS NOW COST TWO PUSHES.**
"A pipeline exits with its LAST command's status" is ordinary shell knowledge, and this repository has now paid for it
twice in one session, both times by landing a red suite on `main`:

  * round 117 — `bash scripts/hooks/pre-commit >/dev/null 2>&1` ran the hook and THREW THE EXIT CODE AWAY, so a failing
    `production-host-check` rode out with the commit;
  * round 128 — `npm test 2>&1 | grep … | head -2` let a FAILING gateway suite pass, because `set -e` saw `head` succeed.
    The gate that caught it was `code-viewer-mirror.test.mjs`, refusing a mirror that no longer matched the source.

The rule this file already carries — READ THE EXIT CODE, NOT THE OUTPUT — was not disobeyed either time. It was
PRESERVED and then destroyed one step later. So the operating form is narrower than the slogan:

```bash
npm test >/tmp/out 2>&1 || { echo FAILED; exit 1; }     # status kept
grep -E 'pass|fail' /tmp/out                            # output read afterwards
```

**Redirect, check, THEN filter.** A pipe is for reading output; it is not a way to keep a status.

**AND THE LOOP FORM THAT LOOKS RIGHT IS THE ONE THAT FAILS — `cmd && echo ok || echo FAIL` THROWS THE STATUS AWAY.** The
`||` branch is what runs when the command fails, so the LINE SUCCEEDS either way and `set -e` never fires; a suite can be red,
print `FAIL`, and let the commit through. Round 171 did exactly this in a loop whose whole job was to keep statuses. **Keep the
status by making the failure EXIT, not by printing:**

```bash
if timeout 300 node "$g" >/dev/null 2>&1; then echo "ok   $g"; else echo "FAIL $g"; exit 1; fi
```


**AND WHEN THE TEXT YOU ARE WRITING IS FULL OF BACKTICKS, PUT IT THROUGH A QUOTED HEREDOC — NOT `python3 -c "…"`.**
The two failures above were about KEEPING a status; this one is about the TEXT surviving the shell that carries it. Round 140
wrote a ledger line with `python3 -c "…"` — DOUBLE-quoted — and every backtick in that line was executed as COMMAND
SUBSTITUTION before Python ever saw it:

    committed:  "round 139's inventory moved to  §12 …"     # `docs/agents/inventory.md` ran as a command
    committed:  "('s pin refusal on the device, …"          # `setup` ran as a command

`stderr` said so — `Permission denied`, `command not found` — and the commit went out anyway, which is the round-117/128
failure again, in the one command that was supposed to be careful. **BACKTICKS INSIDE DOUBLE QUOTES ARE NOT LITERAL**, and
this repository's prose is nothing but backticks, so the hazard is permanent.

**AND THE DELIMITER ITSELF IS A HAZARD, WHICH ROUND 141 PROVED BY WRITING THIS PARAGRAPH**: the first attempt used the
obvious delimiter `PYEOF` — and this paragraph QUOTES `PYEOF` as an example, so the shell ended the heredoc in the middle
of the text and the editor script died with a truncated Python traceback. **Nothing was committed, because the failure was
loud.** Choose a delimiter that cannot occur in the body:

    python3 - <<'AGENTS_R141_EOF'      # quoted, and a string the body cannot contain
    ...
    AGENTS_R141_EOF

**The quoting is on the DELIMITER, not on the content**, and the NAME matters as much as the quoting.


**AND WHEN YOU COUNT SOMETHING, COUNT IT WITH THE TOOL THAT PRODUCES IT — THREE CONVENIENT COMMANDS ARE WRONG HERE.**
All three were used by this loop to report a number, and all three were wrong in the same direction, which is the direction
that makes finished work look unfinished:

| do NOT count with | because | use |
|---|---|---|
| `grep -l <name> <dir>` | it counts FILES THAT MENTION a thing, not the thing: it said three gates read `panel.css`, and `panel-sheet-freshness-check.mjs:75` says **five** read it | read the tool's own message, or `grep -c` the count it prints |
| `grep -c '^| ' <table>` | it counts the header row and the separator too: the mutation table read 46 and has **44** rows | `awk '/^\| /{n++} END{print n-2}' <table>` |
| `git tag` | **this clone has no tag above 1.2.456** while the remote has 94 up to 1.2.474 — tags are made through the GitHub API and the mirror refuses `git fetch --tags` (HTTP 400), so `git tag` answers ZERO for every release this loop made | `GET /repos/SilasVale/summrise/git/refs/tags` |

**A summary is a claim set.** If a number is going into a commit message, a ledger line or a report, the command that produced it
belongs beside it — and a command that merely CORRELATES with the number is not that command.

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
# 3. the release commit goes on a BRANCH, like everything else, and reaches main by merge
#    (main advances only by merge — the rule is under Test, and the hook enforces it)
git checkout -b release/1.2.N
git add agent/summrise-agent-npm/package.json index/public/summrise-agent/version.json
git commit -m "release: 1.2.N"
git checkout main && git merge --no-ff release/1.2.N
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
#    the tree is unchanged, so the asset matches what shipped.
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
`browser_run_script`. **THE INVOCATION IS ONE LINE, BECAUSE ROUND 269 CHANGED THE SHAPE AND BOTH EARLIER INSTRUCTIONS
DESCRIBED THE ONE BEFORE IT:**

```js
require('D:/Summrise/live-panel-probe.js');   // the whole run: token from the device's config, both densities, JSON, exit code
```

Round 183 wrote "do NOT `require()` it, which exports an empty object and runs nothing", and round 184 replaced that with
a `new Function(src + ';return probe;')` wrapper. Both were true of the template-literal emitter, which exported a bare
`probe` function. The emitter is a real module now and the assembler prints a bundled PROGRAM whose last statement is
`__require("live-run.cjs")` — so requiring it IS running it, and the round-184 wrapper throws
`ReferenceError: probe is not defined` (measured, round 7 of the standing goal). Nothing is passed in: the program reads
the panel token out of the device's own config, navigates both densities itself, prints the JSON and exits with a verdict
code.

**AND DO NOT `page.goto` THE PANEL FIRST**: the attached view is ONE page shared with the operator's screen, usually already on the
panel, so navigating it aborts (`net::ERR_ABORTED`). The program does its own navigation, so this only bites a caller
driving the attached view by hand.

**MEASURED ON THE LIVE PANEL, release 1.2.474 (round 7 of the standing goal)**: `verdict: {ok: true}` — 91 rows over two
densities (61 panel, 30 desktop), `textFailing: []`, `graphicFailing: []`, `unmeasurable: 0`, no mark collisions, no
ring+fill, `errors: []`. Run headless (`ATTACHED=false`), which is the right mode for a batch measurement because it does
not touch the operator's screen.

**AND THE EMITTED FILE IN `index/public/` MUST NOT BE GITIGNORED** (the file above is the DEVICE's copy, which is a different thing):
Workers Assets uploads the directory but HONOURS `.gitignore`, so an ignored file is silently absent from the deploy — which is why the playwright zip was never a
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

**MUTATION THAT MUST BREAK IT** (moved here from the ledger table, landing 4b): *add a parameter inside a device tool's
`properties`* → **exit 101, snapshot diff.** A snapshot that does not notice a new parameter is a snapshot that is not
pinning the surface.
