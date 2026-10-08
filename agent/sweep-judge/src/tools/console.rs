//! `judge()` from `agent/scripts/console-design-sweep.mjs:145-181`.
//!
//! THE SHARED CLAUSES, NOT A COPY OF THEM: this calls the shared judge and adds only what is the console's
//! own — `navless: ["login"]` (the console's one page with no navigation BY DESIGN), the `stat-off` class
//! that is deliberately unstyled, and the contrast queue's shorter line.

use crate::js::{arr, get, js_str, slice0, truthy};
use crate::out::Out;
use crate::paths;
use crate::report::{judge_report, Finding, JudgeOpts};
use crate::summary::report_summary;
use crate::tools::{read_report, verdict};

/// The console's options.
pub fn options() -> JudgeOpts<'static> {
    JudgeOpts {
        // PREFIX, NOT EXACT: a navless entry names a FAMILY of renders — the same page in another theme, at
        // another width — so `login-dark` is covered by `login` without a second entry.
        navless: &["login"],
        prose_floor: None,
        implicit_states: &[(
            "stat-off",
            "the Overview's default tone: the base .stat-card::before already paints the faint bar that off means",
        )],
        unstyled_floor: None,
        unmeasurable_floor: None,
        ignore: &[],
    }
}

pub fn judge(file: &str, out: &mut Out) -> i32 {
    let Some(report) = read_report(file, out) else {
        return 1;
    };
    let mut findings: Vec<Finding> = judge_report(&report, &options(), out);

    // The console's own theme check, identical in shape to the panel's: a report that says it navigated to
    // dark and rendered light would be describing a page it did not render.
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
    // DELETED (round 20 of the standing goal). The number round 19 measured here was ZERO (this sweep's
    // `overview:22c/2u` passes the full rule), which is why enforcing it there was safe to do.
    //
    // At most ten, unshifted so the contrast queue leads: a report with 200 failing rows would otherwise
    // print all of them and bury the sentence the reader needs.
    for r in crate::contrast::failures(arr(get(&report, "rows")))
        .into_iter()
        .take(10)
    {
        findings.insert(
            0,
            Finding::text(format!(
                "{} {}{} {} \"{}\"",
                js_str(get(r, "cr")),
                js_str(get(r, "page")),
                if truthy(get(r, "width")) {
                    format!("@{}px", js_str(get(r, "width")))
                } else {
                    String::new()
                },
                js_str(get(r, "sel")),
                slice0(&js_str(get(r, "text")), 24)
            )),
        );
    }

    // THE STATES THIS SHEET DECLARES AND THIS RUN NEVER PAINTED (round 50 of the standing goal). The panel
    // has had this queue since round 32 — it is how `cmd-dot` was found rendering four of its six states,
    // one of them with no rule at all — and until now it lived inside the panel's sweep, so the console's
    // unrendered states were SILENT. The console's styles are SOURCE files (its dist is a pruned build
    // artifact), which is the same choice `console-marks-check.mjs` makes for the same reason.
    match paths::console_sheets() {
        Some(css) => {
            for line in crate::marks::mark_coverage_notes(&css, &report) {
                out.note(&line);
            }
        }
        // The JavaScript THROWS here and node exits 1 with a stack trace. A verdict of "findings" is the
        // right exit code for that, and printing which sheet could not be read is more use than a stack.
        None => {
            out.err("could not read the console's stylesheets under gateway/ui/src/styles — the mark-coverage note has no sheet to read");
            return 1;
        }
    }

    out.note(&report_summary("console", &report));
    if !crate::contrast::unmeasurable(arr(get(&report, "rows"))).is_empty() {
        out.note(&format!(
            "note: {} node(s) unmeasurable",
            crate::contrast::unmeasurable(arr(get(&report, "rows"))).len()
        ));
    }

    let texts: Vec<String> = findings.into_iter().map(|f| f.text).collect();
    verdict(
        &texts,
        "console design sweep OK: nothing above found a defect",
        out,
    )
}
