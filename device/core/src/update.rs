//! The update decision lives in `summrise-release`, not here.
//!
//! This module used to hold `update_decision`, a pure policy over `Manifest` that no runtime path
//! called. The wired decision is `summrise_release::select`, and the two disagreed: a document
//! naming the installed version was "nothing to do" here and a downgrade there, so re-running a
//! canary check exited 4. There is now one decision, and this module exposes none: it remains only
//! as the regression guard below, so a second implementation cannot grow back unnoticed.
//!
//! `Manifest` stays in [`crate::manifest`]: it is the `--stage` document type, not part of the
//! decision.

#[cfg(test)]
mod tests {
    /// The deleted function is gone, and the module exposes nothing that could be mistaken for the
    /// decision. `include_str!` reads this file (the module's own source), so reintroducing the old
    /// name fails the suite rather than silently recreating the second truth C4 removed.
    #[test]
    fn the_deleted_update_decision_is_not_exposed() {
        const SOURCE: &str = include_str!("update.rs");

        assert!(
            !SOURCE.contains(concat!("pub fn update", "_decision")),
            "summrise_release::select is the one update decision; core must not expose a second"
        );
        assert!(
            !SOURCE.contains(concat!("Update", "Decision")),
            "the deleted decision and its types must not come back"
        );
    }
}
