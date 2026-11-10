//! **EVERY `#[ignore]` IN THIS WORKSPACE IS EITHER NAMED BY A CI STEP OR SAYS `manual`.**
//!
//! The repository's convention (I8, `launch/tests/windows_lifecycle.rs`) is that a test needing
//! something this workspace cannot provide is `#[ignore]`d AND RUN BY EXACT NAME in CI — "so deleting
//! the CI step turns the proof red instead of silently green". **That only holds if something checks
//! the two halves against each other.** Nothing did, and the window it leaves is the one that
//! matteres: a `#[ignore]` written with a reason nobody wired is a test that never runs and never
//! complains.
//!
//! Two forms are allowed, and nothing else:
//!   1. `#[ignore = "manual …"]` — a canary that waits for a person (a browser, an official client).
//!      CI cannot run it, and the reason says so at the attribute, which is where a reader looks.
//!   2. any other `#[ignore = "…"]` or bare `#[ignore]` — the test's NAME must appear in
//!      `.github/workflows/ci.yml` with `--ignored`, because that is the only thing that runs it.
//!
//! The scan reads the tree rather than a list, so a new ignored test is accounted for the moment it
//! is written instead of the moment somebody remembers.
//!
//! MUTATION (run, not asserted): appending `#[test] #[ignore = "a probe nobody wired"]
//! fn a_probe_that_ci_never_names()` to this file made the gate fail with
//! ``…/ignored_tests_have_a_runner.rs:195 `a_probe_that_ci_never_names` (reason: a probe nobody
//! wired) is ignored and CI never names it``. Removed, green.

use std::fs;
use std::path::{Path, PathBuf};

/// Every `.rs` file under the device workspace's crate directories.
fn rust_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Build output and vendored dependencies are not sources of this workspace.
            if path.file_name().and_then(|name| name.to_str()) == Some("target") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// One `#[ignore …]` found in a source file, with the reason it carries (if any).
#[derive(Debug)]
struct Ignored {
    file: PathBuf,
    line: usize,
    reason: Option<String>,
    /// The `fn` name the attribute belongs to: the next `fn <name>` after the attribute.
    name: Option<String>,
}

fn ignored_tests(sources: &[PathBuf]) -> Vec<Ignored> {
    let mut found = Vec::new();
    for file in sources {
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("#[ignore") {
                continue;
            }
            // `#[ignore]` or `#[ignore = "reason"]`; anything else (an argument we do not
            // understand) is reported as no reason rather than guessed at.
            let reason = trimmed
                .strip_prefix("#[ignore = \"")
                .and_then(|rest| rest.strip_suffix("\"]"))
                .map(str::to_owned);
            // The name: the first `fn ` after this attribute, skipping doc comments and other
            // attributes. A test is written as attribute(s), then `fn`, and nothing else.
            let name = text
                .lines()
                .skip(index + 1)
                .take(30)
                .find_map(|line| {
                    let line = line.trim_start();
                    let rest = line.strip_prefix("pub ").unwrap_or(line);
                    rest.strip_prefix("async fn ")
                        .or_else(|| rest.strip_prefix("fn "))
                        .map(|rest| {
                            rest.split(|c: char| !c.is_alphanumeric() && c != '_')
                                .next()
                                .unwrap_or_default()
                                .to_owned()
                        })
                })
                .filter(|name| !name.is_empty());
            found.push(Ignored {
                file: file.clone(),
                line: index + 1,
                reason,
                name,
            });
        }
    }
    found
}

/// The CI workflow, as text.
fn workflow() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml");
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("the CI workflow is readable at {}: {error}", path.display())
    })
}

/// **THE GATE.** Every ignored test is either a `manual` canary or named by a CI step.
#[test]
fn every_ignored_test_is_named_by_a_ci_step_or_says_manual() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root")
        .to_path_buf();
    let mut sources = Vec::new();
    rust_files(&root, &mut sources);
    let ignored = ignored_tests(&sources);
    assert!(
        !ignored.is_empty(),
        "the scan found no `#[ignore]` at all, which means it is reading the wrong tree: {}",
        root.display()
    );

    let ci = workflow();
    // Only the `--ignored` invocations count. `--nocapture` and the test name may appear on either
    // side of them, so the check is "the name is in the file AND the file runs something --ignored".
    let ci_runs_ignored = ci.contains("--ignored");
    assert!(
        ci_runs_ignored,
        "no CI step runs anything with `--ignored`, so no ignored test can be proved"
    );

    let mut unaccounted = Vec::new();
    for test in &ignored {
        let manual = test
            .reason
            .as_deref()
            .is_some_and(|reason| reason.trim_start().starts_with("manual"));
        if manual {
            continue;
        }
        // NAMED EITHER BY NAME OR BY ITS FILE. `--test <file> -- --ignored` runs every ignored test
        // in that file, which is a legitimate wiring; `--exact <name>` is the other form. The
        // workspace-relative path after `tests/` is what a `--test` invocation names.
        let by_name = test
            .name
            .as_deref()
            .is_some_and(|name| !name.is_empty() && ci.contains(name));
        let by_file = test
            .file
            .to_str()
            .is_some_and(|path| match path.rsplit_once("tests/") {
                Some((_, rest)) => ci.contains(rest.trim_end_matches(".rs")),
                None => false,
            });
        let named = by_name || by_file;
        if !named {
            unaccounted.push(format!(
                "{}:{} {}",
                test.file.display(),
                test.line,
                match (&test.name, &test.reason) {
                    (Some(name), Some(reason)) => {
                        format!("`{name}` (reason: {reason}) is ignored and CI never names it")
                    }
                    (Some(name), None) => {
                        format!("`{name}` has a bare `#[ignore]` and CI never names it")
                    }
                    (None, _) => "an `#[ignore]` whose function could not be read".to_owned(),
                }
            ));
        }
    }
    assert!(
        unaccounted.is_empty(),
        "**A `#[ignore]` NOBODY RUNS IS A TEST THAT NEVER RUNS AND NEVER COMPLAINS.** Either give it \
         `#[ignore = \"manual …\"]` because a person must drive it, or add a CI step that runs it by \
         exact name with `--ignored`:\n  {}",
        unaccounted.join("\n  ")
    );
}

/// The scan itself, on a string, so the parsing is proved rather than assumed.
#[test]
fn the_scan_reads_both_forms_and_finds_the_name() {
    let dir = std::env::temp_dir().join(format!("ignored-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let file = dir.join("sample.rs");
    std::fs::write(
        &file,
        "#[test]\n#[ignore]\nfn bare_one() {}\n\n#[tokio::test]\n#[ignore = \"manual: needs a browser\"]\nasync fn a_canary() {}\n\n#[test]\n#[ignore = \"the CI job runs it\"]\nfn named_by_ci() {}\n",
    )
    .expect("the sample is written");

    let found = ignored_tests(&[file]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(found.len(), 3, "three attributes, three entries");
    assert_eq!(found[0].name.as_deref(), Some("bare_one"));
    assert_eq!(found[0].reason, None);
    assert_eq!(found[1].name.as_deref(), Some("a_canary"));
    assert_eq!(found[1].reason.as_deref(), Some("manual: needs a browser"));
    assert_eq!(found[2].name.as_deref(), Some("named_by_ci"));
    assert_eq!(found[2].reason.as_deref(), Some("the CI job runs it"));
}
