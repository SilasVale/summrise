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

/** The three kinds `terminal.create` accepts, and the shape of the form each one needs. */
type ConnectKind = "pty" | "ssh" | "serial";

/**
 * What the connect dialog holds while it is open.
 *
 * The password and the key path live here and nowhere else: they are put into one `terminal.create`
 * call and dropped when the dialog closes. Nothing about this draft is persisted — not a preset, not
 * `localStorage`, not the URL.
 */
type ConnectDraft = {
  kind: ConnectKind;
  host: string;
  port: string;
  user: string;
  /** Exactly one credential is sent for SSH; this is which one the operator is filling in. */
  auth: "password" | "key";
  password: string;
  key_path: string;
  port_path: string;
  baud: string;
  parity: "none" | "odd" | "even";
  data_bits: "5" | "6" | "7" | "8";
  stop_bits: "1" | "2";
};

type Connect = {
  draft: ConnectDraft;
  /** The preset this dialog was opened from, when there was one; it names the tab. */
  name: string | null;
  error: string | null;
  busy: boolean;
};

const NEW_CONNECT: ConnectDraft = {
  kind: "pty",
  host: "",
  port: "22",
  user: "",
  auth: "password",
  password: "",
  key_path: "",
  port_path: "",
  baud: "115200",
  parity: "none",
  data_bits: "8",
  stop_bits: "1"
};

/** A preset's own form: the connection's shape, never a credential. */
type Preset = {
  name: string;
  kind: ConnectKind;
  host: string;
  port: string;
  user: string;
  port_path: string;
  baud: string;
};

const NEW_PRESET: Preset = { name: "", kind: "pty", host: "", port: "22", user: "", port_path: "", baud: "115200" };

function terminalTheme() {
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  return dark
    ? { background: "#0e1319", foreground: "#e7edf4", cursor: "#bf3a0a", selectionBackground: "#2e3a47" }
    : { background: "#10151c", foreground: "#d5dde6", cursor: "#ff8a4c", selectionBackground: "#2a3540" };
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * The arguments one dialog draft implies.
 *
 * The device's `terminal.create` has `additionalProperties: false` and refuses a field another kind
 * owns by name (`kind ssh takes host, port, user, auth …`), so each kind sends only its own fields:
 * pty nothing, ssh the four connection fields plus one `auth`, serial the port path and its framing.
 */
function connectArguments(draft: ConnectDraft): { args: Record<string, unknown> } | { error: string } {
  if (draft.kind === "ssh") {
    const host = draft.host.trim();
    const user = draft.user.trim();
    const port = Number(draft.port.trim());
    if (!host) return { error: "An SSH session needs a host." };
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      return { error: "The port must be a whole number from 1 to 65535 (22 is the SSH default)." };
    }
    if (!user) return { error: "An SSH session needs a user." };
    if (draft.auth === "password") {
      if (draft.password === "") return { error: "Type the password, or choose a key path instead." };
      return {
        args: { kind: "ssh", host, port, user, auth: { kind: "password", password: draft.password } }
      };
    }
    const path = draft.key_path.trim();
    if (!path) return { error: "Enter the absolute path to the private key on the device, or choose a password." };
    if (!isAbsolutePath(path)) {
      return {
        error:
          "The device needs an absolute key path, e.g. /home/operator/.ssh/id_ed25519 or C:\\Users\\operator\\.ssh\\id_ed25519."
      };
    }
    return { args: { kind: "ssh", host, port, user, auth: { kind: "key", path } } };
  }
  if (draft.kind === "serial") {
    const port = draft.port_path.trim();
    if (!port) return { error: "A serial session needs the port path, e.g. COM3 or /dev/ttyUSB0." };
    const baud = Number(draft.baud.trim());
    if (!Number.isInteger(baud) || baud < 1 || baud > 4_000_000) {
      return { error: "The baud rate must be a whole number from 1 to 4000000." };
    }
    return {
      args: {
        kind: "serial",
        port,
        baud,
        parity: draft.parity,
        data_bits: Number(draft.data_bits),
        stop_bits: Number(draft.stop_bits)
      }
    };
  }
  return { args: { kind: "pty" } };
}

/** The device reads its keys from its own filesystem, so a relative path is a refusal, not a lookup. */
function isAbsolutePath(text: string): boolean {
  return text.startsWith("/") || /^[A-Za-z]:[\\/]/.test(text) || text.startsWith("\\\\");
}

