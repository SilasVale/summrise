#!/usr/bin/env node
// final.mjs — the cold-request fraction per Worker, from Cloudflare's own
// per-request dataset plus the measured isolate-survival curve.
//
//   node final.mjs <hours>
//
// THREE numbers per worker, in decreasing order of assumption:
//
//  1. `structural` — NO arrival model, NO survival model. For each
//     (minute, colo) bucket take the LATEST the previous request in that colo
//     could have happened (:59 of its own minute) and the EARLIEST this bucket's
//     first request could have happened (:00). If that minimum possible gap
//     already exceeds G*, the bucket's FIRST request is cold whatever the arrival
//     process did.
//     ONE per bucket, not n: the rest of a bucket's requests are inside the same
//     minute, so their gaps are at most 60 s and they are NOT certainly cold.
//     (The first version of this file counted n and reported gateway at 15.0%
//     where the honest floor is 6.4% — a bound that is wrong in the direction
//     that flatters the finding is the one to distrust.)
//
//  2. `model[tail]` — Monte Carlo over three within-minute arrival models, with
//     the measured survival curve for gaps the sweep actually tested. Beyond the
//     largest tested gap the curve says nothing, so BOTH tails are reported:
//     `tailAlive` (an isolate that survived 600 s survives 2 h) and `tailDead`
//     (it does not). The truth is between them, and the gap between the two
//     columns IS the unmeasured part.
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';

// The raw 7-day pulls are committed GZIPPED (2.1 MiB of `vale-gate` buckets is
// 185 KiB gzipped), so read either form. The query that produces them is in
// pull2.mjs's header; nothing here depends on which form is on disk.
const readJSON = (base) => {
  const plain = `${base}.json`;
  return JSON.parse(existsSync(plain) ? readFileSync(plain,'utf8') : gunzipSync(readFileSync(`${plain}.gz`)).toString('utf8'));
};

const HOURS = Number(process.argv[2] ?? 168);
const REPS = 300;
const TAGS = ['vale-gate','summrise-dist','opencode-go-proxy','zen-us-proxy','summrise-relay'];
const sw = readJSON('/tmp/p30frac/out/sweep');

const byGap = new Map();
for (const t of sw.trials) {
  if (t.sameColo !== true || typeof t.survived !== 'boolean') continue;
  if (!byGap.has(t.gapS)) byGap.set(t.gapS, []);
  byGap.get(t.gapS).push(t.survived);
}
const curve = [...byGap.entries()].map(([g,v])=>({g, n:v.length, s:v.filter(Boolean).length/v.length})).sort((a,b)=>a.g-b.g);
const MAXG = curve.length ? Math.max(...curve.map(c=>c.g)) : 0;
const mkS = (tailAlive) => (g) => {
  if (g > MAXG) return tailAlive ? curve.find(c=>c.g===MAXG).s : 0;
  let s = 1; for (const c of curve) if (g >= c.g) s = c.s; return s;
};

function coldInBucket(n, minGap, model, S) {
  let times;
  if (model === 'burst') { const t0 = Math.random()*60; times = new Array(n).fill(t0); }
  else if (model === 'uniform') times = Array.from({length:n},(_,i)=>60*(i+0.5)/n);
  else times = Array.from({length:n},()=>Math.random()*60).sort((a,b)=>a-b);
  let cold = 0, prev = -minGap;
  for (const t of times) { const s = (prev === -Infinity) ? 0 : S(t - prev); if (Math.random() >= s) cold++; prev = t; }
  return cold;
}

