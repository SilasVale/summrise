#!/usr/bin/env node
// ledger-budget-check — THE INSTRUCTION FILE HAS TO STAY SMALL ENOUGH TO BE READ WHOLE.
//
// MEASURED, round 67: `AGENTS.md` reached 65,366 bytes, and the workspace harness TRUNCATES an instruction file at
// 65,536 — it dropped the tail, which is the Release and Agent-layout sections. Nothing failed. Nothing warned
// except a one-line note in a system reminder, and the operator's own instructions were being silently cut off at
// the end of the file. An append-only ledger cannot be an instruction file: the evidence has to live somewhere it
// can be long, and what stays here has to fit.
//
// SO THE RULE IS STRUCTURAL, not a size alone: `AGENTS.md` holds the OPERATIONAL sections (build, test, which gate
// each mutation must fail, committing, release, layout) and a POINTER; the round-by-round narratives live in
// `docs/agents/design-ledger.md`. This check fails if a narrative grows back into the instruction file, if the file
// approaches the budget again, or if the archive is emptied — because "pruned" must mean MOVED, not deleted.

import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const INSTRUCTIONS = "AGENTS.md";
const ARCHIVE = "docs/agents/design-ledger.md";
// THE LOOKUP TABLES MOVED OUT (round 49): "Looking for one thing" (302 KB) and the gate table (91 KB)
// were 59% of the archive and are TABLES rather than narrative, so they live in an appendix. The
// index at the top of the archive still NAMES their sections, so a title resolves against
// EITHER file — and a ceiling on each is what stops either growing back into the other.
const APPENDIX = "docs/agents/ledger-appendix.md";
// THE MUTATION TABLE IS ITS OWN ARCHIVE NOW (round 107): it is what rounds APPEND to, its host had 3,861 bytes of
// headroom, and a ceiling per file is what stops either half growing back into the other.
const MUTATIONS = "docs/agents/ledger-mutations.md";
const ARCHIVE_CEILING = 400_000;
const ARCHIVE_FLOOR_APX = 100_000;
// The harness truncates at 65,536 and NAME it here, so the number in the failure is the real one.
const HARNESS_BUDGET = 65536;
const CEILING = 48_000;
const ARCHIVE_FLOOR = 20_000;

const failures = [];
const text = readFileSync(join(ROOT, INSTRUCTIONS), "utf8");
const bytes = Buffer.byteLength(text, "utf8");

