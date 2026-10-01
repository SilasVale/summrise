// THE MONITOR MARK'S STATE, DERIVED ONCE (round 126 of the standing goal).
//
// The spine's rule is one source of truth per fact, and this family was computing the same fact in two components with
// three spellings: `MonitorAlerts` wrote `a.up ? "is-up" : "is-down"`, `MonitorChip` wrote `is-flapping` for a flapping
// target and a BARE `monitor-mark` for a down one — so the sheet had to know that "no modifier" means down in one place and
// that `is-down` means it in another. Round 126 found that by tracing the fact, not by reading the sheet, and it is the same
// shape `one-derivation-check` guards for command endings.
//
// WHAT IT DOES NOT DO: change a single class on screen. The output is exactly what both components already rendered, which
// is why the rendered marks gate cannot see this change — the point is that the STATES are now named once, and a second
// rendering of the family cannot invent a fourth spelling.
type MonitorMarkState = "up" | "down" | "flapping";

// ── RUST SINCE 2026-09-30 (block ②), AND THE GATE FOLLOWS IT ────────────────────────────────────
//
// Both functions are `agent/resources/panel-logic/src/marks.rs` now; the differential is 169 corpus
// cases with 0 divergences and every arm reached, and the two components render byte-identically.
//
// `agent/tests/one_derivation.rs` — the gate this module is guarded by — scans the CRATE for the
// mark-state literals now, and `marks.rs` is the declared home. THE WRAPPER BELOW IS NO LONGER EXEMPT:
// it used to be skipped by name, and it is not any more, because a literal here would be a second
// spelling of the fact this module exists to derive once.
import { logic } from "../wasm/panelLogic";

/** THE MODIFIER ALONE — the same state, for the elements that carry the family's vocabulary without being the mark itself. */
export function monitorModifier(state: MonitorMarkState): string {
  return logic().monitor_modifier(state) as string;
}

/** The class list for a monitor mark. `flapping` wins over `up`: a target that is up now but has been dropping is the
 *  thing the chip exists to say. */
export function monitorMarkClass(state: MonitorMarkState, extra = ""): string {
  // `extra = ""` IS A DEFAULT PARAMETER, which fires only for `undefined` — so the wrapper decides that
  // case and the crate receives whatever the caller actually passed (`null` and `0` are filtered by
  // truthiness there, which is what `[...].filter(Boolean)` does).
  return logic().monitor_mark_class(state, extra === undefined ? "" : extra) as string;
}
