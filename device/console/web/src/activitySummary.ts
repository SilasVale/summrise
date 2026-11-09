import { auditFailed } from "./auditFilter";
import type { AuditDay, AuditLine } from "./api";
import { coverageClause } from "./format";

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
 * Whether a line is a failed console poll rather than a call.
 *
 * A poll that succeeded is not in the audit at all: it was a read of the same screen the console
 * already shows. A failed one stays a line so the operator can find the machine's silence in the
 * Activity Table, but it is the console retrying itself, never the AI's work — so it is counted in
 * its own bucket, excluded from the client, console and unattributed failure counts, and the hub's
 * day never treats it as a call.
 */
export function isPoll(line: Pick<AuditLine, "kind">): boolean {
  return line.kind === "poll";
}

/**
 * Whether a line is a client's call, by the same proof the hub's day line uses.
 *
 * The `caller` field is newer than the audit itself: 908 of the live 1000 lines have no caller, and
 * 376 of those carry a token — no console line carries one (0 of 30). Counting only `caller ===
 * "client"` undercounted the busiest machine's day by 5.9× (62 counted, 363 proven). A line with a
 * token is a client call even when the caller was never recorded; a console line never is, even if
 * a token were to appear on one. A channel boundary is never one, and neither is a failed poll,
 * whatever else the line says.
 */
export function isClientCall(
  line: Pick<AuditLine, "caller" | "token_id" | "tool" | "kind">
): boolean {
  if (isChannelBoundary(line)) return false;
  if (isPoll(line)) return false;
  if (line.caller === "client") return true;
  if (line.caller === "console") return false;
  return line.token_id != null;
}

/**
 * Whether a line is the console's own read of a machine rather than work the AI asked for.
 *
 * The caller is the console's own statement of who made the call. An older line says nothing, and an
 * absent caller is not proof of the console: it belongs to the unattributed bucket. The hub folds
 * these rows behind one chip; Activity keeps them in the list, because the audit is the record. A
 * failed poll is a console fact but not a read of the screen — it gets its own bucket, so the chip
 * does not offer the machine's retries as console reads.
 */
export function isConsoleRead(
  line: Pick<AuditLine, "caller" | "kind" | "tool">
): boolean {
  // **A CHANNEL BOUNDARY IS IN NO BUCKET (the forty-fifth list).** `is_client_call` in the console's
  // `audit.rs` drops `device.joined` / `device.left` before anything else, so a boundary is never a
  // client call AND never a console read. This predicate used to answer `true` for a boundary the
  // console wrote; `activityCounts` tested the boundary first, so the day's numbers were right and
  // the predicate was wrong — which the shared table (docs/superpowers/specs/audit-buckets.json)
  // caught the moment both sides ran it.
  if (isChannelBoundary(line)) return false;
  return line.caller === "console" && !isPoll(line);
}

/**
 * Whether a line belongs to neither the AI nor the console, and is not a poll.
 *
 * This is the fourth bucket Activity's header counts: a real line the audit wrote before it recorded
 * a caller, with no token that would prove a client made it. A channel boundary and a failed poll are
 * facts of their own, so neither is unattributed. The hub uses exactly this predicate for its third
 * fold, which is why the chip's count and Activity's "N unattributed" cannot disagree.
 */
export function isUnattributed(
  line: Pick<AuditLine, "caller" | "token_id" | "tool" | "kind">
): boolean {
  return (
    !isChannelBoundary(line) && !isPoll(line) && !isClientCall(line) && !isConsoleRead(line)
  );
}

/**
 * The four buckets the Activity header counts, and the failures inside each one.
 *
 * `console` is a bucket of its own rather than part of `unattributed`: the console's own calls are a
 * known caller, and folding them into "nobody can say who" would overstate the unknown. `poll` is
 * the third: a failed console poll is not work by the console either, it is the console retrying a
 * read of the screen it already shows. It stays a row in the table, so the machine's silence is
 * visible, but it is excluded from every call and failure count.
 *
 * The failures are counted per bucket and never summed into one silent number: the measured header
 * read "301 client calls · 55 failed", and 18 of those 55 were not the AI's (15 console, 2
 * unattributed), so "failed" beside "client calls" was 1.5× the client failure rate. `clientFailed`
 * is the one the sentence leads with; the other three are named for what they are.
 */
