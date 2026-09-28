#!/usr/bin/env node
// measure.mjs — the client half of the P3.0 cold-start instrument.
//
//   node measure.mjs --url=https://coldstart-spike-ts.zhengsaisi.workers.dev \
//                    --label=ts --out=out/ts.json
//
// WHAT IT MEASURES, and why each number is the one it claims to be:
//
//   * `ttfb` is time from writing the request to the first byte of the response
//     HEADERS (node:https `response` event), over a keep-alive socket. It is the
//     client-visible number and it carries this box's RTT to the Cloudflare edge.
//   * Every response reports `requestIndex` and `isolateAgeMs` from inside the
//     isolate. `requestIndex === 1` means the request CREATED that isolate, so
//     its latency is a cold start BY DEFINITION — no forced coldness, no guess.
//     Requests with `requestIndex > 1` are warm and are the baseline.
//   * `rid` is a unique marker per request so `wrangler tail --format json` lines
//     can be matched back to the request that produced them, which is how the
//     SERVER-side `cpuTime` / `wallTime` numbers are attached to a cold sample.
//
// It prints a JSON document; `report.mjs` is what turns that into prose.

import https from "node:https";
import fs from "node:fs";
import path from "node:path";

const argv = Object.fromEntries(
  process.argv.slice(2).map((a) => {
    const m = a.match(/^--([^=]+)=?(.*)$/);
    return m ? [m[1], m[2]] : [a, true];
  }),
);

const BASE = argv.url;
if (!BASE) throw new Error("--url is required");
const LABEL = argv.label ?? "unknown";
const WARM = Number(argv.warm ?? 250);
const BURST = Number(argv.burst ?? 40);
const BURSTS = Number(argv.bursts ?? 5);
const GAP = Number(argv.gap ?? 1500);
const WORK = Number(argv.work ?? 30);
const WORKN = Number(argv.workn ?? 300000);
const OUT = argv.out ?? `out/${LABEL}.json`;

const agent = new https.Agent({ keepAlive: true, maxSockets: 128, maxFreeSockets: 128 });

