//! `judge()` from `agent/scripts/panel-design-sweep.mjs:299-628`.
//!
//! THE CHECKS ARE THE SHARED CORE'S. This file adds only what is the PANEL's own — the `--expect` coverage
//! rule, the panel's implicitStates and its one `ignore` entry, the concrete DECORATIVE waivers, the prose
//! floor, the harness sheet's mark-coverage note, the band margins and the timing note.

use crate::js::{arr, at, get, is_num, js_str, num, slice0, to_fixed, truthy};
use crate::out::Out;
use crate::paths;
use crate::report::{judge_report, Finding, IgnoreRule, JudgeOpts};
use crate::summary::report_summary;
use crate::tools::{read_report, verdict};
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

/// THE PANEL'S IGNORE LIST. `ignore` entries are consulted against FINDINGS, and this is the only entry:
/// the working rail dot's halo, matched UNANCHORED so one ordered pattern cannot cover one path and miss
/// the other, with the value left in the test so a DIFFERENT ratio on this element is still a finding.
static PANEL_IGNORE: [IgnoreRule; 1] = [IgnoreRule {
    matches: |text, _entry| {
        Regex::new(r"div\.rail-dot").expect("compiles").is_match(text)
            && Regex::new(r"2\.33").expect("compiles").is_match(text)
    },
    reason: "the working rail dot's halo is emphasis, not the signal — the fill carries the state and clears 3:1 in both themes (measured, round 212)",
    dormant: Some(
        "4 hover surfaces, 14/14 and 11/11 interactive, underAA empty (round 22) — nothing to excuse today; the unanchored pattern still matches the row shape, so it stays for the state it guards",
    ),
}];

/// CLASSES WITH NO MATCHING RULE THAT ARE NOT DEFECTS, each with the mechanism named. Measured round 90:
/// the panel density renders 1123 styled classes and eleven such names, and ten of the eleven are xterm.js's
/// own DOM — styled by a stylesheet it INJECTS AT RUNTIME, which a CSSOM walk cannot see (the same runtime
/// injection that defeated the reduced-motion override in round 77). The last two are ours and also
/// deliberate: `terminal` is a VIEW NAME on an element already carrying `.view`, and `warn` rides a base
/// rule.
///
/// PRUNED: `serial`, whose reason was "a session-kind modifier; the element is painted by its [data-kind]
/// rule" (round 24). The class it exempts no longer exists: the mark language's TabBar emits
/// `className="mark tab-dot" data-kind={s.kind}` and the sheet paints the kind with
/// `.tab-dot[data-kind="serial"]`, so nothing puts a bare `serial` on screen. MEASURED, not assumed.
static PANEL_IMPLICIT_STATES: [(&str, &str); 11] = [
    (
        "xterm-viewport",
        "xterm.js DOM, styled by the sheet it injects at runtime",
    ),
    (
        "xterm-screen",
        "xterm.js DOM, styled by the sheet it injects at runtime",
    ),
    (
        "xterm-helpers",
        "xterm.js DOM, styled by the sheet it injects at runtime",
    ),
    (
        "xterm-helper-textarea",
        "xterm.js DOM, styled by the sheet it injects at runtime",
    ),
    (
        "xterm-scroll-area",
        "xterm.js DOM, styled by the sheet it injects at runtime",
    ),
    (
        "xterm-char-measure-element",
        "xterm.js DOM, styled by the sheet it injects at runtime",
    ),
    (
        "xterm-width-cache-measure-container",
        "xterm.js DOM, styled by the sheet it injects at runtime",
    ),
    (
        "xterm-decoration-container",
        "xterm.js DOM, styled by the sheet it injects at runtime",
    ),
    (
        "composition-view",
        "xterm.js DOM (IME composition), styled by the sheet it injects at runtime",
    ),
    ("terminal", "a VIEW NAME; the element's .view class does the styling"),
    (
        "warn",
        "the boot chip's tone modifier. The BASE rule paints it — .boot-mark is the warn triangle and .boot-mark.info is the exception — so the name needs no rule of its own. Round 121 followed it anyway, and found a real defect behind it: the triangle used --state-warn, which measures 2.80 on the dark chip surface against the 3:1 a graphic needs. Fixed to --warn-ink (6.45 / 8.76).",
    ),
];

