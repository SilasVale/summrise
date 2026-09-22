// sweep-bundle-check — the emitter's assembler must produce a script that RUNS, not one that merely looks right.
//
// WHY THIS EXISTS (round 266). The five sweeps used to build their device-side script inside a template literal, and
// the round is replacing that with real payload modules assembled by `lib/sweep-bundle.mjs`. The first version of the
// assembler passed the obvious checks — it emitted a file, the file parsed, and every program PIECE was byte-identical
// to what the old emitter produced (verified: 12/12, probes, passes and page checks all identical) — and it was still
// broken: the loader's relative `require` returned the module ID STRING instead of calling `__require`, so the first
// payload that touched a required value died with "Cannot read properties of undefined (reading 'root')".
//
// THE LESSON IS THE CHECK: comparing pieces proves the TEXT is the same, and a seam is only proved by EXECUTING it.
// Every case below builds a payload in memory, assembles it, writes it to a temp directory with NO payload modules
// beside it, and RUNS it with node — so a bundle that quietly still needs its sources on disk, or that resolves a
// require to a string, fails here instead of on a device.
//
// A GATE THAT CANNOT FAIL IS WORSE THAN NO GATE: `gate-mutations-check.mjs` carries the mutation (the loader's
// lookup without the call), and it must make this file exit 1.
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { bundleSweep, assertParses } from "../../agent/scripts/lib/sweep-bundle.mjs";

const TMP = mkdtempSync(join(tmpdir(), "sweep-bundle-"));
let pass = 0;
const failed = [];
const ok = (what) => { pass++; console.log("  ok: " + what); };
const bad = (what, detail) => { failed.push(what); console.error("  FAIL: " + what + (detail ? " — " + detail : "")); };

/** Assemble, write it somewhere with NO payload beside it, run it, and return { status, stdout, stderr }. */
function runBundle(name, modules, entry, timeout = 20000) {
  const { code, ids } = bundleSweep({ modules, entry });
  const file = join(TMP, name + ".js");
  writeFileSync(file, code);
  const r = spawnSync(process.execPath, [file], { encoding: "utf8", cwd: TMP, timeout });
  return { ...r, code, ids, file };
}

