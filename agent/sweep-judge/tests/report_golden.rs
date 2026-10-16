//! THE GOLDEN PIN FOR `report::judge_report` — the whole judgement, over a fixed corpus, byte for byte.
//!
//! **WHY IT EXISTS.** `judge_report` is seventeen named clauses, and a measurement of the crate's own suite
//! found SEVEN of them noticed by NO test at all: `judge_accessible_names`, `judge_focus`,
//! `judge_reduced_motion`, `judge_unstyled`, `judge_hover`, `judge_delivered_push` and `judge_reflow`. The
//! gaps were invisible because the other ten clauses each had *a* test, so "the suite is green" read as
//! "the judgement is covered". This file is the floor: every clause's body was emptied in turn and this test
//! went red for each one, which is the check that was missing rather than another test of one clause.
//!
//! **WHY THE CORPUS IS NOT THE TWIN'S.** `agent/summrise-command-core` judges the same report shape with the
//! same seventeen clause names, and it has a corpus the example instrument prints
//! (`summrise-command-core/examples/judge-diff.rs`). Its options are JSON (`JudgeOptions::from_json`); this
//! crate's are a Rust struct whose `ignore` entries are `fn` POINTERS, because the panel's one entry is a
//! two-part test that must not be expressible as a single ordered pattern. Two option types that cannot be
//! built from one another are two corpora: the reports below are written out here so a change to THIS
//! judge's corpus can never move the twin's golden, and the twin's fixtures can be regenerated without this
//! pin going dark. (The reports were adapted from that instrument's `kitchen`/`edges`/`quiet`, which are
//! already written to reach every firing branch of the shared clause set.)
//!
//! **THE OPTION SETS ARE THE THREE REAL ADAPTERS PLUS A STRICT ONE.** `panel`, `console` and `landing` are
//! the same `options()` the shipping CLIs call, so the dump pins what those runs actually decide; `strict`
//! turns every floor on and carries an exemption of each kind (one that MATCHES, one that matches nothing
//! and does not say why, one that declares itself dormant), which are the branches a real adapter's options
//! leave unreachable.
//!
//! **WHAT IS DUMPED, AND IN WHAT ORDER.** Findings — with the reflow entries the exemptions may consult —
//! in push order, then the `note:` lines `Out` captured in call order. The two halves are grouped rather
//! than interleaved because `judge_report` returns the findings and writes the notes; the CLI's streams are
//! the only place the interleaving exists, and it is not this clause set's contract.
//!
//! **REFRESH.** `SUMMRISE_REFRESH_SWEEP_JUDGE_GOLDEN=1 cargo test -p summrise-sweep-judge --test report_golden`
//! rewrites the file and PASSES — the repository's convention for every generated artefact (`grep
//! SUMMRISE_REFRESH`): the write is the assertion, and the next un-refreshed run is what proves the file is
//! the corpus's output. A failure prints the first differing lines from BOTH sides with line numbers, because
//! a pin whose message is two hashes costs the next reader the investigation it exists to save.

use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use summrise_sweep_judge::out::Out;
use summrise_sweep_judge::report::{judge_report, IgnoreRule, JudgeOpts};
use summrise_sweep_judge::tools::{console, landing, panel};

/// The committed dump, beside the corpus it was produced from.
const GOLDEN: &str = "tests/fixtures/judge-golden.txt";

/// The `SUMMRISE_REFRESH_*` spelling this test answers to.
const REFRESH: &str = "SUMMRISE_REFRESH_SWEEP_JUDGE_GOLDEN";

/// The test's own name, quoted in the refresh hint so the reader can paste the command.
const TEST: &str = "report_golden";

/// The smallest corpus this pin will accept as itself. A corpus that silently shrank is a pin that proves
/// less than it claims — and the three reports x four option sets here is the number the mutation inventory
/// was run against (12).
const MIN_CASES: usize = 12;

/// One report, judged under every option set in [`option_sets`].
struct Case {
    label: String,
    report: Value,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            label: "kitchen".to_string(),
            report: kitchen(),
        },
        Case {
            label: "edges".to_string(),
            report: edges(),
        },
        Case {
            label: "quiet".to_string(),
            report: quiet(),
        },
    ]
}

/// The three real adapters' options, plus the strict set their own choices leave unreachable.
fn option_sets() -> Vec<(&'static str, JudgeOpts<'static>)> {
    vec![
        ("panel", panel::options()),
        ("console", console::options()),
        ("landing", landing::options()),
        ("strict", strict_opts()),
    ]
}

/// THE STRICT SET: every floor on, and an exemption of each kind.
///
/// The three entries are not decoration. `used` MATCHES the kitchen report's press row, so its suppression
/// line is printed; `stale` matches nothing and declares nothing, which is the "prune it, or declare it
/// dormant" branch; `dormant` matches nothing and says why it stays, which is the other.
static STRICT_IGNORE: [IgnoreRule; 3] = [
    IgnoreRule {
        matches: |text, _entry| text.contains("renders NOTHING when pressed"),
        reason: "the pointer never reached this control",
        dormant: None,
    },
    IgnoreRule {
        matches: |text, _entry| text.contains("zzz-never-matches"),
        reason: "an entry nothing needs",
        dormant: None,
    },
    IgnoreRule {
        matches: |text, _entry| text.contains("zzz-dormant"),
        reason: "a guard for a state that passes today",
        dormant: Some("no measurement needs it in this corpus"),
    },
];

