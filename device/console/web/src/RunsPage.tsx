import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { callTool, type Grant } from "./terminal";

/**
 * `runs.exec`'s answer: the run's outcome and the retained head of each stream.
 *
 * `exit_code` is null both when the timeout killed the process group (`timed_out`) and when the
 * process ended from a signal; the two nulls are different facts and the result panel says which.
 */
type RunResult = {
  id: string;
  exit_code: number | null;
  timed_out: boolean;
  duration_ms: number;
  stdout: string;
  stderr: string;
  truncated: boolean;
  history_recorded: boolean;
};

/**
 * One run as the device's history keeps it: the metadata, and the output only as byte counts.
 *
 * `runs.list` and `runs.get` return this same shape; neither carries stdout or stderr, because the
 * history never stored them.
 */
type RunRecord = {
  id: string;
  command: string;
  args: string[];
  cwd: string;
  exit_code: number | null;
  timed_out: boolean;
  duration_ms: number;
  started_at_ms: number;
  stdout_bytes: number;
  stderr_bytes: number;
};

/// The rows one history read asks for: `runs.list` defaults to 20 and refuses more than 100.
const HISTORY_LIMIT = 20;

/// `runs.exec`'s timeout default and ceiling, mirrored from the device so the form states them.
const DEFAULT_TIMEOUT_MS = 60_000;
const MAX_TIMEOUT_MS = 600_000;

/**
 * Runs, as a page: one command on the device, its exit and output, and the history of the last runs.
 *
 * A run is a device call like any other, so it goes through the same grant as the terminal, the
 * browser, memory and monitors: `runs.exec` runs it and blocks until it has finished or timed out,
 * `runs.list` reads the history and `runs.get` reads one row. There is no console route. The history
 * keeps no output bodies, so this page never grows an output box for a past run: it says why there
 * is nothing to show instead.
 */
