# Main advances only by merge — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `main` advance only by merge, enforced by the pre-commit hook on the developer's machine and by a gate in CI, so that work reaches `main` through a branch that was verified and reviewed.

**Architecture:** One pure decision function (`verdict({branch, parents})`) shared by a CI gate that carries its own fixture table, plus a rule in the existing `pre-commit` hook that refuses a non-merge commit while HEAD is on `main`. The two halves are deliberately different instruments: the hook is fast, local and bypassable with `--no-verify`; the gate is server-side and cannot be bypassed. Neither is the only line.

**Tech Stack:** Node 24 (ESM, `node --test`-free — this repo's gates are plain scripts with `process.exit`), bash for the hook proof, GitHub Actions for the CI step.

**Spec:** `docs/superpowers/specs/2026-09-26-change-release-deploy-design.md` — §6 is this plan's whole subject; §3.1 supplies the standing answers; §9 places this as landing 1 of 4.

## Global Constraints

- **Node 24** for every gate; gates are `.mjs` and exit `0` pass / `1` bite / `2` "this host cannot run me" / `3` "I ran and could not measure".
- **Never put a command whose status you need on the left of a pipe** (AGENTS.md). Redirect, check, then filter.
- **`core.hooksPath` is `.githooks`** (measured); `scripts/hooks/pre-commit` is symlinked there and is the only hook file this plan modifies.
- **A gate that cannot fail is worse than no gate.** Every new check ships with the mutation that must break it.
- **`main` today has ZERO merge commits in the last 300** (measured). This plan changes that deliberately, and the commit that introduces the rule must itself land as a merge.
- **Do not touch the ledger.** It is retired by a later landing; this plan neither reads nor writes it.

## Review Focus

Five input classes the spec implies but no task's own tests exercise. Each gets its test in the task that owns the code, and the list is repeated here so a reviewer knows what to look for.

1. **A push to a non-`main` branch must pass untouched.** The gate's whole risk is that it blocks ordinary branch work. → Task 2's fixture `change/x` + 1 parent → PASS, and Task 2 Step 4's end-to-end run from a branch.
2. **A plain `git merge` that FAST-FORWARDS creates no commit at all**, so the hook never runs and `main` advances linearly with the branch's non-merge commits. The hook cannot see this; the CI gate is the only thing that can. → Task 2's fixture `main` + 1 parent → FAIL, and the note in Task 5.
3. **The rule must not block its own introduction.** The commit that adds the hook rule has to reach `main` somehow. → Task 5 lands on a branch and merges `--no-ff`.
4. **`git commit --amend` on `main`** is a non-merge commit and must be refused — rewriting `main` is exactly the shape this plan exists to stop. → Task 4 Step 3.
5. **The release commit is not exempt, AND THE SCRIPT DOES NOT MAKE IT.** Measured: `scripts/publish-release.sh` contains **no `git commit` at all** — it packs, publishes, deploys and smokes, and the commit is a documented MANUAL step in `AGENTS.md`'s `## Release`. So landing 1 does not break the script; it invalidates a WRITTEN INSTRUCTION, which is worse, because a reader follows it and the hook refuses. → Task 3 Step 5.

---

### Task 1: `.worktrees/` is ignored

**Files:**
- Modify: `.gitignore` (append one line)

**Interfaces:**
- Consumes: nothing
- Produces: nothing — but `using-git-worktrees` **requires** this before any worktree exists, or the worktree's contents get committed into the repo. Measured: `.worktrees` is NOT ignored today.

- [ ] **Step 1: Confirm the current state — this is the failing test**

```bash
git check-ignore -q .worktrees/; echo "exit=$?"
```

Expected: `exit=1` — not ignored, so the prerequisite is unmet. If it prints `exit=0`, the directory is already ignored, this task is a no-op, and you should say so and skip to Task 2.

> **TWO TRAPS HERE, BOTH MEASURED, AND THE FIRST ONE IS IN THE SKILL ITSELF.**
>
> | query | `.worktrees` exists | no pattern | pattern present |
> |---|---|---|---|
> | `check-ignore .worktrees` (no slash) | **no** | exit 1 | **exit 1 — WRONG** |
> | `check-ignore .worktrees` (no slash) | yes | exit 1 | exit 0 |
> | `check-ignore .worktrees/` (slash) | no | exit 1 | exit 0 |
> | `check-ignore .worktrees/` (slash) | yes | exit 1 | exit 0 |
>
> A directory-only pattern is reported as a match **only when the queried path ends in a slash, or the directory already
> exists** — so the no-slash form answers "not ignored" for a correctly ignored directory that has not been created yet.
> **`using-git-worktrees` uses exactly that form**, in the check it runs *before* creating the worktree:
> `git check-ignore -q .worktrees 2>/dev/null || git check-ignore -q worktrees 2>/dev/null`. It therefore sends a reader
> to add a line that is already there. **Always query with the trailing slash.**
>
> **AND DO NOT "TEST IT BEHAVIOURALLY" WITH `git status` ALONE**: git does not track empty directories, so an empty
> `.worktrees` is invisible to `git status` whether or not it is ignored, and a test built on that cannot distinguish the
> two states. Measured, both ways. If you want the behavioural form, it needs a file inside:
>
> ```bash
> mkdir -p .worktrees && touch .worktrees/probe
> git status --porcelain --untracked-files=all | grep -q worktrees; echo "exit=$?"   # 0 = would commit, 1 = ignored
> rm -rf .worktrees
> ```

- [ ] **Step 2: Add the line**

Append to `.gitignore`:

```gitignore
# Worktrees for the branch-per-change flow (using-git-worktrees). MUST be ignored:
# an unignored worktree directory commits the whole tree into the repository.
.worktrees/
```

- [ ] **Step 3: Verify it passes**

```bash
git check-ignore -q .worktrees/; echo "exit=$?"
```

Expected: `exit=0`.

- [ ] **Step 4: Commit**

```bash
git add .gitignore
git commit -m "chore(git): ignore .worktrees, which the branch-per-change flow needs"
```

---

### Task 2: `main-shape-check.mjs` — the decision, with its fixture table

**Files:**
- Create: `scripts/test/main-shape-check.mjs`
- Test: the `SELF_TEST` table inside that file (this repo's pattern for a gate whose subject is not a source file — `powershell-structure-check.mjs` carries its own the same way)

**Interfaces:**
- Consumes: nothing
- Produces: `export function verdict({ branch, parents })` → `{ ok: boolean, why: string }`. Task 3's CI step invokes the file as a script; nothing else imports it.

- [ ] **Step 1: Write the failing test — the fixture table, before the implementation**

Create `scripts/test/main-shape-check.mjs` with **only** this content:

```js
// main-shape-check — MAIN ADVANCES ONLY BY MERGE.
//
// MEASURED, 2026-09-26: `main` has ZERO merge commits in its last 300, so every commit was made directly on it — and red
// has reached `main` three times (rounds 117, 128, 171), each time by a status that was thrown away one step after it was
// read. Nothing structural stood in the way. This is the structure.
//
// THE TWO HALVES, AND WHY BOTH EXIST. `scripts/hooks/pre-commit` refuses a non-merge commit while HEAD is on `main` —
// fast, local, and bypassable with `--no-verify`. THIS gate runs in CI, server-side, and cannot be bypassed. Neither is
// the only line, because a rule with one enforcement point is a rule with one way around it.
//
// AND THE HALF THIS GATE ALONE CAN SEE: a plain `git merge` FAST-FORWARDS when `main` has not moved, creating NO commit —
// so the hook never runs, and `main` advances linearly carrying the branch's non-merge commits. The hook cannot observe
// that; a check of the pushed commit's parents can. The flow merges `--no-ff` (spec §6) and this is the backstop for when
// somebody does not.
//
// Run: node scripts/test/main-shape-check.mjs

const SELF_TEST = [
  // [ branch, parent count, expected ok, what it is ]
  ["main", 1, false, "a direct commit on main — the shape this exists to refuse"],
  ["main", 0, false, "a root commit on main"],
  ["main", 2, true, "a --no-ff merge of a change branch: the shape the flow produces"],
  ["main", 3, true, "an octopus merge, which is still a merge"],
  ["change/some-work", 1, true, "ordinary branch work must never be blocked"],
  ["change/some-work", 0, true, "a branch's first commit"],
];
```

- [ ] **Step 2: Run it to verify it fails**

```bash
node scripts/test/main-shape-check.mjs; echo "exit=$?"
```

Expected: the file parses and exits **0**, having checked **nothing** — `SELF_TEST` is declared and never read. That is the failure this step is for: a fixture table that nothing consumes is a comment.

- [ ] **Step 3: Write the minimal implementation**

Append to `scripts/test/main-shape-check.mjs`:

```js
import { execFileSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { pathToFileURL } from "node:url";

/** The whole decision, pure and testable: `main` must be reached by a merge, and nothing else is constrained. */
export function verdict({ branch, parents }) {
  if (branch !== "main") return { ok: true, why: `not main (${branch}) — this rule constrains main only` };
  if (parents >= 2) return { ok: true, why: `main at a merge commit (${parents} parents)` };
  return {
    ok: false,
    why:
      `main is at a commit with ${parents} parent(s), so it advanced WITHOUT a merge. Work reaches main through a branch ` +
      `that was verified and reviewed, and a plain \`git merge\` FAST-FORWARDS when main has not moved — creating no ` +
      `commit at all. Merge with --no-ff.`,
  };
}

