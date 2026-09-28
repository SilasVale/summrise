#!/usr/bin/env node
// control.mjs — the one confound the survival sweep cannot rule out by itself.
//
// The sweep holds ONE keep-alive connection (it must: without it the requests
// alternate colos, and two colos are two isolate pools). If Cloudflare pinned an
// isolate to a connection, every "SURVIVED" in the sweep would be an artifact of
// the socket rather than of the isolate pool, and the survival curve would be an
// overestimate. So: after a gap already known to be survivable, ask for the same
// isolate on a FRESH connection, retrying until the fresh connection lands in the
// same colo (a different colo proves nothing either way).
import https from 'node:https';
import { writeFileSync } from 'node:fs';

const URL_ = 'https://p30frac.saisi.online/probe';
const GAP_S = Number(process.argv[2] ?? 120);
const keep = new https.Agent({ keepAlive: true, maxSockets: 1, maxFreeSockets: 1 });

const get = (agent) => new Promise((res, rej) => {
  const req = https.get(URL_, { agent, timeout: 30000 }, r => {
    let b = ''; r.on('data', c => (b += c));
    r.on('end', () => { try { res({ ...JSON.parse(b), colo: r.headers['cf-ray']?.split('-').pop() ?? null }); } catch (e) { rej(e); } });
  });
  req.on('timeout', () => req.destroy(new Error('timeout')));
  req.on('error', rej);
});
const sleep = ms => new Promise(r => setTimeout(r, ms));

const trials = [];
// A: keep-alive connection establishes / confirms an isolate
const a = await get(keep);
console.log(`A  keep-alive  colo=${a.colo} id=${a.isolateId.slice(0,8)} idx=${a.requestIndex}`);
await sleep(GAP_S * 1000);

// B: same gap, SAME connection
const b = await get(keep);
console.log(`B  same conn   colo=${b.colo} id=${b.isolateId.slice(0,8)} idx=${b.requestIndex}  -> ${b.isolateId===a.isolateId?'SAME isolate':'DIFFERENT'}`);
trials.push({ arm: 'same-connection', gapS: GAP_S, coloA: a.colo, coloB: b.colo, same: b.isolateId === a.isolateId, idA: a.isolateId, idB: b.isolateId });

// C: same gap, FRESH connection, retried until the colo matches
await sleep(GAP_S * 1000);
let c = null;
for (let i = 1; i <= 6; i++) {
  const fresh = new https.Agent({ keepAlive: false });
  const r = await get(fresh);
  fresh.destroy();
  console.log(`C${i} fresh conn  colo=${r.colo} id=${r.isolateId.slice(0,8)} idx=${r.requestIndex}${r.colo === a.colo ? '' : '  (colo differs — retry)'}`);
  if (r.colo === a.colo) { c = r; break; }
  await sleep(1500);
}
if (c) trials.push({ arm: 'fresh-connection', gapS: GAP_S, coloA: a.colo, coloB: c.colo, same: c.isolateId === a.isolateId, idA: a.isolateId, idB: c.isolateId, requestIndex: c.requestIndex });

writeFileSync('/tmp/p30frac/out/control.json', JSON.stringify({ gapS: GAP_S, trials }, null, 1));
console.log('\n' + JSON.stringify(trials, null, 1));
