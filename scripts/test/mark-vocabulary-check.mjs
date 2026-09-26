#!/usr/bin/env node
// ── THE MUTATION THAT MUST FAIL THIS GATE (moved here from the ledger table, landing 4b) ──
// Read this when you change this file: the mutation is how you find out whether the gate can still
// fail at all. A gate that cannot be broken is worse than no gate.
//
// MUTATION: stop the console's failure mark from being a diamond (`.dot.err` loses `rotate(45deg)`), or give the panel's `waiting` the `idle` ring
// RESULT:   exit 1 both ways: ".dot.err means FAILURE and does not draw the diamond (transform: none) — the panel spells that state as a diamond", and for the panel "the panel's most urgent state draws \"ring\", not a diamond — the shape that means \"this wants you\" has moved" plus "the panel's four states share 1 silhouette(s)". It is the ONLY check that reads both surfaces at once — every other marks check is about one of them — and it reads the panel's vocabulary as DATA (`liveness.ts`) and the console's as PAINTED (its sheet, with the same signature rule `console-marks-check.mjs` uses). Measured 2026-09-18 (**FIVE panel states since round 97**): panel `waiting`=diamond `working`=solid-halo `failed`=triangle `idle`=ring `off`=dashed-ring; console 11 marks, 4 distinct shapes, agreeing on attention=diamond, absent=ring, fine=fill. The failed silhouette arrived only when the DEVICE learned to report the fact (round 96, `last_exit_code`), and it is deliberately NOT the diamond: the panel spends that on a QUESTION and the console spends it on FAILURE, so a fifth state reusing it would put two meanings on one shape in the same tab strip. Mutations: folding `failed` onto the diamond fails with "the shape it already uses for a QUESTION", and dropping the state fails the floor ("read 4 panel state(s), expected at least 5")

// mark-vocabulary-check — ONE state, ONE silhouette, across BOTH surfaces.
//
// WHY THIS EXISTS. Each surface already checks its own marks: the panel's `statePalette.test.ts` reads its BUILT
// sheet, the console's `console-marks-check.mjs` reads its SOURCE sheet, and the shared probe reads the COMPUTED
// styles of a rendered page. Every one of those is about ONE surface. Nothing compared the two languages, so the
// thing the objective actually asks for — a mark language where a state has its own silhouette, so it survives
// colour loss, colour-vision deficiency and reduced motion — was pinned per surface and free to drift between them.
//
// An operator uses both: the console to see the fleet, the panel to see one device. If a diamond means "a question
// is waiting for you" on one and "this channel is failing" on the other, the shape stops carrying the state and the
// two surfaces teach two vocabularies. Measured 2026-09-18, they AGREE:
//
//     panel (liveness.ts, as data)      console (globals.css, as painted)
//     waiting  diamond                  .sig-dot.err / .dot.err  rotate(45deg) + fill   diamond
//     working  solid-halo               .dot.ok / .dev-led.on    fill + outer shadow    fill
//     idle     ring                     .dev-led / .dot          inset ring, no fill    ring
//     off      dashed-ring              .sig-dot.off             dashed border, empty   dashed-ring
//
// THE FOUR FACTS BELOW ARE THE ONES BOTH SURFACES CAN BE HELD TO. They are deliberately about MEANING rather than
// about class names: the panel spells state as `data-state` and the console spells it as a class, which is a
// difference of expression and not of language.
//
// A FLOOR IS PART OF THE CHECK. Two surfaces that both stopped rendering marks would otherwise agree perfectly.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const PANEL_PALETTE = "agent/resources/panel-react/src/lib/liveness.ts";
const CONSOLE_SHEET = "gateway/ui/src/styles/globals.css";

const failures = [];
const notes = [];

