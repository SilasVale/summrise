// The documented 5xx contract on the two .ts relay handlers (round 133).
//
// proxies/README.md:13: "5xx responses use generic client text (detail stays in
// the worker/function log)". github.ts and gform.ts passed the upstream body
// through, so a GitHub or Google 5xx page reached the caller verbatim. Round 132
// fixed the same shape in zen.js/proxy.js; this closes the pair rather than
// leaving it half done. Neither handler forwards credentials (their request
// allowlists exclude `authorization`), so this is a contract/consistency fix,
// not a disclosure one — said plainly so the severity is not overstated.
import test from "node:test";
import assert from "node:assert/strict";

const github = (await import("../github.ts")).default;
const gform = (await import("../gform.ts")).default;

async function withUpstream(handler, url, status, body) {
  const real = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(body, { status, headers: { "content-type": "text/html" } });
  try {
    const r = await handler(new Request(url));
    return { status: r.status, body: await r.text() };
  } finally {
    globalThis.fetch = real;
  }
}

for (const [name, handler, url] of [
  ["github.ts", github, "https://relay.example/api/github?path=/web/octocat/Hello-World"],
  ["gform.ts", gform, "https://relay.example/api/gform?path=/gle/abc"],
]) {
  test(`${name}: a 5xx body does NOT reach the caller`, async () => {
    const out = await withUpstream(handler, url, 503, "<html>upstream said: internal-token-abc123</html>");
    assert.equal(out.status, 503, "the status still travels");
    assert.doesNotMatch(out.body, /internal-token-abc123/, `upstream text leaked: ${out.body}`);
    assert.match(out.body, /upstream unavailable/i, out.body);
  });

  test(`${name}: a 4xx body still reaches the caller`, async () => {
    const out = await withUpstream(handler, url, 404, "<html>no such thing</html>");
    assert.equal(out.status, 404);
    assert.match(out.body, /no such thing/, out.body);
  });
}