// THE MODULE IS IMPORTABLE WITHOUT RUNNING THE CHECK. Anything a reader imports must not have side effects, or the only
// way to test it is to copy it — which is how a decision function ends up with two versions that drift.
//
// THIS GUARD IS STRONGER THAN THIS PLAN'S FIRST VERSION, which compared basenames and would confuse two files of the same
// name in different directories. That exact mistake is already recorded in this repository: `workflow-yaml-check.mjs`
// printed its pass line and called `process.exit` when a reader imported it to test one function. Resolve the real path
// and compare URLs instead.
const isMain = process.argv[1] && pathToFileURL(realpathSync(process.argv[1])).href === import.meta.url;

if (isMain) {
  // 1. the fixture table first: a gate whose fixtures do not run is a gate nobody has proven
  let failed = 0;
  for (const [branch, parents, want, what] of SELF_TEST) {
    const got = verdict({ branch, parents }).ok;
    if (got !== want) {
      console.error(`FAIL self-test: ${branch} with ${parents} parent(s) should be ${want ? "ok" : "refused"} — ${what}`);
      failed++;
    }
  }
  if (failed) {
    console.error(`\nFAIL main-shape: ${failed} of ${SELF_TEST.length} fixture(s) wrong — the decision does not do what it says.`);
    process.exit(1);
  }

  // 2. then the real repository, where CI has a real HEAD
  const branch = process.env.GITHUB_REF_NAME || execFileSync("git", ["branch", "--show-current"], { encoding: "utf8" }).trim();
  const parents = execFileSync("git", ["rev-list", "--parents", "-n", "1", "HEAD"], { encoding: "utf8" }).trim().split(/\s+/).length - 1;
  const r = verdict({ branch, parents });
  if (!r.ok) {
    console.error(`FAIL main-shape: ${r.why}`);
    process.exit(1);
  }
  console.log(`main-shape: ${SELF_TEST.length} fixture(s) hold; ${branch} at ${parents} parent(s) — ${r.why}`);
}
```

- [ ] **Step 4: Run it to verify it passes**

```bash
node scripts/test/main-shape-check.mjs; echo "exit=$?"
```

Expected: `exit=0`, printing `6 fixture(s) hold; …`. On a branch it prints `not main (…)`; **on `main` today it will FAIL**, because `main` is at a one-parent commit — which is correct, and is why Task 5 lands this on a branch and merges.

- [ ] **Step 5: Prove the table bites — break the decision on purpose**

```bash
sed -i 's/if (parents >= 2)/if (parents >= 1)/' scripts/test/main-shape-check.mjs
node scripts/test/main-shape-check.mjs; echo "exit=$?"
git checkout scripts/test/main-shape-check.mjs
```

Expected: the mutated file exits **1** naming the `main` + 1 parent fixture. If it still exits 0, the table is not wired to the decision and Step 3 is wrong.

- [ ] **Step 6: Commit**

```bash
git add scripts/test/main-shape-check.mjs
git commit -m "test(gates): main advances only by merge, with the fixture table that proves the decision"
```

---

### Task 3: Wire the gate into CI and name it where an operator reads

**Files:**
- Modify: `.github/workflows/ci.yml` (one new `- name:` / `run:` pair, in the `pack-chain` job beside `workflow-yaml-check`)
- Modify: `AGENTS.md` (a line in `## Test` naming the gate, and the release commit in `## Release` — which this rule makes impossible as written)

