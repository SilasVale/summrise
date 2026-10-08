//! `judge()` from `agent/scripts/landing-design-sweep.mjs:146-189`.
//!
//! THE SHARED CLAUSES, NOT A COPY OF THEM. The first version of this function re-implemented names, marks
//! and unstyled by hand — and got the target axis wrong on its first CI run, because `undersized` in that
//! probe is a COUNT and the list is `distinct`. The console's judge had the right shape all along (it calls
//! `judgeReport` and adds only what is its own), and this does the same.
//!
//! `navless`: the landing has no navigation BY DESIGN — it is one page with steps, not an application — and
//! the landmark clause would otherwise read that as a defect on every surface.
//!
//! `unstyledFloor`: the shared floor is 100 because the panel and the console each style hundreds of
//! classes. The landing is ONE PAGE with 22, and the floor's purpose is to make a broken collector loud —
//! so 15 is the number that does that job here without failing a page that is simply small. Measured: 22.

use crate::js::{arr, get, js_str, truthy};
use crate::out::Out;
use crate::paths;
use crate::report::{judge_report, Finding, JudgeOpts};
use crate::summary::report_summary;
use crate::tools::{read_report, verdict};

pub fn options() -> JudgeOpts<'static> {
    JudgeOpts {
        navless: &["installer", "npm-only"],
        prose_floor: None,
        implicit_states: &[],
        unstyled_floor: Some(15.0),
        unmeasurable_floor: None,
        ignore: &[],
    }
}

pub fn judge(file: &str, out: &mut Out) -> i32 {
    let Some(report) = read_report(file, out) else {
        return 1;
    };
    let mut findings: Vec<Finding> = judge_report(&report, &options(), out);

    // READ OFF THE PAGE, not off the instruction: the sweep asks the browser for its colour-scheme media
    // query and the report carries the answer, so a page that ignored the emulation cannot pass as the
    // scheme it was asked for.
    for t in arr(get(&report, "themeChecks")) {
        let scheme = get(t, "scheme");
        if let Some(actual) = scheme.and_then(|s| s.as_bool()) {
            let intended_dark =
                matches!(get(t, "intended"), Some(serde_json::Value::String(s)) if s == "dark");
            if actual != intended_dark {
                findings.push(Finding::text(format!(
                    "scheme: {} was rendered for \"{}\" but the page reports prefers-color-scheme: dark = {}",
                    js_str(get(t, "page")),
                    js_str(get(t, "intended")),
                    js_str(scheme)
                )));
            }
        }
    }

    // TARGET SIZE IS JUDGED IN THE SHARED JUDGE NOW — `judgeReport` owns the clause and this file's copy was
    // DELETED (round 20 of the standing goal): three judges, one criterion, and round 17 had strengthened
    // only one of them. The number round 19 measured here was ZERO, which is why enforcing it there was
    // safe to do.
    //
    // THIS CLAUSE IS NOT A DUPLICATE OF THE SHARED ONE, even though both read `entryCheck`: the shared judge
    // fires on `stale` and names the mismatch, and this one is the landing's own reading of the same field
    // (`check.expected && check.expected.bytes` — the `&&` prints `undefined` when the digest never
    // travelled). Both are kept, in this order, because both fire today.
    let check = get(&report, "entryCheck").unwrap_or(&serde_json::Value::Null);
    if truthy(get(check, "stale")) {
        let expected = get(check, "expected");
        let expected_field = |key: &str| {
            if truthy(expected) {
                js_str(get(expected.unwrap_or(&serde_json::Value::Null), key))
            } else {
                js_str(expected)
            }
        };
        findings.push(Finding::text(format!(
            "the delivered entry is {} bytes / sha {} but this sweep was emitted against {} / {} — every measurement below is of a stale build",
            js_str(get(check, "bytes")),
            js_str(get(check, "sha")),
            expected_field("bytes"),
            expected_field("sha")
        )));
    }

    // THE STATES THIS SHEET DECLARES AND THIS RUN NEVER PAINTED (round 51 of the standing goal), the third
    // and last surface to get the queue the panel has had since round 32. The landing's styles were a
    // `<style>` block inside `index/src/page.js` and were cropped out of the module; the landing migrated to
    // Rust (2026-09-28) and they are a stylesheet of their own at `index/landing/assets/page.css`. The JS
    // THROWS rather than measuring an empty string, because a note about zero declared families reads
    // exactly like a clean surface.
    match paths::landing_sheet() {
        Ok(css) => {
            for line in crate::marks::mark_coverage_notes(&css, &report) {
                out.note(&line);
            }
        }
        // The JavaScript THROWS here and node exits 1 with a stack trace. Printing which sheet could not be
        // read and exiting with the same code is the useful half of that, and it is not a verdict about the
        // page: the coverage note simply has no sheet.
        Err(why) => {
            out.err(&format!(
                "could not read the landing's stylesheet, so its mark-coverage note has no sheet — {why}"
            ));
            return 1;
        }
    }

    out.note(&report_summary("landing", &report));

    let texts: Vec<String> = findings.into_iter().map(|f| f.text).collect();
    verdict(
        &texts,
        "landing design sweep OK: nothing above found a defect",
        out,
    )
}
