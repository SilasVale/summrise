//! THE DRIVER'S LADDER AND ITS BUDGETS — what `agent/scripts/lib/sweep/*.cjs` still decided, as data.
//!
//! **TWO THINGS, AND THEY ARE THE TWO KINDS OF DECISION THE MANIFEST NAMES.** The ORDER the passes run in on
//! one surface (a sequencing decision, which the payloads held as an `if (wants("focus")) … if
//! (wants("press")) …` ladder threaded through 1,300 lines of driving) and the NUMBERS they spend while
//! doing it (a policy: a poll count, a settle, a retry — "a number that decides whether evidence EXISTS is a
//! policy, and the sweeps' rule already says policies travel as data").
//!
//! **WHAT THIS IS NOT.** It is not `summrise-sweep-plan`, which owns WHICH SURFACES are visited and WHICH
//! PASSES are wanted given `--passes`, and which already travelled as data before this landing. The two are
//! deliberately separate: the plan decides whether a rung is visited AT ALL, and this decides what the rung
//! does when it is. `the_ladder_covers_the_plans_own_vocabulary` in `tests/plan.rs` is the seam — the names
//! here must be exactly `Tool::Panel::vocabulary()`'s, so one crate cannot learn a pass the other has never
//! heard of.
//!
//! **WHY THE PANEL IS THE CANONICAL LADDER.** It is the only tool that runs every pass — its vocabulary is
//! the superset — so its run order is the only one that can be canonical. The console and the landing run
//! subsets and their selection is `wants()`, not this file.
//!
//! MUTATION: swap the first two rungs of `LADDER` (`Timing` and `Focus`).
//! RESULT:   `cargo test -p summrise-sweep-judge --test plan` → exit 101, MEASURED:
//!
//! ```text
//! the_ladder_is_the_order_the_payload_runs ... FAILED
//!   left:  ["focus", "timing", "idle", "press", …]
//!   right: ["timing", "focus", "idle", "press", …]
//! test result: FAILED. 7 passed; 1 failed
//! ```
//!
//! ONE test breaks, and the one that does NOT is worth the line: `only_keeps_the_ladder_order_not_the_callers`
//! stays green under this mutation, because the two rungs it selects (`timing`, `reflow`) keep their relative
//! order however the first two are swapped. It is a property of `only`, not a second witness of the ladder —
//! which is exactly the kind of thing a header that PREDICTED its result would have got wrong.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One rung of a design sweep, named by the `--passes` word that selects it.
///
/// The names are `summrise-sweep-plan`'s vocabulary and the ORDER is the panel payload's run order; the two
/// are different lists and both are asserted in `tests/plan.rs`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    Timing,
    Focus,
    Idle,
    Press,
    Pages,
    Targets,
    Ack,
    Unstyled,
    Hover,
    Motion,
    Reflow,
}

/// **THE PANEL'S RUN ORDER, AND THE ONLY PLACE IT IS WRITTEN DOWN.** Read out of
/// `agent/scripts/lib/sweep/panel-run.cjs` by taking the `report.<field>.push(...)` sites in source order and
/// collapsing the ones that belong to one rung — `timing 244 · focus 249 · idle 263 · press 279 ·
/// rows/surfaces/names/sse 295-303 (pages) · targets 493 · ack 945 · unstyled 1090 · hover 1162 ·
/// motion 1204 · reflow 1230`.
///
/// It is a decision rather than a consequence, and two of its steps say why: `timing` is first because it is
/// the navigation's own cost and measuring it after anything else would measure the wrong page, and `press`
/// comes before `pages` because pressing a control can navigate and a surface measured afterwards is a
/// different surface.
pub const LADDER: [Section; 11] = [
    Section::Timing,
    Section::Focus,
    Section::Idle,
    Section::Press,
    Section::Pages,
    Section::Targets,
    Section::Ack,
    Section::Unstyled,
    Section::Hover,
    Section::Motion,
    Section::Reflow,
];

