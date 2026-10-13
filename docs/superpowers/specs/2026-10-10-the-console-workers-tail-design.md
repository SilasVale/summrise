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

---

## MEASURED 2026-10-12 — THREE QUARTERS OF THE REMAINING TYPESCRIPT IS ALREADY RUST

**THE NUMBER THAT DECIDES WHAT CRITERION ① ACTUALLY NEEDS.** Every `export`ed function in the 70 LOGIC files was
matched against the Rust tree by NORMALIZED name (case and separators removed, so `mapOpenRouterUsage` matches
`map_openrouter_usage`):

| | |
|---|---|
| exported functions across the 70 files | **195** |
| already present in Rust | **147 — 75%** |
| files with NO function present in Rust | **3** |

**THE THREE FILES WITH NOTHING IN RUST ARE THE WHOLE OF THE GENUINE GAP, AND TWO OF THEM ARE NOT GAPS AT ALL:**
* `plugins/status.ts` — **on the status path, so it can never move** (the exclusion above);
* `gateway/src/route-do.ts` — the Durable Object's request authorisation, which takes a live `Request` (group A);
* `gateway/src/store/file-config.ts` — the catalogue-file parser, **and this one is a real, unported decision**.

**SO CRITERION ① IS NOT 17,151 LINES OF PORTING. IT IS MOSTLY ONE DECISION THAT ONLY THE OPERATOR CAN MAKE**: the
TypeScript that remains is, for three quarters of its functions, **a reference and a fallback for a decision that is
already Rust** — the state the sweeps' own landing established when each Rust rule landed BESIDE its JavaScript twin
because each twin had a live caller. **Deleting the twins is what turns 70 into a small number**, and that is the
fallback decision `/v1` already made once: *"THE TYPESCRIPT IMPLEMENTATION IS DELETED, so there is no fallback to fall
back to."*

**WHAT REMAINS GENUINELY TO PORT, IN ORDER OF SIZE**: `store/providers.ts`'s `parseProviderSpec` + its private
`parseProviderModel` (~190 lines, byte-exact error strings, the largest reachable decision), `store/file-config.ts`'s
catalogue parser, and whatever the 75%-matched remainder still OWNS rather than merely mirrors — **which is the
measurement this number does not make, because a matching name is not a matching behaviour.** That is the next
honest piece of work: for a file whose functions all match, diff the BEHAVIOUR, not the names.

---

## THE BEHAVIOUR DIFF — the method, and its first worked case (2026-10-12)

**"A MATCHING NAME IS NOT A MATCHING BEHAVIOUR" IS THE MEASUREMENT ABOVE'S OWN WARNING, AND THIS IS THE WORK IT
CALLS FOR.** 147 of 195 exported functions have a Rust counterpart by name; **not one of those pairs has been shown
to AGREE.** So the next honest piece of work is not another port — it is a diff, one function at a time, and this
section is the method and its first case.

**THE METHOD, IN FOUR STEPS:**
1. read the TypeScript as the ORACLE (it is what ships today, and its tests are the corpus);
2. read the Rust counterpart;
3. **enumerate the CASE FAMILIES where the two languages' primitives differ** — that is where a port goes wrong, not
   in the ordinary path. For a URL predicate: the parser's strictness, host case folding, the port, IPv6 brackets,
   and the several spellings of an IPv4 literal;
4. write down the answer for each, and only then say "they agree". **A diff that lists no differing primitives has
   not been done — it has been asserted.**

**THE FIRST CASE: `isLoopbackOrigin(origin)`, `http.ts` vs `wasm/src/cors.rs`.** It is the console's local-origin
check, so it is a security predicate and a false positive is a real defect.

