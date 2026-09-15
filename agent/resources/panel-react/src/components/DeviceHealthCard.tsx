// DeviceHealthCard — what this machine has been doing with itself, as a SHAPE.
//
// WHY THIS EXISTS. Every instrument in the panel was instantaneous: a dial, two percentages,
// a number of seconds. That answers "how is it now" and cannot answer the question an
// operator actually has when a device feels slow — **"has it been like this, or did I catch
// a moment?"** The agent now keeps a bounded series (see `metrics.rs`: one reading every
// 30 s, two hours) and this card is where a human reads it.
//
// WHAT IT SHOWS, in the order an operator reads it:
//   1. a VERDICT SENTENCE — the same facts in words, including the one thing a chart cannot
//      say ("no readings" vs "steady" vs "pegged for the last 12 minutes");
//   2. the two series as sparklines WITH their min/avg/max printed beside them (the shape is
//      a summary; the numbers are the value — the rule `VitalsDial` states for its arcs);
//   3. the window it covers, stated, because "the last 8 minutes" and "the last 2 hours" are
//      different claims about the same picture.
//
// HONEST EMPTIES: a host that reports no vitals, and an agent that started less than a
// couple of minutes ago, both draw ONE sentence saying which of the two it is. Neither
// draws a flat line.
import { Sparkline } from "./Sparkline";
import { loadNotice, seriesStats } from "../lib/spark";
import type { VitalsSeries } from "../hooks/useVitalsSeries";

/** `45s`, `12m`, `1h 04m` — the shapes the rest of the panel uses for durations. */
function fmtSpan(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  const h = Math.floor(secs / 3600);
  return `${h}h ${String(Math.floor((secs % 3600) / 60)).padStart(2, "0")}m`;
}

const pct = (v: number): string => `${Math.round(v)}%`;

function SeriesRow({
  name,
  tone,
  values,
  total,
}: {
  name: string;
  tone: "cpu" | "mem";
  values: (number | null)[];
  total?: string;
}) {
  const stats = seriesStats(values);
  const label = stats
    ? `${name} over this window: now ${pct(values.filter((v): v is number => v !== null).slice(-1)[0] ?? stats.avg)}, ` +
      `average ${pct(stats.avg)}, low ${pct(stats.min)}, high ${pct(stats.max)}`
    : `${name}: no readings in this window`;
  return (
    <div className="health-series" data-series={tone}>
      <span className="health-series-name">{name}</span>
      <Sparkline values={values} label={label} tone={tone} />
      {stats ? (
        <span className="health-series-stats">
          <span className="health-stat" title="average over the window">
            avg {pct(stats.avg)}
          </span>
          <span className="health-stat" title="lowest reading in the window">
            low {pct(stats.min)}
          </span>
          <span className="health-stat" title="highest reading in the window">
            high {pct(stats.max)}
          </span>
          {total && <span className="health-stat health-stat-total">{total}</span>}
        </span>
      ) : (
        // The chart drew nothing; the row says WHY rather than leaving a blank gap beside a
        // label that looks like it failed to load.
        <span className="health-series-stats">
          <span className="muted">no readings yet</span>
        </span>
      )}
    </div>
  );
}

export function DeviceHealthCard({
  series,
  failed,
  nowMs = Date.now(),
}: {
  series: VitalsSeries;
  failed?: boolean;
  /** Injected by tests so a rule about "the last 15 minutes" can be exercised without
   *  waiting for one. */
  nowMs?: number;
}) {
  const samples = series.samples;
  const cpu = samples.map((s) => s.cpu);
  const mem = samples.map((s) => s.mem);
  const totalMb = samples[samples.length - 1]?.memTotalMb ?? null;
  const notice = loadNotice(
    samples.map((s) => ({ tsMs: s.tsMs, cpu: s.cpu, mem: s.mem })),
    nowMs,
  );
  const cpuStats = seriesStats(cpu);
  const hasAny = cpuStats !== null || seriesStats(mem) !== null;

  return (
    <div className="settings-section">
      <h2>Device health</h2>
      {failed && !hasAny ? (
        <p className="muted">
          The device did not answer, so its vitals could not be read. That is not the same as
          a device with nothing to report.
        </p>
      ) : !hasAny ? (
        <p className="muted">
          {series.intervalSecs > 0
            ? `This host has no vitals readings yet — the agent samples every ${series.intervalSecs}s, ` +
              `and a CPU reading needs two of them. (A host that cannot report vitals, such as a ` +
              `non-Windows build, never will.)`
            : "This device did not report a sampling cadence, so there is nothing to draw."}
        </p>
      ) : (
        <>
          <p className={`health-verdict${notice ? " is-load" : ""}`} data-tone={notice?.tone ?? "ok"}>
            {notice
              ? notice.title
              : `No sustained load in this window: CPU high ${pct(cpuStats?.max ?? 0)}, memory high ${pct(
                  seriesStats(mem)?.max ?? 0,
                )}.`}
          </p>
          <div className="health-series-list">
            <SeriesRow name="CPU" tone="cpu" values={cpu} />
            <SeriesRow
              name="Mem"
              tone="mem"
              values={mem}
              total={totalMb !== null ? `${(totalMb / 1024).toFixed(1)} GB` : undefined}
            />
          </div>
          <p className="muted health-window">
            {samples.length} readings over {fmtSpan(series.spanSecs)} — one every{" "}
            {series.intervalSecs || "?"}s since this agent started. A gap in the line is a
            reading the device did not take.
          </p>
        </>
      )}
    </div>
  );
}
