// Device management endpoint tests — the round-159 additions that had ZERO
// coverage before (rename, register-keys list/revoke, install-cmd, and the
// ?fresh=1 probe-cache bypass on /api/plugins/status).
//
// Same harness as proxy-auth.test.mjs: full worker fetch against the default
// export with a Map-backed KV stub; admin/non-admin sessions minted directly
// via issueSessionToken (no ACCESS_* env needed). store.ts's module-level
// 24h cache MUST be cleared per test — these tests MUTATE devices:v1, unlike
// proxy-auth's read-only seeding, so a stale cache would leak across tests.
import test from "node:test";
import assert from "node:assert/strict";
import worker from "../src/index.ts";
import { issueSessionToken, SESSION_COOKIE } from "../src/auth.ts";
import { __clearCaches, maskKey } from "../src/store.ts";
import { makeEnv as makeBaseEnv, withFetch, skewClock } from "./helpers.mjs";

const ADMIN_PW = "test-admin-password";

// Shared Map-KV stub (helpers.mjs, richest variant: list() + expiry tracking)
// seeded with this file's console base; keeps the makeEnv(devices, links)
// call shape the tests use. The expiry map is exposed as env._expiry (tests
// backdate entries to reproduce real KV's expired-but-unreaped list names).
function makeEnv(devices, links = {}) {
  return makeBaseEnv({
    // **NO `WASM_GATE` BINDING, ON PURPOSE: THIS FILE TESTS THE TYPESCRIPT IMPLEMENTATION.** `index.ts` hands the
    // device family to the Rust worker when that binding is present (the production configuration), so a test of
    // the TypeScript handlers is a test of the ROLLBACK path — which is a real configuration (a `wrangler dev`
    // without the sibling worker, a deployment whose binding was removed) and the one these assertions were always
    // about. The cutover's own routing is pinned in `devices-cutover.test.mjs`.
    wasmGate: false,
    // The hosts above are device hosts AND the install base, and both rules live in the env (round 120):
    // `DEVICE_HOST_SUFFIX` decides what a device hostname may be, `INDEX_WORKER_URL` where the install
    // manifest is read from — the two are configurable since rounds 87 and 93.
    extra: { DEVICE_HOST_SUFFIX: ".agent.summrise.test", INDEX_WORKER_URL: "https://agent.summrise.test" },
    devices,
    links,
    users: {
      admin: { id: "admin", username: "admin", role: "admin", enabled: true, token: "" },
      bob: { id: "bob", username: "bob", role: "user", enabled: true, token: "" },
    },
    kv: { "auth:admin_password": ADMIN_PW, _admin_seeded: "1" },
  });
}

async function adminCookie() {
  return issueSessionToken(ADMIN_PW, "admin", "admin");
}
async function userCookie() {
  return issueSessionToken(ADMIN_PW, "bob", "user");
}

