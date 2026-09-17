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
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";

const SCALE = new Set([0, 2, 4, 8, 12, 16, 24]);
const SPACING = /(?:^|[\s;{])(gap|row-gap|column-gap|padding|padding-(?:top|right|bottom|left)|margin|margin-(?:top|right|bottom|left))\s*:\s*([^;}]+)/g;

// THE BASELINES, measured in rounds 220-222, one per UI. Each may only move in the good direction.
const UIS = [
  {
    name: "panel",
    sheets: () => readdirSync("agent/resources/panel-react/src/styles").filter((f) => f.endsWith(".css")).sort()
      .map((f) => join("agent/resources/panel-react/src/styles", f)),
    tokenUses: 151,
    offScaleUses: 305,
  },
  {
    name: "console",
    sheets: () => readdirSync("gateway/ui/src/styles").filter((f) => f.endsWith(".css")).sort()
      .map((f) => join("gateway/ui/src/styles", f)),
    // 86 token uses as of round 223: the console adopted the panel's scale, and every one of those 86 was
    // already a literal with that exact value, so NO PIXEL MOVED. The ratchet is tightened to hold it.
    tokenUses: 86,
    // 136, NOT the 194 an ad-hoc scan reported: that scan swept gateway/public/style.css as well, which round
    // 209 established is DEAD (nothing links it). A baseline has to come from the instrument that will enforce
    // it — taken from the ad-hoc number, this ratchet allowed 58 new literals and a planted 13px passed it.
    offScaleUses: 136,
  },
  {
    name: "extension",
    sheets: () => ["extension/options/options.css"].filter(existsSync),
    tokenUses: 0,
    offScaleUses: 6,
  },
];

const failures = [];
const lines = [];
for (const ui of UIS) {
  let tokenUses = 0;
  let offScaleUses = 0;
  const offenders = [];
  for (const file of ui.sheets()) {
    const css = readFileSync(file, "utf8");
    for (const m of css.matchAll(SPACING)) {
      const value = m[2].trim();
      tokenUses += [...value.matchAll(/var\(--[a-z0-9-]+\)/g)].length;
      for (const px of value.matchAll(/(-?\d+(?:\.\d+)?)px/g)) {
        const n = Number(px[1]);
        if (n === 0 || SCALE.has(n)) continue;
        offScaleUses++;
        if (offenders.length < 3) offenders.push(file.split("/").pop() + ": " + value.slice(0, 28));
      }
    }
  }
  if (offScaleUses > ui.offScaleUses) {
    failures.push(ui.name + ": off-scale spacing rose from " + ui.offScaleUses + " to " + offScaleUses + " — " + offenders.join(" | "));
  }
  if (tokenUses < ui.tokenUses) {
    failures.push(ui.name + ": token uses fell from " + ui.tokenUses + " to " + tokenUses);
  }
  lines.push(ui.name + " " + tokenUses + " token / " + offScaleUses + " off-scale");
}

if (failures.length) {
  console.error("spacing scale: FAILED");
  for (const f of failures) console.error("  " + f);
  process.exit(1);
}
console.log("spacing scale: ok — " + lines.join(" · "));
