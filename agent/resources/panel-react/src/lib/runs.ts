// Run identity — folding the device's operation timeline into RUNS.
//
// An AI client can declare the boundaries of "one run" (one execution of its
// work) with the device's `run_begin` / `run_end` tool pair. The device mints a
// `run_id` and stamps it onto the records it writes; `GET /api/operation`
// returns those records (`events`) and the boundaries (`runs`) as two SEPARATE
// arrays on one millisecond axis. This module is the only place that joins them.
//
// THE ONE RULE THAT MATTERS: never fabricate an attribution.
//
//   * An event whose `run_id` is absent is UNATTRIBUTED. It gets its own group
//     and is never folded into whatever run happened to precede it in the
//     stream. Folding would assert "this command belonged to that run" from
//     nothing but adjacency, which is precisely the claim the run id exists to
//     make verifiable. It is also the failure that is hardest to notice: the
//     numbers would look complete.
//
//   * An id with no `run/begin` is UNREGISTERED, and is shown under the raw id
//     rather than hidden or merged into a neighbour. The device's run log is
//     best-effort and capped, so this happens honestly.
//
//   * An OPEN run (a begin with no matching end) is the COMMON case — the client
//     may still be working, may have stopped, or the agent may have restarted.
//     Its extent is `[begin, the newest event carrying that id]`, NEVER
//     `[begin, now]`: a live-ticking span grows forever on screen and implies a
//     knowledge of "still running" that no one here has.
//
// WHAT ELSE THIS MODULE CARRIES. Grouping alone answers "how many" and "how
// long", which is all a one-line strip needs. The device's records themselves —
// what ran, how it ended, why — are extracted here too (`operationRows`, and the
// `rows` on every group) so that no reader has to re-derive them from the raw
// array. Two readers use this: the run strip beside a session (counts and
// extent) and the device-level Activity page (the records). Both read the SAME
// grouping, so they cannot disagree about which run a record belongs to, and the
// extraction rules — drop an unplaceable record, never fold an unattributed one
// — live in one place.
//
// THE FOLD AND THE PROJECTION ARE RUST NOW (P2, 2026-09-28), and this module is the seam they are
// reached through. `groupOperation` and `operationRows` moved to
// `agent/resources/panel-logic/src/runs.rs`, transliterated — the same drops, the same three
// nothing-kinds collapsed by one rule, the same `closed`-first state, the same never-fabricate-an-
// attribution discipline.
//
// ── THE SYNC STORY, WHICH IS WHY THIS FAMILY MOVED THE WAY IT DID ────────────
//
// This is the first family whose call site is DURING RENDER: `RunStrip` and `ActivityPage` each
// called one of these inside a `useMemo`. A migrated function cannot be called synchronously
// during render — the wasm is fetched at the first call, never at page load (criterion ③) — and
// inlining the bytes costs +18,913 gz on the first-load payload and grows with every family. So
// the derivation moved to the DATA BOUNDARY instead: `useOperationRuns`'s `reduce` is where
// `/api/operation`'s reply becomes this panel's value, it may already return a promise, and the
// wasm is in hand there. The groups and the rows are computed in that same fold and ride out on
// the same snapshot the events do — so the render reads a FIELD, and there is no `await`, no
// fallback and no second implementation of anything.
//
// The rule that comes with it, and it is what keeps this from being an exemption: **a function
// that reads the DERIVED value is not a second derivation.** `groupCount`, `runStateNote`,
// `RUN_STATE_LABEL` and `ActivityPage`'s `extent` all read the grouped result — the very array the
// views map over — so they cannot disagree with it, and they stay here, where they are read.
//
// NO LONGER "pure by construction": the two functions below await the panel's Rust, and the
// sentence that said otherwise is the one thing this header had to lose.
import { panelLogic } from "../wasm/panelLogic";

/** One row of `GET /api/operation`'s `events` array. Every field is optional in
 *  the type because the panel must survive a device that sends fewer of them
 *  than this build expects — a missing key must read as absent, not crash. */
export interface OperationEvent {
  /** Which feed the row came from. Anything that is not `browser` is the
   *  terminal audit trail (the only other producer on the device). */
  source?: string | null;
  /** ALWAYS milliseconds — the only axis this module sorts or spans on. */
  ts_ms?: number | null;
  /** Terminal events only; browser actions have no session. */
  session?: string | null;
  kind?: string | null;
  /** Terminal only: the audit file's per-session monotonic sequence. */
  seq?: number | null;
  command?: string | null;
  text?: string | null;
  status?: string | null;
  exit_code?: number | null;
  duration_ms?: number | null;
  intent?: string | null;
  considered?: string[] | null;
  plan_step?: number | null;
  /** Browser rows. */
  script?: string | null;
  screenshots?: string[] | null;
  timed_out?: boolean | null;
  /** The run this activity was attributed to. `null`/absent = never attributed. */
  run_id?: string | null;
}