// ── the panel's vocabulary, read from the module that IS the vocabulary ─────────────────────────
const paletteSrc = readFileSync(join(ROOT, PANEL_PALETTE), "utf8");
const silhouetteBlock = /export const SILHOUETTE[^{]*\{([^}]*)\}/.exec(paletteSrc);
if (!silhouetteBlock) {
  console.error(`mark-vocabulary-check: FAILED — no SILHOUETTE map in ${PANEL_PALETTE}, so this proves nothing`);
  process.exit(1);
}
const panel = {};
for (const m of silhouetteBlock[1].matchAll(/(\w+)\s*:\s*"([\w-]+)"/g)) panel[m[1]] = m[2];
// FIVE SINCE ROUND 97, and the floor is how a silent SHRINK is caught: the vocabulary grew a state when the device
// learned to report a command's outcome, and a later edit that drops one would otherwise leave this gate reading
// four and reporting a clean sheet.
if (Object.keys(panel).length < 5) {
  console.error(`mark-vocabulary-check: FAILED — read ${Object.keys(panel).length} panel state(s), expected at least 5`);
  process.exit(1);
}
// AND NO TWO STATES MAY SHARE A SHAPE, which is the property the map exists for. `liveness.test.ts` asserts it too;
// this file asserts it on the DATA the two surfaces are compared through, because a shape added here without one
// there is exactly how "the silhouette carries the state" would quietly stop being true.
{
  const shapes = Object.values(panel);
  if (new Set(shapes).size !== shapes.length) {
    console.error(`mark-vocabulary-check: FAILED — the panel's ${shapes.length} states share ${new Set(shapes).size} silhouette(s): ${shapes.join(", ")}`);
    process.exit(1);
  }
}

