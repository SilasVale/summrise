//! The ledger's head line and the journal's head line are two HAND-WRITTEN numbers
//! describing the same fact — how far the loop has run — and nothing compared them.
//!
//! Round 213 found the ledger 91 rounds behind: it read `round 121` while the journal
//! read `round 212`, and no test could refuse the claim "the ledger is current".
//!
//! That is the same shape rounds 199 and 211 found in the code-viewer mirror: two
//! artifacts carry one obligation, only one of them is written every round, and the
//! unwritten one drifts silently. Repairing the number is the small half; this is the
//! half that makes the next drift a RED TEST instead of a discovery.
//!
//! It reads both files as DATA rather than restating either number, so it cannot go
//! stale the way a hardcoded expectation would.

use std::path::PathBuf;

fn repo_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("agent/ has a parent")
        .to_path_buf()
}

/// The highest `round NNN` in a `Last updated:` line of the journal.
fn journal_round(text: &str) -> Option<u32> {
    text.lines()
        .find(|l| l.starts_with("Last updated:"))?
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|w| w[0] == "round")
        .and_then(|w| {
            w[1].trim_matches(|c: char| !c.is_ascii_digit())
                .parse()
                .ok()
        })
}

/// The number in the ledger's `Round log head: **round NNN**` line.
fn ledger_round(text: &str) -> Option<u32> {
    let line = text.lines().find(|l| l.starts_with("- Round log head:"))?;
    let after = line.split("round ").nth(1)?;
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[test]
fn the_ledger_head_and_the_journal_head_name_the_same_round() {
    let journal = std::fs::read_to_string(repo_dir().join("agent/AGENTS.md"))
        .expect("agent/AGENTS.md is readable");
    let ledger = std::fs::read_to_string(repo_dir().join("docs/agents/iteration-coverage.md"))
        .expect("docs/agents/iteration-coverage.md is readable");

    let j = journal_round(&journal).expect("the journal has a `Last updated: … round N` line");
    let l = ledger_round(&ledger)
        .expect("the ledger has a `- Round log head:` line with a round number");

    assert_eq!(
        j, l,
        "docs/agents/iteration-coverage.md's `Round log head` says round {l} while \
         agent/AGENTS.md's `Last updated` says round {j}. The ledger is written by hand \
         and the journal is written every round, so the ledger drifts silently unless \
         something compares them — that is what this test is for. Update the ledger's \
         head line (and its Current state) in the SAME commit as the round that moved it.",
    );
}
