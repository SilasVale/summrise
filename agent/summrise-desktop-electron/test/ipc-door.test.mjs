// THE IPC DOOR AND THE LOAD DOOR, pinned as a SOURCE check.
//
// WHY A SOURCE CHECK: `main.ts` imports electron, which this suite deliberately does not install (it
// runs on node builtins alone, like the CLI suite), so nothing here can import the shell and execute
// a handler. `test/embedded-bridge.test.mjs` had the same problem and solved the same way — parse the
// text. The property that made a fourteenth handler SILENT was structural, so the text is where it
// can be caught: a handler that calls `ipcMain.handle` itself brings its own frame check (or forgets
// one) and its own refusal shape, and none of that is visible to any runtime test this package can
// run. Counting the call sites is the instrument.
//
// What the shell promises, checked below:
//   * `ipcMain.handle` is called exactly ONCE — inside the door helper, which applies the frame check;
//   * a forbidden frame gets ONE refusal shape, `{ ok: false, error: "forbidden frame" }` — never the
//     bare `{ ok: false }` the SPA cannot tell apart from a dead view;
//   * the channels the preload invokes and the channels main.ts registers are the SAME set, so a
//     handler cannot appear without a caller or a caller without a handler;
//   * `sanitizeBrowserUrl` is called exactly ONCE — inside the one load door.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const MAIN = path.join(HERE, "..", "src", "main.ts");
const PRELOAD = path.join(HERE, "..", "src", "preload.ts");

/** The source with comments removed — a rule about CODE must not be satisfied or broken by prose
 *  (this file's own header mentions `ipcMain.handle` and `sanitizeBrowserUrl`), while STRINGS are
 *  kept, because the channel names being compared ARE string literals. Hand-rolled rather than
 *  regexed so `//` inside a URL literal is not mistaken for a comment. */
function codeOnly(source) {
  let out = "";
  for (let i = 0; i < source.length; i++) {
    const c = source[i];
    const next = source[i + 1];
    if (c === "/" && next === "/") {
      while (i < source.length && source[i] !== "\n") i++;
      out += "\n";
      continue;
    }
    if (c === "/" && next === "*") {
      i += 2;
      while (i < source.length && !(source[i] === "*" && source[i + 1] === "/")) i++;
      i += 1;
      continue;
    }
    if (c === '"' || c === "'" || c === "`") {
      out += c;
      for (i++; i < source.length && source[i] !== c; i++) {
        if (source[i] === "\\") {
          out += source[i];
          i++;
        }
        if (i < source.length) out += source[i];
      }
      out += c;
      continue;
    }
    out += c;
  }
  return out;
}

/** The body of a top-level `function …` declaration, by brace matching. */
function functionBody(code, declaration) {
  const at = code.indexOf(declaration);
  assert.ok(at > -1, `main.ts must declare ${declaration}`);
  const open = code.indexOf("{", at);
  let depth = 0;
  for (let i = open; i < code.length; i++) {
    if (code[i] === "{") depth += 1;
    else if (code[i] === "}") {
      depth -= 1;
      if (depth === 0) return code.slice(open + 1, i);
    }
  }
  assert.fail(`${declaration} must have a body`);
}

const matches = (code, re) => [...code.matchAll(re)].map((m) => m[1] ?? m[0]);

test("main.ts calls ipcMain.handle exactly once — inside the door helper", () => {
  const code = codeOnly(readFileSync(MAIN, "utf8"));
  const handles = matches(code, /ipcMain\.handle\s*\(/g);
  assert.equal(
    handles.length,
    1,
    `main.ts must call ipcMain.handle exactly ONCE (found ${handles.length}) — a handler registered outside the door does its own frame check, or none, silently`,
  );
  // The frame check exists in TWO code places, and both are the door: the predicate's definition and
  // its single call. A third use would mean a handler gated itself.
  const frameChecks = matches(code, /frameOk\s*\(/g);
  assert.equal(
    frameChecks.length,
    2,
    `frameOk must have one definition and one call (found ${frameChecks.length}) — the door is the ONLY place a frame is judged`,
  );
  const door = functionBody(code, "function ipcHandle");
  assert.ok(door.includes("ipcMain.handle"), "the door helper must be the call site of ipcMain.handle");
  assert.match(
    door,
    /if\s*\(!frameOk\(e\)\)\s*return\s+FORBIDDEN_FRAME\s*;/,
    "the door must return the ONE refusal for a forbidden frame",
  );
  // ONE refusal shape, stated once. It used to be two (`{ ok: false }` for six handlers), and the SPA
  // reads `j?.ok`, so the bare form was indistinguishable from a dead view.
  const refusals = matches(code, /forbidden frame/g);
  assert.equal(refusals.length, 1, `the refusal must be stated once (found ${refusals.length}) — a second copy is a second shape`);
  const declaration = code.slice(code.indexOf("const FORBIDDEN_FRAME"), code.indexOf("const FORBIDDEN_FRAME") + 200);
  assert.match(declaration, /ok:\s*false/, "the refusal must answer ok: false");
  assert.match(declaration, /error:\s*"forbidden frame"/, 'the refusal must carry error: "forbidden frame"');
});

test("the channels the preload invokes are exactly the channels the door registers", () => {
  const preload = codeOnly(readFileSync(PRELOAD, "utf8"));
  const main = codeOnly(readFileSync(MAIN, "utf8"));
  const invoked = matches(preload, /ipcRenderer\.invoke\(\s*"([^"]+)"/g);
  const handled = matches(main, /ipcHandle\(\s*"([^"]+)"/g);
  assert.ok(invoked.length > 0, "the preload must invoke at least one channel");
  assert.deepEqual(
    handled.slice().sort(),
    invoked.slice().sort(),
    "every channel the preload invokes must be registered behind the door, and every channel the door registers must be one the preload exposes — a missing handler is a dead bridge, an extra one is unexposed attack surface",
  );
});

test("main.ts decides every raw browser URL in exactly one place", () => {
  const code = codeOnly(readFileSync(MAIN, "utf8"));
  const calls = matches(code, /sanitizeBrowserUrl\s*\(/g);
  assert.equal(
    calls.length,
    1,
    `main.ts must call sanitizeBrowserUrl exactly ONCE (found ${calls.length}) — the load door owns the policy, so a new load site cannot pick a weaker one`,
  );
  const door = functionBody(code, "function loadTarget");
  assert.ok(door.includes("sanitizeBrowserUrl"), "the load door must be the call site of sanitizeBrowserUrl");
  // The door is USED, not merely declared: its own declaration plus the three load sites
  // (browserOpen, the embedded view's window-open handler, embeddedNavigate).
  const uses = matches(code, /loadTarget\s*\(/g);
  assert.ok(
    uses.length >= 4,
    `the three load sites must reach the policy through loadTarget() (found ${uses.length} uses including the declaration)`,
  );
});
