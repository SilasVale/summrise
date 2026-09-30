// WorkspacesPanel — ADD A LINUX MACHINE, then every path on it is a workspace.
//
// IT LIVES INSIDE THE HARNESS PAGE, not on a rail entry of its own: the operator said it plainly ("那个 WorkSpaces
// 一定要吗，不能都放在 harness 面板里面吗"), and they are right — a workspace is what the harness works IN, so
// managing one belongs where the harness is. The panel is collapsed by default so the embedded harness keeps the
// page's height, and expanding it shrinks the slot, which the pane re-reports on its own.
//
// THE OPERATOR'S OWN MODEL, twice over. First: "工作区不能添加linux主机啊" — the page could only pick from
// connections the agent had ALREADY saved, so a host nobody had opened a terminal to could not be added at all.
// Now the page takes a host, a user and a credential, connects ONCE (a successful `terminal_open` is what saves a
// connection), closes that session, and registers the machine.
//
// Second: "添加linux机器后里面的工作区可以任意添加" — and the store already works that way. A mapping is a PATH
// PREFIX and the longest matching prefix wins, so registering `/` against a connection makes EVERY path on that
// host resolvable. Measured on the device: after registering `/`, `/tmp`, `/etc` and
// `/home/zhengsaisi/summrise/agent` all ran, none of them registered individually.
//
// WHY THE PANEL AND NOT DSH'S OWN UI: DSH's client half reaches its host half through typert, whose method
// discovery reads a compiler-injected prototype descriptor, and no plugin-facing route seam on DSH's HTTP server
// was found. The panel is served BY the agent, so it is the same origin as the registry.
//
// AND THE HONEST LIMIT, said on the page rather than implied: this page decides WHICH HOST a path lives on. The
// workspace list inside DSH is DSH's own — its picker is switched to the in-app browser variant on this device, so
// "add workspace" there works, and the paths below are what it can offer.
import { useCallback, useEffect, useState } from "react";
import { callApi } from "../lib/api";
import { useAck } from "../lib/useAck";

interface Connection {
  id: string;
  kind?: string;
  label?: string;
  target?: string;
}

interface Mapping {
  path: string;
  connection_id: string;
}

interface Entry {
  name: string;
  kind?: string;
}

/** A machine's registered root: the prefix that makes everything under it resolvable. */
const MACHINE_ROOT = "/";

/**
 * `onOpenTerminal` is how this page ASKS FOR a terminal instead of growing one. The terminal UI is a
 * whole page already (`TerminalWorkspace`), and a second one here would be a second thing to keep alive;
 * what this page knows is WHICH MACHINE, and the shell knows how to show it.
 */
/**
 * The machines, as a STRIP above the harness page's own view, and one machine opened in its place.
 *
 * WHY A STRIP AND NOT A RAIL, and the measurement behind it: `ContextRail` exists in the PANEL density
 * only — `DesktopShell` draws an 84px icon rail and a canvas, and no right-hand side at all, so a rail
 * entry for the machines renders in one shell and is absent from the other, which is the shell the
 * operator is actually looking at. A strip under the header is in BOTH, and it sits OUTSIDE the
 * `DshPage` slot, so the harness's native WebContentsView cannot cover it: that view is composited over
 * whatever rectangle the pane reports, and the pane is the canvas.
 *
 * So: the strip is always there (machines + "Add a host"), and opening a machine REPLACES the harness
 * view with that machine's files, editor, command line and terminal link — the page is the harness, and
 * this is what the harness works on. `onOpenedChange` tells the shell which of the two to show, because
 * the native view's visibility belongs to the shell.
 */
