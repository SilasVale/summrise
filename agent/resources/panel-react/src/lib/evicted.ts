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

/** Read one `session-evicted` frame. `null` for anything this build cannot describe. */
export function parseEvicted(detail: unknown): EvictionNotice | null {
  const d = (detail ?? {}) as Record<string, unknown>;
  if (d.ev !== "session-evicted") return null;
  const cause = d.cause === "idle" || d.cause === "cap" ? d.cause : null;
  if (!cause) return null;
  const raw = Array.isArray(d.sessions) ? d.sessions : [];
  const sessions: EvictedSession[] = [];
  for (const s of raw) {
    const r = (s ?? {}) as Record<string, unknown>;
    if (typeof r.id !== "string" || !r.id) continue;
    sessions.push({
      id: r.id,
      label: typeof r.label === "string" ? r.label : "",
      kind: typeof r.kind === "string" ? r.kind : "",
      idleMs: typeof r.idle_ms === "number" ? r.idle_ms : 0,
      reason: typeof r.reason === "string" ? r.reason : "",
    });
  }
  if (sessions.length === 0) return null;
  return { cause, limit: typeof d.limit === "number" ? d.limit : 0, sessions };
}

/** How long a silence lasts — ONE OWNER now (`lib/duration.ts`). This file kept a private copy that
 *  emitted `1h04m` where the panel writes `1h 04m`, while the doc below claimed the opposite. The
 *  import is what `evictedText` calls; the re-export keeps this module's public surface. */
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
