import type { TokenMeta } from "./api";

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

/** The scope column's words: one credential list, two scopes, one label each. */
export function scopeLabel(entry: Pick<TokenMeta, "device_id" | "device_ids">): string {
  return tokenScope(entry) === "fleet" ? "all machines" : "this machine";
}

/**
 * One list from the two sources: newest first, each record once.
 *
 * The device list and `GET /tokens` can both carry the same record (an older console answers the
 * whole registry on both), so the id is the identity and the first sighting wins. The created row
 * is appended by the same function, which is how a mint shows up before the next read lands.
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
