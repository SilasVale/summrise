// Sparkline — a series as a SHAPE, with its numbers still printed beside it.
//
// The geometry lives in `lib/spark.ts` (pure, tested); this component only draws it. Two
// disciplines are inherited from that file and both are visible here:
//
//   * A GAP IN THE DATA BREAKS THE LINE (`sparkSegments` returns one path per run). The
//     chart must not connect across a reading nobody took.
//   * AN EMPTY SERIES DRAWS NOTHING AT ALL — no baseline, no flat line at zero. "I have no
//     readings" and "the reading is zero" look identical in a chart that draws both as a
//     line along the floor, and this panel has that failure recorded more than once.
//
// The label is a full sentence with the numbers in it, so the shape is never the only
// channel (the rule `VitalsDial` states for its arcs).
import { sparkSegments, seriesStats } from "../lib/spark";

export function Sparkline({
  values,
  width = 132,
  height = 28,
  label,
  tone = "cpu",
  emptyLabel = "no readings",
}: {
  values: (number | null)[];
  width?: number;
  height?: number;
  /** What to say when there is nothing to draw. A CALLER'S FACT, not this component's: "no
   *  readings yet" (nothing has been probed) and "never answered" (probed, and every probe
   *  failed) are different situations, and the component cannot tell them apart. */
  emptyLabel?: string;
  /** What it is a picture OF, and its numbers — read out by a screen reader and shown as
   *  the hover. The chart is a summary; the sentence is the value. */
  label: string;
  tone?: "cpu" | "mem";
}) {
  const paths = sparkSegments(values, width, height);
  const stats = seriesStats(values);
  if (paths.length === 0 || !stats) {
    // Nothing measured: draw the frame and say so in the label, so the card can render an
    // honest empty state instead of a chart-shaped lie.
    return (
      <span className="spark spark-empty" title={label} aria-label={label} role="img">
        <span className="spark-none">{emptyLabel}</span>
      </span>
    );
  }
  return (
    <span className="spark" title={label} role="img" aria-label={label}>
      <svg
        className="spark-svg"
        data-tone={tone}
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        aria-hidden="true"
      >
        {paths.map((d, i) => (
          <path key={i} className="spark-line" d={d} fill="none" />
        ))}
      </svg>
    </span>
  );
}
