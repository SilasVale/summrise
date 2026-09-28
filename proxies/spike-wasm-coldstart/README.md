# P3.0 — what a cold Workers isolate costs a Rust→wasm Worker vs the TypeScript one

**THROWAWAY INSTRUMENT, and it has been torn down.** Every resource this
measurement created is deleted; see *Teardown* at the bottom for the proof.
The sources stay because they are the instrument, and because every number
below can be reproduced from them.

This answers the plan's **P3.0** gate
(`docs/superpowers/plans/2026-09-28-the-product-moves-to-rust.md`, task P3):

> **3.0 必须先做**：**量冷启动**——Workers 上 wasm 每 isolate 要实例化，**可能比 JS 慢**。

## The answer

**Yes. On a fresh isolate the Rust→wasm Worker costs a median of 9.6 ms of CPU
against the TypeScript Worker's 0.58 ms — 16.5× — and it ships 141.56 KiB
gzipped against 0.51 KiB.**

| | TypeScript | Rust → wasm | ratio |
|---|---|---|---|
| **cold** `cpuTime` p50 (µs) | **583** | **9 627** | **16.5×** |
| **cold** `wallTime` p50 (µs) | 965 | 10 230 | 10.6× |
| **warm** `cpuTime` p50 (µs) | 543 | 977 | 1.8× |
| **warm** driver-observed p50 / p99 (ms) | 13 / 30 | 12 / 25 | ≈1 |
| **bundle** gzipped | **0.51 KiB** | **141.56 KiB** | **277×** |
| `Worker Startup Time` | not reported | 2 ms | — |

**The plan's stop condition fires.** *"冷启动明显变差 → 只搬 `proxies`"* — a
16.5× CPU penalty on the first request to every fresh isolate is 明显变差.

**But the penalty is per ISOLATE, not per request**, which is what decides
*how* to migrate rather than *whether*. See *What this implies* below.

## The instrument problem, and why the obvious measurement is wrong

Two instruments were built. **They disagree, and the disagreement is the most
useful thing here.**

**Instrument A — observe the request from outside** (`measure.mjs`, `rounds.mjs`,
`driver/src/index.js`). A driver Worker runs *inside* Cloudflare and times its
subrequests, so the cross-border RTT that would otherwise swamp a 10 ms effect
is gone. This is a good instrument and it produced a **confident wrong answer**:

```
cold, driver-observed ms        ts                    rust
  rounds.mjs   (8 rounds)   81 65 80 28 69 35 41 96   126 52 61 35 54 50 54 88
  rounds2.mjs  (8 rounds)   81 65 80 28 69 31 53 91   126 52 61 35 54 44 63 90
  my round     (13:24:29)   71                        68      <-- rust FASTER
  ---------------------------------------------------------------
  p50                         69                        61      <-- rust FASTER
```

The sibling agent's `analyze.mjs` pooled 2 128 rows from the driver's KV and
reached the same reading (`out/analysis.json`): **`cold_ms.p50` ts 69, rust 61.**
Instrument A says the Rust Worker is *faster* to start.

It is wrong, and it cannot be fixed by adding samples: the sample is
`network + queueing + isolate start`, the first two terms are 50–90 ms of
variance, and the term under test is ~9 ms. The variance is larger than the
effect.

**Instrument B — ask Cloudflare what the request cost** (`analytics.mjs`,
`cpu.mjs`). The Workers GraphQL dataset (`workersInvocationsAdaptive`) carries
`cpuTime` and `wallTime` **measured at the edge**, per bucket, with no network
in the number at all. When a bucket holds exactly one request, its quantiles
*are* that request's cost.

**Instrument B is the one that answers the question.** Its units are
**microseconds** — note that `analytics.mjs`'s own comment says milliseconds and
is wrong; the reconciliation is that a trivial echo handler measured 543 µs of
CPU, which is only coherent as µs.

```
$ CLOUDFLARE_API_TOKEN=… node analytics.mjs coldstart-spike-rust 240
```

## Cold start — how it was forced, and the proof it was cold

