// verify.mjs — run the BUILT Rust worker and the SHIPPING JavaScript on the same request, and
// compare the bytes. The criterion is P3's own: "同请求新旧响应字节可比" — not "it works", the bytes.
//
// ── THE MUTATION THAT MUST FAIL THIS CHECK ──────────────────────────────────────────────────────
// Read this when you change this file: the mutation is how you find out whether the check can still
// fail at all. A check that cannot be broken is worse than no check.
//
// MUTATION: in `src/lib.rs`, change the 503 detail on the manifest route from
//           "assets unavailable and no cached manifest" to "no manifest" and rebuild with
//           `worker-build --release`.
// RESULT:   exit 1 on the manifest cases, naming the body and both sides. (The other routes stay
//           green in the same run, which is the point: the failure is a FINDING about one route and
//           not a broken harness.)
//
// ── WHY THE LOADER IS REGISTERED FROM `gateway/wasm` AND NOT COPIED ─────────────────────────────
//
// `gateway/wasm/loader.mjs` answers exactly the two imports a workers-rs module has —
// `cloudflare:workers`' `WorkerEntrypoint`, and the `./index_bg.wasm` module import — and it is
// thirty lines of that. A second copy would be the shape this whole migration is deleting, so this
// file REGISTERS the existing one by relative path: one loader, two workers.
//
// ── WHAT THE TWO SIDES ARE ───────────────────────────────────────────────────────────────────────
//
//   old — `index/src/index.js` AS PUBLISHED, driven with stub bindings.
//   new — `build/index.js`, the module `worker-build --release` produced, executed through its real
//         `fetch` entrypoint with the same env and the same request URL.
//
// ── THE BINDING STUBS ARE DUCK-TYPED BY NAME, AND THE NAMES ARE NOT GUESSED ─────────────────────
//
// `worker-0.8.7/src/env.rs:148` reads `obj.constructor().name` and compares it to the binding's
// `TYPE_NAME`: `Fetcher` for the ASSETS binding and `R2Bucket` for the bucket. So the classes below
// are NAMED, and the naming is the whole contract — an anonymous class fails the cast with
// "Binding cannot be cast to the type Fetcher", which is the failure to expect if this drifts.
//
// ── WHAT IT DOES NOT COVER, STATED RATHER THAN IMPLIED ──────────────────────────────────────────
//
// **The cloudflared proxy.** It reaches the network through the runtime's global fetch, which is a
// WORKER-RUNTIME binding, not an `env` one — and the loader stubs the runtime with an empty class. So
// that route is checked here for its DECISION (it is the right route, and the request carries the
// pinned version) and not for its bytes, and the pinned-version test below is the part that matters:
// an unpinned proxy is the bug the worker's own comment describes at length.
import { register } from "node:module";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";

// ONE LOADER, TWO WORKERS — see the header.
register("../../gateway/wasm/loader.mjs", import.meta.url);

// The third runtime global the glue reaches for: it registers an `error` listener at module scope to
// reinitialise the wasm after a panic (the hook workers-rs 0.8.7 added).
globalThis.addEventListener ??= () => {};

const HERE = fileURLToPath(new URL(".", import.meta.url));
const BUILT = fileURLToPath(new URL("build/index.js", import.meta.url));

// ── build ───────────────────────────────────────────────────────────────────────────────────────
// **THIS HARNESS ASSUMED A PRE-BUILT `build/` AND NOTHING EVER MADE ONE.** Measured 2026-10-05, on a clean
// tree: `ENOENT: no such file or directory, open '…/index/worker/build/index_bg.wasm.mjs'` — the file it
// writes below, into a directory that did not exist. **IT AND `gateway/wasm/verify.mjs` BOTH HAD THIS**,
// which is why neither had ever run anywhere but its author's machine; the two satellite harnesses built
// first, and these do now.
if (!existsSync(BUILT) || process.argv.includes("--build")) {
  console.log("  building with worker-build --release …");
  execFileSync("worker-build", ["--release"], { cwd: HERE, stdio: "inherit" });
}

// The compiled module the glue expects Cloudflare's bundler to hand it (see the loader).
writeFileSync(
  `${HERE}build/index_bg.wasm.mjs`,
  `import { readFileSync } from "node:fs";\n` +
    `export default new WebAssembly.Module(readFileSync(new URL("./index_bg.wasm", import.meta.url)));\n`,
);

// ── the bindings ────────────────────────────────────────────────────────────────────────────────
const ASSET_BYTES = Buffer.from("ASSET-BYTES");
const R2_BYTES = Buffer.from("R2-BYTES");

class Fetcher {
  constructor(store) { this.store = store; }
  async fetch(url) {
    const path = new URL(typeof url === "string" ? url : url.url).pathname;
    if (this.store[path] === undefined) return new Response("not found", { status: 404 });
    return new Response(this.store[path], { status: 200 });
  }
  async fetch_request(request) { return this.fetch(request.url); }
}

