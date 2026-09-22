// The device's REACHABILITY MONITORS — watched host:port targets, their probe series, and the
// actions the card offers.
//
// WHY A HOOK AND NOT A CARD-LOCAL FETCH. Two surfaces read this: the Settings card (charts and
// controls) and the strip's chip ("192.168.1.1:22 down 4m"), which must be visible from any
// page. One poller per shell feeds both, and the actions live here too so a card and any future
// caller send exactly the same requests.
import { useCallback, useEffect, useRef, useState } from "react";
import { callApi } from "../lib/api";

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

export const EMPTY_MONITORS: Monitors = { targets: [], intervalSecs: 0, seriesMax: 0 };

const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);
const str = (v: unknown): string => (typeof v === "string" ? v : "");

/** Read `/api/monitors`. A body this build cannot use is an EMPTY monitor list — never a throw
 *  — and a probe with no usable stamp is dropped (it cannot be placed on the time axis). */
export function parseMonitors(j: unknown): Monitors {
  const body = (j ?? {}) as Record<string, unknown>;
  const rows = Array.isArray(body.targets) ? body.targets : [];
  const targets: MonitorTarget[] = rows.flatMap((raw) => {
    const r = (raw ?? {}) as Record<string, unknown>;
    const id = str(r.id);
    if (!id) return [];
    const s = (r.summary ?? {}) as Record<string, unknown>;
    const lat = (s.latency ?? null) as Record<string, unknown> | null;
    const series: MonitorProbe[] = (Array.isArray(r.series) ? r.series : []).flatMap((p) => {
      const pr = (p ?? {}) as Record<string, unknown>;
      const tsMs = num(pr.ts_ms);
      if (tsMs === null) return [];
      return [{ tsMs, ok: pr.ok === true, ms: num(pr.ms) }];
    });
    const transitions: MonitorTransition[] = (Array.isArray(r.transitions) ? r.transitions : []).flatMap(
      (raw) => {
        const t = (raw ?? {}) as Record<string, unknown>;
        const atMs = num(t.at_ms);
        if (atMs === null) return [];
        return [{ atMs, up: t.up === true, lastedMs: num(t.lasted_ms) ?? 0 }];
      },
    );
    return [
      {
        id,
        host: str(r.host),
        port: num(r.port) ?? 0,
        path: r.path === null || r.path === undefined ? null : str(r.path) || null,
        expect: r.expect === null || r.expect === undefined ? null : str(r.expect) || null,
        transitions,
        series,
        summary: {
          probes: num(s.probes) ?? series.length,
          up: num(s.up) ?? 0,
          down: num(s.down) ?? 0,
          upPct: num(s.up_pct),
          upNow: typeof s.up_now === "boolean" ? s.up_now : null,
          sinceMs: num(s.since_ms),
          drops: num(s.drops),
          latency: lat
            ? { min: num(lat.min) ?? 0, avg: num(lat.avg) ?? 0, max: num(lat.max) ?? 0 }
            : null,
          lastStatus: num(s.last_status),
          lastExpectOk: typeof s.last_expect_ok === "boolean" ? s.last_expect_ok : null,
        },
      },
    ];
  });
  return {
    targets,
    intervalSecs: num(body.interval_secs) ?? 0,
    seriesMax: num(body.series_max) ?? 0,
  };
}

/** The monitors, refreshed on the device's own cadence (a probe every 15 s: polling slower
 *  would show a stale state on the one surface that exists to show a state change). */
export function useMonitors(intervalMs = 20_000): Monitors & {
  failed: boolean;
  refresh: () => Promise<void>;
  add: (host: string, port: number, path?: string, expect?: string) => Promise<{ ok: boolean; error?: string }>;
  remove: (id: string) => Promise<void>;
  probe: (id: string) => Promise<void>;
} {
  const [monitors, setMonitors] = useState<Monitors>(EMPTY_MONITORS);
  const [failed, setFailed] = useState(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    try {
      const j = await callApi("/api/monitors");
      if (!alive.current) return;
      if (j?.ok !== true) {
        setFailed(true);
        return;
      }
      setMonitors(parseMonitors(j));
      setFailed(false);
    } catch {
      // Keep the last good list: a missed poll is not a device that stopped watching.
      if (alive.current) setFailed(true);
    }
  }, []);

  const add = useCallback(
    async (host: string, port: number, path = "", expect = "") => {
      try {
        const j = await callApi("/api/monitors/add", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ host, port, path, expect }),
        });
        if (j?.ok !== true) return { ok: false, error: str(j?.error) || "the device refused it" };
        // Probe immediately: a target that shows "no readings yet" for 15 s after being added
        // reads as a target that is not being watched.
        const id = str(j?.target?.id);
        if (id) await callApi("/api/monitors/probe", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ id }),
        }).catch(() => {});
        await refresh();
        return { ok: true };
      } catch (e) {
        return { ok: false, error: e instanceof Error ? e.message : "the call failed" };
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

  useEffect(() => {
    void refresh();
    const t = window.setInterval(() => void refresh(), Math.max(5_000, intervalMs));
    return () => window.clearInterval(t);
  }, [refresh, intervalMs]);

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
 *  error inside an event handler, and never a banner about something that did not happen. */
export function parseMonitorChange(detail: unknown): MonitorAlert | null {
  const d = (detail ?? {}) as Record<string, unknown>;
  if (d.ev !== "monitor-change") return null;
  const id = str(d.id);
  const atMs = num(d.at_ms);
  if (!id || atMs === null) return null;
  return {
    key: `${id}:${atMs}`,
    id,
    host: str(d.host),
    port: num(d.port) ?? 0,
    up: d.up === true,
    lastedMs: num(d.lasted_ms) ?? 0,
    atMs,
    status: num(d.status),
  };
}

const MAX_ALERTS = 3;

/** The alerts, newest first, expiring on their own. */
export function useMonitorAlerts(ttlMs = 12_000): MonitorAlert[] {
  const [alerts, setAlerts] = useState<MonitorAlert[]>([]);
  useEffect(() => {
    const onFrame = (e: Event) => {
      const alert = parseMonitorChange((e as CustomEvent).detail);
      if (!alert) return;
      setAlerts((prev) => [alert, ...prev.filter((a) => a.key !== alert.key)].slice(0, MAX_ALERTS));
      window.setTimeout(() => {
        setAlerts((prev) => prev.filter((a) => a.key !== alert.key));
      }, ttlMs);
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
  return h >= 24 ? `${Math.floor(h / 24)}d ${h % 24}h` : `${h}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
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
