// The traffic source for the P3.0 measurement, running INSIDE Cloudflare.
//
// WHY THIS EXISTS — every other trigger is closed on this box, each measured:
//   * `*.workers.dev` is TLS-reset (SNI blocked) from here AND from the devices,
//     so no client on this network can reach the two throwaway workers;
//   * `wrangler tail` dies with ECONNRESET (tail.developers.workers.dev: 000);
//   * `wrangler dev --remote` cannot start (bundled workerd wants GLIBC >= 2.32,
//     this box has 2.31);
//   * the driver's own cron trigger registered but fired ZERO times in 10
//     minutes (schedule created_on 13:05:07Z, no invocation, no KV key).
// What is left is a temporary route under saisi.online (reachable) plus a Worker
// that makes the requests from inside Cloudflare's network, where the network
// component of every sample is sub-ms instead of ~1s of throttled cross-border
// RTT — the 1s would swamp the 5-50ms this measurement is looking for.
//
// THROWAWAY, deleted with the rest (see ../README.md).

const DEFAULT_SUITE = { warm: 28, bursts: 2, burst: 6, work: 1, workN: 300000 };

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

async function timed(rid, url) {
  const t0 = performance.now();
  try {
    const r = await fetch(url);
    const text = await r.text();
    const ms = performance.now() - t0;
    let body = null;
    try {
      body = JSON.parse(text);
    } catch {
      /* a body that is not JSON is itself a finding */
    }
    return {
      rid,
      ms,
      status: r.status,
      colo: String(r.headers.get("cf-ray") ?? "").split("-").pop() || null,
      bytes: text.length,
      body,
    };
  } catch (e) {
    return { rid, ms: performance.now() - t0, status: 0, colo: null, bytes: 0, body: null, error: String(e) };
  }
}

async function runTarget(label, base, suite) {
  const samples = [];
  for (let i = 0; i < suite.warm; i++) {
    samples.push(await timed(`${label}-w${i}`, `${base}/echo?msg=warm&rid=${label}-w${i}`));
  }
  for (let j = 0; j < suite.bursts; j++) {
    const batch = await Promise.all(
      Array.from({ length: suite.burst }, (_, i) =>
        timed(`${label}-b${j}-${i}`, `${base}/echo?msg=burst&rid=${label}-b${j}-${i}`),
      ),
    );
    samples.push(...batch);
  }
  for (let i = 0; i < suite.work; i++) {
    samples.push(await timed(`${label}-k${i}`, `${base}/work?n=${suite.workN}&rid=${label}-k${i}`));
  }

  const echo = samples.filter((s) => s.rid.includes("-w") || s.rid.includes("-b"));
  const cold = echo.filter((s) => s.body?.requestIndex === 1);
  const warm = echo.filter((s) => (s.body?.requestIndex ?? 0) > 1);
  const work = samples.filter((s) => s.rid.includes("-k"));

  return {
    label,
    base,
    isolates: new Set(echo.map((s) => s.body?.isolateId).filter(Boolean)).size,
    colos: [...new Set(samples.map((s) => s.colo).filter(Boolean))],
    errors: samples.filter((s) => s.error).map((s) => ({ rid: s.rid, error: s.error })),
    statuses: [...new Set(samples.map((s) => s.status))],
    stats: {
      warm_ms: stats(warm.map((s) => s.ms)),
      cold_ms: stats(cold.map((s) => s.ms)),
      cold_isolateAgeMs: stats(cold.map((s) => s.body?.isolateAgeMs).filter((x) => x != null)),
      cold_handlerMs: stats(cold.map((s) => s.body?.handlerMs).filter((x) => x != null)),
      warm_handlerMs: stats(warm.map((s) => s.body?.handlerMs).filter((x) => x != null)),
      work_ms: stats(work.map((s) => s.ms)),
      work_handlerMs: stats(work.map((s) => s.body?.handlerMs).filter((x) => x != null)),
      work_acc: [...new Set(work.map((s) => s.body?.acc))],
    },
    samples,
  };
}

function targets(env) {
  return [
    { label: "ts", base: env.TARGET_TS },
    { label: "rust", base: env.TARGET_RUST },
  ].filter((t) => t.base);
}

async function runAll(env, suite) {
  const runs = [];
  for (const t of targets(env)) {
    try {
      runs.push(await runTarget(t.label, t.base, suite));
    } catch (e) {
      runs.push({ label: t.label, base: t.base, fatal: String(e), samples: [] });
    }
  }
  return runs;
}

// A cron run that throws writes `error/<stamp>` and a run that starts writes
// `begin/<stamp>`, so "the cron never fired", "the run threw" and "the targets
// are unreachable" are three different readings of KV instead of one silence.
async function runAndStore(env, event) {
  const stamp = new Date().toISOString();
  try {
    await env.RESULTS.put(
      `begin/${stamp}`,
      JSON.stringify({ stamp, cron: event.cron, scheduledTime: event.scheduledTime }),
    );
    const runs = await runAll(env, DEFAULT_SUITE);
    await env.RESULTS.put(`run/${stamp}`, JSON.stringify({ stamp, runs }));
  } catch (e) {
    await env.RESULTS.put(`error/${stamp}`, JSON.stringify({ stamp, error: String(e), stack: e?.stack }));
  }
}

export default {
  async scheduled(event, env, ctx) {
    ctx.waitUntil(runAndStore(env, event));
  },

  // GET /run?warm=28&bursts=2&burst=6&work=1
  // `warm=0&bursts=1&burst=8` is the shape used right after a deployment: the
  // new version's isolate pool is empty, so a burst of 8 concurrent requests
  // creates 8 isolates and every one of them is a cold start.
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    if (!url.pathname.endsWith("/run")) return new Response("coldstart-spike-driver\n", { status: 200 });
    const q = (k, d) => (url.searchParams.has(k) ? Number(url.searchParams.get(k)) : d);
    const suite = {
      warm: q("warm", DEFAULT_SUITE.warm),
      bursts: q("bursts", DEFAULT_SUITE.bursts),
      burst: q("burst", DEFAULT_SUITE.burst),
      work: q("work", DEFAULT_SUITE.work),
      workN: q("workn", DEFAULT_SUITE.workN),
    };
    const runs = await runAll(env, suite);
    const stamp = new Date().toISOString();
    const meta = {
      stamp,
      tag: url.searchParams.get("tag"),
      // The colo THIS invocation ran in. It matters: a subrequest to a target
      // is served by the target's isolate in the caller's colo, so a fresh
      // version has an empty isolate pool in every colo, and each colo is a
      // separate chance at a genuinely cold start.
      selfColo: request.cf?.colo ?? null,
      suite,
    };
    await env.RESULTS.put(`manual/${stamp}`, JSON.stringify({ ...meta, runs }));
    return Response.json({ ...meta, runs });
  },
};
