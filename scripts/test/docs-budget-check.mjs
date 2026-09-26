#!/usr/bin/env node
// docs-budget-check — THE INSTRUCTION FILE HAS TO STAY SMALL ENOUGH TO BE READ WHOLE, AND THE GLOSSARY HAS TO STAY A
// GLOSSARY.
//
// MEASURED, round 67: `AGENTS.md` reached 65,366 bytes, and the workspace harness TRUNCATES an instruction file at 65,536 —
// it dropped the tail, which is the Release and Agent-layout sections. Nothing failed, nothing warned except a one-line note
// in a system reminder, and the operator's own instructions were being silently cut off at the end of the file.
//
// SO THE RULE IS STRUCTURAL, not a size alone: `AGENTS.md` holds the OPERATIONAL sections (build, test, which gate each
// mutation must fail, committing, release, layout) and POINTERS; the reference tables live in `docs/agents/`. This check
// fails if a narrative grows back into the instruction file, or if the file approaches the budget again.
//
// RENAMED FROM `ledger-budget-check` IN ROUND 5, AND THE HALF THAT WENT IS THE POINT. Until then this gate also budgeted
// `docs/agents/design-ledger.md`, `ledger-early-rounds.md` and an index that resolved a section title across all four
// archives. **Those two files are deleted, and the round-narrative process with them** — the operator retired it, because
// the loop's output had drifted into prose ABOUT the work rather than the work. What remains here is what was never about
// the process: the instruction file's ceiling, the two REFERENCE tables' floors and ceilings, and the glossary's.
//
// A RENAME RATHER THAN A QUIET EDIT, because a gate called `ledger-budget-check` that no longer budgets a ledger is a name
// that lies, and this repository treats a name that lies as the defect it is.

import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const INSTRUCTIONS = "AGENTS.md";
// THERE ARE NO REFERENCE TABLES ANY MORE (landing 4b). The mutation table's rows moved INTO the gates they name, and
// the appendix's findings are in git history. What this gate budgets now is the instruction file and the glossary —
// which is all that is left of the long form, and all that should be.
// The harness truncates at 65,536 and NAME it here, so the number in the failure is the real one.
const GLOSSARY = "CONTEXT.md";
const GLOSSARY_CEILING = 12_000;
const HARNESS_BUDGET = 65536;
const CEILING = 48_000;

const failures = [];
const text = readFileSync(join(ROOT, INSTRUCTIONS), "utf8");
const bytes = Buffer.byteLength(text, "utf8");

if (bytes > CEILING) {
  failures.push(`${INSTRUCTIONS} is ${bytes} bytes and the ceiling is ${CEILING} — the workspace harness truncates at ${HARNESS_BUDGET}, silently dropping the END of the file, which is where the release steps live (round 67)`);
}
for (const must of ["## Build", "## Test", "### Which gates have been PROVEN to bite", "## The vocabulary", "## Committing", "## Release", "## Agent layout"]) {
  if (!text.includes(must)) failures.push(`${INSTRUCTIONS} no longer has "${must}" — the operational half is what stays here`);
}
// THE STRUCTURAL RULE: one `###` section is allowed (the gate table); anything else is a narrative, and a narrative in the
// instruction file is what this gate exists to refuse. Headings inside fenced code blocks are not headings.
const outsideCode = text.replace(/```[\s\S]*?```/g, "");
const subsections = [...outsideCode.matchAll(/^### (.+)$/gm)].map((m) => m[1]);
for (const s of subsections) {
  if (!s.startsWith("Which gates have been PROVEN to bite")) {
    failures.push(`"${s}" is a narrative section in ${INSTRUCTIONS} — the instruction file holds what changes what you DO, and a narrative belongs in a commit message or an ADR`);
  }
}

const glossary = existsSync(join(ROOT, GLOSSARY)) ? readFileSync(join(ROOT, GLOSSARY), "utf8") : "";
if (!glossary) failures.push(`${GLOSSARY} does not exist — the glossary was DELETED rather than moved, and every surface that names a thing now has nowhere to check the word`);
else if (Buffer.byteLength(glossary, "utf8") > GLOSSARY_CEILING) failures.push(`${GLOSSARY} is ${Buffer.byteLength(glossary, "utf8")} bytes and the ceiling is ${GLOSSARY_CEILING} — it is a GLOSSARY: terms, not a rulebook, and not a place for implementation decisions`);

if (failures.length) {
  for (const f of failures) console.error(`docs-budget-check: ${f}`);
  process.exit(1);
}
console.log(
  `docs-budget-check: ok — the instruction file fits (${bytes} of ${CEILING} bytes — the ENFORCED ceiling; the harness ` +
    `truncates at ${HARNESS_BUDGET}), the glossary is ${Buffer.byteLength(glossary, "utf8")} B of ${GLOSSARY_CEILING}, and ` +
    `the gates carry their own proofs`,
);
