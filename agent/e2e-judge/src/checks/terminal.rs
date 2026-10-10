//! §1 `terminal` — a session opens, list/resize/execute agree, a background job is collected, the keystroke
//! path echoes to the screen tail, an unknown session is `evicted`, and a closed one is retained in history.
//!
//! **THE TWO CHECKS THAT MATTER MOST HERE ARE ABOUT AGREEMENT, NOT ABOUT SUCCESS** (round 96). `ex.exit_code`
//! is the answer the execute CALLER gets; `last_exit_code` on `terminal_list` is the same fact on the row every
//! client polls, which is what lets a surface say FAILED about a session whose audit trail it has never loaded.
//! What must never happen is the two audiences DISAGREEING — asserted in both directions, because "there is a
//! code" is a claim about the runner's shell and "no code was observed" is a legitimate answer that a bash PTY
//! in CI returns every time.
//!
//! **THE MARKER IS THE PROOF; THE STATE NAME IS REPORTED, NOT DEMANDED** (round 114). This check once required
//! `state === 'done'` AND the marker, and on the device a PTY execute of `Write-Output` returns
//! `state=partial, exit_code=null` EVERY TIME with the marker present and correct — measured twice in a row
//! with a real prompt gate, a retry and a 30 s timeout in place. It was never a timing flake, so the assertion
//! is about the OUTPUT and the state is recorded beside it.

use crate::rows::{self, js_field, js_get, last_of, screen_of, text_of, View};
use crate::verdict::{Check, Verdict};

pub fn judge(rows: &View, out: &mut Verdict) {
    let fx = rows.fixtures();

    // The session id is a FIELD READ off the open answer, done in the driver because it cannot type into a
    // session it cannot name. The judge reads the same answer. A driver that could not read one stopped the
    // run, and `driver.abort` is the check that names it.
    let Some(open) = rows.got("terminal.open").ok() else {
        return;
    };
    let sid = open
        .get("sid")
        .and_then(|v| v.as_str())
        .or_else(|| open.as_str())
        .unwrap_or_default()
        .to_string();

    // ── the session is in the list every client polls ────────────────────────────────────────────────
    let Some(ls) = rows.got("terminal.list.sessions").ok() else {
        return;
    };
    let sessions = rows::list_of(ls);
    let found = sessions.iter().any(|s| {
        s.get("id").and_then(|v| v.as_str()) == Some(sid.as_str())
            || s.as_str() == Some(sid.as_str())
    });
    out.push(Check::pass_or_fail(
        "terminal list contains session",
        found,
        format!("sessions={}", sessions.len()),
    ));

    // ── resize answers the one word ──────────────────────────────────────────────────────────────────
    let Some(rs) = rows.got("terminal.resize").ok() else {
        return;
    };
    out.push(Check::pass_or_fail(
        "terminal resize ok",
        rs.as_str() == Some("OK"),
        crate::text::str_trunc(rs, 20),
    ));

    let execs = rows.series_values("terminal.execute");
    let Some(ex) = last_of(&execs) else {
        return;
    };
    let ran = text_of(ex).contains(&fx.session_marker);
    out.push(Check::pass_or_fail(
        "terminal session execute",
        ran,
        format!(
            "state={} exit={}",
            js_field(ex, "state"),
            js_field(ex, "exit_code")
        ),
    ));

    let Some(list) = rows.got("terminal.row.after_exec").ok() else {
        return;
    };
    let row = rows::row_of(list, &sid);
    let agrees = row
        .as_ref()
        .map(|r| rows::same_outcome(r, ex))
        .unwrap_or(false);
    out.push(Check::pass_or_fail(
        "terminal_list agrees with the caller about the last command outcome",
        agrees,
        format!(
            "row={} caller={} state={}",
            js_get(row.as_ref(), "last_exit_code"),
            js_field(ex, "exit_code"),
            js_field(ex, "state")
        ),
    ));

    // ── a failure is the NUMBER, not a boolean ───────────────────────────────────────────────────────
    let Some(failed) = rows.got("terminal.exec.fail").ok() else {
        return;
    };
    let not_success = match failed.get("exit_code") {
        Some(v) => v.as_f64() != Some(0.0),
        None => true,
    };
    out.push(Check::pass_or_fail(
        "a failing command does not report success",
        not_success,
        format!(
            "exit={} state={}",
            js_field(failed, "exit_code"),
            js_field(failed, "state")
        ),
    ));
    let Some(list) = rows.got("terminal.row.after_fail").ok() else {
        return;
    };
    let bad_row = rows::row_of(list, &sid);
    let agrees = bad_row
        .as_ref()
        .map(|r| rows::same_outcome(r, failed))
        .unwrap_or(false);
    out.push(Check::pass_or_fail(
        "and the session row carries whatever the caller was told",
        agrees,
        format!(
            "row={} caller={}",
            js_get(bad_row.as_ref(), "last_exit_code"),
            js_field(failed, "exit_code")
        ),
    ));

    // ── the background job ───────────────────────────────────────────────────────────────────────────
    let Some(bg) = rows.got("terminal.exec.bg").ok() else {
        return;
    };
    let running = bg.get("status").and_then(|v| v.as_str()) == Some("running")
        || bg.get("job_id").map(rows::truthy).unwrap_or(false);
    out.push(Check::pass_or_fail(
        "terminal run_in_background",
        running,
        format!("job={}", js_field(bg, "job_id")),
    ));
    let Some(rd) = rows.got("terminal.read.bg").ok() else {
        return;
    };
    let collected = text_of(rd);
    out.push(Check::pass_or_fail(
        "terminal background collect",
        collected.contains(&fx.background_marker),
        format!("len={}", collected.chars().count()),
    ));

    // ── the keystroke path, not the execute path ─────────────────────────────────────────────────────
    let write_marker = rows
        .got("mint.terminal_write")
        .ok()
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let Some(wr) = rows.got("terminal.write").ok() else {
        return;
    };
    out.push(Check::pass_or_fail(
        "terminal write ok",
        wr.as_str() == Some("OK"),
        crate::text::str_trunc(wr, 20),
    ));
    let Some(sc) = rows.got("terminal.screen").ok() else {
        return;
    };
    let screen = screen_of(sc);
    out.push(Check::pass_or_fail(
        "terminal screen shows write",
        screen.contains(&write_marker),
        format!("len={}", screen.chars().count()),
    ));

    // ── an unknown session is a data-shaped answer, not a throw ──────────────────────────────────────
    let Some(bad) = rows.got("terminal.read.unknown").ok() else {
        return;
    };
    out.push(Check::pass_or_fail(
        "terminal read unknown evicted",
        bad.get("evicted").and_then(|v| v.as_bool()) == Some(true),
        crate::text::json_trunc(bad, 60),
    ));

    // ── a closed session stays queryable ─────────────────────────────────────────────────────────────
    let Some(hist) = rows.got("terminal.history").ok() else {
        return;
    };
    let hrow = hist.as_array().and_then(|a| {
        a.iter()
            .find(|h| h.get("id").and_then(|v| v.as_str()) == Some(&sid))
    });
    let retained = hrow.and_then(|h| h.get("status")).and_then(|v| v.as_str()) == Some("closed");
    out.push(Check::pass_or_fail(
        "terminal history retains closed",
        retained,
        hrow.map(|h| js_field(h, "status"))
            .unwrap_or_else(|| "missing".to_string()),
    ));
}
