import type { AuditLine } from "./api";
import type { LiveLine } from "./liveActivity";
import { isChannelBoundary } from "./activitySummary";

/** The merged window, and how many lines one read (or the other) had already carried. */
export type MergedActivity = {
  lines: AuditLine[];
  /** Lines dropped because the same line — instant, tool, status — was already in the list. */
  repeats: number;
};

/**
 * One list out of two reads of the same file, and how many repeats it dropped.
 *
 * The activity page fetches `/audit` (a window of the index) and also follows `/events` (the newest
 * lines, live). The live lines are the newest of the same lines, so they come first and the fetched
 * window fills in behind them; a line seen through both is shown once, keyed by the facts that make
 * it the same line — its time, the tool it named and the status it returned. `repeats` is the
 * count of lines the merge did not carry, which the cap sentence names rather than leaving the
 * difference between fetched and drawn unexplained.
 */
export function mergeActivityReport(audit: AuditLine[], live: LiveLine[]): MergedActivity {
  const seen = new Set<string>();
  const merged: AuditLine[] = [];
  let repeats = 0;
  for (const line of [...live, ...audit]) {
    const key = `${line.ts_ms}-${line.tool ?? ""}-${line.status}`;
    if (seen.has(key)) {
      repeats += 1;
      continue;
    }
    seen.add(key);
    merged.push(line);
  }
  return { lines: merged, repeats };
}

/** The merged window alone; callers that need the withheld count use [`mergeActivityReport`]. */
export function mergeActivity(audit: AuditLine[], live: LiveLine[]): AuditLine[] {
  return mergeActivityReport(audit, live).lines;
}

/**
 * The console's `/audit` cap (`MAX_LOG_LIMIT` in `device/console/src/http.rs`). The page asks for
 * this many lines, and live events can add a few newer ones on top, so the shown list is capped at
 * the same number rather than growing past what the page called its day.
 */
export const ACTIVITY_LIMIT = 1000;

export type ActivityWindow = {
  lines: AuditLine[];
  /** The merged list was cut at the limit: rows the page already held are not drawn. */
  capped: boolean;
  /**
   * The fetched window came back with exactly the requested limit.
   *
   * This is a fact about the request, not about the merged list: three duplicate live lines can
   * leave 997 rows on screen from a full 1000-line answer, and the page must still say it asked for
   * a full window. It says no more than that: the server cannot prove an older line exists.
   */
  atLimit: boolean;
  /**
   * Lines both reads returned, before the merge dropped repeats. The cap sentence says "of N
   * fetched" from this, not from the list's length: the differences are the rows it names.
   */
  fetched: number;
  /**
   * Channel boundaries (`device.left`/`device.joined`) in the window this page holds. The timeline
   * and the hub never draw them, so they are part of what the sentence says was withheld; the
   * audit table draws them and passes its own count instead.
   */
  boundaries: number;
  /** Lines the merge dropped because an equal line was already in the list. */
  repeats: number;
};

/** What one surface of the page withheld from a fetched window, for the cap sentence. */
export type WindowCounts = {
  /** Lines the reads returned, before repeats were dropped. */
  fetched: number;
  /** How many of the merged window this surface draws. */
  drawn: number;
  /** Channel boundaries the merge held and this surface does not draw. */
  boundaries: number;
  /** Lines the merge dropped because an earlier line had the same identity. */
  repeats: number;
};

/** The rows a cap left, and whether it cut any: all `capActivity` can know from the list. */
export type CappedActivity = {
  lines: AuditLine[];
  capped: boolean;
};

/**
 * The one sentence a capped or full list shows.
 *
 * It describes the fetched window, not a day: the rows behind the cut may be yesterday's, so
 * "of today" would be a claim the row list cannot keep. When the window is one machine's, the cut
 * sentence names it: the cut is that machine's lines, not the fleet's.
 *
 * `counts` makes the sentence name what the reader can see and what was withheld: the list's length
 * is not the fetch's when the merge dropped repeated keys or a view dropped channel boundaries. The
 * three figures come from the same merge the list does. With nothing withheld the sentence keeps
 * its simpler form, because "1000 of 1000 fetched" would be noise.
 *
 * `at-limit` is the other fact: the request filled and nothing else was dropped, so the sentence
 * names the window's size and claims nothing about what lies behind it. It names no machine because
 * no cut happened; the machine is the page's scope, not this sentence's subject.
 */
export function capSentence(
  limit = ACTIVITY_LIMIT,
  machine: string | null = null,
  fact: "capped" | "at-limit" = "capped",
  counts: WindowCounts | null = null
): string {
  const withheld = withheldClause(counts);
  if (counts !== null && withheld !== null) {
    const from = machine === null ? "" : ` from ${machine}`;
    const tail = fact === "capped" ? "; older ones are not on this page." : ".";
    return `Showing the newest ${counts.drawn} lines of ${counts.fetched} fetched${from} (${withheld})${tail}`;
  }
  if (fact === "at-limit") return `Showing the newest ${limit} lines — the window's limit.`;
  const from = machine === null ? "" : ` from ${machine}`;
  return `Showing the newest ${limit} lines${from}; older ones are not on this page.`;
}

/**
 * The parenthetical's content: how many channel events and repeated lines were not drawn.
 *
 * Only the kinds that exist are named — a window with no boundary must not claim one — and "not
 * drawn" is stated once at the end because it applies to both counts.
 */
