// contract-vocabulary-check.mjs — THE WIRE VOCABULARY, CHECKED AT BOTH ENDS.
//
// WHY THIS EXISTS (round 44 of the standing goal). Every string in `agent/contract-vocabulary.json` is written by the
// DEVICE and switched on by an INTERFACE, and until this round each end spelled them independently: `"monitor-change"`
// in `monitor.rs` and again in the panel's `useMonitors`, `"crashed"` in `runstate.rs` and again in `bootNotice.ts`,
// the command end reasons in `exec.rs` and again in `stateFromEnd`. Two hand-written copies of one fact is the exact
// shape the objective exists to remove, and each copy was kept in step by tests that read ONE side.
//
// THE SOURCE OF TRUTH IS RUST (`agent/src/vocabulary.rs`); the artifacts are generated from it by
// `VALE_REFRESH_CONTRACT=1 cargo test contract_vocabulary_snapshot`. This gate cannot run cargo (it runs in the node
// job), so it checks the DIRECTIONS that catch real drift, by name and by file:
//
//   1. the two artifacts agree with each other — a hand-edited one is caught;
//   2. every `"ev"` literal in the agent's Rust sources is a listed frame — a NEW frame cannot be invented silently;
//   3. every boot kind the enum spells is listed, and every kind listed is spelled by the enum — the fifth kind
//      (`machine-restart`) was missing from the first draft of the list, which is how this direction earned its place;
//   4. the PANEL's readers only compare against listed values (`bootNotice.ts` kinds, `stateFromEnd` reasons) — a
//      comparison against a string the device never writes is a state that silently never renders.
//
// Exit 1 with a named file and value for any of them. Run: node scripts/test/contract-vocabulary-check.mjs
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const read = (p) => readFileSync(join(ROOT, p), "utf8");

let fail = 0;
const bad = (msg) => {
  fail++;
  console.error(`FAIL ${msg}`);
};
const ok = (msg) => console.log(`ok   ${msg}`);

// ── the artifacts ────────────────────────────────────────────────────────────
const jsonRaw = read("agent/contract-vocabulary.json");
const contract = JSON.parse(jsonRaw.split("\n").filter((l) => !l.startsWith("//")).join("\n"));
const tsGen = read("agent/resources/panel-react/src/lib/contract.gen.ts");

for (const [key, tsName] of [
  ["frames", "FRAMES"],
  ["boot_kinds", "BOOT_KINDS"],
  ["end_reasons", "END_REASONS"],
]) {
  const asTs = JSON.stringify(contract[key]).replace(/,/g, ",");
  // the generated TS writes the same array with no spaces; compare values, not formatting
  const m = new RegExp(`export const ${tsName} = (\\[[^\\]]*\\]) as const;`).exec(tsGen);
  if (!m) {
    bad(`contract.gen.ts has no ${tsName} — regenerate with VALE_REFRESH_CONTRACT=1 cargo test contract_vocabulary_snapshot`);
    continue;
  }
  const fromTs = JSON.parse(m[1]);
  if (JSON.stringify(fromTs) !== JSON.stringify(contract[key])) {
    bad(`contract.gen.ts ${tsName} and contract-vocabulary.json disagree: ${JSON.stringify(fromTs)} vs ${JSON.stringify(contract[key])}`);
  } else {
    ok(`${tsName}: both artifacts carry the same ${contract[key].length} value(s)`);
  }
}
if (!tsGen.includes(`export const EXITED_PREFIX = ${JSON.stringify(contract.exited_prefix)};`)) {
  bad(`contract.gen.ts EXITED_PREFIX is not ${JSON.stringify(contract.exited_prefix)}`);
}

