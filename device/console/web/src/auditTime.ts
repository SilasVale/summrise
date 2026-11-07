/**
 * The Activity Table's time range: the presets, the custom fields, the sentence the table states,
 * and the fragment a chosen range lives in.
 *
 * The page used to narrow its 1,000-line window in the browser and fire nothing. A range is the
 * console's answer now (`GET /audit?from=&to=`), so its two moments are the request's identity and
 * they are the thing that can be linked. Everything here is pure: the page's clock is an argument,
 * and a local day is read with `Date`'s local methods, the same cut the browser draws.
 */

/** A half-open local interval, `[from, to)`, in Unix milliseconds. */
export type AuditRange = { from: number; to: number };

/** The presets the Time control offers. The words are the control's labels. */
export type AuditPreset = "hour" | "today" | "yesterday" | "week";

export const RANGE_PRESETS: readonly { value: AuditPreset; label: string }[] = [
  { value: "hour", label: "Last hour" },
  { value: "today", label: "Today" },
  { value: "yesterday", label: "Yesterday" },
  { value: "week", label: "Last week" }
];

/** What the range's answer proved about its own bounds. Every field is optional: absent means the
 * server did not say, and the sentence then says nothing about it. */
export type RangeFacts = {
  /** Where the current file's coverage starts, when it starts after `from`. */
  since_ms?: number | null;
  /** The backwards scan hit its line bound before reaching `from`. */
  bounded?: boolean;
  /** The bound's size, named when it was hit. */
  scan_limit?: number | null;
  /** The range held more matching lines than the page's limit; the answer is its newest prefix. */
  at_limit?: boolean;
};

