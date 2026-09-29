// lib/path.ts — derive the PATH of a session from its audit rounds.
//
// WHY THIS EXISTS (design: docs/adr/proposal-game-design.md §2.1, §7).
//
// The design's central claim is that Summrise records tool calls and the operator
// thinks in goals, and the layer between them is empty. This module builds the
// smallest honest piece of that layer: one session's work as a single scannable
// PATH with a summary, instead of a timeline the operator has to read and count.
//
// WHAT IT DELIBERATELY DOES NOT DO — and this is the important part.
//
// The prototype on branch `prototype/control-path` drew each step with the
// ALTERNATIVES that were legal at that moment ("ghost branches"). That
// information DOES NOT EXIST in the audit trail; it needs the control plane's
// gate records (proposal-control-path.md) and, for "considered and rejected",
// the intent layer. So this module produces no branches, and the view says so
// out loud rather than drawing an empty fork that would imply the data was
// merely unavailable right now. A path view that fakes branches would be worse
// than none: it would look like the product already knows what the AI chose
// between.
//
// It reuses `groupRounds` (hooks/useTrajectory) rather than re-segmenting the
// event stream: rounds are already the ONE definition of "one command", and a
// second definition here would drift from the trajectory view's.
//
// It also cannot say WHO ran a step. `SessionEvent` carries no actor field, and
// the panel's own keystrokes use the same terminal_write path the AI does — so
// "who" is unknown by construction, not by omission.
import type { CommandCard as CardData } from "../hooks/useCommandEvents";
import type { CommandEvent } from "../hooks/useCommandEvents";
import type { TrajRound } from "../hooks/useTrajectory";
import { logic } from "../wasm/panelLogic";

// ── RUST SINCE 2026-09-29 (P2), AND THIS FAMILY COULD NOT HAVE MOVED A DAY EARLIER ────────────────
//
// The five functions below are `agent/resources/panel-logic/src/path.rs`, transliterated: the same
// precedence in `stateFromEnd`, the same `exited:` prefix arm, the same ownership fold, the same
// `commandMs` FLOOR that `untimed` makes explicit, and the same `bg`-ranks-with-`running` rule in
// `attentionSteps`. **THEIR SIGNATURES DID NOT CHANGE**, which is the whole reason they could move
// without a reshape: `PathView.tsx` calls `derivePath` and `attentionSteps` inside a `useMemo`, and
// the old "fetched at the first call" seam made a render-path call impossible — the boundary road
// the plan priced for this family (derive at the data boundary) is no longer needed. The crate's
// header is the specification: the five things a reader would otherwise have to take on trust (UTF-16
// output length, the loose `durationMs == null` test, the at-or-before ownership fold, the total-order
// sort, and the vocabulary that now lives in `panel-logic/src/vocabulary.rs` beside the table).
//
// WHAT IS GONE: the whole body of each function, including the `END_STATE`/`END_LABEL` tables — the
// endings vocabulary moved into the crate with them, and `agent/tests/contract_vocabulary.rs` reads
// THAT table now. **NO SECOND DERIVATION IS LEFT IN THE PANEL.**
/** The five-state vocabulary, re-exported so the path view and the command
 *  cards cannot drift apart on what a state is called. The STATES are the
 *  crate's now; this list is the type they are spelled with. */
/** What `stateFromEnd` and `cardState` answer — named here because the glue's own types are the
 *  wasm module's (`{}` for an object return), and a cast needs a target. */
type StateRow = { state: PathState; label: string; compact: string };

export const PATH_STATES = ["running", "ok", "fail", "warn", "bg", "muted"] as const;

export type PathState = (typeof PATH_STATES)[number];


/** THE ONE DERIVATION OF A COMMAND'S STATE, from the three facts that decide it.
 *
 *  It lives here — not in a component — because three views read it: the command card, the details panel and the
 *  path summary, plus the trajectory's per-event dot. It used to be `cardState` in `CommandCard.tsx`, with
 *  `lib/path.ts` importing FROM a component (the only place this tree inverts its own layering), and `TrajectoryView`
 *  keeping a SECOND private copy that mapped `backgrounded` to `warn`. The cost was measurable: one backgrounded
 *  command wore `bg` in its round marker and `warn` in its own event row, in the same view, and `cardState`'s own
 *  comment records the round where that mapping was fixed for the cards and left standing for the events.
 *
 *  `reason` is the STATUS string the trail carries (`backgrounded`, `closed`, `interrupted`, `exited:3`), which is
 *  why feeding it through unchanged is what makes the two views agree. */
