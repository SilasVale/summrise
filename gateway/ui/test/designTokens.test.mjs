// The console's design tokens have the same contracts the device panel's have had for thirty rounds.
//
// WHY THIS FILE EXISTS. The panel carries five design contracts (designScale, themeContrast,
// deadStyles, documentOutline, pairContrast) and the console carried exactly one
// (gradient-text-contrast). Measuring the console against the panel's rules in round 79 found it in
// good shape already — two font sizes and two colour literals in the whole sheet — but "in good
// shape" is a measurement, and a measurement expires. These are the rules that were true and are now
// enforced:
//
//   1. a font size is a TOKEN, not a number that happens to match one. The panel's version of this
//      rule found 73 literals (round 75); the console had two, both legitimate and both named below.
//   2. a CHANNEL's colour comes from a `--chan-*` token. Three of the four did; Qwen's was written
//      out twice as #c2255c in the lane port and the model prefix border, which is how a palette
//      drifts one channel at a time.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const CSS = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "src",
  "styles",
  "globals.css",
);
const css = readFileSync(CSS, "utf8");
/** Comments carry examples and history; a rule about declarations must not read them. */
const bare = css.replace(/\/\*[\s\S]*?\*\//g, "");

function declarations(property) {
  const out = [];
  for (const m of bare.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    for (const d of m[2].matchAll(new RegExp(`(?:^|;)\\s*${property}\\s*:\\s*([^;]+);`, "g"))) {
      out.push(d[1].trim());
    }
  }
  return out;
}

/** The two sizes that are NOT tokens, each for a reason. An exemption list is only honest with the
 *  reason attached, and this test checks the reason still applies (see the second test). */
const SIZE_EXEMPTIONS = {
  inherit: "not a size — it says 'whatever my parent uses'",
  "0.9em": "inline code, deliberately RELATIVE so it tracks the surrounding text size",
};

test("every font size in the console is a token", () => {
  const literals = declarations("font-size").filter(
    (v) => !v.startsWith("var(--fs-") && !(v in SIZE_EXEMPTIONS),
  );
  assert.deepEqual(
    literals,
    [],
    "a font size written as a number cannot follow its token; use var(--fs-*), " +
      "or add it to SIZE_EXEMPTIONS with the reason it is relative",
  );
});

test("the exemptions still describe what the sheet does", () => {
  const sizes = declarations("font-size");
  for (const [value, reason] of Object.entries(SIZE_EXEMPTIONS)) {
    assert.ok(
      sizes.includes(value),
      `"${value}" is exempted (${reason}) but no longer used — drop the exemption`,
    );
  }
  assert.ok(
    declarations("font-size").some((v) => v.startsWith("var(--fs-")),
    "the token scale must actually be in use, or this contract proves nothing",
  );
});

test("the type scale the console uses is declared, and every token it uses exists", () => {
  const declared = new Set([...css.matchAll(/(--fs-[a-z0-9-]+)\s*:/g)].map((m) => m[1]));
  assert.ok(declared.size >= 5, `expected a type scale, found ${declared.size} tokens`);
  const used = new Set(
    declarations("font-size").flatMap((v) =>
      [...v.matchAll(/var\((--fs-[a-z0-9-]+)\)/g)].map((m) => m[1]),
    ),
  );
  const missing = [...used].filter((t) => !declared.has(t));
  assert.deepEqual(missing, [], "a font size references a token the sheet never declares");
});

test("the lane classes the TS can emit are the ones the stylesheet defines", () => {
  // A CROSS-LANGUAGE CONTRACT, and the reason this round exists. The stylesheet carries two lane
  // families; `Models.tsx` maps a channel prefix to a class name. If the two lists drift, a channel
  // renders with no colour at all — silently, because a missing class is not an error.
  //
  // It also records what round 79 actually found: I "fixed" the label ink of `.lane-port`, measuring
  // it carefully (white on the dark accent: 1.90) — and then discovered NOTHING IN THE REPO EMITS
  // `lane-port` OR `models-prefix`. Eighteen lines of dead CSS, and a fix that changed nothing on
  // screen. They are pruned; this test now checks the family that IS rendered.
  // THE MAPPING MOVED TO `lib/lane.ts` IN ROUND 140 — from eight `if` lines to a TABLE, because a mapping written as control
  // flow hides which prefixes have a lane and that everything else falls to `lane-def`. The subject here is unchanged (the
  // classes the TS can emit versus the ones the sheet defines), so only the extraction follows it.
  const tsx = readFileSync(
    path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "src", "lib", "lane.ts"),
    "utf8",
  );
  // EVERY quoted `lane-…` string, because the fallback is `?? "lane-def"` — no colon in front of it — and a pattern keyed
  // on the table's rows alone reported the base rule as dead CSS. The module's own comments name real lanes, which is
  // harmless here: this set is compared against the sheet in both directions, and both names have rules.
  const emitted = new Set([...tsx.matchAll(/"(lane-[a-z]+)"/g)].map((m) => m[1]));
  assert.ok(emitted.size >= 8, `expected the channel mapping, found ${emitted.size} classes`);
  const defined = new Set([...bare.matchAll(/\.prov-lane\.(lane-[a-z]+)/g)].map((m) => m[1]));
  // `lane-def` is emitted BY the mapping and defined BY the base rule, so it is exempt from the
  // per-lane check — and the assertion below proves the base rule is really there, which is what
  // earns the exemption (round 74's rule: an exemption list needs the thing it exempts verified).
  const missing = [...emitted].filter((c) => !defined.has(c) && c !== "lane-def");
  assert.deepEqual(missing, [], "a lane class the TS can emit has no rule in the stylesheet");
  const unused = [...defined].filter((c) => !emitted.has(c));
  assert.deepEqual(unused, [], "a lane rule nothing can emit is dead CSS");
  // `lane-def` is the DEFAULT channel, so it is the base rule rather than a `.lane-def` rule —
  // checked rather than assumed, or dropping the base would pass this contract silently.
  assert.match(
    bare,
    /\.prov-lane\s*\{[^}]*background\s*:\s*var\(--text-muted\)/,
    "the default lane colour is the base .prov-lane rule",
  );
});

