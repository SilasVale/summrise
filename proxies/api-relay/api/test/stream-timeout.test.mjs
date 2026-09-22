// The header budget must not become a BODY budget (round 129).
//
// `proxies/README.md:13` states the 30 s upstream timeout "covers waiting for
// response headers only — streamed response bodies … forwarded untimed". Three of
// the five relay handlers used `AbortSignal.timeout()`, whose clock keeps RUNNING
// while the body streams — the shape that README describes as wrong — and nothing
// pinned it (zero grep hits for the timeout under api/test/).
//
// WHAT THIS TEST DOES *NOT* ESTABLISH, measured rather than assumed (round 129):
// reverting a handler to `AbortSignal.timeout` does NOT truncate the body here.
// With a 120 ms budget and a body that drips for 400 ms, BOTH shapes deliver the
// full body on Node 24/undici — aborting a signal after the Response has been
// returned does not cancel its body stream in this runtime. So the defect was a
// CONTRACT violation (a documented headers-only budget that three handlers did not
// implement, while two siblings did), NOT a reproduced 30 s truncation. What is
// proven here is the property the contract asks for: a body outliving the budget
// still arrives, and a HEADER that outlives it is still aborted.
//
// The budget is read from SUMMRISE_RELAY_HEADER_TIMEOUT_MS so these can tell the two
// shapes apart in milliseconds instead of sleeping 30 s. It is set BEFORE the
// dynamic imports because the handlers read it at module load.
process.env.SUMMRISE_RELAY_HEADER_TIMEOUT_MS = "120";

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const git = (await import("../git.ts")).default;
const github = (await import("../github.ts")).default;

/** A body that gives its FIRST chunk at once and its second after `ms` — i.e. one
 *  that is still flowing long after the headers arrived. */
function drippingBody(first, second, ms) {
  const enc = new TextEncoder();
  return new ReadableStream({
    start(c) {
      c.enqueue(enc.encode(first));
      setTimeout(() => {
        c.enqueue(enc.encode(second));
        c.close();
      }, ms);
    },
  });
}

const GIT_URL =
  "https://relay.example/api/git?path=/o/r.git/info/refs&service=git-upload-pack";
const GH_URL = "https://relay.example/api/github?path=/web/octocat/Hello-World";

async function bodyThrough(handler, url) {
  const real = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(drippingBody("first-", "second", 400), {
      status: 200,
      headers: { "content-type": "text/plain" },
    });
  try {
    const res = await handler(new Request(url));
    return await res.text();
  } finally {
    globalThis.fetch = real;
  }
}

test("git: a body still flowing after the header budget is NOT cut", async () => {
  assert.equal(
    await bodyThrough(git, GIT_URL),
    "first-second",
    "the body was aborted by the header budget",
  );
});

test("github: same, for the asset/release download path", async () => {
  assert.equal(
    await bodyThrough(github, GH_URL),
    "first-second",
    "the body was aborted by the header budget",
  );
});

test("...and the budget still aborts a slow HEADER (the protection is not gone)", async () => {
  const real = globalThis.fetch;
  globalThis.fetch = (_url, opts) =>
    new Promise((_resolve, reject) => {
      const signal = opts && opts.signal;
      if (!signal) return; // never resolves: a signal-less fetch would hang the test
      signal.addEventListener("abort", () => reject(new Error("aborted by budget")));
    });
  try {
    const out = await git(new Request(GIT_URL)).catch((e) => e);
    assert.ok(
      out instanceof Error || out.status >= 400,
      `a header that never arrives must fail, not hang: got ${out}`,
    );
  } finally {
    globalThis.fetch = real;
  }
});

// gform's route table is not exercised here (its paths are form ids, not the
// shapes above), so its SHAPE is pinned instead of its behaviour — the same
// source instrument this repo uses where a platform cannot be driven. All three
// files are checked, so a fix applied to two of them fails here.
test("no handler is left with a whole-fetch timeout", () => {
  for (const f of ["git.ts", "github.ts", "gform.ts"]) {
    const src = readFileSync(path.join(HERE, "..", f), "utf8");
    assert.doesNotMatch(
      src,
      /signal:\s*AbortSignal\.timeout/,
      `${f} still aborts the whole fetch — its body is cut at the header budget`,
    );
    assert.match(src, /clearTimeout\(headerTimer\)/, `${f} has no header-only shape`);
  }
});