**There is no "force a cold isolate" button, and nothing here guesses at one.**

1. `wrangler deploy` publishes a **new script version**, and a version's isolate
   pool starts empty in every colo. That is the forcing function.
2. Then **exactly one request** is sent, via the driver.
3. **The isolate states its own coldness.** Both workers stamp their identity on
   the first request they serve and return `requestIndex` and `isolateAgeMs`;
   `requestIndex === 1` means *this request created the isolate*. That is not a
   proxy for coldness — it is the isolate saying so.

**16 of the saved samples carry that proof** (`out/cold-r*.json`), and the
confirmation round does too:

```
$ curl -s "https://coldstart.saisi.online/driver/run?warm=0&bursts=1&burst=1&work=0&tag=confirm-132429"
  ts    driverMs=71.0 requestIndex=1 isolateAgeMs=0 handlerMs=0 colo=FRA status=200
  rust  driverMs=68.0 requestIndex=1 isolateAgeMs=0 handlerMs=0 colo=FRA status=200
```

`isolateAgeMs=0` is the second, independent confirmation: zero milliseconds had
elapsed in that isolate's life before this request.

### The result, 18 paired rounds

Each row is one bucket containing exactly one request, from a fresh deployment,
paired ts-vs-rust by timestamp (`out/rounds.log`, `out/rounds2.log`):

| bucket (UTC) | ts cpu µs | rust cpu µs | ratio |
|---|---|---|---|
| 13:14:45 | 642 | 12 729 | 19.8× |
| 13:18:23 | 569 | 15 113 | 26.6× |
| 13:18:45 | 402 | 7 468 | 18.6× |
| 13:18:59 | 496 | 8 309 | 16.8× |
| 13:19:21 | 422 | 5 866 | 13.9× |
| 13:19:33 | 597 | 15 956 | 26.7× |
| 13:19:52 | 652 | 10 817 | 16.6× |
| 13:20:09 | 710 | 10 459 | 14.7× |
| 13:20:40 | 288 | **846** | 2.9× |
| 13:21:35 | 935 | 28 441 | 30.4× |
| 13:21:58 | 415 | 7 095 | 17.1× |
| 13:22:15 | 903 | 11 639 | 12.9× |
| 13:22:37 | 360 | 7 684 | 21.3× |
| 13:22:53 | 435 | 6 899 | 15.9× |
| 13:23:15 | 630 | 8 795 | 14.0× |
| 13:23:30 | 509 | 6 672 | 13.1× |
| 13:23:53 | 613 | 11 958 | 19.5× |
| **13:24:29 (mine)** | **635** | **14 001** | **22.0×** |

```
ts    cold cpuTime µs: min=288  p50=583   max=935     wallTime µs: p50=965
rust  cold cpuTime µs: min=846  p50=9627  max=28441   wallTime µs: p50=10230

median ratio 16.5×   ·   absolute penalty +9.0 ms CPU per fresh isolate
```

**Two honest caveats on this table.**

* **One rust sample (13:20:40, 846 µs) is indistinguishable from warm.** It is
  consistent with V8's compiled-module cache being shared per machine: a *second*
  fresh isolate on a machine that already holds the compiled wasm skips the
  compile. I cannot prove that from this data, and I am not going to claim it —
  the honest reading is that **one of 18 rust cold starts was cheap and 17 were
  not**, and the mechanism is a hypothesis.
* The ts figures are tight (288–935 µs, all 18 samples). **TypeScript shows no
  cold penalty at all** — a fresh TS isolate costs what a warm one costs.

**And `Worker Startup Time: 2 ms`** is wrangler's own deploy-time number for the
Rust worker. It does not contradict the 9.6 ms: 2 ms is *module instantiation*,
while the first request additionally pays **lazy wasm function compilation** when
the handler first calls into the module. Wrangler prints no such line for the
TypeScript worker at all (measured twice — see `out/rounds.log`, `startup=not
reported` on every ts round and `startup=1..2` on every rust round).

## Warm — p50 and p99, with the spread

**240 samples per worker**, from 12 runs of `warm=20` (`out/warm-*.json`),
classified by the isolate's own `requestIndex > 1`:

