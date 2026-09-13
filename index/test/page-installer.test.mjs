// The landing page's installer button (round 125).
//
// WHY THIS FILE EXISTS. Measured live on 2026-09-14: the CDN's
// `ValeAgent-Setup.exe` carried etag `f1dc1c8e86ab125f9f8d078b23ed400c` — byte
// for byte the same file as `ValeAgent-Setup-1.2.361.exe` — while the current
// release was 1.2.364 and `/api/version` advertised NO installer at all
// (1.2.364 was published tgz-only, the documented emergency path). The page
// linked that alias unconditionally, so "Download Windows installer" handed a
// fresh install a three-release-old build and said nothing.
//
// The rule these pin: the door offers an installer only when the release
// describes one.
import test from "node:test";
import assert from "node:assert/strict";
import { PAGE } from "../src/page.js";

const page = (setupUrl) =>
  PAGE(
    "https://console.example",
    "https://cdn.example/vale-agent/vale-agent-latest.tgz",
    setupUrl,
  );

test("an advertised installer is linked", () => {
  const html = page("https://cdn.example/vale-agent/ValeAgent-Setup.exe");
  assert.match(html, /Download Windows installer/);
  assert.match(
    html,
    /href="https:\/\/cdn\.example\/vale-agent\/ValeAgent-Setup\.exe"/,
    "the caller's URL must be the one linked",
  );
});

test("no advertised installer = no button, no alias, and a truthful hint", () => {
  for (const missing of [null, undefined, ""]) {
    const html = page(missing);
    assert.doesNotMatch(
      html,
      /Download Windows installer/,
      `setupUrl=${String(missing)} must not render the button`,
    );
    assert.doesNotMatch(
      html,
      /ValeAgent-Setup\.exe/,
      "a stale alias link is worse than no link: name nothing you cannot back",
    );
    assert.match(html, /No Windows installer is published for this release/);
    assert.match(html, /npm i -g/, "the channel that DOES work must stay");
  }
});
