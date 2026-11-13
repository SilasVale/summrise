import type {
  AuditLine,
  Device,
  MachineDayAnswer,
  MachineReadAnswer
} from "./api";
import { shortId } from "./format";
import { machineCallRows } from "./machineRows";

/**
 * **THE FLEET'S RECORD (round 13).** Rounds 8–12 made one machine's own journal readable and drew
 * it on that machine's window, and counted its day. A fleet window still drew only the console's
 * rows, because no single journal answers for a fleet, and its day counted only the console's audit.
 * This module is the page's answer to that: a bounded plan over the fleet's online machines, the
 * rows their newest pages add, the sum of their direct calls, and the sentences that say what the
 * fan-out did and did not read.
 *
 * Five facts the functions keep apart, each one a test:
 *
 * 1. **A machine that was not asked is not a machine whose journal was empty.** Offline machines
 *    cannot answer; a fleet larger than the cap is cut at a stated number; both are named.
 * 2. **A read in flight is not a "not counted".** The day note stays silent while a read the page
 *    started is still out, so a slow machine is not reported as a machine that failed.
 * 3. **A journal-less machine is complete, not missing.** Its calls are all in the console's audit.
 * 4. **The console's row wins the same call.** The machine's journal holds every proxied call too;
 *    the trace is the join, and a row without one falls back to its device, its clock, its tool and
 *    its outcome — never to two machines' clocks alone.
 * 5. **The day's number is a floor when anything was cut or missed**, and the sentences say so.
 */

/**
 * The most machines a fleet window asks in one fan-out.
 *
 * A fixed number is a cost the page can state: each asked machine costs one `journal.read` and one
 * `journal.day` per 30 s, on top of the console's own audit read. The cap is on *asking*, not on
 * drawing — a bigger fleet still draws every console row, and the machines beyond the cap are named
 * as not asked so the operator knows which direct calls only a machine window can show.
 */
export const FLEET_MACHINES = 8;

/** A fleet window's machines, split by what the page can ask. */
export type FleetPlan = {
  /** Online machines, in the fleet's order, at most [`FLEET_MACHINES`]. */
  asked: Device[];
  /** Online machines the cap left out, in the fleet's order. */
  beyond: Device[];
  /** Machines the fleet knows but that are not online: a read would only wait for a timeout. */
  offline: Device[];
};

/**
 * The machines a fleet window asks, in the fleet's own order.
 *
 * The order is the fleet list's, not this page's: the cap falls on the same machines on every
 * render, so the window does not reshuffle which direct calls it shows. Offline machines are kept
 * apart from the ones beyond the cap because they are two different facts: one cannot answer, the
 * other was not asked.
 */
export function fleetPlan(devices: readonly Device[]): FleetPlan {
  const online = devices.filter((device) => device.online);
  return {
    asked: online.slice(0, FLEET_MACHINES),
    beyond: online.slice(FLEET_MACHINES),
    offline: devices.filter((device) => !device.online)
  };
}

/**
 * One machine's two reads for a fleet window.
 *
 * Both facts stay apart, exactly as they do in the machine window: the page's newest journal page
 * answers "what did this machine run", and `journal.day` answers "how many of today's calls came
 * straight to it". Either can fail or be `unknown` on its own.
 */
export type FleetMachineRead = {
  device: Device;
  page: {
    state: "idle" | "reading" | "read" | "failed";
    answer: MachineReadAnswer | null;
    error: string | null;
  };
  day: {
    state: "idle" | "reading" | "read" | "failed";
    answer: MachineDayAnswer | null;
    error: string | null;
  };
};

/** What a row's machine is called: the fleet's name when it has one, else the id's short form. */
function label(device: Device): string {
  return device.name ?? shortId(device.id);
}

/** Whether this machine answered for its journal window with something the page can draw. */
function pageAnswered(read: FleetMachineRead): boolean {
  const answer = read.page.answer;
  return read.page.state === "read" && answer !== null && !answer.unknown && answer.journal;
}

/** Whether this machine answered for its day with counts the page can add. */
function dayAnswered(read: FleetMachineRead): boolean {
  const answer = read.day.answer;
  return read.day.state === "read" && answer !== null && !answer.unknown && answer.journal;
}

