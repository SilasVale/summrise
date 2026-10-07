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

// ── THE DIVERGENCE SWEEP — the number that answers "how much longer" ─────────────────────────────
// **EVERY CUTOVER BLOCKER FOUND SINCE THE TAKEOVER WAS FOUND THE SAME WAY**: drive the SHIPPING front door and
// the BUILT worker with the same inputs and compare. What was missing was the SUMMARY, so "are we done?" was a
// feeling rather than a count. This is that matrix — same KV, same env, the same stubbed upstream and a
// RECORDING BreakerDO, comparing status, body, headers, the captured upstream request and the DO call log.
//
// Measured 2026-10-06, first run: **11/18 identical**. The seven differences were four families — the breaker's
// read side (`channelDegradedError`), its write side (`/trip` and `/reset`), the per-arm failure envelope, and
// the per-token rate limit — and the sections above now pin each of them individually. This section is what
// keeps them at zero: **0 DIFFERENT IS THE EXIT CRITERION FOR THE CUTOVER.**
{
  const shippingDoor = (await import(new URL("../src/index.ts", import.meta.url).href)).default;
  const { __clearCaches } = await import(new URL("../src/store/cache.ts", import.meta.url).href);

  const UID = "u-sweep";
  const TOKEN = "tok-sweep";
  const JSON_OK = JSON.stringify({
    choices: [{ message: { role: "assistant", content: "ok" }, finish_reason: "stop" }],
    usage: { prompt_tokens: 1, completion_tokens: 1 },
  });
  const SSE_OK = 'data: {"choices":[{"delta":{"content":"hi"}}]}\n\ndata: [DONE]\n\n';

  // **THE NAME IS THE BINDING CHECK**, so this class shadows verify.mjs's own `DurableObjectNamespace` (which
  // takes only `check`) with one that also records the paths. A stub called `RecordingBreaker` makes every DO
  // read on the wasm side fail silently — `read_breaker` returns Err and its caller's `unwrap_or(false)` says
  // "not degraded" — which is how this section's first run reported four breaker differences that were the
  // harness's, not the worker's.
  class DurableObjectNamespace {
    constructor(check, calls) {
      this.check = check;
      this.calls = calls;
    }
    idFromName(name) {
      return { name };
    }
    get() {
      const self = this;
      return {
        fetch: async (url) => {
          self.calls.push(typeof url === "string" ? url : (url?.url ?? "[request]"));
          return new Response(self.check, { status: 200 });
        },
      };
    }
  }

  const kvFor = (extra = {}, token = TOKEN) => {
    const map = new Map([
      [`token:${token}`, UID],
      [`user:${UID}`, JSON.stringify({ id: UID, enabled: true })],
      [
        `ukeys:${UID}`,
        JSON.stringify({ OPENCODE_GO_API_KEY: "sk-user-og", DEEPSEEK_API_KEY: "sk-user-ds" }),
      ],
      ...Object.entries(extra),
    ]);
    return {
      async get(key, type) {
        if (!map.has(key)) return null;
        const v = map.get(key);
        if (type === "json" && typeof v === "string") {
          try {
            return JSON.parse(v);
          } catch {
            return null;
          }
        }
        return v;
      },
      async put(k, v) {
        map.set(k, v);
      },
      async delete(k) {
        map.delete(k);
      },
      async list({ prefix = "" } = {}) {
        return {
          keys: [...map.keys()].filter((k) => k.startsWith(prefix)).map((name) => ({ name })),
          list_complete: true,
          cursor: undefined,
        };
      },
    };
  };

  const SWEEP = [
    { name: "GET /api/health", req: ["GET", "/api/health"] },
    { name: "GET /v1/models", req: ["GET", "/v1/models"] },
    { name: "GET /v1/messages, no token", req: ["GET", "/v1/messages"], token: null },
    {
      name: "POST /v1/messages, no token",
      req: ["POST", "/v1/messages", { model: "og/deepseek-v4.1-flash", messages: [] }],
      token: null,
    },
    {
      name: "POST /v1/messages, bad token",
      req: ["POST", "/v1/messages", { model: "og/deepseek-v4.1-flash", messages: [] }],
      token: "wrong",
    },
    {
      name: "POST /v1/messages, og, upstream JSON 200",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
    },
    {
      name: "POST /v1/messages, og, stream:true, upstream SSE",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
          stream: true,
        },
      ],
      upstream: { body: SSE_OK, type: "text/event-stream", status: 200 },
    },
    {
      name: "POST /v1/messages, ds (passthrough), upstream 200",
      req: [
        "POST",
        "/v1/messages",
        { model: "ds/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
    },
    {
      name: "POST /v1/messages, og, upstream 503",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: "upstream down", type: "text/plain", status: 503 },
    },
    {
      name: "POST /v1/messages, og, upstream 500",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: "upstream exploded", type: "text/plain", status: 500 },
    },
    {
      name: "POST /v1/messages, og, HARD network error",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { throws: true },
    },
    {
      name: "POST /v1/messages, og, breaker DEGRADED",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      breaker: "1",
    },
    {
      name: "POST /v1/messages, retired model",
      req: ["POST", "/v1/messages", { model: "ds/deepseek-v4-flash", messages: [] }],
    },
    {
      name: "POST /v1/messages/count_tokens",
      req: [
        "POST",
        "/v1/messages/count_tokens",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }] },
      ],
    },
    {
      name: "POST /v1/chat/completions, og",
      req: [
        "POST",
        "/v1/chat/completions",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
    },
    { name: "OPTIONS /v1/messages", req: ["OPTIONS", "/v1/messages"] },
    { name: "GET /foo/v1/messages (outside the prefix)", req: ["GET", "/foo/v1/messages"] },
    {
      name: "POST /v1/messages, og, upstream 429 (does it retry?)",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: "slow down", type: "text/plain", status: 429, retryAfter: "1" },
    },
    {
      name: "POST /v1/messages, og, upstream 429 with a JSON body",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: {
        body: JSON.stringify({ error: { message: "slow down", type: "rate_limit_error" } }),
        type: "application/json",
        status: 429,
        retryAfter: "3",
      },
    },
    {
      name: "POST /v1/messages, og, upstream 503 (NOT retried: the billing guard)",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: "upstream down", type: "text/plain", status: 503 },
    },
    {
      name: "POST /v1/messages, og, upstream 502 with Retry-After",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: "bad gateway", type: "text/plain", status: 502, retryAfter: "7" },
    },
    {
      name: "POST /v1/messages, og, upstream that NEVER answers (OG_TIMEOUT_MS=1500)",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { hangs: true },
      env: { OG_TIMEOUT_MS: "1500" },
      deadlineMs: 8000,
    },
    {
      name: "POST /v1/messages, nv (translate branch), upstream 200",
      req: [
        "POST",
        "/v1/messages",
        { model: "nv/meta/llama-3.3-70b-instruct", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
    },
    {
      name: "POST /v1/responses, og/muse-spark-1.2-contributor",
      req: ["POST", "/v1/responses", { model: "og/muse-spark-1.2-contributor", input: "hi" }],
      upstream: {
        body: JSON.stringify({
          id: "r1",
          output: [{ type: "message", content: [{ type: "output_text", text: "ok" }] }],
        }),
        type: "application/json",
        status: 200,
      },
    },
    {
      name: "POST /v1/messages, og, upstream SSE with an IN-BAND error",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
          stream: true,
        },
      ],
      upstream: {
        body: 'data: {"error":{"message":"upstream died","type":"api_error"}}\n\n',
        type: "text/event-stream",
        status: 200,
      },
    },
    {
      name: "POST /v1/messages, og, US_PROXY=1 in the ENV",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      env: { US_PROXY: "1" },
    },
    {
      name: "POST /v1/messages, og, settings:US_PROXY=1 in KV",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "settings:US_PROXY": "1" },
    },
    {
      // The KV setting WINS over the env var — `getGlobalSetting(kv, env)`'s precedence, and a case where the
      // two disagree is the only way to see which one the chain reads.
      name: "POST /v1/messages, og, the KV setting OFF beats the env ON",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      env: { US_PROXY: "1" },
      kv: { "settings:US_PROXY": "0" },
    },
    {
      name: "POST /v1/messages, og/muse-spark-1.2-contributor (FORCED US exit)",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/muse-spark-1.2-contributor", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
    },
    {
      // **THE CASE THAT FOUND A MISSING ENV NAME**: with the allowlist set, the shipping route makes ONE call
      // (the model sees images itself) and this worker made TWO, because `VISION_CAPABLE_MODELS` was not among
      // the names `v1.rs` read off the worker. Same shape as the provider `apiKeyEnv` a round earlier.
      name: "POST /v1/messages, an IMAGE to a model on VISION_CAPABLE_MODELS",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/mimo-v2.5",
          messages: [
            {
              role: "user",
              content: [
                {
                  type: "image",
                  source: { type: "base64", media_type: "image/png", data: "iVBORw0KGgo=" },
                },
                { type: "text", text: "what is this" },
              ],
            },
          ],
          max_tokens: 8,
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      env: { VISION_CAPABLE_MODELS: "og/mimo-v2.5" },
    },
    {
      // A failed describe FAILS the request (round-119) — both sides answer 500 "Internal error".
      name: "POST /v1/messages, an IMAGE with the describe FAILING (upstream 500)",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [
            {
              role: "user",
              content: [
                {
                  type: "image",
                  source: { type: "base64", media_type: "image/png", data: "iVBORw0KGgo=" },
                },
                { type: "text", text: "what is this" },
              ],
            },
          ],
          max_tokens: 8,
        },
      ],
      upstream: { body: "boom", type: "text/plain", status: 500 },
    },
    {
      name: "POST /v1/messages, an IMAGE with VISION_MODEL naming a CUSTOM provider",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [
            {
              role: "user",
              content: [
                {
                  type: "image",
                  source: { type: "base64", media_type: "image/png", data: "iVBORw0KGgo=" },
                },
                { type: "text", text: "what is this" },
              ],
            },
          ],
          max_tokens: 8,
        },
      ],
      upstream: {
        body: JSON.stringify({
          choices: [{ message: { role: "assistant", content: "a picture" }, finish_reason: "stop" }],
        }),
        type: "application/json",
        status: 200,
      },
      // `VISION_MODEL` is operator-configurable and may name a CUSTOM provider's model — the reason the source
      // resolves the describe's route through `resolveRoute` rather than `pickRoute` ("pickRoute would not know
      // the prefix and would silently fall through to the DEFAULT channel").
      env: { VISION_MODEL: "acme/acme-vision" },
      kv: {
        "providers:custom": JSON.stringify([
          {
            prefix: "acme/",
            label: "Acme",
            baseURL: "https://acme.test/v1",
            api: "openai-completions",
            apiKey: "sk-acme",
            models: [{ id: "acme-vision", vision: true }],
          },
        ]),
      },
    },
    {
      name: "POST /v1/messages/count_tokens, with an IMAGE (the pass is skipped)",
      req: [
        "POST",
        "/v1/messages/count_tokens",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [
            {
              role: "user",
              content: [
                {
                  type: "image",
                  source: { type: "base64", media_type: "image/png", data: "iVBORw0KGgo=" },
                },
              ],
            },
          ],
        },
      ],
    },
    {
      name: "POST /v1/chat/completions, with an IMAGE (the pass is messages-only)",
      req: [
        "POST",
        "/v1/chat/completions",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [
            {
              role: "user",
              content: [
                { type: "image_url", image_url: { url: "data:image/png;base64,iVBORw0KGgo=" } },
              ],
            },
          ],
          max_tokens: 8,
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
    },
    {
      name: "POST /v1/messages, an IMAGE to a custom provider WITHOUT vision",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "acme/acme-text",
          messages: [
            {
              role: "user",
              content: [
                {
                  type: "image",
                  source: { type: "base64", media_type: "image/png", data: "iVBORw0KGgo=" },
                },
                { type: "text", text: "what is this" },
              ],
            },
          ],
          max_tokens: 8,
        },
      ],
      upstream: {
        body: JSON.stringify({
          choices: [{ message: { role: "assistant", content: "described" }, finish_reason: "stop" }],
        }),
        type: "application/json",
        status: 200,
      },
      kv: {
        "providers:custom": JSON.stringify([
          {
            prefix: "acme/",
            label: "Acme",
            baseURL: "https://acme.test/v1",
            api: "openai-completions",
            apiKey: "sk-acme",
            models: [{ id: "acme-text" }],
          },
        ]),
      },
    },
    {
      name: "GET /v1/models, with a CUSTOM provider in KV",
      req: ["GET", "/v1/models"],
      kv: {
        "providers:custom": JSON.stringify([
          {
            prefix: "acme/",
            label: "Acme",
            baseURL: "https://acme.test/v1",
            api: "openai-completions",
            apiKey: "sk-acme",
            models: [
              { id: "acme-chat", name: "Acme Chat", contextWindow: 128000, maxTokens: 8192, vision: true },
            ],
          },
        ]),
      },
    },
    {
      name: "POST /v1/messages, og with a [1m] bracket suffix",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash[1m]", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
    },
    {
      name: "POST /v1/chat/completions, a retired model",
      req: ["POST", "/v1/chat/completions", { model: "ds/deepseek-v4-flash", messages: [] }],
    },
    {
      name: "POST /v1/messages, with an Origin the deployment ALLOWS",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      headers: { origin: "https://console.test" },
      env: { CONSOLE_ORIGINS: "https://console.test" },
    },
    {
      name: "POST /v1/messages, with an Origin the deployment does NOT allow",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      headers: { origin: "https://evil.test" },
      env: { CONSOLE_ORIGINS: "https://console.test" },
    },
    {
      name: "POST /v1/messages, BOTH token spellings (x-api-key wins)",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      token: "both",
    },
    {
      // **THE THREE BODY CASES, AND THE ONE THAT MUST NOT BITE.** The source parses `rawText` in the POST arms
      // and lets a `JSON.parse` throw reach the front door's catch (500 "Internal error"); this worker used to
      // swallow the failure and answer the DEFAULT channel's `502 config_error — "CMD_API_KEY not configured"`,
      // a diagnosis pointing an operator at a channel they never asked for. `count_tokens` never parses, so it
      // is the exemption that proves the rule is scoped rather than blanket.
      name: "POST /v1/messages, a MALFORMED JSON body",
      req: ["POST", "/v1/messages", null],
      rawBody: "{not json",
    },
    {
      name: "POST /v1/messages, an EMPTY body",
      req: ["POST", "/v1/messages", null],
      rawBody: "",
    },
    {
      name: "POST /v1/messages/count_tokens with a MALFORMED body",
      req: ["POST", "/v1/messages/count_tokens", null],
      rawBody: "{not json",
    },
    {
      name: "POST /v1/messages, a JSON body that is an ARRAY",
      req: ["POST", "/v1/messages", null],
      rawBody: "[]",
    },
    {
      name: "POST /v1/responses, og/muse-spark-1.2-contributor with MUSE_RESPONSES_EXIT set",
      req: ["POST", "/v1/responses", { model: "og/muse-spark-1.2-contributor", input: "hi" }],
      upstream: {
        body: JSON.stringify({ id: "r1", output: [] }),
        type: "application/json",
        status: 200,
      },
      env: { MUSE_RESPONSES_EXIT: "https://exit.test" },
    },
    {
      name: "POST /v1/messages, settings:US_PROXY a truthy non-1 value",
      req: [
        "POST",
        "/v1/messages",
        { model: "og/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "settings:US_PROXY": "true" },
    },
    {
      name: "POST /v1/messages, ds (passthrough) with a MALFORMED body",
      req: ["POST", "/v1/messages", null],
      rawBody: "{not json",
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
    },
    {
      name: "POST /v1/messages, a STREAM whose body DIES mid-response",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
          stream: true,
        },
      ],
      upstream: {
        body: 'data: {"choices":[{"delta":{"content":"par"}}]}\n\n',
        type: "text/event-stream",
        status: 200,
        truncate: true,
      },
    },
    {
      name: "POST /v1/messages, a STREAM that ends WITHOUT [DONE]",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
          stream: true,
        },
      ],
      upstream: {
        body: 'data: {"choices":[{"delta":{"content":"hi"}}]}\n\n',
        type: "text/event-stream",
        status: 200,
      },
    },
    {
      name: "POST /v1/messages, an upstream SSE with a 200 but an EMPTY body",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
          stream: true,
        },
      ],
      upstream: { body: "", type: "text/event-stream", status: 200 },
    },
    {
      name: "POST /v1/messages, stream:true but the upstream answers JSON (the ignore path)",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
          stream: true,
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
    },
    {
      name: "POST /v1/messages/count_tokens with a TOOLS body",
      req: [
        "POST",
        "/v1/messages/count_tokens",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          tools: [
            {
              name: "read_file",
              description: "read a file",
              input_schema: { type: "object", properties: { path: { type: "string" } } },
            },
          ],
        },
      ],
    },
    {
      // **THE og/ SEARCH PATH.** A forced `web_search` makes the shipping route dial zen's own
      // `/v1/messages` — a PASSTHROUGH, because the server-side tool is not executed on the
      // chat/completions translation this route otherwise uses. Measured 2026-10-06: this worker used to dial
      // `/v1/chat/completions` and answer a reshaped Anthropic message, so web search was broken in a way no
      // status code shows.
      name: "POST /v1/messages, a body with web_search declared (the native search path)",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
          tools: [{ type: "web_search_20250305", name: "web_search" }],
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
    },
    {
      name: "POST /v1/messages, web_search with US_PROXY=1 (the swap keeps the exit)",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
          tools: [{ type: "web_search_20250305", name: "web_search" }],
          tool_choice: { type: "tool", name: "web_search" },
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      env: { US_PROXY: "1" },
    },
    {
      name: "POST /v1/messages, web_search on a TRANSLATE-only model (the forced swap)",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/mimo-v2.5",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
          tools: [{ type: "web_search_20250305", name: "web_search" }],
          tool_choice: { type: "tool", name: "web_search" },
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
    },
    {
      name: "POST /v1/messages, a SEARCH-ONLY body (one tool, no tool_choice)",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
          tools: [{ type: "web_search_20250305", name: "web_search" }],
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
    },
    {
      name: "POST /v1/messages, a custom provider whose baseURL carries a PATH",
      req: [
        "POST",
        "/v1/messages",
        { model: "acme/acme-chat", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: {
        "providers:custom": JSON.stringify([
          {
            prefix: "acme/",
            label: "Acme",
            baseURL: "https://acme.test/openai/v1",
            api: "openai-completions",
            apiKey: "sk-acme",
            models: [{ id: "acme-chat" }],
          },
        ]),
      },
    },
    {
      name: "POST /v1/messages, a custom provider with an UNSUPPORTED api (the error route)",
      req: [
        "POST",
        "/v1/messages",
        { model: "acme/acme-chat", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      kv: {
        "providers:custom": JSON.stringify([
          {
            prefix: "acme/",
            label: "Acme",
            baseURL: "https://acme.test/v1",
            api: "anthropic-messages",
            apiKey: "sk-acme",
            models: [{ id: "acme-chat" }],
          },
        ]),
      },
    },
    {
      name: "POST /v1/messages, a custom provider model spelled WITH the prefix twice",
      req: [
        "POST",
        "/v1/messages",
        { model: "acme/acme/acme-chat", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: {
        "providers:custom": JSON.stringify([
          {
            prefix: "acme/",
            label: "Acme",
            baseURL: "https://acme.test/v1",
            api: "openai-completions",
            apiKey: "sk-acme",
            models: [{ id: "acme-chat" }],
          },
        ]),
      },
    },
    {
      // **THE REMAINING BUILT-IN CHANNELS, WHICH THE SWEEP HAD NEVER TOUCHED.** Until this case set, only
      // og/ds/nv/cm and the custom providers were exercised end-to-end — the other five prefixes had no case
      // at all, and the `gmi/` one below is what found the nv/gmi translate branch.
      name: "POST /v1/messages, nv/ WITH a user key (the nv/gmi translate branch)",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "nv/nvidia/nemotron-3-ultra-550b-a55b",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ NVAPI_KEY: "sk-nv-user" }) },
    },
    {
      name: "POST /v1/messages, or/ (openrouter, BYOK)",
      req: [
        "POST",
        "/v1/messages",
        { model: "or/z-ai/glm-5.2:free", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ OPENROUTER_API_KEY: "sk-or-user" }) },
    },
    {
      name: "POST /v1/messages, qw/ (qwen, BYOK)",
      req: [
        "POST",
        "/v1/messages",
        { model: "qw/qwen3.8-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ QWEN_API_KEY: "sk-qw-user" }) },
    },
    {
      name: "POST /v1/messages, gmi/ (pure BYOK, the translate branch)",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "gmi/MiniMaxAI/MiniMax-M3",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ GMI_API_KEY: "sk-gmi-user" }) },
    },
    {
      name: "POST /v1/messages, gmi/ with NO user key (the key gate)",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "gmi/MiniMaxAI/MiniMax-M3",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
        },
      ],
      env: { GMI_API_KEY: "sk-gmi-env" },
    },
    {
      name: "POST /v1/messages, r4/ (anthropic-shape passthrough)",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "r4/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ R4_API_KEY: "sk-r4-user" }) },
    },
    {
      name: "POST /v1/messages, amd/ (anthropic-shape passthrough)",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "amd/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ AMD_API_KEY: "sk-amd-user" }) },
    },
    {
      name: "POST /v1/messages, cm/ (commandgoat, BYOK)",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "cm/deepseek/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ CMD_API_KEY: "sk-cm-user" }) },
    },
    {
      // **THIS WAS THE ONE KNOWN DIFFERENCE, AND IT IS FIXED** — `translate-vision.ts` is ported: the image is
      // described with `VISION_MODEL` (default `og/mimo-v2.5`) and the block is replaced by the description, so
      // both sides now make TWO upstream calls with the same bodies. It was carried here with a
      // `knownDifference` flag while the port was missing — declared rather than omitted, because leaving it out
      // would have made the number look better than it was — and the flag is gone because the case passes.
      name: "POST /v1/messages, og, an IMAGE content block",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "og/deepseek-v4.1-flash",
          messages: [
            {
              role: "user",
              content: [
                {
                  type: "image",
                  source: { type: "base64", media_type: "image/png", data: "iVBORw0KGgo=" },
                },
                { type: "text", text: "what is this" },
              ],
            },
          ],
          max_tokens: 8,
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
    },
    {
      name: "POST /v1/messages, a CUSTOM provider record with vision:true",
      req: [
        "POST",
        "/v1/messages",
        {
          model: "acme/acme-chat",
          messages: [
            {
              role: "user",
              content: [
                {
                  type: "image",
                  source: { type: "base64", media_type: "image/png", data: "iVBORw0KGgo=" },
                },
                { type: "text", text: "what is this" },
              ],
            },
          ],
          max_tokens: 8,
        },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: {
        "providers:custom": JSON.stringify([
          {
            prefix: "acme/",
            label: "Acme",
            baseURL: "https://acme.test/v1",
            api: "openai-completions",
            apiKey: "sk-acme",
            models: [{ id: "acme-chat", vision: true }],
          },
        ]),
      },
    },
    {
      name: "POST /v1/messages, a CUSTOM provider record",
      req: [
        "POST",
        "/v1/messages",
        { model: "acme/acme-chat", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: {
        "providers:custom": JSON.stringify([
          {
            prefix: "acme/",
            label: "Acme",
            baseURL: "https://acme.test/v1",
            api: "openai-completions",
            apiKey: "sk-acme",
            models: [{ id: "acme-chat" }],
          },
        ]),
      },
    },
  ];

  const buildRequest = (c, token = TOKEN) => {
    const [method, path, body] = c.req;
    const headers = { "content-type": "application/json", ...(c.headers ?? {}) };
    if (c.token !== null) headers["x-api-key"] = c.token ?? token;
    // `token: "both"` is not a token: it is the case that sends BOTH spellings, and the precedence between them
    // is what is being measured.
    if (c.token === "both") {
      headers["x-api-key"] = token;
      headers["authorization"] = "Bearer tok-other";
    }
    return new Request(`https://console.test${path}`, {
      method,
      headers,
      // `rawBody` is for the cases whose body is NOT JSON — the request has to carry the bytes as written.
      ...(c.rawBody ? { body: c.rawBody } : body ? { body: JSON.stringify(body) } : {}),
    });
  };

  const stubUpstream = (c, capture) => {
    if (!c.upstream) {
      globalThis.fetch = async () => {
        throw new Error("the sweep has no upstream for this case");
      };
      return;
    }
    globalThis.fetch = async (url, init = {}) => {
      const req = new Request(url, init);
      capture.push({
        url: req.url,
        method: req.method,
        auth: req.headers.get("authorization") ?? null,
        body: await req.text(),
      });
      if (c.upstream.throws) throw new Error("network down");
      if (c.upstream.hangs) {
        // **A HANGING STUB MUST STILL HONOR THE ABORT SIGNAL**, or it measures nothing: the first version of
        // this case returned a never-settling promise and ignored `init.signal`, so the SHIPPING side hung too
        // and the case reported "identical" for two sides that both never answered.
        return new Promise((_, reject) => {
          const signal = init.signal;
          if (signal && typeof signal.addEventListener === "function") {
            signal.addEventListener("abort", () =>
              reject(Object.assign(new Error("The operation was aborted"), { name: "AbortError" })),
            );
          }
        });
      }
      const bytes = new TextEncoder().encode(c.upstream.body);
      const stream = new ReadableStream({
        start(controller) {
          controller.enqueue(bytes);
          // `truncate` is a stream that DIES: the reader gets an error instead of a close, which is the
          // mid-response failure the SSE transform's `on_end(true)` branch exists for.
          if (c.upstream.truncate) controller.error(new Error("upstream stream died"));
          else controller.close();
        },
      });
      const headers = { "content-type": c.upstream.type };
      if (c.upstream.retryAfter) headers["retry-after"] = c.upstream.retryAfter;
      return new Response(stream, { status: c.upstream.status, headers });
    };
  };

  // **A HANGING UPSTREAM CANNOT BE WAITED OUT FOREVER**: this is how long the harness gives each side, so a
  // hang is a MEASUREMENT ("HUNG") rather than a hung harness.
  const withDeadline = async (work, ms) => {
    if (!ms) return work();
    return Promise.race([work(), new Promise((resolve) => setTimeout(() => resolve("HUNG"), ms))]);
  };

  const realFetch = globalThis.fetch;
  let sweepSame = 0;
  const sweepDifferent = [];
  const sweepKnown = [];
  for (const [i, c] of SWEEP.entries()) {
    // **A UNIQUE TOKEN PER CASE, AND THE REASON IS THE RATE LIMIT**: the shipping side's counters live in ONE
    // process (`__rlMin` is module state) while the wasm side gets a FRESH module instance per case (the
    // `?sweep=N` import) — so with a shared token the shipping side hit its 48/minute budget around case 49 and
    // reported two "divergences" that were the harness's own asymmetry. The rate limit has its own case, with
    // its own token and its own 50 requests.
    const token = `tok-sweep-${i}`;
    const shipCalls = [];
    const shipBreaker = new DurableObjectNamespace(c.breaker ?? "0", []);
    stubUpstream(c, shipCalls);
    __clearDegradedCache();
    // The KV cache is per-isolate module state with a TTL: without this, the case that ADDS a
    // `providers:custom` record reads the absence the cases before it cached.
    __clearCaches();
    let shipStatus = 0;
    let shipBody = "";
    let shipHeaders = [];
    try {
      const env = {
        KEYS: kvFor(c.kv, token),
        BREAKER: shipBreaker,
        DO_AUTH: "stub",
        CONSOLE_HOST: "console.test",
        CONSOLE_ORIGINS: "https://console.test",
        ...(c.env ?? {}),
      };
      const res = await withDeadline(() => shippingDoor.fetch(buildRequest(c, token), env, {}), c.deadlineMs);
      if (res === "HUNG") {
        shipStatus = -1;
        shipBody = "HUNG";
      } else {
        shipStatus = res.status;
        shipBody = await res.text();
        shipHeaders = [...res.headers.entries()].map(([k, v]) => `${k.toLowerCase()}: ${v}`).sort();
      }
    } catch (e) {
      shipBody = `THREW ${e}`;
    }
    globalThis.fetch = realFetch;

    const wasmCalls = [];
    const wasmBreaker = new DurableObjectNamespace(c.breaker ?? "0", []);
    stubUpstream(c, wasmCalls);
    const worker = await import(`${pathToFileURL(BUILT).href}?sweep=${i}`);
    const instance = new worker.default();
    instance.env = { KEYS: kvFor(c.kv, token), BREAKER: wasmBreaker, DO_AUTH: "stub", ...(c.env ?? {}) };
    instance.ctx = {};
    let wasmStatus = 0;
    let wasmBody = "";
    let wasmHeaders = [];
    try {
      const res = await withDeadline(() => instance.fetch(buildRequest(c, token)), c.deadlineMs);
      if (res === "HUNG") {
        wasmStatus = -1;
        wasmBody = "HUNG";
      } else {
        wasmStatus = res.status;
        wasmBody = await res.text();
        wasmHeaders = [...res.headers.entries()].map(([k, v]) => `${k.toLowerCase()}: ${v}`).sort();
      }
    } catch (e) {
      wasmBody = `THREW ${e}`;
    }
    globalThis.fetch = realFetch;

    const notes = [];
    if (shipStatus !== wasmStatus) notes.push(`status ${shipStatus} vs ${wasmStatus}`);
    if (shipBody !== wasmBody) {
      notes.push(
        `body ${shipBody.length}B vs ${wasmBody.length}B :: ship ${JSON.stringify(shipBody.slice(0, 70))} :: wasm ${JSON.stringify(wasmBody.slice(0, 70))}`,
      );
    }
    if (JSON.stringify(shipHeaders) !== JSON.stringify(wasmHeaders)) notes.push("headers differ");
    if (JSON.stringify(shipCalls) !== JSON.stringify(wasmCalls)) {
      notes.push(`upstream calls ${shipCalls.length} vs ${wasmCalls.length}`);
    }
    if (JSON.stringify(shipBreaker.calls) !== JSON.stringify(wasmBreaker.calls)) {
      notes.push(
        `breaker ${JSON.stringify(shipBreaker.calls)} vs ${JSON.stringify(wasmBreaker.calls)}`,
      );
    }
    if (notes.length === 0) {
      sweepSame++;
      if (c.knownDifference) {
        console.log(`      FIXED ${c.name} — remove it from the known-difference list`);
      }
    } else if (c.knownDifference) {
      // **A KNOWN DIFFERENCE IS COUNTED SEPARATELY, AND IT STAYS VISIBLE.** It is not a pass (the number says
      // how many cases match, and this is not one) and it is not a failure of `main` (the port it waits for is
      // named work, not a regression).
      sweepKnown.push({ name: c.name, why: c.knownDifference });
    } else {
      sweepDifferent.push({ name: c.name, notes });
    }
  }

  // ── and the per-token rate limit, which needs its own budget rather than one request ───────────
  {
    const rlToken = "tok-sweep-rate";
    const rlKv = () => {
      const k = kvFor();
      return k;
    };
    const rlRequest = () =>
      new Request("https://console.test/v1/messages", {
        method: "POST",
        headers: { "x-api-key": rlToken, "content-type": "application/json" },
        body: JSON.stringify({
          model: "og/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
          max_tokens: 8,
        }),
      });
    const seed = async (keys) => {
      await keys.put(`token:${rlToken}`, UID);
      await keys.put(`user:${UID}`, JSON.stringify({ id: UID, enabled: true }));
      await keys.put(`ukeys:${UID}`, JSON.stringify({ OPENCODE_GO_API_KEY: "sk-user-og" }));
    };
    let shipFirst429 = null;
    {
      const keys = rlKv();
      await seed(keys);
      __clearCaches();
      const env = { KEYS: keys, BREAKER: new DurableObjectNamespace("0", []), DO_AUTH: "stub" };
      for (let n = 1; n <= 50; n++) {
        stubUpstream({ upstream: { body: JSON_OK, type: "application/json", status: 200 } }, []);
        const res = await shippingDoor.fetch(rlRequest(), env, {});
        if (res.status === 429 && shipFirst429 === null) shipFirst429 = n;
        await res.text();
      }
      globalThis.fetch = realFetch;
    }
    let wasmFirst429 = null;
    {
      const keys = rlKv();
      await seed(keys);
      const worker = await import(`${pathToFileURL(BUILT).href}?sweep=rate`);
      const instance = new worker.default();
      instance.env = { KEYS: keys, BREAKER: new DurableObjectNamespace("0", []), DO_AUTH: "stub" };
      instance.ctx = {};
      for (let n = 1; n <= 50; n++) {
        stubUpstream({ upstream: { body: JSON_OK, type: "application/json", status: 200 } }, []);
        const res = await instance.fetch(rlRequest());
        if (res.status === 429 && wasmFirst429 === null) wasmFirst429 = n;
        await res.text();
      }
      globalThis.fetch = realFetch;
    }
    if (shipFirst429 === wasmFirst429 && shipFirst429 !== null) {
      sweepSame++;
    } else {
      sweepDifferent.push({
        name: "the per-token rate limit (50 POSTs in one minute)",
        notes: [`first 429 at ${shipFirst429} vs ${wasmFirst429}`],
      });
    }
  }

  const total = SWEEP.length + 1;
  console.log(
    `  the divergence sweep, shipping front door against the built worker: ${sweepSame}/${total} identical, ${sweepKnown.length} known difference(s)`,
  );
  for (const d of sweepDifferent) {
    console.log(`      FAIL ${d.name}`);
    for (const n of d.notes) console.log(`           ${n}`);
  }
  for (const k of sweepKnown) {
    console.log(`      KNOWN ${k.name} — ${k.why}`);
  }
  bad += sweepDifferent.length;
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
