// verify.mjs — the byte comparison, and the reason "the same request" is a fact rather than a claim.
//
// ── THE MUTATION THAT MUST FAIL THIS CHECK ──────────────────────────────────────────────────────
// Read this when you change this file: the mutation is how you find out whether the check can still
// fail at all. A check that cannot be broken is worse than no check.
//
// MUTATION: in `src/lib.rs`, drop ONE character from one model id in `HEALTH_CHANNELS` —
//           `("cm", "cm/deepseek/deepseek-v4.1-flash")` → `…v4.1-flas")` — and rebuild.
// RESULT:   exit 1, both cases, naming the byte counts rather than only the verdict:
//             FAIL …/api/health — 1228 bytes, status 200
//             body bytes DIFFERS   ts: "1228"   rust: "1226"
//             body DIFFERS         ts: "{…\"model\":\"cm/deepseek/deepseek-v4.1-flash\"}…"
//           (measured 2026-09-28, on this file's first run against a real mutation). The 404 arm
//           stayed green in the same run, which is the point: the failure was a FINDING about the
//           route and not a broken harness.
//
// THE CRITERION (P3's own): the same request produces a BYTE-COMPARABLE response from the old and
// the new route — not "it works", the bytes. So this runs BOTH, on the same request, and compares
// status, headers and the response BODY as bytes.
//
//   old — `gateway/src/tooling.ts`'s `buildHealth`, the TypeScript the deployed `vale-gate` runs,
//         wrapped in `http.ts`'s `jsonOk` exactly as `index.ts` wraps it (`jsonOk(await
//         buildHealth(env))`). The `withCors` stamp is applied by the DISPATCH layer to every
//         route's response, not by this route, so it is on neither side here — see the module
//         header of src/lib.rs for why that boundary is the honest one.
//   new — `gateway/wasm/build/index.js`, the module `worker-build --release` produced, executed
//         through its real `fetch` entrypoint with the same env and the same request URL.
//
// THE OUTPUT SPACE IS TWO DOCUMENTS, SO THIS IS EXHAUSTIVE RATHER THAN SAMPLED. `build_health`'s
// only non-constant input is the og circuit breaker (one boolean: `reliability.ts`'s
// `isChannelDegraded` reads BreakerDO and compares the body to "1"), and every other card is `ok`
// by construction. Both values are run below, so between them they cover every response this route
// can produce.
//
// THE BREAKER IS STUBBED ON BOTH SIDES WITH THE SAME OBJECT SHAPE, and the shape is not invented:
// `gateway/test/health.test.mjs` already builds `{ BREAKER: { idFromName, get } }`, and the class
// below is named `DurableObjectNamespace` because workers-rs' `EnvBinding::get` duck-types on
// `obj.constructor().name` (worker-0.8.7/src/env.rs:148) — the real runtime's namespace object has
// that exact constructor name, so this stub is what the binding check is written against.
//
// WHAT IT DOES NOT COVER, stated rather than implied: Cloudflare's own runtime glue (both
// implementations would run under workerd in production, and workerd will not start on this box),
// and the DO's real `/check` handler — the stub answers "1"/"0" directly, which is exactly what
// BreakerDO's `/check` returns (reliability.ts compares the TEXT to "1").
import { register } from "node:module";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";

register("./loader.mjs", import.meta.url);

// The third runtime global the glue reaches for: it registers a `error` listener at module scope to
// reinitialise the wasm after a panic (the panic-recovery hook workers-rs 0.8.7 added). Node's
// globalThis is not an EventTarget here, so this is the same class of shim as the loader's two.
globalThis.addEventListener ??= () => {};

const HERE = fileURLToPath(new URL(".", import.meta.url));
const BUILT = fileURLToPath(new URL("build/index.js", import.meta.url));
// A TEST HOST, and it is not a disguise: this route reads the PATH and nothing else (`build_health`
// never sees the URL), so the host cannot change either side's bytes — and `production-host-check`
// (`agent/tests/production_host.rs`) refuses the deployment's real hostname in any file that has not
// declared it, which is that gate doing its job rather than an inconvenience.
const URL_UNDER_TEST = "https://console.test/api/health";

