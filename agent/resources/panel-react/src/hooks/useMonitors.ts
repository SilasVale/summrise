// The device's REACHABILITY MONITORS — watched host:port targets, their probe series, and the
// actions the card offers.
//
// WHY A HOOK AND NOT A CARD-LOCAL FETCH. Two surfaces read this: the Settings card (charts and
// controls) and the strip's chip ("192.168.1.1:22 down 4m"), which must be visible from any
// page. One poller per shell feeds both, and the actions live here too so a card and any future
// caller send exactly the same requests.
import { useCallback, useEffect, useRef, useState } from "react";
import { callApi } from "../lib/api";

export interface MonitorProbe {
  tsMs: number;
  ok: boolean;
  /** Connect time when the probe ANSWERED; null when it did not — a latency for a connection
   *  that never happened would be a fabricated measurement. */
  ms: number | null;
}

export interface MonitorSummary {
  probes: number;
  up: number;
  down: number;
  /** Share of the probes taken, or null with no probes yet ("unknown", never 0%). */
  upPct: number | null;
  upNow: boolean | null;
  /** When the CURRENT state began (the oldest probe of the current run of identical states). */
  sinceMs: number | null;
  latency: { min: number; avg: number; max: number } | null;
}

export interface MonitorTarget {
  id: string;
  host: string;
  port: number;
  summary: MonitorSummary;
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
    return [
      {
        id,
        host: str(r.host),
        port: num(r.port) ?? 0,
        series,
        summary: {
          probes: num(s.probes) ?? series.length,
          up: num(s.up) ?? 0,
          down: num(s.down) ?? 0,
          upPct: num(s.up_pct),
          upNow: typeof s.up_now === "boolean" ? s.up_now : null,
          sinceMs: num(s.since_ms),
          latency: lat
            ? { min: num(lat.min) ?? 0, avg: num(lat.avg) ?? 0, max: num(lat.max) ?? 0 }
            : null,
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
  add: (host: string, port: number) => Promise<{ ok: boolean; error?: string }>;
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
    async (host: string, port: number) => {
      try {
        const j = await callApi("/api/monitors/add", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ host, port }),
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

/** `4m`, `1h 04m`, `2d 4h` — the shapes the rest of the panel uses for durations. */
export function fmtSince(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  const h = Math.floor(s / 3600);
  return h >= 24 ? `${Math.floor(h / 24)}d ${h % 24}h` : `${h}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
}

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
