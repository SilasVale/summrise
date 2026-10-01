import type { ReadState } from "./readState";

/**
 * WHAT AN EMPTY TRAIL IS ALLOWED TO SAY — ONE WORDING, THREE VIEWS.
 *
 * A view that draws its own "nothing here yet" line makes a CLAIM about the
 * device: that the session ran nothing. That claim is only available when a read
 * actually SUCCEEDED. `useCommandEvents` has reported `readState`
 * (`"reading" | "ok" | "unreadable"`) for longer than the Archive has used it —
 * but the Archive was its ONLY consumer, so the live trajectory and path views
 * printed "No commands in this session yet." and "This session has not run a
 * command." unconditionally. Two reachable windows, one of them on EVERY session
 * switch: `useCommandEvents` resets `events` to `[]` synchronously while the new
 * read is in flight, so the operator is told the session is empty for the whole
 * round trip.
 *
 * This is round 27's defect one field over: `firstSeq` was hoisted through `App`
 * for exactly this reason and `readState` was left behind in the same object
 * literal. The Archive's header states the rule — "a session whose trail cannot
 * be read SAYS SO, and never renders as an empty history" — and it was honoured
 * in one place out of three.
 *
 * The wording lives HERE rather than in each view because three copies of one
 * sentence is how this repo's views come to disagree about what they are saying.
 * The Archive's phrasing is the original and is preserved verbatim.
 *
 * Returns `null` when the caller's own empty state is TRUE and may be shown.
 */
// ── RUST SINCE 2026-09-30 (block ②) ─────────────────────────────────────────────────────────────
//
// The wording and the rule are `agent/resources/panel-logic/src/trail.rs` now; the differential is 14
// corpus cases with 0 divergences and every arm reached (null, the in-flight line, the failed line),
// with the object's key order compared too. `components/__tests__/TrailReadNotice.test.tsx` runs
// UNCHANGED.
//
// THE COMPARISONS ARE STRICT THERE, so a state this build has never heard of takes the IN-FLIGHT
// branch — the safe direction: an unknown state is not a licence to claim the session ran nothing.
import { logic } from "../wasm/panelLogic";

export function trailReadNotice(read: ReadState): { text: string; failed: boolean } | null {
  return logic().trail_read_notice(read) as { text: string; failed: boolean } | null;
}
