#!/usr/bin/env node
// ── THE MUTATION THAT MUST FAIL THIS GATE (moved here from the ledger table, landing 4b) ──
// Read this when you change this file: the mutation is how you find out whether the gate can still
// fail at all. A gate that cannot be broken is worse than no gate.
//
// MUTATION: give two console signal states the same silhouette (put `off` back to a plain circle), or put the failing ink back (`--text-faint` for `off`)
// RESULT:   exit 1 either way: ".sig-dot.off draws the same shape as .sig-dot.ok (50%

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

/**
 * EVERY MARK THE CONSOLE DRAWS A STATE WITH. `.sig-dot` was the first family and the only one this check knew; the
 * LED families below kept the pre-round-11 arrangement — identical circles told apart by fill colour — for
 * thirteen rounds because nothing looked at them (round 24 of the standing goal). A family names its base
 * selector and each state's modifier; the empty string is the base rule itself.
 */
const MARKS = [
  { what: "device signal", base: ".sig-dot", states: ["ok", "err", "off"] },
  { what: "key LED", base: ".ov-keyled", states: ["", "on"] },
  // The BASE is not a state here: ui.tsx renders a bare "dot" as decoration, and the four that carry state are
  // the two the Overview draws (ok/err) and the two the connection row draws (online/offline).
  // TWO STATES, NOT FOUR (round 44): `online` had no producer and `offline` restated the base rule, so both were
  // pruned from the sheet. The family is the LIVE pair, and the ring that "offline" used to spell out is the base.
  { what: "connection dot", base: ".dot", states: ["ok", "err"] },
  // The two LEDs followed the same path: `.dev-led` had NO mark at all for "off" and `.dev-mini-led` said it with a
  // grey disc. Both are rings now, which is what the rest of this sheet already means by absent.
  // NO `off` IN THIS LIST, AND THAT IS CORRECT (round 76, learned by trying it): the DOM renders `dev-led off`, and
  // the BASE is what paints it, so adding `off` reports the base and the arm as "one state" the moment the arm is
  // removed — which it was, because the arm was the defect. The states a family lists are the states that have rules
  // of their own; a class that resolves to the base is the base. (The rendered probe covers what this cannot: it
  // walks the DOM, so an inherited cascade is visible to it and to nothing here.)
  { what: "device LED", base: ".dev-led", states: ["", "on"] },
  { what: "mini LED", base: ".dev-mini-led", states: ["", "on"] },
];

/**
 * The body of ONE rule, matched at a SELECTOR BOUNDARY.
 *
 * The first version was a substring match, so `blockOf(".dot")` found `.badge .dot {` — an earlier rule that
 * happens to contain the selector — and read its body: no box-shadow there, so the mark's ink came out as
 * `transparent` and the check reported a dot it could not measure. A boundary is the start of the sheet or a `}`
 * that ended the previous rule.
 */
