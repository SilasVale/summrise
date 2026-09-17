#!/usr/bin/env node
// particles-check.mjs — THREE COPIES OF ONE FIELD, HELD TO ONE SET OF FACTS.
//
// WHY THIS EXISTS (round 15 of the standing goal). The ambient particle field exists three times — the panel and
// the console as TypeScript modules (a canvas behind every surface), the landing inlined in its page script
// (because that page has no bundler). The objective's spine says no surface should compute its own version of the
// same fact, and this is the one place where the copies are unavoidable; so the FACTS they must share are checked
// instead of the code being shared.
//
// They had already drifted into the same two defects, one round apart:
//
//   * THE RETIRED PALETTE. Both modules asked for `--aura-1/3/4` — tokens the rebrand REMOVED — read empty, and
//     fell back to 190 cyan / 280 violet / 330 pink: the background of the whole product in the colours the brand
//     had abandoned. (Round 14 fixed that, from the operator's inbox.)
//   * DRAWING BY HUE. All three converted a token to a hue and drew `hsla(h 90% 62%)`, so even a correct token
//     would not have been the colour on screen. A palette entry is a COLOUR, and the field draws it as one.
//
// THE THREE FACTS, each checkable in the source and each one the shape of a real defect:
//
//   1. IT READS BRAND TOKENS, and no retired one.
//   2. EVERY HEX IT FALLS BACK TO IS ONE OF THE BRAND'S. A fallback is what people see when a token is missing —
//      which is exactly how the cyan arrived — so a fallback that is not a brand colour is the defect itself.
//   3. IT DRAWS WITH `rgba(` FROM A TRIPLE, not `hsla(` from a hue.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

/** Every copy of the field, and the reason it is its own copy. */
const FIELDS = [
  ["agent/resources/panel-react/src/lib/particles.ts", "the panel's — a module, bundled"],
  ["gateway/ui/src/lib/particles.ts", "the console's — a module, bundled"],
  ["index/src/page.js", "the landing's — INLINE, because that page has no bundler"],
];

/** The brand's own values, across all three surfaces: the white-text gradient pair, the accent, and the mark. */
const BRAND_HEXES = new Set(["#c2410c", "#9a3412", "#bf3a0a", "#f59f00", "#e8590c", "#ffd43b"]);

const failures = [];
let fields = 0;

for (const [rel, why] of FIELDS) {
  const src = readFileSync(path.join(ROOT, rel), "utf8");
  // only the FIELD's own code: the file may mention a token in a comment explaining why it left
  const code = src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "").replace(/^\s*\*.*$/gm, "").replace(/^\s*\/\/.*$/gm, "");
  fields++;

  // ── 1. brand tokens, and no retired one
  const tokens = [...code.matchAll(/["'`](--[a-z0-9-]+)["'`]/g)].map((m) => m[1]);
  const brand = tokens.filter((t) => /^--brand-(grad|mark)-/.test(t));
  const retired = tokens.filter((t) => /^--aura-/.test(t));
  if (brand.length < 2) failures.push(`${rel} (${why}) reads ${brand.length} brand token(s) — the field must take its palette from the brand`);
  if (retired.length) failures.push(`${rel} reads the RETIRED ${retired.join(", ")} — that is the palette the rebrand removed`);

  // ── 2. the fallbacks are the brand's
  const fallbacks = [...code.matchAll(/["'`](#[0-9a-f]{6})["'`]/gi)].map((m) => m[1].toLowerCase());
  for (const hex of fallbacks) {
    if (!BRAND_HEXES.has(hex)) failures.push(`${rel} falls back to ${hex}, which is not one of the brand's colours — a fallback is what people SEE when a token is missing`);
  }
  if (fallbacks.length < 2) failures.push(`${rel} has ${fallbacks.length} hex fallback(s) — a reader with no fallback draws nothing sensible`);

  // ── 3. drawn as a colour, not as a hue
  if (/hsla\(\s*['"]?\s*\+?\s*(m\.)?hue/.test(code) || /hsla\(['"]\s*\+\s*m\.hue/.test(code)) {
    failures.push(`${rel} draws hsla() from a hue — a token is a COLOUR, and the field must draw that colour`);
  }
  if (!/rgba\(/.test(code)) failures.push(`${rel} never draws rgba() — the palette is supposed to arrive as a colour triple`);

  // ── 4. IT REFUSES TO RUN AT ALL UNDER prefers-reduced-motion.
  // A canvas loop is not a CSS animation, so `motion-check.mjs` cannot see it: it reads the sheets, and this is
  // JavaScript painting every 1/30th of a second forever. All three copies return before creating their canvas,
  // which is the honest fallback — the static wash stays, the motion does not — and VERIFIED ON THE DEVICE:
  // with reduced motion emulated, rAF 0/s, clears 0/s, fills 0/s and no canvas in the DOM at all.
  if (!/prefers-reduced-motion/.test(code)) {
    failures.push(`${rel} runs without checking prefers-reduced-motion — a canvas loop is motion the CSS gate cannot see`);
  }
}

// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN.
if (fields < 3) {
  console.error(`particles-check: FAILED — read ${fields} field(s), and there are three`);
  process.exit(1);
}

if (failures.length) {
  console.error("particles-check: FAILED — the copies of the ambient field no longer agree on the facts:");
  for (const f of failures) console.error("  " + f);
  process.exit(1);
}
console.log(`particles-check: ok — ${fields} copies of the ambient field: brand tokens as colours, brand fallbacks, and no motion under prefers-reduced-motion`);