const GSTARS = [30, 60, 120, 300, 600, 1800, 3600, 21600];
const out = { hours: HOURS, reps: REPS, measuredCurve: curve, maxTestedGapS: MAXG, workers: {} };
for (const tag of TAGS) {
  let j; try { j = JSON.parse(readJSON(`/tmp/p30frac/out/w168-${tag}`)); } catch { continue; }
  const rows = j.rows.filter(r => (Date.now()-new Date(r.dimensions.datetimeMinute).getTime()) <= HOURS*3600_000);
  const total = rows.reduce((a,r)=>a+r.sum.requests,0);
  if (!total) { out.workers[tag] = { requests: 0, note: 'no requests in this window' }; continue; }

  const byColo = new Map();
  for (const r of rows) {
    const c = r.dimensions.coloCode;
    if (!byColo.has(c)) byColo.set(c, []);
    byColo.get(c).push({ m: new Date(r.dimensions.datetimeMinute).getTime(), n: r.sum.requests });
  }
  const seq = [];
  for (const [, arr] of byColo) {
    arr.sort((a,b)=>a.m-b.m);
    let prevLatest = null, prevEarliest = null;
    for (const b of arr) {
      seq.push({
        n: b.n,
        // latest the previous request could have been (:59 of its minute) vs
        // earliest this bucket's first request could be (:00) -> a LOWER bound
        minGap: prevLatest === null ? Infinity : (b.m - prevLatest)/1000,
        // earliest the previous request could have been (:00) vs latest this
        // bucket's first request could be (:59) -> an UPPER bound
        maxGap: prevEarliest === null ? Infinity : (b.m + 59000 - prevEarliest)/1000,
      });
      prevLatest = b.m + 59000; prevEarliest = b.m;
    }
  }

  // The sweep brackets the eviction point E: it survived L, and (if it was seen)
  // was evicted at D, so L < E <= D.
  const L = MAXG ? Math.max(...curve.filter(c=>c.s>0).map(c=>c.g)) : 0;
  const died = curve.filter(c=>c.s<1);
  const D = died.length ? Math.min(...died.map(c=>c.g)) : null;
  const bracket = {
    L_survived: L, D_evicted: D,
    // certainly cold: the gap was certainly > D >= E
    lowerCertainCold: D ? +(seq.filter(b=>b.minGap > D).length/total).toFixed(4) : null,
    // certainly warm: the gap was certainly <= L < E
    certainWarm: +(seq.filter(b=>b.maxGap <= L).reduce((a,b)=>a+b.n,0)/total).toFixed(4),
  };
  bracket.upperCertainCold = +(1 - bracket.certainWarm).toFixed(4);

  const structural = {};
  for (const G of GSTARS) structural[`G${G}s`] = +(seq.filter(b=>b.minGap > G).length/total).toFixed(4);

  const model = {};
  for (const tail of ['tailAlive','tailDead']) {
    const S = mkS(tail === 'tailAlive');
    const perModel = {};
    for (const m of ['poisson','uniform','burst']) {
      let acc = 0;
      for (let rep=0; rep<REPS; rep++) { let cold=0; for (const b of seq) cold += coldInBucket(b.n, b.minGap, m, S); acc += cold/total; }
      perModel[m] = +(acc/REPS).toFixed(4);
    }
    model[tail] = perModel;
  }
  const lo = Math.min(...Object.values(model.tailDead)), hi = Math.max(...Object.values(model.tailAlive));
  out.workers[tag] = {
    requests: total, colos: byColo.size, buckets: seq.length,
    bracket, structural, model,
    coldFracRange: [+lo.toFixed(4), +hi.toFixed(4)],
    penaltyMsPerRequestRange: [+(lo*9).toFixed(2), +(hi*9).toFixed(2)],
  };
}
// ---- the proxies/ GROUP: the brief asks per directory, and proxies/ is three scripts
{
  const members = ['opencode-go-proxy','zen-us-proxy','summrise-relay'];
  const live = members.filter(m => out.workers[m] && out.workers[m].requests);
  const n = live.reduce((a,m)=>a+out.workers[m].requests,0);
  const key = (m,f) => f(out.workers[m]);
  const agg = (f) => +(live.reduce((a,m)=>a+key(m,f),0)/n).toFixed(4);
  const structural = {};
  for (const G of GSTARS) structural[`G${G}s`] = agg(w => w.structural[`G${G}s`]*w.requests);
  const model = {};
  for (const tail of ['tailDead','tailAlive']) {
    model[tail] = {};
    for (const mm of ['poisson','uniform','burst']) model[tail][mm] = agg(w => w.model[tail][mm]*w.requests);
  }
  const lo = Math.min(...Object.values(model.tailDead)), hi = Math.max(...Object.values(model.tailAlive));
  out.workers['proxies/ GROUP'] = {
    requests: n, scripts: live, colos: null, buckets: live.reduce((a,m)=>a+out.workers[m].buckets,0),
    bracket: { L_survived: out.workers[live[0]].bracket.L_survived, D_evicted: out.workers[live[0]].bracket.D_evicted,
               lowerCertainCold: out.workers[live[0]].bracket.D_evicted ? agg(w=>w.bracket.lowerCertainCold*w.requests) : null,
               upperCertainCold: agg(w=>w.bracket.upperCertainCold*w.requests) },
    structural, model, coldFracRange: [+lo.toFixed(4), +hi.toFixed(4)],
    penaltyMsPerRequestRange: [+(lo*9).toFixed(2), +(hi*9).toFixed(2)],
  };
}

const OUTP = `/tmp/p30frac/out/final-${HOURS}h.json`;
writeFileSync(OUTP, JSON.stringify(out,null,1));
console.log(`wrote ${OUTP}`);
console.log(`window=${HOURS}h  measured survival curve: ${curve.map(c=>`${c.g}s:${c.s.toFixed(2)}(n=${c.n})`).join('  ')}  maxTested=${MAXG}s`);
for (const [k,v] of Object.entries(out.workers)) {
  if (!v.requests) { console.log(`${k.padEnd(20)} no requests in window`); continue; }
  console.log(`\n${k}   n=${v.requests} over ${HOURS}h, ${v.colos} colos, ${v.buckets} (minute,colo) buckets`);
  console.log(`  model-free bracket: coldFrac in [${v.bracket.lowerCertainCold ?? '?'}, ${v.bracket.upperCertainCold}]  (L=${v.bracket.L_survived}s survived, D=${v.bracket.D_evicted ?? 'not reached'}s evicted)`);
  console.log(`  structural (model-free lower bound): ${Object.entries(v.structural).map(([a,b])=>`${a}=${b}`).join(' ')}`);
  for (const tail of ['tailDead','tailAlive']) {
    const m = v.model[tail];
    console.log(`  model ${tail.padEnd(10)}: poisson=${m.poisson} uniform=${m.uniform} burst=${m.burst}   -> ms/req ${(m.poisson*9).toFixed(2)} / ${(m.uniform*9).toFixed(2)} / ${(m.burst*9).toFixed(2)}`);
  }
  console.log(`  model coldFrac range = ${v.coldFracRange[0]} .. ${v.coldFracRange[1]}   ->  ${v.penaltyMsPerRequestRange[0]} .. ${v.penaltyMsPerRequestRange[1]} ms/request`);
}
