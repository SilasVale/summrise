import type { AuditSource } from "./api";
import { auditSourceSentence, type AuditWindowSpan } from "./auditSource";

/**
 * The one muted line beside the window's cap sentence: what is read, and which span this page shows.
 *
 * It is rendered from the server's facts and the page's own lines only. The file's lifetime and the
 * window's span are two different facts, so the line states them as two: `lines since 10-22` is the
 * file, and `This window: 11-06 21:01 → 11-08 01:52` is the page (audit-44 F10). When `/audit/source`
 * did not answer, or the file it names could not be read, the sentence is absent and so is this line
 * — the page says nothing rather than guessing a size or a moment.
 */
export function AuditSourceLine({
  source,
  window
}: {
  source?: AuditSource | null;
  /** The span of the window the page drew; omitted when the page is showing a chosen range. */
  window?: AuditWindowSpan | null;
}) {
  const sentence = auditSourceSentence(source, window);
  if (sentence === null) return null;
  return <p className="hint audit-source">{sentence}</p>;
}
