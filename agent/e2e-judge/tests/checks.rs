//! THE CHECKS, AGAINST SYNTHETIC ROWS — the whole judge, with no device, no browser and no agent.
//!
//! **WHY THIS IS THE ORACLE FOR NOW, AND WHAT IT IS NOT.** The 75 checks were ported out of
//! `agent/scripts/e2e/e2e.js`, and the thing that would prove the port reproduces the JavaScript is a
//! RECORDED RUN — the 906 KB `e2e-rec.json` the `--record` mode wrote for exactly this purpose. That recording
//! is stranded on a device that cannot reach the relay (see `agent/scripts/e2e/README.md`), so the equivalence
//! oracle is NOT wired yet, and until it is, these are the tests the verdicts rest on: a synthetic run that is
//! green in every section, and one planted defect per check that must turn that check red.
//!
//! **A GREEN FIXTURE ALONE WOULD PROVE ALMOST NOTHING** — a judge that always says PASS would satisfy it. So
//! every check in `MUTATIONS` gets a defect planted in its own input and must fail on it. That table is the
//! suite's real spec: it names the input each check depends on, which is also how the next reader finds out
//! what a check is actually about.
//!
//! **AND THE FIXTURE IS DELIBERATELY SHAPED LIKE THE DEVICE, NOT LIKE A HAPPY PATH.** `terminal exec` ends
//! `state=partial` with `exit_code: null` on BOTH the caller and the row, because that is what a PTY on the
//! device does (round 114, measured twice) — a fixture that reported `done` and `0` would be testing a device
//! nobody has.

use base64::Engine;
use serde_json::{json, Value};
use summrise_e2e_judge::plan::{plan_from_cli, Fixtures, Plan};
use summrise_e2e_judge::rows::{Observation, Rows};
use summrise_e2e_judge::verdict::{Outcome, Verdict};

// ── building a run ─────────────────────────────────────────────────────────────────────────────────────────

fn o(id: &str, result: Value) -> Observation {
    Observation {
        id: id.to_string(),
        args: None,
        result: Some(result),
        error: None,
        at_ms: None,
    }
}

fn oa(id: &str, args: Value, result: Value) -> Observation {
    Observation {
        id: id.to_string(),
        args: Some(args),
        result: Some(result),
        error: None,
        at_ms: None,
    }
}

fn plan() -> Plan {
    plan_from_cli(None, false).unwrap()
}

fn judge(obs: Vec<Observation>) -> Verdict {
    judge_with(plan(), obs)
}

fn judge_with(p: Plan, obs: Vec<Observation>) -> Verdict {
    let doc = Rows {
        schema: summrise_e2e_judge::rows::SCHEMA,
        base: "http://127.0.0.1:18080".to_string(),
        plan: p,
        observations: obs,
    };
    summrise_e2e_judge::judge(&doc)
}

fn fx() -> Fixtures {
    Fixtures::default()
}

// ── the green run: every section, every check, as the device answers ───────────────────────────────────────

