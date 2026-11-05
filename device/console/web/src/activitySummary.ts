import { auditFailed } from "./auditFilter";
import type { AuditLine } from "./api";

/**
 * The two tool names the console writes when a device's channel closes and opens.
 *
 * They are console facts, not tool calls: a `device` line with `caller: "console"` records the
 * boundary the fleet's absence is measured from. The timeline keeps them out of its rows, and no
 * count on the page may treat them as work.
 */
const CHANNEL_BOUNDARIES = ["device.left", "device.joined"];

/** Whether an audit line is one of the console's channel boundaries rather than a call. */
export function isChannelBoundary(line: Pick<AuditLine, "tool">): boolean {
  return line.tool != null && CHANNEL_BOUNDARIES.includes(line.tool);
}

/**
 * Whether a line is a client's call, by the same proof the hub's day line uses.
 *
 * The `caller` field is newer than the audit itself: 908 of the live 1000 lines have no caller, and
 * 376 of those carry a token — no console line carries one (0 of 30). Counting only `caller ===
 * "client"` undercounted the busiest machine's day by 5.9× (62 counted, 363 proven). A line with a
 * token is a client call even when the caller was never recorded; a console line never is, even if
 * a token were to appear on one. A channel boundary is never one, whatever else the line says.
 */
export function isClientCall(
  line: Pick<AuditLine, "caller" | "token_id" | "tool">
): boolean {
  if (isChannelBoundary(line)) return false;
  if (line.caller === "client") return true;
  if (line.caller === "console") return false;
  return line.token_id != null;
}

/**
 * The three buckets the Activity header counts, and the failures inside each one.
 *
 * `console` is a bucket of its own rather than part of `unattributed`: the console's own polling is
 * a known caller, and folding its 30 rows into "nobody can say who" would overstate the unknown.
 *
 * The failures are counted per bucket and never summed into one silent number: the measured header
 * read "301 client calls · 55 failed", and 18 of those 55 were not the AI's (15 console, 2
 * unattributed), so "failed" beside "client calls" was 1.5× the client failure rate. `clientFailed`
 * is the one the sentence leads with; the other two are named for what they are.
 */
export type ActivityCounts = {
  client: number;
  clientFailed: number;
  console: number;
  consoleFailed: number;
  unattributed: number;
  unattributedFailed: number;
};

/** Midnight in the console's own timezone: the header counts today, not the window's whole span. */
function startOfToday(now: number): number {
  const date = new Date(now);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

/** Today's rows, split by who can be proven to have made them, with each bucket's failures. */
export function activityCounts(lines: readonly AuditLine[], now = Date.now()): ActivityCounts {
  const start = startOfToday(now);
  const counts: ActivityCounts = {
    client: 0,
    clientFailed: 0,
    console: 0,
    consoleFailed: 0,
    unattributed: 0,
    unattributedFailed: 0
  };
  for (const line of lines) {
    if (line.ts_ms < start) continue;
    // A channel boundary is a console fact, not a call: it belongs to none of the three buckets.
    if (isChannelBoundary(line)) continue;
    const failed = auditFailed(line);
    if (isClientCall(line)) {
      counts.client += 1;
      if (failed) counts.clientFailed += 1;
    } else if (line.caller === "console") {
      counts.console += 1;
      if (failed) counts.consoleFailed += 1;
    } else {
      counts.unattributed += 1;
      if (failed) counts.unattributedFailed += 1;
    }
  }
  return counts;
}

/**
 * The header's one sentence.
 *
 * It leads with the AI's calls and the AI's failures, so the number beside "client calls" is the
 * hub's number and not every caller's. The console's own failures follow as their own clause,
 * because they used to be folded into that first number (15 of the measured 55). The unattributed
 * bucket is a count of rows, and the failures among them are stated in a parenthetical only when
 * there are any — otherwise the line would suggest the unknown rows were all successful.
 *
 * With `clientOnly` (the "Clients only" chip) the sentence is the hub's own shape: the client call
 * count and the client failure count, and nothing the chip has hidden from the list.
 */
export function activityCountSentence(counts: ActivityCounts, clientOnly = false): string {
  const calls = counts.client === 1 ? "client call" : "client calls";
  const ai = `Today ${counts.client} ${calls} · ${counts.clientFailed} failed`;
  if (clientOnly) return ai;
  const consoleClause =
    counts.consoleFailed === 0
      ? ""
      : ` · ${counts.consoleFailed} console ${
          counts.consoleFailed === 1 ? "failure" : "failures"
        }`;
  const unknownClause =
    counts.unattributedFailed === 0
      ? `${counts.unattributed} unattributed`
      : `${counts.unattributed} unattributed (${counts.unattributedFailed} failed)`;
  return `${ai}${consoleClause} · ${unknownClause}`;
}

/** The sentence from a window and the moment it is read; `clientOnly` mirrors the chip. */
export function activityCountLine(
  lines: readonly AuditLine[],
  now = Date.now(),
  clientOnly = false
): string {
  return activityCountSentence(activityCounts(lines, now), clientOnly);
}
