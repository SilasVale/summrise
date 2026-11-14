import { useEffect, useState } from "react";
import { api, type AuditLine } from "./api";
import {
  JOIN_TRACES,
  joinCounts,
  joinKind,
  joinTraces,
  readFailureSentence,
  type JoinCounts,
  type JoinKind
} from "./joinMarks";

/**
 * **THE JOIN IS ASKED, NOT GUESSED (the ledger's third item).** Every proxied call is written twice —
 * this console's audit line and the machine's own `call.started`/`call.finished|denied` pair — and
 * one `trace.call` names both. This hook asks the machine which of the traces the window is drawing
 * its journal still holds, and keeps the answers so the page can mark the one row that exists in only
 * one record.
 *
 * Four properties it has to keep, and each is a measurement rather than a preference:
 *
 * 1. **A trace is asked about once.** Activity re-renders on every live line and every five-second
 *    poll. Asking the machine again for a trace it already answered would cost a scan of its journal
 *    per row; the answer is remembered, and only traces nobody has asked about go on the wire.
 * 2. **A burst is one read.** A busy client can make ten calls in a second; the debounce below turns
 *    that into one question instead of ten.
 * 3. **An answer ages.** A journal rotates, so a trace it held at 09:00 may be gone by 09:40: after
 *    [`JOIN_CACHE_MS`] the traces still drawn are asked again, and a row can gain its mark later.
 * 4. **A failed read draws no claim.** The rows stay `unknown` and the console's sentence is the
 *    page's fact. The failure is also remembered for the same window, so an offline machine is asked
 *    once per half minute rather than once per render.
 */

/** How long one machine's answer is trusted before the traces still on screen are asked again. */
export const JOIN_CACHE_MS = 30_000;

/**
 * How long a changed window waits before it asks.
 *
 * A client that makes ten calls in a second would otherwise cost ten scans of the machine's journal.
 * Most of a second is short enough that a mark appears while the call is still on screen, and long
 * enough that the burst is one question.
 */
export const JOIN_DEBOUNCE_MS = 750;

/**
 * How many traces one machine's answer keeps.
 *
 * A page left open all day draws calls it will never draw again; without a bound the map would grow
 * with the day. Two thousand is more than a thousand-row window can hold, so eviction can only drop a
 * trace no window still shows — and one that came back would simply be asked about again.
 */
const LEARNED_CAPACITY = 2_000;

type Learned = {
  /** What each asked trace came to. Insertion order is the eviction order. */
  kinds: Map<string, JoinKind>;
  /** When the last answer or failure was recorded, in the console's own clock. */
  at: number;
  /** The console's sentence for the last failed read, or null after an answer. */
  error: string | null;
};

/** Per machine, for the life of the page: what the journal has been asked and what it said. */
const learned = new Map<string, Learned>();

/** Test seam: this is module state, and a test must start from a clean console. */
export function resetJoinMarks() {
  learned.clear();
}

function stateFor(deviceId: string): Learned {
  const existing = learned.get(deviceId);
  if (existing) return existing;
  const fresh: Learned = { kinds: new Map(), at: 0, error: null };
  learned.set(deviceId, fresh);
  return fresh;
}

function remember(deviceId: string, trace: string, kind: JoinKind) {
  const state = stateFor(deviceId);
  state.kinds.delete(trace);
  state.kinds.set(trace, kind);
  while (state.kinds.size > LEARNED_CAPACITY) {
    const oldest = state.kinds.keys().next().value;
    if (oldest === undefined) break;
    state.kinds.delete(oldest);
  }
}

export type JoinState = {
  /** What is known about each trace the window asked about. A missing key is `unknown`. */
  kinds: ReadonlyMap<string, JoinKind>;
  /** The window's counts, for the one sentence under it. */
  counts: JoinCounts;
  /** The traces the window asks about, in draw order. Empty when there is nothing to ask. */
  traces: string[];
  /** The console's sentence for a failed read, or null. The page states it; no row is marked. */
  error: string | null;
};

/**
 * Ask the machine about the traces a window is drawing.
 *
 * `deviceId` is the machine the window is read from, or null for the fleet's window — a fleet window
 * spans machines, so no single journal answers for it and nothing is asked: the rows stay `unknown`,
 * which is the truth (the console did not check them).
 */
