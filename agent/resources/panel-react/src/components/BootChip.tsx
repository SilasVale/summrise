// The boot notice — the chip itself. The RULE lives in lib/bootNotice.ts (pure,
// tested there); this component only renders its answer, in both densities.
//
// Same shape discipline as WaitingChip: rendered ONLY when there is something to say,
// the mark is a SHAPE (a triangle for the fault, a dot for the news) so the two tones
// are distinguishable without colour, and the full device sentence rides the `title`.
import { bootNotice } from "../lib/bootNotice";
import type { LastBoot } from "../hooks/useAgentVitals";

export function BootChip({
  lastBoot,
  uptimeSecs,
}: {
  lastBoot?: LastBoot | null;
  uptimeSecs?: number | null;
}) {
  const notice = bootNotice(lastBoot, uptimeSecs);
  if (!notice) return null;
  return (
    <span className={`boot-chip ${notice.tone}`} title={notice.title}>
      <span className={`boot-mark ${notice.tone}`} aria-hidden="true" />
      {notice.text}
    </span>
  );
}
