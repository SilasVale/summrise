import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Device, type MachineDayAnswer, type MachineReadAnswer } from "./api";
import { readFailureSentence } from "./joinMarks";
import { MACHINE_ROWS } from "./machineRows";
import { fleetPlan, type FleetMachineRead, type FleetPlan } from "./fleetRecord";

/**
 * **THE FLEET'S RECORD, ASKED (round 13).**
 *
 * A fleet window spans machines, so no single journal answers for it. This hook runs the bounded
 * fan-out the ledger's item describes: for each online machine the fleet names, at most
 * [`FLEET_MACHINES`](fleetRecord.ts), one `journal.read` of its newest page and one `journal.day`
 * of the block the header counts — one request each, both cached for 30 s, and a re-render is not a
 * refresh.
 *
 * Four facts it keeps apart, each pinned by a test:
 *
 * 1. **Asking is the fleet list's, not this hook's.** Offline machines and the machines past the cap
 *    are in the returned plan, named, and never asked: a read that can only time out is not a read.
 * 2. **A read in flight belongs to the window that asked for it.** The cache is keyed by
 *    `(machine, from, to)`, so a machine's answer from yesterday's block can never answer today's,
 *    and two mounts that ask at once share one request.
 * 3. **One machine's failure is not the fleet's.** Page and day land independently; a machine whose
 *    page failed still tells its day, and vice versa.
 * 4. **A stale answer is asked for again on a timer**, because the scope has not changed and nothing
 *    else would wake the hook — the same rule as the machine window's own page.
 *
 * It deliberately does not share the machine window's cache: that cache holds a paging session with
 * a generation of its own, and this hook never pages. The cost is one duplicate read per scope
 * switch inside 30 s, stated in the round's rulings.
 */

/** How long one machine's pair of answers is trusted before it is asked for again. */
export const FLEET_CACHE_MS = 30_000;

type PageState = {
  state: "idle" | "reading" | "read" | "failed";
  answer: MachineReadAnswer | null;
  error: string | null;
};

type DayState = {
  state: "idle" | "reading" | "read" | "failed";
  answer: MachineDayAnswer | null;
  error: string | null;
};

type Entry = { page: PageState; day: DayState; startedAt: number };

/** Per `(machine, from, to)`, for the life of the page. */
const cache = new Map<string, Entry>();

/** The answers each key has in flight, so a remount waits on the read instead of asking twice. */
const pending = new Map<string, Promise<void>>();

/**
 * Which cache generation a read belongs to. A reset (the test seam) moves it, so a promise still on
 * the wire from a previous console cannot repopulate the cache of the next one.
 */
let generation = 0;

/** Test seam: this is module state, and a test must start from a clean console. */
export function resetFleetRecord() {
  cache.clear();
  pending.clear();
  generation += 1;
}

function keyOf(deviceId: string, from: number, to: number): string {
  return `${deviceId}\u0000${from}\u0000${to}`;
}

function blank(): Entry {
  return {
    page: { state: "reading", answer: null, error: null },
    day: { state: "reading", answer: null, error: null },
    startedAt: 0
  };
}

/**
 * Start one machine's two reads and record each outcome as it lands.
 *
 * The promise resolves when both have landed; its failures are already recorded per read, so the
 * caller never sees a rejected timer-driven read.
 */
function startReads(
  deviceId: string,
  from: number,
  to: number,
  bump: () => void
): Promise<void> {
  const key = keyOf(deviceId, from, to);
  const epoch = generation;
  const update = (change: (entry: Entry) => Entry) => {
    if (epoch !== generation) return;
    const current = cache.get(key);
    if (current === undefined) return;
    cache.set(key, change(current));
    bump();
  };
  const read = Promise.all([
    api.journalRead(deviceId, { limit: MACHINE_ROWS }).then(
      (answer) =>
        update((entry) => ({ ...entry, page: { state: "read", answer, error: null } })),
      (cause: unknown) =>
        update((entry) => ({
          ...entry,
          page: { state: "failed", answer: null, error: readFailureSentence(cause) }
        }))
    ),
    api.journalDay(deviceId, from, to).then(
      (answer) =>
        update((entry) => ({ ...entry, day: { state: "read", answer, error: null } })),
      (cause: unknown) =>
        update((entry) => ({
          ...entry,
          day: { state: "failed", answer: null, error: readFailureSentence(cause) }
        }))
    )
  ]).then(() => undefined);
  pending.set(key, read);
  void read.finally(() => {
    if (pending.get(key) === read) pending.delete(key);
  });
  return read;
}

