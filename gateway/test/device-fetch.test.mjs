// deviceHostError precision (SSRF guard shared by deviceFetch + the MCP
// browser bridge): 172.16/12 is second-octet 16-31 only (not all of 172/8),
// and the fc/fd/fe80 v6 prefixes must be actual address forms (contain ':'
// — hostnames never do), so 'fc.example.com' stays reachable.
import test from "node:test";
import assert from "node:assert/strict";
import { deviceFetch, deviceHostError } from "../src/device-fetch.ts";

test("172.16/12 blocked: 172.16.x through 172.31.x", () => {
  for (const h of ["172.16.0.1", "172.20.5.4", "172.31.255.255"]) {
    assert.match(deviceHostError(h) || "", /private\/internal/, `${h} blocked`);
  }
});

test("172/8 outside 16/12 allowed: 172.15.x and 172.32.x", () => {
  for (const h of ["172.15.0.1", "172.32.0.1", "172.0.0.1", "172.33.1.2"]) {
    assert.equal(deviceHostError(h), null, `${h} allowed`);
  }
});

test("hostname strings with v6-like prefixes allowed: fc.example.com", () => {
  for (const h of ["fc.example.com", "fd.example.com", "fe80.example.com"]) {
    assert.equal(deviceHostError(h), null, `${h} allowed`);
  }
});

test("actual v6 private forms still blocked", () => {
  for (const h of ["fc00::1", "fd00::1234", "fe80::1"]) {
    assert.match(deviceHostError(h) || "", /private\/internal/, `${h} blocked`);
  }
});

test("classic guards unchanged: loopback, mapped, metadata, public", () => {
  for (const h of [
    "127.0.0.1",
    "::ffff:127.0.0.1",
    "localhost",
    "10.0.0.5",
    "192.168.1.1",
    "169.254.169.254",
    "0.0.0.0",
    "::1",
  ]) {
    assert.match(deviceHostError(h) || "", /private\/internal/, `${h} blocked`);
  }
  // This list is the SSRF guard's business (public vs private), NOT the suffix rule — `deviceHostError` takes no env, so
  // the host only has to be public (round 114).
  for (const h of ["d1.agent.summrise.test", "example.com", "8.8.8.8"]) {
    assert.equal(deviceHostError(h), null, `${h} allowed`);
  }
});

// ── deviceFetch path sanitization (round-120/121 SSRF fixes, round-361) ──
// The authority-prefix gate + hostname-equality gate had NO direct tests
// (only indirect exercise via mcp-handler). Stub globalThis.fetch: the
// module calls it through fetchWithTimeout, which uses the global.
// The device dials through an env that must accept its hostname: the two are one fixture (rounds 94-113).
const ENV = { DEVICE_HOST_SUFFIX: ".agent.summrise.test" };
const DEV = { hostname: "d1.agent.summrise.test", token: "tok-device-1" };

async function withStubFetch(handler, fn) {
  const real = globalThis.fetch;
  globalThis.fetch = handler;
  try {
    return await fn();
  } finally {
    globalThis.fetch = real;
  }
}

const okUpstream = (seen) => async (url, init) => {
  seen.url = String(url);
  seen.init = init;
  return new Response("ok", { status: 200 });
};

test("deviceFetch: userinfo smuggling (@evil) → 400, upstream never called (round-120)", async () => {
  const r = await withStubFetch(
    async () => {
      throw new Error("must not be called");
    },
    () => deviceFetch(ENV, DEV, "@evil.example/x"),
  );
  assert.equal(r.status, 400);
  assert.equal(r.error, "invalid proxy path");
  assert.equal(r.resp, undefined);
});

test("deviceFetch: leading scheme in path → 400 (round-120)", async () => {
  for (const p of ["https://evil.example/x", "http://evil.example/"]) {
    const r = await withStubFetch(
      async () => {
        throw new Error("must not be called");
      },
      () => deviceFetch(ENV, DEV, p),
    );
    assert.equal(r.status, 400, p);
  }
});

