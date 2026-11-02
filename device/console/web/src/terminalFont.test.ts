import { describe, expect, it } from "vitest";
import {
  clampFontSize,
  DEFAULT_FONT_SIZE,
  FONT_SIZE_KEY,
  loadFontSize,
  MAX_FONT_SIZE,
  MIN_FONT_SIZE,
  saveFontSize
} from "./terminalFont";

/** A storage that answers one value for the font key and nothing else. */
function reading(value: string | null): Pick<Storage, "getItem"> {
  return { getItem: (key: string) => (key === FONT_SIZE_KEY ? value : null) };
}

describe("clampFontSize", () => {
  it("keeps a size already inside the range", () => {
    expect(clampFontSize(13)).toBe(13);
    expect(clampFontSize(MIN_FONT_SIZE)).toBe(10);
    expect(clampFontSize(MAX_FONT_SIZE)).toBe(20);
  });

  it("clamps below the minimum and above the maximum", () => {
    expect(clampFontSize(9)).toBe(10);
    expect(clampFontSize(21)).toBe(20);
    expect(clampFontSize(0)).toBe(10);
    expect(clampFontSize(99)).toBe(20);
  });

  it("rounds a fractional size", () => {
    expect(clampFontSize(13.6)).toBe(14);
    expect(clampFontSize(13.4)).toBe(13);
  });

  it("falls back to the default for NaN and clamps an infinity", () => {
    expect(clampFontSize(Number.NaN)).toBe(DEFAULT_FONT_SIZE);
    expect(clampFontSize(Number.POSITIVE_INFINITY)).toBe(MAX_FONT_SIZE);
    expect(clampFontSize(Number.NEGATIVE_INFINITY)).toBe(MIN_FONT_SIZE);
  });
});

describe("loadFontSize", () => {
  it("answers the default when there is no storage at all", () => {
    expect(loadFontSize(null)).toBe(DEFAULT_FONT_SIZE);
  });

  it("reads a stored size", () => {
    expect(loadFontSize(reading("16"))).toBe(16);
  });

  it("answers the default for a value that is not a number", () => {
    expect(loadFontSize(reading("abc"))).toBe(DEFAULT_FONT_SIZE);
    expect(loadFontSize(reading(""))).toBe(DEFAULT_FONT_SIZE);
  });

  it("clamps a stored value the range no longer allows", () => {
    expect(loadFontSize(reading("99"))).toBe(MAX_FONT_SIZE);
    expect(loadFontSize(reading("2"))).toBe(MIN_FONT_SIZE);
  });

  it("keeps the default when reading the storage throws", () => {
    const refusing: Pick<Storage, "getItem"> = {
      getItem: () => {
        throw new Error("SecurityError: the storage is not available");
      }
    };
    expect(loadFontSize(refusing)).toBe(DEFAULT_FONT_SIZE);
  });
});

describe("saveFontSize", () => {
  it("writes the clamped size under the one key", () => {
    const writes: [string, string][] = [];
    const storage: Pick<Storage, "setItem"> = {
      setItem: (key, value) => {
        writes.push([key, value]);
      }
    };

    saveFontSize(storage, 14.6);

    expect(writes).toEqual([[FONT_SIZE_KEY, "15"]]);
  });

  it("does not throw when there is no storage", () => {
    expect(() => saveFontSize(null, 14)).not.toThrow();
  });

  it("does not throw when the storage refuses the write", () => {
    const refusing: Pick<Storage, "setItem"> = {
      setItem: () => {
        throw new Error("QuotaExceededError: private mode");
      }
    };
    expect(() => saveFontSize(refusing, 14)).not.toThrow();
  });
});
