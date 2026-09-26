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
// that; a check of the pushed commit's parents can. The flow merges `--no-ff` and this is the backstop for when somebody
// does not.
//
// Run: node scripts/test/main-shape-check.mjs

import { execFileSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { pathToFileURL } from "node:url";

// THE FIXTURE TABLE IS THE PROOF, and it lives here rather than in a table somewhere else because a proof kept away from
// its subject is the arrangement that let this repository's old mutation table grow to 110 KB and rot. Six rows, and the
// two that matter most are the pair no single-direction test can cover: `main` + 1 parent must FAIL and `change/x` +
// 1 parent must PASS, because a rule that blocks ordinary branch work would be reverted within a round.
const SELF_TEST = [
  // [ branch, parent count, expected ok, what it is ]
  ["main", 1, false, "a direct commit on main — the shape this exists to refuse"],
  ["main", 0, false, "a root commit on main"],
  ["main", 2, true, "a --no-ff merge of a change branch: the shape the flow produces"],
  ["main", 3, true, "an octopus merge, which is still a merge"],
  ["change/some-work", 1, true, "ordinary branch work must never be blocked"],
  ["change/some-work", 0, true, "a branch's first commit"],
];

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
    console.error(
      `\nFAIL main-shape: ${failed} of ${SELF_TEST.length} fixture(s) wrong — the decision does not do what it says.`,
    );
    process.exit(1);
  }

  // 2. then the real repository, where CI has a real HEAD. GITHUB_REF_NAME is set by Actions on a push; locally the
  //    branch is asked for directly. The parent count comes from git, never from a guess about the branch.
  const branch =
    process.env.GITHUB_REF_NAME ||
    execFileSync("git", ["branch", "--show-current"], { encoding: "utf8" }).trim() ||
    "(detached)";
  // THE PARENT COUNT COMES FROM THE COMMIT OBJECT, NOT FROM A REV-LIST WALK, AND CI IS WHY (measured 2026-09-26).
  //
  // `actions/checkout@v4` defaults to `fetch-depth: 1`, so the runner holds a SHALLOW clone. Git grafts the shallow boundary
  // commit and `git rev-list --parents -n 1 HEAD` answers with the SHA ALONE — ZERO PARENTS — for a commit that has two. The
  // first version of this gate used exactly that, so it FAILED CI on a `main` that WAS a merge, and NO LOCAL RUN COULD
  // REPRODUCE IT because a developer's clone is full. Measured in `git clone --depth 1`:
  //
  //     git rev-list --parents -n 1 HEAD    ->  '3a54cfa35e97c483fb868ed6c7ef7747790639e7'   (no parents at all)
  //     git cat-file -p HEAD | grep ^parent ->  2 parent line(s)                            (the object always has them)
  //
  // It is this repository's own rule one level down — RUN THE COMMAND THE OTHER END RUNS — and this gate was the other end.
  const parents = execFileSync("git", ["cat-file", "-p", "HEAD"], { encoding: "utf8" })
    .split("\n")
    .filter((l) => l.startsWith("parent ")).length;
  const r = verdict({ branch, parents });
  if (!r.ok) {
    console.error(`FAIL main-shape: ${r.why}`);
    process.exit(1);
  }
  console.log(`main-shape: ${SELF_TEST.length} fixture(s) hold; ${branch} at ${parents} parent(s) — ${r.why}`);
}
