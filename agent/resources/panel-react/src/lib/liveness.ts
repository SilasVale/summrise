// liveness.ts — ONE state per entity, and ONE SHAPE per state.
//
// WHY THIS EXISTS. The panel said "how is this thing doing" in four places, each with its own
// vocabulary and its own arithmetic:
//
//   the rail's dot     data-state  off · waiting · working · idle   — an inline ternary in IconRail
//   the tab's dot      data-kind   pty · ssh · serial               — the transport lane
//   the tab's wait mark .tab-wait  a diamond, rendered BESIDE the dot — a second element, in the row
//   the session card   data-state  ai · human                       — who holds the keyboard
//
// Two of those answer different questions and are right to. The problem is that NOTHING WAS SHARED.
// The precedence rule (a question outranks activity, activity outranks quiet) lived in one component
// as a ternary; the diamond meant "waiting" in the strip and "waiting" again in the rail at a
// different size; and a session that was waiting drew TWO marks in the strip, because the lane dot
// had no way to say it. Sixteen tabs, and "which one is holding a question" was readable only from
// an aria-label.
//
// SO: one function decides liveness, one table names the shapes, and the silhouette carries the
// state while colour rides along as the second channel. That order is the point — it is what
// survives greyscale, colour-vision deficiencies, and `prefers-reduced-motion`, where an animation
// channel disappears entirely.
//
// THE VOCABULARY IS ONLY WHAT THE DEVICE CAN ACTUALLY REPORT. Four states, because four are
// derivable: a transport that is down, a question holding, activity, and neither. "Failed" is not
// here because no field reports it per session — inventing a state would put a shape on the screen
// that nothing can ever mean.

/** What a mark can say about an entity's LIVENESS. Urgency, not category. */
export type Liveness = "off" | "waiting" | "working" | "idle";

/** The silhouette a state draws — the channel that survives colour loss. */
export type Silhouette = "diamond" | "solid-halo" | "ring" | "dashed-ring";

/** Ordered by URGENCY, and the order is the contract: a mark may only be louder than another if it
 *  outranks it. `waiting` beats `working` because a question DECAYS if it is not seen, while work
 *  continues; `off` is quietest because it says nothing can be done either way. */
export const URGENCY: Record<Liveness, number> = { waiting: 3, working: 2, idle: 1, off: 0 };

/** One shape per state, no two alike. Pinned by liveness.test.ts, because "shape carries the state"
 *  is worthless if two states share a shape. */
export const SILHOUETTE: Record<Liveness, Silhouette> = {
  waiting: "diamond",
  working: "solid-halo",
  idle: "ring",
  off: "dashed-ring",
};

/** Motion is an ADDITION, never the message: only `working` moves, and it still reads as a solid
 *  mark with a halo when motion is off. */
export const MOVES: Record<Liveness, boolean> = { waiting: false, working: true, idle: false, off: false };

/**
 * THE PRECEDENCE, in one place. Every surface that shows liveness calls this rather than writing its
 * own ternary — the rail's inline `!connected ? "off" : waiting ? … ` was the whole model until now,
 * and a second copy of it is how two surfaces come to disagree about the same session.
 *
 * `reachable` is about the TRANSPORT, not the entity: a session on a dead connection cannot be
 * answered even if a question is outstanding, so it is `off` and the mark must not claim otherwise.
 */
export function livenessOf(input: { reachable: boolean; pending: boolean; active: boolean }): Liveness {
  if (!input.reachable) return "off";
  if (input.pending) return "waiting";
  if (input.active) return "working";
  return "idle";
}

/** The device as a whole: reachable, holding questions, or busy. */
export function deviceLiveness(input: { connected: boolean; pendingCount: number; working: boolean }): Liveness {
  return livenessOf({ reachable: input.connected, pending: input.pendingCount > 0, active: input.working });
}

/** A session. `reachable` follows the device because a session lives on it; a CLOSED session is not
 *  liveness at all, so it degrades to `off` rather than pretending to be idle. */
export function sessionLiveness(
  session: { pendingApproval: unknown; closed?: boolean },
  device: { connected: boolean; working: boolean },
): Liveness {
  return livenessOf({
    reachable: device.connected && !session.closed,
    pending: !!session.pendingApproval,
    active: device.working,
  });
}
