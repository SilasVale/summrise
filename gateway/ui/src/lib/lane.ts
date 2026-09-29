// lane.ts — A CHANNEL PREFIX'S LANE COLOUR, as a TABLE rather than a ladder (round 140 of the standing goal).
//
// The mapping lived in `Models.tsx` as eight `if (p === "…") return "lane-…";` lines and a fallback, under a docstring saying
// it was "the same mapping the Routes page uses" — and there is NO second copy in this console, so the sentence described a
// consumer that does not exist. Both are the spine's subject one layer out: a mapping is DATA, and written as control flow
// it hides two things — which prefixes have a lane at all, and that anything else falls silently to `lane-def`.
//
// WHAT IT DOES NOT DO: decide which prefixes exist. That is the gateway's payload (`or/` and the rest arrive from
// `/api/admin/public`), so an unknown prefix lands on `lane-def` on purpose — the table just makes that visible, and the
// sheet's rules are the other half of the same list (see the follow-up recorded in the ledger: a clause that compares them).
//
// ── RUST SINCE 2026-09-29 (block ③), AND IT IS THE CONSOLE'S FIRST WIRED MODULE ──────────────────
//
// Both functions are `gateway/ui-logic/src/lib.rs`, transliterated — the same table, the same wider
// trailing-slash rule, the same `lane-def` fallback for a prefix the gateway's payload does not name.
// **THE SIGNATURES DID NOT CHANGE**, which is what let them move: `Models.tsx` calls both DURING
// RENDER, and until the module was fetched-and-compiled while the bundle downloads and awaited before
// the first render (`wasm/consoleLogic.ts`) a synchronous call from a component was impossible. The
// table that used to sit here is GONE — there is no second copy of it, or of either rule, left in
// this console.
import { logic } from "../wasm/consoleLogic.ts";

/** THE BARE PREFIX (`or/` and `or` are one channel) — ONE definition, eight call sites (round 172).
 *
 *  The rule was written out at eight places in `Models.tsx` and here, and with TWO different regexes: `/\/$/` strips one
 *  trailing slash and `/\/+$/` strips all of them, so two of the eight already disagreed about what a prefix is. This is the
 *  spine's first clause in its smallest form, and the fix is the one this repository uses everywhere: name it once.
 *
 *  It strips ALL trailing slashes, which is the wider of the two behaviours and the one a name wants — a prefix is a name and
 *  `/` is the separator, so `or//` and `or/` and `or` are the same channel. */
export function barePrefix(prefix: string): string {
  return logic().bare_prefix(prefix);
}

/** The lane class for a channel prefix, with its trailing slash ignored (`or/` and `or` are one channel). */
export function laneClass(prefix: string): string {
  return logic().lane_class(prefix);
}