| case family | TypeScript (`new URL`) | Rust (`url::Url::parse`) | agree |
|---|---|---|---|
| a scheme that is not http/https | `false` | `false` | ✓ |
| not a URL at all (the throw / the `Err`) | `false` | `false` | ✓ |
| `localhost`, any case | lowercased by WHATWG → `true` | lowercased by the crate → `true` | ✓ |
| a PORT (`http://localhost:8080`) | `hostname` excludes it → `true` | `host_str()` excludes it → `true` | ✓ |
| IPv6 (`http://[::1]/`) | `hostname` keeps the brackets → `false` | `host_str()` keeps them → `false` | ✓ |
| a decimal IPv4 (`http://2130706433/`) | normalized to `127.0.0.1` → `true` | normalized → `true` | ✓ |
| hex/octal IPv4 (`http://0x7f.0.0.1/`) | normalized → `true` | normalized → `true` | ✓ |
| a trailing dot (`http://localhost./`) | kept → `false` | kept → `false` | ✓ |
| a relative reference (`"/x"`) | throws → `false` | `Err` → `false` | ✓ |
| userinfo (`http://a@localhost/`) | `hostname` is `localhost` → `true` | `host_str()` is `localhost` → `true` | ✓ |

**TEN FAMILIES, TEN AGREEMENTS — and the reason to believe it is that the two parsers are the SAME SPEC**: `new URL`
and the `url` crate both implement WHATWG URL, which is what makes the case folding, the port exclusion and the IPv4
normalisation line up rather than merely happen to.

**AND THE OTHER 146 PAIRS ARE NOT THEREBY CLEARED.** This one was chosen because its primitives are the ones most
likely to differ. **The method is what generalises; the result does not.**

**THE SECOND CASE: `safeEq(a, b)`, `auth.ts` vs `wasm/src/auth.rs`.** The console's constant-time compare for
credential-shaped values — *"plain `!==` on the admin token leaked timing, and the compare itself must not
early-exit"* — so the property under test is TIMING, not the boolean.

| case family | TypeScript (`charCodeAt`) | Rust (`encode_utf16`) | agree |
|---|---|---|---|
| equal strings | `true` | `true` | ✓ |
| equal length, one unit differs | `false`, no early exit | `false`, no early exit | ✓ |
| DIFFERENT lengths | `false`, **early exit on the length** | `false`, **the same early exit** | ✓ |
| a non-ASCII pair | UTF-16 CODE UNITS | `encode_utf16`, the same units | ✓ |
| a non-string (the `typeof` guard) | `false` | not expressible in Rust — the type is `&str` | n/a |

**AND THE INTERESTING PART IS WHAT A LAZY PORT WOULD HAVE DONE**: comparing `&str` with `==`, or with `as_bytes()`,
answers the SAME BOOLEAN for every input — **so every functional test would pass** — and leaks a different length
(a UTF-8 length is not a UTF-16 length for any non-ASCII string) and short-circuits at the first differing byte.
**The port chose `encode_utf16` and kept the length early-exit, which is the source's timing behaviour**, and no test
in either suite can tell the difference. **This is the case that justifies the whole method.**

---

## A FLAKE, ITS MECHANISM, AND WHY IT IS THE SOURCE'S OWN KNOWN RACE (2026-10-12)

**CI FAILED ON A DOCS-ONLY COMMIT, WHICH IS WHAT MADE IT WORTH READING.** `da1b0109` changes ONE `.md` file and
nothing else; `gateway`'s Tests answered `not ok 216 - register: 30 attempts then 429 (per-IP rate limit)`, and the
SAME TREE run locally answers **385 pass / 0 fail**. So it is a flake — and the mechanism is findable in the two
tests rather than in the runner:

* `reset-password: 30 attempts then 429` and `register: 30 attempts then 429` are two tests, in ONE file, that **both
  drive the limiter from the SAME `cf-connecting-ip`: `192.0.2.99`**;
* the counter is **module-level per-isolate state**, and `__clearCaches()` — which both tests call — clears the STORE
  caches, **not the limiter's**;
* **the limiter's key is bucketed by the wall-clock MINUTE** — and the source says so itself: `createIpRateLimiter`'s
  own comment names *"a minute-boundary race double-spent"* as the reason the probe's limiter went per-IP.

**SO THE SECOND TEST INHERITS THE FIRST'S THIRTY ATTEMPTS EXACTLY WHEN BOTH FALL INSIDE THE SAME MINUTE BUCKET**, and
starts clean when they straddle one. **That is a flake with a clock for a seed, and it is why the same tree can be red
in CI and green on the desk.**