test("deviceFetch: @ in a query string is legitimate → passes through (round-121 narrowing)", async () => {
  const seen = {};
  const r = await withStubFetch(okUpstream(seen), () =>
    deviceFetch(ENV, DEV, "/api/x?user=a@b.com"),
  );
  assert.equal(r.status, 200);
  assert.match(seen.url, /\/api\/x\?user=a@b\.com/, "query preserved verbatim");
  assert.match(seen.url, /^https:\/\/d1\.agent\.summrise\.test\//, "host is the device's own");
});

test("deviceFetch: header hygiene — host/cookie stripped, device Bearer injected", async () => {
  const seen = {};
  await withStubFetch(okUpstream(seen), () =>
    deviceFetch(ENV, DEV, "/api/tools/x", {
      headers: { host: "attacker.example", cookie: "sess=1", "x-keep": "yes" },
    }),
  );
  const h = new Headers(seen.init.headers);
  assert.equal(h.get("host"), null, "client Host must not ride upstream");
  assert.equal(h.get("cookie"), null, "client cookies must not ride upstream");
  assert.equal(h.get("authorization"), "Bearer tok-device-1");
  assert.equal(h.get("x-keep"), "yes", "unrelated headers pass through");
});

test("deviceFetch: uppercase registration hostname still dials (round-121 case-insensitive)", async () => {
  const seen = {};
  // CAPITALISED ON PURPOSE — the rule lowercases both sides (device-fetch.ts), so this case only works if the fixture and
  // the declared suffix agree in every spelling, which is why `grep -i` is the checklist for this migration (round 114).
  const upper = { hostname: "D1.Agent.Summrise.Test", token: "tok-device-1" };
  const r = await withStubFetch(okUpstream(seen), () => deviceFetch(ENV, upper, "/api/status"));
  assert.equal(r.status, 200);
  assert.match(seen.url, /^https:\/\/d1\.agent\.summrise\.test\//i);
});

test("deviceFetch: private device hostname → 400 via deviceHostError, never dialed", async () => {
  const r = await withStubFetch(
    async () => {
      throw new Error("must not be called");
    },
    () => deviceFetch(ENV, { hostname: "169.254.169.254", token: "tok-x" }, "/api/status"),
  );
  assert.equal(r.status, 400);
  assert.match(r.error || "", /private\/internal/);
});

test("deviceFetch: unreachable device → 502 with reason, no throw", async () => {
  const r = await withStubFetch(
    async () => {
      throw new TypeError("fetch failed");
    },
    () => deviceFetch(ENV, DEV, "/api/status"),
  );
  assert.equal(r.status, 502);
  assert.equal(r.ok, false);
  assert.match(r.error || "", /Device unreachable: fetch failed/);
});

/* ---------------- redirects are NOT followed on the device dial ----------------
 * A security audit found the device path used the default `redirect: "follow"`, which
 * is two holes in one:
 *   1. the SSRF guard runs on the INITIAL url only, so a redirect to 127.0.0.1 or a
 *      link-local metadata address is never checked;
 *   2. Cloudflare forwards ALL headers to a cross-host redirect target — documented
 *      behaviour, "even if the destination is a different hostname or domain … this
 *      includes sensitive headers like Cookie, Authorization" — and this path sends a
 *      `Bearer <device token>` and `x-summrise-auth: <proxySecret>`.
 * The fix is `redirect: "manual"`, which turns both into a visible 3xx.
 */
test("device dial: the request is issued with redirect:'manual', never 'follow'", async () => {
  const seen = [];
  const real = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    seen.push({ url: String(url), redirect: init?.redirect });
    return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
  };
  try {
    await deviceFetch(
      ENV,
      { name: "d1", hostname: "d1.agent.summrise.test", token: "t".repeat(64) },
      "/api/status",
      {},
    );
  } finally {
    globalThis.fetch = real;
  }
  assert.equal(seen.length, 1, "exactly one dial");
  assert.equal(
    seen[0].redirect,
    "manual",
    "the device dial must NOT follow redirects: the guard only sees the initial url, and Cloudflare forwards Authorization/x-summrise-auth to a cross-host Location",
  );
});
