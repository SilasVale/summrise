//! A DIFFERENTIAL HARNESS FOR `judge_report` — the instrument that makes a refactor of the judge provable
//! instead of hopeful.
//!
//! WHY IT EXISTS. `judge_report` decides what a sweep report means: which findings a run files, IN WHICH
//! ORDER, the sentences it prints and the notes it hands back. A refactor of it must not move one byte of
//! that output, and the crate's own suite does not pin every clause (the mutation table recorded beside the
//! change that split the function says exactly which clauses nothing notices), so "the tests pass" is not
//! the safety argument. The argument is: run this harness over a corpus x option sets BEFORE the change,
//! run it AFTER, and compare md5 — and prove the harness REACHES every clause by emptying each clause's
//! body in turn and showing this output changes.
//!
//! WHY AN EXAMPLE RATHER THAN A TEST. A test would be a gate that fails on a legitimate behaviour change,
//! and — worse for the work this harness exists for — it would become a *test that notices* every mutation
//! in the table, which is the one thing the inventory of UNNOTICED clauses must not have. This prints; the
//! comparison is run by hand, and the comparison is the point.
//!
//! THE CORPUS IS THE FIXTURES PLUS SYNTHETICS. `tests/fixtures/design-sweep/*.json` are the reports whose
//! `expect` blocks were captured from the JavaScript judge, and they are the most trustworthy material in
//! the tree. They are also a PARTIAL corpus — a report only carries the axes its sweep measured — so
//! `synthetic()` adds reports written to reach the clauses no fixture fires (and the quiet, boundary rows
//! a clause must NOT fire on: a mutation that ADDS output is as visible here as one that removes it).
//! Each report is judged three ways: its own options, a `strict` set that turns every floor on and carries
//! exemptions of every kind (used, dormant, undeclared), and `bare` (the defaults).
//!
//! USAGE:
//!
//!     cargo run -p summrise-agent-core --example judge-diff > /tmp/judge-before.txt
//!     md5sum /tmp/judge-before.txt
//!
//! Every string is printed JSON-escaped, so the dump is one line per finding/line/note and the difference
//! between two runs is a line diff rather than an eyeball.

use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use summrise_agent_core::sweep_judge::{judge_report, report_summary, JudgeOptions};

/// One report and the options its own sweep would judge it with.
struct Case {
    label: String,
    report: Value,
    opts: Value,
}

fn main() {
    let mut cases: Vec<Case> = Vec::new();
    for (name, fixture) in fixtures() {
        cases.push(Case {
            label: name,
            report: fixture.get("report").cloned().unwrap_or(Value::Null),
            opts: fixture.get("opts").cloned().unwrap_or_else(|| json!({})),
        });
    }
    cases.extend(synthetic());

    let strict = strict_opts();
    let bare = json!({});

    let mut dumped = 0usize;
    for case in &cases {
        for (tag, opts_json) in [("own", &case.opts), ("strict", &strict), ("bare", &bare)] {
            let opts = JudgeOptions::from_json(Some(opts_json));
            dump(&format!("{} [{tag}]", case.label), &case.report, &opts);
            dumped += 1;
        }
    }
    // A FLOOR ON THE RUN: a corpus that silently shrank is a harness that proves less than it claims, and
    // an empty corpus compares equal forever.
    println!(
        "### harness: {dumped} case(s) over {} report(s)",
        cases.len()
    );
    assert!(
        dumped >= 42,
        "the corpus shrank to {dumped} case(s) — this harness no longer covers what it claims"
    );
}

fn dump(label: &str, report: &Value, opts: &JudgeOptions) {
    let outcome = judge_report(report, opts);
    println!("=== {label}");
    for f in &outcome.findings {
        println!("finding: {}", quote(f));
    }
    for l in &outcome.lines {
        println!("line: {}", quote(l));
    }
    for n in &outcome.report_notes {
        println!("note: {}", quote(n));
    }
    println!("summary: {}", quote(&report_summary("panel", report)));
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
        names.len() >= 11,
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