```
ts    n=240  min=8  p50=13  p90=20  p99=30  max=88  mean=14.28  sd=6.32
rust  n=240  min=8  p50=12  p90=17  p99=25  max=27  mean=12.89  sd=3.52
```

**Report the spread, not the median** — so, per-run p50s, which show the drift is
larger than the difference between the workers:

```
ts    per-run p50s: [19, 13, 17, 12, 10, 13, 12, 11, 14, 14, 15, 11]   range 10–19
rust  per-run p50s: [16, 12, 15, 10, 12, 10, 12, 12, 14, 10, 11, 12]   range 10–16
```

**Warm, the two workers are the same.** Rust's p50 is 1 ms lower and its tail is
tighter, but the ranges overlap and the driver-side number carries a ~10 ms
transport floor — `handlerMs` was **0** for every warm sample of both workers, so
essentially none of that 12–13 ms is the handler.

The floor-free comparison is Instrument B, and there **Rust is slower warm too**:

```
ts    warm cpuTime µs: p50=543  (387 requests across 15 buckets)
rust  warm cpuTime µs: p50=977  (269 requests across 13 buckets)     -> 1.8x
```

**So "wasm's win is CPU" does not hold for this route.** A trivial JSON echo has
no computation for wasm to win on, and the wasm↔JS boundary is pure cost. This is
the plan's own principle ④ arriving from the other direction: *"wasm 不会让界面
更快（UI 没有重计算，DOM 跨边界反而更慢）"*. The `/work?n=300000` route — the
same integer loop in both, and both returned the identical `acc=146613704860` —
was also a wash (ts p50 13 ms, rust p50 11 ms over 12 samples each), because the
loop is smaller than the transport floor. **The CPU win, if there is one, needs a
route that does real work to show up, and this instrument does not have one.**

## Bundle size

```
$ cd ts   && wrangler deploy --dry-run --outdir=/tmp/dry-ts
$ cd rust && wrangler deploy --dry-run --outdir=/tmp/dry-rust

ts    Total Upload: 0.94 KiB   / gzip: 0.51 KiB
rust  Total Upload: 402.99 KiB / gzip: 141.56 KiB

rust artifacts, gzipped individually:
  index_bg.wasm   380 190 B raw -> 133 017 B gz
  index.js         20 552 B raw ->   5 250 B gz     (wasm-bindgen glue)
  shim.mjs            164 B raw ->     145 B gz
```

`--dry-run` is what wrangler *would* upload, so this is the number the CDN
serves, not an estimate. **277× more bytes gzipped**, and the wasm binary alone
is 133 KiB gz against a 0.51 KiB TypeScript bundle.

## CPU time per request

**`wrangler tail` cannot be used on this box.** Its websocket is reset —
`tail.developers.workers.dev` is blocked from here (`out/wrangler-tail.ECONNRESET.err`,
ECONNRESET). So the brief's *"if the dashboard or `wrangler tail` exposes it"*
resolves to **the dashboard's own dataset, reached directly**:

```
$ node analytics.mjs <scriptName> <windowMinutes>     # workersInvocationsAdaptive, µs
```

All CPU figures above come from there. Three other triggers are also closed on
this box, each measured, each recorded in `out/`:

| path | what happened |
|---|---|
| `*.workers.dev` | TLS reset — SNI blocked, from this box *and* the devices |
| `wrangler tail` | ECONNRESET (`out/wrangler-tail.ECONNRESET.err`) |
| `wrangler dev --remote` | workerd needs GLIBC ≥ 2.32, this box has 2.31 (`out/wrangler-dev-remote.GLIBC.err`) |
| cron trigger on the driver | registered, **zero** invocations in 10 minutes |

`*.saisi.online` is reachable, so the instrument is a temporary proxied DNS
record plus three Workers routes (`setup-routes.mjs`, which deletes them too).

## What this implies for `proxies` → `index` → `gateway`