fn green() -> Vec<Observation> {
    let f = fx();
    let cfg = "{\"e2e\":true,\"ts\":1759999999999}";
    let total = f.transfer_page_bytes * f.transfer_pages as usize;
    let page = base64::engine::general_purpose::STANDARD.encode(vec![f.transfer_fill; total]);

    let mut v: Vec<Observation> = vec![
        // ── terminal ──
        o("terminal.open", json!("sess-1")),
        o("terminal.prompt.0", json!({ "text": "PS C:\\Users\\d1>" })),
        o("terminal.list.sessions", json!([{ "id": "sess-1" }])),
        o("terminal.resize", json!("OK")),
        o(
            "terminal.execute.0",
            json!({ "state": "partial", "exit_code": null, "text": "E2E-SESSION-OK\r\n" }),
        ),
        o(
            "terminal.row.after_exec",
            json!([{ "id": "sess-1", "last_exit_code": null }]),
        ),
        o(
            "terminal.exec.fail",
            json!({ "state": "partial", "exit_code": 1, "text": "not recognized" }),
        ),
        o(
            "terminal.row.after_fail",
            json!([{ "id": "sess-1", "last_exit_code": 1 }]),
        ),
        o(
            "terminal.exec.bg",
            json!({ "status": "running", "job_id": "job-7" }),
        ),
        o(
            "terminal.read.bg",
            json!({ "text": "sleeping...\r\nE2E-BG-DONE\r\n" }),
        ),
        o("mint.terminal_write", json!("E2E-WRITE-1759999999999")),
        o("terminal.write", json!("OK")),
        o(
            "terminal.screen",
            json!({ "screen": "PS> Write-Output \"E2E-WRITE-1759999999999\"\r\nE2E-WRITE-1759999999999\r\n" }),
        ),
        o("terminal.read.unknown", json!({ "evicted": true })),
        o("terminal.close", json!("OK")),
        o(
            "terminal.history",
            json!([{ "id": "sess-1", "status": "closed" }]),
        ),
        // ── file ──
        o(
            "file.write.page.0",
            json!({ "ok": true, "bytes": f.transfer_page_bytes }),
        ),
        o(
            "file.write.page.1",
            json!({ "ok": true, "bytes": f.transfer_page_bytes }),
        ),
        o("file.stat", json!({ "size": total })),
        o("file.read.raw", json!({ "data": page })),
        o(
            "file.list",
            json!({ "entries": [{ "name": "e2e_suite_transfer.bin" }] }),
        ),
        o(
            "file.stat.missing",
            json!({ "ok": false, "error": "系统找不到指定的文件" }),
        ),
        o("file.text.write", json!({ "ok": true })),
        o(
            "file.text.read",
            json!({ "text": "E2E-TEXT-OK", "bytes": 11 }),
        ),
        // ── workflow ──
        o(
            "workflow.process_list",
            json!({ "processes": [{}, {}, {}] }),
        ),
        o("workflow.execute", json!({ "text": "E2E-WF-1\r\n" })),
        o("mint.workflow_config", json!(cfg)),
        o("workflow.file.write", json!({ "ok": true })),
        o("workflow.file.stat", json!({ "size": cfg.len() })),
        o("mint.memory_token", json!("toolchain-1759999999999")),
        o("workflow.memory.save", json!({ "ok": true, "id": "mem-1" })),
        o(
            "workflow.memory.search",
            json!({ "results": [{ "id": "mem-1" }] }),
        ),
        o(
            "workflow.memory.list",
            json!({ "results": [{ "id": "mem-1" }] }),
        ),
        o("mint.memory_update", json!("toolchain-upd-1759999999999")),
        o("workflow.memory.update", json!({ "ok": true })),
        o(
            "workflow.memory.search.updated",
            json!({ "results": [{ "id": "mem-1" }] }),
        ),
        o(
            "workflow.memory.export",
            json!({ "lines": 3, "export": "{\"id\":\"mem-1\"}\n" }),
        ),
        o("workflow.memory.delete", json!({ "ok": true })),
        o("workflow.memory.search.deleted", json!({ "results": [] })),
        // ── panel ──
        o("mint.panel_marker", json!("PANEL-VIS-1759999999999")),
        o("panel.open", json!("sess-2")),
        o(
            "panel.exec",
            json!({ "state": "partial", "text": "PANEL-VIS-1759999999999\r\n" }),
        ),
        o(
            "panel.cdp.targets",
            json!([{ "url": "http://127.0.0.1:9333/desktop/index.html" }]),
        ),
        o("panel.rail.click", json!("clicked")),
        o("panel.tab.click", json!("clicked-last-session")),
        o(
            "panel.xterm.0",
            json!("PS> Write-Output \"PANEL-VIS-1759999999999\"\nPANEL-VIS-1759999999999\n"),
        ),
        o("panel.close", json!("OK")),
        // ── evidence ──
        o("evidence.rail.click", json!(true)),
        o(
            "mint.evidence_shot",
            json!("e2e_evidence_1759999999999.png"),
        ),
        o(
            "evidence.screenshot",
            json!({ "exit_code": 0, "stdout": "SHOT_SAVED\n" }),
        ),
        o(
            "evidence.pwshots",
            json!({ "shots": [{ "name": "e2e_evidence_1759999999999.png" }] }),
        ),
        // ── browser ──
        o(
            "browser.pw_info",
            json!({ "pw_dir": "D:\\Summrise\\components\\playwright", "playwright_core_version": "1.2.3" }),
        ),
        o(
            "browser.script.fail",
            json!({ "exit_code": 1, "stdout": "E2E-FAIL-PATH\nFAIL:e2e-intentional\n" }),
        ),
        o(
            "browser.script.drive",
            json!({ "exit_code": 0, "stdout": "TITLE=Example Domain\n" }),
        ),
        o(
            "browser.cdp.targets",
            json!([{ "url": "http://127.0.0.1:9333/desktop/index.html" }]),
        ),
        o(
            "browser.bar.poll.0",
            json!("{\"onBrowser\":true,\"url\":\"https://www.example.com/\"}"),
        ),
        o("mint.focus_marker", json!("focus-trap-1759999999999")),
        o("browser.trap.focus", json!("focused")),
        o(
            "browser.trap.nav",
            json!({ "exit_code": 0, "stdout": "NAV-OK\n" }),
        ),
        o(
            "browser.trap.bar.0",
            json!("{\"url\":\"https://example.com/focus-trap-1759999999999\"}"),
        ),
        o("browser.trap.blur", json!("ok")),
        // ── governance ──
        o("gov.open", json!("sess-3")),
        o(
            "gov.control.goal",
            json!({ "status": 200, "json": { "goal": f.gov_goal, "held_by_human": null } }),
        ),
        o(
            "gov.list.after_goal",
            json!([{ "id": "sess-3", "goal": f.gov_goal }]),
        ),
        o(
            "gov.control.arm.0",
            json!({ "status": 200, "json": { "approval_required": true } }),
        ),
        o(
            "gov.control.arm.1",
            json!({ "status": 200, "json": { "approval_required": true } }),
        ),
        o(
            "gov.exec.result",
            json!({ "state": "partial", "exit_code": null }),
        ),
        o(
            "gov.pending.0",
            json!([{ "id": "sess-3", "pending_approval": { "id": "appr-1" } }]),
        ),
        o(
            "gov.approval",
            json!({ "decided": true, "approval_grants": ["echo"] }),
        ),
        o(
            "gov.list.after_exec",
            json!([{ "id": "sess-3", "last_exit_code": null }]),
        ),
        o("gov.exec.sibling", json!({ "text": "granted-sibling\r\n" })),
        o("gov.sibling_ms", json!(120)),
        o("gov.grants.revoke", json!({ "approval_grants": [] })),
        o(
            "gov.control.disarm",
            json!({ "status": 200, "json": { "approval_required": false } }),
        ),
        o(
            "gov.session",
            json!({ "events": [
            { "kind": "approval", "status": "armed", "text": "" },
            { "kind": "approval", "status": "asked", "text": "echo governed" },
            { "kind": "approval", "status": "approved", "text": "echo governed" },
            { "kind": "approval", "status": "granted", "text": "echo" },
            { "kind": "approval", "status": "revoked", "text": "" },
            { "kind": "approval", "status": "disarmed", "text": "" },
            { "kind": "command/start", "command": "echo governed", "intent": f.gov_intent, "considered": ["skip the check", "reopen the session instead"] },
            { "kind": "goal", "text": f.gov_goal }
        ] }),
        ),
        o("gov.close", json!("OK")),
        // ── runs ──
        o("runs.open", json!("sess-4")),
        o("mint.runs_label", json!("e2e-run-1759999999999")),
        o("runs.begin", json!({ "run_id": "run-abc", "known": true })),
        o(
            "runs.exec.in",
            json!({ "state": "partial", "exit_code": null }),
        ),
        o(
            "runs.exec.loose",
            json!({ "state": "partial", "exit_code": null }),
        ),
        o(
            "runs.operation.before_end",
            json!({ "runs": [
            { "kind": "run/begin", "run_id": "run-abc", "label": "e2e-run-1759999999999", "goal": f.runs_goal, "ts_ms": 100 }
        ], "events": [
            { "command": "echo inside-the-run", "run_id": "run-abc", "ts_ms": 200 },
            { "command": "echo loose-work", "run_id": null, "ts_ms": 210 }
        ] }),
        ),
        o("runs.end", json!({ "known": true })),
        o(
            "runs.operation.after_end",
            json!({ "runs": [
            { "kind": "run/begin", "run_id": "run-abc", "label": "e2e-run-1759999999999", "goal": f.runs_goal },
            { "kind": "run/end", "run_id": "run-abc", "outcome": "done" }
        ], "events": [] }),
        ),
        o("runs.end.unknown", json!({ "known": false })),
        o("runs.close", json!("OK")),
    ];

    // The two mcp transports, arm by arm. Both drive the view, so the green run has no SKIP in it and the
    // skip arms get their own test below.
    for tag in ["stdio", "http"] {
        let marker = format!("{tag}-autoselect-1759999999999");
        v.extend([
            o(&format!("mcp.{tag}.disconnect"), json!("OK")),
            o(&format!("mcp.{tag}.mint"), json!(marker)),
            oa(
                &format!("mcp.{tag}.connect"),
                json!({ "name": "pw", "transport": tag }),
                json!({ "status": "connected" }),
            ),
            o(&format!("mcp.{tag}.navigate"), json!({ "ok": true })),
            o(
                &format!("mcp.{tag}.view.0"),
                json!([
                    { "url": "http://127.0.0.1:9333/desktop/index.html" },
                    { "url": format!("https://example.com/{marker}") }
                ]),
            ),
            o(&format!("mcp.{tag}.navigate.home"), json!({ "ok": true })),
            o(&format!("mcp.{tag}.inject"), json!({ "ok": true })),
            o(
                &format!("mcp.{tag}.snapshot.0"),
                json!({ "content": "link \"E2E-INNER-LINK\" [ref=abc123]" }),
            ),
            oa(
                &format!("mcp.{tag}.click.0"),
                json!({ "target": "abc123", "label": "E2E-INNER-LINK" }),
                json!({ "ok": true }),
            ),
            o(
                &format!("mcp.{tag}.view2.0.0"),
                json!([
                    { "url": "http://127.0.0.1:9333/desktop/index.html" },
                    { "url": "https://example.com/inner-click-test" }
                ]),
            ),
            o(&format!("mcp.{tag}.disconnect.2"), json!("OK")),
        ]);
    }
    v
}

