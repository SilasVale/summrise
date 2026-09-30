// WorkspacesPage — ADD A WORKSPACE ON A REMOTE HOST, from the UI.
//
// WHY IT IS HERE AND NOT IN DSH. The first design put this form in DSH's own UI, in the
// `sidebar.workspaces.directoryFlow` slot the "add workspace" action renders. That route is blocked by a
// measured wall: a third-party package CAN ship a DSH client half, but the client-to-host channel is typert,
// whose method discovery reads a COMPILER-INJECTED prototype descriptor — hand-written JS would have to write
// that descriptor itself, and no plugin-facing route seam on DSH's HTTP server was found either.
//
// The panel has none of those problems and one advantage: it is served by the agent, so it is the SAME ORIGIN
// as the registry. Adding a workspace is one authenticated call, and this page is that call with a form on it.
//
// THE THREE STATES THAT MATTER, and none of them is an empty list pretending to be an answer:
//   * loading            — nobody has checked yet
//   * the agent refused  — its own words, verbatim: a refusal is the most useful thing on this page
//   * the agent is down  — SAID, because "no connections" and "nobody asked" are different facts
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

export function WorkspacesPage() {
  const [connections, setConnections] = useState<Connection[] | null>(null);
  const [mappings, setMappings] = useState<Mapping[] | null>(null);
  const [unreachable, setUnreachable] = useState<string | null>(null);
  const [connectionId, setConnectionId] = useState("");
  const [path, setPath] = useState("");
  const [outcome, setOutcome] = useState<{ ok: boolean; text: string } | null>(null);
  // THE PANEL'S OWN ACKNOWLEDGEMENT MECHANISM, not a hand-kept flag: `run` paints `data-busy` on the control
  // while the call is in flight, which is the only reason the design sweep can SEE that a press was
  // acknowledged. A private boolean would work and be invisible.
  const { busy, ack, run } = useAck();

  const load = useCallback(async () => {
    try {
      const [c, m] = await Promise.all([
        callApi("/api/workspace/connections"),
        callApi("/api/workspace/mappings"),
      ]);
      // A WORKSPACE IS A POSIX HOST: a pty or a serial line cannot be one, so offering them would be
      // offering a choice that cannot work.
      setConnections(((c?.connections ?? []) as Connection[]).filter((x) => x.kind === "ssh"));
      setMappings((m?.mappings ?? []) as Mapping[]);
      setUnreachable(null);
    } catch (e) {
      setUnreachable(e instanceof Error ? e.message : String(e));
      setConnections(null);
      setMappings(null);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const add = useCallback(async () => {
    if (!connectionId || !path.trim()) return;
    setOutcome(null);
    await run("add", async () => {
      try {
        const j = await callApi("/api/workspace/register", {
          method: "POST",
          body: JSON.stringify({ path: path.trim(), connection_id: connectionId }),
        });
        // THE AGENT'S OWN WORDS, verbatim. `workspace/unknown-connection` and its message are the whole reason
        // this form is usable: they say WHICH connection is missing, and a generic failure would not.
        setOutcome(
          j?.ok
            ? { ok: true, text: `registered ${path.trim()}` }
            : { ok: false, text: j?.error || j?.code || "the agent refused without saying why" },
        );
        await load();
      } catch (e) {
        setOutcome({ ok: false, text: e instanceof Error ? e.message : String(e) });
      }
    });
  }, [connectionId, path, load, run]);

  const remove = useCallback(
    async (p: string) => {
      setOutcome(null);
      await run(`remove:${p}`, async () => {
        try {
          const j = await callApi("/api/workspace/unregister", {
            method: "POST",
            body: JSON.stringify({ path: p }),
          });
          setOutcome(
            j?.ok
              ? { ok: true, text: j.removed ? `removed ${p}` : `${p} was not registered` }
              : { ok: false, text: j?.error || j?.code || "the agent refused without saying why" },
          );
          await load();
        } catch (e) {
          setOutcome({ ok: false, text: e instanceof Error ? e.message : String(e) });
        }
      });
    },
    [load, run],
  );

  return (
    <div className="workspaces-page">
      <h1 className="sr-only">Workspaces</h1>
      <p className="workspaces-lede">
        A workspace is a path on a host this device can reach. Files and commands for that path go to
        that host — the connection is one the agent already saved, and its credentials never leave the
        agent.
      </p>

      {unreachable ? (
        <div className="workspaces-unreachable" role="status">
          <strong>The agent did not answer</strong>
          <span className="workspaces-note">
            {unreachable} — so this page cannot say which connections exist. That is not the same fact as
            &ldquo;there are none&rdquo;.
          </span>
        </div>
      ) : null}

      <div className="workspaces-form">
        <label className="workspaces-field">
          <span>Connection</span>
          <select
            value={connectionId}
            onChange={(e) => setConnectionId(e.target.value)}
            aria-label="Connection"
          >
            <option value="">
              {connections === null
                ? "…"
                : connections.length === 0
                  ? "no saved ssh connection yet"
                  : "pick a saved ssh connection"}
            </option>
            {(connections ?? []).map((c) => (
              <option key={c.id} value={c.id}>
                {c.label || c.target || c.id}
              </option>
            ))}
          </select>
        </label>
        <label className="workspaces-field">
          <span>Path on that host</span>
          <input
            value={path}
            onChange={(e) => setPath(e.target.value)}
            placeholder="/home/you/project"
            aria-label="Path on that host"
            spellCheck={false}
          />
        </label>
        <button
          className="btn btn-primary btn-mini"
          onClick={() => void add()}
          disabled={busy || !connectionId || !path.trim()}
          {...ack("add")}
        >
          Add workspace
        </button>
      </div>

      {outcome ? (
        <p className={outcome.ok ? "workspaces-outcome" : "workspaces-outcome error"} role="status">
          {outcome.text}
        </p>
      ) : null}

      <h2 className="workspaces-heading">Registered</h2>
      {mappings === null ? (
        <p className="workspaces-note">{unreachable ? "not read" : "reading…"}</p>
      ) : mappings.length === 0 ? (
        <p className="workspaces-note">None yet.</p>
      ) : (
        <ul className="workspaces-list">
          {mappings.map((m) => (
            <li key={m.path} className="workspaces-row">
              <code className="workspaces-path">{m.path}</code>
              <span className="workspaces-conn">{m.connection_id}</span>
              <button
                className="btn btn-mini"
                onClick={() => void remove(m.path)}
                disabled={busy}
                aria-label={`Remove ${m.path}`}
                {...ack(`remove:${m.path}`)}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
