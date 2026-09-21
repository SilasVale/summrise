// console-derivation-check.mjs — THE CONSOLE'S SIDE OF "ONE FACT, ONE DERIVATION".
//
// WHY THIS EXISTS (round 173 of the standing goal). The PANEL has had `one-derivation-check` since round 78: it fails, by file
// and line, when a module other than `lib/path.ts` turns a command's ending into a state. The CONSOLE — the other front end,
// with its own state vocabulary (`dot`, `sig-dot`, `prov-lane`) — had nothing of the kind, and round 172 found what that
// costs: the rule "a provider prefix's trailing slash does not count" was written out NINE times in `Models.tsx` and
// `lane.ts`, with two different regexes, so several of the nine already disagreed about what a prefix IS.
//
// THE RULES, each with the round that paid for it:
//
//   1. `barePrefix` (the trailing-slash strip) is written ONCE, in `lib/lane.ts` (round 172).
//
// WHAT IT DOES NOT CHECK: the panel (its own gate), the Rust, or the shape of the console's marks (`console-marks-check`).
//
// Run: node scripts/test/console-derivation-check.mjs
import { readFileSync } from "node:fs";
import { relative } from "node:path";
import { execFileSync } from "node:child_process";
import { decomment } from "./lib/decomment.mjs";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const UI = "gateway/ui/src";

/** The files that own a derivation. Everything else must ask them. */
const HOMES = { "lib/lane.ts": "the bare provider prefix (round 172)" };

const files = execFileSync("git", ["ls-files", UI], { cwd: ROOT, encoding: "utf8" })
  .split("\n")
  .filter((f) => /\.(ts|tsx)$/.test(f) && !f.includes(".test."));

const offenders = [];
let scanned = 0;
for (const rel of files) {
  const short = relative(UI, rel);
  scanned++;
  if (HOMES[short]) continue;
  decomment(readFileSync(`${ROOT}/${rel}`, "utf8"))
    .split("\n")
    .forEach((line, i) => {
      // the trailing-slash strip, in either of the two spellings the console used
      if (/replace\(\/\\\/\+\?\$\/, *""\)/.test(line)) {
        offenders.push(`${short}:${i + 1} strips a trailing slash by hand — ${HOMES["lib/lane.ts"]}`);
      }
    });
}

if (scanned < 15) {
  console.error(`FAIL scanned only ${scanned} console module(s) — the tree moved, so this proves nothing`);
  process.exit(1);
}
if (offenders.length) {
  console.error(
    `console-derivation: ${offenders.length} second derivation(s):\n  ` +
      offenders.join("\n  ") +
      "\n\nThe console's derivations live in lib/, in ONE place each. Round 172 found this rule written nine times with two" +
      "\n" +
      `different behaviours; a tenth copy is how they drift again.`,
  );
  process.exit(1);
}
console.log(`console-derivation: ${scanned} console module(s) scanned, and the prefix rule lives only in lib/lane.ts`);
