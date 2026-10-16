# The model catalogue gets a lifecycle — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An advertised model row can never outlive the upstream that serves it, and a limited-time free tier can be adopted without a Rust release.

**Architecture:** Three new pure modules (`catalogue`, `lifecycle`, `catalogue_service`) plus lifecycle fields on store rows. A scheduled observation run writes one order-free ring entry per channel; the verdict is *read* from the ring. A committed catalogue slice is the floor; a Cron refresh keeps it current. Only the timer may write status; the manual probe stays a report.

**Tech Stack:** Rust compiled to wasm for Cloudflare Workers (`gateway/wasm`), KV storage, `wrangler` Cron Triggers, the TypeScript console in `gateway/ui`.

**Spec:** `docs/superpowers/specs/2026-10-15-the-model-catalogue-gets-a-lifecycle-design.md` — read it with this plan; the plan argues from it.

> **ADOPTED INTO THIS REPOSITORY 2026-10-15, WITH TWO CORRECTIONS FROM THE TREE IT MEETS.** It arrived
> untracked in a second checkout, so it is committed here before any of it is executed — a plan that lives in one
> working tree is a plan one `git clean` can delete. **The corrections are facts about `summrise-main`, not
> opinions:**
>
> 1. **`HEALTH_CHANNELS` IS IN `gateway/wasm/src/registry.rs`, NOT `lib.rs`** (line 55), together with
>    `HEALTH_PRIORITY` — they moved so the channel registry could be EMITTED. Task 1's edit goes there.
> 2. **AND THAT EMISSION IS A STEP TASK 1 DOES NOT LIST**: `registry.rs` renders
>    `gateway/src/channels-logic.ts`, so "20 -> 19" must be followed by
>    `SUMMRISE_REFRESH_CHANNELS=1 cargo test --manifest-path gateway/wasm/Cargo.toml registry::emission`, or the
>    twin's freshness test is red. The file did not exist when this plan was written.
>
> **A third correction is smaller and mechanical**: the emitted twin's own test is no longer at
> `model_store.rs:1018` — line numbers move; the emission is intact.
>
> **Everything else was checked rather than assumed**: `advertised_ids_for` is at `admin_routes.rs:141` exactly as
> written, `og/ox-alpha-free` is still a health card (`registry.rs:63`) and still a registry row (`:234`),
> `route-oracle.mjs` is indeed gone (the warning about hand-diffing an expectation stands), `gateway/ui/src/i18n.ts`
> exists, and the two byte-pinned corpora are where the plan says.

**Commands:** the gate list and the exact commands are `ci.yml` (read them there; `AGENTS.md` forbids copying them here). The suites do not share a reporter: `cargo test` → `test result: ok. N passed`; `npm test` in `gateway/` → `ℹ pass N`; `npx vitest run` in `panel-react/` → `Tests N passed`. Read the exit code, not the output.

## Global Constraints

- **Rust first.** A change to a `.js`/`.mjs`/`.cjs` file needs a reason; `agent/tests/js_boundary_inventory.rs` classifies every tracked JS/TS file and caps the LOGIC count.
- **`/v1/models` and `/api/health` bytes are pinned** by `gateway/wasm/fixtures/*.json`. Any change to them is a deliberate, reviewed diff in the same commit.
- **Every gate carries its own `MUTATION:`/`RESULT:` header**; the commit message carries the measurement and a `VERIFIED:` line.
- **`gateway/src/store/models-logic.ts` is GENERATED** from `gateway/wasm/src/model_store.rs` (test at `model_store.rs:1018`). Edit the Rust; the twin follows.
- **No undeclared production hostname** in a tracked file (`agent/tests/production_host.rs` is a ratchet). `models.dev` is a third party, not ours.
- **Deploys need the operator.** This plan produces commits.
- **`K = 2`** (`DEMOTE_AFTER`), and every value in this plan is exact: do not round it to a "sensible" default.

## Review Focus

These are the five failure modes the spec implies and a reasonable person would still expect to work. Each has its test in the task that owns the code.

1. **A 200 with an empty or renamed listing body** must hide nothing (it is "inconclusive", not "clean"). → Task 4.
2. **A catalogue row whose dialect is not the channel's arm** (a `responses` shape on a chat channel) must be refused and listed, never adopted. → Task 7.
3. **A demoted row that comes back upstream** must return to `/v1/models` without a human. → Task 6.
4. **Two runs racing, or one half-written KV value**, must not change a verdict. → Task 5.
5. **The console's manual probe**, run with an admin's own BYOK key, must write no status. → Task 6.

