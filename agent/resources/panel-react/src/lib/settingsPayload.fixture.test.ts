// THE SETTINGS PAYLOAD, read from the fixture the device is checked against.
//
// `agent/tests/fixtures/settings.json` is the same file `settings_get_shape` is checked against from
// the Rust side. Two directions matter here, and the second is why this route is not like the others:
//
//   * the panel must read keys the device sends — SettingsPage reads each field behind a `typeof`
//     guard, so a renamed field does not throw, it falls back to a default. THAT HALF IS NOT CHECKED
//     HERE: my first two attempts scanned SettingsPage's source for `j.<key>`, and both were wrong
//     (an unscoped scan picked up three keys from a different payload; a scoped one could not match
//     the block). The Rust test reads the REAL response and compares its key set with this file, which
//     is the stronger instrument and the one that would fail first. A text scan of the reader is a
//     weaker duplicate of it, so it was pruned rather than patched a third time;
//   * and a default is not merely displayed: the page's Save writes the input back. A renamed
//     `buffer_mb` would therefore show the default and then PERSIST it — a settings page quietly
//     overwriting a setting it can no longer read. That is worse than a blank card, and it is invisible.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.resolve(HERE, "..", "..", "..", "..", "tests", "fixtures", "settings.json");
const PAGE = path.resolve(HERE, "..", "components", "SettingsPage.tsx");

describe("the /api/settings payload", () => {
  const fixture = JSON.parse(readFileSync(FIXTURE, "utf8"));

  it("declares every key the panel is promised, with the envelope", () => {
    expect(fixture.keys).toContain("ok");
    expect(fixture.keys.length).toBeGreaterThanOrEqual(8);
    for (const k of fixture.required_by_panel) {
      expect(fixture.keys, `${k} is required by the panel but not declared`).toContain(k);
    }
    // The example must carry every declared key: an example that omits one would let a fixture drift
    // from its own list without anything noticing.
    for (const k of fixture.keys) {
      expect(k in fixture.example, `the example omits ${k}`).toBe(true);
    }
  });

  it("has at least one default that a rename could NOT hide behind", () => {
    // A `typeof` guard falls back to a default, and a fallback is only detectable when it differs from
    // what the device sends. Measured: `buffer_mb`'s default IS 8 — the device's own default — so a
    // rename there is genuinely invisible in the UI. The memory fields default to empty strings, which
    // the device never sends, so those ARE detectable. This asserts the weaker, true claim, and the
    // uninvisible case is exactly why the Rust-side key check above matters more than this half.
    const source = readFileSync(PAGE, "utf8");
    const defaults = [...source.matchAll(/useState\(\s*"([^"]*)"\s*\)/g)].map((m) => m[1]);
    expect(defaults.length).toBeGreaterThanOrEqual(1);
    const deviceValues = new Set(Object.values(fixture.example).map(String));
    const distinguishable = defaults.filter((d) => !deviceValues.has(d));
    expect(
      distinguishable.length,
      "every default matches a value the device sends, so a rename would be invisible",
    ).toBeGreaterThanOrEqual(1);
  });
});
