//! The three helpers the judges read out of `agent/scripts/lib/contrast-probe.mjs`.
//!
//! They are small, and they are here rather than in `report.rs` because they belong to the CONTRAST
//! instrument's vocabulary: `aaThreshold` is the WCAG bar, `failures` is "under its bar and not
//! deliberately inactive", `unmeasurable` is "the probe could not read a ratio at all".

use crate::js::{is_nullish, num};
use serde_json::Value;

/// The WCAG AA bar for TEXT at this size/weight: 3.0 counts as "large" at >=24px, or >=18.66px when bold;
/// everything else needs 4.5.
pub fn aa_threshold(font_size: Option<&Value>, font_weight: Option<&Value>) -> f64 {
    let size = num(font_size).unwrap_or(f64::NAN);
    let weight = num(font_weight).unwrap_or(f64::NAN);
    if size >= 24.0 || (size >= 18.66 && weight >= 700.0) {
        3.0
    } else {
        4.5
    }
}

/// `rows.filter((r) => r.cr !== null && !r.inactive && r.cr < (r.need ?? aaThreshold(r.size, r.weight)))`.
///
/// The strict `!== null` is not enough on its own to exclude an absent `cr`, but the comparison that
/// follows is false for NaN, which is what `undefined` coerces to — the same shape the JS relies on.
pub fn failures(rows: &[Value]) -> Vec<&Value> {
    rows.iter()
        .filter(|r| {
            let cr = crate::js::get(r, "cr");
            if is_nullish(cr) {
                return false;
            }
            if crate::js::truthy(crate::js::get(r, "inactive")) {
                return false;
            }
            let need = crate::js::get(r, "need");
            let bar = if is_nullish(need) {
                aa_threshold(crate::js::get(r, "size"), crate::js::get(r, "weight"))
            } else {
                num(need).unwrap_or(f64::NAN)
            };
            num(cr).map(|c| c < bar).unwrap_or(false)
        })
        .collect()
}

/// `rows.filter((r) => r.cr === null)` — strictly null, not absent: a row that never carried a ratio was
/// not counted as unmeasurable by the JS, and this port keeps that distinction.
pub fn unmeasurable(rows: &[Value]) -> Vec<&Value> {
    rows.iter()
        .filter(|r| matches!(crate::js::get(r, "cr"), Some(Value::Null)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn aa_threshold_follows_the_wcag_size_and_weight_rules() {
        assert_eq!(aa_threshold(Some(&json!(11)), Some(&json!("400"))), 4.5);
        assert_eq!(aa_threshold(Some(&json!(24)), Some(&json!("400"))), 3.0);
        assert_eq!(aa_threshold(Some(&json!(18.66)), Some(&json!("700"))), 3.0);
        assert_eq!(aa_threshold(Some(&json!(18.66)), Some(&json!("400"))), 4.5);
        // 23px BOLD is still "large" (>= 18.66px at weight 700), which is the half of the rule that is
        // easy to get backwards; 18px bold is not.
        assert_eq!(aa_threshold(Some(&json!(23)), Some(&json!(700))), 3.0);
        assert_eq!(aa_threshold(Some(&json!(18)), Some(&json!(700))), 4.5);
        // A numeric weight is the common case in a real report; a string one is what the probe emits.
        assert_eq!(aa_threshold(Some(&json!(20)), Some(&json!(700))), 3.0);
    }

    #[test]
    fn failures_uses_the_rows_own_bar_when_it_has_one() {
        let rows = vec![
            json!({"cr": 2.5, "need": 4.5}),
            json!({"cr": 7.0, "need": 4.5}),
            json!({"cr": null, "need": 4.5}),
            json!({"need": 4.5}),
            json!({"cr": 2.5, "need": 4.5, "inactive": true}),
            json!({"cr": 2.5, "size": 24, "weight": "400"}),
            json!({"cr": 3.5, "size": 24, "weight": "400"}),
        ];
        let got = failures(&rows);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0]["cr"], json!(2.5));
        assert_eq!(got[1]["cr"], json!(2.5));
    }

    #[test]
    fn unmeasurable_counts_only_a_strict_null() {
        let rows = vec![
            json!({"cr": null}),
            json!({}),
            json!({"cr": 0}),
            json!({"cr": 1.0}),
        ];
        assert_eq!(unmeasurable(&rows).len(), 1);
    }
}
