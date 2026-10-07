// Front-door branches (index.ts fetch) with zero direct pins: the
// /models + /chat/completions aliases, the http→https 308, and the
// never-leak-internals 500. All worker.fetch-level, no auth needed
// except where the alias target itself requires it.
import test from "node:test";
import assert from "node:assert/strict";
import worker from "../src/index.ts";
import { makeEnv as makeBaseEnv } from "./helpers.mjs";

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
  const csrfEnv = () => makeBaseEnv({ kv: { "auth:admin_password": "pw", _admin_seeded: "1" } });
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
  assert.equal(same.status, 401, "same-origin passes the gate, fails at auth instead");
  const get = await worker.fetch(
    new Request("https://x/api/me", { headers: { cookie: "ag_session=abc", "sec-fetch-site": "cross-site" } }),
    csrfEnv(),
  );
  assert.equal(get.status, 401, "reads are never CSRF-gated");
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
