//! `judgeReport(report, opts)` — ported from `agent/scripts/lib/design-sweep.mjs:1782-2361`.
//!
//! EVERY MESSAGE BELOW IS BYTE-IDENTICAL TO THE JAVASCRIPT'S, including the em-dashes, the interpolated
//! numbers and the order the findings are pushed in. These strings are the product: a human reads them to
//! learn what is wrong with a page, and a port that decides the same thing but says it differently is a
//! failed port. Where a clause's reason is worth keeping, the JavaScript's own comment is carried beside
//! it with its round number, because the reason is what tells the next reader whether the clause can move.
//!
//! The report is loosely typed and every field is optional (the JS duck-types everywhere), so everything
//! goes through `serde_json::Value` and the small helpers in `crate::js`.

use crate::js::{
    arr, at, get, is_nullish, is_num, join, js_str, len, num, num_is, slice0, to_fixed, truthy,
};
use crate::out::Out;
use regex::{Regex, RegexBuilder};
use serde_json::Value;

/// One judgement about a page.
///
/// Mostly text; a reflow row carries the REPORT ENTRY as well, because an exemption may look at the entry
/// rather than at the sentence (the panel's 320px artifact is only an artifact when the offending
/// scrollers are tab children, and a rule matching the text alone would also hide a genuine defect).
#[derive(Clone, Debug)]
pub struct Finding {
    pub text: String,
    pub entry: Option<Value>,
}

impl Finding {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            entry: None,
        }
    }
}

/// An exemption consulted against FINDINGS (not rows), with the reason printed when it is used.
///
/// A `fn` pointer rather than a pattern string because the panel's single entry is a two-part test
/// (`/div\.rail-dot/` AND `2\.33`) written as a function precisely so one ordered pattern could not match
/// one path and miss the other.
pub struct IgnoreRule {
    pub matches: fn(&str, Option<&Value>) -> bool,
    pub reason: &'static str,
    /// Present when the entry is a GUARD for a state that currently passes: the text says why it is
    /// expected to match nothing in a clean run.
    pub dormant: Option<&'static str>,
}

#[derive(Default)]
pub struct JudgeOpts<'a> {
    /// Page names (prefixes) that have no navigation BY DESIGN.
    pub navless: &'a [&'a str],
    /// Fail a prose row over this many characters per line. Absent means this UI has not measured the axis.
    pub prose_floor: Option<f64>,
    /// Classes with no matching rule that are not defects, each with the mechanism named.
    pub implicit_states: &'a [(&'a str, &'a str)],
    /// The floor under "the collector read almost nothing". Per-UI, because a page can be legitimately small.
    pub unstyled_floor: Option<f64>,
    /// The share of rows allowed to be unmeasurable before the report's absences prove nothing.
    pub unmeasurable_floor: Option<f64>,
    pub ignore: &'a [IgnoreRule],
}

impl<'a> JudgeOpts<'a> {
    fn implicit_reason(&self, class: &str) -> Option<&'a str> {
        self.implicit_states
            .iter()
            .find(|(name, _)| *name == class)
            .map(|(_, reason)| *reason)
    }
}

const DEFAULT_UNSTYLED_FLOOR: f64 = 100.0;
const DEFAULT_UNMEASURABLE_FLOOR: f64 = 0.1;

/// The vocabulary exceptions for the "one focal point at most" clause.
///
/// THE EXCEPTION IS ABOUT ELEMENTS, NOT PAGE NAMES (round 68). It used to except a whole page by name
/// prefix, which hid its siblings — CI rendered `Terminal-16-sessions`, `Desktop-16-sessions`,
/// `Desktop-Terminal-fail-dark` and `Desktop-empty`, all two-loud with the SAME rail button and the
/// new-session action, and none of them was covered. Naming the ELEMENTS cannot widen.
fn nav_loud(entry: &str) -> bool {
    static RULES: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    let rules = RULES.get_or_init(|| {
        [
            // the rail button is WHICH PAGE YOU ARE ON (navigation state)
            r"rail-btn|desktop-rail-btn",
            // the active session tab is WHICH SESSION (navigation state)
            r"^div\.tab|^button\.dtab",
            // the primary action on the page, which state-colour-check protects deliberately
            r"btn-new",
            // the approval gate's Approve — the one control that outranks the page's action while a
            // command is held (round 69)
            r"approval-approve",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("a NAV_LOUD pattern must compile"))
        .collect()
    });
    rules.iter().any(|r| r.is_match(entry))
}

/// A CLOCK IS NOT A REPAINT (round 69). The first CI run of the idle pass reported "6 mutations in 6s
/// (#text x6)" — one per second — and naming the PARENT turned it into `span.approval-left x6`: the
/// approval countdown counting down. That is a value that is SUPPOSED to change, and calling it a repaint
/// of unchanged output would be a false finding, which is worse than none.
fn is_clock(name: &str) -> bool {
    static RULES: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    let rules = RULES.get_or_init(|| {
        [
            // the approval countdown — seconds until the gate closes
            r"^span\.approval-left$",
            // a running command's elapsed time (the panel's one clock)
            r"^span\.cmd-duration$",
            // the same elapsed time inside the trajectory view
            r"^span\.traj-",
            // the same elapsed time in the details column
            r"^\.details-duration$",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("a CLOCKS pattern must compile"))
        .collect()
    });
    rules.iter().any(|r| r.is_match(name))
}

