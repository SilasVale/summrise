//! `panel-sheet-freshness-check` — A PANEL SOURCE CHANGE OWES ITS BUILT SHEET, AND A REBUILD MUST PROVE IT.
//!
//! WHAT IT IS FOR (round 177, from a gap the fifteenth exploration measured): FIVE gates read the
//! COMMITTED `agent/resources/panel/panel.css` — chrome-stillness, feedback, motion, state-colour and
//! stylesheet-hygiene — and NOTHING compared that sheet to the source that generates it:
//!
//!   * `agent/build.rs` guards by MTIME (it refuses to compile when `panel-react/src` is newer than
//!     `resources/panel/`), which catches "source edited, sheet not rebuilt" but not a CONTENT drift — a
//!     restored file, a checkout with fresh timestamps, or a rebuild that produced different bytes at the
//!     same second;
//!   * CI's `panel` job DOES run `npm run build` and throws the result away — it never asked whether the
//!     tree it built from was consistent;
//!   * so a source edit whose build output was never committed leaves five gates measuring a stale sheet,
//!     and every one of them passes.
//!
//! THE CHECK IS A REBUILD, and the assertion is that a rebuild changes NOTHING git tracks. That shape is
//! COPIED from `agent/tests/console_assets.rs`, which exists for exactly this reason on the console side. In
//! the good case this is a no-op on the tree; in the bad case the diff it leaves behind IS the fix, which
//! is why a rebuild beats an mtime comparison — round 146 recorded what an mtime guard costs when a file
//! is RESTORED rather than edited.
//!
//! IT NEEDS `panel-react` DEPENDENCIES and takes a few seconds.
//!
//! ── MIGRATED FROM `scripts/test/panel-sheet-freshness-check.mjs`, WHICH IS DELETED ────────────────
//! A migration that leaves both is two gates, not one. It runs in `cargo test -p summrise-agent` — a
//! command CI already runs — and the `agent` job gained the `panel-react` install it needs, so the `panel`
//! job's step went with the file.
//!
//! ── THE MUTATION THAT MUST FAIL THIS GATE ──────────────────────────────────────────────────────────
//! Read this when you change this file: the mutation is how you find out whether the gate can still fail
//! at all. A gate that cannot be broken is worse than no gate.
//!
//! MUTATION: commit a sheet `agent/resources/panel-react` would not produce, or move its CI step to a job
//!           that installs no panel-react dependencies
//! RESULT:   exit 1 both ways: "a rebuild of the panel changed tracked files, so the COMMITTED sheet is
//!           not what the source produces" — and the diff it leaves IS the fix. Round 190's placement bug
//!           was the second case in reverse: the step sat in `ui`, which installs only gateway/ui's
//!           dependencies, and the check faithfully reported "the panel build itself failed" for a missing
//!           node_modules. Round 191 added exit 2 for a host that cannot rebuild at all, which is how
//!           `all-gates` runs it. **NAMED HERE IN ROUND 199 BECAUSE THE CENSUS COULD NOT SEE IT**: that
//!           census matched only an invocation written as `node scripts/test/...`, and this step carries
//!           the path-prefix form, so it was invisible — the same trap `all-gates.bash` hit before round
//!           170.

mod common;

use common::{git, has_node_modules, repo, tail, Spawn};

/// The one directory this gate may not find pre-modified. Round 190 is why it is a directory and not the
/// whole tree: the check's own uncommitted edit used to fire the precondition, which is a false positive
/// with a message about the sheet.
const ARTIFACT: &str = "agent/resources/panel";

#[test]
fn a_panel_source_change_owes_its_built_sheet() {
    // THE QUESTION IS ABOUT THE ARTIFACT, NOT THE TREE (round 190). This used to refuse on a dirty tree,
    // which is the wrong precondition twice over: it cannot be satisfied in CI's `panel` job (which
    // builds before its gates) and it says nothing about whether the SHEET is current. Scoped to the
    // built output, `git status` answers the real question and tolerates whatever else is in the tree.
    let dirty = git(&["status", "--porcelain", "--", ARTIFACT]);
    assert!(
        dirty.is_empty(),
        "FAIL agent/resources/panel/ was already modified before this check rebuilt it, so a rebuild's \
         diff\ncould not be told from the change that was already there:\n{dirty}\n\
         Commit or revert that first — this check compares against HEAD."
    );

    // THE HOST MUST BE ABLE TO RUN THIS CHECK AT ALL (round 191). This gate REBUILDS the panel, so it
    // needs that project's dependencies. `pack-chain` runs every gate via `all-gates.bash` and installs
    // only gateway/ui's — so without this, the check reports a FAILED BUILD for a missing node_modules
    // and reddens a job that has nothing to do with the panel. The `.mjs` exits 2 here, which its runner
    // maps to n/a and does NOT count as a failure; in `cargo test` the same declaration is a printed line
    // and no panic. The `panel` job and the `agent` job, which install them, are where the real
    // measurement happens.
    if !has_node_modules("agent/resources/panel-react") {
        println!(
            "n/a agent/resources/panel-react/node_modules is absent, so this host cannot rebuild the \
             panel. CI's `panel` job installs them and runs this check there; nothing is proved or \
             disproved here."
        );
        return;
    }

    let panel_react = repo().join("agent/resources/panel-react");
    let build = Spawn::new("npm")
        .args(["run", "build"])
        .cwd(&panel_react)
        .run();
    assert!(
        build.ok(),
        "FAIL the panel build itself failed (rc={}) — that is a different problem, and it should be fixed \
         first:\n{}",
        build.code(),
        tail(&build.stdout, 500)
    );

    // SCOPED TO THE ARTIFACT, like the pre-flight above (round 190). The status is the whole assertion:
    // this is not a `diff` against a captured "before", because a rebuild that changes the sheet is a
    // finding whether or not the sheet was clean when the build started — and it was, one check above.
    let after = git(&["status", "--porcelain", "--", ARTIFACT]);
    assert!(
        after.is_empty(),
        "FAIL a rebuild of the panel changed tracked files, so the COMMITTED sheet is not what the source \
         produces:\n{after}\n\
         Five gates read agent/resources/panel/panel.css (chrome-stillness, feedback, motion, state-colour,\n\
         stylesheet-hygiene), so they were measuring the OLD sheet. The diff above IS the fix: commit it.\n\
         (Leave the rebuild in place — it is the artifact the source says should ship.)"
    );

    println!(
        "panel-sheet-freshness: a rebuild of the panel changes nothing git tracks — the committed sheet \
         matches the source"
    );
}
