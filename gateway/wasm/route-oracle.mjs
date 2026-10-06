// route-oracle.mjs — THE ROUTE'S DIFFERENTIAL, in the shape `verify.mjs` already uses for the breaker.
//
// `oracle.mjs`/`oracle-translate.mjs` drive the SHIPPING FUNCTIONS. This drives the SHIPPING ROUTE: it calls
// the real `handleGateway` from `plugins/translate.ts` with a FAKE env, and records what came back — status,
// headers and body — as a fixture the Rust side replays.
//
//   cd gateway/wasm && node route-oracle.mjs
//
// **IT WRITES THE FIXTURES ITSELF — THERE IS NO REDIRECT**, and the line above used to say
// `> fixtures/route-corpus.json`. That instruction was wrong twice over: the handler's log wrapper prints one
// JSON line per request to stdout, so a redirect would have carried eleven log lines and one fixture (which is
// what the first run produced), and there are now ELEVEN fixtures rather than one.
//
// THE FIXTURES, each with one subject: `route-corpus.json` (the count_tokens arm), `front-door-corpus.json`
// (the ORDER of the gates above the arm — auth, the route match, the retired check), `auth-header-corpus.json`
// (the two spellings of the token), `passthrough-corpus.json`, `chat-corpus.json`, `responses-corpus.json`,
// `stream-ignored-corpus.json`, `non-stream-corpus.json`, `upstream-failure-corpus.json`,
// `models-corpus.json`, `stream-frame-corpus.json`.
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
import { opencodeSessionHeader, wireModelName } from "../src/upstream.ts";
import { advertisedIds, extraModelEntries, facetOverrides, disabledModels, customModels } from "../src/store/models.ts";
import { handleGateway } from "../src/plugins/translate.ts";
import { streamOgToAnthropic } from "../src/anthropic-translate.ts";
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
function envWith(keys = {}, enabled = true) {
  return {
    KEYS: fakeKV({
      [`token:${TOKEN}`]: UID,
      // **`enabled` IS A PARAMETER BECAUSE THE HANDLER CHECKS IT** and the first version of this fixture
      // could not reach the disabled branch at all: its 401 case had NO user, so a port that dropped the
      // `enabled` test would have passed every case.
      [`user:${UID}`]: JSON.stringify({ id: UID, enabled, role: "user" }),
      [`ukeys:${UID}`]: JSON.stringify(keys),
    }),
    BREAKER: new DurableObjectNamespace(),
  };
}

async function callRoute(body, { keys = {}, token = TOKEN, headers = {}, enabled = true } = {}) {
  const request = new Request("https://relay.example/v1/messages/count_tokens", {
    method: "POST",
    headers: { "x-api-key": token, "content-type": "application/json", ...headers },
    body: typeof body === "string" ? body : JSON.stringify(body),
  });
  const url = new URL(request.url);
  const response = await handleGateway(request, envWith(keys, enabled), url);
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
    // **THE PATH IS A PARAMETER**, because the chat/completions arm is a different branch of the same
    // handler and its request is built by different code.
    const request = new Request(`https://relay.example${c.path || "/v1/messages"}`, {
      method: "POST",
      headers: { "x-api-key": TOKEN, "content-type": "application/json" },
      body: JSON.stringify({ ...c.body, model: c.model }),
    });
    // A per-case env, so a capture can point an exit at a TEST host instead of a production one.
    const env = envWith(c.keys);
    if (c.env) Object.assign(env, c.env);
    const response = await handleGateway(request, env, new URL(request.url));
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
      // **THE STATUS IS A PARAMETER BECAUSE THE FAILURE PATH IS A DIFFERENT DECISION.** A 200 goes through
      // the response shaping; a 5xx goes through `upstreamBodyErrorResponse`, which unwraps, scrubs, keeps a
      // known upstream type and carries `Retry-After`.
      status: c.upstreamStatus ?? 200,
      headers: {
        "content-type": c.upstreamType || "application/json",
        ...(c.retryAfter ? { "retry-after": c.retryAfter } : {}),
      },
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

/** `GET /v1/models` — public, so no token, and it reads KV only (no upstream call). */
async function callModels(c) {
  __clearCaches();
  const env = envWith({});
  // The three keys the listing reads. `models:disabled` is a JSON array of ids; the other two are arrays of
  // records. Absent keys are what an untouched deployment has.
  if (c.disabled) env.KEYS.__map.set("models:disabled", JSON.stringify(c.disabled));
  if (c.custom) env.KEYS.__map.set("models:custom", JSON.stringify(c.custom));
  if (c.overrides) env.KEYS.__map.set("models:overrides", JSON.stringify(c.overrides));
  if (c.providerRecords) env.KEYS.__map.set("providers:custom", JSON.stringify(c.providerRecords));
  const request = new Request("https://relay.example/v1/models", { method: "GET" });
  const response = await handleGateway(request, env, new URL(request.url));
  // **THE THREE REAL INPUTS, RECORDED RATHER THAN RE-DERIVED.** `advertisedIds`, `extraModelEntries` and
  // `facetOverrides` read KV, a catalogue file and the provider records; a Rust test that rebuilt them would
  // be re-implementing the store instead of driving the arm, which is the seam every other port here uses.
  const liveIds = await advertisedIds(env);
  const extraEntries = await extraModelEntries(env);
  const facet = await facetOverrides(env);
  // **THE THREE STORE READS, RECORDED SO THE CHAIN THAT PRODUCES THEM IS REPLAYABLE.** `liveIds` and
  // `extraEntries` are the OUTPUTS; without their inputs a Rust replay of `advertisedIds`/
  // `extraModelEntries` would be comparing a function with itself.
  const disabled = [...(await disabledModels(env))];
  const custom = await customModels(env);
  // `readList` is PRIVATE, so the same KV key is read here — the raw provider records are what
  // `advertisedProviderModels` takes, and without them a Rust replay would be circular.
  const providers = await env.KEYS.get("providers:custom", "json").catch(() => null) ?? [];
  return {
    status: response.status,
    headers: Object.fromEntries(response.headers),
    body: await response.text(),
    liveIds,
    extraEntries,
    facet,
    // The INPUTS too, so the chain that produces `liveIds`/`extraEntries` is replayable rather than circular.
    disabled,
    custom,
    providers,
  };
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
    // **A DISABLED USER, WHICH THE FIRST FIXTURE COULD NOT REACH.** The handler's rule is
    // `!user || !user.enabled`, and its 401 case only covered the FIRST half — so the `enabled` check was a
    // named gap in `request_shape.rs` until this case existed.
    name: "a DISABLED user is refused even with a valid token and key",
    body: { model: "ds/deepseek-v4.1-flash", messages: [] },
    opts: { keys: { DEEPSEEK_API_KEY: "sk-test" }, enabled: false },
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
  const env = envWith(c.opts.keys || {}, c.opts.enabled ?? true);
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
    // **THE AUTH OUTCOME, RECORDED RATHER THAN RE-DERIVED.** The corpus's 401 case is the one whose token
    // resolves to no user, and a Rust test cannot know that from the expectation without reasoning
    // backwards from the answer it is checking.
    // **THE AUTH OUTCOME, RECORDED RATHER THAN RE-DERIVED.** A Rust test cannot know from the EXPECTATION
    // whether the token resolved, let alone whether the user was enabled — so the fixture records the USER
    // SHAPE the store would hand the handler: `null`, or a record whose `enabled` may be false.
    user:
      (c.opts.token ?? TOKEN) === TOKEN
        ? { id: "u-route-test", enabled: c.opts.enabled ?? true }
        : null,
    ...(c.beforeArm ? { beforeArm: true } : {}),
    expected: {
      status: response.status,
      headers: Object.fromEntries(response.headers),
      body: await response.text(),
    },
  });
}

