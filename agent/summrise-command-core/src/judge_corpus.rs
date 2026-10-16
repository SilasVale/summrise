//! THE DIFFERENTIAL CORPUS FOR `judge_report`, SHARED BY THE INSTRUMENT AND THE PIN.
//!
//! WHY IT IS A LIBRARY MODULE RATHER THAN `#[cfg(test)]`. The corpus has TWO readers and they are compiled
//! differently: `examples/judge-diff.rs` (the differential instrument — a binary, where `cfg(test)` is off)
//! and this module's own `tests` (the golden pin). A `#[cfg(test)]` corpus would compile for one of them and
//! not the other, and a feature gate would make `cargo run --example judge-diff` need a flag — an instrument
//! whose default invocation does not work is an instrument nobody runs. `#[doc(hidden)]` keeps it out of the
//! crate's documented surface; it is the only thing in this crate that is test material rather than API.
//!
//! THE CORPUS IS THE FIXTURES PLUS SYNTHETICS, and the two halves exist for different reasons:
//!
//!   * `tests/fixtures/design-sweep/*.json` are the reports whose `expect` blocks were captured from the
//!     JavaScript judge — the most trustworthy material in the tree, and a PARTIAL corpus: a report only
//!     carries the axes its sweep measured.
//!   * `synthetic()` adds the reports written to reach the clauses no fixture fires, and the quiet, boundary
//!     rows a clause must NOT fire on (a mutation that INVENTS output is as visible here as one that removes
//!     it).
//!
//! Each report is judged three ways: its own options, a `strict` set that turns every floor on and carries
//! exemptions of every kind (used, dormant, undeclared), and `bare` (the defaults). [`render`] is the whole
//! dump, in the order the instrument prints it, so that `cargo run --example judge-diff` and
//! `the_corpus_digest_is_pinned_to_the_golden_file` are comparing the SAME bytes.

use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

use crate::sweep_judge::{judge_report, report_summary, JudgeOptions};

/// The smallest corpus this harness will accept as itself. `dumped >= MIN_CASES` is checked by [`render`]:
/// a corpus that silently shrank is a harness that proves less than it claims, and an empty corpus compares
/// equal forever.
pub const MIN_CASES: usize = 42;

/// The smallest number of FIXTURE reports the corpus will accept — a floor on the other half for the same
/// reason, and the number the tree measured when this corpus was factored out (11).
const MIN_FIXTURES: usize = 11;

/// One report and the options its own sweep would judge it with.
pub struct Case {
    pub label: String,
    pub report: Value,
    pub opts: Value,
}

/// Every case the instrument prints: the fixtures in name order, then the synthetics.
pub fn cases() -> Vec<Case> {
    let mut cases: Vec<Case> = fixtures()
        .into_iter()
        .map(|(name, fixture)| Case {
            label: name,
            report: fixture.get("report").cloned().unwrap_or(Value::Null),
            opts: fixture.get("opts").cloned().unwrap_or_else(|| json!({})),
        })
        .collect();
    cases.extend(synthetic());
    cases
}

/// The whole dump the instrument prints, as one string ending in a newline.
///
/// The three option sets are `own`, `strict` and `bare`, in that order, and the trailing line is the corpus
/// floor rather than a case.
pub fn render() -> String {
    let cases = cases();
    let strict = strict_opts();
    let bare = json!({});

    let mut dumped = 0usize;
    let mut buf = String::new();
    for case in &cases {
        for (tag, opts_json) in [("own", &case.opts), ("strict", &strict), ("bare", &bare)] {
            let opts = JudgeOptions::from_json(Some(opts_json));
            dump(
                &mut buf,
                &format!("{} [{tag}]", case.label),
                &case.report,
                &opts,
            );
            dumped += 1;
        }
    }
    buf.push_str(&format!(
        "### harness: {dumped} case(s) over {} report(s)\n",
        cases.len()
    ));
    assert!(
        dumped >= MIN_CASES,
        "the corpus shrank to {dumped} case(s) — this harness no longer covers what it claims"
    );
    buf
}

