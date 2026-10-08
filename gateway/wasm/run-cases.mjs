// run-cases.mjs — EXECUTE A BUILT WORKER MODULE OVER A CASE LIST AND PRINT WHAT IT SAW.
//
// ── WHY THIS FILE IS JAVASCRIPT, SINCE THE MIGRATION IS MOVING THINGS TO RUST ────────────────────
//
// It runs `worker-build`'s output: `build/index.js` (the wasm-bindgen glue) plus `index_bg.wasm`, through
// the module's real `fetch` entrypoint. That glue speaks wasm-bindgen's JS-side contract and imports
// `cloudflare:workers`; the only runtime that can execute it is a JS engine. **THE DECISIONS ARE NOT
// HERE** — the case list, the upstream answers, the expected bytes and the comparison all live in the
// Rust judges (`proxies/zen-{us,go}-proxy/worker/tests/differential.rs`), which pass this file a spec and
// read the JSON it prints. This is the "spawn stays JS, decision moves to Rust" shape the plan settled on
// for gates that must drive a JS artifact.
//
// Usage: node run-cases.mjs --module <build/index.js> --spec <spec.json>
//
// spec.json:
//   {
//     "host":  "https://zen.test",
//     "env":   { … what the worker's `env` binding gets … },
//     "answers": { "<upstream path>": { "status": 200, "headers": {…}, "body": "…" }, … },
//     "cases": [ { "label", "method", "path", "headers"?, "body"?, "override"?: {status,headers,body} } ]
//   }
//
// stdout (JSON, one line): { "observed": [ { label, status, headers[], bytes, body,
//                                              upstream: { method, path, headers[] } | null } ] }
//
// `answers` is the upstream's answer PER PATH — the table the shipping harness had — and a case's
// `override` (same shape) takes precedence for that case, which is how a 4xx and a 5xx are exercised on
// one path. `upstream` is the request the worker actually made, which is the only place the session
// header and the credentials are visible.
import { readFileSync, writeFileSync } from "node:fs";
import { register } from "node:module";
import { dirname, join } from "node:path";
import { pathToFileURL } from "node:url";

const argv = process.argv.slice(2);
const arg = (name) => {
  const i = argv.indexOf(`--${name}`);
  return i === -1 ? null : argv[i + 1];
};
const MODULE = arg("module");
const SPEC_PATH = arg("spec");
if (!MODULE || !SPEC_PATH) {
  console.error("usage: node run-cases.mjs --module <build/index.js> --spec <spec.json>");
  process.exit(2);
}
const SPEC = JSON.parse(readFileSync(SPEC_PATH, "utf8"));
const HOST = SPEC.host;
const HERE = dirname(MODULE);

// ── ONE WORKER GLOBAL NODE DOES NOT HAVE ────────────────────────────────────────────────────────
// The glue calls `addEventListener("error", …)` at MODULE LOAD. Every harness in this repository carries
// the same line for the same reason; it is the ONLY runtime global they need.
globalThis.addEventListener ??= () => {};

// ── AND ONE PLACE WHERE NODE IS STRICTER THAN THE RUNTIME ───────────────────────────────────────
// `new Request(url, { body })` in Node throws `duplex option is required when sending a body`; a Worker
// runtime does not ask. The built module hits this through `web_sys::Request`, which is this global, so
// the port's upstream call would die on a rule its runtime does not have. **A HARNESS ACCOMMODATION,
// NOT A FIX TO THE PORT** — named rather than hidden, because a shim that is not written down is a
// difference between the tested artifact and the shipped one.
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
// It answers exactly the two imports a workers-rs module has (`cloudflare:workers` and the `.wasm`
// module). One loader, every worker: a second copy would be the shape this migration is deleting.
register(new URL("./loader.mjs", import.meta.url));

// The compiled module the glue expects Cloudflare's bundler to hand it. **`index_bg.wasm`, NOT
// `<crate>_bg.wasm`** — `worker-build` names the entry `index.js` and the module beside it
// `index_bg.wasm` whatever the crate is called, and the loader appends `.mjs` to whatever the glue
// imported.
const WASM = join(HERE, "index_bg.wasm");
writeFileSync(
  `${WASM}.mjs`,
  `import { readFileSync } from "node:fs";\n` +
    `export default new WebAssembly.Module(readFileSync(new URL("./index_bg.wasm", import.meta.url)));\n`,
);

// ── the upstream stub, and the record of what was asked ─────────────────────────────────────────
/** The request the worker made, as the comparison sees it. */
let upstream = null;

/**
 * **THE TWO SHAPES A CALLER MAY USE, AND THE TWO SIDES USE A DIFFERENT ONE.** The shipping worker called
 * `fetch(url, init)`; workers-rs's `Fetch::Request(req).send()` hands the global `fetch` a REQUEST OBJECT
 * and no init. Reading only `init?.method`/`init?.headers` recorded every Rust upstream call as `GET`
 * with no headers — a difference in the harness that looked exactly like a difference in the port.
 */
function describe(input, init) {
  // **THE SHIMMED `Request`, NOT `RealRequest`** — the shipping worker passed the INCOMING REQUEST'S BODY
  // STREAM as `init.body`, and constructing that with the unshimmed class throws the same `duplex` error
  // the shim exists for.
  const req = input instanceof globalThis.Request ? input : new globalThis.Request(input, init);
  return {
    method: req.method,
    path: new URL(req.url).pathname,
    headers: [...req.headers.entries()]
      .map(([k, v]) => `${k.toLowerCase()}: ${v}`)
      .sort(),
  };
}

const asResponse = (answer) =>
  new Response(answer.body, { status: answer.status, headers: answer.headers || {} });

/** The case being run, so the stub knows whether an `override` applies. */
let CURRENT = null;

globalThis.fetch = async (input, init) => {
  const described = describe(input, init);
  upstream = described;
  if (CURRENT?.override) return asResponse(CURRENT.override);
  const answer = SPEC.answers?.[described.path];
  if (!answer) return new Response("not found", { status: 404 });
  return asResponse(answer);
};

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

const observed = [];
for (const [i, c] of SPEC.cases.entries()) {
  const url = HOST + c.path;
  const init = { method: c.method, headers: c.headers || {} };
  if (c.body !== undefined) init.body = c.body;

  CURRENT = c;
  upstream = null;
  // A FRESH module instance per case: the worker reads its env per request, and a fresh instance is also
  // what a new isolate is.
  const mod = await import(`${pathToFileURL(MODULE).href}?case=${i}`);
  const instance = new mod.default();
  instance.env = SPEC.env || {};
  instance.ctx = {};
  const resp = await instance.fetch(new Request(url, init));
  observed.push({ label: c.label, ...(await shape(resp)), upstream });
}

process.stdout.write(`${JSON.stringify({ observed })}\n`);
