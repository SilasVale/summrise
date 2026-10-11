//! `DECORATIVE_WAIVERS` — ported from `agent/scripts/lib/design-sweep.mjs:27`, and THE ONE PLACE the two
//! instruments that read this list can no longer disagree about it.
//!
//! **DECORATIVE GRAPHICS ARE DRAWN TO DELIMIT, NOT TO INFORM.** WCAG 1.4.11 applies to a non-text element
//! that CARRIES MEANING; a chip's 1px hairline does not. What makes an entry legitimate is that the
//! element's MEANING is measured somewhere else — the grant chip's command text and revoke control, the icon
//! chip's glyph — and each entry's `reason` says so in the words a reader sees.
//!
//! **AND A WAIVER IS FOR THE RATIO IT WAS MEASURED AT, NOT FOR THE ELEMENT.** That is round 95 and it is why
//! every entry carries a BAND: the first version matched on the selector alone, so an entry written for one
//! number set aside every ratio the element could ever produce. `waived_at` is the band applied;
//! `is_waived` is the selector alone and exists for callers that have no ratio to offer (a stale-entry note,
//! a coverage count) — a caller with a measured ratio must use `waived_at` or it has re-opened round 95.
//!
//! **WHY THIS IS A MODULE OF ITS OWN** (and the honest account, because the plan expected something else).
//! The list was ALREADY Rust — `tools/panel.rs` held these two entries with both bands, both slacks and both
//! reasons. What was missing is the other half of "ONE PLACE": `design-sweep.mjs` carries a SECOND COPY,
//! which `agent/scripts/panel-render-audit.mjs` reads, and nothing compared the two. So hoisting the list
//! here is only half of the task; `tests/waivers.rs` is the half that matters — it reads the JavaScript array
//! out of the file a browser is served and requires this list to be the same list, entry for entry, band for
//! band. Move a band in one list and the gate fails naming the entry.
//!
//! **`slack` IS CARRIED AND IS NOT THE BAND TEST.** The band test is `lo <= cr <= hi`; `slack` is the
//! per-entry allowance the band-WIDTH note uses to decide whether the band has drifted wider than its
//! evidence. Both the JavaScript and the judge read it that way, and the cross-language gate compares it so
//! the two cannot drift about the margin either.
//!
//! MUTATION: delete the second entry (`span.nm-ico`) from `decorative()`.
//! RESULT:   `cargo test -p summrise-sweep-judge --test waivers` → exit 101, MEASURED:
//!
//! ```text
//! the_rust_list_is_the_javascript_list ... FAILED
//!   the JavaScript waives 2 entr(ies) and Rust waives 1 —
//!   /^span\.nm-ico$/ is in the list a browser reads and not in the one the judge reads
//! every_entry_is_waived ... FAILED
//! a_waiver_covers_its_band_and_nothing_else ... FAILED
//! test result: FAILED. 2 passed; 3 failed
//! ```
//!
//! THREE tests fail, and each is a different reading of one defect: the equality gate sees the counts
//! diverge, `every_entry_is_waived` sees the JavaScript entry stop being waived here, and the band test sees
//! the chip's own ratio go from waived to a finding. Restoring the entry returns 7 passed.

use regex::Regex;
use std::sync::OnceLock;

/// One decorative waiver: the selector it is about, the band it was measured in, and the reason a reader is
/// shown. `display` is `String(entry.match)` — the note prints the RegExp, slashes and all.
pub struct Decorative {
    pub pattern: Regex,
    /// `/^span\.approval-grant$/` — the display form, compared character for character against the
    /// JavaScript's by `tests/waivers.rs`.
    pub display: &'static str,
    pub values: &'static [(f64, f64)],
    pub slack: Option<f64>,
    pub reason: &'static str,
}

/// The list. Verbatim from `agent/scripts/lib/design-sweep.mjs:27`, reasons included, because the reason is
/// what a reader triages a waiver by and a waiver with a paraphrase is a waiver nobody can check.
pub fn decorative() -> &'static [Decorative] {
    static LIST: OnceLock<Vec<Decorative>> = OnceLock::new();
    LIST.get_or_init(|| {
        vec![
            Decorative {
                pattern: Regex::new(r"^span\.approval-grant$").expect("compiles"),
                display: r"/^span\.approval-grant$/",
                // FOUR SURFACES, FOUR RATIOS — 1.19, 1.20, 1.25, 1.27, measured on the device round 95 —
                // because the outline composites over a different surface on each. The band is that range
                // plus the declared slack, and the slack is the only margin left to argue about.
                values: &[(1.17, 1.29)],
                // PROBE ROUNDING ONLY: the ratios are printed to two decimals, so a true 1.185 reports as
                // 1.19 and a band written at the printed value would refuse it.
                slack: Some(0.02),
                reason: "the grant chip's outline delimits the pill at 1.19; the signal is its command text (worst 5.53 of 4.5) and its revoke control (5.33 of 4.5) — both measured every run",
            },
            Decorative {
                pattern: Regex::new(r"^span\.nm-ico$").expect("compiles"),
                display: r"/^span\.nm-ico$/",
                values: &[(1.03, 1.12)],
                slack: Some(0.02),
                reason: "the icon chip's BACKGROUND delimits a coloured glyph at 1.05/1.10; the glyph itself is measured by the SVG rule since round 94 and clears 3:1 — the lane colour it carries has its own row now",
            },
        ]
    })
}

/// Whether ANY entry is about this selector. **NOT A VERDICT ABOUT A ROW** — a row that matches an entry at a
/// ratio outside its band is a finding, which is what `waived_at` is for. This is for the callers that have
/// only a selector: the stale-entry note ("a waiver nothing uses is weight") and the coverage count.
pub fn is_waived(sel: &str) -> bool {
    decorative().iter().any(|d| d.pattern.is_match(sel))
}

/// The entry that waives THIS selector at THIS ratio, or `None` — which is a finding, not an absence.
///
/// The edge is inclusive on both sides, exactly as the JavaScript's `r.cr >= lo && r.cr <= hi` and as the
/// judge's own clause have always been; a `NaN` ratio matches nothing, because every comparison against `NaN`
/// is false, and treating a `NaN` as `0` would drag the band down to nothing.
pub fn waived_at(sel: &str, cr: f64) -> Option<&'static Decorative> {
    decorative()
        .iter()
        .find(|d| d.pattern.is_match(sel) && d.values.iter().any(|(lo, hi)| cr >= *lo && cr <= *hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The display form is what the notes print, and the cross-language gate compares it — so it is derived
    /// from the pattern here rather than typed twice.
    #[test]
    fn every_display_is_the_patterns_own_source() {
        for d in decorative() {
            assert_eq!(
                d.display,
                format!("/{}/", d.pattern.as_str()),
                "the display form and the compiled pattern disagree"
            );
        }
    }

    /// A vacuity floor of the kind the repository asks for everywhere else: a list that looked at nothing
    /// must not pass. `tests/waivers.rs` carries the cross-language floor; this one is the local half, so a
    /// `decorative()` that returned an empty slice fails without needing the JavaScript file to exist.
    #[test]
    fn the_list_is_not_empty_and_every_entry_has_a_band() {
        assert!(
            !decorative().is_empty(),
            "a policy list with no entries waives nothing and checks nothing"
        );
        for d in decorative() {
            assert!(!d.values.is_empty(), "{} waives every ratio", d.display);
            assert!(
                !d.reason.is_empty(),
                "{} carries no reason a reader can check",
                d.display
            );
        }
    }
}
