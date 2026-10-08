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
const CASES = JSON.parse(readFileSync(join(HERE, "worker-cases.json"), "utf8"));

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
    const resp = await worker.fetch(buildRequest(CASES.host, c), env);
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
    try {
      return { "relay/src/index.js": execFileSync("git", ["-C", REPO, "hash-object", "relay/src/index.js"], { encoding: "utf8" }).trim() };
    } catch {
      return { "relay/src/index.js": "(not a git checkout)" };
    }
  })(),
  host: CASES.host,
  cases,
};

const out = join(HERE, "worker-corpus.json");
const text = JSON.stringify(corpus, null, 2) + "\n";
if (process.argv.includes("--check")) {
  const before = readFileSync(out, "utf8");
  if (before !== text) {
    console.error("worker-corpus.json DIFFERS from what the shipping JavaScript answers now.");
    process.exit(1);
  }
  console.log(`worker-corpus.json matches the shipping JavaScript (${cases.length} case(s))`);
} else {
  writeFileSync(out, text);
  console.log(
    `recorded ${cases.length} case(s): ${cases.filter((c) => c.r2.length).length} touched R2, ` +
      `${cases.filter((c) => c.forwarded.length).length} reached the claim DO`,
  );
}
