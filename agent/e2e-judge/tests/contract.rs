//! THE CONTRACT BETWEEN THE TWO LANGUAGES, CHECKED FROM BOTH SIDES.
//!
//! The driver is JavaScript and the judge is Rust, and the only thing they share is a NAMING CONVENTION: the
//! observation ids the driver writes and the judge reads, and the plan keys the judge writes and the driver
//! reads. Neither compiler can see across that line, and the failure it invites is the quiet kind — a renamed
//! id makes a check stop existing rather than fail, so the run still prints `N/N passed` with a smaller N.
//!
//! **THIS IS NOT THE EQUIVALENCE ORACLE, AND IT IS NOT PRETENDING TO BE.** What proves the port reproduces the
//! JavaScript verdicts is a recorded run judged by both (`e2e-rec.json`, stranded — see the README). What this
//! proves is narrower and still worth having: every name one half depends on is a name the other half writes
//! down, so a rename is a red test rather than a device surprise.
//!
//! MUTATION: rename `terminal.row.after_exec` to `terminal.row.after_execs` in `agent/scripts/e2e/e2e.js`.
//! RESULT:   exit 101 — `the judge reads `terminal.row.after_exec` and the driver never mentions it`.

use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives at agent/e2e-judge/")
        .to_path_buf()
}

fn driver() -> String {
    std::fs::read_to_string(repo().join("agent/scripts/e2e/e2e.js"))
        .expect("the driver source is there")
}

/// Every string literal passed to `got(`, `series_values(`, `find(` or the mcp `id(` helper, anywhere in the
/// check modules. Hand-rolled rather than regexed: the crate has no regex dependency and this scan is a page
/// of code.
fn ids_the_judge_reads() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/checks");
    let mut out = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("src/checks")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "rs").unwrap_or(false))
        .collect();
    files.sort();
    for f in files {
        let text = std::fs::read_to_string(&f).expect("a check module");
        for needle in ["got(", "series_values(", "find(", "id("] {
            let mut from = 0;
            while let Some(at) = text[from..].find(needle) {
                let start = from + at + needle.len();
                from = start;
                // `got(&id("x"))` and `got("x")` and `find(&click_id)` — only the literal forms matter.
                let rest = &text[start..];
                let rest = rest.strip_prefix('&').unwrap_or(rest);
                let rest = rest.strip_prefix("id(").unwrap_or(rest);
                let Some(rest) = rest.strip_prefix('"') else {
                    continue;
                };
                let Some(end) = rest.find('"') else { continue };
                let lit = &rest[..end];
                // Both halves build the mcp ids out of a tag, so the useful literals are the suffixes; a
                // single character is a slice index, not a name.
                if lit.len() > 1
                    && lit
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
                {
                    out.push(lit.to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn every_observation_id_the_judge_reads_is_one_the_driver_records() {
    let src = driver();
    let ids = ids_the_judge_reads();
    let missing: Vec<&String> = ids.iter().filter(|id| !src.contains(id.as_str())).collect();
    assert!(
        missing.is_empty(),
        "the judge reads these observation ids and the driver never mentions them, so the checks that depend \
         on them can never be produced:\n  {}\n\
         FIX: the ids are built in agent/scripts/e2e/e2e.js (`record(...)` / `tool(...)`) and read in \
         agent/e2e-judge/src/checks/. Rename both, or neither.",
        missing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn every_plan_key_is_read_by_one_of_the_two_halves_and_no_driver_key_is_missing_from_the_plan() {
    // **THE PLAN CARRIES TWO KINDS OF KEY, AND THE DIFFERENCE IS WHO READS IT.** An INPUT is a command the
    // driver types or a path it joins (`FX.session_command`); an EXPECTATION is a marker or a ceiling the JUDGE
    // compares an answer against (`FX.background_marker`, `budgets.gov_sibling_ms`). A key NEITHER half reads
    // is dead policy — and a key the DRIVER reads that the plan does not publish is worse: `undefined` gets
    // typed into a terminal or interpolated into a URL on a device, where nothing says why.
    let src = driver();
    let plan = serde_json::to_value(summrise_e2e_judge::plan::Plan::default()).unwrap();
    let mut judge_src = String::new();
    for entry in std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/checks"))
        .expect("src/checks")
    {
        let p = entry.expect("a dir entry").path();
        if p.extension().map(|e| e == "rs").unwrap_or(false) {
            judge_src.push_str(&std::fs::read_to_string(&p).expect("a check module"));
        }
    }

    let mut dead: Vec<String> = Vec::new();
    for group in ["fixtures", "budgets"] {
        for (key, _) in plan[group].as_object().expect("an object") {
            if !src.contains(key.as_str()) && !judge_src.contains(key.as_str()) {
                dead.push(format!("{group}.{key}"));
            }
        }
    }
    assert!(
        dead.is_empty(),
        "the plan publishes these keys and NEITHER the driver nor the judge reads them:\n  {}\n\
         FIX: use them, or delete them from agent/e2e-judge/src/plan.rs. A key nobody reads is a policy that \
         is not in force.",
        dead.join("\n  ")
    );

    // The other direction, and it is the one a device finds out about: `FX.<key>` and `B.<key>` in the driver.
    let mut dangling: Vec<String> = Vec::new();
    for (member, group) in [("FX.", "fixtures"), ("B.", "budgets")] {
        let mut from = 0;
        while let Some(at) = src[from..].find(member) {
            let start = from + at + member.len();
            from = start;
            let rest = &src[start..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            let key = &rest[..end];
            if key.is_empty() {
                continue;
            }
            if plan[group].get(key).is_none() {
                dangling.push(format!("{member}{key}"));
            }
        }
    }
    dangling.sort();
    dangling.dedup();
    assert!(
        dangling.is_empty(),
        "the driver reads these plan keys and the plan does not publish them, so a device would get \
         `undefined`:\n  {}\n\
         FIX: add them to agent/e2e-judge/src/plan.rs, or stop reading them.",
        dangling.join("\n  ")
    );
}

#[test]
fn every_recorded_observation_id_belongs_to_a_section_the_judge_can_name() {
    // The `prefix()` map is what lets the judge tell "the run never reached this section" from "the run
    // reached it and the judge could not read one id". A driver that renamed a section's prefix would make the
    // second look like the first, which is the silence the guard exists to break.
    let src = driver();
    for section in summrise_e2e_judge::plan::ALL_SECTIONS {
        // The prefix, not a quoted literal: the driver builds its ids by concatenation (`'mcp.' + tag + '.view.'`
        // and `'terminal.' + 'open'`), so what must be there is the prefix itself appearing in an id it writes.
        assert!(
            src.contains(section.prefix()),
            "the driver never writes the `{}` prefix that section `{}` is judged under",
            section.prefix(),
            section.name()
        );
    }
}
