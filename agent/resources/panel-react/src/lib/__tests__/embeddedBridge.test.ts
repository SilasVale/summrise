// THE BRIDGE SURFACE, pinned against the shell's preload.
//
// `agent/tests/fixtures/embedded-bridge.json` is read here and by
// `vale-desktop-electron/test/embedded-bridge.test.mjs`, which parses the preload that actually
// exposes it. Before this test the two sides were typed independently and neither could see the
// other: a renamed member would have broken the desktop app silently, in the one surface no rendered
// sweep can reach (behind `window.valeEmbedded`, which a plain-browser harness does not have).
//
// This pins the NAMES. Behaviour still needs the desktop app, and this file says so rather than
// implying the surface is covered.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { EMBEDDED_MEMBERS } from "../embeddedBridge";

describe("the Electron bridge", () => {
  it("exposes exactly the members the preload exposes", () => {
    const fixture = JSON.parse(
      readFileSync(
        path.join(path.dirname(fileURLToPath(import.meta.url)), "../../../../../tests/fixtures/embedded-bridge.json"),
        "utf8",
      ),
    ) as { bridges: Record<string, string[]> };
    expect([...EMBEDDED_MEMBERS].sort()).toEqual([...fixture.bridges.valeEmbedded].sort());
    // The other two bridges are consumed by other components; the fixture lists them so the shell's
    // half of this contract covers them too, and a third bridge cannot appear unnoticed.
    expect(Object.keys(fixture.bridges).sort()).toEqual(["valeBrowser", "valeDesktop", "valeEmbedded"]);
  });
});
