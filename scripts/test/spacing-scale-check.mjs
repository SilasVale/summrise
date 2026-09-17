#!/usr/bin/env node
// spacing-scale-check.mjs — the panel's spacing is mostly AD-HOC, and it may not get worse.
//
// WHY THIS EXISTS (round 220). The objective names five axes — spacing, hierarchy, contrast, typography,
// states — and four of them have rendered checks. Spacing had geometry, overflow, reflow and the 2.5.8 spacing
// clause; nothing looked at the SCALE itself. Measured for the first time:
//
//     token uses     151   (5 of the 6 scale steps: --sp-0-5/1/2/3/4/5 = 2/4/8/12/16/24px)
//     literal px     539
//     off-scale      305 uses across 20 distinct values — 6px x62, 10px x61, 14px x32, 5px x31, 7px x29 ...
//
// So 44% of the panel's spacing is off-scale and the scale carries 22%. REWRITING 305 DECLARATIONS IS NOT THIS
// ROUND'S BUSINESS: spacing is the one axis where a mechanical change is VISIBLE, and a late sweep of 305
// values would be a redesign wearing a refactor's clothes. What is worth having is the measurement held still:
// the counts below may improve and may not get worse, which is the same ratchet the unstyled scan's floor and
// the coverage floor use. A new ad-hoc `13px` fails this, and so does deleting a token use to make room for one.
//
// ONE STEP IS FREE: --sp-5 (24px) is defined and used nowhere, in any of the three UIs (checked in round 221).
// KEPT, not pruned, and the distinction matters: an unused DECLARATION is dead weight, but an unused STEP IN A
// SCALE is a slot the system offers. The other five steps carry 153 uses between them and this one is where a
// section gap would go; deleting it would leave a scale with a hole in it, which is worse than a token that is
// waiting. Recorded here so the next reader knows the sixth step is available rather than wondering whether the
// measurement missed it.
//
// Deliberately counts EVERY literal, including the legitimate ones (0 is skipped; 1px hairlines and negative
// optical adjustments are counted and ratcheted like the rest). An allow-list would need a reason per entry and
// would drift; a number that may only improve does not.
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const STYLES = "agent/resources/panel-react/src/styles";

// THE BASELINE, measured in round 220 from the built sources. It may only move in the good direction.
const BASELINE = { tokenUses: 151, offScaleUses: 305 };

const SCALE = new Set([0, 2, 4, 8, 12, 16, 24]);
const SPACING = /(?:^|[\s;{])(gap|row-gap|column-gap|padding|padding-(?:top|right|bottom|left)|margin|margin-(?:top|right|bottom|left))\s*:\s*([^;}]+)/g;

let tokenUses = 0;
let offScaleUses = 0;
const offenders = [];

for (const file of readdirSync(STYLES).filter((f) => f.endsWith(".css")).sort()) {
  const css = readFileSync(join(STYLES, file), "utf8");
  for (const m of css.matchAll(SPACING)) {
    const value = m[2].trim();
    tokenUses += [...value.matchAll(/var\(--[a-z0-9-]+\)/g)].length;
    for (const px of value.matchAll(/(-?\d+(?:\.\d+)?)px/g)) {
      const n = Number(px[1]);
      if (n === 0 || SCALE.has(n)) continue;
      offScaleUses++;
      offenders.push(`${file}: ${m[1]}: ${value.slice(0, 40)}`);
    }
  }
}

const failures = [];
if (offScaleUses > BASELINE.offScaleUses) {
  failures.push(
    `off-scale spacing rose from ${BASELINE.offScaleUses} to ${offScaleUses} — the scale is ` +
      `${[...SCALE].join("/")}px, and a new literal widens the gap between the design and its tokens`,
  );
}
if (tokenUses < BASELINE.tokenUses) {
  failures.push(`token uses fell from ${BASELINE.tokenUses} to ${tokenUses} — a scale step was replaced by a literal`);
}

if (failures.length) {
  console.error("spacing scale: FAILED");
  for (const f of failures) console.error("  " + f);
  console.error(`  first offenders: ${offenders.slice(0, 4).join(" | ")}`);
  process.exit(1);
}
console.log(
  `spacing scale: ok — ${tokenUses} token uses (at least ${BASELINE.tokenUses}), ` +
    `${offScaleUses} off-scale literals (at most ${BASELINE.offScaleUses})`,
);
