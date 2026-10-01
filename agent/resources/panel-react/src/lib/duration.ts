/**
 * HOW LONG A SILENCE LASTED — one owner, because two files each kept a private copy and both had
 * drifted from the shape the rest of the panel uses.
 *
 * WHAT WAS WRONG (found 2026-09-24 by the panel exploration's duration-family sweep). `evicted.ts` and
 * `idleSessions.ts` each declared their own `humanIdle`, and:
 *
 *   - both emitted `1h04m` where `useAttention` and `useMonitors` emit `1h 04m` — the same fact, two
 *     spellings, on surfaces an operator reads side by side;
 *   - they disagreed with EACH OTHER below a minute: `evicted.ts` said `45s`, `idleSessions.ts` said
 *     `0m`. The second is not a style difference, it is a false statement about a 45-second silence,
 *     and it was reached by the branch order rather than by a decision.
 *
 * `evicted.ts`'s doc comment claimed the copy was "in the words the rest of the panel uses", which is
 * exactly the claim it could not keep. The shape below is the panel's dominant one, with the
 * seconds branch kept: under a minute, seconds are the useful unit and `0m` is a lie.
 *
 * THE WIDER FAMILY IS STILL WIDER, deliberately not touched here: `fmtDuration`, `fmtSince`, `humanMs`,
 * `fmtUptime` (twice) and `fmtSpan` format DIFFERENT spans for different surfaces (a command's runtime,
 * a monitor's uptime, a trajectory's span), and four of them claim authority over the same shapes. This
 * module takes the pair that had actually drifted; unifying the rest is a separate, larger pass.
 */
// ── RUST SINCE 2026-09-30 (block ②), WITH THE IDLE FAMILY ───────────────────────────────────────
//
// `humanIdle` is `agent/resources/panel-logic/src/idle.rs` now, together with `idleSessions`,
// `idleOfferText` and `pruneSessionViews` — one module, because they answer one question: which
// sessions nobody is using, and which views are still worth keeping.
//
// THE IDENTITY THE TEST ASSERTS STILL HOLDS: `evicted.ts` and `idleSessions.ts` re-export THIS
// function object, and `duration.test.ts` pins that they are the same reference — which is what stops
// the two drifted copies coming back.
import { logic } from "../wasm/panelLogic";

export function humanIdle(ms: number): string {
  return logic().human_idle(ms) as string;
}
