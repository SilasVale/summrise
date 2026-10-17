// The last two surface modules with TypeScript LOGIC: `gate.ts`'s decision client and two `store/` helpers.
//
// **WHAT THIS FILE USED TO BE, AND WHY IT IS SMALL (2026-10-17).** It was `plugins.test.mjs` — 37 cases over the
// plugin registry, the admin/auth plugins and the routes they served. Every one of those routes is the Rust
// worker's now and every plugin is deleted, so the cases left with their subjects (the corpora in
// `gateway/wasm/verify.mjs` are where those routes are proved case for case), and what remained was the
// decision client plus two store helpers that were only ever here by proximity. The NAME went with the plugins:
// `plugins.test.mjs` described a framework that no longer exists.
//
// **`decide` IS STILL LIVE**: `store/providers.ts` asks it for the `public_provider` decision — the LAST row in
// `decisions.rs`'s table, and the one whose departure empties it.
import test from "node:test";
import assert from "node:assert/strict";
import { decide } from "../src/gate.ts";
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
test("decide: a decision that ran is returned, and null is a VALUE", async () => {
  const env = { WASM_GATE: { fetch: async () => new Response(JSON.stringify({ ok: true, value: null })) } };
  const answeredNull = await decide(env, "upstream_models_url", { upstream: "https://x/v1/messages" });
  assert.equal(answeredNull, null);
  assert.notEqual(answeredNull, undefined);
});

test("decide: no gate, a refusal and an unreachable gate are all undefined", async () => {
  assert.equal(await decide({}, "upstream_models_url", {}), undefined);
  assert.equal(
    await decide(
      { WASM_GATE: { fetch: async () => new Response(JSON.stringify({ ok: false }), { status: 400 }) } },
      "x",
      {},
    ),
    undefined,
  );
  assert.equal(
    await decide(
      {
        WASM_GATE: {
          fetch: async () => {
            throw new Error("binding unreachable");
          },
        },
      },
      "upstream_models_url",
      {},
    ),
    undefined,
  );
});

test("decide: POST to the decision path, with the name and args as JSON", async () => {
  let seen = null;
  const env = {
    WASM_GATE: {
      fetch: async (request) => {
        seen = { method: request.method, path: new URL(request.url).pathname, body: await request.json() };
        return new Response(JSON.stringify({ ok: true, value: 1 }));
      },
    },
  };
  await decide(env, "offered_ids", { payload: ["a"] });
  assert.equal(seen.method, "POST");
  assert.equal(seen.path, "/__decision");
  assert.deepEqual(seen.body, { name: "offered_ids", args: { payload: ["a"] } });
});
