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
 *
 * ── AND ITS DECISIONS ARE EMITTED FROM RUST (2026-10-13) ───────────────────
 *
 * The file stays — the binding and the `fetch` that may throw are the platform's — and what it DECIDES moved: the
 * path, the address, the content type, the `{name, args}` envelope and the answer's parse rule are
 * `gateway/wasm/src/decision_client.rs`, reaching this file as `gate-logic.ts` (GENERATED, and the freshness test is
 * a byte comparison). **AND THE PATH IS NOW ONE NAME FOR BOTH SIDES**: the client spelled it inside `DECISION_URL`
 * while `decisions.rs`'s `in_family` spelled it again, and the server now READS the constant — one wire, one name,
 * the repair `x-do-auth` needed one landing over.
 */

import { DECISION_CONTENT_TYPE, DECISION_URL, decisionBody, decisionValue } from "./gate-logic.ts";

// The emitted decisions, re-exported so `from "./gate.ts"` keeps working for callers and tests.
export * from "./gate-logic.ts";

/** The service binding, or `null` when this deployment has none. */
export function wasmGate(env: any): { fetch: (r: Request) => Promise<Response> } | null {
  const binding = env?.WASM_GATE;
  return binding && typeof binding.fetch === "function" ? binding : null;
}

/**
 * Ask the Rust worker for one decision. `gateway/wasm/src/decisions.rs` is the server half; the wire — the path, the
 * envelope and the parse rule — is `gate-logic.ts`.
 *
 * **`undefined` IS NOT `null`**, and the rule now lives in `decisionValue` with its reasoning: `null` is a decision
 * that ran and answered null, `undefined` is a gate that is absent, refused, or unreachable — and the caller falls
 * back to its TypeScript twin for THAT case only. **Collapsing the two would let a wrong `null` from Rust be silently
 * corrected by the twin**, which is the one failure this route must not hide: the twin keeps the product working when
 * the gate is gone, it does not make a wrong answer look right.
 *
 * **THERE IS NO PRESENCE CHECK, AND THE `try` IS WHY**: a gate that is absent and a gate that fails are the same
 * answer here, so `env?.WASM_GATE?.fetch(…)` throwing into the catch covers both.
 */
export async function decide<T>(env: any, name: string, args: unknown): Promise<T | undefined> {
  try {
    const res = await env?.WASM_GATE?.fetch(
      new Request(DECISION_URL, {
        method: "POST",
        headers: { "Content-Type": DECISION_CONTENT_TYPE },
        body: decisionBody(name, args),
      }),
    );
    // A binding that is not there answers `undefined` here, which is the same answer as one that threw: the parse
    // rule never sees it, because there is no status to give it.
    if (!res) return undefined;
    return decisionValue<T>(res.ok, await res.json());
  } catch {
    return undefined;
  }
}
