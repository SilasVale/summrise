import { readFileSync, writeFileSync } from 'node:fs';
const TOK = readFileSync(process.env.HOME + '/.cloudflare-token','utf8').trim();
const ACCOUNT = "8e9ea6cb01f2336a2f00039a51f96c6d";
const [, , scriptName, hoursArg, tag] = process.argv;
const hours = Number(hoursArg);
const to = new Date();
const from = new Date(to.getTime() - hours*3600_000);
const Q = `
query ($tag: String!, $name: String!, $from: Time!, $to: Time!) {
  viewer { accounts(filter: { accountTag: $tag }) {
    workersInvocationsAdaptive(limit: 10000
      filter: { scriptName: $name, datetimeMinute_gt: $from, datetimeMinute_leq: $to }
      orderBy: [datetimeMinute_ASC]) {
      sum { requests cpuTimeUs errors subrequests }
      quantiles { cpuTimeP50 cpuTimeP99 }
      max { cpuTime } min { cpuTime }
      dimensions { datetimeMinute coloCode scriptVersion }
    }
  } }
}`;
const call = async (f) => {
  const r = await fetch("https://api.cloudflare.com/client/v4/graphql", {
    method:'POST', headers:{Authorization:`Bearer ${TOK}`,'Content-Type':'application/json'},
    body: JSON.stringify({query:Q, variables:{tag:ACCOUNT,name:scriptName,from:f.toISOString(),to:to.toISOString()}})});
  const j = await r.json();
  if (j.errors) throw new Error(JSON.stringify(j.errors).slice(0,400));
  return j.data.viewer.accounts[0].workersInvocationsAdaptive ?? [];
};
let all = [], cursor = from, pages = 0, truncated = false;
while (pages++ < 20) {
  const rows = await call(cursor);
  if (!rows.length) break;
  all = all.concat(rows);
  if (rows.length < 10000) break;
  truncated = true;
  cursor = new Date(new Date(rows[rows.length-1].dimensions.datetimeMinute).getTime());
  console.log(`  page ${pages}: ${rows.length} rows, cursor -> ${cursor.toISOString()}`);
}
const req = all.reduce((a,x)=>a+(x.sum.requests??0),0);
const cpu = all.reduce((a,x)=>a+(x.sum.cpuTimeUs??0),0);
writeFileSync(`/tmp/p30frac/out/${tag}.json`, JSON.stringify({scriptName, hours, from:from.toISOString(), to:to.toISOString(), pages, rows:all}));
console.log(`${scriptName.padEnd(20)} hours=${hours} pages=${pages} rows=${all.length} requests=${req} meanCpuUs=${req?(cpu/req).toFixed(1):'-'}`);
