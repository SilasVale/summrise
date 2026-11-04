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
