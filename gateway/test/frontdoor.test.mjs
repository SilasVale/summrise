// Front-door branches (index.ts fetch) with zero direct pins: the
// /models + /chat/completions aliases, the http→https 308, and the
// never-leak-internals 500. All worker.fetch-level, no auth needed
// except where the alias target itself requires it.
import test from "node:test";
import assert from "node:assert/strict";
import worker from "../src/index.ts";
import { makeEnv as makeBaseEnv } from "./helpers.mjs";

// **THE BINDING IS PRESENT HERE, WHICH IS THE CONFIGURATION THIS FILE IS ABOUT.** `helpers.mjs` stubs
// `WASM_GATE` by default and records what reached it, so the front door's own routing — the `/models` alias, the
// host split, the 404s — is asserted against the DEPLOYED shape. The one test below that needs the ROLLBACK
// configuration (the CSRF gate, which runs BEFORE the handover and is therefore only observable where a
// request that passes it reaches a TypeScript handler) builds its own env and says so.
const env = () => makeBaseEnv({});

test("bare /models aliases to /v1/models (public model list)", async () => {
  // **THE ASSERTION IS THE PATH THE FRONT DOOR WAS ASKED FOR, NOT THE BODY.** Since the cutover, `/v1/*` is
  // forwarded to the Rust worker through `WASM_GATE`; the stub in `helpers.mjs` records every request it receives,
  // so this pins the ALIAS — `/models` must arrive as `/v1/models` — which is the property the branch owns. The
  // body is the Rust worker's, and its bytes are pinned by the 91-case fixture sweep.
  const e = env();
  const a = await worker.fetch(new Request("https://x/models"), e);
  assert.equal(a.status, 200);
  assert.deepEqual(e._frontDoor, ["GET /v1/models"], "the alias must rewrite the path on the way through");
  const ja = await a.json();
  assert.ok(Array.isArray(ja.data) && ja.data.length > 0, "model list must be non-empty");

  const e2 = env();
  await worker.fetch(new Request("https://x/v1/models"), e2);
  assert.deepEqual(e2._frontDoor, ["GET /v1/models"]);
});

test("bare /chat/completions aliases to /v1/chat/completions transparently", async () => {
  const body = JSON.stringify({ model: "ds/deepseek-chat", messages: [] });
  const mk = (p) =>
    new Request(`https://x${p}`, { method: "POST", headers: { "content-type": "application/json" }, body });
  const e1 = env();
  const e2 = env();
  const a = await worker.fetch(mk("/chat/completions"), e1);
  const b = await worker.fetch(mk("/v1/chat/completions"), e2);
  assert.equal(a.status, b.status);
  assert.deepEqual(await a.json(), await b.json());
  assert.deepEqual(e1._frontDoor, e2._frontDoor, "both spellings must reach the front door as one path");
});

test("WITHOUT the binding the front door says so, loudly", async () => {
  // The TypeScript implementation is deleted, so a deployment without `WASM_GATE` cannot serve the API. A 404
  // would read like a routing mistake; this is a 503 that names the missing binding.
  const e = env();
  delete e.WASM_GATE;
  const res = await worker.fetch(new Request("https://x/v1/models"), e);
  assert.equal(res.status, 503);
  const body = await res.json();
  assert.match(body.error.message, /WASM_GATE/);
});

test("plain-http request 308-redirects to https (never serves the Secure cookie over http)", async () => {
  const res = await worker.fetch(
    new Request("https://x/api/me", { headers: { "x-forwarded-proto": "http" } }),
    env(),
  );
  assert.equal(res.status, 308);
  assert.equal(res.headers.get("location"), "https://x/api/me");
});

test("unhandled throw answers 500 Internal error without internals", async () => {
  const origErr = console.error;
  console.error = () => {};
  try {
    const res = await worker.fetch(new Request("https://x/api/me"), null);
    assert.equal(res.status, 500);
    const data = await res.json();
    assert.equal(data.error.message, "Internal error");
    assert.ok(!JSON.stringify(data).includes("CONSOLE_HOST"), "no internals leak");
  } finally {
    console.error = origErr;
  }
});

// round-471 (coverage-driven): the index.ts CSRF-gate 403 arm had ZERO
// route pins (only unit pins on csrfCookieViolation itself).
test("cross-site cookie-authed mutation 403s at the front door", async () => {
  // The ROLLBACK again: with the binding present this POST is handed to the worker before the CSRF gate
  // could be observed (the gate runs first, and this case asserts the gate).
  const csrfEnv = () =>
    makeBaseEnv({ wasmGate: false, kv: { "auth:admin_password": "pw", _admin_seeded: "1" } });
  const mk = (site) =>
    new Request("https://x/api/me/keys", {
      method: "POST",
      headers: {
        cookie: "ag_session=abc",
        "content-type": "application/json",
        "sec-fetch-site": site,
      },
      body: "{}",
    });
  const blocked = await worker.fetch(mk("cross-site"), csrfEnv());
  assert.equal(blocked.status, 403);
  assert.equal((await blocked.json()).error.message, "Cross-site request blocked");
  const same = await worker.fetch(
    new Request("https://x/api/auth/login", {
      method: "POST",
      headers: {
        cookie: "ag_session=abc",
        "content-type": "application/json",
        "sec-fetch-site": "same-origin",
      },
      body: JSON.stringify({ username: "nobody", password: "whatever-long" }),
    }),
    csrfEnv(),
  );
  // **THE CSRF ASSERTION ABOVE IS THE TEST'S SUBJECT AND IT STILL PASSES; THE 401 IS THE ROUTING, AND IT MOVED
  // BACK WHEN THE ROLLBACK DID.** `csrfEnv()` builds the NO-BINDING configuration on purpose (see its comment),
  // and `index.ts`'s three `wasmGate(env)` conditions send a binding-less request to the TypeScript plugins —
  // the rollback `index.ts` describes in its own words and the cascade had removed (restoring it was worth 35
  // failures at once). So a same-origin login falls through to `plugins/auth.ts` and is refused at AUTH, which
  // is what this 401 means. The 503 this assertion carried between e726b88b and here belongs to a tree whose
  // handover was unconditional AND whose fallback was deleted; that tree is not this one, and the count caught
  // the difference (401 !== 503) rather than hiding it.
  assert.equal(same.status, 401, "same-origin passes the gate, fails at auth instead");
  const get = await worker.fetch(
    new Request("https://x/api/me", { headers: { cookie: "ag_session=abc", "sec-fetch-site": "cross-site" } }),
    csrfEnv(),
  );
  // Same reading as above: the gate does not apply to reads (which is the claim), and the answer behind it is
  // the rollback plugin's refusal of a bogus cookie.
  assert.equal(get.status, 401, "reads are never CSRF-gated, and the fallback refuses the bogus cookie");
});