/// The panel's options, as one value so the judge and the tests cannot see two different sets.
pub fn options() -> JudgeOpts<'static> {
    JudgeOpts {
        navless: &[],
        // PROSE HAS A MEASURE, AND THIS ROUND MEASURED WHETHER IT DOES (round 265). THE FLOOR IS 90, and it
        // is the defect class that chose it: the twelve offenders start at 93, the panel's own capped ledes
        // render 70-73, and 90 leaves a fifth of headroom over the rule (66ch) rather than encoding the rule
        // itself — a `ch` cap and a measured `cpl` are different units. The console and the landing carry the
        // same numbers in their reports and are NOT failed by this floor: neither has been measured on this
        // axis, and a threshold somebody else picked is not a finding about them.
        prose_floor: Some(90.0),
        implicit_states: &PANEL_IMPLICIT_STATES,
        unstyled_floor: None,
        unmeasurable_floor: None,
        ignore: &PANEL_IGNORE,
    }
}

// NOT CARRIED: `twoloud: ["panel-Terminal", "desktop-Terminal"]`. The panel's options object still names it
// and the shared judge reads no such key at all — the page-name exception became the ELEMENT-scoped
// NAV_LOUD list in round 68 and the option stayed behind, with a comment above it that still claims a
// suppression it no longer performs. It is dropped here rather than carried as a setting that does
// nothing; see the report beside this change.

/// DECORATIVE GRAPHICS: drawn to DELIMIT, not to inform. WCAG 1.4.11 applies to a non-text element that
/// CARRIES MEANING; a chip's 1px hairline does not. The waivers live in ONE place because the panel's OTHER
/// instrument reads the same rows, and two instruments disagreeing about one measurement is what round 265
/// fixed.
pub struct Decorative {
    pub pattern: Regex,
    /// `String(entry.match)` — the note prints the RegExp, slashes and all.
    pub display: &'static str,
    pub values: &'static [(f64, f64)],
    pub slack: Option<f64>,
    pub reason: &'static str,
}

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

const BAND_SLACK: f64 = 0.02;

/// `--expect` coverage. WHAT THIS REPORT WAS SUPPOSED TO COVER: the sweep outgrew its caller's timeout, so
/// it can be run in passes — and a partial run that reported "nothing found" would read exactly like a
/// clean full one. The caller states its expectation; the judge fails when the report is missing any of it.
pub fn coverage(report: &Value, expect: &str) -> Vec<String> {
    let passes = get(report, "passes");
    let is_all = matches!(passes, Some(Value::String(s)) if s == "all");
    let missing: Vec<String> = if expect == "all" {
        if is_all {
            Vec::new()
        } else {
            vec![format!("the run measured only \"{}\"", js_str(passes))]
        }
    } else {
        expect
            .split(',')
            .map(|x| x.trim())
            .filter(|x| {
                if is_all {
                    return false;
                }
                let text = if truthy(passes) {
                    js_str(passes)
                } else {
                    String::new()
                };
                !text.split(',').map(|y| y.trim()).any(|y| y == *x)
            })
            .map(|x| x.to_string())
            .collect()
    };
    missing
        .iter()
        .map(|m| format!("INCOMPLETE REPORT — {m}; the absences below prove nothing"))
        .collect()
}

