//! `markCoverageNotes(cssText, report)` — ported from `agent/scripts/lib/design-sweep.mjs:2392-2455`.
//!
//! THE STATES A SHEET DECLARES AND THIS RUN NEVER PAINTED (round 32, made shared in round 50). The panel
//! learned this the expensive way: `cmd-dot` rendered four of its six states for rounds, and the two that
//! had no surface hid a missing rule and a colour-only pair. The note is the queue those rounds worked
//! from — and until round 50 it lived INSIDE `panel-design-sweep.mjs`, so the console and the landing had
//! no such queue at all. One implementation, three callers, because a second copy is how the two drift.
//!
//! `css_text` is the sheet whose state rules define the families (the panel's BUILT sheet, the console's
//! SOURCE sheets, the landing's stylesheet); `report` is the sweep report whose surfaces carry
//! `marks.families`/`present`.

use crate::js::{arr, at, get, js_str, regex_escape};
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

fn comment_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"/\*[\s\S]*?\*/").expect("the comment pattern compiles"))
}

/** `/^([^[]+)\[([^\]]*)\]$/` — a rendered family string, `name[state,state]`. */
fn family_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^([^\[]+)\[([^\]]*)\]$").expect("the family pattern compiles"))
}

/** The probe's own family rule (a name ending in dot / dotcol / mark / led / chip / signal / state), so the
 *  two instruments cannot disagree about what a family is. */
fn family_rule() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(dot|dotcol|mark|led|chip|signal|state)$").expect("compiles"))
}

/** `/\.([A-Za-z][\w-]*)(\[[^\]]+\]|\.[A-Za-z][\w-]*)/g`. `\w` is spelled out because JavaScript's is
 *  ASCII-only without the `u` flag, while the regex crate's is Unicode-aware. */
fn class_rule() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"\.([A-Za-z][A-Za-z0-9_-]*)(\[[^\]]+\]|\.[A-Za-z][A-Za-z0-9_-]*)")
            .expect("compiles")
    })
}

/** A CLASS NAMED `*-state` IS NOT AUTOMATICALLY A MARK (round 32). The naming rule is deliberately loose,
 *  and it swept in three classes the sheet styles as a TEXT LINE. Their is-error/is-ok/is-granted variants
 *  are INK on words, so the silhouette question does not apply to them and the colour-only distinction is
 *  fine: the text itself says which state it is. Declared here with a reason rather than filtered by a
 *  heuristic, because a heuristic would also hide the day one of them becomes a real mark. */
const TEXT_STATE_CLASSES: [(&str, &str); 3] = [
    (
        "update-state",
        "a mono paragraph on the update card — its is-error/is-ok are ink on words",
    ),
    (
        "notify-state",
        "the notifications card's line — is-granted/is-denied are ink on words",
    ),
    (
        "monitor-state",
        "the word beside the reachability mark — the mark next to it carries the shape",
    ),
];

