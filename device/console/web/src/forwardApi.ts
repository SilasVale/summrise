import { callTool, ToolError, type Grant } from "./terminal";

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
  // A row without a name cannot be removed and cannot be identified in a refusal, so it is not a
  // row this page can act on: dropping it is the only answer that does not invent one.
  if (name === "") return null;
  return {
    name,
    listen: String(raw.listen ?? ""),
    target: String(raw.target ?? ""),
    via: String(raw.via ?? ""),
    listening: raw.listening === true,
    since_ms: typeof raw.since_ms === "number" ? raw.since_ms : null,
    attempts: Number.isFinite(Number(raw.attempts)) ? Number(raw.attempts) : 0,
    connections: Number.isFinite(Number(raw.connections)) ? Number(raw.connections) : 0,
    last_error: raw.last_error == null ? null : String(raw.last_error)
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
  if (!Array.isArray(parsed)) return null;
  return parsed.map(row).filter((entry): entry is ForwardRow => entry !== null);
}

export async function listForwards(grant: Grant): Promise<ForwardRow[]> {
  const rows = forwardRows(await callTool(grant, "forward.list", {}));
  if (rows === null) throw new Error("the machine answered forward.list without a list");
  return rows;
}

/**
 * The five values a rule is, plus the credential that makes the connection.
 *
 * `host_key` is optional and absent means "the device's `known_hosts` decides". The device refuses a
 * pin of the wrong shape at creation rather than at the first connection, so a form that sends a
 * mangled one is refused where the operator can still see it.
 */
export type NewForward = {
  name: string;
  listen: string;
  target: string;
  via: string;
  auth: { kind: "password"; password: string } | { kind: "key"; path: string };
  host_key?: string;
};

export async function addForward(grant: Grant, rule: NewForward): Promise<ForwardRow> {
  const args: Record<string, unknown> = {
    name: rule.name,
    listen: rule.listen,
    target: rule.target,
    via: rule.via,
    auth: rule.auth
  };
  // Omitted when empty, not sent as "": the device's own schema says `host_key` optionally pins a
  // fingerprint, and an empty string is a shape it refuses.
  if (rule.host_key !== undefined && rule.host_key !== "") args.host_key = rule.host_key;
  const added = row(JSON.parse(await callTool(grant, "forward.add", args)) as unknown);
  if (added === null) throw new Error("the machine added the rule without returning it");
  return added;
}

/** `removed` says whether a rule was there; `false` is an answer, not a failure. */
export async function removeForward(grant: Grant, name: string): Promise<boolean> {
  const text = await callTool(grant, "forward.remove", { name });
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new Error("the machine answered forward.remove with something this page cannot read");
  }
  const removed =
    parsed !== null && typeof parsed === "object"
      ? (parsed as Record<string, unknown>).removed
      : undefined;
  if (typeof removed !== "boolean") {
    throw new Error("the machine answered forward.remove without saying whether a rule was removed");
  }
  return removed;
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
