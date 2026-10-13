/**
 * ratelimit-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/ip_rate_limit.rs` (`render_typescript`), which is now the single source. The
 * freshness test in that module is a byte comparison against this file, so a hand edit fails there with the text and
 * one command to fix it:
 *
 *     SUMMRISE_REFRESH_RATELIMIT=1 cargo test --manifest-path gateway/wasm/Cargo.toml ip_rate_limit::emission
 *
 * `gateway/src/lib/ratelimit.ts` re-exports this module and keeps the HOST: the per-isolate `Map`, the KV read and
 * write, and the `try` that makes a KV failure best-effort. **The RULES are here** — the bucket, the key, the budget
 * test, the eviction bound, the persist TTL, and the fail-open verdict — and `UNKNOWN_IP` is one name now where the
 * literal was written three times (twice in Rust, once in TypeScript).
 */

/** `cf-connecting-ip`, or this literal when the header is absent. */
export const UNKNOWN_IP = "unknown";

/** The per-isolate table's bound: exceeding it evicts the OLDEST key (insertion order), on the fresh-key arm only. */
export const MAX_COUNTERS = 4096;

/** The posture on an INTERNAL error: NOT over budget — **FAIL OPEN**, the posture the breaker takes too. */
export const FAIL_OPEN_VERDICT = false;

/** `Math.floor(Date.now() / windowMs)` — the fixed window a request counts against. */
export function bucketOf(nowMs: number, windowMs: number): number {
  return Math.floor(nowMs / windowMs);
}

/** `${name}:${ip}:${bucket}`. */
export function counterKey(name: string, ip: string, bucket: number): string {
  return `${name}:${ip}:${bucket}`;
}

/** The budget test. **THE ASYMMETRY BETWEEN THE ARMS IS THE SOURCE'S**: an existing counter already AT the limit is
 *  refused WITHOUT incrementing, while a fresh key inserts `cur + 1` and answers `cur >= limit` — always false on a
 *  bucket's first hit in memory-only mode. */
export function overBudget(count: number, limit: number): boolean {
  return count >= limit;
}

/** The eviction test, on the same arm that can grow the table. */
export function needsEviction(size: number): boolean {
  return size > MAX_COUNTERS;
}

/** `Math.ceil((windowMs / 1000) * 2) + 10` — the TTL on the persist write. For the probe's 60 000 ms window that is
 *  **130 seconds**: the bucket plus its successor, plus ten, so a key that outlives its window cannot be inherited
 *  by the next one. */
export function seedTtlSecs(windowMs: number): number {
  return Math.ceil((windowMs / 1000) * 2) + 10;
}

/** Seeding needs BOTH the option and a store bound. */
export function shouldSeed(kvSeed: boolean, hasKeys: boolean): boolean {
  return kvSeed && hasKeys;
}

/** A persisted count read back: a missing or non-numeric value is ZERO, which is the source's `Number(v) || 0`. */
export function seededCount(raw: unknown): number {
  return Number(raw) || 0;
}
