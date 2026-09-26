# Change → Release → Deploy, redesigned around the superpowers flow

**Date:** 2026-09-26 · **Status:** design, awaiting implementation · **Path:** architectural (brainstorming skill)

> **THIS FILE IS A DESIGN ARTIFACT, NOT AN APPARATUS.** It exists to be implemented, and it is DELETED when the
> implementation lands — its durable content moves to `CONTEXT.md` and to the commit that implements it. There is no
> journal here and nothing appends to it. That distinction is the whole point of this redesign: this repository already
> paid once for an append-only record (`25507166`, "delete the remaining design-doc apparatus"), and re-creating that
> shape under a new name would repeat the mistake.

---

## 1 · Why

The repository's working protocol is `AGENTS.md` (26 KB, read once at session start) plus an append-only record,
`docs/agents/design-ledger.md` and its three sibling archives — **884,867 bytes across four files**, which six gates and
the pre-commit hook read. The loop's output drifted into prose about the work rather than the work: measured before the
round-189 rule, **18 of the last 30 commits touched the ledger and nothing else**.

The operator's instruction, twice and then plainly: the ledger process is retired, and the change → release → deploy
flow is redesigned around the superpowers skills.

**What is NOT wrong and must survive:** the gate suite (23 gates with proven bites), the release mechanics (npm + CDN +
tag through the API + device update), and the operational rules this repository paid for in incidents — the reporter
table, the pipe-status rule, "run the command the other end runs", the counting rules. None of that is generic
superpowers content, and none of it has a home in a skill pack.

## 2 · Intent and success criteria

| | |
|---|---|
| **Intent** | A change is made, verified, reviewed and landed through the superpowers phases — each one *loaded when it applies* rather than read once — and release/deploy follow from a landed, reviewed `main`. |
| **Success 1** | `AGENTS.md` ≤ 12 KB and is a map, not a manual. Measured: it is 26,045 B today, of which **`## Release` is 8,269 B and `## The design ledger` is 1,411 B** — both leave entirely (§4, §5) — and `## Committing`'s 9,251 B is mostly process that moves to a skill. The number is reachable, not aspirational. |
| **Success 2** | Every phase is a skill an agent can be handed at the moment it applies. |
| **Success 3** | `main` advances **only by merge**. Measured today: **zero merge commits in the last 300** — the repo is purely linear, and red has reached `main` three times (rounds 117, 128, 171) because nothing structural stood in the way. |
| **Success 4** | The ledger is gone, and **no gate loses its bite proof** — the proofs move next to the gates they prove. |
| **Success 5** | The durable record is smaller than what it replaces, by an order of magnitude. |

## 3 · The flow

```
brainstorming                    architectural → this spec ; bounded → a short design in chat
  → writing-plans                docs/superpowers/plans/YYYY-MM-DD-<slug>.md
  → using-git-worktrees          .worktrees/<slug> on change/<slug>
  → test-driven-development      red → green → refactor
  → verification-before-completion   evidence and exit codes, not assertions
  → requesting-code-review       two axes: does it follow the repo's standards, does it match the spec
  → receiving-code-review        dispose of every finding, in writing
  → finishing-a-development-branch   tests green → merge to main
  → summrise-release             from a clean, merged main only
  → summrise-deploy              device update + its verification loop
```

**The plan is a per-change artifact, under the same rule as this spec**: it is written where the `writing-plans` skill
puts it (`docs/superpowers/plans/`), it rides in the change's branch, and it is deleted when the change lands — its
durable content is the commit message. **Neither directory is a journal, and nothing appends to either.**

### 3.1 The checkpoints the skills assume a human is standing at — and their STANDING ANSWERS

This is the part that does not work out of the box, and it is stated here rather than discovered mid-flow. Two skills
stop and ask, which an autonomous loop cannot answer per change:

- `using-git-worktrees` Step 0: *"Would you like me to set up an isolated worktree?"*
- `finishing-a-development-branch` Step 4: a three-option menu — merge locally / push a PR / keep as-is — and the skill
  is explicit that *"integration is your human partner's decision"*.

**Resolution: the operator answers each ONCE, and the answer is recorded where the agent reads it.** A standing answer is
not a skipped checkpoint; it is a checkpoint moved to where it is asked once instead of every time.

| checkpoint | standing answer | where it is recorded |
|---|---|---|
| worktree consent | **yes, always** | `summrise-change` skill, and this table |
| worktree directory | `.worktrees/` (must be gitignored first — measured: **it is not**) | `.gitignore` + the skill |
| integration method | **option 1 — merge locally to `main`** | `summrise-change` skill |
| when a release may run | only when the operator's active goal says so; never as a side effect of a change | `summrise-release` skill |
| code-review disposition | the loop may accept or reject a finding, but must record the reasoning in the commit | `summrise-change` skill |

