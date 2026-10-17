// Plugin REGISTRY tests — the route table, the stats counters, the plugin
// context and the admin/auth plugins. (The plugin-LINK store cases left with
// `store/plugins.ts` on 2026-10-13; their witnesses are named in the table
// below, and the links themselves still guard the device reverse proxy and are
// revoked on device delete/rename, which the corpus and the same-isolate
// sequences in `gateway/wasm/verify.mjs` now prove.)
import test from "node:test";
import assert from "node:assert/strict";
import worker from "../src/index.ts";
import { decide } from "../src/gate.ts";
import { __clearCaches, setAdminPassword, maskKey } from "../src/store.ts";
import { makeEnv as makeBaseEnv, freezeClock } from "./helpers.mjs";

// Full worker fetch: pair/claim + ws-ticket are public (no admin session) —
// the extension has no session cookie. Asserted by behavior, not source order.
// Shared Map-KV stub (helpers.mjs) seeded with this file's minimal base.
function makeEnv() {
  // NO `WASM_GATE` BINDING, ON PURPOSE: THE ROLLBACK — see `makeEnv` above.
  return makeBaseEnv({
    wasmGate: false,
    // **NO `WASM_GATE` BINDING, ON PURPOSE: THIS FILE TESTS THE TYPESCRIPT IMPLEMENTATION.** `index.ts` hands the
    // identity surface (`/api/auth/*` and the ported `/api/me*` routes) to the Rust worker when that binding is
    // present — the production configuration — so a test of these handlers is a test of the ROLLBACK path, which
    // is a real configuration and the one these assertions were always about. The cutover's own routing is
    // witnessed in `frontdoor.test.mjs` (the binding-less fallthrough) and just below (the one exclusion, with
    // the binding present); `auth-cutover.test.mjs` went with the code it tested (rule 1) and asserted the
    // UNCONDITIONAL handover this tree no longer has.
    wasmGate: false,
    users: { admin: { id: "admin", username: "admin", role: "admin", enabled: true, token: "" } },
    kv: { _admin_seeded: "1", "auth:admin_password": "pw" },
  });
}

