import { HttpError, type AuditLine, type JoinAnswer } from "./api";

/**
 * **THE JOIN, DRAWN (the ledger's third item).** A proxied call is written twice: the console's audit
 * line, and the machine's own `call.started` / `call.finished|denied` pair. Both carry one
 * `trace.call`, so the two records can be paired on that value instead of on two clocks. This module
 * is the page's half of it: which traces the window is drawing, which of them the machine still
 * holds, and the one sentence that says what the page could and could not check.
 *
 * The machine answers through `GET /audit/join` (`journal.get` on the device). Three outcomes, and
 * they are not interchangeable:
 *
 * - `one` — the machine's journal still holds the call: both records agree, and the page says nothing
 *   about the row. Agreement is the default; a chip on every healthy row is noise on most of the page.
 * - `console` — the machine's journal no longer holds it. The row is real and the call happened; what
 *   is gone is the machine's copy, so the row is marked as this console's alone.
 * - `unknown` — the console could not ask, or the machine could not answer, or the scan stopped at its
 *   cap. **Never** drawn as `console`: a page must not accuse a machine it never heard from.
 */

/**
 * How many traces one join read asks about.
 *
 * The console refuses more (`invalid_traces`), and the device's own schema refuses more, so this is
 * the bound on both sides of the wire rather than a page preference. A window draws at most a
 * thousand rows and rarely more than a hundred calls on one machine, so this is the cap that is
 * reached only by a page that was left open.
 */
export const JOIN_TRACES = 128;

/** What the machine said about one call, from the page's point of view. */
export type JoinKind = "one" | "console" | "unknown";

/** What is drawn about a row: the word, and the sentence the opened record explains it with. */
export const JOIN_ONLY_WORD = "only here";

/**
 * The row's own sentence, for the opened record.
 *
 * `console` names what is missing — the machine's copy, not the call — because "not in the journal"
 * reads as "did not happen" to anyone who has not read the architecture.
 */
export const JOIN_ONLY_TITLE =
  "Only this console's record still holds this call; the machine's journal has rotated it away.";

/** The `one` row's sentence: the join that needs no mark. */
export const JOINED_TITLE = "The machine's journal holds this call too.";

/**
 * The traces in a window worth asking about: the calls' own, newest first, deduplicated, capped.
 *
 * Only `kind === "mcp"` rows carry a trace — a grant, a token, a sign-in and a channel boundary proxy
 * nothing, so there is no machine call to join them to. A row written before the trace existed has
 * none either, and is left unasked rather than guessed at.
 */
export function joinTraces(lines: readonly AuditLine[]): string[] {
  const seen = new Set<string>();
  for (const line of lines) {
    if (line.kind !== "mcp") continue;
    const trace = typeof line.trace === "string" ? line.trace : "";
    if (trace === "") continue;
    seen.add(trace);
    if (seen.size >= JOIN_TRACES) break;
  }
  return [...seen];
}

/**
 * What one answer says about one trace.
 *
 * A bounded scan that did not find a trace is not evidence of absence: the row may sit past the cap,
 * so the answer is `unknown`. A bounded scan that *did* find it is still proof of presence, which is
 * why the bound only downgrades the negative.
 */
export function joinKind(answer: JoinAnswer | null, trace: string): JoinKind {
  if (answer === null) return "unknown";
  if (answer.unknown || !answer.journal) return "unknown";
  if (answer.joined.includes(trace)) return "one";
  if (answer.missing.includes(trace)) return answer.bounded ? "unknown" : "console";
  return "unknown";
}

/** The three counts a window's join adds up to. */
export type JoinCounts = {
  /** Calls whose trace the machine's journal still holds. */
  one: number;
  /** Calls the machine's journal no longer holds. */
  console: number;
  /** Calls the console could not check. */
  unknown: number;
  /** How many calls the window asked about. */
  asked: number;
};

