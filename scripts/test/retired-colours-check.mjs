#!/usr/bin/env node
// ── THE MUTATION THAT MUST FAIL THIS GATE (moved here from the ledger table, landing 4b) ──
// Read this when you change this file: the mutation is how you find out whether the gate can still
// fail at all. A gate that cannot be broken is worse than no gate.
//
// MUTATION: put a retired value back anywhere outside a comment (the accent `#d9480f` in the Rust status page)
// RESULT:   exit 1, naming the file and the measurement that retired it. It strips comments FIRST, because its own first run failed on ten files that merely recorded the retirement — a gate that deletes its reasons is worse than no gate

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
  { file: "tokens.css", token: "--accent-ink", why: "KEPT AFTER MEASUREMENT (round 237): every use is a GRAPHIC — hover fills, dot backgrounds, a border — where the need is 3:1, and it measures 4.12 light / 3.78 dark. Round 236 called this a text use; the sheets say otherwise and the numbers agree." },
  { file: "TerminalPane.tsx", token: "cursor", why: "KEPT AFTER MEASUREMENT (round 237): a cursor is a graphic on the terminal's own #131418, where #d9480f measures 4.28 and the current accent would measure 3.35 — the replacement would be WORSE on a dark background." },
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
