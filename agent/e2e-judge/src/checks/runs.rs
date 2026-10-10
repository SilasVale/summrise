//! §8 `runs` — one AI execution gets a name, and the work wears it.
//!
//! **THE PROPERTY IS A JOIN ACROSS LAYERS, WHICH IS WHY IT IS HERE.** A tool mints the id, a producer stamps
//! it, and `/api/operation` returns the boundary and the stamped event on one axis. Each layer can be green
//! alone while the id is minted and forgotten — which is exactly what happened before the feature was wired,
//! and what no per-layer test could see.
//!
//! **AND THE NEGATIVE HALF IS HALF THE SECTION.** Work sent WITHOUT a run must stay unattributed rather than
//! being absorbed into whatever run happens to precede it, so `run_id === null` is asserted as strictly as the
//! stamped case. Platform-neutral: terminal and HTTP only, no Electron and no screenshots, which is what lets
//! it be one of the two sections CI runs.

use crate::rows::{js_field, js_json_or_empty, View};
use crate::verdict::{Check, Verdict};

pub fn judge(rows: &View, out: &mut Verdict) {
    let fx = rows.fixtures();

    let Some(open) = rows.got("runs.open").ok() else {
        return;
    };
    let sid = match open {
        serde_json::Value::String(s) => s.clone(),
        other => other
            .get("sid")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    };
    out.push(Check::pass_or_fail(
        "runs: session opens",
        !sid.is_empty(),
        format!("sid={}", crate::text::js_string(open)),
    ));

    let label = rows
        .got("mint.runs_label")
        .ok()
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let Some(begun) = rows.got("runs.begin").ok() else {
        return;
    };
    let run_id = begun
        .get("run_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    out.push(Check::pass_or_fail(
        "runs: run_begin mints an id",
        run_id.starts_with("run-"),
        format!("id={run_id}"),
    ));

    // ── the timeline, first snapshot ─────────────────────────────────────────────────────────────────
    let Some(op) = rows.got("runs.operation.before_end").ok() else {
        return;
    };
    let runs = op
        .get("runs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let events = op
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let begin = runs
        .iter()
        .find(|r| js_field(r, "kind") == "run/begin" && js_field(r, "run_id") == run_id);
    out.push(Check::pass_or_fail(
        "runs: the boundary is on the timeline",
        begin
            .map(|b| js_field(b, "label") == label)
            .unwrap_or(false),
        match begin {
            Some(b) => format!("label={}", js_field(b, "label")),
            None => format!("no run/begin for {run_id}"),
        },
    ));
    out.push(Check::pass_or_fail(
        "runs: the run carries the goal it was given",
        begin
            .map(|b| js_field(b, "goal") == fx.runs_goal)
            .unwrap_or(false),
        match begin {
            Some(b) => format!("goal={}", js_field(b, "goal")),
            None => "missing".to_string(),
        },
    ));

    let mine = events
        .iter()
        .find(|e| js_field(e, "command") == fx.runs_inside_command);
    out.push(Check::pass_or_fail(
        "runs: the command wears the id run_begin minted",
        mine.map(|e| js_field(e, "run_id") == run_id)
            .unwrap_or(false),
        match mine {
            Some(e) => format!("run_id={}", js_field(e, "run_id")),
            None => "command not on the timeline".to_string(),
        },
    ));

    let loose = events
        .iter()
        .find(|e| js_field(e, "command") == fx.runs_loose_command);
    out.push(Check::pass_or_fail(
        "runs: work sent without a run is NOT absorbed into one",
        loose
            .map(|e| matches!(e.get("run_id"), Some(serde_json::Value::Null)))
            .unwrap_or(false),
        match loose {
            Some(e) => format!("run_id={}", js_json_or_empty(e.get("run_id"))),
            None => "command not on the timeline".to_string(),
        },
    ));

    // The boundary precedes the work it brackets, ordered by ts_ms.
    match (begin, mine) {
        (Some(b), Some(m)) => {
            let bt = b.get("ts_ms").and_then(|v| v.as_f64());
            let mt = m.get("ts_ms").and_then(|v| v.as_f64());
            out.push(Check::pass_or_fail(
                "runs: the boundary precedes the work it names",
                bt.zip(mt).map(|(b, m)| b <= m).unwrap_or(false),
                format!(
                    "begin={} event={}",
                    js_field(b, "ts_ms"),
                    js_field(m, "ts_ms")
                ),
            ));
        }
        _ => out.push(Check::fail(
            "runs: the boundary precedes the work it names",
            "prerequisites missing",
        )),
    }

    // ── closed, and the closure is visible ───────────────────────────────────────────────────────────
    let Some(op) = rows.got("runs.operation.after_end").ok() else {
        return;
    };
    let end = op.get("runs").and_then(|v| v.as_array()).and_then(|a| {
        a.iter()
            .find(|r| js_field(r, "kind") == "run/end" && js_field(r, "run_id") == run_id)
    });
    out.push(Check::pass_or_fail(
        "runs: run_end closes the run with its outcome",
        end.map(|e| js_field(e, "outcome") == "done")
            .unwrap_or(false),
        match end {
            Some(e) => format!("outcome={}", js_field(e, "outcome")),
            None => "no run/end".to_string(),
        },
    ));

    // ── A LABEL, NEVER A CREDENTIAL ──────────────────────────────────────────────────────────────────
    //
    // An id that was never minted is ACCEPTED and reported as `known:false` rather than refused: the runs log
    // is best-effort, so a client that lost its begin record must still be able to close. The auth half of the
    // rule is covered by the harness's own 401 checks, which is why this asserts the data shape and not the
    // security property.
    let Some(unknown) = rows.got("runs.end.unknown").ok() else {
        return;
    };
    out.push(Check::pass_or_fail(
        "runs: an unregistered id is accepted and reported as unknown",
        unknown.get("known").and_then(|v| v.as_bool()) == Some(false),
        crate::text::js_json(unknown),
    ));
}
