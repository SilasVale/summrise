// contract-vocabulary-check.mjs — THE WIRE VOCABULARY, CHECKED AT BOTH ENDS.
//
// WHY THIS EXISTS (round 44 of the standing goal). Every string in `agent/contract-vocabulary.json` is written by the
// DEVICE and switched on by an INTERFACE, and until this round each end spelled them independently: `"monitor-change"`
// in `monitor.rs` and again in the panel's `useMonitors`, `"crashed"` in `runstate.rs` and again in `bootNotice.ts`,
// the command end reasons in `exec.rs` and again in `stateFromEnd`. Two hand-written copies of one fact is the exact
// shape the objective exists to remove, and each copy was kept in step by tests that read ONE side.
//
// THE SOURCE OF TRUTH IS RUST (`agent/src/vocabulary.rs`); the artifacts are generated from it by
// `SUMMRISE_REFRESH_CONTRACT=1 cargo test contract_vocabulary_snapshot`. This gate cannot run cargo (it runs in the node
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
import { decomment } from "./lib/decomment.mjs";
import { execFileSync } from "node:child_process";
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
    bad(`contract.gen.ts has no ${tsName} — regenerate with SUMMRISE_REFRESH_CONTRACT=1 cargo test contract_vocabulary_snapshot`);
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
  const text = decomment(readFileSync(f, "utf8"));
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
// THE DERIVATION READS THE GENERATED VOCABULARY NOW (round 52), so the scan reads its TABLE rather than a switch: the
// endings are keys of `END_STATE`, keyed by the generated `EndReason` union, which means a reason the device adds and
// the table does not name is a TYPESCRIPT error — stricter than this gate. What the gate still owns is the direction
// TypeScript cannot see: every reason the vocabulary lists must be a key of that table (a missing one would make the
// union wider than the table and fail the build, but a STALE table with extra keys would not), and the prefix must be
// the generated one rather than a literal.
// The table's entries are indented under `const END_STATE … = {`; the first version of this scan asked for exactly two
// spaces and found nothing, which its own floor reported as "the derivation moved" rather than as a clean pass.
// (`END_LABEL` has the same keys and different values, so requiring a STATE value is what tells the two tables apart.)
const tableKeys = [...pathTs.matchAll(/^\s+([a-z]+):\s*"(muted|warn|bg|ok|fail|running)",/gm)].map((m) => m[1]);
if (tableKeys.length === 0) {
  bad("path.ts has no end-state table — the derivation moved and this scan proves nothing");
} else {
  const named = new Set(tableKeys);
  for (const r of contract.end_reasons) {
    if (!named.has(r)) bad(`path.ts's table does not name the end reason "${r}" the device writes`);
  }
  for (const r of named) {
    if (!contract.end_reasons.includes(r)) bad(`path.ts's table names "${r}", which contract-vocabulary.json does not list`);
  }
  if (!pathTs.includes("EXITED_PREFIX")) bad("path.ts no longer reads the generated EXITED_PREFIX");
  ok(`${tableKeys.length} end reason(s) in the derivation's table, every one in the vocabulary (and the type is generated)`);
}

// ── AND EVERY DECLARED VALUE IS ONE THE DEVICE ACTUALLY WRITES (round 156). The artifact's own header says it holds "the
// strings the device writes and the interfaces read", and this file's header has always claimed that "a state the device
// never writes is a state that silently never renders" — but nothing checked the WRITING half: the clauses above compare the
// two ends for agreement, and a value declared in `vocabulary.rs` and emitted nowhere would agree with itself perfectly.
// Measured before adding this: all 16 values (5 boot kinds, 6 end reasons, 5 frames) appear as string literals in `agent/src`.
// THE FILE LIST COMES FROM GIT, the way `production-host-check` does it: a hand-rolled walk recursed until the stack blew
// (a symlink, most likely), and `git ls-files` is authoritative, ordered and cannot loop.
const rustSources = execFileSync("git", ["ls-files", "agent/src"], { cwd: ROOT, encoding: "utf8" })
  .split("\n")
  .filter((f) => f.endsWith(".rs"))
  .map((f) => readFileSync(join(ROOT, f), "utf8"))
  .join("\n");

const neverWritten = [];
for (const [group, values] of Object.entries(contract)) {
  if (!Array.isArray(values)) continue;
  for (const v of values) {
    if (!rustSources.includes(JSON.stringify(v))) neverWritten.push(`${group}: ${v}`);
  }
}
if (neverWritten.length) {
  fail += neverWritten.length;
  console.error(
    `FAIL ${neverWritten.length} declared value(s) the device never writes — a state that silently never renders:\n  ` +
      neverWritten.join("\n  ") +
      `\n\nEither the device emits it somewhere in agent/src, or it is a name only the interfaces know.`,
  );
}

// ── AND EVERY DECLARED VALUE IS READ BY AN INTERFACE (round 157), the symmetric half of the clause above. The artifact's
// header claims both directions — "the strings the device writes and the interfaces read" — and round 156 checked the writing;
// this checks the reading, because a value the device emits and nobody switches on is dead vocabulary that will drift out of
// the language entirely.
//
// Measured before adding it: all 16 values are read somewhere (every one by the panel; three boot kinds additionally by the
// gateway and the console). The gate's own `bootNotice.ts` clause compares against SEVEN kinds because that reader handles
// retired ones for older devices — a superset, which is legitimate and is why both clauses can be true at once.
const interfaceSources = [
  "agent/resources/panel-react/src",
  "gateway/src",
  "gateway/ui/src",
]
  .flatMap((dir) =>
    execFileSync("git", ["ls-files", dir], { cwd: ROOT, encoding: "utf8" }).split("\n").filter(Boolean),
  )
  .filter((f) => /\.(ts|tsx)$/.test(f))
  .map((f) => readFileSync(join(ROOT, f), "utf8"))
  .join("\n");

const neverRead = [];
for (const [group, values] of Object.entries(contract)) {
  if (!Array.isArray(values)) continue;
  for (const v of values) {
    if (!interfaceSources.includes(JSON.stringify(v))) neverRead.push(`${group}: ${v}`);
  }
}
if (neverRead.length) {
  fail += neverRead.length;
  console.error(
    `FAIL ${neverRead.length} declared value(s) no interface reads — vocabulary the device speaks to nobody:\n  ` +
      neverRead.join("\n  ") +
      `\n\nEither an interface switches on it, or the device has no reason to emit it.`,
  );
}

if (fail) {
  console.error(`\ncontract-vocabulary: ${fail} problem(s) — the two ends do not spell one vocabulary`);
  process.exit(1);
}
console.log("\ncontract-vocabulary: the device and the interfaces spell one vocabulary");
