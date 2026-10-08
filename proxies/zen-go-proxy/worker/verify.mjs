// verify.mjs — run the BUILT Rust worker and the SHIPPING JavaScript on the same request, and
// compare the bytes. The criterion is P3's own: "同请求新旧响应字节可比" — not "it works", the bytes.
//
// ── THE MUTATION THAT MUST FAIL THIS CHECK ──────────────────────────────────────────────────────
// Read this when you change this file: the mutation is how you find out whether the check can still
// fail at all. A check that cannot be broken is worse than no check.
//
// MUTATION: in `worker.rs`, skip the gate for a path no route names —
//           `let skip = matches!(route(method, &pathname), Route::NotFound);` and then
//           `if !skip && !x_api_key_allows(…)`. (The observable equivalent of moving the gate below the
//           `match`, which is the shape the shipping worker does NOT have.)
// RESULT:   `FAIL GET /nope — 97 bytes, status 401`, and the rows name both sides:
//             status DIFFERS   ts: "401"  rust: "404"
//             body DIFFERS     ts: {"type":"error","error":{"type":"authentication_error",…}}
//                              rust: {"type":"error","error":{"type":"not_found_error",…}}
//           (measured 2026-10-05.)
//
//           **THAT IS THE STRUCTURAL DIFFERENCE FROM `zen-us`**, and the case is in this corpus precisely
//           because a byte comparison is what notices it.
//
// ── WHAT THE TWO SIDES ARE ──────────────────────────────────────────────────────────────────────
//
//   old — `shipping-answers.json`: what `proxies/zen-go-proxy/src/index.js` answered, RECORDED by this
//         file on a run where both sides agreed on all 17 cases (`SWEEP_RECORD=1 node verify.mjs
//         --build`). **THE JAVASCRIPT ITSELF IS DELETED (2026-10-08)**, which is what the fixture is
//         for: the port replaced it, and a differential whose left side is gone has to hold the
//         answers rather than re-run the implementation. Recording in a run where the two AGREED is
//         what makes this a reference rather than a pinned failure.
//   new — `build/index.js`, the module `worker-build --release` produced, driven through its real
//         `fetch` entrypoint with the same env and the same request.
//
// **RE-RECORDING IS NOT A LOCAL OPERATION ANY MORE**, and it is named rather than implied: with the
// shipping half deleted, `SWEEP_RECORD=1` needs `src/index.js` back (`git show <sha>:proxies/
// zen-go-proxy/src/index.js`), and a changed case list therefore needs a deliberate re-record rather
// than a silent one. The fixture carries each case's method and path so a stale one is REFUSED with
// that sentence instead of compared against the wrong row.
//
// ── AND THE UPSTREAM IS STUBBED BY REPLACING THE GLOBAL `fetch` ─────────────────────────────────
//
// The loader answers the two imports a workers-rs module has (`cloudflare:workers` and the `.wasm`
// module) and stubs the runtime with an EMPTY class — so a fetch from the built module would reach the
// real `opencode.ai`. **workers-rs's `Fetch` calls the JavaScript global `fetch`, and so does the
// shipping worker**, so one replacement stubs BOTH sides — and it RECORDS what it was asked, which is
// the only way to see the headers the worker sends upstream.
//
// WHAT IT DOES NOT COVER, STATED RATHER THAN IMPLIED: the real upstream's behaviour is replaced by fixed
// answers. This compares the two IMPLEMENTATIONS against the same upstream, not against the provider.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { register } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const BUILT = join(HERE, "build", "index.js");

// ── build ───────────────────────────────────────────────────────────────────────────────────────
if (!existsSync(BUILT) || process.argv.includes("--build")) {
  console.log("  building with worker-build --release …");
  execFileSync("worker-build", ["--release"], { cwd: HERE, stdio: "inherit" });
}

// ── TWO RUNTIME DIFFERENCES, BOTH NAMED ─────────────────────────────────────────────────────────
// 1. The glue calls `addEventListener("error", …)` at MODULE LOAD. Both of this repository's other
//    harnesses carry the same line for the same reason (`index/worker/verify.mjs:51`,
//    `gateway/wasm/verify.mjs:54`).
globalThis.addEventListener ??= () => {};
// 2. `new Request(url, { body })` in Node throws `RequestInit: duplex option is required when sending a
//    body`; a Worker runtime does not ask, and the BUILT MODULE hits it through `web_sys::Request`, which
//    is the global `Request` here. **A HARNESS ACCOMMODATION, NOT A FIX TO THE PORT** — named because a
//    shim that is not written down is a difference between the tested artifact and the shipped one.
const RealRequest = globalThis.Request;
globalThis.Request = class extends RealRequest {
  constructor(input, init) {
    if (init && init.body != null && init.duplex === undefined) {
      init = { ...init, duplex: "half" };
    }
    super(input, init);
  }
};

