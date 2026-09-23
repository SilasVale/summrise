// /api/version tarball-field tests (P2-1, node:test, no CF runtime).
//
// The version.json `tarball` field is live data: /api/version must serve
// exactly the file it names (validated to a flat basename), falling back
// to the derived versioned name only when the field is absent/invalid so
// older manifests keep working. Smoke (scripts/smoke-index.sh) pins the
// consistent case live (tarball field == download basename).
import test from "node:test";
import assert from "node:assert/strict";
import worker from "../src/index.js";

function versionEnv(versionJson) {
  return {
    TEMP_CLAIM: {
      idFromName: (name) => ({ __name: name }),
      get: () => ({ fetch: async () => new Response("unused") }),
    },
    ASSETS: {
      fetch: async () =>
        new Response(JSON.stringify(versionJson), { headers: { "content-type": "application/json" } }),
    },
  };
}

const GOOD_SHA = "b".repeat(64);

// ── the components block (grilling Q4) ──────────────────────────────────────
// `summrise setup` verifies each boxed component against a sha256 in THIS manifest;
// without the pins it can only warn that it verified nothing. The pins are DATA
// written by the release flow, so they get the same treatment as `tarball` and
// `installer`: validated, and the URL REBUILT against this request's origin.

test("components: pins pass through, and the URL is rebuilt against THIS origin", async () => {
  const resp = await worker.fetch(
    new Request("https://dl.local/api/version"),
    versionEnv({
      version: "1.2.3",
      tarball: "summrise-agent-latest.tgz",
      sha256: GOOD_SHA,
      components: {
        cloudflared: { url: "https://evil.example/depot/cloudflared.exe", sha256: GOOD_SHA },
      },
    }),
  );
  const j = await resp.json();
  assert.equal(
    j.components.cloudflared.url,
    "https://dl.local/summrise-agent/cloudflared.exe",
    "a manifest must not be able to point a device at another host",
  );
  assert.equal(j.components.cloudflared.sha256, GOOD_SHA);
});

test("components: a bad entry is DROPPED rather than served", async () => {
  const resp = await worker.fetch(
    new Request("https://dl.local/api/version"),
    versionEnv({
      version: "1.2.3",
      tarball: "summrise-agent-latest.tgz",
      sha256: GOOD_SHA,
      components: {
        good: { url: "https://dl.local/summrise-agent/electron-win32-x64.zip", sha256: GOOD_SHA },
        // A path, not a flat basename: it must not survive.
        path: { url: "https://dl.local/summrise-agent/../../secrets", sha256: GOOD_SHA },
        // A digest-shaped lie is still not a digest.
        sha: { url: "https://dl.local/summrise-agent/x.zip", sha256: "not-a-sha" },
        // Not an object at all.
        junk: "cloudflared.exe",
      },
    }),
  );
  const j = await resp.json();
  // `path` SURVIVES — NORMALISED, not trusted. Only the flat basename is used and the
  // URL is rebuilt against this origin, so "../../secrets" becomes
  // <origin>/summrise-agent/secrets: our host, our route, and a 404 at fetch time.
  // Canonicalising the manifest's own spelling is the treatment `tarball` already
  // gets. What must NOT survive is a digest-shaped lie or a non-object at all.
  assert.deepEqual(Object.keys(j.components).sort(), ["good", "path"]);
  assert.equal(j.components.path.url, "https://dl.local/summrise-agent/secrets");
  assert.equal(j.components.path.sha256, GOOD_SHA);
});

test("components: a manifest without them serves a manifest without them", async () => {
  const resp = await worker.fetch(
    new Request("https://dl.local/api/version"),
    versionEnv({ version: "1.2.3", tarball: "summrise-agent-latest.tgz", sha256: GOOD_SHA }),
  );
  const j = await resp.json();
  assert.equal("components" in j, false, "an older release must keep working unchanged");
});

test("tarball field is honored: latest alias served verbatim", async () => {
  const resp = await worker.fetch(
    new Request("https://dl.local/api/version"),
    versionEnv({ version: "1.2.3", tarball: "summrise-agent-latest.tgz", sha256: GOOD_SHA }),
  );
  assert.equal(resp.status, 200);
  const j = await resp.json();
  assert.equal(j.download, "https://dl.local/summrise-agent/summrise-agent-latest.tgz");
});

test("tarball field is honored: versioned name served verbatim", async () => {
  const resp = await worker.fetch(
    new Request("https://dl.local/api/version"),
    versionEnv({ version: "1.2.3", tarball: "summrise-agent-1.2.3.tgz", sha256: GOOD_SHA }),
  );
  assert.equal(resp.status, 200);
  const j = await resp.json();
  assert.equal(j.download, "https://dl.local/summrise-agent/summrise-agent-1.2.3.tgz");
});

test("absent tarball falls back to the derived versioned name (old manifests)", async () => {
  const resp = await worker.fetch(
    new Request("https://dl.local/api/version"),
    versionEnv({ version: "1.2.3", sha256: GOOD_SHA }),
  );
  assert.equal(resp.status, 200);
  const j = await resp.json();
  assert.equal(j.download, "https://dl.local/summrise-agent/summrise-agent-1.2.3.tgz");
});

test("hostile tarball (slashes / wrong suffix) falls back, never escapes /summrise-agent/", async () => {
  for (const tarball of ["../secret.tgz", "/etc/passwd", "summrise-agent-1.2.3.zip", "", null, undefined, 42]) {
    const resp = await worker.fetch(
      new Request("https://dl.local/api/version"),
      versionEnv({ version: "1.2.3", tarball, sha256: GOOD_SHA }),
    );
    assert.equal(resp.status, 200, `tarball ${JSON.stringify(tarball)} must fall back, not 503`);
    const j = await resp.json();
    assert.equal(j.download, "https://dl.local/summrise-agent/summrise-agent-1.2.3.tgz");
    assert.ok(!j.download.includes(".."), "download URL must not contain path traversal");
  }
});
