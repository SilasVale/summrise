import type { AuditLine } from "./api";
import { shortId } from "./format";

/**
 * What to call one token in a cell, a filter or a record.
 *
 * The name is the operator's, and it is the thing they recognise; the id is what the console recorded
 * and what the filter compares. So a name the list knows wins, and an id it does not (a revoked token,
 * one created in another tab, or the whole list when `GET /tokens` failed) falls back to its short
 * form, which still tells two tokens apart.
 */
export function tokenLabel(id: string, names: ReadonlyMap<string, string>): string {
  const name = names.get(id);
  return name != null && name !== "" ? name : shortId(id);
}

/**
 * The record's Token fact.
 *
 * A line with a token names it, by the operator's name when the page knows it and by its short id
 * otherwise. A line with no token is not one fact: the console's own `/mcp` calls ran on the
 * operator's session, while a client's call on the standard route was a signed grant — the two
 * audiences the audit's `caller` field keeps apart. An operator action (a grant, a token) or a line
 * older than `caller` names no audience, so it gets an em dash rather than a guess.
 */
export function tokenFact(
  line: Pick<AuditLine, "token_id" | "caller">,
  names: ReadonlyMap<string, string>
): string {
  if (line.token_id) return tokenLabel(line.token_id, names);
  if (line.caller === "console") return "operator session";
  if (line.caller === "client") return "signed grant";
  return "—";
}