---

## File Structure

| File | Responsible for |
|---|---|
| `gateway/wasm/src/lib.rs` *(modify)* | `HEALTH_CHANNELS` — the 20 labels; one leaves |
| `gateway/wasm/src/catalogue.rs` *(create)* | parsing/validating the slice into `Facts`; pure |
| `gateway/wasm/src/catalogue/opencode.json`, `opencode-go.json` *(create)* | the committed slice (data) |
| `scripts/update-model-catalogue.sh` *(create)* | fetching `models.dev/api.json` and extracting the slice |
| `gateway/wasm/src/lifecycle.rs` *(create)* | the observation ring and the verdict; pure |
| `gateway/wasm/src/model_store.rs` *(modify)* | lifecycle fields on a row; the emitted twin |
| `gateway/wasm/src/store.rs` *(modify)* | `advertised_ids` — the store filter |
| `gateway/wasm/src/catalogue_service.rs` *(create)* | KV cache + TTL + floor; the injected fetcher |
| `gateway/wasm/src/models_probe.rs` *(modify)* | the listing fetch (worker key) + the conclusive floor |
| `gateway/wasm/src/admin_routes.rs` *(modify)* | `advertised_ids_for`, the admin state, the probe route |
| `gateway/wasm/wrangler.jsonc` *(modify)* | the Cron Trigger |
| `gateway/ui/src/i18n.ts` + the Models page *(modify)* | the copy and the two switches |

---

### Task 1: Step 0 — the dead row stops being a health card

**Files:**
- Modify: `gateway/wasm/src/lib.rs` (`HEALTH_CHANNELS`, 20 entries → 19)
- Modify: `gateway/wasm/fixtures/front-door-cors-corpus.json`, `admin-corpus.json` (the `/api/health` bodies)
- Test: a new `#[cfg(test)] mod tests` in `gateway/wasm/src/lib.rs`

**Interfaces:**
- Consumes: `registry::MODEL_REGISTRY` (unchanged by this task).
- Produces: `pub(crate) fn health_model_ids() -> Vec<&'static str>` — the card models, in order, for Task 2's ratchet.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn no_card_names_a_row_we_do_not_advertise() {
    // D5: a card is a label, and a label for a dead row is the lie this design ends.
    assert!(!HEALTH_CHANNELS.iter().any(|(_, m)| *m == "og/ox-alpha-free"));
}

