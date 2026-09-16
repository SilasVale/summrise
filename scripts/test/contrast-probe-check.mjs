#!/usr/bin/env node
// contrast-probe-check.mjs — the contrast math, pinned.
//
// Four rounds of contrast sweeps used an ad-hoc snippet retyped each time, and it
// was wrong twice. Both defects are assertions here, with the numbers they cost:
//
//   1. TRANSLUCENT BACKGROUNDS. Reading `rgba(255,255,255,0.07)` as if it were
//      white reported 2.51 for text that actually sits on a composited
//      rgb(44,45,49) and measures 5.49 — twenty of fifty findings were this.
//   2. A SKIP RULE THAT DISABLED THE SWEEP. Skipping anything with a
//      background-image ancestor skipped EVERY node in the panel and reported
//      `checked=0, underAA=0`, which reads exactly like a pass.
//
// The functions tested here are the ones the BROWSER runs: `PROBE_SOURCE` embeds
// them with `Function.prototype.toString()`, so this is not a copy that can drift.
import {
  compositeStack, contrastRatio, aaThreshold, parseColour, failures, unmeasurable, inactive, PROBE_SOURCE,
  gradientStops, worstOverGradient,
} from "../../agent/scripts/lib/contrast-probe.mjs";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

let n = 0;
const t = (desc, fn) => { fn(); n += 1; };

t("a translucent white over a dark base composites — it is NOT white", () => {
  const bg = compositeStack([{ r: 255, g: 255, b: 255, a: 0.07 }], { r: 28, g: 29, b: 34 });
  assert.ok(bg.r > 40 && bg.r < 50, `expected a dark grey, got ${JSON.stringify(bg)}`);
  // ...and the difference is the whole point.
  assert.equal(contrastRatio({ r: 162, g: 163, b: 172 }, bg), 5.49);
  assert.equal(contrastRatio({ r: 162, g: 163, b: 172 }, { r: 255, g: 255, b: 255 }), 2.51);
});

t("the stack applies innermost LAST (order is the defect both times)", () => {
  // Element's own bg first in the array, then its parent, then the canvas.
  const over = compositeStack([
    { r: 255, g: 255, b: 255, a: 0.5 },   // the element
    { r: 0, g: 0, b: 0, a: 1 },           // an opaque parent stops the walk
  ]);
  assert.deepEqual(over, { r: 127.5, g: 127.5, b: 127.5 });
});

t("an opaque layer ends the walk regardless of what is beneath", () => {
  const over = compositeStack([{ r: 19, g: 20, b: 24, a: 1 }, { r: 255, g: 0, b: 0, a: 1 }]);
  assert.deepEqual(over, { r: 19, g: 20, b: 24 });
});

t("WCAG ratios match the reference values", () => {
  assert.equal(contrastRatio({ r: 255, g: 255, b: 255 }, { r: 0, g: 0, b: 0 }), 21);
  assert.equal(contrastRatio({ r: 0, g: 0, b: 0 }, { r: 0, g: 0, b: 0 }), 1);
  // The two numbers this repo argues about, so they cannot be re-derived wrongly.
  assert.equal(contrastRatio({ r: 162, g: 161, b: 170 }, { r: 255, g: 255, b: 255 }), 2.56);
  assert.equal(contrastRatio({ r: 82, g: 82, b: 91 }, { r: 255, g: 255, b: 255 }), 7.73);
});

t("the AA bar depends on size and weight", () => {
  assert.equal(aaThreshold(12, 400), 4.5);
  assert.equal(aaThreshold(24, 400), 3);
  assert.equal(aaThreshold(18.66, 700), 3);
  // 18px bold is NOT large — the boundary is 18.66, and rounding it to 18 would
  // let a real failure through.
  assert.equal(aaThreshold(18, 700), 4.5);
});

t("colours parse in every form the app emits", () => {
  assert.deepEqual(parseColour("rgb(29, 29, 31)"), { r: 29, g: 29, b: 31, a: 1 });
  assert.deepEqual(parseColour("rgba(255, 255, 255, 0.07)"), { r: 255, g: 255, b: 255, a: 0.07 });
  assert.equal(parseColour("transparent"), null);
  assert.equal(parseColour(""), null);
});

t("failures() uses each row's OWN bar, not a flat 4.5", () => {
  const rows = [
    { cr: 3.5, size: 30, weight: 400, need: 3 },     // large text, passes
    { cr: 3.5, size: 12, weight: 400, need: 4.5 },   // body text, fails
  ];
  assert.equal(failures(rows).length, 1);
  assert.equal(failures(rows)[0].size, 12);
});

