/**
 * gate.ts — the `WASM_GATE` binding, and nothing else.
 *
 * **WHAT THIS FILE WAS (2026-10-17).** It held `decide`, the client half of `POST /__decision` — the mechanism
 * that let the TypeScript twin ask the Rust worker for a decision by name while the migration was in flight.
 * Its last caller was `store/providers.ts`, which went with the model-store cluster (no TypeScript module
 * imports that cluster any more), so the client, its GENERATED twin (`gate-logic.ts`) and the route behind
 * them retire together. **THE DECISIONS DID NOT GO WITH THEM**: they are in-process functions in
 * `gateway/wasm/src/decisions.rs`, called by the routes that need them — which is what the route was a
 * workaround for.
 *
 * `wasmGate` is why the file survives: the binding's shape is a platform question, and a gate that is absent
 * and a gate that fails are the same answer to every caller.
 */

export function wasmGate(env: any): { fetch: (r: Request) => Promise<Response> } | null {
  const binding = env?.WASM_GATE;
  return binding && typeof binding.fetch === "function" ? binding : null;
}
