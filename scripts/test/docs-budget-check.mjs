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
// THE TWO REFERENCE TABLES THAT REMAIN. They are NOT the ledger: nothing appends to them as a round ritual, and their
// content has a stated destination (the mutation table's rows move into the gates they name — landing 4b).
const APPENDIX = "docs/agents/ledger-appendix.md";
const MUTATIONS = "docs/agents/ledger-mutations.md";
const GLOSSARY = "CONTEXT.md";
const ARCHIVE_CEILING = 400_000;
const ARCHIVE_FLOOR_APX = 100_000;
// A GLOSSARY THAT GROWS INTO A RULEBOOK HAS STOPPED BEING A GLOSSARY, and the skill that prescribes the file says so:
// "a glossary and nothing else... totally devoid of implementation details". The ceiling is what makes that checkable
// rather than a matter of intent.
const GLOSSARY_CEILING = 12_000;
// The harness truncates at 65,536 and NAME it here, so the number in the failure is the real one.
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

const appendix = existsSync(join(ROOT, APPENDIX)) ? readFileSync(join(ROOT, APPENDIX), "utf8") : "";
const mutations = existsSync(join(ROOT, MUTATIONS)) ? readFileSync(join(ROOT, MUTATIONS), "utf8") : "";
if (!mutations) failures.push(`${MUTATIONS} does not exist — the mutation table was DELETED rather than moved`);
else if (Buffer.byteLength(mutations, "utf8") > ARCHIVE_CEILING) failures.push(`${MUTATIONS} is ${Buffer.byteLength(mutations, "utf8")} bytes and the ceiling is ${ARCHIVE_CEILING} — a table that outgrows its own file is the round-49 problem again`);
if (!appendix) failures.push(`${APPENDIX} does not exist — the lookup tables were DELETED rather than moved`);
else if (Buffer.byteLength(appendix, "utf8") < ARCHIVE_FLOOR_APX) failures.push(`${APPENDIX} is under ${ARCHIVE_FLOOR_APX} bytes — the tables were pruned rather than moved`);
else if (Buffer.byteLength(appendix, "utf8") > ARCHIVE_CEILING) failures.push(`${APPENDIX} is ${Buffer.byteLength(appendix, "utf8")} bytes and the ceiling is ${ARCHIVE_CEILING} — a ceiling on each is what stops either growing back into the other`);

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
    `the reference tables are in ${APPENDIX} and ${MUTATIONS}`,
);