**Interfaces:**
- Consumes: `scripts/test/main-shape-check.mjs` from Task 2, invoked as a script (no import)
- Produces: nothing

**Why the AGENTS.md line is part of THIS task:** `numbered-claims-check.mjs` fails on any gate CI invokes that is named neither in `AGENTS.md` nor in the ledger — and this plan does not touch the ledger. Measured on the previous landing: adding a gate without naming it produced exactly that failure.

- [ ] **Step 1: Add the CI step — as its OWN `- name:` / `run:` pair**

In `.github/workflows/ci.yml`, immediately after the `every workflow file is valid YAML` step in `pack-chain`, add:

```yaml
      - name: main advances only by merge
        # MAIN IS THE ONE BRANCH WHERE A MISTAKE IS EVERYONE'S. `main` had zero merge commits in its last 300 and red
        # reached it three times (rounds 117, 128, 171). The pre-commit hook refuses a non-merge commit on main locally;
        # THIS runs server-side and cannot be bypassed with --no-verify. It is also the only half that can see a plain
        # `git merge` that fast-forwarded — which creates no commit, so the hook never runs.
        run: node scripts/test/main-shape-check.mjs
```

**Do not append the `run:` line under the previous step's `run:`** — that is the shape that left this file unparseable for ~35 commits.

