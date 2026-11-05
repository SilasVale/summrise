import type { DeviceUpdate } from "./api";

/**
 * THE DEVICE'S OWN ACCOUNT OF ITS LAST START, OR NOTHING.
 *
 * The console cannot tell a machine that slept from one an update broke: it knows only that frames
 * stopped. So it says the silence (`offline · last seen 7m ago`) and, beside it, repeats what the
 * device said about itself. This module builds that second line from the section the device sent —
 * the verdict's `detail`, and the updater's outcome and versions — and adds only the parentheses and
 * the one word the operator uses for an applied release ("updated").
 *
 * The two forms the operator reads:
 *
 *   started 0.3.275 (update to 0.3.276 refused)
 *   started 0.3.276 (updated)
 *
 * A verdict that is itself an updater sentence ("updated to 0.3.276", "refused: …") is shown alone:
 * the running app writes those when it folds a result in without restarting, and a parenthetical
 * would repeat the same fact in the console's grammar. A sentence with no update clause needs no
 * parentheses, and no verdict at all draws no line — a device that never sent the section did not
 * say "started nothing", it said nothing.
 */
export function startedLine(update: DeviceUpdate | null | undefined): string | null {
  const detail = update?.verdict?.detail?.trim();
  if (!detail) return null;
  const last = update?.last_update;
  // Only a start sentence gets the updater's answer appended. The device's own update sentence is
  // already the whole story, and the console must not paraphrase it into a second clause.
  if (!last || !detail.startsWith("started ")) return detail;
  const clause = updateClause(last, update?.rolled_back_to);
  return clause ? `${detail} (${clause})` : detail;
}

/**
 * The updater's last answer as the parenthetical, in the device's vocabulary.
 *
 * `refused` and `rolled_back` are the device's own words and are used unchanged; `applied` is the
 * one the page reads as "updated" because that is what the operator calls a release that took over.
 * An outcome this console does not know returns null, so a newer device's word draws no parenthetical
 * rather than a guessed one.
 */
function updateClause(
  last: NonNullable<DeviceUpdate["last_update"]>,
  rolledBackTo?: string
): string | null {
  switch (last.outcome) {
    case "refused":
      return last.to ? `update to ${last.to} refused` : "update refused";
    case "applied":
      return "updated";
    case "rolled_back":
      if (last.to) {
        return rolledBackTo
          ? `update to ${last.to} rolled back to ${rolledBackTo}`
          : `update to ${last.to} rolled back`;
      }
      return rolledBackTo ? `rolled back to ${rolledBackTo}` : "rolled back";
    default:
      return null;
  }
}
