// ViewSwitch — the per-session view selector, ONE copy for both densities.
//
// Extracted when the PATH view became the third option. Both densities had
// grown their own two-button copy (TabBar's `.view-switch` and DesktopShell's
// `.desktop-view-switch`), and R131's commit records exactly how that ends:
// "fixing one and missing the others is how they drifted apart in the first
// place". Adding a third button to two hand-maintained copies is the same
// mistake with a shorter fuse, so the label list now lives here once.
//
// The two densities keep their own CONTAINER styling (different chrome, pill vs
// rounded-rect) via `className`; only the button set and the accessibility
// wiring are shared.
import type { SessionView } from "./TabBar";

/** Label + tooltip per view, in display order.
 *
 *  THE TWO LABELS WERE "Trajectory" AND "Path" UNTIL 2026-09-26, AND THE OPERATOR SAID SO TWICE — "Are these controls
 *  duplicated? A lot of it I cannot understand" — because neither word says which is which; the difference lived in a
 *  tooltip nobody hovers. **"Timeline" and "Steps" say it without hovering**: one is every event in order, the other is
 *  the same work grouped into what it was trying to do. This is the rename the loop recommended and left to the operator,
 *  who delegated it (`docs/agents/ideas.md` row 19).
 *
 *  THE INTERNAL NAMES KEEP THEIR SPELLING — `TrajectoryView`, `useTrajectory`, `lib/path.ts` — because they name the DATA
 *  (the raw audit timeline, the derived path through it), and the labels name what a PERSON sees. Renaming the data to
 *  match a button would be the tail wagging the dog. */
const VIEW_LABELS: Array<{ id: SessionView; label: string; title: string }> = [
  {
    id: "terminal",
    label: "Terminal",
    title: "The live terminal session",
  },
  {
    id: "trajectory",
    label: "Timeline",
    title: "Raw audit timeline — every event, exactly as logged",
  },
  {
    id: "path",
    label: "Steps",
    title: "This session's work as steps, with a summary of how it went",
  },
];

export function ViewSwitch({ view, onChange, className }: {
  view: SessionView;
  onChange: (v: SessionView) => void;
  /** Container class: `view-switch` (panel) or `desktop-view-switch`. */
  className: string;
}) {
  return (
    <div className={className} role="tablist" aria-label="Session view">
      {VIEW_LABELS.map((v) => (
        <button
          key={v.id}
          type="button"
          role="tab"
          aria-selected={view === v.id}
          className={`view-switch-btn${view === v.id ? " active" : ""}`}
          title={v.title}
          onClick={() => onChange(v.id)}
        >{v.label}</button>
      ))}
    </div>
  );
}
