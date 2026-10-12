# The console worker's tail — the four route families, and what a worker entry is

## 0 · Where this sits

**Landing 5's tail**, of `2026-10-08-every-decision-is-rust-design.md`, whose §4 row 5 is the whole slice:

> `| 5 | the worker entries | gateway/src 4,913 · index/src 417 · proxies api 344 | byte-for-byte parity …`

**The device-proxy slice of landing 5 is landing now** (its own branch), and this spec is what remains after it.

## 1 · What this changes, in one sentence

**The console worker's remaining decisions become Rust** — the four route families that were never ported — so that
`LOGIC | gateway/src/ |` and `LOGIC | gateway/test/ |` can be reclassified with reasons rather than left as the last
LOGIC rules in the repository.

## 2 · The inventory, MEASURED ON `main@35bf4328`

| rule | files |
|---|---|
| `LOGIC | gateway/src/ |` | **38** |
| `LOGIC | gateway/test/ |` | **54** |
| **the gate's LOGIC count** | **91** — `gateway/src/` 37 + `gateway/test/` 54, in TWO rules |

**AND `agent/scripts/` IS ALREADY BOUNDARY** — every tracked JS file under it carries a longer, more specific rule, so
the directory rule is fully shadowed and matches zero files. **The sweeps' landing measured that and this spec repeats
it because a reader will otherwise expect the sweeps' slice to move the count. It does not.**

## 3 · The four families, and where each lives today

| family | its home now | why it was not ported |
|---|---|---|
| **`/api/admin/*`** | `plugins/admin.ts`, `store/admin.ts`, `store/models.ts` | the console's own administration surface; it reads the KV stores the worker's dispatch does not model |
| **`/api/plugins/status`** | **`plugins/status.ts` (NEW, 314 lines, landing in the device-proxy slice)** | it carries `routeStats(ctx)` — a registration index and a per-isolate hit counter for every route of every plugin, **a property of the TypeScript plugin table** that the Rust dispatches by family without |
| **the model-route selection** | `plugins/models-probe.ts`, `plugins/admin.ts`, `plugins/auth.ts` | the provider catalogue and the probe that decides which channel answers |
| **the key diagnostics** | `plugins/auth.ts`, `lib/ratelimit.ts` | `me/keys`, the reveal, the regenerate sweep, and the per-bucket limiters |

**AND `/api/plugins/status` IS THE PRECEDENT THIS SPEC ARGUES FROM.** It is the one route whose absence from the corpus
is *by design* — `gateway/wasm/src/mcp.rs:62` says *"deliberately NOT here"* and asserts it, and `index.ts` says
*"THE ONE EXCLUSION … The route therefore stays on the TypeScript path."* **A route that the oracle cannot cover is a
route whose test has no other witness**, and the device-proxy slice learned that the hard way: it deleted that test by
a name match, the suite's count FELL, and restoring it is what found a live 404.

## 4 · The rule, and the one thing this landing must not do

**A worker entry that replaces a shipping JavaScript one needs BYTE-FOR-BYTE PARITY EVIDENCE** — the ancestor's own
requirement in §4 row 5 — and the ported surface's oracle is the corpus (`gateway/wasm/…/expected.json`, 399 cases),
which pins **what the Rust answers**. So:

* a family that IS ported goes to the worker, and its tests go with a pointer naming the corpus cases that now cover
  them (`rename` 9 · the admin gate matrix 14 · `login` 9 · `panel-grant` 8 · `upload` 17 · `self-register` 9 ·
  `register` 9 · `/api/devices` 54 · `tunnel-token` 3 · `register-keys` 3 · `install-cmd` 1);
* **a family that is NOT ported STAYS, with a written reason** — and the reason is a fact about the Rust, not about
  effort: *the counters are a property of the TypeScript plugin table and the worker dispatches by family without it*;
* and **no test is deleted whose subject has no other witness.** Two categories of survivor, not one: **the subject
  survived a port that has not happened yet**, and **the subject has no other witness at all.**

## 5 · What verifies it

1. **The inventory gate**, with `MAX_LOGIC` measured LAST on the landed tree — force it to 0, read the refusal, write
   what it printed **and the tree**, because every number quoted in this migration has been true of some other one.
2. **The differential**, for any family that moves: the corpus is byte-compared, and it is the only oracle that can say
   the Rust answers what the TypeScript answered.
3. **`bash scripts/test/all-gates.bash`** — 49 ok / 0 failed on this repository's own bar — **and the CI run**, because
   a local bar that is not CI's check is not the same check. **That gap has cost this session four CI cycles.**

