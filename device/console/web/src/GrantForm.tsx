import { useEffect, useState } from "react";
import { api, getToken } from "./api";

// The device's tool catalogue, as the grant form offers it. It is duplicated here rather than fetched
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

export function GrantForm({
  deviceId,
  onGranted
}: {
  deviceId: string;
  onGranted: (envelope: unknown, tools: string[]) => void;
}) {
  // Everything selected to begin with: the operator is the only user, the grant expires, and a
  // half-selected list is the reason a first attempt fails with "tool not permitted".
  const [names, setNames] = useState<string[]>(TOOL_NAMES);
  const [chosen, setChosen] = useState<Set<string>>(new Set(TOOL_NAMES));
  const [lifetime, setLifetime] = useState(3600);
  const [destructive, setDestructive] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // The registry is the list. A copy in the page drifted once and offered a tool that does not exist.
  useEffect(() => {
    let live = true;
    fetch("/tools", { headers: { authorization: `Bearer ${getToken()}` } })
      .then((response) => (response.ok ? response.json() : Promise.reject(new Error(String(response.status)))))
      .then((tools: { name: string }[]) => {
        if (!live || tools.length === 0) return;
        const list = tools.map((tool) => tool.name);
        setNames(list);
        setChosen(new Set(list));
      })
      .catch(() => {
        /* keep the fallback */
      });
    return () => {
      live = false;
    };
  }, []);

  const toggle = (name: string) => {
    const next = new Set(chosen);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    setChosen(next);
  };

  const mint = async () => {
    if (chosen.size === 0) {
      setError("Pick at least one tool.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      // The whole envelope goes to the device — the grant facts AND the console's signature over
      // them. Sending `envelope.grant` alone is a grant nobody signed, and the device says so.
      const envelope = await api.grant(deviceId, [...chosen], lifetime, destructive);
      onGranted(envelope, [...chosen]);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="grant">
      <div className="grant-row">
        <label htmlFor="lifetime">Lifetime (seconds)</label>
        <input id="lifetime" type="number" min={60} max={604800} value={lifetime} onChange={(event) => setLifetime(Number(event.target.value))} />
        <label className="check">
          <input type="checkbox" checked={destructive} onChange={(event) => setDestructive(event.target.checked)} />
          Allow destructive tools
        </label>
      </div>

      <fieldset className="tools">
        <legend>
          Tools ({chosen.size} of {names.length})
          <button type="button" className="mini" onClick={() => setChosen(new Set(names))}>All</button>
          <button type="button" className="mini" onClick={() => setChosen(new Set())}>None</button>
        </legend>
        <div className="tool-grid">
          {names.map((name) => (
            <label key={name} className="check mono">
              <input type="checkbox" checked={chosen.has(name)} onChange={() => toggle(name)} />
              {name}
            </label>
          ))}
        </div>
      </fieldset>

      <button type="button" className="primary" disabled={busy} onClick={() => void mint()}>
        {busy ? "Signing…" : "Grant access"}
      </button>
      {error && <p className="error">{error}</p>}
    </div>
  );
}
