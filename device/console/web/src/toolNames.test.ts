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
 * The vocabulary as it stands, 2026-11-12. One name is the device's (`forward_list`); the rest are this page's dotted
 * spelling, recorded with the open question above rather than quietly normalised. The memory pair joined with the
 * hundred-and-seventy-first list: the device has advertised `memory.list`/`memory.search` since the plugin landed.
 */
const INVENTORY = [
  "browser.close",
  "browser.eval",
  "browser.list",
  "browser.navigate",
  "browser.open",
  "browser.screenshot",
  "browser.shutdown",
  "files.list",
  "files.mkdir",
  "files.read",
  "files.write",
  "forward.list",
  "host.facts",
  "host.restart",
  "memory.list",
  "memory.search",
  "monitor.add",
  "monitor.list",
  "monitor.remove",
  "plugins.add",
  "plugins.inspect",
  "plugins.list",
  "plugins.logs",
  "plugins.remove",
  "plugins.restart",
  "state.get",
  "runs.exec",
  "runs.get",
  "runs.list",
  "screen.capture",
  "system.net_test",
  "system.process_kill",
  "system.process_list",
  "terminal.close",
  "terminal.create",
  "terminal.list",
  "terminal.read",
  "terminal.resize",
  "terminal.write"
] as const;

describe("the console's tool vocabulary", () => {
  it("reads names at all — a scan over nothing must not pass", () => {
    const names = calledNames();
    expect(names.size).toBeGreaterThan(20);
  });

  it("is exactly the recorded inventory, so a new name cannot slip in", () => {
    const found = [...calledNames().keys()].sort();
    const recorded: string[] = [...INVENTORY].sort();
    const added = found.filter((name) => !recorded.includes(name));
    const removed = recorded.filter((name) => !found.includes(name));
    expect(added, "a name this page calls that the inventory does not record").toEqual([]);
    expect(removed, "an inventory name this page no longer calls").toEqual([]);
  });

  it("keeps the one name the device declares, and it is still the device's", () => {
    // **MEASURED ON A LIVE DEVICE, 2026-11-12** (`GET /fleet/<id>/tools` on DESKTOP-9VH92BJ, 0.3.292): 46 tools,
    // every one namespaced with a dot — `terminal.read`, `files.read`, `plugins.list`, `system.process_list` — and
    // the console's list matches 31 of them exactly. The name that does NOT appear is `forward` itself: that build
    // has no forward plugin, so the Forward section's `forward.list` is this vocabulary's one unverified entry.
    expect(calledNames().has("forward.list")).toBe(true);
  });
});
