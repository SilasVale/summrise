// Address-bar merge rule for embedded-browser nav pushes.
//
// Focus-trap fix: the pane drops nav pushes while the address bar is
// focused (so typing is never clobbered). But the focus flag is cleared
// only by DOM blur — clicking into the NATIVE view (a separate OS window
// over the SPA) never fires blur, so the flag sticks forever and every
// later push is dropped: the bar freezes until the pane remounts.
// Rule: follow the push unless the user has UNSENT edits (focused AND the
// value differs both from what it was at focus time and from the last
// pushed URL).
interface NavPushMerge {
  editing: boolean;
  inputValue: string;
  valueAtFocus: string;
  lastPushedUrl: string;
}

// ── RUST SINCE 2026-09-30 (block ②) ─────────────────────────────────────────────────────────────
//
// The rule is `agent/resources/panel-logic/src/nav.rs` now; the differential is 1,568 corpus cases with
// 0 divergences and both arms reached, and the interface below is what the caller passes.
import { logic } from "../wasm/panelLogic";

export function shouldAcceptNavPush(m: NavPushMerge): boolean {
  return logic().should_accept_nav_push(m) as boolean;
}
