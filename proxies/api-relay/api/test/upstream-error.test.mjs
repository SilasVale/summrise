// The documented 5xx contract on the two caller-key relay handlers (round 132).
//
// proxies/README.md:13: "5xx responses use generic client text (detail stays in
// the worker/function log)". zen.js and proxy.js streamed the upstream body
// through unconditionally, so a provider's 500 text reached the caller verbatim —
// while the two Cloudflare siblings genericized theirs. The 4xx path is
// deliberately unchanged: that text is what a caller needs to fix their request,
// and these handlers forward the CALLER's own key, so an echo returns a secret to
// the person who already holds it (unlike the CF workers, which forward the
// worker's paid key — fixed in round 131).
import test from "node:test";
import assert from "node:assert/strict";

const zen = (await import("../zen.js")).default;
const proxy = (await import("../proxy.js")).default;

async function withUpstream(status, body, handler, path, auth) {
  const real = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(body, { status, headers: { "content-type": "application/json" } });
  try {
    // The caller's own key: these handlers are BYOK, and 401 comes first.
    const r = await handler(
      new Request(`https://relay.example${path}`, {
        method: "POST",
        headers: auth,
        body: "{}",
      }),
    );
    return { status: r.status, body: await r.text() };
  } finally {
    globalThis.fetch = real;
  }
}

// The two handlers authenticate DIFFERENTLY on purpose (proxy.js pins that
// x-api-key alone does not authenticate it): zen.js takes the caller's key either
// way, proxy.js requires an Authorization bearer.
for (const [name, handler, path, auth] of [
  ["zen.js", zen, "/api/zen?target=og", { "x-api-key": "sk-test-caller" }],
  ["proxy.js", proxy, "/api/proxy", { authorization: "Bearer sk-or-test-caller" }],
]) {
  test(`${name}: a 5xx body does NOT reach the caller`, async () => {
    const out = await withUpstream(503, '{"error":"upstream said: internal-token-abc123"}', handler, path, auth);
    assert.equal(out.status, 503, "the status still travels");
    assert.doesNotMatch(out.body, /internal-token-abc123/, `upstream text leaked: ${out.body}`);
    assert.match(out.body, /Upstream unavailable/, out.body);
  });

  test(`${name}: a 4xx body still reaches the caller (it is the actionable one)`, async () => {
    const out = await withUpstream(400, '{"error":"bad model name"}', handler, path, auth);
    assert.equal(out.status, 400);
    assert.match(out.body, /bad model name/, out.body);
  });
}
