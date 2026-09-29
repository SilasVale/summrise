import { WORKING_MS } from "../hooks/useDeviceActivity";
import { logic } from "../wasm/panelLogic";
// liveness.ts — ONE state per entity, and ONE SHAPE per state.
//
// ── THE PREDICATES ARE RUST SINCE 2026-09-29 (P2), AND THEY ARE THE LARGEST FAMILY SO FAR ─────────
//
// The seven functions below are `agent/resources/panel-logic/src/liveness.rs`, transliterated — the
// same truthiness tests, the same `typeof idleMs === "number"`, the same composition, and the same
// two refusals the differential found (`null` raises there as it raises here, and `pendingCount > 0`
// coerces through the engine's ToNumber). **THEIR SIGNATURES DID NOT CHANGE**, which is what let the
// family move at all: every one of them is called DURING RENDER, and until the module was preloaded
// and awaited before the first render a synchronous call from a component was impossible. The panel's
// own `liveness.test.ts` runs UNCHANGED against the crate — that is the parity evidence.
//
// WHAT DID NOT MOVE: `URGENCY`, `SILHOUETTE` and `MOVES` below, and the reason is measured rather
// than assumed — **no product code reads them.** They are the shape vocabulary that the test pins
// ("no two states share a silhouette", "only `working` moves", the urgency order), and a table only a
// test reads would be bytes every operator downloads to answer a question no operator asks.
// `WORKING_MS` also stays: it belongs to `useDeviceActivity`, and it is PASSED INTO the crate rather
// than restated there, so the window still has exactly one definition.
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
// THE VOCABULARY IS ONLY WHAT THE DEVICE CAN ACTUALLY REPORT. Five states, because five are
// derivable: a transport that is down, a question holding, activity, FAILURE, and neither.
//
// THIS SAID FOUR UNTIL IT WAS CORRECTED, and the correction is the principle working rather than
// bending: the paragraph used to end "'Failed' is not here because no field reports it per session —
// inventing a state would put a shape on the screen that nothing can ever mean". That was true when
// it was written. Round 96 made it false — the device learned to report `last_exit_code`
// (TermSessionInfo, tools/terminal/mod.rs) — and round 97 added the state and argued its position in
// the urgency order below. A vocabulary that grows only when the device can feed it, and says so
// when it does, is the whole point of this file.

/** What a mark can say about an entity's LIVENESS. Urgency, not category. */
export type Liveness = "off" | "waiting" | "working" | "failed" | "idle";

/** The silhouette a state draws — the channel that survives colour loss. */
type Silhouette =
  "diamond" | "solid-halo" | "ring" | "dashed-ring" | "triangle";

/** Ordered by URGENCY, and the order is the contract: a mark may only be louder than another if it
 *  outranks it. `waiting` beats `working` because a question DECAYS if it is not seen, while work
 *  continues; `off` is quietest because it says nothing can be done either way.
 *
 *  `failed` SITS BETWEEN ACTIVITY AND QUIET, and the position is the argument (round 97). It is a fact about
 *  what ALREADY HAPPENED, so anything happening NOW outranks it — a session that is working or holding a
 *  question is not described by its last exit code. It outranks `idle` because "quiet, and the last thing here
 *  broke" is more than "quiet". And it LINGERS by design: the device clears the code when the next command is
 *  written, so the state ends when the session does something else, not on a timer this surface invents. */
export const URGENCY: Record<Liveness, number> = {
  waiting: 4,
  working: 3,
  failed: 2,
  idle: 1,
  off: 0,
};

/** One shape per state, no two alike. Pinned by liveness.test.ts, because "shape carries the state"
 *  is worthless if two states share a shape. */
export const SILHOUETTE: Record<Liveness, Silhouette> = {
  waiting: "diamond",
  working: "solid-halo",
  failed: "triangle",
  idle: "ring",
  off: "dashed-ring",
};

