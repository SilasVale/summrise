// sessionFacts.test.ts — A COMPONENT RENDERS STATE; IT DOES NOT COMPUTE IT.
//
// WHY (round 34 of the standing goal). Two audit rounds in a row found the same class of defect, one fact at a time:
//
//   * round 30 — `idleSessions` offered to CLOSE a session that was mid-command, because it read `idleMs` (output
//     recency) and did not know about the device's `command_running`. An ACTION taken on a wrong fact.
//   * round 33 — three surfaces derived "is this session waiting" three ways; `DesktopShell` omitted the closed
//     check, so a tombstone's title announced "waiting for your approval" while its own mark said `off`.
//
// Both were fixed by moving the fact into `lib/liveness.ts`. This is the ratchet that keeps it there: a COMPONENT may
// not mention the four fields the model turns into marks and labels — `pendingApproval`, `idleMs`, `commandRunning`,
// `heldByHuman` — because each has a model function that every surface is supposed to share:
//
//     pendingApproval  ->  sessionWaiting(session)
//     idleMs           ->  sessionActive(session)          (with command_running, and WORKING_MS)
//     commandRunning   ->  sessionActive / anyCommandRunning
//
// `closed` and `savedOnly` are NOT in the list: a component legitimately decides what it can OFFER from them (a
// tombstone has no close button), and that is a render gate rather than a claim about the session's state.
//
// THE COMMAND FACTS ARE SCANNED TOO (`exitCode`, `reason`, `ended`, and what rides with them), by a second check
// whose shape is a COMPARISON rather than the bare field — see COMMAND_FACTS below for why the ban has to be
// narrower there. It was added when the inventory measured this file's blind spot (§5.4).
//
// COMMENTS ARE STRIPPED FIRST — for the third time this session (rounds 20, 23, and here): a comment that explains
// why a field is NOT read is not a read, and a check that cannot tell them apart deletes its own reasons.
//
// AND A BLOCK COMMENT'S OWN NEWLINES SURVIVE THE STRIP, so the line a hit reports is a line somebody can open.
// Removing them took the newline count with them — `ActivityPage.tsx`'s hit at :98 was reported at :82 — and a line
// number that is sixteen lines off is worse than no number at all. Every character of the comment still goes, so no
// hit is gained or lost, and that half is not left to trust either: the comment the fix added at that chip quotes
// the very expression the second check bans, so a strip that stopped working would fail the check on its own reason.
//
// WHAT THIS CANNOT SEE, said here rather than discovered later, because a gate whose blind spots are undocumented
// gets trusted for more than it measures:
//
//   * IT READS TEXT, NOT CODE. A derivation that puts an intermediate between the component and the field —
//     `const code = row.exitCode; … code === 0` — matches no pattern here. Neither does a component passing the field
//     to something else that decides (`<Chip code={row.exitCode} />`, with the meaning drawn one component down).
//     Both are the shape the ban is aimed at, and both walk past it.
//   * IT READS `.tsx` ONLY, and only `src/components/*.tsx` at the top level — not a recursive walk. `src/App.tsx`,
//     `src/main.tsx`, `src/ui/Icon.tsx` and the `src/components/__tests__/` directory are never opened (a test file
//     that sits at the top level, like `CommandCard.test.tsx`, IS read). Scanning every non-test `.tsx` under `src/`
//     recursively finds nothing today, so the few it skips hide no violation right now — it is a limit, not a
//     backlog. The same limit is why a derivation living in a `.ts` file is out of reach: `lib/path.ts` and
//     `lib/liveness.ts` are where the two owners above ARE, and `lib/archive.ts` labels an archive row from
//     `last.exitCode` while `lib/browserAction.ts` decides a browser action's verdict from the code it took off
//     `exit_code`. Those files are the model — the rule is that a COMPONENT may not re-decide, and a check that
//     walked them would have to exempt the very functions it exists to protect.
//   * IT SEES THE FIRST COMPARISON IN A FILE, like the field scan: one offender per file and field, so a second one
//     below it is not named until the first is fixed. It is a ratchet, not a census.
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SRC = path.dirname(fileURLToPath(import.meta.url));
const COMPONENTS = path.join(SRC, "..", "components");

