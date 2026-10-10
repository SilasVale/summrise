# The design sweeps' judge and driver are Rust — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move every DECISION in `agent/scripts/lib/design-sweep.mjs` (1,765 lines) and its five
`agent/scripts/lib/sweep/*.cjs` payloads (3,091 lines) into Rust, leaving the page-side probes and the
device-side run programs as JavaScript that only measure.

**Architecture:** The same split the e2e sweep landed on 2026-10-10 (`agent/e2e-judge/` + a 1,007-line JS
driver that decides nothing). The **probes** — template strings a browser evaluates — stay JS. The **passes**
(Playwright over a `page`) become the Rust driver. The **judge** — the pure functions and the policy lists —
goes into the existing `summrise-sweep-judge` crate, whose `hover.rs`, `contrast.rs` and `marks.rs` already are
sweep judgments. Every task below is verified against the JavaScript it replaces, on one tree, before that
JavaScript is deleted.

**Tech Stack:** Rust 2021 (the `agent/` workspace), Playwright over CDP, `serde_json` for the rows document,
Node only as the process that evaluates a probe string in a page.

**Spec:** `docs/superpowers/specs/2026-10-10-the-design-sweeps-split-design.md`

## Global Constraints

* `cargo` must run from `agent/` — the workspace root is there, not the repo root.
* Both feature configs must pass: `cargo test -p summrise-sweep-judge` and, for anything touching
  `summrise-agent`, `cargo test -p summrise-agent` with and without `--features terminal,keyring`.
* `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings` clean, both configs.
* **Every new gate carries its own `MUTATION:` / `RESULT:` block in its header**, naming the edit that must
  break it and what it said when it did.
* **`MAX_LOGIC` (91 on `main@f6ea434f`) is measured LAST**, by forcing the constant to 0, reading the gate's
  refusal, and writing that number with the tree it was measured on.
* A file that stays JavaScript needs a written reason in `agent/tests/fixtures/js-boundaries.txt`, and the
  reason for the probes is the only one that cannot be argued away: **`browser_run_script` takes a JS file**.
* `main` advances only by merge (`--no-ff`), never a direct commit.

## Review Focus

Five conditions the spec implies and no task's tests would otherwise reach. Each gets its test in the task
that owns the code, named here with the behaviour a person would expect:

1. **A probe string that no longer parses** — a page-side payload is JS inside a Rust template; a stray brace
   must fail loudly at emit, not return zero rows that read as a clean sweep.
2. **A colour with no saturation data** (`colour.r` missing) — `loudnessOf` must not divide by undefined and
   report `loud: false` as if it had measured; the JS returns `NaN` fields and `loud: false`.
3. **A press target that vanished between discovery and press** — the JS `pressDelta(null, pressed)` returns
   `[]`; Rust must agree rather than panic on a missing key.
4. **A budget of zero** — a cap of `0` must mean "nothing", not "unlimited"; the JS `Math.min(cap, n)` shape
   makes `0` a real zero.
5. **A rows document from an older schema** — a missing field must be an error naming the field, not a default
   that silently judges nothing.

---

### Task 1: `press_delta` — the smallest judge, and the shape the rest copies

**Files:**
- Create: `agent/sweep-judge/src/press.rs`
- Modify: `agent/sweep-judge/src/lib.rs` (one `pub mod press;` and one re-export)
- Test: `agent/sweep-judge/tests/press.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `pub fn press_delta(hovered: Option<&serde_json::Value>, pressed: Option<&serde_json::Value>) -> Vec<&'static str>` — the four keys `["transform", "opacity", "background", "filter"]` whose values differ, in that order, and `vec![]` if either side is `None`.

- [ ] **Step 1: Write the failing test** — the JS's three cases plus the two the JS leaves implicit:

```rust
#[test] fn no_press_reports_no_change() { assert!(press_delta(None, None).is_empty()); }
#[test] fn a_missing_side_reports_no_change() { assert!(press_delta(Some(&h()), None).is_empty()); }
#[test] fn only_the_differing_keys_are_reported() { /* hovered transform X, pressed transform Y -> ["transform"] */ }
#[test] fn the_order_is_the_js_key_order() { /* all four differ -> exactly that order */ }
```

