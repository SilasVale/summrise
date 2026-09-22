// sweep-fixture-dupes-check.mjs — A FIXTURE TABLE MUST NOT SAY ONE THING TWICE.
//
// WHY THIS EXISTS (round 60 of the standing goal). Round 57 added `/api/admin/public` to the console sweep's inline
// `API` table to give the Models page something to render; round 58 measured that `prov-dot` STILL rendered 0; round 59
// found why: the key was ALREADY in that object, as a later stub, and **in a JS object literal the last key wins** — so
// the real body never reached the page and the page rendered its failure banner instead. Nothing anywhere looks for
// that: the panel's harness has a fixture check, and the console's table is inline in the sweep with none.
//
// A duplicate key is the two-copies-of-one-fact defect this objective exists to remove, and in a fixture it is worse
// than in code because it is SILENT: the page simply renders a different world than the one the table describes, and
// every other axis stays green because there is nothing to measure.
//
// WHAT IT READS: the top-level keys of each sweep's `const API = { … }` table. Entries are written at two spaces of
// indentation as quoted keys, which is what makes a top-level scan reliable without parsing JS; the FLOOR below refuses
// to pass if it found fewer than ten keys, so a moved or renamed table is reported as "this proves nothing" rather
// than as a clean run.
//
// Run: node scripts/test/sweep-fixture-dupes-check.mjs
import { decomment } from "./lib/decomment.mjs";
import { readFileSync } from "node:fs";

// THE PAYLOAD MODULES, NOT THE EMITTERS (round 268). Each sweep's inline `const API = {...}` fixture table now lives in
// the device-side program it belongs to (`agent/scripts/lib/sweep/<ui>-run.cjs`) — the emitters no longer hold markup at
// all. Scanning the old paths found ZERO tables and reported "no duplicates" across nothing, which is the shape this
// file's own floor (below) exists to refuse.
const SWEEPS = [
  "agent/scripts/lib/sweep/console-run.cjs",
  "agent/scripts/lib/sweep/landing-run.cjs",
  "agent/scripts/lib/sweep/panel-run.cjs",
];

let fail = 0;
let totalKeys = 0;

for (const path of SWEEPS) {
  const src = readFileSync(new URL("../../" + path, import.meta.url), "utf8");
  const start = src.indexOf("const API = {");
  if (start < 0) {
    console.log(`skip ${path}: no inline API table`);
    continue;
  }
  const end = src.indexOf("\n};", start);
  if (end < 0) {
    console.error(`FAIL ${path}: the API table is not closed by a top-level }; — this scan proves nothing`);
    fail++;
    continue;
  }
  const body = src.slice(start, end);
  const seen = new Map();
  for (const m of body.matchAll(/^ {2}(['"])([^'"]+)\1:/gm)) {
    const key = m[2];
    const line = body.slice(0, m.index).split("\n").length;
    if (seen.has(key)) {
      console.error(
        `FAIL ${path}:${line}: fixture key ${key} appears TWICE (first at line ${seen.get(key)}) — in a JS object the ` +
          `LAST one wins, so the earlier body never reaches the page. This is how prov-dot rendered 0 for three rounds.`,
      );
      fail++;
    } else {
      seen.set(key, line);
    }
  }
  if (seen.size < 10) {
    console.error(`FAIL ${path}: read only ${seen.size} fixture key(s) — the table moved, so this proves nothing`);
    fail++;
    continue;
  }
  totalKeys += seen.size;
  console.log(`ok   ${path}: ${seen.size} fixture key(s), none repeated`);
}

if (fail) {
  console.error(`\nsweep-fixture-dupes: ${fail} problem(s)`);
  process.exit(1);
}
// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN (round 268). The per-file floor above cannot see this: every path can
// legitimately `skip` (a UI with no inline table), so the whole run can print "0 fixture key(s) … no duplicates" and
// exit 0 — which is what happened for one round after the console's table moved into its payload module while this
// list still named the emitters. It now refuses to report success having read nothing.
if (totalKeys === 0) {
  console.error(`\nsweep-fixture-dupes: read 0 fixture key(s) across ${SWEEPS.length} sweep(s) — every path was skipped, so this proves nothing`);
  process.exit(1);
}
console.log(`\nsweep-fixture-dupes: ${totalKeys} fixture key(s) across ${SWEEPS.length} sweep(s), no duplicates`);
