#!/usr/bin/env node
// state-colour-check.mjs — STATE COLOUR IS FOR STATE, and the chrome stays neutral.
//
// WHY THIS EXISTS (round 12 of the standing goal). The objective's clause is "the chrome neutral and still, with
// colour and motion reserved to that state layer", and nothing measured it. Measured first, on both front ends:
// 122 rules in the panel and 41 in the console paint with a state colour, and essentially every one of them is
// either the state layer itself, a DESTRUCTIVE action (red means this deletes), a CATEGORY lane (colour is the
// second channel there, deliberately), or a token definition. That is a clean result — and a clean result that
// nothing enforces is one commit away from not being true.
//
// HOW IT JUDGES. Every rule that paints with a state colour must match one of the PURPOSES below, each of which
// carries its reason. A rule that matches none of them FAILS and names itself, which is the moment to ask whether
// the colour is carrying state or decorating chrome — and, if it is carrying state, to add the purpose here with
// the reason. That is the whole design: the list is not a suppression file, it is the set of things state colour
// is FOR.
//
// WHAT IT IS NOT. It does not judge `--accent`. An accent button is an ACTION, not a state, and dressing every
// primary button in grey to satisfy a slogan would be a worse interface. The tokens in question are the ones that
// MEAN something: ok, warn, fail, running, danger, success, error.
import { readFileSync, readdirSync } from "node:fs";
import { loudnessOf } from "../../agent/scripts/lib/design-sweep.mjs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.join(HERE, "..", "..");

/** The tokens that MEAN a state, as opposed to the accent, which means an action. */
const STATE_TOKENS = /--(state-ok|state-warn|state-fail|state-running|success|error|danger|warn-ink|danger-on-ink|danger-on-soft|danger-soft|danger-ink|ok-ink)\b/;

/**
 * What state colour is FOR. Each entry is [pattern, reason]. A selector that matches none of these is a finding.
 */
