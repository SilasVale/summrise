//! The design sweep's JUDGE, in Rust — the half of `agent/scripts/lib/design-sweep.mjs` that reads a
//! sweep's result rows and decides whether each one is a defect.
//!
//! WHY THIS EXISTS. The operator's decision (`docs/superpowers/plans/2026-09-28-the-product-moves-to-rust.md`,
//! P1) is that the repository unifies on one language, and this module is the first half of the sweep
//! family's move: the JUDGE is pure logic over JSON, so it moves now. The DRIVER — a Playwright/CDP page
//! driver that navigates, presses and samples — is `lib/sweep/*.cjs` plus the passes in `design-sweep.mjs`
//! and stays JavaScript until someone builds the Node↔Rust boundary the plan names as its cost. **The plan's
//! stop condition for this tier is explicit: if the driver cannot be done, the judge still moves and the
//! driver stays JS.** Partial migration is a valid outcome.
//!
//! NOT THE DESIGN PLUGIN. `agent/src/plugins/design/` is the other half of the same *subject* and not the
//! same *thing*: `page_view` FETCHES a page's HTML/CSS so a model can read its design, and it has no
//! criteria, no report and no findings. This module reads a report's rows and decides. They share a subject
//! and nothing else — an input shape, an output shape and a failure vocabulary are all different — so they
//! stay separate modules. Merging them would be writing the same thing twice in the other direction.
//!
//! ── THE FIXED SEMANTICS, WHICH IS WHAT THIS PORTS ────────────────────────────────────────────────────
//!
//! On 2026-09-28 the JS judge filed twelve findings against a live button — "renders NOTHING when pressed"
//! — and a real pointer showed `:active` matching and `transform: none → matrix(1, 0, 0, 1, 0, 1)`. The
//! criterion was wrong twice (`f4a65696`; `scripts/test/press-anchor-check.mjs` then, and
//! `agent/tests/press_anchor.rs` since landing 3), and the fixes are the
//! reason the port is worth doing, so they are ported rather than the semantics that produced the false
//! finding:
//!
//!   (A) THE BASELINE WAS READ FROM A DIFFERENT ELEMENT THAN THE ONE PRESSED. `styleOf` and the box probe
//!       both took `querySelectorAll(sel)` match #1, and only the box probe asked `checkVisibility` — so on
//!       Settings the press landed on `Save & connect` while BOTH snapshots described a button inside a
//!       CLOSED `<details>`. That is a DRIVER defect, and it is why the judge's own press clause carries the
//!       second half: a row the pointer never reached (`reached: false`) is NOT a control that ignored a
//!       press.
//!   (B) A SELECTOR IS NOT A HANDLE. A discovered target's whole handle was the STRING `button.btn`, so the
//!       box probe re-resolved it too: both targets pressed element #1 while `found: 2` claimed two controls.
//!       That too is a DRIVER fix — and it is why the judge's floor is derived from `found` (what the page
//!       HAD) rather than from a constant, so a row that claims two controls and measured none cannot pass.
//!
//! ── WHAT IS DELIBERATELY DIFFERENT FROM THE JS ──────────────────────────────────────────────────────
//!
//!   * `report.notes` IS RETURNED, NOT WRITTEN BACK. The JS judge does `const notes = report.notes ||
//!     (report.notes = [])` and pushes two clauses into it — and NOTHING IN THIS REPOSITORY READS THAT
//!     ARRAY (`grep -rn '\.notes\b' agent/scripts` answers with the one line that creates it). Two clauses
//!     that say "REPORTED, NOT FAILED" are therefore reported to nobody, and the sentence beside them ("the
//!     count rides in the summary so it cannot be ignored") is false — `reportSummary` never reads `notes`.
//!     [`JudgeOutcome::report_notes`] carries them so a caller can print them, and the port does not mutate
//!     its input.
//!   * JSON OBJECT KEY ORDER. `serde_json`'s `Map` is a `BTreeMap` (sorted) unless the `preserve_order`
//!     feature is on; a JS object keeps INSERTION order. Two clauses print a list built from object keys —
//!     `report.idle[].byTarget` (a finding's sentence) and `opts.implicitStates` (the unused-exemption note)
//!     — so with more than one key those lists come out SORTED here and in insertion order there. The SET of
//!     sentences is identical and no verdict changes. This is the port's one measured divergence; it is
//!     named rather than hidden, and the escape hatch is `preserve_order` on `serde_json` (a change to every
//!     JSON object in the workspace, which is a large price for two lists).
//!   * `opts.ignore` MATCHES LITERAL SUBSTRINGS. The JS rules are closures — `test: (text, entry) => …` —
//!     which cannot cross a JSON boundary at all. Today's one rule (`/div\.rail-dot/ && /2\.33/`) is a pair
//!     of literal substrings, which is what [`IgnoreRule::contains`] models: EVERY entry must appear. A rule
//!     that needs a real pattern, or that reads the report ENTRY, is not expressible here and must be added
//!     deliberately, with the equivalence re-run.
//!   * Number and string rendering follows JavaScript, because the findings are sentences a person reads:
//!     `js_num` is `String(Number)`, `js_slice` counts UTF-16 code units (`String.slice`). The residual
//!     differences are the ones every JS/Rust pair has — `1e21` and `1e-7` print in exponent form there and
//!     in full here — and they are reachable only from a report carrying such a number.
//!
//! ── HOW IT IS PROVEN ────────────────────────────────────────────────────────────────────────────────
//!
//! `tests/fixtures/design-sweep/*.json` is a corpus of reports that BOTH judges read: the file holds the
//! report, the options its sweep judges it with, and an `expect` block CAPTURED FROM THE JS JUDGE. The test
//! module at the bottom of this file asserts this module reproduces `expect` exactly — findings, printed
//! lines, the report notes, the summary and the mark-coverage notes. The corpus includes the Settings pair
//! that read `0pressed/2found` before the fix and `2pressed/2found` after, a passing row, failing and
//! boundary rows, and cases that must NOT bite.

use serde_json::Value;

// ── JAVASCRIPT VALUE SEMANTICS ───────────────────────────────────────────────────────────────────────
// The findings are sentences, so the rendering has to match the JS character for character or the
// equivalence is a comparison of verdicts only — which this repository's standard says has lost the useful
// half.

/// `String(v)`, for the values a JSON report can carry.
pub fn js(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => js_num(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => s.clone(),
        // `String([1,2])` is `"1,2"`, and a null/undefined element renders as the empty string.
        Value::Array(a) => a
            .iter()
            .map(|e| match e {
                Value::Null => String::new(),
                other => js(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `String(v)` where `v` is a property that may be ABSENT — which is not the same as `null` in JS, and the
/// difference is printed: `${s.h1Count}` is `undefined` for a missing key and `null` for a null one.
pub fn jsf(v: Option<&Value>) -> String {
    v.map(js).unwrap_or_else(|| "undefined".to_string())
}

/// `String(Number)`. JS prints an integral double without a decimal point and everything else in its
/// shortest round-tripping form.
pub fn js_num(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if x == 0.0 {
        return "0".to_string(); // -0 renders as "0"
    }
    if x.fract() == 0.0 && x.abs() < 9.0e15 {
        return format!("{}", x as i64);
    }
    format!("{x}")
}

/// `String(v).slice(0, n)` — BY UTF-16 CODE UNITS, which is what `slice` counts: an astral character is two
/// of them. A Rust `chars().take(n)` would cut a different string, and these slices end up inside findings.
pub fn js_slice(s: &str, n: usize) -> String {
    String::from_utf16_lossy(&s.encode_utf16().take(n).collect::<Vec<u16>>())
}

/// JS truthiness. An empty array and an empty object are TRUTHY; `0`, `""`, `null`, `undefined` and `NaN`
/// are not.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|x| x != 0.0 && !x.is_nan()).unwrap_or(false),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// `Number(v)` — the COERCING form, for `<`, `>` and `Math.min`. `undefined` is `NaN`; `null` is 0.
fn number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                0.0
            } else {
                t.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        Some(Value::Array(_)) | Some(Value::Object(_)) => f64::NAN,
    }
}

/// `=== n` against a number — STRICT, so the string `"1"` is not 1 and an absent key is not 1 either.
fn strict_num_eq(v: Option<&Value>, n: f64) -> bool {
    matches!(v, Some(Value::Number(x)) if x.as_f64() == Some(n))
}

/// `=== false` / `=== true` — strict, so a missing key is neither.
fn is_false(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Bool(false)))
}
fn is_true(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Bool(true)))
}
/// `typeof v === "number"`.
fn is_num(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Number(_)))
}