/// One report under one option set: the findings, the printed lines, the report notes and the summary.
fn dump(buf: &mut String, label: &str, report: &Value, opts: &JudgeOptions) {
    let outcome = judge_report(report, opts);
    buf.push_str(&format!("=== {label}\n"));
    for f in &outcome.findings {
        buf.push_str(&format!("finding: {}\n", quote(f)));
    }
    for l in &outcome.lines {
        buf.push_str(&format!("line: {}\n", quote(l)));
    }
    for n in &outcome.report_notes {
        buf.push_str(&format!("note: {}\n", quote(n)));
    }
    buf.push_str(&format!(
        "summary: {}\n",
        quote(&report_summary("panel", report))
    ));
}

/// JSON-escaped, so a string carrying a newline cannot make two different dumps look like one.
fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|e| panic!("a string that cannot be quoted: {e}"))
}

fn fixtures() -> Vec<(String, Value)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/design-sweep");
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|n| n.ends_with(".json") && !n.starts_with('_'))
        .collect();
    names.sort();
    assert!(
        names.len() >= MIN_FIXTURES,
        "only {} fixture(s) in {} — the corpus moved, so this harness proves nothing",
        names.len(),
        dir.display()
    );
    names
        .into_iter()
        .map(|n| {
            let text =
                fs::read_to_string(dir.join(&n)).unwrap_or_else(|e| panic!("cannot read {n}: {e}"));
            let v: Value =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{n} is not JSON: {e}"));
            (n, v)
        })
        .collect()
}

/// The option set that turns every floor on, with an exemption of each kind: one that MATCHES (so its
/// suppression line is printed), one that matches nothing and says nothing about why (the "prune it" note),
/// and one that matches nothing and declares itself dormant (the "declared" note).
fn strict_opts() -> Value {
    json!({
        "navless": ["Settings", "Terminal"],
        "proseFloor": 60,
        "unstyledFloor": 40,
        "unmeasurableFloor": 0.05,
        "implicitStates": {
            "kitchen-ghost": "the kitchen sink paints this on purpose",
            "xterm-screen": "xterm.js DOM, styled by the sheet it injects at runtime"
        },
        "ignore": [
            {"contains": ["renders NOTHING when pressed"], "reason": "the pointer never reached this control"},
            {"contains": ["zzz-never-matches"], "reason": "an entry nothing needs"},
            {"contains": ["zzz-dormant"], "reason": "a guard for a state that passes today", "dormant": "no measurement needs it in this corpus"}
        ]
    })
}

