#!/usr/bin/env node
// skill-frontmatter-check — EVERY SKILL IN .agents/skills/ MUST BE ONE THE AGENT CAN ACTUALLY LOAD.
//
// WHY THIS EXISTS (round 170). This repository ships 35 skills and a malformed one fails SILENTLY: a skill whose `---` block is
// broken, or whose `name:`/`description:` is missing, does not error — IT SIMPLY NEVER APPEARS IN THE AGENT'S CATALOG, and every
// later session works without it. Measured in round 170: 35 of 35 were correct and NOTHING checked them (`grep -rln
// "agents/skills|SKILL.md" scripts/test/ gateway/test/` matched no file). Same shape as rounds 146-151 (a route the header never
// named) and 134 (a mirror direction nobody checked): the file is present, and the reader is never told.
//
// THE MUTATION THAT MUST FAIL IT: delete the closing `---` of any SKILL.md, or empty its description. The count of catalog-visible
// skills drops with no other symptom — which is the whole argument for a gate.

import { readFileSync, readdirSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const SKILLS = join(ROOT, ".agents/skills");

if (!existsSync(SKILLS)) {
  console.error("skill-frontmatter: .agents/skills/ is not here — this check cannot pass vacuously.");
  process.exit(1);
}

const problems = [];
let checked = 0;

for (const dir of readdirSync(SKILLS).sort()) {
  const file = join(SKILLS, dir, "SKILL.md");
  if (!existsSync(file)) {
    // a directory WITH skill-shaped content but no SKILL.md is a skill the agent cannot load at all
    const entries = readdirSync(join(SKILLS, dir));
    if (entries.length) problems.push(`${dir}: no SKILL.md (has ${entries.length} file(s))`);
    continue;
  }
  checked++;
  const text = readFileSync(file, "utf8");
  const m = /^---\r?\n([\s\S]*?)\r?\n---\r?\n/.exec(text);
  if (!m) {
    problems.push(`${dir}: no frontmatter block — the agent cannot see this skill`);
    continue;
  }
  const fm = m[1];
  const name = /^name:\s*(\S.*)$/m.exec(fm);
  if (!name) problems.push(`${dir}: frontmatter has no name:`);
  else if (name[1].trim().replace(/^["']|["']$/g, "") !== dir) {
    problems.push(`${dir}: name: says "${name[1].trim()}" — the directory is what the catalog lists`);
  }
  const desc = /^description:\s*(.+)$/m.exec(fm);
  if (!desc) problems.push(`${dir}: frontmatter has no description — this is the pointer that decides when it fires`);
  else if (desc[1].trim().replace(/^["']|["']$/g, "").length < 20) {
    problems.push(`${dir}: description is ${desc[1].trim().length} chars — too short to trigger on`);
  }
}

if (checked === 0) {
  console.error("skill-frontmatter: found no SKILL.md at all — the layout moved, and this check cannot pass vacuously.");
  process.exit(1);
}

if (problems.length) {
  console.error(`skill-frontmatter: ${problems.length} skill(s) the agent cannot load:`);
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}

console.log(`skill-frontmatter: ok — ${checked} skill(s) all carry a loadable frontmatter block`);
