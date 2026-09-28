#!/usr/bin/env node
// tail/run.mjs — does an isolate survive gaps far longer than the sweep reaches?
//
// The sweep's largest gap is 600 s. The proxy workers' requests are separated by
// MINUTES TO HOURS, so the sweep cannot speak for them — and the tail is exactly
// where their answer lives. Three separate scripts (three isolate pools) are
// tested in PARALLEL, one gap each; a single script could not do it, because each
// request refreshes the isolate the next test needs to find dead.
import https from 'node:https';
import { writeFileSync } from 'node:fs';

const GAPS = [900, 1800, 3600];
const OUT = '/tmp/p30frac/out/tail.json';
const results = {};
const save = () => writeFileSync(OUT, JSON.stringify({ updated: new Date().toISOString(), results }, null, 1));
const sleep = ms => new Promise(r => setTimeout(r, ms));

const get = (url) => new Promise((res, rej) => {
  const agent = new https.Agent({ keepAlive: false });
  const req = https.get(url, { agent, timeout: 30000 }, r => {
    let b = ''; r.on('data', c => (b += c));
    r.on('end', () => { agent.destroy(); try { res({ ...JSON.parse(b), colo: r.headers['cf-ray']?.split('-').pop() ?? null }); } catch (e) { rej(e); } });
  });
  req.on('timeout', () => req.destroy(new Error('timeout')));
  req.on('error', rej);
});

async function test(gap) {
  const url = `https://p30tail${gap}.saisi.online/probe`;
  const a = await get(url);
  results[gap] = { gapS: gap, a: { colo: a.colo, id: a.isolateId, idx: a.requestIndex }, startedAt: new Date().toISOString() };
  save();
  console.log(`gap=${gap}s  A colo=${a.colo} id=${a.isolateId.slice(0,8)} idx=${a.requestIndex}`);
  await sleep(gap * 1000);
  for (let i = 1; i <= 8; i++) {
    const b = await get(url);
    if (b.colo !== a.colo) { console.log(`gap=${gap}s  try${i} colo=${b.colo} != ${a.colo} — retry`); await sleep(2000); continue; }
    results[gap] = { ...results[gap], b: { colo: b.colo, id: b.isolateId, idx: b.requestIndex, ageMs: b.isolateAgeMs },
      survived: b.isolateId === a.isolateId, finishedAt: new Date().toISOString() };
    save();
    console.log(`gap=${gap}s  B colo=${b.colo} id=${b.isolateId.slice(0,8)} idx=${b.requestIndex}  -> ${b.isolateId === a.isolateId ? 'SURVIVED' : 'EVICTED'}`);
    return;
  }
  results[gap] = { ...results[gap], error: 'no same-colo probe after 8 tries' }; save();
}

await Promise.all(GAPS.map(g => test(g).catch(e => { results[g] = { gapS: g, error: e.message }; save(); console.log(`gap=${g}s ERROR ${e.message}`); })));
console.log('\n' + JSON.stringify(results, null, 1));
