export { shortId, age, ageAt } from "./logic";

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

