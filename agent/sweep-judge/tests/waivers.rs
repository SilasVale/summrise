//! THE DECORATIVE POLICY LIST, AND THE GATE THE TWO INSTRUMENTS NEVER HAD.
//!
//! **WHAT WAS ACTUALLY WRONG, WHICH IS NOT WHAT THE PLAN EXPECTED TO FIND.** The plan's Task 3 reads as if
//! the list were about to move for the first time: *"Create `agent/sweep-judge/src/waivers.rs`; Produces
//! `DECORATIVE_WAIVERS` (verbatim from `design-sweep.mjs:27`)"*. It is ALREADY Rust — `tools/panel.rs`
//! carries `Decorative` with the same two patterns, both bands, both slacks and both reasons, and three
//! judges read it (the band-refusal finding, the band-width note, the stale-entry note). What was missing is
//! the other half of "ONE PLACE": **the JavaScript array is a SECOND COPY, read by a second instrument**
//! (`agent/scripts/panel-render-audit.mjs:336`), and NOTHING in this repository compared the two. Change a
//! band in the sheet a browser reads and the judge keeps waiving the old range; change it in Rust and the
//! audit keeps waiving the old one — silently, in both directions, with the two instruments disagreeing
//! about one measurement, which is the exact failure round 265 created this list to prevent.
//!
//! So this file is a CROSS-LANGUAGE GATE: it reads the JavaScript array out of
//! `agent/scripts/lib/design-sweep.mjs` and requires the Rust list to be the same list — same number of
//! entries, same patterns, same bands, same slack, same reason. It is the durability the plan's Task 6
//! differential would otherwise only have on the day it was run.
//!
//! **THE EXTRACTOR IS DELIBERATELY LOUD ABOUT NOTHING.** A text reader over JavaScript is fragile by nature,
//! and the failure that matters is the quiet one: a format change that yields ZERO entries would make every
//! assertion below trivially true. So the floors come first and are asserted on their own — the array must be
//! found, it must yield at least one entry, every entry must carry all four fields, and every JS pattern must
//! be an ANCHORED LITERAL (which is the only shape from which a test can derive the selector the pattern
//! matches without re-implementing a regex engine).
//!
//! MUTATION: delete the second entry (`span.nm-ico`) from `decorative()` in `src/waivers.rs`.
//! RESULT:   `cargo test -p summrise-sweep-judge --test waivers` → exit 101, MEASURED:
//!
//! ```text
//! the_rust_list_is_the_javascript_list ... FAILED
//!   the JavaScript waives 2 entr(ies) and Rust waives 1 — `/^span\.nm-ico$/` is in the
//!   list a browser reads and not in the one the judge reads
//! test result: FAILED. 5 passed; 1 failed
//! ```
//!
//! ...and `every_entry_is_waived` fails beside it naming the entry, which is the same defect seen from the
//! other side. Restoring the entry returns 7 passed.

use summrise_sweep_judge::waivers::{decorative, is_waived, waived_at};

/// One entry of the JavaScript array, as text.
#[derive(Debug, PartialEq)]
struct JsWaiver {
    /// `/^span\.approval-grant$/` — the display form, which is what the notes print.
    display: String,
    /// The regex source between the slashes: `^span\.approval-grant$`.
    source: String,
    values: Vec<(f64, f64)>,
    slack: Option<f64>,
    reason: String,
}

/// The slice of `design-sweep.mjs` that holds the array, or a panic naming what was looked for.
fn js_array_body() -> String {
    let root = summrise_sweep_judge::paths::repo_root()
        .expect("the repository root, found by the walk `paths.rs` documents");
    let path = root.join("agent/scripts/lib/design-sweep.mjs");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let start = text
        .find("export const DECORATIVE_WAIVERS = [")
        .unwrap_or_else(|| {
            panic!(
                "{}: `export const DECORATIVE_WAIVERS = [` is not in the file — the array this gate reads \
                 is gone or renamed, and every assertion below would be vacuously true",
                path.display()
            )
        });
    let rest = &text[start..];
    let end = rest.find("\n];").unwrap_or_else(|| {
        panic!(
            "{}: the DECORATIVE_WAIVERS array is not closed by a line holding `];` — the extractor cannot \
             tell where it ends, and a body that ran to the end of the file would sweep up every `match:` \
             in it",
            path.display()
        )
    });
    rest[..end].to_string()
}