export function stateFromEnd(
  ended: boolean,
  exitCode: number | null,
  reason: string | null,
): { state: PathState; label: string; compact: string } {
  return logic().state_from_end(ended, exitCode, reason) as StateRow;
}

/** A command card's state — the same derivation, over the card's fields. */
export function cardState(card: CardData): { state: PathState; label: string; compact: string } {
  return logic().card_state(card) as StateRow;
}
type Owner = "ai" | "human";

export interface PathStep {
  /** The round id it came from (`r-<seq>`), so a step can be traced back. */
  id: string;
  /** 1-based position along the path. */
  index: number;
  command: string;
  state: PathState;
  /** WHO was driving when this step started, from the session's `control`
   *  events. Defaults to "ai", which is what the trail means before any
   *  handoff — an unflagged step is the agent's. */
  owner: Owner;
  /** Short label for the state, from cardState (e.g. "exit 1"). */
  stateLabel: string;
  /** Unix seconds. */
  startedAt: number;
  durationMs: number | null;
  exitCode: number | null;
  reason: string | null;
  /** Output character count — a cheap size signal without shipping the text. */
  outputChars: number;
  /** WHY the agent says it ran this. Null for every step logged before the
   *  intent surface existed, and for clients that do not send one — the view
   *  must render those two cases identically, because to a reader they are the
   *  same thing: no reason was given. */
  intent: string | null;
  /** The alternatives the agent says it passed over. The branches NOT taken —
   *  the one thing a command log can never reconstruct, and the reason this
   *  field exists at all. */
  considered: string[];
  /** The 1-based plan step this command claimed, or null if it claimed none.
   *  Null is the interesting case as much as a number: an unclaimed step is how
   *  a run visibly departs from what the agent said it would do. */
  planStep: number | null;
  /** The run this command claimed to belong to, or null. A LABEL, NEVER A
   *  CREDENTIAL — it is the AI's own attribution, recorded verbatim because the
   *  device does not verify it — so it is rendered as a claim and never used to
   *  GROUP anything. Grouping lives in `lib/runs.ts` on the device-level
   *  timeline; a second grouping here would be two implementations of one read. */
  runId: string | null;
}

export interface PathSummary {
  /** Command steps (the preamble round is not a step). */
  steps: number;
  counts: Record<PathState, number>;
  /** Sum of the KNOWN per-step durations. Not wall-clock: backgrounded and
   *  still-running steps have no duration, and two steps can overlap. */
  commandMs: number;
  /** Steps whose duration is unknown — so `commandMs` is a floor, and the view
   *  can say "at least" instead of implying a total it cannot know. */
  untimed: number;
  /** Steps a PERSON drove. Surfaced because an operator returning to a session
   *  needs to know which work was theirs and which was the agent's — the whole
   *  reason the handoff is recorded. */
  humanSteps: number;
  /** Wall-clock span from the first step's start to the last known end. Null
   *  when nothing has finished. */
  spanMs: number | null;
  /** True while at least one step is still running. */
  live: boolean;
}

export interface SessionPath {
  steps: PathStep[];
  summary: PathSummary;
  /** Round id → step index, for jumping from the path into the timeline. */
  indexOf: Record<string, number>;
}

/** A session-level status before any command (e.g. "opened") forms the
 *  preamble round; it is context, not a step along the path. */
// ── THE THREE HELPERS BELOW MOVED WITH THEIR CALLER ───────────────────────────────────────────────
// `PREAMBLE_ID`, `ownershipTimeline` and `ownerAt` were the path's private helpers and they are in
// the crate now, transcribed, with their own comments. They are not duplicated here: a second
// ownership fold is exactly what `lib/path.ts` was written to end (round 167/170), and the crate's
// header is where a reader looks for the rule now.

export function derivePath(rounds: TrajRound[], controlEvents: CommandEvent[] = []): SessionPath {
  return logic().derive_path(rounds, controlEvents) as SessionPath;
}

/** Fold the steps into the numbers a person actually wants: how much work, how
 *  much of it failed, and how long it took. */
export function summarizePath(steps: PathStep[]): PathSummary {
  return logic().summarize_path(steps) as unknown as PathSummary;
}

/** Steps worth a second look, worst first: a failure or interruption is what an
 *  operator returning to a session is looking for. Ordered by position within a
 *  severity band so the list reads along the path. */
export function attentionSteps(steps: PathStep[]): PathStep[] {
  return logic().attention_steps(steps) as PathStep[];
}
