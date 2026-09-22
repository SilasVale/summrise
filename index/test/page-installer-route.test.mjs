// The SERVED landing page, end to end through the worker's fetch handler.
//
// The template test (page-installer.test.mjs) proves the page honours a null
// installer URL. It does NOT prove the handler ever passes one — the handler
// decides by reading the manifest, and on 2026-09-14 that decision was missing
// entirely: the alias served the 1.2.361 installer (same etag) while the release
// was 1.2.364 and the manifest advertised nothing, so a fresh install silently
// got a three-release-old build.
//
// These two cases are that decision: manifest advertises an installer -> the
// button; manifest does not -> no button, no alias, npm channel intact.
import test from "node:test";
import assert from "node:assert/strict";
import worker from "../src/index.js";

const SHA = "a".repeat(64);

function envWithManifest(manifest) {
  return {
    CONSOLE_URL: "https://console.example",
    ASSETS: {
      async fetch(req) {
        const path = new URL(req.url).pathname;
        if (path === "/summrise-agent/version.json" && manifest) {
          return new Response(JSON.stringify(manifest), { status: 200 });
        }
        return new Response("not found", { status: 404 });
      },
    },
  };
}

async function landing(manifest) {
  const res = await worker.fetch(
    new Request("https://agent.example/"),
    envWithManifest(manifest),
    {},
  );
  assert.equal(res.status, 200, "the landing page must still render");
  return res.text();
}

test("a release that published an installer gets the button", async () => {
  const html = await landing({
    version: "1.2.364",
    tarball: "summrise-agent-latest.tgz",
    sha256: SHA,
    installer: "SummriseAgent-Setup-1.2.364.exe",
    installer_sha256: SHA,
  });
  assert.match(html, /Download Windows installer/);
  assert.match(html, /SummriseAgent-Setup\.exe/);
});

test("a tgz-only release gets no installer button and no alias link", async () => {
  const html = await landing({
    version: "1.2.364",
    tarball: "summrise-agent-latest.tgz",
    sha256: SHA,
  });
  assert.doesNotMatch(
    html,
    /Download Windows installer/,
    "the alias still serves the PREVIOUS release — offering it silently ships an old build",
  );
  assert.doesNotMatch(html, /SummriseAgent-Setup\.exe/);
  assert.match(html, /No Windows installer is published for this release/);
  assert.match(html, /npm i -g/, "the channel that works must stay");
});

test("an unreadable manifest makes no promise either", async () => {
  const html = await landing(null); // ASSETS answers 404
  assert.doesNotMatch(html, /Download Windows installer/);
  assert.match(html, /npm i -g/);
});

test("a HALF-written installer manifest is not a promise", async () => {
  // The smoke already treats `installer` without `installer_sha256` as an error
  // ("both or neither"), so the page must not be more credulous than the smoke:
  // a URL with no digest is an artifact nobody can verify.
  for (const half of [
    { installer: "SummriseAgent-Setup-1.2.364.exe" },
    { installer_sha256: SHA },
  ]) {
    const html = await landing({
      version: "1.2.364",
      tarball: "summrise-agent-latest.tgz",
      sha256: SHA,
      ...half,
    });
    assert.doesNotMatch(
      html,
      /Download Windows installer/,
      `half-present manifest rendered the button: ${JSON.stringify(half)}`,
    );
  }
});