/// Reports written to reach what the fixtures do not, each labelled with the clauses it exists for.
fn synthetic() -> Vec<Case> {
    let kitchen_opts = json!({
        "proseFloor": 60,
        "unstyledFloor": 40,
        "unmeasurableFloor": 0.05,
        "implicitStates": {
            "kitchen-ghost": "a class the kitchen sink paints on purpose",
            "never-painted": "a guard for a state nothing paints yet"
        },
        "ignore": [{"contains": ["renders NOTHING when pressed"], "reason": "the kitchen sink's own waiver"}]
    });

    vec![
        // ── THE LOUD REPORT: every clause that FILES something, plus the two report notes inside
        // SURFACES and IDLE REPAINT. Branches that need an option set (the prose floor, the navless
        // prefix, the exemptions) are reached by the same report under `strict`.
        Case {
            label: "synthetic-kitchen".to_string(),
            opts: kitchen_opts,
            report: json!({
                "harnessBuild": "h-old",
                "expectedHarnessBuild": "h-new",
                "harnessStale": true,
                "entryCheck": {"bytes": 11, "sha": "old", "stale": true, "expected": {"bytes": 22, "sha": "new"}},
                "surfaces": [
                    {
                        "page": "Kitchen", "width": 1280,
                        "h1Count": 2, "firstIsH1": false, "skipped": 3, "mains": 0, "navs": 3,
                        "over": ["div.wide 1400>1280"], "clipped": ["span.cut"], "slivers": ["p.sliver"],
                        "measure": {"worst": [
                            {"sel": "p.lede", "cpl": 120, "chars": 720, "lines": 6, "w": 880, "fs": 16, "maxw": "none"}
                        ]},
                        "marks": {"ringFill": ["div.dot"], "collisions": ["div.a ~ div.b"]},
                        "loudUnreadable": 2,
                        "loudUnreadableSamples": ["rgb(0 0 0 / 0.1) on a gradient", "color-mix(in oklab, ...)"],
                        "loud": ["span.big", "div.other"],
                        "claims": ["could not read /api/sessions"]
                    },
                    {
                        "page": "Rejected", "width": 320,
                        "h1Count": 1, "firstIsH1": true, "skipped": 0, "mains": 1, "navs": 1,
                        "measure": {"worst": [
                            {"sel": "p.tight", "cpl": 55, "chars": 220, "lines": 4, "w": 320, "fs": 15, "maxw": "60ch"}
                        ]},
                        "marks": {"ringFill": [], "collisions": []},
                        "loud": ["button.rail-btn", "div.tab-settings"],
                        "claims": ["the status card could not be read"]
                    }
                ],
                "names": [{"page": "Kitchen", "width": 1280, "unnamed": ["button.icon", "a.close"], "titleOnly": ["button.help"]}],
                "focus": [{"page": "Kitchen", "density": "panel", "theme": "dark", "pressed": 8, "landed": 0,
                           "escaped": 8, "missing": 3, "unconfirmed": 2, "unconfirmedOn": ["a.skip", "button.btn"],
                           "paintConfirmed": 4}],
                "press": [
                    {"density": "panel", "theme": "dark", "mode": "rail", "page": "Kitchen", "found": 4, "measured": 1,
                     "rows": [
                        {"sel": "button.btn", "where": "button.btn.btn-primary", "size": "111x24", "changed": false, "reached": true},
                        {"sel": "a.link", "where": "a.link", "size": "40x18", "changed": false, "reached": false,
                         "note": "the pointer never reached this control"},
                        {"sel": "span.row", "where": "span.row", "size": "200x24", "changed": true, "reached": true,
                         "note": "not rendered in this theme"}
                     ]},
                    {"density": "panel", "theme": "light", "page": "Kitchen", "found": 9, "measured": 1,
                     "rows": [{"sel": "x.one", "changed": true, "reached": true, "note": "a plain note about the row"}]},
                    {"density": "console", "theme": "dark", "page": "Kitchen",
                     "rows": [{"sel": "y.two", "changed": true, "reached": true}]}
                ],
                "ack": [
                    {"density": "panel", "page": "Kitchen", "sel": "button.nocounter", "hasCounter": false},
                    {"density": "panel", "page": "Kitchen", "sel": "button.noted", "hasCounter": true,
                     "note": "the pass skipped this control"},
                    {"density": "panel", "page": "Kitchen", "sel": "button.tab", "hasCounter": true, "acked": false, "asked": false},
                    {"density": "panel", "page": "Kitchen", "sel": "button.slow", "where": "button.slow.wide",
                     "size": "80x24", "hasCounter": true, "acked": false, "asked": true},
                    {"density": "panel", "page": "Kitchen", "sel": "button.fast", "hasCounter": true, "acked": true,
                     "asked": true, "via": "data-busy", "msToAck": 412, "msToClear": 430, "budgetMs": 100,
                     "attempts": 2, "presses": 2}
                ],
                "targets": [
                    {"density": "panel", "page": "Kitchen", "mode": "rail", "distinct": [
                        {"sel": "button.mini", "w": 18, "h": 18, "text": "x", "passesBySpacing": false, "nearest": 4},
                        {"sel": "button.nested", "w": 20, "h": 20, "text": "in", "passesBySpacing": true,
                         "passesByFullRule": false, "nearSel": "div.card", "nearW": 300, "nearH": 200, "gapToBox": 6,
                         "insideSel": "div.card", "insideW": 300, "insideH": 200},
                        {"sel": "button.ok", "w": 24, "h": 24, "text": "ok", "passesBySpacing": true, "passesByFullRule": true}
                    ]},
                    {"distinct": [{"sel": "button.nowhere", "w": 10, "h": 10, "text": "", "passesBySpacing": false, "nearest": 1}]}
                ],
                "sse": [
                    {"page": "Kitchen", "opened": true, "fail": false},
                    {"page": "History", "opened": false, "fail": false},
                    {"page": "Rejected", "opened": true, "fail": true}
                ],
                "idle": [
                    {"density": "panel", "theme": "dark", "page": "Kitchen", "seconds": 6, "mutations": 5,
                     "byTarget": {"__probe": 1, "span.approval-left": 3, "div.unrelated": 1}},
                    {"density": "panel", "theme": "light", "page": "Kitchen", "seconds": 6, "mutations": 0, "byTarget": {}}
                ],
                "motion": [
                    {"density": "panel", "stillAnimating": ["div.spinner", "span.pulse", "div.skeleton", "span.extra"],
                     "normal": 19},
                    {"density": "console", "stillAnimating": [], "animating": ["div.other"], "normal": 0},
                    {"density": "console", "animating": ["div.other"], "normal": 4}
                ],
                "unstyled": [
                    {"page": "Kitchen", "classes": ["kitchen-ghost", "inert-extra"], "sheetsUnreadable": 1, "styledClasses": 38},
                    {"page": "Kitchen", "classes": [], "sheetsUnreadable": 0, "styledClasses": 1123}
                ],
                "rows": [
                    {"sel": "span.tiny", "page": "Kitchen", "kind": "text", "size": 9,
                     "text": "a note that is far too long to print in full", "cr": 4.1},
                    {"sel": "span.floor", "page": "Kitchen", "kind": "text", "size": 10, "text": "at the floor", "cr": 5.0},
                    {"sel": "svg.icon", "page": "Kitchen", "kind": "graphic", "size": 8, "text": "", "cr": 3.0},
                    {"sel": "span.unmeasurable", "kind": "text", "size": 12, "text": "no contrast", "cr": null},
                    {"sel": "span.sessions", "page": "Kitchen", "kind": "text", "size": 12,
                     "text": "Sessions unavailable", "cr": 5.1},
                    {"sel": "span.elsewhere", "page": "History", "kind": "text", "size": 12,
                     "text": "SESSIONS UNAVAILABLE", "cr": 4.0},
                    {"sel": "span.rejected", "page": "Rejected", "kind": "text", "size": 12,
                     "text": "sessions unavailable", "cr": 4.2}
                ],
                "hover": [
                    {"density": "panel", "theme": "dark", "rule": "panel-hover", "rows": [
                        {"sel": "span.side-label", "text": "serial:COM4", "cr": 1.96, "need": 4.5,
                         "paint": "rgb(185, 185, 185) (text)", "surface": "rgb(255, 255, 255)", "size": 13,
                         "kind": "text", "inactive": false}
                    ]},
                    {"density": "console", "theme": "dark", "rule": "console-hover-dark", "rows": [
                        {"sel": "a.null", "cr": null, "need": 4.5, "kind": "text"},
                        {"sel": "svg.graphic", "cr": 1.0, "need": 4.5, "kind": "graphic"}
                    ]},
                    {"density": "console", "theme": "light", "rule": "console-hover-light",
                     "underAA": ["a.link 3.9"]},
                    {"density": "panel", "theme": "light", "rule": "panel-hover",
                     "rows": [{"sel": "a.ok", "cr": 7.0, "need": 4.5, "kind": "text"}, "not a row"]}
                ],
                "reflow": [
                    {"width": 320, "docScrollsSideways": true, "docScrollWidth": 340, "viewport": 320,
                     "sideScrollers": ["div.tabrow 340>320"], "overflowing": ["div.tabrow"],
                     "spilling": ["div.card 340>320"], "clipped": ["span.cmd-duration"],
                     "term": {"cols": 38, "containerW": 300, "cellW": 8}},
                    {"width": 1280, "docScrollsSideways": false, "docScrollWidth": 1280, "viewport": 1280,
                     "sideScrollers": [], "overflowing": [], "spilling": [], "clipped": []}
                ]
            }),
        },
        // ── THE EDGES: the branches a loud report cannot reach — a report that declares NOTHING about what
        // its fixture served (so the read-failure clause has no premise), a delivery that matches the build,
        // a navless page NAME under the prefix rule, and the ack clause's "measured N and NONE acknowledged".
        Case {
            label: "synthetic-edges".to_string(),
            opts: json!({}),
            report: json!({
                "harnessBuild": "h-now",
                "entryCheck": {"bytes": 11, "sha": "sha-ok", "stale": false},
                "surfaces": [
                    {"page": "Dashboard", "navs": 0, "h1Count": 1, "firstIsH1": true, "mains": 1,
                     "loud": ["button.rail-btn", "div.tab-x"], "claims": ["the card could not be read"]},
                    {"page": "Terminal-16-sessions", "navs": 0, "h1Count": 1, "firstIsH1": true, "mains": 1,
                     "loud": ["span.only-one"]}
                ],
                "sse": [],
                "ack": [{"density": "panel", "page": "Nowhere", "sel": "button.dead", "hasCounter": true,
                         "acked": false, "asked": true}],
                "press": [{"density": "panel", "theme": "dark", "page": "Nowhere", "found": 1, "measured": 1,
                           "rows": [{"sel": "a.one", "changed": true, "reached": true}]}],
                "targets": [{"density": "panel", "page": "Nowhere", "distinct": []}],
                "idle": [{"density": "panel", "theme": "dark", "page": "Nowhere", "seconds": 6, "mutations": 1,
                          "byTarget": {"__probe": 1}}],
                "motion": [],
                "unstyled": [],
                "rows": []
            }),
        },
        // ── THE QUIET REPORT: boundary rows a clause must NOT fire on — a 10px row (the floor is a floor),
        // a graphic below it, a navs-1 page, a nav_loud element, a fully acknowledged press. A mutation that
        // INVENTS output shows up here as a difference too.
        Case {
            label: "synthetic-quiet".to_string(),
            opts: json!({}),
            report: json!({
                "surfaces": [{"page": "Terminal", "width": 1280, "h1Count": 1, "firstIsH1": true, "skipped": 0,
                              "mains": 1, "navs": 1, "over": [], "clipped": [], "slivers": [],
                              "measure": {"worst": []}, "marks": {"ringFill": [], "collisions": []},
                              "loud": ["button.rail-btn"]}],
                "names": [{"page": "Terminal", "width": 1280, "unnamed": [], "titleOnly": []}],
                "press": [{"density": "panel", "theme": "light", "page": "Terminal", "found": 2, "measured": 2,
                           "rows": [{"sel": "a.one", "changed": true, "reached": true},
                                    {"sel": "a.two", "changed": true, "reached": false}]}],
                "ack": [{"density": "panel", "page": "Terminal", "sel": "button.btn", "hasCounter": true,
                         "acked": true, "asked": true, "via": "data-busy", "msToAck": 40, "msToClear": 90,
                         "budgetMs": 100, "attempts": 1}],
                "targets": [{"density": "panel", "page": "Terminal", "distinct": [
                    {"sel": "button.ok", "w": 24, "h": 24, "passesBySpacing": true, "passesByFullRule": true}
                ]}],
                "sse": [{"page": "Terminal", "opened": true, "fail": false}],
                "idle": [{"density": "panel", "theme": "light", "page": "Terminal", "seconds": 6, "mutations": 1,
                          "byTarget": {"__probe": 1}}],
                "motion": [{"density": "panel", "stillAnimating": [], "animating": [], "normal": 12}],
                "unstyled": [{"page": "Terminal", "classes": [], "sheetsUnreadable": 0, "styledClasses": 1123}],
                "rows": [{"sel": "span.ok", "page": "Terminal", "kind": "text", "size": 12, "text": "fine", "cr": 5.0},
                         {"sel": "span.floor", "page": "Terminal", "kind": "text", "size": 10, "text": "at the floor", "cr": 5.0}],
                "hover": [{"density": "panel", "theme": "light", "rule": "panel-hover", "rows": [
                    {"sel": "a.ok", "cr": 7.0, "need": 4.5, "kind": "text", "inactive": false}
                ]}],
                "reflow": [{"width": 1280, "docScrollsSideways": false, "docScrollWidth": 1280, "viewport": 1280}]
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    //! THE PIN: the corpus above, against a committed golden file.
    //!
    //! WHY A GOLDEN AND NOTHING LESS. Nine of the seventeen clauses of `sweep_judge::judge_report` were, as
    //! measured, noticed only when `tests/fixtures/design-sweep/axes.json` moved — one fixture holding nine
    //! clauses is nine clauses that go dark together the day it is regenerated or pruned. This test notices
    //! every one of them, in both directions: a clause whose body is emptied stops producing its lines, and a
    //! clause that INVENTS output changes the dump too.
    //!
    //! IT IS NOT THE HARNESS'S REPLACEMENT. `examples/judge-diff.rs` exists to BUILD cases — it prints, and
    //! the comparison is by hand, so that the inventory of UNNOTICED clauses stays meaningful while clauses
    //! move. This test is the opposite tool on purpose: it is the pin that must go red on any movement.
    //!
    //! REFRESH: `SUMMRISE_REFRESH_JUDGE_GOLDEN=1 cargo test -p summrise-agent-core judge_golden`, which
    //! rewrites the file and PASSES — the repository's convention for every generated artefact (`grep
    //! SUMMRISE_REFRESH`): the write is the assertion, and the next un-refreshed run is what proves the file
    //! is the corpus's output. The refresh is not silent about the SIZE: [`render`] refuses a shrunken corpus
    //! before a byte is written.
    use super::*;

    /// The committed dump, beside the fixtures it was produced from.
    const GOLDEN: &str = "tests/fixtures/judge-golden.txt";

    /// The `SUMMRISE_REFRESH_*` spelling this test answers to.
    const REFRESH: &str = "SUMMRISE_REFRESH_JUDGE_GOLDEN";

    /// The test's own name, quoted in the refresh hint so the reader can paste the command.
    const TEST: &str = "judge_golden";

    #[test]
    fn judge_golden() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
        let want = render();
        if std::env::var(REFRESH).is_ok_and(|v| !v.is_empty()) {
            fs::write(&path, &want).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
            return;
        }
        let have = fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run {REFRESH}=1 cargo test -p summrise-agent-core {TEST}",
                path.display()
            )
        });
        if have != want {
            panic!("{}", difference(&have, &want, &path));
        }
    }

    /// THE FAILURE MESSAGE IS THE TOOL: the first differing lines from BOTH sides, with their line numbers —
    /// two hashes would cost the next reader the very investigation this test exists to save.
    fn difference(have: &str, want: &str, path: &std::path::Path) -> String {
        let have_lines: Vec<&str> = have.lines().collect();
        let want_lines: Vec<&str> = want.lines().collect();
        let first = have_lines
            .iter()
            .zip(&want_lines)
            .position(|(a, b)| a != b)
            .unwrap_or(have_lines.len().min(want_lines.len()));
        let mut msg = format!(
            "{} is not what the corpus renders: {} line(s) committed vs {} rendered; first difference at line {}.\n",
            path.display(),
            have_lines.len(),
            want_lines.len(),
            first + 1
        );
        // Two lines of lead-in, then the first eight lines of each side, so the reader sees the sentence
        // that moved rather than only the fact that something did.
        for (label, lines) in [("committed", &have_lines), ("rendered ", &want_lines)] {
            msg.push_str(&format!("  ── {label} ──\n"));
            // `take(end).skip(start)` rather than a range read with `lines[i]`: the window is the same, and a
            // slice index in a loop is the shape clippy refuses under `-D warnings`.
            let start = first.saturating_sub(2);
            let end = (first + 8).min(lines.len()).max(start);
            for (i, line) in lines.iter().enumerate().take(end).skip(start) {
                msg.push_str(&format!("    {:>5} | {}\n", i + 1, line));
            }
        }
        if first >= have_lines.len() {
            msg.push_str("  the committed file ENDS here and the corpus does not\n");
        } else if first >= want_lines.len() {
            msg.push_str("  the corpus ENDS here and the committed file does not\n");
        }
        msg.push_str(&format!(
            "FIX: if the movement is intended, run {REFRESH}=1 cargo test -p summrise-agent-core {TEST} and \
             commit the file; if it is not, a clause changed what it says."
        ));
        msg
    }
}