impl Section {
    pub fn name(self) -> &'static str {
        match self {
            Section::Timing => "timing",
            Section::Focus => "focus",
            Section::Idle => "idle",
            Section::Press => "press",
            Section::Pages => "pages",
            Section::Targets => "targets",
            Section::Ack => "ack",
            Section::Unstyled => "unstyled",
            Section::Hover => "hover",
            Section::Motion => "motion",
            Section::Reflow => "reflow",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        LADDER.into_iter().find(|x| x.name() == s)
    }
}

/// The driver's plan: the rungs that run, in order, and the policy numbers they spend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweepPlan {
    pub sections: Vec<Section>,
    /// **`BTreeMap<String, u64>` RATHER THAN A STRUCT OF NAMED FIELDS, AND THAT IS THE SHAPE THE SUBJECT
    /// FORCES.** The three tools spend the same budget under different values (`focus_presses_panel` is 14
    /// and `focus_presses_console` is 16), a rung can be added without a schema change, and the map is what
    /// crosses JSON to a driver that reads a number by the name the pass knows it by. Every key is pinned by
    /// `the_budgets_are_the_numbers_the_passes_actually_spend`, so the looseness is in the type and not in
    /// the knowledge.
    pub budgets: BTreeMap<String, u64>,
}

impl SweepPlan {
    /// The budget, or `None` when this plan does not set one. **`None` IS NOT `Some(0)`** — see `cap`.
    pub fn budget(&self, name: &str) -> Option<u64> {
        self.budgets.get(name).copied()
    }

    /// **THE CAP, WITH THE ONE RULE THAT IS EASY TO GET WRONG: A BUDGET OF ZERO IS ZERO.** The payloads cap
    /// with `Math.min(cap, n)` — `discoverPressTargets` ends in `out.slice(0, cap)` — so a cap of `0` means
    /// "measure nothing" and is a legitimate answer. A plan that has no budget for `name` is a different
    /// thing: it is a budget this plan does not set, so the caller's own number stands.
    ///
    /// Review Focus 4 of the plan names this case, and it is the one a "sensible" implementation gets wrong
    /// in either direction: `unwrap_or(0)` turns every unset budget into a silent zero, and `unwrap_or(u64::MAX)`
    /// turns a deliberate zero into "unlimited".
    pub fn cap(&self, name: &str, n: u64) -> u64 {
        self.budget(name).map(|b| b.min(n)).unwrap_or(n)
    }

    /// Whether this rung runs.
    pub fn runs(&self, section: Section) -> bool {
        self.sections.contains(&section)
    }
}

/// Every rung, in the panel's order, with the numbers the passes actually spend.
pub fn canonical_plan() -> SweepPlan {
    SweepPlan {
        sections: LADDER.to_vec(),
        budgets: budgets(),
    }
}