// ── the loader, REGISTERED FROM `gateway/wasm` AND NOT COPIED ───────────────────────────────────
register(new URL("../../../gateway/wasm/loader.mjs", import.meta.url));

// The compiled module the glue expects Cloudflare's bundler to hand it (see the loader).
// **`index_bg.wasm`, NOT `<crate>_bg.wasm`** — `worker-build` names the entry `index.js` and the module
// beside it `index_bg.wasm` whatever the crate is called.
const WASM = join(HERE, "build", "index_bg.wasm");
writeFileSync(
  `${WASM}.mjs`,
  `import { readFileSync } from "node:fs";\n` +
    `export default new WebAssembly.Module(readFileSync(new URL("./index_bg.wasm", import.meta.url)));\n`,
);

// ── the upstream stub, and the record of what was asked ─────────────────────────────────────────
const WORKER_KEY = "worker-key-1234567890";
const CLIENT_KEY = "client-key-1234567890";

let upstream = null;

/**
 * **THE TWO SHAPES A CALLER MAY USE.** The shipping worker calls `fetch(url, init)`; workers-rs's
 * `Fetch::Request(req).send()` hands the global `fetch` a REQUEST OBJECT and no init. A stub that reads
 * `init?.method` only records every Rust upstream call as `GET` with no headers — **a difference in the
 * harness that looks exactly like a difference in the port.**
 */
function describe(input, init) {
  const req = input instanceof globalThis.Request ? input : new globalThis.Request(input, init);
  return {
    method: req.method,
    path: new URL(req.url).pathname,
    headers: [...req.headers.entries()]
      .map(([k, v]) => `${k.toLowerCase()}: ${v}`)
      .sort(),
  };
}

