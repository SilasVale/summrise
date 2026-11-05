import { useEffect, useRef, useState, type FormEvent } from "react";
import { copyText } from "./copy";
import { message } from "./format";
import { useRequestGate } from "./requests";
import { callTool, ToolError, type Grant } from "./terminal";
import {
  DEFAULT_TIMEOUT_MS,
  MAX_TIMEOUT_MS,
  exitSentence,
  spellDuration,
  type RunResult
} from "./runFormat";

/**
 * One command on the machine, and the record of the run it just made.
 *
 * THE FORM MOVED HERE FROM THE RUNS PAGE. Runs is the history now; this is the only place a command
 * is composed, so the page that lists runs cannot become a second way to run one. The fields, the
 * argv/shell split, the device-owned bounds, the refusal sentences and the result panel are the ones
 * the Runs page carried, unchanged: the request is still `runs.exec` with `command`/`args` (or
 * `shell`), an optional `cwd` and a `timeout_ms`, and the Tools tab takes the same grant the history
 * page does.
 *
 * THE PLACEHOLDER NAMES A PROGRAM THE MACHINE HAS, OR NO EXAMPLE AT ALL. It used to read `echo
 * hello` on every machine; `echo` is a builtin of both platform shells, so in argv mode the device
 * looked for a program named `echo` and answered "runs.exec: program not found" (measured on
 * DESKTOP-9VH92BJ, 4 ms). The example now comes from the machine's own OS fact — `cmd.exe` on
 * Windows, `uname` elsewhere — and a machine whose platform the console does not know (offline, or
 * no frame since the console started) gets no example, only the shape of what the field takes. The
 * shell field keeps `echo hello && echo bye` because there `echo` is the shell's own.
 */
export function RunForm({
  grant,
  deviceOnline,
  osName
}: {
  grant: Grant;
  /** The fleet's reachability, as the history page takes it; `false` is the only fact. */
  deviceOnline?: boolean;
  /** The device's own `host.os.name`, when its last frame carried one. */
  osName?: string | null;
}) {
  const offline = deviceOnline === false;
  const [command, setCommand] = useState("");
  const [shell, setShell] = useState(false);
  const [cwd, setCwd] = useState("");
  const [timeoutMs, setTimeoutMs] = useState(String(DEFAULT_TIMEOUT_MS));
  const [running, setRunning] = useState(false);
  const [runError, setRunError] = useState<string | null>(null);
  const [result, setResult] = useState<RunResult | null>(null);
  const [copied, setCopied] = useState<"stdout" | "stderr" | null>(null);

  // The disabled button is the visible guard, but a second submit can arrive before the re-render
  // disables it; `running` in this handler is the value from the render that created it, so the ref
  // is the real gate.
  const runningRef = useRef(false);
  const runGate = useRequestGate();

  // A different grant is a different device's draft, and it invalidates a run still waiting: its
  // answer belongs to the device this form has left. The history page's own reset is its business.
  useEffect(() => {
    runGate.begin();
    runningRef.current = false;
    setRunning(false);
    setCommand("");
    setShell(false);
    setCwd("");
    setTimeoutMs(String(DEFAULT_TIMEOUT_MS));
    setResult(null);
    setRunError(null);
  }, [grant, runGate, offline]);

  const run = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (runningRef.current) return;
    runningRef.current = true;
    const ticket = runGate.begin();
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
    setCopied(null);
    try {
      const finished = JSON.parse(
        await callTool(grant, "runs.exec", request)
      ) as RunResult;
      if (runGate.stale(ticket)) return;
      // The record of the run just made: id, exit, duration and the retained head of each stream.
      setResult(finished);
    } catch (cause) {
      if (runGate.stale(ticket)) return;
      // The last result stays where it is: a failed run says nothing about the one that did run.
      setRunError(runFailure(cause));
    } finally {
      if (!runGate.stale(ticket)) {
        runningRef.current = false;
        setRunning(false);
      }
    }
  };

  const copyStream = async (which: "stdout" | "stderr", text: string) => {
    if (await copyText(text)) setCopied(which);
  };

  return (
    <div className="runs-exec">
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
            placeholder={commandPlaceholder(osName, shell)}
            autoComplete="off"
            spellCheck={false}
          />
          <span className="field-help">
            {shell ? "Run through the platform shell." : "Split on whitespace; no quoting."}
          </span>
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
          <span className="field-help">Relative to the files root; not a jail.</span>
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
          <span className="field-help">1 to {MAX_TIMEOUT_MS} ms; default {DEFAULT_TIMEOUT_MS}.</span>
        </div>
        <label className="check runs-shell">
          <input
            type="checkbox"
            checked={shell}
            onChange={(event) => setShell(event.target.checked)}
          />
          Through the shell
        </label>
        <button
          type="submit"
          className="primary"
          disabled={running || offline}
          title={offline ? "The machine is offline; a command can run when it reconnects" : undefined}
        >
          {running ? "Running…" : "Run"}
        </button>
      </form>

      {running && (
        <p className="hint">
          Running on the device… runs.exec does not return until the command finishes or the timeout
          kills it, and the form stays disabled until it does.
        </p>
      )}
      {runError && <p className="error">{runError}</p>}
      {runError && result && (
        <p className="hint">
          The result below is from the last run that finished; this attempt did not replace it.
        </p>
      )}

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
              <div className="runs-stream-head">
                <h3>stdout</h3>
                {result.stdout !== "" && (
                  <button
                    type="button"
                    className="mini"
                    onClick={() => void copyStream("stdout", result.stdout)}
                  >
                    {copied === "stdout" ? "Copied" : "Copy stdout"}
                  </button>
                )}
              </div>
              {result.stdout === "" ? (
                <p className="hint">(no stdout)</p>
              ) : (
                <pre>{result.stdout}</pre>
              )}
            </div>
            <div className="runs-stream">
              <div className="runs-stream-head">
                <h3>stderr</h3>
                {result.stderr !== "" && (
                  <button
                    type="button"
                    className="mini"
                    onClick={() => void copyStream("stderr", result.stderr)}
                  >
                    {copied === "stderr" ? "Copied" : "Copy stderr"}
                  </button>
                )}
              </div>
              {result.stderr === "" ? (
                <p className="hint">(no stderr)</p>
              ) : (
                <pre>{result.stderr}</pre>
              )}
            </div>
          </div>
        </section>
      )}
    </div>
  );
}

