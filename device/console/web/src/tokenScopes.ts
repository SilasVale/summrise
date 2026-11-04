import type { Device, TokenMeta } from "./api";
import { shortId } from "./format";

/** Which endpoint a token belongs to: one device's, or the fleet endpoint's aggregate scope. */
export type TokenScope = "device" | "fleet";

/**
 * An aggregate token and a device token are the same record shape, told apart by their scope:
 * `device_ids` is the fleet scope, `device_id` the single-machine one. The console writes an empty
 * `device_id` on an aggregate.
 */
export function tokenScope(entry: Pick<TokenMeta, "device_id" | "device_ids">): TokenScope {
  return (entry.device_ids?.length ?? 0) > 0 ? "fleet" : "device";
}

/** The Scope cell's words, and the `title` that keeps the short label honest. */
export type ScopeWords = { label: string; title: string };

/**
 * The Scope column: what this record can actually reach.
 *
 * A device token names its machine — "this machine" only when this is the sheet's machine, otherwise
 * the machine's name, so a token minted elsewhere is not mistaken for one of ours. An aggregate
 * token says how many machines it was minted for: `/mcp/all` enforces that recorded list literally,
 * so a machine enrolled after minting is not covered and the title says so rather than letting a
 * count read as "everything".
 */
export function scopeLabel(
  entry: Pick<TokenMeta, "device_id" | "device_ids">,
  machines: readonly Pick<Device, "id" | "name">[] = [],
  currentDeviceId: string | null = null
): ScopeWords {
  if (tokenScope(entry) === "fleet") {
    const count = entry.device_ids?.length ?? 0;
    return {
      label: `${count} machine${count === 1 ? "" : "s"} it was minted for`,
      title:
        "It reaches only the machines recorded on it; a machine enrolled after it was minted is not covered."
    };
  }
  if (entry.device_id === currentDeviceId) {
    return {
      label: "this machine",
      title: "Minted for the machine this sheet was opened on."
    };
  }
  const machine = machines.find((candidate) => candidate.id === entry.device_id);
  const named = machine?.name || shortId(entry.device_id);
  return { label: named, title: `Minted for ${named}.` };
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
