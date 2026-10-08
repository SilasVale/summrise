//! `reportSummary(label, report)` — ported from `agent/scripts/lib/design-sweep.mjs:2364-2379`.
//!
//! How a sweep reports its result, so three tools read the same way. The middle dot is U+00B7 and the
//! separators are part of the sentence: this line is what a reader sees before any finding.

use crate::js::{arr, at, get, js_str, num};
use serde_json::Value;

pub fn report_summary(label: &str, report: &Value) -> String {
    let surfaces = arr(get(report, "surfaces"));
    let rows = arr(get(report, "rows"));
    let blind = rows
        .iter()
        .filter(|r| matches!(get(r, "cr"), Some(Value::Null)))
        .count();
    // THE WORST LINE OF PROSE, in the summary, because a floor that only SPEAKS when it is crossed tells a
    // reader nothing about how close the rest of the run is to it (round 265). Absent when a sweep did not
    // measure it. The reduce starts at `{cpl: 0}`, so a report with no measure block prints no segment.
    let mut worst: Option<&Value> = None;
    let mut worst_cpl = 0.0_f64;
    for s in surfaces {
        let Some(first) = arr(at(get(s, "measure"), "worst")).first() else {
            continue;
        };
        let cpl = num(get(first, "cpl"));
        if cpl.map(|c| c > worst_cpl).unwrap_or(false) {
            worst = Some(first);
            worst_cpl = cpl.unwrap_or(0.0);
        }
    }
    let prose = if worst_cpl != 0.0 {
        format!(
            " · worst prose line {} chars ({})",
            js_str(Some(&serde_json::json!(worst_cpl))),
            js_str(worst.and_then(|w| get(w, "sel")))
        )
    } else {
        String::new()
    };
    let unmeasurable = if blind > 0 {
        format!(" · {blind} unmeasurable")
    } else {
        String::new()
    };
    format!(
        "{label}: {} text nodes · {} surface(s) · {} name checks{prose}{unmeasurable}",
        rows.len(),
        surfaces.len(),
        arr(get(report, "names")).len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_summary_carries_the_counts_a_reader_needs() {
        let report = json!({
            "rows": [{"cr": 1.0}, {"cr": null}],
            "surfaces": [{"measure": {"worst": [{"sel": "p.x", "cpl": 73}]}}],
            "names": [{}],
        });
        assert_eq!(
            report_summary("panel", &report),
            "panel: 2 text nodes · 1 surface(s) · 1 name checks · worst prose line 73 chars (p.x) · 1 unmeasurable"
        );
    }

    #[test]
    fn the_prose_and_unmeasurable_segments_are_absent_when_there_is_nothing_to_say() {
        let report = json!({"rows": [{"cr": 7.0}], "surfaces": [{}], "names": []});
        assert_eq!(
            report_summary("console", &report),
            "console: 1 text nodes · 1 surface(s) · 0 name checks"
        );
    }

    #[test]
    fn a_row_that_is_not_a_number_is_not_unmeasurable() {
        // Only a strict `null` counts, which is what the JS filter says.
        let report = json!({"rows": [{"cr": "0"}, {}], "surfaces": [], "names": []});
        assert_eq!(
            report_summary("landing", &report),
            "landing: 2 text nodes · 0 surface(s) · 0 name checks"
        );
    }
}