// ── THE FRONT DOOR'S ORDER ───────────────────────────────────────────────────────────────────────
// **THE NINE CASES ABOVE ALL RIDE ONE SHAPE — `POST /v1/messages/count_tokens` — SO NONE OF THEM CAN SEE
// THE ORDER OF THE GATES ABOVE THE ARM.** Measured on 2026-10-06 against the deployed `vale-gate-wasm`:
// `GET /v1/messages` and `POST /v1/models` answered **404** where the shipping gateway answers **401**,
// and every case in `route-corpus.json` was green through it. The shipping order, read in
// `handleGatewayImpl`:
//
//     GET …/models (public, 200) -> auth (401) -> admin cutover -> rate limit -> route match (404)
//       -> the body scan -> the retired check (400) -> the key gate -> the arm
//
// The three boundaries below are the ones a port can invert while every served-path case still passes:
// auth against the route match, and the retired check against both.
const orderCases = [
  {
    name: "GET /v1/messages, NO token — auth answers before the route match",
    method: "GET",
    path: "/v1/messages",
    token: "wrong",
  },
  {
    name: "GET /v1/messages, a valid token — the route match answers 404",
    method: "GET",
    path: "/v1/messages",
  },
  {
    name: "POST /v1/models, NO token — auth answers before the route match",
    method: "POST",
    path: "/v1/models",
    token: "wrong",
  },
  {
    name: "POST /v1/models, a valid token — the route match answers 404",
    method: "POST",
    path: "/v1/models",
  },
  {
    name: "DELETE /v1/messages, NO token — a method the route does not take",
    method: "DELETE",
    path: "/v1/messages",
    token: "wrong",
  },
  {
    name: "DELETE /v1/messages, a valid token — the route match answers 404",
    method: "DELETE",
    path: "/v1/messages",
  },
  {
    // **THE RETIRED CHECK IS BELOW AUTH IN THE SOURCE, AND THIS PAIR IS WHAT SAYS SO.** The wasm worker
    // checked it BEFORE the token was even read, so a retired model with no token answered 400 where the
    // shipping gateway answers 401 — the same inversion as the route match, one gate further down.
    name: "a RETIRED model with NO token — auth still answers first",
    method: "POST",
    path: "/v1/messages",
    token: "wrong",
    model: "ds/deepseek-v4-flash",
  },
  {
    name: "a RETIRED model with a valid token — the retired check answers 400",
    method: "POST",
    path: "/v1/messages",
    model: "ds/deepseek-v4-flash",
  },
];

