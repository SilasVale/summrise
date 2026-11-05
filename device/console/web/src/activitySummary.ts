import { auditFailed } from "./auditFilter";
import type { AuditLine } from "./api";

/**
 * Whether a line is a client's call, by the same proof the hub's day line uses.
 *
 * The `caller` field is newer than the audit itself: 908 of the live 1000 lines have no caller, and
 * 376 of those carry a token — no console line carries one (0 of 30). Counting only `caller ===
 * "client"` undercounted the busiest machine's day by 5.9× (62 counted, 363 proven). A line with a
 * token is a client call even when the caller was never recorded; a console line never is, even if
 * a token were to appear on one.
 */
export function isClientCall(line: Pick<AuditLine, "caller" | "token_id">): boolean {
  if (line.caller === "client") return true;
  if (line.caller === "console") return false;
  return line.token_id != null;
}

/**
 * The three buckets the Activity header counts, and the failures across all of them.
 *
 * `console` is a bucket of its own rather than part of `unattributed`: the console's own polling is
 * a known caller, and folding its 30 rows into "nobody can say who" would overstate the unknown.
 * `failed` is deliberately not per bucket: the live window's 12 failed rows were all unattributed,
 * so a client-only failure count would have read zero.
 */
export type ActivityCounts = {
  client: number;
  console: number;
  failed: number;
  unattributed: number;
};

/** Midnight in the console's own timezone: the header counts today, not the window's whole span. */
function startOfToday(now: number): number {
  const date = new Date(now);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

/** Today's rows, split by who can be proven to have made them, with the failures counted beside. */
export function activityCounts(lines: readonly AuditLine[], now = Date.now()): ActivityCounts {
  const start = startOfToday(now);
  const counts: ActivityCounts = { client: 0, console: 0, failed: 0, unattributed: 0 };
  for (const line of lines) {
    if (line.ts_ms < start) continue;
    if (isClientCall(line)) counts.client += 1;
    else if (line.caller === "console") counts.console += 1;
    else counts.unattributed += 1;
    if (auditFailed(line)) counts.failed += 1;
  }
  return counts;
}

/**
 * The header's one sentence.
 *
 * It states the AI's calls, the failures and the rows nobody can attribute. The console's own
 * reads are counted apart (in [`activityCounts`]) and stay out of both the client and the
 * unattributed number; they are not one of the three facts this line is for.
 */
export function activityCountSentence(counts: ActivityCounts): string {
  const calls = counts.client === 1 ? "client call" : "client calls";
  return `Today ${counts.client} ${calls} · ${counts.failed} failed · ${counts.unattributed} unattributed`;
}

/** The sentence from a window and the moment it is read. */
export function activityCountLine(lines: readonly AuditLine[], now = Date.now()): string {
  return activityCountSentence(activityCounts(lines, now));
}