A standing answer is **revocable in one line** and lives in one file. It is also visible: the skills' own questions still
appear in their text, so a reader can see which ones were pre-answered and by what.

## 4 · Where the process lives

| | |
|---|---|
| `AGENTS.md` | a **map**, ≤ 12 KB: what this repo is · the red lines · the phase table (which skill, when) · the build/test/release entry points. The operational rules it keeps are the ones that change what you DO. |
| `.agents/skills/summrise-change/SKILL.md` | the phases, this repo's gates, the standing answers, the commit discipline |
| `.agents/skills/summrise-release/SKILL.md` | the release sequence, verbatim from today's `AGENTS.md` — it is hard-won and correct |
| `.agents/skills/summrise-deploy/SKILL.md` | device update + the verification loop (`summrise status`, the live-panel probe) |
| `.agents/skills/summrise-gates/SKILL.md` | **how** to add a gate and prove it bites, and how to run the mutation audit. **The mutations themselves are NOT here** — each lives in its own gate (§5), because a proof kept away from its subject is exactly the arrangement that let the ledger's table grow to 110 KB |
| `.claude/skills/summrise-*` | **symlinks** into `.agents/skills/` — which is the pattern the vendored pack already uses (measured: every entry in `.claude/skills/` is a symlink to `../../.agents/skills/<name>`). Nothing vendored is patched. |

**Why skills and not prose:** a rule in a 26 KB file is read once, at session start, by an agent that does not yet know
which part applies. A skill is handed over at the moment it applies. The ledger grew precisely because prose has no
moment of relevance and no ceiling that anyone was enforcing.

## 5 · What replaces the ledger

| the ledger's job | new home | the rule |
|---|---|---|
| the mutation table — 110 KB, the bite proofs for 23 gates | **into each gate as its own self-test** | the proof lives with the thing it proves. `harness-fixture-check.mjs` and `powershell-structure-check.mjs` already carry theirs this way; `gate-mutations-check.mjs` stays as the runner for the ones that can be automated |
| domain vocabulary and invariants | **`CONTEXT.md`** — ONE file, at the root | the `domain-modeling` mechanism. Not a directory, not a set of files |
| decisions and their measurements | **the commit message** | already this repository's strongest artifact: the bodies carry the before/after, the exact command, and a VERIFIED section |
| ~700 KB of round narratives | **nothing — git history** | they are already there, in more detail than the ledger's summary |
| the index ("looking for one thing") | `CONTEXT.md` + the gate skills | |

**AND NO NEW `docs/adr/` DIRECTORY.** It was pruned in `25507166` along with the rest of the design-doc apparatus, and
re-creating it would undo a decision the operator already paid for. Decisions go in commit messages; rules go in
`CONTEXT.md`; proofs go next to their gates.

## 6 · The branch gate, made mechanical

Today `main` is linear and every commit is made on it. The redesign makes `main` **advance only by merge**, enforced in
two places so that neither is the only line:

1. **`scripts/hooks/pre-commit`** (already installed via `core.hooksPath = .githooks`): **refuse a non-merge commit made
   while HEAD is on `main`.** A merge (`MERGE_HEAD` present) passes. This is the same shape as the existing
   archive-only rule, which already refuses a commit for a structural reason.
2. **A `main-shape` CI job**: on a push to `main`, fail unless `HEAD` has two or more parents. Server-side, and it cannot
   be `--no-verify`'d away.

**AND THE MERGE MUST BE `--no-ff`, WHICH IS THE DEFECT THIS SECTION HAD ON FIRST WRITING.** The `finishing-a-development-branch`
skill's option 1 runs a plain `git merge <feature-branch>`; when `main` has not moved, that **fast-forwards**, creating
**no merge commit at all** — so check 2 would fail on a flow that had followed the skill exactly. The flow therefore
merges with `git merge --no-ff`, and the skill wrapper says so. A rule whose two halves disagree about what a merge is
would have been discovered by a red CI job, one round later, by whoever tried it first.

`publish-release.sh`'s release commit is not exempt: it is made on a branch and merged like everything else. The release
sequence itself does not change.

**Prerequisite, measured: `.worktrees/` is NOT gitignored today.** `using-git-worktrees` requires it to be, or the
worktree's contents get committed. The implementation adds it before any worktree exists.

## 7 · Release and deploy