/** The counts, from the traces the window draws and what is known about them. */
export function joinCounts(
  traces: readonly string[],
  kinds: ReadonlyMap<string, JoinKind>
): JoinCounts {
  const counts: JoinCounts = { one: 0, console: 0, unknown: 0, asked: traces.length };
  for (const trace of traces) {
    const kind = kinds.get(trace) ?? "unknown";
    counts[kind] += 1;
  }
  return counts;
}

/**
 * The line under the window, or `null` when it has nothing to say.
 *
 * Silence is the answer for a window every one of whose calls the machine still holds: the join
 * worked, and the proof of it is that nothing needed saying. A window with nothing to ask about
 * (no machine scope, or no call carrying a trace) says nothing either — the page must not open with
 * a sentence about a check it never made.
 *
 * Both deviations can be true at once — three calls rotated away and two the machine refused to
 * answer for — and the sentence then carries both, because a reader who is told only the first would
 * take the second for a join that worked.
 *
 * `error` is the console's own reason for an unchecked call (the machine was offline, or did not
 * answer in time). It is appended after a colon rather than written as a second sentence, so the
 * line stays one line; without it, "not checked" leaves the operator to guess which failure it was.
 */
export function joinSentence(counts: JoinCounts, error: string | null = null): string | null {
  if (counts.asked === 0) return null;
  if (counts.console === 0 && counts.unknown === 0) return null;
  if (counts.asked === 1) {
    return counts.console === 1
      ? "This call is in this console's record only — the machine's journal no longer holds it."
      : `This call was not checked against the machine's journal${error === null ? "" : `: ${error}`}.`;
  }
  const clauses: string[] = [];
  if (counts.console > 0) {
    const verb = counts.console === 1 ? "is" : "are";
    const them = counts.console === 1 ? "it" : "them";
    clauses.push(
      `${counts.console} of these ${counts.asked} calls ${verb} in this console's record only ` +
        `— the machine's journal no longer holds ${them}`
    );
  }
  if (counts.unknown > 0) {
    // **WHY IT WAS NOT CHECKED IS PART OF THE FACT.** "Not checked" alone leaves the operator to
    // guess between an older machine, an offline one and a console that did not answer; the console's
    // own sentence says which, and it is the same sentence every other failed read on the page shows.
    const why = error === null ? "" : `: ${error}`;
    clauses.push(
      `${counts.unknown} ${counts.unknown === 1 ? "was" : "were"} not checked against the ` +
        `machine's journal${why}`
    );
  }
  return `${clauses.join("; ")}.`;
}

/** What one window's join is: the answer about each trace, the counts it adds up to, and a failure. */
export type JoinMarks = {
  kinds: ReadonlyMap<string, JoinKind>;
  counts: JoinCounts;
  /** The console's sentence for the traces it could not check, or null. */
  error?: string | null;
};

/**
 * The mark for one row.
 *
 * A row that names no trace was never asked about — a grant, a token, a call written before the field
 * existed — so it is `unknown` and draws nothing. A row whose trace is not in the map for any other
 * reason is the same: the answer did not cover it.
 */
export function markOf(marks: JoinMarks | null | undefined, line: AuditLine): JoinKind {
  const trace = typeof line.trace === "string" ? line.trace : "";
  if (trace === "") return "unknown";
  return marks?.kinds.get(trace) ?? "unknown";
}

/**
 * The page's sentence for a per-machine read that failed.
 *
 * The console answers a failed read with a status and a code; both are the console's words, not the
 * operator's. The three common causes get the page's voice, and anything else falls back to the
 * error's own sentence so an unexpected failure is still stated rather than swallowed. It lives here,
 * beside the join, because the join and the journal read are the same kind of read: the page asking
 * one machine about the window it draws, with one voice for both.
 */
export function readFailureSentence(cause: unknown): string {
  if (cause instanceof HttpError) {
    if (cause.status === 503) return "the machine is not connected";
    if (cause.status === 504) return "the machine did not answer in time";
    if (cause.status === 404) return "this console does not know that machine";
    if (cause.message && cause.message !== cause.code) return cause.message;
    return cause.code;
  }
  return cause instanceof Error ? cause.message : String(cause);
}
