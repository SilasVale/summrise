import { describe, expect, it } from "vitest";
import { parseRunLink, runLink } from "./run-link";

describe("run links", () => {
  it("round-trips a device and a run id", () => {
    const hash = runLink("device/one", "run 1");
    expect(hash).toBe("#run/device%2Fone/run%201");
    expect(parseRunLink(hash)).toEqual({ deviceId: "device/one", runId: "run 1" });
  });

  it("reads the plain ids Activity carries", () => {
    expect(parseRunLink("#run/4c718d6e/r-1")).toEqual({ deviceId: "4c718d6e", runId: "r-1" });
  });

  it("rejects anything that is not one device and one run", () => {
    for (const hash of ["", "#", "#run", "#run/a", "#run/a/", "#run//b", "#run/a/b/c", "#other/a/b"]) {
      expect(parseRunLink(hash)).toBeNull();
    }
    expect(parseRunLink("#run/a/%E0%A4%A")).toBeNull();
  });
});