// ── build ───────────────────────────────────────────────────────────────────────────────────────
// **THIS HARNESS ASSUMED A PRE-BUILT `build/` AND NOTHING EVER MADE ONE.** Measured 2026-10-05, on a clean
// tree: `ENOENT: no such file or directory, open '…/gateway/wasm/build/index_bg.wasm.mjs'` — the file it
// writes below, into a directory that did not exist. `index/worker/verify.mjs` and both satellite
// harnesses build first; this one does now too, and `--build` forces it.
if (!existsSync(BUILT) || process.argv.includes("--build")) {
  console.log("  building with worker-build --release …");
  execFileSync("worker-build", ["--release"], { cwd: HERE, stdio: "inherit" });
}

// The compiled module the glue expects Cloudflare's bundler to hand it (see loader.mjs).
writeFileSync(
  `${HERE}build/index_bg.wasm.mjs`,
  `import { readFileSync } from "node:fs";\n` +
    `export default new WebAssembly.Module(readFileSync(new URL("./index_bg.wasm", import.meta.url)));\n`,
);

// ── the old route: the TypeScript the deployed worker runs ──────────────────────────────────────
const { buildHealth } = await import(new URL("../src/tooling.ts", import.meta.url).href);
const { jsonOk } = await import(new URL("../src/http.ts", import.meta.url).href);
const { __clearDegradedCache } = await import(new URL("../src/reliability.ts", import.meta.url).href);

/** The Durable Object namespace stub — `constructor.name` is what workers-rs' binding check reads. */
class DurableObjectNamespace {
  constructor(check) {
    this.check = check;
  }
  idFromName() {
    return {};
  }
  get() {
    const { check } = this;
    return { fetch: async () => new Response(check) };
  }
}
const envFor = (check) => ({ BREAKER: new DurableObjectNamespace(check), DO_AUTH: "stub-do-auth" });

const CASES = [
  { check: "0", label: "breaker closed — every card ok" },
  { check: "1", label: "breaker open — the og cards carry reason" },
];

const pairs = (headers) =>
  [...headers.entries()].map(([k, v]) => `${k.toLowerCase()}: ${v}`).sort();

let bad = 0;
for (const [i, c] of CASES.entries()) {
  // The old side. `__clearDegradedCache()` because `degradedCache` is module state with a 5 s TTL,
  // and a stale value would make the second case answer with the first case's bytes.
  __clearDegradedCache();
  const tsResponse = jsonOk(await buildHealth(envFor(c.check)));
  const tsBody = Buffer.from(await tsResponse.arrayBuffer());

  // The new side, in a FRESH module instance per case — the same reason, one level down: the Rust
  // cache is also isolate state, and a fresh instance is also what a new isolate looks like.
  const worker = await import(`${pathToFileURL(BUILT).href}?case=${i}`);
  const instance = new worker.default();
  instance.env = envFor(c.check);
  instance.ctx = {};
  const rsResponse = await instance.fetch(new Request(URL_UNDER_TEST));
  const rsBody = Buffer.from(await rsResponse.arrayBuffer());

  const rows = [
    ["status", String(tsResponse.status), String(rsResponse.status)],
    ["headers", pairs(tsResponse.headers).join(" | "), pairs(rsResponse.headers).join(" | ")],
    ["body bytes", String(tsBody.length), String(rsBody.length)],
    ["body", tsBody.toString("utf8"), rsBody.toString("utf8")],
  ];
  let differs = 0;
  for (const [what, want, got] of rows) if (want !== got) differs++;

  console.log(`  ${c.label}`);
  console.log(`      ${differs ? "FAIL" : "ok  "} ${URL_UNDER_TEST} — ${tsBody.length} bytes, status ${tsResponse.status}`);
  if (differs) {
    bad++;
    for (const [what, want, got] of rows) {
      if (want === got) continue;
      console.log(`      ${what} DIFFERS`);
      console.log(`        ts  : ${JSON.stringify(want).slice(0, 400)}`);
      console.log(`        rust: ${JSON.stringify(got).slice(0, 400)}`);
    }
  } else {
    console.log(`      body is byte-identical (${tsBody.length} bytes), and so are status and headers`);
  }
}

