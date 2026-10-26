import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { message } from "./format";
import { Modal } from "./Modal";
import { useRequestGate } from "./requests";
import { callTool, type Grant } from "./terminal";

/**
 * One running process, as `system.process_list` returns it.
 *
 * `memory_kb` is the resident set size the device could read: 0 is what the platform reports for a
 * process it will not open (and for a kernel thread), so the table shows the number rather than
 * pretending the process is weightless or omitted.
 */
type DeviceProcess = {
  pid: number;
  name: string;
  memory_kb: number;
};

/** A kill the platform refused, with the device's own reason. */
type KillFailure = { pid: number; reason: string };

/** What `system.process_kill` did: the pids it signalled and the ones it could not. */
type KillReport = { killed: number[]; failed: KillFailure[] };

/**
 * What `system.net_test` established.
 *
 * `connected` is null when no address answered, and `error` then carries the device's reason. That
 * is a result, not a failed call: the name resolved, the port refused.
 */
type NetReport = {
  addresses: string[];
  connected: string | null;
  latency_ms: number | null;
  error: string | null;
};

/** A kill report kept with the row it was confirmed for, because the table may have moved on. */
type KillOutcome = { target: DeviceProcess; report: KillReport };

/** A test's report kept with the target it ran against, because the form may have moved on. */
type NetResult = { host: string; port: number } & NetReport;

/// `system.net_test`'s default and ceiling, mirrored from the device so the form states them.
const DEFAULT_NET_TIMEOUT_MS = 5_000;
const MAX_NET_TIMEOUT_MS = 60_000;

const READING_PROCESSES = "Reading the device's process list…";

/**
 * The device's processes and its reachability, as a page.
 *
 * Both are device state, so this reads and acts through the same grant as the terminal, the browser,
 * memory, monitors and runs: `system.process_list` for a snapshot, `system.process_kill` for one
 * deliberate end, `system.net_test` for one TCP connect. There is no console route — every call is
 * signed by the console, checked by the device, and audited like the other panes' calls.
 *
 * A kill is not one click away from a typo: the row names the process, and the confirm names the pid
 * and that name again. A net test that reaches nothing is a result (the addresses resolved and the
 * device says why the connection failed), not an error page: the device answered.
 */
