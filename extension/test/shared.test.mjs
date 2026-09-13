// shared.js guard pins (SOLID Round-24 — the extension's FIRST unit
// tests: CI gated syntax only until now). httpsOrigin is the MITM guard
// for the code-server session cookies: http must never pass, and only a
// bare origin may come out. shared.js stays a classic script (manifest
// load order + isolated-world channel); the module.exports shim at its
// tail is inert in the browser and feeds this file under node.
import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";

const require = createRequire(import.meta.url);
const { DEFAULT_STUDIO_ORIGIN, httpsOrigin, resolveDir, studioFolderUrl, extractPathJobs } = require("../shared.js");

test("default origin is the code-server host", () => {
  assert.equal(DEFAULT_STUDIO_ORIGIN, "https://vscode.saisi.online");
  assert.equal(httpsOrigin(DEFAULT_STUDIO_ORIGIN), DEFAULT_STUDIO_ORIGIN, "default passes its own guard");
});

test("https URLs normalize to the bare origin (path/query/port kept correctly)", () => {
  assert.equal(httpsOrigin("https://vscode.saisi.online/a/b?q=1"), "https://vscode.saisi.online");
  assert.equal(httpsOrigin("https://h.example:8443/x"), "https://h.example:8443", "explicit port kept");
  assert.equal(httpsOrigin("HTTPS://UPPER.example/x"), "https://upper.example", "scheme/host normalized");
});

test("http never passes (MITM guard for session cookies)", () => {
  for (const v of ["http://vscode.saisi.online/", "http://localhost:8080/", "http://127.0.0.1/"]) {
    assert.equal(httpsOrigin(v), null, `cleartext rejected: ${v}`);
  }
});

test("non-URLs are null, never throw", () => {
  for (const v of ["", "notaurl", "//protocol-relative", "ftp://h.example/x", null, undefined, 42]) {
    assert.equal(httpsOrigin(v), null, `rejected: ${JSON.stringify(v)}`);
  }
});

// Path→folder resolution (SOLID Round-96 — verbatim core of the content
// script's resolve; the TTL cache stays page-side, this mapping is pure).
test("resolveDir: absolute file → folder, absolute dir kept, slashes trimmed", () => {
  assert.equal(resolveDir("/home/zhengsaisi/vale/gateway/src/index.ts"), "/home/zhengsaisi/vale/gateway/src");
  assert.equal(resolveDir("/home/zhengsaisi/vale/"), "/home/zhengsaisi/vale");
  // `/` is outside the studio root, so there is no folder to open — refused.
  assert.equal(resolveDir("/"), "");
  assert.equal(resolveDir("/home/zhengsaisi/vale/a/b///"), "/home/zhengsaisi/vale/a/b");
  // ...and this function no longer answers for paths OUTSIDE the studio root, which
  // the old fake-path cases did by accident (`/a/b` was a link nobody could open).
  assert.equal(resolveDir("/a/b///"), "");
});

test("resolveDir: relative joins the WORKSPACE, file part stripped", () => {
  // Re-pinned: this used to assert `/home/zhengsaisi/gateway/src`, which does not
  // exist — the base was the user's home directory while DSH runs in the project.
  assert.equal(resolveDir("gateway/src/index.ts"), "/home/zhengsaisi/vale/gateway/src");
  assert.equal(resolveDir("docs/x.md"), "/home/zhengsaisi/vale/docs");
  assert.equal(resolveDir("notes/"), "/home/zhengsaisi/vale/notes");
  assert.equal(resolveDir("./gateway/src/index.ts"), "/home/zhengsaisi/vale/gateway/src");
  assert.equal(resolveDir("a/b.js", "/home/zhengsaisi/x"), "/home/zhengsaisi/x/a");
});

// The allowlist (round 121). A chat message is untrusted text, and this function
// turns it into a one-click link inside an ALREADY-AUTHENTICATED IDE session — so
// what it refuses matters more than what it resolves.
test("resolveDir refuses what is not the project's to open", () => {
  for (const evil of [
    "/home/zhengsaisi/.ssh/id_rsa", // the key directory
    "/home/zhengsaisi/.dsh/settings.yaml", // the harness's own config
    "/home/zhengsaisi/.aws/credentials",
    "/home/zhengsaisi/.gnupg/secring.gpg",
    "/etc/passwd", // outside the code-server root entirely
    "/home/zhengsaisi", // the root itself is not a mention's folder
    "../../etc/passwd", // traversal is refused, not normalised
    "/home/zhengsaisi/vale/../../.ssh/id_rsa",
    "",
  ]) {
    assert.equal(resolveDir(evil), "", `accepted: ${evil}`);
  }
  // ...while project content still resolves, including the ONE dot-directory that
  // is content rather than credentials.
  assert.equal(resolveDir("/home/zhengsaisi/vale/.github/workflows"), "/home/zhengsaisi/vale/.github/workflows");
  assert.equal(resolveDir(".github/workflows/ci.yml"), "/home/zhengsaisi/vale/.github/workflows");
});

test("studioFolderUrl: folder encoded under the origin", () => {
  assert.equal(
    studioFolderUrl("https://vscode.saisi.online", "/home/zhengsaisi/vale"),
    "https://vscode.saisi.online/?folder=%2Fhome%2Fzhengsaisi%2Fvale",
  );
});

