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
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { EMPTY_BOOT_HISTORY, parseBootHistory } from "../useBootHistory";

const HERE = path.dirname(fileURLToPath(import.meta.url));

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

// ── the device's OWN promises, read from the shared fixture ──────────────────────────────────────
//
// The tests above use bodies written here. `agent/tests/fixtures/boot-history.json` is different: it
// is the file `agent/src/runstate.rs` is checked against from the other end, so a rename on EITHER
// side fails on one of the two. Four promises travel in it, in the device's own words, and two of
// them are load-bearing for what an operator sees:
//
//   * NEWEST FIRST. `RestartHistoryCard` renders `boots.slice(0, VISIBLE)` as "the latest N", so a
//     reverse at the device would silently show the OLDEST restarts as the newest — on a card that
//     exists precisely to make a restart LOOP visible.
//   * the first run of an install has NO uptime_secs/gap_secs — absent, never zero, because "ran 0s"
//     is a claim about a run that never existed.
describe("parseBootHistory — the shared fixture", () => {
  const fixture = JSON.parse(
    readFileSync(path.resolve(HERE, "..", "..", "..", "..", "..", "tests", "fixtures", "boot-history.json"), "utf8"),
  );

  it("keeps the device's order, because the card renders the head as the latest", () => {
    const { boots } = parseBootHistory(fixture);
    expect(boots.length).toBe(3);
    const stamps = boots.map((b) => b.tsMs);
    expect(stamps).toEqual([...stamps].sort((a, b) => b - a));
    expect(boots[0].kind).toBe("crashed");
    expect(boots[boots.length - 1].kind).toBe("first-run");
    for (const b of boots) expect(b.tsMs).toBeGreaterThan(1_600_000_000_000);
  });

  it("keeps an absent measurement absent, and a real zero a zero", () => {
    const { boots } = parseBootHistory(fixture);
    const first = boots[boots.length - 1];
    expect(first.uptimeSecs, "no previous run to measure").toBeNull();
    expect(first.gapSecs, "and no gap before it").toBeNull();
    expect(boots[1].uptimeSecs, "the next boot DID measure a run that lasted zero seconds").toBe(0);
  });

  it("reads an optional release as absent when the device did not name one", () => {
    const { boots, summary } = parseBootHistory(fixture);
    expect(boots[0].release).toBe("1.2.403");
    expect(boots[2].release).toBeNull();
    expect(summary).toEqual({ windowSecs: 86_400, boots: 3, crashes: 1 });
  });
});
