import { useMemo } from "react";
import type { CommandEvent } from "./useCommandEvents";

// Trajectory (round-admin-ui Task 5): the RAW audit event timeline for a
// session, grouped into rounds (one per command/start). This is a raw view
// distinct from the grouped command cards: status events like "opened" /
// "backgrounded" stay visible as their own rows, and a round's end state
// derives from the same round-99/100 terminal markers the cards use
// (command/end, or a status of backgrounded / closed / exited:N). Events
// before the first command/start (session-level statuses) form the preamble
// round.

export interface TrajRound {
  /** `r-<start seq>`; `r-pre` for the preamble (no command/start). */
  id: string;
  /** seq of the round's command/start; null for the preamble round. */
  startSeq: number | null;
  /** The round's command; "(session)" for the preamble. */
  command: string;
  /** Unix seconds of the round's first event. */
  startTs: number;
  /** All events in the round, in seq order (includes the command/start). */
  events: CommandEvent[];
  ended: boolean;
  exitCode: number | null;
  reason: string | null;
  durationMs: number | null;
}

// ── RUST SINCE 2026-09-29 (P2) ──────────────────────────────────────────────────────────────────────
//
// `groupRounds` is `panel-logic/src/events.rs` now, and `derivePath` — which reads these rounds — is
// `panel-logic/src/path.rs`, so the session's timeline is ONE derivation in ONE language: events in,
// rounds and a path out, with no JavaScript object in the middle. The function is `sync` because the
// module is loaded before the first render; see `wasm/panelLogic.ts` for the measurement.
//
// THE RULE IS UNCHANGED AND IS THE POINT: a `command/start` opens a round, the next one opens the
// next; a round is ENDED by a `command/end` OR by a terminal status, and THE LAST MARKER WINS — a
// backgrounded command can later log `closed`. A superseded round is sealed AS-IS (the raw view:
// what the log says), which is where this deliberately disagrees with `groupEvents` below's cards.
import { logic } from "../wasm/panelLogic";

export function groupRounds(events: CommandEvent[]): TrajRound[] {
  return logic().group_rounds(events) as TrajRound[];
}

/** Group raw audit events into trajectory rounds. round-128: the caller
 *  passes the events from the SHARED command-event poll (App-level) so the
 *  trajectory tab does not run a second full-log fetch every 2s. */
export function useTrajectory(events: CommandEvent[]): TrajRound[] {
  return useMemo(() => groupRounds(events), [events]);
}
