import type { AuditLine, AuditSource } from "./api";

/** A byte count at the scale a file size is read: whole bytes, or one decimal of KiB/MiB. */
function bytesLabel(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${bytes} B`;
}

/** A moment as `MM-DD HH:MM` in the operator's own timezone, with seconds when asked. */
function monthDayMinute(ts: number, withSeconds = false): string {
  const date = new Date(ts);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(
    date.getMinutes()
  )}${withSeconds ? `:${pad(date.getSeconds())}` : ""}`;
}

/** Whether two moments print the same `MM-DD HH:MM`, i.e. their difference is invisible at that scale. */
function sameMinute(left: number, right: number): boolean {
  return monthDayMinute(left) === monthDayMinute(right);
}

/**
 * The two ends of the window this page holds: its oldest and newest line moments.
 *
 * It is the drawn window's own span, not the file's: `lines` is the list the page drew (the fetched
 * window plus any live lines merged into it), and a filter narrows the rows without narrowing the
 * window. A line without a finite moment is not evidence for either end, so it is skipped; no line
 * with a moment at all means there is no span to state.
 */
export type AuditWindowSpan = {
  oldest_ts_ms: number;
  newest_ts_ms: number;
};

export function auditWindowSpan(
  lines: readonly Pick<AuditLine, "ts_ms">[]
): AuditWindowSpan | null {
  let oldest: number | null = null;
  let newest: number | null = null;
  for (const line of lines) {
    const ts = line.ts_ms;
    if (typeof ts !== "number" || !Number.isFinite(ts)) continue;
    if (oldest === null || ts < oldest) oldest = ts;
    if (newest === null || ts > newest) newest = ts;
  }
  return oldest !== null && newest !== null ? { oldest_ts_ms: oldest, newest_ts_ms: newest } : null;
}

/**
 * The one line that says which file the window is read from, how far back that file goes, and which
 * span this page's window covers.
 *
 * It is the cap sentence's companion, not a second cap sentence: that one says what was **drawn**,
 * this one says what was **read**. The file's lifetime and the window's span are different facts and
 * are stated as such: `lines since 10-22 18:55` is the file's oldest line, while `This window:
 * 11-06 21:01 → 11-08 01:52` is the span the page is actually showing. Mixing the two made the
 * 17-day file lifetime read as this page's range (audit-44 F10).
 *
 * Every file figure comes from `/audit/source`; the window's ends come from the lines the page
 * holds. A fact that could not be read is left out rather than replaced by a zero or an invented
 * moment. `null` means there is nothing the page can honestly say — the route did not answer, or the
 * file could not be read — and the page draws nothing.
 *
 * `source.bytes` of zero is a readable empty file and is stated; a missing size is absent from the
 * line entirely. The rotated clause is present only when the server proved a predecessor exists;
 * its date is present only when that file's first line could be read.
 */
export function auditSourceSentence(
  source: AuditSource | null | undefined,
  window?: AuditWindowSpan | null
): string | null {
  if (source === null || source === undefined || source.file === undefined || source.file === "") {
    return null;
  }
  let head = `Reading ${source.file}`;
  if (source.bytes !== undefined) head += ` — ${bytesLabel(source.bytes)}`;
  if (source.oldest_ts_ms !== undefined) {
    head += `, lines since ${monthDayMinute(source.oldest_ts_ms)}`;
  }
  let tail = "";
  if (source.rotated !== undefined) {
    tail = "rotated once";
    if (source.rotated.oldest_ts_ms !== undefined) {
      tail += `, older lines start ${monthDayMinute(source.rotated.oldest_ts_ms)}`;
    }
  }
  const file = `${head}${tail === "" ? "" : `; ${tail}`}.`;
  if (
    window === null ||
    window === undefined ||
    !Number.isFinite(window.oldest_ts_ms) ||
    !Number.isFinite(window.newest_ts_ms) ||
    window.newest_ts_ms < window.oldest_ts_ms
  ) {
    return file;
  }
  // One line has one moment: an arrow between a moment and itself would draw a span that is not
  // there. A span inside one minute keeps its seconds; otherwise the two ends would print as the
  // same moment and the window would look empty.
  let span: string;
  if (window.oldest_ts_ms === window.newest_ts_ms) {
    span = monthDayMinute(window.oldest_ts_ms);
  } else if (sameMinute(window.oldest_ts_ms, window.newest_ts_ms)) {
    span = `${monthDayMinute(window.oldest_ts_ms, true)} → ${monthDayMinute(
      window.newest_ts_ms,
      true
    )}`;
  } else {
    span = `${monthDayMinute(window.oldest_ts_ms)} → ${monthDayMinute(window.newest_ts_ms)}`;
  }
  return `${file} This window: ${span}.`;
}
