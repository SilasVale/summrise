import type { AuditSource } from "./api";
import { auditSourceSentence } from "./auditSource";

/**
 * The one muted line beside the window's cap sentence: what is read, next to what is drawn.
 *
 * It is rendered from the server's facts only. When `/audit/source` did not answer, or the file it
 * names could not be read, the sentence is absent and so is this line — the page says nothing
 * rather than guessing a size or a moment.
 */
export function AuditSourceLine({ source }: { source?: AuditSource | null }) {
  const sentence = auditSourceSentence(source);
  if (sentence === null) return null;
  return <p className="hint audit-source">{sentence}</p>;
}
