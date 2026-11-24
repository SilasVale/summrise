import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type AuditLine, type MachineReadAnswer, type MachineRow } from "./api";
import { readFailureSentence } from "./joinMarks";
import { machineRowLines, MACHINE_ROWS, type MachineReadState } from "./machineRows";

/**
 * **THE MACHINE'S WINDOW PAGES (round 10).**
 *
 * Activity asks the machine for its newest 200 journal rows once. On a busy machine that is a few
 * minutes of work, and a device-only call older than that — this morning's panel action, a mesh
 * peer's call — would be unreachable even though the journal still holds it. `journal.read` has taken
 * `before_seq` and answered `more` since it shipped; this hook now uses both.
 *
 * The shape is a **fetch session**, not a cursor on the server: the newest page when the machine
 * scope opens, older pages on request, and a 30 s refresh of the newest page that is merged in front
 * of what was already asked for. Five facts this hook keeps apart:
 *
 * 1. **No machine asks nothing.** A fleet window spans machines, so no single journal answers.
 * 2. **A read in flight is not an empty journal.**
 * 3. **A failed first read is not an empty journal either**; its sentence is the footer's.
 * 4. **A failed older page is not a lost window** — the rows already fetched stay, and the control
 *    becomes a retry with the console's reason.
 * 5. **`unknown` is the machine's answer, not a failure.**
 */

/** How long the newest page is trusted before it is asked for again. */
export const MACHINE_ROWS_CACHE_MS = 30_000;

type Entry = {
  /** The newest page's answer: the envelope facts, not the accumulated rows. */
  answer: MachineReadAnswer | null;
  /** Summaries, newest first, deduplicated by `seq`: the newest page plus the older pages asked for. */
  rows: MachineRow[];
  /** A retained row older than the oldest fetched exists (the last older page's answer). */
  more: boolean;
  /** The smallest machine sequence fetched, or null before the first answer. */
  oldestSeq: number | null;
  /** True once an older page has been asked for: the session is deeper than the newest page. */
  paged: boolean;
  state: MachineReadState;
  error: string | null;
  /** An older page in flight, or the last one's failure. */
  paging: "idle" | "reading" | "failed";
  olderError: string | null;
  /** When the newest page was read, in the page's own clock. */
  at: number;
};

/** Per machine, for the life of the page. */
const cache = new Map<string, Entry>();

/**
 * **A LATE ANSWER BELONGS TO THE SESSION THAT ASKED FOR IT.** A scope change drops the window's
 * pages; an older page still on the wire must not resurrect them into the cache afterwards. This
 * counter is the session identity: every scope change moves it, and a read compares the value it
 * started under before writing anything.
 */
let generation = 0;

/**
 * The newest-page read each machine has in flight.
 *
 * A mount can begin a read and be replaced before the answer lands (a route switch that unmounts the
 * page, a strict-mode remount). The cache entry would then say `reading` forever to the new mount,
 * because the answer re-renders the component that asked. Keeping the promise lets any later mount
 * wait on the same read instead of starting a second one.
 */
const pending = new Map<string, Promise<void>>();

/** Test seam: this is module state, and a test must start from a clean console. */
export function resetMachineRows() {
  cache.clear();
  pending.clear();
  generation += 1;
}

/**
 * **A FETCH SESSION DOES NOT OUTLIVE THE WINDOW IT WAS READ FOR.** The paged rows are history the
 * operator asked for while looking at one machine; leaving the machine window (a scope change, the
 * fleet, another machine) drops them, so re-entering starts from the newest page again — the memory
 * a deep session holds is not kept for a page that is no longer drawn. The 30 s newest-page cache is
 * untouched: it is one page, and it is what makes a remount free.
 */
function dropPages() {
  generation += 1;
  for (const [device, entry] of cache) {
    if (entry.paged) {
      cache.delete(device);
    } else if (entry.paging !== "idle" || entry.olderError !== null) {
      // A page that was in flight for the window just left must not come back as "reading" forever.
      cache.set(device, { ...entry, paging: "idle", olderError: null });
    }
  }
}

function blankEntry(): Entry {
  return {
    answer: null,
    rows: [],
    more: false,
    oldestSeq: null,
    paged: false,
    state: "reading",
    error: null,
    paging: "idle",
    olderError: null,
    at: 0
  };
}