// ── the green run passes, and the check set is the one that was ported ─────────────────────────────────────

#[test]
fn a_green_run_passes_every_check_in_every_section() {
    let v = judge(green());
    let failures: Vec<&str> = v
        .checks
        .iter()
        .filter(|c| c.outcome == Outcome::Fail)
        .map(|c| c.name.as_str())
        .collect();
    assert!(
        failures.is_empty(),
        "the green run was not green: {failures:#?}\n{}",
        v.lines().join("\n")
    );
    assert_eq!(
        v.skipped(),
        0,
        "the green run skipped something: {}",
        v.lines().join("\n")
    );
    assert_eq!(v.exit_code(), 0);
    // **74 CALL SITES BECAME 76 CHECKS, AND THE ARITHMETIC IS THE PORT'S OWN.** The JS file carries 74
    // `check(...)` calls (its own comment says 75, counting the function's definition line), of which six are
    // inside `mcpAutoselectProbe` and therefore run TWICE, once per transport: 74 - 6 + 12 = 80 sites reached.
    // Four of those are alternative arms that a green run does not take — the two "no desktop SPA target"
    // early returns, the `panel xterm` one, and `runs`' `prerequisites missing` branch. 80 - 4 = 76.
    assert_eq!(
        v.checks.len(),
        76,
        "the check set changed; the ported suite emitted 76 checks on a clean run:\n{}",
        v.lines().join("\n")
    );
}

#[test]
fn every_check_name_is_the_one_the_js_suite_printed() {
    // The names are what a reader greps for in a device transcript, so they are pinned rather than merely
    // counted. A rename here is a rename in every saved run's triage.
    let names: Vec<String> = judge(green())
        .checks
        .iter()
        .map(|c| c.name.clone())
        .collect();
    for want in [
        "terminal list contains session",
        "terminal_list agrees with the caller about the last command outcome",
        "file single-read download",
        "workflow memory_delete verified",
        "panel xterm shows ai output",
        "mcp stdio click drives embedded view",
        "mcp http click learn-more",
        "evidence pwshots lists it",
        "browser focus-trap bar follows",
        "gov: the trail records the whole approval posture",
        "runs: work sent without a run is NOT absorbed into one",
    ] {
        assert!(
            names.iter().any(|n| n == want),
            "missing check name: {want}\n{names:#?}"
        );
    }
}

// ── one planted defect per check ───────────────────────────────────────────────────────────────────────────

type Mutation = Box<dyn Fn(&mut Vec<Observation>)>;

/// **THE ONE CHECK A DEFECT CANNOT TURN RED, AND WHY THAT IS THE SUITE'S OWN SEMANTICS RATHER THAN A GAP.**
/// An `http` arm whose navigation never reached the marker is reported as a SKIP: only that transport can be
/// on playwright-mcp's private-headless fork, where "the embedded view followed" asserts a PRECONDITION the
/// device does not have (round 100, measured — eleven rounds were spent asserting it anyway). So this check is
/// pass-or-skip BY CONSTRUCTION, and the mutation table says so instead of quietly excusing it.
const DEFECT_SKIPS: [&str; 1] = ["mcp http drives embedded view"];

fn set(id: &'static str, value: Value) -> Mutation {
    Box::new(move |obs| {
        let o = obs.iter_mut().find(|o| o.id == id).expect(id);
        o.result = Some(value.clone());
    })
}

