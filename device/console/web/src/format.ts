export { shortId, age, ageAt } from "./logic";

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

/** A clock a person reads on a summary line: HH:MM in the console's own timezone. */
export function clockTime(ts: number): string {
  const date = new Date(ts);
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/**
 * `2026-11-04`: the console's one absolute date, in the operator's own timezone.
 *
 * `toLocaleDateString()` put the browser's order in the token sheet — `11/4/2026` in en-US, and a
 * different order in every other locale, so a CN operator could not tell the day from the month
 * without knowing the browser's locale. The audit writes ISO, and this is the same day's shape.
 * The day is the console's local day, because the clock beside it (`last 17:23`) is; `exactMoment`
 * states the time behind it.
 */
export function isoDate(ts: number): string {
  const at = new Date(ts);
  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}`;
}

/**
 * The exact moment behind an `isoDate`, for a cell's `title`: the console's clock to the second,
 * with the full UTC ISO instant the audit's filenames use beside it.
 *
 * `20261102T113000.000Z` and the token sheet's day are the same moment stated two ways; a title
 * that names both lets an operator match a token to the export it appears in.
 */
export function exactMoment(ts: number): string {
  const at = new Date(ts);
  return (
    `${isoDate(ts)} ${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}` +
    ` (${at.toISOString()})`
  );
}

/**
 * The day's coverage, when the file cannot prove all of it.
 *
 * `since` is the local time the read starts at — a rotation, or a console that started after
 * midnight — and the line bound is the other way a read can be partial. Both are stated on the
 * sentence; a number that covers three hours of a day must not read as the day.
 */
export type DayCoverage = {
  since_ms?: number | null;
  bounded?: boolean;
  scan_limit?: number | null;
};

/** The one coverage clause a day sentence appends, or nothing for a whole day. */
export function coverageClause(day: DayCoverage): string {
  const parts: string[] = [];
  if (day.since_ms != null) parts.push(`since ${clockTime(day.since_ms)}`);
  if (day.bounded) {
    parts.push(
      day.scan_limit != null
        ? `first ${String(day.scan_limit).replace(/\B(?=(\d{3})+(?!\d))/g, ",")} lines`
        : "line bound hit"
    );
  }
  return parts.length === 0 ? "" : ` · ${parts.join(" · ")}`;
}

/**
 * The fleet's one sentence about a machine's day: `283 calls · 38 failed`.
 *
 * The failed half is omitted at zero, so a healthy machine reads as its work and not as a
 * reassurance; a machine the day's read names nowhere has no day at all, and returns null so the
 * row draws nothing rather than a zero. The counts are the day's, and the hub's day line reads the
 * same field by the same rule; when the file cannot prove the whole day, `since` (and a bounded
 * scan) is stated rather than swallowed.
 */
export function callsLine(
  day:
    | ({ client_calls: number; failed: number } & DayCoverage)
    | null
    | undefined
): string | null {
  if (!day) return null;
  const calls = `${day.client_calls} ${day.client_calls === 1 ? "call" : "calls"}`;
  return (day.failed > 0 ? `${calls} · ${day.failed} failed` : calls) + coverageClause(day);
}

/** A failed call's sentence: an Error's message, or the value as a string. */
export function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * How long ago a moment was, with the minutes that make two hours distinguishable from two hours
 * and one minute.
 *
 * The fleet's shared `age` rounds to one unit ("2h ago"), which is right for a row's glance and
 * wrong for the one place a machine's silence is the fact: DESKTOP-14RJCR8 was away 1 h 49 m and
 * read "2h ago", the same as a machine that dropped on the hour. Seconds stay seconds and minutes
 * under an hour stay minutes; from an hour on the minutes are stated, and a whole hour keeps its
 * shorter spelling.
 */
export function seenAgo(atMs: number, nowMs = Date.now()): string {
  const elapsed = Math.max(0, nowMs - atMs);
  const seconds = Math.round(elapsed / 1_000);
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(elapsed / 60_000);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours < 24) {
    return rest === 0 ? `${hours}h ago` : `${hours}h ${String(rest).padStart(2, "0")}m ago`;
  }
  const days = Math.floor(hours / 24);
  return `${days}d ${hours % 24}h ago`;
}

/**
 * The moment the console last heard a device, from the newest evidence it has.
 *
 * A frame is presence's own proof, and wins even when the channel closed later: the close is when
 * the console noticed, while the frame is when the machine last spoke. Without a frame (a console
 * that restarted mid-outage), the newest still-open `device.left` is the moment the silence started.
 * The registry's `last_seen_ms` is the last channel open — older than both, and the last resort
 * rather than a fact to invent with.
 */
export function lastHeardMs(device: {
  last_frame_ms?: number | null;
  last_left_ms?: number | null;
  last_seen_ms?: number | null;
}): number | null {
  return device.last_frame_ms ?? device.last_left_ms ?? device.last_seen_ms ?? null;
}

/**
 * The part of a machine's name that tells it apart, for the phone's 67 px audit cell.
 *
 * The measured fleet is `DESKTOP-9VH92BJ`, `DESKTOP-14RJCR8` and `DESKTOP-CPH4KB2`: the only shared
 * prefix is the vendor's, and the tail after it is the whole name a person uses. The prefix is
 * dropped only when it ends on a separator: `alpha1`/`alpha2` share `alpha` inside a word, and
 * shortening those to `1`/`2` would be a different name rather than the same name's tail. A name
 * with no shared prefix, or a fleet of one, stays whole.
 */
export function shortDeviceName(name: string, fleetNames: readonly string[]): string {
  const names = fleetNames.filter((entry) => entry !== "");
  if (names.length < 2 || name === "") return name;
  let prefix = names[0];
  for (const other of names.slice(1)) {
    let index = 0;
    while (index < prefix.length && index < other.length && prefix[index] === other[index]) {
      index += 1;
    }
    prefix = prefix.slice(0, index);
    if (prefix === "") return name;
  }
  const separator = prefix.length > 0 && !/[\p{L}\p{N}]$/u.test(prefix);
  if (!separator || !name.startsWith(prefix) || name.length <= prefix.length) return name;
  return name.slice(prefix.length);
}