**NOT FIXED HERE, AND THE REASON IS THE POINT OF WRITING IT DOWN**: the repair is either distinct IPs per test or a
`__clearCaches()` that also drops the limiter's counters — and **which one is right depends on whether the two tests
are meant to share a bucket**, which is a decision about the TEST rather than about the product. **A flake whose
mechanism is written down is a five-minute fix for the next reader; one that is re-run until green is a tax.**

---

## THE TWO LIMITS OF "DELETE THE TWINS", FOUND BY TRYING IT (2026-10-12)

**THE OPERATOR CHOSE THIS PATH AND THE FIRST LANDING TOOK ① FROM 70 TO 69** — `store/grants.ts`, deleted, because the
Rust port is live (`devices.rs` calls all three functions) and the TypeScript had **zero consumers anywhere**. That is
the clean case, and running it surfaced the two limits that decide how much further the path goes.

**LIMIT 1: `grants.ts` WAS THE ONLY FILE OF ITS KIND.** The audit was run again, this time asking the question that
finds dead code — **"is any exported NAME used elsewhere", not "is the file imported"** (the second question is what
missed it: `store.ts` is a barrel that re-exports `grants.ts` wholesale, so the file looked imported while not one of
its names was used). Across the remaining 69 LOGIC files, **ZERO have all their exports unused.** So every other file
has at least one live consumer, and deleting its twins means **re-pointing that consumer at Rust** — which for a store
means the decision route, and for a handler means moving the route.

**LIMIT 2: A MODULE-LEVEL TABLE CANNOT MOVE THROUGH AN ASYNCHRONOUS ROUTE.** `store/byok.ts` is the sharpest case: its
`BYOK_CHANNELS` IS the shared source (the Rust is a projection of it, and `agent/tests/byok_channel_keys.rs` now gates
the two to agree), so it looks like the ideal thing to single-source in Rust. **It cannot be, as the table stands**: the
TypeScript reads it at MODULE LOAD and synchronously — `byPrefix`/`byKind` are plain lookups — and `decide` is
`async`. **A synchronous module-level table cannot be fetched over an asynchronous route at the moment it is needed.**
Moving it needs either a build-time emission (as `mcp-tools.ts` already is) or a synchronous seam that does not exist.

**SO THE PATH IS REAL BUT INCREMENTAL, AND ITS UNIT IS THE RESIDUAL DECISION.** After `grants.ts`, the next files are
not "delete a twin" but "port a small decision and then delete the line": `store/settings.ts` keeps the key prefix and
the env-var fallback after `normalizeSetting` moves; `store/byok.ts` keeps the table; `upstream.ts` keeps the route
builders. **Each is a decision, each is reachable through the route, and none is a file.**

---

## THE NEXT LANDING, MEASURED TWICE AND CORRECTED ONCE: `store/settings.ts` (2026-10-12)

**MY FIRST ANALYSIS OF THIS FILE WAS WRONG IN TWO WAYS, AND BOTH ARE INSTRUCTIVE.**

**1. IT HAS THREE DECISIONS, NOT TWO, AND MY SCAN COULD NOT SEE ONE OF THEM.** `settings.ts` carries `normalizeSetting`
(**private**), `globalSettingEnabled` (**exported**) and the canonicalization inside `setGlobalSetting`. The
name-matching scan counts EXPORTED names — so a private function that is nonetheless a decision is invisible to it.
**And the two are not the same function**: `normalizeSetting("")` answers `""` (the empty string is not `"0"` or
`"false"`) while `globalSettingEnabled("")` answers `false`. Both are already in Rust (`store::normalize_setting`,
`store::global_setting_enabled`), which is why the pair did not show up as a gap either.

