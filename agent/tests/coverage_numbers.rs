//! The ledger's headline coverage numbers must equal what the tool measures — for the
//! two numbers that are stable.
//!
//! `docs/agents/iteration-coverage.md`'s never-examined-surfaces section opens with
//! "MEASURED ROUND 236: **282 of the repository's 358 non-excluded files are in scope
//! (79%), and 52 are never named**". Round 245 read it against the tool and found the
//! last number had drifted to 39 while the first two still held.
//!
//! **The drift is not a bookkeeping slip, it is the instrument working**: the scope
//! (282/358) changes only when ROOTS changes, while the never-named count falls every
//! time some round's entry happens to mention a file — which is the tool's own stated
//! limit, "it measures 'named', not 'examined'". So this test asserts the TWO STABLE
//! numbers and deliberately NOT the third: asserting a value that legitimately moves
//! would fail on a normal working day, which is exactly why `surface-coverage.mjs` is
//! an ops tool and not a CI gate (ADR 0011's deletion criterion — do not assert what
//! has no stable subject; state the limit instead).
//!
//! What the third number needs is not an assertion but a habit: re-run
//! `node scripts/surface-coverage.mjs` before trusting the sentence. Round 221 recorded
//! the same conclusion for the convergence table's live-measurement evidence cells.

use std::path::PathBuf;
use std::process::Command;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("agent/ has a parent")
        .to_path_buf()
}

/// The ledger's headline, as three numbers: (in_scope, repository, percent).
///
/// ANCHORED ON THE PHRASES, not counted backwards from "are in scope": the sentence
/// begins "MEASURED ROUND 236, RE-MEASURED ROUND 245: ...", so two ROUND NUMBERS sit
/// before the counts and a backwards count reads `245` as the repository size. That is
/// exactly what this parser did on its first run — the test failed with
/// "the ledger's headline says 282 of 245 files are in scope" — which is why the
/// instrument is anchored to words rather than to position.
fn ledger_headline() -> Option<(u32, u32, u32)> {
    let s = std::fs::read_to_string(repo().join("docs/agents/iteration-coverage.md"))
        .expect("the ledger is readable");
    let i = s.find("MEASURED ROUND")?;
    let seg = &s[i..s[i..].find('\n').map(|e| i + e).unwrap_or(s.len())];
    let grab = |key: &str| -> Option<u32> {
        let k = seg.find(key)?;
        let before = &seg[..k];
        let digits: String = before
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        digits.chars().rev().collect::<String>().parse().ok()
    };
    Some((
        grab(" of the repository")?,
        grab(" non-excluded files are in scope")?,
        grab("%)")?,
    ))
}

/// The tool's own numbers, from its `--json` output.
fn measured() -> (u32, u32) {
    let out = Command::new("node")
        .arg("scripts/surface-coverage.mjs")
        .arg("--json")
        .current_dir(repo())
        .output()
        .expect("node runs surface-coverage.mjs");
    assert!(out.status.success(), "surface-coverage.mjs exited non-zero");
    let s = String::from_utf8_lossy(&out.stdout);
    let grab = |key: &str| -> u32 {
        let i = s
            .find(key)
            .unwrap_or_else(|| panic!("{key} in the tool's JSON"));
        s[i + key.len()..]
            .split(|c: char| !c.is_ascii_digit())
            .find(|t| !t.is_empty())
            .and_then(|t| t.parse().ok())
            .unwrap_or_else(|| panic!("a number after {key}"))
    };
    (grab("\"files\""), grab("\"repositoryFiles\""))
}

#[test]
fn the_ledgers_headline_scope_matches_what_the_tool_measures() {
    let (in_scope, total, pct) = ledger_headline().expect("the ledger's headline parses");
    let (measured_scope, measured_total) = measured();
    assert_eq!(
        (in_scope, total),
        (measured_scope, measured_total),
        "the ledger's headline says {in_scope} of {total} files are in scope; the tool measures \
         {measured_scope} of {measured_total}. The scope changes only when ROOTS changes, so this \
         is a drift, not a moving target — re-run `node scripts/surface-coverage.mjs` and update \
         the headline. (The NEVER-NAMED count is deliberately not asserted here: it falls every \
         time a round's entry names a file, which the tool itself documents.)",
    );
    // The percentage is self-consistency within the sentence, so a typo cannot hide.
    let expected = ((in_scope as f64 / total as f64) * 100.0).round() as u32;
    assert_eq!(
        pct, expected,
        "the headline's percentage ({pct}%) does not match its own ratio {in_scope}/{total} \
         (which rounds to {expected}%)",
    );
}
