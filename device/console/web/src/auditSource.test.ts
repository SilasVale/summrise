import { describe, expect, it } from "vitest";
import { auditSourceSentence, auditWindowSpan } from "./auditSource";

// The audit source line is the companion to the cap sentence, not a second cap sentence: it says
// which file the window is read from and how far back that file goes, and — since audit-44 F10 — the
// span this page's window covers, stated apart from the file's lifetime. Every figure comes from the
// server or the page's own lines; a fact either could not give is absent, never guessed.

const source = {
  file: "audit.jsonl",
  bytes: 8_808_038,
  oldest_ts_ms: new Date(2026, 10, 2, 9, 12).getTime(),
  newest_ts_ms: new Date(2026, 10, 5, 17, 0).getTime(),
  max_bytes: 8_388_608,
  rotations: 1,
  rotated: {
    file: "audit.1.jsonl",
    oldest_ts_ms: new Date(2026, 10, 2, 3, 10).getTime()
  }
};

const window = {
  oldest_ts_ms: new Date(2026, 10, 5, 16, 30).getTime(),
  newest_ts_ms: new Date(2026, 10, 5, 17, 0).getTime()
};

describe("auditSourceSentence", () => {
  it("states the file, its size, where its lines start and the older generation", () => {
    expect(auditSourceSentence(source)).toBe(
      "Reading audit.jsonl — 8.4 MiB, lines since 11-02 09:12; rotated once, older lines start 11-02 03:10."
    );
  });

  it("states the window's span as its own fact, after the file's lifetime", () => {
    // The measured defect: `5.7 MiB, lines since 10-22` beside `the newest 1000 lines` made the
    // file's 17-day lifetime read as this page's range. The two spans are now named as two.
    expect(auditSourceSentence(source, window)).toBe(
      "Reading audit.jsonl — 8.4 MiB, lines since 11-02 09:12; rotated once, older lines start 11-02 03:10. " +
        "This window: 11-05 16:30 → 11-05 17:00."
    );
  });

  it("says one moment, not an arrow to itself, for a one-line window", () => {
    const one = { oldest_ts_ms: window.oldest_ts_ms, newest_ts_ms: window.oldest_ts_ms };
    expect(auditSourceSentence(source, one)).toContain("This window: 11-05 16:30.");
    expect(auditSourceSentence(source, one)).not.toContain("→");
  });

  it("keeps the seconds when both ends print as the same minute", () => {
    // Two rows two seconds apart: at minute scale both ends read `11-05 16:30`, which would draw a
    // span between one moment and itself rather than the window that is there.
    const tight = {
      oldest_ts_ms: new Date(2026, 10, 5, 16, 30, 5).getTime(),
      newest_ts_ms: new Date(2026, 10, 5, 16, 30, 7).getTime()
    };
    expect(auditSourceSentence(source, tight)).toContain(
      "This window: 11-05 16:30:05 → 11-05 16:30:07."
    );
  });

  it("omits the rotation clause when no predecessor exists", () => {
    expect(auditSourceSentence({ ...source, rotated: undefined })).toBe(
      "Reading audit.jsonl — 8.4 MiB, lines since 11-02 09:12."
    );
  });

  it("says only that it rotated when the older file's oldest line cannot be read", () => {
    expect(auditSourceSentence({ ...source, rotated: { file: "audit.1.jsonl" } })).toBe(
      "Reading audit.jsonl — 8.4 MiB, lines since 11-02 09:12; rotated once."
    );
  });

  it("drops a moment the server could not read instead of inventing one", () => {
    expect(auditSourceSentence({ file: "audit.jsonl", bytes: 0 })).toBe(
      "Reading audit.jsonl — 0 B."
    );
  });

  it("draws no window clause when there is no window to state", () => {
    // An empty page, or a chosen range: the file's facts are still true, but "this window" would
    // point at lines the page is not showing.
    const empty = { file: "audit.jsonl", bytes: 0 };
    expect(auditSourceSentence(empty, null)).toBe("Reading audit.jsonl — 0 B.");
    expect(auditSourceSentence(empty, undefined)).toBe("Reading audit.jsonl — 0 B.");
    expect(
      auditSourceSentence(empty, { oldest_ts_ms: Number.NaN, newest_ts_ms: 1 })
    ).toBe("Reading audit.jsonl — 0 B.");
    expect(
      auditSourceSentence(empty, { oldest_ts_ms: 2, newest_ts_ms: 1 })
    ).toBe("Reading audit.jsonl — 0 B.");
  });

  it("says nothing when there is no file to read", () => {
    expect(auditSourceSentence(null)).toBeNull();
    expect(auditSourceSentence(undefined)).toBeNull();
    expect(auditSourceSentence({})).toBeNull();
  });
});

describe("auditWindowSpan", () => {
  it("is the oldest and newest moment the lines carry, in either order", () => {
    expect(auditWindowSpan([{ ts_ms: 30 }, { ts_ms: 10 }, { ts_ms: 20 }])).toEqual({
      oldest_ts_ms: 10,
      newest_ts_ms: 30
    });
  });

  it("ignores a line with no finite moment and is null when none has one", () => {
    expect(
      auditWindowSpan([{ ts_ms: 30 }, { ts_ms: Number.NaN }, { ts_ms: 10 }])
    ).toEqual({ oldest_ts_ms: 10, newest_ts_ms: 30 });
    expect(auditWindowSpan([])).toBeNull();
    expect(auditWindowSpan([{ ts_ms: Number.NaN }])).toBeNull();
  });
});
