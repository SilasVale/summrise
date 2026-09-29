// A SESSION THE DEVICE TOOK AWAY, in the terms the operator needs.
//
// WHY THIS EXISTS (round 37). The device has a 16-session cap and a 15-minute idle sweeper, and it
// enforced both IN SILENCE: measured on the operator's own box, sixteen sessions sat there with
// several silent for ELEVEN HOURS (the sweeper had never run), and the next session opened would
// have closed one of their tabs with no message anywhere. The device now announces every eviction
// (`{"ev":"session-evicted", cause, limit, sessions:[…]}`) and this turns that frame into one line a
// person can read — WHAT went, and WHICH rule took it.
//
// Validated, not trusted: a frame this build cannot use is null, never a notice about something
// that did not happen (the same rule `parseMonitorChange` follows).

interface EvictedSession {
  id: string;
  label: string;
  kind: string;
  idleMs: number;
  reason: string;
}

export interface EvictionNotice {
  cause: "idle" | "cap";
  limit: number;
  sessions: EvictedSession[];
}

/** Read one `session-evicted` frame. `null` for anything this build cannot describe.
 *
 *  ── IT IS RUST NOW (P2, 2026-09-29), and this is the seam ────────────────────────────────────
 *
 *  The parse moved to `agent/resources/panel-logic/src/evicted.rs`, transliterated: the same strict
 *  `d.ev !== "session-evicted"`, the same two accepted causes, the same `!r.id` truthiness that
 *  skips an EMPTY id as well as a missing one, the same "no sessions left ⇒ null".
 *
 *  WHY IT IS `async`: this one is called from an EVENT HANDLER (`useEvictedNotice`'s listener), where
 *  nothing is on the first render's path — so the promise costs nothing. It HAD to be `async` when it
 *  moved, because the wasm was fetched at the first call then; **SUPERSEDED 2026-09-29: the module is
 *  preloaded and awaited before the first render (`wasm/panelLogic.ts`), so a render-path call is
 *  legal now.** `evictedText` below stays TypeScript because `EvictedNotice` calls it while RENDERING,
 *  and that is a decision this file has not revisited.
 *
 *  AND `idleMs` IS `typeof … === "number"`, NOT this crate's `num()`. The two differ on `NaN` and
 *  `Infinity`: `num` is for values plotted on an axis, where a non-finite number is not a reading,
 *  and this field is formatted into a sentence. The Rust file says the same thing at the same
 *  boundary, because that is where a port would have gone wrong silently. */
export async function parseEvicted(detail: unknown): Promise<EvictionNotice | null> {
  const logic = await panelLogic();
  return logic.parse_evicted(detail) as EvictionNotice | null;
}

/** How long a silence lasts — ONE OWNER now (`lib/duration.ts`). This file kept a private copy that
 *  emitted `1h04m` where the panel writes `1h 04m`, while the doc below claimed the opposite. The
 *  import is what `evictedText` calls; the re-export keeps this module's public surface. */
import { panelLogic } from "../wasm/panelLogic";
import { humanIdle } from "./duration";
export { humanIdle };

/** ONE LINE: what was closed, by which rule, after how long. */
export function evictedText(n: EvictionNotice): string {
  const names = n.sessions.map((s) => s.label || s.id).join(", ");
  const count =
    n.sessions.length === 1 ? "session" : `${n.sessions.length} sessions`;
  if (n.cause === "cap") {
    return `Closed ${names} — the ${n.limit}-session cap was reached${names.length ? "" : ""}.`;
  }
  const longest = Math.max(...n.sessions.map((s) => s.idleMs));
  return `Closed ${count} idle for ${humanIdle(longest)}: ${names} — silent past the ${Math.round(
    n.limit / 60,
  )}-minute limit.`;
}