fn error(id: &'static str, message: &'static str) -> Mutation {
    Box::new(move |obs| {
        let o = obs.iter_mut().find(|o| o.id == id).expect(id);
        o.result = None;
        o.error = Some(message.to_string());
    })
}

/// **THE THIRD COLUMN IS THE OUTCOME THE DEFECT MUST PRODUCE, AND ONE OF THEM IS A `Skip`.** An `http` arm
/// whose navigation did not reach the marker is reported as a SKIP rather than a failure — because on
/// playwright-mcp's private-headless fork "the embedded view followed" asserts a PRECONDITION the device does
/// not have. That is the JS suite's own semantics (round 100, measured), and the mutation table records it
/// instead of pretending the check can fail: `mcp http drives embedded view` is pass-or-skip by construction.
fn mutations() -> Vec<(&'static str, Mutation)> {
    vec![
        // ── terminal ──
        (
            "terminal list contains session",
            set("terminal.list.sessions", json!([])),
        ),
        (
            "terminal resize ok",
            set("terminal.resize", json!("resized")),
        ),
        (
            "terminal session execute",
            set(
                "terminal.execute.0",
                json!({ "state": "done", "exit_code": 0, "text": "nothing" }),
            ),
        ),
        (
            "terminal_list agrees with the caller about the last command outcome",
            set(
                "terminal.row.after_exec",
                json!([{ "id": "sess-1", "last_exit_code": 0 }]),
            ),
        ),
        (
            "a failing command does not report success",
            set(
                "terminal.exec.fail",
                json!({ "state": "done", "exit_code": 0 }),
            ),
        ),
        (
            "and the session row carries whatever the caller was told",
            set("terminal.row.after_fail", json!([{ "id": "sess-1" }])),
        ),
        (
            "terminal run_in_background",
            set("terminal.exec.bg", json!({ "status": "done" })),
        ),
        (
            "terminal background collect",
            set("terminal.read.bg", json!({ "text": "still sleeping" })),
        ),
        ("terminal write ok", set("terminal.write", json!("OK\n"))),
        (
            "terminal screen shows write",
            set("terminal.screen", json!({ "screen": "another marker" })),
        ),
        (
            "terminal read unknown evicted",
            set("terminal.read.unknown", json!({ "evicted": false })),
        ),
        (
            "terminal history retains closed",
            set(
                "terminal.history",
                json!([{ "id": "sess-1", "status": "live" }]),
            ),
        ),
        // ── file ──
        (
            "file upload 2 pages",
            set("file.write.page.1", json!({ "ok": false, "bytes": 0 })),
        ),
        ("file stat", set("file.stat", json!({ "size": 1 }))),
        ("file single-read download", {
            // The right LENGTH of the wrong bytes: this is the mutation the all-fill half exists for.
            let mut wrong = vec![b'B'; 307200];
            wrong[9] = b'X';
            let encoded = base64::engine::general_purpose::STANDARD.encode(wrong);
            set("file.read.raw", json!({ "data": encoded }))
        }),
        (
            "file list contains upload",
            set("file.list", json!({ "entries": [{ "name": "other.bin" }] })),
        ),
        (
            "file stat missing ok:false",
            set("file.stat.missing", json!({ "ok": true, "size": 0 })),
        ),
        (
            "file text write",
            set("file.text.write", json!({ "ok": false })),
        ),
        (
            "file text read",
            set(
                "file.text.read",
                json!({ "text": "E2E-TEXT-NO", "bytes": 11 }),
            ),
        ),
        // ── workflow ──
        (
            "workflow process_list",
            set("workflow.process_list", json!(null)),
        ),
        (
            "workflow local execute",
            set("workflow.execute", json!({ "text": "E2E-WF-2" })),
        ),
        (
            "workflow file_write",
            set("workflow.file.write", json!({ "ok": false })),
        ),
        (
            "workflow file_stat",
            set("workflow.file.stat", json!({ "size": 3 })),
        ),
        (
            "workflow memory_save",
            set("workflow.memory.save", json!({ "ok": true })),
        ),
        (
            "workflow memory_search",
            set("workflow.memory.search", json!({ "results": [] })),
        ),
        (
            "workflow memory_list",
            set(
                "workflow.memory.list",
                json!({ "results": [{ "id": "mem-9" }] }),
            ),
        ),
        (
            "workflow memory_update",
            set("workflow.memory.update", json!({ "ok": false })),
        ),
        (
            "workflow memory_search updated",
            set("workflow.memory.search.updated", json!({ "results": [] })),
        ),
        (
            "workflow memory_export",
            set(
                "workflow.memory.export",
                json!({ "lines": 0, "export": "" }),
            ),
        ),
        (
            "workflow memory_delete verified",
            set(
                "workflow.memory.search.deleted",
                json!({ "results": [{ "id": "mem-1" }] }),
            ),
        ),
        // ── panel ──
        (
            "panel ai write",
            set(
                "panel.exec",
                json!({ "state": "partial", "text": "nothing" }),
            ),
        ),
        (
            "panel xterm shows ai output",
            set("panel.xterm.0", json!("PS> nobody wrote anything here")),
        ),
        // ── evidence ──
        (
            "evidence screenshot saved",
            set(
                "evidence.screenshot",
                json!({ "exit_code": 1, "stdout": "FAIL:no chromium\n" }),
            ),
        ),
        (
            "evidence pwshots lists it",
            set(
                "evidence.pwshots",
                json!({ "shots": [{ "name": "a-different-shot.png" }] }),
            ),
        ),
        // ── browser ──
        (
            "browser pwinfo bundled",
            set("browser.pw_info", json!({ "pw_dir": "D:\\x" })),
        ),
        (
            "browser_run_script fail path",
            set(
                "browser.script.fail",
                json!({ "exit_code": 0, "stdout": "E2E-FAIL-PATH\n" }),
            ),
        ),
        (
            "browser_run_script drives view",
            set(
                "browser.script.drive",
                json!({ "exit_code": 0, "stdout": "TITLE=Something Else\n" }),
            ),
        ),
        (
            "browser SPA bar sync",
            set(
                "browser.bar.poll.0",
                json!("{\"onBrowser\":true,\"url\":\"https://elsewhere.test/\"}"),
            ),
        ),
        (
            "browser focus-trap bar follows",
            set(
                "browser.trap.bar.0",
                json!("{\"url\":\"https://example.com/stale\"}"),
            ),
        ),
        // ── governance ──
        ("gov: session opens", set("gov.open", json!(""))),
        (
            "gov: goal accepted",
            set(
                "gov.control.goal",
                json!({ "status": 200, "json": { "goal": "something else", "held_by_human": null } }),
            ),
        ),
        (
            "gov: partial patch leaves holder alone",
            set(
                "gov.control.goal",
                json!({ "status": 200, "json": { "goal": fx().gov_goal, "held_by_human": "d1" } }),
            ),
        ),
        (
            "gov: the AI reads the goal off terminal_list",
            set(
                "gov.list.after_goal",
                json!([{ "id": "sess-3", "goal": "a stale goal" }]),
            ),
        ),
        (
            "gov: the gate publishes the pending command",
            set("gov.pending.0", json!([{ "id": "sess-3" }])),
        ),
        (
            "gov: approve+grant decides",
            set("gov.approval", json!({ "decided": false })),
        ),
        (
            "gov: the grant is the command WORD, server-derived",
            set(
                "gov.approval",
                json!({ "decided": true, "approval_grants": ["echo governed"] }),
            ),
        ),
        (
            "gov: the approved command ran",
            error("gov.exec.result", "the gate never decided"),
        ),
        (
            "gov: the session row agrees with the caller about the last command outcome",
            set(
                "gov.list.after_exec",
                json!([{ "id": "sess-3", "last_exit_code": 0 }]),
            ),
        ),
        (
            "gov: a granted sibling runs without asking",
            set("gov.sibling_ms", json!(9000)),
        ),
        (
            "gov: revoke-all empties the grants",
            set("gov.grants.revoke", json!({ "approval_grants": ["echo"] })),
        ),
        ("gov: the trail records the whole approval posture", {
            // THE ONE THE SECTION EXISTS FOR: arming the gate leaves no trace.
            Box::new(|obs| {
                let o = obs.iter_mut().find(|o| o.id == "gov.session").unwrap();
                let events = o
                    .result
                    .as_mut()
                    .unwrap()
                    .get_mut("events")
                    .unwrap()
                    .as_array_mut()
                    .unwrap();
                events.remove(0);
            })
        }),
        ("gov: a repeated arm request is not recorded twice", {
            Box::new(|obs| {
                let o = obs.iter_mut().find(|o| o.id == "gov.session").unwrap();
                let events = o
                    .result
                    .as_mut()
                    .unwrap()
                    .get_mut("events")
                    .unwrap()
                    .as_array_mut()
                    .unwrap();
                events.insert(
                    1,
                    json!({ "kind": "approval", "status": "armed", "text": "" }),
                );
            })
        }),
        ("gov: intent is recorded beside the command", {
            Box::new(|obs| {
                let o = obs.iter_mut().find(|o| o.id == "gov.session").unwrap();
                o.result.as_mut().unwrap()["events"][6]["intent"] = json!("");
            })
        }),
        ("gov: the alternatives are recorded", {
            Box::new(|obs| {
                let o = obs.iter_mut().find(|o| o.id == "gov.session").unwrap();
                o.result.as_mut().unwrap()["events"][6]["considered"] = json!([]);
            })
        }),
        ("gov: the goal statement is durable", {
            Box::new(|obs| {
                let o = obs.iter_mut().find(|o| o.id == "gov.session").unwrap();
                let events = o
                    .result
                    .as_mut()
                    .unwrap()
                    .get_mut("events")
                    .unwrap()
                    .as_array_mut()
                    .unwrap();
                events.retain(|e| e["kind"] != "goal");
            })
        }),
        // ── runs ──
        ("runs: session opens", set("runs.open", json!({}))),
        (
            "runs: run_begin mints an id",
            set("runs.begin", json!({ "run_id": "not-a-run" })),
        ),
        ("runs: the boundary is on the timeline", {
            Box::new(|obs| {
                let o = obs
                    .iter_mut()
                    .find(|o| o.id == "runs.operation.before_end")
                    .unwrap();
                o.result.as_mut().unwrap()["runs"][0]["label"] = json!("some other run");
            })
        }),
        ("runs: the run carries the goal it was given", {
            Box::new(|obs| {
                let o = obs
                    .iter_mut()
                    .find(|o| o.id == "runs.operation.before_end")
                    .unwrap();
                o.result.as_mut().unwrap()["runs"][0]["goal"] = json!(null);
            })
        }),
        ("runs: the command wears the id run_begin minted", {
            Box::new(|obs| {
                let o = obs
                    .iter_mut()
                    .find(|o| o.id == "runs.operation.before_end")
                    .unwrap();
                o.result.as_mut().unwrap()["events"][0]["run_id"] = json!(null);
            })
        }),
        ("runs: work sent without a run is NOT absorbed into one", {
            Box::new(|obs| {
                let o = obs
                    .iter_mut()
                    .find(|o| o.id == "runs.operation.before_end")
                    .unwrap();
                o.result.as_mut().unwrap()["events"][1]["run_id"] = json!("run-abc");
            })
        }),
        ("runs: the boundary precedes the work it names", {
            Box::new(|obs| {
                let o = obs
                    .iter_mut()
                    .find(|o| o.id == "runs.operation.before_end")
                    .unwrap();
                o.result.as_mut().unwrap()["runs"][0]["ts_ms"] = json!(500);
            })
        }),
        ("runs: run_end closes the run with its outcome", {
            Box::new(|obs| {
                let o = obs
                    .iter_mut()
                    .find(|o| o.id == "runs.operation.after_end")
                    .unwrap();
                o.result.as_mut().unwrap()["runs"][1]["outcome"] = json!("abandoned");
            })
        }),
        (
            "runs: an unregistered id is accepted and reported as unknown",
            set("runs.end.unknown", json!({ "known": true })),
        ),
        // ── mcp, both transports (the ids are built per tag) ──
        (
            "mcp stdio connect",
            set(
                "mcp.stdio.connect",
                json!({ "status": "already_connected" }),
            ),
        ),
        (
            "mcp stdio navigate ok",
            set("mcp.stdio.navigate", json!({ "ok": false })),
        ),
        (
            "mcp stdio drives embedded view",
            set(
                "mcp.stdio.view.0",
                json!([{ "url": "http://127.0.0.1:9333/desktop/index.html" }]),
            ),
        ),
        (
            "mcp stdio SPA intact",
            set(
                "mcp.stdio.view.0",
                json!([{ "url": "https://example.com/stdio-autoselect-1759999999999" }]),
            ),
        ),
        (
            "mcp stdio click learn-more",
            set("mcp.stdio.click.0", json!({ "ok": false })),
        ),
        (
            "mcp stdio click drives embedded view",
            set(
                "mcp.stdio.view2.0.0",
                json!([{ "url": "https://example.com/" }]),
            ),
        ),
        (
            "mcp http connect",
            set("mcp.http.connect", json!({ "status": "error" })),
        ),
        (
            "mcp http navigate ok",
            set("mcp.http.navigate", json!({ "ok": false })),
        ),
        (
            "mcp http drives embedded view",
            set(
                "mcp.http.view.0",
                json!([{ "url": "http://127.0.0.1:9333/desktop/index.html" }]),
            ),
        ),
        (
            "mcp http SPA intact",
            set(
                "mcp.http.view.0",
                json!([{ "url": "https://example.com/http-autoselect-1759999999999" }]),
            ),
        ),
        (
            "mcp http click learn-more",
            set("mcp.http.click.0", json!({ "ok": false })),
        ),
        (
            "mcp http click drives embedded view",
            set(
                "mcp.http.view2.0.0",
                json!([{ "url": "https://example.com/" }]),
            ),
        ),
    ]
}

