import { useCallback, useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { callTool, ToolError, type Grant } from "./terminal";
import { createTerminalWriteBatcher, type TerminalWriteBatcher } from "./terminalWriteBatcher";
import { nextTabTitle, renameTab, titleFromInput, type Tab } from "./terminalTabs";
import { earliestCursor, readTranscript, transcriptFile } from "./terminalTranscript";
import { api, type SavedConnection } from "./api";
import { downloadFile } from "./download";
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
type Instance = {
  term: Terminal;
  fit: FitAddon;
};

/**
 * Why a tab stopped working, in the words that tell the three cases apart.
 *
 * `closed` is the device saying it no longer has the session; `unreachable` is the device not
 * answering; `grant` is permission being refused (the one that names the grant). `read` and `write`
 * are a single call failing while the device still lists the session, and they say which half failed.
 */
type FaultKind = "closed" | "unreachable" | "grant" | "read" | "write";

type Fault = {
  kind: FaultKind;
  /** The full sentence the tab shows; the marker is only its short label. */
  sentence: string;
};

const FAULT_LABELS: Record<FaultKind, string> = {
  closed: "closed",
  unreachable: "offline",
  grant: "grant",
  read: "read failed",
  write: "not sent"
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

/** A session id as a file stem: ids are usually safe, but a name from the device is still its data. */
function safeSession(session: string): string {
  return session.replace(/[^A-Za-z0-9._-]+/g, "_") || "session";
}

/**
 * The sentence a grant refusal deserves, or `null` when the failure was not one.
 *
 * Two surfaces refuse a call for grant reasons and they answer differently: the console checks the
 * signature and the expiry before the frame leaves (HTTP 403 with a code), and the device checks the
 * same grant again (a JSON-RPC error, code -32001 or -32002). Both are named here so a page never has
 * to match English to know which of the three failures it is.
 */
function grantRefusal(cause: unknown): string | null {
  if (!(cause instanceof ToolError)) return null;
  if (cause.status === 403) {
    switch (cause.message) {
      case "grant_expired":
        return "Your grant expired: the console refused the call before it reached the device. Sign a new grant to resume.";
      case "invalid_grant_signature":
        return "Your grant's signature was refused. Sign a new grant to resume.";
      case "grant_device_mismatch":
        return "Your grant was signed for another device. Sign a new grant for this one.";
      case "tool not permitted by grant":
        return "Your grant does not cover this tool. Sign a grant that includes it.";
      default:
        return `Your grant refused the call: ${cause.message}. Sign a new grant to resume.`;
    }
  }
  if (cause.code === -32001) {
    return `Your grant refused the call: ${cause.message}. Sign a new grant to resume.`;
  }
  if (cause.code === -32002) {
    return `Your grant did not consent to this call: ${cause.message}. Sign a grant that does.`;
  }
  return null;
}

/** A failure that is the link, not the permission and not the session. */
function unreachableSentence(cause: unknown): string {
  if (cause instanceof ToolError && cause.status === 401) {
    return "The console's sign-in expired, so the call did not reach the device. Reload and sign in again.";
  }
  if (cause instanceof ToolError && cause.status === 404) {
    return "The console no longer has this device: it was removed from the fleet.";
  }
  return `The device is unreachable: ${messageOf(cause)}. The tab keeps its output and resumes when the device answers again.`;
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

export function TerminalWorkspace({
  grant,
  visible = true,
  onGrantRefused
}: {
  grant: Grant;
  visible?: boolean;
  onGrantRefused?: () => void;
}) {
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [faults, setFaults] = useState<Record<string, Fault>>({});
  const [status, setStatus] = useState("No session. Open one, or attach to a session this device already has.");
  const [available, setAvailable] = useState<{ id: string; kind: string }[]>([]);
  const [presets, setPresets] = useState<SavedConnection[]>([]);
  const [connect, setConnect] = useState<Connect | null>(null);
  const [renaming, setRenaming] = useState<{ key: string; title: string; error: string | null } | null>(null);
  const [presetDialog, setPresetDialog] = useState(false);
  const [preset, setPreset] = useState<Preset>({ ...NEW_PRESET });
  const [presetError, setPresetError] = useState<string | null>(null);
  const instances = useRef(new Map<string, Instance>());
  const batches = useRef(new Map<string, TerminalWriteBatcher>());
  const hosts = useRef(new Map<string, HTMLDivElement>());
  /** Each tab's read cursor: the absolute byte count the device has already sent this xterm. */
  const cursors = useRef(new Map<string, number>());
  /** The current fault per tab, readable synchronously inside a poll or a keystroke handler. */
  const faultRef = useRef(new Map<string, Fault>());
  /** When the last `terminal.list` probe ran, so a repeating refusal does not probe every 700 ms. */
  const probing = useRef(new Map<string, number>());
  const diagnosing = useRef(new Set<string>());
  const grantRef = useRef(grant);
  const grantRefusedRef = useRef(onGrantRefused);
  const activeRef = useRef<string | null>(null);
  const pollTimer = useRef<number | null>(null);

  const setFault = useCallback((key: string, fault: Fault) => {
    faultRef.current.set(key, fault);
    setFaults((previous) => ({ ...previous, [key]: fault }));
  }, []);

  const clearFault = useCallback((key: string) => {
    if (!faultRef.current.delete(key)) return;
    setFaults((previous) => {
      if (!(key in previous)) return previous;
      const next = { ...previous };
      delete next[key];
      return next;
    });
  }, []);

  useEffect(() => {
    grantRef.current = grant;
  }, [grant]);

  // A newly signed grant is the remedy a grant fault names, so a renewal clears those faults. This is
  // declared before the poll effect so the immediate poll after a renewal already sees them cleared.
  useEffect(() => {
    for (const [key, fault] of [...faultRef.current]) {
      if (fault.kind === "grant") {
        clearFault(key);
        probing.current.delete(key);
      }
    }
  }, [grant, clearFault]);

  useEffect(() => {
    grantRefusedRef.current = onGrantRefused;
  }, [onGrantRefused]);

  useEffect(() => {
    activeRef.current = active;
  }, [active]);

  /** One read at this tab's cursor, and the cursor it returns. */
  const readOnce = useCallback(async (tab: Tab): Promise<{ data?: string; cursor?: number }> => {
    const cursor = cursors.current.get(tab.key) ?? 0;
    return JSON.parse(await callTool(grantRef.current, "terminal.read", { id: tab.session, cursor }));
  }, []);

  /**
   * Ask the device once what it still has, and turn the answer into one of the three sentences.
   *
   * The probe is `terminal.list`: a session missing from it is `closed` (with the device's own
   * refusal as the reason when it gave one), a probe that fails is `unreachable`, and a refusal that
   * names the grant is `grant` — whichever of the two calls carried it.
   */
  const diagnose = useCallback(
    async (tab: Tab, source: "read" | "write", cause: unknown) => {
      const note = source === "write" ? " Your keystrokes were not sent." : "";
      const refused = grantRefusal(cause);
      if (refused) {
        setFault(tab.key, { kind: "grant", sentence: refused + note });
        grantRefusedRef.current?.();
        return;
      }
      if (diagnosing.current.has(tab.key)) return;
      diagnosing.current.add(tab.key);
      probing.current.set(tab.key, Date.now());
      try {
        const answer = JSON.parse(await callTool(grantRef.current, "terminal.list", {}));
        const sessions: { id?: string }[] = Array.isArray(answer) ? answer : [];
        if (!sessions.some((entry) => entry.id === tab.session)) {
          // Only a sentence the device itself sent is a reason it gave; a proxy failure is not one.
          const reason = cause instanceof ToolError && cause.fromDevice ? cause.message : null;
          setFault(tab.key, {
            kind: "closed",
            sentence: reason
              ? `The device closed this session: ${reason}.${note}`
              : `The device closed this session: it no longer lists ${tab.session}. The device closes a session after 30 minutes with no reads, writes or resizes.${note}`
          });
          return;
        }
        // The device still has the session, so one call failed. Only a cursor it refused as older
        // than the ring moves, to the earliest byte it still has; a transient failure leaves the
        // cursor where it was, so output the terminal already shows is never replayed.
        const earliest = source === "read" ? earliestCursor(messageOf(cause)) : null;
        if (earliest !== null) cursors.current.set(tab.key, earliest);
        setFault(
          tab.key,
          source === "write"
            ? { kind: "write", sentence: `Your keystrokes were not sent: ${messageOf(cause)}.` }
            : {
                kind: "read",
                sentence:
                  earliest !== null
                    ? `Reading this session failed: ${messageOf(cause)}. The tab continues from the earliest output the device retains.`
                    : `Reading this session failed: ${messageOf(cause)}. The tab keeps its place and tries again.`
              }
        );
      } catch (probeCause) {
        const probeRefused = grantRefusal(probeCause);
        if (probeRefused) {
          setFault(tab.key, { kind: "grant", sentence: probeRefused + note });
          grantRefusedRef.current?.();
        } else {
          setFault(tab.key, { kind: "unreachable", sentence: unreachableSentence(probeCause) + note });
        }
      } finally {
        diagnosing.current.delete(tab.key);
      }
    },
    [setFault]
  );

  /**
   * Record a failed read or write on its tab.
   *
   * The first failure of an episode sets the immediate sentence and probes once. Later failures keep
   * that episode's kind (so an unreachable tab is not relabelled by every retry) and only refresh the
   * sentence; the one exception is a device-spoken refusal, which may mean the session has since
   * closed, so it earns a new probe at most every ten seconds.
   */
  const fail = useCallback(
    (tab: Tab, source: "read" | "write", cause: unknown) => {
      const current = faultRef.current.get(tab.key);
      if (current && (current.kind === "closed" || current.kind === "grant")) return;
      // A grant refusal answers the question by itself, whatever the tab's previous fault was: the
      // permission is the reason, not the link the earlier sentence described.
      const refused = grantRefusal(cause);
      if (refused) {
        const note = source === "write" ? " Your keystrokes were not sent." : "";
        setFault(tab.key, { kind: "grant", sentence: refused + note });
        grantRefusedRef.current?.();
        return;
      }
      const detail = messageOf(cause);
      const note = source === "write" ? " Your keystrokes were not sent." : "";
      if (current) {
        if (current.kind === "unreachable") {
          setFault(tab.key, { kind: "unreachable", sentence: unreachableSentence(cause) + note });
        } else if (source === "write") {
          setFault(tab.key, { kind: "write", sentence: `Your keystrokes were not sent: ${detail}.` });
        } else {
          setFault(tab.key, { kind: "read", sentence: `Reading this session failed: ${detail}.` });
        }
        const spoke = cause instanceof ToolError && cause.fromDevice;
        const sinceProbe = Date.now() - (probing.current.get(tab.key) ?? 0);
        if (spoke && sinceProbe > 10_000) void diagnose(tab, source, cause);
        return;
      }
      setFault(
        tab.key,
        source === "write"
          ? { kind: "write", sentence: `Your keystrokes were not sent: ${detail}.` }
          : { kind: "read", sentence: `Reading this session failed: ${detail}.` }
      );
      void diagnose(tab, source, cause);
    },
    [diagnose, setFault]
  );

  /**
   * One keystroke, buffered into one MCP call.
   *
   * A failure is the tab's, not a console log's: the batch's error path produces the same sentence a
   * per-keystroke write did. The buffer lives per tab because each tab is a different session.
   */
  const write = useCallback(
    (tab: Tab, data: string) => {
      const current = faultRef.current.get(tab.key);
      if (current && (current.kind === "closed" || current.kind === "grant")) return;
      let batch = batches.current.get(tab.key);
      if (!batch) {
        batch = createTerminalWriteBatcher(
          async (chunk) => {
            await callTool(grantRef.current, "terminal.write", { id: tab.session, data: chunk });
            if (faultRef.current.get(tab.key)?.kind === "write") clearFault(tab.key);
          },
          (cause) => fail(tab, "write", cause)
        );
        batches.current.set(tab.key, batch);
      }
      batch.push(data);
    },
    [clearFault, fail]
  );

  const poll = useCallback(
    async (tab: Tab) => {
      const instance = instances.current.get(tab.key);
      if (!instance) return;
      const current = faultRef.current.get(tab.key);
      if (current && (current.kind === "closed" || current.kind === "grant")) return;
      try {
        const read = await readOnce(tab);
        if (read.data) instance.term.write(read.data);
        if (typeof read.cursor === "number") cursors.current.set(tab.key, read.cursor);
        // A read that works clears a fault about the link or about reading. It does not clear "your
        // keystrokes were not sent": that sentence stays until a write succeeds (or the tab is
        // retried), because a working read says nothing about the keystroke that was dropped.
        const fault = faultRef.current.get(tab.key);
        if (fault && (fault.kind === "unreachable" || fault.kind === "read")) clearFault(tab.key);
      } catch (cause) {
        fail(tab, "read", cause);
      }
    },
    [clearFault, fail, readOnce]
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
        void write(tab, data);
      });
      instances.current.set(tab.key, { term, fit });
      if (activeRef.current === null) {
        activeRef.current = tab.key;
        setActive(tab.key);
      }
    }
    for (const [key, instance] of [...instances.current]) {
      if (tabs.some((tab) => tab.key === key)) continue;
      batches.current.get(key)?.cancel();
      batches.current.delete(key);
      instance.term.dispose();
      instances.current.delete(key);
      cursors.current.delete(key);
      clearFault(key);
      probing.current.delete(key);
    }
  }, [tabs, write, clearFault]);

  // The active tab polls; switching polls once immediately so the tab is current when it appears.
  //
  // A page change does not unmount the workspace, so `visible` is what stops the poll instead: a
  // hidden tab must not keep a device session alive past its idle close. On return the effect runs
  // again, refits the terminal to a host that has a size again, and reads everything the device
  // retained since the cursor — which is what makes a live tab reappear without an Attach.
  useEffect(() => {
    if (pollTimer.current !== null) window.clearInterval(pollTimer.current);
    pollTimer.current = null;
    if (!active || !visible) return;
    const tab = tabs.find((entry) => entry.key === active);
    if (!tab) return;
    const instance = instances.current.get(active);
    if (instance) {
      try {
        instance.fit.fit();
      } catch {
        /* the host may not be measurable yet */
      }
      void callTool(grantRef.current, "terminal.resize", {
        id: tab.session,
        rows: instance.term.rows,
        cols: instance.term.cols
      }).catch(() => {});
      instance.term.focus();
      void poll(tab);
    }
    pollTimer.current = window.setInterval(() => {
      if (activeRef.current === active) void poll(tab);
    }, 250);
    return () => {
      if (pollTimer.current !== null) window.clearInterval(pollTimer.current);
      pollTimer.current = null;
    };
  }, [active, tabs, grant, visible, poll]);

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

  // The device switch that unmounts the workspace takes the xterms with it.
  useEffect(
    () => () => {
      for (const batch of batches.current.values()) batch.cancel();
      batches.current.clear();
      for (const instance of instances.current.values()) instance.term.dispose();
      instances.current.clear();
    },
    []
  );

  /**
   * Open one session and add its tab. Returns the device's sentence on failure, null on success.
   *
   * `title` is what the operator named the session (a preset's name); null means a generated label.
   * The label is chosen inside the updater so two opens in one render cannot pick the same number.
   */
  const openWith = async (args: Record<string, unknown>, title: string | null): Promise<string | null> => {
    try {
      const answer = JSON.parse(await callTool(grantRef.current, "terminal.create", args));
      const session = answer.id as string;
      const kind = String(args.kind);
      setTabs((previous) => [
        ...previous,
        { key: session, session, kind, title: title ?? nextTabTitle(kind, previous) }
      ]);
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
    const failure = await openWith(built.args, connect.name);
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
      // Only the chosen kind's fields leave the page: an ssh preset has no baud, a serial preset has
      // no host. The console's own validation would refuse the mismatch, one step later.
      const saved = await api.saveConnection(grant.deviceId, {
        name: preset.name.trim(),
        kind: preset.kind,
        host: preset.kind === "ssh" ? preset.host.trim() || undefined : undefined,
        port: preset.kind === "ssh" ? Number(preset.port) || undefined : undefined,
        user: preset.kind === "ssh" ? preset.user.trim() || undefined : undefined,
        port_path: preset.kind === "serial" ? preset.port_path.trim() || undefined : undefined,
        baud: preset.kind === "serial" ? Number(preset.baud) || undefined : undefined
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
      const answer = JSON.parse(await callTool(grantRef.current, "terminal.list", {}));
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
    setTabs((previous) => [...previous, { key: session, session, kind, title: nextTabTitle(kind, previous) }]);
    setActive(session);
    setAvailable((previous) => previous.filter((entry) => entry.id !== session));
    setStatus(`Attached to ${session}.`);
  };

  const close = async (tab: Tab) => {
    // Flush what was typed before the session goes away; a keystroke queued behind the close would
    // otherwise be sent to an id the device already ended.
    await batches.current.get(tab.key)?.flush();
    batches.current.get(tab.key)?.cancel();
    batches.current.delete(tab.key);
    await callTool(grantRef.current, "terminal.close", { id: tab.session }).catch(() => {});
    setTabs((previous) => previous.filter((entry) => entry.key !== tab.key));
    cursors.current.delete(tab.key);
    probing.current.delete(tab.key);
    clearFault(tab.key);
    if (activeRef.current === tab.key) {
      const next = tabs.find((entry) => entry.key !== tab.key) ?? null;
      setActive(next?.key ?? null);
    }
    setStatus(`Session ${tab.session} is closed.`);
  };

  /** Start a faulted tab again: drop the sentence and read once from the retained output. */
  const retry = (tab: Tab) => {
    clearFault(tab.key);
    probing.current.delete(tab.key);
    void poll(tab);
  };

  /**
   * Give the tab the operator is looking at a name of its own.
   *
   * This is a console label, not a device operation: no call leaves the page and the session id is
   * unchanged. An empty name is refused here rather than silently turning into the old title.
   */
  const submitRename = () => {
    if (!renaming) return;
    const title = titleFromInput(renaming.title);
    if (title === null) {
      setRenaming({ ...renaming, error: "A tab needs a name." });
      return;
    }
    setTabs((previous) => renameTab(previous, renaming.key, title));
    setActive(renaming.key);
    setRenaming(null);
    setStatus(`Renamed to "${title}".`);
  };

  const activeTab = tabs.find((tab) => tab.key === active) ?? null;
  const activeFault = activeTab ? faults[activeTab.key] : undefined;

  /**
   * Save what the device still retains of the active session.
   *
   * The export reads through its own calls from cursor 0, so it never touches `cursors.current`: the
   * tab's poll keeps reading from where the terminal's output ended, not from the export's start.
   */
  const exportLog = async () => {
    if (!activeTab) return;
    try {
      const transcript = await readTranscript(async (cursor) =>
        JSON.parse(await callTool(grantRef.current, "terminal.read", { id: activeTab.session, cursor }))
      );
      downloadFile(
        `${safeSession(activeTab.session)}.log`,
        transcriptFile(transcript, { session: activeTab.session, at: new Date() }),
        "text/plain"
      );
      const exported = Math.max(0, transcript.to - transcript.from);
      setStatus(
        transcript.dropped > 0
          ? `Exported ${exported} bytes; ${transcript.dropped} bytes are no longer retained by the device.`
          : `Exported ${exported} bytes.`
      );
    } catch (cause) {
      setStatus("Could not export: " + messageOf(cause));
    }
  };

  return (
    <div className="terminal-workspace">
      <div className="panel-head">
        <h2>Terminal</h2>
        <div className="actions">
          <button type="button" onClick={() => void attach()}>Attach to a session</button>
          <button type="button" onClick={() => setPresetDialog(true)}>Save a preset</button>
          <button
            type="button"
            disabled={!activeTab}
            onClick={() =>
              activeTab && setRenaming({ key: activeTab.key, title: activeTab.title, error: null })
            }
          >
            Rename tab
          </button>
          <button type="button" disabled={!activeTab} onClick={() => void exportLog()}>Export log</button>
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
              {faults[tab.key] && (
                <span
                  className={"tab-state state-" + faults[tab.key].kind}
                  title={faults[tab.key].sentence}
                >
                  {FAULT_LABELS[faults[tab.key].kind]}
                </span>
              )}
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

      {activeTab && activeFault && (
        <div className={"terminal-fault fault-" + activeFault.kind} role="alert">
          <p>{activeFault.sentence}</p>
          <div className="terminal-fault-actions">
            <span className="hint">
              {activeFault.kind === "closed" || activeFault.kind === "grant"
                ? "No keystrokes are sent from this tab while it is in this state."
                : `Tab ${activeTab.title}`}
            </span>
            {activeFault.kind !== "closed" && activeFault.kind !== "grant" && (
              <button type="button" className="mini" onClick={() => retry(activeTab)}>
                Try again
              </button>
            )}
          </div>
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

      {renaming && (
        <Modal title="Rename tab" onClose={() => setRenaming(null)}>
          <label htmlFor="rename-title">Tab name</label>
          <input
            id="rename-title"
            value={renaming.title}
            onChange={(event) => setRenaming({ ...renaming, title: event.target.value, error: null })}
            onKeyDown={(event) => {
              if (event.key === "Enter") submitRename();
            }}
          />
          <p className="hint">The name stays in this console. The device session keeps its own id.</p>
          {renaming.error && <p className="error">{renaming.error}</p>}
          <div className="modal-actions">
            <button type="button" onClick={() => setRenaming(null)}>Cancel</button>
            <button type="button" className="primary" onClick={() => submitRename()}>Save</button>
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
