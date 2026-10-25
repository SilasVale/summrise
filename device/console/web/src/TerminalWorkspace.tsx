import { useCallback, useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { callTool, type Grant } from "./terminal";

/**
 * Several sessions at once, each in its own tab.
 *
 * The legacy panel's workspace had this and it is the thing an operator misses first: one shell is a
 * debugging tool, three shells are working. A tab is a device session plus one xterm; the device keeps
 * the session after the page loses it, so `Attach` picks up a session that is already running rather
 * than opening another shell.
 *
 * Each tab polls only while it is the active one — a hidden tab's output is still there when it is
 * selected again, because the device retains it and the tab reads from its own cursor.
 */
type Tab = {
  key: string;
  session: string;
  kind: string;
  title: string;
};

type Instance = {
  term: Terminal;
  fit: FitAddon;
};

function terminalTheme() {
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  return dark
    ? { background: "#0e1319", foreground: "#e7edf4", cursor: "#bf3a0a", selectionBackground: "#2e3a47" }
    : { background: "#10151c", foreground: "#d5dde6", cursor: "#ff8a4c", selectionBackground: "#2a3540" };
}

export function TerminalWorkspace({ grant }: { grant: Grant }) {
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [status, setStatus] = useState("No session. Open one, or attach to a session this device already has.");
  const [available, setAvailable] = useState<{ id: string; kind: string }[]>([]);
  const instances = useRef(new Map<string, Instance>());
  const hosts = useRef(new Map<string, HTMLDivElement>());
  const activeRef = useRef<string | null>(null);
  const pollTimer = useRef<number | null>(null);

  useEffect(() => {
    activeRef.current = active;
  }, [active]);

  const poll = useCallback(
    async (tab: Tab) => {
      const instance = instances.current.get(tab.key);
      if (!instance) return;
      try {
        const read = JSON.parse(await callTool(grant, "terminal.read", { id: tab.session }));
        if (read.data) instance.term.write(read.data);
      } catch {
        /* a failed poll is retried; the session id decides whether it is alive */
      }
    },
    [grant]
  );

  // One xterm per tab, created when its host element exists and disposed when the tab goes away.
  useEffect(() => {
    for (const tab of tabs) {
      if (instances.current.has(tab.key)) continue;
      const host = hosts.current.get(tab.key);
      if (!host) continue;
      const term = new Terminal({
        convertEol: true,
        cursorBlink: true,
        fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
        fontSize: 13,
        theme: terminalTheme()
      });
      const fit = new FitAddon();
      term.loadAddon(fit);
      term.open(host);
      term.onData((data) => {
        void callTool(grant, "terminal.write", { id: tab.session, data }).catch(() => {});
      });
      instances.current.set(tab.key, { term, fit });
      if (activeRef.current === null) {
        activeRef.current = tab.key;
        setActive(tab.key);
      }
    }
    for (const [key, instance] of [...instances.current]) {
      if (tabs.some((tab) => tab.key === key)) continue;
      instance.term.dispose();
      instances.current.delete(key);
    }
  }, [tabs, grant]);

  // The active tab polls; switching polls once immediately so the tab is current when it appears.
  useEffect(() => {
    if (pollTimer.current !== null) window.clearInterval(pollTimer.current);
    pollTimer.current = null;
    if (!active) return;
    const tab = tabs.find((entry) => entry.key === active);
    if (!tab) return;
    const instance = instances.current.get(active);
    if (instance) {
      try {
        instance.fit.fit();
      } catch {
        /* the host may not be measurable yet */
      }
      void callTool(grant, "terminal.resize", {
        id: tab.session,
        rows: instance.term.rows,
        cols: instance.term.cols
      }).catch(() => {});
      instance.term.focus();
      void poll(tab);
    }
    pollTimer.current = window.setInterval(() => {
      if (activeRef.current === active) void poll(tab);
    }, 700);
    return () => {
      if (pollTimer.current !== null) window.clearInterval(pollTimer.current);
      pollTimer.current = null;
    };
  }, [active, tabs, grant, poll]);

  const open = async () => {
    try {
      const answer = JSON.parse(await callTool(grant, "terminal.create", { kind: "pty" }));
      const session = answer.id as string;
      setTabs((previous) => [
        ...previous,
        { key: session, session, kind: "pty", title: `pty ${previous.length + 1}` }
      ]);
      setActive(session);
      setStatus(`Session ${session} is open.`);
    } catch (cause) {
      setStatus("Could not open: " + (cause instanceof Error ? cause.message : String(cause)));
    }
  };

  const attach = async () => {
    try {
      const answer = JSON.parse(await callTool(grant, "terminal.list", {}));
      const sessions: { id: string; kind: string }[] = Array.isArray(answer) ? answer : [];
      setAvailable(sessions);
      setStatus(sessions.length === 0 ? "No open session on this device." : `${sessions.length} session(s) on this device.`);
    } catch (cause) {
      setStatus("List failed: " + (cause instanceof Error ? cause.message : String(cause)));
    }
  };

  const attachTo = (session: string, kind: string) => {
    if (tabs.some((tab) => tab.session === session)) {
      setActive(session);
      return;
    }
    setTabs((previous) => [...previous, { key: session, session, kind, title: `${kind} ${previous.length + 1}` }]);
    setActive(session);
    setAvailable((previous) => previous.filter((entry) => entry.id !== session));
    setStatus(`Attached to ${session}.`);
  };

  const close = async (tab: Tab) => {
    await callTool(grant, "terminal.close", { id: tab.session }).catch(() => {});
    setTabs((previous) => previous.filter((entry) => entry.key !== tab.key));
    if (activeRef.current === tab.key) {
      const next = tabs.find((entry) => entry.key !== tab.key) ?? null;
      setActive(next?.key ?? null);
    }
    setStatus(`Session ${tab.session} is closed.`);
  };

  return (
    <div className="terminal-workspace">
      <div className="panel-head">
        <h2>Terminal</h2>
        <div className="actions">
          <button type="button" onClick={() => void attach()}>Attach to a session</button>
          <button type="button" className="primary" onClick={() => void open()}>+ New</button>
        </div>
      </div>
      <p className={status.startsWith("Could not") || status.startsWith("List failed") ? "error" : "hint"}>{status}</p>

      {available.length > 0 && (
        <ul className="picker">
          {available.map((entry) => (
            <li key={entry.id}>
              <span className="mono">{entry.kind}</span>
              <span className="mono muted">{entry.id}</span>
              <button type="button" className="mini" onClick={() => attachTo(entry.id, entry.kind)}>Attach</button>
            </li>
          ))}
        </ul>
      )}

      {tabs.length > 0 && (
        <div className="tabbar" role="tablist">
          {tabs.map((tab) => (
            <button
              key={tab.key}
              type="button"
              role="tab"
              aria-selected={tab.key === active}
              className={"tab" + (tab.key === active ? " tab-active" : "")}
              onClick={() => setActive(tab.key)}
            >
              {tab.title}
              <span
                className="tab-close"
                role="button"
                tabIndex={0}
                aria-label={`Close ${tab.title}`}
                onClick={(event) => {
                  event.stopPropagation();
                  void close(tab);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.stopPropagation();
                    void close(tab);
                  }
                }}
              >
                ×
              </span>
            </button>
          ))}
        </div>
      )}

      {tabs.map((tab) => (
        <div
          key={tab.key}
          className="term-host"
          hidden={tab.key !== active}
          ref={(element) => {
            if (element) hosts.current.set(tab.key, element);
            else hosts.current.delete(tab.key);
          }}
        />
      ))}

      {tabs.length === 0 && (
        <div className="panel empty">
          <p>No session yet.</p>
          <p className="hint">+ New opens a shell on the device; Attach picks up one that is already running.</p>
        </div>
      )}
    </div>
  );
}
