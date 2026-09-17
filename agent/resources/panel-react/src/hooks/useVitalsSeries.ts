// The device's VITALS SERIES — `/api/vitals/history`, polled on its own cadence.
//
// WHY A SEPARATE POLL, and why 30 s. The series is written by the agent's own sampler once
// per `interval_secs`, so polling faster than that would return the same array repeatedly;
// polling much slower would leave a chip announcing a load the device has already shed.
// The device states its cadence in the reply and this hook uses it, rather than hardcoding
// a number that a future sampler change would silently invalidate.
import { useEffect, useState } from "react";
import { callApi } from "../lib/api";

interface VitalsSample {
  tsMs: number;
  cpu: number | null;
  mem: number | null;
  memTotalMb: number | null;
}

export interface VitalsSeries {
  samples: VitalsSample[];
  /** The device's sampling cadence, in seconds. 0 when the reply did not state one. */
  intervalSecs: number;
  /** The span the samples cover, in seconds — the device's own arithmetic, not re-derived. */
  spanSecs: number;
}

export const EMPTY_SERIES: VitalsSeries = { samples: [], intervalSecs: 0, spanSecs: 0 };

const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);

/** Read `/api/vitals/history`. A body this build cannot use is an EMPTY series — never a
 *  throw — and a sample with no usable stamp is dropped, because it cannot be placed on the
 *  time axis the chip's window and the chart's x-axis both use. */
export function parseVitalsSeries(j: unknown): VitalsSeries {
  const body = (j ?? {}) as { samples?: unknown; interval_secs?: unknown; span_secs?: unknown };
  const samples: VitalsSample[] = (Array.isArray(body.samples) ? body.samples : []).flatMap((raw) => {
    const r = (raw ?? {}) as Record<string, unknown>;
    const tsMs = num(r.ts_ms);
    if (tsMs === null) return [];
    return [
      {
        tsMs,
        cpu: num(r.cpu_pct),
        mem: num(r.mem_pct),
        memTotalMb: num(r.mem_total_mb),
      },
    ];
  });
  return {
    samples,
    intervalSecs: num(body.interval_secs) ?? 0,
    spanSecs: num(body.span_secs) ?? 0,
  };
}

/** The series, refreshed on the device's own cadence (default 30 s, floored at 10 s so a
 *  malformed reply cannot turn the panel into a poller). */
export function useVitalsSeries(intervalMs = 30_000): VitalsSeries & { failed: boolean } {
  const [series, setSeries] = useState<VitalsSeries>(EMPTY_SERIES);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const j = await callApi("/api/vitals/history");
        if (!alive) return;
        if (j?.ok !== true) {
          setFailed(true);
          return;
        }
        setSeries(parseVitalsSeries(j));
        setFailed(false);
      } catch {
        // Keep the last good series: a missed poll is not a device that went quiet, and
        // blanking the chart would be this panel asserting it.
        if (alive) setFailed(true);
      }
    };
    void tick();
    const t = window.setInterval(tick, Math.max(10_000, intervalMs));
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, [intervalMs]);
  return { ...series, failed };
}