- [ ] **Step 2: Run it to verify it fails** — `cd agent && cargo test -p summrise-sweep-judge --test press` → FAIL: `press_delta` not found.
- [ ] **Step 3: Implement `press_delta` in `agent/sweep-judge/src/press.rs`** — `KEYS` as a `const [&str; 4]`, `.filter()` over it, `None` short-circuit first. Header carries the `MUTATION:`/`RESULT:` block: *swap two entries of `KEYS`* → **the order test fails, naming both positions**.
- [ ] **Step 4: Run it to verify it passes** — same command → PASS, 4 passed.
- [ ] **Step 5: Commit** — `git add agent/sweep-judge/ && git commit -m "sweep-judge: press_delta, and the four keys in the JS's own order"`

### Task 2: `loudness_of` — and the review-focus case that no JS test covers

**Files:** Create `agent/sweep-judge/src/loudness.rs`; Modify `lib.rs`; Test `agent/sweep-judge/tests/loudness.rs`

**Interfaces:**
- Produces: `pub struct Loudness { pub sat: f64, pub l: f64, pub loud: bool }` and `pub fn loudness_of(colour: &serde_json::Value) -> Loudness`.

- [ ] **Step 1: Write the failing test** — the JS's own condition, copied verbatim from `design-sweep.mjs:1649`:
  `loud = is_finite(sat) && is_finite(l) && sat >= 0.35 && l >= 0.2 && l <= 0.9` — **and assert BOTH SIDES of each
  boundary** (0.349 vs 0.35, 0.199 vs 0.2, 0.899 vs 0.9), because a threshold tested only from inside passes for
  any threshold. **Plus Review Focus 2**: a colour with `r` missing -> `loud == false` **and** both fields
  not-finite, asserted explicitly rather than left to `NaN` propagation — the JS's `Number.isFinite` guard is
  what makes that `false` a measurement rather than an accident.
- [ ] **Step 2: Run it to verify it fails** → FAIL: `loudness_of` not found.
- [ ] **Step 3: Implement** it as the JS's own formula (`/255`, max/min, the HSL saturation), header mutation: *move `sat >= 0.35` to `0.34`* → **the boundary test fails**.
- [ ] **Step 4: Verify it passes** → PASS.
- [ ] **Step 5: Commit**.

### Task 3: the policy lists are decisions, so they are Rust data

> **ALREADY DONE WHEN THE LANDING MEASURED IT, AND THE DEFECT WAS UNDERNEATH.** `DECORATIVE_WAIVERS` is already
> Rust in `sweep-judge/src/tools/panel.rs`. What this task actually fixed: `design-sweep.mjs` carries a **second
> copy** of the list, read by `panel-render-audit.mjs`, and **nothing in the repository compared the two** — move a
> band in either and two instruments disagree silently. The gate now reads the JS array out of the file a browser is
> served and requires the Rust list to be the same list. **Task 4's plan data is likewise already `summrise-sweep-plan`**
> (slice 1; `panel-run.cjs:921` already reads `PLAN.plan.caps.ack_budget_ms`). **A plan written from a spec goes stale
> against the tree it lands on, and the only way to find that is to measure before porting.**

**Files:** Create `agent/sweep-judge/src/waivers.rs`; Modify `lib.rs`; Test `agent/sweep-judge/tests/waivers.rs`

**Interfaces:**
- Produces: `pub const DECORATIVE_WAIVERS: &[&str]` (verbatim from `design-sweep.mjs:27`) and `pub fn is_waived(sel: &str) -> bool`.

- [ ] **Step 1: Write the failing test** — every entry of the JS array is waived; a selector that is not is not; **and the list is non-empty** (a vacuity floor — *a check must prove it looked at something*).
- [ ] **Step 2-4:** the usual cycle. Header mutation: *delete one entry* → **its own membership test fails, naming the entry**.
- [ ] **Step 5: Commit**.

### Task 4: the plan data, exactly as the e2e landing did it

**Files:** Create `agent/sweep-judge/src/plan.rs`; Modify `lib.rs`; Test `agent/sweep-judge/tests/plan.rs`