/// Parse the array body into entries. Every `match:` opens one, and the three other fields are read from the
/// slice up to the next `match:` — so a field that drifts into a comment cannot be attributed to the wrong
/// entry without also moving past the following `match:`.
fn js_waivers() -> Vec<JsWaiver> {
    let body = js_array_body();
    let mut starts: Vec<usize> = Vec::new();
    let mut from = 0;
    while let Some(i) = body[from..].find("match:") {
        starts.push(from + i);
        from += i + "match:".len();
    }
    assert!(
        !starts.is_empty(),
        "the DECORATIVE_WAIVERS array body holds no `match:` at all — a list with no entries would make \
         every cross-language assertion in this file pass while checking nothing"
    );
    let re_match = regex::Regex::new(r"^match:\s*/((?:[^/\\]|\\.)*)/").expect("compiles");
    let re_values =
        regex::Regex::new(r"values:\s*\[((?:\s*\[[^\]]*\]\s*,?)+)\]").expect("compiles");
    let re_pair = regex::Regex::new(r"\[\s*([-\d.]+)\s*,\s*([-\d.]+)\s*\]").expect("compiles");
    let re_slack = regex::Regex::new(r"slack:\s*([-\d.]+)").expect("compiles");
    let re_reason = regex::Regex::new(r#"reason:\s*"((?:[^"\\]|\\.)*)""#).expect("compiles");

    let mut out = Vec::new();
    for (n, start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(body.len());
        let entry = &body[*start..end];
        let source = re_match
            .captures(entry)
            .unwrap_or_else(|| panic!("entry {n}: no `match: /…/` could be read from:\n{entry}"))
            .get(1)
            .expect("group 1")
            .as_str()
            .to_string();
        let pairs: Vec<(f64, f64)> = re_values
            .captures(entry)
            .unwrap_or_else(|| {
                panic!("entry {n} (`/{source}/`): no `values: [[lo, hi]]` in:\n{entry}")
            })
            .get(1)
            .expect("group 1")
            .as_str()
            .split("],")
            .filter_map(|p| re_pair.captures(p))
            .map(|c| {
                (
                    c.get(1).unwrap().as_str().parse().expect("a lo bound"),
                    c.get(2).unwrap().as_str().parse().expect("a hi bound"),
                )
            })
            .collect();
        assert!(
            !pairs.is_empty(),
            "entry {n} (`/{source}/`): `values:` was found and yielded no band — a waiver with no band \
             waives the element at EVERY ratio, which is the round-95 defect this field exists to prevent"
        );
        let slack = re_slack
            .captures(entry)
            .map(|c| c.get(1).unwrap().as_str().parse().expect("a slack"));
        let reason = re_reason
            .captures(entry)
            .unwrap_or_else(|| panic!("entry {n} (`/{source}/`): no `reason: \"…\"` in:\n{entry}"))
            .get(1)
            .expect("group 1")
            .as_str()
            .to_string();
        out.push(JsWaiver {
            display: format!("/{source}/"),
            source,
            values: pairs,
            slack,
            reason,
        });
    }
    out
}

/// **THE SELECTOR A PATTERN IS ABOUT**, derived rather than re-implemented: the two entries are ANCHORED
/// LITERALS, so stripping `^`/`$` and unescaping `\.` gives the string the pattern matches. Anything else is
/// refused loudly, because the alternative is a test that invents a selector the pattern does not match and
/// then reports the list as broken.
fn sample_selector(source: &str) -> String {
    let body = source
        .strip_prefix('^')
        .and_then(|s| s.strip_suffix('$'))
        .unwrap_or_else(|| {
            panic!(
                "`/{source}/` is not an anchored literal (`^…$`), so this gate cannot derive the selector it \
                 matches without implementing a regex engine — extend the extractor rather than guess"
            )
        });
    assert!(
        !body.contains(['*', '+', '?', '[', ']', '(', ')', '|', '{', '}']),
        "`/{source}/` uses a regex construct beyond an escaped literal, so the derived sample would be a \
         guess: {body}"
    );
    body.replace("\\.", ".")
}

/// The floor that makes the rest meaningful, and the direction that catches a list that only grew.
#[test]
fn the_extractor_found_the_javascript_array() {
    let js = js_waivers();
    assert!(
        !js.is_empty(),
        "the DECORATIVE_WAIVERS extractor returned {} entries — the assertions below would be vacuous",
        js.len()
    );
    for w in &js {
        assert!(
            !sample_selector(&w.source).is_empty(),
            "`{}` derives an empty selector",
            w.display
        );
        assert!(!w.reason.is_empty(), "`{}` carries no reason", w.display);
    }
    println!(
        "js-boundary: {} DECORATIVE_WAIVERS entries read out of design-sweep.mjs: {}",
        js.len(),
        js.iter()
            .map(|w| w.display.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
}

/// **THE GATE.** Same length, same patterns, same bands, same slack, same reason — read out of the file a
/// browser is served, checked against the list the judge decides with.
#[test]
fn the_rust_list_is_the_javascript_list() {
    let js = js_waivers();
    let rust = decorative();
    assert_eq!(
        rust.len(),
        js.len(),
        "the JavaScript waives {} entr(ies) and Rust waives {} — {}",
        js.len(),
        rust.len(),
        if rust.len() < js.len() {
            format!(
                "{} is in the list a browser reads and not in the one the judge reads",
                js.iter()
                    .map(|w| w.display.as_str())
                    .filter(|d| !rust.iter().any(|r| r.display == *d))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            "Rust waives something the JavaScript does not, so the audit would report it and the judge \
             would not"
                .to_string()
        }
    );
    for w in &js {
        let r = rust
            .iter()
            .find(|r| r.display == w.display)
            .unwrap_or_else(|| {
                panic!(
                    "`{}` is waived in design-sweep.mjs and not in src/waivers.rs — the two instruments \
                     disagree about one measurement",
                    w.display
                )
            });
        assert_eq!(
            r.values, w.values,
            "`{}`: Rust waives {:?} and the JavaScript waives {:?} — a band that moved in one list is a \
             waiver that moved for one instrument only",
            w.display, r.values, w.values
        );
        assert_eq!(
            r.slack, w.slack,
            "`{}`: Rust declares slack {:?} and the JavaScript {:?}",
            w.display, r.slack, w.slack
        );
        assert_eq!(
            r.reason, w.reason,
            "`{}`: the reason a reader is shown is not the same in both lists",
            w.display
        );
    }
}

/// ...and every entry of the JS array is waived BY SELECTOR, which is the plan's own phrasing.
#[test]
fn every_entry_is_waived() {
    for w in js_waivers() {
        let sel = sample_selector(&w.source);
        assert!(
            is_waived(&sel),
            "`{}` waives `{sel}` and `is_waived(\"{sel}\")` says no",
            w.display
        );
    }
}

/// A selector that is not in the list is not waived, and the patterns are ANCHORED: a class that merely
/// starts with a waived one must not inherit its exemption.
#[test]
fn a_selector_that_is_not_waived_is_not_waived() {
    for sel in [
        "span.other",
        "div.approval-grant",
        "span.approval-grant-x",
        "span.approval",
        "span.nm-ico-x",
        "",
    ] {
        assert!(!is_waived(sel), "`{sel}` must not be waived");
    }
}

/// **A WAIVER IS FOR THE RATIO IT WAS MEASURED AT, NOT FOR THE ELEMENT** (round 95), and this is the half
/// the JS array cannot state on its own — it is only true if the JUDGE applies the band. Both sides of the
/// grant chip's band, and a value inside the second entry's band.
#[test]
fn a_waiver_covers_its_band_and_nothing_else() {
    assert!(waived_at("span.approval-grant", 1.19).is_some());
    assert!(
        waived_at("span.approval-grant", 1.17).is_some(),
        "the lo edge is inclusive"
    );
    assert!(
        waived_at("span.approval-grant", 1.29).is_some(),
        "the hi edge is inclusive"
    );
    assert!(
        waived_at("span.approval-grant", 1.169).is_none(),
        "below the band is a finding, not a waiver"
    );
    assert!(
        waived_at("span.approval-grant", 1.291).is_none(),
        "above the band is a finding, not a waiver"
    );
    assert!(
        waived_at("span.approval-grant", 3.2).is_none(),
        "the ratio that prompted the entry is not a licence for every ratio"
    );
    // The bands are per entry, not shared: 1.08 is inside the icon chip's band and outside the grant's.
    assert!(waived_at("span.nm-ico", 1.08).is_some());
    assert!(waived_at("span.approval-grant", 1.08).is_none());
    // And a waived selector with no band at all is not a waiver.
    assert!(waived_at("span.other", 1.19).is_none());
}