// ── the front-door stub: a path this worker does not serve ──────────────────────────────────────
// NOT part of the route. It is here because a worker that answers only one path is not a worker,
// and an unchecked arm is the one that turns out to be wrong.
{
  const worker = await import(`${pathToFileURL(BUILT).href}?case=404`);
  const instance = new worker.default();
  instance.env = envFor("0");
  instance.ctx = {};
  const rs = await instance.fetch(new Request("https://console.test/api/nothing-here"));
  const body = Buffer.from(await rs.arrayBuffer()).toString("utf8");
  const want = JSON.stringify({ type: "error", error: { type: "not_found_error", message: "Not Found" } });
  const ok = rs.status === 404 && body === want;
  console.log(`  front-door stub (not the route): GET /api/nothing-here`);
  console.log(`      ${ok ? "ok  " : "FAIL"} status ${rs.status}, body ${ok ? "byte-identical" : JSON.stringify(body)}`);
  if (!ok) bad++;
}

// ── THE TOKEN'S TWO DOORS, DRIVEN THROUGH THE BUILT WORKER ───────────────────────────────────────
// **THE CORPUS TEST IN `request_shape.rs` PROVES THE RULE; THIS PROVES THE WIRING.** `effective_token` is a
// pure function and its nine shipping shapes are replayed in Rust — but the line that CALLS it is in
// `v1.rs`, on the other side of the `worker::Env` boundary, and a port that computed the right token and
// then read `x-api-key` anyway would pass that test and answer 401 to every OpenAI-compatible client.
// Measured 2026-10-06: the worker DID read `x-api-key` alone, and the live comparison could not see it —
// a live check needs a VALID token, and this box does not hold one.
//
// **THE KV IS A MAP, WHICH IS WHAT MAKES THIS REACHABLE AT ALL.** `KvStore::from_this` does not duck-type
// the binding the way `DurableObjectNamespace` is duck-typed; it reads `get`/`put`/`list`/`delete` off the
// object (`worker-0.8.7/src/kv/mod.rs:63`), and `kv.get(key).text()` calls `get(key, {type:"text"})` and
// awaits the result. An async function over a `Map` is therefore the whole stub.
{
  const corpus = JSON.parse(
    readFileSync(fileURLToPath(new URL("fixtures/auth-header-corpus.json", import.meta.url)), "utf8"),
  );
  const valid = corpus.validToken;
  const uid = "u-route-test";
  const map = new Map([
    [`token:${valid}`, uid],
    [`user:${uid}`, JSON.stringify({ id: uid, enabled: true })],
  ]);
  const KEYS = {
    async get(key) {
      return map.has(key) ? map.get(key) : null;
    },
    async put(key, value) {
      map.set(key, value);
    },
    async delete(key) {
      map.delete(key);
    },
    async list() {
      return { keys: [...map.keys()].map((name) => ({ name })), list_complete: true };
    },
  };
  const authEnv = { ...envFor("0"), KEYS };
  let authBad = 0;
  for (const [i, c] of corpus.cases.entries()) {
    // A fresh module instance per case, for the same reason as above: the store caches in isolate state.
    const worker = await import(`${pathToFileURL(BUILT).href}?auth=${i}`);
    const instance = new worker.default();
    instance.env = authEnv;
    instance.ctx = {};
    const headers = { "content-type": "application/json" };
    if (c.xApiKey !== null) headers["x-api-key"] = c.xApiKey;
    if (c.authorization !== null) headers.authorization = c.authorization;
    const res = await instance.fetch(
      new Request("https://console.test/v1/messages", { method: "GET", headers }),
    );
    const body = Buffer.from(await res.arrayBuffer()).toString("utf8");
    const ok = res.status === c.expected.status && body === c.expected.body;
    if (!ok) authBad++;
    console.log(
      `      ${ok ? "ok  " : "FAIL"} ${c.name} — status ${res.status} (shipping ${c.expected.status})`,
    );
  }
  console.log(
    `  the token's two doors, through the built worker: ${corpus.cases.length - authBad}/${corpus.cases.length}`,
  );
  bad += authBad;
}

