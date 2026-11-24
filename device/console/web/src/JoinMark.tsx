import type { JoinKind } from "./joinMarks";
import { JOIN_ONLY_TITLE, JOIN_ONLY_WORD, JOINED_TITLE, joinSentence, type JoinCounts } from "./joinMarks";

/**
 * **THE MARK ON ONE ROW, AND ONLY EVER ON A DEVIATION.** A call the machine's journal still holds
 * needs no word: the two records agree, and a chip on every healthy row would be noise on most of the
 * page. What earns a mark is the row whose other half has rotated out of the machine's journal — the
 * console is then the only place the call can still be read.
 *
 * `unknown` draws nothing at all. The console could not ask (a fleet window, an older machine, a
 * failed read), and a mark there would accuse a machine of losing a call it was never asked about.
 */
export function JoinMark({ kind }: { kind: JoinKind }) {
  if (kind !== "console") return null;
  return (
    <span className="chip chip-warn" title={JOIN_ONLY_TITLE}>
      {JOIN_ONLY_WORD}
    </span>
  );
}

/**
 * The one sentence under a window, or nothing.
 *
 * It sits beside the provenance line because it is the same kind of fact: what this window could read,
 * and what it could not. Silence is the ordinary case — a window every one of whose calls the machine
 * still holds has nothing to report, and the join's success is that nothing needed saying.
 *
 * `error` is the console's reason for the calls it could not check; it is the same sentence the
 * page's other failed reads use, so the line never invents a cause of its own.
 */
export function JoinLine({ counts, error = null }: { counts: JoinCounts; error?: string | null }) {
  const sentence = joinSentence(counts, error);
  if (sentence === null) return null;
  return <p className="hint audit-join">{sentence}</p>;
}

/**
 * What the opened record says about the join, or null when there is nothing to say.
 *
 * The row's own sentence, spelled out: `one` names the machine's copy, `console` names what is missing
 * and where, and `unknown` stays quiet because a record that says "not checked" beside every row on a
 * fleet window is a fact about the page, not about the call.
 */
export function joinRecordSentence(kind: JoinKind): string | null {
  if (kind === "one") return JOINED_TITLE;
  if (kind === "console") return JOIN_ONLY_TITLE;
  return null;
}
