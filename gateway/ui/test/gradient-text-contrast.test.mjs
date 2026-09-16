// TEXT ON A GRADIENT MUST CLEAR AA AGAINST EVERY STOP.
//
// The console's first design measurement (round 55, the built bundle rendered in a real browser)
// found the account avatar at 2.13: white on `linear-gradient(135deg, #f59f00, #e8590c)`, graded
// against its BRIGHTEST stop — which is the safe direction, because a gradient cannot hide a failure
// behind an average. The console's own suite runs in jsdom, where no colour is ever rendered, so it
// had never seen this; this test reads the stylesheet and does the arithmetic with the repo's tested
// probe maths rather than a second implementation of contrast.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import test from "node:test";
import assert from "node:assert/strict";
import { contrastRatio, parseColour, aaThreshold } from "../../../agent/scripts/lib/contrast-probe.mjs";

const CSS = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "src", "styles", "globals.css");

/** Every rule in the sheet that paints TEXT on a gradient, with the stops it must survive. */
function gradientTextRules(css) {
  const bare = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const out = [];
  for (const m of bare.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const body = m[2];
    const colour = /(?:^|;)\s*color\s*:\s*([^;]+);/.exec(body);
    const bg = /(?:^|;)\s*background(?:-image)?\s*:\s*([^;]*gradient\([^;]+);/.exec(body);
    if (!colour || !bg) continue;
    const stops = [...bg[1].matchAll(/#[0-9a-fA-F]{3,8}|rgba?\([^)]+\)/g)].map((s) => s[0]);
    if (stops.length >= 2) out.push({ selector: m[1].trim().split("\n").pop().trim(), colour: colour[1].trim(), stops });
  }
  return out;
}

test("every text-on-gradient rule clears AA against its worst stop", () => {
  const rules = gradientTextRules(readFileSync(CSS, "utf8"));
  assert.ok(rules.length >= 1, "the sheet must still paint text on at least one gradient");
  const need = aaThreshold(13, 700);
  for (const r of rules) {
    const fg = parseColour(r.colour);
    assert.ok(fg, `cannot read ${r.selector}'s colour ${r.colour}`);
    for (const stop of r.stops) {
      const bg = parseColour(stop);
      assert.ok(bg, `cannot read the stop ${stop} in ${r.selector}`);
      const ratio = contrastRatio(fg, bg);
      assert.ok(
        ratio >= need,
        `${r.selector}: ${r.colour} on ${stop} = ${ratio.toFixed(2)}, needs ${need} — white on an amber gradient is the classic miss`,
      );
    }
  }
});
