import { describe, expect, it } from "vitest";
import { age, compactAge } from "./age";

describe("age", () => {
  it("says seconds under a minute", () => {
    expect(age(0)).toBe("0s");
    expect(age(42_000)).toBe("42s");
  });

  it("says minutes and seconds under an hour", () => {
    expect(age(185_000)).toBe("3m 05s");
  });

  it("says hours and minutes under a day", () => {
    expect(age(7_800_000)).toBe("2h 10m");
  });

  it("says days and hours past a day", () => {
    expect(age(273_600_000)).toBe("3d 4h");
  });

  it("never says a negative age; a clock that moved backwards is zero", () => {
    expect(age(-5_000)).toBe("0s");
  });
});

describe("compactAge", () => {
  it("says one unit in the scale a glance needs", () => {
    expect(compactAge(12_000)).toBe("12s");
    expect(compactAge(5 * 60_000)).toBe("5m");
    expect(compactAge(2 * 3_600_000)).toBe("2h");
    expect(compactAge(3 * 86_400_000)).toBe("3d");
  });

  it("rounds down so a short age never claims the next unit", () => {
    expect(compactAge(59_999)).toBe("59s");
    expect(compactAge(61_000)).toBe("1m");
    expect(compactAge(59 * 60_000 + 59_000)).toBe("59m");
    expect(compactAge(23 * 3_600_000 + 59 * 60_000)).toBe("23h");
  });

  it("never says a negative age; a clock that moved backwards is zero", () => {
    expect(compactAge(-5_000)).toBe("0s");
  });
});