const orderOut = [];
for (const c of orderCases) {
  const body = { model: c.model || "ds/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }] };
  __clearCaches();
  const request = new Request(`https://relay.example${c.path}`, {
    method: c.method,
    headers: { "x-api-key": c.token ?? TOKEN, "content-type": "application/json" },
    // A GET with a body is not a thing; the route reads the body only after the route match anyway.
    ...(c.method === "GET" ? {} : { body: JSON.stringify(body) }),
  });
  const response = await handleGateway(request, envWith({ DEEPSEEK_API_KEY: "sk-test" }), new URL(request.url));
  orderOut.push({
    name: c.name,
    // **THE METHOD AND THE PATH TRAVEL WITH THE CASE**, which the count_tokens corpus does not need and
    // this one is nothing but: the replay's inputs ARE the method and the path.
    method: c.method,
    path: c.path,
    kind: kindOf(body),
    ukeys: { DEEPSEEK_API_KEY: "sk-test" },
    env: {},
    rawText: c.method === "GET" ? "" : JSON.stringify(body),
    // The same rule as above: a token that resolves to no user is recorded as no user, rather than
    // re-derived from the expectation.
    user: (c.token ?? TOKEN) === TOKEN ? { id: "u-route-test", enabled: true } : null,
    expected: {
      status: response.status,
      headers: Object.fromEntries(response.headers),
      body: await response.text(),
    },
  });
}
writeFileSync(
  new URL("./fixtures/front-door-corpus.json", import.meta.url),
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/route-oracle.mjs from the SHIPPING handleGateway. Do not edit by hand.",
      cases: orderOut,
    },
    null,
    1,
  ) + "\n",
);

// ── THE TOKEN'S TWO DOORS ────────────────────────────────────────────────────────────────────────
// **`handleGatewayImpl` ACCEPTS TWO SPELLINGS OF THE SAME CREDENTIAL, AND THE WASM WORKER READ ONE.**
// The source's own comment says why both exist — *"x-api-key = the user's gateway token; also accept
// Authorization: Bearer (OpenAI-compatible clients like DSH send Bearer, not x-api-key)"* — and the rule
// has three parts that a one-line port gets wrong in three different ways:
//
//     const token = request.headers.get("x-api-key") || "";      // an EMPTY one falls through
//     const bearerToken = auth.startsWith("Bearer ") ? auth.slice(7) : "";   // the exact scheme, and a space
//     const effectiveToken = token || bearerToken;               // x-api-key WINS, wrong or not
//
// `GET /v1/messages` is the probe rather than an assertion about internals: with a token that resolves it
// answers 404 (the route match), and without one 401 — so the status IS the answer to "did the token
// resolve", which is the whole of this decision.
const authHeaderCases = [
  { name: "x-api-key alone", headers: { "x-api-key": TOKEN } },
  { name: "Authorization: Bearer alone", headers: { authorization: `Bearer ${TOKEN}` } },
  {
    name: "both, and the x-api-key WINS even though it is the wrong one",
    headers: { "x-api-key": "wrong", authorization: `Bearer ${TOKEN}` },
  },
  {
    name: "an EMPTY x-api-key falls through to Bearer",
    headers: { "x-api-key": "", authorization: `Bearer ${TOKEN}` },
  },
  { name: "a lowercase scheme is NOT Bearer", headers: { authorization: `bearer ${TOKEN}` } },
  { name: "no space after the scheme", headers: { authorization: `Bearer${TOKEN}` } },
  { name: "Bearer with an empty token", headers: { authorization: "Bearer " } },
  { name: "no header at all", headers: {} },
  { name: "a token that resolves to nothing", headers: { "x-api-key": "wrong" } },
];

const authHeaderOut = [];
for (const c of authHeaderCases) {
  __clearCaches();
  // The body is carried but never read: the route match answers before the body scan on this path.
  const request = new Request("https://relay.example/v1/messages", {
    method: "GET",
    headers: { "content-type": "application/json", ...c.headers },
  });
  const response = await handleGateway(request, envWith({ DEEPSEEK_API_KEY: "sk-test" }), new URL(request.url));
  // **WHAT THE TOKEN RESOLVED TO, RECORDED RATHER THAN RE-DERIVED** — the same rule the route corpus uses
  // for its 401 case. The oracle knows which spelling it sent, so it can say whether the store would have
  // found the user; a Rust test cannot know that from the status alone without reasoning backwards.
  const sent = c.headers["x-api-key"] || c.headers.authorization || "";
  const effective = c.headers["x-api-key"] ? c.headers["x-api-key"] : sent.startsWith("Bearer ") ? sent.slice(7) : "";
  authHeaderOut.push({
    name: c.name,
    headers: c.headers,
    // The headers as the WIRE spells them, because the port reads them by name.
    xApiKey: c.headers["x-api-key"] ?? null,
    authorization: c.headers.authorization ?? null,
    tokenResolves: effective === TOKEN,
    user: effective === TOKEN ? { id: UID, enabled: true } : null,
    expected: {
      status: response.status,
      headers: Object.fromEntries(response.headers),
      body: await response.text(),
    },
  });
}
writeFileSync(
  new URL("./fixtures/auth-header-corpus.json", import.meta.url),
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/route-oracle.mjs from the SHIPPING handleGateway. Do not edit by hand.",
      // **THE TOKEN THE FAKE KV HOLDS**, recorded so the replay can ask "did these headers name it"
      // without hard-coding a string the fixture owns.
      validToken: TOKEN,
      cases: authHeaderOut,
    },
    null,
    1,
  ) + "\n",
);

