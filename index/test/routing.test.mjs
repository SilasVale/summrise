// Worker static-routing regression tests (structure refactor round — these
// routes previously had ZERO coverage). The Setup.exe serving exists
// precisely because a past bug served the download PAGE as 200 HTML for a
// missing binary (devices silently downloaded HTML as SummriseAgent-Setup.exe);
// these tests pin the documented contract so it can't regress.
import test from "node:test";
import assert from "node:assert/strict";
import worker from "../src/index.js";
import { readFile } from "node:fs/promises";

// ASSETS stub: answer version.json with JSON, everything else with an
// octet-stream "binary" so tgz routing is observable (status + pass-through).
function makeEnv(versionJson) {
  const assetsFetches = [];
  return {
    assetsFetches,
    env: {
      CONSOLE_URL: "https://console.example",
      ASSETS: {
        async fetch(req) {
          assetsFetches.push(String(req.url));
          const path = new URL(req.url).pathname;
          if (path === "/summrise-agent/version.json") {
            if (versionJson === null)
              return new Response("no such key", { status: 404 });
            return new Response(JSON.stringify(versionJson), {
              headers: { "content-type": "application/json" },
            });
          }
          return new Response("fake-binary", {
            status: 200,
            headers: { "content-type": "application/octet-stream" },
          });
        },
      },
    },
  };
}

test("versioned tgz + latest alias both serve from ASSETS", async () => {
  const { env, assetsFetches } = makeEnv(null);
  for (const p of [
    "/summrise-agent/summrise-agent-1.2.297.tgz",
    "/summrise-agent/summrise-agent-latest.tgz",
  ]) {
    const resp = await worker.fetch(new Request(`https://dl.local${p}`), env);
    assert.equal(resp.status, 200, p);
    assert.equal(await resp.text(), "fake-binary", p);
  }
  assert.equal(assetsFetches.length, 2, "both requests must reach ASSETS");
});

test("near-miss tgz paths are NOT routed to ASSETS (exact-pattern discipline)", async () => {
  const { env, assetsFetches } = makeEnv(null);
  for (const p of [
    "/summrise-agent/summrise-agent-latest.tgz.exe",
    "/summrise-agent/summrise-agent-1.2.tgz",
    "/summrise-agent/summrise-agent-1.2.297.tgz/",
    "/summrise-agent/evil-1.2.297.tgz",
  ]) {
    const resp = await worker.fetch(new Request(`https://dl.local${p}`), env);
    assert.equal(resp.status, 404, `${p} must fall to the 404 fallback`);
  }
  assert.equal(assetsFetches.length, 0, "no near-miss may reach ASSETS");
});

test("SummriseAgent-Setup.exe alias + versioned names serve from ASSETS", async () => {
  const { env, assetsFetches } = makeEnv(null);
  for (const p of [
    "/summrise-agent/SummriseAgent-Setup.exe",
    "/summrise-agent/SummriseAgent-Setup-1.2.307.exe",
  ]) {
    const resp = await worker.fetch(new Request(`https://dl.local${p}`), env);
    assert.equal(resp.status, 200, p);
    assert.equal(await resp.text(), "fake-binary", p);
  }
  assert.equal(assetsFetches.length, 2, "both requests must reach ASSETS");
});

test("near-miss Setup.exe paths are NOT routed to ASSETS (exact-pattern discipline)", async () => {
  const { env, assetsFetches } = makeEnv(null);
  for (const p of [
    "/summrise-agent/SummriseAgent-Setup.exe.exe",
    "/summrise-agent/SummriseAgent-Setup-1.2.exe",
    "/summrise-agent/SummriseAgent-Setup-1.2.307.exe/",
    "/summrise-agent/summriseagent-setup.exe",
    "/summrise-agent/SummriseAgent-Setup-1.2.307.tgz",
  ]) {
    const resp = await worker.fetch(new Request(`https://dl.local${p}`), env);
    assert.equal(resp.status, 404, `${p} must fall to the 404 fallback`);
  }
  assert.equal(assetsFetches.length, 0, "no near-miss may reach ASSETS");
});

test("/api/version serves the release manifest derived from version.json", async () => {
  const { env } = makeEnv({
    version: "1.2.297",
    sha256: "a".repeat(64),
    tarball: "summrise-agent-latest.tgz",
  });
  const resp = await worker.fetch(
    new Request("https://dl.local/api/version"),
    env,
  );
  assert.equal(resp.status, 200);
  const body = await resp.json();
  assert.equal(body.version, "1.2.297");
  assert.equal(body.sha256, "a".repeat(64));
  assert.equal(
    body.download,
    "https://dl.local/summrise-agent/summrise-agent-latest.tgz",
  );
});

