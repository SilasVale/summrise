#!/usr/bin/env node
// THE PARITY CHECK: the same inputs through the JAVASCRIPT and the RUST, compared case by case.
//
//   node agent/sweep-plan/parity/compare.mjs --base <rev|path> [--bin <path>] [--verbose]
//
// WHY A WORKTREE, AND WHY `--base` IS NOT OPTIONAL IN PRACTICE. The JavaScript side has to be the
// PRE-CHANGE code, or this compares an implementation with itself and proves nothing. `--base` names a
// revision (default `HEAD`), and the harness makes a `git worktree` of it, runs the emitters there —
// which is the JavaScript's own plan computation, executed, not re-read — and removes the worktree
// afterwards. A path may be given instead, for a checkout that already exists.
//
// **THE DEFAULT IS ONLY CORRECT BEFORE THE LANDING IS COMMITTED.** Afterwards `HEAD` IS the
// post-change tree, and the harness REFUSES it by name (it hashes the three payloads on both sides and
// fails when they are the same code) — a CI invocation must name the pre-change commit:
//
//   cargo build -p summrise-sweep-plan && node agent/sweep-plan/parity/compare.mjs --base <rev>
//
// MEASURED ON THE LANDING: 229 cases compared, 0 differed, against `b5e00dea` (the commit before it);
// and 229/0 with `SUMMRISE_PARITY_JS_TREE` pointed at the working tree, which is the check to run after
// every payload edit — the Rust side is already pinned to the pre-change JavaScript by the default run.
//
// WHAT IS COMPARED, for every `--passes` value, for all three tools:
//
//   1. THE SURFACE TRACE. The pre-change emitter is run, its emitted bundle is EXECUTED against a stub
//      browser (parity/stub-browser.cjs), and the ordered sequence of what it asked the browser to do
//      — resize, navigate, emulate media, set the SPA hash, reload — is compared with the same
//      sequence PROJECTED FROM THE RUST PLAN. This is the surface list and its order.
//   2. THE PASS SET. The payload's own `const wants = ...` line is extracted from the pre-change
//      payload and evaluated over that tool's whole pass vocabulary, and compared with the plan's
//      `wants` map. This is the predicate, not a restatement of it.
//   3. THE `passes` VALUE the report records, read out of the emitted bundle's own pieces module
//      (`config.passes`) and compared with the plan's.
//
// Exits non-zero when any case differs, and prints how many were compared and how many differed —
// 0 differing is the bar.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { trace, awaitTrace } from "./trace.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const REPO = join(here, "..", "..", "..");

function arg(name, dflt) {
  const i = process.argv.indexOf(name);
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : dflt;
}
const VERBOSE = process.argv.includes("--verbose");
const BASE = arg("--base", "HEAD");
const BIN = arg("--bin", join(REPO, "agent", "target", "debug", "summrise-sweep-plan"));

// ── THE JAVASCRIPT SIDE'S TREE ──────────────────────────────────────────────────────────────────────
// `SUMMRISE_PARITY_JS_TREE` points the JavaScript side at a checkout instead of the base worktree. It
// has two uses, and the second is the one that matters while a payload is being rewritten: it proves a
// MODIFIED payload still executes the same plan (the Rust side is already pinned to the pre-change
// JavaScript by the default run), which is the check to run after every edit rather than once at the end.
let worktree = null;
function headTree() {
  if (process.env.SUMMRISE_PARITY_JS_TREE) return process.env.SUMMRISE_PARITY_JS_TREE;
  if (existsSync(BASE)) return BASE;
  worktree = mkdtempSync(join(tmpdir(), "sweep-plan-base-"));
  execFileSync("git", ["worktree", "add", "--detach", worktree, BASE], { cwd: REPO, stdio: "pipe" });
  return worktree;
}

// ── THE TOOLS, THEIR VOCABULARIES, AND THE CASES ────────────────────────────────────────────────────
// The vocabularies are the pass names each tool's `wants()` is asked about. They are re-stated here
// rather than imported from the Rust, because the whole point is that this file does not take either
// side's word for what the names are: `wants` is compared over the union of what BOTH sides name.
const TOOLS = {
  panel: ["pages", "focus", "timing", "hover", "press", "idle", "targets", "ack", "unstyled", "motion", "reflow"],
  console: ["dark", "reflow", "contrast", "names", "unstyled", "focus", "press", "ack", "idle", "motion", "targets"],
  landing: ["contrast", "names", "focus", "idle", "unstyled", "targets", "press", "reflow", "motion"],
};

/** EVERY `--passes` value these tools accept, as the cases that can distinguish them:
 *  the flag absent, the empty value, `all`, each single pass, EVERY PAIR, every "all but one", a
 *  spelled-out multi-value, a spaced multi-value (the two predicates disagree about trimming), an
 *  unknown name (both must simply not want it), and a long list. The value space is unbounded — the
 *  flag is a CSV — so the coverage claim is this enumeration rather than "all strings". */
