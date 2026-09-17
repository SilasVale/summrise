#!/usr/bin/env node
// console-marks-check.mjs — the console's state dots must differ in SHAPE, and every shape must be legible.
//
// WHY THIS EXISTS (round 11 of the standing goal). The console carried three `.sig-dot` states — ok, err, off —
// as THREE IDENTICAL 7px CIRCLES distinguished only by fill colour. That is the arrangement the PANEL retired in
// round 245 (`lib/liveness.ts` + one shared `.mark[data-live=…]` rule), still live on the other front end, and the
// panel's own guards cannot see it: they read the panel's sheet. Measured on this one, the "off" dot was also
// 2.46:1 on `--bg` in the light theme — under the 3:1 a graphic needs — so it was not merely shapeless, it was
// barely there. The word beside each dot is what kept the row readable, which is exactly why nobody noticed: the
// row reads fine and the DOT says nothing.
//
// TWO RULES, and both are the panel's own, restated where the console can be held to them:
//
//   1. A SHAPE PER STATE. With the colour stripped out, two states that draw the same silhouette are one state.
//      The signature is (border-radius, border-style, transform) — the properties that make a circle a ring, a
//      diamond or a dash, and the ones a colour-blind reader depends on.
//   2. LEGIBLE INK, BOTH THEMES. A dot is a graphic: 3:1 against the surface it lands on. `--text-faint` failed
//      this in light at 2.46, which is why the fix moved the `off` dot to `--text-muted` (4.63 light / 6.58 dark).
//
// IT READS THE SOURCE SHEETS, because the console's `dist/` is a build artifact that is pruned and not committed —
// unlike the panel, whose build output IS the tracked mirror. The styles are plain CSS, so what is concatenated
// here is what the bundler emits.
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const STYLES = path.join(HERE, "..", "..", "gateway", "ui", "src", "styles");

const sheet = readdirSync(STYLES)
  .filter((f) => f.endsWith(".css"))
  .map((f) => readFileSync(path.join(STYLES, f), "utf8"))
  .join("\n")
  .replace(/\/\*[\s\S]*?\*\//g, "");

/** The states the console's device signals can be in. Named here so a state added to the component is a decision. */
const STATES = ["ok", "err", "off"];

const blockOf = (sel) => {
  const m = new RegExp(sel.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "\\s*\\{([^{}]*)\\}").exec(sheet);
  return m ? m[1] : null;
};

const tokensIn = (selector) => {
  const m = new RegExp(selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "\\s*\\{([\\s\\S]*?)\\n\\}").exec(sheet);
  const out = {};
  if (m) for (const d of m[1].matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out[d[1]] = d[2].trim();
  return out;
};

const light = tokensIn(":root");
const dark = { ...light, ...tokensIn('[data-theme="dark"]') };

const resolve = (value, tokens, depth = 0) => {
  if (depth > 6) return null;
  const v = String(value).trim();
  const m = /^var\((--[\w-]+)\s*(?:,\s*([^)]+))?\)$/.exec(v);
  if (m) {
    const next = tokens[m[1]];
    if (next) return resolve(next, tokens, depth + 1);
    return m[2] ? resolve(m[2], tokens, depth + 1) : null;
  }
  return v;
};

const parseColour = (value) => {
  const v = String(value || "").trim();
  const hex = /^#([0-9a-f]{6})$/i.exec(v);
  if (hex) {
    const n = parseInt(hex[1], 16);
    return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255, a: 1 };
  }
  const rgb = /^rgba?\(([^)]+)\)$/.exec(v);
  if (rgb) {
    const parts = rgb[1].split(/[\s,/]+/).filter(Boolean).map(Number);
    if (parts.length >= 3 && parts.slice(0, 3).every((x) => !Number.isNaN(x))) {
      return { r: parts[0], g: parts[1], b: parts[2], a: parts.length > 3 ? parts[3] : 1 };
    }
  }
  return null;
};

