#!/usr/bin/env node
// ── THE MUTATION THAT MUST FAIL THIS GATE (moved here from the ledger table, landing 4b) ──
// Read this when you change this file: the mutation is how you find out whether the gate can still
// fail at all. A gate that cannot be broken is worse than no gate.
//
// MUTATION: put a backtick in one of the probe's embedded comments, or let a `\\s` in a regex collapse to
// `s`. RESULT: exit 1 — "the probe is a template literal: no backticks inside it", or "a collapsed \\s
// reached the emitted source".
//
// ── WHAT IS LEFT HERE, AND WHY IT IS FIVE ASSERTIONS ABOUT ONE STRING ───────────────────────────────
//
// THE OTHER THIRTEEN ASSERTIONS ARE RUST NOW: `agent/tests/contrast_probe.rs` carries the probe's pure
// functions (compositeStack, contrastRatio, aaThreshold, parseColour, failures, inactive, unmeasurable,
// gradientStops, worstOverGradient, svgRootPaints) and the gate's own cases for them, numbers included —
// the translucent-background composite that reported 2.51 for text measuring 5.49, the inactive-control
// exemption, the gradient row that must be UNMEASURABLE rather than a failure or a pass, the SVG root
// whose inherited `rgb(0,0,0)` filed ten false findings, and hex equalling its `rgb()` form.
//
// WHAT COULD NOT MOVE IS `PROBE_SOURCE`: the probe the DESIGN SWEEPS INJECT INTO A BROWSER. The plan's
// carve-out list keeps that JavaScript, because `browser_run_script` takes a JS file and the measurement
// runs in the DOM — so these five assertions are about a JS ARTIFACT rather than about a rule, and they
// stay until the probe itself is not JavaScript. That is the plan's rule for a gate that cannot follow
// its subject: keep it, and write down why.
//
// THE FUNCTIONS TESTED HERE ARE THE ONES THE BROWSER RUNS: `PROBE_SOURCE` embeds them with
// `Function.prototype.toString()`, so this is not a copy that can drift — and THAT is the property these
// five protect. The moment the functions move to Rust, the embedding is what changes, and these
// assertions are what will say so.
import { compositeStack, contrastRatio, aaThreshold, parseColour, svgRootPaints, PROBE_SOURCE } from "../../agent/scripts/lib/contrast-probe.mjs";
import assert from "node:assert/strict";

let n = 0;
const t = (desc, fn) => { fn(); n += 1; };

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
  // guard, and the module failed to PARSE at import time rather than at
  // sweep time. Compiling it here turns that into a test failure instead.
  assert.doesNotThrow(() => new Function(`return ${PROBE_SOURCE}`), "the probe must compile");
  assert.ok(!PROBE_SOURCE.includes("`"), "the probe is a template literal: no backticks inside it");
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

t("the probe measures the SVG root and skips only its children", () => {
  assert(
    /el instanceof SVGElement && el\.tagName\.toLowerCase\(\) !== 'svg'/.test(PROBE_SOURCE),
    "the loop must let the svg ROOT through: it is the element that knows the icon's colour (fill: none, stroke: currentColor)",
  );
  assert(
    !/el instanceof SVGElement \|\| el\.closest\('svg'\)/.test(PROBE_SOURCE),
    "a blanket SVG exclusion is back — it takes the root with it and leaves painterOf's fill/stroke branches unreachable",
  );
  assert(PROBE_SOURCE.includes(svgRootPaints.toString()), "svgRootPaints is not embedded — the browser would run a different rule");
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

console.log(`contrast-probe (the emitted probe): ok — ${n} assertion(s) about the JS artifact the sweeps inject; the probe's rules are cargo test --test contrast_probe`);
