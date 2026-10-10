//! §3 `workflow` — a cross-plugin chain: process list, a local execute, a file written then stat-ed, a memory
//! entry saved, found, listed, updated, re-found, exported, deleted, and then proved gone.
//!
//! **THE LAST CHECK IS THE ONE THE CHAIN EXISTS FOR.** `memory_delete verified` searches for the token it just
//! deleted and requires ZERO hits, because a delete that returns `ok` and leaves the entry searchable is the
//! device-caught defect this section was written after (round 294: `memory_search` hits grew across runs).
//! The self-clean is best-effort driving; whether it TOOK is a verdict, and it is here.

use crate::rows::{js_field, results_of, text_of, View};
use crate::verdict::{Check, Verdict};

pub fn judge(rows: &View, out: &mut Verdict) {
    let fx = rows.fixtures();

    // ── process_list ─────────────────────────────────────────────────────────────────────────────────
    let Some(pl) = rows.got("workflow.process_list").ok() else {
        return;
    };
    // `pl.processes ? pl.processes.length : (Array.isArray(pl) ? pl.length : -1)`
    let n: i64 = match pl.get("processes").and_then(|v| v.as_array()) {
        Some(a) => a.len() as i64,
        None => match pl.as_array() {
            Some(a) => a.len() as i64,
            None => -1,
        },
    };
    out.push(Check::pass_or_fail(
        "workflow process_list",
        n >= 0,
        format!("electron procs={n}"),
    ));

    // ── a local execute reaches the shell ────────────────────────────────────────────────────────────
    let Some(ex) = rows.got("workflow.execute").ok() else {
        return;
    };
    out.push(Check::pass_or_fail(
        "workflow local execute",
        text_of(ex).contains(&fx.workflow_marker),
        "",
    ));

    // ── a written file is the size it was written ────────────────────────────────────────────────────
    let Some(fw) = rows.got("workflow.file.write").ok() else {
        return;
    };
    let wrote = fw
        .get("ok")
        .map(|v| v.as_bool() != Some(false))
        .unwrap_or(true);
    out.push(Check::pass_or_fail("workflow file_write", wrote, ""));
    let Some(st) = rows.got("workflow.file.stat").ok() else {
        return;
    };
    // `Buffer.byteLength(cfg)` — the JSON the driver minted, whose byte length is the expectation. The driver
    // records the text it wrote (`mint.workflow_config`); the arithmetic is here.
    let cfg = rows
        .got("mint.workflow_config")
        .ok()
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let size = st.get("size").and_then(|v| v.as_u64());
    out.push(Check::pass_or_fail(
        "workflow file_stat",
        size == Some(cfg.len() as u64),
        format!("size={}", js_field(st, "size")),
    ));

    // ── memory: save, find, list ─────────────────────────────────────────────────────────────────────
    let Some(ms) = rows.got("workflow.memory.save").ok() else {
        return;
    };
    let saved = ms
        .get("ok")
        .map(|v| v.as_bool() != Some(false))
        .unwrap_or(true)
        && crate::rows::truthy(crate::rows::field(ms, "id"));
    let id = ms.get("id").cloned().unwrap_or(serde_json::Value::Null);
    out.push(Check::pass_or_fail(
        "workflow memory_save",
        saved,
        format!("id={}", js_field(ms, "id")),
    ));
    let Some(mq) = rows.got("workflow.memory.search").ok() else {
        return;
    };
    let hits = results_of(mq).len();
    out.push(Check::pass_or_fail(
        "workflow memory_search",
        hits > 0,
        format!("hits={hits}"),
    ));
    let Some(ml) = rows.got("workflow.memory.list").ok() else {
        return;
    };
    let list = results_of(ml);
    let enumerated = list.iter().any(|r| r.get("id") == Some(&id));
    out.push(Check::pass_or_fail(
        "workflow memory_list",
        enumerated,
        format!("rows={}", list.len()),
    ));

    // ── memory: update, re-find, export ──────────────────────────────────────────────────────────────
    let Some(mu) = rows.got("workflow.memory.update").ok() else {
        return;
    };
    out.push(Check::pass_or_fail(
        "workflow memory_update",
        mu.get("ok")
            .map(|v| v.as_bool() != Some(false))
            .unwrap_or(true),
        "",
    ));
    let Some(mq2) = rows.got("workflow.memory.search.updated").ok() else {
        return;
    };
    let hits2 = results_of(mq2).len();
    out.push(Check::pass_or_fail(
        "workflow memory_search updated",
        hits2 > 0,
        format!("hits={hits2}"),
    ));
    let Some(me) = rows.got("workflow.memory.export").ok() else {
        return;
    };
    let lines = me.get("lines").and_then(|v| v.as_f64());
    let exported = me
        .get("export")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let id_text = match &id {
        serde_json::Value::String(s) => s.clone(),
        other => crate::text::js_string(other),
    };
    out.push(Check::pass_or_fail(
        "workflow memory_export",
        lines.map(|l| l >= 1.0).unwrap_or(false) && exported.contains(&id_text),
        format!("lines={}", js_field(me, "lines")),
    ));

    // ── THE DELETE MUST TAKE ─────────────────────────────────────────────────────────────────────────
    let Some(mq3) = rows.got("workflow.memory.search.deleted").ok() else {
        return;
    };
    let left = results_of(mq3).len();
    out.push(Check::pass_or_fail(
        "workflow memory_delete verified",
        left == 0,
        format!("hits={left}"),
    ));
}
