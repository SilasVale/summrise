#!/usr/bin/env node
// http-route-header-check — EVERY ROUTE THE HEADER NAMES MUST EXIST IN THE TABLE IT POINTS AT.
//
// WHY THIS EXISTS (rounds 146-150). `agent/src/web/mod.rs` is 8,302 lines and the largest file in the agent. Its authority is
// the table of `Pattern::Exact` / `Pattern::Prefix` rows that `route_of` resolves — 33 + 5 = 38 patterns. Its HEADER said
// `Routes:` and named TEN, so for as long as anyone read the header as the list, 23 routes were invisible to a reader:
// `/api/settings`, `/api/monitors` and its three verbs, `/api/logs`, `/api/boots`, `/api/sessions`, `/api/vitals/history`,
// `/api/update`, `/api/run/mark-exit` — most of what the panel calls. Round 147 made the header say `A SELECTION, NOT THE
// INVENTORY` and point at the table.
//
// SO ONLY ONE DIRECTION REMAINS CHECKABLE, and this is it: a header that NAMES a route the device does not serve sends a reader
// looking for something that is not there, or "fixing" one that is. The reverse direction is deliberately NOT asserted — the
// header is a selection by design, and asserting it would make every new table row a header edit.
//
// THE HEADER WRITES ITS ROUTES AS PROSE, which is why the extraction below is not a single regex: `GET /panel, /panel/` and
// `POST /api/plugins/playwright/start|stop` are one line each, and a naive match takes the comma and the pipe literally
// (rounds 146 and 147 each recorded that mistake). Splitting on `,`/`|`/whitespace is the fix, and a route that still contains
// `{` or `}` is a template the table spells differently, so it is matched by prefix.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const SRC = join(ROOT, "agent/src/web/mod.rs");
const text = readFileSync(SRC, "utf8");

// 1. the routes the header names, however it spells them
const head = text.slice(0, text.indexOf("\nuse "));
const named = [];
for (const line of head.split("\n")) {
  const m = /^\/\/!\s+(?:GET|POST|DELETE|PUT|ANY)\s+(.+)$/.exec(line.trim());
  if (!m) continue;
  const body = m[1].split("→")[0];                       // drop the "→ explanation" tail
  for (const piece of body.split(/[,\s|]+/).filter(Boolean)) {
    if (piece.startsWith("/")) named.push(piece);
  }
}
if (named.length === 0) {
  console.error("http-route-header: no routes found in the header — did its shape change? The check cannot pass vacuously.");
  process.exit(1);
}

// 2. the patterns the table actually resolves
const exact = [...text.matchAll(/Pattern::Exact\("([^"]+)"\)/g)].map((m) => m[1]);
const prefix = [...text.matchAll(/Pattern::Prefix\("([^"]+)"\)/g)].map((m) => m[1]);
if (exact.length === 0) {
  console.error("http-route-header: no Pattern rows found — the table moved or was renamed, and this check cannot pass vacuously.");
  process.exit(1);
}

const covered = (route) =>
  exact.includes(route.replace(/\/$/, "")) ||
  exact.includes(route) ||
  prefix.some((p) => route.startsWith(p)) ||
  prefix.some((p) => p.startsWith(route.replace(/\{.*\}$/, "")));

const missing = named.filter((r) => !covered(r));
if (missing.length) {
  console.error(`http-route-header: the header names ${missing.length} route(s) no Pattern row resolves — a reader is sent looking for something the device does not serve:`);
  for (const r of missing) console.error(`  ${r}`);
  process.exit(1);
}

console.log(`http-route-header: ok — ${named.length} header route(s) all resolve against ${exact.length} Exact + ${prefix.length} Prefix pattern(s)`);
