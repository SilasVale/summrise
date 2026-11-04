/**
 * "42s", "3m 05s", "2h 10m", "3d 4h" — an age, not a timestamp.
 *
 * A history row and a session list are read as "how long ago", and the exact clock time of something
 * an hour old is noise. The scale is shared so the two pages cannot drift apart.
 */
export function age(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${String(seconds % 60).padStart(2, "0")}s`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ${String(minutes % 60).padStart(2, "0")}m`;
  const days = Math.floor(hours / 24);
  return `${days}d ${hours % 24}h`;
}

/**
 * One unit, no remainder: "12s", "5m", "2h", "3d".
 *
 * A tab is a few characters wide and its label sits beside the session id, so the shared `age`
 * spelling ("3m 05s") is more than the strip can carry. The unit is rounded down: a tab that has
 * lived 59 seconds has not lived a minute yet.
 */
export function compactAge(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}
