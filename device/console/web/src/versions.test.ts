import { describe, expect, it } from "vitest";
import { versionBelow } from "./versions";

/**
 * A version is a comparison, not a sort. The fleet's cue says "behind 0.3.284" only when both
 * versions are known and the machine's is numerically older; a string comparison would put
 * 0.3.9 after 0.3.10 and a malformed value would make every row a guess.
 */
describe("versionBelow — the fleet's numeric comparison against the staged installer", () => {
  it("compares the numeric parts, not their spelling", () => {
    expect(versionBelow("0.3.9", "0.3.10")).toBe(true);
    expect(versionBelow("0.3.42", "0.3.280")).toBe(true);
    expect(versionBelow("0.3.280", "0.3.284")).toBe(true);
    expect(versionBelow("0.3.10", "0.3.9")).toBe(false);
  });

  it("is false when the versions are equal, whatever their spelling", () => {
    expect(versionBelow("0.3.284", "0.3.284")).toBe(false);
    expect(versionBelow("0.3.284", "0.3.284.0")).toBe(false);
  });

  it("is false when either side is unknown, so no row invents a cue", () => {
    expect(versionBelow(null, "0.3.284")).toBe(false);
    expect(versionBelow("0.3.280", null)).toBe(false);
    expect(versionBelow(undefined, undefined)).toBe(false);
    expect(versionBelow("", "0.3.284")).toBe(false);
    expect(versionBelow("0.3.280", "0.3.284-rc1")).toBe(false);
    expect(versionBelow("0.3.x", "0.3.284")).toBe(false);
  });
});
