//! THE HOVER CONTRAST RULES, and the sentence each one produces.
//!
//! Three sites in two payloads measured hover contrast, and until 2026-10-09 each of them compared the
//! ratio AND built the sentence the reader sees — in JavaScript. The payloads now hand over the ROW they
//! measured and this module compares and formats, so the sentence has one implementation and one language.
//!
//! ── THE THREE ARE NOT ONE RULE WRITTEN THREE TIMES, AND THAT IS THE POINT OF THE `rule` FIELD ─────────
//!
//! Two of the three look like duplicates and are not, so the ENTRY names which one it measured rather than
//! the judge inferring it from `density`/`theme`/`page` — an inference that would encode an accident of
//! history as a rule:
//!
//!   * [`HoverRule::Panel`] — `panel-run.cjs`: a row is under AA when `cr !== null`, it is not `inactive`,
//!     and `cr` is below `need ?? 4.5`. The sentence quotes the element, its text (16 units), the ratio,
//!     the bar, and what was painted on what.
//!   * [`HoverRule::ConsoleLight`] — `console-run.cjs`'s WIDTHS pass: the SAME comparison as the panel's,
//!     with a sentence that stops after the bar. It does NOT exclude graphics, which the dark pass does.
//!   * [`HoverRule::ConsoleDark`] — `console-run.cjs`'s OVERVIEW-DARK pass: a different comparison. It
//!     skips `kind === 'graphic'`, and it has NO `4.5` default — an absent `need` makes the comparison
//!     false — while `cr: null` coerces to `0` and CAN be reported (`"… null<4.5"`). That last one is a
//!     live quirk of the payload, and a port that "tidied" it would stop reproducing the sweep.
//!
//! The names are the payload's spelling, and [`HoverRule::of`] is the only place that maps them.

use crate::js::{get, js_str, num, slice0, truthy};
use serde_json::Value;

/// The WCAG default both payloads fall back to when a row carries no `need` of its own.
const DEFAULT_NEED: f64 = 4.5;

/// Which of the three hover measurements an entry reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverRule {
    Panel,
    ConsoleLight,
    ConsoleDark,
}

impl HoverRule {
    /// The rule an entry measured, read off the name the payload wrote on it.
    ///
    /// A missing or unrecognised name is the PANEL's rule, deliberately: it is the first and simplest of
    /// the three, the shape a report carried before the payloads named their passes, and a guess is worse
    /// than a documented default. Every payload names its rule today, so this is a floor rather than a
    /// path a real report takes.
    pub fn of(entry: &Value) -> Self {
        match get(entry, "rule").and_then(Value::as_str) {
            Some("console-hover-light") => HoverRule::ConsoleLight,
            Some("console-hover-dark") => HoverRule::ConsoleDark,
            _ => HoverRule::Panel,
        }
    }

    /// The name the payload writes on an entry for this rule.
    pub fn name(self) -> &'static str {
        match self {
            HoverRule::Panel => "panel-hover",
            HoverRule::ConsoleLight => "console-hover-light",
            HoverRule::ConsoleDark => "console-hover-dark",
        }
    }

    /// `r.cr < (r.need ?? 4.5)`, with the panel's two guards (`cr !== null`, not `inactive`).
    ///
    /// The comparison coerces both sides with `Number`, exactly as [`crate::contrast::failures`] does for
    /// the page rows. JavaScript compares LEXICOGRAPHICALLY when both operands are strings (`"2" < "10"`
    /// is false there), which coercion would answer true; the probe emits numbers, so the residual is
    /// unreachable from a sweep report and is named here rather than hidden.
    fn panel_like(r: &Value) -> bool {
        if matches!(get(r, "cr"), Some(Value::Null)) || truthy(get(r, "inactive")) {
            return false;
        }
        match (num(get(r, "cr")), bar(r)) {
            (Some(cr), Some(need)) => cr < need,
            _ => false,
        }
    }

    /// `r.kind !== 'graphic' && !r.inactive && r.cr < r.need` — the console's dark pass, and the only one
    /// of the three that is not `panel_like`. No default: an absent `need` leaves the comparison false.
    fn console_dark(r: &Value) -> bool {
        if js_str(get(r, "kind")) == "graphic" || truthy(get(r, "inactive")) {
            return false;
        }
        match (num(get(r, "cr")), num(get(r, "need"))) {
            (Some(cr), Some(need)) => cr < need,
            _ => false,
        }
    }

    /// Does this row fail this rule?
    fn fails(self, r: &Value) -> bool {
        match self {
            HoverRule::Panel | HoverRule::ConsoleLight => Self::panel_like(r),
            HoverRule::ConsoleDark => Self::console_dark(r),
        }
    }

    /// The sentence the JavaScript built, byte for byte — including `String()`'s rendering of an absent
    /// field as `undefined` and of an explicit null as `null`, and the 16-UNIT truncation of the text.
    fn sentence(self, r: &Value) -> String {
        let sel = js_str(get(r, "sel"));
        let cr = js_str(get(r, "cr"));
        match self {
            HoverRule::Panel => format!(
                "{sel} \"{}\" {cr}<{} painted {} on {}, {}px {}",
                slice0(&js_str(get(r, "text")), 16),
                need_display(r),
                js_str(get(r, "paint")),
                js_str(get(r, "surface")),
                js_str(get(r, "size")),
                js_str(get(r, "kind")),
            ),
            HoverRule::ConsoleLight => format!(
                "{sel} \"{}\" {cr}<{}",
                slice0(&js_str(get(r, "text")), 16),
                need_display(r),
            ),
            HoverRule::ConsoleDark => format!("{sel} {cr}<{}", js_str(get(r, "need")),),
        }
    }
}