// round-476 (coverage-driven): the static-asset branch arms had ZERO pins.
test("static page: off-host path 404s; page host proxies ASSETS or 404s", async () => {
  const offHost = await worker.fetch(
    new Request("https://x/some-page"),
    makeBaseEnv({ extra: { CONSOLE_HOST: "other.example" } }),
  );
  assert.equal(offHost.status, 404, "non-/v1/ on a non-page host");
  const noAssets = await worker.fetch(new Request("https://x/some-page"), env());
  assert.equal(noAssets.status, 404, "page host without ASSETS binding");
  const proxied = await worker.fetch(
    new Request("https://x/some-page"),
    makeBaseEnv({ extra: { ASSETS: { fetch: async () => new Response("landing", { status: 200 }) } } }),
  );
  assert.equal(proxied.status, 200);
  assert.equal(await proxied.text(), "landing");
});

// ── THE ADMIN FAMILY'S CUTOVER ───────────────────────────────────────────────────────────────────
//
// **THIS IS THE LANDING THAT MAKES SEVENTEEN PORTED ROUTES LIVE.** The worker has served `/api/admin/*` since
// the model probe landed; until this cutover nothing routed to it, so the ported family was dead weight behind a
// path the console's own plugin registry answered first. These tests pin the PREDICATE (path for path, because a
// path handed over is a path that must be served) and the ROLLBACK (no binding → the TypeScript plugin).
test("the admin family is handed to the worker, path for path", async () => {
  const cases = [
    ["GET", "/api/admin/users"],
    ["GET", "/api/admin/cloudflare-token"],
    ["GET", "/api/admin/password"],
    ["GET", "/api/admin/models"],
    ["GET", "/api/admin/providers"],
    ["GET", "/api/admin/catalogue"],
    ["GET", "/api/admin/public"],
    ["PUT", "/api/admin/password"],
    ["PUT", "/api/admin/cloudflare-token"],
    ["PUT", "/api/admin/users/bob/enabled"],
    ["PUT", "/api/admin/models/og%2Fmimo-v2.5/enabled"],
    ["POST", "/api/admin/invite"],
    ["POST", "/api/admin/models"],
    ["POST", "/api/admin/models/probe"],
    ["POST", "/api/admin/providers"],
    ["DELETE", "/api/admin/models/og%2Fmimo-v2.5"],
    ["DELETE", "/api/admin/providers/acme%2F"],
    // **THE EMPTY PREFIX IS A ROUTE, NOT A MISS**: `DELETE /api/admin/providers/` is in the family and answers
    // the route's own 400 — the Rust predicate claims it, so this one has to as well.
    ["DELETE", "/api/admin/providers/"],
  ];
  for (const [method, path] of cases) {
    const e = env();
    await worker.fetch(new Request(`https://x${path}`, { method }), e);
    assert.deepEqual(
      e._frontDoor,
      [`${method} ${path}`],
      `${method} ${path} must be handed to the worker`,
    );
  }
});

test("a path the family does not claim is NOT handed over", async () => {
  // Each of these shares a prefix or a shape with the family and must stay on the TypeScript path: an unknown
  // `/api/admin/...` path, a GET where only a PUT exists, and a DELETE that is not one of the two dynamic arms.
  for (const [method, path] of [
    ["GET", "/api/admin/anything"],
    ["GET", "/api/admin/models/og%2Fmimo-v2.5/enabled"],
    ["PUT", "/api/admin/users/bob"],
    ["POST", "/api/admin/users"],
    ["DELETE", "/api/admin/invite"],
    ["GET", "/api/admin"],
  ]) {
    const e = env();
    await worker.fetch(new Request(`https://x${path}`, { method }), e);
    assert.deepEqual(e._frontDoor, [], `${method} ${path} must NOT be handed to the worker`);
  }
});

test("WITHOUT the binding the admin family falls back to the TypeScript plugin", async () => {
  // **THE ROLLBACK, AND IT IS THE BINDING'S ABSENCE.** `plugins/admin.ts` is unchanged and still registered, so
  // a local `wrangler dev` or a deployment whose binding was removed serves the family as it did before. The
  // assertion is that the request reached a TypeScript handler at all — a session-gated route answers 401 there,
  // where an unbound handover would have reached no one.
  const e = env();
  delete e.WASM_GATE;
  const res = await worker.fetch(new Request("https://x/api/admin/users"), e);
  assert.equal(res.status, 401, "the plugin's own session gate, not a routing 404");
  assert.deepEqual(e._frontDoor, [], "nothing may reach a worker that is not bound");
});
