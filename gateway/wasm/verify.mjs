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
import { readFileSync, writeFileSync } from "node:fs";
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
