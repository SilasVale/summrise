// THE CONSOLE'S TOOL VOCABULARY, PINNED — the hundred-and-forty-first list's other half.
//
// **THE AGENT AND THIS PAGE AGREE BY SPELLING, AND NOTHING CHECKED IT.** `callTool(grant, "terminal.read", …)` travels to the
// device's MCP surface, which resolves the name against its own `ToolDef` table with an EXACT lookup — a name that is not
// there is not a compile error anywhere; it is a failed call in the UI, in production, on a device. The panel has carried a
// gate for exactly this since 2026-10-15 (`agent/tests/panel_tool_names.rs`: *"the panel and the agent are two processes that
// agree by spelling"*). The console had none, and this file is it — in inventory form.
//
// WHY INVENTORY AND NOT A DIRECT COMPARISON. Measured 2026-11-12 against a LIVE DEVICE (`GET /fleet/<id>/tools` on
// DESKTOP-9VH92BJ, 0.3.292): 46 tools, EVERY ONE namespaced with a dot — `terminal.read`, `files.read`, `plugins.list`,
// `system.process_list` — and this page's list matches 31 of them exactly. The one it does not match is `forward.list`:
// that build carries no forward plugin at all, so nothing can confirm or deny the name on it.
//
//     the console's names (this list)   32, dotted
//     a live device's catalogue         46, dotted (the same convention; `forward.*` absent on 0.3.292)
//     the agent's declarations          55, underscored — `ToolDef::new("terminal_read", …)` and friends are the
//                                       DECLARATION literals, not the wire names; the wire is dotted, as the live
//                                       catalogue shows. Reading those literals as the vocabulary is what renamed the
//                                       Forward section to `forward_list` for a round (corrected here).
//
// So the inventory is not waiting for permission: it is the census that keeps this list honest while the ONE unverified
// name (`forward.list`) ships with the device's forward plugin, which no live build carries yet.
//
// WHAT IS STILL UNVERIFIED: `forward.list`. The live catalogue proves the device's convention is dotted; it does not prove
// the forward plugin's own names until a device ships that plugin. Until then this list carries one entry the catalogue has
// never shown, and the test below says so rather than pretending otherwise.
import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** Every tool name `callTool` is handed anywhere under `src/` (tests excluded — they answer, they do not call). */
function calledNames(): Map<string, string[]> {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)));
  const walk = (dir: string): string[] =>
    readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
      const full = path.join(dir, entry.name);
      return entry.isDirectory() ? walk(full) : [full];
    });
  const out = new Map<string, string[]>();
  for (const file of walk(root)) {
    if (!/\.(ts|tsx)$/.test(file) || /\.test\.(ts|tsx)$/.test(file)) continue;
    const body = readFileSync(file, "utf8");
    for (const match of body.matchAll(/callTool\(\s*[^,]+,\s*"([^"]+)"/g)) {
      const list = out.get(match[1]) ?? [];
      list.push(path.relative(root, file));
      out.set(match[1], list);
    }
  }
  return out;
}

/**
 * **THE INVENTORY IS MEASURED NOW, NOT TYPED (the hundred-and-eighty-third list).** Every name below used to be a literal in this
 * file, added by hand one round at a time — forty chances to mistype a name that only a live device can refute. It is a
 * SNAPSHOT of what a device actually advertises (`device-tools.snapshot.json`, taken from `GET /fleet/<id>/tools`), refreshed by
 * one command: `bash scripts/device-tools-snapshot.sh <device> <release>`.
 *
 * **The snapshot records NAMES AND SCHEMAS, never answer shapes** — the fleet answers older shapes than the source tree, and
 * `docs/superpowers/specs/2026-11-12-device-wire-shapes.md` is the file that holds those.
 */
type SnapshotTool = { name: string; required: string[]; props: string[] };
const SNAPSHOT = JSON.parse(
  readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "device-tools.snapshot.json"), "utf8")
) as { release: string; measured_at: string; tools: SnapshotTool[] };

/**
 * **THE ONE NAME NO LIVE BUILD HAS SHOWN.** `forward.list` is called by the Forward section, and no device on this fleet carries the
 * forward plugin (`0.3.292` advertises 46 tools, none of them `forward.*`). It is listed here rather than quietly accepted, so the
 * exemption is visible in the diff if a future device ships the plugin and this entry should go.
 */
const NOT_IN_ANY_LIVE_CATALOGUE = ["forward.list"] as const;

describe("the console's tool vocabulary", () => {
  it("reads names at all — a scan over nothing must not pass", () => {
    expect(calledNames().size).toBeGreaterThan(20);
  });

  it("draws every name it calls from the MEASURED snapshot, or names the exemption", () => {
    const catalogue = new Set(SNAPSHOT.tools.map((tool) => tool.name));
    const exempt = new Set<string>(NOT_IN_ANY_LIVE_CATALOGUE);
    const unknown = [...calledNames().keys()].filter((name) => !catalogue.has(name) && !exempt.has(name)).sort();
    expect(unknown, "a name this page calls that no measured catalogue has shown").toEqual([]);
  });

  it("keeps the one exemption honest — it is still called, and still absent from the catalogue", () => {
    for (const name of NOT_IN_ANY_LIVE_CATALOGUE) {
      expect(calledNames().has(name), `${name} is exempted but no longer called`).toBe(true);
      expect(SNAPSHOT.tools.some((tool) => tool.name === name), `${name} is now in the catalogue — drop the exemption`).toBe(
        false
      );
    }
  });

  it("names the release the snapshot was measured from, so a stale one is visible", () => {
    expect(SNAPSHOT.release).not.toBe("");
    expect(SNAPSHOT.measured_at).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(SNAPSHOT.tools.length).toBeGreaterThan(30);
    // A snapshot with a duplicate name is one a refresh went wrong on.
    const names = SNAPSHOT.tools.map((tool) => tool.name);
    expect(new Set(names).size).toBe(names.length);
  });
});