/**
 * What the command field shows before anything is typed: an example the machine can run, chosen
 * from the device's own OS fact, or the field's shape when the console does not know the platform.
 *
 * A shell builtin is not a program in argv mode. `echo hello` sent command `echo`, and the device
 * answered "runs.exec: program not found"; `cmd.exe` is in System32 on every Windows machine, and
 * `uname` is a program on every Unix-like one. Both take their arguments without quoting, which is
 * the only shape argv mode can express — `sh -c 'echo hello'` cannot be typed in this field, because
 * the split is on whitespace and the quotes would be passed through literally. A machine whose
 * platform the fleet does not know gets no example at all: a plausible one is the bug this replaces.
 */
function commandPlaceholder(osName: string | null | undefined, shell: boolean): string {
  // The shell runs `echo` itself, so this example is a command line the platform shell can execute.
  if (shell) return "echo hello && echo bye";
  const name = typeof osName === "string" ? osName.trim().toLowerCase() : "";
  if (name.includes("windows")) return "cmd.exe /c echo hello";
  if (name !== "") return "uname -a";
  return "program and arguments";
}

/// The sentence for a failed `runs.exec`. A console timeout is not a device refusal: the proxy
/// stopped waiting for an answer while the command may still be running, and its history row can
/// land later. Every other failure is the device's own sentence.
function runFailure(cause: unknown): string {
  const sentence = message(cause);
  // Two console answers mean "I stopped waiting", and neither is the device refusing: the proxy's own
  // backstop (`device timeout`) and the channel's answer bound (`device did not answer`). The command
  // is not cancelled by either — it keeps running on the device and its history row lands when it ends.
  if (sentence === "device timeout" || sentence === "device did not answer") {
    return (
      "The console stopped waiting for the device: the command may still be running there, and its " +
      "history row may appear when it finishes. Refresh the history to look for it."
    );
  }
  if (cause instanceof ToolError && cause.status === 502) {
    return (
      "The device disconnected before the console heard the result: the command may have run. " +
      "Check the history before running it again."
    );
  }
  return "Could not run the command: " + sentence;
}
