#!/usr/bin/env node
// ── THE MUTATION THAT MUST FAIL THIS GATE (moved here from the ledger table, landing 4b) ──
// Read this when you change this file: the mutation is how you find out whether the gate can still
// fail at all. A gate that cannot be broken is worse than no gate.
//
// MUTATION: plant an ORPHANED `*/` outside any comment (round 97), or the round-87 shapes (a comment holding a rule-like selector line, or declaration-shaped prose)
// RESULT:   exit 1: "ORPHANED '*/' outside any comment — the browser reads it as a parse error and drops rules until the next '}'". The walk finds `/*` and takes the NEXT `*/`, so a stray close used to be skipped as ordinary text while the BROWSER discarded everything up to the next `}` — one stray close silently deleted a large region of the panel's sheet (marks lost their size, session rows lost `display: flex`) and every gate stayed green. The clause checks the GAPS between the comment spans, which is exactly where the walk cannot look

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
//      `--token: value;`;
//   3. NO ORPHANED `*/` OUTSIDE a comment (round 97). The walk below finds `/*` and takes the NEXT `*/`, so a
//      stray close — a comment that ended early, with prose after it — is skipped over as ordinary text and the
//      sheet looks clean. THE BROWSER DOES NOT SKIP IT: `*/` in a selector prelude is a parse error, and the
//      parser then discards everything up to the next `}`, so ONE stray close silently deleted a large region of
//      the panel's sheet — the marks, the session rows and the marks' own sizes stopped applying — while every
//      gate here stayed green and the page merely LOOKED wrong.
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

  // 3. AN ORPHANED CLOSE IS A PARSE ERROR, NOT TEXT. Checked in the GAPS between the spans, because that is
  // exactly where the walk above cannot see one.
  const orphanLines = [];
  let cursor = 0;
  for (const [a, b] of spans) {
    const at = css.slice(cursor, a).indexOf("*/");
    if (at >= 0) orphanLines.push(css.slice(0, cursor + at).split("\n").length);
    cursor = b;
  }
  const tailAt = css.slice(cursor).indexOf("*/");
  if (tailAt >= 0) orphanLines.push(css.slice(0, cursor + tailAt).split("\n").length);
  for (const line of orphanLines) {
    problems.push(`${rel}:${line}: ORPHANED '*/' outside any comment — the browser reads it as a parse error and drops rules until the next '}', which is how one stray close deleted a large region of this sheet`);
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
