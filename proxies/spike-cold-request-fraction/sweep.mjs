#!/usr/bin/env node
// sweep.mjs — how long does an IDLE Workers isolate survive?
//
// The cold-request fraction of a Worker is decided by ONE parameter the
// analytics dataset does not carry: the idle gap after which Cloudflare evicts
// an isolate and the next request pays a fresh start. `workersInvocationsAdaptive`
// has no isolate dimension, so it must be measured directly — a throwaway probe
// whose isolate states its own identity, and a gap sweep against it.
//
// Two confounds found while building this, both handled here:
//   * a request from this box can land in more than one colo (measured: AMS and
//     LHR alternating), and two colos are two isolate pools — so a "new isolate"
//     after a gap can be an anycast hop, not an eviction. A keep-alive agent with
//     maxSockets 1 pins the colo; every pair is still required to agree on colo,
//     and a pair that does not is retried rather than counted.
//   * the TCP connection can drop across a long gap and the replacement can land
//     in another colo — same check, same retry.
//
//   node sweep.mjs
import https from 'node:https';
import { writeFileSync } from 'node:fs';

const URL_ = 'https://p30frac.saisi.online/probe';
const OUT = '/tmp/p30frac/out/sweep.json';
const agent = new https.Agent({ keepAlive: true, maxSockets: 1, maxFreeSockets: 1 });

const get = () => new Promise((res, rej) => {
  const req = https.get(URL_, { agent, timeout: 30000 }, r => {
    let b = ''; r.on('data', c => (b += c)); r.on('end', () => { try { res(JSON.parse(b)); } catch (e) { rej(e); } });
  });
  req.on('timeout', () => req.destroy(new Error('timeout')));
  req.on('error', rej);
});
const sleep = ms => new Promise(r => setTimeout(r, ms));

// gap seconds -> how many independent trials at that gap
const PLAN = [[1,3],[3,3],[7,3],[15,3],[30,2],[60,2],[120,2],[240,1],[600,1]];
const MAX_ATTEMPTS = 4;

const trials = [];
const started = new Date().toISOString();
const t0 = Date.now();
const save = () => writeFileSync(OUT, JSON.stringify({ started, updated: new Date().toISOString(), elapsedS: (Date.now()-t0)/1000, plan: PLAN, trials }, null, 1));

for (const [gap, reps] of PLAN) {
  for (let rep = 0; rep < reps; rep++) {
    let done = false;
    for (let attempt = 1; attempt <= MAX_ATTEMPTS && !done; attempt++) {
      try {
        const a = await get();
        await sleep(gap * 1000);
        const b = await get();
        const sameColo = a.colo === b.colo;
        const rec = {
          gapS: gap, rep, attempt, coloA: a.colo, coloB: b.colo, sameColo,
          idA: a.isolateId, idB: b.isolateId,
          survived: sameColo ? a.isolateId === b.isolateId : null,
          bRequestIndex: b.requestIndex, bIsolateAgeMs: b.isolateAgeMs,
          at: new Date().toISOString(),
        };
        trials.push(rec); save();
        console.log(`gap=${String(gap).padStart(3)}s rep=${rep} try=${attempt} colo=${a.colo}->${b.colo} ${sameColo ? (rec.survived ? 'SURVIVED' : 'EVICTED ') : 'colo-hop (invalid)'} b.idx=${b.requestIndex} b.age=${b.isolateAgeMs}ms`);
        if (sameColo) done = true;
      } catch (e) {
        console.log(`gap=${gap}s rep=${rep} try=${attempt} ERROR ${e.message}`);
        trials.push({ gapS: gap, rep, attempt, error: e.message, at: new Date().toISOString() }); save();
      }
    }
  }
}
save();
console.log('\n=== SURVIVAL CURVE ===');
for (const [gap] of PLAN) {
  const v = trials.filter(t => t.gapS === gap && t.sameColo);
  const s = v.filter(t => t.survived).length;
  console.log(`  gap ${String(gap).padStart(3)}s : survived ${s}/${v.length}`);
}
console.log(`elapsed ${((Date.now()-t0)/1000).toFixed(0)}s`);
