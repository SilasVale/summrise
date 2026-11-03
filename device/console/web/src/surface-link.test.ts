import { describe, expect, it } from "vitest";
import { parseSurfaceHash, surfaceHash } from "./surface-link";

describe("surface links", () => {
  it("round-trips every destination and Device tab", () => {
    for (const page of ["home", "capabilities", "plugins", "activity"] as const) {
      expect(parseSurfaceHash(surfaceHash({ page }))).toEqual({ page });
    }
    // Device has no bare form of its own: the fragment always names the tab, with terminal first.
    expect(parseSurfaceHash(surfaceHash({ page: "device" }))).toEqual({
      page: "device",
      tab: "terminal"
    });
    for (const tab of ["terminal", "screen", "files", "runs", "machine"] as const) {
      expect(parseSurfaceHash(surfaceHash({ page: "device", tab }))).toEqual({
        page: "device",
        tab
      });
    }
  });

  it("round-trips a tool sheet, including a namespaced plugin tool", () => {
    for (const tool of ["terminal.create", "hello.greet", "files.read"]) {
      expect(parseSurfaceHash(surfaceHash({ page: "capabilities", tool }))).toEqual({
        page: "capabilities",
        tool
      });
    }
  });

  it("round-trips the Browser view and its short link", () => {
    const browser = { page: "device", tab: "screen", view: "browser" } as const;
    expect(surfaceHash(browser)).toBe("#device/screen/browser");
    expect(parseSurfaceHash("#device/screen/browser")).toEqual(browser);
    expect(parseSurfaceHash("#device/browser")).toEqual(browser);
    expect(parseSurfaceHash("#browser")).toEqual(browser);
  });

  it("maps every legacy surface fragment as an alias", () => {
    expect(parseSurfaceHash("#devices")).toEqual({ page: "home" });
    expect(parseSurfaceHash("#terminal")).toEqual({ page: "device", tab: "terminal" });
    expect(parseSurfaceHash("#screen")).toEqual({ page: "device", tab: "screen" });
    expect(parseSurfaceHash("#browser")).toEqual({
      page: "device",
      tab: "screen",
      view: "browser"
    });
    expect(parseSurfaceHash("#files")).toEqual({ page: "device", tab: "files" });
    expect(parseSurfaceHash("#runs")).toEqual({ page: "device", tab: "runs" });
    expect(parseSurfaceHash("#system")).toEqual({ page: "device", tab: "machine" });
    expect(parseSurfaceHash("#audit")).toEqual({ page: "activity" });
  });

  it("accepts #device as the default tab", () => {
    expect(parseSurfaceHash("#device")).toEqual({ page: "device" });
  });

  it("writes the legacy names as the new fragments", () => {
    expect(surfaceHash({ page: "device", tab: "machine" })).toBe("#device/machine");
    expect(surfaceHash({ page: "activity" })).toBe("#activity");
    expect(surfaceHash({ page: "home" })).toBe("#home");
  });

  it("rejects run links, unknown names and near misses", () => {
    for (const hash of [
      "",
      "#",
      "#run",
      "#run/a/b",
      "#Files",
      "#memory",
      "#files/",
      "#files?x",
      "#device/",
      "#device/camera",
      "#capabilities/",
      "#plugins/extra"
    ]) {
      expect(parseSurfaceHash(hash)).toBeNull();
    }
  });
});
