// verify.mjs — run the BUILT Rust worker and the SHIPPING JavaScript on the same request, and
// compare the bytes. The criterion is P3's own: "同请求新旧响应字节可比" — not "it works", the bytes.
//
// ── THE MUTATION THAT MUST FAIL THIS CHECK ──────────────────────────────────────────────────────
// Read this when you change this file: the mutation is how you find out whether the check can still
// fail at all. A check that cannot be broken is worse than no check.
//
// MUTATION: in `worker.rs`, drop the `sessionHeader` push from the `/responses` arm — one line, and the
//           response bytes do not change at all, because the session header goes UPSTREAM.
// RESULT:   `zen-us verify: 2 of 17 case(s) DIFFER`, and the two are the `/responses` upstream cases:
//             FAIL POST /v1/responses — 29 bytes, status 200
//             upstream request DIFFERS
//               ts  : "POST /zen/go/v1/responses | authorization: Bearer … | content-type: application/json | x-opencode-session: s-1"
//               rust: "POST /zen/go/v1/responses | authorization: Bearer … | content-type: application/json"
//           (measured 2026-10-05, on this file's first run against a real mutation.)
//
//           **AND THE `29 bytes, status 200` ON BOTH SIDES IS THE POINT.** The response bytes are
//           IDENTICAL under this mutation — the session header only goes upstream — so a response-only
//           check would have called it green. **THE UPSTREAM REQUEST ROW IS WHAT MAKES THE CHECK REAL.**
//
// ── WHY THIS FILE CAN EXIST NOW, AND COULD NOT BEFORE ───────────────────────────────────────────
//
// `zen-us`'s crate was ONE `lib.rs` with the policy in it and no entrypoint — its own `Cargo.toml`
// called it "FIRST HALF". The entrypoint and its three upstream arms landed on 2026-10-03/05, so there
// is finally something to drive.
//
// ── WHAT THE TWO SIDES ARE ──────────────────────────────────────────────────────────────────────
//
//   old — `shipping-answers.json`: what `proxies/zen-us-proxy/src/index.js` answered, RECORDED by this
//         file on a run where both sides agreed on all 17 cases (`SWEEP_RECORD=1 node verify.mjs
//         --build`). **THE JAVASCRIPT ITSELF IS DELETED (2026-10-08)**, which is what the fixture is
//         for: the port replaced it, and a differential whose left side is gone has to hold the
//         answers rather than re-run the implementation. Recording in a run where the two AGREED is
//         what makes this a reference rather than a pinned failure.
//   new — `build/index.js`, the module `worker-build --release` produced, executed through its real
//         `fetch` entrypoint with the same env and the same request.
//
// **RE-RECORDING IS NOT A LOCAL OPERATION ANY MORE**, and it is named rather than implied: with the
// shipping half deleted, `SWEEP_RECORD=1` needs `src/index.js` back (`git show <sha>:proxies/
// zen-us-proxy/src/index.js`), and a changed case list therefore needs a deliberate re-record rather
// than a silent one. The fixture carries each case's method and path so a stale one is REFUSED with
// that sentence instead of compared against the wrong row.
//
// ── AND THE UPSTREAM IS STUBBED BY REPLACING THE GLOBAL `fetch` ─────────────────────────────────
//
// The loader answers the two imports a workers-rs module has (`cloudflare:workers` and the `.wasm`
// module), and it stubs the runtime with an EMPTY class — so a fetch from the built module would reach
// the real `opencode.ai`. **workers-rs's `Fetch` calls the JavaScript global `fetch`, and so does the
// shipping worker**, so replacing `globalThis.fetch` stubs BOTH sides with one function — and that
// function RECORDS what it was asked, which is the only way to see the headers the worker sends
// upstream. `x-opencode-session` and both credentials live there and nowhere else.
//
// WHAT IT DOES NOT COVER, STATED RATHER THAN IMPLIED: the real upstream's own behaviour (its status
// codes, its content types, its SSE framing) is replaced by fixed answers below. This compares the two
// IMPLEMENTATIONS against the same upstream, not against the provider.
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

// ── ONE WORKER GLOBAL NODE DOES NOT HAVE ────────────────────────────────────────────────────────
// The glue calls `addEventListener("error", …)` at MODULE LOAD, to catch a `WebAssembly.RuntimeError`
// and re-initialise the instance. Both of this repository's other harnesses carry the same line for the
// same reason — `index/worker/verify.mjs:51` and `gateway/wasm/verify.mjs:54` — and it is the ONLY
// runtime global the three of them need.
globalThis.addEventListener ??= () => {};

