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
  // EVERY GATE THE WORKFLOW INVOKES, not only the ones named *-check (round 184). The two patterns
  // above collected `-check.mjs` and `-check.bash` and the summary called that set "all", while TWELVE
  // gate files CI runs were invisible to it: build-pins, publish-release, release-lib, script-syntax,
  // panel-design-sweep, sweep-judges, smoke-index, smoke-helpers, release-audit, stylesheet-hygiene,
  // scan-dups-check.py and all-gates itself. A census that counts a subset is the defect this gate
  // exists to catch, one level up.
  // THE EXTENSION IS STRIPPED, because the prose names gates by their BASE NAME — "beginning with
  // `contract-vocabulary-check`, `one-derivation-check` …" (AGENTS.md). The first version of this
  // widening kept the extension and reported EIGHT gates as unnamed that the very file it checks names
  // in a paragraph — an instrument wrong before its subject, which is this suite s oldest lesson.
  ...[...ci.matchAll(/(?:node|bash|python3)\s+scripts\/test\/([A-Za-z0-9._-]+)/g)].map((m) =>
    m[1].replace(/\.(mjs|bash|py)$/, ""),
  ),
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