**2. I CRIED "THE SYNC LIMIT" BEFORE CHECKING THE CALLER, AND IT DOES NOT APPLY.** `globalSettingEnabled` has exactly
ONE consumer — `plugins/auth.ts:373`, inside an `async` handler that already returns a `Response` — so it can be
awaited. **The sync limit is real (`byok.ts`'s module-level table is the case) but it is a property of the CALL SITE,
not of the function**, and asserting it from the function's signature is how a solvable file gets written off.

**SO THE LANDING IS THREE ROWS AND A REWRITE, AND EVERY PIECE IS MEASURED:**
* `get_global_setting` ← `store::get_global_setting(kv, env_var)` — **already landed once and reverted** (see below);
* `set_global_setting` ← the round-94 empty arm (delete) and the round-439 canonicalization (`"1"`/`"true"`/boolean
  `true` → `"1"`), which are the two arms a careless port collapses;
* `global_setting_enabled` ← `store::global_setting_enabled`;
* and in the TypeScript, `normalizeSetting` and the canonicalization are DELETED — the twins — leaving a KV read, a var
  read, a cache and three `decide` calls. **That is a BOUNDARY**, and it is 69 -> 68.

**AND THE REVERT IS PART OF THE RECORD**: the first two rows were written, tested (16 passed, both mutations proven
biting) and then **thrown away**, because at that moment I believed the third decision could not move — and a
foundation with no caller is not a landing. **They are two `decide` rows and a commit message away from being restored;
what they are not is mergeable on their own.**

---

## WHY THE TWINS CANNOT BE DELETED YET — MEASURED IN THE TEST ENVIRONMENT (2026-10-12)

**THE OPERATOR CHOSE "DELETE THE TWINS" AND THE PATH IS NOW BLOCKED BY SOMETHING NOBODY HAD LOOKED AT: THE TEST
ENVIRONMENT CANNOT RUN THE RUST.** Every decision the route carries is reached by `decide(env, …)`, and `decide`
answers `undefined` when the gate does not answer — so **deleting a twin removes the only implementation the tests can
reach.**

**WHAT THE SUITE ACTUALLY HAS, MEASURED:**
* `gateway/test/helpers.mjs` DOES stub `WASM_GATE` — behind a flag, and **its `fetch` answers a fixed model list**
  (`{object:"list", data:[{id:"og/…"}]}`) and records the path in `env._frontDoor`. It exists for the `/v1` ALIAS
  tests, and its own comment says why: *"the TypeScript implementation is deleted, so a test env without this binding
  gets the front door's loud 503 — which is correct and useless as a test of the alias."*
* it does NOT know `/__decision` ✗ — so with the stub ON, `decide` reads a 200 whose body has no `ok: true` and answers
  `undefined`; with the stub OFF it answers `undefined` too. **Either way the decision route is unreachable from
  `npm test`.**

**SO THE THREE WAYS FORWARD ARE ALL DECISIONS, AND NONE OF THEM IS A PORT:**
1. **the stub answers `/__decision` with a JavaScript re-implementation of the decisions** — which is a THIRD copy of
   every decision (the Rust, the twin, and the stub) and is exactly the drift the route exists to prevent;
2. **the suite builds and loads the wasm** — `gateway/wasm/verify.mjs` already runs a differential corpus, so the
   machinery exists, but it is NOT in `npm test`'s path and wiring it in is a test-infrastructure landing;
3. **the twins stay as the fallback** — which is where the count stands, and which is why ① has moved by exactly ONE
   file (a dead one) since the path was chosen.

**AND THE `/v1` PRECEDENT DOES NOT TRANSFER, WHICH IS THE PART WORTH KNOWING.** `/v1` deleted its TypeScript and its
tests are satisfied by a stub that answers a FIXED body, because what those tests assert is the ALIAS REWRITE (the
path that reached the binding), not the front door's decisions. **The decision route is different: its whole purpose is
that a DECISION moved, so a test that cannot reach it cannot test anything.** **A stub that answers a constant is
enough to prove a ROUTE was taken and not enough to prove a DECISION was made.**

---

## THE UNBLOCKER: THE RUST EMITS THE TYPESCRIPT, AND THE EMISSION IS GENERATED (2026-10-12)

**THE BLOCKER ABOVE HAS A SOLUTION ALREADY PROVEN IN THIS REPOSITORY, AND IT IS NOT A THIRD COPY.** The problem was:
`decide` answers `undefined` when the gate does not, so deleting a twin removes the only implementation `npm test` can
reach. **But a decision does not have to be REACHED at test time — it can be EMITTED at build time.**

**`mcp_tools.rs` ALREADY DOES EXACTLY THIS**, and its own words are the pattern: *"**THE PRODUCER.**
`gateway/src/mcp-tools.ts` is exactly this function's output, and the freshness test is a byte comparison against
it."* `render_typescript()` writes the file, `typescript_path()` names it, and the manifest classifies it
**GENERATED** — which is why it is not one of the 69.

**SO THE SHAPE OF EVERY REMAINING LANDING IS:**
1. the decision lives in Rust (where it already does, for 147 of the 195 exported names);
2. **the Rust RENDERS the TypeScript module that carries it**, beside `render_typescript`;
3. the rendered module is committed and classified **GENERATED** with its producer named (the manifest's fourth field,
   which the gate already refuses a GENERATED rule without);
4. the hand-written file **imports the emitted module and deletes its twins** — so its remaining body is I/O, caching
   and calls, which is a **BOUNDARY**;
5. and **`npm test` keeps working, because the emitted module is JavaScript** — no stub, no wasm in the test path, and
   **no third copy: the emission IS the Rust's output, and the freshness test is a byte comparison.**

**COUNTED FOR THE FIRST CANDIDATE, `store/settings.ts`**: its three decisions (`normalizeSetting`,
`globalSettingEnabled`, and the round-94/round-439 canonicalization) move into one emitted module; `settings.ts` keeps
`env.KEYS.get`, `env.KEYS.put`/`delete`, the cache and the reads. **That is 69 -> 68, and the same shape applies to
every file whose residual decisions are PURE** — which is exactly the set the decision route already reaches.

**AND IT EXPLAINS WHAT THE DECISION ROUTE IS *NOT* FOR.** The route is for a decision needed at REQUEST time by
TypeScript that must not depend on the worker (the admin probes, the usage mappers). **The emission is for a decision
needed at MODULE-LOAD time and in tests** — including the synchronous tables `byok.ts` and `globalSettingEnabled` that
the route can never carry. **Two seams, two jobs, and neither is a second implementation of the other.**

---

## THE THIRD EMISSION, AND THE GATE THAT READ ITS OWN COMMENT (2026-10-13)

**`gateway/src/http.ts` IS A BOUNDARY NOW — 66 → 65 — AND THE EMISSION WAS FORCED BY THE STATUS PAGE, A CONSTRAINT
THE FIRST TWO EMISSIONS DID NOT HAVE.** `plugins/status.ts` calls these predicates, and the status page reports ON the
worker, so it must not ask it (`plugins.test.mjs`: *"the worker must never be asked"*). **Deletion was closed for the
same reason `/v1`'s was not available**: `npm test` cannot reach `/__decision`, so the TypeScript twin is the only
implementation the suite can run. Two seams unavailable, one left — which is the cleanest evidence yet that the
emission is a SEAM rather than a convenience.

**WHAT MOVED**, into the GENERATED `gateway/src/cors-logic.ts` (producer `gateway/wasm/src/cors.rs`): the allowlist
(`DEFAULT_ALLOWED_ORIGINS`), `allowedOrigins`, `isLoopbackOrigin`, `isLoopbackHost`, `isAllowedOrigin` (audit P2's
pair rule), the base header table — now a Rust `CORS_HEADERS` const that `cors_headers_for` builds from rather than
repeating — and the reflect/stamp pair. What is left in `http.ts` is extraction and assembly, plus ONE runtime
condition: `withCors` returns a 101/`webSocket` response untouched, because a `Response` cannot be rebuilt around a
live socket. The Rust needs no such branch — its upgrade path returns before `with_cors` — so that condition is the
platform's, not the product's, and the manifest's reason says so.

**AND THE EMITTED PRIMITIVES TAKE THE RUST'S SIGNATURE, NOT THE TYPESCRIPT'S** — `(origin, requestHost, configured)`
rather than `(request, env)`. That is what makes the file a boundary instead of a smaller twin: extracting the Origin
and the host is the adapter, the decision happens once, and the emitted text can be read beside `cors.rs` line by
line. **AND A DEAD TWIN WENT WITH IT**: `errorTypeForStatus`'s only occurrence in the whole tree was its own
definition, while its subject is live in `responses.rs` (`error_type_for_status`, three callers there) — the same "is
it USED" measurement that found `grants.ts` and `store/regkeys.ts`.

**AND A TEXT-READING GATE BROKE ON ITS OWN SUBJECT'S COMMENT — THE FINDING WORTH KEEPING.**
`agent/tests/proxy_cors_parity.rs` reads the gateway's allowlist out of the source as TEXT, anchored on the bare
`ALLOWED_ORIGINS`. Pointing it at the emission made it read **the emission's own doc comment** — which names the
anchors, because documenting them is its job — take the comment's empty `new Set([ … ])` for the list, and fail a file
that declares exactly what the gate asks for: *"could not read an allowlist — this proves nothing"*. Narrowing the
anchor to `const ALLOWED_ORIGINS` was **not** enough: the comment names that spelling too. **The rule the reader
actually means is the ASSIGNMENT** (`const ALLOWED_ORIGINS`, then `=`, then `new Set([`), and its fixture is now the
emitted comment's own prose — *prose can name a constant; only a declaration assigns one*. **A text-reading gate
cannot be made unbreakable; it can be made to FAIL LOUDLY instead of passing on prose**, and a test beside the emitter
holds the literal spelling for the same reason: an emitter that reformatted the list would leave BOTH text-reading
gates reading zero origins and passing.

**AND `production_host.rs` ABSORBED A MOVE WITHOUT MOVING.** The entry for the gateway's hostnames followed the list
into the emission (`gateway/src/http.ts` → `gateway/src/cors-logic.ts`), so the declared count stayed at **51** — one
entry left, one arrived, which is what a MOVE looks like in a ratchet that would otherwise have to be raised.

**AND THE LANDING WAS SWEPT INTO ANOTHER SESSION'S MERGE COMMIT, WHICH IS WHY THIS SECTION EXISTS.** Two sessions
were working in ONE checkout; this one staged its index, the other ran `git merge` on `main`, and the merge commit
(`1af19fdf`) carries these bytes under a message about DERP. **The changes are right and their own CI jobs are green;
the record was not that commit's to make**, so it is written here and in the commit that adds this section. The
operational rule this costs is one line: **one checkout, one writer — `git add` is not a private act when the index is
shared.**

---

## THE FOURTH LANDING: A DEAD FILE WAITS FOR ITS WITNESS (2026-10-13)

**`store/plugins.ts` IS DELETED — 65 → 64 — AND IT WENT ONE LANDING AFTER `store/regkeys.ts`, WHICH IS THE FINDING.**
Both were dead in the same way (nothing outside `store.ts`'s barrel called their exports), and the second one's
deletion was blocked **not by a decision but by a witness**: three of its eight cases pinned SAME-ISOLATE behaviour —
write-through freshness, the stale-cache guard on revoke, the abort inside its sweep lock — and the frozen corpus is
blind to isolate state by construction (`run-cases.mjs` imports a FRESH module instance per case: *"a fresh instance
is also what a new isolate is"*). Deleting the file first would have deleted the only record of those arms.

**SO THE WITNESS WAS BUILT FIRST: three SAME-ISOLATE SEQUENCES in the corpus harness** — one instance, N requests, one
KV, the shape the device family's per-IP gate already had — and **each was then PROVED ABLE TO FAIL by a planted
mutation in `device_store.rs`** (the mutations and their measured results are in that section's own `MUTATION:` /
`RESULT:` header). Only then did the file, its eight cases and the manifest rule leave together. **The order is the
rule this landing exists to establish: a dead file is not deleted until the behaviour only its tests pinned is
witnessed where the code now lives, and a witness is not a witness until it has been broken on purpose.**

**AND THE FIRST VERSION OF THE REVOKE SEQUENCE MEASURED THE WRONG REFUSAL.** It read back through the PROXY, and after
`DELETE /api/devices/<name>` a proxy request is refused for the MISSING DEVICE whatever the link map says — an
assertion that cannot tell the two refusals apart, and one that PASSED against a worker mutated to keep the stale
cache. It reads back through `/api/upload`, which resolves the SAME plugin link (`upload.rs` →
`get_plugin_by_token`) and does not care whether the device still exists. **A sequence is only as good as its
read-back path** — and this is the second landing in a row where pointing an instrument at a new subject found the
instrument's own defect (the first was `proxy_cors_parity.rs`'s reader anchoring on a mention instead of an
assignment).

**AND ONE MUTATION NEEDED TWO EDITS, WHICH IS A MEASUREMENT RATHER THAN AN AWKWARDNESS.** The delete path carries TWO
protections — `read_plugin_links_fresh`'s `Option`, and `if removed > 0` — and **each alone is sufficient**:
corruption-as-empty removes nothing from an empty map, and an unconditional save is never reached when the `Option`
aborts. That is why this sequence pins the WRITE (no `{}` written over the record) rather than a status, and why
proving it took both edits. **A redundant guard is not a witness; the outcome is.**

**AND THE EIGHT CASES LEFT WITH A NAMED WITNESS EACH** — four corpus cases, three `device_registry` units and the three
new sequences, in a table where the cases were. `removePluginLink`, the SINGLE-token removal, has **no Rust
counterpart and never had a product caller**: one of those cases was its only consumer in the tree, so nothing was
ported for it — the honest end of a function that existed to be tested.

---

## THE FIFTH LANDING: AN EMISSION WHOSE SOURCE MODULE DID NOT EXIST (2026-10-13)

**`model-route.ts` IS A BOUNDARY NOW — 64 → 63 — AND IT IS THE FIRST EMISSION WHOSE RUST MODULE DID NOT EXIST BEFORE
THE LANDING.** The pattern above assumes step 1 (*"the decision lives in Rust"* — where it already did, for 147 of the
195 exported names); **these three decisions did not.** `GET|PUT /api/me/route` stays on the TypeScript path, and
`auth_routes.rs` says why in its own words: the handler resolves through the model CATALOGUE and the `RouteDO` —
*"a different surface with its own corpus, the catalogue, not identity"* — so the Rust worker never needed the key
rule, `og`'s breaker leg or the fallback policy, and **they had to be PORTED before they could be emitted.**

**SO THE ORDER FOR THIS SHAPE IS: PORT → TEST → EMIT → REPLACE.** `gateway/wasm/src/auto_route.rs` holds the three
decisions with their own tests and three written mutations (the environment leg of the key rule, the truthiness of an
empty `envKey`, the ladder walk), renders `model-route-logic.ts`, and `model-route.ts` keeps the reads. **And a ported
module whose only caller is the emission is NOT the "foundation, not a landing" the decision route refuses** — the
emission IS the caller: the module the console runs is the Rust's output, and the Rust's own tests are what hold it.

**AND THE PORT TURNED A COMMENT INTO A WITNESS.** The ladder's comment says it *"must be a SUBSET of the catalogue"* —
a typo there is a fallback that can never satisfy `isModelUsable`, and the ladder would skip it in silence — and
nothing checked it. It does now, in Rust, against the worker's own `registry::model_spec`, for all five rungs. **A
sentence in a comment is a sentence nobody runs**, which is this repository's oldest lesson.

**AND THE EMISSION'S INTERFACE WAS SHAPED BY AN EXTENSION POINT, NOT BY CONVENIENCE.** `channelKeyOk` takes the
channel's RULE as an argument instead of looking one up, because `CHANNEL_KEY_RULES` is mutable in TypeScript and
`registerChannelKey` is an OCP extension point a channel registers through; a rule that did its own lookup in Rust
could not be extended from there. **Deriving must not quietly remove an extension point** — the same rule `byok.rs`
carries one file over.

---

## THE SIXTH LANDING: A HOST IS KEPT, ITS DECISIONS MOVE (2026-10-13)

**`route-do.ts` IS A BOUNDARY NOW — 63 → 62 — AND ITS HOST COULD NOT BE PORTED AT ALL.** A Durable Object class IS
its runtime: `state.storage` and `fetch` are the platform's, and there is no Durable Object in the Rust worker's world.
That is the Electron shell's shape one landing over — **a host is kept and its decisions move** — and it decides where
the boundary falls: the emitted gate takes two STRINGS rather than a `Request`, because reading a header off a request
is the host's job, and an `async` decision route could not have served a predicate that `authorized()` calls
synchronously.

**AND THE PORT SINGLE-SOURCED A NAME THAT WAS A LITERAL ON ONE SIDE.** `lib.rs`'s `breaker_call` — the Rust worker IS
the DO's client — sent `"x-do-auth"` while the gate that VERIFIES that header lived only in TypeScript.
`DO_AUTH_HEADER` is one name now, and `the_header_name_is_the_one_the_client_sends` reads `lib.rs` and fails if it goes
back to spelling it. **A wire with a name on only one side is a wire that can drift in silence**, and this one had
been.

**AND THE GATE'S ONE ASYMMETRY IS WHY IT IS NOT A COMPARISON**: `safeEq("","")` is TRUE, so a gate written as a bare
compare waves every headerless caller through on a deployment that never set `DO_AUTH`. *The empty-secret guard is gate
policy; the comparison is `auth.ts`'s primitive* — the source said so in a comment, and the emission carries both the
comment and the truth table beside the guard.

**AND THE DISPATCH'S TWO LOOK-ALIKE ARMS SURVIVED BECAUSE THE PORT KEPT THE PARSED VALUE**: an EMPTY uid is missing
(truthiness, not presence — `?uid=` is a 400), and `model: 0`/`model: false` STORE while `null` deletes. A port that
sniffed `String(model)` first would have turned `null` into `"null"` and stored it. **The decision is made on the
VALUE, so the port takes the value** — `Option<&Value>`, not `Option<&str>`.

---

## THE SEVENTH LANDING: TWO LANDINGS, TWO WIRES, ONE NAME EACH (2026-10-13)

**`gate.ts` IS A BOUNDARY NOW — 62 → 61 — AND IT IS THE SECOND LANDING IN A ROW TO FIND ONE WIRE SPELLED TWICE.**
`route-do.ts` spelled `"x-do-auth"` beside a Rust client that spelled it too; `gate.ts`'s `DECISION_URL` spelled
`/__decision` while `decisions.rs`'s `in_family` spelled it again. **Neither spelling was wrong on its own — that is
what makes this class invisible**: each end works, each end's tests pass, and only a reader holding BOTH ends sees the
second copy.

**AND THE PATTERN IS STRUCTURAL RATHER THAN ACCIDENTAL: A WIRE WHOSE TWO ENDS LIVE IN TWO LANGUAGES GETS ITS NAME
WRITTEN TWICE.** The migration makes that the normal case — the Rust worker and the TypeScript console speak to each
other constantly — so the rule this pair of landings earns is: **when a landing moves one end of a wire, name the wire
and make the OTHER end read the name.** Twice now that other end has been the side nobody was touching: the Rust
client in the first landing, the Rust server in the second. Both are guarded by a test that reads the other side's
source and fails on the literal's return, because **a constant nobody is forced to read is a comment.**

**AND THE DECISION THIS FILE OWNS IS ONE DISTINCTION, WHICH IS WHY IT WAS WORTH OWNING.** `undefined` is a gate that
is absent, refused or unreachable — the ONLY case a caller may fall back to its TypeScript twin on — and `null` is a
decision that RAN and answered null. **Collapsing them would let a wrong `null` from Rust be silently corrected by the
twin**, which is the one failure this route must not hide. `decision_value` answers `Option<Value>`, so the two cases
are two VALUES, and a body with no `value` key is the first rather than the second — the source read `body.value`, and
a missing key is `undefined` in JavaScript. **A distinction that lives in a comment is a distinction a port loses; one
that lives in a type is a distinction it cannot.**