/** One record of `GET /api/operation`'s `runs` array: a `run/begin` or a
 *  `run/end`.
 *
 *  `label`, `goal` and `outcome` ARRIVE AS `null` WHEN THE CLIENT SUPPLIED
 *  NOTHING — the key is PRESENT. This said the opposite ("ABSENT — a missing key,
 *  not `null` and not `""` … so presence is read, never truthiness"), and the
 *  device's own comment is explicit about which is true: "`json!` renders `None`
 *  as `null`, so the JSONL — and `recent`, which passes it straight through —
 *  carries `"label": null` rather than omitting the key … Every consumer must
 *  therefore treat null, missing AND blank alike."
 *
 *  So there are THREE kinds of nothing on this wire, and the discipline is to
 *  collapse all three rather than to test for one of them. `value()` below is
 *  that collapse and is what the code has always used; the sentence above was
 *  the wrong half, and a future reader would have taken it as licence to write a
 *  key-existence check that silently misses every real case. */
export interface RunBoundary {
  kind?: string | null;
  run_id?: string | null;
  ts_ms?: number | null;
  label?: string | null;
  goal?: string | null;
  outcome?: string | null;
}

type RunState = "closed" | "open" | "unregistered" | "unattributed";

/** Which of the device's two feeds a row came from. Anything that is not the
 *  browser feed is the terminal audit trail — the only two producers there are. */
type ActivitySource = "terminal" | "browser";

/** ONE record of the timeline, as a row a reader can render.
 *
 *  A row is a READING of a record, never a summary of two of them: every field
 *  is either the value the device sent or `null`, and nothing here is inferred
 *  from a neighbouring record. That is why a `command/end` whose
 *  `command/start` fell outside the window stands as its own row instead of
 *  being attached to the command before it — joining the two is a per-session
 *  derivation that already has exactly one owner (`lib/path.ts`, for the Path
 *  view), and a second copy of it here would be a second answer to "which
 *  command did this end belong to". */
export interface ActivityRow {
  /** Stable identity for list rendering, built from the record's own fields.
   *  Never rendered. */
  id: string;
  source: ActivitySource;
  /** ALWAYS milliseconds — the only axis rows are ordered or displayed on. */
  tsMs: number;
  /** The record's own kind: `command/start`, `command/end`, `goal`, `plan`,
   *  `approval`, `control`, or `action` for the browser feed. */
  kind: string | null;
  /** Terminal rows only; the browser feed has no session ownership. */
  session: string | null;
  seq: number | null;
  /** Terminal: what ran, when the record carries it. */
  command: string | null;
  /** Browser: the script that ran, when the record carries it. */
  script: string | null;
  /** The record's own text payload (a goal, a plan, an approval subject). */
  text: string | null;
  /** The record's own status word (`human` / `ai` on a handoff, the action on
   *  an approval record). */
  status: string | null;
  /** A VALUE, not a flag: `0` is an outcome the device recorded and `null` is
   *  the absence of one. The two are never collapsed — an exit code of zero is
   *  the single most common real outcome there is. */
  exitCode: number | null;
  durationMs: number | null;
  intent: string | null;
  /** Alternatives the client says it passed over. `[]` when it named none —
   *  the reader renders nothing rather than a note about the silence. */
  considered: string[];
  planStep: number | null;
  /** Screenshot names a browser action produced. `[]` when none. */
  screenshots: string[];
  /** The RECORDED timeout flag: true only when the device wrote true. */
  timedOut: boolean;
  /** The run the record declared. `null` means it declared none, and the row
   *  then belongs to the unattributed bucket — never folded into a neighbour. */
  runId: string | null;
}

export interface RunGroup {
  /** `null` for the unattributed bucket. Inventing an id ("unknown") there would
   *  render as an id the device could be asked about, and there is none. */
  runId: string | null;
  state: RunState;
  /** Absent ⇒ `null`. Never `""`, never a placeholder word. */
  label: string | null;
  goal: string | null;
  outcome: string | null;
  /** Wall-clock span on the `ts_ms` axis. For an open run `endMs` is the newest
   *  event carrying the id — never the current time. */
  startMs: number;
  endMs: number;
  /** How many of the device's terminal events carry this `run_id`. */
  terminal: number;
  /** How many browser actions carry it. */
  browser: number;
  /** The records themselves, oldest first. `rows.length` is exactly
   *  `terminal + browser`: a record whose `ts_ms` could not be used is counted
   *  NOWHERE and shown nowhere, because it cannot be placed in any group — the
   *  same drop the device performs at the source (see `groupOperation`). */
  rows: ActivityRow[];
}

/** WHAT THE FOLD PRODUCES, and the shape the two surfaces read. Both halves are the panel's Rust
 *  (`panel-logic/src/runs.rs`), computed at the data boundary — see the header. */