// ── THE LIVE SSE RESPONSE, RECORDED FROM THE SHIPPING ROUTE ──────────────────────────────────────
// **THE TRANSFORM IS ALREADY PINNED; WHAT WAS NOT IS THE WIRING AROUND IT.** `stream-frame-corpus.json` drives
// `streamOgToAnthropic` DIRECTLY, so it cannot see the response's status, its headers, or whether the bytes
// survive the transport. This section drives the whole shipping `handleGateway` with a stubbed
// `text/event-stream` upstream and records the response as the fixture `verify.mjs` replays against the BUILT
// wasm worker — the plan's P3 criterion, on the half whose wiring was still unproven.
//
// Measured 2026-10-06 with the pair: 864 B (og translate), 256 B (ds passthrough) and 807 B (a JSON upstream
// for a `stream: true` request) — byte-identical on both sides. The three cases are the three branches the
// live path can take: a translated stream, a forwarded one, and the one where the upstream ignored `stream`.
const liveCases = [
  {
    name: "og: an OpenAI SSE stream becomes an Anthropic one",
    model: "og/deepseek-v4.1-flash",
    keys: { OPENCODE_GO_API_KEY: "sk-user-og" },
    upstreamBody: 'data: {"choices":[{"delta":{"content":"hi"}}]}\n\ndata: [DONE]\n\n',
    upstreamType: "text/event-stream",
  },
  {
    name: "ds: an Anthropic SSE stream is FORWARDED",
    model: "ds/deepseek-v4.1-flash",
    keys: { DEEPSEEK_API_KEY: "sk-user-ds" },
    upstreamBody:
      'event: message_start\ndata: {"type":"message_start","message":{"id":"m1","content":[]}}\n\n' +
      'event: content_block_delta\ndata: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}\n\n' +
      'event: message_stop\ndata: {"type":"message_stop"}\n\n',
    upstreamType: "text/event-stream",
  },
  {
    name: "og: a stream:true request whose upstream answered JSON",
    model: "og/deepseek-v4.1-flash",
    keys: { OPENCODE_GO_API_KEY: "sk-user-og" },
    upstreamBody: JSON.stringify({
      choices: [{ message: { role: "assistant", content: "hello" }, finish_reason: "stop" }],
      usage: { prompt_tokens: 1, completion_tokens: 1 },
    }),
    upstreamType: "application/json",
  },
];

const liveOut = [];
for (const c of liveCases) {
  const realFetch = globalThis.fetch;
  globalThis.fetch = async () => {
    const chunk = new TextEncoder().encode(c.upstreamBody);
    const body = new ReadableStream({
      start(controller) {
        controller.enqueue(chunk);
        controller.close();
      },
    });
    return new Response(body, { status: 200, headers: { "content-type": c.upstreamType } });
  };
  try {
    __clearCaches();
    const request = new Request("https://relay.example/v1/messages", {
      method: "POST",
      headers: { "x-api-key": TOKEN, "content-type": "application/json" },
      body: JSON.stringify({
        model: c.model,
        messages: [{ role: "user", content: "hi" }],
        max_tokens: 16,
        stream: true,
      }),
    });
    const response = await handleGateway(request, envWith(c.keys), new URL(request.url));
    liveOut.push({
      name: c.name,
      model: c.model,
      // The inputs the replay needs: the raw body it sends, and the upstream's answer as BYTES (the fixture
      // carries the exact stream the shipping route was handed, so the wasm side is handed the same one).
      rawText: JSON.stringify({
        model: c.model,
        messages: [{ role: "user", content: "hi" }],
        max_tokens: 16,
        stream: true,
      }),
      ukeys: c.keys,
      upstreamBody: c.upstreamBody,
      upstreamType: c.upstreamType,
      expected: {
        status: response.status,
        contentType: response.headers.get("content-type") ?? "",
        // **THE BYTES, NOT A PARAPHRASE**: this is the whole comparison, so the fixture carries the body as
        // the text it is rather than a summary of it.
        body: await response.text(),
      },
    });
  } finally {
    globalThis.fetch = realFetch;
  }
}
writeFileSync(
  new URL("./fixtures/stream-response-corpus.json", import.meta.url),
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/route-oracle.mjs from the SHIPPING handleGateway, fetch stubbed with a live SSE body.",
      cases: liveOut,
    },
    null,
    1,
  ) + "\n",
);

// ── THE FRONT DOOR, NOT THE ROUTE: status, EVERY header, and the body ────────────────────────────
// **EVERY OTHER SECTION HERE DRIVES `handleGateway`, WHICH IS THE ROUTE AND NOT THE DOOR.** `index.ts` owns
// the global OPTIONS preflight, the `withCors` stamp that reflects an allowlisted `Origin`, the console-host
// isolation and the 404 for a non-`/v1/` path — and those are exactly what a route-level comparison cannot
// see. Measured 2026-10-06 with both doors driven side by side: the shipping door answered
// `access-control-allow-origin: https://api.saisi.online` plus `vary: Origin` on all nine cases, and the wasm
// worker answered only the static `access-control-allow-headers/methods` pair — so a browser client (the
// console's own UI among them) would have had every cross-origin response refused at the cutover, on routes
// whose status and body matched exactly.
//
// The env is the deployed shape, read through the API: `CONSOLE_HOST` (hostnames, for the console-host
// isolation) and `CONSOLE_ORIGINS` (origins, for the allowlist — the name `allowedOrigins(env)` reads, and the
// one the wasm door was NOT reading).
const doorEnv = (keys = {}) => {
  const env = envWith(keys);
  env.CONSOLE_HOST = "ai.saisi.online,api.saisi.online";
  env.CONSOLE_ORIGINS = "https://ai.saisi.online,https://api.saisi.online";
  env.UPSTREAM_TIMEOUT_MS = "120000";
  return env;
};

const ALLOWED_ORIGIN = "https://api.saisi.online";
const FOREIGN_ORIGIN = "https://evil.example";

