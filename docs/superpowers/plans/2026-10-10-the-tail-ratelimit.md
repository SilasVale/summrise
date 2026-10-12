# The console worker's tail — `ratelimit` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The per-IP rate limiter's DECISIONS become the Rust worker's, and `gateway/src/lib/ratelimit.ts` becomes a
boundary that decides nothing.

**Architecture:** `gateway/wasm/src/ip_rate_limit.rs` already owns the algorithm — `IpRateLimiter`, `bucket_of`,
`counter_key`, `check` — and deliberately left the KV arms out, because in the devices-gate family memory-only is the
ABSENCE of a KV field rather than a flag. What is still TypeScript is the **probe** call site's KV seed and the factory
that carries it. The wasm is reached through `wasmGate(env)` in `gateway/src/index.ts`, so the limiter is wired where
the other ported families are: a request-level gate, not a library call.

**Tech Stack:** Rust (`gateway/wasm`, wasm-bindgen), TypeScript (`gateway/src`), `node --test`.

**Spec:** `docs/superpowers/specs/2026-10-10-the-console-workers-tail-design.md`

## Global Constraints

- **`MAX_LOGIC` may only shrink.** It is 70 on the merged tree, measured by forcing the constant to 0 and reading the
  gate's refusal. Every number in this plan names the tree it was measured on.
