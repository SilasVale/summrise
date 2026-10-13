/**
 * ratelimit — the single per-IP in-memory rate limiter (structure refactor:
 * three verbatim copies of the same algorithm collapsed into one factory).
 *
 * Policy is a parameter because the three historical call sites differ in
 * ONE deliberate dimension — whether the counter is KV-SEEDED:
 *   - kvSeed: true   (probe) — each bucket's first sight per IP reads KV
 *     once and persists the count, so a new isolate inherits the budget.
 *     Costs 1 read + 1 write on that first call only (KV quota).
 *   - kvSeed: false  (auth register, devices public gate) — memory-only.
 *     round-104: these endpoints cost 2-3 KV writes per attempt themselves;
 *     a per-request KV write HERE would let an attacker exhaust the
 *     Free-plan daily KV write quota. The per-isolate ceiling is the
 *     accepted trade (documented, not a bug).
 *     THE DEVICES GATE IS NOT THIS FILE'S CALLER ANY MORE: the cutover moved the
 *     family to `gateway/wasm/src/ip_rate_limit.rs`, where memory-only is not a
 *     flag but the ABSENCE of a KV field on `IpRateLimiter` — the KV arms are
 *     unreachable in that mode, so the port left them out. The probe and auth
 *     register call sites are still here, and `test/ratelimit-kvseed.test.mjs`
 *     reads all three (two as option literals, one as the Rust struct's fields).
 *
 * Semantics preserved verbatim from the three originals:
 *   - per-IP (cf-connecting-ip) per fixed window bucket, in-memory
 *     `name:${ip}:${bucket}` → count, per-isolate (never shared),
 *   - 4096-key capacity cap with insertion-order eviction,
 *   - fail-open on any internal error (same posture as the breaker).
 *
 * ── AND ITS RULES ARE EMITTED FROM RUST (2026-10-13) ───────────────────────
 *
 * The bucket, the counter key, the budget test, the eviction bound, the persist TTL, the seeding predicate and the
 * fail-open VERDICT are `gateway/wasm/src/ip_rate_limit.rs`, reaching this file as `ratelimit-logic.ts` (GENERATED,
 * and the freshness test is a byte comparison). **AND THE `"unknown"` FALLBACK IS ONE NAME NOW** where the literal
 * was spelled three times — twice in Rust (`auth_routes.rs`, `devices.rs`) and once here — which is the third wire
 * this migration has found written twice in two languages. What is left here is the HOST: the per-isolate `Map`,
 * the KV read and write, and the `try` that makes a KV failure best-effort rather than fatal.
 */

import {
  FAIL_OPEN_VERDICT,
  UNKNOWN_IP,
  bucketOf,
  counterKey,
  needsEviction,
  overBudget,
  seedTtlSecs,
  seededCount,
  shouldSeed,
} from "./ratelimit-logic.ts";

// The emitted rules, re-exported so `from "./lib/ratelimit.ts"` keeps working for callers and tests.
export * from "./ratelimit-logic.ts";

export interface IpRateLimiterOptions {
  /** Human-readable bucket prefix (e.g. "probe-rate", "auth-rate"). */
  name: string;
  /** Max requests per window per IP. */
  limit: number;
  /** Window length in ms. */
  windowMs: number;
  /** Seed the counter from KV so new isolates inherit the budget. */
  kvSeed?: boolean;
}

export interface IpRateLimiter {
  /** True when the caller is over budget for this window. */
  (request: Request, env?: any): Promise<boolean>;
  /** The KV key prefix (test/debug introspection). */
  readonly keyPrefix: string;
}

export function createIpRateLimiter(opts: IpRateLimiterOptions): IpRateLimiter {
  const { name, limit, windowMs, kvSeed = false } = opts;
  const counters = new Map<string, number>(); // `${ip}:${bucket}` → count

  const limiter = async (request: Request, env?: any): Promise<boolean> => {
    try {
      const ip = request?.headers?.get?.("cf-connecting-ip") || UNKNOWN_IP;
      const key = counterKey(name, ip, bucketOf(Date.now(), windowMs));
      const hit = counters.get(key);
      if (hit !== undefined) {
        if (overBudget(hit, limit)) return true;
        counters.set(key, hit + 1);
        return false;
      }
      let cur = 0;
      if (shouldSeed(kvSeed, !!env?.KEYS)) {
        try {
          cur = seededCount(await env.KEYS.get(key));
        } catch {
          /* KV read failed — isolate-local count from zero */
        }
      }
      counters.set(key, cur + 1);
      if (needsEviction(counters.size)) {
        const oldest = counters.keys().next().value;
        if (oldest !== undefined) counters.delete(oldest);
      }
      if (shouldSeed(kvSeed, !!env?.KEYS)) {
        try {
          await env.KEYS.put(key, String(cur + 1), {
            expirationTtl: seedTtlSecs(windowMs),
          });
        } catch {
          /* KV write error: isolate-local count still better than nothing */
        }
      }
      return overBudget(cur, limit);
    } catch {
      return FAIL_OPEN_VERDICT;
    } // fail-open on internal errors, like the breaker
  };
  return Object.assign(limiter, { keyPrefix: name });
}