/// The `note:` lines this sheet and this report produce, in order.
pub fn mark_coverage_notes(css_text: &str, report: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let css = comment_re().replace_all(css_text, "").to_string();

    // A Map<family, Set<state>> in INSERTION order: the report's families first, then the sheet's.
    let mut seen_by_family: Vec<(String, Vec<String>)> = Vec::new();

    for s in arr(get(report, "surfaces")) {
        let families = arr(at(get(s, "marks"), "families"));
        for fam in families {
            let fam = js_str(Some(fam));
            let Some(caps) = family_re().captures(&fam) else {
                continue;
            };
            let name = caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
            let states = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let entry = entry_of(&mut seen_by_family, &name);
            for st in states.split(',') {
                if !st.is_empty() && !entry.iter().any(|x| x == st) {
                    entry.push(st.to_string());
                }
            }
        }
    }

    {
        // THE CLASSES THE RUN PUT ON SCREEN, so a family with no painted states can be told apart from a
        // family the probe attributes elsewhere: `mark tab-dot` is measured as `mark`, and reporting it as
        // "no surface rendered tab-dot" would be a finding about the probe's naming, not about the page.
        let mut on_screen: Vec<String> = Vec::new();
        for s2 in arr(get(report, "surfaces")) {
            for c in arr(at(get(s2, "marks"), "present")) {
                let name = js_str(Some(c));
                if !on_screen.contains(&name) {
                    on_screen.push(name);
                }
            }
        }

        // Every class the sheet gives a STATE rule to, by the probe's own family rule.
        for caps in class_rule().captures_iter(&css) {
            let name = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            if !family_rule().is_match(name) {
                continue;
            }
            if TEXT_STATE_CLASSES.iter().any(|(c, _)| *c == name) {
                continue;
            }
            entry_of(&mut seen_by_family, name);
        }

        let data_prefix = Regex::new(r"^data-(state|live|kind)=").expect("compiles");
        for (family, rendered) in &seen_by_family {
            // A CLASS NAMED `*-state` IS NOT AUTOMATICALLY A MARK — the declaration above.
            let declared = declared_states(&css, family);
            if rendered.is_empty() && on_screen.iter().any(|c| c == family) {
                out.push(format!(
                    "note: mark family {family} declares {} state(s) and the CLASS IS ON SCREEN — the probe did not record it as a family of its own, either because its marks carry a state attribute (the family is then the FIRST class, which is how mark tab-dot is measured as mark) or because they are larger than the 40px the mark probe measures. Not a missing surface; a naming and sizing question.",
                    declared.len()
                ));
                continue;
            }
            let missing: Vec<&String> = declared
                .iter()
                .filter(|d| {
                    !rendered.iter().any(|r| r == *d)
                        && !rendered
                            .iter()
                            .any(|r| data_prefix.replace(d.as_str(), "") == r.as_str())
                })
                .collect();
            if !missing.is_empty() {
                out.push(format!(
                    "note: mark family {family} declares {} state(s) and this run rendered {} ({}) over {} surface(s) — NO SURFACE RENDERED {}, so those silhouettes and their collisions are unverified",
                    declared.len(),
                    rendered.len(),
                    if rendered.is_empty() {
                        "none".to_string()
                    } else {
                        let mut sorted = rendered.clone();
                        sorted.sort();
                        sorted.join(", ")
                    },
                    arr(get(report, "surfaces")).len(),
                    missing
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }
    out
}

fn entry_of<'a>(map: &'a mut Vec<(String, Vec<String>)>, name: &str) -> &'a mut Vec<String> {
    if let Some(i) = map.iter().position(|(k, _)| k == name) {
        return &mut map[i].1;
    }
    map.push((name.to_string(), Vec::new()));
    let last = map.len() - 1;
    &mut map[last].1
}

/// The states the sheet declares for a family, by the same selector shape the probe uses: an attribute
/// (`[data-state=…]` → the value without quotes) or a modifier class (`.is-error` → `is-error`).
fn declared_states(css: &str, family: &str) -> Vec<String> {
    let pattern = format!(
        r"\.{}(\[[^\]]+\]|\.[A-Za-z][A-Za-z0-9_-]*)",
        regex_escape(family)
    );
    let Ok(re) = Regex::new(&pattern) else {
        return Vec::new();
    };
    let mut declared: Vec<String> = Vec::new();
    for caps in re.captures_iter(css) {
        let Some(sel) = caps.get(1) else { continue };
        let sel = sel.as_str();
        let value = if let Some(inner) = sel.strip_prefix('[') {
            inner.strip_suffix(']').unwrap_or(inner).replace('"', "")
        } else {
            sel[1..].to_string()
        };
        if !declared.contains(&value) {
            declared.push(value);
        }
    }
    declared
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_family_the_sheet_declares_and_the_run_never_painted_is_named() {
        let css =
            ".cmd-dot[data-state=ok] { color: red } .cmd-dot[data-state=running] { color: blue }";
        let report = json!({"surfaces": [{"marks": {"families": ["cmd-dot[data-state=ok]"]}}]});
        let notes = mark_coverage_notes(css, &report);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("mark family cmd-dot declares 2 state(s)"));
        assert!(notes[0].contains("NO SURFACE RENDERED data-state=running"));
        assert!(notes[0].contains("over 1 surface(s)"));
    }

    #[test]
    fn a_family_that_rendered_every_declared_state_says_nothing() {
        let css = ".cmd-dot[data-state=ok] { color: red }";
        let report = json!({"surfaces": [{"marks": {"families": ["cmd-dot[data-state=ok]"]}}]});
        assert!(mark_coverage_notes(css, &report).is_empty());
    }

    #[test]
    fn an_on_screen_class_measured_under_another_family_is_a_naming_note() {
        let css = ".tab-dot[data-kind=serial] { color: red }";
        let report = json!({"surfaces": [{"marks": {"families": ["mark[data-kind=serial]"], "present": ["tab-dot"]}}]});
        let notes = mark_coverage_notes(css, &report);
        assert!(notes
            .iter()
            .any(|n| n
                .contains("mark family tab-dot declares 1 state(s) and the CLASS IS ON SCREEN")));
        assert!(!notes.iter().any(|n| n.contains("NO SURFACE RENDERED")));
    }

    #[test]
    fn the_three_text_state_classes_are_not_mark_families() {
        let css = ".update-state.is-error { color: red } .notify-state.is-granted { color: green }";
        let report = json!({"surfaces": []});
        assert!(mark_coverage_notes(css, &report).is_empty());
    }

    #[test]
    fn comments_are_stripped_before_the_sheet_is_read() {
        let css = "/* .comment-dot[data-state=ghost] {} */ .ok-dot[data-state=on] { color: red }";
        let report = json!({"surfaces": [{"marks": {"families": ["ok-dot[data-state=on]"]}}]});
        assert!(mark_coverage_notes(css, &report).is_empty());
    }
}
