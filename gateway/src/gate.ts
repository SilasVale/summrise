/**
 * gate.ts — the WASM_GATE binding, and the DECISION ROUTE's client.
 *
 * ── WHY THIS FILE EXISTS, AND WHY THE CAP WENT UP FOR IT ───────────────────
 *
 * `decide` was written inside `plugins/models-probe.ts` because that was its only consumer, with a note saying that
 * when a SECOND plugin needed it, it would move here and **the cap would get a deliberate entry with the reason.**
 * `plugins/auth.ts` needs it now (the three usage mappers), so this is that entry: `MAX_LOGIC` 69 -> 70.
 *
 * **THE ALTERNATIVE WAS TO PROTECT THE NUMBER, AND IT WAS WORSE.** A plugin importing from another plugin is a
 * layering inversion, and hiding a gate client in `http.ts` to avoid a file would be a number kept at the cost of
 * the reader. **A ratchet that can only be raised in the open is doing its job; one that is worked around is not.**
 *
 * `wasmGate` is here for the same reason: it was private to `index.ts` and now has two callers.
 */

/** The service binding, or `null` when this deployment has none. */
export function wasmGate(env: any): { fetch: (r: Request) => Promise<Response> } | null {
  const binding = env?.WASM_GATE;
  return binding && typeof binding.fetch === "function" ? binding : null;
}

/** The route's own address. A service binding ignores the host, so this is a placeholder that cannot resolve. */
const DECISION_URL = "https://gate.invalid/__decision";

/**
 * Ask the Rust worker for one decision. `gateway/wasm/src/decisions.rs` is the server half.
 *
 * **`undefined` IS NOT `null`.** `null` is a decision that ran and answered null; `undefined` is a gate that is
 * absent, refused, or unreachable — and the caller falls back to its TypeScript twin for THAT case only.
 * **Collapsing the two would let a wrong `null` from Rust be silently corrected by the twin**, which is the one
 * failure this route must not hide: the twin keeps the product working when the gate is gone, it does not make a
 * wrong answer look right.
 *
 * **THERE IS NO PRESENCE CHECK, AND THE `try` IS WHY**: a gate that is absent and a gate that fails are the same
 * answer here, so `env?.WASM_GATE?.fetch(…)` throwing into the catch covers both.
 */
export async function decide<T>(env: any, name: string, args: unknown): Promise<T | undefined> {
  try {
    const res = await env?.WASM_GATE?.fetch(
      new Request(DECISION_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name, args }),
      }),
    );
    if (!res?.ok) return undefined;
    const body: any = await res.json();
    return body?.ok === true ? (body.value as T) : undefined;
  } catch {
    return undefined;
  }
}
