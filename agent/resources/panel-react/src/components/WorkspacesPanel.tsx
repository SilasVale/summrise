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

export function WorkspacesPanel() {
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

  /** The mapping that makes a machine's whole filesystem resolvable, if it is registered. */
  /**
   * THE MACHINE'S OWN PREFIX — the LONGEST path registered against its connection, whatever it is.
   * Looking for "/" specifically was a bug the rewrite would have shipped: the prefix is now DERIVED
   * (a machine's home, see addMachine), so a lookup keyed on the old constant would never match and
   * every machine would read as "not added" with its Open button disabled.
   */
  const rootOf = (id: string) =>
    (mappings ?? [])
      .filter((m) => m.connection_id === id)
      .sort((a, b) => b.path.length - a.path.length)[0];

  // ── THE MACHINES VIEW: the list is the page, the form is behind a button ─────────────────────────
  if (opened === null) {
    return (
      <div className="workspaces-panel">
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
          <h2 className="workspaces-heading">Machines</h2>
          <button
            className="btn btn-primary btn-mini"
            onClick={() => setShowAdd((v) => !v)}
            aria-expanded={showAdd}
            {...ack("toggle-add")}
          >
            {showAdd ? "Cancel" : "Add a host"}
          </button>
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

        {machines === null ? (
          <p className="workspaces-note">{unreachable ? "not read" : "reading…"}</p>
        ) : machines.length === 0 ? (
          <p className="workspaces-note">No machine yet — add one.</p>
        ) : (
          <ul className="workspaces-machines">
            {machines.map((m) => {
              const root = rootOf(m.id);
              return (
                <li key={m.id} className="workspaces-machine">
                  <div className="workspaces-machine-head">
                    <code className="workspaces-machine-name">{m.label || m.target || m.id}</code>
                    {root ? (
                      <span className="workspaces-machine-state">ready</span>
                    ) : (
                      <span className="workspaces-machine-state">not added</span>
                    )}
                    <button
                      className="btn btn-primary btn-mini"
                      onClick={() => {
                        setOpened({ id: m.id, path: root?.path ?? MACHINE_ROOT });
                        setEditing(null);
                        setRan(null);
                        void listDir(m.id, root?.path ?? MACHINE_ROOT);
                      }}
                      disabled={busy || !root}
                      {...ack(`open:${m.id}`)}
                    >
                      Open
                    </button>
                    {root ? (
                      <button
                        className="btn btn-mini"
                        onClick={() => void unregister(root.path)}
                        disabled={busy}
                        aria-label={`Remove ${m.id}`}
                        {...ack(`remove:${root.path}`)}
                      >
                        Remove
                      </button>
                    ) : (
                      <button
                        className="btn btn-mini"
                        onClick={() => void register(`add:${m.id}`, m.id, MACHINE_ROOT, "added this machine")}
                        disabled={busy}
                        aria-label={`Add ${m.id}`}
                        {...ack(`add:${m.id}`)}
                      >
                        Add this machine
                      </button>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    );
  }

  // ── ONE MACHINE OPENED: its files, the open file, and a command line ─────────────────────────────
  const machine = (machines ?? []).find((m) => m.id === opened.id);
  const name = machine?.label || machine?.target || opened.id;
  return (
    <div className="workspaces-panel">
      <div className="workspaces-view-head">
        <button className="btn btn-mini" onClick={() => setOpened(null)} {...ack("back")}>
          Back to machines
        </button>
        <h2 className="workspaces-heading">{name}</h2>
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
            value={editing.text}
            onChange={(e) => setEditing({ ...editing, text: e.target.value })}
            spellCheck={false}
          />
        </div>
      ) : null}

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
