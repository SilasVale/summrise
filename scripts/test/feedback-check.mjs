#!/usr/bin/env node
// feedback-check.mjs — every thing that answers a HOVER must answer a PRESS, and no transition may cost a layout.
//
// WHY THIS EXISTS (round 246). Measured on the built sheet: 74 selectors with `:hover` and **3** with `:active`.
// The panel answered a pointed-at cursor everywhere and answered a PRESSED one almost nowhere — so the feedback
// an operator actually feels when they click was whatever the network did next. That is the difference between
// an interface that responds and one that merely reacts, and it is invisible to every other gate here: contrast
// cannot see it, the design sweep cannot see it (it photographs states, it does not press anything), and no unit
// test touches CSS.
//
// FOUR RULES, each with the measurement behind it:
//
//   1. HOVER IMPLIES PRESS. For every selector that styles `:hover`, one with the same base must style `:active`.
//      Exceptions are listed with a reason, because a timestamp is not pressable.
//   2. NO LAYOUT IN A TRANSITION. A transition on width/height/margin/padding/inset/font-size re-lays-out the page
//      every frame; `transform` and `opacity` are the two the compositor can do alone. The sheet transitioned
//      `width` and used `transition: all`, which is how a hover comes to stutter on a busy device.
//   3. A STATED BUDGET. No transition above 240ms: past that, an interface feels slow rather than responsive.
//   4. MOTION IS OPTIONAL. The sheet must carry at least one `prefers-reduced-motion` block, or the responses
//      it adds cannot be turned off.
//
// IT READS THE BUILT SHEET (resources/panel/panel.css), like the other CSS contracts here, so it measures what
// ships rather than what the source intends.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const CSS_PATH = fileURLToPath(new URL("../../agent/resources/panel/panel.css", import.meta.url));
const css = readFileSync(CSS_PATH, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");

// THE WHOLE SELECTOR LIST, NOT ITS LAST LINE. The idiom `.split("\n").pop()` is used elsewhere to drop a
// banner line above a rule, and it silently discards every selector but the last when the sheet groups a comma
// list across lines — which is exactly how the press layer is written, so the first run of this check saw 6
// `:active` rules instead of ~90 selectors and reported the whole layer as missing.
const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({
  sel: m[1].trim(),
  body: m[2],
}));

/** A selector without its state, so `.btn:hover` and `.btn:active` are recognised as the same thing. */
// `:not(...)` IS STRIPPED TOO, and leaving it in is how the first run of this check reported `.btn` as having no
// press state when it has had one since round 148 (`transform: translateY(1px)`): the base of
// `.btn:active:not(:disabled)` came out as `.btn:not()`, which matches nothing. A gate that misreads a rule as
// absent is worse than no gate — it sends you to add what is already there.
const base = (s) => s.replace(/:hover|:active|:focus(-visible)?|:disabled/g, "").replace(/:not\([^)]*\)/g, "").replace(/\s+/g, " ").trim();

// THE HOVERED ELEMENT MUST BE THE SUBJECT. `.tab:hover .tab-export` is not a pressable thing — it REVEALS a
// descendant, and the state belongs to the tab, not to the export button. A gate that demands a press state from
// a reveal rule is a gate people learn to ignore, so those are recognised and skipped here rather than listed as
// exceptions one by one.
const isReveal = (s) => /:hover\s+\S/.test(s);
const hovers = new Map();
const actives = new Set();
let reveals = 0;
for (const r of rules) {
  for (const one of r.sel.split(",")) {
    const s = one.trim();
    if (/:hover/.test(s)) {
      if (isReveal(s)) { reveals++; continue; }
      hovers.set(base(s), s);
    }
    if (/:active/.test(s)) actives.add(base(s));
  }
}

// NOT PRESSABLE, each with its reason. A gate that demands a press state from a timestamp teaches people to
// ignore the gate.
const NOT_PRESSABLE = new Map([
  ["*::-webkit-scrollbar-thumb", "a scrollbar thumb is dragged, not pressed"],
  [".side-row .side-time", "the row's relative timestamp — text, not a control"],
  [".log-line", "a log line is read, not clicked"],
  [".mono-line", "a log line is read, not clicked"],
]);

// A PRESS STATE ON A BASE COVERS ITS VARIANTS, which is what CSS does: `.btn:active` matches an element that
// carries both `btn` and `primary`, so `.btn.primary:hover` is already answered by `.btn:active`. Requiring the
// two bases to be identical reported every variant as a gap — 64 of them — and would have sent me to write
// rules that change nothing.
const hasPress = (b) => [...actives].some((a) => b === a || b.startsWith(a + ".") || b.startsWith(a + ":"));