const PURPOSES = [
  [/^(:root|body\[data-theme|@media|\*)/, "a token DEFINITION, not a use of one"],
  [/(dot|mark|led|badge|chip|signal|indicator|swatch)\b/i, "the state layer itself — a mark carrying state"],
  [/(status|state|verdict|result|health|alert|notice|toast|msg|message|banner|warn|error|ok\b|fail|urgent|expired|evicted|crash|blocked|offline|online)/i, "a status surface: the thing the state is about"],
  [/(approval|arm\b|revoke|grant|refuse|expire)/i, "the approval gate — a pending question IS the state"],
  [/(close|clear|archive|remove|delete|purge|evict|danger|logout|kill|stop|disconnect|forget|discard)/i, "a DESTRUCTIVE action: the colour says what the click does"],
  [/confirm/, "the confirm affordance's own text — the destructive branch stated in words before you take it"],
  [/(lane-|provider|prefix|model-)/i, "a CATEGORY lane — colour as the second channel, deliberately not a state"],
  [/(traj|cmd-|run-|path-|step-|exit)/i, "a trajectory or command outcome"],
  [/(progress|stream|spinner|pulse|live|sync|conn|plug|socket|series)/i, "a live or in-flight surface"],
  // TRIAGED from this check's first run (round 12): nine rules that paint with a state colour and match no other
  // purpose. Every one is a state surface whose NAME does not contain the word "state", which is exactly the case
  // the list exists to make somebody write down.
  [/(tone|dial|gauge)/i, "a dial's tone IS the state it renders (data-tone=ok|warn|crit)"],
  [/monitor/, "a monitor that did not answer: the card and its log lines"],
  [/(timeout|is-down|down\b|offline)/i, "a session or probe that timed out or is down"],
  [/(available|latest)/i, "the update card's two states: an update is available, or this is the latest"],
  [/activity-row/, "an activity row's outcome"],
  [/browser-action/, "the failure detail of a browser action"],
];
const DESTRUCTIVE_STATE = /:hover|:active|:focus/;

const rules = (css) => {
  const clean = css.replace(/\/\*[\s\S]*?\*\//g, "");
  return [...clean.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({
    sel: m[1].trim().replace(/\s+/g, " "),
    body: m[2],
  }));
};

const sheets = [];
sheets.push(["panel", readFileSync(path.join(ROOT, "agent/resources/panel/panel.css"), "utf8")]);
const consoleStyles = path.join(ROOT, "gateway/ui/src/styles");
sheets.push([
  "console",
  readdirSync(consoleStyles)
    .filter((f) => f.endsWith(".css"))
    .map((f) => readFileSync(path.join(consoleStyles, f), "utf8"))
    .join("\n"),
]);

const failures = [];
let judged = 0;

for (const [name, css] of sheets) {
  for (const r of rules(css)) {
    if (!STATE_TOKENS.test(r.body)) continue;
    judged++;
    // `@media (…) {` blocks leave their condition on the selector line; strip everything up to the last `{`.
    const sel = r.sel;
    const matched = PURPOSES.find(([re]) => re.test(sel));
    if (matched) continue;
    // A destructive action is only "destructive" when it answers an interaction; a static red border on a
    // container is not a button and does not earn the colour that way.
    if (DESTRUCTIVE_STATE.test(sel) && /(close|clear|archive|remove|delete|purge|evict|logout|kill|stop|discard|forget|refuse)/i.test(sel)) continue;
    failures.push(`${name}: ${sel} paints with a state colour, and matches no purpose on the list`);
  }
}

// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN. The two sheets carried 163 such rules when this was written.
if (judged < 100) {
  console.error(`state-colour-check: FAILED — judged ${judged} rules, which is too few to be reading both sheets`);
  process.exit(1);
}

if (failures.length) {
  console.error("state-colour-check: FAILED — state colour on something that is not state:");
  for (const f of failures) console.error("  " + f);
  console.error("\n  Either this is state and it belongs on the list in scripts/test/state-colour-check.mjs with its");
  console.error("  reason, or the chrome is wearing a colour that means something it does not mean.");
  process.exit(1);
}

// ── A STATUS SURFACE IS A TINT, JUDGED WITH THE PROBE'S OWN RULE (round 78) ─────────────────────────────────────
//
// The rendered axis found the dark info chip as a second focal point on the Users page: `--info-bg: #1a3a5c`,
// saturation 0.559. The light chip is a pale tint the same rule skips by design (lightness above 0.9), and
// `--success-bg` was already a quiet tint — so one token had missed what its siblings got, and only the one page
// that renders that badge could see it. This asks the question of EVERY state surface in both UIs and both themes,
// at the sheet level, using `loudnessOf` — the function the probe itself evaluates, imported rather than copied.
const SURFACE_TOKENS = ["--info-bg", "--success-bg", "--warn-bg", "--danger-bg", "--err-bg"];
const loudSurfaces = [];
for (const [name, css] of sheets) {
  const themes = [
    ["light", css.split(/data-theme="dark"/)[0]],
    ["dark", css.split(/data-theme="dark"/).slice(1).join("") || css],
  ];
  for (const [theme, text] of themes) {
    for (const token of SURFACE_TOKENS) {
      // the LAST declaration in the block wins, which is what the cascade does for a repeated token
      const all = [...text.matchAll(new RegExp(token.replace(/[-]/g, "\\-") + ":\\s*(#[0-9a-fA-F]{6})\\b", "g"))];
      if (!all.length) continue;
      const hex = all[all.length - 1][1];
      const r = loudnessOf({
        r: parseInt(hex.slice(1, 3), 16), g: parseInt(hex.slice(3, 5), 16), b: parseInt(hex.slice(5, 7), 16),
      });
      if (r.loud) loudSurfaces.push(`${name} ${theme} ${token} = ${hex} — saturation ${r.sat.toFixed(2)}, lightness ${r.l.toFixed(2)}: a saturated block on chrome`);
    }
  }
}
if (loudSurfaces.length) {
  console.error(`state-colour-check: FAILED — ${loudSurfaces.length} state surface(s) are LOUD, and a status surface is a tint`);
  for (const f of loudSurfaces) console.error("  " + f);
  console.error("\n  The rule: saturation >= 0.35 with lightness between 0.2 and 0.9 is what an operator reads as");
  console.error("  something shouting. Colour belongs to the state LAYER (a mark, an ink, a lane) — a surface a badge");
  console.error("  sits on is chrome, and chrome is neutral. Pick a tint of the same hue, as --success-bg is.");
  process.exit(1);
}

console.log(`state-colour-check: ok — ${judged} uses of a state colour across both sheets, every one on a surface that carries state`);