/** Whether this machine says it keeps no journal at all: a fact, and a complete day. */
function journalLess(read: FleetMachineRead): boolean {
  const answer = read.day.answer;
  return (
    read.day.state === "read" && answer !== null && !answer.unknown && !answer.journal
  );
}

/**
 * The calls the machines' own journals hold, as rows of the timeline.
 *
 * Only the doors the console cannot have are drawn (`machineCallRows` owns that rule); a machine
 * that did not answer contributes nothing here and is named by the footer instead.
 */
export function fleetMachineRows(reads: readonly FleetMachineRead[]): AuditLine[] {
  const rows: AuditLine[] = [];
  for (const read of reads) {
    if (!pageAnswered(read)) continue;
    rows.push(...machineCallRows(read.page.answer, read.device.id, label(read.device)));
  }
  return rows;
}

/**
 * The fleet's day, as the machines that answered counted it.
 *
 * Only `loopback` is added: `panel` is the machine's own desktop, `mesh` a peer device, `console`
 * this console's proxying — none of them is a client call the console's audit missed. `more` is any
 * machine's cut scan, and `unread` counts every machine whose direct calls are missing from the
 * number: a failed or `unknown` day among the asked, an offline machine, and one beyond the cap.
 * `null` when no machine answered at all: there is no number to print, only a note.
 */
export type FleetDay = {
  calls: number;
  failed: number;
  more: boolean;
  /** Machines whose direct calls are missing from this number. */
  unread: number;
};

export function fleetDay(reads: readonly FleetMachineRead[], plan: FleetPlan): FleetDay | null {
  const answered = reads.filter(dayAnswered);
  if (answered.length === 0) return null;
  let calls = 0;
  let failed = 0;
  let more = false;
  for (const read of answered) {
    const answer = read.day.answer as MachineDayAnswer;
    calls += answer.calls.loopback.calls;
    failed += answer.calls.loopback.failed;
    more = more || answer.more;
  }
  return { calls, failed, more, unread: notCounted(reads, plan) };
}

/** How many machines' direct calls the fleet's day does not hold. */
function notCounted(reads: readonly FleetMachineRead[], plan: FleetPlan): number {
  const missing = plan.asked.filter((device) => {
    const read = reads.find((entry) => entry.device.id === device.id);
    if (read === undefined) return true;
    if (read.day.state === "reading" || read.day.state === "idle") return false;
    if (dayAnswered(read) || journalLess(read)) return false;
    return true;
  });
  return missing.length + plan.beyond.length + plan.offline.length;
}

/**
 * The header's note when the fleet's day is not the whole day.
 *
 * A count, not a list: the footer names the machines and their reasons once, and a sentence beside
 * the day that repeated every name would make the header unreadable. The note is silent while a read
 * the page started is still out, because "not counted" describes a read that ended badly — not one
 * that is still running.
 */
export function fleetDayNote(
  reads: readonly FleetMachineRead[],
  plan: FleetPlan
): string | null {
  if (reads.some((read) => read.day.state === "reading" || read.page.state === "reading")) {
    return null;
  }
  const missing = notCounted(reads, plan);
  if (missing === 0) return null;
  if (plan.asked.length === 0 && plan.offline.length > 0) {
    return "The direct calls were not counted: no machine in this fleet is online.";
  }
  const machines = missing === 1 ? "1 machine" : `${missing} machines`;
  return `The direct calls of ${machines} were not counted.`;
}

/** Whether any asked machine answered for a journal at all: the empty state's grammar. */
export function fleetJournal(reads: readonly FleetMachineRead[]): boolean {
  return reads.some(pageAnswered);
}

/** Up to three names, then a count: a footer line must stay one line. */
function listNames(names: readonly string[]): string {
  if (names.length <= 3) return names.join(", ");
  return `${names.slice(0, 3).join(", ")} and ${names.length - 3} more`;
}

/**
 * The footer's line for a fleet window, or `null` when there is nothing to say.
 *
 * Three facts, each only when it is not zero: what the machines' records added; which machines'
 * records were not read and why; and the machines the cap left out. A machine whose read is still
 * in flight is not named — the sentence would say it was not read while the page is reading it.
 */