export function WorkspacesPanel({
  onOpenTerminal,
  onOpenedChange,
}: {
  onOpenTerminal?: (sessionId: string) => void;
  /** True while a machine is open, so the shell can stand the harness's native view down. */
  onOpenedChange?: (open: boolean) => void;
} = {}) {
  const [machines, setMachines] = useState<Connection[] | null>(null);
  const [mappings, setMappings] = useState<Mapping[] | null>(null);
  const [unreachable, setUnreachable] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<{ ok: boolean; text: string } | null>(null);
  const [browsing, setBrowsing] = useState<{ connectionId: string; path: string; entries: Entry[] } | null>(null);
  // WHICH VIEW: the machines, or ONE machine opened. A page that scrolls from a form into a listing makes
  // the reader hunt; a view makes "which machine am I in" a fact of the page, not of the scroll position.
  const [opened, setOpened] = useState<{ id: string; path: string } | null>(null);
  const [showAdd, setShowAdd] = useState(false);
  // The open file: the VERSION is what a save must present, so it is state and not a local.
  const [editing, setEditing] = useState<{ path: string; version: string; text: string; original: string } | null>(null);
  const [command, setCommand] = useState("");
  const [ran, setRan] = useState<{ text: string; status: string; failed: boolean; cut: boolean } | null>(null);
  const [host, setHost] = useState("");
  const [user, setUser] = useState("");
  const [port, setPort] = useState("22");
  const [keyPath, setKeyPath] = useState("");
  const [password, setPassword] = useState("");
  // THE PANEL'S OWN ACKNOWLEDGEMENT MECHANISM, not a hand-kept flag: `run`/`ack` paint `data-busy` while a call
  // is in flight, which is the only reason the design sweep can SEE that a press was acknowledged.
  const { busy, ack, run } = useAck();

  const load = useCallback(async () => {
    try {
      const [c, m] = await Promise.all([
        callApi("/api/workspace/connections"),
        callApi("/api/workspace/mappings"),
      ]);
      // A 200 WITH `ok:false` IS A REFUSAL, NOT AN EMPTY LIST. A build without the terminal feature answers
      // exactly that (`internal`), and drawing it as "no saved ssh connection yet" is the status-lie this
      // repository's own rule names: an empty list is a claim that nobody checked.
      if (c?.ok === false || m?.ok === false) {
        setUnreachable(c?.error || m?.error || c?.code || m?.code || "the agent refused without saying why");
        setMachines(null);
        setMappings(null);
        return;
      }
      // A WORKSPACE IS A POSIX HOST: a pty or a serial line cannot be one.
      setMachines(((c?.connections ?? []) as Connection[]).filter((x) => x.kind === "ssh"));
      setMappings((m?.mappings ?? []) as Mapping[]);
      setUnreachable(null);
    } catch (e) {
      setUnreachable(e instanceof Error ? e.message : String(e));
      setMachines(null);
      setMappings(null);
    }
  }, []);

  // THE SHELL OWNS THE NATIVE VIEW, so it needs to know which of the two it is standing down.
  useEffect(() => {
    onOpenedChange?.(opened !== null);
  }, [opened, onOpenedChange]);

  useEffect(() => {
    void load();
  }, [load]);

  const register = useCallback(
    async (key: string, connectionId: string, path: string, what: string) => {
      await run(key, async () => {
        const j = await callApi("/api/workspace/register", {
          method: "POST",
          body: JSON.stringify({ path, connection_id: connectionId }),
        });
        // THE AGENT'S OWN WORDS, verbatim: `workspace/unknown-connection` says WHICH connection is missing.
        setOutcome(
          j?.ok
            ? { ok: true, text: `${what} — ${path}` }
            : { ok: false, text: j?.error || j?.code || "the agent refused without saying why" },
        );
        await load();
      });
    },
    [load, run],
  );

  const unregister = useCallback(
    async (path: string) => {
      await run(`remove:${path}`, async () => {
        const j = await callApi("/api/workspace/unregister", {
          method: "POST",
          body: JSON.stringify({ path }),
        });
        setOutcome(
          j?.ok
            ? { ok: true, text: j.removed ? `removed ${path}` : `${path} was not registered` }
            : { ok: false, text: j?.error || j?.code || "the agent refused without saying why" },
        );
        await load();
      });
    },
    [load, run],
  );

  const listDir = useCallback(
    async (connectionId: string, path: string) => {
      await run(`browse:${path}`, async () => {
        const j = await callApi("/api/workspace/fs", {
          method: "POST",
          body: JSON.stringify({ op: "listDir", path }),
        });
        if (j?.ok) setBrowsing({ connectionId, path, entries: (j.entries ?? []) as Entry[] });
        else setOutcome({ ok: false, text: j?.error || j?.code || "the agent refused without saying why" });
      });
    },
    [run],
  );

  /**
   * OPEN A TERMINAL ON THIS MACHINE. The credentials are not asked for again: adding the machine saved a
   * connection, and `terminal_connect_saved` reconnects it by the id this page already holds. A refusal
   * is the agent's own sentence — an unknown id is answered with the id it did not find and the list of
   * ones it has, which is exactly what a reader needs.
   */
  const openTerminal = useCallback(
    (connectionId: string) =>
      run(`terminal:${connectionId}`, async () => {
        const j = await callApi("/api/tools/terminal_connect_saved", {
          method: "POST",
          body: JSON.stringify({ id: connectionId }),
        });
        if (!j?.ok) {
          setOutcome({ ok: false, text: j?.error || j?.code || "the agent refused without saying why" });
          return;
        }
        const sessionId = typeof j.result === "string" ? j.result : j?.result?.session_id;
        if (!sessionId) {
          setOutcome({ ok: false, text: "the agent opened a terminal but did not say which session it is" });
          return;
        }
        setOutcome({ ok: true, text: `terminal ${sessionId} opened on this machine` });
        onOpenTerminal?.(String(sessionId));
      }),
    [onOpenTerminal, run],
  );

  /**
   * THE MACHINE'S OWN PREFIX — the LONGEST path registered against its connection, whatever it is.
   * Looking for "/" specifically was a bug the rewrite would have shipped: the prefix is DERIVED (a
   * machine's home, see addMachine), so a lookup keyed on the old constant would never match and every
   * machine would read as "not added" with its Open control disabled.
   */
  const rootOf = (id: string) =>
    (mappings ?? [])
      .filter((m) => m.connection_id === id)
      .sort((a, b) => b.path.length - a.path.length)[0];

  /**
   * OPEN A MACHINE FROM ITS CHIP. A saved connection with no workspace prefix is the one state that
   * cannot be opened, and the chip says which it is (`not added`), so the click does the missing step
   * and says what it did rather than refusing with a button the reader has to find.
   */
  const openMachine = useCallback(
    (m: Connection) => {
      const root = rootOf(m.id);
      if (!root) {
        setShowAdd(false);
        void register(`add:${m.id}`, m.id, MACHINE_ROOT, "added this machine");
        return;
      }
      setOpened({ id: m.id, path: root.path });
      setEditing(null);
      setRan(null);
      void listDir(m.id, root.path);
    },
    [listDir, register, rootOf],
  );

  /** Open one file: the READ is what produces the version a save must carry. */
  const openFile = useCallback(
    (connectionId: string, path: string) =>
      run(`open:${path}`, async () => {
        const j = await callApi("/api/workspace/fs", {
          method: "POST",
          body: JSON.stringify({ op: "readText", path, connection_id: connectionId }),
        });
        if (!j?.ok) {
          setOutcome({ ok: false, text: j?.error || j?.code || "the agent refused without saying why" });
          return;
        }
        setEditing({ path, version: String(j.version ?? ""), text: String(j.text ?? ""), original: String(j.text ?? "") });
      }),
    [run],
  );

  /**
   * SAVE. `expect_version` is the whole guard: a write that cannot say what it is replacing is the silent
   * loss the door refuses, so the page carries the version the READ returned. A conflict is the agent's
   * own sentence, shown as it wrote it — and the editor KEEPS the text, because the reader's work is not
   * the thing that was stale.
   */
  const saveFile = useCallback(
    (connectionId: string) =>
      run(`save:${editing?.path ?? ""}`, async () => {
        if (!editing) return;
        const j = await callApi("/api/workspace/fs", {
          method: "POST",
          body: JSON.stringify({
            op: "writeText",
            path: editing.path,
            text: editing.text,
            expect_version: editing.version,
            connection_id: connectionId,
          }),
        });
        if (!j?.ok) {
          setOutcome({ ok: false, text: j?.error || j?.code || "the agent refused without saying why" });
          return;
        }
        setEditing({ ...editing, version: String(j.version ?? ""), original: editing.text });
        setOutcome({ ok: true, text: `saved ${editing.path}` });
      }),
    [editing, run],
  );

  /** Run one command ON the machine, through the exec door. An argv, never a shell string. */
  const runCommand = useCallback(
    (connectionId: string) =>
      run("command", async () => {
        const argv = command.trim().split(/\s+/).filter(Boolean);
        if (argv.length === 0) return;
        const j = await callApi("/api/workspace/exec", {
          method: "POST",
          body: JSON.stringify({ path: opened?.path ?? MACHINE_ROOT, connection_id: connectionId, argv }),
        });
        if (!j?.ok) {
          setRan({ text: j?.error || j?.code || "the agent refused without saying why", status: "", failed: true, cut: false });
          return;
        }
        const out = j?.stdout_b64 ? atob(String(j.stdout_b64)) : String(j?.stdout ?? "");
        const err = j?.stderr_b64 ? atob(String(j.stderr_b64)) : String(j?.stderr ?? "");
        // THE DOOR ANSWERS `ok` FOR THE CALL AND `status` FOR THE COMMAND, and they are different facts:
        // `ok:true` with `{kind:'exited', code:2}` is a call that worked reporting a command that did not.
        // Reading only `ok` printed a failed command's output as though it had succeeded.
        const status = j?.status ?? {};
        const code = Number.isInteger(status.code) ? status.code : null;
        const failed = status.kind === "signalled" || (status.kind === "exited" && code !== 0) || status.kind === "unknown";
        const how =
          status.kind === "exited"
            ? `exit ${code ?? "?"}`
            : status.kind === "signalled"
              ? `signalled ${status.name ?? "?"}`
              : "the host did not say how it ended";
        setRan({
          text: `${out}${err}`.trim() || "(no output)",
          status: how,
          failed,
          // A TRUNCATED STREAM IS REPORTED LOSSY: the door says so with a flag, and a page that dropped it
          // would show a cut stream as the whole answer.
          cut: Boolean(j?.stdout_overflow || j?.stderr_overflow),
        });
      }),
    [command, opened, run],
  );

  /**
   * ADD A MACHINE. A successful `terminal_open` is what SAVES a connection, so this connects once with the
   * credentials given, closes that session again, and registers the machine. The connect is not a formality: it
   * is the only thing that proves the credentials work before a workspace depends on them.
   */
  const addMachine = useCallback(async () => {
    const h = host.trim();
    const u = user.trim();
    const p = port.trim() || "22";
    if (!h || !u) return;
    await run("add-machine", async () => {
      const target = `${u}@${h}:${p}`;
      try {
        // THE LITERAL TOOL ROUTE, which is how this panel already calls tools (ConnModal, UpdateCard) and the
        // only form the route-coverage gate can see. A successful open is what SAVES the connection.
        const opened = await callApi("/api/tools/terminal_open", {
          method: "POST",
          body: JSON.stringify({
            kind: "ssh",
            target,
            ...(keyPath.trim() ? { key_path: keyPath.trim() } : {}),
            ...(password ? { password } : {}),
            rows: 24,
            cols: 80,
          }),
        });
        if (opened && opened.ok === false) {
          setOutcome({ ok: false, text: opened.error || "the agent refused the connection" });
          return;
        }
        const res = opened?.result ?? opened;
        const sid = typeof res === "string" ? res : res?.session_id || res?.sessionId || res?.id || null;
        if (sid) {
          try {
            await callApi("/api/tools/terminal_close", {
              method: "POST",
              body: JSON.stringify({ session_id: sid }),
            });
          } catch {
            /* a session left open is not a reason to fail the add */
          }
        }
        const id = `ssh:${target}`;
        // THE PREFIX IS DERIVED, NOT ASKED FOR. A machine's home is where its work is — and it
        // steps around a real collision: a path-keyed registry can give "/" to only ONE machine,
        // which this loop measured the hard way. `printenv` rather than a shell, because the exec
        // door takes an argv and never a shell string.
        let prefix = MACHINE_ROOT;
        try {
          const home = await callApi("/api/workspace/exec", {
            method: "POST",
            body: JSON.stringify({ path: MACHINE_ROOT, connection_id: id, argv: ["printenv", "HOME"] }),
          });
          const text = home?.stdout_b64 ? atob(String(home.stdout_b64)).trim() : String(home?.stdout ?? "").trim();
          if (home?.ok && text.startsWith("/")) prefix = text;
        } catch {
          // A machine that will not say where its home is still gets its root: broader than precise
          // is worse than precise and better than none.
        }
        const j = await callApi("/api/workspace/register", {
          method: "POST",
          body: JSON.stringify({ path: prefix, connection_id: id }),
        });
        setOutcome(
          j?.ok
            ? { ok: true, text: `added ${target} — every path on it is a workspace now` }
            : { ok: false, text: j?.error || j?.code || "connected, but the registry refused it" },
        );
        setPassword("");
        await load();
      } catch (e) {
        // The agent's own words: an auth failure or an unreachable host says what it was.
        setOutcome({ ok: false, text: e instanceof Error ? e.message : String(e) });
      }
    });
  }, [host, user, port, keyPath, password, load, run]);

  // ── THE MACHINES VIEW: the list is the page, the form is behind a button ─────────────────────────
  // ONE STRIP, ABOVE BOTH VIEWS. The machines view and the open machine are an early-return pair, so a
  // strip written into each of them appears TWICE on the machines view — two "Add a host" buttons, one
  // of them inert. Holding it here makes the row part of the PAGE rather than of either view, which is
  // what it was when it was a list.
  const THE_STRIP = (
    <MachinesStrip
      machines={machines}
      unreachable={unreachable}
      showAdd={showAdd}
      opened={opened}
      rootOf={rootOf}
      onToggleAdd={() => setShowAdd((v) => !v)}
      onOpen={openMachine}
      onClose={() => setOpened(null)}
    />
  );

  if (opened === null) {
    return (
      <div className="workspaces-panel">
      {THE_STRIP}
        {/* NO H1 HERE, and that is the correction. The page's own sweep line was "h1 count 0, first-is-h1
            false", so this used to carry an sr-only one; then the machines moved into ContextRail, which
            renders inside the HARNESS page — whose DshPage already has its own sr-only "Harness" — and the
            same line became "h1 count 2". A page inside a page does not get a second h1: the sweep's rule is
            `h1Count !== 1 || !firstIsH1`, and the page it is judging is the Harness page now. */}
        <p className="workspaces-lede">
          A workspace host is a machine this device can work on. Add one once — the agent keeps the
          connection and the credentials, and every path on it becomes a workspace after that.
        </p>

        {unreachable ? (
          <div className="workspaces-unreachable" role="status">
            <strong>The agent did not answer</strong>
            <span className="workspaces-note">
              {unreachable} — so this page cannot say which machines exist. That is not the same fact as
              &ldquo;there are none&rdquo;.
            </span>
          </div>
        ) : null}

        {outcome ? (
          <p className={outcome.ok ? "workspaces-outcome" : "workspaces-outcome error"} role="status">
            {outcome.text}
          </p>
        ) : null}

        <div className="workspaces-view-head">
          {/* A LABEL, NOT A HEADING — and the sweep is what says so. `ContextRail` renders beside the
              canvas, so this markup comes BEFORE the page's own `<h1>`, and the rule is
              `heads[0].tagName === 'H1'`: the reading was `h1 count 1, first-is-h1 false` — the count
              right, the position wrong, because a heading here claimed to be the page's first. The rail
              already made this exact call for its own labels: `side-title` is a `<div>`, because
              "NOT a heading: this labels the side list, and as an `<h1>` it was the largest-level
              heading in the whole product at 13px". Same reason, same element. */}
        {/* No toggle here: THE STRIP owns "Add a host", and a second control on the same screen is
            two ways to do one thing. The strip's is the one that stays put while the two views
            swap. */}
        {showAdd ? (
          <button className="btn btn-mini" onClick={() => setShowAdd(false)} {...ack("cancel-add")}>
            Cancel adding a host
          </button>
        ) : null}
        </div>

        {showAdd ? (
          <div className="workspaces-form">
            <label className="workspaces-field">
              <span>Host</span>
              <input value={host} onChange={(e) => setHost(e.target.value)} placeholder="10.10.61.83" aria-label="Host" spellCheck={false} />
            </label>
            <label className="workspaces-field">
              <span>User</span>
              <input value={user} onChange={(e) => setUser(e.target.value)} placeholder="root" aria-label="User" spellCheck={false} />
            </label>
            <label className="workspaces-field">
              <span>Port</span>
              <input value={port} onChange={(e) => setPort(e.target.value)} aria-label="Port" inputMode="numeric" />
            </label>
            <label className="workspaces-field">
              <span>Private key path on THIS device</span>
              <input value={keyPath} onChange={(e) => setKeyPath(e.target.value)} placeholder="C:\\ProgramData\\Summrise\\key" aria-label="Private key path on THIS device" spellCheck={false} />
            </label>
            <label className="workspaces-field">
              <span>…or a password</span>
              <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} aria-label="…or a password" />
            </label>
            <button
              className="btn btn-primary btn-mini"
              onClick={() => void addMachine()}
              disabled={busy || !host.trim() || !user.trim()}
              {...ack("add-machine")}
            >
              Connect and add
            </button>
          </div>
        ) : null}

        {machines !== null && machines.length > 0 ? (
          <p className="workspaces-note">Pick a machine above to work on it.</p>
        ) : null}
      </div>
    );
  }

  // ── ONE MACHINE OPENED: its files, the open file, and a command line ─────────────────────────────
  const machine = (machines ?? []).find((m) => m.id === opened.id);
  const name = machine?.label || machine?.target || opened.id;
  return (
    <div className="workspaces-panel">
      {THE_STRIP}
      <div className="workspaces-view-head">
        <button className="btn btn-mini" onClick={() => setOpened(null)} {...ack("back")}>
          Back to machines
        </button>
        {(() => {
          const root = rootOf(opened.id);
          return root ? (
            <button
              className="btn btn-mini"
              onClick={() => void unregister(root.path)}
              disabled={busy}
              aria-label={`Remove ${opened.id}`}
              {...ack(`remove:${root.path}`)}
            >
              Remove this machine
            </button>
          ) : null;
        })()}
        {/* The same label-not-a-heading call, for the same reason. */}
        <div className="workspaces-heading">{name}</div>
        <code className="workspaces-path">{opened.path}</code>
      </div>

      {outcome ? (
        <p className={outcome.ok ? "workspaces-outcome" : "workspaces-outcome error"} role="status">
          {outcome.text}
        </p>
      ) : null}

      <div className="workspaces-browser">
        <div className="workspaces-crumbs">
          <button
            className="btn btn-mini"
            onClick={() => void listDir(opened.id, opened.path.replace(/\/[^/]+\/?$/, "") || "/")}
            disabled={busy || opened.path === "/"}
          >
            Up
          </button>
          <button
            className="btn btn-mini"
            onClick={() => void register(`pin:${opened.path}`, opened.id, opened.path, "pinned")}
            disabled={busy}
            aria-label={`Use ${opened.path}`}
            {...ack(`pin:${opened.path}`)}
          >
            Use this folder
          </button>
        </div>
        <ul className="workspaces-entries">
          {browsing && browsing.connectionId === opened.id
            ? browsing.entries
                .filter((e) => e.kind === "dir")
                .map((e) => (
                  <li key={e.name}>
                    <button
                      className="workspaces-entry-dir"
                      onClick={() => void listDir(opened.id, `${opened.path.replace(/\/$/, "")}/${e.name}`)}
                      disabled={busy}
                    >
                      {e.name}/
                    </button>
                  </li>
                ))
            : null}
          {browsing && browsing.connectionId === opened.id
            ? browsing.entries
                .filter((e) => e.kind !== "dir")
                .map((e) => (
                  <li key={e.name}>
                    <button
                      className="workspaces-entry-file"
                      onClick={() => void openFile(opened.id, `${opened.path.replace(/\/$/, "")}/${e.name}`)}
                      disabled={busy}
                    >
                      {e.name}
                    </button>
                  </li>
                ))
            : null}
        </ul>
      </div>

      {editing ? (
        <div className="workspaces-editor">
          <div className="workspaces-view-head">
            <code className="workspaces-path">{editing.path}</code>
            {editing.text !== editing.original ? (
              <span className="workspaces-note">edited</span>
            ) : null}
            <button
              className="btn btn-primary btn-mini"
              onClick={() => void saveFile(opened.id)}
              disabled={busy || editing.text === editing.original}
              {...ack("save")}
            >
              Save
            </button>
          </div>
          <textarea
            className="workspaces-editor-text"
            aria-label="File contents"
            rows={14}
            value={editing.text}
            onChange={(e) => setEditing({ ...editing, text: e.target.value })}
            spellCheck={false}
          />
        </div>
      ) : null}

      <div className="workspaces-view-head">
        <button
          className="btn btn-mini"
          onClick={() => void openTerminal(opened.id)}
          disabled={busy}
          {...ack("terminal")}
        >
          Open a terminal
        </button>
        <span className="workspaces-note">
          a session on this machine — the command line below runs one argv through the exec door
        </span>
      </div>

      <div className="workspaces-command">
        <label className="workspaces-field">
          <span>Run a command on this machine</span>
          <input
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            placeholder="uname -a"
            aria-label="Command"
            spellCheck={false}
            onKeyDown={(e) => {
              if (e.key === "Enter") void runCommand(opened.id);
            }}
          />
        </label>
        <button
          className="btn btn-mini"
          onClick={() => void runCommand(opened.id)}
          disabled={busy || !command.trim()}
          {...ack("run")}
        >
          Run
        </button>
      </div>
      {ran !== null ? (
        <>
          <p className={ran.failed ? "workspaces-run-status failed" : "workspaces-run-status"} role="status">
            {ran.failed ? "the command FAILED" : "the command exited"} — {ran.status}
            {ran.cut ? " · the output was CUT at the door's cap, so this is not all of it" : ""}
          </p>
          <pre className="workspaces-run-output">{ran.text}</pre>
        </>
      ) : null}
    </div>
  );
}

