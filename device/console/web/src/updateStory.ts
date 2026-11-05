import type { DeviceUpdate } from "./api";
import { seenAgo } from "./format";

/**
 * THE DEVICE'S OWN ACCOUNT OF ITS LAST START, OR THE ONE UPDATE FACT THAT IS NEW.
 *
 * The console cannot tell a machine that slept from one an update broke: it knows only that frames
 * stopped. So it says the silence (`offline · last seen 7m ago`) and, beside it, repeats only what
 * the device said that the surface does not already show. This module builds that second line from
 * the section the device sent — the verdict's `detail`, and the updater's last answer — and adds
 * only the age and the one word the operator uses for an applied release ("updated").
 *
 * THE VERSION HAS ONE HOME PER SURFACE. The fleet row's chip and the header's chip already carry
 * the running version, so the line never repeats it: a start sentence whose only content is that
 * version draws no line at all. What remains is the fact the chips cannot state:
 *
 *   updated 12m ago                              the answer arrived inside this start
 *   update to 0.3.276 refused                    the updater's refusal, inside this start
 *   update to 0.3.276 rolled back to 0.3.275     the updater's rollback, inside this start
 *   refused: nothing was installed               the device's own current sentence
 *
 * NO SENTENCE ATTACHES AN UPDATE TO A START IT PREDATES. `last_update.at_ms` belongs to a process
 * (the one that folded the answer in); the newest entry in the host frame's `restarts` journal is
 * when the running process started. An answer before that moment is history — the running version
 * proves it took — so the line says nothing about it. Without either moment the console cannot prove
 * the answer belongs to this start, and says nothing about the update rather than guess.
 */
export type StartStoryContext = {
  /** When the running process started, from the host frame's newest restart entry. */
  startedAtMs?: number | null;
  /** The version the surface shows beside this line, so the line does not repeat it. */
  shownVersion?: string | null;
  /** The clock the age is read against; the components leave it to `Date.now()`. */
  nowMs?: number;
};

export function startedLine(
  update: DeviceUpdate | null | undefined,
  context: StartStoryContext = {}
): string | null {
  const detail = update?.verdict?.detail?.trim();
  if (!detail) return null;
  const last = update?.last_update ?? null;

  // The device's own current sentence. The running app writes one when it folds the updater's
  // answer in without restarting; the console shows it as written. The one exception is an applied
  // sentence that names the version the surface already shows: its new fact is when, so it is aged
  // instead of repeated.
  if (!detail.startsWith("started ")) {
    const updatedTo = /^updated to\s+(\S+)/.exec(detail)?.[1] ?? null;
    if (updatedTo !== null && updatedTo === context.shownVersion) {
      return last && insideStart(last.at_ms, context)
        ? `updated ${seenAgo(last.at_ms, context.nowMs)}`
        : null;
    }
    return detail;
  }

  // A start sentence's version is a repeat when the surface shows the same one. A start naming a
  // version the surface does not show is the contradiction the operator needs, so it stays.
  const startVersion = detail.slice("started ".length).trim();
  const base =
    startVersion !== "" && startVersion !== (context.shownVersion ?? null) ? detail : null;
  const clause = updateClause(last, update?.rolled_back_to, context);
  if (!clause) return base;
  return base ? `${base} (${clause})` : clause;
}

/**
 * The moment the running process started, from the host frame's own boot journal.
 *
 * `host.restarts` is the device's record of its own boots, newest first; the newest entry is this
 * process. The machine's `uptime_s` is not that fact — it is how long the host has been up — and a
 * line built from it could attach an old answer to a start it predates.
 */
export function processStartMs(device: {
  host?: { restarts?: { at_ms?: number }[] | null } | null;
}): number | null {
  const at = device.host?.restarts?.[0]?.at_ms;
  return typeof at === "number" && Number.isFinite(at) && at > 0 ? at : null;
}

function insideStart(atMs: number, context: StartStoryContext): boolean {
  const startedAt = context.startedAtMs;
  // Both moments must exist. A missing start is not "probably after": rule (c) forbids borrowing
  // one start's moment for another start's answer.
  return typeof startedAt === "number" && Number.isFinite(startedAt) && atMs >= startedAt;
}

/**
 * The updater's last answer as the line's own sentence, or null when it belongs to another start.
 *
 * `refused` and `rolled_back` are the device's own words and are used unchanged; `applied` is the
 * one the page reads as "updated" because that is what the operator calls a release that took over,
 * and it is aged rather than dated absolutely: the question is "did the 0.3.277 update take", and
 * "12m ago" answers when. An outcome this console does not know returns null, so a newer device's
 * word draws no guessed sentence.
 */
function updateClause(
  last: NonNullable<DeviceUpdate["last_update"]> | null,
  rolledBackTo: string | undefined,
  context: StartStoryContext
): string | null {
  if (!last || !insideStart(last.at_ms, context)) return null;
  switch (last.outcome) {
    case "refused":
      return last.to ? `update to ${last.to} refused` : "update refused";
    case "applied":
      return `updated ${seenAgo(last.at_ms, context.nowMs)}`;
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
