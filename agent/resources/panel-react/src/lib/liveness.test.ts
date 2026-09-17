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
  type Liveness,
} from "./liveness";

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
    const device = { connected: true, working: true };
    expect(sessionLiveness({ pendingApproval: null, closed: true }, device)).toBe("off");
    expect(sessionLiveness({ pendingApproval: null }, { connected: false, working: true })).toBe("off");
    expect(sessionLiveness({ pendingApproval: { command: "x" } }, device)).toBe("waiting");
    expect(sessionLiveness({ pendingApproval: null }, device)).toBe("working");
  });
});
