// route-oracle.mjs — THE ROUTE'S DIFFERENTIAL, in the shape `verify.mjs` already uses for the breaker.
//
// `oracle.mjs`/`oracle-translate.mjs` drive the SHIPPING FUNCTIONS. This drives the SHIPPING ROUTE: it calls
// the real `handleGateway` from `plugins/translate.ts` with a FAKE env, and records what came back — status,
// headers and body — as a fixture the Rust side replays.
//
//   cd gateway/wasm && node route-oracle.mjs > fixtures/route-corpus.json
//
// WHY A FAKE ENV IS ENOUGH FOR THIS ROUTE: the `count_tokens` arm estimates LOCALLY (the upstream count
// endpoint was dropped on 2026-08-12 because it cost a round-trip on every Claude Code turn), so nothing on
// this path dials out. What it does read is KV — the token record, the user record and the user's key
// record — and that is a map.
//
// THE BREAKER STUB IS COPIED FROM `verify.mjs`, including its class name: workers-rs duck-types on
// `obj.constructor().name`, and the real runtime's namespace object has that exact name.
//
// WHAT IT DOES NOT COVER, stated rather than implied: workerd's own runtime glue, and any path that dials
// an upstream (this route does not).
import { writeFileSync } from "node:fs";
import { handleGateway } from "../src/plugins/translate.ts";
import { __clearCaches } from "../src/store/cache.ts";

/** A KV namespace over a plain map: the three methods the store touches. */
function fakeKV(entries = {}) {
  const map = new Map(Object.entries(entries));
  return {
    // **THE TYPE ARGUMENT IS NOT DECORATION.** The store reads records with `env.KEYS.get(key, "json")`,
    // which PARSES in the real binding — a fake that ignores the second argument hands back a string, and
    // `ukeys.DEEPSEEK_API_KEY` on a string is undefined. That is exactly what happened on this harness's
    // first run: every case with a user key answered 502 "not configured".
    async get(key, type) {
      if (!map.has(key)) return null;
      const value = map.get(key);
      if (type === "json" && typeof value === "string") {
        try {
          return JSON.parse(value);
        } catch {
          return null;
        }
      }
      return value;
    },
    async put(key, value) {
      map.set(key, value);
    },
    async delete(key) {
      map.delete(key);
    },
    async list({ prefix = "" } = {}) {
      return {
        keys: [...map.keys()].filter((k) => k.startsWith(prefix)).map((name) => ({ name })),
        list_complete: true,
        cursor: undefined,
      };
    },
    __map: map,
  };
}

/** The breaker namespace `verify.mjs` builds, by the same constructor-name rule. */
class DurableObjectNamespace {
  idFromName(name) {
    return { name };
  }
  get() {
    return { fetch: async () => new Response("0", { status: 200 }) };
  }
}

const TOKEN = "tok-route-test";
const UID = "u-route-test";

/** A user record with a role and `enabled`, and a key record for one channel. */
function envWith(keys = {}) {
  return {
    KEYS: fakeKV({
      [`token:${TOKEN}`]: UID,
      [`user:${UID}`]: JSON.stringify({ id: UID, enabled: true, role: "user" }),
      [`ukeys:${UID}`]: JSON.stringify(keys),
    }),
    BREAKER: new DurableObjectNamespace(),
  };
}

async function callRoute(body, { keys = {}, token = TOKEN, headers = {} } = {}) {
  const request = new Request("https://relay.example/v1/messages/count_tokens", {
    method: "POST",
    headers: { "x-api-key": token, "content-type": "application/json", ...headers },
    body: typeof body === "string" ? body : JSON.stringify(body),
  });
  const url = new URL(request.url);
  const response = await handleGateway(request, envWith(keys), url);
  return {
    status: response.status,
    headers: Object.fromEntries(response.headers),
    body: await response.text(),
  };
}

// ── the passthrough arm, which DOES dial out ─────────────────────────────────────────────────────
// **THIS CAPTURES THE REQUEST THE ROUTE BUILDS**, which is the security-shaped half of a passthrough: the
// body decides what the upstream runs and the headers decide which credential it sees. The stub answers a
// canned OpenAI-shaped response and records what it was asked; the real `fetch` is restored in a `finally`.
async function captureUpstream(request) {
  const captured = { url: request.url, method: request.method, headers: {}, body: "" };
  for (const [k, v] of request.headers.entries()) captured.headers[k] = v;
  captured.body = await request.text();
  return captured;
}

