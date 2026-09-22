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

  // ── 8b. THE BROWSER TARGET: a harness payload runs in a PAGE, where require does not exist ────────────────
  {
    const modules = {
      "stub.cjs": 'const help = require("./help.cjs");\n(function () { globalThis.__stubRan = help.value; })();\n',
      "help.cjs": 'module.exports = { value: "stubbed" };\n',
    };
    const { code } = bundleSweep({ modules, entry: "stub.cjs", target: "browser" });
    // RUN IT WITH NO require IN SCOPE, which is the only way to prove the preamble does not reach for one.
    let ran = null;
    try {
      new Function("globalThis", code)({});
      ran = "no-error";
    } catch (e) {
      ran = "threw: " + String(e.message).slice(0, 80);
    }
    // the payload sets a global on ITS globalThis; in this sandbox that is the fake one passed in
    const sandbox = {};
    try {
      new Function("globalThis", code)(sandbox);
    } catch (e) {
      /* recorded below by the value */
    }
    if (ran === "no-error" && sandbox.__stubRan === "stubbed") ok("a browser-target bundle loads with NO require in scope");
    else bad("a browser-target bundle loads with no require in scope", `ran=${ran} value=${sandbox.__stubRan}`);
    if (!/__nativeRequire/.test(code)) ok("...and the word __nativeRequire does not appear anywhere in it");
    else bad("the browser preamble still names __nativeRequire");
    if (/^\(function \(\) \{/.test(code)) ok("...and it is wrapped, so the loader's names stay out of the page's global scope");
    else bad("a browser bundle is not wrapped in an IIFE");
    let msg = "";
    try {
      bundleSweep({ modules: { "stub.cjs": 'require("fs");\n' }, entry: "stub.cjs", target: "browser" });
    } catch (e) {
      msg = String(e.message);
    }
    if (/browser/.test(msg) && /fs/.test(msg)) ok("a browser payload that requires a BUILTIN is refused at bundle time, naming it");
    else bad("a browser payload requiring a builtin is refused at bundle time", msg || "no error at all");
  }

  // ── 8a. THE PIECES BINDING MUST NOT BE SHADOWED IN ITS OWN MODULE ─────────────────────────────────────────
  // Round 270 shipped exactly this: the harness stub binds the pieces module to `P` at the top of the file and then
  // declares its own `var P` for the query flags, which HID the fixture — SID, SESSION and EVENTS all became
  // undefined, the page rendered ids where labels belonged and lost its goal and approval rows. Every local check
  // passed, because the pieces VALUES in the artifact were byte-identical to before; only a page showed it. `var` is
  // function-scoped and hoisted, so the shadowing is invisible at the binding line, which is why this is a scan and
  // not a reading.
  {
    const dir = new URL("../../agent/scripts/lib/sweep/", import.meta.url);
    const { readdirSync, readFileSync } = await import("node:fs");
    const payloads = readdirSync(dir).filter((f) => f.endsWith(".cjs"));
    let checked = 0;
    const shadowed = [];
    for (const f of payloads) {
      const src = readFileSync(new URL(f, dir), "utf8");
      const m = /const\s+([A-Za-z_$][\w$]*)\s*=\s*require\("(\.\/pieces\.cjs)"\)/.exec(src);
      if (!m) continue;
      checked++;
      const name = m[1];
      const after = src.slice(src.indexOf(m[0]) + m[0].length);
      // any other declaration of that name ANYWHERE in the module can shadow it (var hoists, a nested function scopes
      // it, and both leave the binding line looking perfectly correct)
      const re = new RegExp(`(?:^|[^\\w$.])(?:var|let|const|function)\\s+${name}\\b`);
      if (re.test(after)) shadowed.push(`${f}: another ${name} is declared later in the same payload`);
    }
    if (checked < 4) bad("every payload module binds the pieces module", `read ${checked} of ${payloads.length} payload(s) — this proves nothing`);
    else ok(`all ${checked} payload modules bind the pieces module to a name (scanned for shadowing)`);
    if (shadowed.length === 0) ok("...and none of them declares that name again, which is how the harness fixture was hidden (round 270)");
    else bad("a payload module shadows its own pieces binding", shadowed.join("; "));
  }

  // ── 8b. NO PROBE IS CALLED WITHOUT ITS SELECTOR (round 271) ────────────────────────────────────────────────
  // The root selector used to be baked into each probe's TEXT, so a call could not be wrong. It is an argument now:
  // `page.evaluate(SURFACE, SELECTOR)`. Forgetting the second argument throws in the page (a function receives
  // undefined and builds `undefined + ' *'`), and this is the cheap scan that refuses it before a device run finds out.
  {
    const dir = new URL("../../agent/scripts/lib/sweep/", import.meta.url);
    const { readdirSync, readFileSync } = await import("node:fs");
    const payloads = readdirSync(dir).filter((f) => f.endsWith(".cjs"));
    const unparameterised = [];
    let calls = 0;
    for (const f of payloads) {
      const src = readFileSync(new URL(f, dir), "utf8");
      const all = src.match(/page\.evaluate\(\s*(SURFACE|NAMES|REFLOW|MARKS)\s*(,[^)]*)?\)/g) || [];
      calls += all.length;
      for (const call of all) if (!/,/.test(call)) unparameterised.push(`${f}: ${call}`);
    }
    if (calls < 20) bad("the probes are called with a selector", `found ${calls} probe call(s) — this proves nothing`);
    else ok(`all ${calls} probe call(s) across ${payloads.length} payload module(s) pass the root selector`);
    if (unparameterised.length === 0) ok("...and none calls a probe with the selector left out");
    else bad("a probe is called without its selector — it would measure undefined", unparameterised.join("; "));
  }

  // ── 8c. NO EATEN BACKSLASH INSIDE THE TEXT THAT BECOMES THE PAYLOAD (round 272) ────────────────────────────
  // THIS IS THE ROUND-55 BUG, CHECKED WHERE IT CAN STILL HAPPEN. The emitted script used to BE a template literal, so
  // `/\s+/` written in the emitter reached the page as `/s+/` and the loud axis counted every element for
  // thirty-seven rounds; `panel-design-sweep.bash` walked the ARTIFACT for that. The payloads are real modules now and
  // hold no template literals of their own, so the walk only reacted to backticks in comments — the invariant moved
  // here, to the source an author actually writes. A template literal in a module is fine; a single backslash inside
  // one is eaten when the module runs, and this is the thing that says so.
  {
    const dir = new URL("../../agent/scripts/lib/sweep/", import.meta.url);
    const { readdirSync, readFileSync } = await import("node:fs");
    // THE FILES WHOSE TEXT ENDS UP IN THE ARTIFACT: the payload modules, and the library that holds every probe, pass
    // and *_SOURCE constant the emitters embed. Comments are stripped first (`decomment`) — a backtick quoted in a
    // comment is not a template literal, and counting those is what made the first version of this scan look busier
    // than it was (it reported 20 "literals", nearly all of them in the migration notes).
    const { decomment } = await import("./lib/decomment.mjs");
    const sources = readdirSync(dir).filter((f) => f.endsWith(".cjs")).map((f) => [f, new URL(f, dir)]);
    sources.push(["lib/design-sweep.mjs", new URL("../../agent/scripts/lib/design-sweep.mjs", import.meta.url)]);
    const problems = [];
    let literals = 0;
    for (const [f, url] of sources) {
      const src = decomment(readFileSync(url, "utf8"));
      let inside = false, line = 1, i = 0;
      while (i < src.length) {
        const ch = src[i];
        if (ch === "\n") line++;
        if (ch === "\\" && inside) {
          let j = i;
          while (j < src.length && src[j] === "\\") j++;
          const run = j - i;
          const next = j < src.length ? src[j] : "";
          // an EVEN run is a literal backslash and an ODD run ending on a backtick is an escaped backtick; an odd run
          // ending anywhere else escapes the next character at the template level and is eaten before the module runs
          if (run % 2 === 1 && next !== "`") problems.push(`${f}:${line}: a single backslash inside a template literal (\\${next}) is eaten when the payload runs`);
          i = j; continue;
        }
        if (ch === "`") { inside = !inside; if (inside) literals++; }
        i++;
      }
      if (inside) problems.push(`${f}: a template literal is never closed`);
    }
    if (problems.length === 0) {
      ok(`no eaten backslash in the text that becomes the payload (${literals} template literal(s) across ${sources.length} source file(s))`);
    } else bad("a payload template literal eats a backslash", problems.join("; "));
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
