import { describe, expect, it } from "vitest";
import {
  dateTimeInputValue,
  parseDateTimeInput,
  presetRange,
  rangeSentence,
  rangeText,
  RANGE_PRESETS
} from "./auditTime";

/** A local instant, so the assertions do not depend on the machine's timezone. */
function local(year: number, month: number, day: number, hour: number, minute = 0): number {
  return new Date(year, month - 1, day, hour, minute, 0, 0).getTime();
}

describe("the Activity page's time presets", () => {
  it("names the four presets in the order the control draws them", () => {
    expect(RANGE_PRESETS).toEqual([
      { value: "hour", label: "Last hour" },
      { value: "today", label: "Today" },
      { value: "yesterday", label: "Yesterday" },
      { value: "week", label: "Last week" }
    ]);
  });

  it("turns Today and Yesterday into local midnights, not rolling windows", () => {
    const now = local(2026, 11, 5, 14, 32);
    expect(presetRange("today", now)).toEqual({ from: local(2026, 11, 5, 0), to: now });
    expect(presetRange("yesterday", now)).toEqual({
      from: local(2026, 11, 4, 0),
      to: local(2026, 11, 5, 0)
    });
  });

  it("keeps Last hour and Last week rolling, as their words say", () => {
    const now = local(2026, 11, 5, 14, 32);
    expect(presetRange("hour", now)).toEqual({ from: now - 60 * 60_000, to: now });
    expect(presetRange("week", now)).toEqual({ from: now - 7 * 24 * 60 * 60_000, to: now });
  });

  it("keeps Last hour one instant long when it crosses midnight", () => {
    // At 00:30 the last hour is 23:30–00:30: it is an instant's width, not "today so far". A range
    // that started at the local midnight would silently lose the previous day's tail.
    const now = local(2026, 11, 5, 0, 30);
    const range = presetRange("hour", now);
    expect(range).toEqual({ from: local(2026, 11, 4, 23, 30), to: now });
    expect(rangeText(range)).toBe("11-04 23:30 – 11-05 00:30");
    // Today is still its own midnight, not the hour's start.
    expect(presetRange("today", now)).toEqual({ from: local(2026, 11, 5, 0), to: now });
  });

  it("leaves a midday Last hour unchanged", () => {
    const now = local(2026, 11, 5, 12, 0);
    const range = presetRange("hour", now);
    expect(range).toEqual({ from: local(2026, 11, 5, 11), to: now });
    expect(rangeText(range)).toBe("11:00–12:00 of 11-05");
  });

  it("holds Today's midnight boundary at 00:00", () => {
    const midnight = local(2026, 11, 5, 0, 0);
    expect(presetRange("today", midnight)).toEqual({ from: midnight, to: midnight });
    // A tick after midnight it is still the same local midnight, not yesterday's or the range's.
    expect(presetRange("today", midnight + 1)).toEqual({ from: midnight, to: midnight + 1 });
  });
});

describe("the range sentence", () => {
  const range = { from: local(2026, 11, 5, 9), to: local(2026, 11, 5, 17) };

  it("names the range it drew and how many lines it holds", () => {
    expect(rangeText(range)).toBe("09:00–17:00 of 11-05");
    expect(rangeSentence(range, 342)).toBe("Showing 09:00–17:00 of 11-05, 342 lines");
    expect(rangeSentence(range, 1)).toBe("Showing 09:00–17:00 of 11-05, 1 line");
  });

  it("names a range that crosses a local day by both ends", () => {
    const week = { from: local(2026, 10, 30, 9, 5), to: local(2026, 11, 5, 17) };
    expect(rangeText(week)).toBe("10-30 09:05 – 11-05 17:00");
  });

  it("states where coverage starts when the range begins before the file", () => {
    const since = local(2026, 10, 22, 10, 55);
    expect(rangeSentence(range, 342, { since_ms: since })).toBe(
      "Showing 09:00–17:00 of 11-05, 342 lines; since 10-22 10:55"
    );
  });

  it("states the scan bound that cut a range", () => {
    expect(
      rangeSentence(range, 342, { bounded: true, scan_limit: 50_000 })
    ).toBe(
      "Showing 09:00–17:00 of 11-05, 342 lines; the 50,000-line scan bound cut it"
    );
  });

  it("names the window's limit when the range held more lines than the page drew", () => {
    expect(rangeSentence(range, 1_000, { at_limit: true })).toBe(
      "Showing the newest 1,000 lines of 09:00–17:00 of 11-05 — the window's limit"
    );
  });

  it("counts the filter's matches when a filter narrows the range's rows", () => {
    // The measured 390 shape: `29 lines` above `No calls match … failed calls`. The range still
    // held 29 lines; the filter kept none of them, and the sentence says both numbers.
    expect(rangeSentence(range, 29, null, { matched: 0, drawn: 29 })).toBe(
      "Showing 09:00–17:00 of 11-05, 29 lines; 0 of the 29 lines match the filters"
    );
    expect(rangeSentence(range, 29, null, { matched: 29, drawn: 29 })).toBe(
      "Showing 09:00–17:00 of 11-05, 29 lines"
    );
  });

  it("states both bounds at once without hiding either", () => {
    const sentence = rangeSentence(range, 1_000, {
      since_ms: local(2026, 10, 22, 10, 55),
      bounded: true,
      scan_limit: 50_000,
      at_limit: true
    });
    expect(sentence).toContain("since 10-22 10:55");
    expect(sentence).toContain("50,000-line scan bound cut it");
    expect(sentence).toContain("the window's limit");
  });
});

describe("the custom from and to fields", () => {
  it("round-trips a local minute through the datetime-local value", () => {
    const ms = local(2026, 11, 5, 9, 7);
    expect(dateTimeInputValue(ms)).toBe("2026-11-05T09:07");
    expect(parseDateTimeInput("2026-11-05T09:07")).toBe(ms);
    expect(parseDateTimeInput("2026-11-05T09:07:42")).toBe(local(2026, 11, 5, 9, 7) + 42_000);
  });

  it("refuses a value that is not a moment", () => {
    expect(parseDateTimeInput("")).toBeNull();
    expect(parseDateTimeInput("yesterday")).toBeNull();
    expect(parseDateTimeInput("2026-13-05T09:07")).toBeNull();
  });
});