export function TerminalWorkspace({ grant }: { grant: Grant }) {
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [status, setStatus] = useState("No session. Open one, or attach to a session this device already has.");
  const [available, setAvailable] = useState<{ id: string; kind: string }[]>([]);
  const [presets, setPresets] = useState<SavedConnection[]>([]);
  const [connect, setConnect] = useState<Connect | null>(null);
  const [presetDialog, setPresetDialog] = useState(false);
  const [preset, setPreset] = useState<Preset>({ ...NEW_PRESET });
  const [presetError, setPresetError] = useState<string | null>(null);
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

  /** Open one session and add its tab. Returns the device's sentence on failure, null on success. */
  const openWith = async (args: Record<string, unknown>, title: string): Promise<string | null> => {
    try {
      const answer = JSON.parse(await callTool(grant, "terminal.create", args));
      const session = answer.id as string;
      setTabs((previous) => [...previous, { key: session, session, kind: String(args.kind), title }]);
      setActive(session);
      setStatus(`Session ${session} is open.`);
      return null;
    } catch (cause) {
      const sentence = messageOf(cause);
      setStatus("Could not open: " + sentence);
      return sentence;
    }
  };

  const openDialog = (prefill: Partial<ConnectDraft>, name: string | null) => {
    setConnect({ draft: { ...NEW_CONNECT, ...prefill }, name, error: null, busy: false });
  };

  const patchConnect = (patch: Partial<ConnectDraft>) => {
    setConnect((previous) =>
      previous ? { ...previous, draft: { ...previous.draft, ...patch }, error: null } : previous
    );
  };

  const submitConnect = async () => {
    if (!connect) return;
    const built = connectArguments(connect.draft);
    if ("error" in built) {
      setConnect({ ...connect, error: built.error });
      return;
    }
    setConnect({ ...connect, busy: true, error: null });
    const title = connect.name ?? `${connect.draft.kind} ${tabs.length + 1}`;
    const failure = await openWith(built.args, title);
    if (failure === null) setConnect(null);
    else setConnect({ ...connect, busy: false, error: failure });
  };

  /**
   * A preset is a connection, not a credential.
   *
   * pty and serial presets have nothing secret left to ask for, so they open straight away. An SSH
   * preset opens the connect dialog prefilled with host, port and user — the password or key path is
   * the one thing the operator still has to supply, and there is now a field for it.
   */
  const openPreset = (saved: SavedConnection) => {
    if (saved.kind === "ssh") {
      openDialog(
        { kind: "ssh", host: saved.host ?? "", port: String(saved.port ?? 22), user: saved.user ?? "" },
        saved.name
      );
      return;
    }
    if (saved.kind === "serial") {
      void openWith({ kind: "serial", port: saved.port_path, baud: saved.baud ?? 115200 }, saved.name);
      return;
    }
    void openWith({ kind: "pty" }, saved.name);
  };

  const savePreset = async () => {
    if (!preset.name.trim()) {
      setPresetError("Give the preset a name.");
      return;
    }
    try {
      const saved = await api.saveConnection(grant.deviceId, {
        name: preset.name.trim(),
        kind: preset.kind,
        host: preset.host.trim() || undefined,
        port: Number(preset.port) || undefined,
        user: preset.user.trim() || undefined,
        port_path: preset.port_path.trim() || undefined,
        baud: Number(preset.baud) || undefined
      });
      setPresets((previous) => [...previous, saved]);
      setPresetError(null);
      setPresetDialog(false);
      setStatus(`Preset "${saved.name}" saved.`);
    } catch (cause) {
      setPresetError(messageOf(cause));
    }
  };

  const forgetPreset = async (saved: SavedConnection) => {
    await api.removeConnection(grant.deviceId, saved.id).catch(() => {});
    setPresets((previous) => previous.filter((entry) => entry.id !== saved.id));
  };

  const attach = async () => {
    try {
      const answer = JSON.parse(await callTool(grant, "terminal.list", {}));
      const sessions: { id: string; kind: string }[] = Array.isArray(answer) ? answer : [];
      setAvailable(sessions);
      setStatus(sessions.length === 0 ? "No open session on this device." : `${sessions.length} session(s) on this device.`);
    } catch (cause) {
      setStatus("List failed: " + messageOf(cause));
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
          <button type="button" onClick={() => setPresetDialog(true)}>Save a preset</button>
          <button type="button" className="primary" onClick={() => openDialog({}, null)}>+ New</button>
        </div>
      </div>
      <p className={status.startsWith("Could not") || status.startsWith("List failed") ? "error" : "hint"}>{status}</p>

      {presets.length > 0 && (
        <ul className="picker">
          {presets.map((saved) => (
            <li key={saved.id}>
              <span className="mono">{saved.kind}</span>
              <strong>{saved.name}</strong>
              <span className="mono muted">
                {saved.kind === "serial"
                  ? `${saved.port_path ?? "?"} ${saved.baud ?? ""}`
                  : saved.kind === "ssh"
                    ? `${saved.user ?? "?"}@${saved.host ?? "?"}:${saved.port ?? 22}`
                    : "local shell"}
              </span>
              <button type="button" className="mini" onClick={() => openPreset(saved)}>
                Open
              </button>
              <button type="button" className="mini" onClick={() => void forgetPreset(saved)}>Forget</button>
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

      {connect && (
        <Modal
          title={connect.name ? `Open ${connect.name}` : "New terminal session"}
          onClose={() => setConnect(null)}
        >
          <label htmlFor="connect-kind">Kind</label>
          <select
            id="connect-kind"
            value={connect.draft.kind}
            onChange={(event) => patchConnect({ kind: event.target.value as ConnectKind })}
          >
            <option value="pty">pty — a shell on the device</option>
            <option value="ssh">ssh — a remote host</option>
            <option value="serial">serial — a port on the device</option>
          </select>

          {connect.draft.kind === "ssh" && (
            <>
              <label htmlFor="connect-host">Host</label>
              <input
                id="connect-host"
                value={connect.draft.host}
                onChange={(event) => patchConnect({ host: event.target.value })}
                placeholder="db.internal"
              />
              <label htmlFor="connect-port">Port</label>
              <input
                id="connect-port"
                inputMode="numeric"
                value={connect.draft.port}
                onChange={(event) => patchConnect({ port: event.target.value })}
                placeholder="22"
              />
              <label htmlFor="connect-user">User</label>
              <input
                id="connect-user"
                value={connect.draft.user}
                onChange={(event) => patchConnect({ user: event.target.value })}
                placeholder="operator"
              />
              <fieldset className="auth-choice">
                <legend>Credential for this open</legend>
                <label className="check">
                  <input
                    type="radio"
                    name="connect-auth"
                    checked={connect.draft.auth === "password"}
                    onChange={() => patchConnect({ auth: "password" })}
                  />
                  Password
                </label>
                <label className="check">
                  <input
                    type="radio"
                    name="connect-auth"
                    checked={connect.draft.auth === "key"}
                    onChange={() => patchConnect({ auth: "key" })}
                  />
                  Key path
                </label>
              </fieldset>
              {connect.draft.auth === "password" ? (
                <>
                  <label htmlFor="connect-password">Password</label>
                  <input
                    id="connect-password"
                    type="password"
                    autoComplete="new-password"
                    spellCheck={false}
                    value={connect.draft.password}
                    onChange={(event) => patchConnect({ password: event.target.value })}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" && !connect.busy) void submitConnect();
                    }}
                  />
                  <p className="hint">
                    Sent with this open only. It is not written to the preset, to browser storage or to
                    the URL.
                  </p>
                </>
              ) : (
                <>
                  <label htmlFor="connect-key">Key path</label>
                  <input
                    id="connect-key"
                    spellCheck={false}
                    value={connect.draft.key_path}
                    onChange={(event) => patchConnect({ key_path: event.target.value })}
                    placeholder="/home/operator/.ssh/id_ed25519"
                  />
                  <p className="hint">
                    An absolute path on the device. The key is read there and is never sent to this page
                    or saved in a preset.
                  </p>
                </>
              )}
            </>
          )}

          {connect.draft.kind === "serial" && (
            <>
              <label htmlFor="connect-port-path">Port path</label>
              <input
                id="connect-port-path"
                value={connect.draft.port_path}
                onChange={(event) => patchConnect({ port_path: event.target.value })}
                placeholder="COM3 or /dev/ttyUSB0"
              />
              <div className="serial-framing">
                <div>
                  <label htmlFor="connect-baud">Baud</label>
                  <input
                    id="connect-baud"
                    inputMode="numeric"
                    value={connect.draft.baud}
                    onChange={(event) => patchConnect({ baud: event.target.value })}
                    placeholder="115200"
                  />
                </div>
                <div>
                  <label htmlFor="connect-parity">Parity</label>
                  <select
                    id="connect-parity"
                    value={connect.draft.parity}
                    onChange={(event) => patchConnect({ parity: event.target.value as ConnectDraft["parity"] })}
                  >
                    <option value="none">none</option>
                    <option value="odd">odd</option>
                    <option value="even">even</option>
                  </select>
                </div>
                <div>
                  <label htmlFor="connect-data-bits">Data bits</label>
                  <select
                    id="connect-data-bits"
                    value={connect.draft.data_bits}
                    onChange={(event) => patchConnect({ data_bits: event.target.value as ConnectDraft["data_bits"] })}
                  >
                    <option value="5">5</option>
                    <option value="6">6</option>
                    <option value="7">7</option>
                    <option value="8">8</option>
                  </select>
                </div>
                <div>
                  <label htmlFor="connect-stop-bits">Stop bits</label>
                  <select
                    id="connect-stop-bits"
                    value={connect.draft.stop_bits}
                    onChange={(event) => patchConnect({ stop_bits: event.target.value as ConnectDraft["stop_bits"] })}
                  >
                    <option value="1">1</option>
                    <option value="2">2</option>
                  </select>
                </div>
              </div>
              <p className="hint">
                115200 8N1 unless changed. The device opens exactly this framing; a rate or mode the
                port rejects is an error, never a fallback.
              </p>
            </>
          )}

          {connect.draft.kind === "pty" && (
            <p className="hint">A shell runs on the device as its own user. Nothing else is needed.</p>
          )}

          {connect.error && <p className="error">{connect.error}</p>}
          <div className="modal-actions">
            <button type="button" onClick={() => setConnect(null)}>Cancel</button>
            <button type="button" className="primary" disabled={connect.busy} onClick={() => void submitConnect()}>
              {connect.busy ? "Opening…" : "Open"}
            </button>
          </div>
        </Modal>
      )}

      {presetDialog && (
        <Modal title="Save a terminal preset" onClose={() => setPresetDialog(false)}>
          <p className="hint">A preset names a connection. It never stores a password or a key.</p>
          <label htmlFor="preset-name">Name</label>
          <input
            id="preset-name"
            value={preset.name}
            onChange={(event) => setPreset({ ...preset, name: event.target.value })}
            placeholder="the db host"
          />
          <label htmlFor="preset-kind">Kind</label>
          <select
            id="preset-kind"
            value={preset.kind}
            onChange={(event) => setPreset({ ...preset, kind: event.target.value as ConnectKind })}
          >
            <option value="pty">pty — a shell on the device</option>
            <option value="ssh">ssh — a remote host</option>
            <option value="serial">serial — a port on the device</option>
          </select>
          {preset.kind === "ssh" && (
            <>
              <label htmlFor="preset-host">Host</label>
              <input
                id="preset-host"
                value={preset.host}
                onChange={(event) => setPreset({ ...preset, host: event.target.value })}
                placeholder="db.internal"
              />
              <label htmlFor="preset-port">Port</label>
              <input
                id="preset-port"
                inputMode="numeric"
                value={preset.port}
                onChange={(event) => setPreset({ ...preset, port: event.target.value })}
                placeholder="22"
              />
              <label htmlFor="preset-user">User</label>
              <input
                id="preset-user"
                value={preset.user}
                onChange={(event) => setPreset({ ...preset, user: event.target.value })}
                placeholder="operator"
              />
              <p className="hint">
                Open asks for the password or key path when the session starts, so there is no
                credential field here — and nothing to leak if this file is read.
              </p>
            </>
          )}
          {preset.kind === "serial" && (
            <>
              <label htmlFor="preset-path">Port path</label>
              <input
                id="preset-path"
                value={preset.port_path}
                onChange={(event) => setPreset({ ...preset, port_path: event.target.value })}
                placeholder="COM3 or /dev/ttyUSB0"
              />
              <label htmlFor="preset-baud">Baud</label>
              <input
                id="preset-baud"
                inputMode="numeric"
                value={preset.baud}
                onChange={(event) => setPreset({ ...preset, baud: event.target.value })}
                placeholder="115200"
              />
            </>
          )}
          {presetError && <p className="error">{presetError}</p>}
          <div className="modal-actions">
            <button type="button" onClick={() => setPresetDialog(false)}>Cancel</button>
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
