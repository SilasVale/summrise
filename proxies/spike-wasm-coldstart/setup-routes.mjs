#!/usr/bin/env node
// setup-routes.mjs — make the throwaway workers reachable from this box.
//
// WHY: every other trigger is closed here, each measured:
//   * `*.workers.dev`            — TLS reset (SNI blocked)
//   * `wrangler tail`            — ECONNRESET: tail.developers.workers.dev is blocked
//   * `wrangler dev --remote`    — the bundled workerd needs GLIBC >= 2.32, this box has 2.31
//   * the driver's cron trigger  — registered, zero invocations in 10 minutes
// `*.saisi.online` IS reachable, so a temporary proxied DNS record plus Workers
// routes is the one door left. Everything created here is deleted by
// teardown.mjs, which is run in the same session.
//
//   node setup-routes.mjs          create
//   node setup-routes.mjs --delete remove

const ZONE = "79860b60a8681147d0ce38294d404ead";
const HOST = "coldstart.saisi.online";
const token = process.env.CLOUDFLARE_API_TOKEN;
if (!token) throw new Error("CLOUDFLARE_API_TOKEN is required");

const api = async (method, path, body) => {
  const r = await fetch(`https://api.cloudflare.com/client/v4${path}`, {
    method,
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: body ? JSON.stringify(body) : undefined,
  });
  const j = await r.json();
  if (!j.success) throw new Error(`${method} ${path} -> ${JSON.stringify(j.errors)}`);
  return j.result;
};

const ROUTES = [
  { pattern: `${HOST}/ts/*`, script: "coldstart-spike-ts" },
  { pattern: `${HOST}/rust/*`, script: "coldstart-spike-rust" },
  { pattern: `${HOST}/driver/*`, script: "coldstart-spike-driver" },
];

if (process.argv.includes("--delete")) {
  const routes = await api("GET", `/zones/${ZONE}/workers/routes`);
  for (const r of routes.filter((r) => r.pattern.startsWith(HOST))) {
    await api("DELETE", `/zones/${ZONE}/workers/routes/${r.id}`);
    console.log("deleted route", r.pattern);
  }
  const records = await api("GET", `/zones/${ZONE}/dns_records?name=${HOST}`);
  for (const rec of records) {
    await api("DELETE", `/zones/${ZONE}/dns_records/${rec.id}`);
    console.log("deleted dns record", rec.type, rec.name);
  }
  const left = await api("GET", `/zones/${ZONE}/dns_records?name=${HOST}`);
  const leftRoutes = (await api("GET", `/zones/${ZONE}/workers/routes`)).filter((r) => r.pattern.startsWith(HOST));
  console.log(`VERIFY: dns_records=${left.length} routes=${leftRoutes.length} (both must be 0)`);
  process.exit(0);
}

const existing = await api("GET", `/zones/${ZONE}/dns_records?name=${HOST}`);
if (!existing.length) {
  const rec = await api("POST", `/zones/${ZONE}/dns_records`, {
    type: "AAAA",
    name: HOST,
    content: "100::",
    proxied: true,
    ttl: 1,
    comment: "TEMPORARY: P3.0 wasm cold-start measurement (throwaway workers); delete after",
  });
  console.log("created dns record", rec.type, rec.name, "->", rec.content, "proxied", rec.proxied);
} else {
  console.log("dns record already present:", existing[0].type, existing[0].name);
}

const have = await api("GET", `/zones/${ZONE}/workers/routes`);
for (const want of ROUTES) {
  const found = have.find((r) => r.pattern === want.pattern);
  if (found) {
    console.log("route already present:", found.pattern, "->", found.script);
    continue;
  }
  const made = await api("POST", `/zones/${ZONE}/workers/routes`, want);
  console.log("created route", made.pattern, "->", made.script);
}
