/**
 * decisions.ts — the TypeScript half of the DECISION ROUTE (`gateway/wasm/src/decisions.rs`).
 *
 * The Rust worker exposes `POST /__decision` through the service binding that already exists, so a PURE decision
 * can be ported one row at a time while its route stays where it is. See
 * `docs/superpowers/specs/2026-10-10-the-console-workers-tail-design.md` for why that is the second way to move a
 * decision and what it costs (one subrequest per call).
 *
 * **`undefined` IS NOT `null`.** `null` is a decision that ran and answered null; `undefined` is a gate that is
 * absent, refused, or unreachable — and the caller must fall back to its TypeScript twin for THAT case only.
 * Collapsing the two would let a wrong `null` from Rust be silently corrected by the twin, which is the one failure
 * this route must not hide.
 */

import { wasmGate } from "./gate.ts";

/** The route's own address. A service binding ignores the host, so this is a placeholder that cannot resolve. */
const DECISION_URL = "https://gate.invalid/__decision";

/**
 * Ask the Rust worker for one decision. Returns the decision's value, or `undefined` when the gate did not answer.
 */
export async function decide<T>(env: any, name: string, args: unknown): Promise<T | undefined> {
  const gate = wasmGate(env);
  if (!gate) return undefined;
  try {
    const res = await gate.fetch(
      new Request(DECISION_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name, args }),
      }),
    );
    if (!res.ok) return undefined;
    const body: any = await res.json();
    return body?.ok === true ? (body.value as T) : undefined;
  } catch {
    // A gate that is unreachable is the same answer as a gate that is absent: use the twin.
    return undefined;
  }
}