export function fleetSentence(
  reads: readonly FleetMachineRead[],
  plan: FleetPlan,
  contributed: number
): string | null {
  const parts: string[] = [];
  if (contributed > 0) {
    const verb = contributed === 1 ? "is" : "are";
    parts.push(`${contributed} of these calls ${verb} on the machines' records, not this console's.`);
  }
  const unread: string[] = [];
  for (const read of reads) {
    if (read.page.state === "reading" || read.page.state === "idle") continue;
    if (pageAnswered(read) || journalLess(read)) continue;
    const reason =
      read.page.state === "failed" ? "did not answer" : "does not offer its journal";
    unread.push(`${label(read.device)} (${reason})`);
  }
  for (const device of plan.offline) unread.push(`${label(device)} (offline)`);
  if (unread.length > 0) {
    const machines = unread.length === 1 ? "1 machine" : `${unread.length} machines`;
    parts.push(`The journal of ${machines} was not read: ${listNames(unread)}.`);
  }
  if (plan.beyond.length > 0) {
    const machines =
      plan.beyond.length === 1 ? "1 more machine was" : `${plan.beyond.length} more machines were`;
    parts.push(`${machines} not asked (the page reads at most ${FLEET_MACHINES}).`);
  }
  return parts.length === 0 ? null : parts.join(" ");
}

/**
 * The fallback key for a row with no trace: its machine, clock, tool and outcome.
 *
 * The single-machine merge's key is `ts-tool-status`; across a fleet the machine must be part of it,
 * or two machines' calls written on the same millisecond would be treated as one call.
 */
function fleetFallbackKey(line: AuditLine): string {
  return `${line.device_id ?? ""}\u0000${line.ts_ms}\u0000${line.tool ?? ""}\u0000${line.status}`;
}

/**
 * The machine rows the console's window does not already hold.
 *
 * Matched by `trace.call` when both records carry one — two different traces are two calls, whatever
 * their clocks or tools say. The composite key is the fallback for the half that has no trace, and it
 * names the machine. A machine row is also drawn at most once per `(machine, sequence)`: one page
 * read twice, or a repeated row in one page, is one call.
 */
export function fleetContribution(
  consoleLines: AuditLine[],
  machineRows: readonly AuditLine[]
): AuditLine[] {
  if (machineRows.length === 0) return [];
  const traces = new Set<string>();
  const untracedConsoleKeys = new Set<string>();
  const everyConsoleKey = new Set<string>();
  for (const line of consoleLines) {
    const traced = line.kind === "mcp" && typeof line.trace === "string" && line.trace !== "";
    if (traced) traces.add(line.trace as string);
    everyConsoleKey.add(fleetFallbackKey(line));
    if (!traced) untracedConsoleKeys.add(fleetFallbackKey(line));
  }
  const seen = new Set<string>();
  return machineRows.filter((row) => {
    // The machine's sequence is its own; two machines both numbering a row 1 are two rows.
    if (typeof row.machine_seq === "number") {
      const position = `${row.device_id ?? ""}\u0000${row.machine_seq}`;
      if (seen.has(position)) return false;
      seen.add(position);
    }
    const traced = typeof row.trace === "string" && row.trace !== "";
    if (!traced) return !everyConsoleKey.has(fleetFallbackKey(row));
    if (traces.has(row.trace as string)) return false;
    return !untracedConsoleKeys.has(fleetFallbackKey(row));
  });
}

/**
 * The console's rows and the fleet's machine rows, with the same call drawn once.
 *
 * The console's row always wins — it is the richer record, and the one an operator acts on — and
 * the machine rows are inserted in time order so the page's composition still sees one newest-first
 * list.
 */
export function mergeFleetRows(
  consoleLines: AuditLine[],
  machineRows: readonly AuditLine[]
): AuditLine[] {
  if (machineRows.length === 0) return consoleLines;
  const contributed = fleetContribution(consoleLines, machineRows);
  if (contributed.length === 0) return consoleLines;
  return [...consoleLines, ...contributed].sort((left, right) => right.ts_ms - left.ts_ms);
}
