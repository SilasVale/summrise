#!/usr/bin/env node
// chrome-stillness-check — THE CHROME IS NEUTRAL AND STILL, and this is what "still" means.
//
// The objective reserves colour AND motion to the state layer: "the chrome neutral and still, with colour and motion
// reserved to that state layer". Colour has had `state-colour-check.mjs` since round 245. Motion had only
// `motion-check.mjs`, which asks whether an animation is SILENCED under `prefers-reduced-motion` — a different
// question. Nothing asked whether an animation has any business existing, so a decorative pulse would have been
// silenced for the users who ask for that and left running for everybody else.
//
// THE LIST IS THE CHECK, and each entry is a PURPOSE with a reason — not a suppression file. That distinction is the
// whole design: `state-colour-check` writes it down as "each entry is a PURPOSE with a reason, which is the case the
// list exists to make somebody write down", and it deliberately does NOT judge `--accent` because an accent button is
// an ACTION. Here the three purposes are:
//
//   STATE       motion that describes something still happening. May run forever, because the thing it describes has
//               not stopped — and this is the mark language's own motion channel, the one that DISAPPEARS under
//               reduced motion, which is exactly why the shape has to carry the state on its own.
//   ENTRANCE    a thing arriving: a drawer, a menu, a card, a toast. One-shot, at most 400ms.
//   ATTENTION   "this changed while you were looking elsewhere". One-shot, may outlast an entrance (a flash has to
//               be long enough to be noticed), at most 5s.
//
// A NEW ANIMATION FAILS UNTIL SOMEBODY WRITES DOWN WHICH IT IS. That is the point of the table rather than a regex:
// my first version of this file classified by SELECTOR NAME and reported five defects that were not defects —
// `.mem-busy` and `.browser-ev-live` are state-bearing and the pattern could not see it, and the two `.flash` rules
// are attention states that decay. Reading the markup settled it: `.browser-ai-dot` and `.browser-ev-live` are
// rendered only while `aiActive` holds. The fourth time this session a probe has reported a defect the real rule did
// not have, and the fourth time the fix was to write the rule down instead of approximating it.
//
// Measured 2026-09-18: panel 11, console 4 — 5 infinite (every one state-bearing) and 10 one-shot.
//
// A FLOOR IS PART OF THE CHECK: a sheet whose animations stopped being parsed would otherwise pass this perfectly.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

const KINDS = {
  state: { infinite: true, maxMs: null },
  entrance: { infinite: false, maxMs: 400 },
  attention: { infinite: false, maxMs: 5000 },
};

const PURPOSES = [
  // ── the panel ──────────────────────────────────────────────────────────────────────────────────
  { sheet: "panel", selector: '.cmd-dot[data-state="running"]', kind: "state", why: "a command is in flight — the running mark's motion channel" },
  { sheet: "panel", selector: '.traj-ev-dot[data-state="running"]', kind: "state", why: "the same state inside the trajectory view" },
  { sheet: "panel", selector: '.plug-dot[data-state="ongoing"]', kind: "state", why: "the plugin reports its work as ongoing" },
  { sheet: "panel", selector: ".browser-ai-dot", kind: "state", why: "rendered only while aiActive holds — the agent is operating the browser" },
  { sheet: "panel", selector: ".mem-busy", kind: "state", why: "the memory view is waiting on the device" },
  { sheet: "panel", selector: ".browser-ev-live", kind: "state", why: "rendered only while aiActive holds — the action log is being appended to" },
  { sheet: "panel", selector: ".browser-ev-toggle.flash", kind: "attention", why: "new browser actions arrived while the log was closed" },
  { sheet: "panel", selector: ".desktop-status.flash", kind: "attention", why: "the device status line just changed" },
  { sheet: "panel", selector: ".gs-card", kind: "entrance", why: "the getting-started card arrives" },
  { sheet: "panel", selector: "#drawer", kind: "entrance", why: "the details drawer slides in" },
  { sheet: "panel", selector: ".new-menu", kind: "entrance", why: "the new-session menu opens" },
  // ── the console ────────────────────────────────────────────────────────────────────────────────
  { sheet: "console", selector: ".user-pop", kind: "entrance", why: "the account menu opens" },
  { sheet: "console", selector: ".toast", kind: "entrance", why: "a toast lands" },
  { sheet: "console", selector: ".loading-spinner", kind: "state", why: "a request is in flight" },
  { sheet: "console", selector: ".skeleton-card", kind: "state", why: "content is still loading" },
];