class R2Bucket {
  constructor(store) { this.store = store; }
  // THE STUB'S SHAPE IS THE BINDING'S, NOT THE CALL SITE'S. `GetOptionsBuilder::execute` calls
  // `bucket.get(name, options)` and AWAITS the result — so `get` returns a PROMISE of the object
  // (or of `null` for an absent key), and the object is a REAL `Response` when a body is wanted.
  //
  // The first version modelled it as the chain the READING code uses — a `get` that answered a builder
  // with an `execute` on it — and every R2 route died with "e.then is not a function". The failure
  // names a promise, which is the right clue: the harness had the call chain right and the
  // CONVERSION wrong.
  get(key) {
    const { store } = this;
    return (async () => {
      const bytes = store[key];
      if (bytes === undefined) return null;
      // PROPERTIES, NOT METHODS, AND THAT IS THE THIRD SHAPE THIS STUB GOT WRONG. `Object::size()`
      // and `Object::http_etag()` are RUST accessors over the JS object's PROPERTIES, and the
      // JavaScript reads the same properties — so a `size: () => …` method made the zero-byte rule
      // read a function (never `=== 0`), and the previous plain-object version answered a 58-byte
      // body: **the source text of the function**, which is what a device would have written to disk.
      // `body` is a real `ReadableStream`, which is what R2's own object carries and what
      // `new Response(obj.body, …)` expects on the JavaScript side.
      const res = new Response(new Uint8Array(bytes), { status: 200 });
      return {
        bodyUsed: false,
        size: bytes.length,
        httpEtag: '"etag-' + key + '"',
        key: key,
        version: "stub",
        body: res.body,
      };
    })();
  }
}

const VERSION_JSON = (extra) => JSON.stringify({
  version: "1.2.297",
  sha256: "a".repeat(64),
  tarball: "summrise-agent-latest.tgz",
  ...extra,
});

const envFor = (manifest) => ({
  ASSETS: new Fetcher({
    "/summrise-agent/version.json": manifest,
    "/summrise-agent/summrise-agent-1.2.297.tgz": ASSET_BYTES,
    "/summrise-agent/summrise-agent-latest.tgz": ASSET_BYTES,
    "/summrise-agent/SummriseAgent-Setup.exe": ASSET_BYTES,
  }),
  TEMP_FILES: new R2Bucket({
    "electron-win32-x64.zip": R2_BYTES,
    "summrise-playwright.zip": R2_BYTES,
  }),
  CONSOLE_URL: "https://console.test",
});

// ── the old side: the JavaScript the CDN runs, FROM A FILE ────────────────────────────────────
// A `data:` URL cannot host it: `index/src/index.js` imports `./page.js`, and a data: URL has no
// hierarchy to resolve a relative specifier against ("Invalid relative URL or base scheme is not
// hierarchical"). **The first version did exactly that and the failure was a module-resolution
// error, not a worker error** — the same shape as the route differential's first attempt, and the
// same answer: import the real file, do not reconstruct it.
const { default: jsWorker } = await import(
  new URL("../src/index.js", import.meta.url).href
);

const HOST = "https://download.test";
const pairs = (h) => [...h.entries()].map(([k, v]) => `${k.toLowerCase()}: ${v}`).sort();

/** The response identity the comparison is made on: status, sorted headers, and the body BYTES. */
async function shape(resp) {
  const body = Buffer.from(await resp.arrayBuffer());
  return { status: resp.status, headers: pairs(resp.headers), bytes: body.length, body: body.toString("utf8") };
}

const CASES = [
  { path: "/api/version", manifest: VERSION_JSON({}), label: "the manifest: no installer published" },
  { path: "/api/version", manifest: VERSION_JSON({ installer: "SummriseAgent-Setup-1.2.297.exe", installer_sha256: "a".repeat(64) }), label: "the manifest: an installer IS published" },
  { path: "/api/version", manifest: null, label: "the manifest: no version.json at all" },
  { path: "/api/version", manifest: JSON.stringify({ version: "1.2.297", sha256: "abc" }), label: "the manifest: a digest agent_update would refuse" },
  { path: "/", manifest: VERSION_JSON({}), label: "the landing page, tgz-only" },
  { path: "/", manifest: VERSION_JSON({ installer: "SummriseAgent-Setup-1.2.297.exe", installer_sha256: "a".repeat(64) }), label: "the landing page, with the door" },
  { path: "/index.html", manifest: VERSION_JSON({}), label: "the landing page by its other name" },
  { path: "/summrise-agent/summrise-agent-1.2.297.tgz", manifest: VERSION_JSON({}), label: "the tarball (ASSETS pass-through)" },
  { path: "/summrise-agent/SummriseAgent-Setup.exe", manifest: VERSION_JSON({}), label: "the installer alias (ASSETS pass-through)" },
  { path: "/summrise-agent/electron-win32-x64.zip", manifest: VERSION_JSON({}), label: "the electron runtime (R2)" },
  { path: "/summrise-agent/summrise-playwright.zip", manifest: VERSION_JSON({}), label: "the playwright bundle (R2)" },
  { path: "/robots.txt", manifest: VERSION_JSON({}), label: "a path that names no artifact" },
  { path: "/summrise-agent/summrise-agent-1.2.tgz", manifest: VERSION_JSON({}), label: "a near miss on the version pattern" },
];

