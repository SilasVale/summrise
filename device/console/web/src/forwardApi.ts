import { callTool, ToolError, type Grant } from "./terminal";

/**
 * **THE MOMENT A ROW WAS READ, IN THE CONSOLE'S CLOCK.** `since_ms` comes from the DEVICE; every
 * "how long ago" on this page would otherwise subtract it from the BROWSER's `Date.now()`, and those
 * are two machines' clocks. A device or a console a minute out is a minute added to (or taken off)
 * every rule's age — silently, because "bound 1 m" and "bound 2 m" both look plausible. The value is
 * taken once, when the list is read, so every row on one read is dated by the same moment.
 */
export type ForwardRead = { rows: ForwardRow[]; readAtMs: number };

/**
 * One rule's live state, as `forward.list` describes it.
 *
 * **THE LIST CANNOT CARRY A CREDENTIAL, AND THAT IS WHY IT CAN BE DRAWN.** The device stores
 * `auth` inside the row (D5: a rule outlives the caller, so its executor must reach `via` at boot
 * with nobody present), and `forward.list` reports the row WITHOUT those fields. A page that showed
 * what the device holds would be a second copy of the secret, in a browser, for as long as a
 * screenshot lives. Every field here is read defensively: the page must survive a device running an
 * older or partial shape.
 */
export type ForwardRow = {
  /** The rule's name: how an edit replaces the rule with that name. */
  name: string;
  /** The `host:port` bound on the machine. */
  listen: string;
  /** The `host:port` the connection dials. */
  target: string;
  /** The `user@host:port` of the SSH connection that carries it. */
  via: string;
  /** Whether bind() has returned RIGHT NOW — not whether the rule exists. */
  listening: boolean;
  /** When the current run of `listening` began, in Unix milliseconds. */
  since_ms: number | null;
  /** How many bind attempts the ladder has made. A bound rule still reports its ladder's history. */
  attempts: number;
  /** How many connections the executor CARRIED — both directions ended without an error. */
  connections: number;
  /** The last failure's sentence, cleared by the next success. */
  last_error: string | null;
};

const row = (value: unknown): ForwardRow | null => {
  if (value === null || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  const name = typeof raw.name === "string" ? raw.name : "";
  // A row without a name cannot be identified in a refusal, so it is not a row this page can show:
  // dropping it is the only answer that does not invent one.
  if (name === "") return null;
  // **THE LIVE STATE IS NESTED, AND THIS READER EXPECTED IT FLAT (the hundred-and-fortieth list).** The
  // device's `forward.list` answers `{rules: [{name, listen, target, via, state: {…}}]}` — the row plus the
  // executor's own `ForwardState`. This reader was written against a bare array of FLAT rows, a shape the
  // device has never sent, so every state field it looked for was undefined and a bound rule would draw as
  // a rule that is not listening. The shape below is read from `agent/src/plugins/forward/tools.rs` and
  // `forward::ForwardState`, not from this page's own fixture.
  const state = (raw.state !== null && typeof raw.state === "object" ? raw.state : {}) as Record<
    string,
    unknown
  >;
  // `since_ms` is 0 when the executor has NEVER listened (the device's own doc on the field): a 1970
  // timestamp is not a fact about this machine, and "bound 56 years" is not a sentence anyone can act on.
  const since = typeof state.since_ms === "number" && state.since_ms > 0 ? state.since_ms : null;
  const count = (value: unknown) => (Number.isFinite(Number(value)) ? Number(value) : 0);
  return {
    name,
    listen: String(raw.listen ?? ""),
    target: String(raw.target ?? ""),
    via: String(raw.via ?? ""),
    listening: state.listening === true,
    since_ms: since,
    attempts: count(state.attempts),
    connections: count(state.connections),
    last_error: state.last_error == null ? null : String(state.last_error)
  };
};

/**
 * Parse the answer to `forward.list`.
 *
 * `[]` and `null` are different answers and must stay different: an empty array is "this machine
 * carries no rules", and `null` is "the device said something this page cannot read". Rendering the
 * second as the first would tell an operator their machine has no rule while it has one.
 */
export function forwardRows(text: string): ForwardRow[] | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  // The device wraps the list: `{ "rules": [...] }`. A bare array is what this page USED to expect and the
  // device has never sent it; an answer with no `rules` array is `null` — "this page cannot read it" — which
  // is a different fact from "the machine carries no rules".
  const listed =
    parsed !== null && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>).rules
      : undefined;
  if (!Array.isArray(listed)) return null;
  return listed.map(row).filter((entry): entry is ForwardRow => entry !== null);
}

export async function listForwards(grant: Grant): Promise<ForwardRead> {
  const text = await callTool(grant, "forward.list", {});
  const rows = forwardRows(text);
  if (rows === null) throw new Error("the machine answered forward.list without a list");
  // **READ AFTER THE ANSWER, NOT BEFORE THE CALL.** `Date.now()` before the call would date the
  // rows by a moment that is already older than the answer; after it, the age of a bound listener is
  // measured from the moment the page learned it was bound.
  return { rows, readAtMs: Date.now() };
}

/**
 * The sentence a person reads for a refusal.
 *
 * The device refuses a rule with a code of its own (`-32001`/`-32002`) or with a plain tool error;
 * either way the thing worth showing is its own message, because every refusal the device writes is
 * a sentence about THIS rule. A transport failure is the console's, and says so.
 */
export function forwardFailure(cause: unknown): string {
  if (cause instanceof ToolError) {
    if (cause.status === 0) return "The console could not reach the device.";
    if ([502, 503, 504].includes(cause.status)) {
      return `The console could not reach the device (${cause.status}).`;
    }
    return cause.message;
  }
  return cause instanceof Error ? cause.message : String(cause);
}