#[test]
fn the_card_list_is_nineteen() {
    assert_eq!(HEALTH_CHANNELS.len(), 19);
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p summrise-gate-wasm no_card_names_a_row_we_do_not_advertise the_card_list_is_nineteen` (from the repo root; if the package name differs, take it from `ci.yml`).
Expected: FAIL — the id is present, and the length is 20.

- [ ] **Step 3: Delete the `("og", "og/ox-alpha-free")` entry** from `HEALTH_CHANNELS` and add `health_model_ids()` beside it.

- [ ] **Step 4: Diff the frozen bytes on purpose**

Run the gateway job's command from `ci.yml`. Expected: failures in exactly the corpora that carry the `/api/health` body. Hand-edit those bodies to 19 cards — the diff is the record; note it in the commit message. **Do not** regenerate with a retired oracle (`route-oracle.mjs` is gone; hand-diffing an expectation is the sanctioned move, hand-editing a recording is not).

- [ ] **Step 5: The operator action, written down, not performed here**

The row leaves `/v1/models` by data, not by code: `POST /api/admin/models` family's disable route (or the console's own toggle) writes `og/ox-alpha-free` into `models:disabled`. Record the exact call in the commit message and in the PR body; the code commit does not perform it.

- [ ] **Step 6: Commit**

```bash
git add gateway/wasm/src/lib.rs gateway/wasm/fixtures/
git commit -m "fix(health): drop og/ox-alpha-free's card — the row answers 400 upstream"
```

---

### Task 2: Lifecycle fields on a store row, and the advertised filter

**Files:**
- Modify: `gateway/wasm/src/model_store.rs`
- Modify: `gateway/wasm/src/store.rs:728` (`advertised_ids`)
- Modify: `gateway/wasm/src/admin_routes.rs:141` (`advertised_ids_for`) and `:543` (`admin_model_state`)
- Modify: `gateway/src/store/models-logic.ts` **only by re-running the emitter's test**, never by hand
- Test: `model_store.rs`'s own `#[cfg(test)]`, `store.rs`'s own, plus the corpus case in `gateway/wasm/fixtures/models-corpus.json`

**Interfaces:**
- Consumes: `health_model_ids()` (Task 1), `registry::MODEL_REGISTRY`.
- Produces: `pub struct Lifecycle { pub status: Status, pub ephemeral: bool, pub verified_at: Option<i64>, pub verified_by: Option<String>, pub assumed: Vec<String> }`; `pub enum Status { Active, Alpha, Beta, Deprecated }`; `pub fn lifecycle_of(row: &Value) -> Lifecycle`; `pub async fn set_lifecycle(env: &Env, id: &str, l: &Lifecycle) -> Vec<Value>`; `model_store::is_deprecated(row) -> bool`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_row_without_lifecycle_fields_is_active() { /* lifecycle_of(&json!({"id":"og/x"})) == Lifecycle::active() */ }

#[test]
fn a_deprecated_store_row_leaves_the_advertised_set() { /* advertised_ids() drops it; a curated row with the same status does NOT move */ }

#[test]
fn a_deprecated_store_row_is_still_reported_to_the_console() { /* /api/admin/models still carries it, with its status */ }
```

- [ ] **Step 2: Run them and watch them fail** (`cargo test -p summrise-gate-wasm lifecycle`), then implement `Lifecycle`, `lifecycle_of`, `set_lifecycle`, and the filter in `advertised_ids`.

- [ ] **Step 3: Make the three contract moves explicit**

`advertised_ids_for` is emitted into the twin and pinned by the corpus case in `models-corpus.json` ("one disabled id is removed AND the created values shift"). Run the emitter test, commit the regenerated twin, and update the corpus deliberately. **A curated row is filtered only by `models:disabled`; only store rows are filtered by `status`** — `is_built_in` is the predicate, and Step 4 pins it.

- [ ] **Step 4: Write the ratchet that keeps it true**

```rust
#[test]
fn no_health_card_names_a_non_curated_row() {
    // D5, enforced rather than written down.
    for id in crate::health_model_ids() { assert!(crate::registry::MODEL_REGISTRY.iter().any(|m| m.id == id)); }
}
```

- [ ] **Step 5: Run the gateway job and commit**

```bash
git add gateway/wasm/src worktree:gateway/src/store/models-logic.ts gateway/wasm/fixtures/models-corpus.json
git commit -m "feat(catalogue): a store row carries its lifecycle, and a deprecated one leaves /v1/models"
```

---

### Task 3: The catalogue slice and its extractor

**Files:**
- Create: `scripts/update-model-catalogue.sh`
- Create: `gateway/wasm/src/catalogue/opencode.json`, `gateway/wasm/src/catalogue/opencode-go.json`
- Create: `gateway/wasm/src/catalogue.rs`
- Test: `gateway/wasm/src/catalogue.rs`'s `#[cfg(test)]`

**Interfaces:**
- Produces: `pub struct Facts { pub shape: Option<Shape>, pub input_cost: Option<f64>, pub output_cost: Option<f64>, pub context: Option<i64>, pub output: Option<i64>, pub status: Option<&'static str>, pub vision: bool }`; `pub enum Shape { Completions, Responses }`; `pub fn slice() -> &'static Slice`; `Slice::facts_for(&self, id: &str) -> Option<&Facts>`.

- [ ] **Step 1: Write the extractor** — `scripts/update-model-catalogue.sh` fetches `https://models.dev/api.json` (**2,006,627 B measured 2026-10-15**), keeps only the `opencode` and `opencode-go` providers, maps each row to the six facts above, sorts by id, and writes the two JSON files. `bash` + `jq` is acceptable here: this is a build-time tool, not product logic.

- [ ] **Step 2: Run it, and commit what it wrote** — the two files are data; the commit message carries the byte count and the catalogue's own date.

- [ ] **Step 3: Write the failing tests**

```rust
#[test]
fn an_unknown_field_is_dropped_and_a_bad_id_is_ignored() { /* {"nonsense":1}, {"id":"../../etc/passwd"} */ }

#[test]
fn facts_come_through() { /* shape, cost, context, vision for deepseek-v4.1-flash */ }

#[test]
fn a_row_the_slice_does_not_carry_is_none() { /* facts_for("og/nope") is None — absence is not evidence */ }
```

- [ ] **Step 4: Implement `catalogue.rs`** — `include_str!` the two files, parse once, validate (`Shape` from `provider.shape`; unknown → `None`), and expose the lookup. No network, no KV: this module is pure and testable in-process.

- [ ] **Step 5: Commit**

```bash
git add scripts/update-model-catalogue.sh gateway/wasm/src/catalogue.rs gateway/wasm/src/catalogue/
git commit -m "feat(catalogue): the metadata slice is data, extracted by a script"
```

---

### Task 4: The listing fetch gets a worker key and a conclusive floor

**Files:**
- Modify: `gateway/wasm/src/models_probe.rs` (`probe`, `offered_ids`)
- Test: `models_probe.rs`'s `#[cfg(test)]`

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub fn conclusive(offered: &[String], prefix: &str, curated: &[&str]) -> bool` — the D3 floor; `pub async fn listing(env: &Env, prefix: &str, key: Option<&str>) -> Result<Vec<String>, RouteFailure>` — the listing call, never inference.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn an_empty_200_is_not_conclusive() { /* offered=[] → false */ }
#[test] fn an_unrecognised_body_is_not_conclusive() { /* offered_ids({"object":"list"}) == [] → false */ }
#[test] fn a_listing_without_a_curated_row_is_not_conclusive() { /* offered=["something-new"] → false */ }
#[test] fn a_listing_with_one_curated_row_is_conclusive() { /* offered=["deepseek-v4.1-flash"] → true */ }
```

- [ ] **Step 2: Run them and watch them fail**, then implement `conclusive` and route the timer's listing through `listing(env, "og", Some(worker_key))` — the worker's own key, because a scheduled run has no caller (`models_probe.rs:201` is the caller-key path and stays for the button).

- [ ] **Step 3: Pin the scope** — a `#[test] fn only_og_is_checkable_today()` over the prefix list (`og nv gmi cm` listable; `or qw r4 amd` not), so widening the scope later is a deliberate act.

- [ ] **Step 4: Commit**

```bash
git add gateway/wasm/src/models_probe.rs
git commit -m "feat(probe): a listing is conclusive only when it carries a curated row"
```

---

### Task 5: The observation ring — order-free state

**Files:**
- Create: `gateway/wasm/src/lifecycle.rs`
- Test: `lifecycle.rs`'s `#[cfg(test)]`

**Interfaces:**
- Produces: `pub const DEMOTE_AFTER: usize = 2;`; `pub struct Observation { pub run_id: String, pub at: i64, pub conclusive: bool, pub seen: Vec<String>, pub missing: Vec<String> }`; `pub fn push(ring: &[Observation], obs: Observation) -> Vec<Observation>` (keeps the last `DEMOTE_AFTER + 1`); `pub enum Verdict { Active, Demote, Unchanged }`; `pub fn verdict(ring: &[Observation], id: &str, normalise: impl Fn(&str) -> String) -> Verdict`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn one_missing_run_is_not_enough() { /* [miss] → Unchanged */ }
#[test] fn two_conclusive_missing_runs_demote() { /* [miss, miss] → Demote */ }
#[test] fn an_inconclusive_run_breaks_the_streak() { /* [miss, inconclusive, miss] → Unchanged */ }
#[test] fn a_seen_row_returns_to_active() { /* [miss, miss, seen] → Active */ }
#[test] fn a_lost_write_changes_no_verdict() { /* the ring is read, never incremented: [miss] + replay == [miss] */ }
#[test] fn the_comparison_is_normalised() { /* a wire-remapped id and a [1m]-bracketed id both match */ }
```

- [ ] **Step 2: Run them and watch them fail**, then implement the ring and the verdict. **No counter is incremented**: the verdict is a pure function of the last `K` entries.

- [ ] **Step 3: Commit**

```bash
git add gateway/wasm/src/lifecycle.rs
git commit -m "feat(lifecycle): the verdict is read from a ring, not incremented across isolates"
```

---

### Task 6: The timer writes; the button does not

**Files:**
- Create: `gateway/wasm/src/catalogue_service.rs`
- Modify: `gateway/wasm/src/admin_routes.rs` (the probe route and the admin state)
- Modify: `gateway/wasm/wrangler.jsonc` (the Cron Trigger — there is none today)
- Test: `catalogue_service.rs`'s `#[cfg(test)]`, `admin_routes.rs`'s own

**Interfaces:**
- Consumes: `catalogue::slice()`, `lifecycle::{push, verdict, DEMOTE_AFTER}`, `models_probe::listing`, `model_store::{lifecycle_of, set_lifecycle}`.
- Produces: `pub async fn facts(env: &Env) -> Slice` (KV → floor; TTL refresh); `pub async fn observe(env: &Env) -> Result<Observation, RouteFailure>` — the timer's whole job.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn an_unreachable_catalogue_serves_the_committed_slice() { /* injected fetcher fails → floor, no state change */ }
#[test] fn the_manual_probe_writes_no_status() { /* call the route's handler with a stub env; assert the KV writes are reads only */ }
#[test] fn the_timer_demotes_after_k_and_restores_on_sight() { /* drives observe() twice, then a seen run */ }
```

- [ ] **Step 2: Run them and watch them fail**, then implement `observe` (one ring entry per channel; write `status`/`verifiedAt`/`verifiedBy` on store rows only) and `facts` (KV cache + TTL + floor).

- [ ] **Step 3: Declare the Cron** in `gateway/wasm/wrangler.jsonc` and dispatch the scheduled event to `observe`. The period is the builder's call; it must be longer than the catalogue's TTL, not equal to it.

- [ ] **Step 4: Extend `admin_model_state`** so the console receives `status`, `ephemeral`, `verifiedAt`, `verifiedBy`, `assumed`, and the last ring's `missing` list per channel — the 待查 entries.

- [ ] **Step 5: Run the gateway job and commit**

```bash
git add gateway/wasm/src gateway/wasm/wrangler.jsonc
git commit -m "feat(catalogue): a timer observes listings and only it may demote a store row"
```

---

### Task 7: Adoption refuses what it cannot derive

**Files:**
- Modify: `gateway/wasm/src/admin_routes.rs` (`admin_add_model`'s validation)
- Modify: `gateway/ui/src/i18n.ts`, the console's Models page
- Test: `admin_routes.rs`'s own

**Interfaces:**
- Consumes: `catalogue::facts_for`, `model_store::parse_model_spec`, `Status`, `Lifecycle`.
- Produces: `pub fn adoptable(prefix: &str, id: &str, facts: Option<&Facts>, declared: &Value) -> Result<Lifecycle, Refusal>`; `pub enum Refusal { UnderivableDialect, UndeclaredRegion }`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_responses_shaped_row_on_a_chat_channel_is_refused_and_listed() { /* Refusal::UnderivableDialect */ }
#[test] fn a_row_needing_a_region_facet_is_refused_until_declared() { /* cat-jam-free: Refusal::UndeclaredRegion; with usEgress declared → Ok */ }
#[test] fn an_adopted_row_records_what_was_assumed() { /* lifecycle.assumed == ["usEgress"] */ }
#[test] fn a_catalogue_row_can_never_create_a_curated_row() { /* adopting onto a registry id is refused */ }
```

- [ ] **Step 2: Run them and watch them fail**, then implement `adoptable` and wire it into the add-model route; the catalogue fills `shape`/cost/limits, the operator fills the region.

- [ ] **Step 3: The two switches** — per-channel `auto-adopt` and `free tier`, both default **off**, stored beside the existing store keys, exposed to the console with their state. Auto-adopt only runs inside the timer's `observe`, and only for `og/` (Task 4's scope).

