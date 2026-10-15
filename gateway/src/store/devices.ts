/**
 * store/devices.ts — the Summrise Agent device registry (devices:v1, admin-
 * managed) plus the account-level Cloudflare tunnel API token (cf:api_token)
 * the Windows install fetches with a registration key.
 */

// **THE WHOLE REGISTRY IS AN EMISSION NOW (2026-10-15)** — `devices-logic.ts`, GENERATED from
// `gateway/wasm/src/device_store.rs`, which serves the same registry for the device family's routes. The
// emitted functions take the SAME `env` this file's callers already pass (they are async KV flows, not a
// sequence needing the platform injected), so this file is a re-export and nothing else.
export * from "./devices-logic.ts";
