//! `console-assets-check` — A CONSOLE SOURCE CHANGE OWES ITS BUILT ASSETS, AND A REBUILD MUST PROVE IT.
//!
//! WHAT IT IS FOR, in the `.mjs`'s own header (round 178, from a gap round 177 measured): the console's
//! built assets under `gateway/public/assets/` are TRACKED, hashed, and referenced by
//! `gateway/public/index.html`. Rounds 172 and 173 changed console source and left those assets at the
//! old revision, and NOTHING noticed — the PANEL has a guard for exactly this (`agent/build.rs` refuses
//! to compile when `resources/panel-react/src` is newer than `resources/panel/`), the console embeds
//! nothing, and CI's `ui` job rebuilt the same new file and never asked whether the tree it built from
//! was consistent.
//!
//! THE CHECK IS A REBUILD, and the assertion is that a rebuild changes NOTHING that git tracks. In the
//! good case it is a no-op on the tree; in the bad case the diff it leaves behind IS the fix, which is
//! the whole reason to prefer this shape over an mtime comparison — round 146 recorded what an mtime
//! guard costs when a file is RESTORED rather than edited.
//!
//! IT NEEDS `gateway/ui` DEPENDENCIES and takes about a minute; it refuses to run on a dirty tree, so the
//! diff it produces cannot be confused with somebody's work.
//!
//! ── MIGRATED FROM `scripts/test/console-assets-check.mjs`, WHICH IS DELETED ────────────────────────
//! A migration that leaves both is two gates, not one. It runs in `cargo test -p summrise-agent` — a
//! command CI already runs — so the move costs no new job and no new runner language; the `agent` job
//! gained the `gateway/ui` install this gate needs, and the `ui` job's step went with the file.
//!
//! WHY RUST CAN KEEP A GATE THAT SPAWNS A TOOLCHAIN. §2 of the design spec classes this as the spawn
//! being a BOUNDARY — a platform call, made and not decided. What moved is the DECISION: the dirty-tree
//! precondition, the comparison of the tree against itself across the rebuild, and the two messages that
//! name the fix. `npm run build` is a shell-out in either language.
//!
//! ── THE MUTATION THAT MUST FAIL THIS GATE ──────────────────────────────────────────────────────────
//! Read this when you change this file: the mutation is how you find out whether the gate can still fail
//! at all. A gate that cannot be broken is worse than no gate.
//!
//! MUTATION: commit a console source change without its rebuilt assets — reproduced by appending one byte
//!           to the committed bundle `gateway/public/assets/index-*.js`.
//! RESULT:   exit 101 —
//!   console-assets: the tracked assets did NOT match the source — a rebuild changed them:
//!   M gateway/public/assets/index-<hash>.js
//!
//!   Commit the rebuild's output. The assets under gateway/public/assets/ are tracked and referenced by
//!   index.html, so a freshly checked-out tree would otherwise serve a console built from older source.
//!
//! AND THE MUTATION THAT MUST NOT: the same gate on a clean tree prints the ok line and proves nothing —
//! which is why the mutation is run, not reasoned about.
//!
//! MUTATION: make the rebuild itself fail (a syntax error in `gateway/ui/src`).
//! RESULT:   exit 101 — "the console build itself failed (rc=2) — that is a different problem, and it
//!           should be fixed first", carrying the last 500 characters of the builder's stdout. That
//!           branch exists so a build break is not reported as a stale-asset finding.

mod common;

use common::{git, repo, tail, Spawn};

/// The whole gate, in the order the `.mjs` ran it: the precondition, the rebuild, the comparison.
#[test]
fn a_console_source_change_owes_its_built_assets() {
    let ui = repo().join("gateway/ui");

    // THE PRECONDITION, FIRST — and it is the whole subtree, not one directory: a rebuild's diff has to
    // be distinguishable from somebody's work in progress, and `gateway/ui`'s build writes into
    // `gateway/public/`, which is a different directory from the one it reads.
    let dirty = git(&["status", "--porcelain"]);
    assert!(
        dirty.is_empty(),
        "FAIL the working tree is not clean, so a rebuild's diff could not be told from your work:\n{dirty}\n\
         Commit or stash first — this check rebuilds the console in place."
    );

    // `before` is read AFTER the precondition, as the `.mjs` does. It is therefore always "" — the
    // comparison below is "the rebuild left the tree clean" — and it is kept rather than simplified
    // because the pair is what the original asserts and what the mutation is measured against.
    let before = git(&["status", "--porcelain"]);

    let build = Spawn::new("npm").args(["run", "build"]).cwd(&ui).run();
    assert!(
        build.ok(),
        "FAIL the console build itself failed (rc={}) — that is a different problem, and it should be \
         fixed first:\n{}",
        build.code(),
        tail(&build.stdout, 500)
    );

    let after = git(&["status", "--porcelain"]);
    assert!(
        after == before,
        "console-assets: the tracked assets did NOT match the source — a rebuild changed them:\n{after}\n\n\
         Commit the rebuild's output. The assets under gateway/public/assets/ are tracked and referenced \
         by index.html, so a freshly checked-out tree would otherwise serve a console built from older \
         source."
    );

    println!(
        "console-assets: a rebuild of the console changes nothing git tracks — the committed assets \
         match the source"
    );
}
