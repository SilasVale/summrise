/**
 * gate.ts — the WASM_GATE binding, in ONE place.
 *
 * The predicate was private to `index.ts` until the decision route needed it too. **A second copy is how two
 * callers drift on what "the gate is available" means**, and this one decides whether a request is served by
 * Rust or by the TypeScript twin — so it is worth exactly one definition.
 */

/** The service binding, or `null` when this deployment has none. */
export function wasmGate(env: any): { fetch: (r: Request) => Promise<Response> } | null {
  const binding = env?.WASM_GATE;
  return binding && typeof binding.fetch === "function" ? binding : null;
}