export type ActivityCounts = {
  client: number;
  clientFailed: number;
  console: number;
  consoleFailed: number;
  /** Failed console polls: rows the table draws, no failure count includes them. */
  poll: number;
  pollFailed: number;
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
    poll: 0,
    pollFailed: 0,
    unattributed: 0,
    unattributedFailed: 0
  };
  for (const line of lines) {
    if (line.ts_ms < start) continue;
    // A channel boundary is a console fact, not a call: it belongs to none of the buckets.
    if (isChannelBoundary(line)) continue;
    const failed = auditFailed(line);
    // A poll is the console retrying itself; it keeps its own count so no failure number can read
    // a sleeping machine's retries as the AI's refusals. It is still a row in the table.
    if (isPoll(line)) {
      counts.poll += 1;
      if (failed) counts.pollFailed += 1;
    } else if (isClientCall(line)) {
      counts.client += 1;
      if (failed) counts.clientFailed += 1;
    } else if (isConsoleRead(line)) {
      counts.console += 1;
      if (failed) counts.consoleFailed += 1;
    } else if (isUnattributed(line)) {
      counts.unattributed += 1;
      if (failed) counts.unattributedFailed += 1;
    }
  }
  return counts;
}

/**
 * The header's one readable sentence.
 *
 * It leads with the AI's calls and the AI's failures, so the number beside "client calls" is the
 * day's number and not every caller's. The console's own failures follow as their own clause,
 * because they used to be folded into that first number (15 of the measured 55). A failed poll and
 * the unattributed rows are named after it, each only when its count is not zero: a zero is not a
 * fact the operator needs, and printing one made the old five-counter line read as a dashboard.
 *
 * The noun phrase `client calls` appears once, with the day as its only window. The fetched
 * window's client count belongs to the "Clients only" filter, whose label names its own window;
 * it must never be written beside this noun as a second claim about it (nineteenth item 5).
 *
 * The punctuation is the sentence's shape: a comma for the failures, one em dash for the
 * consequences, and semicolons between buckets. The console's clause carries its own noun —
 * `console failures` — because `2 from the console` left the number without a subject: the day's
 * 59 console reads could be read as the 2, and the 2 are the console's failures, not its calls
 * (audit-40 F9).
 */
function activitySentenceBody(counts: ActivityCounts): string {
  const calls = counts.client === 1 ? "client call" : "client calls";
  let sentence = `Today: ${counts.client} ${calls}`;
  if (counts.clientFailed > 0) sentence += `, ${counts.clientFailed} failed`;
  const clauses: string[] = [];
  if (counts.consoleFailed > 0) {
    clauses.push(
      `${counts.consoleFailed} console ${counts.consoleFailed === 1 ? "failure" : "failures"}`
    );
  }
  if (counts.pollFailed > 0) {
    clauses.push(
      `${counts.pollFailed} poll ${counts.pollFailed === 1 ? "failure" : "failures"}`
    );
  }
  if (counts.unattributed > 0) {
    const failed =
      counts.unattributedFailed > 0 ? ` (${counts.unattributedFailed} failed)` : "";
    clauses.push(`${counts.unattributed} unattributed${failed}`);
  }
  if (clauses.length > 0) sentence += ` — ${clauses.join("; ")}`;
  return sentence;
}

/** The header's sentence, from counts the page already has. */
export function activityCountSentence(counts: ActivityCounts): string {
  return `${activitySentenceBody(counts)}.`;
}

/**
 * The header's day sentence, from the day's own read rather than the fetched window.
 *
 * The measured defect: Activity read `Today 269 client calls · 39 failed` while the same local day
 * held 649 calls and 74 failures, because the number was counted from a 1,000-line window that slid
 * forward. This sentence takes `/audit/day`'s counts (the hub's rule and the fleet's `today`), and
 * appends `since` when the file starts after midnight instead of calling a partial day whole.
 */
export function activityDaySentence(day: AuditDay): string {
  const counts: ActivityCounts = {
    client: day.client_calls,
    clientFailed: day.failed,
    console: day.console,
    consoleFailed: day.console_failed,
    poll: day.poll,
    pollFailed: day.poll_failed,
    unattributed: day.unattributed,
    unattributedFailed: day.unattributed_failed
  };
  return `${activitySentenceBody(counts)}${coverageClause(day)}.`;
}

/** The sentence from a window and the moment it is read. */
export function activityCountLine(lines: readonly AuditLine[], now = Date.now()): string {
  return activityCountSentence(activityCounts(lines, now));
}