const doorCases = [
  { name: "OPTIONS /v1/messages — the preflight, allowed origin", method: "OPTIONS", path: "/v1/messages", origin: ALLOWED_ORIGIN },
  { name: "OPTIONS /nope — the preflight is GLOBAL", method: "OPTIONS", path: "/nope", origin: ALLOWED_ORIGIN },
  { name: "GET /v1/models — the public listing, allowed origin", method: "GET", path: "/v1/models", origin: ALLOWED_ORIGIN },
  { name: "GET /v1/models — a FOREIGN origin gets no ACAO", method: "GET", path: "/v1/models", origin: FOREIGN_ORIGIN },
  { name: "GET /v1/messages — 401, allowed origin", method: "GET", path: "/v1/messages", origin: ALLOWED_ORIGIN },
  { name: "POST /v1/messages — a bad token, allowed origin", method: "POST", path: "/v1/messages", origin: ALLOWED_ORIGIN, token: "wrong", body: true },
  { name: "GET /foo/v1/messages — not the front door's prefix", method: "GET", path: "/foo/v1/messages", origin: ALLOWED_ORIGIN },
  { name: "GET /nope — not the front door", method: "GET", path: "/nope", origin: ALLOWED_ORIGIN },
  { name: "GET /api/health — the public tooling route", method: "GET", path: "/api/health", origin: ALLOWED_ORIGIN },
];

