#!/usr/bin/env node
// THE CLI'S DECISION CORPUS, CHECKED AGAINST THE RUST — 162 cases, one answer each.
//
//   node parity/compare.mjs [path-to-summrise-cli-parity]
//
// Default path is `agent/target/debug/summrise-cli-parity` (build it with
// `cargo build -p summrise-cli --bin summrise-cli-parity`). Exits non-zero when any case differs,
// and prints how many were compared and how many differed — 0 differing is the bar.
//
// ── WHAT THIS USED TO BE, AND WHY IT IS NOT THAT ANY MORE (landing 4b, the cutover) ───────────────
//
// Until this landing this was a DIFFERENTIAL: `ts.mjs` ran the same corpus through the JavaScript the
// device executed (`summrise-agent-npm/bin/summrise.js`, compiled from `src/summrise.ts`) and the two
// answers were compared case by case. The port is finished and the JavaScript no longer ships — the
// npm `bin` is `bin/summrise.exe` — so there is no longer a second implementation to run.
//
// A differential that is deleted with its oracle takes the proof with it, so the oracle's answers were
// FROZEN FIRST, in `expected.json`, from `ts.mjs` at the commit that removed it
// (`cd87d7d726a434316823f8e16208737cba603fc5`, the merge of `main` into this branch, where both sides
// existed and the differential read **compared 162 cases, 0 differed**). Every value in that file is
// the TypeScript's answer, not the Rust's — they were proven equal before the freeze, and the file was
// produced by running `ts.mjs`, never `summrise-cli-parity`.
//
// So this is a REGRESSION gate now, and it is the stronger half of one: the corpus is fixed, the
// answers are fixed, and a change to any ported decision that moves one of the 162 answers fails here
// with the case's id. What it no longer proves is fidelity to a live second implementation — that
// question was answered, once, and the answer is in this header.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const rustBin =
  process.argv[2] || join(here, "..", "..", "target", "debug", "summrise-cli-parity");

// `ProgramData` is an input to the marker path, so it is set explicitly for the side that is run: the
// frozen answers were captured with it set, and a difference caused by the driver would be a false
// positive. `SUMMRISE_CDN` is deliberately NOT set — the answers were captured with both sides
// falling back to their OWN compiled default, so this also pins that default.
const env = { ...process.env, ProgramData: "C:\\ProgramData" };

function run(cmd, args) {
  return JSON.parse(execFileSync(cmd, args, { encoding: "utf8", env, maxBuffer: 64 * 1024 * 1024 }));
}

const expected = JSON.parse(readFileSync(join(here, "expected.json"), "utf8"));
const rs = run(rustBin, []);

const wantById = new Map(expected.cases.map((c) => [c.id, c.value]));
const rsById = new Map(rs.cases.map((c) => [c.id, c.value]));

const ids = [...new Set([...wantById.keys(), ...rsById.keys()])].sort();
let compared = 0;
let missing = 0;
const differing = [];

for (const id of ids) {
  if (!wantById.has(id)) {
    missing += 1;
    differing.push(`${id}\n  only in Rust (expected.json holds no such case)`);
    continue;
  }
  if (!rsById.has(id)) {
    missing += 1;
    differing.push(`${id}\n  only in expected.json (the Rust binary emits no such case)`);
    continue;
  }
  compared += 1;
  const a = JSON.stringify(wantById.get(id));
  const b = JSON.stringify(rsById.get(id));
  if (a !== b) differing.push(`${id}\n  expected: ${a}\n  rust:     ${b}`);
}

console.log(`compared ${compared} cases, ${differing.length} differed (${missing} present on one side only)`);
if (differing.length) {
  console.log("\nDIFFERENCES:\n");
  for (const d of differing.slice(0, 40)) console.log(d + "\n");
  process.exit(1);
}