/** One passthrough call with `fetch` stubbed. Returns the captured request and the route's own answer. */
async function callPassthrough(c) {
  const realFetch = globalThis.fetch;
  let captured = null;
  globalThis.fetch = async (url, init = {}) => {
    captured = await captureUpstream(new Request(url, init));
    return new Response(
      JSON.stringify({
        choices: [{ message: { role: "assistant", content: "ok" }, finish_reason: "stop" }],
        usage: { prompt_tokens: 1, completion_tokens: 1 },
      }),
      { status: 200, headers: { "content-type": "application/json" } },
    );
  };
  try {
    __clearCaches();
    const request = new Request("https://relay.example/v1/messages", {
      method: "POST",
      headers: { "x-api-key": TOKEN, "content-type": "application/json" },
      body: JSON.stringify({ ...c.body, model: c.model }),
    });
    const response = await handleGateway(request, envWith(c.keys), new URL(request.url));
    return { captured, status: response.status, answer: await response.text() };
  } finally {
    globalThis.fetch = realFetch;
  }
}

/** One streaming call whose upstream IGNORES `stream: true` and answers JSON. */
async function callStreamIgnored(c) {
  const realFetch = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(c.upstreamBody, {
      status: 200,
      headers: { "content-type": c.contentType || "application/json" },
    });
  try {
    __clearCaches();
    const request = new Request("https://relay.example/v1/messages", {
      method: "POST",
      headers: { "x-api-key": TOKEN, "content-type": "application/json" },
      body: JSON.stringify({ ...c.body, model: c.model, stream: true }),
    });
    const response = await handleGateway(request, envWith(c.keys), new URL(request.url));
    return {
      status: response.status,
      headers: Object.fromEntries(response.headers),
      body: await response.text(),
    };
  } finally {
    globalThis.fetch = realFetch;
  }
}