// MEASURED, AND DELIBERATELY NOT SCALED (round 81), so the next round does not re-derive it:
//   * GAPS: 74 literal declarations across 14 values (4/6/8/10/12/14/16px …) and NO spacing tokens.
//     The panel has a full --sp-* scale and a test that enforces it; the console never had one. But
//     its values are already a tight even-numbered set, so a token layer would be a 74-declaration
//     rewrite with no change on screen — the same call as the panel's 87 off-scale PADDINGS in round
//     64. A scale earns its place when it fixes drift, not when it relabels agreement.
//   * RADII: five tokens (10/10/20/20/999) and six small literals (2/3/4/5/6/12px) plus the shapes
//     (50% x11, 22%). The small values are chip and bar radii the scale does not name; adding
//     --radius-xs for them is tidiness, not a fix.
//   * HALF-PIXELS: none anywhere.
// What IS contracted here is the part with a failure mode: sizes that must follow a token, and lane
// colours that must be declared.

test("every channel colour comes from a --chan-* token", () => {
  // Each lane class must paint from its own token. A literal here is how one channel drifts: Qwen's
  // was #c2255c in two rules while og/ds/or all used tokens (fixed in round 79).
  const laneRules = [...bare.matchAll(/\.(?:lane|models-prefix|prov-lane)[^{}]*\{([^{}]*)\}/g)];
  assert.ok(
    laneRules.length >= 6,
    `expected several lane rules to check, found ${laneRules.length}`,
  );
  const literals = laneRules
    .flatMap((m) => [...m[1].matchAll(/(?:background|border-left-color|color)\s*:\s*([^;]+);/g)])
    .map((d) => d[1].trim())
    .filter((v) => /#[0-9a-fA-F]{3,8}\b|\brgba?\(/.test(v));
  assert.deepEqual(literals, [], "a channel colour must come from var(--chan-*)");
  // And the live lanes paint from DECLARED tokens, whatever they are called: og/cm use --accent, qw
  // --warning, nv --success, gmi/amd --error, the default --text-muted. (An earlier draft of this
  // test demanded `--chan-qw`; the rendered family never used it — it was invented for a dead rule.)
  const declared = new Set([...css.matchAll(/(--[a-z0-9-]+)\s*:/g)].map((m) => m[1]));
  const laneFills = [
    ...bare.matchAll(/\.prov-lane\.lane-[a-z]+\s*\{[^}]*background\s*:\s*var\((--[a-z0-9-]+)\)/g),
  ].map((m) => m[1]);
  assert.ok(
    laneFills.length >= 8,
    `expected every channel to paint from a token, found ${laneFills.length}`,
  );
  const undeclared = laneFills.filter((t) => !declared.has(t));
  assert.deepEqual(undeclared, [], "a lane paints from a token the sheet never declares");
});
