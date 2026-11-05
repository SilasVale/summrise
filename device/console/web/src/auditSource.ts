import type { AuditSource } from "./api";

/** A byte count at the scale a file size is read: whole bytes, or one decimal of KiB/MiB. */
function bytesLabel(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${bytes} B`;
}

/** A moment as `MM-DD HH:MM` in the operator's own timezone. */
function monthDayMinute(ts: number): string {
  const date = new Date(ts);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(
    date.getMinutes()
  )}`;
}

/**
 * The one line that says which file the window is read from and how far back it goes.
 *
 * It is the cap sentence's companion, not a second cap sentence: that one says what was **drawn**,
 * this one says what was **read**. Every figure comes from `/audit/source`; a fact the server could
 * not read is left out rather than replaced by a zero or an invented moment. `null` means there is
 * nothing the page can honestly say — the route did not answer, or the file could not be read — and
 * the page draws nothing.
 *
 * `source.bytes` of zero is a readable empty file and is stated; a missing size is absent from the
 * line entirely. The rotated clause is present only when the server proved a predecessor exists;
 * its date is present only when that file's first line could be read.
 */
export function auditSourceSentence(
  source: AuditSource | null | undefined
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
  return `${head}${tail === "" ? "" : `; ${tail}`}.`;
}
