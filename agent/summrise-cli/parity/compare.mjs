#!/usr/bin/env node
// THE PARITY CHECK: the SAME inputs through the TypeScript and the Rust, compared case by case.
//
//   node parity/compare.mjs [path-to-summrise-cli-parity]
//
// Default path is `agent/target/debug/summrise-cli-parity` (build it with
// `cargo build -p summrise-cli --bin summrise-cli-parity`). Exits non-zero when any case differs,
// and prints how many were compared and how many differed — 0 differing is the bar.
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const rustBin =
  process.argv[2] || join(here, "..", "..", "target", "debug", "summrise-cli-parity");

// One environment for BOTH sides: `ProgramData` is an input to the marker path, and a difference
// caused by the driver would be a false positive. `SUMMRISE_CDN` is deliberately NOT set: with it
// unset both sides fall back to their OWN compiled default, so the comparison is also the check that
// the two defaults agree.
const env = { ...process.env, ProgramData: "C:\\ProgramData" };

function run(cmd, args) {
  return JSON.parse(execFileSync(cmd, args, { encoding: "utf8", env, maxBuffer: 64 * 1024 * 1024 }));
}

const ts = run(process.execPath, [join(here, "ts.mjs")]);
const rs = run(rustBin, []);

const tsById = new Map(ts.cases.map((c) => [c.id, c.value]));
const rsById = new Map(rs.cases.map((c) => [c.id, c.value]));

const ids = [...new Set([...tsById.keys(), ...rsById.keys()])].sort();
let compared = 0;
let missing = 0;
const differing = [];

for (const id of ids) {
  if (!tsById.has(id)) {
    missing += 1;
    differing.push(`${id}\n  only in Rust`);
    continue;
  }
  if (!rsById.has(id)) {
    missing += 1;
    differing.push(`${id}\n  only in TypeScript`);
    continue;
  }
  compared += 1;
  const a = JSON.stringify(tsById.get(id));
  const b = JSON.stringify(rsById.get(id));
  if (a !== b) differing.push(`${id}\n  ts:   ${a}\n  rust: ${b}`);
}

console.log(`compared ${compared} cases, ${differing.length} differed (${missing} present on one side only)`);
if (differing.length) {
  console.log("\nDIFFERENCES:\n");
  for (const d of differing.slice(0, 40)) console.log(d + "\n");
  process.exit(1);
}
