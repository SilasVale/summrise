//! §4c `evidence` — an AI screenshot lands in the pwout directory, and `/api/browser/pwshots` (the Evidence
//! drawer's data source) lists it.
//!
//! **THE SCREENSHOT IS TAKEN THROUGH `browser_run_script`, SO THE PROOF IS TWO-SIDED**: the payload must have
//! exited 0 having printed `SHOT_SAVED`, and the API the DRAWER reads must then list the file by name. A
//! screenshot that lands on disk and is never listed is a drawer that shows nothing, which is the defect this
//! section exists for — and a run that asserts only the first half would not see it.

use crate::rows::View;
use crate::verdict::{Check, Verdict};

pub fn judge(rows: &View, out: &mut Verdict) {
    let name = rows
        .got("mint.evidence_shot")
        .ok()
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let Some(br) = rows.got("evidence.screenshot").ok() else {
        return;
    };
    let out_text = br
        .get("stdout")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let exit = br.get("exit_code").and_then(|v| v.as_f64());
    let saved = exit == Some(0.0) && out_text.contains("SHOT_SAVED");
    out.push(Check::pass_or_fail(
        "evidence screenshot saved",
        saved,
        crate::text::truncate(out_text.trim(), 60),
    ));

    let Some(shots) = rows.got("evidence.pwshots").ok() else {
        return;
    };
    // `(shotsJson && shotsJson.shots) || []` — an unreadable or non-ok answer is an empty list, which fails
    // the check below rather than skipping it.
    let list: Vec<serde_json::Value> = shots
        .get("shots")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let listed = list.iter().any(|f| {
        f.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .contains(&name)
    });
    out.push(Check::pass_or_fail(
        "evidence pwshots lists it",
        listed,
        format!(
            "shots={} looking={}",
            list.len(),
            crate::text::truncate(&name, 30)
        ),
    ));
}
