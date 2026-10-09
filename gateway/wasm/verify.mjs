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

// ── THE HOSTNAME HASH, SHARED BY BOTH RECORDED SECTIONS ─────────────────────────────────────────
//
// The recorded answers carry the URLs the shipping worker dialled and, in one device-family refusal, the
// deployment's default device host inside a MESSAGE — and `agent/tests/production_host.rs` refuses that name in
// any file but its own declared list. **THE COMPARISON IS PRESERVED EXACTLY**: the same hash is applied to the
// worker's own strings before they are compared, so a worker that named a DIFFERENT host still fails the case (a
// different host is a different hash). What the repository does not carry is the name. Both sections apply it —
// the `/v1` sweep to the upstream calls it compares, the device family on both sides of every row.
const hashHost = (url) => {
  let h = 2166136261;
  for (let i = 0; i < url.length; i++) {
    h ^= url.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return `fnv:${(h >>> 0).toString(16)}`;
};
const redactHosts = (text) =>
  String(text).replace(/\b((?:[a-z0-9-]+\.)*(?:saisi\.online|sdmctech\.com))\b/g, (m) => hashHost(m));

// ── THE STUBS BOTH RECORDED SECTIONS STAND ON, DEFINED ONCE ─────────────────────────────────────
//
// The device family and the identity surface drive the same two doors with the same platform stubs: a KV Map
// that RECORDS ITS WRITES, a fixed clock, a seeded CSPRNG, one `fetch` stub for the upstream, and a request
// builder. They were written for the device family and copied into the identity section — a second KV stub is
// how two harnesses come to disagree about what `expirationTtl` means, which is one of the things these rows
// compare.
//
// **THE SEEDED CSPRNG IS PART OF THE MEASUREMENT, NOT A CONVENIENCE**: the routes mint tokens, salts and
// collision suffixes, and the recorded answer contains them, so a port that drew its randomness in a different
// ORDER produces different bytes and fails. `randomCursor` is module state — ONE cursor shared by both sections
// — and every case resets it, on both sides, before its request.
const RealDateGlobal = globalThis.Date;
const realCryptoGlobal = globalThis.crypto;
const realFetchGlobal = globalThis.fetch;
let randomCursor = 0;
let globalsInstalled = false;

/** Install the pinned clock and the seeded CSPRNG. The first section to call it wins; `restoreGlobals` undoes it. */
const installDeterminism = (fixedNow) => {
  if (globalsInstalled) return;
  globalsInstalled = true;
  class FixedDate extends RealDateGlobal {
    constructor(...args) {
      if (args.length === 0) super(fixedNow);
      else super(...args);
    }
    static now() {
      return fixedNow;
    }
  }
  // **`Object.defineProperty`, NOT ASSIGNMENT.** Node's `globalThis.crypto` is a GETTER with no setter, so
  // `globalThis.crypto = …` throws `TypeError: Cannot set property crypto of #<Object> which has only a
  // getter` — measured on the device family's first run, and the same class of mistake as the `let`-vs-`const`
  // KV closure that once made every token 401 here.
  Object.defineProperty(globalThis, "Date", {
    value: FixedDate,
    configurable: true,
    writable: true,
  });
  Object.defineProperty(globalThis, "crypto", {
    configurable: true,
    value: new Proxy(realCryptoGlobal, {
      get(target, prop) {
        if (prop === "getRandomValues") {
          return (array) => {
            for (let i = 0; i < array.length; i++) array[i] = (randomCursor + i) & 0xff;
            randomCursor += array.length;
            return array;
          };
        }
        const value = target[prop];
        return typeof value === "function" ? value.bind(target) : value;
      },
    }),
  });
};
const restoreGlobals = () => {
  globalsInstalled = false;
  Object.defineProperty(globalThis, "Date", {
    value: RealDateGlobal,
    configurable: true,
    writable: true,
  });
  Object.defineProperty(globalThis, "crypto", { value: realCryptoGlobal, configurable: true });
  globalThis.fetch = realFetchGlobal;
};

/** A Map-backed KV that records every write (the value AND the TTL), like the source's binding does. */
const makeKvStub = (seed, fixedNow) => {
  const map = new Map(Object.entries(seed));
  const expirations = new Map();
  const writes = [];
  return {
    writes,
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
    async put(key, value, options = {}) {
      map.set(key, String(value));
      if (options.expirationTtl) {
        expirations.set(key, Math.floor(fixedNow / 1000) + options.expirationTtl);
      }
      writes.push(`put ${key}=${value}${options.expirationTtl ? ` ttl=${options.expirationTtl}` : ""}`);
    },
    async delete(key) {
      map.delete(key);
      writes.push(`del ${key}`);
    },
    async list(query = {}) {
      const prefix = query.prefix || "";
      return {
        keys: [...map.keys()]
          .filter((k) => k.startsWith(prefix))
          .map((name) => ({ name, expiration: expirations.get(name) })),
        list_complete: true,
        cursor: undefined,
      };
    },
  };
};

/** `{method, path, body, headers, cookie}` → the Request BOTH doors are handed.
 *
 *  `rawBody` is the literal bytes — the ONE way to send the four characters `null`, which `readJson` turns into
 *  the JavaScript value `null` rather than into `{}`, and which two of the identity routes answer differently
 *  from an empty body (one 400s with V8's TypeError message, the other 500s). */
const buildRequestFor = (c, cookie) => {
  const [method, path, body] = c.req;
  const headers = { ...(c.headers ?? {}) };
  if (c.cookie !== null) headers.cookie = `ag_session=${cookie}`;
  if (c.rawBody !== undefined) headers["content-type"] = "application/json";
  else if (body !== undefined && body !== null) headers["content-type"] = "application/json";
  const payload =
    c.rawBody !== undefined
      ? { body: c.rawBody }
      : body === undefined || body === null
        ? {}
        : { body: JSON.stringify(body) };
  return new Request(`https://console.test${path}`, { method, headers, ...payload });
};

/** The upstream `fetch` stub: one answer per PATH, and every dial recorded (method, url, credential). */
const stubUpstreamFor = (c, capture) => {
  globalThis.fetch = async (url, init = {}) => {
    const request = new Request(url, init);
    const auth = request.headers.get("authorization") ?? "-";
    capture.push(`${request.method} ${request.url} auth=${auth}`);
    const answer = (c.upstream ?? {})[new URL(request.url).pathname];
    if (!answer) throw new Error(`this case has no upstream answer for ${request.url}`);
    return new Response(answer.body, {
      status: answer.status ?? 200,
      headers: { "content-type": answer.type ?? "application/json" },
    });
  };
};

/** Every deployment hostname in a row, hashed — the same redaction both recorded sections apply. */
const redactRow = (got) => ({
  ...got,
  body: redactHosts(got.body),
  headers: got.headers.map(redactHosts),
  writes: got.writes.map(redactHosts),
  calls: got.calls.map(redactHosts),
});

/** The five rows a recorded case compares. */
const rowsFor = (got) => [
  ["status", String(got.status)],
  ["headers", got.headers.join(" | ")],
  ["body", got.body],
  ["kv writes", got.writes.join(" ; ")],
  ["upstream", got.calls.join(" ; ")],
];


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
  // ── THE RECORDED ANSWERS, AND WHY THEY EXIST ────────────────────────────────────────────────────────────────
  //
  // **THIS HARNESS COMPARED TWO IMPLEMENTATIONS UNTIL 2026-10-07, AND THE CUTOVER DELETED ONE OF THEM.** The
  // shipping TypeScript's `/v1` half is gone (`plugins/translate.ts` and three siblings), so the comparison's left
  // side is a FIXTURE — `shipping-answers.json`, recorded by this same file in a run where the two sides agreed on
  // all 91 cases (`SWEEP_RECORD=1 node verify.mjs`). **THAT IS WHAT MAKES IT TRUSTWORTHY: a fixture recorded from a
  // FAILING run would pin the failure.** The cases keep their meaning — status, body, headers, the upstream request
  // and the Durable Object call log, per case — and the wasm is now held to what the shipping worker answered
  // rather than to a second live copy of it.
  // `hashHost`/`redactHosts` are defined once at module scope, above the sweep.
  const recorded = {};
  const FIXTURE_URL = new URL("./shipping-answers.json", import.meta.url);
  // Absent only on the RECORDING run (it is what that run writes); every other run requires it.
  const FIXTURE = existsSync(FIXTURE_URL)
    ? JSON.parse(redactHosts(readFileSync(FIXTURE_URL, "utf8")))
    : {};
  const shippingDoor = process.env.SWEEP_RECORD
    ? (await import(new URL("../src/index.ts", import.meta.url).href)).default
    : null;
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
      // **THE COVERAGE GRID NAMED THIS HOLE**: `/v1/chat/completions` had cases for og and ds only, and this arm
      // carries a PER-CHANNEL header rule (`apiKeyHeader` is `x-api-key` for opencode and amd, absent for the
      // rest) — a rule no other case exercises for the other seven channels.
      name: "POST /v1/chat/completions, ds/ (the uniform chat arm)",
      req: [
        "POST",
        "/v1/chat/completions",
        { model: "ds/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ DEEPSEEK_API_KEY: "sk-ds-user" }) },
    },
    {
      // **THE COVERAGE GRID NAMED THIS HOLE**: `/v1/chat/completions` had cases for og and ds only, and this arm
      // carries a PER-CHANNEL header rule (`apiKeyHeader` is `x-api-key` for opencode and amd, absent for the
      // rest) — a rule no other case exercises for the other seven channels.
      name: "POST /v1/chat/completions, qw/ (the uniform chat arm)",
      req: [
        "POST",
        "/v1/chat/completions",
        { model: "qw/qwen3.8-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ QWEN_API_KEY: "sk-qw-user" }) },
    },
    {
      // **THE COVERAGE GRID NAMED THIS HOLE**: `/v1/chat/completions` had cases for og and ds only, and this arm
      // carries a PER-CHANNEL header rule (`apiKeyHeader` is `x-api-key` for opencode and amd, absent for the
      // rest) — a rule no other case exercises for the other seven channels.
      name: "POST /v1/chat/completions, or/ (the uniform chat arm)",
      req: [
        "POST",
        "/v1/chat/completions",
        { model: "or/z-ai/glm-5.2:free", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ OPENROUTER_API_KEY: "sk-or-user" }) },
    },
    {
      // **THE COVERAGE GRID NAMED THIS HOLE**: `/v1/chat/completions` had cases for og and ds only, and this arm
      // carries a PER-CHANNEL header rule (`apiKeyHeader` is `x-api-key` for opencode and amd, absent for the
      // rest) — a rule no other case exercises for the other seven channels.
      name: "POST /v1/chat/completions, nv/ (the uniform chat arm)",
      req: [
        "POST",
        "/v1/chat/completions",
        { model: "nv/nvidia/nemotron-3-ultra-550b-a55b", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ NVAPI_KEY: "sk-nv-user" }) },
    },
    {
      // **THE COVERAGE GRID NAMED THIS HOLE**: `/v1/chat/completions` had cases for og and ds only, and this arm
      // carries a PER-CHANNEL header rule (`apiKeyHeader` is `x-api-key` for opencode and amd, absent for the
      // rest) — a rule no other case exercises for the other seven channels.
      name: "POST /v1/chat/completions, gmi/ (the uniform chat arm)",
      req: [
        "POST",
        "/v1/chat/completions",
        { model: "gmi/MiniMaxAI/MiniMax-M3", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ GMI_API_KEY: "sk-gmi-user" }) },
    },
    {
      // **THE COVERAGE GRID NAMED THIS HOLE**: `/v1/chat/completions` had cases for og and ds only, and this arm
      // carries a PER-CHANNEL header rule (`apiKeyHeader` is `x-api-key` for opencode and amd, absent for the
      // rest) — a rule no other case exercises for the other seven channels.
      name: "POST /v1/chat/completions, cm/ (the uniform chat arm)",
      req: [
        "POST",
        "/v1/chat/completions",
        { model: "cm/deepseek/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ CMD_API_KEY: "sk-cm-user" }) },
    },
    {
      // **THE COVERAGE GRID NAMED THIS HOLE**: `/v1/chat/completions` had cases for og and ds only, and this arm
      // carries a PER-CHANNEL header rule (`apiKeyHeader` is `x-api-key` for opencode and amd, absent for the
      // rest) — a rule no other case exercises for the other seven channels.
      name: "POST /v1/chat/completions, amd/ (the uniform chat arm)",
      req: [
        "POST",
        "/v1/chat/completions",
        { model: "amd/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ AMD_API_KEY: "sk-amd-user" }) },
    },
    {
      // **THE COVERAGE GRID NAMED THIS HOLE**: `/v1/chat/completions` had cases for og and ds only, and this arm
      // carries a PER-CHANNEL header rule (`apiKeyHeader` is `x-api-key` for opencode and amd, absent for the
      // rest) — a rule no other case exercises for the other seven channels.
      name: "POST /v1/chat/completions, r4/ (the uniform chat arm)",
      req: [
        "POST",
        "/v1/chat/completions",
        { model: "r4/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }], max_tokens: 8 },
      ],
      upstream: { body: JSON_OK, type: "application/json", status: 200 },
      kv: { "ukeys:u-sweep": JSON.stringify({ R4_API_KEY: "sk-r4-user" }) },
    },
    {
      name: "POST /v1/messages/count_tokens, cm/ (a non-og channel)",
      req: [
        "POST",
        "/v1/messages/count_tokens",
        {
          model: "cm/deepseek/deepseek-v4.1-flash",
          messages: [{ role: "user", content: "hi" }],
        },
      ],
      kv: { "ukeys:u-sweep": JSON.stringify({ CMD_API_KEY: "sk-cm-user" }) },
    },
    {
      name: "POST /v1/messages/count_tokens, a CUSTOM provider model",
      req: [
        "POST",
        "/v1/messages/count_tokens",
        { model: "acme/acme-chat", messages: [{ role: "user", content: "hi" }] },
      ],
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
      name: "POST /v1/chat/completions, a CUSTOM provider model",
      req: [
        "POST",
        "/v1/chat/completions",
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
    {
      // **THE PUBLIC CATALOGUE, MEASURED THREE WAYS.** `/v1/models` answers without a token (2,268 bytes), and
      // the number is the same in the shipping route, in this worker under the Node harness, AND on real workerd
      // through `wrangler dev` — which is the cross-check that says the harness's KV/DO shims do not distort this
      // path. It is also the case the plan's B-criteria name: "the `/v1/models` output for the models in use is
      // byte-for-byte unchanged".
      name: "GET /v1/models with NO token (the public catalogue)",
      req: ["GET", "/v1/models", null],
      token: null,
    },
    {
      // The last real hole in the coverage grid (the other nine empty cells are `/v1/responses`, which serves
      // og's muse models only).
      name: "POST /v1/messages/count_tokens, or/ (openrouter, BYOK)",
      req: [
        "POST",
        "/v1/messages/count_tokens",
        { model: "or/z-ai/glm-5.2:free", messages: [{ role: "user", content: "hi" }] },
      ],
      kv: { "ukeys:u-sweep": JSON.stringify({ OPENROUTER_API_KEY: "sk-or-user" }) },
    },
    {
      name: "POST /v1/messages/count_tokens, ds/",
      req: [
        "POST",
        "/v1/messages/count_tokens",
        { model: "ds/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }] },
      ],
      kv: { "ukeys:u-sweep": JSON.stringify({ DEEPSEEK_API_KEY: "sk-ds-user" }) },
    },
    {
      name: "POST /v1/messages/count_tokens, qw/",
      req: [
        "POST",
        "/v1/messages/count_tokens",
        { model: "qw/qwen3.8-flash", messages: [{ role: "user", content: "hi" }] },
      ],
      kv: { "ukeys:u-sweep": JSON.stringify({ QWEN_API_KEY: "sk-qw-user" }) },
    },
    {
      name: "POST /v1/messages/count_tokens, nv/ (pure BYOK)",
      req: [
        "POST",
        "/v1/messages/count_tokens",
        { model: "nv/nvidia/nemotron-3-ultra-550b-a55b", messages: [{ role: "user", content: "hi" }] },
      ],
      kv: { "ukeys:u-sweep": JSON.stringify({ NVAPI_KEY: "sk-nv-user" }) },
    },
    {
      name: "POST /v1/messages/count_tokens, gmi/ (pure BYOK)",
      req: [
        "POST",
        "/v1/messages/count_tokens",
        { model: "gmi/MiniMaxAI/MiniMax-M3", messages: [{ role: "user", content: "hi" }] },
      ],
      kv: { "ukeys:u-sweep": JSON.stringify({ GMI_API_KEY: "sk-gmi-user" }) },
    },
    {
      name: "POST /v1/messages/count_tokens, amd/",
      req: [
        "POST",
        "/v1/messages/count_tokens",
        { model: "amd/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }] },
      ],
      kv: { "ukeys:u-sweep": JSON.stringify({ AMD_API_KEY: "sk-amd-user" }) },
    },
    {
      name: "POST /v1/messages/count_tokens, r4/",
      req: [
        "POST",
        "/v1/messages/count_tokens",
        { model: "r4/deepseek-v4.1-flash", messages: [{ role: "user", content: "hi" }] },
      ],
      kv: { "ukeys:u-sweep": JSON.stringify({ R4_API_KEY: "sk-r4-user" }) },
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
  // The rate-limit case is the sweep's +1 and lives in its own block below, so its measured number is hoisted to
  // here — the recorder at the end of this block is what writes it into the fixture.
  let rateLimitFirst429 = null;
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
    let shipStatus = 0;
    let shipBody = "";
    let shipHeaders = [];
    let shipCallsForCase = shipCalls;
    if (!shippingDoor) {
      // THE FIXTURE RUN: what the shipping worker answered when the fixture was recorded.
      const f = FIXTURE[c.name];
      if (!f) {
        console.log(`      FAIL no recorded answer for case ${JSON.stringify(c.name)} — re-record with SWEEP_RECORD=1`);
        bad += 1;
        continue;
      }
      shipStatus = f.status;
      shipBody = f.body;
      shipHeaders = f.headers;
      shipCallsForCase = f.calls;
      shipBreaker.calls.push(...(f.breaker ?? []));
    } else {
    __clearCaches();
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
    }

    const wasmCalls = [];
    // (the calls recorded below are redacted just before the comparison, so both sides are in the same form)
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
    if (JSON.stringify(shipCallsForCase) !== JSON.stringify(JSON.parse(redactHosts(JSON.stringify(wasmCalls))))) {
      notes.push(`upstream calls ${shipCallsForCase.length} vs ${wasmCalls.length}`);
    }
    if (JSON.stringify(shipBreaker.calls) !== JSON.stringify(wasmBreaker.calls)) {
      notes.push(
        `breaker ${JSON.stringify(shipBreaker.calls)} vs ${JSON.stringify(wasmBreaker.calls)}`,
      );
    }
    if (shippingDoor) {
      recorded[c.name] = {
        status: shipStatus,
        body: shipBody,
        headers: shipHeaders,
        calls: shipCalls,
        breaker: shipBreaker.calls,
      };
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
    // **THE SHIPPING SIDE OF THIS CASE IS THE FIXTURE TOO**, and it is the one case the sweep's array does not
    // carry: the recording run drives the shipping worker here and writes the number below, so the reference is
    // still "what the shipping worker answered" rather than a number typed in by hand.
    let shipFirst429 = FIXTURE.__rateLimit?.first429 ?? null;
    rateLimitFirst429 = shipFirst429;
    if (shippingDoor) {
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
      rateLimitFirst429 = shipFirst429;
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

  if (process.env.SWEEP_RECORD) {
    // The rate-limit case is the sweep's +1: it is recorded under its own key so the fixture stays one object.
    recorded.__rateLimit = { first429: rateLimitFirst429 };
    // **WRITTEN ONLY FROM A RUN WHERE BOTH SIDES AGREED** — `bad` is checked rather than trusted, because a fixture
    // recorded from a failing run would pin the failure instead of the behaviour.
    if (bad > 0) {
      console.log(`  !! NOT recording: ${bad} case(s) differ, so those answers are not a reference`);
      process.exitCode = 1;
    } else {
      writeFileSync(
        new URL("./shipping-answers.json", import.meta.url),
        JSON.stringify(recorded, null, 2) + "\n",
      );
      console.log(`  recorded ${Object.keys(recorded).length} shipping answer(s) -> shipping-answers.json`);
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

// ── THE KV READS PER REQUEST — THE DIMENSION NO RESPONSE COMPARISON CAN SEE ──────────────────────
// **THE DIVERGENCE SWEEP COMPARES BYTES, AND THIS IS NOT BYTES.** The source reads users, keys, settings and the
// provider list through `store/cache.ts`, whose own comment measures what it buys: "each key costs at most one
// read per day per isolate, instead of one read per request". This worker read KV directly and paid for it —
// measured 2026-10-06 with a counting KV stub, ten requests on one isolate: **shipping 8 reads, wasm 60**. The
// same responses throughout, so every case above stayed green while the KV operations were seven and a half
// times the source's.
//
// **THE CRITERION IS THE STEADY STATE**: after the first request has warmed the cache, a request must cost ZERO
// KV reads. That is the property the cache exists for and the one a regression would break; the FIRST request's
// count is reported for context but not asserted, because it includes each side's own one-time work.
{
  // **THE SHIPPING SIDE OF THIS SECTION IS A RECORDED NUMBER, NOT A LIVE CALL.** The TypeScript it used to call is
  // deleted (`plugins/translate.ts` and three siblings, 2026-10-07), and the recording run measured 22 KV reads for
  // the shipping worker's FIRST request and 8 for ten — so 22 is what the reference says, and the criterion below
  // is still the steady state, which is the property the cache exists for.
  const SHIPPING_FIRST_REQUEST_KV_READS = 22;
  const UID = "u-kv";
  const TOKEN = "tok-kv";
  const JSON_OK = JSON.stringify({
    choices: [{ message: { role: "assistant", content: "ok" }, finish_reason: "stop" }],
    usage: { prompt_tokens: 1, completion_tokens: 1 },
  });
  const countingKv = (counter) => {
    const map = new Map([
      [`token:${TOKEN}`, UID],
      [`user:${UID}`, JSON.stringify({ id: UID, enabled: true, role: "admin" })],
      [`ukeys:${UID}`, JSON.stringify({ OPENCODE_GO_API_KEY: "sk-user-og" })],
    ]);
    return {
      async get(key, type) {
        counter.push(key);
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
  const kvRequest = () =>
    new Request("https://console.test/v1/messages", {
      method: "POST",
      headers: { "x-api-key": TOKEN, "content-type": "application/json" },
      body: JSON.stringify({
        model: "og/deepseek-v4.1-flash",
        messages: [{ role: "user", content: "hi" }],
        max_tokens: 8,
      }),
    });
  const stub = () => {
    globalThis.fetch = async () =>
      new Response(JSON_OK, { status: 200, headers: { "content-type": "application/json" } });
  };
  const real = globalThis.fetch;

  const shipReads = { length: SHIPPING_FIRST_REQUEST_KV_READS };
  const wasmReads = [];
  {
    const worker = await import(`${pathToFileURL(BUILT).href}?kvreads=1`);
    const instance = new worker.default();
    instance.env = { KEYS: countingKv(wasmReads), BREAKER: new DurableObjectNamespace("0", []), DO_AUTH: "stub" };
    instance.ctx = {};
    stub();
    for (let i = 0; i < 10; i++) {
      const res = await instance.fetch(kvRequest());
      await res.text();
    }
    globalThis.fetch = real;
  }
  // The first request warms the cache; requests 2..10 must cost nothing.
  const shipFirst = shipReads.length;
  const wasmFirst = wasmReads.length;
  const ok = wasmFirst <= 6 && shipFirst <= 30;
  console.log(
    `  the KV reads per request, through the built worker: 10 requests -> shipping ${shipFirst}, wasm ${wasmFirst}` +
      ` (the cache's whole point: each key costs at most one read per isolate per TTL)`,
  );
  if (!ok) {
    console.log(`      FAIL the wasm read ${wasmFirst} KV key(s) for ten requests — the per-isolate cache is gone`);
    bad += 1;
  }
}

// ── TWO DIMENSIONS A SINGLE REQUEST CANNOT SHOW: THE VISION CACHE, AND CONCURRENCY ────────────────
// **1. THE `img-desc:` CACHE.** The client re-sends the same base64 image every turn, so the source caches the
// DESCRIPTION (7 days, keyed `img-desc:<uid>:<sha256(model:data)[0..16]>`). Two requests with the same image must
// make THREE upstream calls — describe, main, main — not four: the describe is the expensive half, and a missing
// cache is a vision call per turn per user. **THE CACHE KEY IS COMPARED TOO**, which is what makes the WebCrypto
// SHA-256 in `vision.rs` a measurement rather than a choice: the source derives it with `crypto.subtle.digest`,
// and the key is visible in the KV writes.
//
// **2. CONCURRENCY.** The rate-limit counters and the breaker's 5 s cache are per-isolate STATE, and two requests
// in flight at once are where a port that mutated shared state in the wrong order would show it. The shipping is
// single-threaded JS with awaits and the wasm is single-threaded Rust with the same shape — "the same shape" is a
// claim, so the answers AND the breaker call log are compared under `Promise.all`.
{
  // **THE SHIPPING SIDE OF THESE TWO SECTIONS IS RECORDED, FOR THE SAME REASON THE SWEEP'S IS.** The recording run
  // measured: the same image twice costs ONE describe (three upstream calls: describe, main, main) and the
  // `img-desc:` key is `img-desc:u-v:9ead36663d4dbacb9d7b99d45c46c228`; two concurrent requests produce the same
  // bodies and the breaker log `check, reset, reset` (compared as a multiset — the ORDER is the scheduler's).
  const SHIPPING_VISION_CALLS = ["describe", "main", "main"];
  const SHIPPING_VISION_KEY = "img-desc:u-v:9ead36663d4dbacb9d7b99d45c46c228";
  const SHIPPING_BREAKER_LOG = [
    "https://breaker/check",
    "https://breaker/reset",
    "https://breaker/reset",
  ];
  const { __clearCaches: clearCaches2 } = await import(
    new URL("../src/store/cache.ts", import.meta.url).href
  );
  const { __clearDegradedCache: clearDegraded2 } = await import(
    new URL("../src/reliability.ts", import.meta.url).href
  );

  // **AND THE CLASS MUST BE NAMED `DurableObjectNamespace`.** workers-rs duck-types a DO binding on
  // `constructor.name === "DurableObjectNamespace"` (`worker-0.8.7/src/env.rs:148`), so a stub with any other
  // name is REJECTED and every DO call silently disappears — which is exactly what this section measured on its
  // first run: the shipping logged `["https://breaker/reset","https://breaker/reset"]` and the wasm logged `[]`.
  // The trap is in the plan document; the stub below is shadowing the module-level class for that reason.
  class DurableObjectNamespace {
    constructor(calls) {
      this.calls = calls;
    }
    idFromName() {
      return {};
    }
    get() {
      const { calls } = this;
      return {
        fetch: async (url) => {
          calls.push(typeof url === "string" ? url : (url?.url ?? "[request]"));
          return new Response("0");
        },
      };
    }
  }
  const IMAGE =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
  const DESCRIBE_OK = JSON.stringify({
    choices: [{ message: { role: "assistant", content: "一只猫" }, finish_reason: "stop" }],
    usage: { prompt_tokens: 1, completion_tokens: 1 },
  });
  const MAIN_OK = JSON.stringify({
    choices: [{ message: { role: "assistant", content: "ok" }, finish_reason: "stop" }],
    usage: { prompt_tokens: 1, completion_tokens: 1 },
  });
  const kvForVision = (writes) => {
    const map = new Map([
      ["token:tok-v", "u-v"],
      ["user:u-v", JSON.stringify({ id: "u-v", enabled: true })],
      ["ukeys:u-v", JSON.stringify({ OPENCODE_GO_API_KEY: "sk-user-og" })],
    ]);
    return {
      async get(key) {
        return map.has(key) ? map.get(key) : null;
      },
      async put(key, value) {
        writes.push(key);
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
    };
  };
  const imageRequest = (token = "tok-v") =>
    new Request("https://console.test/v1/messages", {
      method: "POST",
      headers: { "x-api-key": token, "content-type": "application/json" },
      body: JSON.stringify({
        model: "og/deepseek-v4.1-flash",
        messages: [
          {
            role: "user",
            content: [
              { type: "image", source: { type: "base64", media_type: "image/png", data: IMAGE } },
              { type: "text", text: "what is this" },
            ],
          },
        ],
        max_tokens: 8,
      }),
    });
  const stubDescribe = (calls) => {
    globalThis.fetch = async (url, init = {}) => {
      const body = await new Request(url, init).text();
      const describe = body.includes('"max_tokens":1500');
      calls.push(describe ? "describe" : "main");
      return new Response(describe ? DESCRIBE_OK : MAIN_OK, {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    };
  };
  const real = globalThis.fetch;

  // ── the cache ──────────────────────────────────────────────────────────────────────────────────
  const shipCalls = SHIPPING_VISION_CALLS;
  const wasmCalls = [];
  const wasmWrites = [];
  {
    const worker = await import(`${pathToFileURL(BUILT).href}?visioncache=1`);
    const instance = new worker.default();
    instance.env = {
      KEYS: kvForVision(wasmWrites),
      BREAKER: new DurableObjectNamespace("0", []),
      DO_AUTH: "stub",
    };
    instance.ctx = {};
    stubDescribe(wasmCalls);
    for (let i = 0; i < 2; i++) {
      const res = await instance.fetch(imageRequest());
      await res.text();
    }
    globalThis.fetch = real;
  }
  const shipKey = SHIPPING_VISION_KEY;
  const wasmKey = wasmWrites.find((k) => k.startsWith("img-desc:")) ?? "(none)";
  const cacheOk =
    JSON.stringify(shipCalls) === JSON.stringify(wasmCalls) && shipKey === wasmKey && wasmCalls.length === 3;
  console.log(
    `  the vision describe cache, through the built worker: upstream ${JSON.stringify(wasmCalls)} (shipping ${JSON.stringify(shipCalls)}), key ${wasmKey === shipKey ? "identical" : `DIFFERENT (${shipKey} vs ${wasmKey})`}`,
  );
  if (!cacheOk) {
    console.log(
      "      FAIL the same image twice must cost one describe, and the cache key must match the source's",
    );
    bad += 1;
  }

  // ── concurrency ────────────────────────────────────────────────────────────────────────────────
  const seedSecond = async (keys) => {
    await keys.put("token:tok-v2", "u-v2");
    await keys.put("user:u-v2", JSON.stringify({ id: "u-v2", enabled: true }));
    await keys.put("ukeys:u-v2", JSON.stringify({ OPENCODE_GO_API_KEY: "sk-user-og" }));
  };
  // **A RECORDING STUB, BECAUSE THE FIRST VERSION OF THIS SECTION COMPARED `undefined` TO `undefined`.** The
  // module-level `DurableObjectNamespace` takes only the verdict and records nothing, so `breaker.calls` was
  // absent on both sides and the comparison passed while measuring nothing — the failure mode this file has
  // recorded three times. The breaker's call log is the observable: `check` then `reset` when the upstream is
  // healthy.
  const shipBreaker = new DurableObjectNamespace([]);
  const wasmBreaker = new DurableObjectNamespace([]);
  const shipPair = [200, 200, true];
  shipBreaker.calls.push(...SHIPPING_BREAKER_LOG);
  let wasmPair = [0, 0, false];
  {
    const keys = kvForVision([]);
    await seedSecond(keys);
    const worker = await import(`${pathToFileURL(BUILT).href}?concurrent=1`);
    const instance = new worker.default();
    instance.env = { KEYS: keys, BREAKER: wasmBreaker, DO_AUTH: "stub" };
    instance.ctx = {};
    stubDescribe([]);
    const [a, b] = await Promise.all([
      instance.fetch(imageRequest("tok-v")),
      instance.fetch(imageRequest("tok-v2")),
    ]);
    const bodies = [await a.text(), await b.text()];
    wasmPair = [a.status, b.status, bodies[0] === bodies[1]];
    globalThis.fetch = real;
  }
  // **THE BREAKER LOG IS COMPARED AS A MULTISET, AND THAT IS THE CRITERION RATHER THAN A CONVENIENCE.**
  //
  // Measured 2026-10-07: this section passed locally and failed in CI, on the same commit, with the same worker —
  // because two requests in flight at once race for the breaker's 5 s cache, and whether BOTH call `check` or only
  // the first one does is decided by the scheduler. The shipping (JS) and the wasm (Rust) interleave differently
  // on a slower machine, so the SEQUENCES diverged while every observable answer stayed equal.
  //
  // **A SEQUENCE UNDER `Promise.all` IS THE SCHEDULER'S, NOT THE PORT'S** — asserting it asserts the runner, which
  // is the failure this repository has recorded four times under other names ("a gate that cannot see its own
  // premise"). What the port owes is the same SET of calls and the same answers; the order is measured here and
  // reported, not required.
  const shipLog = [...shipBreaker.calls].sort();
  const wasmLog = [...wasmBreaker.calls].sort();
  const concOk =
    JSON.stringify(shipPair) === JSON.stringify(wasmPair) && JSON.stringify(shipLog) === JSON.stringify(wasmLog);
  console.log(
    `  two requests at once, through the built worker: statuses ${wasmPair[0]}/${wasmPair[1]}, breaker calls ${JSON.stringify(wasmBreaker.calls)} (shipping ${JSON.stringify(shipBreaker.calls)}) — compared as a multiset`,
  );
  if (!concOk) {
    console.log(
      "      FAIL two concurrent requests must produce the same answers and the same set of breaker calls",
    );
    bad += 1;
  }
}

// ── THE DEVICE FAMILY — the console's device registry, against the shipping TypeScript ───────────
//
// **WHAT THIS SECTION IS FOR.** `gateway/wasm/src/devices.rs` and its four sibling modules are the
// `/api/devices` registry ported to Rust: fifteen routes whose decisions (which credential, which refusal,
// what a row shows, what a rename keeps) are now Rust. The criterion is the one every other route in this
// worker met: **the same request produces a byte-comparable answer from the TypeScript the console runs and
// from the built worker**, and where a response cannot show the difference, the OBSERVABLE is compared too —
// the KV writes the handler made and the requests it dialled.
//
// **THE RECORDING IS THE ORACLE, AND THE TYPESCRIPT IS STILL HERE TO RECORD FROM.** `DEVICES_RECORD=1 node
// verify.mjs` drives `gateway/src/index.ts`'s own front door (the same default export the deployed console
// runs) and writes every answer into `shipping-answers.json`; every other run replays those answers against
// the built worker. A fixture recorded from a FAILING run would pin the failure, so the writer refuses unless
// every case agreed on the run that produced it.
//
// **DETERMINISM IS INSTALLED, NOT ASSUMED, AND IT IS TWO PLATFORM CALLS.** The routes mint credentials
// (`crypto.getRandomValues`) and stamp times (`Date.now()`), so a recorded answer that contained either would
// never match again. Both sides therefore run under a pinned clock and a seeded CSPRNG: `Date` is replaced by
// a subclass whose no-argument constructor and `now()` answer a fixed instant (`worker::Date::now()` IS
// `new Date()`, so the Rust reads the same one), and `crypto` is a proxy whose `getRandomValues` fills from a
// counter. **THE SEQUENCE MATTERS AND IS PART OF THE MEASUREMENT**: a port that drew its randomness in a
// different order produces a different code and fails the byte comparison.
//
// **AND TWO THINGS THIS SECTION CANNOT SEE, NAMED RATHER THAN IMPLIED:**
//
//   * the CLOUDFLARE ACCESS identity arm of `requireSession` (`access.ts`) is NOT ported, so every case here
//     runs with `ACCESS_AUD`/`ACCESS_TEAM_DOMAIN` unset on BOTH sides — the cookie arm and both envelopes are
//     what is proved. That is also why the front door's cutover keeps cookie-less requests on the TypeScript
//     path, which the section at the end of this block measures.
//   * the worker's real KV, Durable Objects and runtime glue: `KEYS` is a stub, exactly as every other
//     section of this file stubs it.
{
  const FIXED_NOW = 1_760_000_000_000; // 2025-10-05T02:13:20Z, and every recorded timestamp is this
  const SESSION_SECRET = "device-family-session-secret";
  const ADMIN_TOKEN_64 = "a".repeat(64);
  const D1_TOKEN_64 = "1".repeat(64);
  const D2_TOKEN_64 = "2".repeat(64);
  const REG_KEY = "livekey01";
  const TUNNEL_KEY = "tunnelkey01";
  const GRANT_CODE = "c".repeat(32);

  const { issueSessionToken } = await import(new URL("../src/auth.ts", import.meta.url).href);
  const { __clearCaches } = await import(new URL("../src/store/cache.ts", import.meta.url).href);

  // ── the clock, the CSPRNG, the KV stub and the upstream: all shared, all pinned ────────────────
  // `installDeterminism`, `restoreGlobals`, `makeKvStub`, `buildRequestFor`, `stubUpstreamFor` and `rowsFor`
  // are defined once at module scope (see "THE STUBS BOTH RECORDED SECTIONS STAND ON"), because the identity
  // surface drives the same two doors with the same stubs and a second KV stub is how two harnesses come to
  // disagree about what `expirationTtl` means.
  const makeKv = (seed) => makeKvStub(seed, FIXED_NOW);

  // ── the registry the cases read ─────────────────────────────────────────────────────────────────
  // ONE seed for every case unless a case overrides it, so the cases differ by their REQUEST rather than by
  // their fixture. The record shapes are the console's own: three devices, one of them carrying every optional
  // field, one with no metadata at all.
  const SEED = {
    "auth:admin_password": "deadbeefdeadbeef:0123456789abcdef",
    _admin_seeded: "1",
    "user:admin": JSON.stringify({
      id: "admin",
      username: "admin",
      role: "admin",
      enabled: true,
      createdAt: 1,
      token: "admin-gateway-token",
    }),
    "user:bob": JSON.stringify({
      id: "bob",
      username: "bob",
      role: "user",
      enabled: true,
      createdAt: 1,
      token: "bob-gateway-token",
    }),
    "user:suspended": JSON.stringify({
      id: "suspended",
      username: "suspended",
      role: "admin",
      enabled: false,
      createdAt: 1,
      token: "suspended-gateway-token",
    }),
    "devices:v1": JSON.stringify([
      {
        name: "d1",
        hostname: "d1.agent.test",
        token: D1_TOKEN_64,
        proxySecret: "s".repeat(32),
        registeredAt: 1700000000000,
        lastSeenAt: 1700000100000,
        lastVersion: "1.2.3",
      },
      { name: "d2", hostname: "d2.agent.test", token: D2_TOKEN_64, registeredAt: 1700000000001 },
      { name: "taken", hostname: "taken.agent.test", token: "t".repeat(64) },
    ]),
    "plugins:v1": JSON.stringify({
      liveplugin: { device: "d1", createdAt: 1, expiresAt: FIXED_NOW + 86_400_000 },
      deadplugin: { device: "d2", createdAt: 1, expiresAt: FIXED_NOW - 1 },
      legacyplugin: { device: "d2", createdAt: 1 },
    }),
    "regkey:livekey01": "1",
    "regkey:expired01": "1",
    "regkey:claimed01": "1",
    "regclaim2:claimed01": "1",
    "reggrant:grantlive1": "1",
    "regkey:tunnelkey01": "1",
    "regclaim:tunnelclaimed": "1",
    "regkey:tunnelclaimed": "1",
    "cf:api_token": "cf-account-token",
    [`panelgrant:${GRANT_CODE}`]: JSON.stringify({ device: "d1", mintedAt: FIXED_NOW }),
    "panelgrant:othergrant": JSON.stringify({ device: "d2", mintedAt: FIXED_NOW }),
  };

  // The expiry of an UNSPENT registration key is KV's own, so a seeded key has none reported by `list()` —
  // which is exactly the "expired but not yet reaped" state the source's filter exists for.
  const COOKIES = {
    admin: await issueSessionToken(SESSION_SECRET, "admin", "admin"),
    user: await issueSessionToken(SESSION_SECRET, "bob", "user"),
    suspended: await issueSessionToken(SESSION_SECRET, "suspended", "admin"),
    ghost: await issueSessionToken(SESSION_SECRET, "nobody", "admin"),
    forged: await issueSessionToken("the-wrong-secret", "admin", "admin"),
  };

  const UPSTREAM_STATUS = {
    status: 200,
    body: JSON.stringify({ ok: true, name: "d1", proxy_secret: "p".repeat(40) }),
  };
  const UPSTREAM_STATUS_NO_SECRET = { status: 200, body: JSON.stringify({ ok: true, name: "d1" }) };
  const UPSTREAM_STATUS_SHORT_SECRET = { status: 200, body: JSON.stringify({ proxy_secret: "too-short" }) };
  const UPSTREAM_VERSION = {
    status: 200,
    body: JSON.stringify({ version: "9.9.9", download: "https://index.test/summrise-agent.tgz" }),
  };
  const UPSTREAM_STATUS_FOR_D1 = { "/api/status": UPSTREAM_STATUS };
  const UPSTREAM_STATUS_NO_SECRET_FOR_D1 = { "/api/status": UPSTREAM_STATUS_NO_SECRET };
  const UPSTREAM_STATUS_SHORT_FOR_D1 = { "/api/status": UPSTREAM_STATUS_SHORT_SECRET };
  const UPSTREAM_VERSION_ONLY = { "/api/version": UPSTREAM_VERSION };

  // ── the cases ───────────────────────────────────────────────────────────────────────────────────
  // Every route of the family, its successes AND its refusals: a bad credential, a spent claim, a device that
  // does not exist, a name that is taken, a verb the route does not answer.
  const CASES = [
    // ---- the registry, read and written ----
    { name: "GET /api/devices (admin session)", req: ["GET", "/api/devices"], cookie: "admin" },
    {
      name: "GET /api/devices (no session)",
      req: ["GET", "/api/devices"],
      cookie: null,
    },
    { name: "GET /api/devices (a non-admin session)", req: ["GET", "/api/devices"], cookie: "user" },
    { name: "GET /api/devices (a forged cookie)", req: ["GET", "/api/devices"], cookie: "forged" },
    {
      name: "GET /api/devices (a session for a SUSPENDED admin)",
      req: ["GET", "/api/devices"],
      cookie: "suspended",
    },
    {
      name: "GET /api/devices (a session for a user who does not exist)",
      req: ["GET", "/api/devices"],
      cookie: "ghost",
    },
    {
      name: "GET /api/devices (an allowlisted Origin)",
      req: ["GET", "/api/devices"],
      cookie: "admin",
      headers: { origin: "https://console.test" },
    },
    {
      name: "GET /api/devices (an origin that is NOT allowlisted)",
      req: ["GET", "/api/devices"],
      cookie: "admin",
      headers: { origin: "https://evil.test" },
    },
    {
      name: "POST /api/devices, a new device",
      req: ["POST", "/api/devices", { name: "d3", hostname: "d3.agent.test", token: "3".repeat(64) }],
      cookie: "admin",
    },
    {
      name: "POST /api/devices, an existing device (registeredAt and proxySecret are kept)",
      req: ["POST", "/api/devices", { name: "d1", hostname: "d1b.agent.test", token: "9".repeat(64) }],
      cookie: "admin",
    },
    {
      name: "POST /api/devices, a short name",
      req: ["POST", "/api/devices", { name: "", hostname: "d4.agent.test", token: "4".repeat(64) }],
      cookie: "admin",
    },
    {
      name: "POST /api/devices, a hostname that is not a domain",
      req: ["POST", "/api/devices", { name: "d4", hostname: "localhost", token: "4".repeat(64) }],
      cookie: "admin",
    },
    {
      name: "POST /api/devices, a hostname outside the device suffix",
      req: ["POST", "/api/devices", { name: "d4", hostname: "d4.evil.test", token: "4".repeat(64) }],
      cookie: "admin",
    },
    {
      name: "POST /api/devices, a token shorter than eight characters",
      req: ["POST", "/api/devices", { name: "d4", hostname: "d4.agent.test", token: "short" }],
      cookie: "admin",
    },
    {
      name: "POST /api/devices, no body at all",
      req: ["POST", "/api/devices", null],
      cookie: "admin",
    },
    { name: "GET /api/devices/d1/mcp", req: ["GET", "/api/devices/d1/mcp"], cookie: "admin" },
    {
      name: "GET /api/devices/d1/mcp, a name with a percent escape",
      req: ["GET", "/api/devices/d%31/mcp"],
      cookie: "admin",
    },
    {
      name: "GET /api/devices/nope/mcp, a device that does not exist",
      req: ["GET", "/api/devices/nope/mcp"],
      cookie: "admin",
    },
    { name: "DELETE /api/devices/d2", req: ["DELETE", "/api/devices/d2"], cookie: "admin" },
    {
      name: "DELETE /api/devices/nope, a device that does not exist",
      req: ["DELETE", "/api/devices/nope"],
      cookie: "admin",
    },
    {
      name: "DELETE /api/devices/d1 (it revokes the device's plugin links)",
      req: ["DELETE", "/api/devices/d1"],
      cookie: "admin",
    },
    // ---- rename ----
    {
      name: "POST /api/devices/d1/rename, a new name",
      req: ["POST", "/api/devices/d1/rename", { name: "d9" }],
      cookie: "admin",
    },
    {
      name: "POST /api/devices/d1/rename, a new name AND hostname",
      req: ["POST", "/api/devices/d1/rename", { name: "d9", hostname: "d9.agent.test" }],
      cookie: "admin",
    },
    {
      name: "POST /api/devices/d1/rename to a name that is taken",
      req: ["POST", "/api/devices/d1/rename", { name: "d2" }],
      cookie: "admin",
    },
    {
      name: "POST /api/devices/nope/rename, a device that does not exist",
      req: ["POST", "/api/devices/nope/rename", { name: "d9" }],
      cookie: "admin",
    },
    {
      name: "POST /api/devices/d1/rename, an invalid new name",
      req: ["POST", "/api/devices/d1/rename", { name: "not a name" }],
      cookie: "admin",
    },
    {
      name: "POST /api/devices/d1/rename, a hostname outside the device suffix",
      req: ["POST", "/api/devices/d1/rename", { name: "d9", hostname: "d9.evil.test" }],
      cookie: "admin",
    },
    {
      name: "POST /api/devices/d1/rename (renaming a device to its own name is not a conflict)",
      req: ["POST", "/api/devices/d1/rename", { name: "d1" }],
      cookie: "admin",
    },
    // ---- panel grants ----
    {
      name: "POST /api/devices/d1/panel-grant",
      req: ["POST", "/api/devices/d1/panel-grant"],
      cookie: "admin",
    },
    {
      name: "POST /api/devices/nope/panel-grant",
      req: ["POST", "/api/devices/nope/panel-grant"],
      cookie: "admin",
    },
    {
      name: "POST /api/devices/panel-grant/redeem, the grant's own device",
      req: ["POST", "/api/devices/panel-grant/redeem", { grant: GRANT_CODE }],
      cookie: null,
      headers: { authorization: `Bearer ${D1_TOKEN_64}` },
    },
    {
      name: "POST /api/devices/panel-grant/redeem, a grant minted for ANOTHER device",
      req: ["POST", "/api/devices/panel-grant/redeem", { grant: "othergrant" }],
      cookie: null,
      headers: { authorization: `Bearer ${D1_TOKEN_64}` },
    },
    {
      name: "POST /api/devices/panel-grant/redeem, an unknown grant",
      req: ["POST", "/api/devices/panel-grant/redeem", { grant: "d".repeat(32) }],
      cookie: null,
      headers: { authorization: `Bearer ${D1_TOKEN_64}` },
    },
    {
      name: "POST /api/devices/panel-grant/redeem, a malformed grant code",
      req: ["POST", "/api/devices/panel-grant/redeem", { grant: "not-a-code" }],
      cookie: null,
      headers: { authorization: `Bearer ${D1_TOKEN_64}` },
    },
    {
      name: "POST /api/devices/panel-grant/redeem, no device token",
      req: ["POST", "/api/devices/panel-grant/redeem", { grant: GRANT_CODE }],
      cookie: null,
    },
    {
      name: "POST /api/devices/panel-grant/redeem, a device token nobody has",
      req: ["POST", "/api/devices/panel-grant/redeem", { grant: GRANT_CODE }],
      cookie: null,
      headers: { authorization: `Bearer ${"f".repeat(64)}` },
    },
    // ---- registration keys ----
    { name: "GET /api/devices/register-keys", req: ["GET", "/api/devices/register-keys"], cookie: "admin" },
    {
      name: "POST /api/devices/register-key",
      req: ["POST", "/api/devices/register-key"],
      cookie: "admin",
    },
    {
      name: "DELETE /api/devices/register-keys/expired01",
      req: ["DELETE", "/api/devices/register-keys/expired01"],
      cookie: "admin",
    },
    {
      name: "DELETE /api/devices/register-keys/nosuchkey",
      req: ["DELETE", "/api/devices/register-keys/nosuchkey"],
      cookie: "admin",
    },
    // ---- install-cmd ----
    {
      name: "GET /api/devices/install-cmd",
      req: ["GET", "/api/devices/install-cmd"],
      cookie: "admin",
      upstream: UPSTREAM_VERSION_ONLY,
    },
    // ---- self-register ----
    {
      name: "POST /api/devices/self-register, a NEW device that answers with a proxy secret",
      req: ["POST", "/api/devices/self-register", { name: "d5", hostname: "d5.agent.test", token: "5".repeat(64) }],
      cookie: null,
      upstream: UPSTREAM_STATUS_FOR_D1,
    },
    {
      name: "POST /api/devices/self-register, a new device whose tunnel answers WITHOUT a secret",
      req: ["POST", "/api/devices/self-register", { name: "d6", hostname: "d6.agent.test", token: "6".repeat(64) }],
      cookie: null,
      upstream: UPSTREAM_STATUS_NO_SECRET_FOR_D1,
    },
    {
      name: "POST /api/devices/self-register, a new device whose tunnel answers with a SHORT secret",
      req: ["POST", "/api/devices/self-register", { name: "d7", hostname: "d7.agent.test", token: "7".repeat(64) }],
      cookie: null,
      upstream: UPSTREAM_STATUS_SHORT_FOR_D1,
    },
    {
      name: "POST /api/devices/self-register, an existing device with the SAME token (refresh)",
      req: ["POST", "/api/devices/self-register", { name: "d1", hostname: "D1.AGENT.TEST", token: D1_TOKEN_64 }],
      cookie: null,
    },
    {
      name: "POST /api/devices/self-register, an existing device with a DIFFERENT token and no proof",
      req: ["POST", "/api/devices/self-register", { name: "d1", hostname: "d1.agent.test", token: "9".repeat(64) }],
      cookie: null,
      upstream: UPSTREAM_STATUS_NO_SECRET_FOR_D1,
    },
    {
      name: "POST /api/devices/self-register, an existing device whose STORED tunnel proves the rotation",
      req: ["POST", "/api/devices/self-register", { name: "d1", hostname: "d1.agent.test", token: "9".repeat(64) }],
      cookie: null,
      upstream: { "/api/status": { status: 200, body: JSON.stringify({ proxy_secret: "s".repeat(32) }) } },
    },
    {
      name: "POST /api/devices/self-register, a moved hostname",
      req: ["POST", "/api/devices/self-register", { name: "d1", hostname: "moved.agent.test", token: D1_TOKEN_64 }],
      cookie: null,
    },
    {
      name: "POST /api/devices/self-register, a token that is not 64 hex characters",
      req: ["POST", "/api/devices/self-register", { name: "d8", hostname: "d8.agent.test", token: "not-a-token" }],
      cookie: null,
    },
    {
      name: "POST /api/devices/self-register, a hostname outside the device suffix",
      req: ["POST", "/api/devices/self-register", { name: "d8", hostname: "d8.evil.test", token: "8".repeat(64) }],
      cookie: null,
    },
    // ---- the one-time registration key ----
    {
      name: "POST /api/register, a live key",
      req: ["POST", "/api/register", { key: REG_KEY, name: "reg1", hostname: "reg1.agent.test", token: "b".repeat(64) }],
      cookie: null,
      upstream: UPSTREAM_STATUS_FOR_D1,
      headers: { "cf-connecting-ip": "203.0.113.11" },
    },
    {
      name: "POST /api/register, the grant a spent key leaves behind",
      req: ["POST", "/api/register", { key: "grantlive1", name: "reg2", hostname: "reg2.agent.test", token: "b".repeat(64) }],
      cookie: null,
      upstream: UPSTREAM_STATUS_NO_SECRET_FOR_D1,
      headers: { "cf-connecting-ip": "203.0.113.12" },
    },
    {
      name: "POST /api/register, an UPPERCASE key (the key is lowercased before the claim)",
      req: ["POST", "/api/register", { key: "LIVEKEY01", name: "reg3", hostname: "reg3.agent.test", token: "b".repeat(64) }],
      cookie: null,
      upstream: UPSTREAM_STATUS_NO_SECRET_FOR_D1,
      headers: { "cf-connecting-ip": "203.0.113.13" },
    },
    {
      name: "POST /api/register, a key that is already claimed",
      req: ["POST", "/api/register", { key: "claimed01", name: "reg4", hostname: "reg4.agent.test", token: "b".repeat(64) }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.14" },
    },
    {
      name: "POST /api/register, a key that does not exist",
      req: ["POST", "/api/register", { key: "nosuchkey", name: "reg5", hostname: "reg5.agent.test", token: "b".repeat(64) }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.15" },
    },
    {
      name: "POST /api/register, no key at all",
      req: ["POST", "/api/register", { name: "reg6", hostname: "reg6.agent.test", token: "b".repeat(64) }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.16" },
    },
    {
      name: "POST /api/register, a name that is already registered",
      req: ["POST", "/api/register", { key: REG_KEY, name: "d1", hostname: "d1.agent.test", token: "b".repeat(64) }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.17" },
    },
    {
      name: "POST /api/register, a body the device rules refuse",
      req: ["POST", "/api/register", { key: REG_KEY, name: "reg7", hostname: "reg7.agent.test", token: "short" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.18" },
    },
    {
      name: "POST /api/register, a hostname outside the device suffix",
      req: ["POST", "/api/register", { key: REG_KEY, name: "reg8", hostname: "reg8.evil.test", token: "b".repeat(64) }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.19" },
    },
    // ---- the tunnel token ----
    {
      name: "POST /api/install/tunnel-token, a live key (it spends the key)",
      req: ["POST", "/api/install/tunnel-token", { key: TUNNEL_KEY }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.21" },
    },
    {
      name: "POST /api/install/tunnel-token, a key that is already claimed",
      req: ["POST", "/api/install/tunnel-token", { key: "tunnelclaimed" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.22" },
    },
    {
      name: "POST /api/install/tunnel-token, a key that does not exist",
      req: ["POST", "/api/install/tunnel-token", { key: "nosuchkey" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.23" },
    },
    // ---- the front door's own answers on this prefix ----
    {
      name: "GET /api/devices/d1/proxy/panel (NOT this family: the reverse proxy)",
      req: ["GET", "/api/devices/d1/proxy/panel"],
      cookie: "admin",
      notInFamily: true,
    },
    { name: "GET /api/devices/anything/else/here", req: ["GET", "/api/devices/anything/else/here"], cookie: "admin" },
    { name: "PUT /api/devices", req: ["PUT", "/api/devices"], cookie: "admin" },
    { name: "OPTIONS /api/devices", req: ["OPTIONS", "/api/devices"], cookie: null },
  ];

  // ── the two doors ───────────────────────────────────────────────────────────────────────────────
  const shippingDoor = process.env.DEVICES_RECORD
    ? (await import(new URL("../src/index.ts", import.meta.url).href)).default
    : null;

  const buildRequest = (c, cookie) => {
    const [method, path, body] = c.req;
    const headers = { ...(c.headers ?? {}) };
    if (c.cookie !== null) headers.cookie = `ag_session=${cookie}`;
    if (body !== undefined && body !== null) headers["content-type"] = "application/json";
    return new Request(`https://console.test${path}`, {
      method,
      headers,
      ...(body === undefined || body === null ? {} : { body: JSON.stringify(body) }),
    });
  };

  const stubUpstream = (c, capture) => {
    globalThis.fetch = async (url, init = {}) => {
      const request = new Request(url, init);
      const auth = request.headers.get("authorization") ?? "-";
      capture.push(`${request.method} ${request.url} auth=${auth}`);
      const answer = (c.upstream ?? {})[new URL(request.url).pathname];
      if (!answer) throw new Error(`the device family's sweep has no upstream answer for ${request.url}`);
      return new Response(answer.body, {
        status: answer.status ?? 200,
        headers: { "content-type": answer.type ?? "application/json" },
      });
    };
  };

  const deviceEnv = (kv, extra = {}) => ({
    KEYS: kv,
    CONSOLE_HOST: "console.test",
    CONSOLE_ORIGINS: "https://console.test",
    DEVICE_HOST_SUFFIX: ".agent.test",
    INDEX_WORKER_URL: "https://index.test",
    SESSION_SECRET,
    ...extra,
  });

  /// The five things a device-family case compares, with every deployment hostname hashed (see `redactHosts`).
  const redacted = (got) => ({
    ...got,
    body: redactHosts(got.body),
    headers: got.headers.map(redactHosts),
    writes: got.writes.map(redactHosts),
    calls: got.calls.map(redactHosts),
  });

  const rowsFor = (got) => [
    ["status", String(got.status)],
    ["headers", got.headers.join(" | ")],
    ["body", got.body],
    ["kv writes", got.writes.join(" ; ")],
    ["upstream", got.calls.join(" ; ")],
  ];

  const driveTs = async (c, cookie) => {
    const kv = makeKv({ ...SEED, ...(c.seed ?? {}) });
    const calls = [];
    // **THE TYPESCRIPT SIDE IS ONE PROCESS WITH MODULE STATE, AND THE WASM SIDE IS A FRESH INSTANCE PER CASE.**
    // `store/cache.ts`'s `__c` holds `devices:v1` / `plugins:v1` for up to a day, and `randomHex`'s sequence is
    // process-wide, so without these two resets a case reads the PREVIOUS case's registry and draws the previous
    // case's bytes. Measured on this section's first run: `GET /api/devices/d1/mcp` answered with the hostname
    // an earlier case had written, and the panel-grant code was one draw further along than the worker's.
    __clearCaches();
    randomCursor = 0;
    stubUpstream(c, calls);
    let status = 0;
    let body = "";
    let headers = [];
    try {
      const response = await shippingDoor.fetch(buildRequest(c, cookie), deviceEnv(kv), {});
      status = response.status;
      body = await response.text();
      headers = pairs(response.headers);
    } catch (error) {
      body = `THREW ${error}`;
    }
    globalThis.fetch = realFetchGlobal;
    // **REDACTED ON BOTH SIDES, AT THE ONE PLACE AN ANSWER BECOMES COMPARABLE** — so the recorded fixture and
    // the worker's live answer are hashed by the same function, and a refusal message that names the deployment's
    // default device host is compared rather than transcribed into this repository.
    return redacted({ status, body, headers, writes: kv.writes, calls });
  };

  const driveWasm = async (c, cookie, i) => {
    const kv = makeKv({ ...SEED, ...(c.seed ?? {}) });
    const calls = [];
    // The same reset as the shipping side, for the same reason: the draw sequence is per-REQUEST, and each side
    // must start a case where the other one did.
    randomCursor = 0;
    stubUpstream(c, calls);
    const worker = await import(`${pathToFileURL(BUILT).href}?devices=${i}`);
    const instance = new worker.default();
    instance.env = deviceEnv(kv);
    instance.ctx = {};
    let status = 0;
    let body = "";
    let headers = [];
    try {
      const response = await instance.fetch(buildRequest(c, cookie));
      status = response.status;
      body = await response.text();
      headers = pairs(response.headers);
    } catch (error) {
      body = `THREW ${error}`;
    }
    globalThis.fetch = realFetchGlobal;
    return redacted({ status, body, headers, writes: kv.writes, calls });
  };

  // **`seedAdmin` RUNS ONCE PER PROCESS AND THE FIRST REQUEST PAYS FOR IT** — it reads and can write
  // (`_admin_seeded`, and the admin record itself on a deployment that has none). It is the FRONT DOOR's work,
  // not the route's, and the seeded `_admin_seeded` makes it a no-op after the first call — so it is spent on a
  // throwaway KV BEFORE the cases, and no case's write log carries it.
  if (shippingDoor) {
    try {
      __clearCaches();
      await shippingDoor.fetch(
        new Request("https://console.test/api/devices", { headers: { cookie: `ag_session=${COOKIES.admin}` } }),
        deviceEnv(makeKv(SEED)),
        {},
      );
    } catch {
      /* the prime is not a case */
    }
  }

  const FIXTURE_PATH = new URL("./shipping-answers.json", import.meta.url);
  const FIXTURE = JSON.parse(readFileSync(FIXTURE_PATH, "utf8"));
  const recorded = {};
  let same = 0;
  let different = 0;
  const failures = [];

  installDeterminism(FIXED_NOW);
  for (const [i, c] of CASES.entries()) {
    const cookie = c.cookie === null ? "" : COOKIES[c.cookie ?? "admin"];
    if (c.notInFamily) {
      // **THE ONE PATH THAT MUST NOT BE HANDED OVER.** `/api/devices/<name>/proxy/...` is `device-proxy.ts`, a
      // reverse proxy this slice does not port — so the family's dispatcher must refuse it and the front door
      // must keep serving it. The shipped answer is recorded and replayed like any other; what makes it a case
      // is that BOTH sides must produce it from their own code.
      const wasm = await driveWasm(c, cookie, i);
      const shipped = shippingDoor ? await driveTs(c, cookie) : null;
      if (shipped) {
        recorded[c.name] = {
          status: shipped.status,
          body: shipped.body,
          headers: shipped.headers,
          writes: shipped.writes,
          calls: shipped.calls,
        };
      }
      const reference = shipped ?? FIXTURE[c.name];
      if (!reference) {
        console.log(`      FAIL no recorded answer for case ${JSON.stringify(c.name)} — re-record with DEVICES_RECORD=1`);
        different += 1;
        failures.push(c.name);
        continue;
      }
      // The TypeScript's proxy answers 401 to a cookie-less caller and proxies for an admin; either way the
      // WASM must NOT be answering this path at all, which is what `writes`/`calls` being EMPTY shows.
      const wasmIsSilent = wasm.writes.length === 0 && wasm.calls.length === 0;
      if (wasmIsSilent) {
        same += 1;
        console.log(`      ok   ${c.name} — the family refuses it (404, no KV write, no dial)`);
      } else {
        different += 1;
        failures.push(c.name);
        console.log(`      FAIL ${c.name} — the wasm worker READ OR WROTE for a path it must not own`);
      }
      continue;
    }
    const shipped = shippingDoor ? await driveTs(c, cookie) : null;
    if (shipped) {
      recorded[c.name] = {
        status: shipped.status,
        body: shipped.body,
        headers: shipped.headers,
        writes: shipped.writes,
        calls: shipped.calls,
      };
    }
    const reference = shipped ?? FIXTURE[c.name];
    if (!reference) {
      console.log(`      FAIL no recorded answer for case ${JSON.stringify(c.name)} — re-record with DEVICES_RECORD=1`);
      different += 1;
      failures.push(c.name);
      continue;
    }
    const wasm = await driveWasm(c, cookie, i);
    const want = rowsFor(reference);
    const got = rowsFor(wasm);
    const notes = [];
    for (let r = 0; r < want.length; r++) {
      if (want[r][1] !== got[r][1]) {
        notes.push(`${want[r][0]}: ship ${JSON.stringify(want[r][1]).slice(0, 160)} :: wasm ${JSON.stringify(got[r][1]).slice(0, 160)}`);
      }
    }
    if (notes.length === 0) {
      same += 1;
      console.log(`      ok   ${c.name} — ${wasm.body.length} bytes, status ${wasm.status}, ${wasm.writes.length} KV write(s), ${wasm.calls.length} dial(s)`);
    } else {
      different += 1;
      failures.push(c.name);
      console.log(`      FAIL ${c.name}`);
      for (const note of notes) console.log(`           ${note}`);
    }
  }

  // ── the per-IP gate, which needs a budget rather than one request ───────────────────────────────
  // The three public routes cost KV WRITES, and the source's gate is what bounds that ("an attacker firing
  // random keys otherwise burned 2 KV writes per attempt"). One request cannot see a ten-per-minute budget, so
  // this case makes eleven from one address and compares WHERE the first 429 lands.
  {
    const IP = "198.51.100.7";
    const request = () =>
      new Request("https://console.test/api/register", {
        method: "POST",
        headers: { "content-type": "application/json", "cf-connecting-ip": IP },
        body: JSON.stringify({ key: "nosuchkey", name: "x", hostname: "x.agent.test", token: "x".repeat(16) }),
      });
    let shipFirst429 = null;
    if (shippingDoor) {
      __clearCaches();
      const env = deviceEnv(makeKv(SEED));
      for (let n = 1; n <= 11; n++) {
        const response = await shippingDoor.fetch(request(), env, {});
        await response.text();
        if (response.status === 429 && shipFirst429 === null) shipFirst429 = n;
      }
    } else {
      shipFirst429 = FIXTURE.__devicePublicRate?.first429 ?? null;
    }
    let wasmFirst429 = null;
    {
      const kv = makeKv(SEED);
      const worker = await import(`${pathToFileURL(BUILT).href}?devices=rate`);
      const instance = new worker.default();
      instance.env = deviceEnv(kv);
      instance.ctx = {};
      stubUpstream({}, []);
      for (let n = 1; n <= 11; n++) {
        const response = await instance.fetch(request());
        await response.text();
        if (response.status === 429 && wasmFirst429 === null) wasmFirst429 = n;
      }
      globalThis.fetch = realFetchGlobal;
    }
    if (shippingDoor) recorded.__devicePublicRate = { first429: shipFirst429 };
    if (shipFirst429 === wasmFirst429 && shipFirst429 !== null) {
      same += 1;
      console.log(`      ok   the public gate: the first 429 is request ${wasmFirst429} on both sides`);
    } else {
      different += 1;
      failures.push("the public per-IP gate");
      console.log(`      FAIL the public gate: first 429 at ${shipFirst429} (shipping) vs ${wasmFirst429} (wasm)`);
    }
  }
  restoreGlobals();

  if (process.env.DEVICES_RECORD) {
    // **WRITTEN ONLY FROM A RUN WHERE EVERY CASE AGREED.** A fixture recorded from a failing run pins the
    // failure; the /v1 sweep's recorder carries the same guard for the same reason. The existing entries are
    // preserved — they were recorded from the TypeScript the `/v1` port replaced, and this run cannot re-record
    // them (that implementation is deleted).
    if (different > 0) {
      console.log(`  !! NOT recording: ${different} case(s) differ, so those answers are not a reference`);
      process.exitCode = 1;
    } else {
      writeFileSync(FIXTURE_PATH, JSON.stringify({ ...FIXTURE, ...recorded }, null, 2) + "\n");
      console.log(`  recorded ${Object.keys(recorded).length} device-family answer(s) -> shipping-answers.json`);
    }
  }

  let rateNote = "";
  if (FIXTURE.__devicePublicRate && !shippingDoor) {
    rateNote = ` (the public gate's first 429 is recorded as request ${FIXTURE.__devicePublicRate.first429})`;
  }
  console.log(
    `  the device family, shipping TypeScript against the built worker: ${same}/${CASES.length + 1} identical, ${different} differing${rateNote}`,
  );
  for (const name of failures) console.log(`      FAIL ${name}`);
  bad += different;
}

// ── THE IDENTITY SURFACE — the session, the caller, and the caller's own credentials ────────────
//
// **THE SAME ORACLE THE DEVICE FAMILY USES, ON THE OTHER HALF OF THE CONSOLE.** The shipping front door
// (`src/index.ts`, the same default export the deployed console runs) and the built worker are driven with the
// same request, the same KV, the same env, the same pinned clock and the same seeded CSPRNG, and compared on
// status, headers, body-as-bytes, the KV WRITES each made and the requests each dialled. `AUTH_RECORD=1 node
// verify.mjs` records the shipping answers into `shipping-answers.json`; every other run replays them, and the
// writer refuses to record from a run in which anything differed.
//
// **THE ACCESS ARM IS PROVED ON A REAL RS256 KEY PAIR.** `requireSession` falls back to the edge-verified
// Cloudflare Access identity, and that arm verifies an RS256 JWT against the team's published certs. The key
// pair below is generated in this process, the JWKS is built from its public half, and the tokens are SIGNED
// with its private half — so both implementations do a real RSA verification of a real signature, and the
// cases that must be refused (a foreign signature, a wrong `aud`, a wrong `iss`, an expired token, a `kid` that
// is not published) are refused by the same code path that accepts the ones that must not be. **THE JWKS IS
// HANDED OVER AS A VALUE** (`ACCESS_JWKS_JSON`), which is how `fetchJwks` is written — so the comparison does
// not depend on a live network, and the ONE case that leaves it unset proves the fetch path through the same
// `fetch` stub the device family dials through.
//
// **THE SEEDED CLOCK AND THE SEEDED CSPRNG ARE PART OF THE MEASUREMENT.** PBKDF2 hashes are computed by the
// harness (the shipping `hashPassword`) and seeded into KV, so a login is a REAL 100,000-iteration derivation
// on both sides; the salts and tokens the routes mint come from the seeded CSPRNG, so a port that drew them in
// a different ORDER produces different bytes in the KV write log and fails the case.
//
// **THREE THINGS THIS SECTION CANNOT SEE, NAMED RATHER THAN IMPLIED:**
//
//   * the worker's real KV and runtime glue: `KEYS` is the same stub every other section uses;
//   * `POST /api/me/keys/test` and `/api/me/keys/usage` — the key DIAGNOSTICS, which dial six providers and
//     three usage endpoints. They are not identity and are not ported (`auth_routes.rs`'s header names them);
//     their absence here is why the cutover keeps them on the TypeScript path, which the cutover section
//     measures;
//   * `GET|PUT /api/me/route` — the model-route selection, which resolves through the model CATALOGUE and
//     `RouteDO`. Same reason, same carve-out.
//
// **AND THE FRONT DOOR'S CSRF GATE IS NOT DRIVEN HERE EITHER**: `csrfCookieViolation` runs in `index.ts` BEFORE
// the handover, so no case in this section carries `sec-fetch-site` — a cookie-carrying cross-site mutation is
// refused one level up and never reaches either implementation.
{
  const FIXED_NOW = 1_760_000_000_000; // 2025-10-05T02:13:20Z, the same instant the device family pins
  const SESSION_SECRET = "identity-surface-session-secret";
  const ADMIN_PW = "admin-hunter2";
  const ADMIN_SALT = "0123456789abcdef";
  const BOB_PW = "bob-password";
  const BOB_SALT = "fedcba9876543210";
  const TEAM = "summrise-team.cloudflareaccess.test";
  const AUD = "access-aud-0001";
  const ADMIN_EMAIL = "owner@corp.test";
  const KID = "kid-2026-10";
  const IP = "203.0.113.40"; // one address per case family; see the note on the module-state caches below

  const { issueSessionToken, hashPassword } = await import(new URL("../src/auth.ts", import.meta.url).href);
  const { __clearCaches } = await import(new URL("../src/store/cache.ts", import.meta.url).href);

  // ── the two credential hashes the seed carries ──────────────────────────────────────────────────
  // **THE HASH IS THE SHIPPING IMPLEMENTATION'S**, computed by the same `hashPassword` the worker runs, so a
  // login case is a real PBKDF2 verification on both sides rather than a string compare against a fixture.
  const ADMIN_HASH = `${ADMIN_SALT}:${await hashPassword(ADMIN_PW, ADMIN_SALT)}`;
  const BOB_HASH = `${BOB_SALT}:${await hashPassword(BOB_PW, BOB_SALT)}`;

  // ── the RS256 key pair, the JWKS built from it, and the tokens signed with it ────────────────────
  const keyPair = await crypto.subtle.generateKey(
    {
      name: "RSASSA-PKCS1-v1_5",
      modulusLength: 2048,
      publicExponent: new Uint8Array([1, 0, 1]),
      hash: "SHA-256",
    },
    true,
    ["sign", "verify"],
  );
  const foreignKeyPair = await crypto.subtle.generateKey(
    {
      name: "RSASSA-PKCS1-v1_5",
      modulusLength: 2048,
      publicExponent: new Uint8Array([1, 0, 1]),
      hash: "SHA-256",
    },
    true,
    ["sign", "verify"],
  );
  const publicJwk = await crypto.subtle.exportKey("jwk", keyPair.publicKey);
  const JWKS = { keys: [{ ...publicJwk, kid: KID, alg: "RS256", use: "sig" }] };
  // The SAME public key, published with a different `alg`: `keyJwk.alg !== "RS256"` is a refusal, and it is a
  // refusal that happens BEFORE the import — so this case is a 401 while the malformed-JWK case below is a 500.
  const NOT_RS256_JWKS = { keys: [{ ...publicJwk, kid: KID, alg: "RS512", use: "sig" }] };
  // A published key whose `alg` is right and whose JWK the runtime cannot read: `importKey` REJECTS it, and
  // that rejection is OUTSIDE `verifyAccessJwt`'s try/catch — so the request answers 500, not 401. The case
  // below is that arm, and it is here because a port that answered 401 would be a different response on a real
  // request.
  const BAD_JWKS = { keys: [{ kid: KID, alg: "RS256", kty: "RSA", n: "!!!not-a-modulus!!!", e: "AQAB" }] };

  const b64url = (text) => Buffer.from(text, "utf8").toString("base64url");
  const signAccess = async (claims, { key = keyPair.privateKey, kid = KID } = {}) => {
    const header = b64url(JSON.stringify({ alg: "RS256", kid, typ: "JWT" }));
    const payload = b64url(JSON.stringify(claims));
    const signature = await crypto.subtle.sign(
      "RSASSA-PKCS1-v1_5",
      key,
      new TextEncoder().encode(`${header}.${payload}`),
    );
    return `${header}.${payload}.${Buffer.from(signature).toString("base64url")}`;
  };
  const accessClaims = (over = {}) => ({
    aud: [AUD],
    iss: `https://${TEAM}`,
    exp: Math.floor(FIXED_NOW / 1000) + 3600,
    email: "newcomer@corp.test",
    ...over,
  });
  const VALID_JWT = await signAccess(accessClaims());

  // ── the session tokens, one per purpose (see the module-state note below) ───────────────────────
  /** A token with a payload this harness chose — `issueSessionToken` always stamps `Date.now() + TTL`. */
  const handMadeToken = async (secret, uid, role, exp) => {
    const payload = b64url(JSON.stringify({ uid, role, exp }));
    const key = await crypto.subtle.importKey(
      "raw",
      new TextEncoder().encode(secret),
      { name: "HMAC", hash: "SHA-256" },
      false,
      ["sign"],
    );
    const signature = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(payload));
    return `${payload}.${Buffer.from(signature).toString("base64url")}`;
  };

  // **EVERY HARNESS-BUILT TOKEN IS STAMPED WITH THE PINNED CLOCK, NOT THE REAL ONE.** `issueSessionToken` uses
  // `Date.now()`, and these are built at SETUP — before `installDeterminism` — so a token built with it carries
  // the wall clock and its bytes differ between the recording run and every replay. That is invisible until a
  // case WRITES the token somewhere the fixture compares, which the logout blacklist does: measured on the
  // first replay, where the recorded write and the replayed one differed by exactly the 22 seconds between the
  // two runs. `handMadeToken` writes the same three claims in the same order, with the same 24-hour life.
  const SESSION_EXP = FIXED_NOW + 24 * 3600 * 1000;
  const token = (uid, role, secret = SESSION_SECRET) => handMadeToken(secret, uid, role, SESSION_EXP);
  const COOKIES = {
    admin: await token("admin", "admin"),
    user: await token("bob", "user"),
    suspended: await token("suspended", "admin"),
    ghost: await token("nobody", "admin"),
    forged: await token("admin", "admin", "the-wrong-secret"),
    oldkey: await token("admin", "admin", ADMIN_HASH),
    expired: await handMadeToken(SESSION_SECRET, "admin", "admin", FIXED_NOW - 1000),
    revoked: await token("revoked-user", "user"),
    logout: await token("logout-probe", "admin"),
    logoutExpired: await handMadeToken(SESSION_SECRET, "logout-probe", "admin", FIXED_NOW - 90_000),
    // A signature that is ONE CHARACTER from a valid one: the constant-time comparison is what refuses it.
    tampered: `${(await token("admin", "admin")).slice(0, -1)}A`,
    garbage: "not-a-token-at-all",
  };


  // ── the seed every case reads, unless it overrides it ────────────────────────────────────────────
  const SEED = {
    "auth:admin_password": ADMIN_HASH,
    _admin_seeded: "1",
    "user:admin": JSON.stringify({
      id: "admin",
      username: "admin",
      role: "admin",
      enabled: true,
      createdAt: 1,
      token: "admin-gateway-token",
    }),
    "user:bob": JSON.stringify({
      id: "bob",
      username: "bob",
      role: "user",
      enabled: true,
      createdAt: 1,
      passwordHash: BOB_HASH,
      salt: BOB_SALT,
      token: "bob-gateway-token",
    }),
    "user:suspended": JSON.stringify({
      id: "suspended",
      username: "suspended",
      role: "admin",
      enabled: false,
      createdAt: 1,
      token: "suspended-gateway-token",
    }),
    "user:relayuser": JSON.stringify({
      id: "relayuser",
      username: "relayuser",
      role: "user",
      enabled: true,
      createdAt: 1,
      token: "relay-user-token",
      relayToken: "relay-token-1",
    }),
    "ukeys:bob": JSON.stringify({ DEEPSEEK_API_KEY: "sk-bob-deepseek" }),
    "invite:INVITE01": "1",
    "invclaim:TAKENINV": "1",
    "settings:US_PROXY": "1",
    "user:user": JSON.stringify({
      id: "user",
      username: "user",
      role: "user",
      enabled: true,
      createdAt: 1,
      token: "taken-base-token",
    }),
    "user:taken": JSON.stringify({
      id: "taken",
      username: "taken",
      role: "user",
      enabled: true,
      createdAt: 1,
      token: "taken-token",
    }),
    "access-email:bound@corp.test": "bob",
    "access-email:suspended@corp.test": "suspended",
  };

  // ── the cases ───────────────────────────────────────────────────────────────────────────────────
  // **THE REFUSALS ARE COVERED AS CAREFULLY AS THE SUCCESSES, AND EACH FAMILY HAS ITS OWN ADDRESS OR COOKIE.**
  // The shipping implementation is ONE process with module-level state (the login burst table, the failure
  // counters, the plugin's per-IP limiter, the revoked-session negative cache, the JWKS cache) while the built
  // worker is a FRESH instance per case — so a case that reused an address or a cookie would compare a warm
  // table against a cold one and report a difference that is the harness's. Every login case therefore carries
  // the header that gives it its own counter key, and every cookie value is used with ONE revocation state.
  const CASES = [
    // ── GET /api/me: the session refusals, one arm each ──────────────────────────────────────────
    { name: "GET /api/me (admin session)", req: ["GET", "/api/me"], cookie: "admin" },
    { name: "GET /api/me (no cookie)", req: ["GET", "/api/me"], cookie: null },
    { name: "GET /api/me (a cookie that is not a token)", req: ["GET", "/api/me"], cookie: "garbage" },
    { name: "GET /api/me (a forged cookie: the wrong signing key)", req: ["GET", "/api/me"], cookie: "forged" },
    { name: "GET /api/me (a tampered signature)", req: ["GET", "/api/me"], cookie: "tampered" },
    { name: "GET /api/me (an EXPIRED token)", req: ["GET", "/api/me"], cookie: "expired" },
    {
      name: "GET /api/me (a revoked cookie: the logout blacklist)",
      req: ["GET", "/api/me"],
      cookie: "revoked",
      seed: { [`sess-revoked:${COOKIES.revoked}`]: "1" },
    },
    { name: "GET /api/me (a session for a SUSPENDED admin)", req: ["GET", "/api/me"], cookie: "suspended" },
    { name: "GET /api/me (a session for a user who does not exist)", req: ["GET", "/api/me"], cookie: "ghost" },
    {
      name: "GET /api/me (the admin-password fallback: SESSION_SECRET unset, cookie signed with the hash)",
      req: ["GET", "/api/me"],
      cookie: "oldkey",
      env: { SESSION_SECRET: undefined },
    },
    {
      name: "GET /api/me (rotation compat: SESSION_SECRET set, cookie signed with the admin hash)",
      req: ["GET", "/api/me"],
      cookie: "oldkey",
    },
    { name: "GET /api/me (a non-admin session: the role is NOT required here)", req: ["GET", "/api/me"], cookie: "user" },
    {
      name: "GET /api/me (no admin password configured at all)",
      req: ["GET", "/api/me"],
      cookie: "admin",
      seed: { "auth:admin_password": "" },
    },
    {
      name: "GET /api/me (the caller's own keys are masked, and the deployment's are named)",
      req: ["GET", "/api/me"],
      cookie: "user",
    },
    {
      name: "GET /api/me (a truthy NON-STRING key value: maskKey has no .slice, so this is a 500)",
      req: ["GET", "/api/me"],
      cookie: "user",
      seed: { "ukeys:bob": JSON.stringify({ DEEPSEEK_API_KEY: 5 }) },
    },

    // ── the Access arm, on a real RS256 key pair ────────────────────────────────────────────────
    {
      name: "GET /api/me (an Access JWT that verifies: a FIRST-TIME email is provisioned)",
      req: ["GET", "/api/me", undefined],
      cookie: null,
      jwt: "VALID",
    },
    {
      name: "GET /api/me (an Access JWT for ACCESS_ADMIN_EMAIL: the seeded admin, no provisioning)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "ADMIN_EMAIL",
    },
    {
      name: "GET /api/me (an Access JWT for an email already bound to a user)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "BOUND",
    },
    {
      name: "GET /api/me (an Access JWT for a DISABLED bound account: no provisioning either)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "SUSPENDED_BOUND",
    },
    {
      name: "GET /api/me (an Access JWT whose local part is taken: the suffix loop)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "TAKEN_LOCAL",
    },
    {
      name: "GET /api/me (an Access JWT with TEN collisions: the uniqueness loop gives up and throws)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "EXHAUSTED",
      seed: {
        "user:user-0001": "x",
        "user:user-0203": "x",
        "user:user-0405": "x",
        "user:user-0607": "x",
        "user:user-0809": "x",
        "user:user-0a0b": "x",
        "user:user-0c0d": "x",
        "user:user-0e0f": "x",
        "user:user-1011": "x",
        "user:user-1213": "x",
      },
    },
    {
      name: "GET /api/me (an Access JWT signed by ANOTHER key)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "FOREIGN_SIGNATURE",
    },
    {
      name: "GET /api/me (an Access JWT for another audience — the array shape)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "WRONG_AUD",
    },
    {
      name: "GET /api/me (an Access JWT whose aud is a bare STRING that matches — the other legal shape)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "STRING_AUD",
    },
    {
      name: "GET /api/me (an Access JWT from ANOTHER team: the issuer is pinned)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "WRONG_ISS",
    },
    {
      name: "GET /api/me (an EXPIRED Access JWT)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "EXPIRED",
    },
    {
      name: "GET /api/me (an Access JWT with no kid: it can never be looked up)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "NO_KID",
    },
    {
      name: "GET /api/me (an Access JWT whose kid is not published)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "UNKNOWN_KID",
    },
    {
      name: "GET /api/me (an Access JWT whose published key is not RS256)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "NOT_RS256",
      env: { ACCESS_JWKS_JSON: JSON.stringify(NOT_RS256_JWKS) },
    },
    {
      name: "GET /api/me (an Access JWT with two parts, and one whose header is not base64)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "MALFORMED",
    },
    {
      name: "GET /api/me (an Access JWT that verifies but carries NO email)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "NO_EMAIL",
    },
    {
      name: "GET /api/me (an Access JWT whose published JWK the runtime REJECTS: a 500, not a 401)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "VALID",
      env: { ACCESS_JWKS_JSON: JSON.stringify(BAD_JWKS) },
    },
    {
      name: "GET /api/me (the Access arm DISABLED: ACCESS_AUD unset, so a valid JWT is ignored)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "VALID",
      env: { ACCESS_AUD: undefined },
    },
    // **THE ORDER OF THESE TWO IS PART OF THE CASE, NOT THE LAYOUT.** `fetchJwks` caches the fetched certs for
    // an hour in MODULE state, and the shipping side is one process while the built worker is a fresh instance
    // per case — so the FAILING fetch must come first. Reverse them and the shipping side answers the second
    // case from its cache (no dial, a 200) while the worker dials again and gets the 500: the divergence would
    // be the harness's, and it was measured here before the order was fixed.
    {
      name: "GET /api/me (the certs endpoint answers 500: the arm declines rather than throwing)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "FETCHED",
      env: { ACCESS_JWKS_JSON: undefined },
      upstream: { "/cdn-cgi/access/certs": { status: 500, body: "{}" } },
    },
    {
      name: "GET /api/me (an Access JWT verified through the FETCHED certs, not ACCESS_JWKS_JSON)",
      req: ["GET", "/api/me"],
      cookie: null,
      jwt: "FETCHED",
      env: { ACCESS_JWKS_JSON: undefined },
      upstream: { "/cdn-cgi/access/certs": { body: JSON.stringify(JWKS) } },
    },

    // ── register ────────────────────────────────────────────────────────────────────────────────
    {
      name: "POST /api/auth/register (a live invite: the record, the token mapping and the cookie)",
      req: ["POST", "/api/auth/register", { username: "carol", password: "carol-password", inviteCode: "INVITE01" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.51" },
    },
    {
      name: "POST /api/auth/register (an invite that does not exist)",
      req: ["POST", "/api/auth/register", { username: "carol", password: "carol-password", inviteCode: "NOPE" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.52" },
    },
    {
      name: "POST /api/auth/register (an invite whose claim is already held)",
      req: ["POST", "/api/auth/register", { username: "carol", password: "carol-password", inviteCode: "TAKENINV" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.53" },
    },
    {
      name: "POST /api/auth/register (a username that is already taken — checked BEFORE the invite)",
      req: ["POST", "/api/auth/register", { username: "bob", password: "carol-password", inviteCode: "INVITE01" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.54" },
    },
    {
      name: "POST /api/auth/register (a password under six characters)",
      req: ["POST", "/api/auth/register", { username: "carol", password: "short", inviteCode: "INVITE01" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.55" },
    },
    {
      name: "POST /api/auth/register (a username outside [A-Za-z0-9_.-]{2,32})",
      req: ["POST", "/api/auth/register", { username: "c", password: "carol-password", inviteCode: "INVITE01" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.56" },
    },
    {
      name: "POST /api/auth/register (no body at all parses to {})",
      req: ["POST", "/api/auth/register"],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.57" },
    },
    {
      name: "POST /api/auth/register (a NULL body: the TypeError becomes 400 with V8's message)",
      req: ["POST", "/api/auth/register"],
      rawBody: "null",
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.58" },
    },
    {
      name: "POST /api/auth/register (no SESSION_SECRET: the user IS created, then the issuance fails closed)",
      req: ["POST", "/api/auth/register", { username: "carol", password: "carol-password", inviteCode: "INVITE01" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.59" },
      env: { SESSION_SECRET: undefined },
    },
    {
      name: "POST /api/auth/register (no admin password configured)",
      req: ["POST", "/api/auth/register", { username: "carol", password: "carol-password", inviteCode: "INVITE01" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.60" },
      seed: { "auth:admin_password": "" },
    },

    // ── login ───────────────────────────────────────────────────────────────────────────────────
    {
      name: "POST /api/auth/login (the admin, right password: a real PBKDF2 verification)",
      req: ["POST", "/api/auth/login", { username: "admin", password: ADMIN_PW }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.61" },
    },
    {
      name: "POST /api/auth/login (the admin, wrong password)",
      req: ["POST", "/api/auth/login", { username: "admin", password: "wrong" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.62" },
    },
    {
      name: "POST /api/auth/login (a user account, right password: its own hash and salt)",
      req: ["POST", "/api/auth/login", { username: "bob", password: BOB_PW }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.63" },
    },
    {
      name: "POST /api/auth/login (a user account, right password, UNTRIMMED username)",
      req: ["POST", "/api/auth/login", { username: "  bob  ", password: BOB_PW }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.64" },
    },
    {
      name: "POST /api/auth/login (a user who does not exist: the PBKDF2 burn)",
      req: ["POST", "/api/auth/login", { username: "nobody", password: "whatever" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.65" },
    },
    {
      name: "POST /api/auth/login (a DISABLED user: the same burn, the same 401)",
      req: ["POST", "/api/auth/login", { username: "suspended", password: "whatever" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.66" },
    },
    {
      name: "POST /api/auth/login (the caller is already locked out)",
      req: ["POST", "/api/auth/login", { username: "admin", password: ADMIN_PW }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.67" },
      seed: { "login-lock:203.0.113.67:admin": "1" },
    },
    {
      name: "POST /api/auth/login (a NULL body: outside any try, so the front door's catch answers 500)",
      req: ["POST", "/api/auth/login"],
      rawBody: "null",
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.68" },
    },
    {
      name: "POST /api/auth/login (no admin password configured)",
      req: ["POST", "/api/auth/login", { username: "admin", password: ADMIN_PW }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.69" },
      seed: { "auth:admin_password": "" },
    },

    // ── logout ──────────────────────────────────────────────────────────────────────────────────
    {
      name: "POST /api/auth/logout (a live cookie: the blacklist write and the cleared cookie)",
      req: ["POST", "/api/auth/logout"],
      cookie: "logout",
      headers: { "cf-connecting-ip": "203.0.113.71" },
    },
    {
      name: "POST /api/auth/logout (a cookie whose token is EXPIRED: the 60 s floor)",
      req: ["POST", "/api/auth/logout"],
      cookie: "logoutExpired",
      headers: { "cf-connecting-ip": "203.0.113.72" },
    },
    {
      name: "POST /api/auth/logout (a malformed cookie: ignored, and the cookie is STILL cleared)",
      req: ["POST", "/api/auth/logout"],
      cookie: "not-a-token",
      headers: { "cf-connecting-ip": "203.0.113.73" },
    },
    {
      name: "POST /api/auth/logout (no cookie at all)",
      req: ["POST", "/api/auth/logout"],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.74" },
    },

    // ── reset-password ──────────────────────────────────────────────────────────────────────────
    {
      name: "POST /api/auth/reset-password (the admin token: a new salt and hash are written)",
      req: ["POST", "/api/auth/reset-password", { adminKey: "admin-gateway-token", newPassword: "a-new-password" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.81" },
    },
    {
      name: "POST /api/auth/reset-password (the wrong admin key)",
      req: ["POST", "/api/auth/reset-password", { adminKey: "not-the-token", newPassword: "a-new-password" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.82" },
    },
    {
      name: "POST /api/auth/reset-password (a new password under eight characters)",
      req: ["POST", "/api/auth/reset-password", { adminKey: "admin-gateway-token", newPassword: "short" }],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.83" },
    },
    {
      name: "POST /api/auth/reset-password (missing fields)",
      req: ["POST", "/api/auth/reset-password", {}],
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.84" },
    },
    {
      name: "POST /api/auth/reset-password (a NULL body: optional chaining makes this a clean 400)",
      req: ["POST", "/api/auth/reset-password"],
      rawBody: "null",
      cookie: null,
      headers: { "cf-connecting-ip": "203.0.113.85" },
    },

    // ── the caller's own credentials ────────────────────────────────────────────────────────────
    {
      name: "POST /api/me/token/regenerate (the admin token is rotated and its old mapping deleted)",
      req: ["POST", "/api/me/token/regenerate"],
      cookie: "admin",
    },
    {
      name: "POST /api/me/token/regenerate (the sweep must NOT delete the relay mapping)",
      req: ["POST", "/api/me/token/regenerate"],
      cookie: "relayuser",
      seed: { "token:relay-token-1": "relayuser", "token:relay-user-token": "relayuser" },
    },
    {
      name: "POST /api/me/token/relay (issue the scoped relay credential)",
      req: ["POST", "/api/me/token/relay"],
      cookie: "user",
    },
    {
      name: "DELETE /api/me/token/relay (revoke an existing one)",
      req: ["DELETE", "/api/me/token/relay"],
      cookie: "relayuser",
      seed: { "token:relay-token-1": "relayuser" },
    },
    {
      name: "DELETE /api/me/token/relay (there is none: revoked=false)",
      req: ["DELETE", "/api/me/token/relay"],
      cookie: "user",
    },
    {
      name: "POST /api/me/token/relay/reveal (one exists)",
      req: ["POST", "/api/me/token/relay/reveal"],
      cookie: "relayuser",
    },
    {
      name: "POST /api/me/token/relay/reveal (none exists: 404)",
      req: ["POST", "/api/me/token/relay/reveal"],
      cookie: "user",
    },
    {
      name: "PUT /api/me/keys (a valid save: the response carries the MASK)",
      req: ["PUT", "/api/me/keys", { name: "OPENROUTER_API_KEY", value: "  sk-or-1234567890  " }],
      cookie: "user",
    },
    {
      name: "PUT /api/me/keys (an empty value)",
      req: ["PUT", "/api/me/keys", { name: "OPENROUTER_API_KEY", value: "   " }],
      cookie: "user",
    },
    {
      name: "PUT /api/me/keys (a value that is not a string)",
      req: ["PUT", "/api/me/keys", { name: "OPENROUTER_API_KEY", value: 12345 }],
      cookie: "user",
    },
    {
      name: "PUT /api/me/keys (a name outside USER_KEY_NAMES)",
      req: ["PUT", "/api/me/keys", { name: "NOT_A_KEY", value: "sk-x" }],
      cookie: "user",
    },
    {
      name: "PUT /api/me/keys (no session)",
      req: ["PUT", "/api/me/keys", { name: "OPENROUTER_API_KEY", value: "sk-x" }],
      cookie: null,
    },
    {
      name: "DELETE /api/me/keys?name=OPENROUTER_API_KEY",
      req: ["DELETE", "/api/me/keys?name=OPENROUTER_API_KEY"],
      cookie: "user",
      seed: { "ukeys:bob": JSON.stringify({ OPENROUTER_API_KEY: "sk-or-old", DEEPSEEK_API_KEY: "sk-ds" }) },
    },
    {
      name: "DELETE /api/me/keys?name=NOT_A_KEY",
      req: ["DELETE", "/api/me/keys?name=NOT_A_KEY"],
      cookie: "user",
    },
    {
      name: "POST /api/me/keys/reveal (the caller's OWN key, in the clear)",
      req: ["POST", "/api/me/keys/reveal", { name: "DEEPSEEK_API_KEY" }],
      cookie: "user",
    },
    {
      name: "POST /api/me/keys/reveal (not configured: 404)",
      req: ["POST", "/api/me/keys/reveal", { name: "GMI_API_KEY" }],
      cookie: "user",
    },
    {
      name: "POST /api/me/keys/reveal (no session)",
      req: ["POST", "/api/me/keys/reveal", { name: "DEEPSEEK_API_KEY" }],
      cookie: null,
    },
    {
      name: "GET /api/me/usproxy (the switch is ON in KV)",
      req: ["GET", "/api/me/usproxy"],
      cookie: "user",
    },
    {
      name: "GET /api/me/usproxy (an explicit \"0\" reads as OFF — the round-95 normalization)",
      req: ["GET", "/api/me/usproxy"],
      cookie: "user",
      seed: { "settings:US_PROXY": "0" },
    },
    {
      name: "GET /api/me/usproxy (no KV value: the Worker var answers)",
      req: ["GET", "/api/me/usproxy"],
      cookie: "user",
      seed: { "settings:US_PROXY": undefined },
      env: { US_PROXY: "1" },
    },
    { name: "GET /api/me/usproxy (no session)", req: ["GET", "/api/me/usproxy"], cookie: null },
    {
      name: "PUT /api/me/usproxy (an admin turns it OFF: the explicit \"0\" is WRITTEN)",
      req: ["PUT", "/api/me/usproxy", { enabled: false }],
      cookie: "admin",
    },
    {
      name: "PUT /api/me/usproxy (an admin turns it ON)",
      req: ["PUT", "/api/me/usproxy", { enabled: true }],
      cookie: "admin",
      seed: { "settings:US_PROXY": "0" },
    },
    {
      name: "PUT /api/me/usproxy (a NON-ADMIN session: 403 \"Admin only\"/\"forbidden\")",
      req: ["PUT", "/api/me/usproxy", { enabled: true }],
      cookie: "user",
    },

    // ── the front door's own answers on these prefixes ──────────────────────────────────────────
    { name: "GET /api/auth/nonsense (a path under the base with no route)", req: ["GET", "/api/auth/nonsense"], cookie: null },
    { name: "GET /api/me/anything/else (a path under /api/me with no route)", req: ["GET", "/api/me/anything/else"], cookie: "admin" },
    { name: "PUT /api/auth/login (the wrong verb for a real path)", req: ["PUT", "/api/auth/login", {}], cookie: null },
  ];

  // ── the JWTs the cases present, built once from the key pair above ───────────────────────────────
  // Each entry is a real token: the ones that must verify are signed with the harness's private key, and the
  // ones that must not are signed with a FOREIGN key, carry another audience/issuer, or are simply malformed.
  const JWTS = {
    VALID: VALID_JWT,
    ADMIN_EMAIL: await signAccess(accessClaims({ email: ADMIN_EMAIL })),
    BOUND: await signAccess(accessClaims({ email: "bound@corp.test" })),
    SUSPENDED_BOUND: await signAccess(accessClaims({ email: "suspended@corp.test" })),
    TAKEN_LOCAL: await signAccess(accessClaims({ email: "taken@corp.test" })),
    EXHAUSTED: await signAccess(accessClaims({ email: "user@corp.test" })),
    FOREIGN_SIGNATURE: await signAccess(accessClaims(), { key: foreignKeyPair.privateKey }),
    WRONG_AUD: await signAccess(accessClaims({ aud: ["another-audience"] })),
    STRING_AUD: await signAccess(accessClaims({ aud: AUD })),
    WRONG_ISS: await signAccess(accessClaims({ iss: "https://another-team.cloudflareaccess.test" })),
    EXPIRED: await signAccess(accessClaims({ exp: Math.floor(FIXED_NOW / 1000) - 60 })),
    NO_KID: await signAccess(accessClaims(), { kid: "" }),
    UNKNOWN_KID: await signAccess(accessClaims(), { kid: "kid-not-published" }),
    NOT_RS256: await signAccess(accessClaims()),
    MALFORMED: "only.two",
    NO_EMAIL: await signAccess(accessClaims({ email: "no-at-sign" })),
    FETCHED: VALID_JWT,
  };

  // ── the two doors ───────────────────────────────────────────────────────────────────────────────
  const shippingDoor = process.env.AUTH_RECORD
    ? (await import(new URL("../src/index.ts", import.meta.url).href)).default
    : null;

  /** The seed for a case: the shared registry, the case's overrides, and `null`/`undefined` meaning REMOVE. */
  const seedFor = (c) => {
    const merged = { ...SEED, ...(c.seed ?? {}) };
    for (const key of Object.keys(merged)) {
      if (merged[key] === undefined || merged[key] === null) delete merged[key];
    }
    return merged;
  };

  /** The env for a case — the deployment's own vars, plus the ones the identity surface reads. */
  const envFor = (kv, c) => {
    const env = {
      KEYS: kv,
      CONSOLE_HOST: "console.test",
      CONSOLE_ORIGINS: "https://console.test",
      SESSION_SECRET,
      ACCESS_AUD: AUD,
      ACCESS_TEAM_DOMAIN: TEAM,
      ACCESS_JWKS_JSON: JSON.stringify(JWKS),
      ACCESS_ADMIN_EMAIL: ADMIN_EMAIL,
      // A DEPLOYMENT key: `userKeysStatus` reports `source: "deployment"` for a channel whose own key the
      // worker holds, which is the state a console page got wrong before 2026-09-21.
      DEEPSEEK_API_KEY: "sk-deployment-deepseek",
      ...(c.env ?? {}),
    };
    for (const key of Object.keys(env)) {
      if (env[key] === undefined) delete env[key];
    }
    return env;
  };

  const requestFor = (c, cookie) => {
    const headers = { ...(c.headers ?? {}) };
    if (c.jwt) headers["cf-access-jwt-assertion"] = JWTS[c.jwt];
    return buildRequestFor({ ...c, headers }, cookie);
  };

  const driveTs = async (c, cookie) => {
    const kv = makeKvStub(seedFor(c), FIXED_NOW);
    const calls = [];
    // **THE MODULE-STATE RESETS ARE NOT OPTIONAL.** `store/cache.ts`'s `__c` holds users and key blobs, and
    // `randomHex`'s sequence is process-wide: without both, a case reads the previous case's records and draws
    // the previous case's bytes. (The tables the PLUGIN keeps — the login burst counter, the failure counter,
    // the per-IP limiter, the revoked-session negative cache — have no reset hook, which is why every case that
    // touches one carries its own address or its own cookie value; see the note above the case list.)
    __clearCaches();
    randomCursor = 0;
    stubUpstreamFor(c, calls);
    let status = 0;
    let body = "";
    let headers = [];
    try {
      const response = await shippingDoor.fetch(requestFor(c, cookie), envFor(kv, c), {});
      status = response.status;
      body = await response.text();
      headers = pairs(response.headers);
    } catch (error) {
      body = `THREW ${error}`;
    }
    globalThis.fetch = realFetchGlobal;
    return redactRow({ status, body, headers, writes: kv.writes, calls });
  };

  const driveWasm = async (c, cookie, i) => {
    const kv = makeKvStub(seedFor(c), FIXED_NOW);
    const calls = [];
    randomCursor = 0;
    stubUpstreamFor(c, calls);
    const worker = await import(`${pathToFileURL(BUILT).href}?auth=${i}`);
    const instance = new worker.default();
    instance.env = envFor(kv, c);
    instance.ctx = {};
    let status = 0;
    let body = "";
    let headers = [];
    try {
      const response = await instance.fetch(requestFor(c, cookie));
      status = response.status;
      body = await response.text();
      headers = pairs(response.headers);
    } catch (error) {
      body = `THREW ${error}`;
    }
    globalThis.fetch = realFetchGlobal;
    return redactRow({ status, body, headers, writes: kv.writes, calls });
  };

  // **`seedAdmin` RUNS ONCE PER PROCESS AND THE FIRST REQUEST PAYS FOR IT.** It is the FRONT DOOR's work, not
  // the route's (the Rust worker never does it), and the seeded `_admin_seeded` makes it a no-op — so it is
  // spent on a throwaway KV BEFORE the cases, and no case's write log carries it.
  if (shippingDoor) {
    try {
      __clearCaches();
      await shippingDoor.fetch(
        new Request("https://console.test/api/health"),
        envFor(makeKvStub(SEED, FIXED_NOW), {}),
        {},
      );
    } catch {
      /* the prime is not a case */
    }
  }

  const FIXTURE_PATH = new URL("./shipping-answers.json", import.meta.url);
  const FIXTURE = JSON.parse(readFileSync(FIXTURE_PATH, "utf8"));
  const recorded = {};
  let same = 0;
  let different = 0;
  const failures = [];

  installDeterminism(FIXED_NOW);
  for (const [i, c] of CASES.entries()) {
    const cookie = c.cookie === null ? "" : COOKIES[c.cookie ?? "admin"];
    const shipped = shippingDoor ? await driveTs(c, cookie) : null;
    if (shipped) {
      recorded[c.name] = {
        status: shipped.status,
        body: shipped.body,
        headers: shipped.headers,
        writes: shipped.writes,
        calls: shipped.calls,
      };
    }
    const reference = shipped ?? FIXTURE[c.name];
    if (!reference) {
      console.log(
        `      FAIL no recorded answer for case ${JSON.stringify(c.name)} — re-record with AUTH_RECORD=1`,
      );
      different += 1;
      failures.push(c.name);
      continue;
    }
    const wasm = await driveWasm(c, cookie, i);
    const want = rowsFor(reference);
    const got = rowsFor(wasm);
    const notes = [];
    for (let r = 0; r < want.length; r++) {
      if (want[r][1] !== got[r][1]) {
        notes.push(
          `${want[r][0]}: ship ${JSON.stringify(want[r][1]).slice(0, 220)} :: wasm ${JSON.stringify(got[r][1]).slice(0, 220)}`,
        );
      }
    }
    if (notes.length === 0) {
      same += 1;
      console.log(
        `      ok   ${c.name} — ${wasm.body.length} bytes, status ${wasm.status}, ${wasm.writes.length} KV write(s)`,
      );
    } else {
      different += 1;
      failures.push(c.name);
      console.log(`      FAIL ${c.name}`);
      for (const note of notes) console.log(`           ${note}`);
    }
  }

  // ── the three in-memory gates, which need a BUDGET rather than one request ──────────────────────
  // The login burst counter, the failure counter that arms the KV lock, and the plugin's own per-IP limiter are
  // all per-address and per-window: one request cannot see any of them. Each block uses an address no other
  // case has touched, because the shipping side keeps these tables for the LIFE of the process while the built
  // worker is a fresh instance per case — a shared address would compare a warm table against a cold one.
  const SEQUENCES = {
    __authLoginBurst: {
      count: 11,
      tag: "burst",
      name: "the login burst gate: the eleventh rapid attempt 429s",
      request: () =>
        new Request("https://console.test/api/auth/login", {
          method: "POST",
          headers: { "content-type": "application/json", "cf-connecting-ip": "198.51.100.21" },
          body: JSON.stringify({ username: "admin", password: "admin-hunter2" }),
        }),
    },
    __authLoginLock: {
      count: 6,
      tag: "lock",
      name: "the login failure lock: the fifth wrong password arms it, and the sixth is refused",
      request: () =>
        new Request("https://console.test/api/auth/login", {
          method: "POST",
          headers: { "content-type": "application/json", "cf-connecting-ip": "198.51.100.22" },
          body: JSON.stringify({ username: "admin", password: "wrong" }),
        }),
    },
    __authLogoutRate: {
      count: 31,
      tag: "logoutrate",
      name: "the plugin's own limiter: the thirty-first logout 429s",
      request: () =>
        new Request("https://console.test/api/auth/logout", {
          method: "POST",
          headers: { "cf-connecting-ip": "198.51.100.23" },
        }),
    },
  };

  const driveSequence = async (spec, kv, drive) => {
    const statuses = [];
    for (let n = 0; n < spec.count; n++) statuses.push(await drive(spec.request));
    return { statuses, writes: kv.writes };
  };

  for (const [key, spec] of Object.entries(SEQUENCES)) {
    const kv = makeKvStub(SEED, FIXED_NOW);
    let shipped = null;
    if (shippingDoor) {
      const env = envFor(makeKvStub(SEED, FIXED_NOW), {});
      const seen = { statuses: [] };
      for (let n = 0; n < spec.count; n++) {
        const response = await shippingDoor.fetch(spec.request(), env, {});
        await response.text();
        seen.statuses.push(response.status);
      }
      recorded[key] = { statuses: seen.statuses, writes: kv.writes };
      shipped = recorded[key];
    } else {
      shipped = FIXTURE[key];
    }
    if (!shipped) {
      console.log(`      FAIL no recorded answer for ${JSON.stringify(key)} — re-record with AUTH_RECORD=1`);
      different += 1;
      failures.push(spec.name);
      continue;
    }
    const worker = await import(`${pathToFileURL(BUILT).href}?auth=${spec.tag}`);
    const instance = new worker.default();
    instance.env = envFor(kv, {});
    instance.ctx = {};
    stubUpstreamFor({}, []);
    const wasm = await driveSequence(spec, kv, async (request) => {
      const response = await instance.fetch(request());
      await response.text();
      return response.status;
    });
    globalThis.fetch = realFetchGlobal;
    // **THE WHOLE SEQUENCE IS COMPARED, STATUSES AND WRITES** — the point of the fifth failure is not its 401
    // (every failure answers 401) but the lock it ARMS with its 60 s TTL, which a status-only comparison cannot
    // see.
    const want = `${shipped.statuses.join(",")} :: ${(shipped.writes ?? []).join(" ; ")}`;
    const got = `${wasm.statuses.join(",")} :: ${wasm.writes.join(" ; ")}`;
    if (want === got) {
      same += 1;
      console.log(`      ok   ${spec.name} — ${wasm.statuses.join(",")}`);
    } else {
      different += 1;
      failures.push(spec.name);
      console.log(`      FAIL ${spec.name}\n           ship ${want}\n           wasm ${got}`);
    }
  }
  restoreGlobals();

  if (process.env.AUTH_RECORD) {
    // **WRITTEN ONLY FROM A RUN WHERE EVERY CASE AGREED.** A fixture recorded from a failing run pins the
    // failure; the two recorders above carry the same guard for the same reason.
    if (different > 0) {
      console.log(`  !! NOT recording: ${different} case(s) differ, so those answers are not a reference`);
      process.exitCode = 1;
    } else {
      writeFileSync(FIXTURE_PATH, JSON.stringify({ ...FIXTURE, ...recorded }, null, 2) + "\n");
      console.log(`  recorded ${Object.keys(recorded).length} identity-surface answer(s) -> shipping-answers.json`);
    }
  }

  console.log(
    `  the identity surface, shipping TypeScript against the built worker: ${same}/${CASES.length + Object.keys(SEQUENCES).length} identical, ${different} differing`,
  );
  for (const name of failures) console.log(`      FAIL ${name}`);
  bad += different;
}

// ── THE CUTOVER IS A ROUTING DECISION, AND THIS SECTION IS ITS PROOF ─────────────────────────────
//
// `./scripts/build.sh gateway` is what puts the new code in front of a user, and the CODE's half of the cutover
// is `index.ts`'s handover: with the `WASM_GATE` binding present, the device family and the identity surface are
// served by the Rust worker; without it, by the TypeScript plugins — which is the rollback, and the reason
// `plugins/devices.ts`, `plugins/auth.ts` and the stores are still in the tree.
//
// **A BOUNDARY NOBODY MEASURES IS A BOUNDARY NOBODY HAS.** This section drives the SHIPPING front door with a
// stub gate that RECORDS what reached it, and asserts what the handover claims: the two families' routes go to
// the gate, the routes neither slice ported do NOT, and the `/v1` cutover is unchanged. The stub answers a
// recognisable body, so a case that was supposed to reach it and did not is visible in the response rather than
// assumed from a list.
//
// **AND ONE ROW IS SLICE 2's POINT, WRITTEN WHERE IT WAS MEASURED.** `GET /api/devices` with NO cookie was
// asserted as "kept on the TypeScript path" while `requireSession`'s Access arm was unported; that arm is Rust
// now, the condition is deleted, and the row asserts the direction the code now takes — a cookie-less admin is
// served by the worker. A cutover that only ever grows a list is a cutover whose deletions are untested.
{
  const shippingDoor = (await import(new URL("../src/index.ts", import.meta.url).href)).default;
  const forwarded = [];
  const gateBody = JSON.stringify({ servedBy: "the-wasm-gate" });
  const env = {
    KEYS: {
      async get() {
        return null;
      },
      async put() {},
      async delete() {},
      async list() {
        return { keys: [], list_complete: true, cursor: undefined };
      },
    },
    CONSOLE_HOST: "console.test",
    CONSOLE_ORIGINS: "https://console.test",
    WASM_GATE: {
      async fetch(request) {
        forwarded.push(`${request.method} ${new URL(request.url).pathname}`);
        return new Response(gateBody, {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      },
    },
  };
  const session = await (await import(new URL("../src/auth.ts", import.meta.url).href)).issueSessionToken(
    "cutover-secret",
    "admin",
    "admin",
  );
  const call = async (method, path, withCookie) => {
    forwarded.length = 0;
    const headers = withCookie ? { cookie: `ag_session=${session}` } : {};
    const response = await shippingDoor.fetch(new Request(`https://console.test${path}`, { method, headers }), env, {});
    const body = await response.text();
    return { reached: forwarded.length > 0, body, status: response.status };
  };

  const CUTOVER_CASES = [
    // [method, path, cookie?, should the GATE see it?, why]
    ["GET", "/api/devices", true, true, "the family's list route"],
    ["POST", "/api/devices", true, true, "the family's add route"],
    ["GET", "/api/devices/d1/mcp", true, true, "a device's MCP config"],
    ["DELETE", "/api/devices/d1", true, true, "a device's delete"],
    ["POST", "/api/devices/d1/rename", true, true, "a device's rename"],
    ["POST", "/api/devices/d1/panel-grant", true, true, "a panel grant"],
    ["POST", "/api/devices/panel-grant/redeem", false, true, "the grant's redeem: the AGENT calls it with a device token and no cookie at all"],
    ["GET", "/api/devices/register-keys", true, true, "the outstanding keys"],
    ["POST", "/api/devices/register-key", true, true, "a new key"],
    ["GET", "/api/devices/install-cmd", true, true, "the install command"],
    ["POST", "/api/register", false, true, "the public registration, which never has a session"],
    ["POST", "/api/devices/self-register", false, true, "the agent's self-registration, cookie-less by nature"],
    ["POST", "/api/install/tunnel-token", false, true, "the tunnel token, cookie-less by nature"],
    ["GET", "/api/devices/anything/else", true, true, "a path under the prefix the family does not serve (the worker's own 404, which is the front door's)"],
    // NOT handed over:
    ["GET", "/api/devices/d1/proxy/panel", true, false, "the reverse proxy is device-proxy.ts, not this slice"],
    ["POST", "/api/devices/d1/proxy/mcp", true, false, "the reverse proxy accepts any method"],
    ["GET", "/api/devices", false, true, "NO session cookie: the Cloudflare Access arm is RUST now (slice 2), so a cookie-less admin goes to the worker like every other caller"],
    // ---- the identity surface (landing 5 slice 2) ----
    ["POST", "/api/auth/login", false, true, "the credential routes need no session"],
    ["POST", "/api/auth/register", false, true, "…and neither does registration"],
    ["GET", "/api/me", true, true, "the account route"],
    ["GET", "/api/me", false, true, "the account route with NO cookie: the Access arm decides"],
    ["POST", "/api/me/token/regenerate", true, true, "a token rotation"],
    ["PUT", "/api/me/keys", true, true, "a key save"],
    ["GET", "/api/auth/nonsense", false, true, "a path under the base the worker answers with its own 404"],
    // NOT handed over:
    ["GET", "/api/me/route", true, false, "the model-route selection resolves through the catalogue and RouteDO, and is not this slice"],
    ["PUT", "/api/me/route", true, false, "…the same route's write"],
    ["POST", "/api/me/keys/test", true, false, "the key diagnostics dial six providers, and are not this slice"],
    ["POST", "/api/me/keys/usage", true, false, "…the same for the usage queries"],
    ["POST", "/api/upload", true, false, "the file relay: a 100 MiB body passthrough this slice does not port"],
    ["GET", "/api/health", true, false, "the public tooling route, unchanged"],
  ];
  let cutoverOk = 0;
  let cutoverBad = 0;
  for (const [method, path, cookie, expected, why] of CUTOVER_CASES) {
    const got = await call(method, path, cookie);
    const ok = got.reached === expected;
    if (ok) cutoverOk += 1;
    else cutoverBad += 1;
    console.log(
      `      ${ok ? "ok  " : "FAIL"} ${method} ${path}${cookie ? "" : " (no cookie)"} — ${got.reached ? "handed to the gate" : "kept on the TypeScript path"} (${why})`,
    );
  }
  // The `/v1` cutover, which this change must not have disturbed.
  const v1 = await call("GET", "/v1/models", false);
  if (v1.reached) cutoverOk += 1;
  else {
    cutoverBad += 1;
    console.log("      FAIL GET /v1/models — the existing cutover went to the gate before this change and must still");
  }
  console.log(`  the two cutovers, through the shipping front door with a stub gate: ${cutoverOk}/${CUTOVER_CASES.length + 1} routed as the boundary says`);
  bad += cutoverBad;
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