/** The fields a component must take from the model, and what to use instead. */
// WHY `exitCode` / `reason` / `ended` ARE NOT ON *THIS* LIST, AND ARE SCANNED ANYWAY. This scan bans a FIELD wherever
// it appears in a .tsx, because the four session fields below are only ever DERIVED from — a component that reads
// one is inventing a second opinion. The command facts are different: a component legitimately PRINTS `exit 3` (the
// activity row's chip does, and so does the trajectory's end row), draws an ABSENT code as absent
// (`card.exitCode !== null ? card.exitCode : "—"`) and asks `!card.ended` to decide whether to keep a clock ticking.
// A blanket ban would forbid display to catch derivation, and its price would be a whole-file allowance for each of
// the surfaces that legitimately mention one — and a file-wide allowance covers everything else anyone adds to that
// file. So they are covered by the SECOND check below, whose pattern is a COMPARISON against a value rather than the
// bare field: that is the shape that can disagree. It was added because the inventory measured this hole (§5.4) and
// found `ActivityPage` still comparing the code itself; the derivation it must call is `stateFromEnd` in
// lib/path.ts, called by the command card, the details panel, the path summary and the trajectory's event dot.
const MODELLED = new Map([
  ["pendingApproval", "sessionWaiting() — the ONE predicate the mark, the title and the label read"],
  ["idleMs", "sessionActive() — it knows about command_running and WORKING_MS"],
  ["commandRunning", "sessionActive() / anyCommandRunning()"],
  ["heldByHuman", "the session's holder is drawn by the control that owns it"],
]);

/**
 * Named files that forward a value instead of deriving one. Each needs its reason: a file-wide allowance also
 * covers anything else somebody adds to it. The second check reads this SAME map — one idiom for a named exemption,
 * not two, and the same cost: the allowance covers command facts too.
 */
const ALLOWED = new Map([
  [
    "TerminalWorkspace.tsx",
    "passes `held={!!activeSession.heldByHuman}` to SessionControl, the one component that draws it — it forwards the device's value and derives nothing",
  ],
]);

/** The facts that decide HOW A COMMAND ENDED, and the one derivation that maps them.
 *
 *  The mapping has ONE owner: `stateFromEnd` (lib/path.ts), which the command card, the details panel, the path
 *  summary and the trajectory's event dot all call — that is what makes one backgrounded command wear one state in
 *  every view, and a second copy of the map is how it wore two. The inventory named three of these (§5.4); the two
 *  below ride with them because they are the same question under another name, and a component that asks it itself
 *  has made the same second opinion: `lastExitCode` is the session's LAST command rather than one card's (the one
 *  predicate is `sessionFailed` in lib/liveness.ts, whose own comment says "a second `!== 0` written somewhere else
 *  is how two of them come to disagree"), and `exit_code` is the wire spelling a component reads straight off the
 *  event instead of handing to the derivation.
 */
const COMMAND_FACTS = new Map([
  ["exitCode", "stateFromEnd(ended, exitCode, reason).state — `ok` is its word for an exit of 0, `fail` for any other"],
  ["exit_code", "stateFromEnd(ended, exit_code ?? null, reason ?? null) — the device's spelling of the same fact"],
  ["reason", "stateFromEnd(ended, exitCode, reason) — the reason-to-state table lives there, keyed by the device's own vocabulary"],
  ["ended", "stateFromEnd(ended, exitCode, reason).state — `running` is its word for a command with no recorded end"],
  ["lastExitCode", "sessionFailed(session) — the one predicate for a session whose last command failed"],
]);

/** The shape of a second opinion: a command fact compared against a VALUE.
 *
 *  `!== null` / `== null` / `=== undefined` are the panel's presence tests — an absent code is DRAWN as absent, and
 *  that decides nothing — so they are not this rule's business. Everything else is a component answering "how did
 *  this end" for itself.
 *
 *  TWO GUARDS, AND BOTH ARE LOAD-BEARING. The first version of this line had neither, and it fired on
 *  `DetailsPanel`'s absent exit code — the one rendering the rule exists to leave alone. Both failures are regex
 *  backtracking, and both are pinned by the two probes at the end of the check rather than left to trust:
 *
 *    (?![=])                        keeps the operator whole. Without it the engine shortens `!==` to `!=`, and the
 *                                   null test below is then measured from the `=` it did not consume.
 *    (?!\\s*(?:null|undefined)\\b)   excludes the presence tests — with the whitespace INSIDE the lookahead, because
 *                                   written as `…!==\\s*(?!null)` the `\\s*` backtracks to zero width, the lookahead
 *                                   then sees " null" (which does not begin with `null`) and passes. */
const comparesFact = (field: string) =>
  new RegExp(`\\b${field}\\s*(?:===|!==|==|!=)(?![=])(?!\\s*(?:null|undefined)\\b)`);

