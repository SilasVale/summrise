import { describe, it, expect } from "vitest";
import { pruneLagMarkers } from "./lagMarkers";

// THE MARKER MAP USED TO GROW WITH SESSIONS THAT COULD NOT BE CLEARED (found 2026-09-24 by the panel
// exploration). `useSSE` sets a marker for EVERY registered session on reconnect and clears it when
// that session's frames arrive, so a session that died in between kept its marker for the life of the
// page. The keep set is the subtle half and is what these pin.
describe("pruneLagMarkers", () => {
  it("drops markers for sessions that can no longer frame", () => {
    const m = new Map([
      ["live", 10],
      ["registered-tombstone", 20],
      ["dead", 30],
    ]);
    // `registered-tombstone` is in the keep set even though it is not live: a closed tombstone the pane
    // still holds is REVIVABLE (round-245), and a revived session must find its marker intact.
    expect(pruneLagMarkers(m, ["live", "registered-tombstone"])).toBe(1);
    expect([...m.keys()].sort()).toEqual(["live", "registered-tombstone"]);
  });

  it("keeps everything when every marker is reachable, and reports nothing dropped", () => {
    const m = new Map([
      ["a", 1],
      ["b", 2],
    ]);
    expect(pruneLagMarkers(m, ["a", "b"])).toBe(0);
    expect(m.size).toBe(2);
  });

  it("empties the map when nothing is reachable — the page after a device restart", () => {
    const m = new Map([
      ["a", 1],
      ["b", 2],
    ]);
    expect(pruneLagMarkers(m, [])).toBe(2);
    expect(m.size).toBe(0);
  });
});