const shippingDoor = (await import("../src/index.ts")).default;
const doorOut = [];
for (const c of doorCases) {
  __clearCaches();
  const request = new Request(`https://api.saisi.online${c.path}`, {
    method: c.method,
    headers: {
      origin: c.origin,
      "content-type": "application/json",
      ...(c.method === "POST" ? { "x-api-key": c.token ?? TOKEN } : {}),
    },
    ...(c.body
      ? { body: JSON.stringify({ model: "ds/deepseek-v4.1-flash", messages: [] }) }
      : {}),
  });
  const response = await shippingDoor.fetch(request, doorEnv({ DEEPSEEK_API_KEY: "sk-user-ds" }), {});
  doorOut.push({
    name: c.name,
    method: c.method,
    path: c.path,
    origin: c.origin,
    token: c.method === "POST" ? c.token ?? TOKEN : null,
    body: c.body ? true : false,
    // **EVERY HEADER, SORTED** — the comparison is the whole response, and the CORS pair is what this section
    // exists for. Sorted so the fixture does not depend on header order.
    expected: {
      status: response.status,
      headers: [...response.headers.entries()]
        .map(([k, v]) => `${k.toLowerCase()}: ${v}`)
        .sort(),
      body: await response.text(),
    },
  });
}
writeFileSync(
  new URL("./fixtures/front-door-cors-corpus.json", import.meta.url),
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/route-oracle.mjs from the SHIPPING front door (`index.ts`), env as deployed.",
      consoleHost: "ai.saisi.online,api.saisi.online",
      consoleOrigins: "https://ai.saisi.online,https://api.saisi.online",
      cases: doorOut,
    },
    null,
    1,
  ) + "\n",
);

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
    // **THE SESSION HEADER, RECORDED RATHER THAN RE-DERIVED.** `opencodeSessionHeader(request.headers,
    // user?.id)` reads the CLIENT's headers and the user id, and a Rust test cannot reconstruct what the
    // oracle's request carried — so the fixture records the object the route actually spread into its
    // headers. It is the header zen/go 400s without.
    ogSession: opencodeSessionHeader(
      new Request("https://relay.example/v1/messages", { headers: { "x-api-key": TOKEN } }).headers,
      UID,
    ),
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
// ── `GET /v1/models` ─────────────────────────────────────────────────────────────────────────────
// PUBLIC (it is what a client reads before it chooses anything), so no token. The cases are the three
// inputs the arm reads: the disabled list, the console-added models, and the facet overrides — and the
// second one is the interesting one, because removing an entry SHIFTS every `created` value after it.
const modelsCases = [
  { name: "an untouched deployment lists the registry verbatim" },
  { name: "one disabled id is removed AND the created values shift", disabled: ["og/mimo-v2.5"] },
  { name: "two disabled ids", disabled: ["og/mimo-v2.5", "cm/deepseek/deepseek-v4.1-flash"] },
  {
    name: "a console-added model is APPENDED",
    custom: [{ id: "og/console-added", ownedBy: "opencode" }],
  },
  {
    // **A CUSTOM PROVIDER, WHICH IS THE ONLY THING THAT EXERCISES `advertisedProviderModels`.** Its models are
    // advertised under its prefix, and one of the two providers here has NO prefix — the case that caught
    // `js_text(null)` returning the TEXT "null" where the source's `?? ""` gives the empty string.
    name: "a custom provider advertises its models",
    providerRecords: [
      {
        prefix: "acme/",
        label: "Acme Cloud",
        models: [
          { id: "acme-chat", name: "Acme Chat", contextWindow: 128000 },
          { id: "/leading-slash" },
        ],
      },
      { prefix: "", label: "No Prefix At All", models: [{ id: "orphan" }] },
    ],
  },
  {
    // The wire already carries the prefix: `advertisedModelId` must not prepend it twice.
    name: "a provider whose wire ALREADY carries its prefix",
    providerRecords: [
      { prefix: "acme", models: [{ id: "acme/chat" }, { id: "other" }] },
    ],
  },
  {
    name: "a facet override decorates a built-in listing",
    overrides: [{ id: "og/deepseek-v4.1-flash", name: "Renamed by an operator", contextWindow: 200000 }],
  },
];
const modelsOut = [];
for (const c of modelsCases) {
  const r = await callModels(c);
  modelsOut.push({
    name: c.name,
    disabled: c.disabled || [],
    liveIds: r.liveIds,
    extraEntries: r.extraEntries,
    facet: r.facet,
    disabled: r.disabled,
    custom: r.custom,
    providers: r.providers,
    expected: { status: r.status, headers: r.headers, body: r.body },
  });
}
writeFileSync(
  new URL("./fixtures/models-corpus.json", import.meta.url),
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/route-oracle.mjs from the SHIPPING handleGateway.",
      cases: modelsOut,
    },
    null,
    1,
  ) + "\n",
);
// ── the upstream FAILURE path ────────────────────────────────────────────────────────────────────
// `upstreamBodyErrorResponse`: unwrap AMD's `{"detail":{…}}`, scrub a leaked key in two layers, keep the
// upstream's OWN error.type when it is a known Anthropic type ("Claude Code keys retry/auth flows off it"),
// and carry `Retry-After`. The secrets come from the request the route actually sent.
const failureCases = [
  {
    name: "a plain upstream error keeps its message",
    upstreamStatus: 500,
    upstreamBody: JSON.stringify({ error: { message: "upstream exploded" } }),
  },
  {
    name: "AMD's detail envelope is UNWRAPPED",
    upstreamStatus: 400,
    upstreamBody: JSON.stringify({ detail: { error: { message: "from inside detail", type: "invalid_request_error" } } }),
  },
  {
    name: "a KNOWN upstream type is kept",
    upstreamStatus: 400,
    upstreamBody: JSON.stringify({ error: { message: "bad", type: "permission_error" } }),
  },
  {
    name: "an UNKNOWN upstream type is replaced by the status's own",
    upstreamStatus: 500,
    upstreamBody: JSON.stringify({ error: { message: "bad", type: "made_up_type" } }),
  },
  {
    name: "a 429 becomes rate_limit_error",
    upstreamStatus: 429,
    upstreamBody: JSON.stringify({ error: { message: "slow down" } }),
  },
  {
    name: "Retry-After rides as a HEADER",
    upstreamStatus: 429,
    upstreamBody: JSON.stringify({ error: { message: "slow down" } }),
    retryAfter: "17",
  },
  {
    name: "a NON-JSON error body falls back to the status sentence",
    upstreamStatus: 502,
    upstreamBody: "<html>gateway timeout</html>",
  },
  {
    name: "a bare object with no message is STRINGIFIED and truncated",
    upstreamStatus: 500,
    upstreamBody: JSON.stringify({ weird: true }),
  },
  {
    name: "an EMPTY message falls through to the next candidate",
    upstreamStatus: 500,
    upstreamBody: JSON.stringify({ error: { message: "" }, message: "the second candidate" }),
  },
];
const failureOut = [];
for (const c of failureCases) {
  const r = await callNonStream({
    model: "og/deepseek-v4.1-flash",
    body: { messages: [{ role: "user", content: "hi" }], max_tokens: 16 },
    keys: { OPENCODE_GO_API_KEY: "sk-og" },
    upstreamBody: c.upstreamBody,
    upstreamStatus: c.upstreamStatus,
    retryAfter: c.retryAfter,
  });
  failureOut.push({
    name: c.name,
    status: c.upstreamStatus,
    // The parsed body, `null` when it is not JSON at all — the same value `upstream.json().catch(() => null)`
    // hands the helper.
    json: (() => {
      try {
        return JSON.parse(c.upstreamBody);
      } catch {
        return null;
      }
    })(),
    retryAfter: c.retryAfter || null,
    // **THE KIND, NOT THE LABEL.** The label that rides the message is `translateLabel`'s answer
    // (`og` for an opencode route, `cm` for a commandgoat one), and the Rust side computes it from this —
    // a fixture that recorded the label itself would not exercise that rule at all.
    kind: "opencode",
    expected: { status: r.status, headers: r.headers, body: r.body },
  });
}
writeFileSync(
  new URL("./fixtures/upstream-failure-corpus.json", import.meta.url),
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/route-oracle.mjs from the SHIPPING handleGateway, fetch stubbed.",
      cases: failureOut,
    },
    null,
    1,
  ) + "\n",
);
// ── the `/v1/chat/completions` arm ───────────────────────────────────────────────────────────────
// **A PASSTHROUGH FOR EVERY KIND** — the source's own words: "the translate plugin reshapes Anthropic
// /v1/messages → chat/completions (the og pattern), while OpenAI-format /v1/chat/completions passes through
// directly". Its request differs from the messages passthrough in TWO ways, and both are here:
//
//   * `"role":"developer"` becomes `"role":"system"` by `split`/`join` on the RAW TEXT — so an occurrence
//     inside a message's CONTENT is rewritten too, which a parse-based port would not do;
//   * the retry policy is `ogTimeoutMs`, not `passthroughTimeoutMs`.
const chatCases = [
  {
    name: "chat: a developer role is rewritten",
    model: "og/deepseek-v4.1-flash",
    body: { messages: [{ role: "developer", content: "hi" }], max_tokens: 16 },
    keys: { OPENCODE_GO_API_KEY: "sk-og" },
    path: "/v1/chat/completions",
  },
  {
    name: "chat: the STRING inside a content is rewritten too, because it is split/join",
    model: "og/deepseek-v4.1-flash",
    body: {
      messages: [{ role: "user", content: 'say the words "role":"developer" please' }],
    },
    keys: { OPENCODE_GO_API_KEY: "sk-og" },
    path: "/v1/chat/completions",
  },
  {
    name: "chat: two occurrences, both rewritten",
    model: "og/deepseek-v4.1-flash",
    body: {
      messages: [
        { role: "developer", content: "a" },
        { role: "developer", content: "b" },
      ],
    },
    keys: { OPENCODE_GO_API_KEY: "sk-og" },
    path: "/v1/chat/completions",
  },
  {
    // **THE RAW FORM *DOES* MATCH NESTED SYNTAX** — a string value escapes its quotes, but a nested OBJECT
    // key does not, so this one IS rewritten. The pair of cases is what makes the rule precise.
    name: "chat: a NESTED role key is rewritten, because it is unescaped syntax",
    model: "og/deepseek-v4.1-flash",
    body: {
      messages: [{ role: "user", content: "hi" }],
      metadata: { role: "developer" },
    },
    keys: { OPENCODE_GO_API_KEY: "sk-og" },
    path: "/v1/chat/completions",
  },
  {
    name: "chat: a plain request is untouched",
    model: "ds/deepseek-v4.1-flash",
    body: { messages: [{ role: "user", content: "hi" }] },
    keys: { DEEPSEEK_API_KEY: "sk-ds" },
    path: "/v1/chat/completions",
  },
];
const chatOut = [];
for (const c of chatCases) {
  const r = await callPassthrough(c);
  chatOut.push({
    name: c.name,
    kind: c.model.startsWith("ds/") ? "deepseek" : "opencode",
    upstreamModel: c.model.startsWith("ds/") ? "deepseek-v4.1-flash" : "deepseek-flash",
    rawText: JSON.stringify({ ...c.body, model: c.model }),
    parsed: { ...c.body, model: c.model },
    bearerKey: c.keys.DEEPSEEK_API_KEY || c.keys.OPENCODE_GO_API_KEY,
    ogSession: opencodeSessionHeader(
      new Request("https://relay.example/v1/messages", { headers: { "x-api-key": TOKEN } }).headers,
      UID,
    ),
    status: r.status,
    ...(r.captured ? { captured: r.captured } : { noUpstreamCall: true, answer: r.answer }),
  });
}
writeFileSync(
  new URL("./fixtures/chat-corpus.json", import.meta.url),
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/route-oracle.mjs from the SHIPPING handleGateway, fetch stubbed.",
      cases: chatOut,
    },
    null,
    1,
  ) + "\n",
);
// ── the `/v1/responses` arm ──────────────────────────────────────────────────────────────────────
// It serves ONE family — the source: "only registered og/muse-spark-* Contributor models ride this endpoint.
// Responses requests naming anything else are client bugs". Its exit is FORCED (the Contributor tier is
// responses-only upstream AND Meta region-blocks it for CN), its headers are its OWN (no anthropic-version),
// and both refusals wear `invalid_request` rather than `invalid_request_error`.
const responsesCases = [
  {
    name: "responses: a Contributor model rides the forced exit",
    model: "og/muse-spark-1.3-contributor",
    body: { input: "hi" },
    keys: { OPENCODE_GO_API_KEY: "sk-og" },
    path: "/v1/responses",
  },
  {
    name: "responses: the OTHER Contributor model",
    model: "og/muse-spark-1.2-contributor",
    body: { input: "hi" },
    keys: { OPENCODE_GO_API_KEY: "sk-og" },
    path: "/v1/responses",
  },
  {
    name: "responses: a NON-responses model is refused",
    model: "og/deepseek-v4.1-flash",
    body: { input: "hi" },
    keys: { OPENCODE_GO_API_KEY: "sk-og" },
    path: "/v1/responses",
  },
  {
    name: "responses: an UNREGISTERED model is refused",
    model: "og/muse-spark-9.9-contributor",
    body: { input: "hi" },
    keys: { OPENCODE_GO_API_KEY: "sk-og" },
    path: "/v1/responses",
  },
];
const responsesOut = [];
for (const c of responsesCases) {
  // **THE EXIT IS POINTED AT A TEST HOST, SO THE FIXTURE CARRIES NO PRODUCTION HOSTNAME.** The DEFAULT exit is
  // a host of ours, and `agent/tests/production_host.rs` refuses a file that names one outside its declared
  // list — a fixture is not a good reason to grow that list, and the gate's own note says it "may only shrink".
  // The default is pinned where it LIVES instead (`routing.rs`, already declared, has its own test).
  const r = await callPassthrough({ ...c, env: { MUSE_RESPONSES_EXIT: "https://exit.example" } });
  responsesOut.push({
    name: c.name,
    model: c.model,
    kind: "opencode",
    prefix: c.model.split("/")[0] || "",
    upstreamModel: wireModelName(
      c.model.split("/")[0] || "",
      c.model.slice((c.model.split("/")[0] || "").length + 1),
    ),
    rawText: JSON.stringify({ ...c.body, model: c.model }),
    bearerKey: c.keys.OPENCODE_GO_API_KEY,
    ogSession: opencodeSessionHeader(
      new Request("https://relay.example/v1/responses", { headers: { "x-api-key": TOKEN } }).headers,
      UID,
    ),
    status: r.status,
    ...(r.captured ? { captured: r.captured } : { noUpstreamCall: true, answer: r.answer }),
  });
}
writeFileSync(
  new URL("./fixtures/responses-corpus.json", import.meta.url),
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/route-oracle.mjs from the SHIPPING handleGateway, fetch stubbed.",
      cases: responsesOut,
    },
    null,
    1,
  ) + "\n",
);
// ── the streaming path's FRAMING ─────────────────────────────────────────────────────────────────
// `streamOgToAnthropic` is driven directly with a stream built from a string, and its OUTPUT is captured. The
// Rust side feeds the same chunks through `SseFrameReader` + `AnthropicStreamEncoder` and must produce the
// same bytes — which is the whole streaming path's decision, because the rest of it is a `pull` loop.
const streamFrameCases = [
  {
    name: "one complete frame",
    chunks: [
      'data: {"choices":[{"delta":{"content":"hi"}}]}\n\n',
      "data: [DONE]\n\n",
    ],
  },
  {
    name: "a frame SPLIT across two chunks",
    chunks: [
      'data: {"choices":[{"delta":{"con',
      'tent":"split"}}]}\n\ndata: [DONE]\n\n',
    ],
  },
  {
    name: "a frame with NO data: line is skipped",
    chunks: ["event: ping\n\n", 'data: {"choices":[{"delta":{"content":"x"}}]}\n\n'],
  },
  {
    name: "TWO data: lines in one frame — the FIRST wins",
    chunks: [
      'data: {"choices":[{"delta":{"content":"first"}}]}\ndata: {"choices":[{"delta":{"content":"second"}}]}\n\n',
    ],
  },
  {
    name: "a MALFORMED payload is swallowed",
    chunks: ["data: {not json\n\n", 'data: {"choices":[{"delta":{"content":"after"}}]}\n\n'],
  },
  {
    name: "CRLF DOES split, because the buffer is normalized first",
    chunks: ['data: {"choices":[{"delta":{"content":"crlf"}}]}\r\n\r\n'],
  },
  {
    name: "a bare data: line yields an empty payload",
    chunks: ["data:\n\n", 'data: {"choices":[{"delta":{"content":"after"}}]}\n\n'],
  },
  {
    name: "a trailing partial frame is left in the buffer",
    chunks: ['data: {"choices":[{"delta":{"content":"a"}}]}\n\n', "data: {\"choices\":["],
  },
];
// ── the two ENDINGS ──────────────────────────────────────────────────────────────────────────────
// The `pull` loop has two ways out and each emits its OWN sentence. A read failure is reported only when
// something was in flight; a clean end only when NOTHING started.
const streamEndCases = [
  {
    name: "end: a read failure with a PARTIAL frame in the buffer",
    chunks: ['data: {"choices":[{"delta":{"content":"a"}}]}\n\ndata: {"cho'],
    error: true,
  },
  {
    name: "end: a read failure with NOTHING buffered but the encoder started",
    chunks: ['data: {"choices":[{"delta":{"content":"a"}}]}\n\n'],
    error: true,
  },
  {
    name: "end: a read failure before ANYTHING arrived",
    chunks: [],
    error: true,
  },
  {
    name: "end: an EMPTY stream that closed cleanly",
    chunks: [],
  },
  {
    name: "end: a clean close with only NON-data frames",
    chunks: ["event: ping\n\n"],
  },
];
const streamFrameOut = [];
for (const c of [...streamFrameCases, ...streamEndCases]) {
  // **`start`-TIME `controller.error` DISCARDS EVERYTHING ALREADY ENQUEUED** — measured here: the read-failure
  // cases came back with the FINISH events and NO error frame, because the reader never saw a byte, so
  // `(buffer || started)` was false. The "died mid-response" branch needs a stream that FAILS AFTER A CHUNK WAS
  // READ, which is what the `pull` form below builds: one chunk per pull, then an error.
  let pulled = 0;
  const upstreamBody = new ReadableStream(
    c.error
      ? {
          pull(controller) {
            if (pulled < c.chunks.length) {
              controller.enqueue(new TextEncoder().encode(c.chunks[pulled++]));
              return;
            }
            controller.error(new Error("upstream died"));
          },
        }
      : {
          start(controller) {
            for (const chunk of c.chunks) controller.enqueue(new TextEncoder().encode(chunk));
            controller.close();
          },
        },
  );
  const out = streamOgToAnthropic(upstreamBody, "client-model", "upstream-model");
  const text = await new Response(out).text();
  streamFrameOut.push({
    name: c.name,
    // **THE FLAG HAS TO TRAVEL WITH THE CASE.** My first version captured the OUTPUT of a failing stream and
    // not the fact that it failed, so a replay could not know which branch produced those bytes.
    error: !!c.error,
    chunks: c.chunks,
    clientModel: "client-model",
    upstreamModel: "upstream-model",
    expected: text,
  });
}
writeFileSync(
  new URL("./fixtures/stream-frame-corpus.json", import.meta.url),
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/route-oracle.mjs driving the SHIPPING streamOgToAnthropic.",
      cases: streamFrameOut,
    },
    null,
    1,
  ) + "\n",
);
console.log(`route-oracle: ${out.length} count_tokens and ${orderOut.length} front-door order and ${authHeaderOut.length} auth-header and ${liveOut.length} live-stream and ${doorOut.length} front-door case(s) and ${passthroughOut.length} request case(s) and ${streamIgnoredOut.length} stream-ignored and ${nonStreamOut.length} non-stream and ${modelsOut.length} models and ${failureOut.length} failure and ${chatOut.length} chat and ${responsesOut.length} responses and ${streamFrameOut.length} stream-frame case(s) written`);
