// The device's REACHABILITY MONITORS — watched host:port targets, their probe series, and the
// actions the card offers.
//
// WHY A HOOK AND NOT A CARD-LOCAL FETCH. Two surfaces read this: the Settings card (charts and
// controls) and the strip's chip ("192.168.1.1:22 down 4m"), which must be visible from any
// page. One poller per shell feeds both, and the actions live here too so a card and any future
// caller send exactly the same requests.
//
// AND IT IS NOW HALF A SEAM (P2, 2026-09-28). `parseMonitors` and `parseMonitorChange` — the two
// PARSES, the half of this file that decides what the device said — moved to
// `agent/resources/panel-logic/src/monitors.rs` and are called from here through
// `src/wasm/panelLogic.ts` (P0's BOUNDARY class: it computes nothing, it fetches). `fmtSince`,
// `unstableTargets` and `downTargets` are still TypeScript because they are called during RENDER
// and the wasm is fetched at the first call rather than at page load — P0's "the hooks are a SPLIT,
// not a unit", applied: the parse goes to Rust, the `useEffect` stays.
import { useCallback, useEffect, useState } from "react";
import { callApi, deviceRefused } from "../lib/api";
import { panelLogic } from "../wasm/panelLogic";
import { useDeviceRead } from "./useDeviceRead";

interface MonitorProbe {
  tsMs: number;
  ok: boolean;
  /** Connect time when the probe ANSWERED; null when it did not — a latency for a connection
   *  that never happened would be a fabricated measurement. */
  ms: number | null;
}

interface MonitorSummary {
  probes: number;
  up: number;
  down: number;
  /** Share of the probes taken, or null with no probes yet ("unknown", never 0%). */
  upPct: number | null;
  upNow: boolean | null;
  /** When the CURRENT state began (the oldest probe of the current run of identical states). */
  sinceMs: number | null;
  /** How many times it FELL from up to down inside the window (null with no probes yet). A
   *  target that is down now contributes the drop that started its outage — `upNow` says
   *  whether that state is current. */
  drops: number | null;
  latency: { min: number; avg: number; max: number } | null;
  /** The last HTTP status code, for a target watched with a path (null otherwise). */
  lastStatus: number | null;
  /** Whether the body carried the expected text, when one was given (null when nothing to match). */
  lastExpectOk: boolean | null;
}

/** ONE STATE CHANGE, as the device records it: when it happened, which state took effect, and
 *  how long the state it ENDED had lasted (an outage, for a recovery). */
interface MonitorTransition {
  atMs: number;
  up: boolean;
  lastedMs: number;
}

export interface MonitorTarget {
  id: string;
  host: string;
  port: number;
  /** The HTTP path this target is checked with, or null for a plain TCP connect. */
  path: string | null;
  /** Text the body must contain, when the operator asked for a content check. */
  expect: string | null;
  summary: MonitorSummary;
  /** Newest last; the card shows them newest-first. */
  transitions: MonitorTransition[];
  series: MonitorProbe[];
}

export interface Monitors {
  targets: MonitorTarget[];
  intervalSecs: number;
  seriesMax: number;
}

export const EMPTY_MONITORS: Monitors = {
  targets: [],
  intervalSecs: 0,
  seriesMax: 0,
};

/** `str(v)` — a string, or `""`. THE PARSE BELOW NO LONGER USES IT (`str_of` lives in
 *  `panel-logic/src/monitors.rs` now); the hook's own actions still do, for the device's `error`
 *  and for the id of a target it just added. */
const str = (v: unknown): string => (typeof v === "string" ? v : "");

/** Read `/api/monitors`. A body this build cannot use is an EMPTY monitor list — never a throw
 *  — and a probe with no usable stamp is dropped (it cannot be placed on the time axis).
 *
 *  ── THIS FUNCTION IS RUST NOW (P2, 2026-09-28) ──────────────────────────────
 *
 *  The body that was here moved to `agent/resources/panel-logic/src/monitors.rs`, transliterated:
 *  the same drops (a target with no id, a probe or transition with no stamp), the same collapses
 *  (`str` → `""`, `num` → absence, `str(v) || null` for `path`/`expect`), the same `?? 0` and
 *  `?? series.length` defaults, and the same two strict tests (`=== true`, `typeof … === "boolean"`)
 *  that a truthiness test would have widened. `num` went with it; the TypeScript copy is gone, so
 *  there is no second derivation of a monitor row left in the panel.
 *
 *  WHY IT IS `async` AND THE THREE FUNCTIONS BELOW IT ARE NOT. The wasm is fetched at the first
 *  call rather than at page load — criterion ③ of the migration plan, so it is in neither the
 *  first-load payload nor the page's critical path — and a call during RENDER cannot wait for that
 *  fetch. This one is a `useDeviceRead` fold, so it is free: the fold already may return a promise.
 *  `fmtSince`, `unstableTargets` and `downTargets` below are called while `MonitorAlerts`,
 *  `MonitorsCard` and `MonitorChip` render, so they stay TypeScript until the sync story is
 *  decided; the numbers behind that decision are in the commit message. */
