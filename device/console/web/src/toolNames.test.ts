// THE CONSOLE'S TOOL VOCABULARY, PINNED — the hundred-and-forty-first list's other half.
//
// **THE AGENT AND THIS PAGE AGREE BY SPELLING, AND NOTHING CHECKED IT.** `callTool(grant, "terminal.read", …)` travels to the
// device's MCP surface, which resolves the name against its own `ToolDef` table with an EXACT lookup — a name that is not
// there is not a compile error anywhere; it is a failed call in the UI, in production, on a device. The panel has carried a
// gate for exactly this since 2026-10-15 (`agent/tests/panel_tool_names.rs`: *"the panel and the agent are two processes that
// agree by spelling"*). The console had none, and this file is it — in inventory form.
//
// WHY INVENTORY AND NOT A DIRECT COMPARISON. Measured 2026-11-12, the two sides do not agree:
//
//     the console's names (this list)   32, dotted: terminal.read · files.read · browser.open · plugins.list · …
//     the agent's declarations          55, underscored: terminal_read · browser_run_script · … (never a dot, ever:
//                                       `git log -S'"terminal.read"' -- agent/src` returns nothing)
//     the one name that agrees          `forward_list` — the Forward section, fixed to the device's spelling by the
//                                       hundred-and-fortieth list
//
// A bare-array test (fail on any name the agent does not declare) would be RED today, and a red gate cannot be
// committed; an inventory test can, and it does the two things that matter while the question is open:
//
//   1. **a NEW name fails**, so the vocabulary cannot drift further while the answer is being found; and
//   2. **a REMOVED name fails**, so the list stays an honest census rather than a stale comment.
//
// THE QUESTION THIS FILE WAITS ON: the backlog's own live record (2026-11-06) says 17 concurrent `terminal.read` calls were
// answered at `wait_ms`'s 15 s bound through this console — an answer at the bound is an answer, so on THAT build the device
// understood the dotted name, and today's source says it cannot. The settlement is one device's `mcp_diag.log`
// (`agent/src/mcp/server.rs` logs every `tools/call` it receives, with whether it resolved) — see the backlog's
// hundred-and-forty-first list. When it settles, this fixture is rewritten to the agent's spelling and the test becomes the
// direct comparison.
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
 * spelling, recorded with the open question above rather than quietly normalised.
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
  "forward_list",
  "host.restart",
  "plugins.add",
  "plugins.inspect",
  "plugins.list",
  "plugins.logs",
  "plugins.remove",
  "plugins.restart",
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
    // `forward_list` is the only string here that the agent's `ToolDef` table contains (measured 2026-11-12: 1 of 32).
    // If this fails, either the Forward section drifted back to the dotted spelling or the inventory was edited without
    // the measurement — both are the bug the hundred-and-fortieth list fixed.
    expect(calledNames().has("forward_list")).toBe(true);
  });
});
