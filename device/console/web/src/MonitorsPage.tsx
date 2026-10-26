import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { message } from "./format";
import { Modal } from "./Modal";
import { NotificationsCard } from "./NotificationsCard";
import { useRequestGate } from "./requests";
import { callTool, type Grant } from "./terminal";

/** What a target's probe does: a TCP connect, or an HTTP GET of a path. */
type Kind = "tcp" | "http";

/**
 * One target as `monitor.list` returns it.
 *
 * `summary` is the device's summary at the moment of the list (the device attaches it); the page
 * asks `monitor.status` for the live one, because the summary changes while the list sits still.
 */
type Target = {
  id: string;
  kind: Kind;
  host: string;
  port: number;
  path?: string | null;
  expect?: string | null;
  interval_ms?: number;
  summary?: Status | null;
};

/**
 * One target's summary over the probes the device still holds.
 *
 * Every field is null before the first probe: a zero would read as "measured, stable" rather than
 * "not asked yet", and the card says the difference out loud.
 */
type Status = {
  up_now: boolean | null;
  since_ms: number | null;
  up_pct: number | null;
  drops: number | null;
  last_status: number | null;
};

const UNPROBED: Status = {
  up_now: null,
  since_ms: null,
  up_pct: null,
  drops: null,
  last_status: null
};

/// `monitor.add` refuses an interval shorter than this; the form's minimum is the same number.
const MIN_INTERVAL_MS = 5_000;
const DEFAULT_INTERVAL_MS = 60_000;

/// How often the page re-reads the summaries. The probe itself runs on the device's own timer;
/// this only refreshes what the operator sees.
const POLL_MS = 15_000;

/**
 * The device's reachability targets, as a page.
 *
 * Monitors are device state, so this reads and changes them through the same grant as the terminal,
 * the browser and memory: `monitor.list`, `monitor.status`, `monitor.add`, `monitor.probe` and
 * `monitor.remove`. There is no console route — every call is signed by the console, checked by the
 * device, and audited like the other panes' calls.
 *
 * The list polls while it is mounted, and a down target is painted as a fact (dot, state, the
 * summary's own numbers), never as an error page: the device answered, the target did not.
 */
