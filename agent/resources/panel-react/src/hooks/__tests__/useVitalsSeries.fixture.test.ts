// The vitals series' wire format, pinned from the panel's end against the device's own fixture.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseVitalsSeries } from "../useVitalsSeries";

const HERE = path.dirname(fileURLToPath(import.meta.url));

// ── THE VITALS SERIES, from the fixture the device is checked against ────────────────────────────
//
// `agent/tests/fixtures/vitals-series.json` is the same file `agent/src/metrics.rs` is checked
// against, so a rename on EITHER side fails on one of the two. Two promises travel in it and both are
// load-bearing here rather than decorative:
//
//   * ORDER — the samples go straight into the chart, and `samples[samples.length - 1]` is read as the
//     NEWEST reading's total memory. A reverse at the device would label the oldest reading as current.
//   * KEYS — the parser drops a sample with no `ts_ms`, and a renamed `cpu_pct`/`mem_pct` would flatten
//     the chart to empty rather than erroring, because a missing reading is also a legitimate state.
describe("parseVitalsSeries — the shared fixture", () => {
  const fixture = JSON.parse(
    readFileSync(path.resolve(HERE, "..", "..", "..", "..", "..", "tests", "fixtures", "vitals-series.json"), "utf8"),
  );

  it("keeps the device's order, because the last sample is read as the newest", () => {
    const series = parseVitalsSeries(fixture.example);
    expect(series.samples.length).toBe(3);
    const stamps = series.samples.map((s) => s.tsMs);
    expect(stamps).toEqual([...stamps].sort((a, b) => a - b));
    // What DeviceHealthCard does with it: the LAST entry's total memory is presented as the current one.
    expect(series.samples[series.samples.length - 1].memTotalMb).toBe(16384);
    for (const s of series.samples) expect(s.tsMs).toBeGreaterThan(1_600_000_000_000);
  });

  it("reads the envelope, and keeps a missing reading missing", () => {
    const series = parseVitalsSeries(fixture.example);
    expect(series.intervalSecs).toBe(30);
    expect(series.spanSecs).toBe(3600);
    // A sample the device could not read carries nulls, not zeros: "0% CPU" and "no reading" are
    // different claims, and the chart draws them differently.
    const { samples } = parseVitalsSeries({
      samples: [{ ts_ms: 1789000000000, cpu_pct: null, mem_pct: 12.5, mem_total_mb: null }],
    });
    expect(samples[0].cpu).toBeNull();
    expect(samples[0].memTotalMb).toBeNull();
    expect(samples[0].mem).toBe(12.5);
  });

  it("reads no key outside the fixture, and drops what it cannot place in time", () => {
    const carried = new Set(fixture.keys);
    const source = readFileSync(path.resolve(HERE, "..", "useVitalsSeries.ts"), "utf8");
    const reads = new Set([...source.matchAll(/\br\.([a-z_]+)/g)].map((m) => m[1]));
    const unknown = [...reads].filter((k) => !carried.has(k));
    expect(unknown, "the parser reads a key the fixture does not declare").toEqual([]);
    // And a sample with no usable stamp is dropped rather than placed by guesswork — the same rule the
    // boot history and the session archive follow.
    const { samples } = parseVitalsSeries({
      samples: [{ cpu_pct: 5 }, { ts_ms: 1789000000000, cpu_pct: 1.5 }],
    });
    expect(samples.length).toBe(1);
    expect(samples[0].cpu).toBe(1.5);
  });
});