async function apiFetch(env, path, init = {}) {
  const req = new Request(`https://x${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    ...init,
  });
  return worker.fetch(req, env);
}

// Bare stub for the store-helper tests (direct store.ts calls, no worker).
function env() {
  return makeBaseEnv({ wasmGate: false });
}

// ── THE EIGHT PLUGIN-LINK CASES LEFT WITH `store/plugins.ts` (2026-10-13), AND EACH ONE'S WITNESS IS NAMED ──────
//
// The module was DELETED, one landing after `store/regkeys.ts` and for the same measured reason: nothing under
// `gateway/src` or `gateway/test` called its exports but this file's own cases, and `store.ts`'s blanket `export *`
// made the file look imported while nothing used it. The subject is the Rust worker's now (`device_store.rs` over
// `device_registry.rs`). **IT WAITED ONE LANDING FOR A WITNESS, WHICH IS THE RULE THIS TABLE IS FOR** — three of the
// cases below pinned SAME-ISOLATE behaviour, and the frozen corpus cannot reach that (every case runs in a fresh
// module instance). Those witnesses were built, and PROVED ABLE TO FAIL by planted mutations, before the file left:
// `gateway/wasm/verify.mjs` carries three SAME-ISOLATE sequences beside the plugin-token cases.
//
//   get/remove (KV-seeded)                -> corpus `GET proxy with the paired plugin token — proxied, device Bearer
//                                            minted server-side`; for the removal half, corpus `GET proxy with a
//                                            ROTATED token — the old token is simply not in the map: 401`.
//                                            **`removePluginLink` (a SINGLE token) has no Rust counterpart and never
//                                            had a product caller**: this case was its only consumer in the whole
//                                            tree, so nothing was ported for it
//   expires after 30 days                 -> `device_registry::plugin_link_expired` + corpus `GET proxy with an
//                                            EXPIRED plugin link (revoked/rotated) — 401 and the record is swept`.
//                                            The 30-day VALUE rides the `Set-Cookie` `Max-Age` that the corpus
//                                            records byte-for-byte (`GET proxy navigation with ?token= …`)
//   revoke never resurrects (stale cache) -> SAME-ISOLATE sequence `the revoke reaches the SAME isolate` (proved:
//                                            dropping the write-through makes it read `200 -> delete 200 -> 200`)
//   expired get sweeps the KV record      -> the corpus case named above — its own name says the sweep
//   legacy record without expiresAt       -> `plugin_link_expired`'s unit (`plugin_links_migrate_and_revoke`: a
//                                            missing `expiresAt` IS expired)
//   corrupt blob reads as empty           -> `plugin_links_from_raw`'s unit
//                                            (`the_raw_reads_treat_corruption_the_way_the_source_does`) plus
//                                            `list_plugin_links`'s `unwrap_or_default`; the WRITE side of the same
//                                            question is the SAME-ISOLATE sequence `the delete's revoke ABORTS on a
//                                            corrupt blob`
//   savePluginLinks is write-through      -> the same `the revoke reaches the SAME isolate` sequence: the third
//                                            request reads THROUGH the worker, so a cached map surviving the write
//                                            would accept a revoked link
//   corrupt fresh read in the sweep lock  -> SAME-ISOLATE sequence `the corrupt re-read inside the sweep lock
//                                            answers 401 … and the record is untouched` (proved: an `expect` in
//                                            its place kills the harness on the panic)

// round-340: the public pair/claim + ws-ticket endpoint tests were
// removed with the extension pairing endpoints (browser extension deleted
// round-262). Store-helper tests below stay — handlePair (admin) still uses
// createPairCode / plugin links.
// round-88: /api/plugins/status is admin-session-gated — no cookie → 401,
// a copied pre-logout cookie (sess-revoked blacklist) → 401, a valid admin
// session → 200. The R83 hand-rolled gate must match requireSession.
import { issueSessionToken, SESSION_COOKIE } from "../src/auth.ts";

test("plugins/status: no cookie → 401 (R83 gate)", async () => {
  const env = makeEnv();
  const req = new Request("https://x/api/plugins/status", {
    method: "GET",
    headers: { "content-type": "application/json" },
  });
  const res = await worker.fetch(req, env);
  assert.equal(res.status, 401);
});

test("plugins/status: revoked cookie → 401 (R88 blacklist)", async () => {
  const env = makeEnv();
  // Blacklist a fake session cookie the way logout does (sess-revoked:<cookie>).
  await env.KEYS.put("sess-revoked:fake-cookie", "1", { expirationTtl: 3600 });
  const req = new Request("https://x/api/plugins/status", {
    method: "GET",
    headers: { "content-type": "application/json", cookie: "ag_session=fake-cookie" },
  });
  const res = await worker.fetch(req, env);
  assert.equal(res.status, 401);
});

test("plugins/status: valid admin session → 200 (R83 gate)", async () => {
  const env = makeEnv();
  const cookie = await issueSessionToken("pw", "admin", "admin");
  const req = new Request("https://x/api/plugins/status", {
    method: "GET",
    headers: { "content-type": "application/json", cookie: `ag_session=${cookie}` },
  });
  const res = await worker.fetch(req, env);
  assert.equal(res.status, 200);
});

// **THE EXCLUSION, IN THE CONFIGURATION WHERE HANDING THE ROUTE OVER WOULD SHOW.** `index.ts` says
// `GET /api/plugins/status` "therefore stays on the TypeScript path, and the harness's cutover section asserts
// that it does" — and the file that asserted it (`mcp-cutover.test.mjs`) went with the code it tested while the
// ROUTE survived. `makeEnv` above is the ROLLBACK, where a binding-less env reaches the plugin whatever the
// routing says, so it cannot witness the exclusion at all. This test builds the DEPLOYED shape instead
// (`helpers.mjs` stubs `WASM_GATE` and records what reaches it): the answer must come from the plugin AND the
// worker must never be asked. The `routes` field is the second half of the proof — it is `routeStats(ctx)`, a
// property of the TypeScript plugin table that the Rust worker could not produce — while the stub answers a
// model list, so the two answers cannot be confused.
test("plugins/status: the exclusion holds WITH the binding present (the worker is never asked)", async () => {
  const env = makeBaseEnv({
    users: { admin: { id: "admin", username: "admin", role: "admin", enabled: true, token: "" } },
    kv: { _admin_seeded: "1", "auth:admin_password": "pw" },
  });
  const cookie = await issueSessionToken("pw", "admin", "admin");
  const res = await worker.fetch(
    new Request("https://x/api/plugins/status", {
      method: "GET",
      headers: { "content-type": "application/json", cookie: `ag_session=${cookie}` },
    }),
    env,
  );
  assert.equal(res.status, 200);
  assert.deepEqual(
    env._frontDoor,
    [],
    "the one exclusion must NOT be handed to the Rust worker — the route is not in isMcpFamily",
  );
  const body = await res.json();
  assert.ok(Array.isArray(body.routes), "the answer carries routeStats, which only the plugin table can give");
  assert.ok(body.devices && typeof body.devices === "object", "and the device map, not the stub's model list");
});

/* ---- Reset admin password (round-113, by admin gateway token) ---- */

function makeResetEnv() {
  // NO `WASM_GATE` BINDING, ON PURPOSE: THE ROLLBACK — see `makeEnv` above.
  return makeBaseEnv({
    wasmGate: false,
    users: {
      admin: {
        id: "admin",
        username: "admin",
        role: "admin",
        enabled: true,
        token: "ADMIN_KEY_123",
      },
    },
    kv: { _admin_seeded: "1", "auth:admin_password": "oldsalt:oldhash" },
    // Fail-closed issuance: login refuses without SESSION_SECRET.
    extra: { SESSION_SECRET: "test-session-secret-0123456789abcdef" },
  });
}

test("pre-rotation password-signed cookie still verifies with SESSION_SECRET set", async () => {
  __clearCaches();
  const env = makeEnv();
  env.SESSION_SECRET = "test-session-secret-0123456789abcdef";
  const cookie = await issueSessionToken("pw", "admin", "admin"); // old key = stored admin password
  const req = new Request("https://x/api/plugins/status", {
    method: "GET",
    headers: { "content-type": "application/json", cookie: `ag_session=${cookie}` },
  });
  const res = await worker.fetch(req, env);
  assert.equal(res.status, 200);
});

test("getUser: corrupt user JSON → null (no throw)", async () => {
  __clearCaches();
  const { getUser } = await import("../src/store/users.ts");
  const env = { KEYS: { get: async () => "{corrupt" } };
  assert.equal(await getUser(env, "ghost"), null);
});

// round-525 (coverage-driven): the ADMIN_PASSWORD migration arm had ZERO pins —
// a Worker-secret password with no KV hash must migrate hashed, never plaintext.
/** The env the surviving helper tests use — **AND ITS COMMENT OUTLIVED ITS REASON (2026-10-17)**: it said
 *  "NO `WASM_GATE` BINDING, ON PURPOSE: THE ROLLBACK", and there is no rollback any more — `authPlugin` and
 *  `adminPlugin` are deleted, so a route test that needed the fallback would 404. What is left here is
 *  `getAdminPassword`'s own KV seed, which is a store helper's subject rather than a route's. */
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
