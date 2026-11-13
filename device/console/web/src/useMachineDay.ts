import { useEffect, useState } from "react";
import { api, type MachineDayAnswer } from "./api";
import { readFailureSentence } from "./joinMarks";

/**
 * **THE MACHINE'S OWN DAY (round 12).**
 *
 * The console's day sentence counts its audit, and the hub serves the same number for the same day.
 * A client may call the machine directly, though, and those calls exist only in its journal; a page of
 * rows is not a day. `journal.day` answers the question a window cannot: how many calls finished
 * inside the two instants the page counts, by the door they arrived by, and how many failed.
 *
 * The read is one request per `(machine, from, to)` and is trusted for half a minute, exactly like the
 * machine's window: a page redraws on every live line, and a day does not change on that schedule.
 *
 * Two facts stay apart: **a failed read is not an empty day** (it keeps the console's sentence and
 * says why), and **`unknown` is an answer** (an older machine that does not offer the tool; the day is
 * not counted, not reported as zero).
 */

/** How long one day's answer is trusted before it is asked for again. */
export const MACHINE_DAY_CACHE_MS = 30_000;

type Entry = {
  answer: MachineDayAnswer | null;
  state: "reading" | "read" | "failed";
  error: string | null;
  /** When the read started, in the page's own clock. */
  at: number;
};

/** Per `(machine, from, to)`, for the life of the page. */
const cache = new Map<string, Entry>();

/** The read each key has in flight, so a remount waits on it instead of asking twice. */
const pending = new Map<string, Promise<void>>();

/** Test seam: this is module state, and a test must start from a clean console. */
export function resetMachineDays() {
  cache.clear();
  pending.clear();
}

export type MachineDayRead = {
  state: "idle" | "reading" | "read" | "failed";
  answer: MachineDayAnswer | null;
  /** The console's sentence for a failed read, or null. */
  error: string | null;
};

const NOTHING: MachineDayRead = { state: "idle", answer: null, error: null };

function keyOf(deviceId: string, from: number, to: number): string {
  return `${deviceId}\u0000${from}\u0000${to}`;
}

/** Read one machine's count of one day. */
export function useMachineDay(
  deviceId: string | null,
  from: number | null,
  to: number | null
): MachineDayRead {
  const [version, setVersion] = useState(0);
  const key = deviceId === null || from === null || to === null ? null : keyOf(deviceId, from, to);

  useEffect(() => {
    if (deviceId === null || from === null || to === null || key === null) return;
    const entry = cache.get(key);
    const now = Date.now();
    if (entry !== undefined && entry.state !== "reading" && now - entry.at < MACHINE_DAY_CACHE_MS) {
      const wait = Math.max(0, entry.at + MACHINE_DAY_CACHE_MS - now);
      const timer = window.setTimeout(() => setVersion((value) => value + 1), wait + 1);
      return () => window.clearTimeout(timer);
    }
    const inFlight = pending.get(key);
    if (inFlight !== undefined) {
      let live = true;
      void inFlight.then(() => {
        if (live) setVersion((value) => value + 1);
      });
      return () => {
        live = false;
      };
    }
    cache.set(key, { answer: entry?.answer ?? null, state: "reading", error: null, at: now });
    setVersion((value) => value + 1);
    const read = api
      .journalDay(deviceId, from, to)
      .then((answer: MachineDayAnswer) => {
        cache.set(key, { answer, state: "read", error: null, at: Date.now() });
        setVersion((value) => value + 1);
      })
      .catch((cause: unknown) => {
        cache.set(key, {
          answer: null,
          state: "failed",
          error: readFailureSentence(cause),
          at: Date.now()
        });
        setVersion((value) => value + 1);
      })
      .finally(() => {
        pending.delete(key);
      });
    pending.set(key, read);
    // `version` is how a landed answer (or a cache expiry) reaches this effect.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deviceId, from, to, version]);

  if (deviceId === null || from === null || to === null || key === null) return NOTHING;
  const entry = cache.get(key);
  if (entry === undefined) return NOTHING;
  return { state: entry.state, answer: entry.answer, error: entry.error };
}
