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
        const j = await callApi("/api/workspace/register", {
          method: "POST",
          body: JSON.stringify({ path: MACHINE_ROOT, connection_id: id }),
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
  const rootOf = (id: string) => (mappings ?? []).find((m) => m.connection_id === id && m.path === MACHINE_ROOT);
  const pathsOf = (id: string) =>
    (mappings ?? []).filter((m) => m.connection_id === id && m.path !== MACHINE_ROOT).map((m) => m.path);

  return (
    <div className="workspaces-panel">
      <p className="workspaces-lede">
        Add a Linux machine once — every path on it is a workspace after that. The connection is saved by the
        agent (the same store its terminals use), and the credentials never leave it.
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

      <h2 className="workspaces-heading">Add a machine</h2>
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

      <h2 className="workspaces-heading">Machines</h2>
      {machines === null ? (
        <p className="workspaces-note">{unreachable ? "not read" : "reading…"}</p>
      ) : machines.length === 0 ? (
        <p className="workspaces-note">None yet — add one above.</p>
      ) : (
        <ul className="workspaces-machines">
          {machines.map((m) => {
            const root = rootOf(m.id);
            const paths = pathsOf(m.id);
            return (
              <li key={m.id} className="workspaces-machine">
                <div className="workspaces-machine-head">
                  <code className="workspaces-machine-name">{m.label || m.target || m.id}</code>
                  {root ? (
                    <span className="workspaces-machine-state">
                      added — every path on it works
                      <button
                        className="btn btn-mini"
                        onClick={() => void unregister(MACHINE_ROOT)}
                        disabled={busy}
                        aria-label={`Remove ${m.id}`}
                        {...ack(`remove:${MACHINE_ROOT}`)}
                      >
                        Remove
                      </button>
                    </span>
                  ) : (
                    <button
                      className="btn btn-primary btn-mini"
                      onClick={() => void register(`add:${m.id}`, m.id, MACHINE_ROOT, "added this machine")}
                      disabled={busy}
                      aria-label={`Add ${m.id}`}
                      {...ack(`add:${m.id}`)}
                    >
                      Add this machine
                    </button>
                  )}
                </div>

                {root ? (
                  <>
                    {paths.length > 0 ? (
                      <ul className="workspaces-paths">
                        {paths.map((p) => (
                          <li key={p} className="workspaces-row">
                            <code className="workspaces-path">{p}</code>
                            <button
                              className="btn btn-mini"
                              onClick={() => void unregister(p)}
                              disabled={busy}
                              aria-label={`Remove ${p}`}
                              {...ack(`remove:${p}`)}
                            >
                              Remove
                            </button>
                          </li>
                        ))}
                      </ul>
                    ) : null}
                    <button
                      className="btn btn-mini"
                      onClick={() => void listDir(m.id, browsing?.connectionId === m.id ? browsing.path : "/")}
                      disabled={busy}
                      aria-label={`Browse ${m.id}`}
                    >
                      {browsing?.connectionId === m.id ? "Reload this folder" : "Browse…"}
                    </button>
                  </>
                ) : (
                  <p className="workspaces-browse-note">
                    Add the machine first — browsing it needs the path to resolve, and that is what adding it does.
                  </p>
                )}

                {browsing && browsing.connectionId === m.id ? (
                  <div className="workspaces-browser">
                    <div className="workspaces-crumbs">
                      <code className="workspaces-path">{browsing.path}</code>
                      <button
                        className="btn btn-mini"
                        onClick={() => void register(`pin:${browsing.path}`, m.id, browsing.path, "pinned")}
                        disabled={busy}
                        aria-label={`Use ${browsing.path}`}
                        {...ack(`pin:${browsing.path}`)}
                      >
                        Use this folder
                      </button>
                      {browsing.path !== "/" ? (
                        <button
                          className="btn btn-mini"
                          onClick={() => void listDir(m.id, browsing.path.replace(/\/[^/]+\/?$/, "") || "/")}
                          disabled={busy}
                        >
                          Up
                        </button>
                      ) : null}
                    </div>
                    <ul className="workspaces-entries">
                      {browsing.entries
                        .filter((e) => e.kind === "dir")
                        .map((e) => (
                          <li key={e.name}>
                            <button
                              className="workspaces-entry-dir"
                              onClick={() => void listDir(m.id, `${browsing.path.replace(/\/$/, "")}/${e.name}`)}
                              disabled={busy}
                            >
                              {e.name}/
                            </button>
                          </li>
                        ))}
                    </ul>
                  </div>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
