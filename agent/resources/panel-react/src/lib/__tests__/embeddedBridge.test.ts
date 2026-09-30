// THE BRIDGE SURFACE, pinned against the shell's preload.
//
// `agent/tests/fixtures/embedded-bridge.json` is read here and by
// `summrise-desktop-electron/test/embedded-bridge.test.mjs`, which parses the preload that actually
// exposes it. Before this test the two sides were typed independently and neither could see the
// other: a renamed member would have broken the desktop app silently, in the one surface no rendered
// sweep can reach (behind `window.summriseEmbedded`, which a plain-browser harness does not have).
//
// This pins the NAMES. Behaviour still needs the desktop app, and this file says so rather than
// implying the surface is covered.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { BROWSER_MEMBERS, DESKTOP_MEMBERS, DSH_MEMBERS, EMBEDDED_MEMBERS } from "../embeddedBridge";

describe("the Electron bridge", () => {
  it("exposes exactly the members the preload exposes", () => {
    const fixture = JSON.parse(
      readFileSync(
        path.join(path.dirname(fileURLToPath(import.meta.url)), "../../../../../tests/fixtures/embedded-bridge.json"),
        "utf8",
      ),
    ) as { bridges: Record<string, string[]> };
    // ALL THREE bridges, not just the embedded one. The other two had no manifest until a round-54
    // audit of my own work asked who uses SummriseBrowserBridge — nobody did, because App.tsx reached
    // through `(window as any).summriseBrowser`. A type with no consumer cannot be checked, and a rename
    // would have surfaced as "Browser sessions need the Summrise desktop app" on a machine that has it.
    expect([...EMBEDDED_MEMBERS].sort()).toEqual([...fixture.bridges.summriseEmbedded].sort());
    expect([...BROWSER_MEMBERS].sort()).toEqual([...fixture.bridges.summriseBrowser].sort());
    expect([...DESKTOP_MEMBERS].sort()).toEqual([...fixture.bridges.summriseDesktop].sort());
    // THE FOURTH BRIDGE. It is the same shape of risk as the other three and the same blind spot
    // (behind a `window.*` a plain-browser harness does not have), so it is pinned the same way —
    // and this line is what makes a fifth bridge an explicit decision rather than an omission.
    expect([...DSH_MEMBERS].sort()).toEqual([...fixture.bridges.summriseDsh].sort());
    expect(Object.keys(fixture.bridges).sort()).toEqual(["summriseBrowser", "summriseDesktop", "summriseDsh", "summriseEmbedded"]);
  });
});
