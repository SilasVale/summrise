# P3.0's follow-up — each Worker's COLD-REQUEST FRACTION

**THROWAWAY INSTRUMENT, and it has been torn down.** Every resource this
measurement created is deleted, and the deletion was **re-queried** rather than
assumed — see *Teardown* at the bottom. The sources stay because they are the
instrument, and because every number below is reproducible from them.

This is the step P3.0 named as the one that must come before migrating any
Worker:

> **所以 P3 的第一步不是搬任何一个 ✓，是**量每个 Worker 的冷请求比例**✓（`analytics.mjs` 能 ✓），
> **然后按那个比例排序 ✓**——**而不是把 `proxies` 先行当成定论 ✗。**
>
> — `docs/superpowers/plans/2026-09-28-the-product-moves-to-rust.md`, *P3.0 的答案*

P3.0 settled that a Rust→wasm Worker costs **+9.0 ms of CPU per fresh isolate**
(16.5× the TypeScript one, 18 paired rounds). The penalty is **per isolate, not
per request**, so a Worker's exposure is **the fraction of its requests that are
first-requests to a new isolate**. That fraction is what this measures.

## The answer

Window: **168 hours (7 days), frozen at `2026-09-28T13:39:00Z`**, from
Cloudflare's own per-request dataset (`workersInvocationsAdaptive`, GraphQL,
**microseconds**).

The cold fraction is `structural(E)`, where `E` is how long an idle isolate
survives — so it is a **range**, and the range is set by the two ends of `E` this
measurement actually saw. **`E` was not pinned to a point, and it is not a fixed
timeout** (see *The survival sweep* — the same script in the same colo produced a
**37-minute survivor** and a **death inside 900 s**). The two ends used below are
the measured extremes: **E = 240 s** (survival directly re-confirmed) and
**E = 2 220 s** (a fresh connection reached a 37-minute-idle isolate).

| Worker (script) | requests in window | (minute,colo) buckets | **cold-request fraction** | **`fraction × 9.0 ms`** |
|---|---|---|---|---|
| `gateway/` → `vale-gate` | **36 180** | 7 479 | **0.6 % … 2.7 %** | **0.06 … 0.24 ms / request** |
| `index/` → `summrise-dist` | **7 952** | 2 323 | **4.0 % … 6.3 %** | **0.36 … 0.57 ms / request** |
| `proxies/` → all three | **168** | 141 | **18.5 % … 51.8 %** | **1.66 … 4.66 ms / request** |
| &nbsp;&nbsp;· `summrise-relay` | 154 | 129 | 12.3 % … 48.7 % | 1.11 … 4.38 ms |
| &nbsp;&nbsp;· `opencode-go-proxy` | 13 | 11 | 84.6 % (at every `E`) | 7.62 ms |
| &nbsp;&nbsp;· `zen-us-proxy` | 1 | 1 | 100 % | 9.00 ms |

**Take the pessimistic end for planning** — `E = 240 s`, the shortest persistence
actually measured: **gateway 2.7 %, index 6.3 %, proxies 51.8 %.**

The last 24 hours of that same window (12 333 / 3 079 / 148 requests) give
**0.2–1.3 %**, **2.2–4.6 %** and **15.3–50.0 %** — the two windows agree, so this
is not one busy afternoon.

**And against each Worker's own CPU per request, which is what the number is
actually for** (pessimistic end in bold):

| Worker | own `cpuTime` / request | wasm penalty adds | **relative** |
|---|---|---|---|
| `gateway/` | 15 732 µs | +0.06 … **+0.24** ms | +0.4 % … **+1.5 %** |
| `index/` | 838 µs | +0.36 … **+0.57** ms | +43 % … **+68 %** |
| `proxies/` (`relay`) | 1 904 µs | +1.11 … **+4.38** ms | +58 % … **+230 %** |
| `proxies/` (`go-proxy`) | 1 177 µs | **+7.62** ms | **+647 %** |
| `proxies/` (`zen-us`) | 802 µs | **+9.00** ms | **+1122 %** |

**Migrating `proxies` to wasm doubles to twelvefolds its CPU per request.
Migrating `gateway` adds about one percent.** That is the whole ordering, and it
is the reverse of the plan's `proxies → index → gateway`.

### P3.0's reasoning, checked

P3.0 predicted this without measuring it:

