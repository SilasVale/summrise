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
    // about. The cutover's own routing is witnessed in `frontdoor.test.mjs` (the binding-less fallthrough) and
    // in `plugins.test.mjs` (the one exclusion, with the binding present) — `devices-cutover.test.mjs` went with
    // the code it tested (rule 1) and asserted the UNCONDITIONAL handover this tree no longer has.
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

// round-529 (coverage-driven): the fetch-THROW arm had ZERO pins (only the
// non-ok-response fallback was covered) — a dead upstream must still 200.
// DELETED: "install-cmd: 401 unauth" — its contract is pinned by the frozen corpus (see the commit).

/* ---------------- /api/plugins/status?fresh=1 ---------------- */

test("plugins/status: ?fresh=1 bypasses the 30s probe cache; cached call does not re-probe", async () => {
  // Unique device name — DEVICE_PROBE_CACHE is a module-level Map shared
  // across tests, so probe counting must use a name no other test touches.
  // The stub matcher is LOWERCASE: the WHATWG URL parser lowercases
  // hostnames (deviceFetch round-121), so "dFresh" arrives as "dfresh".
  const env = makeEnv([
    { name: "dFresh", hostname: "dFresh.agent.summrise.test", token: "tok-fresh" },
  ]);
  const { calls, restore } = stubFetch("dfresh.agent.summrise.test/api/status", {
    ok: true,
    version: "9.8.7",
    serial_ports: [],
  });
  try {
    const auth = { cookie: await adminCookie() };
    const first = await worker.fetch(req("GET", "/api/plugins/status", auth), env);
    assert.equal(first.status, 200);
    // round-183: the SAME response must carry the dead-route report (round 179's
    // instrument, wired in round 182). Asserted here rather than in a new test because
    // this test already holds the harness — a fresh worker fetch with an admin session —
    // and a field whose only evidence is "the code adds it" is the shape this log keeps
    // closing. `hits` counts only the route that SERVED, so every entry is a number.
    const body = await first.json();
    const j1 = body.devices.dFresh;
    assert.ok(Array.isArray(body.routes), "the response carries the route statistics");
    assert.ok(
      body.routes.every((r) => typeof r.index === "number" && typeof r.hits === "number"),
      "every entry is {index, hits}",
    );
    assert.ok(
      body.routes.some((r) => r.hits > 0),
      "at least the route that served this very request has been counted",
    );
    assert.equal(j1.agent_up, true);
    assert.equal(j1.tunnel_up, true);
    assert.equal(j1.version, "9.8.7");
    assert.equal(typeof j1.checked_at, "number");
    const probesAfterFirst = calls.filter((u) => u.includes("/api/status")).length;
    assert.equal(probesAfterFirst, 1);

    // Cached call: no new probe.
    await worker.fetch(req("GET", "/api/plugins/status", auth), env);
    assert.equal(calls.filter((u) => u.includes("/api/status")).length, probesAfterFirst);

    // fresh=1: cache bypassed, exactly one more probe.
    const fresh = await worker.fetch(req("GET", "/api/plugins/status?fresh=1", auth), env);
    assert.equal(fresh.status, 200);
    assert.equal(calls.filter((u) => u.includes("/api/status")).length, probesAfterFirst + 1);
  } finally {
    restore();
  }
});

// round-527 (coverage-driven): the device-scan catch had ZERO pins — a
// throwing registry read must deny, not crash.
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
// round-460 (coverage-driven): the PROVED rotation arm + the new-device
// proxySecret capture arm had ZERO pins.
// DELETED: "self-register: tunnel-proved rotation accepted, new device captures the secret" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "self-register: stored-tunnel proof rotates the token" — its contract is pinned by the frozen corpus (see the commit).

/* ---------------- register + tunnel-token (one-time-key chain) ---------- */
// round-441 (coverage-driven): both public one-time-key handlers had ZERO
// route pins — the spend/claim/grant chain and the round-68 anti-takeover.
const regPost = (env, path, body) => worker.fetch(req("POST", path, { body, ip: testIp() }), env);

// DELETED: "register: garbage key 403s with zero KV writes; happy path spends the key" — its contract is pinned by the frozen corpus (see the commit).

// round-530 (coverage-driven): the proxySecret-probe catch had ZERO pins —
// a dead device at register time must still register (probe is best-effort).
// DELETED: "register: unreachable device still registers without proxySecret" — its contract is pinned by the frozen corpus (see the commit).

// round-526 (coverage-driven): the post-claim key recheck had ZERO pins — a
// key consumed by a concurrent request between check and claim must 403.
// round-461 (coverage-driven): register-with-key 400, rename bad-hostname
// 400, tunnel-token claim 403 + vanishing-key cleanup had ZERO pins.
// DELETED: "register: valid key with bad body 400s; rename rejects a bad hostname" — its contract is pinned by the frozen corpus (see the commit).

// DELETED: "tunnel-token: claimed key 403s; vanishing key releases the claim" — its contract is pinned by the frozen corpus (see the commit).

// round-462 (coverage-driven): the insert-race 409 arm (round-122: pre-check
// passes, locked insert loses) + the admin-session upload arm had ZERO pins.
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