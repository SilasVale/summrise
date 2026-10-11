//! THE STEP LADDER — the driver's own output, and the thing the plan's `SweepPlan` is FOR.
//!
//! **THE SPLIT THIS FILE IS THE SEAM OF, STATED ONCE MORE BECAUSE IT IS THE WHOLE DESIGN.** The repository
//! has no Rust browser driver and the parent spec MEASURED that fact rather than assuming it
//! (`docs/superpowers/specs/2026-10-08-every-decision-is-rust-design.md` §4.1): every line of
//! `agent/src/plugins/playwright/` drives the browser by shelling out to node, and there is no CDP crate in
//! the lockfile. So "the Playwright driving becomes Rust" cannot be a port — it would be a new browser
//! dependency — and the rule that decides it is the one already in force: *launching a browser, dispatching
//! a real mouse press and reading `getComputedStyle` are platform calls; deciding that a control whose
//! computed style never changes while it is pressed is a DEAD control is a decision.*
//!
//! What lands on the Rust side of that line is this: the LADDER (which rung runs, in what order — the
//! judge's `LADDER`) and the BUDGETS each rung spends, turned into a **step list** — data, serde, not
//! generated code — that a fixed JavaScript runtime interprets. `steps()` is the translation, and it is
//! where a budget becomes an argument: a rung with no budget for `idle_window_ms` is a rung whose window is
//! the runtime's default, and a rung with a budget of `0` is a rung that measures NOTHING (Review Focus 4 —
//! `SweepPlan::cap` is what makes those two different).
//!
//! **THE ARGUMENTS ARE NAMED AFTER THE JAVASCRIPT'S OWN PARAMETERS**, so a reader can put a step beside the
//! pass that consumes it without a translation table: `idlePass(page, ms)`, `focusPass(page, presses)`,
//! `discoverPressTargets(page, cap, skip)`, `ackPass(page, targets, budgetMs, label)` and the settles inside
//! `pressPass`/`revealPass`/`motionPass`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use summrise_sweep_judge::plan::{Section, SweepPlan};

/// One observation the driver asks for: which rung, on which surface, with which arguments.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// `<surface>.<section>` — deterministic, so two runs of the same plan produce the same document and a
    /// diff between two rows documents points at one rung of one surface.
    pub id: String,
    pub section: &'static str,
    pub surface: String,
    pub args: Value,
}

/// The steps for one run: every surface, in the order given, each one walked down the ladder in the plan's
/// order — which is the payload's own order and the reason the ladder is data.
///
/// **SURFACES OUTSIDE, RUNGS INSIDE.** The two loops are not interchangeable: the payloads navigate to a
/// surface once and run every rung on it before moving on (a second navigation would measure a different
/// page), so the outer loop is the surface and the inner one is the ladder.
pub fn steps(plan: &SweepPlan, surfaces: &[String]) -> Result<Vec<Step>, String> {
    let mut out = Vec::new();
    for surface in surfaces {
        for section in &plan.sections {
            out.push(Step {
                id: format!("{surface}.{}", section.name()),
                section: section.name(),
                surface: surface.clone(),
                args: args_for(plan, *section)?,
            });
        }
    }
    Ok(out)
}

/// **A RUNG WHOSE BUDGET THE PLAN DOES NOT SET IS AN ERROR, AND THIS WAS A MEASURED BUG RATHER THAN A
/// PRINCIPLE.** The first version of `args_for` wrote `plan.cap(name, 0)` — a caller's number of zero — and
/// `SweepPlan::cap` is `min(budget, n)`, so EVERY budget came back as `0`: a driver that would have pressed
/// nothing, idled for no time and polled no times, with a well-formed plan and a green suite. It was found by
/// RUNNING `--emit` and reading the step list, not by a test, and it is the reason this function is
/// fallible: a budget that is not set is a question the plan has not answered, and `cap`'s "the caller's
/// number stands" rule is for a caller that HAS a number (the payload side) rather than for a driver with
/// nothing to offer.
fn require(plan: &SweepPlan, section: Section, name: &str) -> Result<u64, String> {
    plan.budget(name).ok_or_else(|| {
        format!(
            "the plan sets no `{name}` budget, so the `{}` rung has nothing to spend. A driver that defaulted \
             it would measure a different amount than the plan says — and a default of zero would measure \
             nothing at all while every field of the step looked well-formed.",
            section.name()
        )
    })
}