// ── AND ONE PLACE WHERE NODE IS STRICTER THAN THE RUNTIME ───────────────────────────────────────
// `new Request(url, { body })` in Node throws `RequestInit: duplex option is required when sending a
// body`; a Worker runtime does not ask. The BUILT MODULE hits this through `web_sys::Request`, which is
// the global `Request` here, so the port's upstream call died on a rule the runtime it is written for
// does not have. **THIS IS A HARNESS ACCOMMODATION, NOT A FIX TO THE PORT** — named rather than hidden,
// because a shim that is not written down is a difference between the tested artifact and the shipped
// one. It defaults the field and changes nothing else.
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
// It answers exactly the two imports a workers-rs module has, and it is thirty lines of that. A second
// copy would be the shape this whole migration is deleting, so this file registers the existing one:
// one loader, three workers.
register(new URL("../../../gateway/wasm/loader.mjs", import.meta.url));

// The compiled module the glue expects Cloudflare's bundler to hand it (see the loader).
//
// **`index_bg.wasm`, NOT `<crate>_bg.wasm`** — `worker-build` names the entry `index.js` and the module
// beside it `index_bg.wasm` whatever the crate is called, and the loader appends `.mjs` to whatever the
// glue imported. Measured: the first version of this wrote `zen_us_bg.wasm.mjs` and the run died with
// `ENOENT … build/index_bg.wasm.mjs` AFTER a seven-minute build.
const WASM = join(HERE, "build", "index_bg.wasm");
writeFileSync(
  `${WASM}.mjs`,
  `import { readFileSync } from "node:fs";\n` +
    `export default new WebAssembly.Module(readFileSync(new URL("./index_bg.wasm", import.meta.url)));\n`,
);

// ── the upstream stub, and the record of what was asked ─────────────────────────────────────────
const WORKER_KEY = "worker-key-1234567890";
const CLIENT_KEY = "client-key-1234567890";
const CALLER_KEY = "caller-key-1234567890";

/** The upstream's answer for a path, and the record of the request that asked for it. */
let upstream = null;

/**
 * **THE TWO SHAPES A CALLER MAY USE, AND BOTH SIDES USE A DIFFERENT ONE.** The shipping worker calls
 * `fetch(url, init)`; workers-rs's `Fetch::Request(req).send()` hands the global `fetch` a REQUEST
 * OBJECT and no init at all. The first version of this stub read `init?.method` and `init?.headers`
 * only, so every Rust upstream call was recorded as `GET` with no headers — **a difference in the
 * harness that looked exactly like a difference in the port.** Measured: nine cases "failed" on that
 * row alone, and the response bytes matched in all of them.
 */
function describe(input, init) {
  // **THE SHIMMED `Request`, NOT `RealRequest`** — the shipping worker calls `fetch(url, init)` with the
  // INCOMING REQUEST'S BODY STREAM as `init.body`, and constructing that with the unshimmed class throws
  // the same `duplex` error the shim exists for. Measured: with `RealRequest` here, every case whose
  // upstream call carried a body recorded `(none)` for the ts side and "failed" against a Rust side that
  // was right.
  const req = input instanceof globalThis.Request ? input : new globalThis.Request(input, init);
  return {
    method: req.method,
    path: new URL(req.url).pathname,
    headers: [...req.headers.entries()]
      .map(([k, v]) => `${k.toLowerCase()}: ${v}`)
      .sort(),
  };
}

function upstreamAnswer(input, init) {
  const described = describe(input, init);
  upstream = described;
  if (!(described.path in ANSWERS)) return new Response("not found", { status: 404 });
  return ANSWERS[described.path]();
}

const ANSWERS = {
  "/zen/go/v1/models": () =>
    new Response(JSON.stringify({ data: [{ id: "m" }] }), {
      status: 200,
      headers: { "content-type": "application/json" },
    }),
  "/zen/go/v1/responses": () =>
    new Response('data: {"a":1}\n\ndata: [DONE]\n\n', {
      status: 200,
      headers: { "content-type": "text/event-stream" },
    }),
  "/zen/go/v1/messages": () =>
    new Response(JSON.stringify({ content: [{ type: "text", text: "hi" }] }), {
      status: 200,
      headers: { "content-type": "application/json" },
    }),
};

/** A second table for the failure cases, so a 4xx and a 5xx can be exercised on the same path. */
let OVERRIDE = null;

globalThis.fetch = async (input, init) => {
  if (OVERRIDE) {
    const described = describe(input, init);
    upstream = described;
    return OVERRIDE(described.path);
  }
  return upstreamAnswer(input, init);
};

// ── the env both sides get ──────────────────────────────────────────────────────────────────────
const ENV = {
  CLIENT_KEY,
  OPENCODE_GO_API_KEY: WORKER_KEY,
};