- [ ] **Step 2: Verify the workflow still parses — this is the test**

```bash
node scripts/test/workflow-yaml-check.mjs; echo "exit=$?"
python3 -c "import yaml; d=yaml.safe_load(open('.github/workflows/ci.yml')); print('jobs:', len(d['jobs']), 'steps:', sum(len(j.get('steps',[])) for j in d['jobs'].values()))"
```

Expected: `exit=0`, and one more step than before.

- [ ] **Step 3: Name it in `AGENTS.md`**

In `## Test`, in the paragraph that names the workflow gates, add:

```markdown
`scripts/test/main-shape-check.mjs` — **`main` advances only by merge.** A direct commit on `main` is refused by
`scripts/hooks/pre-commit` and, server-side, by this gate; work reaches `main` through a branch that was verified and
reviewed, merged with `--no-ff`.
```

- [ ] **Step 4: Fix the instruction this rule invalidates**

`AGENTS.md`'s `## Release` section tells a reader to make the release commit directly:

```markdown
# 3. ONE commit that includes agent/summrise-agent-npm/package.json and index/public/summrise-agent/version.json
git push origin main          # CI green on the pushed commit
```

That instruction is now impossible — the hook refuses it. Replace it with the shape the rule requires:

```markdown
# 3. the release commit goes on a BRANCH, like everything else, and reaches main by merge
git checkout -b release/1.2.N
git add agent/summrise-agent-npm/package.json index/public/summrise-agent/version.json
git commit -m "release: 1.2.N"
git checkout main && git merge --no-ff release/1.2.N
git push origin main          # CI green on the pushed commit
```

