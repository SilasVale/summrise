# One core, thin hosts — the target architecture, and the migration that reaches it

**2026-10-13 · the plan the operator asked for: migrate to Rust, plan the whole architecture, make each module
whole, spend the time on code rather than on CI.**

This document does not replace the two that came before it — it sits ON them:

* `specs/2026-10-08-every-decision-is-rust-design.md` — **the rule**: a decision lives in Rust, its TypeScript twin is
  either deleted or EMITTED from the Rust, and a gate counts what is left. Also the settled defects (6.1–6.6),
  including **wasm crates are PEERS and compose through the host** — which constrains this plan (see §5).
* `specs/2026-10-10-the-console-workers-tail-design.md` — **the measured map** of the tail: what is left, why each
  piece is blocked, and the emission seam that unblocks the module-load cases.

What is new here is the part neither covers: **the shape of the whole product when the migration is done**, and the
redesign of the two big modules (`gateway`, `agent`) that gets there.

---

## 0 · Where the product stands, MEASURED 2026-10-13

| side | what it is | size |
| --- | --- | --- |
| `gateway/wasm/src` | the Rust worker — the decision owner for every route it serves | **44 modules · 27,207 lines** |
| `gateway/src` | the TypeScript console host (emitted `*-logic.ts` excluded) | **30 files · 9,032 lines**, **59 LOGIC** |
| `agent` | the Windows agent — one binary, one plugin registry, one panel | **75 files · 59,451 lines** |
| `index`, `proxies`, `relay` | CDN, egress, self-hosted DERP | — |

Sixty landings' worth of decisions already live in Rust. What remains on the TypeScript side is **59 LOGIC files**,
and the last eight landings each moved one to three of them — the right direction at the wrong rate.

**THE RATE IS THE FIRST THING THIS PLAN CHANGES, AND THE OPERATOR SAID WHY: the per-landing ceremony (a full
`all-gates` run plus a watched CI pipeline for one file) spends the day on machines.** §7 fixes that, and the fix is
not "skip the gates" — it is "make a landing worth the gates".

---

## 1 · The target, in one sentence

**ONE PURE RUST CORE OWNS EVERY DECISION; THREE HOSTS EXECUTE IT AND DECIDE NOTHING.**

```
                         ┌──────────────────────────────┐
                         │  summrise-core   (pure Rust) │   no wasm-bindgen, no JS, no I/O types
                         │  · the catalogue & registry  │   decisions as TOTAL FUNCTIONS:
                         │  · the wire shapes           │   data in → data out, errors as values
                         │  · the routing/limit/redact  │
                         │    rules, as rules           │
                         └───────────┬──────────────────┘
             ┌───────────────────────┼───────────────────────┐
             │                       │                       │
   ┌─────────▼─────────┐   ┌─────────▼─────────┐   ┌─────────▼──────────┐
   │ gateway worker    │   │ agent (native)    │   │ console / panel    │
   │ (wasm32)          │   │ (windows exe)     │   │ (browser + node)   │
   │ SERVE + DECIDE    │   │ DO + DECIDE       │   │ RENDER ONLY        │
   │ the only server   │   │ the only device   │   │ no product rules   │
   └───────────────────┘   └───────────────────┘   └────────────────────┘
```

Three consequences, and each one is a rule:

1. **THE CORE IS PURE.** No `web_sys`, no `worker`, no `std::fs`, no async runtime: a function in `summrise-core`
   takes values and returns values. That is what lets ONE implementation run in the worker, in the agent, and (as
   emitted text) in the console — and what makes it testable with `cargo test` alone, which is the fastest gate in
   this repository.
2. **THE HOSTS ARE THIN, AND THEIR THINNESS IS COUNTED.** A host may read the platform (a request, a secret, a
   socket), call the core, and execute what it decided. Every host line that contains a product condition is a
   landing that has not happened yet — which is exactly what `js_boundary_inventory` measures today.
3. **THE WIRES HAVE ONE NAME.** Every value that crosses two languages is a constant in the core, and the OTHER side
   is made to read it (the rule the last three landings paid for: `x-do-auth`, `/__decision`, `"unknown"`).

---

## 2 · The module map, and what each module owns

### 2.1 `summrise-core` (new, pure Rust) — the home of every decision that is not worker-specific

Proposed layout, each directory a seam rather than a folder:

| module | owns | who needs it today |
| --- | --- | --- |
| `catalogue` | the model registry (24 specs), the wire spellings, the dialect table, the route prefixes | worker · agent (`model-drift`) · console |
| `wire` | the JSON shapes that cross a process: decisions, sessions, devices, providers, usage | all three |
| `limits` | the rate-limit budget rules, the counter key, the TTLs, the bounds | worker · agent (local API) |
| `guard` | hostname/SSRF rules, allowlists, redaction rules | worker · agent (`mcp_browser` already calls them) |
| `text` | the JS-faithful primitives (`js_text`, `js_number`, truthiness, `trim` semantics) | worker · agent |
| `secrets` | the `salt:hash` format, token shapes, constant-time compare | worker · agent (device auth) |

**NOT in the core, deliberately:** anything that needs a platform handle (`Env`, `Request`, `Response`, `socket`), and
anything that is only true of ONE host (the worker's route table, the agent's plugin registry).

### 2.2 `gateway` — one Rust worker, and a console host that shrinks to a binding

* **`gateway/wasm` stays the worker**, and stops being the home of shared rules: it depends on `summrise-core` and
  keeps only what is worker-shaped (the route table, the DO clients, the wire adapters).
