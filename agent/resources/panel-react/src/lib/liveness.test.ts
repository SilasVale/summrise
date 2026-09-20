// liveness.test.ts — the state model is a CONTRACT, so it is pinned rather than described.
//
// Three claims are load-bearing and each is checked here:
//   1. PRECEDENCE — a question outranks activity, activity outranks quiet, and an unreachable
//      transport outranks all of them. Every surface calls the same function, so this is the only
//      place the order can be wrong.
//   2. DISTINCT SHAPES — "the silhouette carries the state" is worthless if two states draw the
//      same shape. This fails the moment somebody adds a state and forgets a shape.
//   3. NO LYING ABOUT AN UNREACHABLE SESSION — a closed one has no liveness to report, and a mark
//      that says "idle" about a session that cannot answer is worse than no mark.
import { describe, it, expect } from "vitest";
import {
  URGENCY,
  SILHOUETTE,
  MOVES,
  livenessOf,
  deviceLiveness,
  sessionLiveness,
  sessionActive,
  anyCommandRunning,
  sessionWaiting,
  sessionFailed,
  type Liveness,
} from "./liveness";
import { WORKING_MS } from "../hooks/useDeviceActivity";

const STATES = Object.keys(URGENCY) as Liveness[];

describe("the liveness model", () => {
  it("names exactly the states the device can report", () => {
    // FIVE, AND THE FIFTH WAITED FOR THE WIRE (round 97). This list was four for as long as the comment above it
    // said why: "'Failed' is not here because no field reports it per session." `last_exit_code` is that field
    // (round 96), so the state exists now — and this assertion is the one that failed the moment it was added,
    // which is what it is for.
    expect(STATES.sort()).toEqual(["failed", "idle", "off", "waiting", "working"]);
  });

  it("gives FAILED the shape nothing else claims", () => {
    // The panel spends the diamond on WAITING and the console spends it on FAILURE; a fifth state that reused it
    // would put two meanings on one silhouette in the same tab strip. Pinned by NAME so a later "simplification"
    // cannot quietly fold the triangle back into a shape that is already taken.
    expect(SILHOUETTE.failed).toBe("triangle");
    expect(SILHOUETTE.failed).not.toBe(SILHOUETTE.waiting);
    expect(SILHOUETTE.failed).not.toBe(SILHOUETTE.working);
  });

  it("gives every state its own silhouette", () => {
    const shapes = STATES.map((s) => SILHOUETTE[s]);
    expect(new Set(shapes).size, `two states share a shape: ${shapes.join(", ")}`).toBe(STATES.length);
  });

  it("keeps motion as an addition, never the message", () => {
    const moving = STATES.filter((s) => MOVES[s]);
    expect(moving).toEqual(["working"]);
  });

  it("orders urgency: a question beats activity, activity beats quiet, unreachable beats all", () => {
    expect(URGENCY.waiting).toBeGreaterThan(URGENCY.working);
    expect(URGENCY.working).toBeGreaterThan(URGENCY.failed);
    // A FAILURE OUTRANKS QUIET AND LOSES TO ANYTHING HAPPENING NOW (round 97). It describes what already
    // happened, so a session that is working or holding a question is not described by its last exit code.
    expect(URGENCY.failed).toBeGreaterThan(URGENCY.idle);
    expect(URGENCY.idle).toBeGreaterThan(URGENCY.off);
  });

  it("resolves a FAILED session, and never lets it outrank work or a question", () => {
    const failed = { lastExitCode: 1 };
    expect(sessionLiveness(failed)).toBe("failed");
    expect(sessionLiveness({ lastExitCode: 1, commandRunning: true })).toBe("working");
    expect(sessionLiveness({ lastExitCode: 1, pendingApproval: { id: "ap" } })).toBe("waiting");
    expect(sessionLiveness({ lastExitCode: 1, closed: true })).toBe("off");
    // ABSENT IS NOT FAILURE, and it is not success either — it is the third state the device reports by omission
    // (no command yet, no shell marker, an ssh/serial session). A mark that turned it into either answer would be
    // inventing one, so both spellings of "nothing to say" land on idle.
    expect(sessionLiveness({ lastExitCode: null })).toBe("idle");
    expect(sessionLiveness({})).toBe("idle");
    // ZERO IS AN ANSWER, AND IT IS NOT A FAILURE.
    expect(sessionLiveness({ lastExitCode: 0 })).toBe("idle");
    expect(sessionFailed({ lastExitCode: 0 })).toBe(false);
    expect(sessionFailed({ lastExitCode: 130 })).toBe(true);
    expect(sessionFailed({ lastExitCode: null })).toBe(false);
    expect(sessionFailed({})).toBe(false);
  });

  it("resolves the precedence in one place", () => {
    // the whole table, so a change to the order is visible rather than inferred
    expect(livenessOf({ reachable: false, pending: true, active: true })).toBe("off");
    expect(livenessOf({ reachable: true, pending: true, active: true })).toBe("waiting");
    expect(livenessOf({ reachable: true, pending: false, active: true })).toBe("working");
    expect(livenessOf({ reachable: true, pending: false, active: false })).toBe("idle");
  });

  it("reads the device's three inputs without inventing a fourth", () => {
    expect(deviceLiveness({ connected: false, pendingCount: 2, working: true })).toBe("off");
    expect(deviceLiveness({ connected: true, pendingCount: 1, working: true })).toBe("waiting");
    expect(deviceLiveness({ connected: true, pendingCount: 0, working: true })).toBe("working");
    expect(deviceLiveness({ connected: true, pendingCount: 0, working: false })).toBe("idle");
  });

  it("does not claim liveness for a session that cannot answer", () => {
    expect(sessionLiveness({ pendingApproval: null, closed: true, idleMs: 10 })).toBe("off");
    expect(sessionLiveness({ pendingApproval: { command: "x" }, idleMs: 60_000 })).toBe("waiting");
    expect(sessionLiveness({ pendingApproval: null, idleMs: 60_000 })).toBe("idle");
  });

  // ── THE DEVICE'S OWN FACT, PER SESSION (round 9 of the standing goal) ─────────────────────────────────────
  it("reads the session's OWN idle time, not the device's mood", () => {
    // `idle_ms` is the agent's `last_output.elapsed()`: the one per-session fact on the wire, and it sat unused
    // while every surface passed `active: false` and explained that only a DEVICE-wide signal existed.
    expect(sessionActive({ idleMs: 0 })).toBe(true);
    expect(sessionActive({ idleMs: WORKING_MS - 1 })).toBe(true);
    expect(sessionActive({ idleMs: WORKING_MS })).toBe(false);
    expect(sessionActive({ idleMs: 60_000 })).toBe(false);
    // A session row without the field (an older device, a stripped fixture) must NOT read as working.
    expect(sessionActive({})).toBe(false);
    expect(sessionActive({ idleMs: undefined })).toBe(false);
  });

  // ── THE DEVICE'S OWN ANSWER, WHICH OUTPUT RECENCY CANNOT GIVE (round 28 of the standing goal) ──────────────
  it("calls a session WORKING while the device says a command is in flight, however quiet it is", () => {
    // THE CASE THIS EXISTS FOR: a flash, a long probe, a serial command whose only output is the last line. The
    // device's execute wait-loop knows the whole time; `idleMs` sees a minute of silence and calls it idle.
    const quiet = 90_000; // three times the recency window
    expect(sessionActive({ idleMs: quiet })).toBe(false);
    expect(sessionActive({ idleMs: quiet, commandRunning: true })).toBe(true);
    // and it does not overrule the other direction: nothing running leaves recency in charge
    expect(sessionActive({ idleMs: 400, commandRunning: false })).toBe(true);
    expect(sessionLiveness({ pendingApproval: null, idleMs: quiet, commandRunning: true })).toBe("working");
    // approval still outranks work in flight — the operator is what everything else waits for
    expect(sessionLiveness({ pendingApproval: { id: "ap-1" }, idleMs: 0, commandRunning: true })).toBe("waiting");
  });

  it("finds a command in flight anywhere in the list, and survives not having one", () => {
    // The device-scope derivation the rail uses. It has to be safe on an EMPTY or ABSENT list — the rail is
    // rendered by an embedding that may have no sessions at all, which is why `pendingCount` is optional too.
    expect(anyCommandRunning(undefined)).toBe(false);
    expect(anyCommandRunning([])).toBe(false);
    expect(anyCommandRunning([{ commandRunning: false }, { commandRunning: false }])).toBe(false);
    expect(anyCommandRunning([{ commandRunning: false }, { commandRunning: true }])).toBe(true);
  });

  it("a CLOSED session is not waiting, however it is asked", () => {
    // THE TWO DENSITIES DISAGREED (round 33). A tombstone keeps its row's data, so a question that expired with the
    // session survived in `pendingApproval`: the mark said `off` (unreachable outranks pending) while the DESKTOP
    // tab's title said "waiting for your approval" about a tab nothing can be answered on. `sessionWaiting` is the
    // one predicate all three surfaces read now, and this is the case that made it one.
    const closedWithQuestion = { pendingApproval: { id: "ap-1" }, closed: true, idleMs: 0 };
    expect(sessionWaiting(closedWithQuestion)).toBe(false);
    expect(sessionLiveness(closedWithQuestion)).toBe("off");
    // a LIVE one with a question is waiting, and that outranks being busy
    expect(sessionWaiting({ pendingApproval: { id: "ap-1" }, closed: false, commandRunning: true })).toBe(true);
    expect(sessionLiveness({ pendingApproval: { id: "ap-1" }, closed: false, commandRunning: true })).toBe("waiting");
    // and no question is no wait
    expect(sessionWaiting({ pendingApproval: null, closed: false })).toBe(false);
  });

  it("does not smear one busy session over the others", () => {
    // THE BUG THIS REPLACES: `sessionLiveness` took the DEVICE's `working` flag, so a command in one tab would
    // draw a halo on all sixteen — which is why the surfaces stopped calling it and hard-coded `active: false`
    // instead. Two sessions, one working: exactly one halo.
    const quiet = sessionLiveness({ pendingApproval: null, idleMs: 90_000 });
    const busy = sessionLiveness({ pendingApproval: null, idleMs: 500 });
    expect(busy).toBe("working");
    expect(quiet).toBe("idle");
    expect(busy).not.toBe(quiet);
  });
;
});
