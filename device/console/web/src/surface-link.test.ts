import { describe, expect, it } from "vitest";
import { parseSurfaceHash, surfaceHash } from "./surface-link";

describe("surface links", () => {
  it("round-trips every destination", () => {
    for (const page of ["devices", "terminal", "screen", "files", "runs", "system", "audit"] as const) {
      expect(parseSurfaceHash(surfaceHash(page))).toBe(page);
    }
  });

  it("rejects run links, unknown names and near misses", () => {
    for (const hash of ["", "#", "#run", "#run/a/b", "#Files", "#memory", "#files/", "#files?x"]) {
      expect(parseSurfaceHash(hash)).toBeNull();
    }
  });
});
