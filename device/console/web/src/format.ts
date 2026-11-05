export { shortId, age, ageAt } from "./logic";

/**
 * The fleet's one sentence about a machine's day: `283 calls · 38 failed`.
 *
 * The failed half is omitted at zero, so a healthy machine reads as its work and not as a
 * reassurance; a machine the audit window holds no line for has no day at all, and returns null so
 * the row draws nothing rather than a zero. The counts are the window's, and the hub counts the same
 * lines by the same rule.
 */
export function callsLine(
  day: { client_calls: number; failed: number } | null | undefined
): string | null {
  if (!day) return null;
  const calls = `${day.client_calls} ${day.client_calls === 1 ? "call" : "calls"}`;
  return day.failed > 0 ? `${calls} · ${day.failed} failed` : calls;
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
