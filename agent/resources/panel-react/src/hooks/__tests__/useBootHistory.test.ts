// The restart history's wire → panel mapping.
//
// WHAT IS BEING PINNED. `/api/boots` is the device's own record of its starts, and the panel
// draws both a COUNT (the summary line) and the RECORDS. Two failure modes matter and neither
// throws:
//   * a record with no usable stamp cannot be placed on a time axis — it must be DROPPED, not
//     sorted by guesswork;
//   * an unrecognised `kind` from a newer agent must render as "unrecorded", never borrow
//     another kind's words (a panel that says "crashed" about a verdict it cannot read is
//     worse than one that says nothing).
import { describe, expect, it } from "vitest";
import { EMPTY_BOOT_HISTORY, parseBootHistory } from "../useBootHistory";

const rec = (over: Record<string, unknown> = {}) => ({
  ts_ms: 1_789_370_000_000,
  kind: "crashed",
  detail: "run journal: previous run DID NOT EXIT CLEANLY — CRASHED or was killed",
  uptime_secs: 121,
  gap_secs: 125,
  release: "1.2.368",
  ...over,
});

describe("parseBootHistory", () => {
  it("reads records and the summary off a real response", () => {
    const h = parseBootHistory({
      ok: true,
      boots: [rec(), rec({ ts_ms: 1_789_360_000_000, kind: "replaced", release: null })],
      summary: { window_secs: 86_400, boots: 2, crashes: 1 },
    });
    expect(h.boots).toHaveLength(2);
    expect(h.boots[0]).toEqual({
      tsMs: 1_789_370_000_000,
      kind: "crashed",
      detail: "run journal: previous run DID NOT EXIT CLEANLY — CRASHED or was killed",
      uptimeSecs: 121,
      gapSecs: 125,
      release: "1.2.368",
    });
    expect(h.boots[1].release).toBeNull();
    expect(h.summary).toEqual({ windowSecs: 86_400, boots: 2, crashes: 1 });
  });

  it("drops a record it cannot place in time", () => {
    const h = parseBootHistory({
      boots: [rec(), { kind: "crashed", detail: "no stamp" }, { ts_ms: "soon" }],
    });
    expect(h.boots).toHaveLength(1);
    // And the summary falls back to what the panel can actually see rather than to zero.
    expect(h.summary.boots).toBe(1);
  });

  it("does not invent a kind it does not know", () => {
    const h = parseBootHistory({ boots: [rec({ kind: "melted" })] });
    expect(h.boots[0].kind).toBeNull();
    expect(h.boots[0].detail).toContain("CRASHED");
  });

  it("answers a body it cannot use with an empty history, not a throw", () => {
    expect(parseBootHistory(null)).toEqual(EMPTY_BOOT_HISTORY);
    expect(parseBootHistory({ ok: false })).toEqual(EMPTY_BOOT_HISTORY);
    expect(parseBootHistory({ boots: "nope" }).boots).toEqual([]);
  });

  it("keeps the summary's window rather than assuming a day", () => {
    // The window is the DEVICE's rule; if it ever changes, the card's sentence follows it.
    const h = parseBootHistory({ boots: [], summary: { window_secs: 3600, boots: 0, crashes: 0 } });
    expect(h.summary.windowSecs).toBe(3600);
  });
});
