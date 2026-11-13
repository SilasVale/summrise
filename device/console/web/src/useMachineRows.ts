import { useEffect, useMemo, useState } from "react";
import { api, type AuditLine, type MachineReadAnswer } from "./api";
import { readFailureSentence } from "./joinMarks";
import { machineCallRows, MACHINE_ROWS, type MachineReadState } from "./machineRows";

/**
 * **THE MACHINE'S WINDOW IS READ ONCE, AND REMEMBERED (round 8).**
 *
 * Activity redraws on every live line and every five-second poll. The machine's journal does not
 * change on this page's schedule, so asking on every render would cost a scan of its journal per row
 * drawn. One read per machine is remembered for half a minute, exactly as the join's answers are.
 *
 * Four facts this hook keeps apart, because the page must not confuse them:
 *
 * 1. **No machine asks nothing.** A fleet window spans machines, so no single journal answers for it.
 *    The result is `idle`, no rows, no sentence.
 * 2. **A read in flight is not an empty journal.** `reading` draws no claim.
 * 3. **A failed read is not an empty journal either.** It keeps the console's sentence, which the
 *    footer states, rather than leaving the page to read "nothing happened".
 * 4. **`unknown` is the machine's answer, not a failure.** An older machine that does not offer the
 *    tool answered; the row set is empty, and the answer says so.
 */

/** How long one machine's window is trusted before it is asked for again. */
export const MACHINE_ROWS_CACHE_MS = 30_000;

type Entry = {
  answer: MachineReadAnswer | null;
  state: MachineReadState;
  error: string | null;
  /** When the read started, in the page's own clock. */
  at: number;
};

/** Per machine, for the life of the page. */
const cache = new Map<string, Entry>();

/**
 * The read each machine has in flight.
 *
 * A mount can begin a read and be replaced before the answer lands (a route switch that unmounts the
 * page, a strict-mode remount). The cache entry would then say `reading` forever to the new mount,
 * because the answer re-renders the component that asked. Keeping the promise lets any later mount
 * wait on the same read instead of starting a second one — one read per machine is the rule the
 * cache exists for.
 */
const pending = new Map<string, Promise<void>>();

/** Test seam: this is module state, and a test must start from a clean console. */
export function resetMachineRows() {
  cache.clear();
  pending.clear();
}

export type MachineRead = {
  /** The machine's calls the console's record does not hold, newest first. */
  rows: AuditLine[];
  state: MachineReadState;
  /** The console's sentence for a failed read, or null. */
  error: string | null;
  answer: MachineReadAnswer | null;
};

const NOTHING: MachineRead = { rows: [], state: "idle", error: null, answer: null };

/**
 * Read one machine's journal window.
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
      answer: entry?.answer ?? null,
      state: "reading",
      error: entry?.error ?? null,
      at: now
    });
    setVersion((value) => value + 1);
    const read = api
      .journalRead(deviceId, { limit: MACHINE_ROWS })
      .then((answer: MachineReadAnswer) => {
        cache.set(deviceId, { answer, state: "read", error: null, at: Date.now() });
        // The answer may land after this effect's cleanup (the cache's `reading` state already
        // re-rendered once); the render it must reach is the one that reads the cache, not this run.
        setVersion((value) => value + 1);
      })
      .catch((cause: unknown) => {
        cache.set(deviceId, {
          answer: null,
          state: "failed",
          error: readFailureSentence(cause),
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

  const entry = deviceId === null ? undefined : cache.get(deviceId);
  const rows = useMemo(
    () =>
      deviceId === null || entry?.answer === null || entry?.answer === undefined
        ? []
        : machineCallRows(entry.answer, deviceId, deviceName ?? deviceId),
    [deviceId, deviceName, entry?.answer]
  );
  if (deviceId === null || entry === undefined) return NOTHING;
  return { rows, state: entry.state, error: entry.error, answer: entry.answer };
}