**The stop condition fires, and it does not cancel the migration — it sets its
order.** The penalty is **per fresh isolate**, so what decides a Worker's exposure
is **the fraction of its requests that are first-requests to a new isolate**:

* **A Worker with steady traffic amortises the 9.6 ms into nothing.** One cold
  isolate serves thousands of requests; the cost is paid once.
* **A Worker with sparse, bursty traffic pays it on a large share of requests** —
  and pays it in CPU, which is what Workers bills and limits.

Read against that:

1. **`proxies` is the worst case, not the safest.** These are the lowest-traffic
   Workers in the repo, they are bursty, and they sit on the device's critical
   path (the git mirror is `proxies/api-relay/api/git.ts`). Sparse traffic means
   a *high* cold-request fraction — exactly where a 9.6 ms penalty lands hardest.
   The plan's instinct to start here is right for the *smallest-blast-radius*
   reason, but the cold-start measurement argues **against** it on its own merits.
2. **`index`** serves static assets through Workers Assets, so the wasm module is
   only on the routes — and the landing page has already been measured at 4.7×
   size for a Rust version (plan P4). Low traffic plus an existing size finding.
3. **`gateway`** is the most load-bearing but also **the one with the most
   traffic** (a real user is on the console), so it amortises the startup best of
   the three. It is also where Rust's types and unity pay most.

**The honest recommendation: measure each Worker's cold-request fraction before
choosing, rather than taking the plan's `proxies`-first order as settled by this
number.** `analytics.mjs` can produce that fraction from the same dataset. And
**do not justify any of it with "wasm is faster"** — on this route it is 1.8×
slower warm and 16.5× slower cold.

**One limit on all of the above, stated rather than buried:** this is a *trivial
echo*. A route that does real work changes both terms — the 9.6 ms startup
becomes a smaller share, and the warm CPU comparison could invert in wasm's
favour. **This instrument measures the runtime, which is what the question asked;
it does not predict the application.**

## Teardown

Everything created for this measurement was deleted, and deletion was verified
against the API rather than assumed:

```
DELETE coldstart-spike-ts      -> HTTP 200 success=True
DELETE coldstart-spike-rust    -> HTTP 200 success=True
DELETE coldstart-spike-driver  -> HTTP 200 success=True
deleted dns AAAA coldstart.saisi.online
kv delete success= True                    (namespace 6c8bcaff0e7f4f929cebcdd80032da47)
deleted routes  coldstart.saisi.online/{ts,rust,driver}/*

VERIFY  workers matching coldstart : NONE
VERIFY  routes  matching coldstart : NONE
VERIFY  dns records                : NONE
VERIFY  kv namespaces              : NONE
VERIFY  https://coldstart.saisi.online/ts/echo -> HTTP 530   (no worker serves it)
```

`gateway/` was never touched. The other two workers on the zone
(`zen-us.saisi.online/* -> zen-us-proxy`, `agent.saisi.online/files/* ->
summrise-relay`) are untouched and predate this work.

## Known collision — read this before re-running `cpu.mjs`

**A second agent was working in this same worktree while this measurement was
finishing.** It wrote `analyze.mjs` and `cpu.mjs` (both kept here), and both read
the driver's results from the **KV namespace** — which teardown deleted. So:

* `analyze.mjs` had already run (`out/analysis.json`, 2 128 rows, 13:25:54Z) and
  its driver-side result is preserved; it **agrees with Instrument A above**
  (`cold_ms.p50`: ts 69, rust 61) and therefore carries the same error.
* `cpu.mjs` now fails with `namespace not found` (`out/cpu.err`). **Its design was
  sound and it was aimed at exactly the right number** — it just lost its source
  at teardown. The equivalent result is in this README, from GraphQL plus the
  saved `out/cold-r*.json`, both of which survive.

Also worth recording from `out/analysis.json`: **657 of 1 064 ts samples and 771
of 1 064 rust samples were errors** — the driver's subrequests hit the
50-subrequests-per-invocation limit. The cold and warm figures above are drawn
only from samples that returned a body, and the analytics in Instrument B counts
only requests that actually reached the target, so neither is affected — but a
re-run should stay under that limit.
