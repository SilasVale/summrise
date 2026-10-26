/** The first twelve characters of an id, with an ellipsis only when something was cut. */
export function shortId(id: string): string {
  return id.length > 12 ? id.slice(0, 12) + "…" : id;
}

/** A failed call's sentence: an Error's message, or the value as a string. */
export function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