function once(rid, query) {
  return new Promise((resolve) => {
    const url = `${BASE}${query}&rid=${rid}`;
    const t0 = performance.now();
    const done = (o) => resolve({ rid, url, ...o });
    const req = https.get(url, { agent }, (res) => {
      const ttfb = performance.now() - t0;
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => {
        const total = performance.now() - t0;
        const text = Buffer.concat(chunks).toString("utf8");
        let body = null;
        try {
          body = JSON.parse(text);
        } catch {
          /* keep null: a body that is not JSON is itself the finding */
        }
        done({
          ttfb,
          total,
          status: res.statusCode,
          bytes: Buffer.byteLength(text),
          colo: String(res.headers["cf-ray"] ?? "").split("-").pop() || null,
          body,
          error: null,
        });
      });
    });
    req.on("error", (e) => {
      const t = performance.now() - t0;
      done({ ttfb: t, total: t, status: 0, bytes: 0, colo: null, body: null, error: String(e) });
    });
    req.setTimeout(30000, () => req.destroy(new Error("client timeout 30s")));
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function stats(xs) {
  if (!xs.length) return null;
  const s = [...xs].sort((a, b) => a - b);
  const at = (p) => s[Math.min(s.length - 1, Math.max(0, Math.ceil((p / 100) * s.length) - 1))];
  const mean = s.reduce((a, b) => a + b, 0) / s.length;
  return {
    n: s.length,
    min: s[0],
    p50: at(50),
    p90: at(90),
    p99: at(99),
    max: s[s.length - 1],
    mean,
    sd: Math.sqrt(s.reduce((a, b) => a + (b - mean) ** 2, 0) / s.length),
  };
}

const samples = [];
const t_start = Date.now();

// 1 — warm, sequential, on one keep-alive socket.
for (let i = 0; i < WARM; i++) samples.push(await once(`w${i}`, "/echo?msg=warm"));
await sleep(GAP);

// 2 — bursts. N concurrent requests cannot be served by one isolate (an isolate
// serves one request at a time), so each burst is where new isolates are born —
// and every `requestIndex === 1` in the results is a cold start that happened.
const bursts = [];
for (let j = 0; j < BURSTS; j++) {
  const batch = await Promise.all(
    Array.from({ length: BURST }, (_, i) => once(`b${j}-${i}`, "/echo?msg=burst")),
  );
  samples.push(...batch);
  bursts.push({
    j,
    cold: batch.filter((s) => s.body?.requestIndex === 1).length,
    isolates: new Set(batch.map((s) => s.body?.isolateId)).size,
    p50: stats(batch.map((s) => s.ttfb))?.p50,
    max: stats(batch.map((s) => s.ttfb))?.max,
  });
  await sleep(GAP);
}

// 3 — the CPU route: the same integer loop in both workers.
for (let i = 0; i < WORK; i++) samples.push(await once(`k${i}`, `/work?n=${WORKN}`));

const echo = samples.filter((s) => s.url.includes("/echo"));
const work = samples.filter((s) => s.url.includes("/work"));
const cold = echo.filter((s) => s.body?.requestIndex === 1);
const warm = echo.filter((s) => s.body?.requestIndex > 1);

const result = {
  label: LABEL,
  base: BASE,
  startedAt: new Date(t_start).toISOString(),
  params: { WARM, BURST, BURSTS, GAP, WORK, WORKN },
  isolates: {
    distinct: new Set(echo.map((s) => s.body?.isolateId).filter(Boolean)).size,
    coldRequests: cold.length,
    maxRequestIndex: Math.max(0, ...echo.map((s) => s.body?.requestIndex ?? 0)),
  },
  colos: [...new Set(samples.map((s) => s.colo).filter(Boolean))],
  stats: {
    warm_ttfb: stats(warm.map((s) => s.ttfb)),
    warm_total: stats(warm.map((s) => s.total)),
    cold_ttfb: stats(cold.map((s) => s.ttfb)),
    cold_total: stats(cold.map((s) => s.total)),
    cold_isolateAgeMs_at_response: stats(cold.map((s) => s.body?.isolateAgeMs).filter((x) => x != null)),
    cold_handlerMs: stats(cold.map((s) => s.body?.handlerMs).filter((x) => x != null)),
    warm_handlerMs: stats(warm.map((s) => s.body?.handlerMs).filter((x) => x != null)),
    work_ttfb: stats(work.map((s) => s.ttfb)),
    work_handlerMs: stats(work.map((s) => s.body?.handlerMs).filter((x) => x != null)),
    work_acc: [...new Set(work.map((s) => s.body?.acc))],
  },
  bursts,
  errors: samples.filter((s) => s.error).map((s) => ({ rid: s.rid, error: s.error })),
  samples,
};

fs.mkdirSync(path.dirname(OUT), { recursive: true });
fs.writeFileSync(OUT, JSON.stringify(result, null, 1));
const r = (s) => (s ? `n=${s.n} p50=${s.p50.toFixed(1)} p90=${s.p90.toFixed(1)} p99=${s.p99.toFixed(1)} max=${s.max.toFixed(1)}` : "none");
console.log(`[${LABEL}] isolates=${result.isolates.distinct} cold=${cold.length} colos=${result.colos.join(",")}`);
console.log(`[${LABEL}] warm  ${r(result.stats.warm_ttfb)}`);
console.log(`[${LABEL}] cold  ${r(result.stats.cold_ttfb)}`);
console.log(`[${LABEL}] work  ${r(result.stats.work_ttfb)} handlerMs p50=${result.stats.work_handlerMs?.p50}`);
console.log(`[${LABEL}] acc=${JSON.stringify(result.stats.work_acc)} errors=${result.errors.length}`);
console.log(`[${LABEL}] -> ${OUT}`);