try {
  // ── 1. a three-module chain RUNS, and prints what the chain computes ────────────────────────────────────────
  {
    const modules = {
      "a.cjs": 'const b = require("./b.cjs");\nmodule.exports = { value: b.value + 1 };\n',
      "b.cjs": 'const c = require("./c.cjs");\nmodule.exports = { value: c.base * 2 };\n',
      "c.cjs": "module.exports = { base: 20 };\n",
    };
    const r = runBundle("chain", modules, "a.cjs");
    if (r.status !== 0) bad("a three-module chain runs", (r.stderr || "").split("\n").slice(0, 3).join(" | "));
    else ok("a three-module chain runs (a → b → c, assembled in dependency order)");
    // AND THE POINT OF IT: a relative require must hand back the EXPORTS, not the module id.
    const probe = { "a.cjs": 'const b = require("./b.cjs");\nif (typeof b !== "object") { console.error("require returned " + typeof b); process.exit(3); }\nconsole.log(JSON.stringify(b));\n', "b.cjs": "module.exports = { ok: 1 };\n" };
    const p = runBundle("types", probe, "a.cjs");
    if (p.status === 0 && /"ok":1/.test(p.stdout)) ok("a relative require returns the module's exports (not its id)");
    else bad("a relative require returns the module's exports", `status=${p.status} out=${p.stdout.trim()} err=${p.stderr.trim().split("\n")[0] || ""}`);
  }

  // ── 2. node builtins pass through untouched ────────────────────────────────────────────────────────────────
  {
    const modules = { "a.cjs": 'const path = require("path");\nconsole.log(path.join("x", "y"));\n' };
    const r = runBundle("builtins", modules, "a.cjs");
    if (r.status === 0 && r.stdout.trim() === join("x", "y")) ok("a node builtin require is left to node");
    else bad("a node builtin require is left to node", `status=${r.status} out=${r.stdout.trim()} err=${r.stderr.trim().split("\n")[0] || ""}`);
  }

  // ── 3. THE HAZARD THIS WHOLE ROUND IS ABOUT: a backtick and a ${} inside a payload module ──────────────────
  {
    const modules = {
      "a.cjs": "// a comment with a backtick ` and an interpolation ${nope} — this is what ended a host file mid-parse\nconst s = `template ${1 + 1} with a backtick \\` inside`;\nconst d = \"a regex with a backslash: /\\\\s+/\";\nconsole.log(s + \" | \" + d);\n",
    };
    const r = runBundle("backticks", modules, "a.cjs");
    if (r.status === 0 && /template 2 with a backtick ` inside/.test(r.stdout) && /\/\\s\+\//.test(r.stdout)) {
      ok("a backtick, an interpolation and a backslash in a payload module survive assembly");
    } else {
      bad("a backtick / interpolation / backslash survive assembly", `status=${r.status} out=${r.stdout.trim()} err=${r.stderr.trim().split("\n")[0] || ""}`);
    }
  }

  // ── 4. both export spellings ───────────────────────────────────────────────────────────────────────────────
  {
    const modules = {
      "a.cjs": 'const b = require("./b.cjs");\nconst c = require("./c.cjs");\nconsole.log(b.x + "," + c.y);\n',
      "b.cjs": "exports.x = 1;\n",
      "c.cjs": "module.exports = { y: 2 };\n",
    };
    const r = runBundle("exports", modules, "a.cjs");
    if (r.status === 0 && r.stdout.trim() === "1,2") ok("both export spellings work (exports.x and module.exports =)");
    else bad("both export spellings work", `status=${r.status} out=${r.stdout.trim()} err=${r.stderr.trim().split("\n")[0] || ""}`);
  }

  // ── 5. a missing relative require is refused at BUNDLE time, naming both modules ───────────────────────────
  {
    let msg = "";
    try { bundleSweep({ modules: { "a.cjs": 'require("./nope.cjs");\n' }, entry: "a.cjs" }); } catch (e) { msg = String(e.message); }
    if (/a\.cjs/.test(msg) && /nope\.cjs/.test(msg)) ok("a missing relative require is refused, naming the module and the specifier");
    else bad("a missing relative require is refused by name", msg || "no error at all");
  }

  // ── 6. a cycle is refused, naming it ───────────────────────────────────────────────────────────────────────
  {
    let msg = "";
    try {
      bundleSweep({ modules: { "a.cjs": 'require("./b.cjs");\n', "b.cjs": 'require("./a.cjs");\n' }, entry: "a.cjs" });
    } catch (e) { msg = String(e.message); }
    if (/cycle/i.test(msg) && /a\.cjs/.test(msg) && /b\.cjs/.test(msg)) ok("a require cycle is refused, naming the cycle");
    else bad("a require cycle is refused by name", msg || "no error at all");
  }

  // ── 7. THE PARSE GUARANTEE: an emitter must not print a script nobody can run ──────────────────────────────
  {
    let msg = "";
    try { bundleSweep({ modules: { "a.cjs": "const x = ;\n" }, entry: "a.cjs" }); } catch (e) { msg = String(e.message); }
    if (/does not parse/.test(msg)) ok("a payload that does not parse is refused before it is printed");
    else bad("a payload that does not parse is refused", msg || "no error at all");
    // and the exported guard is the same one, callable on its own
    let direct = "";
    try { assertParses("function ( { "); } catch (e) { direct = String(e.message); }
    if (/does not parse/.test(direct)) ok("assertParses is the same guarantee, callable directly");
    else bad("assertParses refuses unparsable code", direct || "no error at all");
  }

  // ── 8. the bundle is SELF-CONTAINED: it runs with no payload module beside it ──────────────────────────────
  {
    const modules = { "a.cjs": 'const b = require("./nested/b.cjs");\nconsole.log(b.deep);\n', "nested/b.cjs": 'module.exports = { deep: "yes" };\n' };
    const r = runBundle("selfcontained", modules, "a.cjs");
    // the temp dir holds only the emitted file — if the bundle still needed nested/b.cjs it would fail here
    if (r.status === 0 && r.stdout.trim() === "yes") ok("a nested module runs from a directory that holds ONLY the emitted file");
    else bad("the bundle is self-contained", `status=${r.status} out=${r.stdout.trim()} err=${r.stderr.trim().split("\n")[0] || ""}`);
  }
} finally {
  rmSync(TMP, { recursive: true, force: true });
}

if (failed.length) {
  console.error(`\nsweep-bundle-check: ${failed.length} failure(s) of ${pass + failed.length}`);
  process.exit(1);
}
console.log(`sweep-bundle-check: ${pass} checks passed — the assembler runs what it emits`);
