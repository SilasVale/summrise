import type { AuditLine, MachineReadAnswer, MachineRow } from "./api";
import { callSentence } from "./liveActivity";

/**
 * **THE MACHINE'S OWN RECORD ON THE PAGE (round 8).**
 *
 * A call the console proxied is written twice: the console's audit line and the machine's journal
 * pair. But a call that never came through the proxy — the machine's own panel, a mesh peer, a
 * direct loopback client — is written *only* in the machine's journal. Activity draws it from the
 * live relay while the tab is open, and after a reload it is gone, because the console never had it.
 *
 * `GET /journal/read` asks the machine for a bounded window of its own summary rows. This module
 * turns that window into rows the timeline already knows how to draw, and merges them with the
 * console's rows without letting the same call appear twice.
 *
 * The rules are the ones the round's plan pinned, and each is a test:
 *
 * 1. **Only a call's outcome is a row.** `call.finished` / `call.denied` are work; `call.started`,
 *    `session.*`, `plugin.*` and `device.started` are not, because one call is one row and the
 *    Device tab's strip already shows the machine's session and plugin facts.
 * 2. **A machine row reads like the console's row for the same call** — same kind, status, tool,
 *    trace and sentence — so one call reads one way whichever record drew it.
 * 3. **The door is the caller.** A machine row has no console `caller`; the journal's own `door` is
 *    the word the timeline shows where a console row shows `console`. `machine_seq` carries the
 *    row's `seq` for the opened record. **Only a door the console does not own is drawn**
 *    (`panel`, `mesh`, `loopback`): a `console` row is a call the console proxied, and the console's
 *    own record is its authority — it either wrote an audit line (the row is already drawn, and the
 *    join marks it when the machine's copy rotates) or it wrote none because the call was the
 *    console's own read of the machine, which is the page's bookkeeping, not the operator's work.
 * 4. **A console row wins a duplicate.** The machine's journal holds every proxied call too, and the
 *    console's row is the richer one (caller, token, `ms`, evidence). A machine row is dropped when
 *    a console row for the same call exists: matched **by `trace.call`** when both have one, and
 *    otherwise by the composite key the live merge already uses.
 */

/**
 * How many of the machine's newest rows one read asks for.
 *
 * The device refuses more (the console states the same bound before relaying), and this is the window
 * the page draws: a page shows a day's worth of calls, not a machine's whole history. A journal that
 * holds more says `more: true`, which is a fact the page does not have to draw to be honest about.
 */
export const MACHINE_ROWS = 200;

/**
 * What a `journal.read` asked of the console is doing.
 *
 * `idle` is a fleet window or no machine at all: nothing is asked, and the page says nothing. A read
 * that failed is not `idle` — its rows are empty for the same reason a `503` is not an empty audit.
 */
export type MachineReadState = "idle" | "reading" | "read" | "failed";

/** The facts the footer's one line is made of. */
export type MachineCounts = {
  /** How many rows on the page came from the machine's journal alone. */
  contributed: number;
  /** The read's own state: silence for `idle`, a reason for `failed`. */
  state: MachineReadState;
  /** True when the machine answered that it does not offer `journal.read` (or the answer was unusable). */
  unknown: boolean;
  /**
   * True when the machine keeps a journal and answered for it.
   *
   * A `journal: false` machine is a fact, not a failure, but it is also not a record the page read:
   * the empty state must not say a direct call "is read from that machine's journal" there.
   */
  journal: boolean;
  /** The console's sentence for a failed read, already in the page's voice. */
  error: string | null;
};

/** The counts of a window nothing was asked about. */
export const EMPTY_MACHINE: MachineCounts = {
  contributed: 0,
  state: "idle",
  unknown: false,
  journal: false,
  error: null
};

/**
 * The doors that prove the console never proxied a call.
 *
 * A journal row's door is the machine's own word for how the call arrived, and `console` means the
 * console's channel — so that call has a console record, one way or another. Drawing it from the
 * journal would either duplicate an audit row the merge could not match (a rotated audit) or, worse,
 * draw the console's own reads of the machine (`tools/list`, `journal.get`, this very read) as if
 * they were the operator's work. The journal is drawn for the doors the console cannot have.
 */
const DEVICE_DOORS: ReadonlySet<string> = new Set(["panel", "mesh", "loopback"]);

/** A call row as the timeline draws it, built from the machine's summary of the call. */
function callRow(row: MachineRow, deviceId: string, deviceName: string): AuditLine | null {
  // The console records a tool-level failure as 424, so a call the machine refused reads failed with
  // every other failed call in the list rather than as a green 200.
  const ok = row.ok === true;
  const type = row.type === "call.finished" || row.type === "call.denied" ? row.type : null;
  if (type === null) return null;
  if (typeof row.tool !== "string" || row.tool === "") return null;
  // A door the console does not own, or no door at all: without one the row cannot prove the console
  // never had it, and drawing it would guess at the one fact the row exists to state.
  if (!DEVICE_DOORS.has(row.door ?? "")) return null;
  // The machine's `ts` is its own clock. A row whose time cannot be read falls back to this page's,
  // which is newer than the row, never older — the same rule the live device rows use.
  const parsed = Date.parse(row.ts);
  const ts_ms = Number.isFinite(parsed) ? parsed : Date.now();
  return {
    ts_ms,
    kind: "mcp",
    status: ok ? 200 : 424,
    ok,
    device_id: deviceId,
    tool: row.tool,
    ...(typeof row.trace === "string" && row.trace !== "" ? { trace: row.trace } : {}),
    ...(typeof row.door === "string" && row.door !== "" ? { door: row.door } : {}),
    machine_seq: row.seq,
    // The same function the live rows use, not a second wording: the record is the machine's but the
    // sentence the reader sees must not depend on which of the two records drew the call.
    sentence: callSentence(deviceName, type, row.tool, row.reason)
  };
}