/// **THE BUDGETS, EACH READ OUT OF `agent/scripts/lib/design-sweep.mjs` RATHER THAN CHOSEN HERE.** The line
/// reference beside each one is the literal it was taken from, because the value of a poll count is its
/// provenance: a number nobody can trace is a number the next person rounds.
fn budgets() -> BTreeMap<String, u64> {
    let mut b = BTreeMap::new();
    let mut put = |k: &str, v: u64| {
        b.insert(k.to_string(), v);
    };
    // `idlePass(page, ms = 2500)` — a SAMPLING WINDOW, not a wait for a condition, and the file's own
    // comment carries the measurement that set it (a 1-second period is visible in ~2.2 seconds).
    put("idle_window_ms", 2500);
    // `if (state.samples.length < 8)` — how many mutation samples the row names before it stops listing them.
    put("idle_samples", 8);
    // `focusPass(page, N, …)` at the three call sites: `panel-run.cjs:249`, `console-run.cjs:435`,
    // `landing-run.cjs:95`. Named per tool because the numbers really do differ — the console has two more
    // tab stops before the ring it is looking for.
    put("focus_presses_panel", 14);
    put("focus_presses_console", 16);
    put("focus_presses_landing", 14);
    // `ackPass`: the baseline settle after the hover (`await page.waitForTimeout(260)`, the "these sheets
    // transition in 120-200ms" settle), then two poll loops with a retry between them.
    put("ack_settle_ms", 260);
    put("ack_polls", 60);
    put("ack_poll_ms", 16);
    put("ack_retry_settle_ms", 300);
    put("ack_retry_return_ms", 120);
    // The clear loop: "the acknowledgement must CLEAR, or a control that spins forever is indistinguishable
    // from one that is still working".
    put("ack_clear_polls", 200);
    put("ack_clear_ms", 25);
    // The window the page is asked about, around the press's own clock: `x >= t - 60 && x <= t + 250`.
    put("ack_window_before_ms", 60);
    put("ack_window_after_ms", 250);
    // `revealPass`: the hover settle the revealed row needs, and the restore afterwards.
    put("reveal_settle_ms", 320);
    put("reveal_restore_ms", 80);
    // `pressPass`: "the hover must have SETTLED before the baseline is taken", the hold, and the gap after
    // the release.
    put("press_settle_ms", 260);
    put("press_hold_ms", 140);
    put("press_release_ms", 60);
    // The panel's discovered-press cap — `discover: 16` at `panel-run.cjs:503` — which is what stops a busy
    // page turning one surface into a minute of clicking.
    put("discover_cap", 16);
    // `motionPass` renders twice: a count without the preference and a count with it. One render would
    // measure the page, not the preference.
    put("motion_renders", 2);
    b
}

/// **`--only`, RESOLVED HERE RATHER THAN IN THE DRIVER**, for the reason `summrise-e2e-judge`'s plan gives:
/// an unknown name is a CONFIGURATION ERROR rather than a filter that matches nothing, because a renamed
/// rung would otherwise leave a step "passing" while testing nothing at all. The message names every rung
/// that would have worked — a refusal a reader cannot act on is a refusal that gets worked around.
///
/// The ORDER IS THE LADDER'S, NOT THE CALLER'S: asking for `reflow,timing` selects the same two rungs as
/// `timing,reflow` and runs them in the ladder's order.
pub fn only(names: &[String]) -> Result<SweepPlan, String> {
    let asked: Vec<&str> = names
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if asked.is_empty() {
        return Err("--only was given with no section names".to_string());
    }
    let unknown: Vec<&&str> = asked
        .iter()
        .filter(|s| Section::parse(s).is_none())
        .collect();
    if !unknown.is_empty() {
        return Err(format!(
            "unknown --only section(s): {}\nknown sections: {}",
            unknown
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            LADDER
                .iter()
                .map(|s| s.name())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut plan = canonical_plan();
    let wanted: Vec<Section> = asked.iter().filter_map(|s| Section::parse(s)).collect();
    plan.sections.retain(|s| wanted.contains(s));
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical plan really is every rung, and the ladder really is a permutation of the vocabulary —
    /// both asserted in `tests/plan.rs` against `summrise-sweep-plan`; this is the local half, so a
    /// `canonical_plan()` that silently dropped a rung fails without another crate being built.
    #[test]
    fn the_canonical_plan_is_every_rung_with_a_budget_for_each_number() {
        let p = canonical_plan();
        assert_eq!(p.sections, LADDER.to_vec());
        assert_eq!(p.sections.len(), 11);
        assert!(
            p.budgets.values().all(|v| *v > 0),
            "a zero here would be a pass that measures nothing"
        );
    }

    /// `runs` is the reader the driver gates on, and an empty plan runs nothing — which is why `only`
    /// refuses an empty selection instead of returning one.
    #[test]
    fn an_empty_plan_runs_nothing() {
        let mut p = canonical_plan();
        p.sections.clear();
        for s in LADDER {
            assert!(!p.runs(s));
        }
    }
}