const blockOf = (sel) => {
  const m = new RegExp("(?:^|[}\\n])\\s*" + sel.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "\\s*\\{([^{}]*)\\}").exec(sheet);
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

for (const family of MARKS) {
  // WITHIN A FAMILY, never across: a solid mark with a halo means "on" in more than one place on purpose — the
  // vocabulary is SHARED, which is the whole point of a mark language. The first version of this compared every
  // signature with every other and reported two families for agreeing.
  const familyShapes = [];
  for (const state of family.states) {
    const sel = state ? `${family.base}.${state}` : family.base;
    const block = blockOf(sel);
    if (block === null) {
      failures.push(`${sel} is missing from the console sheet — ${family.what} has no ${state || "base"} state to draw`);
      continue;
    }
    const base = blockOf(family.base) || "";
    const prop = (name, from) => {
      const m = new RegExp(`(?:^|[;{\\s])${name}\\s*:\\s*([^;}]+)`).exec(from);
      return m ? m[1].trim() : "";
    };
    const val = (name) => prop(name, block) || prop(name, base);

    const bg = val("background") || val("background-color") || "transparent";
    const shadow = val("box-shadow");
    // THE FILL KIND IS PART OF THE SHAPE: a solid mark, a ring (an inset shadow or a border with no fill), and a
    // halo (a fill plus an outer shadow) are three different things to look at even when the geometry matches.
    const filled = bg !== "" && !/transparent|none/.test(bg);
    const inset = /inset/.test(shadow);
    const outer = !!shadow && shadow !== "none" && !/inset/.test(shadow);   // "none" is not a shadow
    // A FILL AND A RING AT ONCE IS ITS OWN KIND, and reducing it to "ring" is how round 44 found a real defect with a
    // mutation that did NOT bite: `.dot.err` set a fill and inherited the base's inset ring, so a failing channel
    // drew a red square INSIDE a grey ring — and the signature called it a ring, distinct from `.dot.ok`'s solid, so
    // it passed. A kind that hides one of the two channels is not a silhouette. (The panel's marks test learned the
    // same lesson in round 25 from the other end: a stray halo the sheet-level check could not see.)
    const kind = inset && filled ? "ring+fill" : inset ? "ring" : filled && outer ? "halo" : filled ? "solid" : "empty";

    // A MARK THAT IS BOTH A FILL AND A RING IS NEITHER, and the vocabulary has no such state: solid, ring, halo and
    // empty are the four, and "ring+fill" is what a rule produces by accident — setting a background while
    // inheriting an inset shadow. That is exactly the `.dot.err` defect of round 44, and this is the assertion that
    // makes it fail rather than merely look different.
    if (kind === "ring+fill") {
      failures.push(`${sel} is a FILL inside a RING — the vocabulary is solid / ring / halo / empty, and a mark that is two of them is neither. Add box-shadow: none for a fill, or drop the background for a ring`);
    }
    const borderDecl = [val("border"), val("border-style")].join(" ");
    const shape = [
      val("border-radius") || "0",
      /dashed|dotted|double|solid|none/.exec(borderDecl)?.[0] ?? "none",
      val("transform") || "none",
      kind,
    ].join("|");
    signatures.set(sel, shape);
    familyShapes.push([sel, shape]);

    // the ink: the fill, the border colour, or the shadow's colour
    const borderColour = /(?:dashed|solid|dotted)\s+([^;]+)$/.exec(borderDecl)?.[1] ?? null;
    // ANY COLOUR-ISH TOKEN IN THE SHADOW, wherever it sits: `inset 0 0 0 1.5px var(--text-muted)` keeps its colour
    // at the END, and the first version of this looked for one at the start or after a space and found nothing —
    // so a ring's ink read as `transparent` and the check reported a mark it could not measure.
    const shadowColour = /(var\(--[\w-]+\)|#[0-9a-f]{3,8}\b|rgba?\([^)]+\))/.exec(shadow)?.[1] ?? null;
    const inkRaw = filled ? bg : inset && shadowColour ? shadowColour : borderColour || bg;

    for (const [theme, tokens] of [["light", light], ["dark", dark]]) {
      const ink = parseColour(resolve(inkRaw, tokens));
      const surface = parseColour(resolve(tokens["--bg"], tokens));
      if (!ink || !surface) {
        failures.push(`${sel} [${theme}]: could not resolve ${inkRaw} and --bg (a mark that cannot be measured is not a passing mark)`);
        continue;
      }
      const ratio = contrast(ink, surface);
      if (ratio < 3.0) failures.push(`${sel} [${theme}] ${inkRaw} on --bg = ${ratio.toFixed(2)} (a mark is a graphic; 3 is the bar)`);
    }
  }
}

// TWO STATES, ONE SILHOUETTE, ONE STATE. With the colour gone they are indistinguishable, which is the rule the
// whole mark language exists for.
for (const family of MARKS) {
  const seen = new Map();
  for (const state of family.states) {
    const sel = state ? `${family.base}.${state}` : family.base;
    const shape = signatures.get(sel);
    if (shape === undefined) continue;
    if (seen.has(shape)) {
      failures.push(`within ${family.what}: ${sel} draws the same shape as ${seen.get(shape)} (${shape}) — strip the colour and they are one state`);
    } else seen.set(shape, sel);
  }
}

// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN.
const EXPECTED = MARKS.reduce((n, m) => n + m.states.length, 0);
if (signatures.size < EXPECTED || Object.keys(light).length < 10) {
  console.error(`console-marks-check: FAILED — read ${signatures.size}/${EXPECTED} marks and ${Object.keys(light).length} tokens, so this proves nothing`);
  process.exit(1);
}

if (failures.length) {
  console.error("console-marks-check: FAILED");
  for (const f of failures) console.error("  " + f);
  process.exit(1);
}
console.log(`console-marks-check: ok — ${signatures.size} state marks across ${MARKS.length} families, ${new Set(signatures.values()).size} distinct signatures, every ink >= 3:1 on --bg in both themes`);