**This step is not bookkeeping.** An instruction that cannot be carried out is the class this repository has already paid
for twice: `production-host-check`'s failure message told a reader to add an `ALLOWED` entry while its own ratchet refused
that entry one commit later. A rule and the document that contradicts it is worse than either alone.

- [ ] **Step 5: Run the census and the gate**

```bash
node scripts/test/numbered-claims-check.mjs; echo "census=$?"
node scripts/test/ci-command-table-check.mjs; echo "cmdtable=$?"
node scripts/test/main-shape-check.mjs; echo "gate=$?"
```

Expected: `census=0`, `cmdtable=0`, and `gate=1` **if you are on `main`** (correct — Task 5 fixes that by landing as a merge) or `gate=0` if you are on a branch.

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/ci.yml AGENTS.md
git commit -m "ci(gates): run main-shape-check, name it in AGENTS.md, and move the release commit onto a branch"
```

---

### Task 4: The rule, as its own script, and its proof

**Files:**
- Create: `scripts/hooks/main-only-by-merge`
- Modify: `scripts/hooks/pre-commit` (ONE call site, placed AFTER the "is this Summrise?" guard)
- Test: `scripts/test/main-only-by-merge.bash` — a real throwaway repository, the rule installed as its `pre-commit` hook

**Interfaces:**
- Consumes: nothing
- Produces: an executable `scripts/hooks/main-only-by-merge` that exits 0 when the commit may proceed and 1 when it may not

> **THIS TASK'S DESIGN CHANGED BECAUSE THE FIRST VERSION WOULD HAVE PROVEN NOTHING.** The plan said to put the rule in a
> block inside `scripts/hooks/pre-commit` and to prove it by copying that hook into a throwaway repository. **Measured by
> opening the hook:** its fifth line is
>
> ```bash
> if [ ! -f agent/scripts/panel-design-sweep.mjs ]; then exit 0; fi
> ```
>
> — it is designed to be symlinked into a GLOBAL `core.hooksPath` and must be inert in every other repository the operator
> owns. A throwaway repository never reaches anything below that guard, so the proof would have reported five green cases
> having run **nothing**. The rule is therefore its own script, which is testable there, and the call site is asserted
> separately because **a rule nothing invokes is a rule that does not exist**.

- [ ] **Step 1: Write the failing test**

Create `scripts/test/main-only-by-merge.bash`. It builds a throwaway repository, **builds its history FIRST and installs the
rule as its `pre-commit` hook SECOND**, then runs six cases through the installed hook:

```bash
#!/usr/bin/env bash
# main-only-by-merge — THE RULE PROVEN ON A REAL REPOSITORY, IN BOTH DIRECTIONS, PLUS ITS WIRING.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
RULE="$PWD/scripts/hooks/main-only-by-merge"
HOOK="$PWD/scripts/hooks/pre-commit"
[ -f "$RULE" ] || { echo "  FAIL scripts/hooks/main-only-by-merge does not exist — there is no rule to prove" >&2; exit 1; }

T=$(mktemp -d) || exit 2
trap 'rm -rf "$T"' EXIT
cd "$T" || exit 2
git init -q . && git symbolic-ref HEAD refs/heads/main
git config user.email t@t.invalid && git config user.name t
[ "$(git branch --show-current)" = "main" ] || { echo "  FAIL the fixture repo is not on main" >&2; exit 2; }
echo one > f && git add f && git commit -qm "root"
mkdir -p scripts/hooks && cp "$RULE" scripts/hooks/pre-commit && chmod +x scripts/hooks/pre-commit
git config core.hooksPath scripts/hooks

