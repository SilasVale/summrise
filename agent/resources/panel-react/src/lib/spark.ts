// The SHAPE of a series — sparkline geometry and the one rule that decides whether a
// sustained load is worth interrupting an operator for.
//
// WHY A SHAPE AT ALL. Every instrument this panel had was instantaneous: a dial, two
// percentages, a number of seconds. "CPU 87%" is a reading; "CPU has been above 90% for
// twelve minutes" is a fact somebody can act on, and the difference between them is the
// series — which the device now keeps (see `metrics.rs`) and this file draws and reads.
//
// TWO RULES, BOTH PURE AND BOTH HERE:
//   * `sparkSegments` turns readings into path data for an SVG polyline, BREAKING the line
//     where a reading is missing rather than bridging it. A chart that connects across a
//     gap asserts values nobody measured — the same reason this panel renders an em dash
//     for an unknown reading instead of a zero.
//   * `loadNotice` decides whether the recent series says something an operator should be
//     told without asking. It says nothing until it has enough evidence, and nothing when
//     the load is merely high ONCE — a spike is not a condition.
//
// ── RUST SINCE 2026-09-30 (block ②) ──────────────────────────────────────────────────────────────
//
// All three are `agent/resources/panel-logic/src/spark.rs` now, transliterated. The differential is
// 513 corpus cases with 0 divergences — values, KEY ORDER and whether each side raised — and every
// arm reached; `lib/spark.test.ts` runs UNCHANGED against it.
//
// AND FOUR `toFixed(2)` RULES HAD TO BE CARRIED ACROSS, each one found by that differential rather
// than by reading: `Infinity` is the STRING `"Infinity"` and not Rust's `"inf"`; at or above 1e21 the
// spec switches to `ToString`, so `(1e21).toFixed(2)` is `"1e+21"` and not twenty-two digits; `-0`
// prints WITHOUT a sign; and ON A TIE IT TAKES THE LARGER MAGNITUDE where Rust's `{:.2}` takes the
// even one — which is REACHABLE here, because `0.125` is exactly representable and a series scaled
// into a one-pixel box produces it.
//
// THE TWO NUMBERS BELOW STAY, and they are the surface's own: how far back an operator's "just now"
// reaches, and how much evidence this panel will wait for. They are passed INTO the crate (the
// arrangement `liveness.rs` records for `WORKING_MS`), because a module-level `logic()` call would
// evaluate before anything could await the seam. The rule's own thresholds — the dial's `crit` band
// and the two-thirds fraction — went with the rule.
import { logic } from "../wasm/panelLogic";

/** How many readings a segment needs before `loadNotice` will judge at all. Ten samples is
 *  five minutes at the device's 30 s cadence: long enough that a burst cannot fire it, short
 *  enough that a device which really is pinned is reported while it is still pinned. */
export const LOAD_MIN_SAMPLES = 10;

/** The window `loadNotice` looks at, in milliseconds. Fifteen minutes: an operator's "just
 *  now", and the span over which a machine that is struggling is worth mentioning. */
export const LOAD_WINDOW_MS = 15 * 60_000;

/** The severity word a sustained load earns. Shares the dial's vocabulary (75 % warn,
 *  90 % crit) so the panel cannot describe one reading two ways. */
type LoadTone = "warn" | "crit";

interface LoadNotice {
  tone: LoadTone;
  /** Which series the notice is about. */
  metric: "cpu" | "mem";
  /** Short text for a chip. */
  text: string;
  /** The numbers behind it, for the hover. */
  title: string;
}

/** Path data for an SVG polyline, one entry per RUN of known values.
 *
 *  `values` is oldest-first (the device's order). `width`/`height` are the drawing box; the
 *  path is scaled to fill it, with the series' own min/max as the vertical range — a
 *  sparkline's job is the shape, and a fixed 0-100 axis would flatten every real series into
 *  a straight line. A run of ONE known value draws a dot-sized segment rather than nothing,
 *  so a single reading is still visible.
 *
 *  Returns `[]` when there is nothing to draw (all values absent, or fewer than one known):
 *  an empty chart must be an ABSENT chart, never a flat line at zero. */
export function sparkSegments(
  values: (number | null)[],
  width: number,
  height: number,
): string[] {
  return logic().spark_segments(values, width, height) as string[];
}

/** min / avg / max over the known readings, or `null` when there are none. Never invents
 *  a zero for an empty series. */
export function seriesStats(
  values: (number | null)[],
): { min: number; avg: number; max: number; n: number } | null {
  return logic().series_stats(values) as {
    min: number;
    avg: number;
    max: number;
    n: number;
  } | null;
}

/** Does the recent series say something worth interrupting an operator for?
 *
 *  `samples` is the device's series, oldest first; `nowMs` the caller's clock. The rule
 *  reads only the last `LOAD_WINDOW_MS`, needs `LOAD_MIN_SAMPLES` of evidence, and fires
 *  only when at least two thirds of those readings sit in the dial's `crit` band.
 *
 *  SILENCE IS THE DEFAULT, and it is the honest one: a device this panel cannot see a
 *  series for (a host that reports no vitals, an agent that just started) produces no
 *  notice, because "I have not looked" and "nothing is wrong" are different facts and a
 *  chip that conflates them is worse than no chip. */
export function loadNotice(
  samples: { tsMs: number; cpu: number | null; mem: number | null }[],
  nowMs: number,
): LoadNotice | null {
  return logic().load_notice(
    samples,
    nowMs,
    LOAD_WINDOW_MS,
    LOAD_MIN_SAMPLES,
  ) as LoadNotice | null;
}