/// What one rung needs, as the numbers the pass takes — every one of them READ FROM THE PLAN rather than
/// defaulted here.
fn args_for(plan: &SweepPlan, section: Section) -> Result<Value, String> {
    let b = |name: &str| require(plan, section, name);
    Ok(match section {
        // `focusPass(page, presses, label)`
        Section::Focus => json!({ "presses": b("focus_presses_panel")? }),
        // `idlePass(page, ms)` plus the probe's own sample cap.
        Section::Idle => json!({
            "window_ms": b("idle_window_ms")?,
            "samples": b("idle_samples")?,
        }),
        // `pressPass`'s three waits and the discovery cap it hands to `discoverPressTargets`.
        Section::Press => json!({
            "settle_ms": b("press_settle_ms")?,
            "hold_ms": b("press_hold_ms")?,
            "release_ms": b("press_release_ms")?,
            "discover_cap": b("discover_cap")?,
        }),
        // `ackPass`'s baseline settle, both poll loops, the retry between them, and the window the page is
        // asked about. The BUDGET the judge compares against is not here — it is the plan's `ack_budget_ms`
        // (`summrise-sweep-plan`'s `caps`), and it is a verdict input rather than a measurement cost.
        Section::Ack => json!({
            "settle_ms": b("ack_settle_ms")?,
            "polls": b("ack_polls")?,
            "poll_ms": b("ack_poll_ms")?,
            "retry_settle_ms": b("ack_retry_settle_ms")?,
            "retry_return_ms": b("ack_retry_return_ms")?,
            "clear_polls": b("ack_clear_polls")?,
            "clear_ms": b("ack_clear_ms")?,
            "window_before_ms": b("ack_window_before_ms")?,
            "window_after_ms": b("ack_window_after_ms")?,
        }),
        // `revealPass`'s hover settle and the restore that puts the page back.
        Section::Targets => json!({
            "settle_ms": b("reveal_settle_ms")?,
            "restore_ms": b("reveal_restore_ms")?,
        }),
        // `motionPass` renders once per preference, and the count is the axis: one render would measure the
        // page rather than the preference.
        Section::Motion => json!({ "renders": b("motion_renders")? }),
        // The rungs whose cost is the page's rather than a policy: the probes are evaluated and the rows are
        // what they are. An empty object rather than an absent one, so a reader sees "no arguments" instead
        // of wondering whether the field was lost.
        Section::Timing | Section::Pages | Section::Unstyled | Section::Hover | Section::Reflow => {
            Value::Object(Map::new())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use summrise_sweep_judge::canonical_plan;

    /// One step per (surface, rung), in that nesting, and the ids are what a rows document is diffed by.
    #[test]
    fn the_steps_are_every_rung_of_every_surface_in_the_ladders_order() {
        let plan = canonical_plan();
        let s = steps(
            &plan,
            &[
                "panel-light-Terminal".to_string(),
                "desktop-dark-History".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(s.len(), 2 * plan.sections.len());
        assert_eq!(s[0].id, "panel-light-Terminal.timing");
        assert_eq!(s[plan.sections.len()].id, "desktop-dark-History.timing");
        let second: Vec<&str> = s[..plan.sections.len()].iter().map(|x| x.section).collect();
        let ladder: Vec<&str> = plan.sections.iter().map(|x| x.name()).collect();
        assert_eq!(second, ladder);
    }

    /// **A BUDGET THE PLAN DOES NOT SET IS REFUSED, AND THIS TEST EXISTS BECAUSE ITS ABSENCE WAS A REAL
    /// BUG.** The first version resolved every argument with `SweepPlan::cap(name, 0)`, whose `min(budget, 0)`
    /// is zero — so a well-formed plan produced a step list that pressed nothing and idled for no time, and
    /// the suite was green. A missing budget must be a question the plan has not answered.
    #[test]
    fn a_rung_whose_budget_is_missing_is_refused_rather_than_defaulted() {
        let mut plan = canonical_plan();
        plan.budgets.remove("ack_polls");
        let e =
            steps(&plan, &["panel-light-Terminal".to_string()]).expect_err("no budget, no step");
        assert!(e.contains("ack_polls"), "{e}");
        assert!(
            e.contains("ack"),
            "the refusal should name the rung too: {e}"
        );
    }

    /// Every budget the ladder names is SET by the canonical plan — the other direction of the same rule,
    /// and the one that would have caught the bug above at the source.
    #[test]
    fn the_canonical_plan_answers_every_question_the_steps_ask() {
        let plan = canonical_plan();
        let s = steps(&plan, &["panel-light-Terminal".to_string()]).expect("every budget is set");
        for step in &s {
            for (k, v) in step.args.as_object().expect("args is an object") {
                assert_ne!(
                    v.as_u64(),
                    Some(0),
                    "{} came out as a zero budget, which is a rung that measures nothing: {step:?}",
                    k
                );
            }
        }
        assert_eq!(s.len(), plan.sections.len());
    }

    /// A rung the plan does not carry produces no step — the driver runs what it was given.
    #[test]
    fn a_rung_that_is_not_in_the_plan_produces_no_step() {
        let mut plan = canonical_plan();
        plan.sections.retain(|s| *s != Section::Reflow);
        let s = steps(&plan, &["panel-light-Terminal".to_string()]).unwrap();
        assert!(!s.iter().any(|x| x.section == "reflow"));
        assert_eq!(s.len(), 10);
    }
}