t("an INACTIVE control is exempt, not a failure", () => {
  // WCAG 1.4.3 exempts inactive UI components. The panel's disabled Start button
  // measures a truthful 2.1:1 through its opacity chain, which is a real reading
  // of a control nobody can use — chasing it as a defect wastes a round.
  const rows = [
    { cr: 2.1, inactive: true, size: 12, weight: "400", need: 4.5 },
    { cr: 2.1, inactive: false, size: 12, weight: "400", need: 4.5 },
  ];
  assert.equal(failures(rows).length, 1);
  assert.equal(inactive(rows).length, 1);
});

// NO SEPARATE BACKTICK CHECK, and the reason is worth keeping. I wrote one three
// times and every version had a wrong premise (exactly-2-in-the-file: 42; last
// backtick: reaches into the JSDoc below; first backtick-semicolon: matched the
// stray's own close). Then I noticed the guard ALREADY EXISTS: this file IMPORTS
// the module, so a stray backtick inside the template makes the import throw a
// SyntaxError and the whole test file fails loudly. I watched it do exactly that
// twice. A hand-rolled parser for a case the import already catches is a check
// that can only be wrong.

t("PROBE_SOURCE is SYNTACTICALLY VALID", () => {
  // It is a template literal, so a stray BACKTICK inside an embedded comment
  // terminates it — which is exactly what happened while adding the gradient
  // guard above, and the module failed to PARSE at import time rather than at
  // sweep time. Compiling it here turns that into a test failure instead.
  assert.doesNotThrow(() => new Function(`return ${PROBE_SOURCE}`), "the probe must compile");
  assert.ok(!PROBE_SOURCE.includes("`"), "the probe is a template literal: no backticks inside it");
});

t("a gradient row is UNMEASURABLE, not a failure and not a pass", () => {
  const rows = [
    { cr: null, gradient: true, size: 40, weight: "400", need: 3 },  // the panel's "V"
    { cr: 2.0, gradient: false, size: 12, weight: "400", need: 4.5 }, // a real failure
  ];
  // It must not be counted as a failure — that was a false positive on the live
  // panel (white on white = 1.0, because the walk went PAST the gradient).
  assert.equal(failures(rows).length, 1);
  assert.equal(failures(rows)[0].cr, 2.0);
  // ...and it must not vanish either: the caller has to be able to SAY how many
  // were skipped, or a sweep that measured nothing reads as a clean pass.
  assert.equal(unmeasurable(rows).length, 1);
});

t("PROBE_SOURCE carries the REAL functions, not a paraphrase", () => {
  // If the module's functions are edited without the probe following, the browser
  // and the test stop being the same code — which is the failure this whole module
  // exists to prevent.
  assert.ok(PROBE_SOURCE.includes(compositeStack.toString()), "compositeStack not embedded");
  assert.ok(PROBE_SOURCE.includes(contrastRatio.toString()), "contrastRatio not embedded");
  assert.ok(PROBE_SOURCE.includes(aaThreshold.toString()), "aaThreshold not embedded");
  assert.ok(PROBE_SOURCE.includes(parseColour.toString()), "parseColour not embedded");
});


// ── GRADIENTS ARE MEASURED AS A BOUND, NOT SKIPPED ───────────────────────────
// 140 of the desktop density's 242 text nodes sat over a gradient and came back
// unmeasurable. A bound is the honest answer: measure every stop, report the worst.
t("a gradient's stops are read, and an unreadable background stays unreadable", () => {
  const g2 = gradientStops('linear-gradient(135deg, rgb(250, 250, 250) 0%, rgb(0, 0, 0) 100%)');
  assert.equal(g2.length, 2);
  assert.equal(g2[0].r, 250);
  // A translucent stop keeps its alpha: compositing it as opaque is how a probe
  // reads rgba(255,255,255,0.07) as WHITE and reports twenty false findings.
  assert.equal(gradientStops('linear-gradient(rgba(255,255,255,0.5), transparent)')[1].a, 0);
  assert.equal(gradientStops('url("x.png")').length, 0);
  assert.equal(gradientStops('none').length, 0);
});

t("the WORST stop decides, never the best", () => {
  const bw = gradientStops('linear-gradient(rgb(255,255,255), rgb(0,0,0))');
  const base = { r: 255, g: 255, b: 255 };
  // White text is unreadable on the white stop and perfect on the black one; the
  // number reported is the one that can hurt somebody.
  assert.equal(worstOverGradient({ r: 255, g: 255, b: 255 }, bw, base).toFixed(2), '1.00');
  assert.equal(worstOverGradient({ r: 0, g: 0, b: 0 }, bw, base).toFixed(2), '1.00');
  assert.equal(worstOverGradient({ r: 0, g: 0, b: 0 }, [], base), null);
});

const bw = gradientStops('linear-gradient(rgb(255,255,255), rgb(0,0,0))');

