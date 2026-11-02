/** The first twelve characters of an id, with an ellipsis only when something was cut. */
export function shortId(id: string): string {
  return id.length > 12 ? id.slice(0, 12) + "…" : id;
}

/** A failed call's sentence: an Error's message, or the value as a string. */
export function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/** How long ago a device was last seen, in the fleet's one vocabulary. */
export function age(ms: number | null): string {
  if (ms === null) return "never";
  const seconds = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  return `${Math.round(minutes / 60)}h ago`;
}
