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

/** THE MODIFIER ALONE — the same state, for the elements that carry the family's vocabulary without being the mark itself.
 *
 *  `MonitorAlerts` renders `<div className={"monitor-alert " + …}>` around a `<span className="monitor-mark …">`, and both
 *  describe ONE fact: this alert is up or down. Round 126 gave the mark the derivation and left the container spelling the
 *  words by hand — the gate this module is guarded by found it on its next run, which is what it is for. */
export function monitorModifier(state: MonitorMarkState): string {
  return state === "up" ? "is-up" : state === "down" ? "is-down" : "is-flapping";
}

/** The class list for a monitor mark. `flapping` wins over `up`: a target that is up now but has been dropping is the
 *  thing the chip exists to say. */
export function monitorMarkClass(state: MonitorMarkState, extra = ""): string {
  return ["monitor-mark", monitorModifier(state), extra].filter(Boolean).join(" ");
}
