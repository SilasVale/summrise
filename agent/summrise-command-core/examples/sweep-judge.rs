//! `sweep-judge` — the Node↔Rust boundary the migration plan names as this port's cost, made concrete.
//!
//! It reads ONE file: a fixture in the shape `tests/fixtures/design-sweep/*.json` uses — the report, the
//! options its sweep judges it with, and optionally the stylesheet the mark-coverage queue reads — and it
//! prints what the judge found.
//!
//!     cargo run -p summrise-agent-core --example sweep-judge -- <fixture.json>
//!     cargo run -p summrise-agent-core --example sweep-judge -- <fixture.json> --json
//!
//! The plain form prints the lines the JS judge prints and then the findings, one per line, and exits 1
//! when there is at least one finding — so it can be used exactly where `panel-design-sweep.mjs --judge` is
//! used today. `--json` prints one canonical object instead, which is what an equivalence run diffs:
//!
//!     {"findings": [...], "lines": [...], "notes": [...], "summary": "...", "marks": [...]}
//!
//! WHY AN EXAMPLE RATHER THAN A GATE. This is not a check — it is the seam a JS driver would call, and it
//! is committed because the equivalence proof needs a runnable Rust side and because the driver's move (if
//! it happens) starts here. It is built by `cargo test`/`cargo clippy --all-targets`, so it cannot rot
//! silently; `./scripts/build.sh agent` builds only `--bin summrise-agent`, so it costs the Windows release
//! nothing.

use std::process::ExitCode;
use summrise_agent_core::sweep_judge::{
    judge_report, mark_coverage_notes, report_summary, JudgeOptions,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let Some(path) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: sweep-judge <fixture.json> [--json]");
        return ExitCode::from(2);
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::from(2);
        }
    };
    let fixture: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{path} is not JSON: {e}");
            return ExitCode::from(2);
        }
    };
    let report = fixture.get("report").unwrap_or(&fixture);
    let opts = JudgeOptions::from_json(fixture.get("opts"));
    let outcome = judge_report(report, &opts);
    let summary = report_summary(
        fixture
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("panel"),
        report,
    );
    let marks = fixture
        .get("css")
        .and_then(|v| v.as_str())
        .map(|css| mark_coverage_notes(css, report))
        .unwrap_or_default();

    if json {
        let out = serde_json::json!({
            "findings": outcome.findings,
            "lines": outcome.lines,
            "notes": outcome.report_notes,
            "summary": summary,
            "marks": marks,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    } else {
        for line in &outcome.lines {
            println!("{line}");
        }
        for note in &outcome.report_notes {
            println!("note: {note}");
        }
        for line in &marks {
            println!("{line}");
        }
        println!("{summary}");
        for f in &outcome.findings {
            println!("{f}");
        }
        if outcome.findings.is_empty() {
            eprintln!("sweep judge OK: nothing above found a defect");
            return ExitCode::SUCCESS;
        }
        eprintln!("sweep judge: {} finding(s)", outcome.findings.len());
    }
    if outcome.findings.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
