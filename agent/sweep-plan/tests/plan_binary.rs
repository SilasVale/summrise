//! The CLI's contract and the plan's internal consistency.
//!
//! THE EXIT CODES ARE THE INTERFACE (0 a plan on stdout, 2 a usage error), and the emitters depend on
//! both: `--emit` runs this binary at emit time and would otherwise embed a usage error as a plan.
//!
//! The equivalence oracle is NOT here — it is `parity/compare.mjs`, which executes the pre-change
//! payload under a stub browser and compares the ordered trace of what it asked the browser to do with
//! this crate's plan, over every `--passes` value, for all three tools. What these tests pin is what a
//! comparison of two implementations cannot: that the interface itself refuses what it should refuse.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_summrise-sweep-plan"))
}

/// `(exit code, stdout, stderr)`.
fn run(args: &[&str]) -> (i32, String, String) {
    let out = bin().args(args).output().expect("the binary ran");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn plan(tool: &str, passes: Option<&str>) -> serde_json::Value {
    let mut args = vec!["--tool", tool];
    let owned;
    if let Some(p) = passes {
        owned = format!("--passes={p}");
        args.push(&owned);
    }
    let (code, out, err) = run(&args);
    assert_eq!(code, 0, "{tool} exited {code}: {err}");
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("{tool} printed no plan: {e}"))
}

// ── THE INTERFACE ───────────────────────────────────────────────────────────────────────────────────

#[test]
fn a_usage_error_exits_two_and_says_what_is_wrong() {
    for args in [
        vec![],
        vec!["--tool"],
        vec!["--tool", "not-a-tool"],
        vec!["--tool", "panel", "--nope"],
        vec!["--tool", "panel", "extra.json"],
        vec!["--passes=all"],
        vec!["--tool", "panel", "--passes"],
    ] {
        let (code, out, err) = run(&args);
        assert_eq!(code, 2, "{args:?} should be a usage error, got {code}");
        assert!(
            out.is_empty(),
            "{args:?} printed to stdout on a usage error"
        );
        assert!(
            err.contains("usage:"),
            "{args:?} did not print the usage line: {err}"
        );
    }
}

