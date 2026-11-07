import { describe, expect, it } from "vitest";
import { parseSurfaceHash, surfaceHash } from "./surface-link";

describe("surface links", () => {
  it("round-trips every destination and Device tab", () => {
    for (const page of ["home", "activity"] as const) {
      expect(parseSurfaceHash(surfaceHash({ page }))).toEqual({ page });
    }
    // Device has no bare form of its own: the fragment always names the tab, with terminal first.
    expect(parseSurfaceHash(surfaceHash({ page: "device" }))).toEqual({
      page: "device",
      tab: "terminal"
    });
    for (const tab of ["overview", "terminal", "screen", "files", "runs", "tools", "machine"] as const) {
      expect(parseSurfaceHash(surfaceHash({ page: "device", tab }))).toEqual({
        page: "device",
        tab
      });
    }
  });

  it("round-trips a tool sheet on the Tools tab, including a namespaced plugin tool", () => {
    for (const tool of ["terminal.create", "hello.greet", "files.read"]) {
      const surface = { page: "device", tab: "tools", tool } as const;
      expect(surfaceHash(surface)).toBe("#device/tools/" + encodeURIComponent(tool));
      expect(parseSurfaceHash(surfaceHash(surface))).toEqual(surface);
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

  it("sends the two retired rail destinations to the machine's Tools tab when there is one", () => {
    for (const hash of ["#capabilities", "#plugins"]) {
      expect(parseSurfaceHash(hash, { machineSelected: true })).toEqual({
        page: "device",
        tab: "tools"
      });
      // No machine is a hub state: the old destination cannot name a device tab it has no device for.
      expect(parseSurfaceHash(hash, { machineSelected: false })).toEqual({ page: "home" });
    }
  });

  it("moves a tool deep link from the retired Capabilities page onto Tools", () => {
    expect(parseSurfaceHash("#capabilities/files.read", { machineSelected: true })).toEqual({
      page: "device",
      tab: "tools",
      tool: "files.read"
    });
    expect(parseSurfaceHash("#capabilities/hello.greet", { machineSelected: true })).toEqual({
      page: "device",
      tab: "tools",
      tool: "hello.greet"
    });
    expect(parseSurfaceHash("#capabilities/files.read", { machineSelected: false })).toEqual({
      page: "home"
    });
  });

  it("accepts #device as the default tab", () => {
    expect(parseSurfaceHash("#device")).toEqual({ page: "device" });
  });

  it("writes the legacy names as the new fragments", () => {
    expect(surfaceHash({ page: "device", tab: "tools" })).toBe("#device/tools");
    expect(surfaceHash({ page: "device", tab: "tools", tool: "files.read" })).toBe(
      "#device/tools/files.read"
    );
    expect(surfaceHash({ page: "device", tab: "machine" })).toBe("#device/machine");
    expect(surfaceHash({ page: "activity" })).toBe("#activity");
    expect(surfaceHash({ page: "home" })).toBe("#home");
  });

  it("addresses an Activity time range, so a chosen window can be linked", () => {
    const range = { from: 1_793_000_000_000, to: 1_793_003_600_000 };
    expect(surfaceHash({ page: "activity", range })).toBe(
      "#activity?from=1793000000000&to=1793003600000"
    );
    expect(parseSurfaceHash(surfaceHash({ page: "activity", range }))).toEqual({
      page: "activity",
      range
    });
    // A range belongs to Activity: another destination cannot carry one, and a malformed or
    // reversed pair is not an address the page may restore.
    expect(parseSurfaceHash("#device/files?from=1&to=2")).toBeNull();
    expect(parseSurfaceHash("#activity?from=2&to=1")).toBeNull();
    expect(parseSurfaceHash("#activity?from=abc&to=2")).toBeNull();
    expect(parseSurfaceHash("#activity?from=1")).toBeNull();
    expect(parseSurfaceHash("#activity?to=1")).toBeNull();
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
      expect(parseSurfaceHash(hash, { machineSelected: true })).toBeNull();
    }
  });
});