/**
 * Newest first, one summary per sequence; a page that repeats a sequence does not draw it twice.
 *
 * The session is a `seq` window, and the bus has no per-journal epoch yet: a journal reset *within*
 * one session could reuse a sequence and be treated as a repeat. A single-row read-back checks the
 * trace (round 9); a page is a summary window and does not. Leaving the machine window starts a fresh
 * session, which is where a reset becomes visible.
 */
function mergePages(newer: readonly MachineRow[], older: readonly MachineRow[]): MachineRow[] {
  const seen = new Set<number>();
  const rows: MachineRow[] = [];
  for (const row of [...newer, ...older]) {
    if (seen.has(row.seq)) continue;
    seen.add(row.seq);
    rows.push(row);
  }
  return rows;
}

function oldestOf(rows: readonly MachineRow[]): number | null {
  let oldest: number | null = null;
  for (const row of rows) {
    if (oldest === null || row.seq < oldest) oldest = row.seq;
  }
  return oldest;
}

export type MachineRead = {
  /** The machine's calls the console's record does not hold, newest first. */
  rows: AuditLine[];
  state: MachineReadState;
  /** The console's sentence for a failed first read, or null. */
  error: string | null;
  answer: MachineReadAnswer | null;
  /** A retained row older than the oldest fetched exists. */
  more: boolean;
  /** An older page was asked for: the session is deeper than the newest page. */
  paged: boolean;
  /** One older page is on the wire. */
  readingOlder: boolean;
  /** The console's sentence for a failed older page, or null. */
  olderError: string | null;
  /** Ask for the next older page. A no-op while one is in flight, or when nothing older exists. */
  loadOlder: () => void;
};

const NOTHING: MachineRead = {
  rows: [],
  state: "idle",
  error: null,
  answer: null,
  more: false,
  paged: false,
  readingOlder: false,
  olderError: null,
  loadOlder: () => {}
};

/**
 * Read one machine's journal window, newest first, and page it older.
 *
 * `deviceName` is what the rows' sentences call the machine; the page already resolves an id to the
 * fleet's name, and a name that arrives after the read (the fleet list loads late) rebuilds the
 * sentences without asking the journal again.
 */