/// `(v || [])` for an array-valued property.
fn list(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// `(v || "?")` — the `||` form, so an empty string falls through too.
fn or_q(v: Option<&Value>) -> String {
    if truthy(v) {
        jsf(v)
    } else {
        "?".to_string()
    }
}

/// `list.join(sep)` — every element through `String()`.
fn join(v: &[Value], sep: &str) -> String {
    v.iter().map(js).collect::<Vec<_>>().join(sep)
}

/// `String(v).includes(needle)` for the `RegExp.test` calls whose patterns are literal substrings.
fn contains(v: Option<&Value>, needle: &str) -> bool {
    jsf(v).contains(needle)
}

// ── THE OPTIONS A SWEEP JUDGES ITS REPORT WITH ───────────────────────────────────────────────────────

/// One entry of `opts.ignore`: an exemption with the reason it is allowed to exist.
///
/// `contains` holds the literal substrings the finding's text must ALL carry — the shape today's one rule
/// uses (`/div\.rail-dot/.test(text) && /2\.33/.test(text)`). An empty list matches every finding, which is
/// the honest reading of a rule with no patterns rather than a silent no-op.
#[derive(Debug, Clone, PartialEq)]
pub struct IgnoreRule {
    pub contains: Vec<String>,
    pub reason: String,
    /// `dormant` declares "this is a GUARD for a state that currently passes" — an entry expected to match
    /// nothing in a clean run, so the unused-exemption note reports it as declared instead of as weight.
    pub dormant: Option<String>,
}

/// The options `judgeReport(report, opts)` takes, as data.
///
/// `proseFloor` is normalised at parse time: the JS guard is `if (opts.proseFloor)`, so a 0 disables the
/// clause rather than failing every line.
#[derive(Debug, Clone, Default)]
pub struct JudgeOptions {
    /// Page names (matched by PREFIX) that have no navigation BY DESIGN.
    pub navless: Vec<String>,
    /// Characters per rendered line above which a block of prose is a finding.
    pub prose_floor: Option<f64>,
    /// Floor under the number of styled classes a sheet scan must have found.
    pub unstyled_floor: Option<f64>,
    /// Fraction of rows allowed to be unmeasurable before the absences prove nothing.
    pub unmeasurable_floor: Option<f64>,
    /// Classes that are unstyled ON PURPOSE, each with the reason printed rather than hidden. ORDERED,
    /// because the note that reports the unused ones prints them in this order.
    pub implicit_states: Vec<(String, String)>,
    pub ignore: Vec<IgnoreRule>,
}

impl JudgeOptions {
    /// Read the options out of the JSON a sweep would pass.
    ///
    /// `implicitStates` comes back SORTED rather than in the object's written order — see the module
    /// header. Build the [`Vec`] by hand when the order matters.
    pub fn from_json(v: Option<&Value>) -> Self {
        let get = |k: &str| v.and_then(|o| o.get(k));
        let floor = |k: &str| {
            let x = get(k);
            if truthy(x) {
                x.and_then(Value::as_f64)
            } else {
                None
            }
        };
        Self {
            navless: list(get("navless")).iter().map(js).collect(),
            prose_floor: floor("proseFloor"),
            unstyled_floor: floor("unstyledFloor"),
            unmeasurable_floor: floor("unmeasurableFloor"),
            implicit_states: get("implicitStates")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .map(|(k, val)| (k.clone(), js(val)))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            ignore: list(get("ignore"))
                .iter()
                .map(|r| IgnoreRule {
                    contains: list(r.get("contains")).iter().map(js).collect(),
                    reason: jsf(r.get("reason")),
                    dormant: r.get("dormant").filter(|d| truthy(Some(*d))).map(js),
                })
                .collect(),
        }
    }

    /// `opts.implicitStates[c]`, which the JS reads as a plain property lookup: the value is the REASON and
    /// it must be truthy for the class to be waived.
    fn implicit_reason(&self, class: &str) -> Option<&str> {
        self.implicit_states
            .iter()
            .find(|(c, _)| c == class)
            .map(|(_, why)| why.as_str())
    }
}

/// What a judged report yields.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JudgeOutcome {
    /// The findings the run must fail on, in the order the JS builds them. Exemptions have already been
    /// applied; the ones that were used are reported in `lines`.
    pub findings: Vec<String>,
    /// Every line the JS judge prints with `console.log`, verbatim and in order — notes included. This is
    /// the half an equivalence that compares verdicts alone would lose.
    pub lines: Vec<String>,
    /// The lines the JS judge pushes into `report.notes` instead of printing (see the module header).
    pub report_notes: Vec<String>,
}

/// The `CLOCKS` table: elements that render a live duration, exempt by name with the reason in the table
/// rather than in a regex nobody can audit.
const CLOCKS: &[(&str, ClockMatch)] = &[
    (
        "^span\\.approval-left$",
        ClockMatch::Exact("span.approval-left"),
    ),
    (
        "^span\\.cmd-duration$",
        ClockMatch::Exact("span.cmd-duration"),
    ),
    ("^span\\.traj-", ClockMatch::Prefix("span.traj-")),
    (
        "^\\.details-duration$",
        ClockMatch::Exact(".details-duration"),
    ),
];

