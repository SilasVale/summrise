//! Every `./scripts/build.sh <sub>` a guide advertises must be a subcommand the script
//! actually accepts.
//!
//! Round 246 read the two guide PAIRS' mirror obligation. `agent/tests/module_map.rs`
//! already enforces the one checkable thing for the agent pair — that `AGENTS.md` and
//! `CLAUDE.md` document the same SET of modules — and its docstring had already reasoned
//! about why a blanket file comparison would be the wrong instrument. **The ROOT pair has
//! no such check at all**, and its headers state the obligation only as "keep both in
//! sync", which is not a testable sentence.
//!
//! So this asserts something narrower and mechanical instead: both root guides list the
//! build entry points, and `scripts/build.sh` is the authority for which exist. Measured
//! before writing it — all six advertised subcommands (`agent`, `gateway`, `index`,
//! `proxies`, `api-relay`, `deploy`) really are accepted, checked by INVOKING the script
//! rather than by grepping its `case` arms, because the first grep missed `agent`
//! (it is handled before the case, with its own usage line). A guide advertising a
//! command that does not exist is round 232/237's shape: a document pointing at
//! something absent.
use std::collections::BTreeSet;
use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("agent/ has a parent")
        .to_path_buf()
}

/// The subcommands a guide advertises, as `./scripts/build.sh <sub>` mentions.
fn advertised(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let mut rest = line;
        while let Some(i) = rest.find("scripts/build.sh ") {
            let after = &rest[i + "scripts/build.sh ".len()..];
            let word: String = after
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                .collect();
            if !word.is_empty() {
                out.insert(word);
            }
            rest = after;
        }
    }
    out
}

#[test]
fn every_build_subcommand_the_guides_advertise_is_one_the_script_accepts() {
    let mut advertised_all = BTreeSet::new();
    for guide in ["AGENTS.md", "CLAUDE.md"] {
        let t = std::fs::read_to_string(repo().join(guide))
            .unwrap_or_else(|e| panic!("{guide} is readable: {e}"));
        advertised_all.extend(advertised(&t));
    }
    assert!(
        advertised_all.len() >= 5,
        "expected the root guides to advertise several build subcommands, found {:?} — if the \
         mention syntax changed, fix THIS extractor rather than deleting the test (ADR 0011's \
         deletion criterion: an assertion with no subject should be removed deliberately and \
         said so, not left passing emptily)",
        advertised_all,
    );

    // PARSE the script's own usage line — DO NOT INVOKE IT.
    //
    // The first version of this test ran `bash scripts/build.sh <sub>` for each advertised
    // subcommand and checked the output for "usage:". It passed, and it was WRONG FOR A
    // REASON THAT MATTERS MORE THAN THE ASSERTION: `agent` performs a full cross-compile
    // (333 s), and `gateway` / `index` / `proxies` / `api-relay` ENTER THEIR DEPLOY PATHS —
    // `require_cf_token` falls back to `~/.cloudflare-token`, which exists on this box, so
    // the guard does NOT stop them. Nothing changed (the worktree stayed clean and the live
    // asset's sha256 was identical before and after), but a test whose verification method
    // can deploy is not a test.
    //
    // The usage line is the script's own statement of its subcommands, and reading it is
    // free. This is rounds 238/244's rule applied where it was skipped: when the direct
    // verification is destructive, find the part of it that is a measurement — and this one
    // was sitting in line 336 the whole time.
    let script = std::fs::read_to_string(repo().join("scripts/build.sh"))
        .expect("scripts/build.sh is readable");
    let usage = script
        .lines()
        .find(|l| l.contains("usage: $0 ["))
        .expect("scripts/build.sh states its subcommands in a usage line");
    let accepted: BTreeSet<String> = usage
        .split('[')
        .nth(1)
        .and_then(|r| r.split(']').next())
        .map(|list| list.split('|').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();
    assert!(
        accepted.len() >= 5,
        "the usage line's subcommand list did not parse (got {accepted:?} from {usage:?}) — fix          THIS extractor rather than deleting the test",
    );

    let unrecognised: Vec<&String> = advertised_all.difference(&accepted).collect();
    assert!(
        unrecognised.is_empty(),
        "these subcommands are advertised in a root guide but scripts/build.sh's own usage line \
         does not list them: {unrecognised:?} (the script accepts {accepted:?}). A guide is a \
         document that sends readers to commands; one that names a command which does not exist \
         is round 232/237's shape. Either fix the guide or add the subcommand.",
    );
}