> *"`proxies` 是**流量最低、最突发**的 ✓ … → **它在**最大比例的请求**上付那 9.6 ms ✗**——**所以**不是**先搬它 ✓。**
> **`gateway` 流量最大 ✓（一个真人在用 ✓），摊销得最好 ✓✓。**"

**Confirmed, and the figure behind the claim is starker than the claim.** P3.0
asserted `proxies` is the lowest-traffic Worker **without a number**; this window
puts one on it: **168 requests / 7 days** against `gateway`'s **36 180** — a
factor of **215**. Per colo that is:

```
  gateway   1 request per  7.5 min per colo   (36 180 / 27 colos)
  index     1 request per   50 min per colo   ( 7 952 / 40 colos)
  proxies   1 request per  5.4 h  per colo   (   154 /  5 colos, relay)
```

P3.0's *direction* was right. The *magnitude* is why the ordering is not close.

## The order the arithmetic produces

```
  cheapest to migrate  ->  most expensive to migrate
  gateway (0.06-0.24 ms)  ->  index (0.36-0.57 ms)  ->  proxies (1.66-4.66 ms)
```

**This disagrees with the plan's P3 order** (`proxies → index → gateway`), and it
agrees with P3.0's expectation that it would. But the disagreement is narrower
than it looks, because **the two orders optimise different things and the plan
said so**:

* the plan's order is **blast radius** — `proxies` is the smallest thing to break;
* this measurement is **cost per request** — and it says `proxies` is the most
  expensive thing to break.

They are opposite, and both are real. What the measurement removes is the
possibility of pretending they coincide: **you cannot migrate `proxies` first
"because it is smallest" without paying the largest CPU penalty in the repo, and
you cannot migrate `gateway` first "because it is cheapest" without touching the
Worker a real user is on.** Whichever is chosen, one of the two reasons has to be
given up explicitly — and it should be given up in the plan, in writing, rather
than discovered later.

### Is any Worker cheap enough that migrating it is cheap?

**`gateway` is — and it is the only one.** +0.4–1.5 % CPU per request on the
Worker that serves the console is a rounding error next to its own 15.7 ms of
work. `index` at +43–68 % is not cheap. `proxies` at +58 %…+1122 % is expensive
in exactly the way P3.0 feared, and it is expensive on **the device's critical
path** (the git mirror is `proxies/api-relay/api/git.ts`).

**In aggregate, though, all three are trivial** — worth saying, because the
per-request number makes it look otherwise:

```
  gateway   36 180 req/wk x 0.06-0.24 ms  =   2.2 -  8.7 s CPU / week
  index      7 952 req/wk x 0.36-0.57 ms  =   2.9 -  4.5 s CPU / week
  proxies      168 req/wk x 1.66-4.66 ms  =   0.3 -  0.8 s CPU / week
```

**The total cost of the wasm penalty across all three Workers is under 15 seconds
of CPU per week.** Workers bills CPU, so this is not a billing problem. What the
fraction actually buys is the **per-request latency**: `fraction` is the share of
requests that get **+9 ms** added (P3.0 measured cold `wallTime` p50 10 230 µs
against 965 µs, so the CPU cost does surface as wall time). Against `gateway`'s
own ~16 ms of work that is invisible; against `index`'s 0.84 ms it is a tenfold
jump for ~5 % of requests; on the git mirror it is invisible for the opposite
reason — that route has been measured answering in **0 s / 32 s / >90 s** for the
same request, so 9 ms is not the problem there.

**So the honest recommendation is not "migrate in this order".** It is that the
ordering argument P3.0 set out to settle **does not settle it**, because the CPU
penalty is too small in absolute terms for any of the three to be decided by it.
Decide P3 on **type-safety and unity** — the reason P3.0 already said to use,
*"不要用速度论证迁移 ✓；理由是类型与统一 ✓"* — and then take the blast-radius
order, which is the plan's. What this measurement contributes is that it
**removes the cold-start objection to `proxies`-first**: at 1.7–4.7 ms per
request on 168 requests a week, `proxies`-first is affordable, and the plan's
order stands on the reason it was chosen for.

## How the fraction was obtained — and why the obvious route is closed

