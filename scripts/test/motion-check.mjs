#!/usr/bin/env node
// motion-check.mjs — NOTHING ANIMATES FOR SOMEONE WHO ASKED FOR NO MOTION.
//
// WHY THIS EXISTS (round 13 of the standing goal). The objective's clause is "the chrome neutral and still", and the
// halves of it were measured before this was written:
//
//   * WHAT IS LOUD: on the Terminal with a question pending, exactly ONE element carries a saturated fill
//     (`.approval-approve` — the operator's decision); on Settings, NONE. One focal point per surface, measured.
//   * WHAT MOVES AT REST: on three surfaces, the only animation running is `.xterm-cursor-blink` — xterm's own
//     cursor, not this UI's chrome. The chrome is still.
//
//   * AND WHAT THE SHEETS DECLARE: 12 running animations in the panel, 4 in the console. The panel silences each
//     one BY NAME inside `prefers-reduced-motion`. The console had a single block setting `--ds-dur: 0s` — which
//     reaches every transition written with that token and NOT ONE of its four animations, because each declares
//     its own literal duration. So a user who asked for reduced motion still got a shimmer, a spinner and two
//     entrances. That is the defect this file exists to keep fixed.
//
// THE RULE: every selector that runs an animation must be silenced in a `prefers-reduced-motion` block — by name,
// or by a global `*` idiom. A clean result nothing enforces is one commit from not being true.
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.join(HERE, "..", "..");

/** Everything inside `@media (prefers-reduced-motion…) { … }`, with balanced braces. */
function reducedMotionBlocks(css) {
  const blocks = [];
  const re = /@media[^{]*prefers-reduced-motion[^{]*\{/g;
  let m;
  while ((m = re.exec(css))) {
    let depth = 1;
    let i = m.index + m[0].length;
    const start = i;
    while (i < css.length && depth > 0) {
      if (css[i] === "{") depth++;
      else if (css[i] === "}") depth--;
      i++;
    }
    blocks.push(css.slice(start, i - 1));
  }
  return blocks;
}

const rules = (css) => {
  const clean = css.replace(/\/\*[\s\S]*?\*\//g, "");
  return [...clean.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({
    sel: m[1].trim().replace(/\s+/g, " "),
    body: m[2],
  }));
};

/** A selector as it would be written in a rule: without its state qualifiers, lower-cased, sorted. */
const base = (sel) => sel.replace(/::?[a-z-]+(\([^)]*\))?/g, "").replace(/\[[^\]]*\]/g, "").trim();

const sheets = [];
sheets.push(["panel", readFileSync(path.join(ROOT, "agent/resources/panel/panel.css"), "utf8")]);
const consoleStyles = path.join(ROOT, "gateway/ui/src/styles");
sheets.push([
  "console",
  readdirSync(consoleStyles)
    .filter((f) => f.endsWith(".css"))
    .map((f) => readFileSync(path.join(consoleStyles, f), "utf8"))
    .join("\n"),
]);

const failures = [];
let animated = 0;

for (const [name, css] of sheets) {
  const rm = reducedMotionBlocks(css);
  const silenced = new Set();
  let globalStop = false;
  for (const block of rm) {
    for (const r of rules(block)) {
      if (!/(?:^|[;{\s])animation(?:-name)?\s*:\s*(none|0)/.test(r.body)) continue;
      for (const one of r.sel.split(",")) {
        const s = one.trim();
        if (s === "*" || s.startsWith("*") || s.includes("::before")) globalStop = true;
        silenced.add(s);
        silenced.add(base(s));
      }
    }
  }

  for (const r of rules(css)) {
    const decl = /(?:^|[;{\s])animation\s*:\s*([^;}]+)/.exec(r.body);
    if (!decl) continue;
    const value = decl[1].trim();
    if (/^none\b/.test(value)) continue; // already stopped — this IS the off switch
    animated++;
    for (const one of r.sel.split(",")) {
      const s = one.trim();
      // `@keyframes` bodies and `from`/`to` steps are not selectors that need silencing.
      if (/^(from|to|\d+%)$/.test(s)) continue;
      if (globalStop || silenced.has(s) || silenced.has(base(s))) continue;
      failures.push(`${name}: ${s} runs "${value.slice(0, 30)}" and no prefers-reduced-motion block stops it`);
    }
  }
}

// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN. Both sheets ran 16 animations when this was written.
if (animated < 10) {
  console.error(`motion-check: FAILED — found ${animated} running animations, which is too few to be reading both sheets`);
  process.exit(1);
}

if (failures.length) {
  console.error("motion-check: FAILED — motion that cannot be turned off:");
  for (const f of failures) console.error("  " + f);
  console.error("\n  A `--dur: 0s` token does NOT reach an `animation:` that declares its own duration. Name the");
  console.error("  selector in a `prefers-reduced-motion` block, and keep the STATE readable without the motion.");
  process.exit(1);
}
console.log(`motion-check: ok — ${animated} animations across both sheets, every one silenced under prefers-reduced-motion`);