- **A test belongs to the code it tests; a fixture is not a decision** (`agent/tests/fixtures/js-boundaries.txt`).
- **The three call sites differ in ONE dimension — whether the counter is KV-seeded** (`ratelimit.ts`'s own header).
  Devices is already Rust; probe seeds; auth register is memory-only.
- **Byte-for-byte parity** is required for anything that replaces a shipping JavaScript path.
- **`main` advances only by merge (`--no-ff`)**; fmt + clippy + tests green in both feature configs.

## Review Focus

1. **The first request from a NEW isolate** — the KV seed reads once and writes once, and a port that seeds on EVERY
   request turns one read into one read per request, which is the KV-quota exhaustion the memory-only arms exist to
   avoid.
2. **The 4097th distinct key** — the insertion-order eviction drops the OLDEST, and an implementation that evicts a
   random or most-recent key passes every small test and changes the budget under load.
3. **`cf-connecting-ip` absent** — the source uses the literal `"unknown"`, so every header-less caller shares ONE
   budget; a port that keys on `null` or skips the limiter fails open for exactly the callers an attacker controls.
4. **A clock at a window boundary** — `Math.floor(Date.now() / windowMs)` is `div_euclid`, not `/`; a negative or
   exactly-divisible timestamp must land in the same bucket on both sides.
5. **The eleventh request** — the existing-counter arm uses `>=`, and `ip_rate_limit.rs` carries the mutation proof for
   it (`the_eleventh_request_is_refused`).

---

### Task 1: the limiter answers from the wasm gate

**Files:**
- Modify: `gateway/wasm/src/lib.rs` (the gate's dispatch), `gateway/src/index.ts` (the route family it covers)
- Test: `gateway/wasm/src/ip_rate_limit.rs` (a new case), `gateway/test/ratelimit-kvseed.test.mjs`

**Interfaces:**
- Consumes: `ip_rate_limit::{IpRateLimiter, Counters, check}` — already present, 152 lines.
- Produces: the probe route's refusal, answered by the wasm, with the same status the TypeScript returned.

- [ ] **Step 1: Write the failing test** — in `ip_rate_limit.rs`, a case that drives `check` with the probe's own
      numbers (`limit` and `window_ms` read from `tooling.ts:41`) and asserts the eleventh call is `true`.
- [ ] **Step 2: Run it** — `cd gateway/wasm && cargo test ip_rate_limit` — expected FAIL (the case does not exist yet).
- [ ] **Step 3: Wire the gate** — dispatch the probe's path family to `ip_rate_limit::check`, holding `Counters` in the
      same place the other ported families hold their state.
- [ ] **Step 4: Run it** — expected PASS, and `gateway/wasm/verify.mjs` unchanged.
- [ ] **Step 5: Commit.**

### Task 2: the KV seed moves into Rust

**Files:**
- Modify: `gateway/wasm/src/ip_rate_limit.rs`, `gateway/src/lib/ratelimit.ts`
- Test: `gateway/test/ratelimit-kvseed.test.mjs` (it already reads all three call sites)

- [ ] **Step 1: Write the failing test** — a case that counts the KV reads for N requests from one IP and asserts
      exactly ONE, then one for a second IP.
- [ ] **Step 2: Run it** — expected FAIL.
- [ ] **Step 3: Implement** the seed on the first-sight arm only, beside `check`'s insert arm.
- [ ] **Step 4: Run it** — expected PASS.
- [ ] **Step 5: Commit.**

### Task 3: `ratelimit.ts` becomes a boundary, and the manifest says so

**Files:**
- Modify: `gateway/src/lib/ratelimit.ts` (it keeps the construction and delegates), `agent/tests/fixtures/js-boundaries.txt`
- Test: `agent/tests/js_boundary_inventory.rs`

- [ ] **Step 1: Write the failing test** — the inventory gate with the rule moved to BOUNDARY and `MAX_LOGIC` forced
      to 0; expected refusal `LOGIC files: 69`.
- [ ] **Step 2: Run it** — `cd agent && cargo test --test js_boundary_inventory` — expected FAIL.
- [ ] **Step 3: Make the change** — the file keeps only the `env.KEYS` presence check and the wasm call.
- [ ] **Step 4: Run it** — set `MAX_LOGIC` to the number the gate printed; expected PASS.
- [ ] **Step 5: Commit.**


---

## APPENDIX — what landing the first three tasks actually taught (2026-10-12)

**TASKS 1 AND 2 ARE DONE ON `feat/the-probe-limit-is-rust` (3 commits, not merged), AND THEY ARE NOT WIRED — WHICH IS THE
POINT OF THIS APPENDIX.** `PROBE_RATE`, `probe_in_family`, `needs_seed`, `seed_ttl_secs` and `seeded_check` exist, are
clippy-clean, carry mutation proofs, and **no caller reads any of them.** That is the exact defect the sweeps' landing
classified Critical — *"a decision that was made and then not wired"* — and merging this branch as-is would ship it
again.

**WHAT THE WIRING ACTUALLY COSTS, MEASURED.** `gateway/src/index.ts` reaches the wasm only through
`wasmGate(env).fetch(request)` — `env.WASM_GATE` is a **request-level binding, not a library**. There is no
`#[wasm_bindgen]` export anywhere in `gateway/wasm/src/`, so `probeRateLimited(env, request)` in TypeScript has no
function to call. The probe route is handled inline at `index.ts:298`, and `summriseProbe` (the probe itself) is the
bulk of what that branch does. **So Task 3 is not "make `ratelimit.ts` a boundary": it is "move the whole
`/api/summrise-probe` route into the wasm gate", which is a route port of the size this plan was written to avoid.**

**AND THREE INSTRUMENTS CAUGHT MY OWN EDITS, ALL THREE BY LOOKING AT SOMETHING ADJACENT:**
1. the `PROBE_RATE` loop read `0..PROBE_RATE.limit`, so it asserted *"the (limit+1)th is refused"* for ANY limit and
   `limit: 60 -> 10` left it **passing** — a test that reads its own subject cannot fail when the subject changes;
2. the TTL mutation did not bite because the `sed` pattern carried a space the source did not have — **a mutation that
   never happened looks exactly like a gate that cannot be broken**;
3. shrinking `AGENTS.md` broke `every_cli_verb_the_root_guide_promises_is_one_the_cli_prints`, which extracts verbs
   from `AGENTS.md` by `strip_prefix("summrise ")` — the runbook moved to `scripts/RELEASE.md` and the extractor kept
   reading the old address, answering *"found 0"*. **The document had stopped being the subject and was still read as
   if it were.**

**REVISED SCOPE, SO THE NEXT READER DOES NOT RE-DERIVE IT:** finish this family by porting the probe route
(`summriseProbe` + the 429 arm) into the wasm gate, then `ratelimit.ts` has no caller and becomes BOUNDARY, then the
manifest rule moves and `MAX_LOGIC` reads 69. **Until that route lands, the three commits stay on the branch** — a
foundation with no caller is not a landing.
