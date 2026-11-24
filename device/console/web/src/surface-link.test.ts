import { describe, expect, it } from "vitest";
import { parseSurfaceHash, surfaceHash } from "./surface-link";

describe("surface links", () => {
  it.each(["machines", "access"] as const)("round-trips the A3 #%s destination without a machine", (page) => {
    expect(parseSurfaceHash("#" + page, { machineSelected: false })).toEqual({ page });
    expect(parseSurfaceHash(surfaceHash({ page }), { machineSelected: true })).toEqual({ page });
  });
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

  it("reads the two one-shot tray links and refuses anything that is not one", () => {
    // **THE TRAY'S LINKS ARE ONE-SHOT.** `#device/terminal/new=<kind>` opens the Connect dialog;
    // `#device/browser/open=<url>` prefills the Browser address. The address writer below never
    // emits either, so the App has to consume the request into state.
    expect(parseSurfaceHash("#device/terminal/new=local")).toEqual({
      page: "device",
      tab: "terminal",
      newSession: { kind: "local" }
    });
    for (const kind of ["ssh", "serial"] as const) {
      expect(parseSurfaceHash(`#device/terminal/new=${kind}`)).toEqual({
        page: "device",
        tab: "terminal",
        newSession: { kind }
      });
    }
    // The dialog's own name is `pty`; the link says `local` and the dialog maps it. The parser is
    // not the dialog, so `new=pty` is not a link.
    expect(parseSurfaceHash("#device/terminal/new=pty")).toBeNull();
    // One decode: `%3F`/`%3D`/`%26` are the URL's own characters, not a second query string.
    expect(
      parseSurfaceHash("#device/browser/open=https%3A%2F%2Fexample.test%2Fa%3Fb%3D1%26c%3D2")
    ).toEqual({
      page: "device",
      tab: "screen",
      view: "browser",
      openBrowser: { url: "https://example.test/a?b=1&c=2" }
    });
    expect(parseSurfaceHash("#device/browser/open=about%3Ablank")).toEqual({
      page: "device",
      tab: "screen",
      view: "browser",
      openBrowser: { url: "about:blank" }
    });
    // A fragment and a space in the address are the URL's own characters, not a second hash or a
    // truncated field: one decode, and the whole address survives.
    expect(parseSurfaceHash("#device/browser/open=https%3A%2F%2Fexample.test%2Fa%23frag")).toEqual({
      page: "device",
      tab: "screen",
      view: "browser",
      openBrowser: { url: "https://example.test/a#frag" }
    });
    expect(parseSurfaceHash("#device/browser/open=https%3A%2F%2Fexample.test%2Fa%20b")).toEqual({
      page: "device",
      tab: "screen",
      view: "browser",
      openBrowser: { url: "https://example.test/a b" }
    });
    for (const bad of [
      // A scheme the WebView must never be handed from a fragment.
      "#device/browser/open=javascript%3Aalert(1)",
      "#device/browser/open=file%3A%2F%2Fx",
      // A broken escape is not an address; a double-encoded scheme is not http(s).
      "#device/browser/open=%E0%A4%A",
      "#device/browser/open=https%253A%252F%252Fexample.test",
      "#device/browser/open=",
      "#device/terminal/new=",
      "#device/terminal/new=local/extra"
    ]) {
      expect(parseSurfaceHash(bad)).toBeNull();
    }
    // The writer is the destination only: a one-shot field on a Surface cannot become an address.
    expect(
      surfaceHash({ page: "device", tab: "terminal", newSession: { kind: "local" } })
    ).toBe("#device/terminal");
    expect(
      surfaceHash({
        page: "device",
        tab: "screen",
        view: "browser",
        openBrowser: { url: "https://example.test/a" }
      })
    ).toBe("#device/screen/browser");
    // The tab's own link is unchanged by the new forms.
    expect(parseSurfaceHash("#device/terminal")).toEqual({ page: "device", tab: "terminal" });
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