/** Midnight at or before `now`, in the browser's own timezone. */
function startOfLocalDay(now: number): number {
  const date = new Date(now);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

/**
 * The preset's range at `now`.
 *
 * `Today` and `Yesterday` are local calendar days, because the page's day labels are; `Last hour`
 * and `Last week` are rolling windows, because their words are. An hour is 3,600,000 ms of instant,
 * not a slice of one calendar day: at 00:30 it starts at 23:30 yesterday and keeps the previous
 * day's tail. `Yesterday` ends at today's midnight: its last moment is the one before it, which
 * `[from, to)` already means.
 */
export function presetRange(preset: AuditPreset, now = Date.now()): AuditRange {
  switch (preset) {
    case "hour":
      return { from: now - 60 * 60_000, to: now };
    case "today":
      return { from: startOfLocalDay(now), to: now };
    case "yesterday": {
      const today = startOfLocalDay(now);
      const yesterday = new Date(today);
      yesterday.setDate(yesterday.getDate() - 1);
      return { from: yesterday.getTime(), to: today };
    }
    case "week":
      return { from: now - 7 * 24 * 60 * 60_000, to: now };
  }
}

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

/** `HH:MM` in the browser's timezone. */
function clock(ms: number): string {
  const at = new Date(ms);
  return `${pad(at.getHours())}:${pad(at.getMinutes())}`;
}

/** `MM-DD` in the browser's timezone. */
function monthDay(ms: number): string {
  const at = new Date(ms);
  return `${pad(at.getMonth() + 1)}-${pad(at.getDate())}`;
}

/** `MM-DD HH:MM`, the shape the `since` clause reads. */
function monthDayTime(ms: number): string {
  return `${monthDay(ms)} ${clock(ms)}`;
}

function sameLocalDay(left: number, right: number): boolean {
  const first = new Date(left);
  const second = new Date(right);
  return (
    first.getFullYear() === second.getFullYear() &&
    first.getMonth() === second.getMonth() &&
    first.getDate() === second.getDate()
  );
}

/**
 * The range in the sentence's words: `09:00–17:00 of 11-05`, or both ends when it crosses a day.
 *
 * The end is `to`, not the last millisecond it holds: a range is `[from, to)`, and 17:00 is how a
 * person names the boundary they chose.
 */
export function rangeText(range: AuditRange): string {
  if (sameLocalDay(range.from, range.to)) {
    return `${clock(range.from)}–${clock(range.to)} of ${monthDay(range.from)}`;
  }
  return `${monthDay(range.from)} ${clock(range.from)} – ${monthDay(range.to)} ${clock(range.to)}`;
}

/** `1,000`: the comma a person reads a count with, without the host's locale deciding it. */
function grouped(count: number): string {
  return String(count).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

function lines(count: number): string {
  return `${grouped(count)} ${count === 1 ? "line" : "lines"}`;
}

/**
 * The one sentence a range draws: the range, how many lines the answer held, and the two honest
 * bounds the server reported — where its coverage starts, and where its scan bound stopped it.
 *
 * The measured shape is `Showing 09:00–17:00 of 11-05, 342 lines`. A range the page's 1,000-line
 * limit cut says so instead of presenting the newest prefix as the whole range. The clauses are
 * appended rather than replacing the range: an operator reading `since 10-22 10:55` still needs to
 * know which range it is a fact about.
 */
export function rangeSentence(
  range: AuditRange,
  count: number,
  facts: RangeFacts | null = null
): string {
  const atLimit = facts?.at_limit === true;
  const head = atLimit
    ? `Showing the newest ${lines(count)} of ${rangeText(range)} — the window's limit`
    : `Showing ${rangeText(range)}, ${lines(count)}`;
  const bounds: string[] = [];
  if (facts?.since_ms != null) bounds.push(`since ${monthDayTime(facts.since_ms)}`);
  if (facts?.bounded === true) {
    bounds.push(`the ${grouped(facts.scan_limit ?? 50_000)}-line scan bound cut it`);
  }
  return bounds.length === 0 ? head : `${head}; ${bounds.join("; ")}`;
}

/**
 * A moment as a `datetime-local` input's value: local, to the minute, no zone suffix.
 *
 * The input carries minutes, so a range picked through it is minute-aligned by construction; the
 * presets keep their milliseconds in the fragment.
 */
export function dateTimeInputValue(ms: number): string {
  const at = new Date(ms);
  return (
    `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}` +
    `T${pad(at.getHours())}:${pad(at.getMinutes())}`
  );
}

/**
 * Read a `datetime-local` value as a local moment, or `null` when it names none.
 *
 * `new Date(2026, 12, 5)` silently becomes January 2027, so the fields are compared after
 * construction: an impossible day is not a moment the page may send to the console.
 */
export function parseDateTimeInput(value: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/.exec(value);
  if (!match) return null;
  const [, year, month, day, hour, minute, second] = match;
  const seconds = Number(second ?? "0");
  const at = new Date(
    Number(year),
    Number(month) - 1,
    Number(day),
    Number(hour),
    Number(minute),
    seconds,
    0
  );
  if (
    at.getFullYear() !== Number(year) ||
    at.getMonth() !== Number(month) - 1 ||
    at.getDate() !== Number(day) ||
    at.getHours() !== Number(hour) ||
    at.getMinutes() !== Number(minute) ||
    at.getSeconds() !== seconds
  ) {
    return null;
  }
  return at.getTime();
}

/** The query part of an Activity range fragment: exact milliseconds, so the link is the request. */
export function rangeQuery(range: AuditRange): string {
  return `from=${Math.trunc(range.from)}&to=${Math.trunc(range.to)}`;
}

/**
 * Read the query of `#activity?from=&to=`, or nothing when it is not one range.
 *
 * Both moments must be decimal milliseconds and the start must be before the end; a fragment the
 * page cannot turn into exactly one request is not a link it may restore.
 */
export function parseRangeQuery(query: string): AuditRange | null {
  const params = new URLSearchParams(query);
  const from = params.get("from");
  const to = params.get("to");
  if (from === null || to === null) return null;
  if (!/^\d+$/.test(from) || !/^\d+$/.test(to)) return null;
  const fromMs = Number(from);
  const toMs = Number(to);
  if (!Number.isSafeInteger(fromMs) || !Number.isSafeInteger(toMs)) return null;
  if (fromMs >= toMs) return null;
  return { from: fromMs, to: toMs };
}