// ── HEX IS A COLOUR, AND IT MUST EQUAL ITS rgb() FORM ────────────────────────────────────────
// The parser scraped numbers out of whatever it was given, so `#f4f4f5` became {4,4,5} — a real
// colour, silently wrong — and `#71717a` became nothing. The rendered sweep never noticed because
// getComputedStyle returns rgb(); a static check reading the token sheet hit it at once. These
// checks compare hex against the rgb() spelling of the SAME colour, so a stride bug cannot pass by
// happening to look plausible.
t("6-digit hex parses to the same colour as its rgb() form", () => {
  const hex = parseColour("#71717a");
  const rgb = parseColour("rgb(113, 113, 122)");
  assert(hex && rgb, "both spellings must parse");
  assert(hex.r === 113 && hex.g === 113 && hex.b === 122, `hex parsed as ${JSON.stringify(hex)}`);
  assert(hex.r === rgb.r && hex.g === rgb.g && hex.b === rgb.b, "hex and rgb() must agree");
});
t("a hex pair and its rgb() pair give the SAME contrast ratio", () => {
  const a = contrastRatio(parseColour("#71717a"), parseColour("#f4f4f5"));
  const b = contrastRatio(parseColour("rgb(113,113,122)"), parseColour("rgb(244,244,245)"));
  assert(Math.abs(a - b) < 0.001, `hex ${a.toFixed(3)} vs rgb ${b.toFixed(3)}`);
  assert(Math.abs(a - 4.4) < 0.15, `expected ~4.4 (the measured ghost-button pair), got ${a.toFixed(2)}`);
});
t("3- and 4-digit hex expand, and 8-digit hex carries alpha", () => {
  const short = parseColour("#fff");
  assert(short && short.r === 255 && short.g === 255 && short.b === 255 && short.a === 1, JSON.stringify(short));
  const shortAlpha = parseColour("#0008");
  assert(shortAlpha && shortAlpha.a > 0 && shortAlpha.a < 1, JSON.stringify(shortAlpha));
  const long = parseColour("#ffffff80");
  assert(long && long.r === 255 && Math.abs(long.a - 0.502) < 0.01, JSON.stringify(long));
});
t("a malformed hex is null, not a colour", () => {
  assert(parseColour("#12345") === null, "5 digits is not a colour");
  assert(parseColour("#1234567") === null, "7 digits is not a colour");
  assert(parseColour("#gggggg") === null, "not hex at all");
});

// ── THE EMITTED SOURCE MUST COMPILE (round 124) ─────────────────────────────────────────────────
// The module can import cleanly while the string it hands the browser is broken: PROBE_SOURCE is a
// template literal, so a single backslash in a regex collapses on the way out. Round 123 shipped exactly
// that and only found it when the device refused to evaluate the probe; round 124 hit it twice more
// (\d became d, \) became )). Importing the module proves nothing about what the browser runs — this
// compiles the EMITTED string, which is the artifact that matters.
t("the emitted probe compiles as JavaScript — the artifact, not the module", () => {
  new Function(PROBE_SOURCE);
});
t("the emitted probe keeps its regex escapes (a collapsed one changes the match, not the parse)", () => {
  // A doubled backslash in the module becomes a single one in the emitted string. If the doubling is
  // forgotten the regex STILL PARSES — it just matches something else — so this checks the content of
  // the artifact, not that it compiles. The selector builder splits a class list on whitespace.
  assert.ok(PROBE_SOURCE.includes("split(/\\s+/)"), "the class-list split lost its \\s escape");
  assert.ok(!PROBE_SOURCE.includes("split(/s+/)"), "a collapsed \\s reached the emitted source");
});

// ── GRAPHICS (round 125) ─────────────────────────────────────────────────────────────────────────
// WCAG 1.4.11 asks 3:1 of a non-text element that carries meaning, and the text loop cannot see one. The
// pass that collects them was built twice (rounds 123-124) and each time its own validation failed; what
// made the third attempt work was putting the two colours the ratio came from ON THE ROW. Round 124 spent
// a whole round on "cr: 7.03" with nothing to say why; round 125 read `paint rgb(82,82,91) (border)` and
// saw the bug in one look — the painter was taking the colour of a ZERO-WIDTH border side.
t("a graphic row carries the evidence its number came from", () => {
  const row = { sel: "span.boot-mark.warn", text: "", kind: "graphic", paint: "rgb(146, 64, 14) (border)", surface: "rgb(244, 244, 245)", size: 7, weight: "400", need: 3.0, inactive: false, cr: 6.45 };
  assert.equal(failures([row]).length, 0, "6.45 clears the 3:1 a graphic needs");
  assert.equal(failures([{ ...row, cr: 2.9 }]).length, 1, "2.9 does not");
  assert.ok(/rgb\(/.test(row.paint) && /rgb\(/.test(row.surface), "paint and surface must name real colours");
  assert.ok(/\(border\)|\(background\)|\(fill\)|\(stroke\)|\(ring\)|\(::/.test(row.paint), "the painter must say WHERE the colour came from");
});

console.log(`contrast-probe: all ${n} checks passed`);