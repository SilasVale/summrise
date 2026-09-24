// useOperationRuns — the device's operation timeline, polled for the run strip.
//
// WHY A SECOND POLL RATHER THAN A PROP. A run is DEVICE-level and crosses
// sessions — the same AI execution may open three terminals and drive the
// browser in between — so the strip needs a source that is not one session's
// file: `GET /api/operation` merges the terminal audit trail and the browser
// action feed onto one ordered axis and reports the run boundaries beside them.
//
// THIS COMMENT USED TO SAY the session route "carries no `run_id` at all". It
// does: `run_id` is written onto every `command/start` the device executes under
// a run, and the route serves the event verbatim. What was missing was on THIS
// side — `CommandEvent` did not declare the field, so the panel dropped an
// attribution the device had already recorded. A stated absence that is not true
// is worse than an unknown one: it explains the second poll away and stops
// anyone looking for the value that is already on the wire.
//
// The two sources are still both needed, for the reason that survives the
// correction: `/api/operation` is retained for a DAY and read-capped, while the
// session trail is the 30-day record. The strip shows what is HAPPENING; the
// trail shows what HAPPENED, and it can now say which run it belonged to.
//
// Polling follows the panel's established discipline:
//   * `since_ms` is the reply's own `cursor_ms`, so each poll asks only for what
//     it has not seen and the accumulated list stays bounded by traffic, not by
//     uptime;
//   * a FAILED poll keeps the last good snapshot instead of blanking the strip
//     (a tunnel blip must not erase runs the operator is reading — the same
//     stance as useSessions and useCommandEvents);
//   * an in-flight reply that lands after unmount is dropped rather than
//     calling setState on a dead component.
//
// THE LAST TWO ARE `useDeviceRead`'S NOW (see its header: keep-last-on-failure and the unmount
// guard), and so are the ordering guard this hook never had and the cadence floor it never had.
// The cursor bullet is the part that stays here, because a cursor is the caller's own state — it
// is what the `path` FUNCTION reads and what the `reduce` advances.
import { useEffect, useRef } from "react";
import { useDeviceRead } from "./useDeviceRead";
import type { OperationEvent, RunBoundary } from "../lib/runs";

interface OperationSnapshot {
  events: OperationEvent[];
  boundaries: RunBoundary[];
}

/** How often to ask while the Path view is on screen. A run boundary is a
 *  low-frequency event (a client declares one, works, declares the end), so
 *  this is a freshness floor, not a stream: the panel's live views keep their
 *  own faster paths. */
const OPERATION_POLL_MS = 5000;

/** Rows per request. Bounded so one poll cannot walk an unbounded log — the
 *  device caps the reply too, and this only has to be small enough to be cheap
 *  and large enough that a run's events are not cut in half. */
const PAGE_LIMIT = 500;

/** Tail caps on what the strip accumulates between polls. The device's own
 *  reads are capped, so these only bite on a very long-lived panel; the tail
 *  wins for the same reason it does in useCommandEvents — the newest work is
 *  what the operator is looking at. */
const MAX_EVENTS = 2000;
const MAX_BOUNDARIES = 200;

const EMPTY: OperationSnapshot = { events: [], boundaries: [] };

/** Identity of one event for de-duplication.
 *
 *  The boundary records are re-sent whenever they sit exactly ON the cursor
 *  (`since_ms` filters `ts_ms < since_ms`, so an event stamped at the cursor is
 *  sent again), and the terminal half is re-read from whole files. `seq`
 *  disambiguates the terminal feed; the browser feed has no sequence, so its key
 *  falls back to the script text. */
function eventKey(e: OperationEvent): string {
  return [
    e?.source ?? "",
    e?.ts_ms ?? 0,
    e?.session ?? "",
    e?.kind ?? "",
    e?.seq ?? "",
    e?.command ?? e?.script ?? "",
  ].join("\u0000");
}

function boundaryKey(b: RunBoundary): string {
  return [b?.kind ?? "", b?.run_id ?? "", b?.ts_ms ?? 0].join("\u0000");
}

/** Merge a reply into the accumulated events, keeping arrival order and
 *  dropping only records already held.
 *
 *  Returns `prev` ITSELF when nothing was added: the poll runs every few
 *  seconds against a mostly-static log, and a fresh array each time would
 *  re-render the whole strip (and the Path view around it) for no new fact —
 *  the same "nothing new, skip the render" rule useCommandEvents follows. */
function mergeEvents(prev: OperationEvent[], incoming: OperationEvent[]): OperationEvent[] {
  const seen = new Set(prev.map(eventKey));
  const next = [...prev];
  for (const e of incoming) {
    const k = eventKey(e);
    if (seen.has(k)) continue;
    seen.add(k);
    next.push(e);
  }
  if (next.length === prev.length) return prev;
  return next.length > MAX_EVENTS ? next.slice(next.length - MAX_EVENTS) : next;
}

/** Merge run boundaries, keyed by (kind, run_id, ts_ms) so a boundary already
 *  seen is never counted twice. Returns `prev` itself when nothing changed. */
