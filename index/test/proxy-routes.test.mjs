// The three binary proxies: what they serve, how they FAIL, and how a device can tell
// a stale bundle from a current one. Round-99 F2/F3 named both gaps:
//   F2 — `summrise-playwright.zip` (an EXECUTED artifact) was served `public, max-age=86400`
//        with no validator, at a mutable key: a device could be handed a stale archive for
//        a day, and `summrise setup` stages it without hashing.
//   F3 — the three routes answered failures with bare text, and a REJECTED fetch had no
//        try/catch, so a GitHub outage became the platform's 500 HTML page — which no
//        device-side reader parses. electron-route.test.mjs recorded that gap in its own
//        header and deliberately did not cement it; these pins close it.
// Zero network: upstream traffic is stubbed, R2 is the shared in-memory mock.
import test from "node:test";
import assert from "node:assert/strict";
import worker from "../src/index.js";
import { makeR2, assertJsonError } from "./helpers.mjs";

const PW = "/summrise-agent/summrise-playwright.zip";
const CF = "/summrise-agent/cloudflared.exe";
const EL = "/summrise-agent/electron-win32-x64.zip";

async function withStubFetch(handler, fn) {
  const real = globalThis.fetch;
  globalThis.fetch = handler;
  try {
    return await fn();
  } finally {
    globalThis.fetch = real;
  }
}

const get = (path, headers = {}) =>
  new Request(`https://dl.example${path}`, { headers });

// ── F2: the mutable bundle must come with a validator, and revalidation must work ──

test("playwright bundle: 200 carries a content-derived ETag, no-cache and the real size", async () => {
  const r2 = makeR2();
  await r2.put("summrise-playwright.zip", "BUNDLE-V1");
  const resp = await worker.fetch(get(PW), { TEMP_FILES: r2 });
  assert.equal(resp.status, 200);
  const etag = resp.headers.get("etag");
  assert.match(etag || "", /^"[0-9a-f]{32}"$/, `etag must be a quoted digest: ${etag}`);
  assert.equal(resp.headers.get("cache-control"), "public, no-cache");
  assert.equal(resp.headers.get("content-length"), String("BUNDLE-V1".length));
  assert.equal(resp.headers.get("content-disposition"), 'attachment; filename="summrise-playwright.zip"');
  assert.equal(await resp.text(), "BUNDLE-V1");
});

test("playwright bundle: a matching If-None-Match is a 304 with no body — not a second download", async () => {
  const r2 = makeR2();
  await r2.put("summrise-playwright.zip", "BUNDLE-V1");
  const first = await worker.fetch(get(PW), { TEMP_FILES: r2 });
  const etag = first.headers.get("etag");
  await first.text();

  const second = await worker.fetch(get(PW, { "if-none-match": etag }), { TEMP_FILES: r2 });
  assert.equal(second.status, 304);
  assert.equal(second.headers.get("etag"), etag);
  assert.equal(second.headers.get("cache-control"), "public, no-cache");
  assert.equal(await second.text(), "", "a 304 must carry no body");
});

test("playwright bundle: REPLACING the bundle changes the ETag — the staleness F2 named", async () => {
  const r2 = makeR2();
  await r2.put("summrise-playwright.zip", "BUNDLE-V1");
  const v1 = await worker.fetch(get(PW), { TEMP_FILES: r2 });
  const etag1 = v1.headers.get("etag");
  await v1.text();

  await r2.put("summrise-playwright.zip", "BUNDLE-V2");
  const stale = await worker.fetch(get(PW, { "if-none-match": etag1 }), { TEMP_FILES: r2 });
  assert.equal(stale.status, 200, "a device holding the OLD validator must be given the NEW bytes");
  const etag2 = stale.headers.get("etag");
  assert.notEqual(etag2, etag1, "a new body must not keep the old validator");
  assert.equal(await stale.text(), "BUNDLE-V2");
});

// ── F3: every failure speaks the file's JSON protocol, and none of them is cacheable ──

test("playwright bundle: missing from R2 → 502 JSON, not bare text", async () => {
  const resp = await worker.fetch(get(PW), { TEMP_FILES: makeR2() });
  await assertJsonError(resp, 502, "playwright bundle unavailable: not in R2");
  assert.equal(resp.headers.get("cache-control"), "no-store", "a failure must not be cached");
});

test("playwright bundle: an R2 read that THROWS → 502 JSON, not a platform 500", async () => {
  const env = {
    TEMP_FILES: {
      async get() {
        throw new Error("r2 down");
      },
    },
  };
  const resp = await worker.fetch(get(PW), env);
  await assertJsonError(resp, 502, "playwright bundle read failed: Error: r2 down");
  assert.equal(resp.headers.get("cache-control"), "no-store");
});

test("cloudflared proxy: a REJECTED fetch → 502 JSON (the path left uncemented before)", async () => {
  const resp = await withStubFetch(
    async () => {
      throw new TypeError("fetch failed");
    },
    () => worker.fetch(get(CF), {}),
  );
  await assertJsonError(resp, 502, "cloudflared upstream fetch failed: TypeError: fetch failed");
  assert.equal(resp.headers.get("cache-control"), "no-store");
});

test("electron proxy: a REJECTED fetch → 502 JSON (the path left uncemented before)", async () => {
  const resp = await withStubFetch(
    async () => {
      throw new TypeError("fetch failed");
    },
    () => worker.fetch(get(EL), {}),
  );
  await assertJsonError(resp, 502, "electron upstream fetch failed: TypeError: fetch failed");
  assert.equal(resp.headers.get("cache-control"), "no-store");
});

test("cloudflared and electron: a non-ok upstream answers the SAME envelope as a throw", async () => {
  for (const [path, what] of [
    [CF, "cloudflared upstream fetch failed"],
    [EL, "electron upstream fetch failed"],
  ]) {
    const resp = await withStubFetch(
      async () => new Response("nope", { status: 503 }),
      () => worker.fetch(get(path), {}),
    );
    await assertJsonError(resp, 502, `${what}: 503`);
    assert.equal(resp.headers.get("cache-control"), "no-store", `${path} failure is not cacheable`);
  }
});