describe("session state", () => {
  it("names its refresh cadence once, and lists every trigger that exists", () => {
    // ROUND 50, AND THE SAME SHAPE AS THE TWO CHECKS BEFORE IT IN THE CONSOLE. `useSessions` carries a round-163 comment
    // that lists what refreshes the session list. Round 245 added a 30-second background sweep and documented it AT
    // THE INTERVAL — and the list, forty lines above, quietly became incomplete. Nothing was false; something was
    // missing, which is how a reader deciding whether a refresh will arrive gets misled.
    const hook = readFileSync(path.join(SRC, "..", "hooks", "useSessions.ts"), "utf8");
    // the cadence is a named constant, used by the interval and by nothing else as a literal
    const named = /const SESSIONS_SWEEP_MS = ([0-9_]+);/.exec(hook);
    expect(named, "SESSIONS_SWEEP_MS is gone — the cadence is a literal again").toBeTruthy();
    expect(hook).toContain("}, SESSIONS_SWEEP_MS);");
    expect(
      /setInterval\([\s\S]{0,40}?,\s*[0-9_]+\s*\)/.test(hook),
      "a setInterval in useSessions has a bare number again — the cadence lives in SESSIONS_SWEEP_MS",
    ).toBe(false);
    // and the comment above the effect lists the sweep among the triggers
    const list = hook.slice(hook.indexOf("round-163"), hook.indexOf("round-163") + 900);
    expect(list, "the refresh contract no longer names the background sweep").toContain("SESSIONS_SWEEP_MS");
  });

  it("is read from the MODEL, never recomputed in a component", () => {
    const offenders: string[] = [];
    let scanned = 0;

    for (const file of readdirSync(COMPONENTS).filter((f) => f.endsWith(".tsx"))) {
      if (ALLOWED.has(file)) continue;
      scanned++;
      const src = readFileSync(path.join(COMPONENTS, file), "utf8")
        .replace(/\/\*[\s\S]*?\*\//g, (comment) => comment.replace(/[^\n]/g, ""))
        .replace(/^\s*\/\/.*$/gm, "");
      for (const [field, instead] of MODELLED) {
        // `s.pendingApproval` / `session.idleMs` / a bare destructure — anything that READS it in code
        const m = new RegExp(`\\b${field}\\b`).exec(src);
        if (m) {
          const line = src.slice(0, m.index).split("\n").length;
          offenders.push(`${file}:${line} reads \`${field}\` — use ${instead}`);
        }
      }
    }

    expect(
      offenders,
      `\n${offenders.length} component(s) compute a state the model already owns. A second derivation is how two\n` +
        `surfaces come to disagree about one session:\n  ` +
        offenders.join("\n  ") +
        `\n\nMove it into lib/liveness.ts, or add the file to ALLOWED with the reason it only forwards a value.`,
    ).toEqual([]);

    // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN.
    expect(scanned, `only ${scanned} component(s) scanned — this test is reading the wrong directory`).toBeGreaterThan(10);
  });

  it("never forms a second opinion about how a command ended", () => {
    const offenders: string[] = [];
    let scanned = 0;

    for (const file of readdirSync(COMPONENTS).filter((f) => f.endsWith(".tsx"))) {
      if (ALLOWED.has(file)) continue;
      scanned++;
      const src = readFileSync(path.join(COMPONENTS, file), "utf8")
        .replace(/\/\*[\s\S]*?\*\//g, (comment) => comment.replace(/[^\n]/g, ""))
        .replace(/^\s*\/\/.*$/gm, "");
      for (const [field, instead] of COMMAND_FACTS) {
        // `row.exitCode === 0` / `s.lastExitCode !== 0` / `ev.reason === "backgrounded"` — the shape of a component
        // deciding for itself what a recorded outcome MEANS. Printing the value, and testing it for absence, stay
        // legal on purpose (see the comment above COMMAND_FACTS).
        const m = comparesFact(field).exec(src);
        if (m) {
          const line = src.slice(0, m.index).split("\n").length;
          offenders.push(`${file}:${line} compares \`${field}\` — use ${instead}`);
        }
      }
    }

    expect(
      offenders,
      `\n${offenders.length} component(s) decide for themselves how a command ended. That mapping has ONE owner\n` +
        `(stateFromEnd in lib/path.ts), and a second copy is how one backgrounded command came to wear \`bg\` in its\n` +
        `round marker and \`warn\` in its own event row:\n  ` +
        offenders.join("\n  ") +
        `\n\nCall the derivation, or add the file to ALLOWED with the reason it only forwards a value.`,
    ).toEqual([]);

    // A PATTERN THAT STOPPED MATCHING IS NOT A CLEAN SCAN EITHER — the worry above, one level down: this rule fires
    // on a SHAPE, so both halves are pinned here rather than assumed. The first probe is the defect this check was
    // added for; the second is the rendering that must stay legal, because forbidding it is what a blanket ban on
    // the field would have done.
    const probe = comparesFact("exitCode");
    expect(probe.test("row.exitCode === 0 ? \"zero\" : \"nonzero\""), "the pattern no longer matches the shape it forbids").toBe(true);
    expect(probe.test("card.exitCode !== null ? card.exitCode : \"—\""), "the pattern now forbids an absent code drawn as absent").toBe(false);

    // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN.
    expect(scanned, `only ${scanned} component(s) scanned — this test is reading the wrong directory`).toBeGreaterThan(10);
  });
});
