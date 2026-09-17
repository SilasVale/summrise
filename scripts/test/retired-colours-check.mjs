#!/usr/bin/env node
// retired-colours-check.mjs — a colour that was replaced may not come back anywhere.
//
// WHY (round 236). The device's status page carried `#d9480f` long after the stylesheets replaced it with
// `#bf3a0a`. Nothing caught it, because the palette lives in FOUR places — the panel's CSS, the console's CSS,
// the extension's CSS, and a Rust string constant that renders the status page — and the checks all looked at
// the first three. Measured on the rendered pair: `#d9480f` on `#ffefe5` is 3.83 against the 4.5 AA needs for
// text, so the code chips on that page were the only sub-AA text in the product.
//
// WHAT THIS IS: a list of values that were REPLACED, each with the reason it was replaced, checked against every
// source file. It is deliberately not a palette check — the four palettes are allowed to differ, which is a
// brand question recorded in the operator's inbox. What they are not allowed to do is carry a value that was
// retired for a measured reason.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const ROOT = fileURLToPath(new URL("../../", import.meta.url));

// RETIRED, with the measurement or decision that retired it.
const RETIRED = [
  {
    value: "#d9480f",
    why: "the accent before round 79: white on it measured 4.30 and it as text on #fafafa measured 4.12, both under AA. Replaced by #bf3a0a (5.49 / 4.90).",
  },
];

const SCAN = ["agent/src", "agent/resources/panel-react/src", "agent/resources/panel-react/src/styles", "gateway/ui/src", "gateway/public", "extension", "index/public"];
const SKIP = /(node_modules|\.git|target|dist|assets\/index-)/;

function walk(dir, out = []) {
  let entries = [];
  try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return out; }
  for (const e of entries) {
    const full = path.join(dir, e.name);
    if (SKIP.test(full)) continue;
    if (e.isDirectory()) walk(full, out);
    else if (/\.(rs|ts|tsx|js|jsx|css|html|json|md)$/.test(e.name)) out.push(full);
  }
  return out;
}

// SURVIVORS, WAIVED WITH REASONS — the same way the sweeps waive what they cannot judge. These are REAL uses of
// the retired value that the first run of this check found, and they are not fixed here because each needs a
// measurement across two themes before a replacement can be chosen: #d9480f on #ffefe5 is 3.83, under the 4.5 AA
// wants, but the same value on a DARK soft surface may pass. Round 236 records them rather than guessing.
const WAIVED = [
  { file: "tokens.css", token: "--accent-ink", why: "the panel's ink-on-soft token still carries the retired accent; round 79 changed --accent and left its two ink siblings. Needs a per-theme measurement before a replacement is chosen." },
  { file: "globals.css", token: "--chrome-active-ink", why: "the console's active-tab ink, same story as --accent-ink." },
  { file: "TerminalPane.tsx", token: "cursor", why: "the terminal's cursor colour follows --accent by hand instead of reading it; the replacement is the same #bf3a0a the token uses." },
  { file: "themeContrast.test.ts", token: "prose", why: "a template literal QUOTING the old measurements as history, not a use — the test documents what the retired value measured." },
];
const waived = (rel, text) => WAIVED.some((w) => rel.endsWith(w.file) && (w.token === "prose" || text.includes(w.token)));

const files = SCAN.flatMap((d) => walk(path.join(ROOT, d)));
const hits = [];
for (const f of files) {
  if (f.endsWith("retired-colours-check.mjs")) continue;
  // COMMENTS ARE NOT USES. The first version of this failed on ten files — and every one of them mentioned the
  // value in a comment RECORDING its retirement ("was #d9480f — white on it measured 4.30"). That history is
  // exactly what should stay; a gate that deletes its own reasons is worse than no gate. So the scan reads the
  // code with comments removed, which is the same distinction round 234 drew: a comment is evidence, a value in
  // a rule or a string is a use.
  const text = readFileSync(f, "utf8")
    .replace(/\/\*[\s\S]*?\*\//g, " ")   // css and js block comments
    .replace(/\/\/[^\n]*/g, " ")           // js, ts and rust line comments
    .replace(/^\s*\*[^\n]*/gm, " ");       // rust doc-comment bodies
  for (const { value, why } of RETIRED) {
    const rel = path.relative(ROOT, f);
    if (text.toLowerCase().includes(value.toLowerCase()) && !waived(rel, text)) hits.push({ file: rel, value, why });
  }
}

// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN.
if (files.length < 100) {
  console.error(`retired colours: FAILED — the scan read only ${files.length} files, so it proves nothing`);
  process.exit(1);
}
if (hits.length) {
  console.error("retired colours: FAILED — a value that was replaced for a measured reason is back:");
  for (const h of hits) console.error(`  ${h.file}  ${h.value}\n      retired because ${h.why}`);
  process.exit(1);
}
console.log(`retired colours: ok — ${files.length} files · ${RETIRED.length} retired value(s) · ${WAIVED.length} known survivors, waived with reasons`);
for (const w of WAIVED) console.log(`    ${w.file} ${w.token} — ${w.why}`);
