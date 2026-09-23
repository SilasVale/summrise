// Electron runtime: STAGED IN R2, not proxied (2026-09-23). This file used to pin
// the proxy shape -- "No env bindings involved (pure fetch-through)" -- which was
// the right answer while the alternative was the DEVICE reaching GitHub for a
// 115MB runtime. Two things retired it:
//
//   1. the release flow cannot hash a proxy, and version.json now carries a sha256
//      per component so `summrise setup` can verify what it fetched;
//   2. the device's dependency on GitHub becomes ZERO instead of "the edge proxies
//      it for us", and R2 already holds the playwright bundle next door.
//
// So these tests pin the R2 shape -- and the fourth one pins the property the move
// exists for: the route must not touch the network AT ALL.
import test from "node:test";
import assert from "node:assert/strict";
import worker from "../src/index.js";
import { makeR2, assertJsonError } from "./helpers.mjs";

const PATH = "/summrise-agent/electron-win32-x64.zip";
const KEY = "electron-win32-x64.zip";
const get = (headers = {}) => new Request(`https://dl.example${PATH}`, { headers });

test("electron runtime: 200 from R2, content-derived ETag, and no-cache (not max-age)", async () => {
  const r2 = makeR2();
  await r2.put(KEY, "ELECTRON-ZIP");
  const r = await worker.fetch(get(), { TEMP_FILES: r2 });
  assert.equal(r.status, 200);
  assert.equal(r.headers.get("content-type"), "application/zip");
  assert.equal(
    r.headers.get("content-disposition"),
    'attachment; filename="electron-win32-x64.zip"',
  );
  // The proxy answered `public, max-age=86400`, i.e. a device could spend a day
  // being handed a stale archive after the object was replaced. Revalidate instead
  // -- the same rule the playwright bundle next door already carries (round-99 F2).
  assert.equal(r.headers.get("cache-control"), "public, no-cache");
  assert.match(
    r.headers.get("etag") || "",
    /^"[0-9a-f]{32}"$/,
    "a content digest, so replacing the object changes the validator",
  );
  assert.equal(await r.text(), "ELECTRON-ZIP", "body streams through untouched");
});

test("electron runtime: a matching If-None-Match is a 304, not a second 115MB download", async () => {
  const r2 = makeR2();
  await r2.put(KEY, "ELECTRON-ZIP");
  const first = await worker.fetch(get(), { TEMP_FILES: r2 });
  const etag = first.headers.get("etag");
  await first.text();

  const second = await worker.fetch(get({ "if-none-match": etag }), {
    TEMP_FILES: r2,
  });
  assert.equal(second.status, 304);
  assert.equal(second.headers.get("etag"), etag);
  assert.equal(await second.text(), "", "a 304 must carry no body");
});

test("electron runtime: REPLACING the object changes the ETag — the staleness F2 named", async () => {
  const r2 = makeR2();
  await r2.put(KEY, "V33-4-11");
  const v1 = await worker.fetch(get(), { TEMP_FILES: r2 });
  const etag1 = v1.headers.get("etag");
  await v1.text();

  await r2.put(KEY, "V33-4-12");
  const stale = await worker.fetch(get({ "if-none-match": etag1 }), {
    TEMP_FILES: r2,
  });
  assert.equal(
    stale.status,
    200,
    "a device holding the OLD validator must be given the NEW bytes",
  );
  assert.notEqual(stale.headers.get("etag"), etag1);
  assert.equal(await stale.text(), "V33-4-12");
});

test("electron runtime: it does NOT touch the network — the property the move exists for", async () => {
  const r2 = makeR2();
  await r2.put(KEY, "ELECTRON-ZIP");
  const real = globalThis.fetch;
  globalThis.fetch = async (url) => {
    throw new Error(`the electron route must not fetch anything, but it fetched ${url}`);
  };
  try {
    const r = await worker.fetch(get(), { TEMP_FILES: r2 });
    assert.equal(r.status, 200);
    assert.equal(await r.text(), "ELECTRON-ZIP");
  } finally {
    globalThis.fetch = real;
  }
});

test("electron runtime: nothing in R2 → 502 naming R2, not an upstream", async () => {
  const r = await worker.fetch(get(), { TEMP_FILES: makeR2() });
  await assertJsonError(r, 502, "electron runtime unavailable: not in R2");
  assert.equal(r.headers.get("cache-control"), "no-store");
});

test("electron runtime: an EMPTY object answers 502 too — absent and empty are ONE verdict", async () => {
  // Learned by doing it: an absent object 502s and setup warns, while an EMPTY
  // object would answer 200 and stage nothing — the false-success shape this
  // suite keeps finding. A zero-byte runtime is not a runtime.
  const r2 = makeR2();
  await r2.put(KEY, "");
  const r = await worker.fetch(get(), { TEMP_FILES: r2 });
  await assertJsonError(r, 502, "electron runtime unavailable: not in R2");
});

test("electron runtime: an R2 read that throws → 502 without leaking internals", async () => {
  const r2 = {
    get: async () => {
      throw new TypeError("r2 exploded");
    },
  };
  const r = await worker.fetch(get(), { TEMP_FILES: r2 });
  await assertJsonError(r, 502, "electron runtime read failed: TypeError: r2 exploded");
  assert.equal(r.headers.get("cache-control"), "no-store");
});
