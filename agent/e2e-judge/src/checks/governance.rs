//! §8 `governance` — the operator states a goal, arms the gate, approves a command with a grant, and the audit
//! trail explains WHY a later command ran unasked.
//!
//! **THE TRAIL ASSERTIONS ARE THE POINT OF THIS SECTION.** Every other check here passed on an early build
//! while ARMING THE GATE left no trace at all: the hold was recorded, the goal was recorded, and the switch
//! that decides whether commands run unasked was invisible. Each piece was individually correct and only the
//! joined-up history was missing, so the `posture.join(',')` equality — armed, asked, approved, granted,
//! revoked, disarmed, in that order — is what turns that from a discovery into a failure.
//!
//! **AND IT IS ONE OF THE TWO SECTIONS CI RUNS** (`--only governance,runs`), which is why the outcome-agreement
//! check appears here as well as in `terminal`: a check in a section nothing invokes is a check that never runs.

use crate::rows::{js_field, js_get, js_json_or_empty, row_of, same_outcome, View};
use crate::verdict::{Check, Verdict};

pub fn judge(rows: &View, out: &mut Verdict) {
    let fx = rows.fixtures();

    // ── the session opens ────────────────────────────────────────────────────────────────────────────
    let Some(open) = rows.got("gov.open").ok() else {
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
        "gov: session opens",
        !sid.is_empty(),
        format!("sid={}", crate::text::js_string(open)),
    ));

    // ── the goal ─────────────────────────────────────────────────────────────────────────────────────
    let Some(g) = rows.got("gov.control.goal").ok() else {
        return;
    };
    let status = g.get("status").and_then(|v| v.as_f64());
    let json = g.get("json");
    let accepted = status == Some(200.0)
        && json.map(crate::rows::truthy).unwrap_or(false)
        && json.and_then(|j| j.get("goal")).and_then(|v| v.as_str()) == Some(fx.gov_goal.as_str());
    out.push(Check::pass_or_fail(
        "gov: goal accepted",
        accepted,
        js_json_or_empty(json),
    ));
    // A partial patch must not clear what it did not mention.
    let holder_alone = json
        .map(|j| matches!(j.get("held_by_human"), Some(serde_json::Value::Null)))
        .unwrap_or(false);
    out.push(Check::pass_or_fail(
        "gov: partial patch leaves holder alone",
        holder_alone,
        js_json_or_empty(json),
    ));

    let Some(rows_list) = rows.got("gov.list.after_goal").ok() else {
        return;
    };
    let row = row_of(rows_list, &sid);
    let reads_goal = row
        .as_ref()
        .map(|r| js_field(r, "goal") == fx.gov_goal)
        .unwrap_or(false);
    out.push(Check::pass_or_fail(
        "gov: the AI reads the goal off terminal_list",
        reads_goal,
        match row.as_ref() {
            Some(r) => format!("goal={}", js_field(r, "goal")),
            None => "session not listed".to_string(),
        },
    ));

    // ── the gate publishes the pending command ───────────────────────────────────────────────────────
    let mut apid = String::new();
    for v in rows.series_values("gov.pending") {
        let r = row_of(v, &sid);
        let id = r
            .as_ref()
            .and_then(|r| r.get("pending_approval"))
            .and_then(|p| p.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        apid = id.clone();
        if !apid.is_empty() {
            break;
        }
    }
    out.push(Check::pass_or_fail(
        "gov: the gate publishes the pending command",
        !apid.is_empty(),
        format!("id={apid}"),
    ));

    let Some(decided) = rows.got("gov.approval").ok() else {
        return;
    };
    out.push(Check::pass_or_fail(
        "gov: approve+grant decides",
        decided.get("decided").and_then(|v| v.as_bool()) == Some(true),
        js_json_or_empty(Some(decided)),
    ));
    // THE GRANT IS THE COMMAND WORD, DERIVED BY THE SERVER rather than sent by the client: the client asked for
    // `echo governed` and the grant it got back must name `echo`. The word comes from the plan's own command.
    let grants = decided
        .get("approval_grants")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let worded = grants
        .iter()
        .any(|g| g.as_str() == Some(fx.gov_grant_word.as_str()));
    out.push(Check::pass_or_fail(
        "gov: the grant is the command WORD, server-derived",
        worded,
        js_json_or_empty(decided.get("approval_grants")),
    ));

    // ── the approved command ran, and the row agrees about its outcome ───────────────────────────────
    let exec = rows.find("gov.exec.result");
    let exec_ok = exec.map(|o| o.error.is_none()).unwrap_or(false);
    let exec_r = exec.and_then(|o| o.result.as_ref());
    let exec_detail = if exec_ok {
        match exec_r {
            Some(r) => format!("state={}", js_field(r, "state")),
            None => "state=undefined".to_string(),
        }
    } else {
        exec.and_then(|o| o.error.clone()).unwrap_or_default()
    };
    out.push(Check::pass_or_fail(
        "gov: the approved command ran",
        exec_ok,
        exec_detail,
    ));

    let Some(gov_rows) = rows.got("gov.list.after_exec").ok() else {
        return;
    };
    let gov_row = row_of(gov_rows, &sid);
    let agrees = exec_ok
        && gov_row
            .as_ref()
            .zip(exec_r)
            .map(|(r, e)| same_outcome(r, e))
            .unwrap_or(false);
    out.push(Check::pass_or_fail(
        "gov: the session row agrees with the caller about the last command outcome",
        agrees,
        format!(
            "row={} caller={} state={}",
            js_get(gov_row.as_ref(), "last_exit_code"),
            match exec_r {
                Some(r) => js_field(r, "exit_code"),
                None => "undefined".to_string(),
            },
            match exec_r {
                Some(r) => js_field(r, "state"),
                None => "undefined".to_string(),
            }
        ),
    ));

    // ── the granted family does NOT ask again ────────────────────────────────────────────────────────
    let sibling = rows.got("gov.exec.sibling").ok();
    let dt = rows.got("gov.sibling_ms").ok().and_then(|v| v.as_f64());
    let ceiling = rows.budgets().gov_sibling_ms as f64;
    out.push(Check::pass_or_fail(
        "gov: a granted sibling runs without asking",
        sibling.is_some() && dt.map(|d| d < ceiling).unwrap_or(false),
        format!("{}ms", dt.map(|d| d as i64).unwrap_or(0)),
    ));

    // ── revoke, then disarm ──────────────────────────────────────────────────────────────────────────
    let Some(rev) = rows.got("gov.grants.revoke").ok() else {
        return;
    };
    let emptied = rev
        .get("approval_grants")
        .and_then(|v| v.as_array())
        .map(|a| a.is_empty())
        .unwrap_or(false);
    out.push(Check::pass_or_fail(
        "gov: revoke-all empties the grants",
        emptied,
        js_json_or_empty(rev.get("approval_grants")),
    ));

    // ── THE TRAIL ────────────────────────────────────────────────────────────────────────────────────
    let Some(sess) = rows.got("gov.session").ok() else {
        return;
    };
    let events = sess
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let posture: Vec<String> = events
        .iter()
        .filter(|e| js_field(e, "kind") == "approval")
        .map(|e| {
            format!(
                "{}:{}",
                js_field(e, "status"),
                e.get("text").and_then(|v| v.as_str()).unwrap_or_default()
            )
        })
        .collect();
    let want_posture = format!(
        "armed:,asked:{cmd},approved:{cmd},granted:{word},revoked:,disarmed:",
        cmd = fx.gov_command,
        word = fx.gov_grant_word
    );
    out.push(Check::pass_or_fail(
        "gov: the trail records the whole approval posture",
        posture.join(",") == want_posture,
        posture.join(","),
    ));
    let armed = posture.iter().filter(|p| p.as_str() == "armed:").count();
    out.push(Check::pass_or_fail(
        "gov: a repeated arm request is not recorded twice",
        armed == 1,
        format!("armed x{armed}"),
    ));

    let start = events.iter().find(|e| {
        js_field(e, "kind") == "command/start" && js_field(e, "command") == fx.gov_command
    });
    out.push(Check::pass_or_fail(
        "gov: intent is recorded beside the command",
        start
            .map(|s| js_field(s, "intent") == fx.gov_intent)
            .unwrap_or(false),
        match start {
            Some(s) => format!("intent={}", js_field(s, "intent")),
            None => "no command/start".to_string(),
        },
    ));
    out.push(Check::pass_or_fail(
        "gov: the alternatives are recorded",
        start
            .and_then(|s| s.get("considered"))
            .and_then(|v| v.as_array())
            .map(|a| a.len() == fx.gov_considered.len())
            .unwrap_or(false),
        match start {
            Some(s) => js_json_or_empty(s.get("considered")),
            None => "no command/start".to_string(),
        },
    ));

    let goal_events: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| js_field(e, "kind") == "goal")
        .collect();
    let durable = goal_events.len() == 1 && js_field(goal_events[0], "text") == fx.gov_goal;
    let texts = serde_json::Value::Array(
        goal_events
            .iter()
            .map(|e| e.get("text").cloned().unwrap_or(serde_json::Value::Null))
            .collect(),
    );
    out.push(Check::pass_or_fail(
        "gov: the goal statement is durable",
        durable,
        crate::text::js_json(&texts),
    ));
}