**The dataset cannot answer the question directly, and this is what shaped the
instrument.** `workersInvocationsAdaptive` carries `cpuTime` and `wallTime` per
bucket but has **no isolate dimension** — nothing in it says "this request
created an isolate". And for a *TypeScript* Worker it could not be inferred from
cost either: P3.0 measured TS cold at 583 µs against warm 543 µs, a 7 %
difference no bucket quantile can resolve.

So the fraction is built from two measured things:

1. **the arrival process, per colo**, which the dataset *does* carry — at
   `datetimeMinute` × `coloCode` granularity, and a colo is an isolate pool;
2. **the one parameter the dataset lacks — how long an idle isolate survives —
   measured directly**, because without it (1) is a traffic description, not a
   cold fraction.

### The survival sweep (`sweep.mjs`, `tail/run.mjs`, `control.mjs`)

A throwaway Worker whose isolate states its own identity (`isolateId`,
`requestIndex`, `isolateAgeMs` — P3.0's design, reused). A keep-alive connection
sends a request, waits G seconds, sends another, and asks whether the *same*
isolate answered.

**Two confounds, both measured, both handled — and the second one changed the
conclusion.**

* **This box's requests alternate colos.** Four `curl` invocations 0/2/5/10 s
  apart answered from **AMS, LHR, AMS, LHR** — and two colos are two isolate
  pools, so *every* gap looked like an eviction. A keep-alive agent with
  `maxSockets: 1` pins the colo; every pair is still required to agree on colo,
  and a pair that does not is **retried rather than counted**. Without this the
  sweep reported an eviction at 2 seconds, which would have put `gateway`'s cold
  fraction near 100 % and reversed the conclusion. **The instrument was wrong in
  the direction that made the finding dramatic.**
* **The keep-alive connection is itself part of the answer, and the first version
  of this report mistook it for the whole answer.** `control.mjs` asked for the
  same isolate twice across a 240 s gap: **on the same connection, the same
  isolate answered (`requestIndex` 1 → 2); on a fresh connection, a different
  one.** That alone is ambiguous — the old isolate might have died, or the pool
  might have routed elsewhere — so `freshconn.mjs` separated the two by asking
  for a **fresh connection 5 s after a request**: it landed back on the *same*
  isolate (`265b84c1`, `requestIndex` 2). **Fresh connections do reuse warm
  isolates.** The 240 s fresh-connection result was therefore not an eviction.

**What that leaves is a distribution, not a timeout — and the strongest single
measurement came from an accident:**

```
  script                colo  last request   next request    idle      outcome
  p30-coldfrac-probe    LHR   13:37:30       13:41:30        240 s     ALIVE (idx 1->2, same conn)
  p30-coldfrac-probe    AMS   13:36:46       14:14:00       2220 s     ALIVE (idx 1->2, FRESH conn)
  p30-coldfrac-tail900  AMS   13:45:19       14:00:34        900 s     DEAD  (new id, idx 1, age 0)
```

**The same script family, in the same colo (AMS), produced a 37-minute survivor
and a death inside 15 minutes.** Isolate persistence is a pool-eviction policy
under memory pressure, not a fixed idle timeout, and **no single `E` is correct**.
That is why the answer above is a range across the measured extremes rather than
a point, and why the sensitivity table below is the load-bearing part of this
document.

* **No partial survival was ever observed**: every measured gap was alive or
  dead, never "sometimes". What is uncertain is `E`, not the shape.
* The 1 800 s tail probe **errored** (empty error, connection lost) and the
  3 600 s one was **torn down mid-flight**; neither is counted. They could only
  have widened the range from above.

### The estimator (`final.mjs`)

**A model-free bound and a Monte Carlo, and they agree exactly — that agreement
is the check that matters.**

* **`structural(E)`** — for each (minute, colo) bucket, take the *latest* the
  previous request in that colo could have happened (:59 of its own minute) and
  the *earliest* this bucket's first request could have happened (:00). If that
  minimum possible gap already exceeds `E`, the bucket's **first** request is cold
  whatever the arrival process did. **One per bucket, not `n`** — the rest of a
  bucket's requests are inside the same minute and their gaps are at most 60 s.
  (The first version of this file counted `n` and reported `gateway` at 15.0 %
  where the honest floor is 5.1 %. A bound that is wrong in the direction that
  flatters the finding is the one to distrust, so it is recorded here.)
* **Monte Carlo over three within-minute arrival models** — `poisson` (smooth),
  `uniform` (evenly spaced), `burst` (the whole minute's traffic at one instant,
  which is what a page load looks like). The dataset's finest granularity is one
  minute, so the gap *inside* a minute is not observable; rather than pick one
  model and call its answer THE answer, all three are simulated.

**The arrival model barely matters; `E` is everything.**

```
  gateway  poisson 0.0270  uniform 0.0270  burst 0.0270   <- spread 0.0000   (at E = 240 s)
  index    poisson 0.0630  uniform 0.0630  burst 0.0631   <- spread 0.0001
```

The three models differ by **0.01 percentage points or less**, and the model
reproduces `structural` to four decimals — the expected result, because under a
step survival curve "cold iff gap > E" the two are the same quantity computed two
different ways.

## The limit, stated plainly

P3.0 said of its own result: *"这是一个 trivial echo ✓——它隔离了运行时 … **但它不预测一条真实路由 ✓**"*.
The equivalent here:

> **This is a traffic-shape measurement whose one free parameter is a
> distribution, not a number.** It says how often each Worker's requests are
> *separated by a gap*, which is exactly what a cold start depends on — and it
> brackets that gap's consequence to a factor of ~4 (0.6–2.7 % for `gateway`,
> 18–52 % for `proxies`). It does **not** say where in that bracket reality sits:
> isolate persistence was observed to span 240 s to 2 220 s on one script in one
> colo, and the mechanism — eviction under memory pressure — is a property of the
> machine, which this instrument cannot see.
>
> **It does not predict a real route either.** It measures the *arrival process*,
> not the handler: nothing here knows whether `proxies`' real routes compile to a
> smaller or larger wasm module than the trivial echo P3.0 built, and a route that
> does real work changes both terms — the 9 ms startup becomes a smaller share,
> and the warm CPU comparison could invert in wasm's favour.