export async function parseMonitors(j: unknown): Promise<Monitors> {
  const logic = await panelLogic();
  return logic.parse_monitors(j) as Monitors;
}

/** The monitors, refreshed on the device's own cadence (a probe every 15 s: polling slower
 *  would show a stale state on the one surface that exists to show a state change). */
export function useMonitors(intervalMs = 20_000): Monitors & {
  failed: boolean;
  refresh: () => Promise<void>;
  add: (
    host: string,
    port: number,
    path?: string,
    expect?: string,
  ) => Promise<{ ok: boolean; error?: string }>;
  remove: (id: string) => Promise<void>;
  probe: (id: string) => Promise<void>;
} {
  // THE POLL AND THE RE-READ AFTER AN ACTION ARE `useDeviceRead`'s (see its header): the
  // refusal guard, the cadence floor, the unmount guard and the ordering guard. `refresh`
  // is the same never-rejecting promise this hook has always handed to `add`/`remove`/
  // `probe`, so every action re-reads the list through one loop — and keep-last means a
  // missed poll is not a device that stopped watching.
  const {
    data: monitors,
    read,
    refresh,
  } = useDeviceRead<Monitors>({
    path: "/api/monitors",
    // A body this build cannot use is an EMPTY monitor list — never a throw (parseMonitors).
    reduce: (_previous, body) => parseMonitors(body),
    initial: EMPTY_MONITORS,
    everyMs: intervalMs,
  });
  // `failed` says the device did not answer; it never says the list is empty. The card's
  // chip and the card itself both draw that distinction.
  const failed = read === "unreadable";

  const add = useCallback(
    async (host: string, port: number, path = "", expect = "") => {
      try {
        const j = await callApi("/api/monitors/add", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ host, port, path, expect }),
        });
        if (deviceRefused(j))
          return { ok: false, error: str(j?.error) || "the device refused it" };
        // Probe immediately: a target that shows "no readings yet" for 15 s after being added
        // reads as a target that is not being watched.
        const id = str(j?.target?.id);
        if (id)
          await callApi("/api/monitors/probe", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ id }),
          }).catch(() => {});
        await refresh();
        return { ok: true };
      } catch (e) {
        return {
          ok: false,
          error: e instanceof Error ? e.message : "the call failed",
        };
      }
    },
    [refresh],
  );

  const remove = useCallback(
    async (id: string) => {
      await callApi("/api/monitors/remove", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ id }),
      }).catch(() => {});
      await refresh();
    },
    [refresh],
  );

  const probe = useCallback(
    async (id: string) => {
      await callApi("/api/monitors/probe", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ id }),
      }).catch(() => {});
      await refresh();
    },
    [refresh],
  );

  return { ...monitors, failed, refresh, add, remove, probe };
}

/** THE DEVICE SPEAKING. A watched target changing state arrives as `summrise-monitor-change` on the
 *  SSE stream (the monitor emits it; the panel does not poll for it), and this turns the window
 *  event into a short list of alerts the shell renders.
 *
 *  WHY AN ALERT AT ALL: everything else about the monitors is something an operator has to GO
 *  AND LOOK AT — a card and a chip. A host you asked the device to watch is the one case where
 *  the device should speak first, because the answer ("it just went down") is only useful now.
 *
 *  Bounded and self-expiring: at most three at a time, each gone after `ttlMs`, because a flapping
 *  link must not be able to fill the screen with banners that never leave. */
export interface MonitorAlert {
  key: string;
  id: string;
  host: string;
  port: number;
  up: boolean;
  lastedMs: number;
  atMs: number;
  /** The HTTP status behind the verdict, when the target is watched with a path. */
  status: number | null;
}

