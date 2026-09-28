#!/usr/bin/env node
// analyze.mjs — every sample, pooled and classified, with the class decided by
// the isolate itself.
//
//   CLOUDFLARE_API_TOKEN=... node analyze.mjs
//
// Source of truth is the KV namespace: the driver stores EVERY run it makes
// (`manual/<iso>` for the ones this box asked for, `run/<iso>` for the cron's),
// so nothing here depends on a log having been captured live — which matters,
// because `wrangler tail` cannot run on this box (its websocket is reset).
//
// The classification is the whole point:
//   cold  = the response says `requestIndex === 1`, i.e. THIS request created
//           the isolate. Not a proxy for coldness — a statement of it.
//   warm  = `requestIndex > 1`, i.e. the isolate had already served a request.
// A sample whose body never arrived is counted as an error, never as a timing.

const ACCOUNT = "8e9ea6cb01f2336a2f00039a51f96c6d";
const NS = "6c8bcaff0e7f4f929cebcdd80032da47";
const token = process.env.CLOUDFLARE_API_TOKEN;
if (!token) throw new Error("CLOUDFLARE_API_TOKEN is required");

const api = async (path) => {
  const r = await fetch(`https://api.cloudflare.com/client/v4${path}`, { headers: { Authorization: `Bearer ${token}` } });
  const j = await r.json();
  if (!j.success) throw new Error(`${path} -> ${JSON.stringify(j.errors)}`);
  return j.result;
};

// The /values/ endpoint answers with the RAW value, not the {success,result}
// envelope the rest of the API uses — measured by this script failing on
// `j.success` being undefined.
const value = async (key) => {
  const r = await fetch(
    `https://api.cloudflare.com/client/v4/accounts/${ACCOUNT}/storage/kv/namespaces/${NS}/values/${encodeURIComponent(key)}`,
    { headers: { Authorization: `Bearer ${token}` } },
  );
  if (!r.ok) throw new Error(`value ${key} -> HTTP ${r.status}`);
  return r.text();
};

function stats(xs) {
  if (!xs.length) return null;
  const s = [...xs].sort((a, b) => a - b);
  const at = (p) => s[Math.min(s.length - 1, Math.max(0, Math.ceil((p / 100) * s.length) - 1))];
  const mean = s.reduce((a, b) => a + b, 0) / s.length;
  return {
    n: s.length,
    min: +s[0].toFixed(1),
    p50: +at(50).toFixed(1),
    p90: +at(90).toFixed(1),
    p99: +at(99).toFixed(1),
    max: +s[s.length - 1].toFixed(1),
    mean: +mean.toFixed(1),
    sd: +Math.sqrt(s.reduce((a, b) => a + (b - mean) ** 2, 0) / s.length).toFixed(1),
  };
}

const keys = (await api(`/accounts/${ACCOUNT}/storage/kv/namespaces/${NS}/keys?limit=1000`)).map((k) => k.name);
const runs = [];
for (const key of keys) {
  if (!/^(manual|run)\//.test(key)) continue;
  const parsed = JSON.parse(await value(key));
  runs.push({ key, kind: key.startsWith("run/") ? "cron" : "manual", ...parsed });
}

const rows = [];
for (const run of runs) {
  for (const r of run.runs ?? []) {
    for (const s of r.samples ?? []) {
      rows.push({
        run: run.key,
        kind: run.kind,
        stamp: run.stamp,
        tag: run.tag ?? null,
        selfColo: run.selfColo ?? null,
        label: r.label,
        ms: s.ms,
        status: s.status,
        colo: s.colo,
        requestIndex: s.body?.requestIndex ?? null,
        handlerMs: s.body?.handlerMs ?? null,
        isolateId: s.body?.isolateId ?? null,
        acc: s.body?.acc ?? null,
        isWork: s.rid?.includes("-k"),
        error: s.error ?? null,
      });
    }
  }
}

const by = (f) => rows.filter(f);
const out = { generatedAt: new Date().toISOString(), kvKeys: keys.length, runs: runs.length, rows: rows.length, workers: {} };

for (const label of ["ts", "rust"]) {
  const mine = by((r) => r.label === label);
  const echo = mine.filter((r) => !r.isWork && r.status === 200 && r.requestIndex != null);
  const cold = echo.filter((r) => r.requestIndex === 1);
  const warm = echo.filter((r) => r.requestIndex > 1);
  const work = mine.filter((r) => r.isWork && r.status === 200);
  const manual = (xs) => xs.filter((r) => r.kind === "manual");
  out.workers[label] = {
    samples: mine.length,
    errors: mine.filter((r) => r.status !== 200).length,
    colos: [...new Set(mine.map((r) => r.colo).filter(Boolean))],
    cold_ms: stats(cold.map((r) => r.ms)),
    cold_ms_manual: stats(manual(cold).map((r) => r.ms)),
    warm_ms: stats(warm.map((r) => r.ms)),
    warm_ms_manual: stats(manual(warm).map((r) => r.ms)),
    cold_handlerMs: stats(cold.map((r) => r.handlerMs).filter((x) => x != null)),
    warm_handlerMs: stats(warm.map((r) => r.handlerMs).filter((x) => x != null)),
    work_handlerMs: stats(work.map((r) => r.handlerMs).filter((x) => x != null)),
    work_ms: stats(work.map((r) => r.ms)),
    work_acc: [...new Set(work.map((r) => r.acc))],
    distinctIsolates: new Set(echo.map((r) => r.isolateId).filter(Boolean)).size,
  };
}

// Paired comparison: a cold round asks BOTH workers once, from the same driver
// isolate, in the same colo, seconds apart. Pairing removes everything the two
// samples have in common (connection setup, colo, time of day) and leaves the
// difference between the two runtimes.
const coldRounds = runs.filter((r) => r.kind === "manual" && (r.tag ?? "").startsWith("cold-r"));
const pairs = [];
for (const run of coldRounds) {
  const ts = run.runs.find((r) => r.label === "ts")?.samples?.[0];
  const rust = run.runs.find((r) => r.label === "rust")?.samples?.[0];
  if (ts?.body && rust?.body) {
    pairs.push({
      tag: run.tag,
      colo: run.selfColo,
      ts: ts.ms,
      rust: rust.ms,
      diff: +(ts.ms - rust.ms).toFixed(1),
      tsIndex: ts.body.requestIndex,
      rustIndex: rust.body.requestIndex,
    });
  }
}
out.coldPairs = {
  n: pairs.length,
  ts_ms: stats(pairs.map((p) => p.ts)),
  rust_ms: stats(pairs.map((p) => p.rust)),
  ts_minus_rust: stats(pairs.map((p) => p.diff)),
  allCold: pairs.every((p) => p.tsIndex === 1 && p.rustIndex === 1),
  rows: pairs,
};

console.log(JSON.stringify(out, null, 1));