#[test]
fn every_check_can_fail_when_its_own_input_is_broken() {
    let table = mutations();
    let held = judge(green());
    let names: Vec<&str> = held.checks.iter().map(|c| c.name.as_str()).collect();
    let mut missing: Vec<&str> = Vec::new();
    let mut not_broken: Vec<&str> = Vec::new();

    for (name, apply) in &table {
        let mut obs = green();
        apply(&mut obs);
        let v = judge(obs);
        let want = if DEFECT_SKIPS.contains(name) {
            Outcome::Skip
        } else {
            Outcome::Fail
        };
        match v.checks.iter().find(|c| c.name == *name) {
            None => missing.push(name),
            Some(c) => {
                if c.outcome != want {
                    not_broken.push(name);
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "these mutations name a check that does not exist — the table has drifted from the judge: {missing:#?}"
    );
    assert!(
        not_broken.is_empty(),
        "these checks did not take the outcome their planted defect must produce, so the table is not \
         describing what the judge does: {not_broken:#?}"
    );

    // AND THE TABLE COVERS THE JUDGE. A check with no mutation is a check whose input nobody has named.
    let covered: Vec<&str> = table.iter().map(|(n, _)| *n).collect();
    let uncovered: Vec<&&str> = names.iter().filter(|n| !covered.contains(n)).collect();
    assert!(
        uncovered.is_empty(),
        "these checks have no planted defect in MUTATIONS, so nothing proves they can fail: {uncovered:#?}"
    );
}

// ── the arms that are not a pass and not a failure ─────────────────────────────────────────────────────────

#[test]
fn the_private_headless_arm_skips_instead_of_asserting_a_missing_precondition() {
    // The http transport did NOT reach the marker: playwright-mcp is on its private-headless fork, where
    // "the embedded view followed" is not a contract that can hold. Asserting it asserts a precondition.
    let mut obs = green();
    obs.retain(|o| !o.id.starts_with("mcp.http.view."));
    obs.push(o(
        "mcp.http.view.0",
        json!([{ "url": "http://127.0.0.1:9333/desktop/index.html" }]),
    ));
    let v = judge(obs);
    let skipped: Vec<&str> = v
        .checks
        .iter()
        .filter(|c| c.outcome == Outcome::Skip)
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        skipped,
        vec![
            "mcp http drives embedded view",
            "mcp http click drives embedded view"
        ],
        "{}",
        v.lines().join("\n")
    );
    // The stdio arm is unaffected: it spawns and owns its own child.
    assert!(v
        .checks
        .iter()
        .any(|c| c.name == "mcp stdio drives embedded view" && c.outcome == Outcome::Pass));
    assert_eq!(v.exit_code(), 0, "{}", v.lines().join("\n"));
}

#[test]
fn a_connection_that_never_came_up_stops_that_probe_without_failing_the_rest() {
    let mut obs = green();
    obs.retain(|o| !o.id.starts_with("mcp.http."));
    obs.push(o("mcp.http.connect", json!({ "status": "error" })));
    let v = judge(obs);
    let failed: Vec<&str> = v
        .checks
        .iter()
        .filter(|c| c.outcome == Outcome::Fail)
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(failed, vec!["mcp http connect"], "{}", v.lines().join("\n"));
    // The stdio probe and every other section still ran.
    assert!(v.checks.iter().any(|c| c.name == "mcp stdio SPA intact"));
    assert!(v
        .checks
        .iter()
        .any(|c| c.name == "runs: run_begin mints an id"));
}

// ── the run that could not finish, and the sections that never started ─────────────────────────────────────

#[test]
fn an_aborted_run_still_counts_what_ran_and_names_the_abort() {
    // The JS suite wrapped every section in ONE try, so a failed call ended the run and the checks registered
    // before it still counted. The judge reproduces that: the sections that recorded nothing contribute
    // nothing, and `suite` carries the message.
    let mut obs: Vec<Observation> = green()
        .into_iter()
        .take_while(|o| !o.id.starts_with("file."))
        .collect();
    obs.push(Observation {
        id: "driver.abort".to_string(),
        args: None,
        result: None,
        error: Some("tool system_file_write HTTP 500".to_string()),
        at_ms: None,
    });
    let v = judge(obs);
    assert!(
        v.checks.iter().any(|c| c.name == "suite"
            && c.outcome == Outcome::Fail
            && c.detail.contains("HTTP 500")),
        "{}",
        v.lines().join("\n")
    );
    assert!(v
        .checks
        .iter()
        .any(|c| c.name == "terminal session execute"));
    assert!(!v.checks.iter().any(|c| c.name.starts_with("file ")));
    assert_eq!(v.exit_code(), 1);
}

#[test]
fn a_section_whose_ids_the_judge_cannot_read_says_so_instead_of_going_quiet() {
    // **THE FAILURE A TWO-LANGUAGE SPLIT INVITES, AND THE REASON THE GUARD EXISTS.** Rename one observation id
    // in the driver and every check behind it stops being REGISTERED rather than failing: the run still prints
    // `N/N passed`, with a smaller N, and nothing in the transcript says a section went missing. The guard
    // turns that into a named failure.
    let mut obs = green();
    let at = obs.iter().position(|o| o.id == "terminal.open").unwrap();
    obs[at].id = "terminal.open.typo".to_string();
    let v = judge(obs);
    let guard = v
        .checks
        .iter()
        .find(|c| c.name == "terminal: the judge produced no checks")
        .unwrap_or_else(|| panic!("the renamed id went unnoticed:\n{}", v.lines().join("\n")));
    assert_eq!(guard.outcome, Outcome::Fail);
    assert!(guard.detail.contains("terminal."), "{}", guard.detail);
    assert_eq!(v.exit_code(), 1);
    // The other sections are untouched: one half of one contract broke, not the suite.
    assert!(v
        .checks
        .iter()
        .any(|c| c.name == "runs: run_begin mints an id"));
}

#[test]
fn a_run_that_never_reached_a_section_is_not_reported_as_a_contract_break() {
    // A section with NO observations is one the run never got to — `suite` names why, and inventing a
    // "produced no checks" failure for it would bury the real cause under seven copies of itself.
    let obs: Vec<Observation> = green()
        .into_iter()
        .take_while(|o| !o.id.starts_with("file."))
        .collect();
    let v = judge(obs);
    assert!(
        !v.checks
            .iter()
            .any(|c| c.name.ends_with(": the judge produced no checks")),
        "{}",
        v.lines().join("\n")
    );
}

#[test]
fn a_recording_with_no_checks_is_exit_2_and_not_a_pass() {
    let v = judge(vec![]);
    assert_eq!(v.checks.len(), 0);
    assert_eq!(v.exit_code(), 2);
}

#[test]
fn the_plan_decides_which_sections_are_judged() {
    // A recording made with `--only governance,runs` produces exactly those sections' checks, and the check
    // names of the other seven do not appear — which is why `--expect` exists at the CLI: a partial run that
    // found nothing reads exactly like a clean full one.
    let v = judge_with(
        plan_from_cli(Some("governance,runs"), false).unwrap(),
        green(),
    );
    assert!(v
        .checks
        .iter()
        .all(|c| c.name.starts_with("gov: ") || c.name.starts_with("runs:")));
    assert_eq!(v.checks.len(), 25, "{}", v.lines().join("\n"));
    assert_eq!(v.exit_code(), 0);
}

#[test]
fn no_browser_plan_drops_the_four_cdp_sections_from_the_verdict() {
    let v = judge_with(plan_from_cli(None, true).unwrap(), green());
    for gone in ["panel ", "mcp ", "evidence ", "browser "] {
        assert!(
            !v.checks.iter().any(|c| c.name.starts_with(gone)),
            "{gone} was judged under --no-browser"
        );
    }
    assert_eq!(v.exit_code(), 0, "{}", v.lines().join("\n"));
}

// ── the detail strings, which are what a reader triages from ───────────────────────────────────────────────

#[test]
fn a_detail_says_what_the_device_answered_and_not_only_that_it_failed() {
    let mut obs = green();
    obs.retain(|o| o.id != "terminal.execute.0");
    obs.push(o(
        "terminal.execute.0",
        json!({ "state": "partial", "exit_code": null, "text": "nothing" }),
    ));
    let v = judge(obs);
    let c = v
        .checks
        .iter()
        .find(|c| c.name == "terminal session execute")
        .unwrap();
    assert!(c.detail.contains("state=partial"), "{}", c.detail);
    assert!(c.detail.contains("exit=null"), "{}", c.detail);

    // `undefined` is not `null`, and the JS printed them differently: a missing field is `undefined`.
    let mut obs = green();
    obs.retain(|o| o.id != "terminal.execute.0");
    obs.push(o("terminal.execute.0", json!({ "text": "nothing" })));
    let v = judge(obs);
    let c = v
        .checks
        .iter()
        .find(|c| c.name == "terminal session execute")
        .unwrap();
    assert!(c.detail.contains("state=undefined"), "{}", c.detail);

    // A Chinese OS error string is CARRIED RAW in the check and SUBSTITUTED on the way to the console — the
    // split the JS suite had (the `detail` field kept the bytes, the `console.log` sanitized them), because a
    // GBK transcript is the only place the mojibake could happen.
    let v = judge(green());
    let c = v
        .checks
        .iter()
        .find(|c| c.name == "file stat missing ok:false")
        .unwrap();
    assert!(
        c.detail.contains('系'),
        "the raw detail lost the device's own words: {}",
        c.detail
    );
    let line = v.lines().join("\n");
    assert!(
        line.contains("file stat missing ok:false  -- ????"),
        "{line}"
    );
    assert!(!line.contains('系'), "{line}");
}

#[test]
fn the_click_triage_is_a_note_beside_the_failure_and_never_a_check() {
    let mut obs = green();
    obs.retain(|o| !o.id.starts_with("mcp.stdio.view2."));
    obs.push(o(
        "mcp.stdio.view2.0.0",
        json!([{ "url": "https://example.com/" }]),
    ));
    obs.push(o(
        "mcp.stdio.geometry",
        json!("no-link-in-view (the Learn more link is not in the embedded view right now)"),
    ));
    let v = judge(obs);
    assert_eq!(
        v.checks
            .iter()
            .filter(|c| c.outcome == Outcome::Fail)
            .count(),
        1
    );
    let lines = v.lines().join("\n");
    assert!(
        lines.contains("[triage] click missed; geometry: no-link-in-view"),
        "{lines}"
    );
    // The note sits immediately before the click check it explains, the way the JS printed it.
    let click = lines
        .find("FAIL mcp stdio click drives embedded view")
        .unwrap();
    assert!(lines[..click].contains("[triage]"), "{lines}");
}

// ── the plan and the rows document are a contract with a JavaScript caller ─────────────────────────────────

#[test]
fn the_plan_serializes_the_field_names_the_driver_reads() {
    // The driver reads these by name out of the JSON. A rename here is a runtime `undefined` on a device, which
    // is the one failure this pair can produce that no compiler sees — so the names are pinned, in one place,
    // for both languages.
    let json = serde_json::to_value(plan()).unwrap();
    for key in ["sections", "fixtures", "budgets"] {
        assert!(json.get(key).is_some(), "the plan lost `{key}`");
    }
    for key in [
        "evidence_dir",
        "pw_dir",
        "session_command",
        "session_marker",
        "failing_command",
        "background_command",
        "background_marker",
        "unknown_session",
        "write_marker_prefix",
        "write_command_template",
        "panel_command_template",
        "marker_url_template",
        "transfer_name",
        "transfer_page_bytes",
        "transfer_pages",
        "transfer_fill",
        "text_name",
        "text_payload",
        "missing_name",
        "workflow_name",
        "workflow_command",
        "workflow_marker",
        "memory_title",
        "memory_tags",
        "memory_token_prefix",
        "memory_update_prefix",
        "memory_list_limit",
        "panel_marker_prefix",
        "mcp_transports",
        "mcp_home_url",
        "mcp_link_labels",
        "mcp_click_targets",
        "evidence_shot_prefix",
        "evidence_shot_suffix",
        "browser_home_url",
        "browser_title_line",
        "browser_bar_fragment",
        "focus_marker_prefix",
        "gov_goal",
        "gov_command",
        "gov_intent",
        "gov_considered",
        "gov_sibling_command",
        "gov_grant_word",
        "runs_label_prefix",
        "runs_goal",
        "runs_inside_command",
        "runs_loose_command",
        "runs_unknown_id",
    ] {
        assert!(
            json["fixtures"].get(key).is_some(),
            "the plan lost fixture `{key}`"
        );
    }
    for key in [
        "prompt_polls",
        "prompt_interval_ms",
        "execute_attempts",
        "execute_retry_ms",
        "background_collect_ms",
        "screen_settle_ms",
        "panel_open_settle_ms",
        "panel_list_settle_ms",
        "panel_rail_settle_ms",
        "panel_tab_settle_ms",
        "panel_xterm_polls",
        "panel_xterm_interval_ms",
        "mcp_disconnect_settle_ms",
        "mcp_view_polls",
        "mcp_view_interval_ms",
        "mcp_nav_settle_ms",
        "mcp_inject_settle_ms",
        "mcp_click_attempts",
        "mcp_click_retry_ms",
        "evidence_rail_settle_ms",
        "evidence_shots_settle_ms",
        "spa_rail_settle_ms",
        "browser_bar_settle_ms",
        "browser_bar_polls",
        "browser_bar_interval_ms",
        "browser_rail_settle_ms",
        "browser_trap_polls",
        "browser_trap_interval_ms",
        "gov_pending_polls",
        "gov_pending_interval_ms",
        "gov_sibling_ms",
        "ws_close_immediate_ms",
        "ws_close_grace_ms",
        "ws_eval_timeout_ms",
    ] {
        assert!(
            json["budgets"].get(key).is_some(),
            "the plan lost budget `{key}`"
        );
    }
}

#[test]
fn a_rows_document_from_a_run_whose_plan_is_a_subset_is_judged_as_that_subset() {
    // The recording carries the plan it ran, and the judge reads the fixtures from it — so a corpus stays
    // judgeable after the defaults move, and `--expect` is what stops that from being a way to pass by editing
    // the recording.
    let mut p = plan_from_cli(Some("terminal"), false).unwrap();
    p.fixtures.session_marker = "SOMETHING-ELSE".to_string();
    let mut obs = green();
    obs.retain(|o| o.id.starts_with("terminal.") || o.id.starts_with("mint.terminal"));
    let v = judge_with(p, obs);
    assert_eq!(
        v.checks
            .iter()
            .filter(|c| c.outcome == Outcome::Fail)
            .count(),
        1
    );
    let c = v
        .checks
        .iter()
        .find(|c| c.name == "terminal session execute")
        .unwrap();
    assert_eq!(
        c.outcome,
        Outcome::Fail,
        "the judge used its own default rather than the run's plan"
    );
}

#[test]
fn the_unused_helpers_are_still_compiled_against_real_shapes() {
    // A tiny guard against `rows.rs`'s accessors rotting: the list shapes the agent answers in are the two the
    // JS had to handle, and both are exercised here rather than only through a whole section.
    use summrise_e2e_judge::rows::{
        list_of, results_of, row_of, rows_list_entries, screen_of, text_of,
    };
    assert_eq!(list_of(&json!({ "sessions": [{ "id": "a" }] })).len(), 1);
    assert_eq!(list_of(&json!([{ "id": "a" }])).len(), 1);
    assert!(row_of(&json!({ "sessions": [{ "id": "a" }] }), "a").is_some());
    assert_eq!(rows_list_entries(&json!({ "entries": [1, 2] })).len(), 2);
    assert_eq!(results_of(&json!({ "results": [1] })).len(), 1);
    assert_eq!(text_of(&json!("raw")), "raw");
    assert_eq!(text_of(&json!({ "text": "wrapped" })), "wrapped");
    assert_eq!(screen_of(&json!({ "screen": "tail" })), "tail");
}