**The full sensitivity, so a reader who believes a different `E` can substitute
it** — this table needs no model at all, only the arrival times:

```
  if an idle isolate survives ...   gateway   index   proxies/
    60 s                              5.1 %   10.3 %    75.0 %
   120 s                              3.8 %    7.9 %    61.9 %
   240 s                              2.7 %    6.3 %    51.8 %   <- measured ALIVE (LHR)
   300 s                              2.4 %    6.0 %    50.6 %
   600 s                              1.3 %    5.2 %    37.5 %
   900 s                              0.95%    4.95%    30.4 %   <- measured DEAD  (AMS)
  1800 s                              0.7 %    4.2 %    19.7 %
  2220 s                              0.6 %    4.0 %    18.5 %   <- measured ALIVE (AMS, 37 min)
  3600 s                              0.4 %    3.5 %    16.7 %
 21600 s                              0.2 %    1.6 %    13.1 %
```

**The ordering is `proxies > index > gateway` at every single row**, which is why
the order in *The answer* does not depend on resolving `E` — only the magnitudes
do.

### What was not measured, and why

* **`E` as a number.** It is a distribution; three trials produced 240 s alive,
  2 220 s alive, 900 s dead. Pinning it needs many trials per gap across several
  colos, which is hours of wall time, and the sensitivity table above is what a
  reader needs instead.
* **Whether a wasm isolate is evicted on the same schedule.** The persistence
  observations are a *TypeScript* probe's. `worker-build --release` was tried for
  a Rust twin and **`wasm-bindgen` 0.2.129 SIGSEGVs on this box** (34 s in, after
  the crate compiled) — a toolchain rabbit hole, not this measurement. The
  direction of the error is at least known: a 133 KiB compiled module makes an
  isolate *heavier*, and a colo evicts under memory pressure, so a wasm isolate
  should be evicted **no less often** than these curves say. **The fractions are
  a floor.**
* **The pool behaviour inside a colo.** How many isolates a colo keeps for one
  script, and how a request is routed among them, is what actually sets `E` — and
  it is invisible from outside. `freshconn.mjs` is the instrument that got closest
  (6 fresh connections, 2 colos, 5 distinct isolates, one of them 37 minutes old)
  and it is a sample of six.

## What this confirms, and what it corrects

**Four things in the source that the measurement contradicted or sharpened:**

