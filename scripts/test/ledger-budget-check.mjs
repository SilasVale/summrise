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
  const archiveBytes = Buffer.byteLength(archive, "utf8");
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
console.log(`ledger-budget-check: ok — the instruction file fits (${bytes} of ${CEILING} bytes — the ENFORCED ceiling; the harness truncates at ${HARNESS_BUDGET}) and the long form is in ${ARCHIVE}`);