// ── THE DEPLOYMENT'S OWN KEY REACHES THE UPSTREAM ────────────────────────────────────────────────
// **THE PURE LAYER READS A FIXED SET OF NAMES OUT OF `env`, AND `v1.rs` HANDED IT `{"US_PROXY"}` ALONE.**
// Measured 2026-10-06 on the built worker, with a valid token, NO user key in KV, and the deployment's own
// `DEEPSEEK_API_KEY` in the worker's env: `502 config_error — "DEEPSEEK_API_KEY not configured — add your own
// key in the console"`, and **no upstream call at all** — while the shipping `handleGateway` on the same three
// inputs called `api.deepseek.com`, `token-plan.ap-southeast-1.maas.aliyuncs.com` and `opencode.ai` with
// `Bearer sk-env-…`. Every user without a key of their own would have met that at the cutover.
//
// **WHY THIS SECTION IS HERE RATHER THAN IN A RUST TEST**: the gap was not in a decision — every decision is
// proved by its own corpus — it was in the WIRING, on the other side of the `worker::Env` boundary. A Rust
// test drives `key_gate`/`bearer_key_for` with an env object it builds itself, so it cannot see what
// `v1.rs` actually reads; this drives the built worker with the real `Env` lookup and stubs `fetch`, which
// makes the difference visible as a CAPTURED UPSTREAM REQUEST.
//
// **AND THE FOURTH CASE MUST NOT BITE**: `nvidia` deliberately has NO deployment fallback (`BYOK_CHANNELS`
// carries the `None`), so a `nv/` request with no user key must still answer the config error and make no
// call. A fix that plumbed the env by inventing a key would pass the first three cases and fail this one.
{
  // **`let`, DECLARED BEFORE `KEYS`, AND THAT IS NOT STYLE.** The first version declared `kvMap` with `const`
  // INSIDE the loop below, so `KEYS`' closure resolved the name in THIS block, found nothing, and every
  // request came back 401 "Missing or invalid x-api-key" — the stub threw, `kv_text` swallowed it into
  // `None`, and the measurement looked like a broken token rather than a broken harness.
  let kvMap = new Map();
  const KEYS = {
    async get(key) {
      return kvMap.has(key) ? kvMap.get(key) : null;
    },
    async put(key, value) {
      kvMap.set(key, value);
    },
    async delete(key) {
      kvMap.delete(key);
    },
    async list() {
      return { keys: [...kvMap.keys()].map((name) => ({ name })), list_complete: true };
    },
  };
  const cases = [
    {
      model: "ds/deepseek-v4.1-flash",
      key: "DEEPSEEK_API_KEY",
      value: "sk-env-deepseek",
      host: "api.deepseek.com",
      label: "ds/ rides the deployment's DEEPSEEK_API_KEY",
    },
    {
      model: "qw/qwen3.8-flash",
      key: "QWEN_API_KEY",
      value: "sk-env-qwen",
      host: "token-plan.ap-southeast-1.maas.aliyuncs.com",
      label: "qw/ rides the deployment's QWEN_API_KEY",
    },
    {
      model: "og/deepseek-v4.1-flash",
      key: "OPENCODE_GO_API_KEY",
      value: "sk-env-opencode",
      host: "opencode.ai",
      label: "og/ rides the deployment's OPENCODE_GO_API_KEY",
    },
    {
      model: "nv/nvidia/llama-3.3-nemotron",
      key: null,
      value: null,
      host: null,
      label: "nv/ has NO deployment fallback and must still refuse",
      mustNotReach: true,
    },
    {
      // **THE USER'S OWN KEY, IN THE SHAPE THE STORE WRITES** — and with NO env key in play, so the only
      // source that can serve this request is `ukeys:<uid>`. `v1.rs` handed that record to the gate raw until
      // 2026-10-06, and this case is what that cost: `502 config_error`, no upstream call.
      model: "ds/deepseek-v4.1-flash",
      key: null,
      value: null,
      host: "api.deepseek.com",
      label: "the user's OWN key, in the store's shape (DEEPSEEK_API_KEY)",
      userKey: { DEEPSEEK_API_KEY: "sk-user-ds" },
      wantBearer: "Bearer sk-user-ds",
    },
  ];
  const realFetch = globalThis.fetch;
  let keyBad = 0;
  for (const [i, c] of cases.entries()) {
    // The KV is the token and the user record only — NO `ukeys:<uid>`, which is the case under test.
    kvMap = new Map([
      ["token:tok-verify", "u-verify"],
      ["user:u-verify", JSON.stringify({ id: "u-verify", enabled: true })],
      ...(c.userKey ? [["ukeys:u-verify", JSON.stringify(c.userKey)]] : []),
    ]);
    let captured = null;
    globalThis.fetch = async (url, init = {}) => {
      const req = new Request(url, init);
      captured = { url: req.url, method: req.method, headers: {} };
      for (const [k, v] of req.headers.entries()) captured.headers[k] = v;
      return new Response(
        JSON.stringify({
          choices: [{ message: { role: "assistant", content: "ok" }, finish_reason: "stop" }],
          usage: { prompt_tokens: 1, completion_tokens: 1 },
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    };
    const worker = await import(`${pathToFileURL(BUILT).href}?envkey=${i}`);
    const instance = new worker.default();
    instance.env = {
      ...envFor("0"),
      KEYS,
      ...(c.key ? { [c.key]: c.value } : {}),
    };
    instance.ctx = {};
    let status = 0;
    let body = "";
    try {
      const res = await instance.fetch(
        new Request("https://console.test/v1/messages", {
          method: "POST",
          headers: { "x-api-key": "tok-verify", "content-type": "application/json" },
          body: JSON.stringify({
            model: c.model,
            messages: [{ role: "user", content: "hi" }],
            max_tokens: 16,
          }),
        }),
      );
      status = res.status;
      body = Buffer.from(await res.arrayBuffer()).toString("utf8");
    } finally {
      globalThis.fetch = realFetch;
    }
    const auth = captured ? captured.headers.authorization ?? captured.headers.Authorization ?? "-" : "-";
    const ok = c.mustNotReach
      ? captured === null && status === 502 && body.includes("not configured")
      : captured !== null &&
        status === 200 &&
        auth === (c.wantBearer ?? `Bearer ${c.value}`) &&
        captured.url.includes(c.host);
    if (!ok) keyBad++;
    console.log(`      ${ok ? "ok  " : "FAIL"} ${c.label} — status ${status}, upstream ${captured ? auth : "NO CALL"}`);
  }
  console.log(
    `  the deployment's own key, through the built worker: ${cases.length - keyBad}/${cases.length}`,
  );
  bad += keyBad;
}

// ── THE LIVE SSE RESPONSE, BYTE FOR BYTE AGAINST THE SHIPPING ROUTE ──────────────────────────────
// **THE TRANSFORM WAS PINNED AND THE WIRING AROUND IT WAS NOT.** `stream-frame-corpus.json` drives
// `streamOgToAnthropic` directly; it cannot see the response's status, its headers, or whether the bytes
// survive `Response::from_stream`. `fixtures/stream-response-corpus.json` is the shipping `handleGateway`
// driven with a stubbed `text/event-stream` upstream, and this drives the BUILT worker with the same request
// and the same upstream bytes and compares all three — which is the plan's P3 criterion on the half whose
// wiring was still unproven.
//
// **AND THE KEY RIDES KV IN THE SHAPE THE STORE WRITES** (`{DEEPSEEK_API_KEY: …}`), which is the second thing
// this round found: `v1.rs` handed the raw record to a gate that reads `extractByokKeys`' SHORT fields, so a
// user's own key was invisible — measured on the built worker, `502 config_error` and no upstream call, while
// the shape only the tests build reached the upstream. A fixture that recorded the short fields would have
// hidden it, so the corpus carries what the console writes.
{
  const corpus = JSON.parse(
    readFileSync(fileURLToPath(new URL("fixtures/stream-response-corpus.json", import.meta.url)), "utf8"),
  );
  let liveBad = 0;
  for (const [i, c] of corpus.cases.entries()) {
    const kvMap = new Map([
      ["token:tok-live", "u-live"],
      ["user:u-live", JSON.stringify({ id: "u-live", enabled: true })],
      ["ukeys:u-live", JSON.stringify(c.ukeys)],
    ]);
    const KEYS = {
      async get(key) {
        return kvMap.has(key) ? kvMap.get(key) : null;
      },
      async put(key, value) {
        kvMap.set(key, value);
      },
      async delete(key) {
        kvMap.delete(key);
      },
      async list() {
        return { keys: [...kvMap.keys()].map((name) => ({ name })), list_complete: true };
      },
    };
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
    let status = 0;
    let contentType = "";
    let body = "";
    try {
      const worker = await import(`${pathToFileURL(BUILT).href}?live=${i}`);
      const instance = new worker.default();
      // No env keys at all: the ONLY key in play is the user's own, so a gate that cannot see it refuses.
      instance.env = { ...envFor("0"), KEYS };
      instance.ctx = {};
      const res = await instance.fetch(
        new Request("https://console.test/v1/messages", {
          method: "POST",
          headers: { "x-api-key": "tok-live", "content-type": "application/json" },
          body: c.rawText,
        }),
      );
      status = res.status;
      contentType = res.headers.get("content-type") ?? "";
      body = await res.text();
    } finally {
      globalThis.fetch = realFetch;
    }
    const ok = status === c.expected.status && body === c.expected.body;
    if (!ok) liveBad++;
    console.log(
      `      ${ok ? "ok  " : "FAIL"} ${c.name} — ${status} ${body.length} B (shipping ${c.expected.status} ${c.expected.body.length} B, ${JSON.stringify(c.expected.contentType)})`,
    );
  }
  console.log(`  the live SSE response, through the built worker: ${corpus.cases.length - liveBad}/${corpus.cases.length}`);
  bad += liveBad;
}

// ── THE FRONT DOOR, REPLAYED: status, EVERY header, and the body ─────────────────────────────────
// **THE SECTION ABOVE DRIVES ROUTES; THIS ONE DRIVES THE DOOR.** `index.ts` owns the global OPTIONS preflight,
// the `withCors` stamp that reflects an allowlisted `Origin`, and the 404 for a path outside `/v1/` — and
// `fixtures/front-door-cors-corpus.json` is those nine cases recorded from the shipping door itself.
//
// Measured 2026-10-06: the wasm door answered only the static `access-control-allow-headers/methods` pair where
// the shipping door also sent `access-control-allow-origin: <origin>` + `vary: Origin`, so **a browser client
// would have had every cross-origin response refused at the cutover** — on routes whose status and body
// matched exactly. The preflight was worse than missing: it passed `env.var("CONSOLE_HOST")` as the ALLOWLIST,
// a list of hostnames where the rule wants origins, so nothing ever matched.
{
  const corpus = JSON.parse(
    readFileSync(fileURLToPath(new URL("fixtures/front-door-cors-corpus.json", import.meta.url)), "utf8"),
  );
  const TOKEN = "tok-route-test";
  let doorBad = 0;
  for (const [i, c] of corpus.cases.entries()) {
    const kvMap = new Map([
      [`token:${TOKEN}`, "u-route-test"],
      ["user:u-route-test", JSON.stringify({ id: "u-route-test", enabled: true })],
      ["ukeys:u-route-test", JSON.stringify({ DEEPSEEK_API_KEY: "sk-user-ds" })],
    ]);
    const KEYS = {
      async get(key) {
        return kvMap.has(key) ? kvMap.get(key) : null;
      },
      async put(key, value) {
        kvMap.set(key, value);
      },
      async delete(key) {
        kvMap.delete(key);
      },
      async list() {
        return { keys: [...kvMap.keys()].map((name) => ({ name })), list_complete: true };
      },
    };
    const worker = await import(`${pathToFileURL(BUILT).href}?door=${i}`);
    const instance = new worker.default();
    instance.env = {
      ...envFor("0"),
      KEYS,
      CONSOLE_HOST: corpus.consoleHost,
      CONSOLE_ORIGINS: corpus.consoleOrigins,
      UPSTREAM_TIMEOUT_MS: "120000",
    };
    instance.ctx = {};
    const res = await instance.fetch(
      // The fixture's own test host: `agent/tests/production_host.rs` refuses a tracked file that names
      // a deployment hostname outside its declared list, and a fixture is not a reason to grow that list.
      new Request(`https://console.test${c.path}`, {
        method: c.method,
        headers: {
          origin: c.origin,
          "content-type": "application/json",
          ...(c.method === "POST" ? { "x-api-key": c.token ?? TOKEN } : {}),
        },
        ...(c.body ? { body: JSON.stringify({ model: "ds/deepseek-v4.1-flash", messages: [] }) } : {}),
      }),
    );
    const body = await res.text();
    const headers = [...res.headers.entries()].map(([k, v]) => `${k.toLowerCase()}: ${v}`).sort();
    const ok =
      res.status === c.expected.status &&
      body === c.expected.body &&
      JSON.stringify(headers) === JSON.stringify(c.expected.headers);
    if (!ok) doorBad++;
    console.log(`      ${ok ? "ok  " : "FAIL"} ${c.name} — ${res.status}`);
    if (!ok) {
      const missing = c.expected.headers.filter((h) => !headers.includes(h));
      const extra = headers.filter((h) => !c.expected.headers.includes(h));
      if (missing.length) console.log(`          missing: ${JSON.stringify(missing)}`);
      if (extra.length) console.log(`          extra:   ${JSON.stringify(extra)}`);
      if (body !== c.expected.body) console.log(`          body differs: ${JSON.stringify(body.slice(0, 90))}`);
    }
  }
  console.log(`  the front door, through the built worker: ${corpus.cases.length - doorBad}/${corpus.cases.length}`);
  bad += doorBad;
}

// ── THE CUSTOM PROVIDER'S REQUEST, THROUGH THE BUILT WORKER ──────────────────────────────────────
// **THE DECISION IS PINNED IN RUST AND THE WIRING IS PINNED HERE**, for the reason the token's two doors are:
// `resolve_model`/`provider_key` are pure and their eleven cases replay in `routing.rs`, but what the WORKER
// reads off KV (`providers:custom`) and hands the plan is on the other side of the `worker::Env` boundary.
// Measured 2026-10-06 on the built worker before this port: a request for a custom provider's model answered
// `502 config_error — "CMD_API_KEY not configured — add your Command Code key"` and made NO upstream call,
// while the shipping route dialled `https://acme.test/v1/chat/completions` with `Bearer sk-acme-inline` — the
// default channel, which is the failure `providerRoute`'s own comment refuses.
{
  const corpus = JSON.parse(
    readFileSync(fileURLToPath(new URL("fixtures/provider-request-corpus.json", import.meta.url)), "utf8"),
  );
  let providerBad = 0;
  for (const [i, c] of corpus.cases.entries()) {
    const kvMap = new Map([
      ["token:tok-route-test", "u-route-test"],
      ["user:u-route-test", JSON.stringify({ id: "u-route-test", enabled: true })],
      ["providers:custom", JSON.stringify(c.providers)],
    ]);
    const KEYS = {
      async get(key) {
        return kvMap.has(key) ? kvMap.get(key) : null;
      },
      async put(key, value) {
        kvMap.set(key, value);
      },
      async delete(key) {
        kvMap.delete(key);
      },
      async list() {
        return { keys: [...kvMap.keys()].map((name) => ({ name })), list_complete: true };
      },
    };
    const realFetch = globalThis.fetch;
    let captured = null;
    globalThis.fetch = async (url, init = {}) => {
      const req = new Request(url, init);
      captured = { url: req.url, method: req.method, headers: {}, body: "" };
      for (const [k, v] of req.headers.entries()) captured.headers[k] = v;
      captured.body = await req.text();
      return new Response(
        JSON.stringify({
          choices: [{ message: { role: "assistant", content: "ok" }, finish_reason: "stop" }],
          usage: { prompt_tokens: 1, completion_tokens: 1 },
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    };
    let status = 0;
    let body = "";
    try {
      const worker = await import(`${pathToFileURL(BUILT).href}?provider=${i}`);
      const instance = new worker.default();
      // The case's env names (an `apiKeyEnv` binding) ride the worker's env, which is where a secret lives.
      instance.env = { ...envFor("0"), KEYS, ...c.env };
      instance.ctx = {};
      const res = await instance.fetch(
        new Request("https://console.test/v1/messages", {
          method: "POST",
          headers: { "x-api-key": "tok-route-test", "content-type": "application/json" },
          body: c.rawText,
        }),
      );
      status = res.status;
      body = await res.text();
    } finally {
      globalThis.fetch = realFetch;
    }
    let ok;
    if (c.captured) {
      ok =
        captured !== null &&
        status === 200 &&
        captured.url === c.captured.url &&
        captured.method === c.captured.method &&
        captured.body === c.captured.body &&
        (captured.headers.authorization ?? captured.headers.Authorization) ===
          (c.captured.headers.authorization ?? c.captured.headers.Authorization);
    } else {
      ok = captured === null && status === c.status && body === c.answer;
    }
    if (!ok) providerBad++;
    console.log(
      `      ${ok ? "ok  " : "FAIL"} ${c.name} — status ${status}, upstream ${captured ? captured.url : "NO CALL"}`,
    );
    if (!ok && c.captured) {
      console.log(`          shipping: ${c.captured.method} ${c.captured.url} (${c.captured.body.slice(0, 70)})`);
      console.log(`          wasm:     ${captured ? `${captured.method} ${captured.url} (${captured.body.slice(0, 70)})` : "NO CALL"} ${body.slice(0, 70)}`);
    }
  }
  console.log(
    `  the custom provider's request, through the built worker: ${corpus.cases.length - providerBad}/${corpus.cases.length}`,
  );
  bad += providerBad;
}

// ── the bundle, measured here because it is the same artifact ───────────────────────────────────
const { execSync } = await import("node:child_process");
const size = (p) => execSync(`gzip -9 -c '${p}' | wc -c`, { encoding: "utf8" }).trim();
const js = size(`${HERE}build/index.js`);
const wasm = size(`${HERE}build/index_bg.wasm`);
const raw = (p) => readFileSync(p).length;
console.log(`  the module worker-build produced`);
console.log(`      index.js       ${String(raw(`${HERE}build/index.js`)).padStart(8)} B  ${String(js).padStart(8)} B gz`);
console.log(`      index_bg.wasm  ${String(raw(`${HERE}build/index_bg.wasm`)).padStart(8)} B  ${String(wasm).padStart(8)} B gz`);
console.log(`      total          ${String(raw(`${HERE}build/index.js`) + raw(`${HERE}build/index_bg.wasm`)).padStart(8)} B  ${Number(js) + Number(wasm)} B gz`);

if (bad) process.exit(1);