// ── every frame literal in the Rust sources is listed ────────────────────────
function rustFiles(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...rustFiles(p));
    else if (name.endsWith(".rs")) out.push(p);
  }
  return out;
}
const rust = rustFiles(join(ROOT, "agent/src"));
const frames = new Set(contract.frames);
let sawFrames = 0;
for (const f of rust) {
  const text = readFileSync(f, "utf8");
  text.split("\n").forEach((line, i) => {
    const m = /"ev"\s*:\s*"([a-z-]+)"/.exec(line);
    if (!m) return;
    sawFrames++;
    if (!frames.has(m[1])) {
      bad(`${relative(ROOT, f)}:${i + 1} pushes frame "${m[1]}", which contract-vocabulary.json does not list — add it to agent/src/vocabulary.rs`);
    }
  });
}
if (sawFrames < 4) {
  bad(`only ${sawFrames} frame literal(s) found in agent/src — the scan has gone stale, so this proves nothing`);
} else {
  ok(`${sawFrames} frame literal(s) in the agent, every one listed (${contract.frames.length} known)`);
}

// ── the boot kinds, both directions ──────────────────────────────────────────
const runstate = read("agent/src/runstate.rs");
const enumKinds = [...runstate.matchAll(/BootKind::\w+ => "([a-z-]+)"/g)].map((m) => m[1]);
if (enumKinds.length < 5) {
  bad(`read ${enumKinds.length} boot kind(s) out of runstate.rs — expected at least 5, so this proves nothing`);
} else {
  for (const k of enumKinds) {
    if (!contract.boot_kinds.includes(k)) bad(`runstate.rs spells boot kind "${k}" and the contract does not list it`);
  }
  for (const k of contract.boot_kinds) {
    if (!enumKinds.includes(k)) bad(`the contract lists boot kind "${k}" and runstate.rs never spells it`);
  }
  ok(`${enumKinds.length} boot kind(s) agree in both directions`);
}

// ── the panel's readers only compare against listed values ───────────────────
const bootNotice = read("agent/resources/panel-react/src/lib/bootNotice.ts");
const kinds = new Set(contract.boot_kinds);
// A SWITCH, LIKE THE DERIVATION — and the first version of this scan looked for `kind === "…"`, found NOTHING, and
// passed silently: mutation two (renaming a case to a kind the device never writes) did not bite. The floor below is
// the fix for that, and it is the same lesson the derivation's scan taught one screen up: a scan that reads nothing
// must say so, or it certifies an empty set.
let sawKinds = 0;
for (const m of bootNotice.matchAll(/^\s*case "([a-z-]+)":/gm)) {
  sawKinds++;
  if (!kinds.has(m[1])) bad(`bootNotice.ts switches on kind "${m[1]}", which the device never writes`);
}
if (sawKinds < 2) {
  bad(`read ${sawKinds} boot kind(s) out of bootNotice.ts — expected at least 2, so this proves nothing`);
} else {
  ok(`${sawKinds} boot kind(s) in bootNotice.ts, all named by contract-vocabulary.json`);
}
const pathTs = read("agent/resources/panel-react/src/lib/path.ts");
const reasons = new Set(contract.end_reasons);
let sawReasons = 0;
// The derivation is a SWITCH over the reason (its own comment explains why `backgrounded` is not folded into `warn`),
// so the scan reads `case "…":` — the first version looked for `reason === "…"`, found nothing, and the floor below
// ("compares against no end reason at all") is what said so instead of reporting a clean pass over an empty set.
for (const m of pathTs.matchAll(/^\s*case "([a-z:]+)":/gm)) {
  sawReasons++;
  if (!reasons.has(m[1])) {
    bad(`path.ts derives a state for end reason "${m[1]}", which contract-vocabulary.json does not list`);
  }
}
if (sawReasons === 0) {
  bad("path.ts compares against no end reason at all — the derivation moved and this scan proves nothing");
} else {
  ok(`${sawReasons} end-reason comparison(s) in the derivation, all named by contract-vocabulary.json`);
}

if (fail) {
  console.error(`\ncontract-vocabulary: ${fail} problem(s) — the two ends do not spell one vocabulary`);
  process.exit(1);
}
console.log("\ncontract-vocabulary: the device and the interfaces spell one vocabulary");
