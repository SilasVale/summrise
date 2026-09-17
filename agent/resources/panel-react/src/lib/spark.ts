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

/** Percentages at or above this are "pegged" for the purpose of a notice. This is the
 *  dial's own `crit` band, restated here because this rule is about DURATION: a single
 *  reading in the band is a spike, and the band is only interesting when it persists. */
const CRIT_PCT = 90;

/** How much of the window must be in the band. Two thirds: a device that is genuinely
 *  pegged stays there, while one that is bursting (builds, browser tabs) does not. */
const SUSTAINED_FRACTION = 2 / 3;

/** The readings' values, `null` where the device reported nothing. */
function seriesValues(samples: (number | null)[]): number[] {
  return samples.filter((v): v is number => typeof v === "number");
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
 *  an empty chart must be an ABSENT chart, never a flat line at zero.
 */
export function sparkSegments(
  values: (number | null)[],
  width: number,
  height: number,
): string[] {
  const known = seriesValues(values);
  if (known.length === 0) return [];
  const min = Math.min(...known);
  const max = Math.max(...known);
  const span = max - min;
  const x = (i: number): number =>
    values.length <= 1 ? width / 2 : (i / (values.length - 1)) * width;
  // A flat series draws through the middle rather than along the top or the floor: with
  // min === max every point is the same value, and pinning it to an edge would read as
  // "at the limit".
  const y = (v: number): number => (span === 0 ? height / 2 : height - ((v - min) / span) * height);

  const out: string[] = [];
  // Coordinates WITHOUT their command letter: the letter depends on the point's position in
  // its run, which is only known when the run ends.
  let run: string[] = [];
  const flush = () => {
    if (run.length === 1) {
      // A lone reading: a zero-length segment, which SVG renders as a dot with a round
      // linecap. Dropping it would hide a real measurement.
      out.push(`M${run[0]} L${run[0]}`);
    } else if (run.length > 1) {
      out.push(run.map((p, i) => `${i === 0 ? "M" : "L"}${p}`).join(" "));
    }
    run = [];
  };
  values.forEach((v, i) => {
    if (typeof v === "number") {
      run.push(`${x(i).toFixed(2)},${y(v).toFixed(2)}`);
    } else {
      flush(); // the gap BREAKS the line — see the module header
    }
  });
  flush();
  return out;
}

/** min / avg / max over the known readings, or `null` when there are none. Never invents
 *  a zero for an empty series. */
export function seriesStats(values: (number | null)[]): { min: number; avg: number; max: number; n: number } | null {
  const known = seriesValues(values);
  if (known.length === 0) return null;
  const sum = known.reduce((a, b) => a + b, 0);
  return {
    min: Math.min(...known),
    avg: sum / known.length,
    max: Math.max(...known),
    n: known.length,
  };
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
 *  chip that conflates them is worse than no chip.
 */
export function loadNotice(
  samples: { tsMs: number; cpu: number | null; mem: number | null }[],
  nowMs: number,
): LoadNotice | null {
  const recent = samples.filter((s) => nowMs - s.tsMs <= LOAD_WINDOW_MS);
  if (recent.length < LOAD_MIN_SAMPLES) return null;

  // The window's OWN span, read off its first and last stamp: the device's cadence is part
  // of its contract (`interval_secs`), but a notice that spells a duration must spell the
  // one the readings actually cover. Never less than a minute — "pegged 0m" is not a fact.
  const spanMinutes = Math.max(
    1,
    Math.round((recent[recent.length - 1].tsMs - recent[0].tsMs) / 60_000),
  );

  const verdict = (
    metric: "cpu" | "mem",
    pick: (s: { cpu: number | null; mem: number | null }) => number | null,
  ): LoadNotice | null => {
    const values = recent.map(pick).filter((v): v is number => typeof v === "number");
    // Not enough of THIS series to judge: the other one may still speak.
    if (values.length < LOAD_MIN_SAMPLES) return null;
    const high = values.filter((v) => v >= CRIT_PCT).length;
    if (high / values.length < SUSTAINED_FRACTION) return null;
    const label = metric === "cpu" ? "CPU" : "memory";
    return {
      tone: "crit",
      metric,
      text: `${label} pegged ${spanMinutes}m`,
      title:
        `${label} has been at or above ${CRIT_PCT}% in ${high} of the last ${values.length} ` +
        `readings (${spanMinutes} minutes) — the device is under sustained load, which is ` +
        `what makes everything on it feel slow.`,
    };
  };

  // CPU first: when both are pegged, the CPU is the one an operator can usually act on.
  return verdict("cpu", (s) => s.cpu) ?? verdict("mem", (s) => s.mem);
}
