// The last two surface modules with TypeScript LOGIC: `gate.ts`'s decision client and two `store/` helpers.
//
// **WHAT THIS FILE USED TO BE, AND WHY IT IS SMALL (2026-10-17).** It was `plugins.test.mjs` — 37 cases over the
// plugin registry, the admin/auth plugins and the routes they served. Every one of those routes is the Rust
// worker's now and every plugin is deleted, so the cases left with their subjects (the corpora in
// `gateway/wasm/verify.mjs` are where those routes are proved case for case), and what remained was the
// decision client plus two store helpers that were only ever here by proximity. The NAME went with the plugins:
// `plugins.test.mjs` described a framework that no longer exists.
//
// **`decide` IS GONE (2026-10-17), SO THE THREE CASES THAT PINNED IT WENT WITH IT** — and so did the route, the
// table and the GENERATED client (`gateway/wasm/src/decision_client.rs`, `decisions.rs`'s dispatch,
// `gate-logic.ts`). Its last caller was `store/providers.ts`, deleted with the model-store cluster; the decisions
// themselves are in-process functions in `decisions.rs` now. What is left here is `getUser` and
// `getAdminPassword`, two `store/` helpers — and the file's name is now half wrong, which is noted rather than
// renamed mid-landing: `gate.ts` holds only `wasmGate`, and nothing in this file touches it.
import test from "node:test";
import assert from "node:assert/strict";
import { __clearCaches } from "../src/store.ts";
import { makeEnv as makeBaseEnv } from "./helpers.mjs";

test("getUser: corrupt user JSON → null (no throw)", async () => {
  __clearCaches();
  const { getUser } = await import("../src/store/users.ts");
  const env = { KEYS: { get: async () => "{corrupt" } };
  assert.equal(await getUser(env, "ghost"), null);
});

// round-525 (coverage-driven): the ADMIN_PASSWORD migration arm had ZERO pins —
// a Worker-secret password with no KV hash must migrate hashed, never plaintext.
/** The env `getAdminPassword`'s migration case needs: a seeded admin password hash, and NO `WASM_GATE`
 *  binding, which **USED TO MEAN "THE ROLLBACK" AND NOW MEANS NOTHING** — the plugins that read it are deleted,
 *  and no case in this file makes a request at all. It is a KV seed, not a routing decision. */
function meEnv() {
  return makeBaseEnv({
    wasmGate: false,
    users: {
      admin: { id: "admin", username: "admin", role: "admin", enabled: true, token: "" },
      bob: { id: "bob", username: "bob", role: "user", enabled: true, token: "bob-tok-1" },
    },
    kv: { _admin_seeded: "1", "auth:admin_password": "pw", "token:bob-tok-1": "bob" },
  });
}

test("getAdminPassword: migrates ADMIN_PASSWORD to a hashed KV record", async () => {
  __clearCaches();
  const { getAdminPassword } = await import("../src/store/admin.ts");
  const env = meEnv();
  await env.KEYS.delete("auth:admin_password");
  env.ADMIN_PASSWORD = "migpw";
  const v = await getAdminPassword(env);
  assert.match(v, /^legacy:[0-9a-f]+$/);
  assert.equal(await env.KEYS.get("auth:admin_password"), v, "migrated hash persisted");
  assert.ok(
    !String(await env.KEYS.get("auth:admin_password")).includes("migpw"),
    "never plaintext",
  );
});

// round-444 (coverage-driven): DELETE /api/me/keys had ZERO route pins
// (store-level deleteUserKey covered, handler not).