/// `r.need ?? 4.5`, rendered the way the sentence interpolates it — the DEFAULT includes the bar the
/// comparison used, so a row with no `need` reports `<4.5` rather than `<undefined`.
fn need_display(r: &Value) -> String {
    match get(r, "need") {
        None | Some(Value::Null) => js_str(Some(&Value::from(DEFAULT_NEED))),
        other => js_str(other),
    }
}

/// `r.need ?? 4.5` as a number, or `None` when the row carries something that is not one.
fn bar(r: &Value) -> Option<f64> {
    match get(r, "need") {
        None | Some(Value::Null) => Some(DEFAULT_NEED),
        other => num(other),
    }
}

/// WHY AN ENTRY CANNOT BE JUDGED, or `None` when it can.
///
/// TWO SHAPES ARE REFUSED, AND BOTH WOULD OTHERWISE BE SILENCE — a hover pass that measured eleven failing
/// elements reading exactly like one that measured none, which is the failure this repository keeps finding
/// in its own checks:
///
///   * `underAA`, THE KEY THE PAYLOADS WROTE BEFORE 2026-10-09. It held finished SENTENCES, so the rows are
///     simply absent and every rule below finds nothing to compare. The key is refused BY NAME rather than
///     by its contents, because a stale report must not read as a clean one.
///   * an element of `rows` that is not a row — a string, a number, a null.
pub fn unreadable(entry: &Value) -> Option<String> {
    if entry.get("underAA").is_some() {
        return Some(
            "`underAA` holds the SENTENCES the payloads built until 2026-10-09 — this report predates the \
             shape change and its rows are not here"
                .to_string(),
        );
    }
    let rows = crate::js::arr(get(entry, "rows"));
    let bad = rows.iter().filter(|r| !r.is_object()).count();
    if bad > 0 {
        return Some(format!(
            "{bad} of {} entry(ies) in `rows` is not a row",
            rows.len()
        ));
    }
    None
}