export function useMachineRows(
  deviceId: string | null,
  deviceName: string | null = null
): MachineRead {
  const [version, setVersion] = useState(0);
  const shownDevice = useRef<string | null>(null);

  // A scope change is the end of the session: the pages asked for under the previous window are not
  // the next window's history. This runs before the read effect, so the new scope starts unpaged.
  useEffect(() => {
    if (shownDevice.current === deviceId) return;
    // A fresh mount that is about to wait on a read already in flight is the same window, not a
    // re-entry: dropping would invalidate the very answer it waits for. (A remount after a paged
    // session, with nothing in flight, still starts clean.)
    const reusing =
      shownDevice.current === null && deviceId !== null && pending.has(deviceId);
    if (!reusing) dropPages();
    shownDevice.current = deviceId;
  }, [deviceId]);

  useEffect(() => {
    if (deviceId === null) return;
    const entry = cache.get(deviceId);
    const now = Date.now();
    if (entry !== undefined && entry.state !== "reading" && now - entry.at < MACHINE_ROWS_CACHE_MS) {
      // **A PAGE LEFT OPEN IS NOT A PAGE THAT STOPPED READING.** Nothing else would wake this effect,
      // because the scope has not changed. This is the wake-up, so a call made after the first read
      // appears without a reload — and a journal that rotated does not keep being drawn as fact.
      const wait = Math.max(0, entry.at + MACHINE_ROWS_CACHE_MS - now);
      const timer = window.setTimeout(() => setVersion((value) => value + 1), wait + 1);
      return () => window.clearTimeout(timer);
    }
    // A read already out for this machine: wait on the same promise rather than starting a second.
    if (entry?.state === "reading") {
      const wait = pending.get(deviceId);
      if (wait === undefined) return;
      let live = true;
      void wait.then(() => {
        if (live) setVersion((value) => value + 1);
      });
      return () => {
        live = false;
      };
    }
    cache.set(deviceId, {
      ...(entry ?? blankEntry()),
      state: "reading",
      paging: entry?.paging === "reading" ? "reading" : "idle",
      at: now
    });
    setVersion((value) => value + 1);
    const session = generation;
    const read = api
      .journalRead(deviceId, { limit: MACHINE_ROWS })
      .then((answer: MachineReadAnswer) => {
        // The window may have been left while this page was in flight.
        if (session !== generation) return;
        // The page may have added older rows while this refresh was in flight; keep them.
        const current = cache.get(deviceId) ?? blankEntry();
        if (answer.unknown || !answer.journal) {
          // **"NO JOURNAL" DOES NOT RETRACT A WINDOW THAT WAS READ.** A build change mid-session can
          // answer `unknown` (a 200, not a failure). With rows already drawn, the window stays and the
          // footer keeps its state: only a read that never had a window is stated as "not read".
          cache.set(
            deviceId,
            current.rows.length > 0
              ? { ...current, state: "read", error: null, at: Date.now() }
              : {
                  ...current,
                  answer,
                  rows: [],
                  more: false,
                  oldestSeq: null,
                  state: "read",
                  error: null,
                  at: Date.now()
                }
          );
          setVersion((value) => value + 1);
          return;
        }
        const rows = mergePages(answer.rows, current.paged ? current.rows : []);
        cache.set(deviceId, {
          ...current,
          answer,
          rows,
          // A paged session's `more` is the last older page's fact, not the newest page's: the newest
          // page always has rows below it when a session has paged, and that is not news.
          more: current.paged ? current.more : answer.more,
          oldestSeq: oldestOf(rows),
          state: "read",
          error: null,
          at: Date.now()
        });
        // The answer may land after this effect's cleanup (the cache's `reading` state already
        // re-rendered once); the render it must reach is the one that reads the cache, not this run.
        setVersion((value) => value + 1);
      })
      .catch((cause: unknown) => {
        if (session !== generation) return;
        const current = cache.get(deviceId) ?? blankEntry();
        cache.set(deviceId, {
          ...current,
          // A failed refresh keeps a paged session's rows; it only changes the sentence when there is
          // no window at all to describe.
          state: current.rows.length > 0 ? "read" : "failed",
          error: current.rows.length > 0 ? null : readFailureSentence(cause),
          at: Date.now()
        });
        setVersion((value) => value + 1);
      })
      .finally(() => {
        pending.delete(deviceId);
      });
    pending.set(deviceId, read);
    // `version` is how a landed answer (or a cache expiry) reaches this effect.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deviceId, version]);

  const loadOlder = useCallback(() => {
    if (deviceId === null) return;
    const entry = cache.get(deviceId);
    if (entry === undefined || entry.paging === "reading") return;
    if (!entry.more || entry.oldestSeq === null) return;
    const before = entry.oldestSeq;
    const session = generation;
    cache.set(deviceId, { ...entry, paging: "reading", olderError: null });
    setVersion((value) => value + 1);
    api
      .journalRead(deviceId, { before, limit: MACHINE_ROWS })
      .then((answer: MachineReadAnswer) => {
        // The window was left while this page was in flight: it must not resurrect the session.
        if (session !== generation) return;
        const current = cache.get(deviceId) ?? entry;
        if (answer.unknown || !answer.journal) {
          cache.set(deviceId, {
            ...current,
            paging: "failed",
            olderError: answer.unknown
              ? "this machine does not offer its journal"
              : "this machine keeps no journal"
          });
          setVersion((value) => value + 1);
          return;
        }
        const rows = mergePages(current.rows, answer.rows);
        cache.set(deviceId, {
          ...current,
          rows,
          oldestSeq: oldestOf(rows),
          more: answer.more,
          paged: true,
          paging: "idle",
          olderError: null
        });
        setVersion((value) => value + 1);
      })
      .catch((cause: unknown) => {
        if (session !== generation) return;
        const current = cache.get(deviceId) ?? entry;
        cache.set(deviceId, {
          ...current,
          paging: "failed",
          olderError: readFailureSentence(cause)
        });
        setVersion((value) => value + 1);
      });
    // `deviceId` is the question; the entry is read at call time so a page always continues from the
    // deepest sequence fetched, not from the render the handler was created in.
  }, [deviceId]);

  const entry = deviceId === null ? undefined : cache.get(deviceId);
  const rows = useMemo(
    () =>
      deviceId === null || entry === undefined
        ? []
        : machineRowLines(entry.rows, deviceId, deviceName ?? deviceId),
    [deviceId, deviceName, entry?.rows]
  );
  if (deviceId === null || entry === undefined) return NOTHING;
  return {
    rows,
    state: entry.state,
    error: entry.error,
    answer: entry.answer,
    more: entry.more,
    paged: entry.paged,
    readingOlder: entry.paging === "reading",
    olderError: entry.olderError,
    loadOlder
  };
}