1. **"`analytics.mjs` 能" is true but not sufficient.** The dataset carries the
   arrival process; it does **not** carry isolate persistence, and P3.0's note
   reads as though the fraction could be read straight out of the dataset. It
   cannot — it needs a second instrument, which is what `sweep.mjs` and the tail
   probes are.
2. **The brief's "三个 Worker" is five deployed scripts.** `proxies/` is not one
   Worker: it is `opencode-go-proxy`, `zen-us-proxy` and `summrise-relay`, with
   independent traffic and independent fractions (84.6 % / 100 % / 12.3–48.7 %).
   Any plan that migrates "`proxies`" as one unit is migrating three scripts.
3. **A sixth script is on the account and is not in the plan at all.**
   `vale-dist` — `docs/BRAND.md:280` already says *"no domain points at it any
   more"* — still served **620 requests in this same 168 h window**. It is the
   pre-rename predecessor of `summrise-dist`. It is not one of the three Workers,
   and it is not dead.
4. **P3.0's warning about a confidently-wrong instrument applied here twice, at
   two different levels.** The first `sweep.mjs` (four `curl` calls, no pinned
   colo) reported **an eviction at 2 seconds**. The first *version of this
   report* then read the pinned-connection sweep as isolate persistence, when
   `control.mjs` shows the connection is part of what is being measured. Both
   errors pointed the same way — toward a more dramatic answer — and both were
   caught only by asking the instrument a question it had not been designed to
   answer.

## Files

| file | what it is |
|---|---|
| `sweep.mjs` | the isolate survival sweep — probe, gap sweep, colo pinning, retry |
| `control.mjs` | the control: same connection vs fresh connection across one gap |
| `freshconn.mjs` | does a fresh connection reuse a warm isolate? (it does) |
| `tail/deploy.sh`, `tail/run.mjs` | three long-gap probes (900 / 1 800 / 3 600 s), one script each, in parallel |
| `final.mjs` | the estimator — model-free bound + Monte Carlo over three arrival models |
| `pull2.mjs` | the GraphQL pull (paginated; the dataset's units are **microseconds**) |
| `probe/src/index.js`, `probe/wrangler.jsonc` | the TypeScript probe Worker |
| `probe-wasm/` | the Rust probe — **does not build here** (`wasm-bindgen` SIGSEGV); kept for the record |
| `teardown.sh` | deletes everything and **re-queries** |
| `out/` | the raw data every number above comes from — the 7-day pulls are committed **gzipped** (2.1 MiB of `vale-gate` buckets is 185 KiB gzipped); `final.mjs` reads either form |

## Teardown

```
=== DELETE workers ===
  DELETE p30-coldfrac-probe      -> HTTP 200 success= True
  DELETE p30-coldfrac-tail900    -> HTTP 200 success= True
  DELETE p30-coldfrac-tail1800   -> HTTP 200 success= True
  DELETE p30-coldfrac-tail3600   -> HTTP 200 success= True
  DELETE p30-coldfrac-probe-wasm -> HTTP 404 success= False   (never deployed — its build failed)

=== DELETE routes matching p30 ===
  (none remained: the route was declared inside probe/wrangler.jsonc and went with the worker)

=== DELETE DNS records matching p30 ===
  DELETE dns AAAA p30frac.saisi.online      -> HTTP 200
  DELETE dns AAAA p30tail900.saisi.online   -> HTTP 200
  DELETE dns AAAA p30tail1800.saisi.online  -> HTTP 200
  DELETE dns AAAA p30tail3600.saisi.online  -> HTTP 200

=== RE-QUERY (the part that makes the teardown a fact) ===
VERIFY workers matching p30  : NONE
VERIFY all workers on account: opencode-go-proxy, summrise-dist, summrise-relay,
                               vale-dist, vale-gate, zen-us-proxy
VERIFY routes matching p30   : NONE
VERIFY all routes on zone    : zen-us.saisi.online/*, agent.saisi.online/files/*
VERIFY dns matching p30      : NONE
VERIFY https://p30frac.saisi.online/probe -> HTTP 530   (no worker serves it)
```

The account is back to exactly the six scripts it had before this work, and the
zone to exactly the two routes it had. `gateway/`, `index/` and `proxies/` were
never deployed, never edited, and never restarted: **this measurement is read-only
with respect to every Worker it measures**, which is why their isolate pools were
left alone and the numbers describe traffic this instrument did not create.
