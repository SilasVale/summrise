//! The user's ideas inbox is the loop's stated TOP-PRIORITY input, and nothing read it.
//!
//! `docs/agents/ideas.md` opens: "Anything in here is the loop's TOP-PRIORITY work, and
//! it is the anchor against drift: round 100's `aurora` was the loop *interpreting* a
//! request instead of following one, and the user had to say plainly that they never
//! wanted it." Its rules require every entry to get "either a round of its own, or an
//! explicit reasoned verdict in the round log — with evidence", and to be "never dropped
//! silently".
//!
//! **Round 251 measured how that has gone: `grep -c 'ideas.md'` over `agent/AGENTS.md`
//! and `docs/agents/iteration-coverage.md` returns ZERO and ZERO.** The file was created
//! as the fix for round 100's drift, and in the 151 rounds since it was never named once
//! — so a reader of the journal could not tell whether the loop knew it existed, and the
//! one rule the file states most plainly ("never dropped silently") had no instrument
//! behind it at all.
//!
//! This asserts the two things that are mechanical: the inbox is reachable from the
//! loop's own records, and no entry is left in a non-standing state without the round log
//! naming it.

use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("agent/ has a parent")
        .to_path_buf()
}

/// The inbox's table rows as (id, status), skipping the header and separator.
fn entries(inbox: &str) -> Vec<(String, String)> {
    inbox
        .lines()
        .filter(|l| l.starts_with("| "))
        .filter_map(|l| {
            let cells: Vec<&str> = l.split('|').map(str::trim).collect();
            // cells[0] is empty (leading '|'); id, idea, status, where
            let id = cells.get(1)?.to_string();
            let status = cells.get(3)?.to_string();
            if id.eq_ignore_ascii_case("#") || id.starts_with("--") || id.is_empty() {
                return None;
            }
            Some((id, status))
        })
        .collect()
}

#[test]
fn the_ideas_inbox_is_reachable_and_no_entry_is_unaccounted_for() {
    let inbox = std::fs::read_to_string(repo().join("docs/agents/ideas.md"))
        .expect("docs/agents/ideas.md exists — it is the loop's stated top-priority input");
    let journal = std::fs::read_to_string(repo().join("agent/AGENTS.md")).expect("the journal");
    let ledger = std::fs::read_to_string(repo().join("docs/agents/iteration-coverage.md"))
        .expect("the ledger");

    // (1) The inbox must be NAMED by the loop's own records, so that a reader of round N
    // can tell the loop knows this file exists. Before round 251 it was named zero times.
    assert!(
        journal.contains("ideas.md") || ledger.contains("ideas.md"),
        "neither agent/AGENTS.md nor docs/agents/iteration-coverage.md mentions \
         `docs/agents/ideas.md`. That file is the objective's TOP-PRIORITY input and its own \
         header calls it \"the anchor against drift\"; if the loop's records never name it, \
         nothing distinguishes \"we considered it\" from \"we forgot it exists\". Name it in \
         the ledger's open-items section (with the current status) and say so in a round.",
    );

    // (2) Every entry whose status is not `standing` must be visible in the round log,
    // because the inbox's own rule is "each entry gets either a round of its own, or an
    // explicit reasoned verdict in the round log — with evidence".
    let rows = entries(&inbox);
    assert!(
        !rows.is_empty(),
        "the inbox's table did not parse — fix THIS extractor rather than deleting the test",
    );
    let unaccounted: Vec<String> = rows
        .iter()
        .filter(|(_, status)| !status.eq_ignore_ascii_case("standing"))
        .filter(|(id, _)| {
            // An id is accounted for when the journal references the inbox AND either the
            // id appears with surrounding context or the status word does.
            !(journal.contains("ideas.md") && journal.contains(id.as_str()))
        })
        .map(|(id, status)| format!("#{id} ({status})"))
        .collect();
    assert!(
        unaccounted.is_empty(),
        "these ideas-inbox entries are not `standing` and the round log does not name them: \
         {unaccounted:?}. The inbox's rule is that each entry gets a round of its own or an \
         explicit reasoned verdict WITH EVIDENCE in the round log — never dropped silently.",
    );
}
