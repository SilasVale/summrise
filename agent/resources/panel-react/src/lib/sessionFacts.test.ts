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
// COMMENTS ARE STRIPPED FIRST — for the third time this session (rounds 20, 23, and here): a comment that explains
// why a field is NOT read is not a read, and a check that cannot tell them apart deletes its own reasons.
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SRC = path.dirname(fileURLToPath(import.meta.url));
const COMPONENTS = path.join(SRC, "..", "components");

/** The fields a component must take from the model, and what to use instead. */
const MODELLED = new Map([
  ["pendingApproval", "sessionWaiting() — the ONE predicate the mark, the title and the label read"],
  ["idleMs", "sessionActive() — it knows about command_running and WORKING_MS"],
  ["commandRunning", "sessionActive() / anyCommandRunning()"],
  ["heldByHuman", "the session's holder is drawn by the control that owns it"],
]);

/**
 * Named files that forward a value instead of deriving one. Each needs its reason: a file-wide allowance also
 * covers anything else somebody adds to it.
 */
const ALLOWED = new Map([
  [
    "TerminalWorkspace.tsx",
    "passes `held={!!activeSession.heldByHuman}` to SessionControl, the one component that draws it — it forwards the device's value and derives nothing",
  ],
]);

describe("session state", () => {
  it("is read from the MODEL, never recomputed in a component", () => {
    const offenders: string[] = [];
    let scanned = 0;

    for (const file of readdirSync(COMPONENTS).filter((f) => f.endsWith(".tsx"))) {
      if (ALLOWED.has(file)) continue;
      scanned++;
      const src = readFileSync(path.join(COMPONENTS, file), "utf8")
        .replace(/\/\*[\s\S]*?\*\//g, "")
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
});
