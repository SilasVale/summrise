// record-worker.mjs — RECORD WHAT THE SHIPPING FILE RELAY ANSWERS, over the whole `/files/*` surface.
//
// ── WHY THIS FILE IS JAVASCRIPT, AND WHY IT IS TEMPORARY ────────────────────────────────────────
//
// The oracle IS JavaScript: `relay/src/index.js` is the worker serving the route, and the only way to
// know what it answers is to run it. This script drives it with the SAME stubs and the SAME cases the
// Rust port is driven with (`bindings-stub.mjs` and `worker-cases.json`), and writes `worker-corpus.json`;
// `tests/worker_differential.rs` then drives the built Rust worker through
// `gateway/wasm/run-cases.mjs` and compares the two.
//
// **IT RETIRES WITH THE JAVASCRIPT IT DRIVES** — when the cutover deletes `relay/src/index.js`, the corpus
// is the record, exactly as `shipping-answers.json` became for the satellites. Until then this is how the
// corpus is re-recorded after a deliberate change, and `--check` is how CI sees a drift.
//
// Usage: node record-worker.mjs            (writes worker-corpus.json beside this file)
//        node record-worker.mjs --check    (records and diffs; exit 1 on any difference)
import { readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { buildRequest, installBindings } from "../../gateway/wasm/bindings-stub.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, "..", "..");
// **ONE RECORDER, TWO SURFACES**: the default names the worker's cases, and `--cases claim-cases.json`
// records the claim Durable Object's — the corpus is named after the case file.
const argOf = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i === -1 ? fallback : process.argv[i + 1];
};
const CASES_FILE = argOf("cases", "worker-cases.json");
const CORPUS_FILE = CASES_FILE.replace("-cases.json", "-corpus.json");
const CASES = JSON.parse(readFileSync(join(HERE, CASES_FILE), "utf8"));

const worker = (await import("../src/index.js")).default;

/** The response identity the comparison is made on: status, sorted headers, and the body BYTES. */
async function shape(resp) {
  const body = Buffer.from(await resp.arrayBuffer());
  return {
    status: resp.status,
    headers: [...resp.headers.entries()].map(([k, v]) => `${k.toLowerCase()}: ${v}`).sort(),
    bytes: body.length,
    body: body.toString("utf8"),
  };
}

async function run() {
  const observed = [];
  for (const c of CASES.cases) {
    // **A FRESH WORLD PER CASE**, because a case's R2 objects and its DO answer are part of what it
    // asserts — and because the globals this replaces (the clock, the randomness) must not leak a
    // previous case's stream into this one.
    const spec = {
      host: CASES.host,
      env: { ...CASES.env, ...(c.env || {}) },
      bindings: { ...(c.bindings || {}), tokenBytes: c.tokenBytes ?? null },
    };
    const { env, log } = installBindings(spec);
    for (const [k, v] of Object.entries(spec.env)) env[k] = v;
    // The worker's entry takes no constructor arguments and is handed its `env`; a named class (the claim
    // object) takes `(state, env)` — the same two shapes `run-cases.mjs` drives.
    const req = buildRequest(CASES.host, c);
    let resp;
    if (c.entry || CASES.entry) {
      const Entry = (await import("../src/claim.js"))[c.entry || CASES.entry];
      resp = await new Entry(c.entryState ?? {}, env).fetch(req);
    } else {
      resp = await worker.fetch(req, env);
    }
    observed.push({
      label: c.label,
      method: c.method,
      path: c.path,
      ...(await shape(resp)),
      r2: log.r2,
      forwarded: log.forwarded,
    });
  }
  return observed;
}

const cases = await run();
const corpus = {
  // The blob this was recorded from, so a reader can tell whether it still describes the file in front
  // of them. `git hash-object` on the WORKING TREE, which is what was actually run.
  source: (() => {
    const files = CASES.entry || argOf("cases", "") === "claim-cases.json" ? ["relay/src/claim.js"] : ["relay/src/index.js"];
    const out = {};
    for (const f of files) {
      try {
        out[f] = execFileSync("git", ["-C", REPO, "hash-object", f], { encoding: "utf8" }).trim();
      } catch {
        out[f] = "(not a git checkout)";
      }
    }
    return out;
  })(),
  host: CASES.host,
  cases,
};

const out = join(HERE, CORPUS_FILE);
const text = JSON.stringify(corpus, null, 2) + "\n";
if (process.argv.includes("--check")) {
  const before = readFileSync(out, "utf8");
  if (before !== text) {
    console.error(`${CORPUS_FILE} DIFFERS from what the shipping JavaScript answers now.`);
    process.exit(1);
  }
  console.log(`${CORPUS_FILE} matches the shipping JavaScript (${cases.length} case(s))`);
} else {
  writeFileSync(out, text);
  console.log(
    `recorded ${cases.length} case(s): ${cases.filter((c) => c.r2.length).length} touched R2, ` +
      `${cases.filter((c) => c.forwarded.length).length} reached the claim DO`,
  );
}
