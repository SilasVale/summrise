// LoadChip — "this device has been pegged for twelve minutes", said out loud.
//
// The strip's dial and its two percentages are INSTANTANEOUS: they can show 96% during a
// build and 4% a moment later, and neither reading tells an operator whether the machine is
// struggling or just busy. The agent keeps a series (`metrics.rs`), `lib/spark.ts` owns the
// rule that reads it, and this component renders that rule's answer — nothing else.
//
// IT IS SILENT BY DEFAULT, and the silence is the feature: the rule needs ten readings and
// two thirds of them in the critical band, so a burst cannot raise it. A chip that fired on
// a spike would be one an operator learns to ignore before the day it matters.
//
// Shape as well as colour, like every other chip here: the mark is a filled square for the
// sustained-load state, distinct from the waiting chip's diamond and the boot chip's
// triangle, because this repo has an incident where two states differed by hue alone.
import { loadNotice } from "../lib/spark";
import type { VitalsSeries } from "../hooks/useVitalsSeries";

export function LoadChip({
  series,
  nowMs = Date.now(),
}: {
  series?: VitalsSeries | null;
  nowMs?: number;
}) {
  if (!series || series.samples.length === 0) return null;
  const notice = loadNotice(
    series.samples.map((s) => ({ tsMs: s.tsMs, cpu: s.cpu, mem: s.mem })),
    nowMs,
  );
  if (!notice) return null;
  return (
    <span className={`load-chip ${notice.tone}`} title={notice.title} data-metric={notice.metric}>
      <span className={`load-mark ${notice.tone}`} aria-hidden="true" />
      {notice.text}
    </span>
  );
}