/**
 * THE MACHINES, AS A ROW. Each chip is one saved connection: its name, and whether it is mapped to a
 * workspace prefix (`ready`) or not (`not added`). A chip you click OPENS; an unmapped one you click
 * REGISTERS first, because that is the only thing missing before it can be worked on — the same two
 * states the old list spelled out with an `Open` button and a `Add this machine` button, with the
 * decision carried by the chip's own state rather than by two controls per row.
 *
 * `data-opened` on the open chip is what a sweep (and a reader of the DOM) can see without running one.
 */
function MachinesStrip({
  machines,
  unreachable,
  showAdd,
  opened,
  rootOf,
  onToggleAdd,
  onOpen,
  onClose,
}: {
  machines: Connection[] | null;
  unreachable: string | null;
  /** Whether a machine is registered, per connection id — the strip cannot ask the component. */
  rootOf: (id: string) => Mapping | undefined;
  showAdd: boolean;
  opened: { id: string; path: string } | null;
  onToggleAdd: () => void;
  onOpen: (m: Connection) => void;
  onClose: () => void;
}) {
  return (
    <div className="workspaces-strip" role="group" aria-label="Machines">
      <button
        className="btn btn-mini"
        onClick={onToggleAdd}
        aria-expanded={showAdd}
        data-opened={showAdd ? "yes" : "no"}
      >
        Add a host
      </button>
      {machines === null ? (
        <span className="workspaces-note">{unreachable ? "machines not read" : "reading machines…"}</span>
      ) : machines.length === 0 ? (
        <span className="workspaces-note">No machine yet.</span>
      ) : (
        machines.map((m) => {
          const name = m.label || m.target || m.id;
          const isOpen = opened?.id === m.id;
          // A chip that CANNOT be opened says so, and clicking it does the one step that is missing. The
          // old list spelled this as a second button ("Add this machine"); a chip's state carries it, and
          // the accessible name carries the state with it so a reader — or a test — can tell the two apart.
          const registered = rootOf(m.id) !== undefined;
          const state = registered ? (isOpen ? "open" : "ready") : "not added";
          return (
            <span key={m.id} className="workspaces-chip" data-opened={isOpen ? "yes" : "no"}>
              <button
                className="workspaces-chip-name"
                data-state={state}
                onClick={() => (isOpen ? onClose() : onOpen(m))}
                title={
                  isOpen
                    ? `${name} — click to close`
                    : registered
                      ? `Open ${name}`
                      : `Add ${name}`
                }
                aria-label={isOpen ? name : registered ? `Open ${name}` : `Add ${name}`}
              >
                {name}
                {!registered ? <span className="workspaces-chip-state"> not added</span> : null}
              </button>
              {isOpen ? (
                <button className="workspaces-chip-x" onClick={onClose} aria-label={`Close ${name}`}>
                  ×
                </button>
              ) : null}
            </span>
          );
        })
      )}
    </div>
  );
}
