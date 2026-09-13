// zen-us-proxy behavior tests (first unit coverage — previously only the
// wrangler dry-run gate). The worker is exercised end to end with a stubbed
// global fetch. Pins the file's documented contracts:
//  - CORS reflect-if-allowlisted (console origins + loopback), closed otherwise
//  - /v1/models and /v1/messages gate on CLIENT_KEY, default-CLOSED (the
//    endpoint spends the worker's own paid OPENCODE_GO_API_KEY)
//  - /v1/responses is BYOK: the CALLER's Bearer key is forwarded, the worker
//    never substitutes its own, blank key 401s
//  - upstream 5xx → generic client text (detail server-side); 4xx → message
//    passthrough; the SSE body streams through with CORS stamped
import test from "node:test";
import assert from "node:assert/strict";
import worker from "../src/index.js";

const CONSOLE = "https://ai.saisi.online";
const NATIVE = "https://opencode.ai/zen/go/v1/messages";
const RESPONSES = "https://opencode.ai/zen/go/v1/responses";
const MODELS = "https://opencode.ai/zen/go/v1/models";

function stubFetch() {
  const calls = [];
  const real = globalThis.fetch;
  globalThis.fetch = async (url, init = {}) => {
    calls.push({ url: String(url), init });
    return new Response(JSON.stringify({ __stub: true }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  return {
    calls,
    respond(status, body, headers = {}) {
      globalThis.fetch = async (url, init = {}) => {
        calls.push({ url: String(url), init });
        return new Response(typeof body === "string" ? body : JSON.stringify(body), {
          status,
          headers: { "content-type": "application/json", ...headers },
        });
      };
    },
    restore: () => {
      globalThis.fetch = real;
    },
  };
}

const env = { CLIENT_KEY: "ck-secret", OPENCODE_GO_API_KEY: "up-key" };
const req = (method, path, { key = "ck-secret", bearer, body, headers = {}, origin } = {}) => {
  const h = { ...headers };
  if (key !== undefined) h["x-api-key"] = key;
  if (bearer !== undefined) h.authorization = bearer ? `Bearer ${bearer}` : "";
  if (origin) h.origin = origin;
  return new Request(`https://zen-us.local${path}`, {
    method,
    headers: h,
    body: body === undefined ? undefined : typeof body === "string" ? body : JSON.stringify(body),
  });
};

test("OPTIONS preflight reflects allowlisted + loopback origins, closed otherwise", async () => {
  const ok = await worker.fetch(new Request("https://zen-us.local/x", { method: "OPTIONS", headers: { origin: CONSOLE } }), env);
  assert.equal(ok.headers.get("access-control-allow-origin"), CONSOLE);
  const loop = await worker.fetch(new Request("http://localhost:9999/x", { method: "OPTIONS", headers: { origin: "http://localhost:9999" } }), env);
  assert.equal(loop.headers.get("access-control-allow-origin"), "http://localhost:9999");
  const denied = await worker.fetch(new Request("https://zen-us.local/x", { method: "OPTIONS", headers: { origin: "https://evil.example" } }), env);
  assert.equal(denied.headers.get("access-control-allow-origin"), null);
});

test("/v1/models is CLIENT_KEY-gated and forwards the worker's upstream key", async () => {
  const noGate = await worker.fetch(req("GET", "/v1/models", { key: "nope" }), env);
  assert.equal(noGate.status, 401);
  const noSecret = await worker.fetch(req("GET", "/v1/models"), {}); // CLIENT_KEY unset → default-closed
  assert.equal(noSecret.status, 401);
  const { calls, restore } = stubFetch();
  try {
    await worker.fetch(req("GET", "/v1/models"), env);
    assert.equal(calls[0].url, MODELS);
    assert.equal(calls[0].init.headers["x-api-key"], "up-key");
  } finally {
    restore();
  }
});

test("/v1/messages: default-CLOSED gate, native Anthropic passthrough with anthropic-version", async () => {
  const noGate = await worker.fetch(req("POST", "/v1/messages", { key: "", body: "{}" }), env);
  assert.equal(noGate.status, 401, "missing gate key must not reach the paid upstream");
  const unset = await worker.fetch(req("POST", "/v1/messages", { key: "x" }), {}); // CLIENT_KEY unset
  assert.equal(unset.status, 401);

  const { calls, respond, restore } = stubFetch();
  try {
    const raw = JSON.stringify({ model: "deepseek-flash", max_tokens: 5, messages: [] });
    respond(200, raw, { "content-type": "text/event-stream" });
    const r = await worker.fetch(req("POST", "/v1/messages", { body: raw }), env);
    assert.equal(r.status, 200);
    assert.equal(calls[0].url, NATIVE);
    assert.equal(calls[0].init.headers["x-api-key"], "up-key");
    assert.equal(calls[0].init.headers["anthropic-version"], "2023-06-01");
    // request.body is a stream — the passthrough forwards it untouched.
    assert.equal(await new Response(calls[0].init.body).text(), raw, "native passthrough forwards the body verbatim");
    assert.match(r.headers.get("content-type") || "", /text\/event-stream/);
  } finally {
    restore();
  }
});

test("/v1/messages upstream 5xx: generic client text; 4xx passes the upstream message through", async () => {
  const s = stubFetch();
  try {
    s.respond(502, { error: { message: "INTERNAL detail" } });
    const r5 = await worker.fetch(req("POST", "/v1/messages", { body: "{}" }), env);
    assert.equal(r5.status, 502);
    assert.equal((await r5.json()).error.message, "Upstream unavailable");
    s.respond(400, { error: { message: "bad anthropic request" } });
    const r4 = await worker.fetch(req("POST", "/v1/messages", { body: "{}" }), env);
    assert.equal(r4.status, 400);
    assert.equal((await r4.json()).error.message, "bad anthropic request");
  } finally {
    s.restore();
  }
});

test("/v1/responses is BYOK: forwards the CALLER's key, never the worker's; blank key 401s", async () => {
  const noKey = await worker.fetch(req("POST", "/v1/responses", { bearer: "", body: {} }), env);
  assert.equal(noKey.status, 401);
  const missing = await worker.fetch(req("POST", "/v1/responses", { body: {} }), env);
  assert.equal(missing.status, 401);

  const { calls, respond, restore } = stubFetch();
  try {
    respond(200, "data: {\"resp\":true}\n\n", { "content-type": "text/event-stream" });
    const r = await worker.fetch(
      req("POST", "/v1/responses", { bearer: "caller-zen-key", body: { model: "og/muse-spark" } }),
      env,
    );
    assert.equal(r.status, 200);
    assert.equal(calls[0].url, RESPONSES);
    assert.equal(calls[0].init.headers.Authorization, "Bearer caller-zen-key");
    assert.match(await r.text(), /"resp":true/, "SSE body passes through");
    assert.match(r.headers.get("content-type") || "", /text\/event-stream/);
  } finally {
    restore();
  }
});

test("/v1/responses upstream errors pass through with upstream message", async () => {
  const s = stubFetch();
  try {
    s.respond(402, { message: "insufficient credits" });
    const r = await worker.fetch(
      req("POST", "/v1/responses", { bearer: "caller-key", body: {} }),
      env,
    );
    assert.equal(r.status, 402);
    assert.equal((await r.json()).error.message, "insufficient credits");
  } finally {
    s.restore();
  }
});

test("/v1/responses forwards the caller's conversation id as x-opencode-session (zen 2026-09-05 requirement)", async () => {
  const { calls, respond, restore } = stubFetch();
  try {
    respond(200, "data: {}\n\n", { "content-type": "text/event-stream" });
    // pi-ai/DSH-style spelling
    await worker.fetch(
      req("POST", "/v1/responses", {
        bearer: "caller-key",
        body: {},
        headers: { "x-client-request-id": "conv-dsh-uuid" },
      }),
      env,
    );
    assert.equal(calls[0].init.headers["x-opencode-session"], "conv-dsh-uuid");
    // native spelling wins when both are present
    await worker.fetch(
      req("POST", "/v1/responses", {
        bearer: "caller-key",
        body: {},
        headers: { "x-opencode-session": "conv-native", "x-client-request-id": "conv-dsh-uuid" },
      }),
      env,
    );
    assert.equal(calls[1].init.headers["x-opencode-session"], "conv-native");
    // absent → no header at all (never fabricate on a BYOK relay)
    await worker.fetch(
      req("POST", "/v1/responses", { bearer: "caller-key", body: {} }),
      env,
    );
    assert.equal(calls[2].init.headers["x-opencode-session"], undefined);
  } finally {
    restore();
  }
});

test("unknown path 404s (gated paths first, then the envelope)", async () => {
  const { respond, restore } = stubFetch();
  try {
    respond(200, {});
    const r = await worker.fetch(req("GET", "/nope"), env);
    assert.equal(r.status, 404);
    assert.equal((await r.json()).error.type, "not_found_error");
  } finally {
    restore();
  }
});

/* ---- credential redaction on the error path (round 131) ---- */

test("a provider that echoes the key back cannot leak it to the client", async () => {
  // The worker had NO redaction anywhere while the gateway redacts and pins it:
  // a provider echoing the key in a 4xx message handed it to the caller verbatim.
  const KEY = "sk-zen-test-key-0123456789abcdef";
  const real = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(JSON.stringify({ error: { message: `invalid api key: ${KEY}` } }), {
      status: 400,
      headers: { "content-type": "application/json" },
    });
  try {
    const r = await worker.fetch(
      req("POST", "/v1/messages", { key: "ck-secret", body: { model: "x", messages: [] } }),
      { CLIENT_KEY: "ck-secret", OPENCODE_GO_API_KEY: KEY },
    );
    const body = await r.text();
    assert.equal(r.status, 400, "the upstream status still reaches the caller");
    assert.ok(!body.includes(KEY), `the provider's echo leaked the key: ${body}`);
    assert.ok(body.includes("***"), `the redaction must be visible: ${body}`);
  } finally {
    globalThis.fetch = real;
  }
});

test("a key shorter than 8 chars is NOT redacted blindly (it would mangle text)", async () => {
  // The gateway's own guard: replacing a 2-char secret would rewrite unrelated
  // words. Pinned here so a future "simplify" cannot delete the guard.
  const KEY = "ab";
  const real = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(JSON.stringify({ error: { message: "abort: bad request" } }), {
      status: 400,
      headers: { "content-type": "application/json" },
    });
  try {
    const r = await worker.fetch(
      req("POST", "/v1/messages", { key: "ck-secret", body: { model: "x", messages: [] } }),
      { CLIENT_KEY: "ck-secret", OPENCODE_GO_API_KEY: KEY },
    );
    const body = await r.text();
    assert.ok(body.includes("abort"), `short secrets must not carve up words: ${body}`);
  } finally {
    globalThis.fetch = real;
  }
});

/* ---- the pass-through label (round 136) ---- */

// CLOSED IN ROUND 137, and the way it closed is the lesson: round 136 fixed ONE of
// TWO identical pass-through sites in this file and recorded the remaining failure as
// "a conversion branch" — a reading, not a measurement. The `todo` test written then
// is what proved otherwise: the failing case took the OTHER pass-through, and fixing
// that one made this test pass. The todo is gone because the gap is gone.
test("a stream:false JSON answer is labelled JSON, not SSE", async () => {
  // The relay hardcoded `text/event-stream` on every pass-through, so a
  // non-streaming answer arrived labelled as a stream — while zen-go, the
  // sibling worker, discriminates. The upstream's own content-type wins now.
  const real = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response('{"ok":true}', { status: 200, headers: { "content-type": "application/json" } });
  try {
    const r = await worker.fetch(
      req("POST", "/v1/responses", { bearer: "sk-zen-us-caller-0123456789", body: { model: "x", stream: false } }),
      { CLIENT_KEY: "ck-secret", OPENCODE_GO_API_KEY: "sk-zen-us-test-key-0123456789" },
    );
    // The test is about the LABEL: what the worker does to the body beyond that is
    // another question (and asserting it here would encode assumptions this suite
    // does not own).
    assert.equal(r.status, 200);
    assert.match(r.headers.get("content-type") || "", /application\/json/, "not relabelled as SSE");
  } finally {
    globalThis.fetch = real;
  }
});

test("a real SSE answer keeps its SSE label (and an unlabelled one still defaults to SSE)", async () => {
  const real = globalThis.fetch;
  try {
    globalThis.fetch = async () =>
      new Response("data: {}\n\n", { status: 200, headers: { "content-type": "text/event-stream" } });
    let r = await worker.fetch(
      req("POST", "/v1/responses", { bearer: "sk-zen-us-caller-0123456789", body: { model: "x", stream: true } }),
      { CLIENT_KEY: "ck-secret", OPENCODE_GO_API_KEY: "sk-zen-us-test-key-0123456789" },
    );
    assert.equal(r.status, 200);
    assert.match(r.headers.get("content-type") || "", /text\/event-stream/, "SSE preserved");

  } finally {
    globalThis.fetch = real;
  }
});

// P9c WAS A PHANTOM, AND THE INSTRUMENT THAT KILLED IT WAS ONE NODE COMMAND
// (round 148). Round 137 recorded "an upstream that declares no content-type comes
// back unlabelled" as an open finding, on the strength of this test. The test was
// WRONG, not the code: `new Response("string", { status: 200 })` AUTO-LABELS the body
// `text/plain;charset=UTF-8` (the Fetch spec's default for a string body — verified:
// `node -e 'console.log(new Response("x",{status:200}).headers.get("content-type"))'`).
// So the stub could never produce the state the test claimed to exercise, the relay
// faithfully forwarded `text/plain`, and the SSE fallback it was accused of skipping
// was never reachable in that scenario. A real upstream that declares nothing sends
// no content-type header at all, and the fallback DOES apply there.
test("an upstream that labels its body text/plain is forwarded AS text/plain", async () => {
  // The honest form of what round 137 was reaching for: the relay must not OVERRIDE
  // what the upstream said. Falling back to SSE is for an answer that says NOTHING —
  // which a string-bodied Response cannot express, because the constructor labels it.
  const real = globalThis.fetch;
  globalThis.fetch = async () => new Response("not sse at all", { status: 200 });
  try {
    const r = await worker.fetch(
      req("POST", "/v1/responses", { bearer: "sk-zen-us-caller-0123456789", body: { model: "x", stream: true } }),
      { CLIENT_KEY: "ck-secret", OPENCODE_GO_API_KEY: "sk-zen-us-test-key-0123456789" },
    );
    assert.equal(r.status, 200);
    assert.match(
      r.headers.get("content-type") || "",
      /text\/plain/,
      "the upstream's own label wins over our fallback",
    );
  } finally {
    globalThis.fetch = real;
  }
});

