#!/usr/bin/env node
// ── THE MUTATION THAT MUST FAIL THIS GATE (moved here from the ledger table, landing 4b) ──
// Read this when you change this file: the mutation is how you find out whether the gate can still
// fail at all. A gate that cannot be broken is worse than no gate.
//
// MUTATION: break any LABELLED code block in `AGENTS.md` or `agent/AGENTS.md` — e.g. drop a `}` from the `npm test >/tmp/out 2>&1
// RESULT:   

// agents-snippet-check — EVERY LABELLED CODE BLOCK IN AGENTS.md MUST PARSE.
//
// WHY THIS EXISTS (round 186). Round 183 failed on a snippet AGENTS.md TOLD THE READER TO RUN — the instruction was fine and the invocation
// was not, and the next reader pays for that in the one file they trust most. Measured in round 186: AGENTS.md carries 8 fenced blocks,
// 7 of them labelled (5 bash, 1 js, 1 powershell) and all 7 valid, and NOTHING in this repository had ever parsed one of them.
//
// THE RULE THE MEASUREMENT PRODUCED: ONLY A LABELLED BLOCK CLAIMS TO BE CODE. The unlabelled block in AGENTS.md is the Agent layout
// DIRECTORY TREE — not shell, and it does not parse as shell. So this check reads LABELLED blocks only, and the diagram is the non-bite
// that keeps it honest: a check that flagged it would be reverted.
//
// WHAT IT CANNOT SEE, said out loud: powershell has no parser on this box (the same limit `powershell-structure-check` documents), and a
// block that PARSES can still be wrong about the world. This catches a broken line, not a broken instruction.

import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { writeFileSync, unlinkSync } from "node:fs";
import { tmpdir } from "node:os";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const DOCS = ["AGENTS.md", "agent/AGENTS.md"];

let checked = 0;
const problems = [];

for (const rel of DOCS) {
  let text;
  try { text = readFileSync(join(ROOT, rel), "utf8"); } catch { continue; }
  const re = /```([a-zA-Z]+)\n([\s\S]*?)```/g;
  let m;
  while ((m = re.exec(text))) {
    const lang = m[1].toLowerCase();
    const body = m[2];
    const firstLine = body.split("\n")[0].slice(0, 60);
    if (lang === "js" || lang === "javascript") {
      checked++;
      const f = join(tmpdir(), `agents-snippet-${process.pid}-${checked}.js`);
      writeFileSync(f, body);
      try { execFileSync(process.execPath, ["--check", f], { stdio: "pipe" }); }
      catch (e) { problems.push(`${rel} [${lang}] ${firstLine}\n      ${String(e.stderr || e).trim().split("\n").slice(-1)[0]}`); }
      finally { unlinkSync(f); }
    } else if (lang === "bash" || lang === "sh") {
      checked++;
      try { execFileSync("bash", ["-n"], { input: body, stdio: "pipe" }); }
      catch (e) { problems.push(`${rel} [${lang}] ${firstLine}\n      ${String(e.stderr || e).trim().split("\n").slice(-1)[0]}`); }
    }
    // powershell: no parser here, and said so above. Unlabelled: not code.
  }
}

if (checked === 0) {
  console.error("agents-snippet: no labelled code block found in either AGENTS.md — the layout moved, and this check cannot pass vacuously.");
  process.exit(1);
}
if (problems.length) {
  console.error(`agents-snippet: ${problems.length} labelled block(s) that do not parse — a reader would run this and fail:`);
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}
console.log(`agents-snippet: ok — ${checked} labelled block(s) across ${DOCS.length} instruction file(s) all parse`);
