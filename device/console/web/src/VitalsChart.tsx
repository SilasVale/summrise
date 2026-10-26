import { VITALS_TTL_MS, type VitalsSample } from "./vitals";

/**
 * The device card's vitals sparkline: memory available % and the tightest disk's free %, drawn from
 * the browser-local history.
 *
 * The chart is additive, never the only statement. The latest values and the coverage are text, so a
 * screen reader and a person who does not read charts get the same facts; the SVG itself is
 * `aria-hidden`. The x axis is a fixed two-hour window ending at the newest sample, so a history that
 * covers five minutes occupies the right-hand sliver rather than being stretched into a claim of two
 * hours. Samples are connected, never invented: a gap is a longer segment, and the caption says so.
 */

const WIDTH = 240;
const HEIGHT = 44;
const TOP = 4;
const BOTTOM = 40;

const CAPTION =
  "Memory available and the tightest disk's free space over the last 2 h, sampled every 30 s while this console page was open in this browser. Gaps are not filled.";

export function VitalsChart({ samples }: { samples: VitalsSample[] }) {
  if (samples.length < 2) {
    return (
      <p className="vitals-collecting">
        Collecting memory and disk history while this page is open — one sample every 30 s; the chart
        starts at the second sample.
      </p>
    );
  }

  const ordered = [...samples].sort((left, right) => left.ts_ms - right.ts_ms);
  const latest = ordered[ordered.length - 1];
  const end = latest.ts_ms;
  const start = end - VITALS_TTL_MS;
  const x = (ts: number) => clamp(((ts - start) / VITALS_TTL_MS) * WIDTH, 0, WIDTH);
  const y = (percent: number) => TOP + (1 - percent / 100) * (BOTTOM - TOP);
  const points = (percent: (sample: VitalsSample) => number) =>
    ordered
      .map((sample) => `${x(sample.ts_ms).toFixed(1)},${y(percent(sample)).toFixed(1)}`)
      .join(" ");

  const memory = memoryPercent(latest);
  const disk = diskPercent(latest);

  return (
    <div className="vitals">
      <div className="vitals-keys">
        <span className="vitals-key vitals-key-memory">Memory available {percentText(memory)}</span>
        <span className="vitals-key vitals-key-disk">
          Tightest disk {latest.disk_name} {percentText(disk)} free
        </span>
      </div>
      <svg
        className="vitals-chart"
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        preserveAspectRatio="none"
        aria-hidden="true"
        focusable="false"
      >
        <polyline
          className="vitals-line vitals-line-memory"
          points={points(memoryPercent)}
          vectorEffect="non-scaling-stroke"
        />
        <polyline
          className="vitals-line vitals-line-disk"
          points={points(diskPercent)}
          vectorEffect="non-scaling-stroke"
        />
      </svg>
      <p className="vitals-caption">{CAPTION}</p>
    </div>
  );
}

/** Available memory as a percentage, or 0 when the stored total cannot support one. */
function memoryPercent(sample: VitalsSample): number {
  return percentOf(sample.memory_available_bytes, sample.memory_total_bytes);
}

/** The tightest disk's free space as a percentage, or 0 when the stored total cannot support one. */
function diskPercent(sample: VitalsSample): number {
  return percentOf(sample.disk_free_bytes, sample.disk_total_bytes);
}

function percentOf(part: number, total: number): number {
  if (!Number.isFinite(part) || !Number.isFinite(total) || total <= 0) return 0;
  return clamp((part / total) * 100, 0, 100);
}

function percentText(percent: number): string {
  return `${percent.toFixed(1)}%`;
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}
