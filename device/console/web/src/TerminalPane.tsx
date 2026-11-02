import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { callTool, type Grant } from "./terminal";
import { createTerminalWriteBatcher, type TerminalWriteBatcher } from "./terminalWriteBatcher";

// The device keeps a session after the page loses it, so this pane attaches to what is there rather
// than always opening another shell.
/**
 * The terminal's colours follow the page's scheme.
 *
 * xterm cannot read a CSS variable, so the two palettes are spelled out here — and they are the console's
 * own: the dark terminal is the page's surface, not a different black, so the pane reads as part of the
 * page in both themes.
 */
function terminalTheme() {
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  return dark
    ? { background: "#0e1319", foreground: "#e7edf4", cursor: "#bf3a0a", selectionBackground: "#2e3a47" }
    : { background: "#10151c", foreground: "#d5dde6", cursor: "#ff8a4c", selectionBackground: "#2a3540" };
}

export function TerminalPane({ grant }: { grant: Grant }) {
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const session = useRef<string | null>(null);
  const timer = useRef<number | null>(null);
  const batch = useRef<TerminalWriteBatcher | null>(null);
  const [status, setStatus] = useState("No session.");
  const [sessions, setSessions] = useState<{ id: string; kind: string }[]>([]);
  const [chosen, setChosen] = useState<string>("");
  const [kind, setKind] = useState<"pty" | "ssh" | "serial">("pty");

  const resize = () => {
    if (!term.current || !fit.current || !session.current) return;
    try {
      fit.current.fit();
    } catch {
      return;
    }
    void callTool(grant, "terminal.resize", {
      id: session.current,
      rows: term.current.rows,
      cols: term.current.cols
    }).catch(() => {});
  };

  const poll = async () => {
    if (!session.current || !term.current) return;
    try {
      const read = JSON.parse(await callTool(grant, "terminal.read", { id: session.current }));
      if (read.data) term.current.write(read.data);
    } catch {
      /* a failed poll is retried; the session id decides whether it is alive */
    }
  };

  const open = async (id?: string) => {
    if (!term.current) {
      term.current = new Terminal({
        convertEol: true,
        cursorBlink: true,
        fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
        fontSize: 13,
        theme: terminalTheme()
      });
      fit.current = new FitAddon();
      term.current.loadAddon(fit.current);
      term.current.open(host.current!);
      batch.current = createTerminalWriteBatcher(
        async (data) => {
          const current = session.current;
          if (!current) return;
          await callTool(grant, "terminal.write", { id: current, data });
        },
        (cause) => setStatus("Your keystrokes were not sent: " + (cause instanceof Error ? cause.message : String(cause)))
      );
      term.current.onData((data) => {
        if (!session.current) return;
        batch.current?.push(data);
      });
    }
    try {
      const sid = id ?? JSON.parse(await callTool(grant, "terminal.create", { kind })).id;
      session.current = sid;
      setStatus(`Session ${sid} is open.`);
      term.current.focus();
      resize();
      window.addEventListener("resize", resize);
      timer.current = window.setInterval(() => void poll(), 250);
      await poll();
    } catch (cause) {
      setStatus("Could not open: " + (cause instanceof Error ? cause.message : String(cause)));
    }
  };

  const close = async () => {
    if (!session.current) return;
    if (timer.current) window.clearInterval(timer.current);
    window.removeEventListener("resize", resize);
    // Flush what was typed before the session goes away; a keystroke queued behind the close would
    // otherwise be sent to an id the device already ended.
    await batch.current?.flush();
    batch.current?.cancel();
    const id = session.current;
    session.current = null;
    await callTool(grant, "terminal.close", { id }).catch(() => {});
    setStatus("Session closed.");
  };

  const list = async () => {
    try {
      const answer = JSON.parse(await callTool(grant, "terminal.list", {}));
      setSessions(answer);
      setStatus(answer.length === 0 ? "No open session on this device." : `${answer.length} session(s).`);
      if (answer.length > 0) setChosen(answer[0].id);
    } catch (cause) {
      setStatus("List failed: " + (cause instanceof Error ? cause.message : String(cause)));
    }
  };

  useEffect(() => () => {
    if (timer.current) window.clearInterval(timer.current);
    window.removeEventListener("resize", resize);
    batch.current?.cancel();
    term.current?.dispose();
  }, []);

  return (
    <div className="terminal-pane">
      <div className="panel-head">
        <h2>Terminal</h2>
        <div className="actions">
          <select value={kind} onChange={(event) => setKind(event.target.value as typeof kind)}>
            <option value="pty">pty — a shell on the device</option>
            <option value="ssh">ssh — a remote host</option>
            <option value="serial">serial — a port on the device</option>
          </select>
          <button type="button" onClick={() => void list()}>List sessions</button>
          <button type="button" className="primary" onClick={() => void open(chosen || undefined)}>
            {chosen ? "Attach" : "Open session"}
          </button>
          <button type="button" onClick={() => void close()}>Close</button>
        </div>
      </div>
      <p className={status.startsWith("Could not") ? "error" : "hint"}>{status}</p>
      {sessions.length > 0 && (
        <select value={chosen} onChange={(event) => setChosen(event.target.value)}>
          {sessions.map((entry) => (
            <option key={entry.id} value={entry.id}>{entry.kind} · {entry.id}</option>
          ))}
        </select>
      )}
      <div className="term-host" ref={host} />
    </div>
  );
}