## 6 · What is NOT in this landing

* **The MCP tool table's single-sourcing.** The ancestor's §6.1 measured it and settled it: the remaining duplication is
  the *description text and the schema shapes* of 800-odd lines, **held together by a gate that reads the Rust side**,
  and *"the decision is unchanged in direction and smaller in scope — landing 5 removes the file by making the entry
  Rust, not by generating the table."*
* **The React/TSX view layer and the Electron host**, out of scope with measured reasons in the ancestor's §6.
* **`index/src` and `proxies api`** — the other two thirds of §4 row 5, each its own landing.

## 7 · The risk this spec names rather than hides

**A route family that stays TypeScript is a route the Rust worker does not answer, and the corpus cannot see it.** The
`/api/plugins/status` precedent shows both halves: the absence is *correct* (the counters are a TS property) and the
absence is *invisible* (the corpus has zero cases, so a deletion there is not a failure). **So every staying family
needs a test that runs, and the manifest rule that keeps it needs a reason a reader can check against the Rust.**

---

## MEASURED 2026-10-12 — the remaining 69 are blocked by the BINDING, not by the decisions

**THE DECISIONS ARE MOSTLY PORTED; THE TYPESCRIPT THAT HOLDS THEM IS MOSTLY A REFERENCE OR A FALLBACK.** Measured file
by file, on the tree that landed the probe route:

| the decision | where it is now |
|---|---|
| the per-IP limiter's algorithm | `wasm/src/ip_rate_limit.rs` (152 lines, three tests, mutation-proofed) |
| the auth register limiter | `wasm/src/auth_routes.rs` — `auth_rate_limited` + `AUTH_RATE_COUNTERS` |
| the probe's limiter (KV-seeded) | `wasm/src/ip_rate_limit.rs` — `PROBE_RATE`, `needs_seed`, `seed_ttl_secs`, `seeded_check` |
| the KV seed's two decisions | `wasm/src/ip_rate_limit.rs` (pure), with the execution beside them |
| the setting normalization | `wasm/src/store.rs` — `normalize_setting`, `global_setting_enabled` |
| the model catalogue | `wasm/src/registry.rs` — `MODEL_REGISTRY: [ModelSpec; 24]` + `model_spec` |
| the health cards | `wasm/src/lib.rs` — `HEALTH_CHANNELS: [(&str, &str); 20]` |
| the og circuit | `wasm/src/lib.rs` — `read_breaker`, `breaker_call` |

**AND THE WALL IS ARCHITECTURAL: THE WASM IS A REQUEST-LEVEL BINDING WITH NO FUNCTION EXPORT.** `index.ts` reaches it
only through `wasmGate(env).fetch(request)` — `env.WASM_GATE` — and there is **no `#[wasm_bindgen]` export anywhere in
`gateway/wasm/src/`** (grep measured: the only uses are `use worker::wasm_bindgen::…` inside `device.rs` and
`vision.rs`). So a TypeScript caller **cannot invoke one Rust function**; a decision can only move by moving the whole
route that carries it.

