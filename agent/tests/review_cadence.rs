//! CHARTER.md:53 requires a design review "every 10 rounds", written into the journal.
//!
//! Round 252 measured the actual cadence: the journal holds reviews at **round 150** ("THE
//! EVERY-10-ROUND DESIGN REVIEW") and **round 196**, with the entry at line 2530 stating
//! "INTERVAL: rounds 151-195. LAST REVIEW: round 150." **So the reviews happen — at
//! intervals of 46 and then 56 rounds, against a stated 10.** The obligation was real,
//! was met twice in 252 rounds, and had nothing that could fail when it was skipped —
//! the same shape round 251 found in the ideas inbox, one document over.
//!
//! CHARTER.md:53-56 is also explicit about what a review must ask: "did any metric move,
//! was any ADR reversed, was the same place changed twice, and is any metric one the loop
//! could have raised by itself? The last question is the one that keeps the rest honest."
//! This asserts the CADENCE, because a review that is never due is never written.
use std::path::PathBuf;

const INTERVAL: u32 = 10; // CHARTER.md:53 — "Every 10 rounds"

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("agent/ has a parent")
        .to_path_buf()
}

/// Every round number that carries a design-review header, ascending.
fn review_rounds(journal: &str) -> Vec<u32> {
    journal
        .lines()
        .filter_map(|l| {
            let l = l.trim_start_matches('#').trim();
            if !l.to_ascii_lowercase().contains("design review") {
                return None;
            }
            // "Design review — round 196 (CHARTER.md:53). ..." and the round-150 entry,
            // whose LAST UPDATED line names the review in its verdict.
            let after = l.split("round ").nth(1)?;
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse().ok()
        })
        .collect()
}

/// The ledger's head round, which is what "now" means for this repo.
fn head_round(ledger: &str) -> u32 {
    ledger
        .lines()
        .find(|l| l.contains("Round log head:"))
        .and_then(|l| {
            let i = l.find("round ")?;
            l[i + 6..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        })
        .expect("the ledger states its round-log head")
}

#[test]
fn a_design_review_is_due_at_least_every_ten_rounds() {
    let journal = std::fs::read_to_string(repo().join("agent/AGENTS.md")).expect("the journal");
    let ledger =
        std::fs::read_to_string(repo().join("docs/agents/iteration-coverage.md")).expect("ledger");
    let head = head_round(&ledger);
    let mut reviews = review_rounds(&journal);
    reviews.sort_unstable();
    let last = *reviews.last().expect(
        "the journal contains no design review at all — CHARTER.md:53 requires one every 10 rounds, \
         with four questions spelled out at :53-56",
    );
    assert!(
        head.saturating_sub(last) <= INTERVAL,
        "CHARTER.md:53 requires a design review every {INTERVAL} rounds; the last one in the \
         journal is round {last} and the ledger's head is round {head} — {} rounds ago. **The \
         reviews this repo actually has were written at intervals of 46 (150 -> 196) and then \
         {} — the obligation was met twice in {head} rounds, which is why the cadence needs an \
         instrument and not just a sentence.** Write one into `agent/AGENTS.md` answering \
         CHARTER.md:53-56's four questions: did any metric move, was any ADR reversed, was the \
         same place changed twice, and is any metric one the loop could have raised by itself.",
        head - last,
        head - 196,
    );
}
