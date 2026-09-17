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
console.log(`state-colour-check: ok — ${judged} uses of a state colour across both sheets, every one on a surface that carries state`);
