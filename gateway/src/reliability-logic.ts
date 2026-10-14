/**
 * reliability-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/reliability.rs` (`render_typescript`). The freshness test in that module is a
 * byte comparison against this file:
 *
 *     SUMMRISE_REFRESH_RELIABILITY=1 cargo test --manifest-path gateway/wasm/Cargo.toml reliability::emission
 *
 * `gateway/src/reliability.ts` re-exports this module and keeps the HOST: the `BreakerDO` Durable Object, the
 * two dials, the retry LOOP and the isolate cache the breaker's reads go through. **THE RULES ARE HERE** — the
 * two budgets, the per-kind policy, what may be retried, what the loop does with one response, how long it
 * waits, and which failures open the circuit.
 *
 * Two of these values are CLIENT-VISIBLE as text: the retry details ("upstream 502 (retried 2/4)") are what a
 * caller prints when there is no response to normalize, so their spelling is part of the port.
 */

/** The OpenRouter `:free` model whose shared pool is won by rapid knocks. */
export const GLM_LOTTERY_MODEL = "z-ai/glm-5.2:free";

/** The default upstream budget — 30 s. */
export const UPSTREAM_TIMEOUT_MS = 30000;

/** og's budget — 120 s, and it gates TIME-TO-HEADERS only. */
export const OG_TIMEOUT_MS = 120000;

/** The retry loop's own defaults, used when a policy arm leaves a key absent. */
export const DEFAULT_ATTEMPTS = 3;
export const DEFAULT_BACKOFF_MS = 750;
export const DEFAULT_MAX_WAIT_MS = 10000;

/** `retryPolicyFor(kind, upstreamModel, timeoutMs)` — **THREE ARMS, AND AN ABSENT KEY IS NOT A FALSE ONE**:
 *  each arm is an object literal, so `attempts`/`backoffMs`/`retry502`/`ignoreRetryAfter` are present or
 *  missing per arm, and the loop's defaults fill only what is missing. */
export function retryPolicyFor(
  kind: string,
  upstreamModel: string,
  timeoutMs: number,
): {
  timeoutMs: number;
  attempts?: number;
  backoffMs?: number;
  retry502?: boolean;
  ignoreRetryAfter?: boolean;
} {
  if (kind === "nvidia" || kind === "gmi") {
    return { timeoutMs, attempts: 4, retry502: true };
  }
  if (kind === "openrouter") {
    return upstreamModel === GLM_LOTTERY_MODEL
      ? { timeoutMs, attempts: 10, backoffMs: 300, retry502: true, ignoreRetryAfter: true }
      : { timeoutMs, attempts: 4, retry502: true };
  }
  return { timeoutMs };
}

/** May this status be retried? A 502/503 needs the request to be idempotent OR the policy to say so; every
 *  other status needs the request to be idempotent. */
export function mayRetry5xx(status: number, idempotent: boolean, retry502: boolean): boolean {
  return status === 502 || status === 503 ? idempotent || retry502 : idempotent;
}

/** What the loop does with ONE response, in the source's own order — and the ORDER is the behaviour.
 *
 *  `ok || !(status >= 500 || status === 429)` stops on everything the caller must see (a 4xx that is not 429,
 *  and every 2xx); then the retryable branch (429 always, 5xx per `mayRetry5xx`); then the billing guard's
 *  detail for a 5xx that may have been processed already. **`detail` IS CLIENT-VISIBLE**: the arms put it in
 *  the error body when there is no response to normalize. */
export function retryDecision(
  status: number,
  attempt: number,
  attempts: number,
  idempotent: boolean,
  retry502: boolean,
): { retry: boolean; detail: string } {
  const ok = status >= 200 && status <= 299;
  if (ok || !(status >= 500 || status === 429)) {
    return { retry: false, detail: "" };
  }
  if (status === 429 || (status >= 500 && mayRetry5xx(status, idempotent, retry502))) {
    const detail = `upstream ${status} (retried ${attempt}/${attempts})`;
    return { retry: attempt < attempts, detail };
  }
  if (attempt >= attempts) {
    return { retry: false, detail: `upstream ${status} (retried ${attempt}/${attempts})` };
  }
  return { retry: false, detail: `upstream ${status} (not retried — POST may have been billed)` };
}

/** The wait between attempts — **with the clock's two inputs as ARGUMENTS**: the jitter the caller drew
 *  (`Math.floor(Math.random() * 200)`) and the upstream's `Retry-After`, read off the response the loop just
 *  got.
 *
 *  `Number(header)` IS A COERCION: an absent header is `NaN`, and `Number.isFinite(ra) && ra > 0` then refuses
 *  it — so a header of `"0"`, `"-5"` or `"later"` all fall back to the ladder. Everything is clamped to
 *  `maxWaitMs` so a huge header cannot stall the request. */
export function retryWaitMs(
  ignoreRetryAfter: boolean,
  retryAfter: string | null | undefined,
  backoffMs: number,
  attempt: number,
  jitter: number,
  maxWaitMs: number,
): number {
  if (ignoreRetryAfter) return Math.min(backoffMs + jitter, maxWaitMs);
  const ra = Number(retryAfter);
  const raMs = Number.isFinite(ra) && ra > 0 ? ra * 1000 : 0;
  return Math.min(Math.max(raMs, backoffMs * attempt) + jitter, maxWaitMs);
}

/** The shared body of the two budget readers: `Number.isFinite(v) && v > 0 ? v : DEFAULT`.
 *
 *  **THE ORDER IS THE SEMANTICS**: `Number.isFinite` refuses `Infinity` and `NaN` alike, and `v > 0` refuses 0
 *  and the negatives — so `""`, `null`, `"abc"`, `Infinity`, `"0"` and `"-5"` ALL take the default, while
 *  `"  12  "` and `"0x10"` do not. The answer is a NUMBER, not a count: `1500.7` survives. */
export function timeoutFrom(env: any, key: string, defaultMs: number): number {
  const v = Number(env?.[key]);
  return Number.isFinite(v) && v > 0 ? v : defaultMs;
}

/** The generic 30 s budget, `UPSTREAM_TIMEOUT_MS` over it. */
export function upstreamTimeoutMs(env: any): number {
  return timeoutFrom(env, "UPSTREAM_TIMEOUT_MS", UPSTREAM_TIMEOUT_MS);
}

/** og's 120 s budget — zen's latency intermittently spikes past 30 s and its real max-thinking requests run
 *  40–54 s to first byte. */
export function ogTimeoutMs(env: any): number {
  return timeoutFrom(env, "OG_TIMEOUT_MS", OG_TIMEOUT_MS);
}

/** Which budget a passthrough channel gets: zen itself, the gateway with the same long-thinking profile, and
 *  the cold-starting self-deploy router share og's; everything else keeps the generic one. */
export function passthroughTimeoutMs(env: any, kind: string): number {
  return kind === "opencode" || kind === "commandgoat" || kind === "amd"
    ? ogTimeoutMs(env)
    : upstreamTimeoutMs(env);
}

/** `isChannelDownFailure(detail)` — the circuit breaker's input. **HARD network errors and FULL TIMEOUTS both
 *  count**, because a channel that stalls and times out every time must open the circuit or every user burns
 *  the 120 s timeout indefinitely. Fast 5xx/429 do NOT count (the retry layer absorbs those), and neither do
 *  short latency spikes. */
export function isChannelDownFailure(detail: string | null | undefined): boolean {
  return !!detail && (detail.startsWith("network error") || detail.startsWith("timeout"));
}