function withheldClause(counts: WindowCounts | null): string | null {
  if (counts === null) return null;
  const parts: string[] = [];
  if (counts.boundaries > 0) {
    parts.push(`${counts.boundaries} channel ${counts.boundaries === 1 ? "event" : "events"}`);
  }
  if (counts.repeats > 0) {
    parts.push(`${counts.repeats} ${counts.repeats === 1 ? "repeat" : "repeats"}`);
  }
  return parts.length === 0 ? null : `${parts.join(", ")} not drawn`;
}

/** "3 console reads", "1 channel event": the number and the noun it keeps. */
function countOf(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

/**
 * What the hub's window can draw, and what it never can.
 *
 * One rule, and it is Activity's header's rule: every non-boundary line is a client's call
 * (`isClientCall`), the console's own read (`isConsoleRead`), a failed poll (`isPoll`), or
 * unattributed — a real line written before the audit recorded a caller, with no token that would
 * prove a client made it (`isUnattributed`). The hub draws the first three: client calls by default,
 * console reads behind one chip, unattributed rows behind the third fold. A poll is the console
 * retrying itself, not a read of the screen; it stays Activity's row and is named as not drawn here.
 */
export type HubWindowCounts = {
  /** Today's client-proven calls: the default list. */
  client: number;
  /** The console's own reads: one chip. */
  console: number;
  /** Lines no caller was recorded for: the third fold. */
  unattributed: number;
  /** Failed console polls: rows Activity draws, no hub control offers. */
  polls: number;
  /** Channel boundaries in the merged window: console facts, never rows on any timeline. */
  boundaries: number;
  /** Lines the merge dropped because an equal line was already in the list. */
  repeats: number;
  /** The rows the fold currently draws: `min(HUB_DAY_ROWS, the visible list)`, never a claim. */
  shown: number;
};

/**
 * The hub's one window sentence.
 *
 * **IT MAY NOT NAME A NUMBER THE PAGE CANNOT REACH.** The fetched window can be larger than the list
 * the fold and the two chips present — the measured hub fetched 1004 lines under a sentence that
 * offered 974, while expanding and turning on the console chip drew 835. So the total here is
 * `client + console + unattributed`, the rows the three controls actually hold, and the fetched
 * figure (the one with the repeats and boundaries still in it) is deliberately absent. The bucket
 * clauses are those controls' counts and add up to the total; the withheld clauses are marked "not
 * drawn", which is the one thing the page says it cannot show.
 *
 * When the fold has drawn every drawable row the sentence says "all" instead of "the newest 974 of
 * 974"; `shown` is the list's current length, so it moves with the chips and the fold.
 */
export function hubWindowSentence(counts: HubWindowCounts): string {
  const drawable = counts.client + counts.console + counts.unattributed;
  const shown = Math.max(0, Math.min(counts.shown, drawable));
  const buckets: string[] = [];
  if (counts.client > 0) buckets.push(countOf(counts.client, "client call", "client calls"));
  if (counts.console > 0) buckets.push(countOf(counts.console, "console read", "console reads"));
  if (counts.unattributed > 0) {
    buckets.push(countOf(counts.unattributed, "unattributed", "unattributed"));
  }
  const withheld: string[] = [];
  if (counts.polls > 0) withheld.push(countOf(counts.polls, "poll", "polls"));
  if (counts.boundaries > 0) {
    withheld.push(countOf(counts.boundaries, "channel event", "channel events"));
  }
  if (counts.repeats > 0) {
    withheld.push(countOf(counts.repeats, "repeat", "repeats"));
  }
  // The bucket clauses add up to the total the sentence leads with; the withheld ones are the only
  // rows it names without a control, so they carry the qualifier that says so.
  const parenthetical = [
    buckets.join(", "),
    withheld.length > 0 ? `${withheld.join(", ")} not drawn` : ""
  ]
    .filter((part) => part !== "")
    .join("; ");
  const head =
    drawable === 0
      ? "No line on this page can be drawn"
      : shown >= drawable
        ? `Showing all ${countOf(drawable, "line", "lines")}`
        : `Showing the newest ${shown} of ${countOf(drawable, "line", "lines")}`;
  return `${head}${parenthetical === "" ? "" : ` (${parenthetical})`}; older ones are not on this page.`;
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
export function capActivity(lines: AuditLine[], limit = ACTIVITY_LIMIT): CappedActivity {
  // `atLimit` is the fetch's fact, not the merged list's: this function only knows the list.
  if (lines.length <= limit) return { lines, capped: false };
  // Both sources arrive newest first and the merge keeps that order, so the tail is the older end.
  return { lines: lines.slice(0, limit), capped: true };
}

/**
 * The merged list, capped, with the one fact only the fetch can supply.
 *
 * The merge can shrink a full window (a line seen through both the file and the live stream is
 * drawn once), so `atLimit` is read from the fetched answer's length. A list that was both full
 * and cut at the merge keeps both facts; the views draw the cut sentence, which is the stronger.
 * The counts are taken after the cut: the sentence describes the window the page holds.
 */
export function mergeActivityWindow(
  audit: AuditLine[],
  live: LiveLine[],
  limit = ACTIVITY_LIMIT
): ActivityWindow {
  const merged = mergeActivityReport(audit, live);
  const cut = capActivity(merged.lines, limit);
  return {
    ...cut,
    atLimit: audit.length >= limit,
    fetched: audit.length + live.length,
    boundaries: cut.lines.filter(isChannelBoundary).length,
    repeats: merged.repeats
  };
}