/** Read one `monitor-change` frame. A frame this build cannot use is null — never a thrown
 *  error inside an event handler, and never a banner about something that did not happen.
 *
 *  ── THIS FUNCTION IS RUST NOW (P2, 2026-09-28) ──────────────────────────────
 *
 *  It moved with `parseMonitors`, into the same module (`monitors.rs`), and it is the FIRST migrated
 *  function whose call site is an EVENT HANDLER rather than a data fold — the `monitor-change`
 *  listener below and `useAttention`'s OS-notification listener. That is free for the same reason a
 *  fold is: nothing in an event handler is on the first render's path.
 *
 *  THE KEY IS THE ONE THING HERE THAT IS NOT A TRANSLITERATION OF A PREDICATE BUT OF A CONVERSION.
 *  `` `${id}:${atMs}` `` is JS number → text, and Rust's `format!` is a different function
 *  (`1e21` → `1000000000000000000000` where JS says `1e+21`; `-0` → `-0` where JS says `0`). A key
 *  spelled differently is a DIFFERENT key, so the same transition would stack in the strip instead
 *  of replacing itself. `monitors.rs` therefore asks the ENGINE (`Number.prototype.toString`), not
 *  Rust's formatter — which is also what keeps the float formatter out of the binary (P0 measured
 *  one `{:.3}` at 8,879 gz). */
export async function parseMonitorChange(
  detail: unknown,
): Promise<MonitorAlert | null> {
  const logic = await panelLogic();
  return logic.parse_monitor_change(detail) as MonitorAlert | null;
}

const MAX_ALERTS = 3;

/** The alerts, newest first, expiring on their own. */
export function useMonitorAlerts(ttlMs = 12_000): MonitorAlert[] {
  const [alerts, setAlerts] = useState<MonitorAlert[]>([]);
  useEffect(() => {
    const onFrame = (e: Event) => {
      // THE FRAME'S DETAIL IS READ BEFORE THE AWAIT, deliberately: `parseMonitorChange` is a promise
      // now, and the event is the dispatcher's object rather than this listener's. What crosses the
      // await is the parsed value, never the event.
      const detail = (e as CustomEvent).detail;
      void parseMonitorChange(detail).then((alert) => {
        if (!alert) return;
        setAlerts((prev) =>
          [alert, ...prev.filter((a) => a.key !== alert.key)].slice(
            0,
            MAX_ALERTS,
          ),
        );
        window.setTimeout(() => {
          setAlerts((prev) => prev.filter((a) => a.key !== alert.key));
        }, ttlMs);
      });
    };
    window.addEventListener("summrise-monitor-change", onFrame);
    return () => window.removeEventListener("summrise-monitor-change", onFrame);
  }, [ttlMs]);
  return alerts;
}

/** `4m`, `1h 04m`, `2d 4h` — the shapes the rest of the panel uses for durations. */
export function fmtSince(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  const h = Math.floor(s / 3600);
  return h >= 24
    ? `${Math.floor(h / 24)}d ${h % 24}h`
    : `${h}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
}

/** A link that has fallen more than once in the window is UNSTABLE — the pattern, as opposed to
 *  the state. Two, not one, because a single drop is often the operator's own reboot; and the
 *  rule only speaks about targets that are currently UP, because one that is down is already
 *  named by `downTargets` and two chips saying different things about the same host would be
 *  worse than one. */
export function unstableTargets(monitors: Monitors): MonitorTarget[] {
  return monitors.targets.filter(
    (t) => t.summary.upNow === true && (t.summary.drops ?? 0) >= UNSTABLE_DROPS,
  );
}

/** The number of drops that makes a link unstable — mirrors `monitor::UNSTABLE_DROPS` on the
 *  device, which is where the count comes from; this constant only decides when to SPEAK. */
const UNSTABLE_DROPS = 2;

/** The targets that are DOWN right now, with how long they have been down. The strip's chip
 *  and the card's headline both read this — one rule, so they cannot disagree. */
export function downTargets(
  monitors: Monitors,
  nowMs: number,
): { target: MonitorTarget; sinceMs: number }[] {
  return monitors.targets
    .filter((t) => t.summary.upNow === false)
    .map((t) => ({
      target: t,
      // A target that has been down since its FIRST probe has no earlier boundary; the oldest
      // probe it does have is the honest answer, never "0s".
      sinceMs: nowMs - (t.summary.sinceMs ?? t.series[0]?.tsMs ?? nowMs),
    }));
}
