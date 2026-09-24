#!/usr/bin/env node
// A COMMENT THAT ASSERTS PARITY IS NOT A COMPARISON.
//
// Five files carry the same CORS allowlist: the gateway — which EXPORTS it and lets config override it —
// and four proxies that restate it. Every copy's comment says it matches the others ("the console origins
// used in this repo"), and NOTHING compared them. The one test whose name claims to be the guard
// (`proxies/api-relay/api/test/proxy-gate.test.mjs:53`, "CORS matrix on the autonomous copy (drift guard
// vs gateway http.ts)") imports `../proxy.js` and no gateway file at all — it checks the copy against
// itself, which is the twelfth instance of this repository's recurring shape: an instrument whose name
// promises more than its body does (round 116 of the standing goal, found by the eleventh exploration).
//
// WHY IT MATTERS BEYOND TIDINESS: the gateway's list can be EXTENDED BY CONFIGURATION, and the proxies'
// cannot. So an origin added for production works on the console and is refused by every proxy — a browser
// error in one place and not the other, with the four copies agreeing with each other and with a list that
// has already moved. The gateway owns the fact; the proxies must not disagree with it.
import { readFileSync } from "node:fs";

const OWNER = "gateway/src/http.ts";
const COPIES = [
  "proxies/api-relay/api/zen.js",
  "proxies/api-relay/api/proxy.js",
  "proxies/zen-go-proxy/src/index.js",
  "proxies/zen-us-proxy/src/index.js",
];

/** The string literals inside the first `new Set([...])` that follows `ALLOWED_ORIGINS`. */
function originsOf(file) {
  const src = readFileSync(file, "utf8");
  const at = src.indexOf("ALLOWED_ORIGINS");
  if (at < 0) return null;
  const set = src.indexOf("new Set([", at);
  if (set < 0) return null;
  const end = src.indexOf("])", set);
  if (end < 0) return null;
  const body = src.slice(set, end);
  // NO COMMENT STRIP HERE, AND THAT IS A CORRECTION RATHER THAN AN OMISSION. The first version stripped
  // `//...` from this slice — the lesson `exports-check` and `retired-colours-check` each recorded, that a
  // gate which reads prose as code reports the explanation as the defect. But THE STRIP ATE THE DATA:
  // every origin is `https://…`, so `//[^\n]*` deleted each URL and its closing quote with it, and the
  // check could read nothing from any file. The slice already starts at `new Set([` and ends at `])`, so
  // no comment can be inside it; the strip was protecting against a case that cannot occur, at the cost
  // of the case that always does. (Comments elsewhere in these files DO quote origins, which is why the
  // search for `ALLOWED_ORIGINS` is anchored and the slice is bounded.)
  return (body.match(/"([^"]+)"/g) || []).map((s) => s.slice(1, -1)).sort();
}

const owner = originsOf(OWNER);
if (!owner || owner.length < 2) {
  console.error(`  FAIL could not read an allowlist from ${OWNER} — this proves nothing`);
  // EXIT 3, NOT 2 (round 172). 2 is documented as "this host cannot run me" and the runner maps it to
  // `n/a` without counting a failure — but this branch MEASURED NOTHING because its subject moved, which
  // is a broken instrument rather than an exempt host. It prints FAIL and now says so in the code too.
  process.exit(3);
}

let bad = 0;
for (const f of COPIES) {
  const got = originsOf(f);
  if (!got) {
    console.error(`  FAIL ${f}: no ALLOWED_ORIGINS found — the copy moved or was renamed`);
    bad += 1;
    continue;
  }
  const missing = owner.filter((o) => !got.includes(o));
  const extra = got.filter((o) => !owner.includes(o));
  if (missing.length || extra.length) {
    console.error(`  FAIL ${f} disagrees with ${OWNER}:`);
    for (const m of missing) console.error(`    missing: ${m}`);
    for (const e of extra) console.error(`    extra:   ${e}`);
    bad += 1;
  }
}

if (bad) {
  console.error(
    `\n  ${bad} CORS allowlist copy/copies disagree with the owner. The gateway's list can be extended by\n` +
      `  configuration and the copies cannot, so a disagreement is a browser error on one surface and not\n` +
      `  the other. Fix the copy, or move the origin into the owner and re-run.`,
  );
  process.exit(1);
}
console.log(
  `  ok — ${owner.length} console origin(s) in ${OWNER}, and all ${COPIES.length} copies agree with it`,
);
