// panel-sheet-freshness-check.mjs — A PANEL SOURCE CHANGE OWES ITS BUILT SHEET, AND A REBUILD MUST PROVE IT.
//
// WHY THIS EXISTS (round 177 of the standing goal, from a gap the fifteenth exploration measured). FIVE gates
// read the COMMITTED `agent/resources/panel/panel.css` — chrome-stillness, feedback, motion, state-colour and
// stylesheet-hygiene — and NOTHING compared that sheet to the source that generates it:
//
//   * `agent/build.rs` guards by MTIME (it refuses to compile when `panel-react/src` is newer than `resources/panel/`),
//     which catches "source edited, sheet not rebuilt" but not a CONTENT drift — a restored file, a checkout with
//     fresh timestamps, or a rebuild that produced different bytes at the same second;
//   * CI's `panel` job DOES run `npm run build` (ci.yml, "Build") and throws the result away — it never asks
//     whether the tree it built from was consistent;
//   * so a source edit whose build output was never committed leaves five gates measuring a stale sheet, and
//     every one of them passes.
//
// THE CHECK IS A REBUILD, and the assertion is that a rebuild changes NOTHING git tracks. That shape is COPIED
// from `console-assets-check.mjs`, which exists for exactly this reason on the console side and whose own header
// records that the panel's mtime guard was the thing it envied. In the good case this is a no-op on the tree; in
// the bad case the diff it leaves behind IS the fix, which is why a rebuild beats an mtime comparison — round 146
// recorded what an mtime guard costs when a file is RESTORED rather than edited.
//
// IT NEEDS `panel-react` DEPENDENCIES and takes a few seconds; it refuses to run on a dirty tree, so the diff it
// produces cannot be confused with somebody's work.
//
// Run: node scripts/test/panel-sheet-freshness-check.mjs
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const git = (...a) => execFileSync("git", a, { cwd: ROOT, encoding: "utf8" }).trim();

// THE QUESTION IS ABOUT THE ARTIFACT, NOT THE TREE (round 190). This used to refuse on a dirty tree,
// which is the wrong precondition twice over: it cannot be satisfied in CI's `panel` job (which builds
// before its gates) and it says nothing about whether the SHEET is current. Scoped to the built output,
// `git diff --quiet HEAD` answers the real question and tolerates whatever else is in the tree.
const dirty = git("status", "--porcelain", "--", "agent/resources/panel");
if (dirty) {
  console.error(
    `FAIL agent/resources/panel/ was already modified before this check rebuilt it, so a rebuild's diff\n` +
      `could not be told from the change that was already there:\n${dirty}\n` +
      `Commit or revert that first — this check compares against HEAD.`,
  );
  process.exit(1);
}

// THE HOST MUST BE ABLE TO RUN THIS CHECK AT ALL (round 191). This gate REBUILDS the panel, so it
// needs that project s dependencies. `pack-chain` runs every gate via all-gates.bash and installs
// only gateway/ui s — so without this, the check reports a FAILED BUILD for a missing node_modules
// and reddens a job that has nothing to do with the panel. Exit 2 means "this host cannot run me",
// which the runner maps to n/a and does NOT count as a failure. The `panel` job, which installs
// them, is where the real measurement happens.
if (!existsSync(`${ROOT}/agent/resources/panel-react/node_modules`)) {
  console.error(
    "n/a agent/resources/panel-react/node_modules is absent, so this host cannot rebuild the panel. " +
      "CI s `panel` job installs them and runs this check there; nothing is proved or disproved here.",
  );
  process.exit(2);
}

try {
  execFileSync("npm", ["run", "build"], { cwd: `${ROOT}/agent/resources/panel-react`, stdio: "pipe" });
} catch (e) {
  console.error(
    `FAIL the panel build itself failed (rc=${e.status ?? 1}) — that is a different problem, and it should be fixed first:\n` +
      String(e.stdout || "").slice(-500),
  );
  process.exit(1);
}

// SCOPED TO THE ARTIFACT, like the pre-flight above (round 190). This one fired on ANY dirty file —
// including the check s own uncommitted edit — which is a false positive with a message about the sheet.
const after = git("status", "--porcelain", "--", "agent/resources/panel");
if (after) {
  console.error(
    `FAIL a rebuild of the panel changed tracked files, so the COMMITTED sheet is not what the source produces:\n${after}\n` +
      `Five gates read agent/resources/panel/panel.css (chrome-stillness, feedback, motion, state-colour,\n` +
      `stylesheet-hygiene), so they were measuring the OLD sheet. The diff above IS the fix: commit it.\n` +
      `(Leave the rebuild in place — it is the artifact the source says should ship.)`,
  );
  process.exit(1);
}

console.log(
  "panel-sheet-freshness: a rebuild of the panel changes nothing git tracks — the committed sheet matches the source",
);