export function RunsPage({ grant }: { grant: Grant }) {
  const [history, setHistory] = useState<RunRecord[]>([]);
  const [historyNote, setHistoryNote] = useState("Reading the device's run history…");
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [loadingHistory, setLoadingHistory] = useState(false);

  const [command, setCommand] = useState("");
  const [shell, setShell] = useState(false);
  const [cwd, setCwd] = useState("");
  const [timeoutMs, setTimeoutMs] = useState(String(DEFAULT_TIMEOUT_MS));
  const [running, setRunning] = useState(false);
  const [runError, setRunError] = useState<string | null>(null);
  const [result, setResult] = useState<RunResult | null>(null);

  const [detail, setDetail] = useState<RunRecord | null>(null);
  const [detailId, setDetailId] = useState<string | null>(null);
  const [detailError, setDetailError] = useState<string | null>(null);

  // A failed first read has nothing to keep showing; a failed refresh must leave the previous rows
  // where they are, so the two cases are told apart by whether a list ever arrived.
  const listed = useRef(false);

  const read = useCallback(async () => {
    setLoadingHistory(true);
    setHistoryError(null);
    try {
      const rows = JSON.parse(
        await callTool(grant, "runs.list", { limit: HISTORY_LIMIT })
      ) as RunRecord[];
      listed.current = true;
      setHistory(rows);
      setHistoryNote(historyNoteFor(rows));
    } catch (cause) {
      if (!listed.current) setHistoryNote("The device's run history could not be read.");
      setHistoryError("Could not read the run history: " + message(cause));
    } finally {
      setLoadingHistory(false);
    }
  }, [grant]);

  // A different grant is a different device's history and a different draft.
  useEffect(() => {
    listed.current = false;
    setHistory([]);
    setHistoryNote("Reading the device's run history…");
    setHistoryError(null);
    setResult(null);
    setRunError(null);
    setDetail(null);
    setDetailError(null);
    setCommand("");
    setShell(false);
    setCwd("");
    setTimeoutMs(String(DEFAULT_TIMEOUT_MS));
    void read();
  }, [read]);

  const run = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    // The disabled button is the visible guard; this one holds when a submit arrives before the
    // state has re-rendered the button, so one form cannot put two runs on the device.
    if (running) return;
    const line = command.trim();
    // argv mode splits on whitespace and does no quoting: the first word is the program and the rest
    // are its arguments. A shell line is handed over whole. An empty command is sent as the empty
    // string rather than refused here, so the sentence the operator reads is the device's.
    const words = line.split(/\s+/).filter((word) => word !== "");
    const request: Record<string, unknown> = shell
      ? { command: line, shell: true }
      : { command: words[0] ?? "", args: words.slice(1) };
    if (cwd.trim() !== "") request.cwd = cwd.trim();
    const wantedTimeout = Number(timeoutMs);
    if (timeoutMs.trim() !== "" && Number.isFinite(wantedTimeout)) {
      // A value outside 1..600000 is still sent: the device refuses it in its own words, and a
      // min/max the page enforced would replace that sentence with the browser's.
      request.timeout_ms = wantedTimeout;
    }

    setRunning(true);
    setRunError(null);
    // The result panel belongs to the run that is about to finish, never to the one before it.
    setResult(null);
    try {
      const finished = JSON.parse(
        await callTool(grant, "runs.exec", request)
      ) as RunResult;
      // Show the run before the re-read: a history read that fails afterwards must not make a
      // successful run look like it did not happen.
      setResult(finished);
      await read();
    } catch (cause) {
      setRunError("Could not run the command: " + message(cause));
    } finally {
      setRunning(false);
    }
  };

  const show = async (row: RunRecord) => {
    if (detailId !== null) return;
    setDetailId(row.id);
    setDetailError(null);
    try {
      const record = JSON.parse(
        await callTool(grant, "runs.get", { id: row.id })
      ) as RunRecord;
      setDetail(record);
    } catch (cause) {
      setDetail(null);
      setDetailError("Could not read run " + row.id + ": " + message(cause));
    } finally {
      setDetailId(null);
    }
  };

  return (
    <div className="runs-page">
      <div className="panel-head">
        <h2>Runs</h2>
        <div className="actions">
          <button
            type="button"
            disabled={loadingHistory || running}
            onClick={() => void read()}
          >
            {loadingHistory ? "Refreshing…" : "Refresh history"}
          </button>
        </div>
      </div>
      <p className="hint">
        One command, run once on the device. The call blocks until the command has finished or its
        timeout has expired; the history below records what ran, its exit and how much it wrote.
      </p>

      {/*
        `noValidate`: the device owns the bounds. Its refusal — an empty command, a timeout outside
        1..600000 — is the sentence worth showing, and the browser's would replace it.
      */}
      <form className="runs-form" noValidate onSubmit={(event) => void run(event)}>
        <div className="runs-field command">
          <label htmlFor="runs-command">Command</label>
          <input
            id="runs-command"
            value={command}
            onChange={(event) => setCommand(event.target.value)}
            placeholder={shell ? "echo hello && dir" : "echo hello"}
            autoComplete="off"
            spellCheck={false}
          />
        </div>
        <div className="runs-field cwd">
          <label htmlFor="runs-cwd">Working directory (optional)</label>
          <input
            id="runs-cwd"
            value={cwd}
            onChange={(event) => setCwd(event.target.value)}
            placeholder="relative to the files root"
            autoComplete="off"
            spellCheck={false}
          />
        </div>
        <div className="runs-field timeout">
          <label htmlFor="runs-timeout">Timeout (ms)</label>
          <input
            id="runs-timeout"
            type="number"
            min={1}
            max={MAX_TIMEOUT_MS}
            step={1000}
            value={timeoutMs}
            onChange={(event) => setTimeoutMs(event.target.value)}
          />
        </div>
        <label className="check runs-shell">
          <input
            type="checkbox"
            checked={shell}
            onChange={(event) => setShell(event.target.checked)}
          />
          Through the shell
        </label>
        <button type="submit" className="primary" disabled={running}>
          {running ? "Running…" : "Run"}
        </button>
      </form>

      <p className="hint">
        {shell
          ? "The whole line goes to the device's platform shell (cmd /C on Windows, sh -c elsewhere), so the shell parses it."
          : "The line is split on whitespace — the first word is the program, the rest are its arguments — and run without a shell, so $, * and ; are ordinary characters. There is no quoting."}{" "}
        The working directory is where the process starts, relative to the device's files root; it is
        not a jail. The timeout is 1 to {MAX_TIMEOUT_MS} ms and defaults to {DEFAULT_TIMEOUT_MS} ms.
      </p>

      {running && (
        <p className="hint">
          Running on the device… runs.exec does not return until the command finishes or the timeout
          kills it, and the form stays disabled until it does.
        </p>
      )}
      {runError && <p className="error">{runError}</p>}

      {result && (
        <section className="card runs-result">
          <header>
            <strong>Result</strong>
            <span className="mono muted" title="run id">
              {result.id}
            </span>
          </header>
          <dl className="facts">
            <div>
              <dt>Exit</dt>
              <dd>{exitSentence(result)}</dd>
            </div>
            <div>
              <dt>Duration</dt>
              <dd>{spellDuration(result.duration_ms)}</dd>
            </div>
            <div>
              <dt>Output</dt>
              <dd>{result.truncated ? "truncated" : "complete"}</dd>
            </div>
            <div>
              <dt>History</dt>
              <dd>{result.history_recorded ? "recorded" : "not recorded"}</dd>
            </div>
          </dl>
          {result.truncated && (
            <p className="hint">
              The device kept the head of each stream (1 MiB) and drained the rest, so the output
              below is cut, not complete.
            </p>
          )}
          {!result.history_recorded && (
            <p className="hint">
              The command ran and its output is below, but its history row could not be written; the
              device's events log carries the failure.
            </p>
          )}
          <div className="runs-streams">
            <div className="runs-stream">
              <h3>stdout</h3>
              {result.stdout === "" ? (
                <p className="hint">(no stdout)</p>
              ) : (
                <pre>{result.stdout}</pre>
              )}
            </div>
            <div className="runs-stream">
              <h3>stderr</h3>
              {result.stderr === "" ? (
                <p className="hint">(no stderr)</p>
              ) : (
                <pre>{result.stderr}</pre>
              )}
            </div>
          </div>
        </section>
      )}

      <p className="hint">{historyNote}</p>
      {historyError && <p className="error">{historyError}</p>}
      {detailId && <p className="hint">Reading run {detailId}…</p>}
      {detailError && <p className="error">{detailError}</p>}

      {detail && (
        <section className="card runs-detail">
          <header>
            <strong className="mono">{detail.id}</strong>
            <div className="actions">
              <button type="button" className="mini" onClick={() => setDetail(null)}>
                Close
              </button>
            </div>
          </header>
          <div className="runs-detail-command">
            <h3>Command</h3>
            <p className="mono">{commandLine(detail)}</p>
          </div>
          <p className="hint">
            This is the record the history kept. It stores each stream's size, not its bytes, so a
            past run has no stdout or stderr to show: the result panel above carries the output of
            the run this page just made, and an earlier run's output is gone.
          </p>
          <dl className="facts">
            <div>
              <dt>Exit</dt>
              <dd>{exitSentence(detail)}</dd>
            </div>
            <div>
              <dt>Duration</dt>
              <dd>{spellDuration(detail.duration_ms)}</dd>
            </div>
            <div>
              <dt>Started</dt>
              <dd>{age(Date.now() - detail.started_at_ms)} ago</dd>
            </div>
            <div>
              <dt>Working directory</dt>
              <dd className="mono">{detail.cwd}</dd>
            </div>
            <div>
              <dt>stdout</dt>
              <dd>{bytes(detail.stdout_bytes)}</dd>
            </div>
            <div>
              <dt>stderr</dt>
              <dd>{bytes(detail.stderr_bytes)}</dd>
            </div>
          </dl>
        </section>
      )}

      {history.length > 0 && (
        <div className="scroll runs-history">
          <table>
            <thead>
              <tr>
                <th>Command</th>
                <th>Exit</th>
                <th>Duration</th>
                <th>Started</th>
                <th>stdout / stderr</th>
              </tr>
            </thead>
            <tbody>
              {history.map((row) => (
                <tr
                  key={row.id}
                  className={"runs-row" + (detail?.id === row.id ? " runs-row-active" : "")}
                  onClick={() => void show(row)}
                >
                  <td>
                    <button
                      type="button"
                      className="runs-open"
                      disabled={detailId !== null}
                      title={"runs.get " + row.id}
                    >
                      {commandLine(row)}
                    </button>
                  </td>
                  <td>
                    <span className={exitChip(row)}>{exitSentence(row)}</span>
                  </td>
                  <td>{spellDuration(row.duration_ms)}</td>
                  <td>{age(Date.now() - row.started_at_ms)} ago</td>
                  <td className="mono">
                    {bytes(row.stdout_bytes)} / {bytes(row.stderr_bytes)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

/// A history row's command line: a shell line as it was, or an argv array quoted only where a word
/// would otherwise split. The history does not record whether the shell was used, so this is a
/// readable spelling of the row, not a reconstruction of the request.
function commandLine(row: RunRecord): string {
  if (row.args.length === 0) return row.command;
  return [row.command, ...row.args]
    .map((part) => (/[\s"']/.test(part) ? JSON.stringify(part) : part))
    .join(" ");
}

/// The exit as one phrase. A timed-out run and a run killed by a signal both have no exit code, and
/// the timeout is the one the page can name.
function exitSentence(run: { exit_code: number | null; timed_out: boolean }): string {
  if (run.timed_out) return "timed out";
  if (run.exit_code === null) return "no exit status";
  return "exit " + String(run.exit_code);
}

/// The chip that colours the same phrase: 0 is success, any other exit is a failure, and a run with
/// no exit status (a timeout or a signal) is neither.
function exitChip(run: { exit_code: number | null; timed_out: boolean }): string {
  if (run.timed_out || run.exit_code === null) return "chip chip-warn";
  return "chip " + (run.exit_code === 0 ? "chip-ok" : "chip-bad");
}

/// "42 ms", "1.5 s", "2m 03s" — a duration, read at a glance next to a byte count.
function spellDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) {
    const seconds = (ms / 1000).toFixed(2).replace(/\.?0+$/, "");
    return `${seconds} s`;
  }
  const seconds = Math.floor(ms / 1000);
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ${String(seconds % 60).padStart(2, "0")}s`;
}

/// "42s", "3m 05s", "2h 10m", "3d 4h" — an age, not a timestamp: a history row is read as how long
/// ago it started. The same scale as the monitors page's state age.
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

/// "0 B", "812 B", "4.2 KB", "1.0 MB" — the history stores bytes, so the row reports them as such.
function bytes(count: number): string {
  if (count < 1024) return `${count} B`;
  if (count < 1024 * 1024) return `${(count / 1024).toFixed(1)} KB`;
  return `${(count / (1024 * 1024)).toFixed(1)} MB`;
}

function historyNoteFor(rows: RunRecord[]): string {
  if (rows.length === 0) {
    return "The device's run history is empty; the form above runs the first command.";
  }
  const noun = rows.length === 1 ? "run" : "runs";
  const capped = rows.length >= HISTORY_LIMIT ? ` (the newest ${HISTORY_LIMIT})` : "";
  return `${rows.length} recent ${noun}${capped}, newest first. Click a row to read its record.`;
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
