//! **MAIN ADVANCES ONLY BY MERGE — THE GATE, MOVED OUT OF `scripts/test/main-shape-check.mjs`.**
//!
//! ── THE MUTATION THAT MUST FAIL THIS GATE ───────────────────────────────────────────────────────
//! Read this when you change this file: the mutation is how you find out whether the check can still fail at
//! all. A gate that cannot be broken is worse than no gate.
//!
//! MUTATION: in `verdict`, change `parents >= 2` to `parents >= 1`.
//! RESULT:   `a direct commit on main — the shape this exists to refuse` fails, because `main` with one
//!           parent would be accepted — which is precisely the commit shape this rule exists to refuse.
//!           (Measured on the port: the fixture table's first row is what catches it.)
//!
//! AND THE SECOND MUTATION, THE ONE THE FIXTURE TABLE EXISTS FOR: change `parents >= 2` to `parents >= 0`
//! and the THIRD row still passes — but flip the rule to constrain every branch instead of `main` alone and
//! `ordinary branch work must never be blocked` fails. **A RULE THAT BLOCKS ORDINARY BRANCH WORK WOULD BE
//! REVERTED WITHIN A ROUND**, which is why the pair is here rather than a one-directional test.
//!
//! WHY IT MOVED. `scripts/test/main-shape-check.mjs` was one of the seven `.mjs` gates the migration's P1
//! left behind. It fails none of the three constraints that decide movability — it reads data, it spawns no
//! DEPENDENCY (it runs `git`, which is a tool every checkout has), and it is provable inside this repository
//! — so it is the first of the seven to come across. The `.mjs` STAYS until this is proven equivalent on the
//! same inputs; the fixture table below is the corpus, and it is the same six rows, in the same order, with
//! the same expected verdicts.
//!
//! THE PARENT COUNT COMES FROM THE COMMIT OBJECT, NOT FROM A REV-LIST WALK, AND CI IS WHY. `actions/
//! checkout@v4` defaults to `fetch-depth: 1`, so the runner holds a SHALLOW clone; git grafts the boundary
//! commit and `git rev-list --parents -n 1 HEAD` answers with the SHA ALONE — zero parents — for a commit
//! that has two. `git cat-file -p HEAD` reads the object, whose `parent` lines are there regardless of depth.
//! The `agent` job is where `cargo test` runs, and it checks out shallow by default, so this gate meets the
//! same shape the original was fixed for.

use std::process::Command;

/// The whole decision, pure and testable: `main` must be reached by a merge, and nothing else is constrained.
///
/// The returned string is the reason, and it is asserted against the original's wording — a gate's OUTPUT is
/// part of its contract, and a port that answered correctly with a different sentence would not be
/// byte-proven equivalent.
pub fn verdict(branch: &str, parents: usize) -> (bool, String) {
    if branch != "main" {
        return (
            true,
            format!("not main ({branch}) — this rule constrains main only"),
        );
    }
    if parents >= 2 {
        return (true, format!("main at a merge commit ({parents} parents)"));
    }
    (
        false,
        format!(
            "main is at a commit with {parents} parent(s), so it advanced WITHOUT a merge. Work reaches main \
             through a branch that was verified and reviewed, and a plain `git merge` FAST-FORWARDS when main \
             has not moved — creating no commit at all. Merge with --no-ff."
        ),
    )
}

/// `[branch, parent count, expected ok, what it is]` — **THE FIXTURE TABLE IS THE PROOF**, and it is the
/// original's six rows. The two that matter most are the pair no single-direction test can cover: `main` + 1
/// parent must FAIL and `change/x` + 1 parent must PASS.
const SELF_TEST: [(&str, usize, bool, &str); 6] = [
    (
        "main",
        1,
        false,
        "a direct commit on main — the shape this exists to refuse",
    ),
    ("main", 0, false, "a root commit on main"),
    (
        "main",
        2,
        true,
        "a --no-ff merge of a change branch: the shape the flow produces",
    ),
    ("main", 3, true, "an octopus merge, which is still a merge"),
    (
        "change/some-work",
        1,
        true,
        "ordinary branch work must never be blocked",
    ),
    ("change/some-work", 0, true, "a branch's first commit"),
];

fn git(args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {}: {e}", args.join(" ")));
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn the_fixture_table_holds() {
    let mut wrong: Vec<String> = Vec::new();
    for (branch, parents, want, what) in SELF_TEST {
        let (got, why) = verdict(branch, parents);
        if got != want {
            wrong.push(format!(
                "  {branch} at {parents} parent(s): wanted ok={want}, got ok={got} — {what} ({why})"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "FAIL main-shape: {} of {} fixture(s) wrong — the decision does not do what it says.\n{}",
        wrong.len(),
        SELF_TEST.len(),
        wrong.join("\n")
    );
}

/// **THE WORDINGS, BYTE FOR BYTE, FROM THE ORIGINAL.** A gate's output is part of its contract: a port that
/// answered correctly with a different sentence would not be equivalent, and the sentence is what a reader
/// acts on. These four strings are copied from `node scripts/test/main-shape-check.mjs`'s own output on
/// 2026-10-03, and this test is the equivalence proof for the decision the `.mjs` still owns.
#[test]
fn the_wordings_match_the_javascript_byte_for_byte() {
    let cases: [(&str, usize, &str); 4] = [
        (
            "main",
            1,
            "main is at a commit with 1 parent(s), so it advanced WITHOUT a merge. Work reaches main through a branch that was verified and reviewed, and a plain `git merge` FAST-FORWARDS when main has not moved — creating no commit at all. Merge with --no-ff.",
        ),
        (
            "main",
            0,
            "main is at a commit with 0 parent(s), so it advanced WITHOUT a merge. Work reaches main through a branch that was verified and reviewed, and a plain `git merge` FAST-FORWARDS when main has not moved — creating no commit at all. Merge with --no-ff.",
        ),
        ("main", 2, "main at a merge commit (2 parents)"),
        (
            "change/x",
            1,
            "not main (change/x) — this rule constrains main only",
        ),
    ];
    for (branch, parents, want) in cases {
        let (_, why) = verdict(branch, parents);
        assert_eq!(why, want, "{branch} at {parents} parent(s)");
    }
}

#[test]
fn the_repository_is_where_it_says_it_is() {
    // The branch comes from Actions on a push; locally it is asked for directly. The parent count comes from
    // git, never from a guess about the branch.
    let branch = std::env::var("GITHUB_REF_NAME")
        .ok()
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| {
            let b = git(&["branch", "--show-current"]).trim().to_string();
            if b.is_empty() {
                "(detached)".to_string()
            } else {
                b
            }
        });
    let parents = git(&["cat-file", "-p", "HEAD"])
        .lines()
        .filter(|l| l.starts_with("parent "))
        .count();
    let (ok, why) = verdict(&branch, parents);
    assert!(ok, "FAIL main-shape: {why}");
    println!(
        "main-shape: {} fixture(s) hold; {branch} at {parents} parent(s) — {why}",
        SELF_TEST.len()
    );
}
