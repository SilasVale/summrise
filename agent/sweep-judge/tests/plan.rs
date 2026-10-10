//! THE DRIVER'S LADDER AND ITS BUDGETS — the last decision `agent/scripts/lib/sweep/*.cjs` still held.
//!
//! **WHAT IS ALREADY RUST, SO THIS FILE DOES NOT DUPLICATE IT.** `summrise-sweep-plan` owns WHICH SURFACES a
//! run visits, in what order, at which density/theme/viewport/mode, which passes are WANTED given `--passes`
//! (`wants()`), and the policy caps that travel in `plan.caps` (`ack_budget_ms`, `ack_settings_targets`, the
//! curated target lists). All of that arrived in slice 1 of this landing and is embedded at emit time.
//!
//! What was left in JavaScript is the ORDER the passes run IN on one surface and the numbers they spend while
//! doing it — the `if (wants("focus")) … if (wants("press")) …` ladder in each payload, and the literals
//! inside each pass (a poll count, a settle, a retry). Both are decisions by the manifest's own definition
//! ("a number that decides whether evidence EXISTS is a policy"), both used to live at their call sites, and
//! neither is derivable from `sweep-plan` — whose vocabulary is in the order the payload's CODE MENTIONS the
//! passes, not the order it RUNS them. Measured on the panel, the two really do differ: `pages` is first in
//! the vocabulary and fifth in the run.
//!
//! **THE LADDER ORDER IS THE DECISION THIS TASK MOVES**, and the literal below was read out of
//! `agent/scripts/lib/sweep/panel-run.cjs` by listing the `report.<field>.push(...)` sites in source order
//! and collapsing the ones that belong to one rung:
//!
//! ```text
//! timing 244 · focus 249 · idle 263 · press 279 · rows/surfaces/names/sse 295-303 (pages)
//! targets 493 · ack 945 · unstyled 1090 · hover 1162 · motion 1204 · reflow 1230
//! ```
//!
//! The panel is the tool this plan is FOR, and the reason is a measurement rather than a preference: it is
//! the only one of the three that runs every pass — `Tool::Panel::vocabulary()` is the superset — so its run
//! order is the only one that can be a canonical order. The other two tools run subsets, and WHICH subset is
//! `summrise-sweep-plan`'s `wants()`, not this file's.
//!
//! **AND THE TWO FILES ARE TIED TOGETHER RATHER THAN TRUSTED TO AGREE.** `the_ladder_covers_the_plans_own_vocabulary`
//! reads `Tool::Panel::vocabulary()` out of `summrise-sweep-plan` and requires this ladder to name exactly
//! those passes. A pass added to the plan and forgotten here would otherwise be a rung no driver ever runs,
//! and — the direction that hides better — a rung here that the plan does not know about is a driver asking
//! `wants()` about a name that will always answer false.
//!
//! MUTATION: swap the first two rungs of `LADDER` (`Timing` and `Focus`).
//! RESULT:   `cargo test -p summrise-sweep-judge --test plan` → exit 101, MEASURED:
//!
//! ```text
//! the_ladder_is_the_order_the_payload_runs ... FAILED
//!   left:  ["focus", "timing", "idle", "press", ...]
//!   right: ["timing", "focus", "idle", "press", ...]
//! only_keeps_the_ladder_order_not_the_callers ... FAILED
//! test result: FAILED. 6 passed; 2 failed
//! ```
//!
//! The second failure is the same defect seen through `only`: a caller that asks for two rungs gets them in
//! the LADDER's order, and that property is only meaningful while the ladder's order is the payload's.

use std::collections::BTreeMap;
use summrise_sweep_judge::plan::{canonical_plan, only, Section};
use summrise_sweep_plan::Tool;

fn names(sections: &[Section]) -> Vec<&'static str> {
    sections.iter().map(|s| s.name()).collect()
}

/// The literal, because the order is the decision. If this list has to change, the payload's own ladder
/// changed first — and the commit that changes it should say which `report.<field>.push` moved.
#[test]
fn the_ladder_is_the_order_the_payload_runs() {
    assert_eq!(
        names(&canonical_plan().sections),
        vec![
            "timing", "focus", "idle", "press", "pages", "targets", "ack", "unstyled", "hover",
            "motion", "reflow",
        ]
    );
}

/// **THE TIE TO `summrise-sweep-plan`.** The names are that crate's vocabulary — one definition, checked —
/// and this test is what stops the two crates' ideas of "a pass" from drifting apart in either direction.
#[test]
fn the_ladder_covers_the_plans_own_vocabulary() {
    let plan_names = Tool::Panel.vocabulary();
    assert!(
        !plan_names.is_empty(),
        "the plan's vocabulary is empty, so this check would pass against any ladder at all"
    );
    let ladder = names(&canonical_plan().sections);
    let missing: Vec<&&str> = plan_names.iter().filter(|p| !ladder.contains(p)).collect();
    assert!(
        missing.is_empty(),
        "the plan knows these passes and the driver's ladder has no rung for them: {missing:?}"
    );
    let extra: Vec<&&str> = ladder.iter().filter(|p| !plan_names.contains(p)).collect();
    assert!(
        extra.is_empty(),
        "the ladder names passes the plan does not know, so `wants()` would answer false for every one of \
         them: {extra:?}"
    );
    assert_eq!(
        ladder.len(),
        plan_names.len(),
        "the two lists are the same set but different sizes"
    );
}

/// `only` filters and never reorders — the same rule the e2e crate's `plan_from_cli` states, and it is worth
/// asserting here because the caller's argument order is the obvious thing for an implementation to follow.
#[test]
fn only_keeps_the_ladder_order_not_the_callers() {
    let p = only(&["reflow".to_string(), "timing".to_string()]).expect("both names are known");
    assert_eq!(names(&p.sections), vec!["timing", "reflow"]);
}