/// The rows an entry reports, under the entry's own rule, as the sentences a reader sees.
///
/// THE DEDUPLICATION IS HERE BECAUSE IT WAS A `Set` OVER THE SENTENCES. The payloads pushed
/// `[...new Set(underAA)]` of FORMATTED STRINGS, so two rows that print the same sentence printed it
/// once — and a `Set` over the raw rows would not have been the same set at all, because rows differ in
/// fields the sentence does not quote. Order is the probe's, first occurrence wins.
pub fn sentences(entry: &Value) -> Vec<String> {
    let rule = HoverRule::of(entry);
    let mut seen: Vec<String> = Vec::new();
    for r in crate::js::arr(get(entry, "rows")) {
        if !rule.fails(r) {
            continue;
        }
        let line = rule.sentence(r);
        if !seen.contains(&line) {
            seen.push(line);
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The shape a payload emits: the raw row the probe measured, under the pass's own rule name.
    fn entry(rule: &str, rows: Value) -> Value {
        json!({ "rule": rule, "rows": rows })
    }

    #[test]
    fn the_rule_is_read_off_the_entry_and_defaults_to_the_panels() {
        assert_eq!(
            HoverRule::of(&entry("panel-hover", json!([]))),
            HoverRule::Panel
        );
        assert_eq!(
            HoverRule::of(&entry("console-hover-light", json!([]))),
            HoverRule::ConsoleLight
        );
        assert_eq!(
            HoverRule::of(&entry("console-hover-dark", json!([]))),
            HoverRule::ConsoleDark
        );
        // An entry that names nothing — the shape every report carried before the payloads named their
        // passes — is the panel's rule rather than a guess at one of the other two.
        assert_eq!(HoverRule::of(&json!({ "rows": [] })), HoverRule::Panel);
        assert_eq!(
            HoverRule::of(&json!({ "rule": "nonsense", "rows": [] })),
            HoverRule::Panel
        );
        // And each rule knows the name it is written under, so the two ends cannot drift.
        for r in [
            HoverRule::Panel,
            HoverRule::ConsoleLight,
            HoverRule::ConsoleDark,
        ] {
            assert_eq!(HoverRule::of(&entry(r.name(), json!([]))), r);
        }
    }

    #[test]
    fn the_panels_sentence_is_the_one_the_payload_built() {
        // A REAL ROW from the 2026-10-09 oracle run — a planted `#b9b9b9 on #ffffff` defect measured on the
        // rendered panel harness — so the expected text is what the JavaScript actually printed, not a
        // sentence written to match this function.
        let rows = json!([{
            "sel": "span.side-label",
            "text": "serial:COM4",
            "cr": 1.96,
            "need": 4.5,
            "paint": "rgb(185, 185, 185) (text)",
            "surface": "rgb(255, 255, 255)",
            "size": 13,
            "kind": "text",
            "inactive": false
        }]);
        assert_eq!(
            sentences(&entry("panel-hover", rows)),
            vec![r#"span.side-label "serial:COM4" 1.96<4.5 painted rgb(185, 185, 185) (text) on rgb(255, 255, 255), 13px text"#.to_string()]
        );
    }

    #[test]
    fn the_panel_truncates_the_text_to_sixteen_units_and_defaults_the_bar() {
        // `"provision the ONU on VLAN 100"` is the text a real run carried; the JavaScript's `slice(0, 16)`
        // printed `provision the ON`, and `need: null` printed as the DEFAULT rather than as `null`.
        let rows = json!([{
            "sel": "span.goal-text", "text": "provision the ONU on VLAN 100",
            "cr": 1.96, "need": null, "paint": "rgb(185, 185, 185) (text)",
            "surface": "rgb(255, 255, 255)", "size": 11, "kind": "text"
        }]);
        assert_eq!(
            sentences(&entry("panel-hover", rows)),
            vec![r#"span.goal-text "provision the ON" 1.96<4.5 painted rgb(185, 185, 185) (text) on rgb(255, 255, 255), 11px text"#.to_string()]
        );
    }

    #[test]
    fn the_three_rules_disagree_about_the_same_rows() {
        // ONE ROW SET, THREE RULES — which is why the entry has to say which one it measured. The graphic is
        // reported by the panel and by the console's LIGHT pass and skipped by its DARK one; the row with no
        // `need` is reported by the panel (4.5 by default) and by neither console pass, because the dark pass
        // has no default and `4.2 < null` is `4.2 < 0`. Only the third row is under AA for all three.
        let rows = json!([
            { "sel": "span.badge", "text": "2", "cr": 2.33, "need": 4.5, "paint": "rgb(1,1,1)", "surface": "rgb(255,255,255)", "size": 12, "kind": "graphic" },
            { "sel": "div.dot", "text": "", "cr": 4.2, "need": null, "paint": "rgb(1,1,1)", "surface": "rgb(255,255,255)", "size": 12, "kind": "text" },
            { "sel": "a.link", "text": "docs", "cr": 3.0, "need": 4.5, "paint": "rgb(1,1,1)", "surface": "rgb(255,255,255)", "size": 12, "kind": "text" }
        ]);
        assert_eq!(sentences(&entry("panel-hover", rows.clone())).len(), 3);
        assert_eq!(
            sentences(&entry("console-hover-light", rows.clone())).len(),
            3
        );
        assert_eq!(
            sentences(&entry("console-hover-dark", rows)),
            vec!["a.link 3<4.5".to_string()],
            "the dark pass skips graphics, and a row with no `need` fails its comparison rather than defaulting"
        );
    }

    #[test]
    fn the_console_sentences_stop_where_the_payloads_stopped() {
        let text = json!([{ "sel": "button.btn-new", "text": "New", "cr": 1.96, "need": 4.5, "kind": "text" }]);
        assert_eq!(
            sentences(&entry("console-hover-light", text.clone())),
            vec![r#"button.btn-new "New" 1.96<4.5"#.to_string()]
        );
        assert_eq!(
            sentences(&entry("console-hover-dark", text)),
            vec!["button.btn-new 1.96<4.5".to_string()]
        );
    }

    #[test]
    fn an_unmeasurable_row_is_reported_by_the_dark_pass_and_not_by_the_others() {
        // `null < 4.5` is TRUE in JavaScript — `null` coerces to `0` — while an ABSENT `cr` is NaN and false,
        // and the panel-like rules exclude an explicit null before comparing. All three of those behaviours
        // are live in the payloads, so all three are pinned.
        let rows = json!([
            { "sel": "a.null", "cr": null, "need": 4.5, "kind": "text" },
            { "sel": "a.absent", "need": 4.5, "kind": "text" }
        ]);
        assert!(sentences(&entry("panel-hover", rows.clone())).is_empty());
        assert!(sentences(&entry("console-hover-light", rows.clone())).is_empty());
        assert_eq!(
            sentences(&entry("console-hover-dark", rows)),
            vec!["a.null null<4.5".to_string()]
        );
    }

    #[test]
    fn two_rows_that_print_the_same_sentence_print_it_once() {
        // The payloads pushed `[...new Set(sentences)]`. For the dark pass the sentence quotes only the
        // selector, the ratio and the bar, so two rows differing in `kind` collapse — and the set was over
        // the SENTENCES, not the rows, so this is the behaviour and not a convenience.
        let rows = json!([
            { "sel": "div.dot", "cr": 2.0, "need": 4.5, "kind": "text" },
            { "sel": "div.dot", "cr": 2.0, "need": 4.5, "kind": "shadow" },
            { "sel": "div.dot", "cr": 3.0, "need": 4.5, "kind": "text" }
        ]);
        assert_eq!(
            sentences(&entry("console-hover-dark", rows)),
            vec!["div.dot 2<4.5".to_string(), "div.dot 3<4.5".to_string()],
            "first occurrence wins, and order is the probe's"
        );
    }

    #[test]
    fn the_old_shape_is_refused_rather_than_read_as_clean() {
        // TWO SHAPES CANNOT BE JUDGED, and both used to be SILENCE: `underAA` is the key the payloads wrote
        // until 2026-10-09, holding finished sentences, and a `rows` element that is not a row gives every
        // rule below nothing to compare. A pass that measured eleven failing elements must not read like one
        // that measured none, so both are named.
        let stale = unreadable(&json!({"rule": "panel-hover", "underAA": ["span.badge 2.33"]}))
            .expect("the pre-2026-10-09 key is refused even when it is empty");
        assert!(stale.contains("underAA"), "{stale}");
        let bad = unreadable(&entry("panel-hover", json!([{"sel": "a"}, "a.link 3.9"])))
            .expect("a non-row is refused");
        assert!(bad.contains("1 of 2"), "{bad}");
        // AND A JUDGEABLE ENTRY IS NOT REFUSED — the direction that keeps the clause from failing
        // everything, which is what makes the two above evidence.
        assert_eq!(
            unreadable(&entry("panel-hover", json!([{"sel": "a"}]))),
            None
        );
        assert_eq!(unreadable(&entry("panel-hover", json!([]))), None);
        assert_eq!(unreadable(&json!({})), None);
    }

    #[test]
    fn a_row_that_passes_its_rule_is_not_a_finding() {
        let rows = json!([
            { "sel": "p.ok", "text": "fine", "cr": 7.0, "need": 4.5, "kind": "text" },
            { "sel": "p.inactive", "text": "dim", "cr": 1.1, "need": 4.5, "kind": "text", "inactive": true }
        ]);
        for rule in ["panel-hover", "console-hover-light", "console-hover-dark"] {
            assert!(
                sentences(&entry(rule, rows.clone())).is_empty(),
                "{rule} reported a row above its bar, or one the page marked inactive"
            );
            // AND THE SAME RULE MUST STILL REPORT A ROW THAT FAILS IT. Without this the loop above is
            // satisfied by an implementation that reports nothing at all — the vacuity this file's own
            // subject keeps finding, so it is closed here rather than commented on.
            let failing =
                json!([{ "sel": "p.bad", "text": "dim", "cr": 1.1, "need": 4.5, "kind": "text" }]);
            assert_eq!(
                sentences(&entry(rule, failing)).len(),
                1,
                "{rule} stopped reporting a row that is genuinely under its bar"
            );
        }
    }

    #[test]
    fn an_absent_field_prints_as_javascript_prints_it() {
        // The sentence interpolates with `+`, so an absent field is the string `undefined` rather than a
        // panic or an empty gap — the panel's sentence is the one that quotes enough fields to show it.
        let rows = json!([{ "cr": 1.0, "need": 4.5 }]);
        assert_eq!(
            sentences(&entry("panel-hover", rows)),
            vec![r#"undefined "undefined" 1<4.5 painted undefined on undefined, undefinedpx undefined"#.to_string()]
        );
    }
}