const luminance = ({ r, g, b }) => {
  const f = (v) => { const s = v / 255; return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4); };
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
};
const contrast = (a, b) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

const failures = [];
const signatures = new Map();

for (const state of STATES) {
  const sel = `.sig-dot.${state}`;
  const block = blockOf(sel);
  if (block === null) {
    failures.push(`${sel} is missing from the console sheet — ${state} has no dot to draw`);
    continue;
  }
  const prop = (name, fallback) => {
    const m = new RegExp(`(?:^|[;{\\s])${name}\\s*:\\s*([^;}]+)`).exec(block);
    return m ? m[1].trim() : fallback;
  };
  // the BASE rule fills in what a state does not restate, which is how CSS resolves it
  const base = blockOf(".sig-dot") || "";
  const baseProp = (name, fallback) => {
    const m = new RegExp(`(?:^|[;{\\s])${name}\\s*:\\s*([^;}]+)`).exec(base);
    return m ? m[1].trim() : fallback;
  };
  // THE BORDER STYLE ARRIVES INSIDE A SHORTHAND. `.sig-dot.off` writes `border: 1.5px dashed var(--text-muted)`,
  // and a `border-style` lookup finds nothing there — which made `off` and `ok` look like the same shape and made
  // this check report the fix as a defect. The panel's contrast test carries the identical lesson about
  // `border-bottom: 7px solid var(--warn-ink)`; read the shorthand, then find the keyword inside it.
  const borderDecl = [prop("border", baseProp("border", "")), prop("border-style", baseProp("border-style", ""))].join(" ");
  const shape = [
    prop("border-radius", baseProp("border-radius", "0")),
    /dashed|dotted|double|solid|none/.exec(borderDecl)?.[0] ?? "none",
    prop("transform", baseProp("transform", "none")),
  ].join("|");
  signatures.set(sel, shape);

  // the ink: a background, or the border when the background is transparent (which is what "off" is)
  const bgRaw = prop("background", baseProp("background", "transparent"));
  const borderRaw = prop("border", baseProp("border", ""));
  const borderColour = /(?:dashed|solid|dotted)\s+([^;]+)$/.exec(borderRaw)?.[1] ?? null;
  const inkRaw = /transparent/.test(bgRaw) && borderColour ? borderColour : bgRaw;

  for (const [theme, tokens] of [["light", light], ["dark", dark]]) {
    const ink = parseColour(resolve(inkRaw, tokens));
    const surface = parseColour(resolve(tokens["--bg"], tokens));
    if (!ink || !surface) {
      failures.push(`${sel} [${theme}]: could not resolve ${inkRaw} and --bg (a dot that cannot be measured is not a passing dot)`);
      continue;
    }
    const ratio = contrast(ink, surface);
    if (ratio < 3.0) failures.push(`${sel} [${theme}] ${inkRaw} on --bg = ${ratio.toFixed(2)} (a dot is a graphic; 3 is the bar)`);
  }
}

// TWO STATES, ONE SILHOUETTE, ONE STATE. With the colour gone they are indistinguishable, which is the rule the
// whole mark language exists for.
const seen = new Map();
for (const [sel, shape] of signatures) {
  if (seen.has(shape)) failures.push(`${sel} draws the same shape as ${seen.get(shape)} (${shape}) — strip the colour and they are one state`);
  else seen.set(shape, sel);
}

// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN.
if (signatures.size < STATES.length || Object.keys(light).length < 10) {
  console.error(`console-marks-check: FAILED — read ${signatures.size}/${STATES.length} signal dots and ${Object.keys(light).length} tokens, so this proves nothing`);
  process.exit(1);
}

if (failures.length) {
  console.error("console-marks-check: FAILED");
  for (const f of failures) console.error("  " + f);
  process.exit(1);
}
console.log(`console-marks-check: ok — ${signatures.size} device signals, ${new Set(signatures.values()).size} distinct silhouettes, every ink >= 3:1 on --bg in both themes`);