export type FleetRecord = {
  /** The machines the fan-out asks, the ones beyond the cap, and the ones offline. */
  plan: FleetPlan;
  /** One entry per asked machine, in the plan's order; `[]` when no window is named. */
  reads: FleetMachineRead[];
  /** True while any asked machine's read is in flight. */
  reading: boolean;
};

const NOTHING: FleetPlan = { asked: [], beyond: [], offline: [] };

/**
 * Ask the fleet's online machines for their records, bounded and cached.
 *
 * `from`/`to` are the day block the header counts (`null` while no window is in hand, which asks
 * nothing). `devices` is the fleet list; the plan is its online order, so the cap falls on the same
 * machines on every render.
 */
export function useFleetRecord(
  devices: readonly Device[],
  from: number | null,
  to: number | null
): FleetRecord {
  const [version, setVersion] = useState(0);
  const plan = useMemo(() => fleetPlan(devices), [devices]);
  // The effect reads the latest plan through a ref: names may change with the same ids, and an
  // effect keyed on the ids must not capture the previous array.
  const planRef = useRef(plan);
  planRef.current = plan;
  const idsKey = plan.asked.map((device) => device.id).join("\u0000");
  const bump = useCallback(() => setVersion((value) => value + 1), []);

  useEffect(() => {
    if (from === null || to === null) return;
    const asked = planRef.current.asked;
    if (asked.length === 0) return;
    const now = Date.now();
    let soonest = Number.POSITIVE_INFINITY;
    let started = false;
    for (const device of asked) {
      const key = keyOf(device.id, from, to);
      const entry = cache.get(key);
      const reading =
        entry !== undefined && (entry.page.state === "reading" || entry.day.state === "reading");
      if (entry !== undefined && reading) {
        // A read already out for this window: wait on the same promise rather than asking twice.
        const wait = pending.get(key);
        if (wait !== undefined) void wait.then(bump);
        continue;
      }
      if (entry !== undefined && now - entry.startedAt < FLEET_CACHE_MS) {
        soonest = Math.min(soonest, entry.startedAt + FLEET_CACHE_MS - now);
        continue;
      }
      // **A REFRESH KEEPS THE ANSWER IT HAD.** The read is replaced by "reading" while the previous
      // answer stays in hand, so a page that redraws on a live line does not blink its rows away
      // every 30 s. `fleetDay` still only adds answers that finished, so a refresh is not counted.
      const kept = entry ?? blank();
      cache.set(key, {
        page: { state: "reading", answer: kept.page.answer, error: null },
        day: { state: "reading", answer: kept.day.answer, error: null },
        startedAt: now
      });
      started = true;
      void startReads(device.id, from, to, bump);
    }
    if (started) bump();
    if (soonest !== Number.POSITIVE_INFINITY) {
      const timer = window.setTimeout(bump, Math.max(1, soonest + 1));
      return () => window.clearTimeout(timer);
    }
    // `version` is how a landed answer (or a cache expiry) reaches this effect; `idsKey` is what
    // makes a different fleet a different fan-out. `planRef` carries the names.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [idsKey, from, to, version, bump]);

  const reads = useMemo(() => {
    if (from === null || to === null) return [];
    return plan.asked.map((device): FleetMachineRead => {
      const entry = cache.get(keyOf(device.id, from, to));
      return {
        device,
        page: entry?.page ?? { state: "idle", answer: null, error: null },
        day: entry?.day ?? { state: "idle", answer: null, error: null }
      };
    });
    // `version` is the cache's revision: every landed answer moves it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [plan, from, to, version]);

  const reading =
    from !== null &&
    to !== null &&
    plan.asked.some((device) => {
      const entry = cache.get(keyOf(device.id, from, to));
      return (
        entry === undefined || entry.page.state === "reading" || entry.day.state === "reading"
      );
    });

  return { plan: from === null || to === null ? NOTHING : plan, reads, reading };
}
