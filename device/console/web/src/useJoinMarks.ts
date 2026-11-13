import { useEffect, useState } from "react";
import { api, type AuditLine, type JoinAnswer } from "./api";
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
export function useJoinMarks(deviceId: string | null, lines: readonly AuditLine[]): JoinState {
  const traces = deviceId === null ? [] : joinTraces(lines);
  // Re-render when an answer lands: the map above is module state, so nothing else would tell React
  // that the marks changed.
  const [answers, setAnswers] = useState(0);
  const key = traces.join("\u0000");

  useEffect(() => {
    if (deviceId === null || traces.length === 0) return;
    const state = stateFor(deviceId);
    const aged = Date.now() - state.at >= JOIN_CACHE_MS;
    const stale = traces.filter((trace) => aged || !state.kinds.has(trace));
    if (stale.length === 0) {
      // **A PAGE LEFT OPEN IS NOT A PAGE THAT STOPPED ASKING.** The answer in hand is good until it
      // ages; nothing else would wake this effect, because the window has not changed. This is the
      // wake-up, so a call that rotates out of the machine's journal while the operator is reading
      // gains its mark without a reload.
      const remaining = Math.max(0, state.at + JOIN_CACHE_MS - Date.now()) + JOIN_DEBOUNCE_MS;
      const timer = window.setTimeout(() => setAnswers((value) => value + 1), remaining);
      return () => window.clearTimeout(timer);
    }
    const ask = stale.slice(0, JOIN_TRACES);
    let live = true;
    const timer = window.setTimeout(() => {
      api
        .auditJoin(deviceId, ask)
        .then((answer: JoinAnswer) => {
          const target = stateFor(deviceId);
          for (const trace of ask) remember(deviceId, trace, joinKind(answer, trace));
          target.at = Date.now();
          target.error = null;
          if (live) setAnswers((value) => value + 1);
        })
        .catch((cause: unknown) => {
          const target = stateFor(deviceId);
          // A failed read is remembered as `unknown` for the same window as an answer, so an offline
          // machine is not asked again on every render. `unknown` draws no mark, which is the point:
          // the console did not learn anything about these calls.
          for (const trace of ask) remember(deviceId, trace, "unknown");
          target.at = Date.now();
          target.error = readFailureSentence(cause);
          if (live) setAnswers((value) => value + 1);
        });
    }, JOIN_DEBOUNCE_MS);
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
    // `key` is the trace set's own content, and `answers` is how a landed answer reaches this render.
    // `traces` is rebuilt every render; depending on it would re-ask on every keystroke elsewhere.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deviceId, key, answers]);

  const state = deviceId === null ? null : learned.get(deviceId) ?? null;
  const kinds = new Map<string, JoinKind>();
  for (const trace of traces) kinds.set(trace, state?.kinds.get(trace) ?? "unknown");
  return {
    kinds,
    counts: joinCounts(traces, kinds),
    traces,
    error: state?.error ?? null
  };
}