* **`gateway/src` (TypeScript) has one destination: nothing but `host`.** Today it is 9,032 lines; the end state is
  the binding plumbing (`WASM_GATE`, `decide`, the front door's dispatch) and nothing else. Every remaining LOGIC
  file is one of three shapes, and each has a named end:
  1. **a twin of a Rust rule** → emitted (8 landed; the rest are §6's queue);
  2. **a route only TypeScript serves** → ported to the worker, then deleted (`/api/admin/*`, `/api/me/route`,
     the probe surface);
  3. **a genuine host duty** (cookie parsing, `crypto.subtle` handles, `Response` construction) → stays, classified
     BOUNDARY with its reason written down.
* **The console's data comes from the worker**: `/__decision` for the module-load and test-reachable cases, the real
  routes for everything else. No new twin is ever created.

### 2.3 `agent` — one binary, a plugin registry, and a panel that renders

The agent is already Rust and already plugin-shaped; the redesign is about **the seams it shares with the gateway**:

* the core arrives by path dependency; `summrise-model-drift` and `mcp_browser` stop carrying their own copies of
  catalogue/guard rules (the 6.6 rule holds: the wasm policy crates stay PEERS and compose through the host);
* **the panel is a host**, so the same rule applies inside the binary: `src/web/` reads a request, calls the core (or
  a plugin), and writes a response — no product condition;
* **the local control channel moves off TCP ports where a pipe is better** (`specs/2026-10-08` §6.2 already names it):
  a pipe cannot be dialled by a page, which is what makes it the right door for control;
* **one release flow**: `publish-release.sh` stays the only channel, and the new crate joins its pack inputs.

---

## 3 · The gateway redesign in one table

| family | today | target | how |
| --- | --- | --- | --- |
| identity & session (`/api/auth/*`, `/api/me/token/*`, `/api/me/keys`) | Rust worker | Rust worker | done |
| devices (REST + proxy + relay, MCP terminal) | Rust worker | Rust worker | done |
| MCP surface | Rust worker | Rust worker | done |
| status/health | Rust worker | Rust worker | done |
| **admin** (`/api/admin/*`) | TypeScript | worker | port the routes; the PARSER first (measured: `parseProviderSpec` is the largest single gap, ~190 lines of byte-exact messages) |
| **`/api/me/route`** (model=auto) | TypeScript | worker | the decisions are already emitted (`auto_route.rs`); the route follows |
| **probes** (`/api/admin/*/test`, usage) | TypeScript + `/__decision` | worker | the mappers are ported; the dials move |
| console shell (`/`, assets) | TypeScript | TypeScript | it is a host: assets and cookies, no rules |

**The order is the admin family, because it is the largest remaining block AND its parser is the only shape in the
tail that is a genuine PORT rather than a re-point.**

---

## 4 · What "a perfect module" means, measurably

A module is whole when all five hold — and each one is already enforced by something in this repository:

1. **one owner per rule** — no rule is written twice in two languages (the wire-name tests of the last three
   landings are the pattern);
2. **a total function per decision** — data in, data out, errors as values; no hidden environment (this is what makes
   the core portable);
3. **a gate per rule, with a mutation proof** — `MUTATION:`/`RESULT:` in the header, as every gate here carries;
4. **a named constant per wire**, read by BOTH ends;
5. **no product condition in a host** — `js_boundary_inventory` is the count, and it may only fall.

---

## 5 · The constraint that shapes everything: wasm crates are peers

`specs/2026-10-08` §6.6 is settled and pinned: wasm-bindgen exports every `#[wasm_bindgen]` item in the crate GRAPH, so
two wasm crates that depend on each other ship the module graph twice, with two copies of their `thread_local!` state.
**`summrise-core` is therefore a PLAIN LIBRARY: no `wasm_bindgen` attribute anywhere in it**, and every host composes
its own thin wrapper. A function that needs to cross into JS crosses through the HOST's wrapper, never through the
core.

---

## 6 · The migration order, with the sizes that decide it

Measured on `main@d2b0f676` (LOGIC 59):

| # | landing | what it is | why here |
| --- | --- | --- | --- |
| 1 | **`summrise-core` exists** | the crate, one migrated rule set (the guard rules that are ALREADY duplicated: `device.rs`'s host rules), both hosts depending on it | it is the plan's spine, and it removes a live duplication on the way |
| 2 | **`store/providers.ts`** | PORT `parseProviderSpec`/`parseProviderModel` to core, EMIT the TS | the largest genuine gap; unblocks the admin family |
| 3 | **the admin family** | port `/api/admin/*` to the worker (providers, users, devices, settings) | biggest block; ends the TS-only write paths |
| 4 | **`/api/me/route`** | port the route onto the already-emitted `auto_route.rs` | small, and it closes the catalogue surface |
| 5 | **the probe surface** | the six provider dials + three usage endpoints | the last TypeScript dials |
| 6 | **the twins' farewell** | delete each `*-logic.ts` that the worker can answer directly | the emission is a seam, not a destination |
| 7 | **`gateway/src` is a host** | 9,032 lines → the binding | definition of done |

**Batching rule for every landing: one MODULE FAMILY per branch, not one file.** A branch may carry three or four
files that share a seam and a gate; it must carry the gate's mutation proof for whichever rule moved.

---

## 7 · The working method, changed where the operator said to change it

What the operator asked for — *don't waste large amounts of time on CI* — is a statement about the RATIO of machine
time to code time, not about the gates. Concretely:

1. **A landing is bigger and rarer.** One branch = one family (§6). The gates run once per landing rather than once
   per file.
2. **CI is started, not watched.** Push, then keep working; the result is read at the END of the session. The
   repository's own rule ("no push while CI is in flight") is kept for RELEASE commits, where a superseded run costs
   a tag — not for ordinary landings.
3. **The fast local gate is the one used while working**: `cargo test` in the touched crate, plus the target file's
   own suite. `all-gates` runs once, on the committed tree, before the merge.
4. **The `design` job's 100 ms press-timing probe is known-flaky under runner load** — a failure there is re-run
   once, with the reason recorded, rather than investigated as a product defect.

---

## 8 · Definition of done (unchanged, and now with a shape)

1. `js_boundary_inventory` green with **zero LOGIC files**;
2. `all-gates` green;
3. CI green on `main`, reached by merge (`--no-ff`);
4. **and the new one this plan adds**: no rule is written twice in two languages — the migrations of §6 are how that
   becomes true rather than asserted.

---

## LANDED 2026-10-13 — §6 row 1, the crate exists, and the four things finishing it cost

**`core/summrise-core` IS IN THE TREE AND `gateway/wasm` DEPENDS ON IT BY PATH.** The guard rules moved out of
`gateway/wasm/src/device.rs` into `core/summrise-core/src/guard.rs`, with the JS-faithful `js_number_of` in
`text.rs`; `device.rs` re-exports every name, so the worker's call sites, its frozen corpus and the emitted
`device-dial-logic.ts` keep their paths. What moved: `DEFAULT_DEVICE_HOST_SUFFIX`, `host_allow_error`,
`device_host_error`, `authority_prefix_ok`, `starts_like_a_scheme`, `dial_path`, `hostname_mismatch`,
`PROXY_PATH_REFUSAL`.

**AND THE FOUR THINGS THAT ONLY SHOWED UP WHEN IT WAS FINISHED, WHICH IS WHAT THIS SECTION IS FOR:**

1. **`production_host.rs` WENT 52 -> 53, AND IT IS A MOVE RATHER THAN A HOST BEING ADDED.** The file that DECIDES the
   device-host suffix is a new path; `gateway/wasm/src/device.rs` still names the host **once**, in the prose that
   records the move. Two declared files, one host, so the ratchet rises while the wire does not change. (The entry's
   own comment says so, because a reader who finds +1 will otherwise assume a host was added.)
2. **THE FIRST VERSION CARRIED `js_number_of` TWICE** — a private copy in `guard.rs` beside the `pub` one in
   `text.rs`, in the same crate, on the day the crate was created. §4's rule 1 is what caught it; `guard.rs` imports
   `crate::text::js_number_of` now. **A new crate is exactly where a rule gets written twice**, because there is no
   older module to notice the second copy.
3. **CI WOULD NOT HAVE BUILT IT AT ALL.** Every cargo step in `ci.yml` names its manifest path, so a crate nobody
   names is a crate whose tests are decoration — the sentence the `gateway/wasm` steps were added for on 2026-09-29.
   The three commands (`test`, `clippy -D warnings`, `fmt --check`) are in the `gateway` job, which already builds the
   crate as a path dependency; the fmt check is safe here even though `gateway/wasm`'s own is not, because that
   exemption is a fact about six pre-existing files rather than a policy.
4. **THE AGENT DOES NOT DEPEND ON IT YET, AND THE PLAN'S §6 ROW 1 SAYS BOTH HOSTS.** What the agent actually carries is
   narrower than §2.1 assumed: `agent/src/web/mod.rs` documents the same suffix trap **in prose** and validates through
   its own config — there is no second SSRF stack to delete. So the agent's win is the RULE, not a deletion, and it is
   the next host to join.

**AND THE ONE THING THAT IS NOT ABOUT THE CORE AT ALL.** This box is one HDD at 84% full, and the measurement that
matters is not the core's: `gateway/wasm/target` was 2.9 GB and `agent/target` 13 GB, of which 11 GB was `debug/deps`.
`~/.cargo/config.toml` now sets `debug = 0` for the `dev`/`test` profiles machine-wide, and
`scripts/clean-build-space.bash` returns what a run leaves. Measured after: `gateway/wasm/target` **403 MB**. **A gate
that cannot run because the disk is full is a gate that does not exist**, which is why this is in a plan about code.

---

## MEASURED 2026-10-13 — §6 ROW 2 HAS A PREREQUISITE, AND IT IS EIGHT COPIES OF THREE PRIMITIVES

**`parseProviderSpec` CANNOT MOVE BEFORE `text` DOES, AND THE REASON IS THE PARSE ITSELF RATHER THAN TASTE.** The
parser's decisions are made of JavaScript coercions — `String(body.prefix ?? "").trim()`, `Array.isArray`,
`Number(v)` + `Number.isInteger(n)`, `JSON.stringify` inside a message, and `Object.keys(raw)` for the
unsupported-field list. `summrise-core` owns none of them today, and the worker owns **eight near-copies**:

| where | what it is |
| --- | --- |
| `stream.rs:49` `js_text` | `String(x)` — arrays join with `,`, an object is `"[object Object]"` |
| `device_registry.rs:84` `js_falsy_string` · `:90` `js_truthy_string` · `:99` `js_to_string_of` · `:107` `js_falsy` · `:119` `js_truthy` · `:124` `js_to_string` | the same three rules, split by argument shape |
| `reliability.rs:26` `js_number` | `Number(x)` |
| `request_shape.rs:197` `js_truthy` · `decisions.rs:145` `js_string` · `tokens.rs:179` `js_string_of` · `auth_routes.rs:311` `js_to_string_of` · `translate.rs:72` `js_join_part` | five more, each written where it was needed |

**AND THEY ARE NOT THE SAME FUNCTION, WHICH IS THE PART THAT DECIDES THE WORK.** `stream::js_text` renders a number
with `Number::to_string`, while `device_registry::js_to_string` renders it with `js_number_string` — JavaScript's
`String(n)`, where `1e21` is `"1e+21"`. So a consolidation that picks either one changes behaviour somewhere, and
**which one is faithful is a question the frozen corpus answers, not the name.** That is the behaviour-diff method
of `specs/2026-10-10` applied to the primitives themselves, and it is landing 2's real first task:

* move the canonical `String()` / truthiness / `Number()` into `summrise-core::text`, with the case families that
  separate them (`-0`, `1e21`, `NaN`, an array of nulls, an object, a lone surrogate) pinned as tests;
* re-export from each host module so no call site moves, exactly as `device.rs` did for the guards;
* let the corpus and each host's own suite say which copy was wrong — a copy that was wrong is a live bug the
  consolidation finds, which is what makes this worth doing before the parser rather than after it.

**THREE OTHER FACTS MEASURED THE SAME DAY, EACH ONE A PREREQUISITE OR A TRAP:**

1. **THE CORE HAS NO `serde_json`, AND THE PARSER IS A JSON TRANSFORM.** `core/summrise-core/Cargo.toml` says "no
   dependencies, deliberately" — and the reason given is build time and blast radius, not purity: `serde_json` is a
   pure-Rust, no-I/O crate that every host already links. **The decision this plan takes: the core takes
   `serde_json` with `preserve_order`,** because the wire shapes are the core's own subject (§2.1) and a JSON
   parser that cannot see JSON cannot own them. The "no dependencies" note was a starting position, not a law.
2. **THE URL PARSE IS THE HOST'S, AND THE CORE MUST NOT OWN IT.** `new URL(rawBase)` is a WHATWG parser (JS) or the
   `url` crate (Rust) — a real dependency and a real platform difference. So the core takes the PARSED PARTS
   (`protocol`, `username`, `password`, `search`, `hash`, `hostname`, `origin`, `pathname`) and decides on them,
   exactly as `dial_plan` takes its parser. **The order is preserved by construction**: the host parses before the
   call and the core reads `base` only after the prefix, label and reserved-prefix refusals, so a body that is
   wrong twice is refused for the reason the shipping console refuses it.
3. **`RESERVED_PREFIXES` HAS NO RUST OWNER AT ALL.** `channels.ts:607` derives it — `ROUTE_INFO` minus the trailing
   slash, plus `"ds"` and `"amd"` — and `gateway/wasm` has no equivalent, so the worker cannot refuse a reserved
   prefix today. The core needs the rule (and the derivation, not a second literal list) before the parser can
   answer 409 in both languages.

### AND THE PREREQUISITE LANDED THE SAME DAY — what consolidating the primitives actually cost

**`summrise-core::text` OWNS THE COERCIONS NOW, AND EVERY HOST NAME IS A RE-EXPORT, SO NO CALL SITE MOVED.**
Ten files changed, 445 lines added and 233 removed, and **the copies were not the same function**: the
consolidation found one live divergence (`stream::js_text` rendered a `1.0` that arrived as a float as `"1.0"`
where the engine says `"1"`, and the frozen corpus does not cover it), one comment that was wrong about its own
subject (`guard.rs` called hex a "recorded divergence" when it was a missing arm, so the hostname rule read
`172.0x10.0.1` as public), and two unreachable-but-wrong arms (`tokens::js_string_of` on a nested empty array,
`request_shape::js_truthy` on NaN). **That is the argument for consolidating BEFORE porting the parser rather
than after it: the parser would have inherited whichever copy its author happened to read.**

**AND IT IS ALSO WHERE THE CORE TOOK ITS FIRST DEPENDENCY.** `Cargo.toml` said "no dependencies, deliberately"
and gave build time as the reason — which is still the reason to keep the list short, and cannot be a reason to
keep it empty when the shared subject is the JSON that crosses between the hosts. `serde_json` is pure Rust with
no I/O, and `preserve_order` is NOT enabled here because it changes `Map` for the whole build graph and the agent
does not enable it.

**SO §6 ROW 2 IS NOW UNBLOCKED, WITH TWO PREREQUISITES STILL OPEN AND BOTH NAMED**: `RESERVED_PREFIXES` needs a
Rust owner (item 3 above), and `parseProviderSpec`'s byte-exact messages need the `Number.isInteger`/`Array.isArray`
arms that now exist. Nothing else stands between this plan and the largest single gap in the console's tail.

---

## LANDED 2026-10-13 — §6 row 2, and it is the first emission the CORE produces (59 -> 58)

**`parseProviderSpec` IS RUST AND THE CONSOLE RUNS ITS OUTPUT.** `core/summrise-core/src/provider_spec.rs` owns
the parser (~200 lines whose every refusal is a response body), the vocabulary, the two prefix helpers and the
record shapes; `gateway/src/store/providers-logic.ts` is its emission; `store/providers.ts` keeps the KV pair, the
cache, the file-layer merge, the record views, the key resolution and the `new URL` call. **The console's 372-case
suite runs the Rust's own output**, which is what makes this a seam rather than a deletion.

**THE URL PARSE IS THE ONE THING THAT COULD NOT MOVE, AND SAYING SO IS THE DESIGN.** `new URL` is a WHATWG parser
the pure core must not carry, so the emitted decision takes the PARSED PARTS (`protocol`, `username`, `password`,
`search`, `hash`, `hostname`, `origin`, `pathname`) and the host hands them over — the `dial_plan` shape one core
module over. **The refusal order survives because the decision reads `base` only after its prefix, reserved-prefix
and label refusals**, so a body that is wrong twice is refused for the reason the shipping console refuses it.

**THREE DUPLICATIONS WENT WITH IT, EACH ONE HELD TOGETHER BY A TEST INSTEAD OF BY ONE OWNER:**

| rule | where it was written twice | what happens now |
| --- | --- | --- |
| `RESERVED_PREFIXES` | DERIVED in `channels.ts` from its console route table, and a `matches!` list in `routing.rs` | `catalogue.rs` owns it, `channels.ts` re-exports it, and the drift test pins the RUST list against the TYPESCRIPT table |
| `SUPPORTED_PROVIDER_APIS` | `providers.ts` (validation) and `routing.rs` (the upstream join) | one const in the core, two readers |
| `barePrefix` / `advertisedModelId` | `providers.ts` and `store.rs` | one implementation in the core |

**AND THE THIRD ROW FOUND A LIVE BUG.** `advertisedModelId` renders the wire through `String(wire ?? "")`, and the
worker's copy used `js_string` — so a NULL wire was advertised as `acme/null` where the source answers `acme/`.
Unreachable from the listing (an empty wire is skipped first) and wrong at the boundary, which is where the
decision route calls it.

**THE TWO THINGS THIS LANDING TAUGHT, FOR THE NEXT EMISSION'S SAKE:**

1. **AN EMITTED FILE IS STILL A FORMATTED FILE.** `npm run format:check` runs prettier over `src/`, so the
   producer must emit text prettier would not rewrite — the ONE line it wanted wrapped (`parseProviderModel`'s
   signature) is wrapped IN THE RUST TEMPLATE. The refresh command re-emits, the freshness test compares bytes,
   and both halves of that loop are now proven.
2. **IT IMPORTS THE SSRF GUARD RATHER THAN RESTATING IT**: `deviceHostError` is an emission of `device.rs`, and a
   second private-address list would be the one duplication in this repository that is a HOLE rather than a bug.

**WHAT IS NEXT, AND IT IS §6 ROW 3**: the admin family. The parser was the prerequisite and it is done; `/api/admin/*`
is now a route port rather than a parser port, and `store/users.ts`, `store/models.ts` and `store/devices.ts` are
the stores it reads.

---

## LANDED 2026-10-14 — two more store hosts, and the pattern the emissions keep paying for (58 -> 56)

**`store/users.ts` AND `store/cache.ts` ARE HOSTS NOW** — their decisions are emitted from `user_store.rs` and
`store.rs`. What moved: the seeded admin's two names, the key names a user may hold, the token/salt/invite sizes,
`maskKey` and `userKeysStatus`; and the cache's two TTLs, its prefix list, its two bounds and the eviction
comparison. What stayed: KV access, the keyed locks, the `Map`s, the lock chain.

**AND THE REASON TO KEEP DOING THIS IS NO LONGER "THE COUNT FALLS" — IT IS THAT WRITING BOTH SIDES DOWN BESIDE
EACH OTHER KEEPS FINDING LIVE BUGS. THREE SO FAR, AND NOT ONE OF THEM WAS REACHABLE BY THE FROZEN CORPUS:**

| the divergence | what the Rust did | what the source does | why it survived |
| --- | --- | --- | --- |
| `advertisedModelId`'s null wire | `js_string(null)` → `"null"` | `String(wire ?? "")` → `""` | unreachable from the listing, reachable from the decision route |
| `maskKey`'s arm | `chars()` — five characters for `"abcd😀"` | UTF-16 units — six, so the SHORT arm | every ASCII row agrees, and a token is never astral |
| `AUTH_PREFIXES` | nine entries, `"providers:"` absent, with a comment asserting the source "keeps it as a COMMENTED entry" | ten entries, active, with a dated reason about credentials and egress destinations | the port's test iterated the constant it was checking, so it passed for any list |

**THE THIRD ONE IS THE INSTRUCTIVE ONE.** The comment was not lazy — it described a state the file had been in.
It had simply stopped being true, and the test that should have caught it was written as a loop over its own
subject. **A port carries a READING of its source, and a reading has a date**; the emission is what forces the
reading to be re-taken, because the two sides must be written down in one place. `providers:` being the missing
entry makes the point sharper: the record it caches carries a credential and an egress destination, and the
console propagates a deletion or a re-point within a minute while the worker was holding it for up to 24 hours.

**AND ONE TEST HAD STOPPED MEASURING ALTOGETHER, FOUND THE SAME WAY.** `auth.test.mjs` slices the `User` record
out of `store/users.ts` between two anchors; the end anchor left the file for the emission, `indexOf` answered
`-1`, and `slice(start, -1)` ran to the end of the module — passing against the whole host file instead of the
record, silently. It is bounded by the record's own braces now and fails loudly when they move.

**THE NEXT TWO CANDIDATES, MEASURED RATHER THAN GUESSED**: `store/admin.ts` (the admin-password seed and the
reads around it — `user_store.rs` already owns the password rules) and `store/models.ts`'s `isBuiltIn` plus the
catalogue projection. **THE ADMIN FAMILY (§6 ROW 3) IS STILL THE BIG ONE**, and the honest sequencing is: finish
the store hosts whose decisions already exist in Rust, then port the routes — because a route port that still
needs its store's decisions in TypeScript is a port with nothing to call.

---

## LANDED 2026-10-14 — the Access layer, and the fourth divergence (56 -> 55)

**`gateway/src/access.ts` IS A HOST NOW.** `access-logic.ts`, GENERATED from `access.rs`, carries the JWT's
three-part split, the four claim checks (`kid`, the audience, the PINNED issuer, a numeric `exp`), the `aud`
list's two shapes, the email and username derivations, the `access-email:`/`user:`/`token:` key families and the
record a provisioned account is written as. The host keeps the JWKS fetch and its cache, the `crypto.subtle`
RSASSA verify, the KV access and the provisioning lock.

**AND THE FOURTH DIVERGENCE IS THE ONE THAT DECIDES WHOSE TOKEN IS ACCEPTED.** `aud_list` mapped every element of
`payload.aud` through `js_to_string` — so `aud: [5]` became `["5"]` and MATCHED a deployment whose `ACCESS_AUD`
is the text `5`, where JavaScript's `[5].includes("5")` is **false**. Its own comment said a non-string entry was
"carried as the empty string", which is `String(null)`, not `js_to_string(5)`: **the comment and the code
disagreed, and neither was the rule.** Both sides compare strings only now.

**THE FOUR, IN ONE PLACE, BECAUSE THE PATTERN IS THE ARGUMENT FOR THE WHOLE EMISSION PATH:**

| # | where | the Rust | the source | why the corpus could not see it |
| --- | --- | --- | --- | --- |
| 1 | `advertisedModelId` | `js_string(null)` → `"null"` | `String(wire ?? "")` → `""` | unreachable from the listing; reachable from the decision route |
| 2 | `maskKey` | `chars()` — five for `"abcd😀"` | UTF-16 units — six, the SHORT arm | every ASCII row agrees; a token is never astral |
| 3 | `AUTH_PREFIXES` | `"providers:"` absent | present, ACTIVE, with a dated reason | the port's test iterated the constant it checked |
| 4 | `aud_list` | every element stringified | only strings can match | a malformed `aud` is not a corpus case |

**WHAT THE NEXT LANDING IS, MEASURED**: `store/models.ts` is the largest store host left (345 lines) and it is
NOT a one-file emission — its `catalogue` projection, `declaredReasoningEffort`'s three-source order, the
facet-override "emptied means deleted" rule, the disabled set's sort and the two custom-model upserts are all
decisions, so the file only becomes BOUNDARY when every one of them has an owner. **AND `store/admin.ts` NEEDS
ITS SEQUENCE IN RUST** (`seedAdmin`'s v1 migration, the empty-token backfill and the round-394 mint rule) before
it can be a host — which is also the first thing the admin family's routes will need. **So the next landing is
`models.ts` in full, and then the admin family's first slice, in that order.**

---

## LANDED 2026-10-14 — the model store, and a comment the code does not obey (55 -> 54)

**`store/models.ts` — the largest store host left — IS A HOST.** `models-logic.ts`, GENERATED from the new
`model_store.rs`, carries the three KV key names, the advertised-id merge, the `/v1/models` extras, the disabled
set's sort, the two custom-model upserts, the facet-override rule, the reasoning-effort chain and the CATALOGUE
PROJECTION. The host keeps the reads and writes, the cache, the file-layer merge, and the two things only a host
can do (flattening the advertised provider models, and parsing each record's `baseURL` for a card's `host`).

**AND IT IS THE FIRST LANDING WHOSE SUBJECT WAS AN EMISSION RATHER THAN A PORT**: `advertised_ids` and
`extra_model_entries` already existed in `store.rs`, so the work was rendering them, writing the six rules that
had no Rust home, and re-pointing the host. The new module re-exports the two that live next door and says so.

**THE FINDING, AND IT IS THE INVERSE OF THE LAST FOUR.** The projection's comment says a custom record REPLACING
a built-in's id is *"gone from this channel's list too"*. The filter does drop it — and then the custom loop
below finds the same card by prefix and pushes the wire name BACK. So the filter's exclusion is what a DISABLED
id gets, and a replacement returns through its own arm. **The first version of the Rust test asserted the
comment, saw `["mimo"]`, and was wrong.** The four earlier divergences were the RUST disagreeing with the source;
this one is a SENTENCE disagreeing with the code it describes, and the port follows the code. **A port may record
that difference; it may not "fix" it.** (The other find this session of the same class — `aud_list`'s comment
claiming non-strings became the empty string — was the reverse: there the code was wrong too.)

**WHAT IS LEFT IN THE STORE LAYER**: `store/admin.ts`, whose `seedAdmin` sequence (the v1 migration, the
empty-token backfill, the round-394 mint rule) has to be Rust before the file can be a host — and that sequence is
also what the admin family's routes need. Then the routes themselves (§6 row 3), which is where the console's tail
stops being a store problem and becomes a route problem.

---

## LANDED 2026-10-14 — the admin is Rust's, and the store layer is done (54 -> 53)

**`store/admin.ts` IS A HOST, AND IT IS THE FIRST EMISSION THAT TAKES THE PLATFORM AS AN ARGUMENT.** Every earlier
one is a pure predicate or a projection; the seed is a SEQUENCE of KV reads and writes whose ORDER is the product,
so `admin-logic.ts` renders it against a `deps` object (four KV calls, the cache, the mint, the clock, the
Worker's secrets). What moved: the v1 schema's addresses and id, the `salt:hash` format and its first-colon
split, the fresh record's shape, the backfill's test, the KV-authoritative password resolution, and the order the
three migrations run in.

**THREE MIGRATIONS IN THAT ORDER, EACH ONE ONCE A LOCKOUT** — the v1 -> v2 schema; the empty-token backfill (a
deployment seeded before the mint fix holds `token: ""` with the marker set, so the early returns lock it out
forever); and round-394's "mint only when no record exists" (a v1 upgrade used to fall through and clobber the
just-migrated record). The emission's test asserts the order and its mutation moves one early return.

**AND THE RUST TWIN HAS A CALLER, WHICH IS A FIX.** `seed_admin` runs at the top of the `/api/auth/*` family — the
only surface that cannot answer without it, because login mints a session for an id that must resolve and
`require_cookie_session` reads `user:<id>`. **A fresh deployment served by this worker had NO admin record at
all**: `getAdminPassword` migrated the Worker secret, so the password verified and the session it issued then
resolved to nothing.

**THE STORE LAYER IS DONE.** Every store the console has is now a host: `users`, `cache`, `providers`, `models`,
`admin`, plus `access` beside them. **What is left in the tail is the ROUTE layer** — `plugins/admin.ts` (the
admin family, §6 row 3), the six plugin files, `auth.ts`, `session.ts`, `upstream.ts` and the three that are
routing glue. The next landing is the admin family's first slice, and the sequencing question it answers is
whether a route port needs a differential (old vs new on the same request) or whether the corpus already covers
what it touches.

---

## LANDED 2026-10-14 — the admin family is COMPLETE, and the route layer has a method (17/17)

**ALL SEVENTEEN `/api/admin/*` ROUTES ARE THE WORKER'S**, over ten landings: the five reads, the model writes and
the probe, the provider CRUD, the password's two arms, the invite, the catalogue document and the public
catalogue. **AND WITH THEM THE ROUTE LAYER HAS A METHOD, WHICH IS THE PART WORTH KEEPING:**

1. **A ROUTE IS PORTED WHEN EVERY INPUT IS ALREADY RUST'S, AND NOT BEFORE.** `/api/admin/public` was the obvious
   first route and it waited until last-but-one, because its catalogue merges `ROUTE_INFO`'s prose with the
   CATALOGUE FILE — two tables with no Rust home. Porting it early would have meant guessing at both, with no
   oracle to catch the guess.
2. **THE WITNESS IS A FIXTURE RECORDED FROM THE SHIPPING PLUGIN, REPLAYED AGAINST THE BUILT WORKER.** The oracle
   drives the plugin's OWN ROUTE TABLE (`registerPlugins` + `dispatch`), so the matcher is pinned too; the replay
   installs the same frozen clock, the same frozen random stream and the same stubs, and compares **status, body,
   KV WRITES (with TTLs) and DIALS (method, URL, credential, redirect)**.
3. **A HARNESS THAT OMITS AN INPUT AGREES WITH EVERY ROUTE THAT ANSWERS "NO".** Four times, this pair produced a
   green that meant nothing: the KV namespace recorded as `{}` (`Object.entries(Map)`), the KV second argument
   (`get(key, "json")` in TypeScript, `get(key, {type})` in `workers-rs`), the request body never recorded, and
   the upstream stub living in the harness instead of in the case. **Every one was found by the cases that
   FAILED**, which is the argument for recording the input as part of the fixture rather than as part of the
   runner.
4. **A NEW ROUTE IS A NEW SUBJECT FOR EVERY PREDICATE THAT WAS ALREADY THERE.** `/api/admin/models` was claimed
   by the v1 fallback's `ends_with("/models")` arm — a PUBLIC listing, answered for an admin path with no session
   — and nothing noticed until the admin route existed to compare against.

**WHAT IS LEFT IN THE CONSOLE: 14 SOURCE FILES, 39 TEST FILES.** The stores, the families and the emissions are
done; what remains is the console's own plumbing — `session.ts`, `auth.ts`, `reliability.ts`, `upstream.ts`,
`tooling.ts`, `channels.ts` (the registry), `index.ts`, and the five plugin files. **AND `plugins/admin.ts` +
`plugins/models-probe.ts` (825 lines) ARE NOW DEAD WEIGHT ON THE TYPESCRIPT SIDE**: the worker serves every route
they hold.

## THE NEXT STEP IS RETIREMENT, NOT ANOTHER PORT, AND IT NEEDS A DECISION

The repository's own precedent is **A3: the TypeScript `/v1` half was DELETED, not emitted** — a route family
whose worker half is complete and proved gets deleted on the TS side, and the LOGIC count falls by deletion rather
than by emission. For the admin family that means three things in order:

1. **THE CONSOLE'S TRAFFIC MUST REACH THE WORKER.** Today `index.ts` dispatches its plugin registry first; the
   worker's admin family is additive. The cutover is a ROUTING decision (the deployed front door sends
   `/api/admin/*` to the worker) and it needs a **ROLLBACK STORY** that is not "redeploy": the TS handler stays in
   the bundle until the cutover has been observed, so a rollback is a routing change rather than a revert.
2. **THE FIXTURE IS THE EVIDENCE FOR THE DELETION.** 82 cases, byte for byte, including refusals, writes and
   dials — "the corpus checks the ANSWER, per path, for all recorded cases", which is exactly the argument A3
   used.
3. **THEN THE FILES GO, AND THE CAP FOLLOWS.** `plugins/admin.ts` and `plugins/models-probe.ts` are deleted in one
   landing (LOGIC 53 -> 51), not emitted: emitting them would keep a second implementation of seventeen routes
   alive to prove something the fixture already proves.

**AND THE NEXT FAMILY, MEASURED RATHER THAN GUESSED**: `reliability.ts` (456 lines) is the largest console file
whose decisions ALREADY EXIST IN RUST — `reliability.rs` owns `GLM_LOTTERY_MODEL`, the three timeouts,
`retry_policy_for`, `retry_decision`, `retry_wait_ms`, `may_retry_5xx` and `timeout_from` — so it is an EMISSION
with a host (the `BreakerDO` class, the dials, the isolate cache), not a port. `session.ts` is the same shape but
its FLOW is the decision (the order of the checks is the security property the Rust documents), so it needs the
`seedAdminSequence` treatment: the sequence emitted, the platform injected.

---

## LANDED 2026-10-14 — the console's plumbing follows the families (LOGIC 53 -> 49)

**THIRTEEN LANDINGS AFTER THE ADMIN FAMILY, AND EVERY ONE OF THEM FOUND SOMETHING.** The families were ports
(decisions written in Rust, routes proved by a fixture); what followed was the console's own plumbing, where the
decisions were ALREADY in Rust because the worker had needed them since the `/v1` cutover. That makes them
EMISSIONS — and an emission is a measurement of whether the two sides really agreed.

| file | what it was | what happened |
|---|---|---|
| `reliability.ts` | 456 lines | **first pure emission**: the budgets, the per-kind retry policy, the decision and its two client-visible details, the wait rule, the three timeout readers |
| `session.ts` | 151 lines | the **SEQUENCE** emitted (`cookieSession(deps)`): the order IS the security property |
| `auth.ts` | 209 lines | the primitives; **the file's "imports NOTHING" claim survives** — an emission, not an edge |
| `channels.ts` | 663 lines | the catalogue: `channels-logic.ts` + `routing-logic.ts`; the file is now **93 lines of re-exports** |
| `upstream.ts` | 489 lines | IN PROGRESS: the URL constants, the request-shaping helpers (`session.rs`), `stripBracket`, `wireModelName` |

**FIVE THINGS THE EMISSIONS FOUND, ALL OF THEM INVISIBLE TO A READING:**

1. **`retiredModelHint` did not trim or lowercase in Rust.** The console resolved `amd/DeepSeek-V4-Flash` to its
   replacement; the worker said "Unknown channel model". Nothing had compared them until the Rust function was
   rendered into TypeScript and the console's own test for that spelling failed. **The differential ran Rust ->
   TypeScript, which is the direction nobody plans for.**
2. **`/api/admin/models` was answered by the v1 fallback's `ends_with("/models")` arm** — a PUBLIC model listing,
   for an admin path, with no session. A new route is a new subject for every predicate that was already there.
3. **`HEALTH_CHANNELS` was mirrored in `lib.rs` "verbatim" and compared by NOTHING** — not even a field test, only
   an `/api/health` fixture snapshot.
4. **`MODEL_REGISTRY`'s two copies** were compared only by a corpus snapshot until round 21 added a field-for-field
   test — which the emission then retired, because one table needs no comparison.
5. **The URL constants are spelled twice**: as constants (now in Rust) and inline inside `pick_route`. Pinned by
   `the_url_constants_are_the_routers_own` — **and that test's first version was wrong**: it expected `og` +
   `/v1/messages` -> `OG_ZEN_ANTHROPIC`, and the router answered the CHAT endpoint, because `og` rides zen's
   OpenAI-compatible endpoint for BOTH paths. The mapping in the test is measured, not assumed.

**THE RULING THAT SETTLES THE NEXT SLICE (round 29).** "Which prefixes does this gateway own" has three
spellings: the core's `RESERVED_PREFIXES` minus the `"none"` sentinel (the worker's rule), `pick_route`'s match
arms, and the console's `hasOwnProperty(ROUTE_TABLE, prefix)`. **The console's is deliberately different**:
`registerRoute` writes into `ROUTE_TABLE` — a plugin seam the worker does not have — and `upstream.test.mjs` pins
that a REGISTERED prefix resolves through the table. For every prefix either side SHIPS they agree, and
`the_built_in_prefixes_are_the_route_tables` now pins the worker's two spellings (mutation: a prefix added to the
list with no arm fails it). **So rendering `pickRoute` means rendering the nine as the table's DATA, with
`registerRoute` and the table-based predicate staying the host's.**

## THE TEN LOGIC SOURCE FILES, CLASSIFIED — AND ONLY THREE OF THEM ARE WORK

**MEASURED, NOT LISTED** (the manifest's own classification against `git ls-files gateway/src`), because the two
estimates before this one were wrong in opposite directions: one called `plugins/status.ts` the next emission, the
other missed two files entirely.

| file | lines | verdict |
|---|---|---|
| `store/file-config.ts` | 217 | **TARGET, HALF DONE.** The two pure rules are emitted (`file-config-logic.ts`, round 34). **What remains is the VALIDATOR** (~90 lines, eight refusal arms, the same messages as the Rust's `parse_catalogue_file`) — renderable, and it is what moves the count to 48. **THE LOADER IS NOT EMITTABLE**: its document read is an `import` of `config/models.ts`, which is the host's by necessity; only its cache-poisoning CHECK is a rule, and splitting one function's check from its read would be worse than leaving both. |
| `store/devices.ts` | 196 | **TARGET.** The console's device registry; `device_store.rs` is the worker's. |
| `tooling.ts` | 260 | **TARGET, and the only one needing NEW Rust**: the two installer templates are decisions with no Rust home yet, and `summriseProbe` is a sequence. |
| `upstream.ts` | 397 | **BOUNDARY WITH A GATE.** Its rules are the console's own runtime (the panel asks "which channel is this model?"), and the **617-case translate corpus already pins them function by function** — `resolve_model` 81, `pick_route` 45, `model_spec` 25, `strip_bracket` 9, `csrf_cookie_violation` 13, `probe_env_key_name` 12. **An emission here would be a transcription, not a single source.** |
| `index.ts` | 520 | **BOUNDARY BY DEFINITION**: it IS the console's front door — the thing that hands routes to the worker. Nothing to port. |
| `plugins/registry.ts` | 246 | **BOUNDARY**: the plugin framework its own plugins are registered into. |
| `plugins/status.ts` | 314 | **BOUNDARY, and the worker says so twice** (see above). |
| `plugins/auth.ts` `plugins/admin.ts` `plugins/models-probe.ts` | 842 / 614 / 211 | **ROLLBACK FILES**, deleted after a deploy observes the cutover — LOGIC 49 -> 46 in one step. The operator's ruling (round 32): continue the emissions, delete after the deploy. |

**ONE MORE METHOD NOTE, MEASURED THE HARD WAY THIS ROUND**: comparing the two validators' refusal WORDING is the check that would catch a drift like the retired-hint one, and a grep for `"config/models.ts…"` cannot do it — the TypeScript builds each message from a `where` variable and the Rust spells it out, so the two extractions do not line up. The instrument that WOULD work is a table of (bad document → the message both sides produce), which needs the four arms fed synthetic input on both sides; that is the right shape for the next landing, and the reason this round did not claim a comparison it had not made.

**SO THE MIGRATION HAS THREE FILES OF REAL WORK LEFT, NOT TEN** — and a lesson worth more than the count: **every
time this session estimated the remaining work from a file LIST, the estimate was wrong; the two times it MEASURED
(the manifest's classification, and the worker's own comments), it was right.**

## WHAT IS LEFT, MEASURED

**LOGIC 49** — 10 source files and 39 test files. The source files, in the order the next landings should take:

  * **`upstream.ts` (finish it)**: `pickRoute` (nine arms as table data), `providerRoute`, `isBuiltInPrefix` (the
    host's, per the ruling), `resolveRoute` (**a sequence**: provider lookup -> the KV setting -> the table, so
    `seedAdminSequence` treatment). Then the file is a host and the count is 48.
  * **`tooling.ts` (260)**: needs the two installer templates moved into Rust (new decisions) and `summriseProbe`
    as a sequence. `buildHealth`, `probeEnvKeyName`, `encodeBase64Utf8` and the limiter's numbers are already
    `lib.rs`/`tooling.rs`/`ip_rate_limit.rs`.
  * **`plugins/registry.ts` (246)**, **`index.ts` (452)**: the plugin framework and the front door — the LAST
    layer, because the front door is what hands routes to the worker.
  * **`plugins/status.ts` (314) IS NOT A TARGET, AND THE WORKER SAYS SO TWICE** (`lib.rs`'s mcp arm, `mcp.rs`'s
    `in_family`): its route `GET /api/plugins/status` carries `routes` — the TypeScript plugin registry's own
    dispatch instrumentation (`routeStats(ctx)`: a registration index and a per-isolate hit counter for every
    route of every plugin). **Those counters are a property of the TypeScript plugin TABLE and of the traffic ONE
    isolate has seen**; a Rust worker cannot reproduce them without porting the plugin framework, and it must not
    guess them. The route stays on the TypeScript path, the cutover section of the harness proves that it does,
    and the honest label for this file is **a deliberate BOUNDARY, not a migration debt.** (A boundary the plan
    itself got wrong once: this list named it as the next emission until the worker's own comment was read.)
  * **`plugins/auth.ts` (842)**, **`plugins/admin.ts` (614)**, **`plugins/models-probe.ts` (211)**: **these three
    are the ROLLBACK FILES** for families the worker already serves and the front door already routes to. They are
    deleted after a deploy observes the cutover — not emitted — and that deletion is LOGIC 49 -> 46 in one step.
    The admin family's cutover is in `main` as of round 23; **what it waits for is the observation, which is the
    operator's call.**

**AND THE ROUTE LAYER'S OWN DISCIPLINE, WHICH THE LAST TEN LANDINGS VALIDATED**: a family is ported when every
input is Rust's; the witness is a fixture recorded from the shipping plugin and replayed against the built worker
(status, body, KV writes with TTLs, dials with credential and redirect); a harness that omits an input agrees
with every route that answers no (four times over); and a new route is a new subject for every predicate that was
already there.