/** The upstream's answers. `/v1/messages` serves BOTH shapes, so it reads the request's `stream` flag. */
function answerFor(input, init) {
  const req = input instanceof globalThis.Request ? input : new globalThis.Request(input, init);
  const path = new URL(req.url).pathname;
  if (path === "/zen/go/v1/models") {
    return new Response(JSON.stringify({ data: [{ id: "m" }] }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  }
  if (path === "/zen/go/v1/chat/completions") {
    // An OpenAI-shaped answer, so `toAnthropicResponse` has something real to translate.
    return new Response(
      JSON.stringify({
        id: "chatcmpl-1",
        choices: [{ index: 0, message: { role: "assistant", content: "hi" }, finish_reason: "stop" }],
        usage: { prompt_tokens: 3, completion_tokens: 1 },
      }),
      { status: 200, headers: { "content-type": "application/json" } },
    );
  }
  if (path === "/zen/go/v1/messages") {
    return new Response('event: message_start\ndata: {"type":"message_start"}\n\n', {
      status: 200,
      headers: { "content-type": "text/event-stream" },
    });
  }
  return new Response("not found", { status: 404 });
}

let OVERRIDE = null;

globalThis.fetch = async (input, init) => {
  upstream = describe(input, init);
  if (OVERRIDE) return OVERRIDE(upstream.path);
  return answerFor(input, init);
};

// ── the env both sides get ──────────────────────────────────────────────────────────────────────
const ENV = { CLIENT_KEY, OPENCODE_GO_API_KEY: WORKER_KEY };
const HOST = "https://zen-go.test";

const KEY = { "x-api-key": CLIENT_KEY };

// ── the cases ───────────────────────────────────────────────────────────────────────────────────
const CASES = [
  // **THE ONE THAT PROVES THE GATE'S POSITION.** No key, and a path NO ROUTE NAMES: the shipping
  // worker answers 401 because the gate runs before it looks at the path.
  { method: "GET", path: "/nope", label: "no key, on a path NO ROUTE NAMES — the gate runs first" },
  { method: "OPTIONS", path: "/v1/messages", label: "the preflight — answered BEFORE the gate" },
  { method: "OPTIONS", path: "/anything", label: "the preflight, on a path no route names" },
  { method: "GET", path: "/v1/models", label: "GET /v1/models with no key" },
  { method: "GET", path: "/v1/models", headers: { "x-api-key": "wrong" }, label: "GET /v1/models, wrong key" },
  { method: "POST", path: "/nope", headers: KEY, label: "a path no route names, WITH the key" },
  { method: "GET", path: "/v1/messages", headers: KEY, label: "GET on the messages path — not a POST" },

  // The upstream arms.
  { method: "GET", path: "/v1/models", headers: KEY, label: "GET /v1/models, gated and served" },
  {
    method: "POST",
    path: "/v1/messages/count_tokens",
    headers: { ...KEY, "content-type": "application/json" },
    body: JSON.stringify({ system: "abc", tools: [{ a: 1 }], messages: [{ role: "user" }] }),
    label: "count_tokens — system + tools + messages, all three counted",
  },
  {
    method: "POST",
    path: "/v1/messages/count_tokens",
    headers: { ...KEY, "content-type": "application/json" },
    body: JSON.stringify({ system: null, messages: null }),
    label: "count_tokens — null parts are dropped by the loose comparison",
  },

  // The native arm: the Flash line, streamed and not.
  {
    method: "POST",
    path: "/v1/messages",
    headers: { ...KEY, "content-type": "application/json" },
    body: JSON.stringify({ model: "deepseek-flash", stream: true, messages: [{ role: "user" }] }),
    label: "messages NATIVE + stream — the raw request goes up, SSE comes back",
  },
  {
    method: "POST",
    path: "/v1/messages",
    headers: { ...KEY, "content-type": "application/json" },
    body: JSON.stringify({ model: "deepseek-v4-flash", messages: [{ role: "user" }] }),
    label: "messages NATIVE, the retired alias, not streamed — JSON, and no Cache-Control",
  },

  // The translated arm: every other model.
  {
    method: "POST",
    path: "/v1/messages",
    headers: { ...KEY, "content-type": "application/json" },
    body: JSON.stringify({ model: "other-model", stream: true, messages: [{ role: "user", content: "hi" }] }),
    label: "messages TRANSLATED + stream — toOpenAIRequest up, toSSE back",
  },
  {
    method: "POST",
    path: "/v1/messages",
    headers: { ...KEY, "content-type": "application/json" },
    body: JSON.stringify({ model: "other-model", messages: [{ role: "user", content: "hi" }] }),
    label: "messages TRANSLATED, not streamed — toAnthropicResponse, jsonOk",
  },
  {
    method: "POST",
    path: "/v1/messages",
    headers: { ...KEY, "content-type": "application/json" },
    body: JSON.stringify({ messages: [{ role: "user" }] }),
    label: "messages with NO model — the deepseek-flash default",
  },

  // The failure paths, where the provider's words reach the client and must not carry a credential.
  {
    method: "POST",
    path: "/v1/messages",
    headers: { ...KEY, "content-type": "application/json" },
    body: JSON.stringify({ model: "deepseek-flash", messages: [] }),
    override: () =>
      new Response(JSON.stringify({ error: { message: `bad key ${WORKER_KEY}` } }), {
        status: 400,
        headers: { "content-type": "application/json" },
      }),
    label: "a 4xx that ECHOES THE WORKER'S KEY — it must be redacted",
  },
  {
    method: "POST",
    path: "/v1/messages",
    headers: { ...KEY, "content-type": "application/json" },
    body: JSON.stringify({ model: "deepseek-flash", messages: [] }),
    override: () =>
      new Response(JSON.stringify({ error: { message: "upstream exploded" } }), {
        status: 503,
        headers: { "content-type": "application/json" },
      }),
    label: "a 5xx — generic client text, the detail stays server-side",
  },
];

const pairs = (h) => [...h.entries()].map(([k, v]) => `${k.toLowerCase()}: ${v}`).sort();

/** The response identity the comparison is made on: status, sorted headers, and the body BYTES. */
async function shape(resp) {
  const body = Buffer.from(await resp.arrayBuffer());
  return {
    status: resp.status,
    headers: pairs(resp.headers),
    bytes: body.length,
    body: body.toString("utf8"),
  };
}

// ── the old side: THE RECORDED ANSWERS, VALIDATED BEFORE ANY CASE RUNS ─────────────────────────
//
// A fixture that belongs to a DIFFERENT case list is refused here, by name, rather than compared row by
// row against the wrong answer: `method` and `path` travel with every entry, so an inserted or
// reordered case is a sentence instead of a silent mismatch.
const FIXTURE = new URL("./shipping-answers.json", import.meta.url);
const RECORD = !!process.env.SWEEP_RECORD;

let recorded = null;
if (RECORD) {
  console.log("  RECORDING: this run writes the fixture, and needs the shipping half (`../src/index.js`)");
} else {
  recorded = JSON.parse(readFileSync(FIXTURE, "utf8"));
  const stale = (why) => {
    console.error(
      `\nshipping-answers.json is not this case list: ${why}\n` +
        `  It is RECORDED, so it can only be made where the shipping half is: put ` +
        `proxies/zen-go-proxy/src/index.js back from git history and run\n` +
        `      SWEEP_RECORD=1 node verify.mjs --build\n` +
        `  (see this file's header — the fixture is the left side now, and a changed list needs a ` +
        `deliberate re-record, not a silent one.)`,
    );
    process.exit(2);
  };
  if (recorded.cases.length !== CASES.length) {
    stale(`it holds ${recorded.cases.length} case(s), CASES has ${CASES.length}`);
  }
  for (const [i, c] of CASES.entries()) {
    const r = recorded.cases[i];
    if (r.method !== c.method || r.path !== c.path) {
      stale(`entry ${i} is ${r.method} ${r.path}, CASES[${i}] is ${c.method} ${c.path}`);
    }
  }
}

// **`src/index.js` IS IMPORTED IN RECORD MODE ONLY.** It is deleted from the tree, so every other run
// reads the fixture — which is the state this harness exists in now.
const shippingFetch = RECORD
  ? (await import(new URL("../src/index.js", import.meta.url).href)).default.fetch
  : null;

let bad = 0;
const fresh = [];
for (const [i, c] of CASES.entries()) {
  const url = HOST + c.path;
  const init = { method: c.method, headers: c.headers || {} };
  if (c.body !== undefined) init.body = c.body;

  let jsShape, jsUpstream;
  if (RECORD) {
    OVERRIDE = c.override || null;
    upstream = null;
    jsShape = await shape(await shippingFetch(new Request(url, init), ENV));
    jsUpstream = upstream;
    fresh.push({ label: c.label, method: c.method, path: c.path, ...jsShape, upstream: jsUpstream });
  } else {
    const r = recorded.cases[i];
    jsShape = { status: r.status, headers: r.headers, bytes: r.bytes, body: r.body };
    jsUpstream = r.upstream;
  }

  OVERRIDE = c.override || null;
  upstream = null;
  // A FRESH module instance per case: the Rust side reads its env per request, and a fresh instance is
  // also what a new isolate is.
  const mod = await import(`${pathToFileURL(BUILT).href}?case=${i}`);
  const instance = new mod.default();
  instance.env = ENV;
  instance.ctx = {};
  const rsResp = await instance.fetch(new Request(url, init));
  const rsShape = await shape(rsResp);
  const rsUpstream = upstream;

  const rows = [
    ["status", String(jsShape.status), String(rsShape.status)],
    ["headers", jsShape.headers.join(" | "), rsShape.headers.join(" | ")],
    ["body bytes", String(jsShape.bytes), String(rsShape.bytes)],
    ["body", jsShape.body, rsShape.body],
    // **THE ROW THAT MAKES THE UPSTREAM VISIBLE** — where the keys and the chosen endpoint live.
    [
      "upstream request",
      jsUpstream ? `${jsUpstream.method} ${jsUpstream.path} | ${jsUpstream.headers.join(" | ")}` : "(none)",
      rsUpstream ? `${rsUpstream.method} ${rsUpstream.path} | ${rsUpstream.headers.join(" | ")}` : "(none)",
    ],
  ];
  const differs = rows.filter(([, want, got]) => want !== got).length;
  console.log(`  ${c.label}`);
  console.log(
    `      ${differs ? "FAIL" : "ok  "} ${c.method} ${c.path} — ${jsShape.bytes} bytes, status ${jsShape.status}`,
  );
  if (differs) {
    bad++;
    for (const [what, want, got] of rows) {
      if (want === got) continue;
      console.log(`      ${what} DIFFERS`);
      console.log(`        ts  : ${JSON.stringify(want).slice(0, 300)}`);
      console.log(`        rust: ${JSON.stringify(got).slice(0, 300)}`);
    }
  }
}

// **A FIXTURE RECORDED FROM A RUN WHERE THE TWO SIDES DISAGREE IS A PINNED FAILURE, NOT A REFERENCE.**
// So the recording run refuses to write one — the only way `shipping-answers.json` can exist is a run
// that proved the port equal to the implementation it replaced.
if (RECORD) {
  if (bad > 0) {
    console.error(
      `\nREFUSING TO RECORD: ${bad} of ${CASES.length} case(s) differ. Fix the port (or the harness) ` +
        `first — a fixture written from a disagreement pins the disagreement.`,
    );
    process.exit(1);
  }
  writeFileSync(FIXTURE, `${JSON.stringify({ cases: fresh }, null, 2)}\n`);
  console.log(`  recorded ${fresh.length} shipping answer(s) -> shipping-answers.json`);
}

console.log(
  bad === 0
    ? `\nzen-go verify: ${CASES.length} case(s) byte-identical, upstream requests included`
    : `\nzen-go verify: ${bad} of ${CASES.length} case(s) DIFFER`,
);
process.exit(bad === 0 ? 0 : 1);
