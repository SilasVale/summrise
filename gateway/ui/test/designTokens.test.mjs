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

test("every lane's LABEL is readable on the fill the sheet gives it", async () => {
  // The measured half of the rule above: a token is only the first step. This pairs each lane's ink
  // with its fill and does the contrast arithmetic, because the sheet's own history shows the trap —
  // the `og` label was #fff on the dark theme's #ffa94d accent: 1.90, well under AA.
  const { contrastRatio, parseColour } =
    await import("../../../agent/scripts/lib/contrast-probe.mjs");
  const tokenIn = (block, name) => {
    const m = new RegExp(`(?:^|[;{\\s])${name}\\s*:\\s*([^;]+);`).exec(block);
    return m ? m[1].trim() : null;
  };
  const lightBlock = /:root\s*\{([\s\S]*?)\n\}/.exec(bare)[1];
  const darkBlock = [...bare.matchAll(/\[data-theme=["']?dark["']?\][^{]*\{([\s\S]*?)\n\}/g)]
    .map((m) => m[1])
    .join("\n");
  assert.ok(darkBlock.length > 0, "the dark theme block must exist or half of this proves nothing");

  // The four fills that do NOT flip, each with the constant ink the sheet gives them.
  for (const name of ["--chan-ds", "--chan-or", "--chan-qw"]) {
    const fill = tokenIn(lightBlock, name);
    const ink = tokenIn(lightBlock, "--chan-fg");
    assert.ok(fill && ink, `${name} and --chan-fg must be declared`);
    const ratio = contrastRatio(parseColour(ink), parseColour(fill));
    assert.ok(ratio >= 4.5, `${name} label: ${ink} on ${fill} = ${ratio.toFixed(2)}, needs 4.5`);
  }
  // The lane that DOES flip: its ink has to come from the same block as its fill.
  for (const [theme, block] of [
    ["light", lightBlock],
    ["dark", darkBlock],
  ]) {
    const fill = tokenIn(block, "--accent") ?? tokenIn(lightBlock, "--accent");
    const ink = tokenIn(block, "--accent-fg") ?? tokenIn(lightBlock, "--accent-fg");
    const ratio = contrastRatio(parseColour(ink), parseColour(fill));
    assert.ok(
      ratio >= 4.5,
      `og lane label (${theme}): ${ink} on ${fill} = ${ratio.toFixed(2)}, needs 4.5`,
    );
  }
  // And the PAIRING itself, so a future lane cannot silently take the wrong ink.
  assert.match(
    bare,
    /\.lane-port\s*\{[^}]*color\s*:\s*var\(--accent-fg\)/,
    ".lane-port (the og lane) takes --accent-fg",
  );
  for (const lane of ["ds", "or", "qw", "def"]) {
    assert.match(
      bare,
      new RegExp(`\\.lane-${lane}\\s+\\.lane-port[^{}]*\\{[^}]*var\\(--chan-fg\\)`),
      `.lane-${lane} takes --chan-fg`,
    );
  }
});

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
  const declared = new Set([...css.matchAll(/(--chan-[a-z0-9-]+)\s*:/g)].map((m) => m[1]));
  for (const name of ["--chan-og", "--chan-ds", "--chan-or", "--chan-qw"]) {
    assert.ok(declared.has(name), `${name} must be declared with the other channels`);
  }
});