static STRICT_IMPLICIT: [(&str, &str); 3] = [
    (
        "kitchen-ghost",
        "a class the kitchen sink paints on purpose",
    ),
    ("never-painted", "a guard for a state nothing paints yet"),
    (
        "xterm-screen",
        "xterm.js DOM, styled by the sheet it injects at runtime",
    ),
];

fn strict_opts() -> JudgeOpts<'static> {
    JudgeOpts {
        // PREFIXES: `Settings-ack-light` and `Terminal-16-sessions` are the same family as their page.
        navless: &["Settings", "Terminal"],
        prose_floor: Some(60.0),
        implicit_states: &STRICT_IMPLICIT,
        unstyled_floor: Some(40.0),
        unmeasurable_floor: Some(0.05),
        ignore: &STRICT_IGNORE,
    }
}

/// The whole dump, as one string ending in a newline.
fn render() -> String {
    let cases = cases();
    let sets = option_sets();
    let mut buf = String::new();
    let mut dumped = 0usize;
    for case in &cases {
        for (tag, opts) in &sets {
            dump(
                &mut buf,
                &format!("{} [{tag}]", case.label),
                &case.report,
                opts,
            );
            dumped += 1;
        }
    }
    buf.push_str(&format!(
        "### corpus: {dumped} case(s) over {} report(s) x {} option set(s)\n",
        cases.len(),
        sets.len()
    ));
    assert!(
        dumped >= MIN_CASES,
        "the corpus shrank to {dumped} case(s) — this pin no longer covers what it claims"
    );
    buf
}

/// One report under one option set: the findings (with their entries), then the captured `note:` lines.
fn dump(buf: &mut String, label: &str, report: &Value, opts: &JudgeOpts<'_>) {
    let mut out = Out::capture();
    let findings = judge_report(report, opts, &mut out);
    buf.push_str(&format!("=== {label}\n"));
    for f in &findings {
        buf.push_str(&format!("finding: {}\n", quote(&f.text)));
        // A reflow finding carries the report entry it was built from, because an exemption may judge the
        // ENTRY rather than the sentence — so the entry is part of what this judge returns and is pinned.
        if let Some(entry) = &f.entry {
            buf.push_str(&format!(
                "entry: {}\n",
                serde_json::to_string(entry)
                    .unwrap_or_else(|e| panic!("an entry that cannot be quoted: {e}"))
            ));
        }
    }
    for n in out.notes() {
        buf.push_str(&format!("note: {}\n", quote(n)));
    }
}

/// JSON-escaped, so a string carrying a newline cannot make two different dumps look like one.
fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|e| panic!("a string that cannot be quoted: {e}"))
}

#[test]
fn report_golden() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
    let rendered = render();
    if std::env::var(REFRESH).is_ok_and(|v| !v.is_empty()) {
        fs::write(&path, &rendered).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        return;
    }
    let committed = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{} missing ({e}) — run {REFRESH}=1 cargo test -p summrise-sweep-judge --test {TEST}",
            path.display()
        )
    });
    if committed != rendered {
        panic!("{}", difference(&committed, &rendered, &path));
    }
}

/// THE FAILURE MESSAGE IS THE TOOL: the first differing lines from BOTH sides, with their line numbers —
/// two hashes would cost the next reader the very investigation this pin exists to save.
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
    // Two lines of lead-in, then the first eight lines of each side, so the reader sees the sentence that
    // moved rather than only the fact that something did.
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
        "FIX: if the movement is intended, run {REFRESH}=1 cargo test -p summrise-sweep-judge --test {TEST} \
         and commit the file; if it is not, a clause changed what it says."
    ));
    msg
}

/// THE LOUD REPORT: every clause that FILES something, plus the notes inside `judge_focus`, `judge_unstyled`
/// and `judge_reflow`. The branches that need an option set (the prose floor, the navless prefixes, the
/// exemptions) are reached by the same report under `strict`.
fn kitchen() -> Value {
    json!({
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
    })
}

/// THE EDGES: the branches a loud report cannot reach — a report that declares NOTHING about what its
/// fixture served (so the read-failure clause has no premise), a delivery that matches the build, a navless
/// page NAME under the prefix rule, and the ack clause's "measured N and NONE acknowledged".
fn edges() -> Value {
    json!({
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
    })
}

/// THE QUIET REPORT: boundary rows a clause must NOT fire on — a 10px row (the floor is a floor), a graphic
/// below it, a navs-1 page, a nav_loud element, a fully acknowledged press. A clause that INVENTS output
/// shows up here as a difference too.
fn quiet() -> Value {
    json!({
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
    })
}
