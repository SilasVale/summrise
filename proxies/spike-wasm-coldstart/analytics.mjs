#!/usr/bin/env node
// analytics.mjs — CPU time per request, from Cloudflare's own analytics.
//
// `wrangler tail` cannot be used on this box: its websocket is reset
// (ECONNRESET, measured), so the per-request `cpuTime` a tail would print is
// not available here. The Workers dashboard's numbers come from this same
// GraphQL dataset, which IS reachable.
//
//   CLOUDFLARE_API_TOKEN=... node analytics.mjs coldstart-spike-rust 60
//
// The last argument is the window in minutes, ending now.

const ACCOUNT = "8e9ea6cb01f2336a2f00039a51f96c6d";
const token = process.env.CLOUDFLARE_API_TOKEN;
if (!token) throw new Error("CLOUDFLARE_API_TOKEN is required");

const [scriptName, minutesArg] = process.argv.slice(2);
if (!scriptName) throw new Error("usage: analytics.mjs <scriptName> [minutes]");
const minutes = Number(minutesArg ?? 60);

const to = new Date();
const from = new Date(to.getTime() - minutes * 60_000);

const query = `
query ($tag: String!, $name: String!, $from: Time!, $to: Time!) {
  viewer {
    accounts(filter: { accountTag: $tag }) {
      workersInvocationsAdaptive(
        limit: 1000
        filter: { scriptName: $name, datetime_geq: $from, datetime_leq: $to }
        orderBy: [datetime_ASC]
      ) {
        sum { requests subrequests errors }
        quantiles { cpuTimeP50 cpuTimeP90 cpuTimeP99 wallTimeP50 wallTimeP99 }
        dimensions { scriptName datetime status }
      }
    }
  }
}`;

const res = await fetch("https://api.cloudflare.com/client/v4/graphql", {
  method: "POST",
  headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
  body: JSON.stringify({ query, variables: { tag: ACCOUNT, name: scriptName, from: from.toISOString(), to: to.toISOString() } }),
});
const json = await res.json();
if (json.errors) {
  console.log(JSON.stringify({ scriptName, windowMinutes: minutes, errors: json.errors }, null, 1));
  process.exit(1);
}
const rows = json.data.viewer.accounts[0].workersInvocationsAdaptive ?? [];
const totals = rows.reduce(
  (a, r) => ({
    requests: a.requests + (r.sum.requests ?? 0),
    subrequests: a.subrequests + (r.sum.subrequests ?? 0),
    errors: a.errors + (r.sum.errors ?? 0),
  }),
  { requests: 0, subrequests: 0, errors: 0 },
);
console.log(
  JSON.stringify(
    {
      scriptName,
      windowMinutes: minutes,
      from: from.toISOString(),
      to: to.toISOString(),
      totals,
      // Each row is one minute-bucket; `cpuTime` quantiles are in milliseconds.
      buckets: rows.map((r) => ({
        at: r.dimensions.datetime,
        status: r.dimensions.status,
        requests: r.sum.requests,
        errors: r.sum.errors,
        cpuTimeP50: r.quantiles.cpuTimeP50,
        cpuTimeP90: r.quantiles.cpuTimeP90,
        cpuTimeP99: r.quantiles.cpuTimeP99,
        wallTimeP50: r.quantiles.wallTimeP50,
        wallTimeP99: r.quantiles.wallTimeP99,
      })),
    },
    null,
    1,
  ),
);