export interface OperationGroups {
  /** Sorted by start time. */
  runs: RunGroup[];
  /** The events with no `run_id`, as their OWN group. `null` when there are none — an empty group
   *  is not rendered at all. */
  unattributed: RunGroup | null;
}

/** One group with the records it holds, in the order the Activity page draws them: "grouped by
 *  run, oldest group first, the unattributed bucket last and separate". */
export interface ActivityGroup {
  group: RunGroup;
  rows: ActivityRow[];
}

/** THE EMPTY DERIVATION — what `groupOperation([], [])` answers, and the value a reader starts
 *  from before the first reply has landed. It is a CONSTANT rather than a call because the first
 *  render happens before anything has been read, and the surfaces read the derived fields
 *  unconditionally (a `null` check in every one of them would be a branch per caller for a case
 *  that has exactly one answer).
 *
 *  AND IT IS A CLAIM, NOT A SECOND IMPLEMENTATION: the panel's own suite asserts that the wasm's
 *  answer for the empty input IS this object, so a derivation that started answering something
 *  else for `[]` would fail there rather than show up as a strip that draws a group nobody sent. */
export const EMPTY_GROUPS: OperationGroups = { runs: [], unattributed: null };

/** THE BOUNDARY DERIVATION — one `await` for the two calls, because the wasm is loaded at the
 *  first of them and every call after it is synchronous. This is the function a DATA BOUNDARY
 *  calls (`useOperationRuns`'s fold); the two wrappers below are the same thing with the
 *  signatures this module has always exported, for the tests and for any caller that holds the
 *  raw events rather than the groups.
 *
 *  `operation_rows` takes the GROUPS rather than the events, deliberately: the grouping has
 *  exactly one implementation, so the rows a reader sees cannot disagree with the counts a strip
 *  shows. Doing it here rather than in Rust's own `operation_rows` is also what keeps the fold
 *  from grouping twice. */
export async function deriveRuns(
  events: OperationEvent[],
  boundaries: RunBoundary[],
): Promise<{ groups: OperationGroups; rows: ActivityGroup[] }> {
  const logic = await panelLogic();
  const groups = logic.group_operation(events, boundaries) as OperationGroups;
  return { groups, rows: logic.operation_rows(groups) as ActivityGroup[] };
}

/** Fold the timeline's events and its run boundaries into one group per run, plus the unattributed
 *  bucket.
 *
 *  Order-independent: both inputs may arrive in any order (the hook accumulates them across polls),
 *  so extents are computed with min/max rather than by position, and a `run/begin` that arrives
 *  after its own events still registers the run. */
export async function groupOperation(
  events: OperationEvent[],
  boundaries: RunBoundary[],
): Promise<OperationGroups> {
  return (await deriveRuns(events, boundaries)).groups;
}

/** Every row of the timeline, in the same groups `groupOperation` builds.
 *
 *  A record with no usable `ts_ms` appears in NEITHER — not counted, not shown. The device drops
 *  such a record before it ever reaches the panel (`operation.rs` orders on the explicit
 *  millisecond stamp and refuses to guess the unit), so this is the defensive half of one rule
 *  rather than a second one: an unplaceable record has no group to belong to, and inventing one
 *  from adjacency is the fabrication this module exists to prevent. */
export async function operationRows(
  events: OperationEvent[],
  boundaries: RunBoundary[],
): Promise<ActivityGroup[]> {
  return (await deriveRuns(events, boundaries)).rows;
}

/** Total rows a fully expanded strip would draw. */
export function groupCount(g: OperationGroups): number {
  return g.runs.length + (g.unattributed ? 1 : 0);
}

/** The word each state is rendered as, in BOTH views that read a group (the run
 *  strip in the Path view and the device-level Activity page). Deliberately the
 *  state's own name rather than a verdict: "closed" says the boundary was
 *  recorded, not that it went well — that judgement is not this panel's to make
 *  from a boundary record. */
export const RUN_STATE_LABEL: Record<RunState, string> = {
  closed: "closed",
  open: "open",
  unregistered: "unregistered",
  unattributed: "unattributed",
};

/** The one short line of honesty each state needs, or `null` for a state whose
 *  own name already says everything (a closed run needs no note).
 *
 *  `null` means NOTHING IS DRAWN — not a placeholder, and not a statement that
 *  there is nothing to say. */
export function runStateNote(g: RunGroup): string | null {
  switch (g.state) {
    // "Still running" is exactly what these views CANNOT say: the client may
    // have stopped, or the agent may have restarted. The absence is the fact.
    case "open": return "no end recorded";
    case "unregistered": return "no begin recorded";
    // The bucket is the ONE group whose reason is not a missing boundary: it is
    // apart because its records declared no run at all, and the whole point of
    // the rule is that no reader should assume the attribution the data does
    // not carry. Both views render this, so neither can show the bucket without
    // saying why it is one.
    case "unattributed":
      return "these events declared no run — shown apart rather than assumed into one";
    default: return null;
  }
}