- [ ] **Step 4: The console copy** — the Models page shows 限时·未验证 for `ephemeral` rows, 上游已下线 for demoted ones, 此渠道无法定时核查 for the four uncheckable channels, and the `assumed` list. Strings live only in `gateway/ui/src/i18n.ts`, both languages.

- [ ] **Step 5: Run the gateway job (both suites) and commit**

```bash
git add gateway/wasm/src/admin_routes.rs gateway/ui/src
git commit -m "feat(console): adoption derives what it can and refuses what it cannot"
```

---

## Self-Review

**Spec coverage:** D1 → Tasks 2/7 (two classes, adoption); D2 → Task 2; D3 → Tasks 4/5/6 (floor, ring, one writer, store-rows-only, the three contract moves); D4 → Task 7; D5 → Tasks 1/2; D6 → Task 1's byte diff; D7 → Tasks 3/6 (slice, extractor, cache, floor, the three rules); §5 Step 0 → Task 1; §10's thirteen tests → Tasks 1 (D5), 4 (7, 8), 5 (2, 4-strip, 9), 6 (1, 2, 3, 10, 11), 7 (5, 6, 12, 13).

**Open:** the registry row's own removal and the fixture emitter are **not** in this plan — the spec puts them after Step 0, and Step 0's data hide needs neither. Add them as a follow-up plan once the emitter's shape is known.

**Type consistency:** `Lifecycle`/`Status` are defined in Task 2 and used in Tasks 6/7; `Facts`/`Shape` in Task 3, used in 6/7; `Observation`/`Verdict`/`DEMOTE_AFTER` in Task 5, used in 6; `health_model_ids()` in Task 1, used in Task 2's ratchet; `conclusive`/`listing` in Task 4, used in 6.

**Proportion:** seven tasks, each one testable deliverable, and each ends in a commit that works on its own.