Mechanics unchanged — npm is the only channel, the CDN alias and `version.json` move together, the tag is made through
the API against a pushed CI-green commit, and the device updates with `summrise update`. What changes is **where it is
written down** (a skill, not `AGENTS.md` prose) and **what may precede it** (a merged, verified `main`, never a working
branch).

`summrise-deploy` carries the verification the release already owes: `summrise status` for the version, and the
live-panel probe against `127.0.0.1:18080` on the device, because the harness is not the panel the operator is running.

## 8 · What gets deleted

| deleted | why it is safe |
|---|---|
| `docs/agents/design-ledger.md`, `ledger-appendix.md`, `ledger-mutations.md`, `ledger-early-rounds.md` (884,867 B) | triaged by §5; git history keeps every byte |
| `scripts/test/ledger-budget-check.mjs` + its `ci.yml` step + its 2 mutation cases | its subject is gone |
| the archive-only rule and `SUMMRISE_LEDGER_ONLY` in `scripts/hooks/pre-commit` | replaced by the branch rule in §6 |
| `numbered-claims-check`'s ledger entry | `AGENTS.md` remains as documented surface |
| `production-host-check`'s 2 ledger allowances | the ratchet goes **43 → 41**, which is it shrinking, as its own rule demands |

**Gate count 23 → 22, and no gate loses its bite proof.** That 23 is the **mutation-audited set** — the gates
`gate-mutations-check.mjs` carries cases for (37 cases over 23 gates, measured on the clean tree at `00eb9292`) — not the
62 gate commands `ci.yml` invokes. The two numbers answer different questions and neither replaces the other.

## 9 · Implementation order, and why it is four landings rather than one

The scope check asked whether this is one plan or four. It is **one design, four landings**, and each one leaves the
repository consistent on its own — a half-converted repo is the failure mode to avoid, and it is concrete here: the
ledger's own gates fail on a half-deleted ledger, so the deletion cannot be spread across landings.

| # | landing | the state it leaves |
|---|---|---|
| 1 | `.worktrees/` gitignored · `pre-commit` refuses a non-merge commit on `main` · the `main-shape` CI job | fully working, and `main` advances only by merge. Nothing else has moved |
| 2 | `CONTEXT.md` · the four `summrise-*` skills · the `.claude/skills` symlinks | purely additive; `AGENTS.md` is still authoritative, so a stall here loses nothing |
| 3 | `AGENTS.md` shrunk to the map, its rules moved **verbatim** into the skills | the protocol IS the skills; a gate asserts `AGENTS.md` still names each phase, so a rule cannot be dropped silently |
| 4 | the mutation table into the gates · the 4 archives, `ledger-budget-check` and its wiring deleted | the redesign is complete: audited gate count 23 → 22, no proof lost |

**Landing 1 goes first** because it is the one that would have prevented the incidents this spec exists for, and because
it depends on nothing else.

## 10 · Risks

| risk | disposition |
|---|---|
| Deleting 884 KB is practically irreversible | git keeps it; the triage rule is stated so nothing is judged twice |
| The branch flow slows the loop and changes the release cadence | accepted deliberately — it is the only change that structurally prevents the "red landed on main" class |
| Repo-local skills must be discoverable by EVERY harness in use | §4 names both directories; the symlink pattern is the pack's own, not an invention |
| A standing answer could hide a checkpoint the operator wanted to see | they are listed in one table, in one file, and revocable in one line |
| Moving `AGENTS.md` rules into skills could lose one | the implementation moves them verbatim and a gate asserts `AGENTS.md` still names each phase |

## 11 · Out of scope

- Changing the release **mechanics** (npm/CDN/tag/device). They are correct and hard-won.
- Changing what the gates check. Only where their proofs are written down changes.
- GitHub branch protection. It is the right second line but it is server-side configuration on the operator's account,
  and §6's CI job already gives the enforcement without it.

---

## Appendix · What the measurements in this spec came from

| claim | command |
|---|---|
| ledger is 884,867 B across 4 files | `wc -c docs/agents/design-ledger.md docs/agents/ledger-*.md` |
| 12 tracked files reference it | `git ls-files \| xargs grep -l …` |
| `.claude/skills/*` are symlinks | `ls -l .claude/skills/ \| head -4` |
| `docs/adr/` was pruned | `git log --diff-filter=D -- 'docs/adr/*'` → `25507166` |
| zero merges in 300 | `git log --merges --oneline -300 \| wc -l` |
| `.worktrees` not ignored | `git check-ignore -q .worktrees` → non-zero |
| the proxy accepts branch pushes | `git ls-remote --heads origin` → `refs/heads/release-1.2.463` |
| `core.hooksPath` is `.githooks` | `git config --get core.hooksPath` |
