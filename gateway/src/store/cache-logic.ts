/**
 * cache-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/store.rs` (`render_typescript`), which is now the single source for the rules
 * below. The freshness test in that module is a byte comparison against this file, so a hand edit fails there
 * with the text and one command to fix it:
 *
 *     SUMMRISE_REFRESH_CACHE=1 cargo test --manifest-path gateway/wasm/Cargo.toml store::emission
 *
 * `gateway/src/store/cache.ts` re-exports this module and keeps the HOST: the two `Map`s and the pruning of the
 * per-key lock chain. **THE RULES ARE HERE** — the two TTLs, the prefix list that chooses between them, the two
 * bounds, and the comparison the cache asks before it evicts.
 *
 * **AND THE PREFIX LIST IS WHY THIS EMISSION EXISTS**: the worker's port of it was missing `"providers:"` —
 * with a comment claiming the source kept that entry commented out, and a test pinning the same wrong answer —
 * so a deleted or re-pointed provider stayed dialled for up to 24 h on a warm isolate where the console
 * propagates it within a minute.
 */

/** The backstop TTL: after 24 h a cached value is re-read from KV even if no write touched this isolate. */
export const CACHE_TTL_MS = 24 * 60 * 60 * 1000;

/** Security-critical keys get the short TTL, so an admin change propagates across isolates within a minute. */
export const AUTH_CACHE_TTL_MS = 60 * 1000;

/** The per-isolate cache's bound: at it, the OLDEST entry goes (a `Map`'s insertion order). */
export const CACHE_MAX_ENTRIES = 512;

/** The per-key lock table's hard bound, for pathological distinct-key bursts. */
export const LOCK_MAX_ENTRIES = 512;

/** The key prefixes that get the SHORT TTL. `"providers:"` is on it: a custom provider record carries a
 *  credential and an egress DESTINATION, so a deleted or re-pointed one must stop being dialled within a
 *  minute, not within a day. */
export const AUTH_PREFIXES: string[] = [
  "settings:",
  "token:",
  "user:",
  "ukeys:",
  "auth:",
  "route:",
  "devices:",
  "plugins:",
  "cf:",
  "providers:",
];

/** `cset`'s TTL decision: the short TTL for an auth-shaped key, the 24 h one otherwise. */
export function ttlFor(key: string): number {
  return AUTH_PREFIXES.some((p) => key.startsWith(p)) ? AUTH_CACHE_TTL_MS : CACHE_TTL_MS;
}

/** `cset`'s eviction test — the comparison, with the eviction itself left to the host that owns the `Map`. */
export function cacheAtBound(size: number): boolean {
  return size >= CACHE_MAX_ENTRIES;
}