export function SystemPage({ grant }: { grant: Grant }) {
  const [processes, setProcesses] = useState<DeviceProcess[]>([]);
  const [note, setNote] = useState(READING_PROCESSES);
  const [listError, setListError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const [filter, setFilter] = useState("");
  // The filter the table was actually read with. The device filters, so what is typed is not in
  // effect until a read carries it; this is what the note and a kill's re-read are built from.
  const [applied, setApplied] = useState("");

  const [killTarget, setKillTarget] = useState<DeviceProcess | null>(null);
  const [killError, setKillError] = useState<string | null>(null);
  const [killing, setKilling] = useState(false);
  const [outcome, setOutcome] = useState<KillOutcome | null>(null);

  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [timeoutMs, setTimeoutMs] = useState(String(DEFAULT_NET_TIMEOUT_MS));
  const [testing, setTesting] = useState(false);
  const [testingFor, setTestingFor] = useState<string | null>(null);
  const [netResult, setNetResult] = useState<NetResult | null>(null);
  const [netError, setNetError] = useState<string | null>(null);

  // A failed first read has nothing to keep showing; a failed refresh must leave the previous rows
  // where they are, so the two are told apart by whether a list ever arrived.
  const listed = useRef(false);
  // The disabled buttons are the visible guard, but a second submit can arrive before the re-render
  // disables them; the handler's `killing`/`testing` is the value from the render that created it,
  // so the refs are the real gate. The kill is destructive: two calls would be two audit lines and,
  // after the first one ended the pid, a second refusal for a process that is already gone.
  const killingRef = useRef(false);
  const testingRef = useRef(false);
  // The list, the kill and the test paint different things, so each has its own ticket: a refresh
  // must not discard a kill that is still on the wire, and a kill's re-read must not paint over a
  // newer refresh. A new grant begins a new epoch for all three; the effect below does that.
  const listGate = useRequestGate();
  const killGate = useRequestGate();
  const netGate = useRequestGate();

  const read = useCallback(
    async (needle: string) => {
      const ticket = listGate.begin();
      setLoading(true);
      setListError(null);
      try {
        // The device applies the filter, case-insensitively on the executable name. Sending nothing
        // for an empty box is the whole list; a client-side filter would hide processes the device
        // would still kill by name.
        const args: Record<string, unknown> = {};
        if (needle !== "") args.name = needle;
        const rows = JSON.parse(
          await callTool(grant, "system.process_list", args)
        ) as DeviceProcess[];
        if (listGate.stale(ticket)) return;
        listed.current = true;
        setProcesses(rows);
        setApplied(needle);
        setNote(listNote(rows, needle));
      } catch (cause) {
        if (listGate.stale(ticket)) return;
        // The rows stay as they were: a refresh that fails should not blank a list the operator can
        // still read, and the sentence says what the device refused.
        if (!listed.current) setNote("The device's process list could not be read.");
        setListError("Could not read the process list: " + message(cause));
      } finally {
        if (!listGate.stale(ticket)) setLoading(false);
      }
    },
    [grant, listGate]
  );

  // A different grant is a different device's processes and reachability, and a different draft. It
  // also invalidates a kill or a test still on the wire: its answer belongs to the device this page
  // has left.
  useEffect(() => {
    killGate.begin();
    netGate.begin();
    killingRef.current = false;
    testingRef.current = false;
    listed.current = false;
    setProcesses([]);
    setFilter("");
    setApplied("");
    setNote(READING_PROCESSES);
    setListError(null);
    setKillTarget(null);
    setKillError(null);
    setKilling(false);
    setOutcome(null);
    setHost("");
    setPort("");
    setTimeoutMs(String(DEFAULT_NET_TIMEOUT_MS));
    setTesting(false);
    setTestingFor(null);
    setNetResult(null);
    setNetError(null);
    void read("");
  }, [read, killGate, netGate]);

  // The filter is sent, not applied here: pressing Refresh (or Enter) is the device call, and the
  // note says what the table is, not what the box says.
  const refresh = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    void read(filter.trim());
  };

  const confirmKill = async () => {
    if (!killTarget || killingRef.current) return;
    const target = killTarget;
    killingRef.current = true;
    const ticket = killGate.begin();
    setKilling(true);
    setKillError(null);
    setOutcome(null);
    try {
      // One target, one answer: a pid, never a pid and a name. The row's name is what the confirm
      // used to make the decision, but the request is the pid the device reported for it.
      const report = JSON.parse(
        await callTool(grant, "system.process_kill", { pid: target.pid })
      ) as KillReport;
      if (killGate.stale(ticket)) return;
      setKillTarget(null);
      setOutcome({ target, report });
    } catch (cause) {
      if (killGate.stale(ticket)) return;
      // The modal stays open: the device's sentence belongs next to the process it was about, and
      // the operator can cancel or try again after reading it.
      setKillError(
        "The device did not end PID " + target.pid + " (" + target.name + "): " + message(cause)
      );
    } finally {
      if (!killGate.stale(ticket)) {
        killingRef.current = false;
        setKilling(false);
      }
    }
    if (killGate.stale(ticket)) return;
    // The list re-reads after every attempt, report or refusal: a console timeout can lose the
    // answer to a kill that landed, and only the list says whether the process is still there.
    await read(applied);
  };

  const test = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (testingRef.current) return;
    testingRef.current = true;
    const ticket = netGate.begin();
    const wantedHost = host.trim();
    const wantedPort = Number(port);
    const args: Record<string, unknown> = { host: wantedHost, port: wantedPort };
    const wantedTimeout = Number(timeoutMs);
    if (timeoutMs.trim() !== "" && Number.isFinite(wantedTimeout)) {
      // An empty box means the device's default (5000); a value it refuses is still sent, so the
      // refusal is in the device's words rather than the browser's. The device clamps it.
      args.timeout_ms = wantedTimeout;
    }
    const target = where(wantedHost, wantedPort);

    setTesting(true);
    setTestingFor(target);
    setNetError(null);
    try {
      const report = JSON.parse(
        await callTool(grant, "system.net_test", args)
      ) as NetReport;
      if (netGate.stale(ticket)) return;
      setNetResult({ host: wantedHost, port: wantedPort, ...report });
    } catch (cause) {
      if (netGate.stale(ticket)) return;
      // A refusal here is the device declining the question (a name that does not resolve, a host
      // that is not a name); a connection that failed is a report and never reaches this branch.
      // The last result stays where it is, labelled as the previous one.
      setNetError("Could not test " + target + ": " + message(cause));
    } finally {
      if (!netGate.stale(ticket)) {
        testingRef.current = false;
        setTesting(false);
        setTestingFor(null);
      }
    }
  };

  const listedFilter = filter.trim();
  const filterPending = listedFilter !== applied;

  return (
    <div className="system-page">
      <div className="panel-head">
        <h2>System</h2>
      </div>
      <p className="hint">
        What the device is running, and whether it can reach one TCP port. Every read and every
        action goes through the grant the console signed; the device audits each call.
      </p>

      <section className="system-section" aria-labelledby="system-processes">
        <h3 id="system-processes">Processes</h3>
        <form className="system-toolbar" onSubmit={refresh}>
          <div className="system-field filter">
            <label htmlFor="system-filter">Filter</label>
            <input
              id="system-filter"
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              placeholder="process name contains…"
              autoComplete="off"
              spellCheck={false}
            />
          </div>
          <button type="submit" disabled={loading}>
            {loading ? "Reading…" : "Refresh"}
          </button>
        </form>
        <p className="hint">{note}</p>
        {filterPending && (
          <p className="hint">
            The device applies the filter, so the table still shows the previous read. Press Refresh
            (or Enter) to ask it for this one.
          </p>
        )}
        {listError && <p className="error">{listError}</p>}

        {outcome && (
          <section className="card system-kill">
            <header>
              <strong>Kill result</strong>
              <div className="actions">
                <button type="button" className="mini" onClick={() => setOutcome(null)}>
                  Dismiss
                </button>
              </div>
            </header>
            {/* Failures come first: a partly failed kill must open with what did not happen, and
                the success line is only for the pids the device says it ended. */}
            {outcome.report.failed.length > 0 && (
              <>
                <p className="error">
                  {outcome.report.failed.length === 1 &&
                  outcome.report.failed[0].pid === outcome.target.pid
                    ? `The device did not end PID ${outcome.target.pid} (${outcome.target.name}).`
                    : `The device did not end every process it was given.`}
                </p>
                <ul className="system-failures">
                  {outcome.report.failed.map((failure) => (
                    <li key={failure.pid}>
                      <span className="mono">PID {failure.pid}</span>: {failure.reason}
                    </li>
                  ))}
                </ul>
              </>
            )}
            {outcome.report.killed.length > 0 && (
              <p>{killedSentence(outcome.target, outcome.report.killed)}</p>
            )}
            {outcome.report.killed.length === 0 && outcome.report.failed.length === 0 && (
              <p className="hint">
                The device returned neither a pid it ended nor one it refused, which is not a
                result this page knows how to read.
              </p>
            )}
          </section>
        )}

        {processes.length > 0 && (
          <div className="scroll system-table">
            <table>
              <thead>
                <tr>
                  <th>PID</th>
                  <th>Process</th>
                  <th>Memory</th>
                  <th>Action</th>
                </tr>
              </thead>
              <tbody>
                {processes.map((process) => (
                  <tr key={process.pid}>
                    <td className="system-pid">{process.pid}</td>
                    <td className="system-name">{process.name}</td>
                    <td
                      className="system-memory"
                      title={`${process.memory_kb} KiB resident, as the device reported it`}
                    >
                      {megabytes(process.memory_kb)}
                    </td>
                    <td className="system-kill-cell">
                      <button
                        type="button"
                        className="mini danger"
                        disabled={killing}
                        aria-label={`Kill PID ${process.pid} (${process.name})`}
                        onClick={() => {
                          setKillError(null);
                          setKillTarget(process);
                        }}
                      >
                        Kill
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section className="system-section" aria-labelledby="system-net">
        <h3 id="system-net">Network test</h3>
        <p className="hint">
          One TCP connect from the device to one port — the question its own clients ask, not an
          ICMP ping. A port that refuses the connection is a result: the addresses are what the
          device resolved, and the reason sits with them. A name that does not resolve is a refusal
          and is shown as one.
        </p>
        <form className="system-net" onSubmit={(event) => void test(event)}>
          <div className="system-field host">
            <label htmlFor="system-host">Host</label>
            <input
              id="system-host"
              value={host}
              onChange={(event) => setHost(event.target.value)}
              placeholder="192.168.1.1 or example.com"
              autoComplete="off"
              spellCheck={false}
              required
            />
          </div>
          <div className="system-field">
            <label htmlFor="system-port">Port</label>
            <input
              id="system-port"
              type="number"
              min={1}
              max={65535}
              value={port}
              onChange={(event) => setPort(event.target.value)}
              placeholder="443"
              required
            />
          </div>
          <div className="system-field">
            <label htmlFor="system-timeout">Timeout (ms)</label>
            <input
              id="system-timeout"
              type="number"
              min={1}
              max={MAX_NET_TIMEOUT_MS}
              value={timeoutMs}
              onChange={(event) => setTimeoutMs(event.target.value)}
            />
          </div>
          <button type="submit" className="primary" disabled={testing}>
            {testing ? "Testing…" : "Test"}
          </button>
        </form>
        <p className="hint">
          The timeout is 1 to {MAX_NET_TIMEOUT_MS} ms and defaults to {DEFAULT_NET_TIMEOUT_MS} ms
          when the field is empty. It covers resolving and connecting together, and the device
          clamps it.
        </p>

        {testingFor && (
          <p className="hint">
            Testing {testingFor}…
            {netResult ? " The result below is from the previous test." : ""}
          </p>
        )}
        {netError && <p className="error">{netError}</p>}
        {netError && netResult && (
          <p className="hint">
            The result below is from the last test that finished; this attempt did not replace it.
          </p>
        )}

        {netResult && (
          <section className="card system-net-result">
            <header>
              <strong className="mono">{where(netResult.host, netResult.port)}</strong>
              <span className={"chip " + (netResult.connected ? "chip-ok" : "chip-warn")}>
                {netResult.connected ? "connected" : "not connected"}
              </span>
            </header>
            <dl className="facts">
              <div>
                <dt>Addresses</dt>
                <dd className="mono">{netResult.addresses.join(", ")}</dd>
              </div>
              <div>
                <dt>Connected</dt>
                <dd className="mono">{netResult.connected ?? "none"}</dd>
              </div>
              <div>
                <dt>Latency</dt>
                <dd>{netResult.latency_ms == null ? "none" : `${netResult.latency_ms} ms`}</dd>
              </div>
              {netResult.error && (
                <div>
                  <dt>Reason</dt>
                  <dd>{netResult.error}</dd>
                </div>
              )}
            </dl>
          </section>
        )}
      </section>

      {killTarget && (
        <Modal
          title={`Kill PID ${killTarget.pid} (${killTarget.name})?`}
          onClose={() => {
            if (killingRef.current) return;
            setKillTarget(null);
            setKillError(null);
          }}
        >
          <p>
            This asks the device to end <span className="mono">PID {killTarget.pid}</span>,{" "}
            {killTarget.name}, now. It cannot be undone, and a pid on its own does not identify a
            process: a reused pid would be a different one, so confirm both.
          </p>
          <p className="hint">
            The device audits the call. A process the platform will not let the device signal is
            reported with its reason instead of ending.
          </p>
          {killError && <p className="error">{killError}</p>}
          <div className="modal-actions">
            <button
              type="button"
              disabled={killing}
              onClick={() => {
                setKillTarget(null);
                setKillError(null);
              }}
            >
              Cancel
            </button>
            <button
              type="button"
              className="danger"
              disabled={killing}
              onClick={() => void confirmKill()}
            >
              {killing ? "Killing…" : `Kill PID ${killTarget.pid}`}
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}

/// The sentence for a kill's `killed` list, naming the process only when the pid is the one the
/// confirm named. A kill by name can answer several pids; this page always sends one pid.
function killedSentence(target: DeviceProcess, killed: number[]): string {
  if (killed.length === 1) {
    const pid = killed[0];
    return pid === target.pid
      ? `The device ended PID ${pid} (${target.name}).`
      : `The device ended PID ${pid}.`;
  }
  return `The device ended ${killed.map((pid) => "PID " + pid).join(", ")}.`;
}

/// "82.3 MB" — resident memory as the table reads it. The device reports KiB, so the conversion is
/// the page's, and the title keeps the number the device actually sent.
function megabytes(kb: number): string {
  return (kb / 1024).toFixed(1) + " MB";
}

/// What the table is, in the device's terms: the filter was applied there, so the note says how
/// many processes answered it rather than what the box contains.
function listNote(rows: DeviceProcess[], filter: string): string {
  if (rows.length === 0) {
    return filter === ""
      ? "The device reports no running processes."
      : `No process on the device has a name containing ${JSON.stringify(filter)}, ` +
          "case-insensitively.";
  }
  const noun = rows.length === 1 ? "process" : "processes";
  if (filter === "") return `${rows.length} ${noun} on the device, by pid.`;
  return (
    `${rows.length} ${noun} whose name contains ${JSON.stringify(filter)}, ` +
    "case-insensitively, by pid."
  );
}

/// `host:port`, bracketing a bare IPv6 host so the port is legible; a host that already carries its
/// brackets is not bracketed twice.
function where(host: string, port: number): string {
  const bare = host.replace(/^\[(.*)\]$/, "$1");
  return (bare.includes(":") ? `[${bare}]` : bare) + ":" + port;
}