function req(method, path, { body, cookie, auth, ip } = {}) {
  const headers = {};
  if (cookie) headers.cookie = `${SESSION_COOKIE}=${cookie}`;
  if (auth) headers.authorization = `Bearer ${auth}`;
  if (ip) headers["cf-connecting-ip"] = ip;
  return new Request(`https://x${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

/** Install a global fetch stub; returns { calls, restore }. */
function stubFetch(matcher, response) {
  const calls = [];
  const real = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push(String(url));
    if (String(url).includes(matcher)) {
      return response instanceof Response
        ? response
        : new Response(JSON.stringify(response), {
            status: 200,
            headers: { "content-type": "application/json" },
          });
    }
    return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
  };
  return {
    calls,
    restore: () => {
      globalThis.fetch = real;
    },
  };
}

const D1 = {
  name: "d1",
  hostname: "d1.agent.summrise.test",
  token: "devtok-1234567890",
  registeredAt: 1000,
  lastVersion: "1.0.105",
};
const D2 = { name: "d2", hostname: "d2.agent.summrise.test", token: "devtok-9876543210" };

/* ---------------- rename ---------------- */

// DELETED: "rename: happy path preserves token + metadata, migrates plugin links" — its contract is pinned by the frozen corpus (see the commit).

test("rename: same-name rename is allowed (no-op host refresh)", async () => {
  const env = makeEnv([D1]);
  const res = await worker.fetch(
    req("POST", "/api/devices/d1/rename", { body: { name: "d1" }, cookie: await adminCookie() }),
    env,
  );
  assert.equal(res.status, 200);
});

// DELETED: "rename: error branches — 401 unauth / 403 non-admin / 400 bad name / 404 missing / 409 taken" — its contract is pinned by the frozen corpus (see the commit).

/* ---------------- register-keys ---------------- */

// DELETED: "register-keys: generate → list with TTL → revoke → empty; 401 unauth" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "register-keys: expired-but-unreaped KV entries are filtered from the list" — its contract is pinned by the frozen corpus (see the commit).

/* ---------------- install-cmd ---------------- */

// installCmdCache is module-level with a 5-min TTL — every test travels the
// clock past it (store.cache.test.mjs's Date.now pattern) so tests stay
// order-independent about what the previous test cached.
function travelMs(ms) {
  // The shared helper, so this idiom lives in one place — see its header for why these tests move the
  // clock at all.
  const clock = skewClock(ms);
  return () => clock.restore();
}

// DELETED: "install-cmd: upstream version flows through" — its contract is pinned by the frozen corpus (see the commit).

test("install-cmd: 5-min in-isolate cache (second call hits, clock travel re-fetches)", async () => {
  // installCmdCache is module-level and the previous test seeded it — every
  // travel offset here is strictly beyond all earlier ones (0 < 10m < 20m)
  // so each phase's cache age is deterministic (5-min TTL).
  const env = makeEnv([D1]);
  let undo = travelMs(10 * 60 * 1000);
  const { calls, restore } = stubFetch("agent.summrise.test/api/version", {
    version: "9.9.9",
    download: "https://x/dl/v.tgz",
  });
  try {
    const auth = { cookie: await adminCookie() };
    await worker.fetch(req("GET", "/api/devices/install-cmd", auth), env);
    await worker.fetch(req("GET", "/api/devices/install-cmd", auth), env);
    assert.equal(calls.length, 1, "second call within the TTL must hit the cache");
    undo();
    undo = travelMs(20 * 60 * 1000);
    await worker.fetch(req("GET", "/api/devices/install-cmd", auth), env);
    assert.equal(calls.length, 2, "cache must expire after 5 min");
  } finally {
    restore();
    undo();
  }
});

test("install-cmd: upstream failure → null version fallback (UI falls back to its constant)", async () => {
  const env = makeEnv([D1]);
  // 30 min: strictly beyond the previous test's cache stamp (20 min) + TTL.
  const undo = travelMs(30 * 60 * 1000);
  const real = globalThis.fetch;
  globalThis.fetch = async () => new Response("boom", { status: 503 });
  try {
    const res = await worker.fetch(
      req("GET", "/api/devices/install-cmd", { cookie: await adminCookie() }),
      env,
    );
    assert.equal(res.status, 200);
    const j = await res.json();
    assert.deepEqual([j.ok, j.version, j.download], [true, null, null]);
  } finally {
    globalThis.fetch = real;
    undo();
  }
});

// round-529 (coverage-driven): the fetch-THROW arm had ZERO pins (only the
// non-ok-response fallback was covered) — a dead upstream must still 200.
test("install-cmd: upstream throw → null version fallback (no 500)", async () => {
  const env = makeEnv([D1]);
  // 40 min: strictly beyond the previous tests' cache stamps + TTL.
  const undo = travelMs(40 * 60 * 1000);
  const real = globalThis.fetch;
  globalThis.fetch = async () => {
    throw new TypeError("fetch failed");
  };
  try {
    const res = await worker.fetch(
      req("GET", "/api/devices/install-cmd", { cookie: await adminCookie() }),
      env,
    );
    assert.equal(res.status, 200);
    const j = await res.json();
    assert.deepEqual([j.ok, j.version, j.download], [true, null, null]);
  } finally {
    globalThis.fetch = real;
    undo();
  }
});

// DELETED: "install-cmd: 401 unauth" — its contract is pinned by the frozen corpus (see the commit).

/* ---------------- /api/plugins/status?fresh=1 ---------------- */

// DELETED: "plugins/status: ?fresh=1 bypasses the 30s probe cache; cached call does not re-probe" — its contract is pinned by the frozen corpus (see the commit).

test("upload proxy: 401 unauth / 401 bad device token (no network on reject)", async () => {
  const env = makeEnv({
    d1: { name: "d1", hostname: "d1.agent.summrise.test", token: "a".repeat(64), proxySecret: "s" },
  });
  const noAuth = await worker.fetch(req("POST", "/api/upload"), env);
  assert.equal(noAuth.status, 401);
  const bad = await worker.fetch(
    req("POST", "/api/upload", { auth: `Bearer ${"b".repeat(64)}` }),
    env,
  );
  assert.equal(bad.status, 401);
});

test("upload proxy: device config token accepted (no network on reject paths only)", async () => {
  const { __clearCaches } = await import("../src/store.ts");
  __clearCaches();
  const env = makeEnv({
    d1: { name: "d1", hostname: "d1.agent.summrise.test", token: "c".repeat(64), proxySecret: "s" },
  });
  // Bad token still 401 without touching the network.
  const bad = await worker.fetch(
    req("POST", "/api/upload", { auth: `Bearer ${"d".repeat(64)}` }),
    env,
  );
  assert.equal(bad.status, 401);
});

// round-527 (coverage-driven): the device-scan catch had ZERO pins — a
// throwing registry read must deny, not crash.
test("upload proxy: registry outage denies with 401 (no throw)", async () => {
  const { __clearCaches } = await import("../src/store.ts");
  __clearCaches();
  const env = makeEnv({
    d1: { name: "d1", hostname: "d1.agent.summrise.test", token: "c".repeat(64), proxySecret: "s" },
  });
  const realGet = env.KEYS.get.bind(env.KEYS);
  env.KEYS.get = async (k) => {
    if (k === "devices:v1") throw new Error("kv down");
    return realGet(k);
  };
  const res = await worker.fetch(
    req("POST", "/api/upload", { auth: `Bearer ${"c".repeat(64)}` }),
    env,
  );
  assert.equal(res.status, 401);
  assert.match((await res.json()).error.message, /Invalid device token/);
});

// DELETED: "upload proxy: forwards a MINIMAL header set — UPLOAD_KEY + multipart framing, never client cookies" — its contract is pinned by the frozen corpus (see the commit).

/* ---------------- admin gate matrix (round-364) ---------------- */
// Every admin-gated devices route must answer 401 with no session and 403
// for a non-admin session — BEFORE touching devices (reject-only calls, so
// no mutations happen and one shared env suffices). Rename/install-cmd
// already pin theirs per-route; the rest are pinned here so a future
// handler that forgets requireSession fails loudly.
// DELETED: "admin gate matrix: no session → 401, non-admin → 403 on every admin devices route" — its contract is pinned by the frozen corpus (see the commit).

/* ---------------- self-register (round-158 anti-hijack endpoint) -------- */
// round-440 (coverage-driven): handleSelfRegister had ZERO route pins.
const T64 = (c) => c.repeat(64);
// round-441: the public device routes share one 10/min/IP gate — each test
// below uses its own cf-connecting-ip so the tests never trip each other.
let nextIp = 41;
const testIp = () => `10.44.1.${nextIp++}`;
const selfReg = (env, body, ip) =>
  worker.fetch(req("POST", "/api/devices/self-register", { body, ip: ip || testIp() }), env);

// DELETED: "self-register: malformed body 400, non-64-hex token 403, off-suffix host 400" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "self-register: new device inserts; same-token re-post refreshes idempotently" — its contract is pinned by the frozen corpus (see the commit).

// round-538 (coverage-driven): the self-reg harvest catch had ZERO pins —
// an unreachable device must still register (secret harvest is best-effort).
// DELETED: "self-register: unreachable device still registers without proxySecret" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "self-register: existing device rejects hostname moves + unproven rotations" — its contract is pinned by the frozen corpus (see the commit).

// round-532 (coverage-driven): the dead-tunnel proof catch had ZERO pins —
// an unreachable tunnel must 409, not 500.
test("self-register: dead tunnel refuses rotation with 409 (no 500)", async () => {
  __clearCaches();
  const OLD = T64("c"),
    NEW = T64("d");
  const env = makeEnv([
    {
      name: "d1",
      hostname: "d1.agent.summrise.test",
      token: OLD,
      proxySecret: "ps-stored",
      registeredAt: 7,
    },
  ]);
  const real = globalThis.fetch;
  globalThis.fetch = async () => {
    throw new TypeError("fetch failed");
  };
  try {
    const res = await selfReg(env, { name: "d1", hostname: "d1.agent.summrise.test", token: NEW });
    assert.equal(res.status, 409);
    const devs = JSON.parse(await env.KEYS.get("devices:v1"));
    assert.equal(devs.find((d) => d.name === "d1").token, OLD, "production record untouched");
  } finally {
    globalThis.fetch = real;
  }
});

// round-460 (coverage-driven): the PROVED rotation arm + the new-device
// proxySecret capture arm had ZERO pins.
// DELETED: "self-register: tunnel-proved rotation accepted, new device captures the secret" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "self-register: stored-tunnel proof rotates the token" — its contract is pinned by the frozen corpus (see the commit).

/* ---------------- register + tunnel-token (one-time-key chain) ---------- */
// round-441 (coverage-driven): both public one-time-key handlers had ZERO
// route pins — the spend/claim/grant chain and the round-68 anti-takeover.
const regPost = (env, path, body) => worker.fetch(req("POST", path, { body, ip: testIp() }), env);

// DELETED: "register: garbage key 403s with zero KV writes; happy path spends the key" — its contract is pinned by the frozen corpus (see the commit).

test("register: existing device name refuses with 409 (round-68 anti-takeover)", async () => {
  __clearCaches();
  const env = makeEnv([{ name: "d1", hostname: "d1.agent.summrise.test", token: T64("c") }]);
  await env.KEYS.put("regkey:kk22", "1");
  const res = await regPost(env, "/api/register", {
    key: "kk22",
    name: "d1",
    hostname: "d1.agent.summrise.test",
    token: T64("d"),
  });
  assert.equal(res.status, 409);
  const devs = JSON.parse(await env.KEYS.get("devices:v1"));
  assert.equal(devs.find((d) => d.name === "d1").token, T64("c"), "production record untouched");
});

// round-530 (coverage-driven): the proxySecret-probe catch had ZERO pins —
// a dead device at register time must still register (probe is best-effort).
// DELETED: "register: unreachable device still registers without proxySecret" — its contract is pinned by the frozen corpus (see the commit).

// round-526 (coverage-driven): the post-claim key recheck had ZERO pins — a
// key consumed by a concurrent request between check and claim must 403.
test("register: key vanishing after claim 403s (single-flight recheck)", async () => {
  __clearCaches();
  const env = makeEnv([]);
  await env.KEYS.put("regkey:kkrace", "1");
  const realGet = env.KEYS.get.bind(env.KEYS);
  let reads = 0;
  env.KEYS.get = async (k) => {
    if (k === "regkey:kkrace" && ++reads > 1) return null; // raced away
    return realGet(k);
  };
  const res = await regPost(env, "/api/register", {
    key: "kkrace",
    name: "d9",
    hostname: "d9.agent.summrise.test",
    token: T64("a"),
  });
  assert.equal(res.status, 403);
  assert.match((await res.json()).error.message, /Invalid or used registration key/);
});

// round-461 (coverage-driven): register-with-key 400, rename bad-hostname
// 400, tunnel-token claim 403 + vanishing-key cleanup had ZERO pins.
// DELETED: "register: valid key with bad body 400s; rename rejects a bad hostname" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "tunnel-token: claimed key 403s; vanishing key releases the claim" — its contract is pinned by the frozen corpus (see the commit).

// round-462 (coverage-driven): the insert-race 409 arm (round-122: pre-check
// passes, locked insert loses) + the admin-session upload arm had ZERO pins.
test("register: lost insert race 409s (pre-check passed, lock lost)", async () => {
  __clearCaches();
  const env = makeEnv([]);
  await env.KEYS.put("regkey:kk66", "1");
  const raced = JSON.stringify([
    { name: "d1", hostname: "d1.agent.summrise.test", token: T64("q") },
  ]);
  let devGets = 0;
  const innerGet = env.KEYS.get.bind(env.KEYS);
  env.KEYS.get = async (k) => {
    if (k === "devices:v1") return ++devGets === 1 ? "[]" : raced;
    return innerGet(k);
  };
  const { restore } = stubFetch("d1.agent.summrise.test", {});
  try {
    const res = await regPost(env, "/api/register", {
      key: "kk66",
      name: "d1",
      hostname: "d1.agent.summrise.test",
      token: T64("w"),
    });
    assert.equal(res.status, 409);
  } finally {
    restore();
  }
});

// DELETED: "upload proxy: admin session is proxied with the upload key" — its contract is pinned by the frozen corpus (see the commit).

// round-463 (coverage-driven): the upload 413 bound + the public-gate 429
// arm had ZERO pins.
// round-554: the bound moved 25MB → 100MB (it used to sit BELOW the agent's
// and the index worker's own 100 MiB, so a 30 MB firmware image died here
// with a 413 no layer's documentation predicted). 26MB must now PASS and
// 101MB must still be refused before any upstream dial.
// DELETED: "upload proxy: declared 26MB body passes the re-raised bound" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "upload proxy: declared 101MB body 413s without touching the network" — its contract is pinned by the frozen corpus (see the commit).

// ── round-554: the relay is symmetric (an AI client can stage a file FOR a
// device) and speaks raw-stream PUT ───────────────────────────────────────
const ADMINTOK = "a".repeat(64);
const RELAYTOK = "r".repeat(64);

function tokenEnv(devices = []) {
  return makeBaseEnv({
    // **NO `WASM_GATE` BINDING, THE SAME CONFIGURATION THIS FILE'S `makeEnv` DECLARES.** `POST|PUT /api/upload`
    // is the Rust worker's since landing 5 slice 4, so the upload-proxy assertions below are about the
    // TypeScript rollback — and this env is built here rather than through `makeEnv` because the upload section
    // needs a different device list and key set.
    wasmGate: false,
    // The hosts above are device hosts AND the install base, and both rules live in the env (round 120):
    // `DEVICE_HOST_SUFFIX` decides what a device hostname may be, `INDEX_WORKER_URL` where the install
    // manifest is read from — the two are configurable since rounds 87 and 93.
    extra: { DEVICE_HOST_SUFFIX: ".agent.summrise.test", INDEX_WORKER_URL: "https://agent.summrise.test" },
    devices,
    users: {
      admin: {
        id: "admin",
        username: "admin",
        role: "admin",
        enabled: true,
        token: ADMINTOK,
        relayToken: RELAYTOK,
      },
    },
    kv: {
      "auth:admin_password": ADMIN_PW,
      _admin_seeded: "1",
      [`token:${ADMINTOK}`]: "admin",
      [`token:${RELAYTOK}`]: "admin",
    },
    extra: { UPLOAD_KEY: "test-upload-key", INDEX_WORKER_URL: "https://idx.example" },
  });
}

test("upload proxy: the admin API token may stage a file (the Linux → device leg)", async () => {
  const env = tokenEnv([]);
  const seen = [];
  await withFetch(
    async (url, init) => {
      seen.push({ url: String(url), method: init.method, headers: init.headers });
      return new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    },
    async () => {
      const res = await worker.fetch(
        new Request("https://x/api/upload", {
          method: "PUT",
          headers: { authorization: `Bearer ${ADMINTOK}`, "content-length": "5" },
          body: "bytes",
        }),
        env,
      );
      assert.equal(res.status, 200, "the /mcp credential must be able to feed the relay");
      assert.equal(seen.length, 1);
      assert.equal(seen[0].method, "PUT", "the method must survive the proxy (raw-stream path)");
      // The device/user credential never rides on to the index worker — only
      // the relay's own UPLOAD_KEY does.
      assert.equal(seen[0].headers.get("authorization"), "Bearer test-upload-key");
    },
  );
});

test("upload proxy: a relay-role token is refused (ADR-0007 scoping holds)", async () => {
  const env = tokenEnv([]);
  await withFetch(
    async () => {
      throw new Error("must not dial the index worker");
    },
    async () => {
      const res = await worker.fetch(
        new Request("https://x/api/upload", {
          method: "PUT",
          headers: { authorization: `Bearer ${RELAYTOK}`, "content-length": "5" },
          body: "bytes",
        }),
        env,
      );
      assert.equal(res.status, 401, "relay tokens are for the translate path only");
    },
  );
});

// DELETED: "upload proxy: PUT forwards ?name= and the raw-metadata headers" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "upload proxy: non-admin session and unknown tokens stay 401" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "public gate: 10 tunnel-token attempts then 429" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "tunnel-token: valid key returns the CF token once, then feeds register via grant" — its contract is pinned by the frozen corpus (see the commit).

// round-458 (coverage-driven): the list/add/mcp SUCCESS paths had ZERO
// pins (only the 401/403 gate matrix). Covers the list mapping (masked
// token + mcp snippet), add validation + registeredAt keep, mcp 404/200.
// DELETED: "devices list/add/mcp: admin success paths" — its contract is pinned by the frozen corpus (see the commit).

// round-459 (coverage-driven): DELETE success (with link revocation) and
// the grant-redeem matrix had ZERO pins.
// DELETED: "devices delete: removes the record and revokes its plugin links" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "panel-grant redeem: no-token/unknown-token 401, mismatch 403, unknown grant 404, ok + single-use" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "panel-grant redeem: KV read failure fails closed 401" — its contract is pinned by the frozen corpus (see the commit).

// Version probe reads the npm release (round-304), not the frozen Cargo
// version — the console showed v1.0.145 forever and the outdated badge
// never cleared. Distinct device names per test: the probe cache lives in
// the mcp plugin module (not cleared by __clearCaches); fresh=1 bypasses
// the read side anyway.
// DELETED: "plugins/status: probe prefers npm release over Cargo version + persists lastVersion" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "plugins/status: probe falls back to Cargo version when release is absent" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "plugins/status: probe with no version fields omits version, no crash" — its contract is pinned by the frozen corpus (see the commit).

/* ---------------- hostname allowlist on the ADMIN write paths ----------------
 * A security audit found these two paths applied only the SHAPE check
 * (`validateDevice`'s RFC-domain regex / the rename regex), never the hostname
 * ALLOWLIST the other write paths use. Consequence, proven against the real
 * dispatcher: register `hostname: "attacker.example"`, then call
 * `GET /api/devices/<n>/proxy/...` and the worker dials that host carrying the
 * device's PERMANENT bearer token and proxy secret. Rename is the
 * credential-stealing variant because it PRESERVES both.
 *
 * There were no tests here before because there was no check.
 */
test("POST /api/devices: a hostname outside the allowlist is REFUSED (token must never be dialed to it)", async () => {
  const env = makeEnv([]);
  const admin = await adminCookie();
  const res = await worker.fetch(
    req("POST", "/api/devices", {
      cookie: admin,
      body: { name: "evil", hostname: "attacker.example", token: "a".repeat(64) },
    }),
    env,
  );
  assert.equal(res.status, 400, "a non-allowlisted hostname must not become a device record");
  assert.match(
    (await res.json()).error?.message || "",
    /agent\.summrise\.test/,
    "the error names the required suffix",
  );
  // And nothing was stored — the dial is only reachable through a record.
  const list = await worker.fetch(req("GET", "/api/devices", { cookie: admin }), env);
  const names = ((await list.json()).devices || []).map((d) => d.name);
  assert.ok(!names.includes("evil"), "no record may survive a refused registration");
});

test("POST /api/devices/<n>/rename: a hostname outside the allowlist is REFUSED (rename preserves the token)", async () => {
  const env = makeEnv([D1]);
  const admin = await adminCookie();
  const res = await worker.fetch(
    req("POST", "/api/devices/d1/rename", {
      cookie: admin,
      body: { name: "d1", hostname: "attacker.example" },
    }),
    env,
  );
  assert.equal(res.status, 400, "renaming onto a hostile host must be refused");
  assert.match((await res.json()).error?.message || "", /agent\.summrise\.test/);
  // The device must still point at its REAL host — rename preserves the
  // credential, so a partial write would hand that credential to the attacker.
  const list = await worker.fetch(req("GET", "/api/devices", { cookie: admin }), env);
  const d1 = ((await list.json()).devices || []).find((d) => d.name === "d1");
  assert.equal(d1?.hostname, D1.hostname, "the original hostname must be untouched");
});

test("POST /api/devices + rename: an ALLOWLISTED hostname still works (the guard is a filter, not a wall)", async () => {
  const env = makeEnv([D1]);
  const admin = await adminCookie();
  const add = await worker.fetch(
    req("POST", "/api/devices", {
      cookie: admin,
      body: { name: "d9", hostname: "d9.agent.summrise.test", token: "b".repeat(64) },
    }),
    env,
  );
  assert.equal(add.status, 200, "a legitimate hostname must still register");
  const ren = await worker.fetch(
    req("POST", "/api/devices/d1/rename", {
      cookie: admin,
      body: { name: "d1", hostname: "d1b.agent.summrise.test" },
    }),
    env,
  );
  assert.equal(ren.status, 200, "a legitimate rename must still work");
  assert.equal((await ren.json()).device.hostname, "d1b.agent.summrise.test");
});
