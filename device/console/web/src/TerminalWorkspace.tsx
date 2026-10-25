import { useCallback, useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { callTool, type Grant } from "./terminal";
import { api, type SavedConnection } from "./api";
import { Modal } from "./Modal";

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
  const [presets, setPresets] = useState<SavedConnection[]>([]);
  const [dialog, setDialog] = useState(false);
  const [draftError, setDraftError] = useState<string | null>(null);
  const [draft, setDraft] = useState({
    name: "",
    kind: "pty",
    host: "",
    port: "22",
    user: "",
    port_path: "",
    baud: "115200",
    password: ""
  });
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

  // The presets belong to the device, so they are re-read when the grant's device changes.
  useEffect(() => {
    let live = true;
    void api
      .connections(grant.deviceId)
      .then((list) => {
        if (live) setPresets(list);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [grant.deviceId]);

  const openWith = async (args: Record<string, unknown>, title: string) => {
    try {
      const answer = JSON.parse(await callTool(grant, "terminal.create", args));
      const session = answer.id as string;
      setTabs((previous) => [
        ...previous,
        { key: session, session, kind: String(args.kind), title }
      ]);
      setActive(session);
      setStatus(`Session ${session} is open.`);
    } catch (cause) {
      setStatus("Could not open: " + (cause instanceof Error ? cause.message : String(cause)));
    }
  };

  const open = () => void openWith({ kind: "pty" }, `pty ${tabs.length + 1}`);

  /** The create arguments a preset (or the dialog's draft) implies. */
  const argsFor = (source: {
    kind: string;
    host?: string;
    port?: number;
    user?: string;
    port_path?: string;
    baud?: number;
    password?: string;
  }): Record<string, unknown> => {
    if (source.kind === "ssh") {
      const args: Record<string, unknown> = {
        kind: "ssh",
        host: source.host,
        port: source.port ?? 22,
        user: source.user
      };
      // The password is typed for this session and never saved: a preset names the connection, not
      // the credential.
      if (source.password) args.auth = { kind: "password", password: source.password };
      return args;
    }
    if (source.kind === "serial") {
      return { kind: "serial", port: source.port_path, baud: source.baud ?? 115200 };
    }
    return { kind: "pty" };
  };

  const savePreset = async () => {
    if (!draft.name.trim()) {
      setDraftError("Give the preset a name.");
      return;
    }
    try {
      const saved = await api.saveConnection(grant.deviceId, {
        name: draft.name.trim(),
        kind: draft.kind,
        host: draft.host.trim() || undefined,
        port: Number(draft.port) || undefined,
        user: draft.user.trim() || undefined,
        port_path: draft.port_path.trim() || undefined,
        baud: Number(draft.baud) || undefined
      });
      setPresets((previous) => [...previous, saved]);
      setDraftError(null);
    } catch (cause) {
      setDraftError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  const forgetPreset = async (preset: SavedConnection) => {
    await api.removeConnection(grant.deviceId, preset.id).catch(() => {});
    setPresets((previous) => previous.filter((entry) => entry.id !== preset.id));
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
          <button type="button" onClick={() => setDialog(true)}>Save a preset</button>
          <button type="button" className="primary" onClick={open}>+ New</button>
        </div>
      </div>
      <p className={status.startsWith("Could not") || status.startsWith("List failed") ? "error" : "hint"}>{status}</p>

      {presets.length > 0 && (
        <ul className="picker">
          {presets.map((preset) => (
            <li key={preset.id}>
              <span className="mono">{preset.kind}</span>
              <strong>{preset.name}</strong>
              <span className="mono muted">
                {preset.kind === "serial"
                  ? `${preset.port_path ?? "?"} ${preset.baud ?? ""}`
                  : preset.kind === "ssh"
                    ? `${preset.user ?? "?"}@${preset.host ?? "?"}:${preset.port ?? 22}`
                    : "local shell"}
              </span>
              <button type="button" className="mini" onClick={() => void openWith(argsFor(preset), preset.name)}>
                Open
              </button>
              <button type="button" className="mini" onClick={() => void forgetPreset(preset)}>Forget</button>
            </li>
          ))}
        </ul>
      )}

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

      {dialog && (
        <Modal title="Save a terminal preset" onClose={() => setDialog(false)}>
          <label htmlFor="preset-name">Name</label>
          <input
            id="preset-name"
            value={draft.name}
            onChange={(event) => setDraft({ ...draft, name: event.target.value })}
            placeholder="the db host"
          />
          <label htmlFor="preset-kind">Kind</label>
          <select
            id="preset-kind"
            value={draft.kind}
            onChange={(event) => setDraft({ ...draft, kind: event.target.value })}
          >
            <option value="pty">pty — a shell on the device</option>
            <option value="ssh">ssh — a remote host</option>
            <option value="serial">serial — a port on the device</option>
          </select>
          {draft.kind === "ssh" && (
            <>
              <label htmlFor="preset-host">Host</label>
              <input
                id="preset-host"
                value={draft.host}
                onChange={(event) => setDraft({ ...draft, host: event.target.value })}
                placeholder="db.internal"
              />
              <label htmlFor="preset-port">Port</label>
              <input
                id="preset-port"
                value={draft.port}
                onChange={(event) => setDraft({ ...draft, port: event.target.value })}
                placeholder="22"
              />
              <label htmlFor="preset-user">User</label>
              <input
                id="preset-user"
                value={draft.user}
                onChange={(event) => setDraft({ ...draft, user: event.target.value })}
                placeholder="operator"
              />
              <p className="hint">The password is not saved: type it when the session opens.</p>
            </>
          )}
          {draft.kind === "serial" && (
            <>
              <label htmlFor="preset-path">Port path</label>
              <input
                id="preset-path"
                value={draft.port_path}
                onChange={(event) => setDraft({ ...draft, port_path: event.target.value })}
                placeholder="COM3 or /dev/ttyUSB0"
              />
              <label htmlFor="preset-baud">Baud</label>
              <input
                id="preset-baud"
                value={draft.baud}
                onChange={(event) => setDraft({ ...draft, baud: event.target.value })}
                placeholder="115200"
              />
            </>
          )}
          {draftError && <p className="error">{draftError}</p>}
          <div className="modal-actions">
            <button type="button" onClick={() => setDialog(false)}>Cancel</button>
            <button type="button" className="primary" onClick={() => void savePreset()}>Save</button>
          </div>
        </Modal>
      )}

      {tabs.length === 0 && (
        <div className="panel empty">
          <p>No session yet.</p>
          <p className="hint">+ New opens a shell on the device; Attach picks up one that is already running.</p>
        </div>
      )}
    </div>
  );
}