/** One NON-streaming call, whose upstream answers JSON. */
async function callNonStream(c) {
  const realFetch = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(c.upstreamBody, {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  try {
    __clearCaches();
    const request = new Request("https://relay.example/v1/messages", {
      method: "POST",
      headers: { "x-api-key": TOKEN, "content-type": "application/json" },
      body: JSON.stringify({ ...c.body, model: c.model }),
    });
    const response = await handleGateway(request, envWith(c.keys), new URL(request.url));
    return {
      status: response.status,
      headers: Object.fromEntries(response.headers),
      body: await response.text(),
    };
  } finally {
    globalThis.fetch = realFetch;
  }
}

// ── the corpus ────────────────────────────────────────────────────────────────────────────────────
// The three arms of the `count_tokens` decision, plus the two shapes that decide whether the arm is reached
// at all: an unauthenticated request (401 before the estimate) and a body whose estimate is NaN.
const cases = [
  {
    name: "no key for a CHECKED kind is a config error",
    body: { model: "ds/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }] },
    opts: { keys: {} },
  },
  {
    name: "a key for that kind estimates",
    body: { model: "ds/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }] },
    opts: { keys: { DEEPSEEK_API_KEY: "sk-test" } },
  },
  {
    name: "an env-level key satisfies the gate",
    body: { model: "qw/qwen3.8-flash", messages: [{ role: "user", content: "hi" }] },
    opts: { keys: {}, env: { QWEN_API_KEY: "env-key" } },
  },
  {
    // **THE ROUTE GATES SOME KINDS BEFORE THIS ARM**, and `or/` is one of them: with no OpenRouter key the
    // handler answers its own config error and the estimate is never reached. The case is kept BECAUSE of
    // that — it is the "which gate answers first" boundary, and a port that reordered them would show up
    // here rather than in a comment.
    name: "a kind the route gates BEFORE the arm",
    body: { model: "or/openai/gpt-5.6-luna:floor[1m]", messages: [{ role: "user", content: "hi" }] },
    opts: { keys: {} },
    // THE ARM IS NEVER REACHED for this case: the handler checks the OpenRouter key on its own path first.
    // The marker is what lets the Rust side assert "this one is NOT mine" instead of pretending the pure
    // decision reproduces a gate that lives above it.
    beforeArm: true,
  },
  {
    name: "an unauthenticated request is refused before the estimate",
    body: { model: "ds/deepseek-v4.1-flash", messages: [] },
    opts: { keys: { DEEPSEEK_API_KEY: "sk-test" }, token: "wrong" },
    // AUTH IS THE FIRST GATE OF ALL, so this case is above the arm too — the 401 comes from
    // `findUserByToken` before any route or key decision.
    beforeArm: true,
  },
  {
    name: "a CJK body estimates",
    body: { model: "ds/deepseek-v4.1-flash", messages: [{ role: "user", content: "中文内容测试" }] },
    opts: { keys: { DEEPSEEK_API_KEY: "sk-test" } },
  },
  {
    name: "a body with an image payload estimates",
    body: {
      model: "ds/deepseek-v4.1-flash",
      messages: [{ role: "user", content: [{ type: "image", source: { data: "A".repeat(600) } }] }],
    },
    opts: { keys: { DEEPSEEK_API_KEY: "sk-test" } },
  },
  {
    // An empty body has NO model, so the route resolves to its DEFAULT channel (`cm/`) and the arm answers
    // for THAT kind — which is why this case's error names CMD_API_KEY rather than DeepSeek's.
    name: "an empty body resolves to the default channel",
    body: "",
    opts: { keys: { DEEPSEEK_API_KEY: "sk-test" } },
    // Same marker, different reason: no model means the DEFAULT channel (`cm/`), whose key gate is above
    // the arm. The arm would estimate for `commandgoat`, so the two answers differ by design.
    beforeArm: true,
  },
];

const out = [];
for (const c of cases) {
  const env = envWith(c.opts.keys || {});
  if (c.opts.env) Object.assign(env, c.opts.env);
  const request = new Request("https://relay.example/v1/messages/count_tokens", {
    method: "POST",
    headers: { "x-api-key": c.opts.token ?? TOKEN, "content-type": "application/json" },
    body: typeof c.body === "string" ? c.body : JSON.stringify(c.body),
  });
  // **THE PER-ISOLATE CACHE IS SHARED BY EVERY CASE IN THIS PROCESS, WHICH IS ONE ISOLATE.** The store's
  // own test hook is the fix, and its comment says why it exists: "tests that flip global settings would
  // otherwise read a stale cached value from an earlier test". Measured on this harness's second run: the
  // FIRST case (no keys) cached `ukeys:u-route-test` as `{}`, and every later case with a key read the
  // empty record back and answered 502 "not configured".
  __clearCaches();
  const response = await handleGateway(request, env, new URL(request.url));
  out.push({
    name: c.name,
    // The Rust side needs the same three inputs the arm reads: the resolved kind, the user's key record
    // and the raw body. The kind is what `pickRoute` answers for this model, recorded here so the port can
    // be driven without resolving a route.
    kind: kindOf(c.body),
    ukeys: c.opts.keys || {},
    env: c.opts.env || {},
    rawText: typeof c.body === "string" ? c.body : JSON.stringify(c.body),
    ...(c.beforeArm ? { beforeArm: true } : {}),
    expected: {
      status: response.status,
      headers: Object.fromEntries(response.headers),
      body: await response.text(),
    },
  });
}

/** The route kind a body's model resolves to — the first path segment, which is what `pickRoute` keys on. */
function kindOf(body) {
  if (typeof body === "string") return "";
  const model = String(body.model || "");
  const prefix = model.split("/")[0] || "";
  return (
    {
      ds: "deepseek",
      qw: "qwen",
      og: "opencode",
      or: "openrouter",
      nv: "nvidia",
      gmi: "gmi",
      cm: "commandgoat",
      amd: "amd",
      r4: "r4",
    }[prefix] || ""
  );
}

// **THE FIXTURE GOES TO A FILE, NOT TO STDOUT.** The shipping handler's log wrapper writes one JSON line
// per request to stdout, so a `> fixtures/route-corpus.json` redirect would carry nine log lines and one
// fixture — which is what the first run produced. Writing the file directly keeps the two apart.
const target = new URL("./fixtures/route-corpus.json", import.meta.url);
writeFileSync(
  target,
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/route-oracle.mjs from the SHIPPING handleGateway. Do not edit by hand.",
      cases: out,
    },
    null,
    1,
  ) + "\n",
);
// The passthrough cases live in their own fixture: they carry a CAPTURED REQUEST rather than a response, so
// the Rust side compares what the route ASKED, not what it answered.
// **`ds/` TAKES THE PASSTHROUGH ARM, WHICH IS THE ONE THE RUST SIDE PORTS.** The first version of these
// cases used `og/`, and the capture showed what that arm actually is: `POST opencode.ai/zen/go/v1/chat/
// completions` with `Bearer` — the TRANSLATE arm (the "og pattern"), which is a different slice and not
// ported yet. Recording the wrong arm would have compared a decision against a route that does not use it.
const passthroughCases = [
  {
    name: "ds: the model is swapped in place and the key rides Bearer",
    model: "ds/deepseek-v4.1-flash",
    body: { messages: [{ role: "user", content: "hi" }], max_tokens: 16 },
    keys: { DEEPSEEK_API_KEY: "sk-ds" },
  },
  {
    name: "ds: a model key already in the body keeps its POSITION",
    model: "ds/deepseek-v4.1-flash",
    body: { model: "ds/old", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
    keys: { DEEPSEEK_API_KEY: "sk-ds" },
  },
  {
    name: "qw: a second passthrough channel behaves the same way",
    model: "qw/qwen3.8-flash",
    body: { messages: [{ role: "user", content: "hi" }] },
    keys: { QWEN_API_KEY: "sk-qw" },
  },
];
// ── the TRANSLATE arm (the "og pattern"), captured the same way ─────────────────────────────────
// Its shape was measured before it was ported, and every part of it differs from the passthrough arm: the
// upstream is chat/completions rather than the native path, the header is `Bearer` even for `og/`, the body
// is the TRANSLATOR's output (`model` first, `stream`, `max_tokens`) and the retry policy is a uniform
// literal rather than the shared table.
const translateCases = [
  {
    name: "og: an Anthropic request becomes an OpenAI one",
    model: "og/deepseek-v4.1-flash",
    body: { messages: [{ role: "user", content: "hi" }], max_tokens: 16 },
    keys: { OPENCODE_GO_API_KEY: "sk-og" },
  },
  {
    name: "og: a system prompt and a tool declaration ride along",
    model: "og/deepseek-v4.1-flash",
    body: {
      system: "be brief",
      messages: [{ role: "user", content: "hi" }],
      tools: [{ name: "t", description: "d", input_schema: { type: "object" } }],
      max_tokens: 32,
    },
    keys: { OPENCODE_GO_API_KEY: "sk-og" },
  },
];

const passthroughOut = [];
for (const c of [...passthroughCases, ...translateCases]) {
  const r = await callPassthrough(c);
  const isTranslate = translateCases.includes(c);
  passthroughOut.push({
    name: c.name,
    // The inputs the Rust side needs, recorded rather than re-derived: the kind, the wire model, the raw
    // text, the PARSED body (og-native parses, so the port takes the parsed branch) and the bearer key.
    // The kind and the wire model the route resolved, recorded so the port is driven without resolving a
    // route. `ds/` -> deepseek (wire = the stripped id), `qw/` -> qwen, `og/` -> opencode with its OWN wire
    // alias, which is why the translate cases name `deepseek-flash` rather than the advertised id.
    kind: isTranslate ? "opencode" : c.model.startsWith("qw/") ? "qwen" : "deepseek",
    upstreamModel: isTranslate
      ? "deepseek-flash"
      : c.model.startsWith("qw/") ? "qwen3.8-flash" : "deepseek-v4.1-flash",
    // **WHICH ARM THIS IS.** The translate arm PARSES (it has to: `toOpenAIRequest` walks the message
    // array), while the passthrough arm forwards raw text. The port takes a different branch for each, so
    // the fixture has to say which one the capture recorded.
    arm: isTranslate ? "translate" : "passthrough",
    rawText: JSON.stringify({ ...c.body, model: c.model }),
    // **`parsed: null`, BECAUSE THESE CHANNELS NEVER PARSE.** `ds/qw/or` forward RAW TEXT with only the
    // top-level model field swapped (no parse, no spread, no full re-stringify, for the 10 ms budget), and
    // the capture proves it: the body's key order is the CLIENT's, not a spread's. Recording a parsed body
    // here would have driven the port down a branch the route does not take.
    parsed: isTranslate ? { ...c.body, model: c.model } : null,
    bearerKey: c.keys.DEEPSEEK_API_KEY || c.keys.QWEN_API_KEY || c.keys.OPENCODE_GO_API_KEY,
    status: r.status,
    ...(r.captured ? { captured: r.captured } : { noUpstreamCall: true, answer: r.answer }),
  });
}
writeFileSync(
  new URL("./fixtures/passthrough-corpus.json", import.meta.url),
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/route-oracle.mjs from the SHIPPING handleGateway, with fetch stubbed.",
      cases: passthroughOut,
    },
    null,
    1,
  ) + "\n",
);
// ── the upstream that IGNORES `stream: true` ─────────────────────────────────────────────────────
// The recorded incident: feeding JSON into the SSE parser produced an EMPTY Anthropic message, so the whole
// answer was silently dropped. Four answers are pinned, including the two failure shapes.
const streamIgnoredCases = [
  {
    name: "a plain JSON completion becomes a ONE-SHOT SSE",
    upstreamBody: JSON.stringify({
      choices: [{ message: { role: "assistant", content: "hello" }, finish_reason: "stop" }],
      usage: { prompt_tokens: 1, completion_tokens: 1 },
    }),
  },
  {
    name: "a 200-wrapped ERROR envelope is surfaced, not swallowed",
    upstreamBody: JSON.stringify({ error: { message: "upstream said no" } }),
  },
  {
    name: "no choices at all",
    upstreamBody: JSON.stringify({ id: "x" }),
  },
  {
    name: "an EMPTY choices array",
    upstreamBody: JSON.stringify({ choices: [] }),
  },
  {
    name: "invalid JSON",
    upstreamBody: "{not json",
  },
  {
    name: "an error envelope with no message falls back to the generic sentence",
    upstreamBody: JSON.stringify({ error: {} }),
  },
];
const streamIgnoredOut = [];
for (const c of streamIgnoredCases) {
  const r = await callStreamIgnored({
    model: "og/deepseek-v4.1-flash",
    body: { messages: [{ role: "user", content: "hi" }], max_tokens: 16 },
    keys: { OPENCODE_GO_API_KEY: "sk-og" },
    upstreamBody: c.upstreamBody,
  });
  // `null` when the body is not JSON at all, which is what the route's `upstream.json().catch(() => null)`
  // produces — recorded rather than re-derived so the port is driven with the same input.
  let upstreamJson = null;
  try {
    upstreamJson = JSON.parse(c.upstreamBody);
  } catch {
    upstreamJson = null;
  }
  streamIgnoredOut.push({
    name: c.name,
    upstreamJson,
    upstreamModel: "deepseek-flash",
    expected: { status: r.status, headers: r.headers, body: r.body },
  });
}
writeFileSync(
  new URL("./fixtures/stream-ignored-corpus.json", import.meta.url),
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/route-oracle.mjs from the SHIPPING handleGateway, fetch stubbed.",
      cases: streamIgnoredOut,
    },
    null,
    1,
  ) + "\n",
);
// ── the NON-streaming answer ─────────────────────────────────────────────────────────────────────
// Its generic sentence is NOT the streaming branch's ("upstream returned an invalid response" against
// "…an error envelope"), and both are reached by the same shape — so a test that checked only the status
// would call them equivalent. These cases pin the BODY.
const nonStreamCases = [
  {
    name: "a plain completion becomes an Anthropic response",
    upstreamBody: JSON.stringify({
      choices: [{ message: { role: "assistant", content: "hello" }, finish_reason: "stop" }],
      usage: { prompt_tokens: 1, completion_tokens: 1 },
    }),
  },
  {
    name: "an error envelope names the upstream's message",
    upstreamBody: JSON.stringify({ error: { message: "upstream said no" } }),
  },
  {
    name: "a top-level message field is used when there is no error object",
    upstreamBody: JSON.stringify({ message: "just a message" }),
  },
  {
    name: "no choices falls back to THIS branch's sentence",
    upstreamBody: JSON.stringify({ id: "x" }),
  },
  {
    name: "invalid JSON",
    upstreamBody: "{not json",
  },
  {
    name: "an EMPTY choices array",
    upstreamBody: JSON.stringify({ choices: [] }),
  },
];
const nonStreamOut = [];
for (const c of nonStreamCases) {
  const r = await callNonStream({
    model: "og/deepseek-v4.1-flash",
    body: { messages: [{ role: "user", content: "hi" }], max_tokens: 16 },
    keys: { OPENCODE_GO_API_KEY: "sk-og" },
    upstreamBody: c.upstreamBody,
  });
  let upstreamJson = null;
  try {
    upstreamJson = JSON.parse(c.upstreamBody);
  } catch {
    upstreamJson = null;
  }
  nonStreamOut.push({
    name: c.name,
    upstreamJson,
    upstreamModel: "deepseek-flash",
    expected: { status: r.status, headers: r.headers, body: r.body },
  });
}
writeFileSync(
  new URL("./fixtures/non-stream-corpus.json", import.meta.url),
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/route-oracle.mjs from the SHIPPING handleGateway, fetch stubbed.",
      cases: nonStreamOut,
    },
    null,
    1,
  ) + "\n",
);
console.log(`route-oracle: ${out.length} count_tokens case(s) and ${passthroughOut.length} request case(s) and ${streamIgnoredOut.length} stream-ignored and ${nonStreamOut.length} non-stream case(s) written`);