const SHEETS = [
  { label: "panel", path: "agent/resources/panel/panel.css", minAnimations: 8 },
  { label: "console", path: "gateway/ui/src/styles/globals.css", minAnimations: 3 },
];

const failures = [];
const seen = new Set();

const parseMs = (value) => {
  const m = /(\d+(?:\.\d+)?)(ms|s)\b/.exec(value);
  if (!m) return null;
  return m[2] === "s" ? parseFloat(m[1]) * 1000 : parseFloat(m[1]);
};

for (const { label, path, minAnimations } of SHEETS) {
  // Comments are stripped first: prose ABOUT an animation is not one (two gates in this directory learned that the
  // hard way, one of them reporting ten files that merely recorded a removal).
  const sheet = readFileSync(join(ROOT, path), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  const found = [];
  for (const m of sheet.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const sels = m[1].split(",").map((s) => s.trim()).filter(Boolean);
    const decl = /(?:^|;)\s*animation\s*:\s*([^;]+)/.exec(m[2]);
    if (!decl) continue;
    const value = decl[1].trim();
    if (/^none\b/.test(value)) continue;
    for (const selector of sels) found.push({ selector, value, infinite: /\binfinite\b/.test(value) });
  }
  if (found.length < minAnimations) {
    failures.push(`${label}: read ${found.length} animation(s), expected at least ${minAnimations} — a floor that is not met proves nothing about stillness`);
    continue;
  }
  for (const a of found) {
    seen.add(`${label}|${a.selector}`);
    const declared = PURPOSES.find((p) => p.sheet === label && p.selector === a.selector);
    if (!declared) {
      failures.push(`${label}: ${a.selector} animates (${a.value}) and NO PURPOSE IS DECLARED — the chrome is still, and motion belongs to the state layer. Add it to PURPOSES with one of STATE / ENTRANCE / ATTENTION and the reason it is one`);
      continue;
    }
    const kind = KINDS[declared.kind];
    const ms = parseMs(a.value);
    if (kind.infinite && !a.infinite) {
      failures.push(`${label}: ${a.selector} is declared ${declared.kind.toUpperCase()} (${declared.why}) but ${a.value} does not run while the state holds — a state that stops moving is not describing anything`);
    }
    if (!kind.infinite && a.infinite) {
      failures.push(`${label}: ${a.selector} is declared ${declared.kind.toUpperCase()} (${declared.why}) and animates FOREVER (${a.value}) — an entrance or an acknowledgement is a ONE-SHOT`);
    }
    if (kind.maxMs !== null && (ms === null || ms > kind.maxMs)) {
      failures.push(`${label}: ${a.selector} is declared ${declared.kind.toUpperCase()} and runs ${a.value} — ${declared.kind} is capped at ${kind.maxMs}ms (read ${ms === null ? "an unreadable duration" : `${ms}ms`})`);
    }
  }
}

for (const p of PURPOSES) {
  if (!seen.has(`${p.sheet}|${p.selector}`)) {
    failures.push(`${p.sheet}: PURPOSES declares ${p.selector} and the sheet no longer animates it — a stale entry is a reason nobody is using`);
  }
}

if (failures.length) {
  for (const f of failures) console.error(`chrome-stillness-check: ${f}`);
  console.error(`chrome-stillness-check: FAILED — ${failures.length} animation(s) the state layer does not own`);
  process.exit(1);
}
const byKind = (k) => PURPOSES.filter((p) => p.kind === k).length;
console.log(
  `chrome-stillness-check: ok — ${PURPOSES.length} animation(s) across both sheets, every one declared: ` +
    `${byKind("state")} state, ${byKind("entrance")} entrance, ${byKind("attention")} attention`,
);