**Interfaces:**
- Produces: `pub struct SweepPlan { pub sections: Vec<Section>, pub budgets: BTreeMap<String, u64> }`, `pub fn canonical_plan() -> SweepPlan`, and `pub fn only(names: &[String]) -> Result<SweepPlan, String>` whose error message lists the known sections (the e2e's `plan.rs` is the template, including its exit-2 refusal).

- [ ] **Step 1: Write the failing test** — **Review Focus 4**: a budget of `0` yields `0`, not "unbounded"; an unknown `--only` name errors and the message contains every known section; the section ORDER is asserted as a literal list (it is the decision this task moves).
- [ ] **Step 2-4:** the usual cycle. Header mutation: *reorder two sections* → **the order test fails**.
- [ ] **Step 5: Commit**.

### Task 5: the driver — the passes, and the probes it keeps

> **RULING R1, WHICH THE LANDING FOUND AND THIS PLAN GOT WRONG: THE PASSES KEEP THEIR `page.*` CALLS.** They are
> Playwright driving; the repository has **no Rust browser driver** and **no CDP crate in its lockfile** (the ancestor
> spec's §4.1 measured it), so moving them would be a **NEW dependency**, not a port. **What moves is every DECISION
> the passes were interleaved with** — so this task produces the rows-document contract and the decision surface
> beside it, not a browser driver in Rust. See the spec's R1 section.

**Files:** Create `agent/design-sweep-driver/` (bin crate) with `src/main.rs`, `src/passes.rs`; Test `agent/design-sweep-driver/tests/passes.rs`

**Interfaces:**
- Consumes: `SweepPlan` (Task 4), `press_delta` (1), `loudness_of` (2), `is_waived` (3).
- Produces: a binary that writes a **rows document** `{schema, base, plan, observations:[{id,args,result|error,at_ms}]}` and whose exit code says whether it COLLECTED (0), not whether the page passed — **the e2e driver's contract verbatim**. The probe strings move here as `const …_SOURCE: &str` **verbatim from the JS**, and a test asserts each one is embedded unchanged.

- [ ] **Step 1: Write the failing test** — **Review Focus 1**: every probe string is present verbatim and a deliberately broken one fails at emit; **Review Focus 5**: a rows document missing `observations` errors naming the field.
- [ ] **Step 2-4:** the cycle. Mutation: *truncate a probe string* → **the verbatim test fails, naming the probe**.
- [ ] **Step 5: Commit**.

### Task 6: the differential, then the deletion, then the manifest

**Files:** Modify `agent/tests/fixtures/js-boundaries.txt`, `agent/tests/js_boundary_inventory.rs`; Delete the moved parts of `agent/scripts/lib/design-sweep.mjs`

- [ ] **Step 1: Run both judges on ONE rows document** — the e2e landing's rule: the JavaScript judge and the Rust judge must agree before either the old file or the old path goes. Record the two verdicts in the commit.
- [ ] **Step 2: Delete what moved**, leaving the probes and the device-side run programs, each with its manifest reason.
- [ ] **Step 3: Reclassify `LOGIC | agent/scripts/ |` to `BOUNDARY`** with the probes as the reason.
- [ ] **Step 4: Measure `MAX_LOGIC`** — force to 0, read the refusal, write that number **and the tree**. **AND EXPECT IT NOT TO MOVE: this plan said 90 and that was wrong.** Every tracked JS file under `agent/scripts/` already has a longer, more specific BOUNDARY rule, so the directory rule is **fully shadowed and matches zero files**; the count on `dfa593c9` is **91, in two rules — `gateway/src/` 37 and `gateway/test/` 54** — which belong to another landing. **So this slice removes decisions from JavaScript without changing the count, and the report says that plainly rather than presenting an unchanged number as a result.**
- [ ] **Step 5: `bash scripts/test/all-gates.bash`** → the line to look for is `49 ok, 0 failed`; then commit.

## Self-Review

**Spec coverage:** §2's three parts → Tasks 1–5 (judge), 5 (driver), probes (kept, Task 5's verbatim test).
§3's inventory → the plan's file map and Task 6. §4's precedent → Task 4's template and Task 5's contract.
§5's three verifications → Task 6 steps 1, 3, 4. §7's risk (a collector's defect is invisible to Rust) → Task 5's
verbatim probe test is the *emit-time* half; the device half is unchanged and named in the spec.

**Step scan:** every step names one action and its check. Task 5 is the largest; it is one task because the
driver's pieces cannot be verified independently — the passes share the page and the rows schema.

**Type consistency:** `press_delta`·`loudness_of`·`is_waived`·`canonical_plan`·`only` are used in Task 5 with
the signatures declared in Tasks 1–4.

**Review Focus:** all five are placed — 1 → Task 5, 2 → Task 2, 3 → Task 1, 4 → Task 4, 5 → Task 5.

**Proportion:** the plan is ~180 lines for ~1,765 lines of source; bodies are omitted wherever the signature
and the test determine them.