fail=0
run() { local name="$1" want="$2"; shift 2; "$@" >/dev/null 2>&1; local got=$?
  if [ "$got" = "$want" ]; then echo "  ok   $name (exit $got)"
  else echo "  FAIL $name — expected exit $want, got $got"; fail=1; fi; }

echo two > f && git add f
run "a direct commit on main is refused" 1 git commit -qm "direct"
git reset -q --hard HEAD
echo three > f && git add f
run "--amend on main is refused" 1 git commit -q --amend -m "rewrite"
git reset -q --hard HEAD
git checkout -qb change/x
echo four > f && git add f
run "a commit on a branch is allowed" 0 git commit -qm "on branch"
git checkout -q main
run "a --no-ff merge into main is allowed" 0 git merge --no-ff -q -m "merge change/x" change/x
parents=$(git rev-list --parents -n 1 HEAD | awk '{print NF-1}')
if [ "$parents" = "2" ]; then echo "  ok   the merge produced a 2-parent commit"
else echo "  FAIL the merge produced $parents parent(s)"; fail=1; fi
if grep -q 'main-only-by-merge' "$HOOK"; then echo "  ok   scripts/hooks/pre-commit invokes the rule"
else echo "  FAIL scripts/hooks/pre-commit does NOT invoke the rule"; fail=1; fi
[ "$fail" = 0 ] && echo "main-only-by-merge: 6 case(s), both directions, through the installed hook"
exit "$fail"
```

- [ ] **Step 2: Run it to verify it fails**

```bash
chmod +x scripts/test/main-only-by-merge.bash
bash scripts/test/main-only-by-merge.bash; echo "exit=$?"
```

Expected: **exit 1**, `scripts/hooks/main-only-by-merge does not exist — there is no rule to prove`.

- [ ] **Step 3: Write the rule and wire it**

Create `scripts/hooks/main-only-by-merge`:

```bash
#!/usr/bin/env bash
# main-only-by-merge — MAIN ADVANCES ONLY BY MERGE.
# Its own file, because `pre-commit` exits 0 unless agent/scripts/panel-design-sweep.mjs exists, so a proof cannot
# observe a rule that lives below that guard. Called AFTER the guard, or it would refuse commits on `main` in every
# other repository on this machine.
set -uo pipefail
branch=$(git branch --show-current 2>/dev/null || true)
if [ "$branch" != "main" ]; then exit 0; fi
if git rev-parse --verify -q MERGE_HEAD >/dev/null; then exit 0; fi
echo "pre-commit: this commit is being made directly on main." >&2
echo "  main advances only by merge. Open a branch, do the work there, then:  git merge --no-ff <branch>" >&2
exit 1
```

Then in `scripts/hooks/pre-commit`, immediately after `fail=0`:

```bash
if ! bash scripts/hooks/main-only-by-merge; then
  fail=1
