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
