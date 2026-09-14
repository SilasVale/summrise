// `lib/spark.ts` — the geometry and the one rule that may interrupt an operator.
//
// WHAT IS BEING PINNED, and why each case is a real failure mode rather than a shape:
//   * a GAP in the series must BREAK the line — a chart that connects across a missing
//     reading asserts values nobody measured (the same lie as printing "0%" for "unknown");
//   * an EMPTY series must produce NO path at all, so the caller renders an empty state
//     rather than a flat line that reads as a real, quiet device;
//   * a FLAT series must draw through the middle, not along the ceiling or the floor;
//   * the load rule must be SILENT until it has evidence, and silent for a spike — a chip
//     that fires on a burst is one an operator learns to ignore.
import { describe, expect, it } from "vitest";
import {
  LOAD_MIN_SAMPLES,
  LOAD_WINDOW_MS,
  loadNotice,
  seriesStats,
  sparkSegments,
} from "./spark";

const W = 100;
const H = 10;

describe("sparkSegments", () => {
  it("draws nothing at all for a series with no readings", () => {
    expect(sparkSegments([], W, H)).toEqual([]);
    expect(sparkSegments([null, null, null], W, H)).toEqual([]);
  });

  it("scales the series to the box, oldest first", () => {
    // Two readings: the lower one sits on the floor, the higher on the ceiling.
    const [d] = sparkSegments([0, 100], W, H);
    expect(d).toBe("M0.00,10.00 L100.00,0.00");
  });

  it("draws a flat series through the middle, not along an edge", () => {
    // min === max. Pinning it to the top would read as "at the limit"; the floor would read
    // as "zero". The middle is the only honest place for a constant.
    const [d] = sparkSegments([42, 42, 42], W, H);
    expect(d).toBe("M0.00,5.00 L50.00,5.00 L100.00,5.00");
  });

  it("BREAKS the line where a reading is missing", () => {
    const segs = sparkSegments([10, 20, null, 40, 50], W, H);
    expect(segs).toHaveLength(2);
    // Each run keeps its own x positions — the gap consumes its slot rather than being
    // closed up, so the time axis stays true.
    expect(segs[0]).toContain("M0.00");
    expect(segs[1].startsWith("M75.00")).toBe(true);
  });

  it("shows a lone reading rather than dropping it", () => {
    const segs = sparkSegments([null, 30, null], W, H);
    expect(segs).toEqual(["M50.00,5.00 L50.00,5.00"]);
  });
});

describe("seriesStats", () => {
  it("summarises only the known readings, and says so when there are none", () => {
    expect(seriesStats([10, null, 20, 30])).toEqual({ min: 10, avg: 20, max: 30, n: 3 });
    expect(seriesStats([null, null])).toBeNull();
    expect(seriesStats([])).toBeNull();
  });
});

describe("loadNotice", () => {
  const now = 1_789_000_000_000;
  /** A window of readings ending at `now`, one every 30 s. */
  const series = (cpu: (number | null)[], mem: (number | null)[] = []) =>
    cpu.map((c, i) => ({
      tsMs: now - (cpu.length - 1 - i) * 30_000,
      cpu: c,
      mem: mem[i] ?? null,
    }));

  it("says NOTHING without enough evidence", () => {
    // Nine readings of a pinned CPU: real, but not yet a condition this panel will claim.
    expect(loadNotice(series(Array(LOAD_MIN_SAMPLES - 1).fill(99)), now)).toBeNull();
    expect(loadNotice([], now)).toBeNull();
  });

  it("fires on a sustained load and names the metric and the span", () => {
    const n = loadNotice(series(Array(12).fill(95)), now);
    expect(n?.metric).toBe("cpu");
    expect(n?.tone).toBe("crit");
    // 12 readings 30 s apart span 5.5 minutes → 6 minutes, stated in the text.
    expect(n?.text).toBe("CPU pegged 6m");
    expect(n?.title).toContain("12 of the last 12 readings");
  });

  it("stays quiet for a SPIKE, however high", () => {
    // Three pinned readings inside a quiet window: the dial should show it, this chip
    // should not — a burst is not a condition.
    const cpu = [...Array(20).fill(20), 99, 99, 99];
    expect(loadNotice(series(cpu), now)).toBeNull();
  });

  it("speaks about memory when the CPU is fine", () => {
    const n = loadNotice(series(Array(14).fill(12), Array(14).fill(93)), now);
    expect(n?.metric).toBe("mem");
    expect(n?.text).toContain("memory pegged");
  });

  it("prefers the CPU when both are pegged", () => {
    const n = loadNotice(series(Array(14).fill(93), Array(14).fill(97)), now);
    expect(n?.metric).toBe("cpu");
  });

  it("ignores readings older than the window", () => {
    // Twelve pinned readings, then a long silence: the device is presumably fine now, and
    // the chip must not still be shouting about yesterday.
    const stale = Array.from({ length: 12 }, (_, i) => ({
      tsMs: now - LOAD_WINDOW_MS - (12 - i) * 30_000,
      cpu: 99,
      mem: null,
    }));
    expect(loadNotice(stale, now)).toBeNull();
  });

  it("does not judge a series whose readings are all missing", () => {
    expect(loadNotice(series(Array(14).fill(null)), now)).toBeNull();
  });
});
