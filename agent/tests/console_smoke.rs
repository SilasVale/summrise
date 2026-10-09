//! `console-smoke-check` — RUN EVERY CONSOLE RENDER SMOKE, AND REFUSE TO REPORT SUCCESS HAVING RUN NONE.
//!
//! WHAT IT IS FOR, in the `.mjs`'s own header: four render smokes live in `gateway/ui/` and CI ran exactly
//! one of them — `render-smoke.mjs`, written inline in `ci.yml`. `overview-render-smoke.mjs` recorded its
//! own neglect in a comment: "Nothing runs this smoke in CI (ci.yml runs render-smoke.mjs only), which is
//! why the drift survived." The drift was that `.ov-firstrun` is a class the view has NEVER rendered, so
//! two of its checks — including scene 3, the honesty rule the whole file exists for — were asserting on
//! the empty string and could not fail. A smoke that runs nowhere is worse than no smoke, because its
//! green is read as coverage.
//!
//! THE SET IS DISCOVERED, NOT LISTED: every `*-render-smoke.mjs` next to the views is run, so a new one is
//! covered the moment it exists and this file cannot become a second list that drifts. The floor below is
//! what tells "the tree moved" from "there are no smokes any more".
//!
//! IT NEEDS `gateway/ui` DEPENDENCIES — the smokes are jsdom, and without them every one of them dies with
//! "Cannot find module 'jsdom'" — and it reads the BUILT bundle named by `gateway/public/index.html`.
//!
//! ── MIGRATED FROM `scripts/test/console-smoke-check.mjs`, WHICH IS DELETED ─────────────────────────
//! A migration that leaves both is two gates, not one. It runs in `cargo test -p summrise-agent`, and the
//! `agent` job gained the `gateway/ui` install and build it needs; the `ui` job's step went with the file.
//! Nothing about the discovery, the floor, the bundle lookup or the failure reporting changes.
//!
//! ── THE MUTATION THAT MUST FAIL THIS GATE ──────────────────────────────────────────────────────────
//! Read this when you change this file: the mutation is how you find out whether the gate can still fail
//! at all. A gate that cannot be broken is worse than no gate.
//!
//! MUTATION: rename one render smoke so the discovery finds fewer than four of them.
//! RESULT:   exit 101 —
//!   FAIL read only 3 console smoke(s) from gateway/ — the tree moved, so this proves nothing
//!
//! MUTATION: plant a failing check in one smoke (`✗` in its output).
//! RESULT:   exit 101, naming the smoke, its first four failing lines, and the count —
//!   FAIL  overview-render-smoke.mjs
//!           ✗ <the line the smoke printed>
//!   3/4 console smoke(s) passed
//!
//! AND THE MUTATION THE FLOOR EXISTS FOR: delete a smoke AND its source edit with it — the gate refuses
//! rather than reporting a smaller green. The `.mjs` was written after a smoke that ran nowhere reported
//! coverage it did not have, and a port that cannot be broken that way would be that failure again.

mod common;

use common::{repo, Spawn};
use std::fs;

/// The floor. Three is "the tree moved"; four is the tree this gate was written against.
const MIN_SMOKES: usize = 4;

/// `/index-[^"]*\.js/` — the bundle `gateway/public/index.html` names, first match, or `None` when the
/// console has never been built. Hand-rolled for the same reason `count_of` is: three anchored shapes do
/// not justify a dependency in a crate that ships.
fn built_bundle(html: &str) -> Option<String> {
    let open = html.find("index-")?;
    let rest = &html[open..];
    let end = rest
        .find('"')
        .expect("the bundle name is inside an attribute, so a quote follows it");
    Some(rest[..end].to_string())
}

#[test]
fn every_console_render_smoke_runs_and_none_of_them_may_be_missing() {
    let root = repo();
    let ui = root.join("gateway/ui");

    // THE SUFFIX IS THE RULE, not `*-render-smoke.mjs`: `render-smoke.mjs` has no screen prefix, and a glob
    // with a leading `*` would drop it — which is the smoke CI already ran, the one this gate exists to stop
    // being the only one.
    let mut smokes: Vec<String> = fs::read_dir(&ui)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", ui.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|f| f.ends_with("render-smoke.mjs"))
        .collect();
    smokes.sort();

    assert!(
        smokes.len() >= MIN_SMOKES,
        "  FAIL read only {} console smoke(s) from gateway/ — the tree moved, so this proves nothing",
        smokes.len()
    );

    let html = fs::read_to_string(root.join("gateway/public/index.html"))
        .expect("gateway/public/index.html must exist — it is the console's entry point");
    let Some(bundle) = built_bundle(&html) else {
        panic!("  FAIL no index-*.js in gateway/public/index.html — the console is not built");
    };
    let built = root.join("gateway/public/assets").join(&bundle);

    let mut failed = 0usize;
    for smoke in &smokes {
        // `execFileSync("node", [s, built], { cwd: UI })` — the SMOKE first, the bundle it must measure
        // second. Getting this order wrong is how the first version of this port reported `0/4`: every smoke
        // was handed a path where it expected its own name.
        let out = Spawn::new("node")
            .args([smoke.as_str()])
            .arg_path(&built)
            .cwd(&ui)
            .run();
        if out.ok() {
            println!("  ok    {smoke}");
            continue;
        }
        failed += 1;
        println!("  FAIL  {smoke}");
        // `/✗|FAIL|not ok/`, first four — the smoke's own report of what it checked.
        let hits: Vec<&str> = out
            .stdout
            .split('\n')
            .filter(|l| l.contains('✗') || l.contains("FAIL") || l.contains("not ok"))
            .take(4)
            .collect();
        if !hits.is_empty() {
            for line in hits {
                println!("          {}", line.trim());
            }
        } else {
            // A SMOKE THAT DIES BEFORE ITS FIRST CHECK HAS NO FAILING LINE, and filtering for one is exactly
            // how this gate reported "FAIL overview-render-smoke.mjs" with no reason in CI for four rounds —
            // the cause was a missing `jsdom` (the job had no console dependencies) and the evidence was a
            // stack trace on a stream nobody printed. Show the line that NAMES the error, then the tail.
            let all = out.both();
            if let Some(named) = all
                .split('\n')
                .find(|l| l.contains("Error") || l.contains("error:"))
            {
                println!("          {}", named.trim());
            }
            let lines: Vec<&str> = all.trim().split('\n').collect();
            for line in lines[lines.len().saturating_sub(3)..].iter() {
                println!("          {}", line.trim());
            }
        }
    }

    let passed = smokes.len() - failed;
    println!("\n  {passed}/{} console smoke(s) passed", smokes.len());
    assert!(
        failed == 0,
        "console-smoke: {failed} of {} console render smoke(s) failed — a smoke that runs nowhere is worse \
         than no smoke, because its green is read as coverage",
        smokes.len()
    );
}