function mergeBoundaries(prev: RunBoundary[], incoming: RunBoundary[]): RunBoundary[] {
  const byKey = new Map<string, RunBoundary>();
  for (const b of prev) byKey.set(boundaryKey(b), b);
  let added = false;
  for (const b of incoming) {
    if (typeof b?.run_id !== "string" || typeof b?.ts_ms !== "number") continue;
    const k = boundaryKey(b);
    if (!byKey.has(k)) added = true;
    byKey.set(k, b);
  }
  if (!added) return prev;
  const out = [...byKey.values()];
  if (out.length <= MAX_BOUNDARIES) return out;
  return out
    .sort((a, b) => (a.ts_ms ?? 0) - (b.ts_ms ?? 0))
    .slice(out.length - MAX_BOUNDARIES);
}

/**
 * The accumulated operation timeline: events plus run boundaries.
 *
 * Returns an empty snapshot until the first reply lands, and keeps returning
 * the last good one after a failed poll.
 */
export function useOperationRuns(pollMs: number = OPERATION_POLL_MS): OperationSnapshot {
  // The newest stamp the device has already given us. Held in a ref because it
  // must survive re-renders without re-arming the effect — and read INSIDE the
  // `path` FUNCTION, so each read asks from where the last reply ended.
  const cursorRef = useRef(0);

  // THE POLL, THE UNMOUNT GUARD, THE ORDERING GUARD AND THE CADENCE FLOOR ARE
  // `useDeviceRead`'s (see its header). Two things stay here, both of them the caller's own:
  //
  //   * THE ROUTE IS A FUNCTION, which is the whole reason `useDeviceRead` accepts one. The
  //     cursor advances per read (`since_ms` is the reply's own `cursor_ms`), so a string
  //     captured at mount would pin `since_ms=0` and re-request the first window forever. The
  //     module resolves the function AT READ TIME, so this asks only for what it has not seen.
  //
  //   * THE MERGE IS THE `reduce`, AND IT UPDATES `cursorRef` — the one thing a cursor reader
  //     does inside `reduce`. It is allowed because `reduce` is this hook's own closure over
  //     this hook's own ref, and it is still ONE LOOP: the module reads, folds and writes, and
  //     the ref is simply part of the fold's state (it is the cursor the next `path()` reads).
  //     The fold returns `prev` ITSELF when nothing changed, and that object identity is the
  //     render-skipping contract the strip depends on — the module writes whatever `reduce`
  //     returns straight back into its state, so an unchanged merge re-renders nothing.
  const { data, refresh } = useDeviceRead<OperationSnapshot>({
    path: () => `/api/operation?since_ms=${cursorRef.current}&limit=${PAGE_LIMIT}`,
    reduce: (prev, body) => {
      const res = body as { events?: unknown; runs?: unknown; cursor_ms?: unknown } | null;
      // NO `deviceRefused` GUARD HERE, DELIBERATELY (round 232). A `{ok:false}` yields `[]`, and this
      // hook MERGES (`mergeEvents(prev.events, [])` returns `prev`), so a refusal cannot blank the
      // timeline — a mutation that removed the guard changed no test. The comment that used to sit here
      // claimed a refusal on the first poll "drew an empty timeline"; the snapshot simply stays EMPTY,
      // which is what it was before the poll answered. `useSessionArchive` needs its throw because it
      // REPLACES its entries; this one accumulates, so the failure mode the throw guards against cannot
      // arise. The mocks carry `ok: true` because the DEVICE sends it, not because this reads it.
      //
      // AND WITH THE LOOP IN `useDeviceRead` THE GUARD IS NOW IMPOSSIBLE TO WRITE HERE AT ALL: the
      // module folds a refusal into `"unreadable"` and never calls `reduce`, so the refusal cannot
      // reach the merge — the same outcome, reached by construction rather than by argument.
      const events: OperationEvent[] = Array.isArray(res?.events) ? res.events : [];
      const boundaries: RunBoundary[] = Array.isArray(res?.runs) ? res.runs : [];
      // `cursor_ms` falls back to the requested `since_ms` on the device, so
      // it can never rewind; the guard makes that a property of the client
      // too, since a rewind would re-request (and re-merge) old history.
      const cursor = Number(res?.cursor_ms);
      if (Number.isFinite(cursor) && cursor >= cursorRef.current) {
        cursorRef.current = cursor;
      }
      const merged = mergeEvents(prev.events, events);
      const bounds = mergeBoundaries(prev.boundaries, boundaries);
      if (merged === prev.events && bounds === prev.boundaries) return prev;
      return { events: merged, boundaries: bounds };
    },
    initial: EMPTY,
    everyMs: pollMs,
  });

  // Coming back to the tab is worth an immediate look: the operator who
  // returns after a while is exactly who wants to know what ran meanwhile.
  // The timer is the module's; this focus refresh is the one the module's
  // `everyMs` cannot express, so it stays here.
  useEffect(() => {
    const onVisible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => document.removeEventListener("visibilitychange", onVisible);
  }, [refresh]);

  return data;
}
