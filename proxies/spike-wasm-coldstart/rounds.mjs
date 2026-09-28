#!/usr/bin/env node
// rounds.mjs — the measurement protocol, in the order the question asks for it.
//
//   node rounds.mjs            cold rounds + warm runs (the whole thing)
//   node rounds.mjs --warm     only the warm runs
//   node rounds.mjs --cold     only the cold rounds
//
// COLD, and how it is forced — there is no "force a cold isolate" button:
//   * A new DEPLOYMENT is a new script version, and a version's isolate pool
//     starts empty in every colo. So: `wrangler deploy`, wait 3s, then ONE
//     request. That request created the isolate — it says so itself
//     (`requestIndex: 1`) — and it is the coldest request there is.
//   * In the same invocation the OTHER worker is asked once as well. It was not
//     redeployed, so its isolate in that colo is usually already warm
//     (`requestIndex > 1`), which makes it a PAIRED CONTROL: both samples pay
//     the same connection setup from the same driver isolate in the same colo,
//     so `cold - control` is the cold-start cost with the network cancelled.
//   * Rounds alternate which worker is redeployed, so both get equal treatment.
//
// WARM: `warm=20&work=1` is 42 subrequests, under the 50-per-invocation limit
// this account has (measured: 49 succeeded, #50 failed with "Too many
// subrequests by single Worker invocation"). Repeated enough times to pass 200
// samples per worker; every sample is classified by the isolate's own
// `requestIndex`, so a run that lands in a fresh colo cannot pass a cold sample
// off as warm.

import { execFileSync } from "node:child_process";
import fs from "node:fs";

const DRIVER = "https://coldstart.saisi.online/driver/run";
const OUT = new URL("./out/", import.meta.url).pathname;
const TOKEN = fs.readFileSync(`${process.env.HOME}/.cloudflare-token`, "utf8").trim();
const CARGO = `${process.env.HOME}/.cargo/bin`;
fs.mkdirSync(OUT, { recursive: true });

const args = process.argv.slice(2);
const doCold = args.length === 0 || args.includes("--cold");
const doWarm = args.length === 0 || args.includes("--warm");

async function call(query, file) {
  const res = await fetch(`${DRIVER}?${query}`, { signal: AbortSignal.timeout(180000) });
  const text = await res.text();
  if (!res.ok) throw new Error(`driver ${res.status}: ${text.slice(0, 300)}`);
  const json = JSON.parse(text);
  fs.writeFileSync(`${OUT}${file}`, JSON.stringify(json, null, 1));
  const line = json.runs
    .map((r) => {
      const c = r.stats?.cold_ms;
      const w = r.stats?.warm_ms;
      return `${r.label}[iso=${r.isolates} cold=${c ? `${c.n}:${c.min.toFixed(0)}-${c.max.toFixed(0)}` : "-"} warm=${w ? `${w.n}:p50=${w.p50.toFixed(1)}` : "-"}]`;
    })
    .join(" ");
  console.log(`${file} colo=${json.selfColo} tag=${json.tag ?? "-"} ${line}`);
  return json;
}

function deploy(which) {
  const dir = `${new URL("./", import.meta.url).pathname}${which}`;
  const env = {
    ...process.env,
    CLOUDFLARE_API_TOKEN: TOKEN,
    PATH: `${CARGO}:${process.env.PATH}`,
    WASM_BINDGEN_BIN: `${CARGO}/wasm-bindgen`,
    WASM_OPT_BIN: "/tmp/wasmopt/node_modules/binaryen/bin/wasm-opt",
  };
  const out = execFileSync("wrangler", ["deploy"], { cwd: dir, env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  const version = /Current Version ID: (\S+)/.exec(out)?.[1] ?? "?";
  const startup = /Worker Startup Time: (\d+) ms/.exec(out)?.[1] ?? null;
  console.log(`  deployed ${which} version=${version} startup=${startup ?? "not reported"}`);
  return { version, startup };
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

if (doCold) {
  const ROUNDS = Number(process.env.ROUNDS ?? 8);
  for (let r = 1; r <= ROUNDS; r++) {
    const which = r % 2 === 1 ? "ts" : "rust";
    console.log(`cold round ${r}/${ROUNDS} (redeploying ${which})`);
    const info = deploy(which);
    await sleep(3000);
    const json = await call(`warm=0&bursts=1&burst=1&work=0&tag=cold-r${r}-${which}-v${info.version}`, `cold-r${r}.json`);
    json.runs.forEach((run) => {
      const s = run.samples[0];
      console.log(`    ${run.label} ms=${s.ms.toFixed(1)} requestIndex=${s.body?.requestIndex} handlerMs=${s.body?.handlerMs} colo=${s.colo} status=${s.status}`);
    });
  }
}

if (doWarm) {
  const RUNS = Number(process.env.WARMRUNS ?? 12);
  for (let i = 1; i <= RUNS; i++) {
    await call(`warm=20&bursts=0&work=1&tag=warm-${i}`, `warm-${i}.json`);
  }
}
