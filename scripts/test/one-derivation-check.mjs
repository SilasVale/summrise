// one-derivation-check.mjs — ONE FACT, ONE DERIVATION: the panel may not map an ENDING to a state twice.
//
// WHY THIS EXISTS (round 78 of the standing goal). The spine of this objective is "one source of truth per fact, with no
// surface computing its own version of the same fact" — and until this gate, that rule was enforced by READING. Round 29
// paid for it: `TrajectoryView` kept a private derivation that mapped a BACKGROUNDED command to `warn` while the
// canonical one mapped it to `bg`, so one backgrounded command wore two different states in the same view, and both
// looked deliberate. The duplicate was deleted; nothing stopped the next one.
//
// WHAT IT CHECKS, and it is deliberately NARROW: the endings the device can report are listed in
// `contract.gen.ts`, GENERATED from `agent/src/vocabulary.rs`. The one place allowed to turn an ending into a
// `PathState` is `lib/path.ts` (`stateFromEnd`, and `cardState` as its two-line adapter). Any OTHER production module
// that compares against one of those literals is a second derivation, and this fails by file and line.
//
// WHAT IT DOES NOT CHECK: the mapping itself (that is the derivation's own tests), the shapes (the sheet and rendered
// checks), or anything about the console — its device state is a different fact with its own fixtures.
//
// Run: node scripts/test/one-derivation-check.mjs
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const PANEL = join(ROOT, "agent/resources/panel-react/src");

/** The endings, from the generated contract, so this check follows the vocabulary instead of copying it. */
const gen = readFileSync(join(PANEL, "lib/contract.gen.ts"), "utf8");
const m = /export const END_REASONS = (\[[^\]]*\]) as const;/.exec(gen);
if (!m) {
  console.error("FAIL contract.gen.ts has no END_REASONS — this check would be scanning for nothing");
  process.exit(1);
}
const REASONS = JSON.parse(m[1]);
const PREFIX = /export const EXITED_PREFIX = "([^"]+)";/.exec(gen)?.[1];
if (!PREFIX) {
  console.error("FAIL contract.gen.ts has no EXITED_PREFIX — this check would be scanning for nothing");
  process.exit(1);
}

/** The one module allowed to derive a state from an ending. */
const ALLOWED = ["lib/path.ts", "lib/contract.gen.ts"];

/**
 * WORDS THAT ARE SHARED BUT FACTS THAT ARE NOT, declared per file with a reason. The first run of this gate found seven
 * comparisons; reading them took a minute and NONE was a second derivation — they are four different vocabularies that
 * happen to use the same words, plus the input side of the derivation itself. That is the distinction the gate exists to
 * make somebody write down, which is why this is a list of reasons rather than a wider pattern:
 *
 *   - the panel's own action verdicts (`ok` / `timeout` / `err`) are about a BROWSER ACTION, not a command;
 *   - the update card's `phase` is its own state machine;
 *   - an eviction's `cause` is why a session was dropped, not how a command ended;
 *   - and `terminalStatus` is UPSTREAM of the derivation: it turns a terminal marker into the reason that `stateFromEnd`
 *     then consumes, so it must name the same words.
 */
const SHARED_WORDS = {
  "components/EvidenceDrawer.tsx": "action verdicts (ok/timeout/err) — a browser action's state, not a command ending",
  "components/UpdateCard.tsx": "the update card's own phase machine",
  "lib/evicted.ts": "an eviction cause (idle/cap) — why a session was dropped",
  "hooks/useCommandEvents.ts": "UPSTREAM of the derivation: it turns a terminal marker into the reason stateFromEnd consumes",
};

function files(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) {
      if (name === "__tests__" || name === "node_modules") continue;
      out.push(...files(p));
    } else if (/\.tsx?$/.test(name)) out.push(p);
  }
  return out;
}

