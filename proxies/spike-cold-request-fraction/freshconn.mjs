// Does a FRESH connection reuse a warm isolate, or does every fresh connection
// get its own? The 240 s control cannot tell those apart on its own: at t=480 s
// the old isolate might have died, or fresh connections might never be routed to
// it. This separates them by asking for a fresh connection ONE SECOND after a
// request — far below any eviction point — and requiring the same colo.
import https from 'node:https';
const URL_ = 'https://p30frac.saisi.online/probe';
const sleep = ms => new Promise(r=>setTimeout(r,ms));
const get = () => new Promise((res,rej)=>{
  const agent = new https.Agent({ keepAlive:false });
  const req = https.get(URL_, { agent, timeout:20000 }, r => {
    let b=''; r.on('data',c=>b+=c);
    r.on('end',()=>{ agent.destroy(); try{ res({...JSON.parse(b), colo:r.headers['cf-ray']?.split('-').pop()}); }catch(e){rej(e);} });
  });
  req.on('timeout',()=>req.destroy(new Error('timeout'))); req.on('error',rej);
});
const seen = [];
for (let i=0;i<6;i++) {
  const j = await get();
  seen.push({i, colo:j.colo, id:j.isolateId.slice(0,8), idx:j.requestIndex, age:j.isolateAgeMs});
  console.log(`fresh#${i} colo=${j.colo} id=${j.isolateId.slice(0,8)} idx=${j.requestIndex} age=${j.isolateAgeMs}ms`);
  await sleep(1000);
}
const lhr = seen.filter(s=>s.colo==='LHR');
console.log(`\nLHR samples: ${lhr.length}, distinct isolates among them: ${new Set(lhr.map(s=>s.id)).size}`);
console.log(`distinct colos: ${new Set(seen.map(s=>s.colo)).size}`);
