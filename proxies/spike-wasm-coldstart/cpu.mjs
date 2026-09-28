#!/usr/bin/env node
// cpu.mjs — CPU time per request, from Cloudflare's own analytics, matched to
// the requests this measurement actually made.
//
//   CLOUDFLARE_API_TOKEN=... node cpu.mjs
//
// WHY NOT `wrangler tail`: its websocket (tail.developers.workers.dev) is TLS-
// reset from this network — measured, ECONNRESET. The Workers dashboard's own
// numbers come from this GraphQL dataset, which is reachable.
//
// HOW A BUCKET IS CLASSIFIED — by the traffic that made it, from the local
// driver responses (`out/*.json`, each carrying its UTC `stamp`):
//   COLD  a bucket with exactly 1 request, inside one of the two cold-round
//         windows. Every cold round asked each worker exactly once, so a
//         1-request bucket in those windows IS that round.
//   WARM  a bucket with >= 10 requests in the warm-run window: those runs did
//         20 warm requests + 1 work request per worker, and every sample in
//         them reported requestIndex > 1 (the isolate had already served one).
// The windows are printed with the numbers so the classification can be checked
// rather than believed.

const ACCOUNT = "8e9ea6cb01f2336a2f00039a51f96c6d";
const token = process.env.CLOUDFLARE_API_TOKEN;
if (!token) throw new Error("CLOUDFLARE_API_TOKEN is required");

const WINDOWS = {
  cold: [
    ["2026-09-28T13:18:00Z", "2026-09-28T13:20:40Z"], // rounds 1-8
    ["2026-09-28T13:21:20Z", "2026-09-28T13:24:10Z"], // rounds 9-16 (stamps in out/cold-r*.json)
  ],
  warm: [["2026-09-28T13:20:30Z", "2026-09-28T13:20:50Z"]], // 12 warm runs, 21 requests each
};

const graphql = async (scriptName, from, to) => {
  const query = `query ($tag: String!, $name: String!, $from: Time!, $to: Time!) {
    viewer { accounts(filter: { accountTag: $tag }) {
      workersInvocationsAdaptive(limit: 1000, filter: { scriptName: $name, datetime_geq: $from, datetime_leq: $to }, orderBy: [datetime_ASC]) {
        sum { requests errors }
        quantiles { cpuTimeP50 cpuTimeP90 cpuTimeP99 wallTimeP50 wallTimeP90 wallTimeP99 }
        dimensions { datetime }
      } } } }`;
  const res = await fetch("https://api.cloudflare.com/client/v4/graphql", {
    method: "POST",
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: JSON.stringify({ query, variables: { tag: ACCOUNT, name: scriptName, from, to } }),
  });
  const json = await res.json();
  if (json.errors) throw new Error(JSON.stringify(json.errors).slice(0, 400));
  return json.data.viewer.accounts[0].workersInvocationsAdaptive ?? [];
};

const inWindow = (t, list) => list.some(([a, b]) => t >= Date.parse(a) && t <= Date.parse(b));
const mean = (xs) => (xs.length ? Math.round(xs.reduce((a, b) => a + b, 0) / xs.length) : null);
const med = (xs) => {
  if (!xs.length) return null;
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
};

const out = { windows: WINDOWS, workers: {} };
for (const [label, script] of [
  ["ts", "coldstart-spike-ts"],
  ["rust", "coldstart-spike-rust"],
]) {
  const buckets = await graphql(script, "2026-09-28T13:00:00Z", "2026-09-28T13:40:00Z");
  const rows = buckets.map((b) => ({
    at: b.dimensions.datetime,
    t: Date.parse(b.dimensions.datetime),
    requests: b.sum.requests,
    errors: b.sum.errors,
    cpuP50: b.quantiles.cpuTimeP50,
    cpuP90: b.quantiles.cpuTimeP90,
    cpuP99: b.quantiles.cpuTimeP99,
    wallP50: b.quantiles.wallTimeP50,
    wallP90: b.quantiles.wallTimeP90,
    wallP99: b.quantiles.wallTimeP99,
  }));
  const cold = rows.filter((r) => r.requests === 1 && inWindow(r.t, WINDOWS.cold));
  const warm = rows.filter((r) => r.requests >= 10 && r.errors === 0 && inWindow(r.t, WINDOWS.warm));
  out.workers[label] = {
    script,
    cold: {
      rounds: cold.length,
      cpuTimeP50_us: { values: cold.map((r) => r.cpuP50), mean: mean(cold.map((r) => r.cpuP50)), median: med(cold.map((r) => r.cpuP50)) },
      wallTimeP50_us: { values: cold.map((r) => r.wallP50), mean: mean(cold.map((r) => r.wallP50)), median: med(cold.map((r) => r.wallP50)) },
      rows: cold,
    },
    warm: {
      buckets: warm.length,
      requests: warm.reduce((a, r) => a + r.requests, 0),
      cpuTimeP50_us: { values: warm.map((r) => r.cpuP50), mean: mean(warm.map((r) => r.cpuP50)), median: med(warm.map((r) => r.cpuP50)) },
      cpuTimeP99_us: { values: warm.map((r) => r.cpuP99), min: Math.min(...warm.map((r) => r.cpuP99)), max: Math.max(...warm.map((r) => r.cpuP99)) },
      wallTimeP50_us: { values: warm.map((r) => r.wallP50), mean: mean(warm.map((r) => r.wallP50)) },
      rows: warm,
    },
  };
}
console.log(JSON.stringify(out, null, 1));