enum ClockMatch {
    Exact(&'static str),
    Prefix(&'static str),
}

fn clock_of(name: &str) -> Option<&'static str> {
    CLOCKS
        .iter()
        .find(|(_, m)| match m {
            ClockMatch::Exact(s) => name == *s,
            ClockMatch::Prefix(s) => name.starts_with(s),
        })
        .map(|(why, _)| *why)
}

/// The `NAV_LOUD` table: elements allowed to be loud because they ARE the page's state or its action.
fn nav_loud(entry: &str) -> bool {
    entry.contains("rail-btn")            // /rail-btn|desktop-rail-btn/
        || entry.starts_with("div.tab")   // /^div\.tab/
        || entry.starts_with("button.dtab") // /^button\.dtab/
        || entry.contains("btn-new")
        || entry.contains("approval-approve")
}

/// THE JUDGE. Reads a sweep report and returns the findings, the printed lines and the report notes.
///
/// `report` is `&Value` rather than `&mut Value` on purpose: the JS judge writes into `report.notes` and
/// nothing reads it (see the module header), so this hands those lines back instead of mutating its input.
pub fn judge_report(report: &Value, opts: &JudgeOptions) -> JudgeOutcome {
    let mut findings: Vec<String> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut report_notes: Vec<String> = Vec::new();
    let mut suppressed: Vec<(String, String)> = Vec::new();

    // ── SURFACES ─────────────────────────────────────────────────────────────────────────────────────
    for s in list(report.get("surfaces")) {
        let where_ = if truthy(s.get("width")) {
            format!("{}@{}px", jsf(s.get("page")), jsf(s.get("width")))
        } else {
            jsf(s.get("page"))
        };
        if !strict_num_eq(s.get("h1Count"), 1.0) || !truthy(s.get("firstIsH1")) {
            findings.push(format!(
                "{where_}: h1 count {}, first-is-h1 {}",
                jsf(s.get("h1Count")),
                jsf(s.get("firstIsH1"))
            ));
        }
        if truthy(s.get("skipped")) {
            findings.push(format!(
                "{where_}: {} skipped heading level(s)",
                jsf(s.get("skipped"))
            ));
        }
        if !strict_num_eq(s.get("mains"), 1.0) {
            findings.push(format!(
                "{where_}: {} main landmark(s), expected exactly 1",
                jsf(s.get("mains"))
            ));
        }
        if number(s.get("navs")) > 1.0 {
            findings.push(format!(
                "{where_}: {} nav landmarks — a page has one navigation",
                jsf(s.get("navs"))
            ));
        } else if !strict_num_eq(s.get("navs"), 1.0)
            && !opts
                .navless
                .iter()
                .any(|n| jsf(s.get("page")).starts_with(n.as_str()))
        {
            // PREFIX, NOT EXACT: a navless entry names a FAMILY of renders — the same page in another
            // theme, at another width — and matching the name alone is the one-of-N shape this session
            // keeps finding.
            findings.push(format!(
                "{where_}: {} nav landmark(s), expected exactly 1",
                jsf(s.get("navs"))
            ));
        }
        for (kind, key) in [
            ("overflow", "over"),
            ("clipping", "clipped"),
            ("sliver", "slivers"),
        ] {
            let l = list(s.get(key));
            if !l.is_empty() {
                findings.push(format!("{where_}: {kind} — {}", join(l, "; ")));
            }
        }
        if let Some(floor) = opts.prose_floor {
            for row in list(s.get("measure").and_then(|m| m.get("worst"))) {
                if number(row.get("cpl")) > floor {
                    // "PER LINE", NOT "ON ONE LINE": `cpl` is chars over rendered lines, so a block that
                    // wraps three times can still be over the floor.
                    findings.push(format!(
                        "{where_}: {} renders {} characters PER LINE ({} chars in {} line(s) over {}px at {}px, max-width {}) — past {} a reader loses the line return; this sheet's own ledes cap at 66ch",
                        jsf(row.get("sel")),
                        jsf(row.get("cpl")),
                        jsf(row.get("chars")),
                        jsf(row.get("lines")),
                        jsf(row.get("w")),
                        jsf(row.get("fs")),
                        jsf(row.get("maxw")),
                        js_num(floor),
                    ));
                }
            }
        }
        if let Some(marks) = s.get("marks") {
            let ring_fill = list(marks.get("ringFill"));
            if !ring_fill.is_empty() {
                findings.push(format!(
                    "{where_}: {} mark(s) are a FILL inside a RING — the vocabulary is solid / ring / halo / empty, and a mark that is two of them is neither: {}",
                    ring_fill.len(),
                    join(ring_fill, "; ")
                ));
            }
            let collisions = list(marks.get("collisions"));
            if !collisions.is_empty() {
                findings.push(format!(
                    "{where_}: states of one mark paint identically — {}",
                    join(collisions, "; ")
                ));
            }
        }
        if truthy(s.get("loudUnreadable")) {
            // REPORTED, NOT FAILED — and, as the module header records, reported to NOBODY in the JS.
            let samples = list(s.get("loudUnreadableSamples"))
                .iter()
                .take(2)
                .map(js)
                .collect::<Vec<_>>()
                .join("; ");
            report_notes.push(format!(
                "{where_}: {} background(s) in a colour syntax this probe cannot read — skipped, not counted{}",
                jsf(s.get("loudUnreadable")),
                if samples.is_empty() {
                    String::new()
                } else {
                    format!(" ({samples})")
                }
            ));
        }
        // ONE FOCAL POINT, AT MOST — with the exception named by ELEMENT rather than by page, because a
        // page-name exception cannot widen to a page that starts shouting about something else.
        let loud = list(s.get("loud"));
        let nav = loud.iter().all(|e| nav_loud(&js(e)));
        if loud.len() > 1 && !nav {
            findings.push(format!(
                "{where_}: {} loud elements — a page has ONE focal point at most — {}",
                loud.len(),
                join(loud, "; ")
            ));
        }
    }

    // ── ACCESSIBLE NAMES ─────────────────────────────────────────────────────────────────────────────
    for n in list(report.get("names")) {
        let where_ = format!(
            "{}{}",
            jsf(n.get("page")),
            if truthy(n.get("width")) {
                format!("@{}px", jsf(n.get("width")))
            } else {
                String::new()
            }
        );
        let unnamed = list(n.get("unnamed"));
        if !unnamed.is_empty() {
            findings.push(format!(
                "{where_}: {} control(s) with NO accessible name — {}",
                unnamed.len(),
                join(unnamed, ", ")
            ));
        }
        let title_only = list(n.get("titleOnly"));
        if !title_only.is_empty() {
            findings.push(format!(
                "{where_}: {} control(s) named only by title — {}",
                title_only.len(),
                join(title_only, ", ")
            ));
        }
    }

    // ── PROVENANCE ───────────────────────────────────────────────────────────────────────────────────
    // WHICH HARNESS GENERATION WAS MEASURED. A fixture older than the build it should match reports
    // findings that look live, so the stamp travels in the report and is printed here.
    if truthy(report.get("harnessBuild")) {
        lines.push(format!(
            "note: harness build {}",
            jsf(report.get("harnessBuild"))
        ));
    }
    if let Some(e) = report.get("entryCheck").filter(|v| truthy(Some(*v))) {
        if !truthy(e.get("stale")) && !truthy(e.get("error")) {
            lines.push(format!(
                "note: delivered entry {} bytes / sha {} — matches the build",
                jsf(e.get("bytes")),
                jsf(e.get("sha"))
            ));
        }
        if truthy(e.get("stale")) {
            findings.push(if truthy(e.get("error")) {
                format!(
                    "the delivered entry could not be read ({}) — expected {} bytes, sha {}",
                    jsf(e.get("error")),
                    jsf(e.get("expected").and_then(|x| x.get("bytes"))),
                    jsf(e.get("expected").and_then(|x| x.get("sha")))
                )
            } else {
                format!(
                    "the delivered entry is {} bytes / sha {} but this sweep was emitted against {} / {} — every measurement below is of a stale build",
                    jsf(e.get("bytes")),
                    jsf(e.get("sha")),
                    jsf(e.get("expected").and_then(|x| x.get("bytes"))),
                    jsf(e.get("expected").and_then(|x| x.get("sha")))
                )
            });
        }
    }
    if truthy(report.get("harnessStale")) {
        findings.push(format!(
            "the harness is build {} but this sweep was emitted against {} — every measurement below is of a stale fixture",
            jsf(report.get("harnessBuild")),
            jsf(report.get("expectedHarnessBuild"))
        ));
    }

    // ── FOCUS ────────────────────────────────────────────────────────────────────────────────────────
    let focus = list(report.get("focus"));
    let paint_total: f64 = focus
        .iter()
        .map(|f| {
            if truthy(f.get("paintConfirmed")) {
                number(f.get("paintConfirmed"))
            } else {
                0.0
            }
        })
        .sum();
    let pressed_total: f64 = focus
        .iter()
        .map(|f| {
            if truthy(f.get("pressed")) {
                number(f.get("pressed"))
            } else {
                0.0
            }
        })
        .sum();
    if paint_total != 0.0 {
        lines.push(format!(
            "note: the pixels overruled the computed-style focus verdict {} time(s) of {} press(es) — the rings are painted, the style check cannot see them",
            js_num(paint_total),
            js_num(pressed_total)
        ));
    }
    for f in focus {
        let page = if truthy(f.get("page")) {
            format!("{}: ", jsf(f.get("page")))
        } else {
            String::new()
        };
        if truthy(f.get("unconfirmed")) {
            let on = list(f.get("unconfirmedOn"));
            findings.push(format!(
                "{page}{} focus candidate(s) could not be confirmed against the pixels{} — the check could not look, so it cannot say",
                jsf(f.get("unconfirmed")),
                if on.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", join(on, ", "))
                }
            ));
        }
        if truthy(f.get("missing")) {
            findings.push(format!(
                "{page}{} Tab stop(s) with no visible focus ring",
                jsf(f.get("missing"))
            ));
        }
        // A RUN THAT LANDED NOWHERE IS NOT A PASSING RUN.
        if number(f.get("pressed")) > 0.0 && strict_num_eq(f.get("landed"), 0.0) {
            findings.push(format!(
                "{}{}: focus landed on nothing in {} Tab press(es) ({} escaped to the body) — that is a report of no focusable targets, not of good focus rings",
                if truthy(f.get("density")) { jsf(f.get("density")) } else { String::new() },
                if truthy(f.get("theme")) { format!("/{}", jsf(f.get("theme"))) } else { String::new() },
                jsf(f.get("pressed")),
                jsf(f.get("escaped")),
            ));
        }
    }

