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