/** Motion is an ADDITION, never the message: only `working` moves, and it still reads as a solid
 *  mark with a halo when motion is off. */
export const MOVES: Record<Liveness, boolean> = {
  waiting: false,
  working: true,
  failed: false,
  idle: false,
  off: false,
};

/**
 * THE PRECEDENCE, in one place. Every surface that shows liveness calls this rather than writing its
 * own ternary — the rail's inline `!connected ? "off" : waiting ? … ` was the whole model until now,
 * and a second copy of it is how two surfaces come to disagree about the same session.
 *
 * `reachable` is about the TRANSPORT, not the entity: a session on a dead connection cannot be
 * answered even if a question is outstanding, so it is `off` and the mark must not claim otherwise.
 */
export function livenessOf(input: {
  reachable: boolean;
  pending: boolean;
  active: boolean;
  failed?: boolean;
}): Liveness {
  // RUST SINCE 2026-09-29 (P2): `panel-logic/src/liveness.rs`. THE SIGNATURE IS UNCHANGED, which is
  // what let this family move at all — every one of these is called DURING RENDER, and until the
  // module was preloaded and awaited before the first render (`wasm/panelLogic.ts`) a synchronous
  // call from a component was impossible. The body is one call; there is no second copy of the
  // precedence left in the panel.
  return logic().liveness_of(input) as Liveness;
}

/** The device as a whole: reachable, holding questions, or busy.
 *
 *  NO `failed` HERE, AND THAT IS A DECISION RATHER THAN AN OMISSION (round 97). A device-level failure would have
 *  to pick WHICH session's last command to blame and say nothing about which — and the rail has no room to name it.
 *  Worse, the rail is the one mark that is always on screen: an AI runs commands continuously and plenty of them
 *  exit non-zero for ordinary reasons (a `grep` with no match), so a device-wide triangle would be a light that is
 *  on most of the time and therefore means nothing. The failure belongs on the SESSION's mark, where the operator
 *  can see which session it is, and the rail keeps answering the question it was built for: is this machine doing
 *  something, and does anything want me. */
export function deviceLiveness(input: {
  connected: boolean;
  pendingCount: number;
  working: boolean;
}): Liveness {
  // RUST SINCE 2026-09-29: `device_liveness` composes `liveness_of` in the crate, so the two cannot
  // disagree about the precedence. `NO failed HERE` above is the decision; it is unchanged.
  return logic().device_liveness(input) as Liveness;
}

/** A session. `reachable` follows the device because a session lives on it; a CLOSED session is not
 *  liveness at all, so it degrades to `off` rather than pretending to be idle. */
/**
 * CAN THIS SESSION STILL ANSWER — the ONE predicate the mark, the tab's title and its aria-label all read.
 *
 * WHY IT IS A FUNCTION (round 33 of the standing goal). Three surfaces derived it three ways:
 *
 *     TabBar          !s.closed && !!s.pendingApproval
 *     DesktopShell    !!s.pendingApproval                        <- no closed check
 *     sessionLiveness !!s.pendingApproval, with `reachable` carrying `!closed`
 *
 * A CLOSED session's row keeps its data — the tombstone is the same record — so a question that expired with the
 * session it belonged to survived in `pendingApproval`. The mark got it right (unreachable outranks pending, so a
 * closed session is `off`), and the DESKTOP tab's title said "waiting for your approval" about a tab that cannot be
 * answered at all. The two densities disagreed about one session, which is the failure this model exists to stop.
 */
export function sessionWaiting(session: {
  pendingApproval?: unknown;
  closed?: boolean;
  commandRunning?: boolean;
}): boolean {
  return logic().session_waiting(session);
}