    // ── THE PRESS ────────────────────────────────────────────────────────────────────────────────────
    // A PRESS THAT RENDERS NOTHING IS A CONTROL THAT DOES NOT ANSWER — and ONLY WHERE THE POINTER
    // ARRIVED: a row the pass could not deliver a press to is not a control that ignored one, which is the
    // distinction that matters when an element sits below the fold and the criterion that keeps this clause
    // from biting the case it is not about.
    for row in list(report.get("press")) {
        // THE LABEL NAMES THE ITERATION, not just the surface: density/theme alone made a finding from the
        // rail walk indistinguishable from one from the mode loop.
        let where_ = format!(
            "{}/{}{}{}",
            or_q(row.get("density")),
            or_q(row.get("theme")),
            if truthy(row.get("mode")) {
                format!(" {}", jsf(row.get("mode")))
            } else {
                String::new()
            },
            if truthy(row.get("page")) {
                format!(" {}", jsf(row.get("page")))
            } else {
                String::new()
            }
        );
        let rows = list(row.get("rows"));
        for d in rows
            .iter()
            .filter(|r| is_false(r.get("changed")) && !is_false(r.get("reached")))
        {
            findings.push(format!(
                "{where_}: {} ({}) renders NOTHING when pressed — before and during are identical ({})",
                jsf(d.get("sel")),
                jsf(d.get("where")),
                jsf(d.get("size"))
            ));
        }
        for r in rows {
            if truthy(r.get("note")) && !contains(r.get("note"), "not rendered") {
                lines.push(format!(
                    "note: {where_} {} — {}",
                    jsf(r.get("sel")),
                    jsf(r.get("note"))
                ));
            }
        }
        // A PASS THAT PRESSED NOTHING IS NOT A CLEAN PASS — AND THE FLOOR IS WHAT THE PAGE HAS, not a
        // constant. A curated list expects several of its selectors, so two is the bar; a DISCOVERED pass
        // reports how many controls the page had, and ONE control pressed is a complete pass on a page that
        // has one.
        let found = row.get("found");
        let found_null = found.is_none() || found == Some(&Value::Null);
        let floor = if found_null {
            2.0
        } else {
            number(found).min(2.0)
        };
        if !found_null && number(found) > rows.len() as f64 {
            lines.push(format!(
                "note: {where_} has {} control(s) and the pass pressed the first {} (cap) — the rest were not measured",
                jsf(found),
                rows.len()
            ));
        }
        let measured = if truthy(row.get("measured")) {
            number(row.get("measured"))
        } else {
            0.0
        };
        if measured < floor {
            findings.push(format!(
                "{where_}: the press pass measured {} control(s){} — a press pass that pressed nothing proves nothing",
                if truthy(row.get("measured")) {
                    jsf(row.get("measured"))
                } else {
                    "0".to_string()
                },
                if found_null {
                    String::new()
                } else {
                    format!(" of the {} this page renders", jsf(found))
                }
            ));
        }
    }

    // ── THE ACKNOWLEDGEMENT ──────────────────────────────────────────────────────────────────────────
    // IMMEDIATE FEEDBACK HAS A BUDGET, and the clause belongs to the REPORT SHAPE rather than to a UI: it
    // lived in the panel's sweep once, so the console's rows — a third of that sweep's runtime — were
    // judged by nothing at all.
    let ack = list(report.get("ack"));
    for a in ack {
        let where_ = format!(
            "{}{}",
            or_q(a.get("density")),
            if truthy(a.get("page")) {
                format!(" {}", jsf(a.get("page")))
            } else {
                String::new()
            }
        );
        // AN UNMEASURED PREMISE IS NOT AN EXCUSE: only a page that HAS a request counter can tell "asked
        // nothing" from "nobody looked", so its absence is a FAILURE rather than a footnote.
        if is_false(a.get("hasCounter")) {
            findings.push(format!(
                "{where_}: {} was pressed on a page with NO request counter, so whether it asked the device anything is UNMEASURED — the acknowledgement axis proves nothing here",
                jsf(a.get("sel"))
            ));
            continue;
        }
        if truthy(a.get("note")) {
            lines.push(format!(
                "note: {where_} {} — {}",
                jsf(a.get("sel")),
                jsf(a.get("note"))
            ));
            continue;
        }
        if !truthy(a.get("acked")) {
            // ONLY WHERE THERE WAS SOMETHING TO WAIT FOR: a control that asked the device nothing (a tab
            // switching a snippet, a disclosure) cannot be late.
            if is_false(a.get("asked")) {
                lines.push(format!(
                    "note: {where_} {} — asked the device nothing, so there was nothing to acknowledge",
                    jsf(a.get("sel"))
                ));
                continue;
            }
            // WHAT THIS CAN HONESTLY CLAIM: the control never acknowledged the press in the window.
            // Whether it asked the device is NOT attributable from a request counter on a page that polls
            // for its own reasons, so the finding does not say it did.
            findings.push(format!(
                "{where_}: {} ({}) never acknowledged the press — no busy state and no painted change within the window ({})",
                jsf(a.get("sel")),
                jsf(a.get("where")),
                jsf(a.get("size"))
            ));
            continue;
        }
        // EVERY ROW'S NUMBERS, ON EVERY RUN: the judge reports only the failures, so a CI-only failure
        // could not be compared with a clean device run without re-running both by hand.
        lines.extend(ack_notes(std::slice::from_ref(a), &where_));
        if is_num(a.get("msToAck"))
            && is_num(a.get("budgetMs"))
            && number(a.get("msToAck")) > number(a.get("budgetMs"))
        {
            findings.push(format!(
                "{where_}: {} ({}) acknowledged the press after {}ms — the budget is {}ms, so this feedback waited on the {}ms network round trip instead of firing on the event",
                jsf(a.get("sel")),
                jsf(a.get("where")),
                jsf(a.get("msToAck")),
                jsf(a.get("budgetMs")),
                jsf(a.get("msToClear"))
            ));
        }
    }
    {
        let acked = ack.iter().filter(|a| truthy(a.get("acked"))).count();
        if !ack.is_empty() && acked == 0 {
            findings.push(format!(
                "the acknowledgement pass measured {} control(s) and NONE acknowledged — a pass that proves nothing is not a pass",
                ack.len()
            ));
        }
    }

