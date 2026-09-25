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
// BOTH WORKFLOWS, NOT ONE (round 199). This read ci.yml alone while its summary said "the workflow" —
// singular, so it was honest about its scope and still incomplete: release.yml invokes a gate too, and it
// sat outside the census entirely. Found in round 192 while wrapping that step for the test-count floor,
// which is the shape of the miss — a rule written for six steps did not notice the seventh.
const ci = ["ci.yml", "release.yml"]
  .map((f) => readFileSync(`${ROOT}/.github/workflows/${f}`, "utf8"))
  .join("\n")
  // THE PATH PREFIX IS STRIPPED FIRST, and this census learned it the way all-gates.bash did in round 170:
  // the workflows spell their invocations `node ${{ github.workspace }}/scripts/test/x.mjs`, so a pattern
  // anchored on `node scripts/test/` sees only the handful written without the prefix. Nine invocations
  // across the two workflows were invisible here — including the one this round was testing for, which is
  // how it was found: a mutation that should have failed the census passed it.
  .replace(/\$\{\{ github\.workspace \}\}\//g, "");
// THE TWO CURATED DOCUMENTS, WHICH IS WHAT THE SUMMARY HAS ALWAYS CLAIMED (round 200). This also read
// `docs/agents/inventory.md` — a 70 KB checkpoint last maintained hundreds of rounds ago — so any gate name
// mentioned once inside its narrative counted as documented. The eighteenth exploration measured the
// consequence: `proxy-cors-parity-check` and `proxy-timeout-parity-check` were reachable ONLY through it.
// They are named in the ledger now, and this asks the question its own message states: AGENTS.md or the ledger.
// THE APPENDIX IS DOCUMENTED SURFACE TOO (round 49): the lookup tables moved out of the ledger's
// narrative and AGENTS.md POINTS AT THEM, so a gate named only there is named where an operator
// reads — and this list is the definition of "where an operator reads".
const docs = ["AGENTS.md", "docs/agents/design-ledger.md", "docs/agents/ledger-appendix.md"]
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
      unnamed.map((g) => `${g} — invoked by the workflow, absent from AGENTS.md and the ledger`).join("\n  ") +
      `\n\nA gate nobody documented is one nobody can run by hand, and this file's own rule is that a count drifts while a\n` +
      `name does not.`,
  );
  process.exit(1);
}
console.log(`numbered-claims: all ${wired.size} gate(s) the workflows invoke are named in AGENTS.md or the ledger`);