// ── the console's vocabulary, computed from its sheet with the SAME rule its own gate uses ─────
// (border-radius, border-style, transform, kind) — copied deliberately rather than re-invented: an
// approximation of a rule is an approximation of the verdict, which is how the last ten probe errors happened.
let sheet = readFileSync(join(ROOT, CONSOLE_SHEET), "utf8");
sheet = sheet.replace(/\/\*[\s\S]*?\*\//g, ""); // prose about a rule is not the rule
const blockOf = (sel) => {
  let out = "";
  for (const m of sheet.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const sels = m[1].split(",").map((s) => s.trim());
    // the cascade's winner, approximated by document order: the arms sit after their bases in this sheet
    if (sels.includes(sel)) out = m[2];
  }
  return out;
};
const propOf = (sel, prop) => {
  const m = new RegExp("(?:^|;)\\s*" + prop + "\\s*:\\s*([^;]+)").exec(blockOf(sel));
  return m ? m[1].trim() : "";
};
const shapeOf = (sel) => {
  const bg = propOf(sel, "background") || propOf(sel, "background-color");
  const shadow = propOf(sel, "box-shadow");
  const border = propOf(sel, "border") + " " + propOf(sel, "border-style");
  const filled = Boolean(bg) && !/transparent|none/.test(bg);
  const inset = /inset/.test(shadow);
  const outer = Boolean(shadow) && !inset;
  const dashed = /dashed|dotted/.test(border);
  const rotated = /rotate\(45deg\)/.test(propOf(sel, "transform"));
  const kind = inset && filled ? "ring+fill" : inset ? "ring" : filled && outer ? "solid-halo" : filled ? "solid" : "empty";
  return { kind, dashed, rotated, radius: propOf(sel, "border-radius") || "0" };
};

// (mark, the state it means) — the console's own families, which its gate already proves distinct within
const CONSOLE_MARKS = [
  [".sig-dot.ok", "fine"],
  [".sig-dot.err", "failure"],
  [".sig-dot.off", "absent"],
  [".dot.ok", "fine"],
  [".dot.err", "failure"],
  [".dev-led", "absent"],
  [".dev-led.on", "fine"],
  [".dev-mini-led", "absent"],
  [".dev-mini-led.on", "fine"],
  [".ov-keyled", "absent"],
  [".ov-keyled.on", "fine"],
];
const consoleShapes = new Map(CONSOLE_MARKS.map(([sel, means]) => [sel, { ...shapeOf(sel), means }]));
if (consoleShapes.size < 10) {
  console.error(`mark-vocabulary-check: FAILED — read ${consoleShapes.size} console marks, expected at least 10`);
  process.exit(1);
}

// ── 1. THE ATTENTION SHAPE IS THE DIAMOND ON BOTH SURFACES ──────────────────────────────────────
if (panel.waiting !== "diamond") {
  failures.push(`the panel's most urgent state draws "${panel.waiting}", not a diamond — the shape that means "this wants you" has moved`);
}
// FAILURE HAS ITS OWN SHAPE, AND IT IS NOT THE DIAMOND (round 97). A failed last command is the second thing that
// asks for a person, and the panel's diamond is already spent on a question — so the state that says "this broke"
// must be drawn as something else, or the tab strip would carry two meanings on one silhouette. The console's own
// failure diamond is asserted above; this is the panel's side of the same rule.
if (panel.failed === panel.waiting) {
  failures.push(`the panel draws FAILURE as "${panel.failed}" — the shape it already uses for a QUESTION, so the two states that both want a person are one silhouette`);
}
if (!panel.failed || panel.failed === "solid-halo") {
  failures.push(`the panel gives FAILED no shape of its own ("${panel.failed}") — a state the device reports and no surface can draw is the hole this vocabulary closed in round 97`);
}
for (const [sel, s] of consoleShapes) {
  if (s.means !== "failure") continue;
  if (!s.rotated) failures.push(`${sel} means FAILURE and does not draw the diamond (transform: ${s.rotated ? "rotate(45deg)" : "none"}) — the panel spells that state as a diamond`);
  if (s.kind === "empty") failures.push(`${sel} means FAILURE and draws nothing at all`);
}

// ── 2. ABSENT IS A RING ON BOTH — outline only, never a fill ────────────────────────────────────
if (!/ring/.test(panel.off) || !/ring/.test(panel.idle)) {
  failures.push(`the panel's quiet states draw "${panel.off}"/"${panel.idle}" — an absent thing is an OUTLINE on that surface`);
}
for (const [sel, s] of consoleShapes) {
  if (s.means !== "absent") continue;
  if (!(s.kind === "ring" || (s.kind === "empty" && s.dashed))) {
    failures.push(`${sel} means ABSENT and draws "${s.kind}"${s.dashed ? " (dashed)" : ""} — absent is a ring or a dashed ring, on both surfaces`);
  }
}

// ── 3. FINE IS A FILL ON BOTH, and the halo is the family's intensifier, not a second language ──
if (panel.working !== "solid-halo") {
  failures.push(`the panel's active state draws "${panel.working}" — activity is a FILL with a halo, because the halo is what motion rides on`);
}
for (const [sel, s] of consoleShapes) {
  if (s.means !== "fine") continue;
  if (!(s.kind === "solid" || s.kind === "solid-halo")) failures.push(`${sel} means FINE and draws "${s.kind}" — a healthy thing is a fill on both surfaces`);
}

// ─ 4. NO MARK IS TWO THINGS AT ONCE, on either surface ────────────────────────────────────────
// The vocabulary is solid / ring / halo / empty; a fill inside a ring is none of them. The console's own gate
// asserts this too — kept here because the PANEL's data cannot express it and a shared language is the subject.
for (const [sel, s] of consoleShapes) {
  if (s.kind === "ring+fill") failures.push(`${sel} is a FILL inside a RING — the vocabulary is solid / ring / halo / empty, and a mark that is two of them is neither`);
}
if (/ring\+fill/.test(Object.values(panel).join(" "))) {
  failures.push("the panel's vocabulary names a ring+fill — that is not a state, it is two");
}

const distinct = new Set([panel.waiting, panel.working, panel.idle, panel.off]);
if (distinct.size !== 4) {
  failures.push(`the panel's four states share ${4 - distinct.size} silhouette(s) — "shape carries the state" is worthless if two states look alike`);
}
notes.push(`panel: ${Object.entries(panel).map(([k, v]) => `${k}=${v}`).join(" ")}`);
notes.push(`console: ${consoleShapes.size} marks, ${new Set([...consoleShapes.values()].map((s) => s.kind + (s.dashed ? "+dashed" : ""))).size} distinct shapes`);

for (const n of notes) console.log(`note: ${n}`);
if (failures.length) {
  for (const f of failures) console.error(`mark-vocabulary-check: ${f}`);
  console.error(`mark-vocabulary-check: FAILED — ${failures.length} disagreement(s) between the two surfaces' mark languages`);
  process.exit(1);
}
console.log(`mark-vocabulary-check: ok — ${Object.keys(panel).length} panel states and ${consoleShapes.size} console marks agree on one silhouette per meaning (attention=diamond, absent=ring, fine=fill)`);