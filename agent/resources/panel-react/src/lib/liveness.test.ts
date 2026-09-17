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
  type Liveness,
} from "./liveness";
import { WORKING_MS } from "../hooks/useDeviceActivity";

const STATES = Object.keys(URGENCY) as Liveness[];

describe("the liveness model", () => {
  it("names exactly the states the device can report", () => {
    expect(STATES.sort()).toEqual(["idle", "off", "waiting", "working"]);
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
    expect(URGENCY.working).toBeGreaterThan(URGENCY.idle);
    expect(URGENCY.idle).toBeGreaterThan(URGENCY.off);
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