fi
```

- [ ] **Step 4: Run the proof to verify it passes**

```bash
bash scripts/test/main-only-by-merge.bash; echo "exit=$?"
```

Expected: **6 cases, both directions**, `exit=0`.

- [ ] **Step 5: Prove the proof bites — neuter the rule**

```bash
cp scripts/hooks/main-only-by-merge /tmp/rule.bak
sed -i 's/^exit 1$/exit 0/' scripts/hooks/main-only-by-merge
bash scripts/test/main-only-by-merge.bash; echo "exit=$?"
cp /tmp/rule.bak scripts/hooks/main-only-by-merge
```

Expected: **exit 1**, with cases 1 and 2 failing — the refusal cases pass when they must fail.

- [ ] **Step 6: Commit**

```bash
git add scripts/hooks/main-only-by-merge scripts/hooks/pre-commit scripts/test/main-only-by-merge.bash
git commit -m "fix(hooks): main advances only by merge, proven on a real repository in both directions"
```

**THREE THINGS THE PROOF'S OWN FIRST RUNS CAUGHT, all recorded in the script's header rather than quietly fixed:**
1. `git init` on this box defaults to `master`, so the rule — which matches `main` exactly, and correctly — was **inert** and two cases reported `exit 0` that should have been refused. The fixture now sets the unborn branch with `symbolic-ref` and asserts it.
2. The merge case invoked the rule *by hand before merging*, when no merge was in progress, and reported a **correct refusal as a failure**. It now runs the real `git merge` through the installed hook, which is the only way `MERGE_HEAD` is ever set.
3. Installing the hook before the root commit made the fixture **unable to create its own history** — the rule refuses a root commit on `main`, and `main-shape-check`'s fixture table asserts the same verdict, so the two halves agree. History is built first.

### Task 5: Land it — on a branch, merged `--no-ff`

**Files:**
- No file changes. This task is the flow proving itself.

**Interfaces:**
- Consumes: every artifact from Tasks 1-4
- Produces: `main` at a merge commit — the first in 300+

- [ ] **Step 1: Run the full suite on the branch, before merging**

```bash
bash scripts/test/all-gates.bash > /tmp/landing1.out 2>&1; echo "exit=$?"; tail -3 /tmp/landing1.out
```

Expected: `exit=0`, and `N ok, 0 failed, 1 not runnable here`. **If any gate refuses because the tree is dirty, commit first** — `gate-mutations-check` and `console-assets-check` both refuse a dirty tree by design, and that refusal is not a failure.

- [ ] **Step 2: Merge with `--no-ff` — the flag is the point**

```bash
git checkout main
git merge --no-ff change/main-only-by-merge -m "merge: main advances only by merge (landing 1 of 4)"
git rev-list --parents -n 1 HEAD | awk '{print NF-1 " parent(s)"}'
```

Expected: `2 parent(s)`. **A plain `git merge` here would print `1`** if the branch had been rebased onto main, or create no commit at all if it fast-forwarded — which is the defect spec §6 records.

- [ ] **Step 3: Verify the rule now holds on the landed tree**

```bash
node scripts/test/main-shape-check.mjs; echo "exit=$?"
bash scripts/test/main-only-by-merge.bash; echo "exit=$?"
```

Expected: both `exit=0`. The gate that FAILED on `main` in Task 2 Step 4 now passes, because `main` is at a merge — that before/after is the proof that it was measuring the real thing.

- [ ] **Step 4: Push and confirm CI runs JOBS, not just a conclusion**

```bash
git push origin main
sleep 60
SHA=$(git rev-parse HEAD)
curl -s -H "Authorization: Bearer $(cat ~/.github-token)" \
  "https://api.github.com/repos/SilasVale/summrise/actions/runs?head_sha=$SHA&per_page=1" \
  | python3 -c "import json,sys;d=json.load(sys.stdin);r=d['workflow_runs'][0];print('run',r['id'],r['status'],r['conclusion'],'instant='+str(r['created_at']==r['updated_at']))"
```

Expected: **`instant=False`**. If it prints `instant=True` with `conclusion: failure`, `ci.yml` is unparseable again — run `node scripts/test/workflow-yaml-check.mjs` and fix that before anything else. **A zero-job run has no log to open**, which is how a ~35-commit outage stayed invisible.

- [ ] **Step 5: Delete the branch**

```bash
git branch -d change/main-only-by-merge
```

Expected: deleted. **`-d` and not `-D`**: git refuses if the branch is not fully merged, which is the check you want.

---

## What this plan does NOT do

Landings 2, 3 and 4 of the spec, each of which gets its own plan:

| landing | subject |
|---|---|
| 2 | `CONTEXT.md` · the four `summrise-*` skills · the `.claude/skills` symlinks |
| 3 | `AGENTS.md` shrunk to a map, its rules moved verbatim into the skills |
| 4 | the mutation table into the gates · the four archives, `ledger-budget-check` and its wiring deleted |

Landing 1 is first because it depends on nothing else and is the change that would have prevented the incidents the spec exists for.
