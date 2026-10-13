/**
 * gate-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/decision_client.rs` (`render_typescript`), which is now the single source. The
 * freshness test in that module is a byte comparison against this file, so a hand edit fails there with the text and
 * one command to fix it:
 *
 *     SUMMRISE_REFRESH_DECISION_CLIENT=1 cargo test --manifest-path gateway/wasm/Cargo.toml decision_client::emission
 *
 * `gateway/src/gate.ts` re-exports this module and keeps the BINDING: `wasmGate` and the `fetch` that may throw.
 * `gateway/wasm/src/decisions.rs` is the server half, and it reads `DECISION_PATH` from here — one wire, one name.
 */

/** The route's own path. A service binding ignores the host, and the server refuses anything but `POST` on exactly
 *  this path (`decisions::in_family`), so a prefix would let `/__decision_x` reach the table. */
export const DECISION_PATH = "/__decision";

/** The route's own address. A service binding ignores the host, so this is a placeholder that cannot resolve. */
export const DECISION_URL = "https://gate.invalid/__decision";

/** The one header the request carries. */
export const DECISION_CONTENT_TYPE = "application/json";

/** The request body — `{name, args}`, the envelope the server's `handle` parses. */
export function decisionBody(name: string, args: unknown): string {
  return JSON.stringify({ name, args });
}

/**
 * The answer's parse rule. **`undefined` IS NOT `null`.** `null` is a decision that ran and answered null;
 * `undefined` is a gate that is absent, refused, or unreachable — and the caller falls back to its TypeScript twin
 * for THAT case only. **Collapsing the two would let a wrong `null` from Rust be silently corrected by the twin**,
 * which is the one failure this route must not hide: the twin keeps the product working when the gate is gone, it
 * does not make a wrong answer look right.
 *
 * A body with no `value` key is the FIRST case, not the second: the source reads `body.value`, which for a missing
 * key is `undefined`.
 */
export function decisionValue<T>(statusOk: boolean, body: any): T | undefined {
  if (!statusOk) return undefined;
  if (body?.ok !== true) return undefined;
  return body.value as T | undefined;
}
