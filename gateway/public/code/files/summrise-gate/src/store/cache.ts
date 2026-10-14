/**
 * store/cache.ts — per-isolate KV cache + per-key write locks.
 *
 * This is the SINGLE process-global instance: every store/<domain> module
 * imports cget/cset/cdel from here (never its own copy) so write-through
 * freshness and the test hook __clearCaches() stay coherent across domains.
 *
 * **THE RULES ARE AN EMISSION NOW (2026-10-14)** — `store/cache-logic.ts`, GENERATED from
 * `gateway/wasm/src/store.rs`. What is left here is the HOST: the two `Map`s and the pruning of the per-key
 * lock chain. **AND THE EMISSION IS WHY A LIVE DIVERGENCE WAS FOUND**: the worker's copy of the prefix list was
 * missing `"providers:"`, with a comment claiming the source kept that entry commented out — so a deleted or
 * re-pointed provider stayed dialled for up to 24 h on a warm isolate where this file propagates it within a
 * minute.
 */

import { LOCK_MAX_ENTRIES, cacheAtBound, ttlFor } from "./cache-logic.ts";

export {
  AUTH_CACHE_TTL_MS,
  AUTH_PREFIXES,
  CACHE_MAX_ENTRIES,
  CACHE_TTL_MS,
  LOCK_MAX_ENTRIES,
  cacheAtBound,
  ttlFor,
} from "./cache-logic.ts";

/** Workers env bindings — the shape we touch (loosely typed, same style as
 *  registry.ts). */
export interface Env {
  [key: string]: any;
}

/* ---- Per-isolate TTL cache ----
 *
 * Reads go through this cache (24h TTL); every write refreshes the cache
 * immediately (write-through), so admin changes take effect instantly on the
 * hot isolate. The TTL only backstops cross-isolate consistency: after 24h a
 * cached value is re-read from KV even if no write touched this isolate.
 * KV read volume is thus decoupled from request volume — each key costs at
 * most one read per day per isolate, instead of one read per request.
 */
/* The two TTLs, the prefix list that chooses between them and the two bounds are `cache-logic.ts`'s — see its
 * header for why `"providers:"` is on the short-TTL list. What follows is the STORE itself. */
const __c = new Map<string, { v: any; exp: number }>(); // kvKey -> { v, exp }; v may be null (cached "not found")
export function cget(k: string): any {
  const e = __c.get(k);
  if (!e) return undefined;
  if (e.exp <= Date.now()) {
    __c.delete(k);
    return undefined;
  }
  return e.v;
}
export function cset(k: string, v: any): void {
  // The bound is the emission's rule; WHICH entry goes (the oldest, by insertion order) is this `Map`'s.
  if (cacheAtBound(__c.size)) __c.delete(__c.keys().next().value!);
  __c.set(k, { v, exp: Date.now() + ttlFor(k) });
}
export function cdel(...ks: string[]): void {
  for (const k of ks) __c.delete(k);
}
/** Test hook: wipe the module-level 24h caches (settings/route/keys). Never
 *  called in production — tests that flip global settings (e.g. US_PROXY)
 *  would otherwise read a stale cached value from an earlier test. */
export function __clearCaches(): void {
  __c.clear();
}

// Per-key single-flight queue: concurrent read-modify-write on the same KV
// blob (ukeys:<id>, devices:v1, plugins:v1) previously LOST updates — two
// requests both read the pre-write value and the second put clobbered the
// first's record. Serializing per key makes same-isolate writes atomic
// (Workers isolates are single-threaded; the queue bridges the awaits).
// round-122: settled chains are pruned opportunistically — the old size
// bound deleted the OLDEST key (always a hot key like devices:v1 whose
// chain could be pending), letting a new caller run CONCURRENTLY with the
// queued writers (the exact lost-update the queue prevents). Evict only
// chains that are no longer in flight: a chain is settled once it resolves,
// so wrap each stored promise to self-prune on completion.
const __locks = new Map<string, Promise<any>>();
export function withKeyLock<T>(key: string, fn: () => Promise<T>): Promise<T> {
  const prev = __locks.get(key) || Promise.resolve();
  const next = prev.then(fn, fn);
  // Self-prune on settle (round-124: the first attempt compared against
  // `next` but stored the .finally() wrapper — a DIFFERENT object, so the
  // prune never fired and every key accumulated until the 512 bound evicted
  // the oldest (hot, possibly-pending) chain). Capture the stored wrapper
  // and compare against it.
  const stored = next
    .catch(() => {})
    .finally(() => {
      if (__locks.get(key) === stored) __locks.delete(key);
    });
  __locks.set(key, stored);
  // Hard bound for pathological distinct-key bursts: evict the oldest key,
  // accepting that a pending chain there loses its queue (its own RMW still
  // completes; only a NEW caller for that key can now race it). Far rarer
  // than the old always-hit-hot-key eviction.
  if (__locks.size > LOCK_MAX_ENTRIES) __locks.delete(__locks.keys().next().value!);
  return next;
}
