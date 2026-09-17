#!/usr/bin/env node
// stylesheet-hygiene.mjs — a comment in a stylesheet must not look like code.
//
// WHY THIS EXISTS (round 87). The console's sheet carried this:
//
//     /* ── Model catalogue admin: add form, per-chip remove {
//       display: flex;
//       align-items: center;
//       gap: 8px;
//       flex-wrap: wrap;
//     }
//     .model-add .form-input {
//       flex: 0 1 auto;
//       min-width: 0;
//     }
//
// Four `/*` and one `*/` in that region. A CSS comment runs to the NEXT `*/`, so the real rule
// `.model-add .form-input` was inside a comment and had never applied — while every instrument that
// reads the sheet as TEXT (the pair sweep, the dead-CSS pruner, both design contracts) saw it and
// judged it as live. A browser and a regex disagreed, and only the browser was rendering.
//
// The panel had the other shape of the same problem: a comment reading "--muted on --surface-chip
// measured 4.40 in the running app" contains `--muted: … ;` — declaration-shaped prose. Round 86
// proved that pattern can fool a token reader (it made `--surface` resolve to a sentence). Comments
// are documentation; they must not be parseable as rules.
//
// WHAT IT CHECKS, on every stylesheet the repo ships or serves:
//   1. comments are BALANCED (a `/*` without its `*/` swallows whatever follows);
//   2. no comment contains a rule-like selector line (`{`-terminated) or a declaration-shaped
//      `--token: value;`.
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import assert from "node:assert/strict";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

const SHEETS = [
  "agent/resources/panel/panel.css",
  "gateway/ui/src/styles/globals.css",
];

let checked = 0;
const problems = [];

for (const rel of SHEETS) {
  const full = path.join(ROOT, rel);
  if (!existsSync(full)) {
    problems.push(`${rel}: missing — a sheet this check expects to read is gone`);
    continue;
  }
  const css = readFileSync(full, "utf8");
  checked++;

  // 1. Walk it the way a parser does: each comment ends at the NEXT */.
  const spans = [];
  let pos = 0;
  for (;;) {
    const a = css.indexOf("/*", pos);
    if (a < 0) break;
    const b = css.indexOf("*/", a + 2);
    if (b < 0) {
      spans.push([a, css.length]);
      problems.push(`${rel}:${css.slice(0, a).split("\n").length}: UNTERMINATED comment — it swallows the rest of the file`);
      break;
    }
    spans.push([a, b + 2]);
    pos = b + 2;
  }

  for (const [a, b] of spans) {
    const body = css.slice(a, b);
    const line = css.slice(0, a).split("\n").length;
    const selectors = body.match(/^[ \t]*[.#][A-Za-z][\w.\- ]*\{/gm) ?? [];
    if (selectors.length) {
      problems.push(`${rel}:${line}: comment contains ${selectors.length} rule-like line(s), e.g. "${selectors[0].trim().slice(0, 40)}" — a comment must not hold a rule`);
    }
    // A COMMENT THAT SWALLOWED A RULE'S SELECTOR (round 23). The selector-shaped check above needs a leading
    // `.` or `#`, so it missed real damage a previous prune left in the console's sheet: a selector line became
    // `is not a dot {` — no dot, no hash — and the comment went on holding `margin-bottom: 14px;` and its closing
    // brace for as long as nobody read it. `.models-card` lost its margin and the sheet stopped parsing there.
    //
    // WHAT IS UNAMBIGUOUS IS THE SHAPE, not the name: a line inside a comment that ends in `{` and is followed by
    // something declaration-shaped. No stylesheet documents itself that way by accident.
    const swallowed = body.match(/^[^\n]*\{[ \t]*\n[ \t]*[a-z-]+\s*:\s*[^;\n]+;/m);
    if (swallowed) {
      problems.push(
        `${rel}:${line}: a comment has swallowed a rule — "${swallowed[0].split("\n")[0].trim().slice(0, 40)}" is ` +
          `followed by a declaration, so a selector was lost (in a sheet, that rule no longer exists)`,
      );
    }
    const decl = body.match(/--[a-z0-9-]+\s*:\s*[^;*\n]+;/);
    if (decl) {
      problems.push(`${rel}:${line}: comment contains a declaration-shaped token "${decl[0].slice(0, 40)}" — a token reader can mistake prose for a value (round 86)`);
    }
  }
}

// A check that reads nothing must not report success.
assert.ok(checked === SHEETS.length, `expected to read ${SHEETS.length} stylesheets, read ${checked}`);
if (problems.length) {
  console.error(`${problems.length} stylesheet hygiene problem(s):`);
  for (const p of problems) console.error("  " + p);
  process.exit(1);
}
console.log(`stylesheet-hygiene: ${checked} sheet(s) clean — comments are balanced and none of them reads as code`);