#[test]
fn help_is_not_a_usage_error() {
    let (code, out, _) = run(&["--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("usage:"));
}

#[test]
fn the_flag_spellings_the_callers_use_are_both_accepted() {
    // `--tool panel` and `--tool=panel` are one keystroke apart and both are used by shell scripts.
    let (code, out, _) = run(&["--tool=panel", "--passes=all"]);
    assert_eq!(code, 0);
    let a: serde_json::Value = serde_json::from_str(&out).unwrap();
    let b = plan("panel", Some("all"));
    assert_eq!(a, b);
}

#[test]
fn the_first_passes_flag_wins_because_the_javascript_reads_it_with_find() {
    // `process.argv.find((a) => a.startsWith("--passes="))` — the FIRST match is what reaches the
    // payload, so a second flag must not override it.
    let (code, out, _) = run(&["--tool", "panel", "--passes=reflow", "--passes=all"]);
    assert_eq!(code, 0);
    let p: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(p["passes"], "reflow");
    assert_eq!(p["wanted"], serde_json::json!(["reflow"]));
}

// ── THE PLAN'S SHAPE ────────────────────────────────────────────────────────────────────────────────

#[test]
fn every_tool_plans_surfaces_with_the_fields_a_payload_needs() {
    for tool in ["panel", "console", "landing"] {
        let p = plan(tool, Some("all"));
        let surfaces = p["surfaces"].as_array().expect("surfaces");
        assert!(!surfaces.is_empty(), "{tool} planned no surfaces");
        let mut ids = std::collections::HashSet::new();
        for s in surfaces {
            let id = s["id"].as_str().expect("id");
            assert!(
                ids.insert(id.to_string()),
                "{tool}: duplicate surface id {id}"
            );
            assert!(s["kind"].is_string(), "{tool}: {id} has no kind");
            assert!(
                s["viewport"]["width"].is_number(),
                "{tool}: {id} has no viewport"
            );
            // A surface is either a URL (path) or an SPA route (hash) — the console's pages are the
            // second kind, and a surface with NEITHER is a surface nothing can navigate to.
            assert!(
                s["path"].is_string() || s["hash"].is_string(),
                "{tool}: {id} has neither a path nor a hash"
            );
            assert!(s["passes"].is_array(), "{tool}: {id} declares no passes");
        }
    }
}

#[test]
fn wanted_and_the_wants_map_are_the_same_decision() {
    for tool in ["panel", "console", "landing"] {
        for passes in [None, Some("all"), Some(""), Some("nope")] {
            let p = plan(tool, passes);
            let vocab = p["vocab"].as_array().expect("vocab");
            let wanted: Vec<&str> = p["wanted"]
                .as_array()
                .expect("wanted")
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            for name in vocab {
                let name = name.as_str().unwrap();
                assert_eq!(
                    p["wants"][name].as_bool().unwrap(),
                    wanted.contains(&name),
                    "{tool} --passes={passes:?}: {name} disagrees between wanted and wants"
                );
            }
        }
    }
}

#[test]
fn a_pass_that_is_not_wanted_contributes_no_surface() {
    // THE PROPERTY THE WHOLE LANDING RESTS ON: the payload iterates `kind(...)`, so a surface the plan
    // did not name cannot be visited. `--passes=reflow` on the panel must plan the reflow surfaces and
    // NOTHING else — no matrix, no fixtures.
    let p = plan("panel", Some("reflow"));
    let kinds: Vec<&str> = p["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["kind"].as_str().unwrap())
        .collect();
    assert!(!kinds.is_empty());
    assert!(kinds.iter().all(|k| *k == "reflow"), "planned {kinds:?}");

    // ... and the same for a pass whose block is one page: `--passes=ack` plans the ack surfaces only.
    let p = plan("panel", Some("ack"));
    let kinds: Vec<&str> = p["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.iter().all(|k| k.starts_with("ack-")),
        "planned {kinds:?}"
    );
}

#[test]
fn the_matrix_renders_a_page_for_every_axis_that_needs_one() {
    // `--passes=focus` used to run the matrix ZERO times: the mode list was empty, no page was loaded,
    // no Tab was pressed, and the report came back `focus: []` — clean because nothing ran. The gate
    // for the matrix is every axis that needs a rendered page, and `timing` is one of them.
    for axis in ["focus", "timing", "idle", "press"] {
        let p = plan("panel", Some(axis));
        let kinds: Vec<&str> = p["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["kind"].as_str().unwrap())
            .collect();
        assert!(
            kinds.contains(&"matrix"),
            "--passes={axis} planned {kinds:?}"
        );
    }
}

#[test]
fn the_panel_and_the_other_two_disagree_about_an_empty_pass_list() {
    // DOCUMENTED DIVERGENCE, PINNED SO IT CANNOT DRIFT SILENTLY: an EMPTY `--passes=` wants nothing on
    // the panel (it tests the raw string) and everything on the console and the landing (they test a
    // list). An ABSENT flag wants everything on all three.
    for tool in ["panel", "console", "landing"] {
        let absent = plan(tool, None);
        assert!(
            !absent["surfaces"].as_array().unwrap().is_empty(),
            "{tool} planned nothing when the flag was absent"
        );
    }
    let panel_empty = plan("panel", Some(""));
    assert_eq!(panel_empty["surfaces"].as_array().unwrap().len(), 0);
    assert!(
        panel_empty["notes"].as_array().unwrap().len() >= 2,
        "the divergences are not reported"
    );
    for tool in ["console", "landing"] {
        let empty = plan(tool, Some(""));
        assert!(!empty["surfaces"].as_array().unwrap().is_empty(), "{tool}");
    }
}

#[test]
fn only_the_panel_trims_the_entries() {
    // The panel's `wants` maps each entry through `.trim()`; the console's and the landing's do not, so
    // `--passes=contrast, targets` wants only `contrast` there. Reproduced, not fixed.
    assert_eq!(
        plan("panel", Some("reflow, motion"))["wanted"],
        serde_json::json!(["motion", "reflow"])
    );
    assert_eq!(
        plan("console", Some("contrast, targets"))["wanted"],
        serde_json::json!(["contrast"])
    );
}

#[test]
fn the_caps_a_payload_reads_are_present_and_are_policy() {
    let panel = plan("panel", Some("all"));
    for key in [
        "focus_tabs",
        "ack_budget_ms",
        "ack_settings_targets",
        "ack_memory_discover",
        "press_curated",
        "press_rail_discover",
        "press_skip",
        "reflow_widths",
        "reflow_height",
    ] {
        assert!(
            !panel["caps"][key].is_null(),
            "the panel's plan carries no caps.{key}"
        );
    }
    let console = plan("console", Some("all"));
    for key in [
        "focus_tabs",
        "ack_budget_ms",
        "press_discover",
        "wide_width",
        "reflow_widths",
        "pages",
    ] {
        assert!(
            !console["caps"][key].is_null(),
            "the console's plan carries no caps.{key}"
        );
    }
    let landing = plan("landing", Some("all"));
    for key in [
        "focus_tabs",
        "press_curated",
        "reflow_widths",
        "schemes",
        "pages",
    ] {
        assert!(
            !landing["caps"][key].is_null(),
            "the landing's plan carries no caps.{key}"
        );
    }
}

#[test]
fn the_order_of_the_surfaces_is_the_order_the_report_is_written_in() {
    // The report's `rows`/`surfaces`/`names` arrays are appended in visit order, and 2b's bar is a
    // report byte-identical to the one the JavaScript produced. So the FIRST surfaces of a full panel
    // run are the matrix's, panel density first, light first, idle before relaxed — the order the
    // payload's own loops produced.
    let p = plan("panel", Some("all"));
    let first: Vec<&str> = p["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .take(4)
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        first,
        vec![
            "matrix.panel.light.idle",
            "matrix.panel.light.relaxed",
            "matrix.panel.dark.idle",
            "matrix.panel.dark.relaxed",
        ]
    );
    // ... and the last four are the reflow widths, in the order the criterion names them.
    let last: Vec<&str> = p["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .take(2)
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(last, vec!["reflow.panel.320", "reflow.panel.640"]);
}

#[test]
fn a_dynamic_surface_names_what_expands_it() {
    // The rail walk's page names come from the DOM, so the plan carries ONE surface per density and
    // theme and says so. A named exception rather than a silent gap.
    let p = plan("panel", Some("all"));
    let dynamic: Vec<&serde_json::Value> = p["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| !s["dynamic"].is_null())
        .collect();
    assert_eq!(
        dynamic.len(),
        4,
        "one dynamic surface per density and theme"
    );
    for s in dynamic {
        assert_eq!(s["dynamic"], "rail-labels");
        assert_eq!(s["kind"], "rail");
    }
}
