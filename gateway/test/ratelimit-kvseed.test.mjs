// ── the rate limiter's `kvSeed` flag is load-bearing, and its placement was a
//    claim nothing checked ─────────────────────────────────────────────────────
//
// `src/lib/ratelimit.ts` enumerates its own call sites: "kvSeed: true (probe)"
// and "kvSeed: false (auth register, devices public gate)". The flag is not a
// preference — round-104's note is explicit that those two endpoints "cost 2-3 KV
// writes per attempt themselves; a per-request KV write HERE would let an attacker
// exhaust the Free-plan daily KV write quota."
//
// So the dangerous edit is one WORD — adding `kvSeed: true` to auth or devices —
// and it would pass every existing test while reintroducing the quota vector the
// doc names. Nothing read the call sites; this does.
//
// It reads the three sources as data rather than restating them, and asserts the
// POSITIVE too (probe still seeds), so moving the flag away from probe is equally
// refused — the doc says that one WANTS the KV seed.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const ROOT = fileURLToPath(new URL("..", import.meta.url));

/** The option object literal passed to `createIpRateLimiter(…)` in one file. */
function limiterOptions(rel) {
  const src = readFileSync(join(ROOT, rel), "utf8");
  const at = src.indexOf("createIpRateLimiter({");
  if (at === -1) return null;
  // Balanced-brace scan, so a nested object or a comment cannot truncate it.
  let depth = 0;
  let end = at + "createIpRateLimiter".length;
  for (let i = src.indexOf("{", at); i < src.length; i++) {
    if (src[i] === "{") depth++;
    else if (src[i] === "}") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  const body = src.slice(at, end);
  const name = body.match(/name:\s*"([^"]+)"/)?.[1];
  return { body, name, kvSeed: /kvSeed:\s*true/.test(body) };
}

test("ratelimit: only the probe limiter seeds its counter from KV", () => {
  const probe = limiterOptions("src/tooling.ts");
  const auth = limiterOptions("src/plugins/auth.ts");
  const devices = limiterOptions("src/plugins/devices.ts");

  for (const [label, o] of [
    ["src/tooling.ts", probe],
    ["src/plugins/auth.ts", auth],
    ["src/plugins/devices.ts", devices],
  ]) {
    assert.ok(o, `${label} no longer creates an IP rate limiter — if a call site moved, ` +
      `move it in the ledger and in src/lib/ratelimit.ts's enumeration too`);
  }

  assert.equal(
    probe.kvSeed,
    true,
    "src/tooling.ts's probe limiter must KEEP kvSeed: true — the doc says that bucket's " +
      "first sight per IP reads and persists KV so a new isolate inherits the budget",
  );

  for (const [label, o] of [
    ["src/plugins/auth.ts", auth],
    ["src/plugins/devices.ts", devices],
  ]) {
    assert.equal(
      o.kvSeed,
      false,
      `${label}'s limiter gained \`kvSeed: true\`. Those two endpoints already cost 2-3 KV ` +
        `writes per attempt themselves, so a per-request KV write here lets an attacker ` +
        `exhaust the Free-plan daily KV write quota — that is round-104's finding, and ` +
        `src/lib/ratelimit.ts records the per-isolate ceiling as the ACCEPTED trade. ` +
        `If this is deliberate, change the doc's enumeration in the same commit.`,
    );
  }
});

test("ratelimit: the three named buckets are the ones the doc enumerates", () => {
  const names = [
    limiterOptions("src/tooling.ts").name,
    limiterOptions("src/plugins/auth.ts").name,
    limiterOptions("src/plugins/devices.ts").name,
  ].sort();
  assert.deepEqual(
    names,
    ["auth-rate", "probe-rate", "pub-rate"],
    "the bucket names no longer match src/lib/ratelimit.ts's enumeration " +
      "(probe / auth register / devices public gate). A fourth bucket appearing is a " +
      "policy decision — give it a line in that header rather than leaving the list stale.",
  );
});