/// The shared verdict. Returns the findings that SURVIVED the caller's exemptions, in push order.
///
/// A NOTE IS NOT A FINDING, and two clauses inside the JavaScript push to `report.notes` instead. That
/// array is WRITTEN AND NEVER READ — no adapter prints it, and `grep` finds no reader anywhere in
/// `agent/scripts/`. The port keeps the writes, in a vector nobody reads, so that "this note was
/// deliberately not shown" stays visible here rather than being silently dropped; see the report beside
/// this change for the defect finding.
pub fn judge_report(report: &Value, opts: &JudgeOpts<'_>, out: &mut Out) -> Vec<Finding> {
    let mut findings: Vec<Finding> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut suppressed: Vec<(String, &'static str)> = Vec::new();

    let re_not_rendered = Regex::new("not rendered").expect("compiles");
    let re_sessions = RegexBuilder::new("Sessions unavailable")
        .case_insensitive(true)
        .build()
        .expect("compiles");

    // ── SURFACES ────────────────────────────────────────────────────────────────────────────────────
    for s in arr(get(report, "surfaces")) {
        let page = js_str(get(s, "page"));
        let width = get(s, "width");
        let where_ = if truthy(width) {
            format!("{page}@{}px", js_str(width))
        } else {
            page.clone()
        };
        if !num_is(get(s, "h1Count"), 1.0) || !truthy(get(s, "firstIsH1")) {
            findings.push(Finding::text(format!(
                "{where_}: h1 count {}, first-is-h1 {}",
                js_str(get(s, "h1Count")),
                js_str(get(s, "firstIsH1"))
            )));
        }
        if truthy(get(s, "skipped")) {
            findings.push(Finding::text(format!(
                "{where_}: {} skipped heading level(s)",
                js_str(get(s, "skipped"))
            )));
        }
        if !num_is(get(s, "mains"), 1.0) {
            findings.push(Finding::text(format!(
                "{where_}: {} main landmark(s), expected exactly 1",
                js_str(get(s, "mains"))
            )));
        }
        if num(get(s, "navs")).map(|n| n > 1.0).unwrap_or(false) {
            findings.push(Finding::text(format!(
                "{where_}: {} nav landmarks — a page has one navigation",
                js_str(get(s, "navs"))
            )));
        }
        // PREFIX, NOT EXACT. `navless: ["login"]` was an exact match on a page name, so when the console's
        // login page gained a dark render (`login-dark`, round 229) the page that has no navigation BY
        // DESIGN was reported as missing one. A navless entry names a FAMILY of renders — the same page in
        // another theme, at another width — and matching the name alone is the one-of-N shape this session
        // keeps finding.
        else if !num_is(get(s, "navs"), 1.0) && !opts.navless.iter().any(|n| page.starts_with(n))
        {
            findings.push(Finding::text(format!(
                "{where_}: {} nav landmark(s), expected exactly 1",
                js_str(get(s, "navs"))
            )));
        }
        for (kind, key) in [
            ("overflow", "over"),
            ("clipping", "clipped"),
            ("sliver", "slivers"),
        ] {
            let list = get(s, key);
            if len(list) > 0 {
                findings.push(Finding::text(format!(
                    "{where_}: {kind} — {}",
                    join(list, "; ")
                )));
            }
        }
        // PROSE HAS A MEASURE, WHEN THE CALLER ASKS FOR ONE (round 265). A policy rather than a law of
        // nature — a dashboard's own answer may differ — so the floor travels in the caller's options and a
        // UI that has not measured this axis is not failed by a number somebody else picked. The row carries
        // the width and the computed max-width, because "this line is 206 characters because the surface is
        // 1339px wide" and "because no cap was ever written" want different repairs.
        if let Some(floor) = opts.prose_floor {
            for row in arr(at(get(s, "measure"), "worst")) {
                let cpl = num(get(row, "cpl"));
                if !cpl.map(|c| c > floor).unwrap_or(false) {
                    continue;
                }
                // "PER LINE", NOT "ON ONE LINE": `cpl` is chars over rendered lines, so a block that wraps
                // three times can still be over the floor — and this axis's first CI run called a
                // 477-character note "159 characters on ONE line", a sentence about a defect that did not
                // exist. The count is per line; the shape is the block's (round 265).
                findings.push(Finding::text(format!(
                    "{where_}: {} renders {} characters PER LINE ({} chars in {} line(s) over {}px at {}px, max-width {}) — past {} a reader loses the line return; this sheet's own ledes cap at 66ch",
                    js_str(get(row, "sel")),
                    js_str(get(row, "cpl")),
                    js_str(get(row, "chars")),
                    js_str(get(row, "lines")),
                    js_str(get(row, "w")),
                    js_str(get(row, "fs")),
                    js_str(get(row, "maxw")),
                    js_str(Some(&serde_json::json!(floor))),
                )));
            }
        }
        // THE MARK LANGUAGE AS PAINTED. A family whose two states render identically is colour-only wherever
        // a cascade override or a missing rule made it so — the sheet can be right while the page is wrong,
        // which is exactly how `.plug-dot[error]` kept a stray halo through a unit test that passed
        // (round 25). A MARK THAT IS BOTH A FILL AND A RING IS NEITHER, and it is not a collision — no other
        // state shares it, so the distinctness check would pass it. Rendered, not read.
        let marks = get(s, "marks");
        if let Some(m) = marks {
            if len(get(m, "ringFill")) > 0 {
                findings.push(Finding::text(format!(
                    "{where_}: {} mark(s) are a FILL inside a RING — the vocabulary is solid / ring / halo / empty, and a mark that is two of them is neither: {}",
                    len(get(m, "ringFill")),
                    join(get(m, "ringFill"), "; ")
                )));
            }
            if len(get(m, "collisions")) > 0 {
                findings.push(Finding::text(format!(
                    "{where_}: states of one mark paint identically — {}",
                    join(get(m, "collisions"), "; ")
                )));
            }
        }
        // A COLOUR THE PROBE COULD NOT READ IS A FAILURE, not a skip: every comparison against NaN is
        // false, so an unreadable background used to pass every guard and be counted as loud (rounds 18-56).
        // REPORTED, NOT FAILED — for now: a NOTE is the honest severity, and the count rides in the summary
        // so it cannot be ignored. (The note is pushed into `report.notes` and never printed; see the
        // function's own comment.)
        if truthy(get(s, "loudUnreadable")) {
            let samples = arr(get(s, "loudUnreadableSamples"))
                .iter()
                .take(2)
                .map(|x| js_str(Some(x)))
                .collect::<Vec<_>>()
                .join("; ");
            let tail = if samples.is_empty() {
                String::new()
            } else {
                format!(" ({samples})")
            };
            notes.push(format!(
                "{where_}: {} background(s) in a colour syntax this probe cannot read — skipped, not counted{tail}",
                js_str(get(s, "loudUnreadable"))
            ));
        }
        // ONE FOCAL POINT, AT MOST. Two loud surfaces means neither is the thing the page is about; the
        // ceiling is one, and zero is allowed because a form or a dashboard is all context and should not
        // shout.
        let loud = arr(get(s, "loud"));
        let nav_loud_all = loud.iter().all(|e| nav_loud(&js_str(Some(e))));
        if loud.len() > 1 && !nav_loud_all {
            findings.push(Finding::text(format!(
                "{where_}: {} loud elements — a page has ONE focal point at most — {}",
                loud.len(),
                join(get(s, "loud"), "; ")
            )));
        }
    }

    // ── ACCESSIBLE NAMES ────────────────────────────────────────────────────────────────────────────
    for n in arr(get(report, "names")) {
        let width = get(n, "width");
        let where_ = format!(
            "{}{}",
            js_str(get(n, "page")),
            if truthy(width) {
                format!("@{}px", js_str(width))
            } else {
                String::new()
            }
        );
        if len(get(n, "unnamed")) > 0 {
            findings.push(Finding::text(format!(
                "{where_}: {} control(s) with NO accessible name — {}",
                len(get(n, "unnamed")),
                join(get(n, "unnamed"), ", ")
            )));
        }
        if len(get(n, "titleOnly")) > 0 {
            findings.push(Finding::text(format!(
                "{where_}: {} control(s) named only by title — {}",
                len(get(n, "titleOnly")),
                join(get(n, "titleOnly"), ", ")
            )));
        }
    }

    // ── PROVENANCE ──────────────────────────────────────────────────────────────────────────────────
    // WHICH HARNESS GENERATION WAS MEASURED. Round 189 lost an afternoon to a delivered harness that
    // predated a CSS fix: its inlined stylesheet collapsed the tab strip to 17px, the sweep reported
    // overflow that looked like a live defect, and a waiver hid it. The stamp travels in the report and is
    // printed here, so a reader can see at a glance that the fixture is older than the build it should match.
    if truthy(get(report, "harnessBuild")) {
        out.note(&format!(
            "note: harness build {}",
            js_str(get(report, "harnessBuild"))
        ));
    }
    // AND WHEN IT IS CURRENT, SAY SO. The panel prints its harness generation on every run, so a reader
    // always knows which artifact was measured; provenance that only appears in a failure is provenance
    // nobody can check.
    if let Some(entry) = get(report, "entryCheck") {
        if !truthy(get(entry, "stale")) && !truthy(get(entry, "error")) {
            out.note(&format!(
                "note: delivered entry {} bytes / sha {} — matches the build",
                js_str(get(entry, "bytes")),
                js_str(get(entry, "sha"))
            ));
        }
    }
    // A DELIVERED COPY OLDER THAN THE BUILD MEASURES SOMETHING NOBODY CAN NAME.
    if truthy(get(
        get(report, "entryCheck").unwrap_or(&Value::Null),
        "stale",
    )) {
        let e = get(report, "entryCheck").unwrap_or(&Value::Null);
        let expected = get(e, "expected").unwrap_or(&Value::Null);
        if truthy(get(e, "error")) {
            findings.push(Finding::text(format!(
                "the delivered entry could not be read ({}) — expected {} bytes, sha {}",
                js_str(get(e, "error")),
                js_str(get(expected, "bytes")),
                js_str(get(expected, "sha"))
            )));
        } else {
            findings.push(Finding::text(format!(
                "the delivered entry is {} bytes / sha {} but this sweep was emitted against {} / {} — every measurement below is of a stale build",
                js_str(get(e, "bytes")),
                js_str(get(e, "sha")),
                js_str(get(expected, "bytes")),
                js_str(get(expected, "sha"))
            )));
        }
    }
    if truthy(get(report, "harnessStale")) {
        findings.push(Finding::text(format!(
            "the harness is build {} but this sweep was emitted against {} — every measurement below is of a stale fixture",
            js_str(get(report, "harnessBuild")),
            js_str(get(report, "expectedHarnessBuild"))
        )));
    }

    // ── FOCUS ───────────────────────────────────────────────────────────────────────────────────────
    // HOW OFTEN THE COMPUTED-STYLE VERDICT WAS OVERRULED BY THE PIXELS. Not a finding — the pixels are the
    // authority and they said the ring is painted — but it is the number to WATCH: it was 18 out of 18 in
    // round 186, where the style check called every console ring missing while the browser drew all of them,
    // and six rounds went by before anyone looked at a screenshot.
    let focus = arr(get(report, "focus"));
    let paint_total: f64 = focus
        .iter()
        .map(|f| num(get(f, "paintConfirmed")).unwrap_or(0.0))
        .sum();
    let pressed_total: f64 = focus
        .iter()
        .map(|f| num(get(f, "pressed")).unwrap_or(0.0))
        .sum();
    if !paint_total.is_nan() && paint_total != 0.0 {
        out.note(&format!(
            "note: the pixels overruled the computed-style focus verdict {} time(s) of {} press(es) — the rings are painted, the style check cannot see them",
            js_str(Some(&serde_json::json!(paint_total))),
            js_str(Some(&serde_json::json!(pressed_total)))
        ));
    }
    for f in focus {
        // A CHECK THAT COULD NOT LOOK IS NOT A PASS. The pixel confirmation can fail (an offscreen element,
        // a clip the browser refuses), and the first version turned that into "no focus indication" — a
        // false finding. It is now its own verdict, and it FAILS the run, because an unperformed measurement
        // proves nothing either way.
        if truthy(get(f, "unconfirmed")) {
            let on = arr(get(f, "unconfirmedOn"));
            findings.push(Finding::text(format!(
                "{}{} focus candidate(s) could not be confirmed against the pixels{} — the check could not look, so it cannot say",
                if truthy(get(f, "page")) {
                    format!("{}: ", js_str(get(f, "page")))
                } else {
                    String::new()
                },
                js_str(get(f, "unconfirmed")),
                if !on.is_empty() {
                    format!(" — {}", join(get(f, "unconfirmedOn"), ", "))
                } else {
                    String::new()
                }
            )));
        }
        if truthy(get(f, "missing")) {
            findings.push(Finding::text(format!(
                "{}{} Tab stop(s) with no visible focus ring",
                if truthy(get(f, "page")) {
                    format!("{}: ", js_str(get(f, "page")))
                } else {
                    String::new()
                },
                js_str(get(f, "missing"))
            )));
        }
        // A RUN THAT LANDED NOWHERE IS NOT A PASSING RUN. Focus escaping to the body used to count as "ok",
        // so a page with nothing focusable reported a clean sheet — the same "a skip reads as a pass" defect
        // the colour sweep bans. A row that says it pressed keys and landed on nothing is a finding.
        if num(get(f, "pressed")).map(|p| p > 0.0).unwrap_or(false) && num_is(get(f, "landed"), 0.0)
        {
            findings.push(Finding::text(format!(
                "{}{}: focus landed on nothing in {} Tab press(es) ({} escaped to the body) — that is a report of no focusable targets, not of good focus rings",
                if truthy(get(f, "density")) {
                    js_str(get(f, "density"))
                } else {
                    String::new()
                },
                if truthy(get(f, "theme")) {
                    format!("/{}", js_str(get(f, "theme")))
                } else {
                    String::new()
                },
                js_str(get(f, "pressed")),
                js_str(get(f, "escaped"))
            )));
        }
    }

    // ── PRESS ───────────────────────────────────────────────────────────────────────────────────────
    // A PRESS THAT RENDERS NOTHING IS A CONTROL THAT DOES NOT ANSWER (round 55). `pressPass` presses each
    // target with the pointer and compares the computed style before and during. `feedback-check.mjs` proves
    // the RULE exists; only this can see whether it reaches the screen.
    for row in arr(get(report, "press")) {
        // THE LABEL NAMES THE ITERATION, not just the surface: density/theme alone made a finding from the
        // rail walk indistinguishable from one from the mode loop.
        let where_ = format!(
            "{}/{}{}{}",
            if truthy(get(row, "density")) {
                js_str(get(row, "density"))
            } else {
                "?".to_string()
            },
            if truthy(get(row, "theme")) {
                js_str(get(row, "theme"))
            } else {
                "?".to_string()
            },
            if truthy(get(row, "mode")) {
                format!(" {}", js_str(get(row, "mode")))
            } else {
                String::new()
            },
            if truthy(get(row, "page")) {
                format!(" {}", js_str(get(row, "page")))
            } else {
                String::new()
            }
        );
        let rows = arr(get(row, "rows"));
        // AND ONLY WHERE THE POINTER ARRIVED: a row the pass could not deliver a press to is not a control
        // that ignored one — the distinction that matters when an element sits below the fold.
        for d in rows.iter().filter(|r| {
            matches!(get(r, "changed"), Some(Value::Bool(false)))
                && !matches!(get(r, "reached"), Some(Value::Bool(false)))
        }) {
            findings.push(Finding::text(format!(
                "{where_}: {} ({}) renders NOTHING when pressed — before and during are identical ({})",
                js_str(get(d, "sel")),
                js_str(get(d, "where")),
                js_str(get(d, "size"))
            )));
        }
        for r in rows {
            let note = get(r, "note");
            if truthy(note) && !re_not_rendered.is_match(&js_str(note)) {
                out.note(&format!(
                    "note: {where_} {} — {}",
                    js_str(get(r, "sel")),
                    js_str(note)
                ));
            }
        }
        // A PASS THAT PRESSED NOTHING IS NOT A CLEAN PASS — AND THE FLOOR IS WHAT THE PAGE HAS, not a
        // constant. A curated list expects a surface to render several of its selectors, so two is the bar.
        // A DISCOVERED pass reports how many controls the page had, and ONE control pressed is a complete
        // pass on a page that has one: the harness's Browser page is an explanation with a single control
        // (round 45), and a floor of two called that vacuous.
        let floor = if is_nullish(get(row, "found")) {
            2.0
        } else {
            2.0_f64.min(num(get(row, "found")).unwrap_or(f64::NAN))
        };
        if !is_nullish(get(row, "found"))
            && num(get(row, "found")).unwrap_or(f64::NAN) > rows.len() as f64
        {
            out.note(&format!(
                "note: {where_} has {} control(s) and the pass pressed the first {} (cap) — the rest were not measured",
                js_str(get(row, "found")),
                rows.len()
            ));
        }
        let measured_value = get(row, "measured");
        let measured = if truthy(measured_value) {
            num(measured_value)
        } else {
            Some(0.0)
        };
        if measured.map(|m| m < floor).unwrap_or(false) {
            findings.push(Finding::text(format!(
                "{where_}: the press pass measured {} control(s){} — a press pass that pressed nothing proves nothing",
                if truthy(measured_value) {
                    js_str(measured_value)
                } else {
                    "0".to_string()
                },
                if is_nullish(get(row, "found")) {
                    String::new()
                } else {
                    format!(" of the {} this page renders", js_str(get(row, "found")))
                }
            )));
        }
    }

    // ── ACKNOWLEDGEMENT ─────────────────────────────────────────────────────────────────────────────
    // IMMEDIATE FEEDBACK HAS A BUDGET (round 19) — AND IT IS JUDGED HERE NOW, FOR EVERY SWEEP THAT MEASURES
    // IT. This clause lived in `panel-design-sweep.mjs`, so the PANEL's acknowledgement rows were judged and
    // the CONSOLE's were not: one row describing a control that never acknowledged a press exits 1 under the
    // panel's judge and 0 under the console's — the same row, the same fixture. ONE DERIVATION, so the two
    // sweeps cannot drift apart again: the clause belongs to the REPORT SHAPE, not to a UI.
    let ack = arr(get(report, "ack"));
    for a in ack {
        let where_ = format!(
            "{}{}",
            if truthy(get(a, "density")) {
                js_str(get(a, "density"))
            } else {
                "?".to_string()
            },
            if truthy(get(a, "page")) {
                format!(" {}", js_str(get(a, "page")))
            } else {
                String::new()
            }
        );
        // AN UNMEASURED PREMISE IS NOT AN EXCUSE (round 11 of the standing goal). The clause below excuses a
        // control that asked the device nothing — but only a page that HAS a request counter can tell "asked
        // nothing" from "nobody looked", and the console had none, so EVERY one of its rows was excused by a
        // note claiming a measurement that never happened.
        if matches!(get(a, "hasCounter"), Some(Value::Bool(false))) {
            findings.push(Finding::text(format!(
                "{where_}: {} was pressed on a page with NO request counter, so whether it asked the device anything is UNMEASURED — the acknowledgement axis proves nothing here",
                js_str(get(a, "sel"))
            )));
            continue;
        }
        if truthy(get(a, "note")) {
            out.note(&format!(
                "note: {where_} {} — {}",
                js_str(get(a, "sel")),
                js_str(get(a, "note"))
            ));
            continue;
        }
        if !truthy(get(a, "acked")) {
            // ONLY WHERE THERE WAS SOMETHING TO WAIT FOR. A control that asked the device nothing (a tab
            // switching a snippet, a disclosure) cannot be late: its row says so rather than becoming a finding.
            if matches!(get(a, "asked"), Some(Value::Bool(false))) {
                out.note(&format!(
                    "note: {where_} {} — asked the device nothing, so there was nothing to acknowledge",
                    js_str(get(a, "sel"))
                ));
                continue;
            }
            // WHAT THIS CAN HONESTLY CLAIM: the control never acknowledged the press in the window. Whether
            // it asked the device is NOT attributable from a request counter on a page that polls for its own
            // reasons, so the finding does not say it did.
            findings.push(Finding::text(format!(
                "{where_}: {} ({}) never acknowledged the press — no busy state and no painted change within the window ({})",
                js_str(get(a, "sel")),
                js_str(get(a, "where")),
                js_str(get(a, "size"))
            )));
            continue;
        }
        // EVERY ROW'S NUMBERS, ON EVERY RUN (round 26). The judge reported only the failures, so a CI-only
        // failure could not be compared with a clean device run without re-running both by hand: eight
        // controls "never acknowledged" in CI and answered in 6-13ms on the device, same sweep, same fixture.
        for line in ack_notes(std::slice::from_ref(a), &where_) {
            out.note(&line);
        }
        if is_num(get(a, "msToAck"))
            && is_num(get(a, "budgetMs"))
            && num(get(a, "msToAck")).unwrap_or(0.0) > num(get(a, "budgetMs")).unwrap_or(0.0)
        {
            findings.push(Finding::text(format!(
                "{where_}: {} ({}) acknowledged the press after {}ms — the budget is {}ms, so this feedback waited on the {}ms network round trip instead of firing on the event",
                js_str(get(a, "sel")),
                js_str(get(a, "where")),
                js_str(get(a, "msToAck")),
                js_str(get(a, "budgetMs")),
                js_str(get(a, "msToClear"))
            )));
        }
    }
    {
        let acked = ack.iter().filter(|a| truthy(get(a, "acked"))).count();
        if !ack.is_empty() && acked == 0 {
            findings.push(Finding::text(format!(
                "the acknowledgement pass measured {} control(s) and NONE acknowledged — a pass that proves nothing is not a pass",
                ack.len()
            )));
        }
    }

    // ── TARGET SIZE, IN ONE PLACE (round 20 of the standing goal) ────────────────────────────────────
    // This criterion lived in THREE judges — panel, console, landing — with three slightly different
    // sentences, and in round 17 only the PANEL's copy was strengthened with the spacing clause's second
    // half. So one page judged by two sweeps got two verdicts, which is the shape "one derivation" exists to
    // prevent. It is here now and all three judge it identically.
    //
    // THE TWO HALVES, AND WHY CONTAINMENT IS NOT ONE OF THEM. `passesBySpacing` is CENTRE-to-CENTRE — the
    // circle-vs-circle test, right against another undersized target and wrong against a LARGE one, where
    // the circle has to clear that neighbour's BOX. The probe computes both, plus `insideSel` (the container,
    // when the target is nested). A nested control's circle is inside its container by construction, so the
    // literal reading of "the circles do not intersect another target" fails EVERY nested control in the
    // product: that is nested interactive content, a different question, and it is NAMED below rather than
    // enforced. What IS enforced is crowding — a small target whose circle reaches a neighbour BESIDE it.
    for t in arr(get(report, "targets")) {
        let where_ = {
            let parts: Vec<String> = ["density", "page", "mode"]
                .iter()
                .filter(|k| truthy(get(t, k)))
                .map(|k| js_str(get(t, k)))
                .collect();
            if parts.is_empty() {
                "?".to_string()
            } else {
                parts.join(" ")
            }
        };
        let distinct = arr(get(t, "distinct"));
        for u in distinct {
            if !truthy(get(u, "passesBySpacing")) {
                findings.push(Finding::text(format!(
                    "target size ({where_}): {} is {}x{} and its nearest neighbour is {}px away — 2.5.8 wants 24x24 or 24px of spacing (\"{}\")",
                    js_str(get(u, "sel")),
                    js_str(get(u, "w")),
                    js_str(get(u, "h")),
                    js_str(get(u, "nearest")),
                    js_str(get(u, "text"))
                )));
                continue;
            }
            if matches!(get(u, "passesByFullRule"), Some(Value::Bool(false))) {
                findings.push(Finding::text(format!(
                    "target size ({where_}): {} is {}x{} and its 24px circle reaches {} ({}x{}) {}px away — the spacing clause wants the centre 12px clear of another target's BOX, not only 24px from its centre (\"{}\")",
                    js_str(get(u, "sel")),
                    js_str(get(u, "w")),
                    js_str(get(u, "h")),
                    js_str(get(u, "nearSel")),
                    js_str(get(u, "nearW")),
                    js_str(get(u, "nearH")),
                    js_str(get(u, "gapToBox")),
                    js_str(get(u, "text"))
                )));
            }
        }
        let nested: Vec<&Value> = distinct
            .iter()
            .filter(|u| truthy(get(u, "insideSel")))
            .collect();
        if !nested.is_empty() {
            out.note(&format!(
                "note: target size ({where_}): {} of {} undersized target(s) sit INSIDE another target — not a spacing failure (the circle is inside its container by construction), but a near-miss there activates the container: {}",
                nested.len(),
                distinct.len(),
                nested
                    .iter()
                    .map(|u| format!(
                        "{} {}x{} in {} {}x{}",
                        js_str(get(u, "sel")),
                        js_str(get(u, "w")),
                        js_str(get(u, "h")),
                        js_str(get(u, "insideSel")),
                        js_str(get(u, "insideW")),
                        js_str(get(u, "insideH"))
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }

    // ── READ-FAILURE CLAIMS ─────────────────────────────────────────────────────────────────────────
    // A SURFACE MAY NOT CLAIM A READ FAILED WHEN THE FIXTURE ANSWERED EVERY CALL (round 100). A normal
    // surface serves every endpoint, so a page that says "could not be read" or "did not answer, so ..." is
    // asserting something about the DEVICE that the fixture contradicts — the panel blaming the device for a
    // question it answered.
    //
    // THE SURFACES THAT MEAN IT ARE EXCUSED BY THE FIXTURE'S OWN ANSWER, not by a list here: report.sse
    // carries whether the run rejected every API call on purpose (?fail=1), and on those pages the claim is
    // TRUE.
    let surfaces = arr(get(report, "surfaces"));
    let sse = arr(get(report, "sse"));
    let rejected_calls: Vec<String> = sse
        .iter()
        .filter(|s| truthy(get(s, "fail")))
        .map(|s| js_str(get(s, "page")))
        .collect();
    // THE PREMISE HAS TO HOLD BEFORE THE CLAUSE APPLIES: a claim is only FALSE where a fixture ANSWERED.
    // The panel's report carries that evidence (`sse` records per surface, including the ?fail=1 pages); the
    // CONSOLE has no backend at all — its sweep serves a static build, every API call fails for real, and
    // "could not be read" on its Models page is TRUE. A clause that failed that page would be the instrument
    // lying about the console, which is the defect this whole round is about, one level up. So a report that
    // declares nothing about what it served is not judged on this axis, and says so the first time it runs.
    let fixture_evidence = !sse.is_empty();
    if !fixture_evidence && surfaces.iter().any(|s| len(get(s, "claims")) > 0) {
        out.note("note: read-failure claims are NOT judged here — this report declares nothing about what its fixture served, so the clause has no premise (the console has no backend)");
    }
    let mut excused_claims: Vec<String> = Vec::new();
    let judged_surfaces: &[Value] = if fixture_evidence { surfaces } else { &[] };
    for s in judged_surfaces {
        let claims = arr(get(s, "claims"));
        if claims.is_empty() {
            continue;
        }
        let page = js_str(get(s, "page"));
        if rejected_calls.contains(&page) {
            for c in claims {
                excused_claims.push(format!("{page}: {}", js_str(Some(c))));
            }
            continue;
        }
        for c in claims {
            findings.push(Finding::text(format!(
                "{page} claims a read failed — \"{}\" — while the fixture answered every call: either the panel is wrong about the device, or the fixture never stubbed an endpoint the card needs",
                js_str(Some(c))
            )));
        }
    }
    if !excused_claims.is_empty() {
        out.note(&format!(
            "note: {} read-failure claim(s) on surfaces whose fixture REJECTS every call — true by construction:",
            excused_claims.len()
        ));
        let mut seen: Vec<&String> = Vec::new();
        for w in &excused_claims {
            if !seen.contains(&w) {
                seen.push(w);
            }
        }
        for w in seen.iter().take(4) {
            out.note(&format!("  {w}"));
        }
    }

    // ── IDLE REPAINT (round 64) ──────────────────────────────────────────────────────────────────────
    // The objective lists it among the things a claim is verified by, and the measurement is DOM mutations
    // on a settled page under STATIC fixtures: React writes to the DOM only when the output differs, so
    // nothing changing means nothing written. The observer proves it is alive by seeing one deliberate
    // mutation of the panel's own root, and that is required — a blind observer reports a perfectly still
    // panel forever.
    for row in arr(get(report, "idle")) {
        let where_ = format!(
            "{}/{}/{}",
            if truthy(get(row, "density")) {
                js_str(get(row, "density"))
            } else {
                "?".to_string()
            },
            if truthy(get(row, "theme")) {
                js_str(get(row, "theme"))
            } else {
                "?".to_string()
            },
            if truthy(get(row, "page")) {
                js_str(get(row, "page"))
            } else {
                "?".to_string()
            }
        );
        let by_target = get(row, "byTarget").unwrap_or(&Value::Null);
        let probe = num(get(by_target, "__probe")).unwrap_or(0.0);
        let real = num(get(row, "mutations")).unwrap_or(0.0) - probe;
        if probe < 1.0 {
            findings.push(Finding::text(format!(
                "{where_}: the idle observer did not see its own probe mutation — a blind instrument reports a still panel forever, so this measurement proves nothing"
            )));
        }
        if real > 0.0 {
            let all: Vec<(&String, &Value)> = match by_target {
                Value::Object(m) => m.iter().filter(|(k, _)| k.as_str() != "__probe").collect(),
                _ => Vec::new(),
            };
            let repaints: Vec<&(&String, &Value)> =
                all.iter().filter(|(k, _)| !is_clock(k.as_str())).collect();
            let clocks: Vec<&(&String, &Value)> =
                all.iter().filter(|(k, _)| is_clock(k.as_str())).collect();
            if !clocks.is_empty() {
                notes.push(format!(
                    "{where_}: {} changed while idle — a live duration, exempt by name with its reason in CLOCKS",
                    clocks
                        .iter()
                        .map(|(k, v)| format!("{k} x{}", js_str(Some(v))))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !repaints.is_empty() {
                let total: f64 = repaints
                    .iter()
                    .map(|(_, v)| num(Some(v)).unwrap_or(0.0))
                    .sum();
                findings.push(Finding::text(format!(
                    "{where_}: {} DOM mutation(s) in {}s while idle — nothing changed under static fixtures, so this is a repaint of unchanged output ({})",
                    js_str(Some(&serde_json::json!(total))),
                    js_str(get(row, "seconds")),
                    repaints
                        .iter()
                        .map(|(k, v)| format!("{k} x{}", js_str(Some(v))))
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }
    }

    // ── REDUCED MOTION ───────────────────────────────────────────────────────────────────────────────
    // REDUCED MOTION IS A CONTRACT, NOT A COURTESY. `motion` entries come from a render with the preference
    // EMULATED: anything still carrying a transition or an infinite animation under it is a finding.
    // Measured round 77 — the panel density honoured the preference and the desktop density did not.
    for m in arr(get(report, "motion")) {
        // BOTH SHAPES: `animating` is the older single-number row, `stillAnimating` the newer one. The
        // JS is `m.stillAnimating || m.animating || []` — a TRUTHINESS fallback, so a falsy-but-present
        // `stillAnimating` (0, "") falls through to `animating` rather than being taken as an empty list.
        let still = if truthy(get(m, "stillAnimating")) {
            arr(get(m, "stillAnimating"))
        } else if truthy(get(m, "animating")) {
            arr(get(m, "animating"))
        } else {
            &[]
        };
        if !still.is_empty() {
            findings.push(Finding::text(format!(
                "reduced motion ({}): {} element(s) still animate — {}",
                if truthy(get(m, "density")) {
                    js_str(get(m, "density"))
                } else {
                    "?".to_string()
                },
                still.len(),
                still
                    .iter()
                    .take(3)
                    .map(|x| js_str(Some(x)))
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }
        // A CHECK THAT FOUND NOTHING TO SUPPRESS PROVES NOTHING. Round 134 added the second measurement: if
        // nothing animates WITHOUT the preference either, then "nothing animates under reduce" says nothing
        // about the rule — the page has no motion to honour, or the probe matched nothing at all.
        if num_is(get(m, "normal"), 0.0) {
            findings.push(Finding::text(format!(
                "reduced motion ({}): 0 elements animate WITHOUT the preference, so this result proves nothing about prefers-reduced-motion — the check found nothing to suppress",
                if truthy(get(m, "density")) {
                    js_str(get(m, "density"))
                } else {
                    "?".to_string()
                }
            )));
        }
    }

    // ── UNSTYLED, AND THE THREE EXEMPTION LISTS ──────────────────────────────────────────────────────
    // HOVER IS A STATE, and until round 84 neither instrument looked at it. A CLASS THE PAGE RENDERS THAT NO
    // RULE STYLES — the mirror of dead CSS, and the failure a prune causes. `opts.implicitStates` names
    // classes that are unstyled ON PURPOSE because a base rule already produces their appearance, each with
    // the reason printed rather than hidden.
    //
    // WHICH DECLARED CLASSES THIS RUN ACTUALLY SAW (round 24). `implicitStates` is the third list of
    // exemptions the suite keeps, after DECORATIVE (rows) and ignore (findings) — and like the other two it
    // had no way to say that an entry was not earning its place. Counted here, reported below with the size
    // of the search.
    let declared_implicit: Vec<(&str, &str)> = opts.implicit_states.to_vec();
    let mut seen_implicit: Vec<String> = Vec::new();
    let unstyled = arr(get(report, "unstyled"));
    for u in unstyled {
        for c in arr(get(u, "classes")) {
            let name = js_str(Some(c));
            if opts.implicit_reason(&name).is_some() && !seen_implicit.contains(&name) {
                seen_implicit.push(name);
            }
        }
    }
    for u in unstyled {
        let all = arr(get(u, "classes"));
        let waived: Vec<String> = all
            .iter()
            .map(|c| js_str(Some(c)))
            .filter(|c| opts.implicit_reason(c).is_some())
            .collect();
        for c in &waived {
            out.note(&format!(
                "note: {c} is unstyled by design — {}",
                opts.implicit_reason(c).unwrap_or("")
            ));
        }
        let live: Vec<String> = all
            .iter()
            .map(|c| js_str(Some(c)))
            .filter(|c| opts.implicit_reason(c).is_none())
            .collect();
        if !live.is_empty() {
            // THE WORDING MATTERS, and it took a round to get it right: a class with no matching rule is NOT
            // an unstyled element. A base class styles it (`.view` on `view terminal`), or an ATTRIBUTE does
            // (`tab-dot serial` is painted by a [data-kind] rule), or it is a deliberate test marker. What is
            // true — and what is worth failing on — is narrower: this name is on screen and nothing matches
            // it, so either it is an inert extra to prune or it needs a reason to stay.
            findings.push(Finding::text(format!(
                "class name(s) with no matching rule on {}: {} — on screen, matched by nothing",
                if truthy(get(u, "page")) {
                    js_str(get(u, "page"))
                } else {
                    "?".to_string()
                },
                live.join(", ")
            )));
        }
        // A READ THAT FOUND NO STYLESHEETS PROVES NOTHING (round 88's collector reported 38 styled classes
        // where the browser sees 221, and its empty findings looked like a clean page). THE THRESHOLD IS THE
        // MEASURED FAILURE, not a guess: the console's pages have 221 styled classes and round 88's broken
        // collector reported 38 — so a floor of 20 would not have caught it. 100 is below every real page in
        // either UI and above every broken read seen so far.
        //
        // THE FLOOR IS PER-UI, because a page can be legitimately small. A SHEET THE COLLECTOR COULD NOT READ
        // IS A HOLE IN ITS BASIS, and every class that sheet styles looks unstyled: the floor above catches a
        // collector that read almost nothing; this catches one that read almost everything.
        if num(get(u, "sheetsUnreadable"))
            .map(|n| n > 0.0)
            .unwrap_or(false)
        {
            findings.push(Finding::text(format!(
                "unstyled check on {}: {} stylesheet(s) could not be read, so their classes look unstyled — the basis is incomplete",
                if truthy(get(u, "page")) {
                    js_str(get(u, "page"))
                } else {
                    "?".to_string()
                },
                js_str(get(u, "sheetsUnreadable"))
            )));
        }
        if is_num(get(u, "styledClasses")) {
            let floor = opts.unstyled_floor.unwrap_or(DEFAULT_UNSTYLED_FLOOR);
            if num(get(u, "styledClasses")).unwrap_or(f64::NAN) < floor {
                findings.push(Finding::text(format!(
                    "unstyled check on {}: only {} styled classes found (floor {}) — the collector read almost nothing, so its silence means nothing",
                    if truthy(get(u, "page")) {
                        js_str(get(u, "page"))
                    } else {
                        "?".to_string()
                    },
                    js_str(get(u, "styledClasses")),
                    js_str(Some(&serde_json::json!(floor)))
                )));
            }
        }
    }
    {
        let unused: Vec<(&str, &str)> = declared_implicit
            .iter()
            .filter(|(c, _)| !seen_implicit.iter().any(|s| s == c))
            .copied()
            .collect();
        if !unused.is_empty() {
            let classes: usize = unstyled.iter().map(|u| arr(get(u, "classes")).len()).sum();
            out.note(&format!(
                "note: {} of {} declared unstyled-by-design class(es) were not seen in this run ({} unstyled name(s) over {} page(s)) — a reason nothing needs is weight; prune it or say why it stays:",
                unused.len(),
                declared_implicit.len(),
                classes,
                unstyled.len()
            ));
            for (c, reason) in unused {
                out.note(&format!("  {c} — {reason}"));
            }
        }
    }

    // ── TYPE FLOOR ──────────────────────────────────────────────────────────────────────────────────
    // designScale.test.ts pins the SCALE — names, order, and a 10px floor — but a token being 10px and the
    // rendered text being 10px are different claims, and only the second one is what a reader experiences.
    for r in arr(get(report, "rows")) {
        if matches!(get(r, "kind"), Some(Value::String(k)) if k == "graphic") {
            continue;
        }
        let size = num(get(r, "size"));
        if is_num(get(r, "size")) && size.map(|s| s > 0.0 && s < 10.0).unwrap_or(false) {
            let text = if truthy(get(r, "text")) {
                js_str(get(r, "text"))
            } else {
                String::new()
            };
            findings.push(Finding::text(format!(
                "type floor: {} renders at {}px on {} — the scale's floor is 10px (\"{}\")",
                js_str(get(r, "sel")),
                js_str(get(r, "size")),
                if truthy(get(r, "page")) {
                    js_str(get(r, "page"))
                } else {
                    "?".to_string()
                },
                slice0(&text, 24)
            )));
        }
    }

    // ── THE UNMEASURABLE FLOOR ───────────────────────────────────────────────────────────────────────
    // A SCAN THAT COULD NOT READ MOST OF THE PAGE IS NOT A CLEAN SCAN. Rows the probe cannot measure are
    // excluded from judgement — correctly, since guessing at them is how a probe starts lying — but until
    // round 165 they appeared ONLY as a number in the summary line, so a report that had stopped measuring
    // anything would still exit 0 with "nothing above found a defect".
    {
        let rows = arr(get(report, "rows"));
        let all = rows.len();
        let blind = rows
            .iter()
            .filter(|r| {
                matches!(get(r, "cr"), None | Some(Value::Null))
                    || matches!(get(r, "cr"), Some(Value::Number(n)) if n.as_f64().map(|f| f.is_nan()).unwrap_or(false))
            })
            .count();
        let floor = opts
            .unmeasurable_floor
            .unwrap_or(DEFAULT_UNMEASURABLE_FLOOR);
        if all > 0 && (blind as f64) / (all as f64) > floor {
            findings.push(Finding::text(format!(
                "only {} of {} rows could be measured ({}% unmeasurable, floor {}%) — the absences below prove nothing",
                all - blind,
                all,
                to_fixed(((blind as f64) / (all as f64)) * 100.0, 1),
                to_fixed(floor * 100.0, 0)
            )));
        }
    }

    // ── HOVER ───────────────────────────────────────────────────────────────────────────────────────
    for h in arr(get(report, "hover")) {
        let under_aa = arr(get(h, "underAA"));
        if !under_aa.is_empty() {
            findings.push(Finding::text(format!(
                "hover ({}/{}): {} element(s) below AA while hovered — {}",
                if truthy(get(h, "density")) {
                    js_str(get(h, "density"))
                } else {
                    "?".to_string()
                },
                if truthy(get(h, "theme")) {
                    js_str(get(h, "theme"))
                } else {
                    "?".to_string()
                },
                under_aa.len(),
                under_aa
                    .iter()
                    .take(3)
                    .map(|x| js_str(Some(x)))
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }
    }

    // ── THE HARNESS DELIVERED THE PUSH ───────────────────────────────────────────────────────────────
    // THE HARNESS DELIVERED THE PUSH, OR THE MEASUREMENT IS OF ANOTHER PANEL. Every panel surface this suite
    // has ever measured was taken with the SSE fixture either serving a frame (connected) or refusing every
    // call (the failure surfaces, which are SUPPOSED to read as reconnecting). Nothing asserted which — so a
    // fixture change that shut the stream would silently turn every surface into a reconnecting panel and
    // every finding into a statement about a screen nobody sees.
    //
    // AND THE RENDERED TEXT IS THE SECOND WITNESS, independent of the flag above. "Sessions unavailable" is
    // the panel's own sentence for a push that never arrived; on a surface whose harness did NOT report the
    // failure fixture, seeing it means the measurement describes a screen the operator never sees.
    let harness_failed: Vec<(String, bool)> = sse
        .iter()
        .map(|r| (js_str(get(r, "page")), truthy(get(r, "fail"))))
        .collect();
    for row in arr(get(report, "rows")) {
        if !re_sessions.is_match(&js_str(get(row, "text"))) {
            continue;
        }
        let page = js_str(get(row, "page"));
        // A Map built from pairs: the LAST record for a page wins.
        // A Map built from pairs: the LAST record for a page wins.
        let failed = harness_failed
            .iter()
            .rfind(|(p, _)| *p == page)
            .map(|(_, f)| *f)
            .unwrap_or(false);
        if failed {
            continue; // the failure fixture is meant to say exactly this
        }
        findings.push(Finding::text(format!(
            "{}: the panel renders \"Sessions unavailable\" and the harness did not report the failure fixture — this surface was measured with the push missing",
            if truthy(get(row, "page")) {
                page
            } else {
                "?".to_string()
            }
        )));
    }
    for row in sse {
        if truthy(get(row, "fail")) {
            continue; // a failure surface is meant to be disconnected
        }
        if !matches!(get(row, "opened"), Some(Value::Bool(true))) {
            findings.push(Finding::text(format!(
                "{}: the harness never opened the SSE stream, so this surface was measured in the RECONNECTING state — the fixture failed, not the panel",
                if truthy(get(row, "page")) {
                    js_str(get(row, "page"))
                } else {
                    "?".to_string()
                }
            )));
        }
    }

    // ── REFLOW ──────────────────────────────────────────────────────────────────────────────────────
    for r in arr(get(report, "reflow")) {
        if truthy(get(r, "docScrollsSideways")) {
            findings.push(Finding {
                text: format!(
                    "reflow @{}px: the document scrolls sideways ({} > {})",
                    js_str(get(r, "width")),
                    js_str(get(r, "docScrollWidth")),
                    js_str(get(r, "viewport"))
                ),
                entry: Some(r.clone()),
            });
        }
        // A toolbar-style scroller is WCAG 1.4.10's own exception; reported, not failed.
        if !arr(get(r, "sideScrollers")).is_empty() {
            out.note(&format!(
                "note: scrollers at {}px (allowed for toolbars) — {}",
                js_str(get(r, "width")),
                join(get(r, "sideScrollers"), "; ")
            ));
        }
        // AND WHAT WIDENS IT, WHEN NOTHING SCROLLS (round 23 of the standing goal). `sideScrollers` lists only
        // elements that are THEMSELVES scrollers, so a document widened by a merely-WIDE element produced
        // `SCROLLS` with an EMPTY scroller list — which is the state an exemption reading "every offending
        // scroller is a tab child" satisfies vacuously. This names the elements whose box leaves the viewport.
        if truthy(get(r, "docScrollsSideways")) && !arr(get(r, "overflowing")).is_empty() {
            out.note(&format!(
                "note: the {}px overflow comes from — {}",
                js_str(get(r, "width")),
                join(get(r, "overflowing"), "; ")
            ));
        }
        // AND WHAT SPILLS WITHOUT SCROLLING (round 27), and what cannot be reached at all (round 28). Both
        // were built as instruments — measure first, enforce when the number is known — and both are findings
        // now (round 37): the number is MEASURED as zero on both surfaces that have reflow rows, and a note
        // cannot fail CI.
        if !arr(get(r, "spilling")).is_empty() {
            out.note(&format!(
                "note: content spilling out of its own box at {}px — {}",
                js_str(get(r, "width")),
                join(get(r, "spilling"), "; ")
            ));
            findings.push(Finding::text(format!(
                "reflow @{}px: content is spilling out of its own box — {}",
                js_str(get(r, "width")),
                join(get(r, "spilling"), "; ")
            )));
        }
        if !arr(get(r, "clipped")).is_empty() {
            out.note(&format!(
                "note: content clipped with no way to reach it at {}px — {}",
                js_str(get(r, "width")),
                join(get(r, "clipped"), "; ")
            ));
            findings.push(Finding::text(format!(
                "reflow @{}px: content is clipped with no way to reach it — {}",
                js_str(get(r, "width")),
                join(get(r, "clipped"), "; ")
            )));
        }
        // AND THE GRID BEHIND IT, so the two remaining suspects for the terminal's clip are told apart by
        // THIS RUN rather than by another round of reading (round 34).
        if truthy(get(r, "term")) {
            let term = get(r, "term").unwrap_or(&Value::Null);
            out.note(&format!(
                "note: the terminal at {}px renders {} column(s) in a {}px container (cell {}px)",
                js_str(get(r, "width")),
                js_str(get(term, "cols")),
                js_str(get(term, "containerW")),
                js_str(get(term, "cellW"))
            ));
        }
    }

    // ── EXEMPTIONS, AND WHICH ONES THIS RUN NEEDED ───────────────────────────────────────────────────
    // WHICH EXEMPTIONS THIS RUN ACTUALLY NEEDED (round 22). An exemption nothing needs is weight in the one
    // list a reader consults, and the panel spent rounds discovering that the hard way (a waiver for
    // `/^div\.rail-dot$/` had matched nothing since a selector changed). Counted here because this is the
    // only place that knows.
    let mut kept: Vec<Finding> = Vec::new();
    let mut used_rules: Vec<usize> = Vec::new();
    for f in &findings {
        let text = f.text.clone();
        let entry = f.entry.clone();
        let found = opts
            .ignore
            .iter()
            .enumerate()
            .find(|(_, i)| (i.matches)(&text, entry.as_ref()));
        if let Some((idx, rule)) = found {
            if !used_rules.contains(&idx) {
                used_rules.push(idx);
            }
            suppressed.push((text, rule.reason));
        } else {
            kept.push(f.clone());
        }
    }
    {
        // A NOTE, NOT A FINDING, and it names the size of the search: a run that measured one axis produces
        // none of the findings these entries exist for, and then every entry looks unused. The count is what
        // lets a reader tell a stale exemption from a partial run.
        //
        // AND AN ENTRY MAY ANSWER THE QUESTION ITSELF (round 22): an exemption whose reason still holds and
        // whose pattern still fires — a GUARD for a state that currently passes — declares `dormant`, and the
        // note then reports it as declared rather than as weight. An entry that is genuinely dead has nothing
        // to declare, which is exactly the one to prune.
        let unused: Vec<&IgnoreRule> = opts
            .ignore
            .iter()
            .enumerate()
            .filter(|(i, _)| !used_rules.contains(i))
            .map(|(_, r)| r)
            .collect();
        let undeclared: Vec<&&IgnoreRule> = unused.iter().filter(|i| i.dormant.is_none()).collect();
        let dormant: Vec<&&IgnoreRule> = unused.iter().filter(|i| i.dormant.is_some()).collect();
        if !undeclared.is_empty() {
            out.note(&format!(
                "note: {} of {} ignore entr(ies) matched none of this run's {} finding(s) and does not say why it stays — an exemption nothing needs is weight; prune it, or declare it dormant with the measurement that makes it a guard:",
                undeclared.len(),
                opts.ignore.len(),
                findings.len()
            ));
            for u in undeclared {
                out.note(&format!("  {}", u.reason));
            }
        }
        for d in dormant {
            out.note(&format!(
                "note: ignore entry dormant as declared — {}",
                d.dormant.unwrap_or("")
            ));
        }
    }
    for (finding, reason) in &suppressed {
        out.note(&format!("note: set aside ({reason}) — {finding}"));
    }
    kept
}

/// `ackNotes(rows, where)` — every row's numbers, on every run.
///
/// Round 26's reason for printing every row stands: the judge reports only failures, and a CI-only failure
/// could not be compared with a clean device run without re-running both by hand — eight controls "never
/// acknowledged" in CI and answered in 6-13ms on the device, same sweep, same fixture.
pub fn ack_notes(rows: &[Value], where_: &str) -> Vec<String> {
    let mut out = Vec::new();
    for a in rows {
        let sel = js_str(get(a, "sel"));
        let suffix = {
            let mut s = String::new();
            let w = js_str(get(a, "where"));
            if truthy(get(a, "where")) && w != sel {
                s.push_str(&format!(" ({w})"));
            }
            if truthy(get(a, "name")) {
                s.push_str(&format!(" [{}]", js_str(get(a, "name"))));
            }
            s
        };
        let ms = if is_nullish(get(a, "msToAck")) {
            "-".to_string()
        } else {
            js_str(get(a, "msToAck"))
        };
        let presses = {
            let p = get(a, "presses");
            if !is_nullish(p) {
                js_str(p)
            } else {
                let at = get(a, "attempts");
                if !is_nullish(at) {
                    js_str(at)
                } else {
                    "1".to_string()
                }
            }
        };
        out.push(format!(
            "note: ack {where_} {sel}{suffix} — acked={} via={} ms={ms} budget={} presses={presses}",
            js_str(get(a, "acked")),
            {
                let via = get(a, "via");
                if truthy(via) {
                    js_str(via)
                } else {
                    "none".to_string()
                }
            },
            js_str(get(a, "budgetMs"))
        ));
        if truthy(get(a, "acked")) && num(get(a, "attempts")).unwrap_or(1.0) > 1.0 {
            out.push(format!(
                "note: {where_} {sel}{suffix} acknowledged only on the SECOND press — the first sample saw nothing, which on a loaded machine is a timing artefact and on a real control is an acknowledgement that depends on state"
            ));
        }
        if is_num(get(a, "msToAck"))
            && is_num(get(a, "budgetMs"))
            && num(get(a, "msToAck")).unwrap_or(0.0) > num(get(a, "budgetMs")).unwrap_or(0.0)
        {
            out.push(format!(
                "note: {where_} {sel} took {}ms against a {}ms budget",
                js_str(get(a, "msToAck")),
                js_str(get(a, "budgetMs"))
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::out::Out;
    use serde_json::json;

    /// A minimal surface that satisfies every clause the tests below are NOT about, so a finding that
    /// appears is the one the fixture planted.
    fn clean_surface() -> Value {
        json!({
            "page": "Terminal", "h1Count": 1, "firstIsH1": true, "skipped": 0, "mains": 1, "navs": 1,
            "over": [], "clipped": [], "slivers": [],
        })
    }

    fn run(report: &Value) -> (Vec<String>, Vec<String>) {
        run_with(report, &JudgeOpts::default())
    }

    fn run_with(report: &Value, opts: &JudgeOpts<'_>) -> (Vec<String>, Vec<String>) {
        let mut out = Out::capture();
        let findings = judge_report(report, opts, &mut out);
        (
            findings.into_iter().map(|f| f.text).collect(),
            out.notes().to_vec(),
        )
    }

    #[test]
    fn a_clean_surface_produces_nothing() {
        let report = json!({"surfaces": [clean_surface()]});
        assert!(run(&report).0.is_empty());
    }

    #[test]
    fn the_h1_and_landmark_clauses_say_what_they_saw() {
        let report = json!({"surfaces": [
            {"page": "Terminal", "h1Count": 2, "firstIsH1": false, "skipped": 0, "mains": 0, "navs": 1},
        ]});
        let (findings, _) = run(&report);
        assert_eq!(
            findings,
            [
                "Terminal: h1 count 2, first-is-h1 false",
                "Terminal: 0 main landmark(s), expected exactly 1",
            ]
        );
    }

    #[test]
    fn a_navless_page_is_excused_by_prefix_and_not_by_exact_name() {
        let opts = JudgeOpts {
            navless: &["login"],
            ..JudgeOpts::default()
        };
        let report = json!({"surfaces": [
            {"page": "login-dark", "h1Count": 1, "firstIsH1": true, "mains": 1, "navs": 0},
        ]});
        assert!(run_with(&report, &opts).0.is_empty());
        let other = json!({"surfaces": [
            {"page": "overview", "h1Count": 1, "firstIsH1": true, "mains": 1, "navs": 0},
        ]});
        assert_eq!(
            run_with(&other, &opts).0,
            ["overview: 0 nav landmark(s), expected exactly 1"]
        );
    }

    #[test]
    fn two_loud_elements_fail_unless_every_one_is_navigation() {
        let navigation_only = json!({"surfaces": [{
            "page": "panel-Terminal", "h1Count": 1, "firstIsH1": true, "mains": 1, "navs": 1,
            "loud": ["button.rail-btn 1444px2 rgb(154,52,18)", "div.tab 3254px2 rgb(154,52,18)"],
        }]});
        assert!(run(&navigation_only).0.is_empty());
        let one_is_not = json!({"surfaces": [{
            "page": "panel-Memory", "h1Count": 1, "firstIsH1": true, "mains": 1, "navs": 1,
            "loud": ["button.rail-btn 1444px2", "div.mem-busy 19680px2"],
        }]});
        let (findings, _) = run(&one_is_not);
        assert_eq!(findings.len(), 1);
        assert!(findings[0]
            .starts_with("panel-Memory: 2 loud elements — a page has ONE focal point at most — "));
    }

    #[test]
    fn the_press_floor_is_the_pages_own_count() {
        let one_of_one = json!({"surfaces": [], "press": [
            {"density": "panel", "theme": "light", "page": "Browser", "found": 1, "measured": 1, "rows": []},
        ]});
        assert!(run(&one_of_one).0.is_empty());
        let one_of_four = json!({"surfaces": [], "press": [
            {"density": "panel", "theme": "light", "page": "Browser", "found": 4, "measured": 1, "rows": []},
        ]});
        assert_eq!(
            run(&one_of_four).0,
            ["panel/light Browser: the press pass measured 1 control(s) of the 4 this page renders — a press pass that pressed nothing proves nothing"]
        );
    }

    #[test]
    fn a_press_the_pointer_never_delivered_is_not_a_dead_control() {
        let report = json!({"surfaces": [], "press": [{
            "density": "panel", "theme": "light", "page": "Browser", "found": 4, "measured": 2,
            "rows": [
                {"sel": ".rail-btn", "where": "button.rail-btn", "size": "38x38", "changed": true, "reached": true},
                {"sel": ".btn", "where": "button.btn", "size": "54x26", "changed": false, "props": [],
                 "reached": false, "note": "the pointer never reached this control"},
            ],
        }]});
        assert!(run(&report).0.is_empty());
    }

    #[test]
    fn an_unmeasured_request_counter_fails_rather_than_being_excused() {
        let report = json!({"surfaces": [], "ack": [
            {"density": "panel", "page": "devices", "sel": "button.ok", "where": "button.ok", "size": "54x26",
             "acked": true, "via": "paint", "msToAck": 12, "msToClear": 40, "budgetMs": 100, "attempts": 1,
             "hasCounter": true},
            {"density": "panel", "page": "devices", "sel": "button.btn", "where": "button.btn", "size": "54x26",
             "acked": false, "budgetMs": 100, "attempts": 2, "hasCounter": false},
        ]});
        let (findings, notes) = run(&report);
        assert_eq!(
            findings,
            ["panel devices: button.btn was pressed on a page with NO request counter, so whether it asked the device anything is UNMEASURED — the acknowledgement axis proves nothing here"]
        );
        // And the acknowledged row still printed its numbers, which is what makes a CI-only failure
        // comparable with a clean device run.
        assert!(notes.iter().any(|n| n.starts_with(
            "note: ack panel devices button.ok — acked=true via=paint ms=12 budget=100 presses=1"
        )));
    }

    #[test]
    fn a_control_that_asked_the_device_nothing_is_a_note_not_an_accusation() {
        let report = json!({"surfaces": [], "ack": [
            {"density": "panel", "page": "devices", "sel": "a.tab", "where": "a.tab", "size": "40x40",
             "acked": false, "asked": false, "msToAck": null, "budgetMs": 100, "attempts": 1},
        ]});
        let (findings, notes) = run(&report);
        assert!(!findings.iter().any(|f| f.contains("never acknowledged")));
        assert!(notes
            .iter()
            .any(|n| n == "note: panel devices a.tab — asked the device nothing, so there was nothing to acknowledge"));
    }

    #[test]
    fn a_late_acknowledgement_names_the_round_trip_it_waited_on() {
        let report = json!({"surfaces": [], "ack": [
            {"sel": ".monitor-btn", "where": "button.btn.monitor-btn", "size": "79x31", "acked": true,
             "via": "data-busy", "msToAck": 912, "msToClear": 1180, "budgetMs": 100,
             "density": "panel", "page": "Settings-ack-light"},
        ]});
        assert_eq!(
            run(&report).0,
            ["panel Settings-ack-light: .monitor-btn (button.btn.monitor-btn) acknowledged the press after 912ms — the budget is 100ms, so this feedback waited on the 1180ms network round trip instead of firing on the event"]
        );
    }

    #[test]
    fn the_target_size_clauses_have_both_halves() {
        let spacing = json!({"surfaces": [], "targets": [{"density": "panel", "page": "overview",
            "distinct": [{"sel": "input.cb", "text": "", "w": 13, "h": 13, "nearest": 93.8, "passesBySpacing": true}]}]});
        assert!(run(&spacing).0.is_empty());
        let crowded = json!({"surfaces": [], "targets": [{"density": "panel", "page": "overview",
            "distinct": [{"sel": "button.x", "text": "x", "w": 12, "h": 12, "nearest": 4.0, "passesBySpacing": false}]}]});
        assert_eq!(
            run(&crowded).0,
            ["target size (panel overview): button.x is 12x12 and its nearest neighbour is 4px away — 2.5.8 wants 24x24 or 24px of spacing (\"x\")"]
        );
    }

    #[test]
    fn the_prose_floor_only_applies_when_the_caller_states_one() {
        let report = json!({"surfaces": [{
            "page": "Settings", "h1Count": 1, "firstIsH1": true, "mains": 1, "navs": 1,
            "measure": {"measured": 24, "worst": [{"sel": "p.muted", "cpl": 206, "chars": 206, "lines": 1,
                                                   "w": 1339, "fs": 13, "maxw": "none"}]},
        }]});
        assert!(run(&report).0.is_empty());
        let opts = JudgeOpts {
            prose_floor: Some(90.0),
            ..JudgeOpts::default()
        };
        let (findings, _) = run_with(&report, &opts);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].contains("renders 206 characters PER LINE"));
        assert!(findings[0].contains("max-width none) — past 90 a reader loses the line return"));
    }

    #[test]
    fn the_type_floor_skips_graphics_and_reads_the_rendered_size() {
        // `cr` is present on every row: a row without one is UNMEASURABLE, and the floor clause would
        // add a second finding that has nothing to do with the type floor under test.
        let report = json!({"surfaces": [], "rows": [
            {"sel": "span.small", "size": 9, "text": "hello world", "page": "Terminal", "cr": 7.0},
            {"sel": "span.graphic", "size": 8, "kind": "graphic", "page": "Terminal", "cr": 7.0},
            {"sel": "span.ok", "size": 11, "text": "hello", "page": "Terminal", "cr": 7.0},
        ]});
        assert_eq!(
            run(&report).0,
            ["type floor: span.small renders at 9px on Terminal — the scale's floor is 10px (\"hello world\")"]
        );
    }

    #[test]
    fn the_unmeasurable_floor_prints_the_percentage_to_one_decimal() {
        let report = json!({"surfaces": [], "rows": [
            {"cr": 7.0}, {"cr": null}, {"cr": null}, {"cr": null},
        ]});
        let (findings, _) = run(&report);
        assert_eq!(
            findings,
            ["only 1 of 4 rows could be measured (75.0% unmeasurable, floor 10%) — the absences below prove nothing"]
        );
    }

    #[test]
    fn a_surface_that_claims_a_read_failed_is_only_judged_with_fixture_evidence() {
        let no_evidence = json!({"surfaces": [{
            "page": "Terminal", "h1Count": 1, "firstIsH1": true, "mains": 1, "navs": 1,
            "claims": ["p.muted: did not answer, so its restart history could not be read."],
        }]});
        let (findings, notes) = run(&no_evidence);
        assert!(findings.is_empty());
        assert!(notes
            .iter()
            .any(|n| n.contains("read-failure claims are NOT judged here")));

        let answered = json!({"sse": [{"page": "Terminal", "opened": true, "fail": false}], "surfaces": [{
            "page": "Terminal", "h1Count": 1, "firstIsH1": true, "mains": 1, "navs": 1,
            "claims": ["p.muted: did not answer, so its restart history could not be read."],
        }]});
        assert_eq!(
            run(&answered).0,
            ["Terminal claims a read failed — \"p.muted: did not answer, so its restart history could not be read.\" — while the fixture answered every call: either the panel is wrong about the device, or the fixture never stubbed an endpoint the card needs"]
        );

        let rejected = json!({"sse": [{"page": "Terminal", "fail": true}], "surfaces": [{
            "page": "Terminal", "h1Count": 1, "firstIsH1": true, "mains": 1, "navs": 1,
            "claims": ["p.muted: did not answer, so its restart history could not be read."],
        }]});
        let (findings, notes) = run(&rejected);
        assert!(findings.is_empty());
        assert!(notes
            .iter()
            .any(|n| n.contains("1 read-failure claim(s) on surfaces whose fixture REJECTS every call — true by construction:")));
    }

    #[test]
    fn a_blind_idle_observer_fails_where_a_still_panel_passes() {
        let still = json!({"surfaces": [], "idle": [
            {"density": "panel", "theme": "light", "page": "Terminal", "seconds": 6,
             "byTarget": {"__probe": 1}, "mutations": 1},
        ]});
        assert!(run(&still).0.is_empty());
        let blind = json!({"surfaces": [], "idle": [
            {"density": "panel", "theme": "light", "page": "Terminal", "seconds": 6,
             "byTarget": {}, "mutations": 0},
        ]});
        assert_eq!(
            run(&blind).0,
            ["panel/light/Terminal: the idle observer did not see its own probe mutation — a blind instrument reports a still panel forever, so this measurement proves nothing"]
        );
        let busy = json!({"surfaces": [], "idle": [
            {"density": "panel", "theme": "light", "page": "Terminal", "seconds": 6,
             "byTarget": {"__probe": 1, "div.totals": 3}, "mutations": 4},
        ]});
        assert_eq!(
            run(&busy).0,
            ["panel/light/Terminal: 3 DOM mutation(s) in 6s while idle — nothing changed under static fixtures, so this is a repaint of unchanged output (div.totals x3)"]
        );
    }

    #[test]
    fn the_ignore_rule_sets_a_finding_aside_and_says_so() {
        fn rail_dot(text: &str, _entry: Option<&Value>) -> bool {
            text.contains("div.rail-dot") && text.contains("2.33")
        }
        let opts = JudgeOpts {
            ignore: &[IgnoreRule {
                matches: rail_dot,
                reason: "the working rail dot's halo is emphasis",
                dormant: None,
            }],
            ..JudgeOpts::default()
        };
        let report = json!({"surfaces": [], "rows": [
            {"sel": "div.rail-dot", "cr": 2.33, "need": 3, "size": 8, "text": "", "kind": "graphic", "page": "p"},
        ]});
        // The shared judge does not judge contrast rows; this exercises the suppression path through a
        // clause it does own, by way of a report whose only finding is one the rule matches.
        let with_finding = json!({"surfaces": [{
            "page": "p", "h1Count": 1, "firstIsH1": true, "mains": 1, "navs": 1,
            "over": ["div.rail-dot 2.33"],
        }]});
        let mut out = Out::capture();
        let kept = judge_report(&with_finding, &opts, &mut out);
        assert!(kept.is_empty());
        assert!(out
            .notes()
            .iter()
            .any(|n| n == "note: set aside (the working rail dot's halo is emphasis) — p: overflow — div.rail-dot 2.33"));
        assert!(run(&report).0.is_empty());
    }

    #[test]
    fn an_unused_dormant_exemption_is_reported_as_declared_rather_than_as_weight() {
        fn never(_text: &str, _entry: Option<&Value>) -> bool {
            false
        }
        let opts = JudgeOpts {
            ignore: &[IgnoreRule {
                matches: never,
                reason: "unused",
                dormant: Some("nothing to excuse today"),
            }],
            ..JudgeOpts::default()
        };
        let report = json!({"surfaces": [clean_surface()]});
        let (findings, notes) = run_with(&report, &opts);
        assert!(findings.is_empty());
        assert!(notes
            .iter()
            .any(|n| n == "note: ignore entry dormant as declared — nothing to excuse today"));
        assert!(!notes
            .iter()
            .any(|n| n.contains("does not say why it stays")));
    }

    #[test]
    fn the_entry_check_names_what_it_expected() {
        let stale = json!({"surfaces": [], "entryCheck": {
            "bytes": 1, "sha": "deadbeef", "stale": true, "expected": {"bytes": 2, "sha": "cafe"},
        }});
        assert_eq!(
            run(&stale).0,
            ["the delivered entry is 1 bytes / sha deadbeef but this sweep was emitted against 2 / cafe — every measurement below is of a stale build"]
        );
        let unreadable = json!({"surfaces": [], "entryCheck": {
            "error": "ENOENT", "stale": true, "expected": {"bytes": 2, "sha": "cafe"},
        }});
        assert_eq!(
            run(&unreadable).0,
            ["the delivered entry could not be read (ENOENT) — expected 2 bytes, sha cafe"]
        );
        let current = json!({"surfaces": [], "entryCheck": {
            "bytes": 2, "sha": "cafe", "stale": false, "expected": {"bytes": 2, "sha": "cafe"},
        }});
        let (findings, notes) = run(&current);
        assert!(findings.is_empty());
        assert_eq!(
            notes,
            ["note: delivered entry 2 bytes / sha cafe — matches the build"]
        );
    }
}