test("/api/version fails honest 503 on missing/unverifiable manifest", async () => {
  // No version.json at all.
  const missing = await worker.fetch(
    new Request("https://dl.local/api/version"),
    makeEnv(null).env,
  );
  assert.equal(missing.status, 503);
  // Truncated sha (round-119: agent_update refuses unverifiable installs —
  // the worker must not serve one either).
  const badSha = await worker.fetch(
    new Request("https://dl.local/api/version"),
    makeEnv({
      version: "1.2.297",
      sha256: "abc",
      tarball: "summrise-agent-latest.tgz",
    }).env,
  );
  assert.equal(badSha.status, 503);

  // AND THE FAILURE MUST NOT BE CACHEABLE — the assertion this test was missing (round 130). The 503 was
  // hand-rolled with no headers at all, while every other failure this worker returns goes through
  // `proxyFailure`, which sets `no-store`. A 503 with no directives may be stored by a shared cache, and
  // this is the route every device's updater calls: the edge would keep answering a failure long after the
  // manifest was fixed, which is the "cached failure outlives the outage" shape the helper exists for.
  for (const [label, res] of [["missing manifest", missing], ["unverifiable manifest", badSha]]) {
    assert.equal(res.headers.get("cache-control"), "no-store", `${label}: a cacheable 503 blocks updates`);
    assert.match(res.headers.get("content-type") || "", /application\/json/, `${label}: the body is JSON`);
  }
});

test("unknown paths 404 (never the landing page as 200 HTML) and / renders it", async () => {
  const notFound = await worker.fetch(
    new Request("https://dl.local/nope"),
    makeEnv(null).env,
  );
  assert.equal(notFound.status, 404);
  assert.notEqual(notFound.headers.get("content-type") || "", "text/html");

  const { env } = makeEnv(null);
  const page = await worker.fetch(new Request("https://dl.local/"), env);
  assert.equal(page.status, 200);
  assert.match(page.headers.get("content-type") || "", /text\/html/);
  assert.match(await page.text(), /<!doctype html>/);

  // AND THE LANDING CARRIES A CACHE POLICY (round 133). It was the ONLY response in that worker without
  // one — six siblings set `no-store`, `max-age` or `no-cache` — and a response with no `cache-control` is
  // heuristically cacheable, so a browser could keep showing the old install instructions long after a
  // deploy replaced them. This test asserted status and content-type only, which is why it survived.
  assert.equal(
    page.headers.get("cache-control"),
    "public, no-cache",
    "the landing must revalidate: it carries the install instructions",
  );
  const etag = page.headers.get("etag");
  assert.ok(etag, "and carry a validator, so revalidation can answer 304 rather than 30 KB");
  const revalidated = await worker.fetch(
    new Request("https://dl.local/", { headers: { "if-none-match": etag } }),
    env,
  );
  assert.equal(revalidated.status, 304, "a matching validator must not re-send the page");
});

test("cloudflared.exe proxies GitHub: pass-through on success, 502 on failure", async () => {
  const real = globalThis.fetch;
  try {
    globalThis.fetch = async () =>
      new Response("clfz-binary", {
        status: 200,
        headers: { "content-type": "application/octet-stream" },
      });
    const ok = await worker.fetch(
      new Request("https://dl.local/summrise-agent/cloudflared.exe"),
      makeEnv(null).env,
    );
    assert.equal(ok.status, 200);
    assert.equal(ok.headers.get("content-type"), "application/octet-stream");
    assert.equal(await ok.text(), "clfz-binary");

    // AND IT REVALIDATES, which is what `public/_headers` requires for this whole prefix (round 135).
    // The route used to answer `public, max-age=3600` on a STABLE url whose bytes move with
    // CLOUDFLARED_VERSION, so the edge could hand out the previous binary for an hour while version.json's
    // pin had already moved — `summise setup` then REFUSES the mismatch, fail-closed but unable to install
    // until the cache expires. A long max-age here is the defect, whatever the bandwidth argument for it.
    assert.equal(
      ok.headers.get("cache-control"),
      "public, no-cache",
      "a mutable artifact URL must revalidate — _headers says never serve a stale body",
    );
    assert.doesNotMatch(
      ok.headers.get("cache-control") || "",
      /max-age=[1-9]/,
      "a positive max-age is exactly what _headers exists to forbid on /summrise-agent/*",
    );

    globalThis.fetch = async () => new Response("nope", { status: 503 });
    const bad = await worker.fetch(
      new Request("https://dl.local/summrise-agent/cloudflared.exe"),
      makeEnv(null).env,
    );
    assert.equal(bad.status, 502);
    assert.match(await bad.text(), /503/);
  } finally {
    globalThis.fetch = real;
  }
});

/* ---------------- the cloudflared proxy is PINNED, not `latest` ----------------
 * An audit found this route proxied `.../releases/latest/...`, so the bytes of a binary the
 * service SPAWNS were chosen by whatever GitHub marked latest — while the agent's own
 * CLOUDFLARED_SHA256 pin only holds "while latest stays" the pinned version (that file says
 * so itself). Upstream moving a release therefore made the proxy serve bytes the pin
 * rejects: the on-demand path failed closed, and the INSTALLER staged the new bytes
 * unverified, because nothing in that chain hashes them.
 */
