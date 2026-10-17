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
//
// **THE DEVICES CALL SITE IS RUST NOW** (the cutover moved the family to `wasm/src/ip_rate_limit.rs`), and the
// assertion follows it rather than the deleted file: the Rust limiter has no KV arm to set, so "memory-only" is
// checked as the STRUCT's field list, not as an absent flag. The other two call sites are still TypeScript and
// are still read as option literals.
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

/** The Rust limiter's own instance, read as data: `PUBLIC_RATE`'s fields and the STRUCT's field list.
 *
 *  `wasm/src/ip_rate_limit.rs` is the devices public gate's limiter since the cutover, and its header records the
 *  decision: "kvSeed IS NOT PORTED, AND THAT IS A DECISION RATHER THAN AN OMISSION … the devices plugin
 *  instantiates the memory-only one". So memory-only is not a flag left unset here — the struct has no KV field
 *  at all, and `kvSeed` below is that field's ABSENCE, which is what the assertion refuses to let appear. */
function rustLimiter(rel, konstName = "PUBLIC_RATE") {
  const src = readFileSync(join(ROOT, rel), "utf8");
  // **`pub const` OR A PLAIN `static`** — the three instances are declared two ways, and the first version of
  // this reader only knew the first: `PROBE_RATE`/`PUBLIC_RATE` are `pub const`s in `ip_rate_limit.rs`, while
  // `auth_routes.rs` keeps its own as a `static` beside the counter table it locks.
  const konst = new RegExp(
    `(?:pub )?(?:const|static) ${konstName}:\\s*IpRateLimiter\\s*=\\s*IpRateLimiter\\s*\\{([\\s\\S]*?)\\};`,
  ).exec(src);
  if (!konst) return null;
  const struct = /pub struct IpRateLimiter\s*\{([\s\S]*?)\}/.exec(src);
  return {
    body: konst[1],
    name: konst[1].match(/name:\s*"([^"]+)"/)?.[1],
    kvSeed: /kv_seed/.test(struct ? struct[1] : ""),
  };
}

/** Does the probe's ROUTE reach the seed? — the Rust's replacement for the deleted `kvSeed: true`. */
function probeRouteSeeds() {
  const src = readFileSync(join(ROOT, "wasm/src/probe.rs"), "utf8");
  return (
    src.includes("ip_rate_limit::needs_seed") &&
    src.includes("device_store::kv_put") &&
    src.includes("ip_rate_limit::seed_ttl_secs")
  );
}

test("ratelimit: only the probe limiter seeds its counter from KV", () => {
  const probe = rustLimiter("wasm/src/ip_rate_limit.rs", "PROBE_RATE");
  // **THE AUTH LIMITER IS RUST'S NOW (2026-10-17)**: `plugins/auth.ts` is deleted, and its per-IP gate is
  // `auth_routes.rs`'s `AUTH_RATE` — the same three parameters, read the same way `PROBE_RATE` and
  // `PUBLIC_RATE` are, with `kv_seed`'s absence from the STRUCT as the memory-only verdict.
  const auth = rustLimiter("wasm/src/auth_routes.rs", "AUTH_RATE");
  const devices = rustLimiter("wasm/src/ip_rate_limit.rs");

  for (const [label, o] of [
    ["wasm/src/ip_rate_limit.rs PROBE_RATE", probe],
    ["wasm/src/auth_routes.rs AUTH_RATE", auth],
    ["wasm/src/ip_rate_limit.rs", devices],
  ]) {
    assert.ok(o, `${label} no longer creates an IP rate limiter — if a call site moved, ` +
      `move it in the ledger and in src/lib/ratelimit.ts's enumeration too`);
  }

  assert.ok(
    probeRouteSeeds(),
    "the probe's route no longer reads/persists its bucket — `probe.rs` must reach `needs_seed`, " +
      "`seed_ttl_secs` and a KV write. That bucket's first sight per IP reads and persists KV so a new " +
      "isolate inherits the budget, and dropping it silently makes the ceiling per-isolate (round-F2).",
  );

  assert.equal(
    auth.kvSeed,
    false,
    `wasm/src/auth_routes.rs's AUTH_RATE gained a KV-seed field. Those two endpoints already cost 2-3 KV ` +
      `writes per attempt themselves, so a per-request KV write here lets an attacker ` +
      `exhaust the Free-plan daily KV write quota — that is round-104's finding, and ` +
      `src/lib/ratelimit.ts records the per-isolate ceiling as the ACCEPTED trade. ` +
      `If this is deliberate, change the doc's enumeration in the same commit.`,
  );

  assert.equal(
    devices.kvSeed,
    false,
    `wasm/src/ip_rate_limit.rs's IpRateLimiter gained a KV-seed field. The port deliberately left the ` +
      `KV arms out — in the memory-only mode the source's KV arms are UNREACHABLE, so porting them ` +
      `would be porting dead code, and the devices gate is the one that costs 2-3 KV writes per attempt ` +
      `already (round-104). A field here is a policy change: give it a line in src/lib/ratelimit.ts's ` +
      `enumeration and re-do that quota analysis, or drop it.`,
  );
});

test("ratelimit: the three named buckets are the ones the doc enumerates", () => {
  const names = [
    rustLimiter("wasm/src/ip_rate_limit.rs", "PROBE_RATE").name,
    rustLimiter("wasm/src/auth_routes.rs", "AUTH_RATE").name,
    rustLimiter("wasm/src/ip_rate_limit.rs").name,
  ].sort();
  assert.deepEqual(
    names,
    ["auth-rate", "probe-rate", "pub-rate"],
    "the bucket names no longer match src/lib/ratelimit.ts's enumeration " +
      "(probe / auth register / devices public gate). A fourth bucket appearing is a " +
      "policy decision — give it a line in that header rather than leaving the list stale.",
  );
});