// Mention matcher (SOLID Round-96 — verbatim core of the per-node scan).
test("extractPathJobs: absolute + line, relative, bare filename; noise skipped", () => {
  const jobs = extractPathJobs("see /a/b/c.rs:42 and docs/x.md plus README and ab");
  assert.deepEqual(
    jobs.map((j) => [j.raw, j.bare, j.lineNo]),
    [
      ["/a/b/c.rs:42", "/a/b/c.rs", 42],
      ["docs/x.md", "docs/x.md", 0],
    ],
  );
  assert.ok(jobs[0].index < jobs[1].index, "document order with indices");
  // Repeat call: the shared /g regex must reset, not resume mid-stream.
  assert.deepEqual(
    extractPathJobs("see /a/b/c.rs:42").map((j) => j.raw),
    ["/a/b/c.rs:42"],
  );
});

test("extractPathJobs: empty and prose-only yield nothing", () => {
  assert.deepEqual(extractPathJobs(""), []);
  assert.deepEqual(extractPathJobs("just some words here"), []);
  // Prose with slashes used to become links through BOTH arms: `read/write` via
  // the relative one, its `/write` via the absolute one.
  for (const prose of [
    "either read/write or and/or him/her",
    "24/7 support",
    "TCP/IP stack",
    "input/output and/or",
  ]) {
    assert.deepEqual(extractPathJobs(prose), [], `prose became links: ${prose}`);
  }
  // A path that is real but unopenable is still MATCHED — it is resolveDir that
  // refuses it, and the content script then leaves the text alone.
  assert.equal(resolveDir(extractPathJobs("see /usr/bin/env bash")[0].bare), "");
});

// X1's decision is a DEFAULT, and a default lives in the content script (which
// cannot be imported here — it talks to chrome.*). Pinned by reading it, the
// instrument this repo uses where a platform cannot be driven. ADR 0010 records
// why: the rewrite freezes the host client's streaming replies.
test("linkify is OFF by default and opts IN explicitly", () => {
  const src = readFileSync(new URL("../content/studio-links.js", import.meta.url), "utf8");
  assert.match(src, /enabled: false/, "the default must be off");
  assert.match(src, /studioLinksEnabled === true/, "the opt-in must be explicit");
  // The third assertion here was `doesNotMatch(src, /studioLinksEnabled !== false/)`.
  // DELETED in round 162, not converted: the two positive assertions above already
  // pin the same fact (the default is off, the opt-in is explicit), so the absence
  // form added no unique coverage — and it carried the round-159 trap, where a grep
  // for a retired form matches the comment that retires it. Round 161's rule, applied:
  // when the subject is a FILE, assert the positive ACT; absence is for runtime VALUES.
});

/* ---- the options page states the state that is in effect (round 143) ---- */

test("options/options.js: opt-in checkbox, no silent substitution, no phantom password", () => {
  // The options page cannot be imported (it talks to chrome.* + the DOM), so these
  // are SOURCE checks — and they match the ACT, not a word: round 124 taught that a
  // pin satisfied by an explanatory comment proves nothing.
  const src = readFileSync(new URL("../options/options.js", import.meta.url), "utf8");

  // 1. The checkbox must use the SAME predicate the content script does (=== true).
  // With `!== false` a user who never touched the setting saw it CHECKED while the
  // feature was off — the UI asserting a state that was not in effect.
  assert.match(
    src,
    /studioLinksEnabled"\)\.checked = st\.studioLinksEnabled === true;/,
    "the toggle must be opt-IN, matching content/studio-links.js",
  );

  // 2. An unusable origin must be REFUSED, not replaced by the default.
  assert.doesNotMatch(
    src,
    /httpsOrigin\(raw\) \|\| DEFAULT_STUDIO_ORIGIN/,
    "an invalid origin must not be silently replaced by the default",
  );

  // 3. The file must STATE the true security model (round 141 fixed shared.js; this
  // file carried a second copy — the pair-defect). POSITIVE assertion, deliberately
  // (round 160): this was `assert.doesNotMatch(src, /code-server\s+password/i)`, and
  // round 159's closure check proved what is wrong with that form — a grep for an
  // ABSENT claim cannot tell the claim from its REFUTATION, and every honest fix of
  // this kind writes the very phrase it retracts. The absence form would therefore
  // false-alarm the day someone documents the fix here; the positive form cannot.
  // It is still a documentation check, not a behavioural one — it asserts what the
  // file SAYS, because what this file does is describe the gate.
  assert.match(
    src,
    /--auth none/,
    "options.js must state that code-server runs --auth none (Access is the only gate)",
  );
});

/* ---- "examined" is per text node, not per subtree (round 144, X3) ---- */

test("content/studio-links.js: examined means the NODE, and in-place edits are seen", () => {
  // Source checks (the script talks to chrome.* and the DOM, so it cannot be
  // imported) matching the ACT, not a word — round 124's lesson.
  const src = readFileSync(new URL("../content/studio-links.js", import.meta.url), "utf8");

  // 1. The no-path branch must NOT stamp the parent: an attribute there says "this
  // whole element is finished", which is how later streaming chunks got skipped.
  assert.doesNotMatch(
    src,
    /\.parentElement\??\.setAttribute\("data-vs-processed", "1"\)/,
    "the processed attribute must not be set on an element to mean 'its text was seen'",
  );

  // 2. The skip decision must consult the per-node set.
  assert.match(src, /examined\.has\(node\)/, "shouldSkip must check the examined set");
  assert.match(src, /const examined = new WeakSet\(\);/, "the set must exist");

  // 3. The observer must be able to see in-place text rewrites, not only childList.
  assert.match(
    src,
    /observe\(document\.body, \{ childList: true, characterData: true, subtree: true \}\)/,
    "the observer must observe characterData too",
  );
  assert.match(
    src,
    /if \(mu\.type === "characterData"\)/,
    "a characterData mutation must be handled, not ignored",
  );
});