/**
 * IS THE **SESSION** WORKING — not "is the device busy".
 *
 * THE FACT WAS ON THE WIRE AND UNUSED. `terminal_list` reports `idle_ms` per session (the agent's own
 * `last_output.elapsed()`, round 37), the panel types it as `idleMs` — and every surface passed `active: false` to
 * `livenessOf`, with a comment explaining that the only activity signal was DEVICE-wide and a halo on all sixteen
 * tabs would say nothing. That was true of the signal being used and false of the one available: the device has
 * known, per session and all along, when that session last produced output.
 *
 * IT IS A RECENCY SIGNAL, NOT "A COMMAND IS RUNNING", and the distinction is the same one `useDeviceActivity`
 * documents: a long command that prints nothing for a while will read as idle here. The panel says "produced
 * output recently" because that is what it can honestly say.
 *
 * THE WINDOW IS `WORKING_MS`, the same number the device-wide signal uses — imported rather than restated, so the
 * two cannot drift into disagreeing about what "recently" means.
 */
export function sessionActive(session: {
  idleMs?: number;
  commandRunning?: boolean;
}): boolean {
  // THE WINDOW IS PASSED IN, not restated: it belongs to `useDeviceActivity` (the device-wide
  // recency signal), and a constant copied into the crate would be the second copy that file's own
  // comment exists to prevent. The RULE that reads it is `session_active` in `liveness.rs`.
  return logic().session_active(session, WORKING_MS);
}

/**
 * IS ANY SESSION HOLDING A COMMAND IN FLIGHT — the device's own answer, for the DEVICE-level mark.
 *
 * WHY IT IS NOT `useDeviceActivity`. That hook is an EVENT-RECENCY signal and says so: "some activity arrived
 * within the last WORKING_MS". The rail's mark used it alone, so a command that runs silently for a minute read as
 * IDLE there — the same blindness the per-session marks had until the device started reporting `command_running`
 * (round 28). This is that fact, at device scope: one derivation, next to the per-session one it belongs with.
 */
export function anyCommandRunning(
  sessions: Array<{ commandRunning?: boolean }> | undefined,
): boolean {
  // `!!sessions?.some(…)` is the Rust's own optional-chaining arm: an absent list is `false`, and
  // anything that is not a list is a THROW there as it is here (`{}.some` is not a function).
  return logic().any_command_running(sessions);
}

/**
 * ONE DERIVATION FOR EVERY SURFACE. The tab strip, the context list and the desktop strip each used to build this
 * inline — three copies of the same three lines, two of which passed `active: false` — which is how they come to
 * disagree. Nothing about a session's own mark needs the device: connectivity is the RAIL's fact and lives on the
 * rail's mark, so a disconnected device does not make every session "off" (which is what a CLOSED session means).
 */
export function sessionLiveness(session: {
  pendingApproval?: unknown;
  closed?: boolean;
  idleMs?: number;
  commandRunning?: boolean;
  lastExitCode?: number | null;
}): Liveness {
  // ONE CALL, and the four inputs are composed in the crate: a caller cannot re-derive one of them
  // here and disagree with the mark.
  return logic().session_liveness(session, WORKING_MS) as Liveness;
}

/** DID THIS SESSION'S LAST COMMAND FAIL — the device's own exit code, and nothing else (round 97).
 *
 *  ONE PREDICATE, because three surfaces ask the question (the tab's mark, the row's mark, the desktop tab's) and
 *  a second `!== 0` written somewhere else is how two of them come to disagree.
 *
 *  ABSENT IS NOT FAILURE and not success: `lastExitCode` is `null` when the device observed no code at all — no
 *  command yet, a wait that ended without a shell marker, or an ssh/serial session with no marker injection. That
 *  is the third state, and a mark that turned it into either answer would be inventing one.
 *
 *  NON-ZERO IS A FAILURE, with no judgement about WHICH codes deserve it: the panel's command cards have called
 *  every non-zero exit "Failed (exit N)" since they existed (`cardState`), and a surface that decided a 1 from a
 *  `grep` was not worth mentioning would be making a claim the device never made. */
export function sessionFailed(session: {
  lastExitCode?: number | null;
}): boolean {
  return logic().session_failed(session);
}
