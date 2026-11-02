import { useEffect, useState } from "react";
import { api } from "./api";

// The device's tool catalogue, as the console offers it. It is duplicated here rather than fetched
// because a grant is what asks the device anything, so there is nothing to ask before one exists.
// Fallback only: the console serves the registry at /tools, and this copy exists so a failed request
// leaves a usable form rather than an empty one. The order is `device/core/src/registry.rs`'s, so a
// diff against the registry is readable.
export const TOOL_NAMES = [
  "files.read", "files.write", "files.mkdir", "files.list",
  "memory.append", "memory.update", "memory.delete", "memory.search", "memory.list", "memory.export",
  "terminal.create", "terminal.write", "terminal.read", "terminal.resize", "terminal.close", "terminal.list", "terminal.screen",
  "browser.list", "browser.open", "browser.navigate", "browser.eval", "browser.screenshot", "browser.close", "browser.shutdown",
  "monitor.add", "monitor.remove", "monitor.list", "monitor.probe", "monitor.status",
  "runs.exec", "runs.list", "runs.get",
  "system.process_list", "system.process_kill", "system.net_test",
  "host.facts",
  "screen.capture",
  "state.get"
];

/**
 * The catalogue a grant form offers: the served registry once the client answers, the fallback
 * before that and after any failure.
 *
 * The registry is the list — a copy in the page drifted once and offered a tool that does not
 * exist. An empty answer is a failure for this purpose: a form with no tools offers nothing to
 * grant, so the copy stays. An answer that lands after unmount is dropped.
 */
export function useToolNames(): string[] {
  const [names, setNames] = useState<string[]>(TOOL_NAMES);

  useEffect(() => {
    let live = true;
    api
      .tools()
      .then((tools) => {
        if (!live || tools.length === 0) return;
        setNames(tools.map((tool) => tool.name));
      })
      .catch(() => {
        /* keep the fallback */
      });
    return () => {
      live = false;
    };
  }, []);

  return names;
}