let bad = 0;
for (const [i, c] of CASES.entries()) {
  const env = envFor(c.manifest);
  const url = HOST + c.path;

  // The old side, AS PUBLISHED.
  const jsResp = await jsWorker.fetch(new Request(url), env);
  const jsShape = await shape(jsResp);

  // The new side: a FRESH module instance per case, because the Rust side has isolate state (the
  // arms and the manifest are read per request, and a fresh instance is also what a new isolate is).
  const mod = await import(`${pathToFileURL(BUILT).href}?case=${i}`);
  const instance = new mod.default();
  instance.env = env;
  instance.ctx = {};
  const rsResp = await instance.fetch(new Request(url));
  const rsShape = await shape(rsResp);

  const rows = [
    ["status", String(jsShape.status), String(rsShape.status)],
    ["headers", jsShape.headers.join(" | "), rsShape.headers.join(" | ")],
    ["body bytes", String(jsShape.bytes), String(rsShape.bytes)],
    ["body", jsShape.body, rsShape.body],
  ];
  const differs = rows.filter(([, want, got]) => want !== got).length;
  console.log(`  ${c.label}`);
  console.log(`      ${differs ? "FAIL" : "ok  "} ${url} — ${jsShape.bytes} bytes, status ${jsShape.status}`);
  if (differs) {
    bad++;
    for (const [what, want, got] of rows) {
      if (want === got) continue;
      console.log(`      ${what} DIFFERS`);
      console.log(`        ts  : ${JSON.stringify(want).slice(0, 300)}`);
      console.log(`        rust: ${JSON.stringify(got).slice(0, 300)}`);
    }
  } else {
    console.log(`      byte-identical (${jsShape.bytes} bytes), and so are status and headers`);
  }
}

// ── the one route whose BYTES cannot be compared here, and the part of it that can ────────────────
{
  const env = envFor(VERSION_JSON({}));
  const mod = await import(`${pathToFileURL(BUILT).href}?route=cloudflared`);
  const instance = new mod.default();
  instance.env = env;
  instance.ctx = {};
  const resp = await instance.fetch(new Request(`${HOST}/summrise-agent/cloudflared.exe`)).catch(
    (e) => ({ status: 0, _threw: String(e.message).slice(0, 80) })
  );
  // The DECISION is checked — it is the cloudflared route and not a 404 — and the PIN is checked in
  // the source, because a proxy that follows `latest` is the defect the worker's comment describes at
  // length and no byte comparison here would notice it.
  const src = readFileSync(new URL("src/worker.rs", import.meta.url), "utf8");
  const pinned = /const CLOUDFLARED_VERSION: &str = "(\d[\w.]*)"/.exec(src)?.[1];
  // THE PIN IS CHECKED IN THE URL, NOT IN A COMMENT. `src/worker.rs` QUOTES the old unpinned URL in
  // its docstring — "this used to proxy `.../releases/latest/download/…`" — so a grep for that path
  // finds the warning ABOUT the bug rather than the bug. The check is therefore: the upstream URL
  // INTERPOLATES the constant, and no line that BUILDS a URL says `latest`.
  const built = /releases\/download\/\{CLOUDFLARED_VERSION\}\//.test(src);
  const unpinned = /releases\/download\/latest\//.test(src);
  const latest = unpinned || !built;
  const ok = resp.status !== 404 && !!pinned && !latest;
  console.log("  the cloudflared proxy (decision only — its fetch is a RUNTIME binding, not an env one)");
  console.log(`      ${ok ? "ok  " : "FAIL"} answered ${resp.status ?? 0}, pinned to ${pinned ?? "(none)"}, an unpinned 'latest' URL in the build: ${unpinned}`);
  console.log(`      ${resp._threw ? `note: the byte path throws here as expected — ${resp._threw}` : ""}`);
  if (!ok) bad++;
}

// ── the bundle, measured here because it is the artifact the deploy would ship ────────────────────
const { execSync } = await import("node:child_process");
const gz = (p) => Number(execSync(`gzip -9 -n -c '${p}' | wc -c`, { encoding: "utf8" }).trim());
const jsRaw = readFileSync(`${HERE}build/index.js`).length;
const wasmRaw = readFileSync(`${HERE}build/index_bg.wasm`).length;
console.log("  the module worker-build produced");
console.log(`      index.js       ${String(jsRaw).padStart(8)} B  ${String(gz(`${HERE}build/index.js`)).padStart(8)} B gz`);
console.log(`      index_bg.wasm  ${String(wasmRaw).padStart(8)} B  ${String(gz(`${HERE}build/index_bg.wasm`)).padStart(8)} B gz`);
console.log(`      total          ${String(jsRaw + wasmRaw).padStart(8)} B  ${gz(`${HERE}build/index.js`) + gz(`${HERE}build/index_bg.wasm`)} B gz`);

if (bad) process.exit(1);
