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
