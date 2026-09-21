// numbered-claims-check.mjs — A GATE THAT RUNS MUST BE A GATE SOMEBODY NAMED.
//
// WHY THIS EXISTS (round 152 of the standing goal). AGENTS.md said "THE STANDING OBJECTIVE'S TEN GATES" while the objective
// was at ten, and a round later there were twelve — the same drift the file itself records for the sweep's check count:
// "the NUMBER is what drifts when a round adds a case without updating this cell". A count in prose cannot be checked; a
// NAME can. So this checks the part that can be: every gate the workflow invokes is named somewhere an operator reads —
// AGENTS.md or the design ledger — and the count is left to whoever wants to count.
//
// WHAT IT DOES NOT CHECK: the numbers (deliberately, per the paragraph above), the mutations (the audit does that), or the
// shell gates' internals.
//
// Run: node scripts/test/numbered-claims-check.mjs
import { readFileSync } from "node:fs";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const ci = readFileSync(`${ROOT}/.github/workflows/ci.yml`, "utf8");
const docs = ["AGENTS.md", "docs/agents/design-ledger.md", "docs/agents/inventory.md"]
  .map((f) => readFileSync(`${ROOT}/${f}`, "utf8"))
  .join("\n");

const wired = new Set([
  ...[...ci.matchAll(/node scripts\/test\/([a-z0-9-]+-check)\.mjs/g)].map((m) => m[1]),
  ...[...ci.matchAll(/bash scripts\/test\/([a-z0-9-]+-check)\.bash/g)].map((m) => m[1]),
]);

if (wired.size < 20) {
  console.error(`FAIL read only ${wired.size} gate(s) from the workflow — the tree moved, so this proves nothing`);
  process.exit(1);
}
const unnamed = [...wired].filter((g) => !docs.includes(g));
if (unnamed.length) {
  console.error(
    `numbered-claims: ${unnamed.length} gate(s) run by CI and named NOWHERE an operator reads:\n  ` +
      unnamed.map((g) => `${g} — invoked by the workflow, absent from AGENTS.md, the ledger and the inventory`).join("\n  ") +
      `\n\nA gate nobody documented is one nobody can run by hand, and this file's own rule is that a count drifts while a\n` +
      `name does not.`,
  );
  process.exit(1);
}
console.log(`numbered-claims: all ${wired.size} gate(s) the workflow invokes are named in AGENTS.md or the ledger`);
