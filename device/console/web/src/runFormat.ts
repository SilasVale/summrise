/**
 * The `runs.*` wire shapes and the way this console spells them.
 *
 * The exec form (now under Tools) and the history page are two readers of the same three tools, so
 * the shapes and the formatting rules live once, here, instead of being copied into both.
 */

/**
 * `runs.exec`'s answer: the run's outcome and the retained head of each stream.
 *
 * `exit_code` is null both when the timeout killed the process group (`timed_out`) and when the
 * process ended from a signal; the two nulls are different facts and the result panel says which.
 */
export type RunResult = {
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
export type RunRecord = {
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
export const HISTORY_LIMIT = 20;

/// `runs.exec`'s timeout default and ceiling, mirrored from the device so the form states them.
export const DEFAULT_TIMEOUT_MS = 60_000;
export const MAX_TIMEOUT_MS = 600_000;

/** A row's command, shortened for the table; the full line is the button's title and the detail. */
export function shortCommand(line: string): string {
  const one = line.replace(/\s+/g, " ").trim();
  return one.length <= 110 ? one : one.slice(0, 109) + "…";
}

/// A history row's command line: a shell line as it was, or an argv array quoted only where a word
/// would otherwise split. The history does not record whether the shell was used, so this is a
/// readable spelling of the row, not a reconstruction of the request.
export function commandLine(row: RunRecord): string {
  if (row.args.length === 0) return row.command;
  return [row.command, ...row.args]
    .map((part) => (/[\s"']/.test(part) ? JSON.stringify(part) : part))
    .join(" ");
}

/// The exit as one phrase. A timed-out run and a run killed by a signal both have no exit code, and
/// the timeout is the one the page can name.
export function exitSentence(run: { exit_code: number | null; timed_out: boolean }): string {
  if (run.timed_out) return "timed out";
  if (run.exit_code === null) return "no exit status";
  return "exit " + String(run.exit_code);
}

/// The chip that colours the same phrase: 0 is success, any other exit is a failure, and a run with
/// no exit status (a timeout or a signal) is neither.
export function exitChip(run: { exit_code: number | null; timed_out: boolean }): string {
  if (run.timed_out || run.exit_code === null) return "chip chip-warn";
  return "chip " + (run.exit_code === 0 ? "chip-ok" : "chip-bad");
}

/// "42 ms", "1.5 s", "2m 03s" — a duration, read at a glance next to a byte count.
export function spellDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) {
    const seconds = (ms / 1000).toFixed(2).replace(/\.?0+$/, "");
    return `${seconds} s`;
  }
  const seconds = Math.floor(ms / 1000);
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ${String(seconds % 60).padStart(2, "0")}s`;
}

/// "0 B", "812 B", "4.2 KB", "1.0 MB" — the history stores bytes, so the row reports them as such.
export function bytes(count: number): string {
  if (count < 1024) return `${count} B`;
  if (count < 1024 * 1024) return `${(count / 1024).toFixed(1)} KB`;
  return `${(count / (1024 * 1024)).toFixed(1)} MB`;
}