const HOST = "https://zen.test";

// ── the cases ───────────────────────────────────────────────────────────────────────────────────
const CASES = [
  // The arms that need no upstream, whose bytes were already comparable before the entrypoint existed.
  { method: "OPTIONS", path: "/v1/messages", label: "the preflight" },
  { method: "OPTIONS", path: "/anything", label: "the preflight, on a path no route names" },
  { method: "GET", path: "/v1/models", label: "GET /v1/models with no x-api-key" },
  {
    method: "GET",
    path: "/v1/models",
    headers: { "x-api-key": "wrong-key-1234567890" },
    label: "GET /v1/models with the WRONG key",
  },
  { method: "POST", path: "/v1/responses", label: "POST /v1/responses with no Authorization" },
  {
    method: "POST",
    path: "/v1/responses",
    headers: { authorization: "Basic zzz" },
    label: "POST /v1/responses with a non-Bearer Authorization",
  },
  { method: "POST", path: "/v1/messages", label: "POST /v1/messages with no x-api-key" },
  { method: "GET", path: "/nope", label: "a path no route names" },

  // The upstream arms, now that they exist.
  {
    method: "GET",
    path: "/v1/models",
    headers: { "x-api-key": CLIENT_KEY },
    label: "GET /v1/models, gated and served",
  },
  {
    method: "POST",
    path: "/v1/responses",
    headers: {
      authorization: `Bearer ${CALLER_KEY}`,
      "content-type": "application/json",
      "x-opencode-session": "s-1",
    },
    body: '{"model":"m"}',
    label: "POST /v1/responses — BYOK, and the session header goes UPSTREAM",
  },
  {
    method: "POST",
    path: "/v1/responses",
    headers: { authorization: `Bearer ${CALLER_KEY}`, "x-session-id": "s-2" },
    body: "{}",
    label: "POST /v1/responses — the SECOND source header, when the first is absent",
  },
  {
    method: "POST",
    path: "/v1/responses",
    headers: { authorization: `Bearer ${CALLER_KEY}`, "x-opencode-session": "   " },
    body: "{}",
    label: "POST /v1/responses — a WHITESPACE-ONLY session header is not forwarded",
  },
  {
    method: "POST",
    path: "/v1/messages",
    headers: { "x-api-key": CLIENT_KEY },
    body: '{"model":"m"}',
    label: "POST /v1/messages, gated and served",
  },

  // The failure paths, where the provider's own words reach the client — and must not carry a key.
  {
    method: "POST",
    path: "/v1/messages",
    headers: { "x-api-key": CLIENT_KEY },
    body: "{}",
    override: (path) =>
      new Response(JSON.stringify({ error: { message: `bad key ${WORKER_KEY}` } }), {
        status: 400,
        headers: { "content-type": "application/json" },
      }),
    label: "a 4xx whose message ECHOES THE WORKER'S KEY — it must be redacted",
  },
  {
    method: "POST",
    path: "/v1/messages",
    headers: { "x-api-key": CLIENT_KEY },
    body: "{}",
    override: () =>
      new Response(JSON.stringify({ error: { message: "upstream exploded" } }), {
        status: 503,
        headers: { "content-type": "application/json" },
      }),
    label: "a 5xx — generic client text, the detail stays server-side",
  },
  {
    method: "POST",
    path: "/v1/responses",
    headers: { authorization: `Bearer ${CALLER_KEY}`, "x-api-key": "caller-api-key-1234" },
    body: "{}",
    override: () =>
      new Response(JSON.stringify({ message: "nope" }), {
        status: 429,
        headers: { "content-type": "application/json" },
      }),
    label: "a 429 on /responses — the `err.message` branch of the relay",
  },
  {
    method: "POST",
    path: "/v1/responses",
    headers: { authorization: `Bearer ${CALLER_KEY}` },
    body: "{}",
    override: () => new Response("not json at all", { status: 400 }),
    label: "a 4xx whose body is NOT JSON — the default message",
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
        `proxies/zen-us-proxy/src/index.js back from git history and run\n` +
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
    // The stub's answer is per-case, so it is set before EACH side runs.
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
    // **THE ROW THAT MAKES THE UPSTREAM VISIBLE.** A response-only comparison cannot see a header the
    // worker sends UPSTREAM — which is where the session id and both credentials live.
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
    ? `\nzen-us verify: ${CASES.length} case(s) byte-identical, upstream requests included`
    : `\nzen-us verify: ${bad} of ${CASES.length} case(s) DIFFER`,
);
process.exit(bad === 0 ? 0 : 1);