    // ── TARGET SIZE, IN ONE PLACE (WCAG 2.5.8) ───────────────────────────────────────────────────────
    // It lived in THREE judges with three slightly different sentences, so one page judged by two sweeps
    // got two verdicts. THE TWO HALVES, and why containment is not one of them: `passesBySpacing` is
    // CENTRE-to-centre, and a nested control's circle is inside its container by construction — that is
    // nested interactive content, a different question, NAMED below rather than enforced.
    for t in list(report.get("targets")) {
        let where_ = {
            let parts = ["density", "page", "mode"]
                .iter()
                .filter(|k| truthy(t.get(*k)))
                .map(|k| jsf(t.get(*k)))
                .collect::<Vec<_>>();
            if parts.is_empty() {
                "?".to_string()
            } else {
                parts.join(" ")
            }
        };
        let distinct = list(t.get("distinct"));
        for u in distinct {
            if !truthy(u.get("passesBySpacing")) {
                findings.push(format!(
                    "target size ({where_}): {} is {}x{} and its nearest neighbour is {}px away — 2.5.8 wants 24x24 or 24px of spacing (\"{}\")",
                    jsf(u.get("sel")),
                    jsf(u.get("w")),
                    jsf(u.get("h")),
                    jsf(u.get("nearest")),
                    jsf(u.get("text"))
                ));
                continue;
            }
            if is_false(u.get("passesByFullRule")) {
                findings.push(format!(
                    "target size ({where_}): {} is {}x{} and its 24px circle reaches {} ({}x{}) {}px away — the spacing clause wants the centre 12px clear of another target's BOX, not only 24px from its centre (\"{}\")",
                    jsf(u.get("sel")),
                    jsf(u.get("w")),
                    jsf(u.get("h")),
                    jsf(u.get("nearSel")),
                    jsf(u.get("nearW")),
                    jsf(u.get("nearH")),
                    jsf(u.get("gapToBox")),
                    jsf(u.get("text"))
                ));
            }
        }
        let nested: Vec<&Value> = distinct
            .iter()
            .filter(|u| truthy(u.get("insideSel")))
            .collect();
        if !nested.is_empty() {
            lines.push(format!(
                "note: target size ({where_}): {} of {} undersized target(s) sit INSIDE another target — not a spacing failure (the circle is inside its container by construction), but a near-miss there activates the container: {}",
                nested.len(),
                distinct.len(),
                nested
                    .iter()
                    .map(|u| format!(
                        "{} {}x{} in {} {}x{}",
                        jsf(u.get("sel")),
                        jsf(u.get("w")),
                        jsf(u.get("h")),
                        jsf(u.get("insideSel")),
                        jsf(u.get("insideW")),
                        jsf(u.get("insideH"))
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }

    // ── A SURFACE MAY NOT CLAIM A READ FAILED WHEN THE FIXTURE ANSWERED EVERY CALL ───────────────────
    // THE PREMISE HAS TO HOLD BEFORE THE CLAUSE APPLIES: a claim is only FALSE where a fixture ANSWERED.
    // The console has no backend at all — its sweep serves a static build, every API call fails for real,
    // and "could not be read" there is TRUE — so a report that declares nothing about what it served is not
    // judged on this axis, and says so the first time it runs.
    let sse = list(report.get("sse"));
    let rejected: Vec<String> = sse
        .iter()
        .filter(|s| truthy(s.get("fail")))
        .map(|s| jsf(s.get("page")))
        .collect();
    let fixture_evidence = !sse.is_empty();
    if !fixture_evidence
        && list(report.get("surfaces"))
            .iter()
            .any(|s| !list(s.get("claims")).is_empty())
    {
        lines.push("note: read-failure claims are NOT judged here — this report declares nothing about what its fixture served, so the clause has no premise (the console has no backend)".to_string());
    }
    let mut excused: Vec<String> = Vec::new();
    let judged: &[Value] = if fixture_evidence {
        list(report.get("surfaces"))
    } else {
        &[]
    };
    for s in judged {
        let claims = list(s.get("claims"));
        if claims.is_empty() {
            continue;
        }
        if rejected.contains(&jsf(s.get("page"))) {
            for c in claims {
                excused.push(format!("{}: {}", jsf(s.get("page")), js(c)));
            }
            continue;
        }
        for c in claims {
            findings.push(format!(
                "{} claims a read failed — \"{}\" — while the fixture answered every call: either the panel is wrong about the device, or the fixture never stubbed an endpoint the card needs",
                jsf(s.get("page")),
                js(c)
            ));
        }
    }
    if !excused.is_empty() {
        lines.push(format!(
            "note: {} read-failure claim(s) on surfaces whose fixture REJECTS every call — true by construction:",
            excused.len()
        ));
        let mut seen: Vec<&String> = Vec::new();
        for w in &excused {
            if !seen.contains(&w) {
                seen.push(w);
            }
        }
        for w in seen.iter().take(4) {
            lines.push(format!("  {w}"));
        }
    }

    // ── IDLE REPAINT ─────────────────────────────────────────────────────────────────────────────────
    // DOM mutations on a settled page under STATIC fixtures: React writes to the DOM only when the output
    // differs, so nothing changing means nothing written. The observer proves it is alive by seeing one
    // deliberate mutation of the panel's own root, and that is REQUIRED — a blind observer reports a
    // perfectly still panel forever.
    for row in list(report.get("idle")) {
        let where_ = format!(
            "{}/{}/{}",
            or_q(row.get("density")),
            or_q(row.get("theme")),
            or_q(row.get("page"))
        );
        let by_target = row.get("byTarget").and_then(Value::as_object);
        let probe = by_target
            .and_then(|m| m.get("__probe"))
            .filter(|v| truthy(Some(v)))
            .map(|v| number(Some(v)))
            .unwrap_or(0.0);
        let real = if truthy(row.get("mutations")) {
            number(row.get("mutations"))
        } else {
            0.0
        } - probe;
        if probe < 1.0 {
            findings.push(format!(
                "{where_}: the idle observer did not see its own probe mutation — a blind instrument reports a still panel forever, so this measurement proves nothing"
            ));
        }
        if real > 0.0 {
            let all: Vec<(&String, &Value)> = by_target
                .map(|m| m.iter().filter(|(k, _)| k.as_str() != "__probe").collect())
                .unwrap_or_default();
            // A CLOCK IS NOT A REPAINT: a value that is SUPPOSED to change, exempt by name with a reason
            // each — a table cannot quietly grow the way a regex can.
            let repaints: Vec<&(&String, &Value)> =
                all.iter().filter(|(k, _)| clock_of(k).is_none()).collect();
            let clocks: Vec<&(&String, &Value)> =
                all.iter().filter(|(k, _)| clock_of(k).is_some()).collect();
            if !clocks.is_empty() {
                report_notes.push(format!(
                    "{where_}: {} changed while idle — a live duration, exempt by name with its reason in CLOCKS",
                    clocks
                        .iter()
                        .map(|(k, v)| format!("{k} x{}", js(v)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !repaints.is_empty() {
                let targets = repaints
                    .iter()
                    .map(|(k, v)| format!("{k} x{}", js(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let total: f64 = repaints.iter().map(|(_, v)| number(Some(v))).sum();
                findings.push(format!(
                    "{where_}: {} DOM mutation(s) in {}s while idle — nothing changed under static fixtures, so this is a repaint of unchanged output ({targets})",
                    js_num(total),
                    jsf(row.get("seconds"))
                ));
            }
        }
    }

    // ── REDUCED MOTION IS A CONTRACT, NOT A COURTESY ─────────────────────────────────────────────────
    for m in list(report.get("motion")) {
        let still = if truthy(m.get("stillAnimating")) {
            list(m.get("stillAnimating"))
        } else {
            list(m.get("animating"))
        };
        let density = or_q(m.get("density"));
        if !still.is_empty() {
            findings.push(format!(
                "reduced motion ({density}): {} element(s) still animate — {}",
                still.len(),
                join(&still[..still.len().min(3)], "; ")
            ));
        }
        // A CHECK THAT FOUND NOTHING TO SUPPRESS PROVES NOTHING.
        if is_num(m.get("normal")) && strict_num_eq(m.get("normal"), 0.0) {
            findings.push(format!(
                "reduced motion ({density}): 0 elements animate WITHOUT the preference, so this result proves nothing about prefers-reduced-motion — the check found nothing to suppress"
            ));
        }
    }

    // ── A CLASS THE PAGE RENDERS THAT NO RULE STYLES ─────────────────────────────────────────────────
    let declared_implicit: Vec<&String> = opts.implicit_states.iter().map(|(c, _)| c).collect();
    let mut seen_implicit: Vec<&String> = Vec::new();
    for u in list(report.get("unstyled")) {
        for c in list(u.get("classes")) {
            let name = js(c);
            if opts.implicit_reason(&name).is_some() && !seen_implicit.iter().any(|s| **s == name) {
                seen_implicit.push(
                    declared_implicit
                        .iter()
                        .copied()
                        .find(|d| **d == name)
                        .expect("declared_implicit holds every key implicit_reason matched"),
                );
            }
        }
    }
    for u in list(report.get("unstyled")) {
        let all = list(u.get("classes"));
        let page = if truthy(u.get("page")) {
            jsf(u.get("page"))
        } else {
            "?".to_string()
        };
        for c in all {
            let name = js(c);
            if let Some(why) = opts.implicit_reason(&name) {
                lines.push(format!("note: {name} is unstyled by design — {why}"));
            }
        }
        // THE WORDING MATTERS: a class with no matching rule is NOT an unstyled element. What is true — and
        // what is worth failing on — is narrower: this name is on screen and nothing matches it, so either
        // it is an inert extra to prune or it needs a reason to stay.
        let live: Vec<String> = all
            .iter()
            .map(js)
            .filter(|c| opts.implicit_reason(c).is_none())
            .collect();
        if !live.is_empty() {
            findings.push(format!(
                "class name(s) with no matching rule on {page}: {} — on screen, matched by nothing",
                live.join(", ")
            ));
        }
        // A READ THAT FOUND NO STYLESHEETS PROVES NOTHING — and so does one that read almost everything:
        // the floor catches the first, this catches the second.
        if number(u.get("sheetsUnreadable")) > 0.0 {
            findings.push(format!(
                "unstyled check on {page}: {} stylesheet(s) could not be read, so their classes look unstyled — the basis is incomplete",
                jsf(u.get("sheetsUnreadable"))
            ));
        }
        let floor = opts.unstyled_floor.unwrap_or(100.0);
        if is_num(u.get("styledClasses")) && number(u.get("styledClasses")) < floor {
            findings.push(format!(
                "unstyled check on {page}: only {} styled classes found (floor {}) — the collector read almost nothing, so its silence means nothing",
                jsf(u.get("styledClasses")),
                js_num(floor)
            ));
        }
    }
    {
        let unused: Vec<&&String> = declared_implicit
            .iter()
            .filter(|c| !seen_implicit.contains(c))
            .collect();
        if !unused.is_empty() {
            let unstyled = list(report.get("unstyled"));
            let classes: usize = unstyled.iter().map(|u| list(u.get("classes")).len()).sum();
            lines.push(format!(
                "note: {} of {} declared unstyled-by-design class(es) were not seen in this run ({} unstyled name(s) over {} page(s)) — a reason nothing needs is weight; prune it or say why it stays:",
                unused.len(),
                declared_implicit.len(),
                classes,
                unstyled.len()
            ));
            for c in unused {
                lines.push(format!(
                    "  {c} — {}",
                    opts.implicit_reason(c).unwrap_or_default()
                ));
            }
        }
    }

    // ── THE TYPE FLOOR, IN THE RENDERED PAGE ─────────────────────────────────────────────────────────
    // A token being 10px and the rendered text being 10px are different claims, and only the second one is
    // what a reader experiences.
    let rows = list(report.get("rows"));
    for r in rows {
        if jsf(r.get("kind")) == "graphic" {
            continue;
        }
        if is_num(r.get("size")) && number(r.get("size")) > 0.0 && number(r.get("size")) < 10.0 {
            findings.push(format!(
                "type floor: {} renders at {}px on {} — the scale's floor is 10px (\"{}\")",
                jsf(r.get("sel")),
                jsf(r.get("size")),
                if truthy(r.get("page")) {
                    jsf(r.get("page"))
                } else {
                    "?".to_string()
                },
                js_slice(
                    &if truthy(r.get("text")) {
                        jsf(r.get("text"))
                    } else {
                        String::new()
                    },
                    24
                )
            ));
        }
    }
    // A SCAN THAT COULD NOT READ MOST OF THE PAGE IS NOT A CLEAN SCAN. Rows the probe cannot measure are
    // excluded from judgement — correctly, since guessing at them is how a probe starts lying — but an
    // excluded row must not be able to make a report exit 0 while having judged almost nothing.
    {
        let blind = rows
            .iter()
            .filter(|r| match r.get("cr") {
                None | Some(Value::Null) => true,
                Some(Value::Number(n)) => n.as_f64().map(f64::is_nan).unwrap_or(false),
                _ => false,
            })
            .count();
        let floor = opts.unmeasurable_floor.unwrap_or(0.1);
        if !rows.is_empty() && blind as f64 / rows.len() as f64 > floor {
            // `toFixed(1)` / `toFixed(0)`, computed first because clippy refuses a `format!` inside a
            // `format!` — and it is right to: the nested call is invisible in the sentence.
            let pct = (blind as f64 / rows.len() as f64) * 100.0;
            let floor_pct = floor * 100.0;
            findings.push(format!(
                "only {} of {} rows could be measured ({pct:.1}% unmeasurable, floor {floor_pct:.0}%) — the absences below prove nothing",
                rows.len() - blind,
                rows.len(),
            ));
        }
    }
    for h in list(report.get("hover")) {
        let under = list(h.get("underAA"));
        if !under.is_empty() {
            findings.push(format!(
                "hover ({}/{}): {} element(s) below AA while hovered — {}",
                or_q(h.get("density")),
                or_q(h.get("theme")),
                under.len(),
                join(&under[..under.len().min(3)], "; ")
            ));
        }
    }

    // ── THE HARNESS DELIVERED THE PUSH, OR THE MEASUREMENT IS OF ANOTHER PANEL ───────────────────────
    // Two signals, one fact: the flag says what the fixture did, and the panel's own sentence says what it
    // concluded from it — so a fixture that lies about itself would have to lie in both places.
    for row in rows {
        if !jsf(row.get("text"))
            .to_lowercase()
            .contains("sessions unavailable")
        {
            continue;
        }
        let failed = sse
            .iter()
            .find(|r| jsf(r.get("page")) == jsf(row.get("page")))
            .map(|r| is_true(r.get("fail")))
            .unwrap_or(false);
        if failed {
            continue;
        }
        findings.push(format!(
            "{}: the panel renders \"Sessions unavailable\" and the harness did not report the failure fixture — this surface was measured with the push missing",
            if truthy(row.get("page")) { jsf(row.get("page")) } else { "?".to_string() }
        ));
    }
    for row in sse {
        if is_true(row.get("fail")) {
            continue;
        }
        if !is_true(row.get("opened")) {
            findings.push(format!(
                "{}: the harness never opened the SSE stream, so this surface was measured in the RECONNECTING state — the fixture failed, not the panel",
                if truthy(row.get("page")) { jsf(row.get("page")) } else { "?".to_string() }
            ));
        }
    }

    // ── REFLOW ───────────────────────────────────────────────────────────────────────────────────────
    for r in list(report.get("reflow")) {
        let width = jsf(r.get("width"));
        if truthy(r.get("docScrollsSideways")) {
            findings.push(format!(
                "reflow @{width}px: the document scrolls sideways ({} > {})",
                jsf(r.get("docScrollWidth")),
                jsf(r.get("viewport"))
            ));
        }
        let scrollers = list(r.get("sideScrollers"));
        if !scrollers.is_empty() {
            lines.push(format!(
                "note: scrollers at {width}px (allowed for toolbars) — {}",
                join(scrollers, "; ")
            ));
        }
        let overflowing = list(r.get("overflowing"));
        if truthy(r.get("docScrollsSideways")) && !overflowing.is_empty() {
            lines.push(format!(
                "note: the {width}px overflow comes from — {}",
                join(overflowing, "; ")
            ));
        }
        let spilling = list(r.get("spilling"));
        if !spilling.is_empty() {
            lines.push(format!(
                "note: content spilling out of its own box at {width}px — {}",
                join(spilling, "; ")
            ));
            // A NOTE CANNOT FAIL CI, so until this line a regression that drew content over the canvas
            // would have been reported and PASSED.
            findings.push(format!(
                "reflow @{width}px: content is spilling out of its own box — {}",
                join(spilling, "; ")
            ));
        }
        let clipped = list(r.get("clipped"));
        if !clipped.is_empty() {
            lines.push(format!(
                "note: content clipped with no way to reach it at {width}px — {}",
                join(clipped, "; ")
            ));
            findings.push(format!(
                "reflow @{width}px: content is clipped with no way to reach it — {}",
                join(clipped, "; ")
            ));
        }
        if truthy(r.get("term")) {
            let t = r.get("term").expect("truthy implies present");
            lines.push(format!(
                "note: the terminal at {width}px renders {} column(s) in a {}px container (cell {}px)",
                jsf(t.get("cols")),
                jsf(t.get("containerW")),
                jsf(t.get("cellW"))
            ));
        }
    }

    // ── EXEMPTIONS: WHICH ONES THIS RUN ACTUALLY NEEDED ──────────────────────────────────────────────
    // An exemption nothing needs is weight in the one list a reader consults, and the panel spent rounds
    // discovering that the hard way (a waiver for `/^div\.rail-dot$/` had matched nothing since a selector
    // changed). Counted here because this is the only place that knows.
    let mut used: Vec<usize> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    for f in &findings {
        let rule = opts
            .ignore
            .iter()
            .enumerate()
            .find(|(_, i)| i.contains.iter().all(|p| f.contains(p.as_str())));
        match rule {
            Some((idx, i)) => {
                if !used.contains(&idx) {
                    used.push(idx);
                }
                suppressed.push((f.clone(), i.reason.clone()));
            }
            None => kept.push(f.clone()),
        }
    }
    {
        // A NOTE, NOT A FINDING, and it names the size of the search: a run that measured one axis produces
        // none of the findings these entries exist for, and then every entry looks unused.
        let unused: Vec<&IgnoreRule> = opts
            .ignore
            .iter()
            .enumerate()
            .filter(|(i, _)| !used.contains(i))
            .map(|(_, r)| r)
            .collect();
        let undeclared: Vec<&&IgnoreRule> = unused.iter().filter(|r| r.dormant.is_none()).collect();
        let dormant: Vec<&&IgnoreRule> = unused.iter().filter(|r| r.dormant.is_some()).collect();
        if !undeclared.is_empty() {
            lines.push(format!(
                "note: {} of {} ignore entr(ies) matched none of this run's {} finding(s) and does not say why it stays — an exemption nothing needs is weight; prune it, or declare it dormant with the measurement that makes it a guard:",
                undeclared.len(),
                opts.ignore.len(),
                findings.len()
            ));
            for u in undeclared {
                lines.push(format!("  {}", u.reason));
            }
        }
        for d in dormant {
            lines.push(format!(
                "note: ignore entry dormant as declared — {}",
                d.dormant.as_deref().unwrap_or_default()
            ));
        }
    }
    for (finding, reason) in &suppressed {
        lines.push(format!("note: set aside ({reason}) — {finding}"));
    }

    JudgeOutcome {
        findings: kept,
        lines,
        report_notes,
    }
}

/// How a sweep reports its result, so three tools read the same way.
pub fn report_summary(label: &str, report: &Value) -> String {
    let surfaces = list(report.get("surfaces")).len();
    let rows = list(report.get("rows"));
    // STRICT `null` HERE, AND THE JUDGE'S OWN BLIND COUNT IS NOT — the judge also counts an ABSENT `cr`,
    // and the two numbers in one run can therefore disagree. Ported as written rather than harmonised: the
    // summary is a label and the floor is a criterion, and changing either is its own measurement.
    let blind = rows
        .iter()
        .filter(|r| r.get("cr") == Some(&Value::Null))
        .count();
    // THE WORST LINE OF PROSE, in the summary, because a floor that only SPEAKS when it is crossed tells a
    // reader nothing about how close the rest of the run is to it.
    let mut prose_cpl = 0.0;
    let mut prose_sel = String::new();
    for s in list(report.get("surfaces")) {
        if let Some(worst) = list(s.get("measure").and_then(|m| m.get("worst"))).first() {
            if truthy(worst.get("cpl")) && number(worst.get("cpl")) > prose_cpl {
                prose_cpl = number(worst.get("cpl"));
                prose_sel = jsf(worst.get("sel"));
            }
        }
    }
    let mut out = format!(
        "{label}: {} text nodes · {surfaces} surface(s) · {} name checks",
        rows.len(),
        list(report.get("names")).len()
    );
    if prose_cpl != 0.0 {
        out.push_str(&format!(
            " · worst prose line {} chars ({prose_sel})",
            js_num(prose_cpl)
        ));
    }
    if blind > 0 {
        out.push_str(&format!(" · {blind} unmeasurable"));
    }
    out
}

/// EVERY ACK ROW'S NUMBERS, AS LINES — one definition for both sweeps.
///
/// The panel printed these from its own copy and the console printed NONE, so the console's rows reached
/// `report.ack`, were judged, and were invisible: a run could show "0 findings" while saying nothing about
/// whether any control answered.
pub fn ack_notes(rows: &[Value], where_: &str) -> Vec<String> {
    let mut out = Vec::new();
    for a in rows {
        let label = format!(
            "{}{}{}",
            jsf(a.get("sel")),
            if truthy(a.get("where")) && a.get("where") != a.get("sel") {
                format!(" ({})", jsf(a.get("where")))
            } else {
                String::new()
            },
            if truthy(a.get("name")) {
                format!(" [{}]", jsf(a.get("name")))
            } else {
                String::new()
            }
        );
        let ms = match a.get("msToAck") {
            None | Some(Value::Null) => "-".to_string(),
            other => jsf(other),
        };
        // `a.presses ?? a.attempts ?? 1` — NULLISH, so a 0 survives where `||` would replace it.
        let presses = ["presses", "attempts"]
            .iter()
            .find_map(|k| a.get(*k).filter(|v| !v.is_null()))
            .map(js)
            .unwrap_or_else(|| "1".to_string());
        out.push(format!(
            "note: ack {where_} {label} — acked={} via={} ms={ms} budget={} presses={presses}",
            jsf(a.get("acked")),
            if truthy(a.get("via")) {
                jsf(a.get("via"))
            } else {
                "none".to_string()
            },
            jsf(a.get("budgetMs"))
        ));
        if truthy(a.get("acked")) && number(a.get("attempts")).max(1.0) > 1.0 {
            out.push(format!(
                "note: {where_} {label} acknowledged only on the SECOND press — the first sample saw nothing, which on a loaded machine is a timing artefact and on a real control is an acknowledgement that depends on state"
            ));
        }
        if is_num(a.get("msToAck"))
            && is_num(a.get("budgetMs"))
            && number(a.get("msToAck")) > number(a.get("budgetMs"))
        {
            out.push(format!(
                "note: {where_} {label} took {}ms against a {}ms budget",
                jsf(a.get("msToAck")),
                jsf(a.get("budgetMs"))
            ));
        }
    }
    out
}

/// THE STATES A SHEET DECLARES AND THIS RUN NEVER PAINTED.
///
/// `cmd-dot` rendered four of its six states for rounds, and the two that had no surface hid a missing rule
/// and a colour-only pair. The note is the queue those rounds worked from — and until it was shared, it
/// lived inside one sweep, so the console and the landing had no such queue at all.
///
/// `css_text` is the sheet whose state rules define the families; `report` is the sweep report whose
/// surfaces carry `marks.families` / `marks.present`.
pub fn mark_coverage_notes(css_text: &str, report: &Value) -> Vec<String> {
    let css = strip_block_comments(css_text);
    // Insertion-ordered, because the notes are printed in the order the families were first seen.
    let mut seen: Vec<(String, Vec<String>)> = Vec::new();
    for s in list(report.get("surfaces")) {
        for fam in list(s.get("marks").and_then(|m| m.get("families"))) {
            let fam = js(fam);
            let Some((family, states)) = split_family(&fam) else {
                continue;
            };
            let idx = match seen.iter().position(|(f, _)| *f == family) {
                Some(i) => i,
                None => {
                    seen.push((family.clone(), Vec::new()));
                    seen.len() - 1
                }
            };
            let slot = &mut seen[idx];
            for st in states.split(',') {
                if !st.is_empty() && !slot.1.iter().any(|x| x == st) {
                    slot.1.push(st.to_string());
                }
            }
        }
    }
    // THE CLASSES THE RUN PUT ON SCREEN, so a family with no painted states can be told apart from a family
    // the probe attributes elsewhere.
    let mut on_screen: Vec<String> = Vec::new();
    for s in list(report.get("surfaces")) {
        for c in list(s.get("marks").and_then(|m| m.get("present"))) {
            let name = js(c);
            if !on_screen.contains(&name) {
                on_screen.push(name);
            }
        }
    }
    // A CLASS NAMED `*-state` IS NOT AUTOMATICALLY A MARK: the naming rule is deliberately loose, and it
    // swept in three classes the sheet styles as a TEXT LINE. Declared with a reason rather than filtered by
    // a heuristic, because a heuristic would also hide the day one of them becomes a real mark.
    const TEXT_STATE_CLASSES: [&str; 3] = ["update-state", "notify-state", "monitor-state"];
    for (class, _) in css_class_state_rules(&css) {
        if !is_mark_family(&class) || TEXT_STATE_CLASSES.contains(&class.as_str()) {
            continue;
        }
        if !seen.iter().any(|(f, _)| *f == class) {
            seen.push((class, Vec::new()));
        }
    }

    let mut out = Vec::new();
    for (family, rendered) in &seen {
        let mut declared: Vec<String> = Vec::new();
        for (class, sel) in css_class_state_rules(&css) {
            if class != *family {
                continue;
            }
            let state = if let Some(inner) = sel.strip_prefix('[') {
                inner.trim_end_matches(']').replace('"', "")
            } else {
                sel.trim_start_matches('.').to_string()
            };
            if !declared.contains(&state) {
                declared.push(state);
            }
        }
        if rendered.is_empty() && on_screen.contains(family) {
            out.push(format!(
                "note: mark family {family} declares {} state(s) and the CLASS IS ON SCREEN — the probe did not record it as a family of its own, either because its marks carry a state attribute (the family is then the FIRST class, which is how mark tab-dot is measured as mark) or because they are larger than the 40px the mark probe measures. Not a missing surface; a naming and sizing question.",
                declared.len()
            ));
            continue;
        }
        let missing: Vec<&String> = declared
            .iter()
            .filter(|d| !rendered.contains(d) && !rendered.contains(&strip_data_prefix(d)))
            .collect();
        if !missing.is_empty() {
            let mut sorted = rendered.clone();
            sorted.sort();
            out.push(format!(
                "note: mark family {family} declares {} state(s) and this run rendered {} ({}) over {} surface(s) — NO SURFACE RENDERED {}, so those silhouettes and their collisions are unverified",
                declared.len(),
                rendered.len(),
                if sorted.is_empty() { "none".to_string() } else { sorted.join(", ") },
                list(report.get("surfaces")).len(),
                missing.iter().map(|m| m.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    out
}

/// `/\/\*[\s\S]*?\*\//g` — a non-greedy block-comment strip.
fn strip_block_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out, // an unterminated comment swallows the rest, as the regex does
        }
    }
    out.push_str(rest);
    out
}

/// `^([^[]+)\[([^\]]*)\]$` — a `family[state,state]` string, split. `None` when the shape does not hold.
fn split_family(fam: &str) -> Option<(String, String)> {
    let open = fam.find('[')?;
    if open == 0 || !fam.ends_with(']') {
        return None;
    }
    let body = &fam[open + 1..fam.len() - 1];
    if body.contains(']') {
        return None;
    }
    Some((fam[..open].to_string(), body.to_string()))
}

/// `/(dot|dotcol|mark|led|chip|signal|state)$/` — the probe's own naming rule for a mark family.
fn is_mark_family(name: &str) -> bool {
    ["dot", "dotcol", "mark", "led", "chip", "signal", "state"]
        .iter()
        .any(|s| name.ends_with(s))
}

/// `/^data-(state|live|kind)=/` — the attribute prefix a declared state may carry that the rendered set
/// does not.
fn strip_data_prefix(state: &str) -> String {
    for p in ["data-state=", "data-live=", "data-kind="] {
        if let Some(rest) = state.strip_prefix(p) {
            return rest.to_string();
        }
    }
    state.to_string()
}

/// `/\.([A-Za-z][\w-]*)(\[[^\]]+\]|\.[A-Za-z][\w-]*)/g` — every `.<class><state>` occurrence in a sheet,
/// as `(class, state-selector)` pairs in source order.
///
/// Hand-rolled rather than pulled in with a `regex` dependency: the pattern is one scanner, and the
/// foundation crate is where a new dependency is a decision rather than a convenience. The scan is
/// NON-OVERLAPPING and LEFTMOST, like the global regex it replaces — it resumes after each match.
fn css_class_state_rules(css: &str) -> Vec<(String, String)> {
    let bytes = css.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'.' {
            i += 1;
            continue;
        }
        let Some((class, after)) = scan_ident(css, i + 1) else {
            i += 1;
            continue;
        };
        // The alternation is ORDERED: an attribute selector is tried first.
        if let Some(sel) = scan_attribute(css, after) {
            i = after + sel.len();
            out.push((class, sel));
            continue;
        }
        if bytes.get(after) == Some(&b'.') {
            if let Some((inner, end)) = scan_ident(css, after + 1) {
                out.push((class, format!(".{inner}")));
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `[A-Za-z][\w-]*` at `start` — the identifier and the index just past it.
fn scan_ident(s: &str, start: usize) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    let first = *bytes.get(start)?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let mut end = start + 1;
    while let Some(b) = bytes.get(end) {
        if b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-' {
            end += 1;
        } else {
            break;
        }
    }
    Some((s[start..end].to_string(), end))
}

/// `\[[^\]]+\]` at `start` — at least one character, none of them `]`.
fn scan_attribute(s: &str, start: usize) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.get(start) != Some(&b'[') {
        return None;
    }
    let mut end = start + 1;
    while let Some(b) = bytes.get(end) {
        if *b == b']' {
            return if end > start + 1 {
                Some(s[start..=end].to_string())
            } else {
                None
            };
        }
        end += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    //! THE EQUIVALENCE, PINNED. Every fixture in `tests/fixtures/design-sweep/` carries an `expect` block
    //! CAPTURED FROM THE JAVASCRIPT JUDGE, and this test asserts the port reproduces it exactly — findings,
    //! printed lines, the report notes, the summary and the mark-coverage notes.
    //!
    //! A fixture whose `expect` is wrong is a fixture that proves nothing, so the block is not hand-written:
    //! it is the JS judge's own output over the same JSON.
    //!
    //! MUTATION: delete the `&& !is_false(r.get("reached"))` guard from the press clause — the criterion
    //! that keeps it from accusing a control the pointer never reached.
    //! RESULT: `cargo test -p summrise-agent-core sweep_judge` -> **2 failed, 3 passed**, and the extra
    //! finding it invents is the point: `panel/dark rail Settings: a.link (a.link) renders NOTHING when
    //! pressed — before and during are identical (40x18)`, filed against the row whose `reached` is
    //! `false`. The fixtures that catch it are `press-not-reached` (which exists for exactly this) and
    //! `axes`; the standalone `a_press_the_pointer_never_delivered_is_not_a_dead_control` fails too. Run,
    //! not assumed.
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn fixture_dir() -> PathBuf {
        // `cargo test` runs from the CRATE root, not the repo root — `CARGO_MANIFEST_DIR` is the only
        // anchor that survives that, and this crate's fixtures live inside it.
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/design-sweep")
    }

    fn fixtures() -> Vec<(String, Value)> {
        let dir = fixture_dir();
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .filter(|n| n.ends_with(".json") && !n.starts_with('_'))
            .collect();
        names.sort();
        assert!(
            names.len() >= 8,
            "only {} fixture(s) in {} — the corpus moved, so this test proves nothing",
            names.len(),
            dir.display()
        );
        names
            .into_iter()
            .map(|n| {
                let text = fs::read_to_string(dir.join(&n))
                    .unwrap_or_else(|e| panic!("cannot read {n}: {e}"));
                let v: Value =
                    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{n} is not JSON: {e}"));
                (n, v)
            })
            .collect()
    }

    fn strings(v: Option<&Value>) -> Vec<String> {
        list(v).iter().map(js).collect()
    }

    #[test]
    fn the_port_reproduces_the_js_judge_on_every_fixture() {
        for (name, f) in fixtures() {
            let report = f.get("report").expect("a fixture carries its report");
            let opts = JudgeOptions::from_json(f.get("opts"));
            let expect = f
                .get("expect")
                .unwrap_or_else(|| panic!("{name}: no expect block"));
            let outcome = judge_report(report, &opts);
            assert_eq!(
                outcome.findings,
                strings(expect.get("findings")),
                "{name}: findings differ from the JS judge"
            );
            assert_eq!(
                outcome.lines,
                strings(expect.get("lines")),
                "{name}: printed lines differ from the JS judge"
            );
            assert_eq!(
                outcome.report_notes,
                strings(expect.get("notes")),
                "{name}: the report notes differ from the JS judge"
            );
            if let Some(want) = expect.get("summary").and_then(Value::as_str) {
                assert_eq!(
                    report_summary("panel", report),
                    want,
                    "{name}: the summary differs from the JS judge"
                );
            }
            if let Some(css) = f.get("css").and_then(Value::as_str) {
                assert_eq!(
                    mark_coverage_notes(css, report),
                    strings(expect.get("marks")),
                    "{name}: the mark-coverage notes differ from the JS judge"
                );
            }
        }
    }

    /// THE CASE THE PRESS CLAUSE IS NOT ABOUT, asserted directly as well as through the corpus: a row the
    /// pointer never reached is not a control that ignored a press. A criterion that cannot distinguish
    /// this from the defect it looks for is not a criterion.
    #[test]
    fn a_press_the_pointer_never_delivered_is_not_a_dead_control() {
        let report = serde_json::json!({
            "surfaces": [],
            "press": [{
                "density": "panel", "theme": "light", "page": "Settings",
                "found": 2, "measured": 2,
                "rows": [
                    {"sel": "button.btn", "where": "button.btn.btn-primary", "size": "111x24",
                     "changed": false, "reached": false, "note": "the pointer never reached this control"},
                    {"sel": "button.btn", "where": "button.btn.btn-ghost", "size": "50x24",
                     "changed": true, "reached": true, "props": ["transform"]}
                ]
            }]
        });
        let out = judge_report(&report, &JudgeOptions::default());
        assert!(
            out.findings.is_empty(),
            "a press that was never delivered is not a defect: {:?}",
            out.findings
        );
    }

    /// AND THE OTHER HALF: the same row WITH the pointer arriving IS a defect, and the sentence names the
    /// control and the two identical snapshots — the port must print the fix, not a bare verdict.
    #[test]
    fn a_press_that_arrived_and_rendered_nothing_is_named_with_its_evidence() {
        let report = serde_json::json!({
            "surfaces": [],
            "press": [{
                "density": "panel", "theme": "light", "page": "Settings",
                "found": 2, "measured": 0,
                "rows": [{"sel": "button.btn", "where": "button.btn.btn-primary", "size": "111x24",
                          "changed": false, "reached": true}]
            }]
        });
        let out = judge_report(&report, &JudgeOptions::default());
        assert_eq!(
            out.findings,
            vec![
                "panel/light Settings: button.btn (button.btn.btn-primary) renders NOTHING when pressed — before and during are identical (111x24)".to_string(),
                "panel/light Settings: the press pass measured 0 control(s) of the 2 this page renders — a press pass that pressed nothing proves nothing".to_string(),
            ]
        );
    }

    /// THE JS RENDERINGS THE SENTENCES DEPEND ON. `String(Number)` and `String.slice` are not Rust's, and
    /// a finding is read by a person — so they are pinned rather than assumed.
    #[test]
    fn javascript_rendering_is_reproduced() {
        assert_eq!(js_num(2.0), "2");
        assert_eq!(js_num(10.8), "10.8");
        assert_eq!(js_num(-0.0), "0");
        assert_eq!(jsf(None), "undefined");
        assert_eq!(jsf(Some(&Value::Null)), "null");
        assert_eq!(js(&serde_json::json!([1, "a", null])), "1,a,");
        // 24 UTF-16 CODE UNITS, which is what `String.slice(0, 24)` counts — an emoji is two of them.
        assert_eq!(
            js_slice("😀😀😀😀😀😀😀😀😀😀😀😀x", 24),
            "😀😀😀😀😀😀😀😀😀😀😀😀"
        );
        assert_eq!(js_slice("abcdef", 4), "abcd");
    }

    /// THE PRESS FLOOR IS DERIVED FROM WHAT THE PAGE HAD. `found: 1` makes one delivered press a complete
    /// pass, because the harness's Browser page is an explanation with a single control.
    #[test]
    fn a_one_control_page_is_not_a_vacuous_pass() {
        let report = serde_json::json!({
            "surfaces": [],
            "press": [{"density": "panel", "theme": "light", "found": 1, "measured": 1,
                       "rows": [{"sel": "a", "changed": true, "reached": true}]}]
        });
        assert!(judge_report(&report, &JudgeOptions::default())
            .findings
            .is_empty());
    }
}