let scanned = 0;
const offenders = [];
for (const f of files(PANEL)) {
  const rel = relative(PANEL, f);
  scanned++;
  if (ALLOWED.includes(rel)) continue;
  if (SHARED_WORDS[rel]) continue;
  const lines = readFileSync(f, "utf8").split("\n");
  lines.forEach((line, i) => {
    // a comparison or a switch case against an ending — the shape a second derivation takes
    for (const r of REASONS) {
      if (new RegExp(`[=!]==?\\s*"${r}"|case\\s+"${r}"\\s*:`).test(line)) {
        offenders.push(`${rel}:${i + 1} compares against the ending "${r}"`);
      }
    }
    if (new RegExp(`[=!]==?\\s*"${PREFIX}|startsWith\\("${PREFIX}"\\)`).test(line)) {
      offenders.push(`${rel}:${i + 1} compares against the "${PREFIX}" prefix`);
    }
  });
}

// ── AND THE MARK FAMILIES' CSS STATES (round 129). The clause above guards command ENDINGS; this one guards the states a
// MARK can be in, which rounds 126-128 each had to unify by hand: the monitor family computed `is-up`/`is-down`/`is-flapping`
// in two components with three spellings, and the plugin row paired its state with its label twice. Both now have one home,
// and nothing stopped a fourth spelling from appearing until this clause.
//
// The rule is absolute and cheap: a mark's CSS state literal may appear ONLY in the module that derives it. `is-flapping` in
// a component means that component decides a state the derivation owns.
const MARK_STATES = {
  "is-flapping": "lib/monitorMark.ts",
  "is-up": "lib/monitorMark.ts",
  "is-down": "lib/monitorMark.ts",
};
/** SHARED WORDS, DIFFERENT FACTS — declared with reasons, because this clause's first run flagged a file that is right.
 *
 *   - `MonitorsCard`'s LOG ROWS spell `is-up`/`is-down` for a `<li>`, not for a mark: the element is a transition entry and
 *     its own stylesheet rule is about log rows. The marks in that file already come from the derivation.
 *
 *  This is the third time in one session that a rule matching a WORD was narrower than the rule it was trying to state — the
 *  others being `one-derivation`'s own first run (four vocabularies sharing a word) and `gateway-device-field-check`'s
 *  widening (the upstream providers' vocabulary). Every one was resolved the same way: keep the rule, declare the exception,
 *  write the reason. */
const MARK_STATE_EXCEPTIONS = { "components/MonitorsCard.tsx": "log rows, not marks" };
for (const f of files(PANEL)) {
  const rel = relative(PANEL, f);
  if (rel === "lib/monitorMark.ts" || rel.startsWith("lib/") && rel.endsWith(".test.ts")) continue;
  if (MARK_STATE_EXCEPTIONS[rel]) continue;
  readFileSync(f, "utf8").split("\n").forEach((line, i) => {
    if (line.trimStart().startsWith("//")) return;
    for (const [literal, home] of Object.entries(MARK_STATES)) {
      if (new RegExp(`["'\`]${literal}["'\`]`).test(line)) {
        offenders.push(`${rel}:${i + 1} spells the mark state "${literal}", which ${home} derives`);
      }
    }
  });
}

if (scanned < 60) {
  console.error(`FAIL scanned only ${scanned} panel module(s) — the tree moved, so this proves nothing`);
  process.exit(1);
}
if (offenders.length) {
  console.error(
    `one-derivation: ${offenders.length} second derivation(s) of an ending:\n  ` +
      offenders.join("\n  ") +
      `\n\nEndings become states in lib/path.ts ONLY (stateFromEnd; cardState is its adapter). A second mapping is how ` +
      `one backgrounded command wore two states in one view (round 29).`,
  );
  process.exit(1);
}
const declared = Object.keys(SHARED_WORDS);
console.log(
  `one-derivation: ${scanned} panel module(s) scanned, and lib/path.ts is the only one that turns an ending ` +
    `(${REASONS.length} of them, plus the ${PREFIX} prefix) into a state; ${declared.length} file(s) declared as ` +
    `sharing the WORDS without sharing the fact: ${declared.join(", ")}`,
);