/**
 * The machine's window as rows, or `[]` when the window says nothing can be drawn.
 *
 * An `unknown` answer and a machine with no journal draw nothing — but they are not silence, which is
 * why the caller also keeps the answer's reason: the footer states the read that did not happen.
 * A `journal: false` machine is not an error, just a machine whose build keeps no journal.
 */
export function machineCallRows(
  answer: MachineReadAnswer | null,
  deviceId: string,
  deviceName: string
): AuditLine[] {
  if (answer === null || answer.unknown || !answer.journal) return [];
  const rows: AuditLine[] = [];
  for (const row of answer.rows) {
    // A window arrives newest first; mapping in order keeps it that way.
    const line = callRow(row, deviceId, deviceName);
    if (line !== null) rows.push(line);
  }
  return rows;
}

/**
 * The key the live merge uses for a row with no trace: `ts-tool-status`.
 *
 * It is only a fallback. Two records' clocks are not the same clock, so a proxied call's two rows
 * rarely land on the same millisecond; the trace is the join key that actually works, and this key
 * catches the rows written before the trace existed.
 */
function fallbackKey(line: AuditLine): string {
  return `${line.ts_ms}-${line.tool ?? ""}-${line.status}`;
}

/**
 * The console's rows and the machine's, with the same call drawn once.
 *
 * The console's row always wins when both records hold a call: it is the richer one, and it is the
 * record an operator acts on. The machine's rows keep their own order and are inserted in time order,
 * so the page's composition (`groupDays`, stretches) still sees one newest-first list.
 */
export function mergeMachineRows(
  consoleLines: AuditLine[],
  machineRows: readonly AuditLine[]
): AuditLine[] {
  if (machineRows.length === 0) return consoleLines;
  const traces = new Set<string>();
  // **ONLY AN UNTRACED CONSOLE ROW CAN MATCH ON THE COMPOSITE KEY.** If both records carry a trace,
  // the trace is the join — two different traces name two different calls, whatever their clocks or
  // tools say. The composite key is the fallback for the half that has no trace to offer, so it is
  // kept for the console rows that wrote none, and for every console row when the machine's row is
  // the one without one.
  const untracedConsoleKeys = new Set<string>();
  const everyConsoleKey = new Set<string>();
  for (const line of consoleLines) {
    const traced = line.kind === "mcp" && typeof line.trace === "string" && line.trace !== "";
    if (traced) traces.add(line.trace as string);
    everyConsoleKey.add(fallbackKey(line));
    if (!traced) untracedConsoleKeys.add(fallbackKey(line));
  }
  const contributed = machineRows.filter((row) => {
    const traced = typeof row.trace === "string" && row.trace !== "";
    if (!traced) return !everyConsoleKey.has(fallbackKey(row));
    // **THE TRACE IS THE JOIN, NOT THE CLOCK.** Both records write one `trace.call` for a proxied
    // call; comparing the machine's `ts` with the console's would be comparing two clocks.
    if (traces.has(row.trace as string)) return false;
    return !untracedConsoleKeys.has(fallbackKey(row));
  });
  if (contributed.length === 0) return consoleLines;
  const merged = [...consoleLines, ...contributed];
  // Stable, so rows that share a millisecond keep the order the console drew them in. The days are
  // composed by walking this list; an out-of-order row would open a second day heading.
  return merged.sort((left, right) => right.ts_ms - left.ts_ms);
}

/** The footer's counts, from the read and how many of its rows the merge kept. */
export function machineCounts(
  read: { state: MachineReadState; error: string | null; answer: MachineReadAnswer | null },
  contributed: number
): MachineCounts {
  const answer = read.answer;
  return {
    contributed,
    state: read.state,
    unknown: answer?.unknown === true,
    journal: read.state === "read" && answer !== null && answer.journal && !answer.unknown,
    error: read.error
  };
}

/**
 * The line under the timeline, or `null` when there is nothing to say.
 *
 * Silence is right for a window whose calls the console's own record already holds: the read worked
 * and needed no sentence. A read that did not happen gets one — an `unknown` answer says the machine
 * does not offer it, a failed read says the console's reason — because an empty journal and a journal
 * nobody could read are different facts, and the page must not let the first be mistaken for the second.
 */
export function machineSentence(counts: MachineCounts): string | null {
  if (counts.state === "failed") {
    const reason = counts.error === null ? "the console could not read it" : counts.error;
    return `The machine's own journal was not read: ${reason.replace(/\.$/, "")}.`;
  }
  if (counts.unknown) {
    return "The machine's own journal was not read: this machine does not offer it.";
  }
  if (counts.contributed <= 0) return null;
  const verb = counts.contributed === 1 ? "is" : "are";
  return (
    `${counts.contributed} of these calls ${verb} on the machine's record, not this console's.`
  );
}

/**
 * What the opened record says about a row only the machine holds, or `null` for any other row.
 *
 * The door is the sentence's second half because it is the fact that explains the missing console
 * line: a call the panel made never had one to write.
 */
export function machineRecordSentence(row: AuditLine): string | null {
  // A `door` is set only on a row the journal drew; an audit line that now carries `machine_seq`
  // is still this console's row, and its sentence is the read-back's, not this one's.
  if (typeof row.machine_seq !== "number" || row.door === undefined) return null;
  const base = "This call is only in the machine's own journal; the console never proxied it.";
  if (typeof row.door !== "string" || row.door === "") return base;
  return `${base} It arrived by the ${row.door}.`;
}