function cases(tool) {
  const v = TOOLS[tool];
  const specs = [
    { label: "absent", value: null },
    { label: "empty", value: "" },
    { label: "all", value: "all" },
    ...v.map((p) => ({ label: p, value: p })),
    { label: "spaced", value: `${v[0]}, ${v[1]}` },
    { label: "unknown", value: "not-a-pass" },
    { label: "unknown+known", value: `not-a-pass,${v[0]}` },
    { label: "everything-spelled-out", value: v.join(",") },
  ];
  for (let i = 0; i < v.length; i++) {
    for (let j = i + 1; j < v.length; j++) {
      specs.push({ label: `${v[i]}+${v[j]}`, value: `${v[i]},${v[j]}` });
    }
  }
  for (const p of v) {
    specs.push({ label: `all-but-${p}`, value: v.filter((x) => x !== p).join(",") });
  }
  return specs;
}

// ── THE RUST SIDE ───────────────────────────────────────────────────────────────────────────────────
function rustPlan(tool, passes) {
  const args = ["--tool", tool];
  if (passes !== null) args.push(`--passes=${passes}`);
  const out = execFileSync(BIN, args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  return JSON.parse(out);
}

/** The plan's surfaces, projected into the same event sequence the stub browser records. */
function project(plan) {
  const ev = [];
  const media = (m) => ev.push({ t: "media", ...m });
  for (const s of plan.surfaces) {
    for (const step of s.pre || []) if (step.emulateMedia) media(step.emulateMedia);
    if (s.set_viewport) {
      ev.push({ t: "viewport", width: s.viewport.width, height: s.viewport.height });
    }
    if (s.media) media(s.media);
    if (s.navigate) {
      const path = s.path === undefined ? "" : s.path.startsWith("/") ? s.path : "/" + s.path;
      const query = (s.query || []).map(([k, v]) => `${k}=${v}`).join("&");
      ev.push({ t: "goto", url: `${plan.origin}${path}${query ? "?" + query : ""}` });
    }
    if (s.hash) ev.push({ t: "hash", hash: s.hash });
    if (s.reload) ev.push({ t: "reload" });
    for (const step of s.post || []) if (step.emulateMedia) media(step.emulateMedia);
  }
  for (const step of plan.post || []) if (step.emulateMedia) media(step.emulateMedia);
  return ev;
}

// ── THE JAVASCRIPT SIDE'S TWO OTHER DECISIONS, READ FROM ITS OWN CODE ───────────────────────────────
/**
 * THE PAYLOAD'S OWN `wants()` LINE, extracted and evaluated. This is the comparison that makes the
 * harness an oracle rather than a mirror: the expression is the JavaScript's, the bindings are the
 * ones its own emitter supplies, and the answers are compared name by name with the plan's map.
 */
function jsWants(tool, tree, passes, plan) {
  const file = join(tree, "agent", "scripts", "lib", "sweep", `${tool}-run.cjs`);
  const src = readFileSync(file, "utf8");
  const m = /^\s*const wants = (.+);\s*$/m.exec(src);
  if (!m) throw new Error(`${file}: could not find the payload's own \`const wants = ...\` line`);
  // `PLAN` is bound as well, because the POST-CHANGE payload's line is `(name) => PLAN.wants(name)` —
  // for those runs this comparison is a tautology and the trace is what carries the weight; for the
  // BASE tree (the default, and the oracle) the expression is the JavaScript's own predicate over
  // `report.passes` / `PASSES`, which is the comparison that can actually fail.
  // eslint-disable-next-line no-new-func
  // The POST-CHANGE payload's `PLAN` is the runtime shim whose `wants()` is a map lookup, so the
  // same object shape is handed over here.
  const shim = { ...plan, wants: (n) => plan.wants[n] === true };
  const fn = new Function("report", "PASSES", "PLAN", `return (${m[1]});`);
  if (tool === "panel") {
    // The panel's `wants` reads `report.passes`, which its emitter defaults to "all" when the flag is
    // absent and passes through verbatim when it is empty.
    return (name) => fn({ passes: passes === null ? "all" : passes }, undefined, shim)(name);
  }
  const list = (passes === null ? "" : passes).split(",").filter(Boolean);
  return (name) => fn(undefined, list, shim)(name);
}

/** The `config.passes` value the emitted bundle carries — the JavaScript's own answer. */
function jsPasses(code) {
  const m = /"config":\s*\{[\s\S]*?"passes":\s*(\[[^\]]*\]|"[^"]*")/.exec(code);
  if (!m) throw new Error("the emitted bundle carries no config.passes — the harness cannot compare it");
  return JSON.parse(m[1]);
}

// ── RUN ─────────────────────────────────────────────────────────────────────────────────────────────
const tree = headTree();
// CHECKED BEFORE THE WORKTREE IS REMOVED: a base that never resolved would make every trace come from
// the working tree, i.e. from the post-change code, and every case would agree for the wrong reason.
const treeHasPayload = existsSync(join(tree, "agent", "scripts", "lib", "sweep", "panel-run.cjs"));
// **AND A BASE THAT IS THE POST-CHANGE CODE MAKES THIS HARNESS VACUOUS**, which is the one wiring
// mistake that would otherwise look like a pass: after the landing is committed, `HEAD` IS the
// post-change tree, and comparing it with the Rust plan compares the plan with its own consumer. So the
// payload is hashed on both sides, and a base that matches the working tree is refused by name — the CI
// invocation must name the PRE-CHANGE commit (`--base <rev>`).
const payloadHash = (root) => {
  const h = createHash("sha256");
  for (const f of ["panel-run.cjs", "console-run.cjs", "landing-run.cjs"]) {
    h.update(readFileSync(join(root, "agent", "scripts", "lib", "sweep", f)));
  }
  return h.digest("hex");
};
// THE OVERRIDE IS THE ONE CASE THE GUARD MUST NOT REFUSE: `SUMMRISE_PARITY_JS_TREE` exists precisely to
// run the WORKING TREE's payloads against the Rust plan — the check to run after every payload edit,
// since the Rust side is already pinned to the pre-change JavaScript by the `--base` run.
const jsTreeIsOverride = Boolean(process.env.SUMMRISE_PARITY_JS_TREE);
let baseIsWorkingTree = false;
if (treeHasPayload && !jsTreeIsOverride) {
  // A THROW HERE IS A HARNESS BUG, not a pass: it used to be swallowed, and a swallowed error in this
  // guard is exactly the vacuous comparison the guard exists to refuse.
  baseIsWorkingTree = payloadHash(tree) === payloadHash(REPO);
}
const tmp = mkdtempSync(join(tmpdir(), "sweep-plan-trace-"));
let compared = 0;
let differing = 0;
const diffs = [];

function fail(id, what, js, rs) {
  differing += 1;
  diffs.push(`${id}: ${what}\n  js:   ${JSON.stringify(js)}\n  rust: ${JSON.stringify(rs)}`);
}

function firstDifference(a, b) {
  const n = Math.max(a.length, b.length);
  for (let i = 0; i < n; i++) {
    if (JSON.stringify(a[i]) !== JSON.stringify(b[i])) return i;
  }
  return -1;
}

try {
  for (const [tool, vocab] of Object.entries(TOOLS)) {
    for (const spec of cases(tool)) {
      const id = `${tool} --passes=${spec.value === null ? "(absent)" : spec.value}`;
      const plan = rustPlan(tool, spec.value);
      const { stub, code } = trace(tree, tool, spec.value, tmp);
      const jsEvents = await awaitTrace(stub);
      const rsEvents = project(plan);

      compared += 1;
      const at = firstDifference(jsEvents, rsEvents);
      if (at >= 0) {
        fail(`${id} [trace]`, `event ${at} of ${jsEvents.length}/${rsEvents.length}`, jsEvents.slice(Math.max(0, at - 2), at + 3), rsEvents.slice(Math.max(0, at - 2), at + 3));
      }

      const jsP = jsPasses(code);
      if (JSON.stringify(jsP) !== JSON.stringify(plan.passes)) {
        fail(`${id} [passes]`, "the report's passes value", jsP, plan.passes);
      }

      const wants = jsWants(tool, tree, spec.value, plan);
      for (const name of vocab) {
        const js = wants(name);
        const rs = plan.wants[name] === true;
        if (js !== rs) fail(`${id} [wants]`, `wants("${name}")`, js, rs);
      }
      if (VERBOSE) {
        console.log(`  ok ${id}: ${jsEvents.length} events, ${plan.surfaces.length} surfaces, ${plan.wanted.length} pass(es) wanted`);
      }
    }
  }
} finally {
  rmSync(tmp, { recursive: true, force: true });
  if (worktree) {
    try {
      execFileSync("git", ["worktree", "remove", "--force", worktree], { cwd: REPO, stdio: "pipe" });
    } catch (e) {
      console.error(`could not remove the worktree at ${worktree}: ${String(e.message).slice(0, 120)}`);
    }
  }
}

console.log(
  `compared ${compared} cases, ${differing} differed` +
    (jsTreeIsOverride
      ? ` — THE JAVASCRIPT SIDE IS THE WORKING TREE (SUMMRISE_PARITY_JS_TREE=${tree}), so this checks the ` +
        `MODIFIED payloads against the plan; the pre-change oracle is the same command without it`
      : ""),
);
if (differing) {
  console.log("\nDIFFERENCES:\n");
  for (const d of diffs.slice(0, 25)) console.log(d + "\n");
  process.exit(1);
}
// A HARNESS THAT COMPARED NOTHING MUST NOT REPORT PARITY.
if (compared < 60) {
  console.error(`only ${compared} cases were compared — this proves nothing`);
  process.exit(1);
}
if (!treeHasPayload) {
  console.error(`the JavaScript side's tree (${tree}) holds no payload — nothing was compared`);
  process.exit(1);
}
if (baseIsWorkingTree) {
  console.error(
    `the JavaScript side's tree (${tree}) carries the SAME payloads as this working tree, so this run ` +
      `compared the Rust plan with its own consumer — name the PRE-CHANGE commit with --base <rev> ` +
      `(default HEAD is only correct before the landing is committed)`,
  );
  process.exit(1);
}