/// The refusal, and the half that makes it usable: an unknown name is a CONFIGURATION ERROR rather than a
/// filter that matches nothing, and the message lists every name that would have worked.
#[test]
fn an_unknown_name_is_refused_and_the_message_names_every_known_section() {
    let e = only(&["timing".to_string(), "timing-typo".to_string()])
        .expect_err("a typo must be refused");
    assert!(
        e.contains("timing-typo"),
        "the message must name what was asked for: {e}"
    );
    for s in canonical_plan().sections {
        assert!(
            e.contains(s.name()),
            "the message must name `{}` among the known sections: {e}",
            s.name()
        );
    }
    assert!(
        e.contains("known"),
        "the message must say the list is the known ones: {e}"
    );
}

/// An empty selection is refused too — `only(&[])` would otherwise return a plan that runs nothing and reads
/// exactly like a clean run, which is the failure the refusal exists to prevent.
#[test]
fn an_empty_selection_is_refused_rather_than_selecting_nothing() {
    let e = only(&[]).expect_err("an empty selection is a configuration error");
    assert!(e.contains("no section"), "{e}");
    let blank = only(&["  ".to_string()]).expect_err("a blank name is not a section");
    assert!(blank.contains("no section"), "{blank}");
}

/// **REVIEW FOCUS 4: A BUDGET OF ZERO IS ZERO.** The payload caps with `Math.min(cap, n)`, so `0` is a real
/// zero — "measure nothing" — and the thing that must NOT do that is an ABSENT budget, which means the
/// caller's own number stands. Two different inputs, two different answers, and a `unwrap_or(0)` or an
/// `unwrap_or(MAX)` on the wrong side of them gets one of the two wrong.
#[test]
fn a_budget_of_zero_is_zero_and_an_absent_budget_is_not() {
    let p = canonical_plan();
    assert_eq!(
        p.budget("discover_cap"),
        Some(16),
        "the panel's discovery cap is measured, not chosen"
    );
    assert_eq!(p.cap("discover_cap", 100), 16, "the budget is the ceiling");
    assert_eq!(
        p.cap("discover_cap", 4),
        4,
        "the caller's number is the floor when it is smaller"
    );

    let mut zeroed = p.clone();
    zeroed.budgets.insert("discover_cap".to_string(), 0);
    assert_eq!(
        zeroed.cap("discover_cap", 100),
        0,
        "a cap of 0 means NOTHING, not unlimited"
    );
    assert_eq!(zeroed.cap("discover_cap", 0), 0);

    assert_eq!(p.budget("no_such_budget"), None);
    assert_eq!(
        p.cap("no_such_budget", 100),
        100,
        "an absent budget is not a zero: it is a budget this plan does not set, so the caller's number \
         stands"
    );
}

/// The budget table is the numbers the PASSES spend, and every one of them was read out of
/// `design-sweep.mjs` rather than chosen here. Pinned as a literal because a silent edit to one of these is
/// a silent change to how much evidence a run collects.
#[test]
fn the_budgets_are_the_numbers_the_passes_actually_spend() {
    let b: &BTreeMap<String, u64> = &canonical_plan().budgets;
    let expected: &[(&str, u64)] = &[
        // `idlePass(page, ms = 2500)` and the probe's own sample cap.
        ("idle_window_ms", 2500),
        ("idle_samples", 8),
        // `focusPass(page, N, …)` — the three call sites, named per tool because they really do differ.
        ("focus_presses_panel", 14),
        ("focus_presses_console", 16),
        ("focus_presses_landing", 14),
        // `ackPass`'s baseline settle, its two poll loops, the retry between them, and the window it asks
        // the page about.
        ("ack_settle_ms", 260),
        ("ack_polls", 60),
        ("ack_poll_ms", 16),
        ("ack_retry_settle_ms", 300),
        ("ack_retry_return_ms", 120),
        ("ack_clear_polls", 200),
        ("ack_clear_ms", 25),
        ("ack_window_before_ms", 60),
        ("ack_window_after_ms", 250),
        // `revealPass`'s settle and the restore after it.
        ("reveal_settle_ms", 320),
        ("reveal_restore_ms", 80),
        // `pressPass`: hover settle, hold, and the release.
        ("press_settle_ms", 260),
        ("press_hold_ms", 140),
        ("press_release_ms", 60),
        // The panel's discovered-press cap (`discover: 16` at `panel-run.cjs:503`).
        ("discover_cap", 16),
        // `motionPass` renders TWICE — once without the preference and once with — and the second render is
        // what makes the axis mean anything.
        ("motion_renders", 2),
    ];
    for (k, v) in expected {
        assert_eq!(b.get(*k), Some(v), "budget `{k}`");
    }
    assert_eq!(
        b.len(),
        expected.len(),
        "the budget table has entries this test does not pin: {:?}",
        b.keys()
            .filter(|k| !expected.iter().any(|(n, _)| n == k))
            .collect::<Vec<_>>()
    );
}

/// The plan round-trips through JSON, because that is how it reaches the driver.
#[test]
fn the_plan_round_trips_through_json() {
    let p = canonical_plan();
    let json = serde_json::to_string(&p).expect("the plan serializes");
    assert!(json.contains("\"timing\""), "{json}");
    assert!(json.contains("\"idle_window_ms\""), "{json}");
    let back: summrise_sweep_judge::plan::SweepPlan =
        serde_json::from_str(&json).expect("the plan deserializes");
    assert_eq!(back, p);
}