if (bytes > CEILING) {
  failures.push(`${INSTRUCTIONS} is ${bytes} bytes and the ceiling is ${CEILING} — the workspace harness truncates at ${HARNESS_BUDGET}, silently dropping the END of the file, which is where the release steps live (round 67)`);
}
for (const must of ["## Build", "## Test", "### Which gates have been PROVEN to bite", "## Committing", "## Release", "## Agent layout", "## The design ledger"]) {
  if (!text.includes(must)) failures.push(`${INSTRUCTIONS} no longer has "${must}" — the operational half is what stays here`);
}
// THE STRUCTURAL RULE: one `###` section is allowed (the gate table); anything else is a narrative and belongs in
// the ledger. Headings inside fenced code blocks are not headings.
const outsideCode = text.replace(/```[\s\S]*?```/g, "");
const subsections = [...outsideCode.matchAll(/^### (.+)$/gm)].map((m) => m[1]);
for (const s of subsections) {
  if (!s.startsWith("Which gates have been PROVEN to bite")) {
    failures.push(`"${s}" is a narrative section in ${INSTRUCTIONS} — that belongs in ${ARCHIVE}, which is where the long form is allowed to be long`);
  }
}

if (!existsSync(join(ROOT, ARCHIVE))) {
  failures.push(`${ARCHIVE} does not exist — the narratives were DELETED rather than moved, and the evidence is gone`);
} else {
  const archive = readFileSync(join(ROOT, ARCHIVE), "utf8");
// THE INDEX NAMES SECTION TITLES, AND A RENAMED SECTION SILENTLY BREAKS IT (round 186).
// There is no index gate anywhere else, and this is the file whose whole problem was findability.
// A title is an exact string, so this is checkable — unlike the `ci.yml:N` citations, which are not,
// because a line number moves when anything above it is inserted and a title does not.
// THE INDEX IS DELIMITED, NOT GUESSED (round 49). It used to be "everything before the first `### `",
// which broke the moment a section that HELD that first `### ` was moved out: the region then swallowed
// narrative containing backticked code (`SEQUENCE=...`, `SummriseIntegrity.ps1`) and the check reported
// nine section titles that were never titles. An explicit marker pair says where the index is, so the
// check cannot be broken by moving a section.
const idxStart = archive.indexOf("<!-- ledger-index:start -->");
const idxEnd = archive.indexOf("<!-- ledger-index:end -->");
if (idxStart < 0 || idxEnd < 0) failures.push(`${ARCHIVE} has no ledger-index markers — the index is delimited rather than guessed, and the check cannot read it without them`);
const index = idxStart >= 0 && idxEnd > idxStart ? archive.slice(idxStart, idxEnd) : "";
const appendix = existsSync(join(ROOT, APPENDIX)) ? readFileSync(join(ROOT, APPENDIX), "utf8") : "";
const mutations = existsSync(join(ROOT, MUTATIONS)) ? readFileSync(join(ROOT, MUTATIONS), "utf8") : "";
if (!mutations) failures.push(`${MUTATIONS} does not exist — the mutation table was DELETED rather than moved`);
else if (Buffer.byteLength(mutations, "utf8") > ARCHIVE_CEILING) failures.push(`${MUTATIONS} is ${Buffer.byteLength(mutations, "utf8")} bytes and the ceiling is ${ARCHIVE_CEILING} — a table that outgrows its own file is the round-49 problem again`);
if (!appendix) failures.push(`${APPENDIX} does not exist — the lookup tables were DELETED rather than moved`);
else if (Buffer.byteLength(appendix, "utf8") < ARCHIVE_FLOOR_APX) failures.push(`${APPENDIX} is under ${ARCHIVE_FLOOR_APX} bytes — the tables were pruned rather than moved`);
// A TITLE RESOLVES ACROSS ALL THREE ARCHIVES (round 107): the index at the top of the ledger names sections wherever
// they now live, and this check is what turns "a pointer nobody tests" into a failure. It caught this round's OWN split —
// the index named a section that had just moved into the third file, which is the gate doing exactly its job.
const headings = [...(archive + "\n" + appendix + "\n" + mutations).matchAll(/^#{2,3} (.+)$/gm)].map((m) => m[1].trim());
const named = [...index.matchAll(/`([^`]+)`/g)]
  .map((m) => m[1])
  .filter((s) => /^[A-Z0-9"]/.test(s) && s.length > 18);
const brokenIndex = named.filter((n) => !headings.some((h) => h.startsWith(n)));
if (brokenIndex.length) {
  failures.push(
    `the ledger index names ${brokenIndex.length} section(s) that do not exist — a renamed section broke ` +
      `the pointer to it: ${brokenIndex.join(", ")}`,
  );
}
  const archiveBytes = existsSync(join(ROOT, ARCHIVE)) ? Buffer.byteLength(readFileSync(join(ROOT, ARCHIVE), "utf8"), "utf8") : 0;
  if (archiveBytes < ARCHIVE_FLOOR) {
    failures.push(`${ARCHIVE} is only ${archiveBytes} bytes — an archive under the floor means the prune deleted instead of moving`);
  }
  const sections = (archive.match(/^### /gm) || []).length;
  if (sections < 8) failures.push(`${ARCHIVE} holds ${sections} section(s) — the ledger is where the rounds live, and it is thinning out`);
  console.log(`note: ${INSTRUCTIONS} ${bytes} B of ${HARNESS_BUDGET} · ${ARCHIVE} ${archiveBytes} B, ${sections} sections`);
}

if (failures.length) {
  for (const f of failures) console.error(`ledger-budget-check: ${f}`);
  process.exit(1);
}
const archiveBytes = existsSync(join(ROOT, ARCHIVE)) ? Buffer.byteLength(readFileSync(join(ROOT, ARCHIVE), "utf8"), "utf8") : 0;
if (archiveBytes > ARCHIVE_CEILING) failures.push(`${ARCHIVE} is ${archiveBytes} bytes and the ceiling is ${ARCHIVE_CEILING} — an append-only record stops being readable long before it stops being writable, and this file was 660 KB when the lookup tables were split out (round 49)`);
console.log(`ledger-budget-check: ok — the instruction file fits (${bytes} of ${CEILING} bytes — the ENFORCED ceiling; the harness truncates at ${HARNESS_BUDGET}) and the long form is in ${ARCHIVE} (${archiveBytes} B of ${ARCHIVE_CEILING}) with the tables in ${APPENDIX}`);
