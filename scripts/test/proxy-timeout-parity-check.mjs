#!/usr/bin/env node
// ONE DOCUMENTED NUMBER, SEVEN IMPLEMENTATIONS — and the documentation is the only thing holding them.
//
// `proxies/README.md:13` says "Upstream fetches carry a 30s timeout". Measured (round 122 of the standing
// goal, from the eleventh exploration): that single sentence is implemented THREE ways in seven places —
//
//   * the relay's three TypeScript handlers read `SUMMRISE_RELAY_HEADER_TIMEOUT_MS ?? 30000`, so they are
//     the only ones an operator can move;
//   * the relay's two legacy JS handlers (`api/zen.js`, `api/proxy.js`) hardcode a module const;
//   * BOTH zen proxies hardcode their own const, and they are SEPARATE WORKERS — nothing shares a module
//     across a Worker boundary, so each is independently editable.
//
// THE RISK IS NOT THE TIDINESS. A change to one of these is invisible to the other six: the README keeps
// stating one number, each surface keeps working, and two callers of the same upstream get different
// budgets depending on which route they took. This gate reads all seven and fails when the numbers stop
// agreeing, which is the check the comment never was.
import { readFileSync } from "node:fs";

const CANONICAL = 30000;

/** [file, how to find the number, whether an env override is allowed here] */
const SITES = [
  ["proxies/api-relay/api/git.ts", /SUMMRISE_RELAY_HEADER_TIMEOUT_MS\s*\?\?\s*(\d+)/, true],
  ["proxies/api-relay/api/github.ts", /SUMMRISE_RELAY_HEADER_TIMEOUT_MS\s*\?\?\s*(\d+)/, true],
  ["proxies/api-relay/api/gform.ts", /SUMMRISE_RELAY_HEADER_TIMEOUT_MS\s*\?\?\s*(\d+)/, true],
  ["proxies/api-relay/api/zen.js", /const HEADER_TIMEOUT_MS = (\d+);/, false],
  ["proxies/api-relay/api/proxy.js", /const HEADER_TIMEOUT_MS = (\d+);/, false],
  ["proxies/zen-go-proxy/src/index.js", /const HEADER_TIMEOUT_MS = (\d+);/, false],
  ["proxies/zen-us-proxy/src/index.js", /const HEADER_TIMEOUT_MS = (\d+);/, false],
];

const bad = [];
let read = 0;
for (const [file, re, envOk] of SITES) {
  let src;
  try {
    src = readFileSync(file, "utf8");
  } catch {
    bad.push(`${file}: cannot be read — the site moved or was deleted`);
    continue;
  }
  const m = src.match(re);
  if (!m) {
    bad.push(`${file}: no header timeout found (expected ${envOk ? "the env form" : "a module const"})`);
    continue;
  }
  read += 1;
  const value = Number(m[1]);
  if (value !== CANONICAL) {
    bad.push(`${file}: ${value} ms, where the documented budget is ${CANONICAL}`);
  }
}

// A FLOOR, because a gate that reads nothing because its patterns went stale is worse than no gate: the
// five `exports-check`-style scans in this repo have each failed this way exactly once.
if (read < SITES.length) {
  console.error(
    `  FAIL read ${read} of ${SITES.length} timeout site(s) — the patterns are stale, so this proves nothing:`,
  );
  for (const b of bad) console.error("    " + b);
  process.exit(2);
}
if (bad.length) {
  console.error(
    `  FAIL the header budget disagrees with the documented ${CANONICAL} ms, and one number is what the README states:`,
  );
  for (const b of bad) console.error("    " + b);
  process.exit(1);
}
console.log(
  `  ok — ${read} header-timeout site(s) across ${new Set(SITES.map((s) => s[0].split("/").slice(0, 2).join("/"))).size} deployment unit(s) all state ${CANONICAL} ms, the number proxies/README.md documents`,
);