test("cloudflared proxy uses the IMMUTABLE versioned asset, never `releases/latest`", async () => {
  const src = await readFile(
    new URL("../src/index.js", import.meta.url),
    "utf8",
  );
  assert.ok(
    !src.includes("cloudflared/releases/latest"),
    "the proxy must not follow `latest`: those bytes are chosen by a third party and nothing downstream hashes them",
  );
  assert.match(
    src,
    /cloudflared\/releases\/download\/\$\{CLOUDFLARED_VERSION\}/,
    "and it must use the versioned path, which GitHub guarantees is immutable",
  );
});

/// The worker's pinned version and the agent's pinned version are ONE contract: the agent's
/// CLOUDFLARED_SHA256 is the hash of THIS asset. Drift is the bug, so the test reads the
/// Rust source rather than trusting a comment.
test("cloudflared_pin_matches_the_agent", async () => {
  const here = await readFile(
    new URL("../src/index.js", import.meta.url),
    "utf8",
  );
  const worker = here.match(/const CLOUDFLARED_VERSION = "([^"]+)"/)?.[1];
  assert.ok(worker, "the worker must declare CLOUDFLARED_VERSION");

  const rust = await readFile(
    new URL("../../agent/src/tunnel.rs", import.meta.url),
    "utf8",
  );
  const agent = rust.match(/const CLOUDFLARED_VERSION: &str = "([^"]+)"/)?.[1];
  assert.ok(agent, "the agent must declare CLOUDFLARED_VERSION");

  assert.equal(
    worker,
    agent,
    `the proxied cloudflared must be the version the agent pins (worker ${worker}, agent ${agent}) — a drift means the installer stages bytes nothing verifies`,
  );
});

test("a malformed component pin is dropped AND said out loud", async () => {
  // The drop is the SAFE half — an unverifiable digest must never ship as a pin. The silent half was
  // the defect (round 134): the manifest simply omitted the component, the device fetched it from the
  // bypass path, and setup printed "fetched WITHOUT a manifest pin" with nothing on this side to say why.
  const good = "a".repeat(64);
  const { env } = makeEnv({
    version: "1.2.297",
    sha256: good,
    tarball: "summrise-agent-1.2.297.tgz",
    components: {
      cloudflared: { url: "https://dl.local/summrise-agent/cloudflared.exe", sha256: "abc" },
      playwright: { url: "https://dl.local/summrise-agent/summrise-playwright.zip", sha256: good },
    },
  });
  const realWarn = console.warn;
  let said = "";
  console.warn = (m) => { said += String(m) + "\n"; };
  try {
    const res = await worker.fetch(new Request("https://dl.local/api/version"), env);
    const body = await res.json();
    assert.equal(body.components?.cloudflared, undefined, "an unverifiable pin must not be shipped");
    assert.ok(body.components?.playwright, "the well-formed sibling must still be pinned");
    assert.match(said, /cloudflared/, "and the drop must be visible on the worker's side");
    assert.match(said, /not a 64-hex digest/, "naming what was wrong, not merely that something was");
  } finally {
    console.warn = realWarn;
  }
});

test("public/_headers states the artifact rule, and no route contradicts it", async () => {
  // THE FILE GOVERNED EVERY /summrise-agent/* RESPONSE AND NOTHING READ IT (round 136). Its rule is
  // explicit and carries its reason — "Device artifacts change under stable URLs — never let the edge
  // serve a stale body (a device would silently receive an old build)" — and the cloudflared route still
  // answered `public, max-age=3600` inside that very prefix, overriding it in code where the file could
  // not see (round 135). A rule with no instrument is a comment.
  const headers = await readFile(new URL("../public/_headers", import.meta.url), "utf8");
  assert.match(headers, /\/summrise-agent\/\*/, "the artifact prefix must be covered");
  assert.match(headers, /no-cache|no-store/, "and must forbid a stale body");
  const rule = headers.slice(headers.indexOf("/summrise-agent/*"));
  assert.doesNotMatch(rule, /max-age=[1-9]/, "…and must not permit even a small positive max-age");

  // AND THE ROUTES AGREE. Electron answers from R2 with a validator; cloudflared revalidates through
  // GitHub's ETag. Both were asserted on their own responses, which is what caught the contradiction —
  // this asserts the file those assertions are FOR is still saying the same thing.
  const src = await readFile(new URL("../src/index.js", import.meta.url), "utf8");
  const positive = [...src.matchAll(/cache-control"?\s*:\s*"public, max-age=([1-9][0-9]*)/g)];
  assert.deepEqual(
    positive.map((m) => m[1]),
    [],
    "no route may answer a positive max-age for an artifact under this prefix",
  );
});