pub fn judge(file: &str, expect: &str, out: &mut Out) -> i32 {
    let Some(report) = read_report(file, out) else {
        return 1;
    };
    let coverage = coverage(&report, expect);
    let mut findings: Vec<Finding> = coverage.iter().map(Finding::text).collect();
    findings.extend(judge_report(&report, &options(), out));

    // A MEASUREMENT THAT FOUND NOTHING TO MEASURE IS NOT A PASS (round 265). The prose axis is only as good
    // as the text blocks it matched: a selector change, a probe that stopped counting, or a harness that
    // rendered an empty page would ALL report "no long lines". SCOPED TO THE RUN THAT PRODUCES SURFACES: a
    // report from `--passes=focus` has none, and that is the coverage clause's business.
    {
        let passes = get(&report, "passes");
        let pages_ran = matches!(passes, Some(Value::String(s)) if s == "all")
            || (if truthy(passes) {
                js_str(passes)
            } else {
                String::new()
            })
            .split(',')
            .map(|p| p.trim())
            .any(|p| p == "pages");
        let measured: f64 = arr(get(&report, "surfaces"))
            .iter()
            .map(|s| num(at(get(s, "measure"), "measured")).unwrap_or(0.0))
            .sum();
        if pages_ran && measured < 12.0 {
            findings.push(Finding::text(format!(
                "the prose-measure axis matched only {} text block(s) across {} surface(s) — a run that measured nothing cannot clear this axis",
                js_str(Some(&serde_json::json!(measured))),
                arr(get(&report, "surfaces")).len()
            )));
        }
    }

    // THE REPORT MUST NOT LIE ABOUT WHAT IT RENDERED. Round 175 shipped a two-theme fixture whose rows all
    // said `theme: light` while the URLs said dark — the renders were right and the report was wrong, so a
    // dark regression would have been filed under light. This reads the theme off the PAGE and fails when it
    // disagrees with what was navigated to. An empty stored value is not judged: an app that reads the theme
    // from the URL alone would legitimately have nothing to store.
    for t in arr(get(&report, "themeChecks")) {
        let seen = if truthy(get(t, "stored")) {
            get(t, "stored")
        } else {
            get(t, "attr")
        };
        if truthy(seen) && js_str(seen) != js_str(get(t, "intended")) {
            findings.push(Finding::text(format!(
                "theme: {} was navigated as \"{}\" and rendered \"{}\" — the report would be describing a page it did not render",
                js_str(get(t, "page")),
                js_str(get(t, "intended")),
                js_str(seen)
            )));
        }
    }

    // TARGET SIZE IS JUDGED IN THE SHARED JUDGE — `judgeReport` owns the clause and this file's copy was
    // DELETED (round 20 of the standing goal). The label that mattered is preserved there.
    let mut waived: Vec<String> = Vec::new();
    for r in crate::contrast::failures(arr(get(&report, "rows"))) {
        // A WAIVER IS FOR THE RATIO IT WAS MEASURED AT, NOT FOR THE ELEMENT (round 95). This used to match on
        // the SELECTOR alone, so an entry written for one number set aside EVERY ratio that element could
        // ever produce. Each entry carries the band it was measured in now, a row outside every band is a
        // finding, and the finding says which band refused it — otherwise the reader sees a bare ratio and
        // cannot tell a new defect from a waiver that moved.
        let sel = js_str(get(r, "sel"));
        let why = decorative().iter().find(|d| d.pattern.is_match(&sel));
        let cr = num(get(r, "cr"));
        if let Some(d) = why {
            if cr
                .map(|c| d.values.iter().any(|(lo, hi)| c >= *lo && c <= *hi))
                .unwrap_or(false)
            {
                waived.push(format!("{sel} {} — {}", js_str(get(r, "cr")), d.reason));
                continue;
            }
            findings.push(Finding::text(format!(
                "{sel} {} on {} — the DECORATIVE entry for this element waives {}, and this is a DIFFERENT value: a waiver is for the ratio it was measured at, not for the element",
                js_str(get(r, "cr")),
                js_str(get(r, "surface")),
                d.values
                    .iter()
                    .map(|(lo, hi)| format!(
                        "{}-{}",
                        js_str(Some(&serde_json::json!(lo))),
                        js_str(Some(&serde_json::json!(hi)))
                    ))
                    .collect::<Vec<_>>()
                    .join(" / ")
            )));
            continue;
        }
        if (findings.len() as i64) < 10 + coverage.len() as i64 {
            // WHAT THE PROBE MEASURED, NOT JUST THE RATIO (round 73). The row has carried `paint`, `surface`,
            // `kind` and `size` all along and the finding printed none of them, so a number like "2.33"
            // arrived with no way to tell which colour on which surface it was. AND THE THEME (round 75):
            // the light and dark passes produce the SAME page name, so a finding about a dark page and a
            // finding about a light one read identically.
            findings.insert(
                0,
                Finding::text(format!(
                    "{} {}/{} [{}] {} \"{}\" — painted {} on {}, {}px {}, needs {}",
                    js_str(get(r, "cr")),
                    js_str(get(r, "density")),
                    js_str(get(r, "page")),
                    js_str(get(r, "theme")),
                    sel,
                    slice0(&js_str(get(r, "text")), 24),
                    js_str(get(r, "paint")),
                    js_str(get(r, "surface")),
                    js_str(get(r, "size")),
                    js_str(get(r, "kind")),
                    js_str(get(r, "need"))
                )),
            );
        }
    }

    out.note(&report_summary("panel", &report));

    // THE TIMING DATA, SAID OUT LOUD. It has been collected on every page since the timing pass was added
    // and read by nothing — not judged (a wall-clock budget would fail on a loaded CI box, which is why it is
    // not a finding) and not printed either, so a ten-fold regression in boot cost would have been invisible.
    // The worst page is the one worth seeing: a report that prints 40 timings is a report nobody reads.
    {
        let timing: Vec<&Value> = arr(get(&report, "timing"))
            .iter()
            .filter(|x| is_num(get(x, "toFirstRowMs")))
            .collect();
        if !timing.is_empty() {
            let worst = timing.iter().fold(timing[0], |a, b| {
                if num(get(b, "toFirstRowMs")).unwrap_or(f64::NAN)
                    > num(get(a, "toFirstRowMs")).unwrap_or(f64::NAN)
                {
                    b
                } else {
                    a
                }
            });
            out.note(&format!(
                "note: boot timing — worst of {} surfaces: {}ms to first row ({}/{}, first paint {}ms, {} nodes)",
                timing.len(),
                js_str(get(worst, "toFirstRowMs")),
                js_str(get(worst, "density")),
                js_str(get(worst, "mode")),
                js_str(get(worst, "firstPaintMs")),
                js_str(get(worst, "nodes"))
            ));
        }
    }
    if !coverage.is_empty() {
        out.err(&format!("\n{}", coverage.join("\n")));
    }
    if !crate::contrast::unmeasurable(arr(get(&report, "rows"))).is_empty() {
        out.note(&format!(
            "note: {} node(s) unmeasurable",
            crate::contrast::unmeasurable(arr(get(&report, "rows"))).len()
        ));
    }

    // WHICH MARK STATES THE RUN RENDERED, AGAINST WHICH THE SHEET DECLARES (round 26). The collision check
    // compares the silhouettes of the states a surface HAPPENS to render, so a family with six declared
    // states and three rendered has half its shapes unverified — and a collision among the unrendered half
    // cannot be seen at all.
    //
    // The JS also builds a `seenByFamily` map here and never reads it; the coverage note recomputes the same
    // thing from the same report. It is not carried over — it decides nothing.
    //
    // COMMENTS FIRST, so prose about a selector is not read as one (the lesson `css-vars-check` and
    // `retired-colours-check` both record from their own first runs).
    if let Some(css) = paths::panel_sheet() {
        let stripped = Regex::new(r"/\*[\s\S]*?\*/")
            .expect("compiles")
            .replace_all(&css, "")
            .to_string();
        for line in crate::marks::mark_coverage_notes(&stripped, &report) {
            out.note(&line);
        }
    }

    // HOW WIDE IS A BAND, MEASURED AGAINST WHAT THE RUN SAW (round 23). A DECORATIVE entry waives a RATIO,
    // not an element (round 95), and the band is what decides. The failure that leaves no trace is the
    // opposite direction — a band WIDER than its evidence excuses a drift nobody measured.
    {
        let rows = arr(get(&report, "rows"));
        for d in decorative() {
            // `typeof r.cr !== "number"` is a TYPE test, not a coercion: a row whose `cr` is null is
            // skipped here, and treating it as 0 would drag `lo` to zero and silence the very drift this
            // note exists to report.
            let ratios: Vec<f64> = rows
                .iter()
                .filter(|r| d.pattern.is_match(&js_str(get(r, "sel"))))
                .filter(|r| is_num(get(r, "cr")))
                .filter_map(|r| num(get(r, "cr")))
                .collect();
            if ratios.is_empty() || d.values.is_empty() {
                continue;
            }
            let lo = ratios.iter().cloned().fold(f64::INFINITY, f64::min);
            let hi = ratios.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let slack = d.slack.unwrap_or(BAND_SLACK);
            // PER SIDE, NOT THE MINIMUM OF THE TWO. The first version took `Math.min(lo - bandLo, bandHi -
            // hi)`, which lets a wide side hide behind a tight one. AND AN EPSILON, because the margin is
            // computed in binary floating point: 1.29 - 1.27 is 0.020000000000000018, so an exact band
            // reported itself as 0.02 over 0.02 — a warning about the arithmetic rather than about the band.
            let worst_below = d
                .values
                .iter()
                .map(|(a, _)| lo - a)
                .fold(f64::NEG_INFINITY, f64::max);
            let worst_above = d
                .values
                .iter()
                .map(|(_, b)| b - hi)
                .fold(f64::NEG_INFINITY, f64::max);
            if worst_below > slack + 1e-9 || worst_above > slack + 1e-9 {
                let mut distinct: Vec<f64> = ratios.clone();
                distinct.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                distinct.dedup();
                out.note(&format!(
                    "note: {} waives {} and this run saw {}-{} ({} distinct over {} row(s)) — margin {} below and {} above against a declared slack of {}; tighten the band or say why the margin is real",
                    d.display,
                    d.values
                        .iter()
                        .map(|(a, b)| format!(
                            "{}-{}",
                            js_str(Some(&serde_json::json!(a))),
                            js_str(Some(&serde_json::json!(b)))
                        ))
                        .collect::<Vec<_>>()
                        .join(" / "),
                    js_str(Some(&serde_json::json!(lo))),
                    js_str(Some(&serde_json::json!(hi))),
                    distinct.len(),
                    ratios.len(),
                    to_fixed(worst_below, 2),
                    to_fixed(worst_above, 2),
                    js_str(Some(&serde_json::json!(slack)))
                ));
            }
        }
    }

    // A WAIVER NOBODY USED IS DEAD WEIGHT IN THE ONE LIST A READER CONSULTS (round 21). A NOTE, NOT A
    // FINDING: this list is judged per run, and a run that measures one axis (the ack pass alone) has rows
    // from nothing else — every entry would look stale. The note says how many rows were looked at, so a
    // reader can tell a real stale entry from a partial run.
    {
        let rows = arr(get(&report, "rows"));
        let unmatched: Vec<&Decorative> = decorative()
            .iter()
            .filter(|d| {
                !rows
                    .iter()
                    .any(|r| d.pattern.is_match(&js_str(get(r, "sel"))))
            })
            .collect();
        if !unmatched.is_empty() {
            out.note(&format!(
                "note: {} of {} DECORATIVE entr(ies) matched NO row in this run ({} rows over {} surface(s)) — a waiver nothing uses is weight; prune it or say why it stays:",
                unmatched.len(),
                decorative().len(),
                rows.len(),
                arr(get(&report, "surfaces")).len()
            ));
            for d in unmatched {
                out.note(&format!("  {}", d.display));
            }
        }
    }
    if !waived.is_empty() {
        out.note(&format!(
            "note: {} decorative graphic(s) set aside, each with its reason:",
            waived.len()
        ));
        let mut seen: Vec<&String> = Vec::new();
        for w in &waived {
            if !seen.contains(&w) {
                seen.push(w);
            }
        }
        for w in seen {
            out.note(&format!("  {w}"));
        }
    }

    let texts: Vec<String> = findings.into_iter().map(|f| f.text).collect();
    verdict(
        &texts,
        "panel design sweep OK: nothing above found a defect",
        out,
    )
}