export function MonitorsPage({ grant }: { grant: Grant }) {
  const [targets, setTargets] = useState<Target[]>([]);
  const [statuses, setStatuses] = useState<Record<string, Status>>({});
  const [note, setNote] = useState("Reading the device's targets…");
  const [error, setError] = useState<string | null>(null);
  const [pollError, setPollError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const [kind, setKind] = useState<Kind>("tcp");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [intervalMs, setIntervalMs] = useState(String(DEFAULT_INTERVAL_MS));
  const [path, setPath] = useState("");
  const [expect, setExpect] = useState("");
  const [adding, setAdding] = useState(false);

  const [probing, setProbing] = useState<string | null>(null);
  const [removing, setRemoving] = useState<Target | null>(null);
  const [deleting, setDeleting] = useState(false);

  // One status batch at a time, across effect lifetimes: a slow device must not queue probes
  // behind themselves. A tick that arrives while a batch is on the wire is skipped, not buffered.
  const pollInFlight = useRef(false);
  // Whether this grant has produced a list yet; a failed first read has nothing to keep showing,
  // while a failed refresh must leave the previous list where it is.
  const listed = useRef(false);
  // The list and the status poll paint different things (the targets and their summaries), so each
  // has its own ticket: a poll must never discard a list read that is still on the wire. A successful
  // read does restart the poll effect, whose cleanup invalidates a batch in flight; the replacement
  // tick is skipped while `pollInFlight` is set, so those summaries wait for the next tick. A probe
  // gets a third ticket: it writes one target's status and its own error, and must not thrash with
  // the poll's batch.
  const listGate = useRequestGate();
  const pollGate = useRequestGate();
  const probeGate = useRequestGate();

  const read = useCallback(async () => {
    const ticket = listGate.begin();
    setBusy(true);
    setError(null);
    try {
      const found = JSON.parse(await callTool(grant, "monitor.list", {})) as Target[];
      if (listGate.stale(ticket)) return;
      listed.current = true;
      setTargets(found);
      // The device answers each target with the summary it has; paint that before the first poll,
      // and keep the last known status when a target arrives without one.
      setStatuses((current) => {
        const next: Record<string, Status> = {};
        for (const target of found) next[target.id] = target.summary ?? current[target.id] ?? UNPROBED;
        return next;
      });
      setNote(listNote(found));
      setPollError(null);
    } catch (cause) {
      if (listGate.stale(ticket)) return;
      // The list stays as it was: a refresh that fails should not blank a list the operator can
      // still act on, and the sentence says what the device refused.
      if (!listed.current) setNote("The device's target list could not be read.");
      setError("Could not read the watched targets: " + message(cause));
    } finally {
      if (!listGate.stale(ticket)) setBusy(false);
    }
  }, [grant, listGate]);

  // A different grant is a different device's registry: its targets, its summaries, its draft.
  useEffect(() => {
    probeGate.begin();
    listed.current = false;
    setTargets([]);
    setStatuses({});
    setRemoving(null);
    setProbing(null);
    setError(null);
    setPollError(null);
    setKind("tcp");
    setHost("");
    setPort("");
    setIntervalMs(String(DEFAULT_INTERVAL_MS));
    setPath("");
    setExpect("");
    setNote("Reading the device's targets…");
    void read();
  }, [read, probeGate]);

  // Poll every target's summary while the list is showing. The effect restarts when the target set
  // changes, so an added target is asked about at once rather than at the next 15-second mark.
  useEffect(() => {
    if (targets.length === 0) return;
    const ids = targets.map((target) => target.id);

    const tick = async () => {
      if (pollInFlight.current) return;
      pollInFlight.current = true;
      const ticket = pollGate.begin();
      try {
        const results = await Promise.allSettled(
          ids.map(async (id) => {
            const text = await callTool(grant, "monitor.status", { id });
            return [id, JSON.parse(text) as Status] as const;
          })
        );
        if (pollGate.stale(ticket)) return;
        const fresh: Record<string, Status> = {};
        let failure: string | null = null;
        for (const result of results) {
          if (result.status === "fulfilled") {
            const [id, status] = result.value;
            fresh[id] = status;
          } else if (failure === null) {
            failure = message(result.reason);
          }
        }
        if (Object.keys(fresh).length > 0) setStatuses((current) => ({ ...current, ...fresh }));
        setPollError(failure === null ? null : "Could not read a target's status: " + failure);
      } catch (cause) {
        // `allSettled` already turned every device refusal into a result; this is the belt for the
        // one thing it cannot catch (a bug in the mapping), so a poll never leaves a rejection loose.
        if (!pollGate.stale(ticket)) {
          setPollError("Could not read a target's status: " + message(cause));
        }
      } finally {
        pollInFlight.current = false;
      }
    };

    void tick();
    const timer = window.setInterval(() => void tick(), POLL_MS);
    return () => {
      // Leaving the effect (unmount, a different grant, a different target set) ends the epoch: a
      // batch on the wire belongs to a list this page no longer shows.
      pollGate.begin();
      window.clearInterval(timer);
    };
  }, [grant, targets, pollGate]);

  const add = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const wantedHost = host.trim();
    const wantedPort = Number(port);
    const wantedInterval = Number(intervalMs);
    // `path` and `expect` are http-only. The device refuses them on a tcp target — not sending them
    // is the contract, not an omission the device would tolerate.
    const args: Record<string, unknown> = {
      kind,
      host: wantedHost,
      port: wantedPort,
      interval_ms: wantedInterval
    };
    if (kind === "http") {
      if (path.trim() !== "") args.path = path.trim();
      if (expect.trim() !== "") args.expect = expect.trim();
    }

    setAdding(true);
    setError(null);
    try {
      await callTool(grant, "monitor.add", args);
      setHost("");
      setPort("");
      setPath("");
      setExpect("");
      await read();
    } catch (cause) {
      setError("Could not add " + (wantedHost ? wantedHost + ":" + wantedPort : "the target") + ": " + message(cause));
    } finally {
      setAdding(false);
    }
  };

  const probe = async (target: Target) => {
    const ticket = probeGate.begin();
    setProbing(target.id);
    setError(null);
    try {
      await callTool(grant, "monitor.probe", { id: target.id });
      // A newer probe (or a new grant) makes the rest of this one pointless: do not read a summary
      // for a card the newest probe already owns.
      if (probeGate.stale(ticket)) return;
      // `monitor.probe` answers with the sample, but the card shows the summary (since, drops,
      // up_pct), so read that back. A probe that ran is a fact even if this second read fails.
      const fresh = JSON.parse(
        await callTool(grant, "monitor.status", { id: target.id })
      ) as Status;
      if (probeGate.stale(ticket)) return;
      setStatuses((current) => ({ ...current, [target.id]: fresh }));
    } catch (cause) {
      if (probeGate.stale(ticket)) return;
      setError("Could not probe " + where(target) + ": " + message(cause));
    } finally {
      // Only the newest probe owns the button: an earlier answer must not clear a later probe's
      // "Probing…" or make a third one clickable while it is still on the wire.
      if (!probeGate.stale(ticket)) setProbing(null);
    }
  };

  const remove = async () => {
    if (!removing) return;
    const target = removing;
    setDeleting(true);
    setError(null);
    try {
      await callTool(grant, "monitor.remove", { id: target.id });
      setTargets((current) => current.filter((item) => item.id !== target.id));
      setStatuses((current) => {
        const next = { ...current };
        delete next[target.id];
        return next;
      });
      setRemoving(null);
      await read();
    } catch (cause) {
      setError("Could not remove " + where(target) + ": " + message(cause));
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div className="monitors-page">
      <div className="panel-head">
        <h2>Monitors</h2>
        <div className="actions">
          <button type="button" disabled={busy} onClick={() => void read()}>
            {busy ? "Reading…" : "Refresh"}
          </button>
        </div>
      </div>
      <p className="hint">
        The device probes each target on its own interval and keeps the last 60 probes for the
        summary. A down target is a fact, not a failure of this page: the dot is its newest probe,
        and the numbers are the device's.
      </p>

      <form className="monitors-add" onSubmit={(event) => void add(event)}>
        <div className="monitors-field kind">
          <label htmlFor="monitor-kind">Kind</label>
          <select id="monitor-kind" value={kind} onChange={(event) => setKind(event.target.value as Kind)}>
            <option value="tcp">tcp</option>
            <option value="http">http</option>
          </select>
        </div>
        <div className="monitors-field host">
          <label htmlFor="monitor-host">Host</label>
          <input
            id="monitor-host"
            value={host}
            onChange={(event) => setHost(event.target.value)}
            placeholder="192.168.1.1 or example.com"
            autoComplete="off"
            spellCheck={false}
            required
          />
        </div>
        <div className="monitors-field port">
          <label htmlFor="monitor-port">Port</label>
          <input
            id="monitor-port"
            type="number"
            min={1}
            max={65535}
            value={port}
            onChange={(event) => setPort(event.target.value)}
            placeholder="443"
            required
          />
        </div>
        <div className="monitors-field interval">
          <label htmlFor="monitor-interval">Interval (ms)</label>
          <input
            id="monitor-interval"
            type="number"
            min={MIN_INTERVAL_MS}
            step={1000}
            value={intervalMs}
            onChange={(event) => setIntervalMs(event.target.value)}
            required
          />
        </div>
        {kind === "http" && (
          <>
            <div className="monitors-field path">
              <label htmlFor="monitor-path">Path</label>
              <input
                id="monitor-path"
                value={path}
                onChange={(event) => setPath(event.target.value)}
                placeholder="/ (the default)"
                autoComplete="off"
                spellCheck={false}
              />
            </div>
            <div className="monitors-field expect">
              <label htmlFor="monitor-expect">Expect</label>
              <input
                id="monitor-expect"
                value={expect}
                onChange={(event) => setExpect(event.target.value)}
                placeholder="text the body must contain"
                autoComplete="off"
              />
            </div>
          </>
        )}
        <button type="submit" className="primary" disabled={adding}>
          {adding ? "Adding…" : "Add target"}
        </button>
      </form>

      <p className="hint">{note}</p>
      {error && !removing && <p className="error">{error}</p>}
      {pollError && <p className="error">{pollError}</p>}

      <NotificationsCard />

      {targets.length > 0 && (
        <div className="cards">
          {targets.map((target) => {
            const status = statuses[target.id] ?? UNPROBED;
            const up = status.up_now;
            return (
              <article className="card" key={target.id}>
                <header>
                  <span
                    className={"dot" + (up === true ? " dot-ok" : up === false ? " dot-off" : "")}
                    title={up === true ? "up" : up === false ? "down" : "not probed yet"}
                  />
                  <strong>{where(target)}</strong>
                  <span className="monitor-kind">{target.kind}</span>
                </header>
                <dl className="facts">
                  <div>
                    <dt>State</dt>
                    <dd>{stateSentence(status)}</dd>
                  </div>
                  <div>
                    <dt>Uptime</dt>
                    <dd>{status.up_pct == null ? "not measured yet" : status.up_pct + "%"}</dd>
                  </div>
                  <div>
                    <dt>Drops</dt>
                    <dd>{status.drops == null ? "not measured yet" : String(status.drops)}</dd>
                  </div>
                  {status.last_status != null && (
                    <div>
                      <dt>Last status</dt>
                      <dd>HTTP {status.last_status}</dd>
                    </div>
                  )}
                  {target.interval_ms != null && (
                    <div>
                      <dt>Probes</dt>
                      <dd>every {target.interval_ms} ms</dd>
                    </div>
                  )}
                  {target.kind === "http" && target.expect && (
                    <div>
                      <dt>Expects</dt>
                      <dd className="mono">{target.expect}</dd>
                    </div>
                  )}
                </dl>
                <footer>
                  <button
                    type="button"
                    className="mini"
                    disabled={probing === target.id}
                    onClick={() => void probe(target)}
                  >
                    {probing === target.id ? "Probing…" : "Probe now"}
                  </button>
                  <button
                    type="button"
                    className="mini danger"
                    disabled={deleting}
                    onClick={() => {
                      setError(null);
                      setRemoving(target);
                    }}
                  >
                    Remove
                  </button>
                </footer>
              </article>
            );
          })}
        </div>
      )}

      {removing && (
        <Modal
          title={`Remove ${where(removing)}?`}
          onClose={() => {
            if (deleting) return;
            setRemoving(null);
            setError(null);
          }}
        >
          <p className="hint">
            The device stops probing it and discards its series. Its calls stay in the journal, so
            the console cannot restore it.
          </p>
          {error && <p className="error">{error}</p>}
          <div className="modal-actions">
            <button
              type="button"
              disabled={deleting}
              onClick={() => {
                setRemoving(null);
                setError(null);
              }}
            >
              Cancel
            </button>
            <button type="button" className="danger" disabled={deleting} onClick={() => void remove()}>
              {deleting ? "Removing…" : "Remove"}
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}

/// `host:port`, plus the path for an http target; an IPv6 host is bracketed so the port is legible.
function where(target: Target): string {
  const host = target.host.includes(":") ? `[${target.host}]` : target.host;
  const path = target.kind === "http" ? target.path ?? "/" : "";
  return `${host}:${target.port}${path}`;
}

/// How long the current state has lasted. `since_ms` is when the run began, so this is its age.
function stateSentence(status: Status): string {
  if (status.up_now == null) return "not probed yet";
  const elapsed = status.since_ms == null ? null : age(Date.now() - status.since_ms);
  const word = status.up_now ? "up" : "down";
  return elapsed === null ? word : `${word} for ${elapsed}`;
}

/// "42s", "3m 05s", "2h 10m", "3d 4h" — an age, not a timestamp: a status is read as a duration.
function age(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${String(seconds % 60).padStart(2, "0")}s`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ${String(minutes % 60).padStart(2, "0")}m`;
  const days = Math.floor(hours / 24);
  return `${days}d ${hours % 24}h`;
}

function listNote(targets: Target[]): string {
  if (targets.length === 0) return "No targets are watched yet; the form above adds the first one.";
  const noun = targets.length === 1 ? "target" : "targets";
  return `${targets.length} ${noun} watched. The dot is each target's newest probe.`;
}
