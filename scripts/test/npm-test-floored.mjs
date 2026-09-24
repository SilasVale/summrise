// npm-test-floored.mjs — A TEST STEP THAT RAN ZERO TESTS IS NOT A PASSING TEST STEP.
//
// WHY THIS EXISTS (round 192 of the standing goal, from the seventeenth exploration's first finding).
// Measured on Node v24.20.0: `node --test` in a directory with no test files prints `ℹ tests 0` /
// `ℹ pass 0` and **EXITS 0**. Six CI steps run `npm test` in packages whose suite is `node --test` — and
// every one of them would go green if its test files were renamed, emptied, or never matched by the glob.
//
// THE REPO ALREADY CARRIES THIS RULE, one level down: `all-gates.bash` FLOOR 40, `script-syntax.bash`
// FLOOR 20 ("A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN"), `console-smoke-check.mjs` floor 4, and
// all-gates fails a gate that exits 0 with NO OUTPUT (round 175). Those floors live in the GATE SCRIPTS;
// nothing floored the JOB STEPS. This is that rule at the step.
//
// IT REPLACES `npm test` IN THE STEP rather than running beside it, so no suite executes twice.
//
// THE REPORTERS DIFFER, and AGENTS.md documents both: `node --test` prints `ℹ pass N` on Node 24 and
// `# pass N` on Node 20; vitest prints `Tests  N passed`. All three are read, and a suite whose output
// carries NO count at all is a FAILURE — that is the vacuity this exists to catch, not an excuse.
//
// Run: node scripts/test/npm-test-floored.mjs   (from the package directory CI runs `npm test` in)
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";

// IT IS A STEP REPLACEMENT, NOT A GATE (round 192). Living in scripts/test/ means all-gates.bash finds it and
// runs it from the REPO ROOT, where there is no package.json — so it must say "this host cannot run me"
// (exit 2, which the runner maps to n/a) rather than failing a run it was never meant to be part of. The
// real measurement happens in the CI step, where the working directory IS the package.
const cwd = process.cwd();
if (!existsSync(`${cwd}/package.json`)) {
  console.error(`n/a ${cwd.replace(/^.*\/summrise\/?/, "") || "."} has no package.json — this wraps a package s ` +
    `test step, so there is nothing to run here. CI invokes it from the package directory.`);
  process.exit(2);
}
let out = "";
let status = 0;
try {
  out = execFileSync("npm", ["test"], { cwd, encoding: "utf8", stdio: "pipe" });
} catch (e) {
  // A failing suite is reported by the suite, with its own message. Do not mask it.
  process.stdout.write(String(e.stdout || ""));
  process.stderr.write(String(e.stderr || ""));
  process.exit(typeof e.status === "number" ? e.status : 1);
}
process.stdout.write(out);

const tests = out.match(/^[ℹ#]\s*tests\s+(\d+)/m);
const pass = out.match(/^[ℹ#]\s*pass\s+(\d+)/m);
const vitest = out.match(/^\s*Tests\s+(\d+)\s+passed/m);

const counted = pass ? Number(pass[1]) : vitest ? Number(vitest[1]) : tests ? Number(tests[1]) : null;

if (counted === null) {
  console.error(
    `FAIL ${cwd}: the suite exited 0 but its output carried no test count at all, so this step proved ` +
      `nothing. Expected "ℹ pass N" (node --test, Node 24), "# pass N" (Node 20) or "Tests  N passed" (vitest).`,
  );
  process.exit(1);
}
if (counted < 1) {
  console.error(
    `FAIL ${cwd}: the suite ran ${counted} test(s) and exited 0. A suite that tests nothing passes ` +
      `everything — check that its test files still exist and that the runner still matches them.`,
  );
  process.exit(1);
}

console.log(`npm-test-floored: ${counted} test(s) ran in ${cwd.replace(/^.*\/summrise\//, "")}`);
process.exit(status);
