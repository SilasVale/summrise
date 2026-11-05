import type { AuditLine } from "./api";
import type { LiveLine } from "./liveActivity";

/**
 * One list out of two reads of the same file.
 *
 * The activity page fetches `/audit` (a window of the index) and also follows `/events` (the newest
 * lines, live). The live lines are the newest of the same lines, so they come first and the fetched
 * window fills in behind them; a line seen through both is shown once, keyed by the facts that make
 * it the same line — its time, the tool it named and the status it returned.
 */
export function mergeActivity(audit: AuditLine[], live: LiveLine[]): AuditLine[] {
  const seen = new Set<string>();
  const merged: AuditLine[] = [];
  for (const line of [...live, ...audit]) {
    const key = `${line.ts_ms}-${line.tool ?? ""}-${line.status}`;
    if (seen.has(key)) continue;
    seen.add(key);
    merged.push(line);
  }
  return merged;
}

/**
 * The console's `/audit` cap (`MAX_LOG_LIMIT` in `device/console/src/http.rs`). The page asks for
 * this many lines, and live events can add a few newer ones on top, so the shown list is capped at
 * the same number rather than growing past what the page called its day.
 */
export const ACTIVITY_LIMIT = 1000;

export type ActivityWindow = { lines: AuditLine[]; capped: boolean };

/**
 * The one sentence a capped list shows.
 *
 * It describes the fetched window, not a day: the rows behind the cut may be yesterday's, so
 * "of today" would be a claim the row list cannot keep. When the window is one machine's, the
 * sentence names it: the cut is that machine's lines, not the fleet's.
 */
export function capSentence(limit = ACTIVITY_LIMIT, machine: string | null = null): string {
  const from = machine === null ? "" : ` from ${machine}`;
  return `Showing the newest ${limit} lines${from}; older ones are not on this page.`;
}

/**
 * The one line shown while a scope change's window is being read.
 *
 * The rows still in hand belong to the previous window, so drawing them under the new scope would
 * answer a question about one machine with another machine's lines. One line says what is happening
 * instead; the rows return when the read answers.
 */
export function windowLoadingSentence(machine: string | null = null): string {
  return machine === null
    ? "Reading the fleet's window…"
    : `Reading ${machine}'s window…`;
}

/** The newest `limit` lines, and whether anything older was dropped to get there. */
export function capActivity(lines: AuditLine[], limit = ACTIVITY_LIMIT): ActivityWindow {
  if (lines.length <= limit) return { lines, capped: false };
  // Both sources arrive newest first and the merge keeps that order, so the tail is the older end.
  return { lines: lines.slice(0, limit), capped: true };
}

/** The merged list, capped. The page says so when `capped` is true; it must not hide rows silently. */
export function mergeActivityWindow(
  audit: AuditLine[],
  live: LiveLine[],
  limit = ACTIVITY_LIMIT
): ActivityWindow {
  return capActivity(mergeActivity(audit, live), limit);
}
