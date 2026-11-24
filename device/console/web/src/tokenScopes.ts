import type { Device, TokenKind, TokenMeta } from "./api";
import { shortId } from "./format";

/** Which endpoint a token belongs to: one device's, the fleet endpoint's, or not recorded. */
export type TokenScope = TokenKind | "unknown";

/**
 * The door a record opens: `/mcp/all` for a fleet token, `/mcp/<short device>` for a device token.
 *
 * The console writes `kind` on every record, so a normal read is that fact. A payload from before
 * the field existed is read from the scope shape — `device_ids` is the fleet scope, a non-empty
 * `device_id` the single-machine one. When neither fact is there the answer is `unknown`: a door
 * the page cannot prove must not be printed as the other one.
 */
export function tokenScope(
  entry: Pick<TokenMeta, "kind" | "device_id" | "device_ids">
): TokenScope {
  if (entry.kind === "device" || entry.kind === "fleet") return entry.kind;
  if ((entry.device_ids?.length ?? 0) > 0) return "fleet";
  if (entry.device_id) return "device";
  return "unknown";
}

/** The Scope cell's words, and the `title` that keeps each label honest. */
export type ScopeWords = { door: string; label: string; title: string };

/**
 * The Scope column: which endpoint this token opens, and what it can actually reach.
 *
 * The door is the first line — `fleet · /mcp/all` versus `device · /mcp/<short>` — because a token
 * is refused at the wrong endpoint, and a machine count alone cannot say where the credential
 * belongs. The reach follows: a device token names its machine ("this machine" only when this is
 * the sheet's machine), and an aggregate says how many machines it was minted for. `/mcp/all`
 * enforces that recorded list literally, so a machine enrolled after minting is not covered and the
 * title says so rather than letting a count read as "everything".
 */
export function scopeLabel(
  entry: Pick<TokenMeta, "kind" | "device_id" | "device_ids">,
  machines: readonly Pick<Device, "id" | "name">[] = [],
  currentDeviceId: string | null = null
): ScopeWords {
  const kind = tokenScope(entry);
  if (kind === "fleet") {
    // A count alone cannot answer "does this token reach the machine I am asking about?", which is
    // the question the refusal `device not permitted by token` sends the operator to this sheet for.
    const ids = entry.device_ids ?? [];
    const names = ids.map((id) => machines.find((machine) => machine.id === id)?.name || shortId(id));
    const shown = names.slice(0, 3);
    const more = names.length - shown.length;
    return {
      door: "fleet · /mcp/all",
      label:
        names.length === 0
          ? "no machines recorded"
          : `${shown.join(", ")}${more > 0 ? ` and ${more} more` : ""}`,
      title:
        `This token is accepted at /mcp/all. Covers ${names.length} machine${names.length === 1 ? "" : "s"}: ${names.join(", ")}. ` +
        "It reaches only the machines recorded on it; a machine enrolled after it was minted is not covered."
    };
  }
  if (kind === "unknown") {
    return {
      door: "unknown · endpoint not recorded",
      label: "scope not recorded",
      title:
        "This record carries neither a token kind nor a device, so the page cannot name the endpoint it opens."
    };
  }
  if (!entry.device_id) {
    // A written device kind with no machine is not a door this page can print: the kind is known,
    // the path is not. Say that rather than inventing an id.
    return {
      door: "device · endpoint not recorded",
      label: "machine not recorded",
      title:
        "This record says it is a device token but names no machine, so the page cannot name the URL it opens."
    };
  }
  const door = `device · /mcp/${shortId(entry.device_id)}`;
  if (entry.device_id === currentDeviceId) {
    return {
      door,
      label: "this machine",
      title: `This token is accepted at /mcp/${entry.device_id}. Minted for the machine this sheet was opened on.`
    };
  }
  const machine = machines.find((candidate) => candidate.id === entry.device_id);
  const named = machine?.name || shortId(entry.device_id);
  return {
    door,
    label: named,
    title: `This token is accepted at /mcp/${entry.device_id}. Minted for ${named}.`
  };
}

/**
 * One list of the whole registry: newest first, each record once.
 *
 * The created row is appended by the same function, which is how a mint shows up before the next
 * read lands; a record both the read and the create carry is the same record and appears once.
 */
export function mergeTokenRows(...sources: readonly TokenMeta[][]): TokenMeta[] {
  const byId = new Map<string, TokenMeta>();
  for (const source of sources) {
    for (const entry of source) {
      if (!byId.has(entry.id)) byId.set(entry.id, entry);
    }
  }
  return [...byId.values()].sort(
    (left, right) => right.created_at_ms - left.created_at_ms || left.id.localeCompare(right.id)
  );
}
