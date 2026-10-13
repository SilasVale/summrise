/**
 * store.ts — KV persistence layer (migrated from store.js, logic verbatim)
 *
 * Multi-user BYOK relay:
 *   user:<id>          → user record {id, username, role, enabled, createdAt, passwordHash, salt, token}
 *   username:<name>    → userId (uniqueness + fast lookup)  [kept for compat; not used]
 *   token:<gatewayToken> → userId (client x-api-key resolution)
 *   ukeys:<id>         → that user's own backend keys {DEEPSEEK_API_KEY, OPENCODE_GO_API_KEY, OPENROUTER_API_KEY}
 *   invite:<code>      → "1" (one-time registration invite)
 *   _admin_seeded      → "1" (admin seeded marker)
 *
 * Compat migration: the admin account is seeded from the existing CLIENT_KEY (gateway
 * token) + backend keys, so the user's local settings.json (x-api-key = CLIENT_KEY)
 * keeps working without changes.
 *
 * Split by domain (structure refactor): the implementation now lives in
 * src/store/<domain>.ts and this module is a pure re-export shim, so every
 * existing `from "../store.ts"` (or "./store.ts") import keeps working.
 * Dependency order is one-way (cache → entities → aggregates):
 *   store/cache.ts     per-isolate KV cache + per-key write locks — the
 *                      SINGLE process-global instance; all siblings import
 *                      cget/cset/cdel from it, never their own copies
 *   store/users.ts     users / tokens / per-user keys / RouteDO routes /
 *                      invites / masking
 *   store/admin.ts     admin seeding (process-once `seeded` flag) + hashed
 *                      admin password
 *   store/settings.ts  global settings (console toggles, e.g. US_PROXY)
 *   store/devices.ts   device registry (devices:v1) + CF tunnel API token
 *
 * **`store/regkeys.ts` LEFT ON 2026-10-13, AND IT WAS DELETED RATHER THAN RE-POINTED.** The registration keys
 * (`regkey:` / `reggrant:`) are the Rust worker's now (`gateway/wasm/src/device_store.rs`, over
 * `device_registry::reg_key_rows`), the corpus drives every route that writes them — `POST /api/devices/register-key`,
 * `GET /api/devices/register-keys`, both `DELETE`s, and the four `POST /api/register` cases including *"an UPPERCASE
 * key (the key is lowercased before the claim)"* — and **not one of its exported names had a consumer outside this
 * barrel**: `store.ts`'s blanket `export *` made the file look imported while nothing called it. That is the same
 * measurement that found `grants.ts`, and the same rule — *a barrel is structure, and a file nothing reaches is not
 * work that is left.*
 *
 * **AND `store/plugins.ts` IS THE SAME KIND OF DEAD AND DID NOT GO WITH IT** — three of its arms pin SAME-ISOLATE
 * behaviour that the corpus cannot witness, because it runs a fresh module instance per case. The reason it still
 * counts as LOGIC, and the landing that will retire it, are in the manifest's own rule for the file.
 */
export * from "./store/cache.ts";
export * from "./store/users.ts";
export * from "./store/admin.ts";
export * from "./store/settings.ts";
export * from "./store/devices.ts";
export * from "./store/plugins.ts";
