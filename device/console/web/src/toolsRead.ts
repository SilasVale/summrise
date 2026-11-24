import type { LiveToolsState } from "./useLiveTools";

/**
 * The tools read has three states, and every surface that draws a count or a list branches on this
 * instead of on `tools.length`.
 *
 * A list in hand is `ready` even while a refresh is in flight: the numbers on screen are a fact the
 * console read. An empty list is `reading` until the read answers, `failed` when it did not, and
 * `ready` when it answered with no tools — that last zero is the machine's own truth, not a
 * placeholder. A zero must never be drawn from a read that has not answered.
 */
export type ToolsReadState = "reading" | "failed" | "ready";

export function toolsReadState(
  live: Pick<LiveToolsState, "tools" | "loading" | "error">
): ToolsReadState {
  if (live.tools.length > 0) return "ready";
  if (live.loading) return "reading";
  return live.error === null ? "ready" : "failed";
}