/** The traces each machine in the window is asked about, keyed by the machine's id. */
function groupsForWindow(
  deviceId: string | null,
  lines: readonly AuditLine[]
): Map<string, string[]> {
  if (deviceId !== null) {
    const traces = joinTraces(lines);
    return traces.length === 0 ? new Map() : new Map([[deviceId, traces]]);
  }
  // **A FLEET WINDOW HAS NO ONE JOURNAL (round 43).** It spans machines, so each trace is asked of
  // the machine that wrote the row it belongs to; a row written before device ids existed is left
  // unasked (unknown) rather than guessed at.
  const rowsByDevice = new Map<string, AuditLine[]>();
  for (const line of lines) {
    if (line.kind !== "mcp") continue;
    const device = typeof line.device_id === "string" ? line.device_id : "";
    if (device === "") continue;
    const rows = rowsByDevice.get(device);
    if (rows) rows.push(line);
    else rowsByDevice.set(device, [line]);
  }
  const groups = new Map<string, string[]>();
  for (const [device, rows] of rowsByDevice) {
    const traces = joinTraces(rows);
    if (traces.length > 0) groups.set(device, traces);
  }
  return groups;
}

/**
 * Ask each machine in the window about the traces its own rows carry.
 *
 * `deviceId` is the machine the window is read from, or null for the fleet's window — then the
 * window is split by the `device_id` each row carries and every drawn machine is asked for its own
 * calls. The cache, the debounce and the age are per machine, so one offline machine neither asks
 * for the others' traces nor silences them.
 */
export function useJoinMarks(deviceId: string | null, lines: readonly AuditLine[]): JoinState {
  const groups = groupsForWindow(deviceId, lines);
  // Re-render when an answer lands: the maps above are module state, so nothing else would tell
  // React that the marks changed.
  const [answers, setAnswers] = useState(0);
  const key = [...groups]
    .map(([device, traces]) => `${device}:${traces.join("\u0000")}`)
    .join("\u0001");

  useEffect(() => {
    if (groups.size === 0) return;
    const now = Date.now();
    const asks: { device: string; traces: string[] }[] = [];
    let wakeAt = Number.POSITIVE_INFINITY;
    for (const [device, traces] of groups) {
      const state = stateFor(device);
      const aged = now - state.at >= JOIN_CACHE_MS;
      const stale = traces.filter((trace) => aged || !state.kinds.has(trace));
      if (stale.length === 0) {
        wakeAt = Math.min(wakeAt, state.at + JOIN_CACHE_MS + JOIN_DEBOUNCE_MS);
      } else {
        asks.push({ device, traces: stale.slice(0, JOIN_TRACES) });
      }
    }
    if (asks.length === 0) {
      // A page left open is not a page that stopped asking: the answers age, and this is the wake-up
      // that lets a trace rotate out of a machine's journal and gain its mark without a reload.
      const remaining = Math.max(0, wakeAt - Date.now());
      const timer = window.setTimeout(() => setAnswers((value) => value + 1), remaining);
      return () => window.clearTimeout(timer);
    }
    let live = true;
    const timer = window.setTimeout(() => {
      void Promise.all(
        asks.map(async ({ device, traces }) => {
          try {
            const answer = await api.auditJoin(device, traces);
            const target = stateFor(device);
            for (const trace of traces) remember(device, trace, joinKind(answer, trace));
            target.at = Date.now();
            target.error = null;
          } catch (cause: unknown) {
            const target = stateFor(device);
            // A failed read is remembered as `unknown` for the same window as an answer, so an
            // offline machine is not asked again on every render, and no row is accused.
            for (const trace of traces) remember(device, trace, "unknown");
            target.at = Date.now();
            target.error = readFailureSentence(cause);
          }
        })
      ).then(() => {
        if (live) setAnswers((value) => value + 1);
      });
    }, JOIN_DEBOUNCE_MS);
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
    // `key` is the window's own content and `answers` is how a landed answer reaches this render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deviceId, key, answers]);

  const kinds = new Map<string, JoinKind>();
  const traces: string[] = [];
  let error: string | null = null;
  for (const [device, group] of groups) {
    const state = learned.get(device);
    for (const trace of group) {
      traces.push(trace);
      kinds.set(trace, state?.kinds.get(trace) ?? "unknown");
    }
    if (error === null && state?.error) error = state.error;
  }
  return {
    kinds,
    counts: joinCounts(traces, kinds),
    traces,
    error
  };
}
