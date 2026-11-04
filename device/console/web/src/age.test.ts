import { describe, expect, it } from "vitest";
import { age } from "./age";

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
