#!/usr/bin/env node
// A TEST THAT MOCKS lib/api WHOLESALE REPLACES EVERY EXPORT — so the next export added there is
// `undefined` at the call site, and a hook that catches its own read failures turns that TypeError into
// "a failed read must not update", which is indistinguishable from correct behaviour.
//
// That is not hypothetical (round 87). Adding ONE export (`deviceRefused`) turned three passing tests red
// with an EMPTY VALUE rather than an error, including two whose fixtures carry `ok: true`, because their
// `vi.mock("../../lib/api", () => ({ callApi: vi.fn() }))` answered `undefined` to the hook's new call.
// `tsc` cannot see it: a `vi.mock` factory is never checked against the module it replaces.
//
// THE RULE: a factory for `lib/api` must SPREAD THE ORIGINAL. Mock the transport, keep the module.
//
// AND `lib/api` IS THE ONLY MODULE THIS SUITE MOCKS AT ALL — measured, not assumed: twenty `vi.mock` calls
// exist in the panel and every one targets it (nineteen as "../../lib/api", one as "./api" from
// `boot.test.ts`). That is what makes a check scoped to one module the whole surface rather than a
// sample of it, and it is why widening this gate is not a TODO: there is nothing else to widen to.
//
// AND NOT IN THE OTHER UI EITHER: `gateway/ui` — the console, which the sibling gates cover alongside the
// panel — has ZERO `vi.mock` calls. It tests through render smokes and node tests, so the trap this gate
// exists for cannot occur there; the question "should this cover the console?" is answered rather than
// left open.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const ROOT = "agent/resources/panel-react/src";
// The spread-form factories that exist as of this gate's writing. A FLOOR, because a check that finds
// nothing because the idiom was renamed is worse than no check — it reports success having looked at
// nothing, which is the failure `e2e-only-check` exists to catch in its own domain.
const FLOOR = 15;

function walk(dir, out = []) {
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (/\.(ts|tsx)$/.test(p)) out.push(p);
  }
  return out;
}

// COMMENTS ARE STRIPPED FIRST, AND THIS GATE'S FIRST RUN IS WHY: it reported `lib/api.ts` — the module
// itself — because the predicate's own doc QUOTES the wholesale idiom it is warning about. A check that
// reads prose as code reports the explanation as the defect, which is the same trap `exports-check` and
// `retired-colours-check` each recorded on their own first run.
function stripComments(src) {
  return src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^[ \t]*\/\/.*$/gm, "");
}

const whole = [];
let spread = 0;
for (const f of walk(ROOT)) {
  const src = stripComments(readFileSync(f, "utf8"));
  // `vi.mock("<something>/api", (` — the factory's own parameter is what separates the two idioms:
  // `async (importOriginal) => ({ ...(await importOriginal…), … })` versus `() => ({ … })`.
  for (const m of src.matchAll(/vi\.mock\(\s*["']([^"']*\/api|\.\/api)["']\s*,\s*(async\s*)?\(/g)) {
    if (m[2]) spread += 1;
    else whole.push(`${f}: vi.mock("${m[1]}") does not spread the original`);
  }
}

if (spread < FLOOR) {
  console.error(
    `  read ${spread} spread-form vi.mock(s) of lib/api, expected at least ${FLOOR} — the idiom may have been renamed, and this check would then pass having examined nothing`,
  );
  process.exit(2);
}
if (whole.length) {
  console.error(
    "  A WHOLE-MODULE MOCK OF lib/api REPLACES EVERY EXPORT — the next one added there vanishes silently:",
  );
  for (const w of whole) console.error("    " + w);
  process.exit(1);
}
console.log(
  `  ok — ${spread} vi.mock(s) of lib/api spread the original; none replaces the module wholesale`,
);
