import { describe, expect, it } from "vitest";
import { versionBelow, versionEqual, versionRelation } from "./versions";

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

/**
 * THE UPDATE STORY'S ONE COMPARISON. `0.3.284` and `0.3.284.0` are the same release, and the
 * device's own two fields may spell it either way; a value that is not a version proves nothing,
 * so it never claims to be what runs.
 */
describe("versionEqual — the running version against the version the updater applied", () => {
  it("is true for the same release, whatever its spelling", () => {
    expect(versionEqual("0.3.290", "0.3.290")).toBe(true);
    expect(versionEqual("0.3.284", "0.3.284.0")).toBe(true);
    expect(versionEqual("0.3.9", "0.3.9.0.0")).toBe(true);
  });

  it("is false for different releases, in either direction", () => {
    expect(versionEqual("0.3.9", "0.3.10")).toBe(false);
    expect(versionEqual("0.3.290", "0.3.280")).toBe(false);
  });

  it("is false when either side is not a comparable version", () => {
    expect(versionEqual(null, "0.3.284")).toBe(false);
    expect(versionEqual("0.3.284", undefined)).toBe(false);
    expect(versionEqual("", "")).toBe(false);
    expect(versionEqual("0.3.284-rc1", "0.3.284")).toBe(false);
    expect(versionEqual("0.3.x", "0.3.284")).toBe(false);
  });
});

/**
 * THE MACHINE PAGE'S ONE ANSWER. The fleet asks "is it behind?"; the Machine tab also has to say
 * "ahead" and "current", because a machine on the release and one a release ahead used to look
 * identical except for the digits.
 */
describe("versionRelation — current, behind or ahead of the staged release", () => {
  it("says current for the same release, whatever its spelling", () => {
    expect(versionRelation("0.3.291", "0.3.291")).toBe("current");
    expect(versionRelation("0.3.291", "0.3.291.0")).toBe("current");
  });

  it("says behind when the running build is numerically older", () => {
    expect(versionRelation("0.3.289", "0.3.291")).toBe("behind");
    expect(versionRelation("0.3.9", "0.3.10")).toBe("behind");
  });

  it("says ahead when the running build is numerically newer", () => {
    // The measured 9VH: 0.3.292 runs while the release is 0.3.291.
    expect(versionRelation("0.3.292", "0.3.291")).toBe("ahead");
    expect(versionRelation("0.3.10", "0.3.9")).toBe("ahead");
  });

  it("says nothing when either side is not a version", () => {
    expect(versionRelation(null, "0.3.291")).toBeNull();
    expect(versionRelation("0.3.291", null)).toBeNull();
    expect(versionRelation(undefined, undefined)).toBeNull();
    expect(versionRelation("", "0.3.291")).toBeNull();
    expect(versionRelation("0.3.291", "0.3.291-rc1")).toBeNull();
    expect(versionRelation("0.3.x", "0.3.291")).toBeNull();
  });
});