**THAT IS WHY THE COUNT DID NOT MOVE WHEN THE PROBE ROUTE DID.** `db1ae41b` moved the route and its guard into Rust
and CI is green, and `MAX_LOGIC` is still 69: `lib/ratelimit.ts` has two live callers left (`plugins/auth.ts`, which
`registerPlugins` still dispatches, and `tooling.ts`'s probe arm, now the fallback). **A deletion is worth more than a
reclassification, and it needs the fallback decision.**

**TWO ROUTES OUT, BOTH MEASURED:**

1. **Make the gate mandatory for the ported families, as `/v1` already did** — its comment reads *"THE TYPESCRIPT
   IMPLEMENTATION IS DELETED, so there is no fallback to fall back to: a deployment without the WASM_GATE binding is a
   deployment that cannot serve the API, and saying so is better than a 500 that reads like a routing mistake."* That
   retires `lib/ratelimit.ts` entirely (69 → 68) and the TS callers with it. **It is a deployment decision**: a device
   with no binding starts answering 503.
2. **A second wasm entry that exposes the pure functions** — `__invoke(name, json)` — so a decision can be ported
   WITHOUT moving its route. That is what the remaining 30-odd files need: `models-probe.ts`'s `upstreamModelsUrl` and
   `offeredIds` are pure and already measured as absent from Rust, and its own comment records FOUR independently-typed
   copies of the BYOK key vocabulary with nothing comparing them (rounds 186-190) — **the exact shape the sweeps'
   landing found in `DECORATIVE_WAIVERS` and fixed with a permanent cross-language gate.**

**WHAT IS *NOT* LEFT TO DO**, so the next reader does not re-derive it: an audit of all 71 tracked JS/TS files under
`gateway/src` + `gateway/test` found **exactly one pure re-export barrel** (`store.ts`, now BOUNDARY) and **zero dead
files** (every file is imported). The rest are genuine LOGIC, and the honest number is 69.

---

## MEASURED 2026-10-12 — THE TAIL IS 17,151 LINES, NOT ~2,000

**THE NUMBER THAT MATTERS, AND THE ONE I GOT WRONG EARLIER.** `MAX_LOGIC` reads 69, and the count alone hides the
size. Measured file by file on `11bbc500`:

| | files | lines |
|---|---|---|
| `gateway/test/` | 39 | **8,834** |
| `gateway/src/` | 30 | **8,317** |
| **total** | **69** | **17,151** |

**"~2,000 LINES, FOUR OR FIVE LANDINGS" WAS ABOUT ONE FAMILY, AND I SAID IT AS IF IT WERE THE WHOLE TAIL.** The 2,032
lines I measured were `admin` 614 + `auth` 813 + `status` 314 + `models-probe` 201 + `ratelimit` 90 — the five PLUGIN
families under `gateway/src/plugins/`. The remaining 15,000 lines are the stores, the registries, the channels table,
the session/route helpers, and **8,834 lines of `gateway/test/` that follow their subjects** (the manifest's own rule:
"a test belongs to the code it tests"). **A count hid a size, which is the same defect as a count hiding a deletion.**

**SO CRITERION ① IS NOT A FEW LANDINGS.** At the sizes this session actually moved (the probe route: one file, ~250
lines of Rust, five commits, a full CI cycle), 17,151 lines is on the order of **dozens of landings**, and the test
files can only move as their subjects do.

**AND THE TWO ROUTES OUT ARE NOW THE ONLY ONES THAT CHANGE THAT SHAPE**, both measured above:
1. **the fallback decision** (make the gate mandatory as `/v1` did) — it retires files instead of porting them, and it
   is a DEPLOYMENT decision: a device with no `WASM_GATE` binding starts answering 503;
2. **a function entry on the wasm** (`__invoke`) — it lets a pure decision move WITHOUT moving its route, which is what
   the 30-odd `gateway/src` files need.

**WHAT IS NOT WORTH DOING AGAIN**: an audit of all 71 tracked JS/TS files under `gateway/src` + `gateway/test` found
exactly ONE pure re-export barrel (`store.ts`, now BOUNDARY) and ZERO dead files. **The 69 are genuine LOGIC, and the
honest reading of criterion ① is that it is a programme, not a landing.**

---

## PROPOSED 2026-10-12 — the DECISION ROUTE: how a pure decision moves without moving its route

**THE MEASUREMENT THAT MAKES THIS CHEAP: `WASM_GATE` IS NOT A WASM MODULE BINDING.** `gateway/wrangler.jsonc` declares
`services: [{"binding":"WASM_GATE","service":"vale-gate-wasm"}, …]` — **a SERVICE BINDING to another Worker**, so
`env.WASM_GATE.fetch(request)` is an HTTP call to a deployed Worker, not a call into a wasm instance. The Rust worker is
`gateway/wasm/wrangler.jsonc`'s `"name": "vale-gate-wasm"`.

**THAT REMOVES THE OBSTACLE THE PREVIOUS SECTION NAMED.** There is no wasm instance to add `#[wasm_bindgen]` exports to
and no new binding technology to introduce: the gate already answers `fetch`, so **one more route on the same worker is
reachable through the binding that already exists** — today, with no deployment change and no second binding.

### THE SHAPE

```
POST /__decision     {"name":"upstream_models_url","args":{"upstream":"…"}}
                 ->  {"ok":true,"value":"…"}   |   {"ok":false,"error":"unknown decision: …"}
```

* **the handler takes the JSON args and NEVER `env`** — that is the whole security argument. The gate worker also serves
  `/v1` on its public host, so this route is publicly reachable; **a decision that cannot read `env` cannot become an
  oracle for a key**, and every decision in the table is a pure function of its arguments by construction.
* **one table, in Rust**: `decisions.rs` holds `(name, fn(&Value) -> Result<Value, String>)`. A decision is added by
  adding a row, and the TS side needs no change to reach it.
* **one helper, in TS**: `decide(env, name, args)` in a single module, so there is one place that knows the protocol.

### WHAT IT COSTS, MEASURED RATHER THAN ASSUMED

* **one subrequest per call.** Service bindings count toward the per-request subrequest limit, so a decision called on a
  hot path is a real cost — **this route is for the decisions that are NOT hot** (the admin probe's URL building and id
  extraction are called once per admin request), and a hot decision should still move by moving its route.
* **a failure mode that must be designed for**: if the gate is unreachable, `decide` must fall back to the TypeScript
  twin rather than fail the request — which is the same fallback question as below, one layer down.

### WHY IT IS NOT IMPLEMENTED HERE

**The process gate says the spec is reviewed by the operator before a plan is written**, and this is a design decision
with a deployment consequence (a new route on a public host). The two routes out are now:

1. **the fallback decision** — make the gate mandatory for a family as `/v1` did; retires TypeScript instead of porting
   it; a device with no binding starts answering 503. **This is the operator's call.**
2. **the decision route** — this section. It does not retire a file by itself; it makes the 30-odd `gateway/src` files
   *portable one decision at a time*, which is what turns 17,151 lines from a programme into a queue.

---

## MEASURED 2026-10-12 — the decision route's REACH, and the two paths it must never touch

**A LANDING WAS REVERTED BECAUSE A TEST REFUSED IT, AND THE REFUSAL IS THE DELIVERABLE.** Three more decisions were
ported into `decisions.rs` (`wire_version`, `b64url_decode_str`, `is_responses_only_model`), all tested, all
mutation-proved — and **none of them could be wired**, for three different reasons that together draw the route's
boundary:

1. **THE STATUS PATH MUST NOT ASK THE WORKER.** `plugins.test.mjs`'s own sentence: *"plugins/status: the exclusion
   holds WITH the binding present (**the worker must never be asked**)"* — it stubs `WASM_GATE` and records what
   reaches it. Wiring `wire_version` through `decide` failed it immediately. **And the reason is the product's, not
   the test's**: the status page reports ON the worker, so **it must not depend on the worker** — the moment you most
   need the status page is the moment the worker is down.
2. **AND THE SESSION-VERIFY PATH IS ON IT.** The second attempt wired `b64url_decode_str` into
   `verifySessionToken`, threading `env` through it and its two callers. The same test failed **again**, because the
   status request carries a session cookie: a gate-dependent session check makes the status page depend on the worker
   one layer down. **The exclusion is transitive over everything the status request touches.**
3. **AND ONE HAD NO CALLER AT ALL.** `isResponsesOnlyModel` is used only by `model-registry.test.mjs`; there is no
   `src/` caller. **A decision with no caller is a foundation, not a landing** — this repository's own rule — so it
   does not go in the table on the strength of being portable.

**SO THE ROUTE'S REACH IS PRECISE, AND IT IS NOT "EVERYTHING PURE":** it serves a pure decision that is **NOT on the
status path and not on the session-verify path** — which is exactly what its two landed batches are (`models-probe`'s
URL building and id extraction, and the three usage mappers, all reached from `/api/admin/*` handlers that already
depend on the worker).

**AND THE MUTATION THAT DID NOT BITE FOUND DEAD CODE IN THE PORT ITSELF**: deleting the `=`-padding fix from
`b64url_decode_str` left every test passing, because `session_gate::b64url_decode` **already pads**. The source's fix
exists because `atob` does not — so on this side it is the decoder's job, not each caller's. **A mutation that does not
bite is a mutation that never happened, and here it was a mutation that was never needed.**

**WHAT THIS SAVES THE NEXT READER**: three plausible, tested, mutation-proved ports were thrown away rather than
landed uncalled, and the reason is now written down instead of being rediscovered by a red test.

---

## MEASURED 2026-10-12 — THE QUEUE: the decision route reaches 158 functions, and the rest is three other jobs

**THE COUNT THAT TURNS 17,151 LINES FROM A WALL INTO A QUEUE.** Every `export`ed function under `gateway/src` was
classified by its PARAMETERS on `d1c5e540`, which is what decides whether the decision route can carry it at all:

| | functions | what moves them |
|---|---|---|
| **A. takes a `Request`/`Response`** | **10** | only a ROUTE port — JSON cannot carry a live request |
| **B. pure JSON in, pure JSON out** | **158** | **the decision route** ✓ |
| **C. on the status or session-verify path** | **17** | **nothing** — the route must never be asked there |

**SO THE ROUTE'S REACH IS 158 FUNCTIONS, AND THE BIGGEST GROUPS ARE THE THREE STORES** — `store/users.ts` (18),
`store/models.ts` (14), `store/providers.ts` (12) — all reached from `/api/admin/*` handlers that already depend on the
worker, so the status exclusion does not touch them. **That is the queue, and it is one decision per row.**

**AND THE OTHER THREE JOBS ARE NOW NAMED RATHER THAN MIXED IN:**
* **10 functions move by a route port** — `http.ts` (4), `access.ts` (2), `plugins/auth.ts`, `plugins/models-probe.ts`,
  `route-do.ts`, `tooling.ts`. This is the original wall, and it is TEN functions, not the whole tail.
* **17 functions can never move** — `auth.ts` (11), `session.ts` (4), `plugins/status.ts` (2) — **and that is a
  property of the product, not a gap**: the status page and the session check must work when the worker is down.
  **Their TypeScript is permanent** unless the fallback decision changes what the status page may depend on.
* **the 8,834 lines of `gateway/test/` follow their subjects** and move as the subjects do.

**WHAT IS *NOT* IN THIS QUEUE AND MUST NOT BE FORCED INTO IT**: a function that needs `env` for anything but reading a
var, a function that takes a live `Request`, and anything on the status path. **A decision that needs the environment is
not a decision — it is a route.**

---

## THE SESSION'S MAP — where the tail stands, and what the next batch is (2026-10-12)

**THE THREE CRITERIA AT THE END OF THIS SESSION, ON `6e93ac86`:**
* **② `all-gates` green** — ✓ inside CI's `pack-chain`;
* **③ CI green on main** — ✓ `6e93ac86` completed success, 10 green / 0 failed;
* **① zero LOGIC files** — ✗ **70**, down from **107** when the session started.

**WHAT MOVED THE NUMBER, AND WHAT DID NOT.** 107 -> 70 is 37 files, and it came from three kinds of work: **retiring
already-ported families** (the sweeps' landing and the console rollback), **ONE honest reclassification** (`store.ts`,
a pure re-export barrel, found by a mechanical audit of all 71 tracked JS/TS files that found exactly one and ZERO dead
files), and **one DELIBERATE increase** (69 -> 70 for `gate/src/gate.ts`, recorded in the manifest with its reason:
a ratchet that can only be raised in the open is doing its job). **Porting `models-probe` and the usage mappers moved
DECISIONS into Rust without moving the count at all** — the TypeScript twins stay as the fallback, which is the
fallback decision the operator has not made.

**THE INFRASTRUCTURE THIS SESSION BUILT, WHICH IS WHAT MAKES THE REST A QUEUE RATHER THAN A WALL:**
`POST /__decision` on the Rust worker, reached through the `WASM_GATE` service binding that already exists. Six rows
today (`upstream_models_url`, `offered_ids`, `map_openrouter_usage`, `map_amd_usage`, `map_og_usage`,
`public_provider`), each with its own tests and mutation proofs. **A decision is added by adding a row**, and the
TypeScript side needs one helper (`decide`) and no protocol knowledge.

**THE NEXT BATCH, AND THE PATTERN IT NEEDS.** `publicProvider` is the model: **it took `env` because the key
RESOLUTION does, and splitting resolution from shape is what made the rest a decision.** The next candidates are the
same shape — a function that reads `env` only to fetch what it then projects:
* `store/users.ts`'s `userKeysStatus` (17 lines) and `getUserKeys` (13) — the per-key status shape;
* `store/models.ts`'s `advertisedIds` (9) and `catalogue` (3) — but note `catalogue` needs the KV list, so its shape
  is separable and its READ is not;
* `store/devices.ts` (10 pure functions) and `upstream.ts` (12) — unsampled this session.
**MEASURED ABSENT FROM RUST AND READY**: `parseProviderSpec` (132 lines, `store/providers.ts`) with its private
`parseProviderModel` (59) — a real validator on the admin path, pure JSON, and the largest single reachable decision
left. It needs `MODEL_FIELDS`, `KNOWN_MODALITIES`, `SUPPORTED_PROVIDER_APIS`, `KNOWN_UNSUPPORTED_APIS` and
`RESERVED_PREFIXES`, and its error strings are response BYTES.

**WHAT THIS SESSION LEARNED AND WOULD COST THE NEXT READER TO REDISCOVER:**
1. **the decision route's reach is 158 functions** (pure JSON, off the status/session path); **10** can only move by a
   route port; **17** can never move, and that is the product's design rather than a gap;
2. **the status page must never ask the worker** — the exclusion is transitive over everything a status request
   touches, including the session check;
3. **a mutation that does not bite finds dead code**: the `=`-padding fix, faithfully copied from the source, was
   already the Rust decoder's job;
4. **the reach of a count**: 69 hid 17,151 lines, and my first estimate of "four or five landings" was ONE family.