const failures = [];
// ── A PRESS WHOSE EVERY PROPERTY IS SHADOWED IS A PRESS THAT CANNOT BE SEEN (round 52).
//
// Round 51 measured presses AS RENDERED for the first time and found the ACTIVE TAB dead: `.tab.active` and
// `.tab:active` are both (0,2,0), the sheet put the state rule later, and the press rule's only property was a
// background — so the one control an operator presses to re-focus it did nothing at all ON SCREEN. This file's other
// checks cannot see that: they prove a rule EXISTS.
//
// THE RULE IS "EVERY PROPERTY", not "any": a press that also moves is a press you can see, so `.tab:active`'s
// background being shadowed is harmless now that a transform survives. Only when NOTHING of the press reaches the
// screen is it the defect.
const specificity = (s) => {
  const ids = (s.match(/#[\w-]+/g) || []).length;
  const classes = (s.match(/\.[\w-]+|\[[^\]]+\]|:(?!:)[\w-]+(\([^)]*\))?/g) || []).length;
  const elements = (s.replace(/[#.][\w-]+|\[[^\]]+\]|::?[\w-]+(\([^)]*\))?/g, " ").match(/[a-zA-Z][\w-]*/g) || []).length;
  return [ids, classes, elements];
};
const atLeast = (a, b) => (a[0] !== b[0] ? a[0] > b[0] : a[1] !== b[1] ? a[1] > b[1] : a[2] >= b[2]);
const declsOf = (body) => {
  const m = new Map();
  for (const d of body.matchAll(/(?:^|[;{\s])([a-z-]+)\s*:\s*([^;}]+)/g)) m.set(d[1], d[2].trim());
  return m;
};
// A WHOLE CLASS TOKEN, NOT A SUBSTRING. `arm.includes(".btn")` is true of `.btn-ghost:active:not(:disabled)` — the
// first run of this check reported `.btn`'s press as shadowed for that reason alone, while the rendered measurement
// had already watched it move. The bar is a value comparison too: a later rule that sets the SAME declaration is not
// a shadow, it is the same instruction said twice.
const escapeRe = (x) => x.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
// AND THE KEY MUST BE THE SUBJECT OF THE RULE — the element the press belongs to, not an ancestor of it.
// `.cmd-btn.cmd-toggle.open svg` and `.traj-round.open .traj-chev` name `.cmd-btn` and `.traj-round` while styling a
// descendant, and the first run of this check called those shadows: two more findings that the rendered measurement
// would have had to disprove one at a time.
const subjectOf = (arm) => arm.split(/\s+|[>+~]/).filter(Boolean).pop() || "";
const namesKey = (arm, key) =>
  key.includes(" ")
    ? new RegExp(escapeRe(key) + "(?![\\w-])").test(arm)
    : new RegExp(escapeRe(key) + "(?![\\w-])").test(subjectOf(arm));

/** Every press, grouped by WHAT IS PRESSED: `.tab:active` and `.tab:active:not(.closed)` are one control. */
const presses = new Map();
const arms = rules.map((r, i) => ({ i, arms: r.sel.split(",").map((x) => x.trim()), body: r.body }));
for (const r of arms) {
  for (const arm of r.arms) {
    if (!/:active/.test(arm)) continue;
    const key = base(arm).replace(/:not\([^)]*\)/g, "").trim();
    if (!key) continue;
    const entry = presses.get(key) || { key, decls: new Map(), order: [] };
    for (const [prop, value] of declsOf(r.body)) entry.decls.set(prop, value);
    entry.order.push({ i: r.i, arm, body: r.body });
    presses.set(key, entry);
  }
}
// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN: 68 pressable hover selectors were measured when this was written,
// and the press layer has grown every time somebody added a control.
if (presses.size < 20) {
  console.error(`feedback-check: FAILED — found ${presses.size} press rule(s), and this sheet has dozens; the shadow scan is reading the wrong thing`);
  process.exit(1);
}
for (const entry of presses.values()) {
  const surviving = [];
  for (const [prop, value] of entry.decls) {
    // SHADOWED WHEN A LATER RULE FOR THE SAME ELEMENTS SETS THE SAME PROPERTY AT >= SPECIFICITY
    // THE STRONGEST ARM OF THE PRESS, not the weakest. A press written as `.btn:active:not(:disabled)` is (0,3,0)
    // while `.btn:active` alone is (0,2,0) — and comparing a later rule against the WEAKER one reported `.btn`'s
    // press as shadowed when the rendered measurement had already shown it moving. The grouped arms are one control,
    // so the bar a later rule must clear is the highest one in the group.
    const bar = entry.order.map((o) => specificity(o.arm)).reduce((a, b) => (atLeast(a, b) ? a : b));
    const shadow = arms.find((r) => {
      if (r.i <= Math.max(...entry.order.map((o) => o.i))) return false;
      if (!r.arms.some((arm) => namesKey(arm, entry.key) && atLeast(specificity(arm), bar))) return false;
      const later = declsOf(r.body).get(prop);
      return later !== undefined && later !== value;
    });
    if (!shadow) surviving.push(prop);
  }
  if (!surviving.length && entry.decls.size) {
    failures.push(`${entry.key}:active sets ${[...entry.decls.keys()].join(", ")} — and EVERY one is shadowed by a later rule, so the press cannot be seen. Round 51's active tab was this: .tab.active put a background over .tab:active's only property. Give the press a transform, which no background rule can override`);
  }
}

const missing = [...hovers.keys()].filter((k) => !hasPress(k) && !NOT_PRESSABLE.has(k));
for (const sel of missing) failures.push(`no :active for ${hovers.get(sel)} — a hover that answers and a press that does not is the feedback gap this checks for`);

// ── 2/3. transitions: what they touch and how long they take
const LAYOUT_PROPS = ["width", "height", "margin", "padding", "top", "right", "bottom", "left", "inset", "font-size", "line-height"];
// DOCUMENTED EXCEPTIONS, each with the reason it cannot be compositor-only. Named by selector AND property so a
// second one cannot ride along on a vague match.
const LAYOUT_OK = [
  { sel: ".side-section-body", prop: "max-height", why: "an accordion that opens by height has no transform equivalent — its content must actually take space" },
  { sel: ".prov-body", prop: "max-height", why: "same accordion idiom" },
  { sel: ".facets-body", prop: "max-height", why: "same accordion idiom" },
  { sel: ".browser-embedded-slot", prop: "width", why: "NOT a paint: the slot sizes an Electron WebContentsView, and the ResizeObserver reports these bounds to the main process. A transform would MOVE that native view instead of shrinking it, so the width is the function, not the animation" },
];
const MAX_TRANSITION_MS = 240;
let transitions = 0;
for (const r of rules) {
  for (const m of r.body.matchAll(/transition:\s*([^;]+);/g)) {
    transitions++;
    const parts = m[1].split(",").map((p) => p.trim());
    for (const part of parts) {
      const prop = part.split(/\s+/)[0];
      if (prop === "all") {
        failures.push(`${r.sel} transitions ALL — every property, including the ones that re-lay-out, is animated on every change`);
      } else if (LAYOUT_PROPS.includes(prop) && !LAYOUT_OK.some((ok) => r.sel.includes(ok.sel) && ok.prop === prop)) {
        failures.push(`${r.sel} transitions ${prop}, which re-lays-out the page every frame (transform and opacity do not)`);
      }
      const ms = /([\d.]+)m?s/.exec(part);
      if (ms) {
        const value = ms[0].endsWith("ms") ? parseFloat(ms[0]) : parseFloat(ms[0]) * 1000;
        if (value > MAX_TRANSITION_MS) failures.push(`${r.sel} transitions over ${MAX_TRANSITION_MS}ms (${ms[0]}) — past that it reads as slow, not responsive`);
      }
    }
  }
}

// ── 4. motion is optional
if (!/prefers-reduced-motion/.test(css)) {
  failures.push("no prefers-reduced-motion block: the feedback this sheet adds cannot be turned off");
}

// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN. The sheet has had dozens of hovers for hundreds of rounds; a
// count near zero means this file is reading the wrong thing, not that the panel got simple.
if (hovers.size < 30 || transitions < 10) {
  console.error(`feedback-check: FAILED — read ${hovers.size} hover selectors and ${transitions} transitions from ${CSS_PATH}, so this proves nothing`);
  process.exit(1);
}

if (failures.length) {
  console.error("feedback-check: FAILED");
  for (const f of failures) console.error("  " + f);
  process.exit(1);
}
console.log(`feedback-check: ok — ${hovers.size} pressable hover selectors, all with a press state (${reveals} reveal rules skipped); ${presses.size} presses with at least one property that survives the cascade; ${transitions} transitions, none touching layout, all within ${MAX_TRANSITION_MS}ms; reduced-motion honoured`);
