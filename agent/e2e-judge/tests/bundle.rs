//! THE COPY A DEVICE FETCHES, CHECKED AGAINST THE ONE THING THAT PRODUCES IT.
//!
//! `index/public/summrise-agent/e2e.js` is how this suite reaches a device — the device downloads it from the
//! CDN and runs it — and the plan it carries is EMBEDDED, because the device has no toolchain to ask. That
//! makes the committed copy a build product, and the failure it invites is the one the README used to ask a
//! person to prevent by hand ("if you edit `e2e.js`, re-sync the copy in the same commit"): a fix that lands in
//! the driver and not in the copy changes nothing on the device, silently, until somebody remembers.
//!
//! So the emitter's output is pinned here. The manifest's own `GENERATED | index/public/summrise-agent/` rule
//! already claimed this file was written by `--emit`; this is the test that makes the claim true.
//!
//! MUTATION: change one character of `agent/scripts/e2e/e2e.js` (add a space to a comment).
//! RESULT:   exit 101 — `the emitted bundle is stale: … differs at byte N`, with the `--bundle` command that
//!           fixes it.

use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives at agent/e2e-judge/")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = repo().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

fn emitted() -> String {
    let source = read("agent/scripts/e2e/e2e.js");
    let plan = summrise_e2e_judge::plan::plan_from_cli(None, false).expect("the default plan");
    summrise_e2e_judge::bundle::bundle(&source, &plan)
        .expect("the driver source carries the marker")
}

#[test]
fn the_committed_bundle_is_what_the_emitter_produces_today() {
    let want = emitted();
    let got = read("index/public/summrise-agent/e2e.js");
    if want == got {
        return;
    }
    let at = want
        .bytes()
        .zip(got.bytes())
        .position(|(a, b)| a != b)
        .unwrap_or(want.len().min(got.len()));
    panic!(
        "the emitted bundle is stale: the committed copy is {} bytes and the emitter produces {} — they \
         differ at byte {at}.\n\
         FIX: from the repository root,\n\
         \x20 cargo run --quiet --manifest-path agent/Cargo.toml -p summrise-e2e-judge -- --bundle -o index/public/summrise-agent/e2e.js\n\
         This is not a formality: a device downloads THAT file, so a driver fix that is not re-emitted changes \
         nothing on the device.",
        got.len(),
        want.len()
    );
}

#[test]
fn the_bundle_carries_the_whole_plan_and_drops_the_null_marker() {
    let out = emitted();
    assert!(
        !out.contains(summrise_e2e_judge::bundle::SOURCE_LINE),
        "the null marker survived"
    );
    assert_eq!(
        out.matches("@e2e-plan").count(),
        1,
        "the marker is not unique in the bundle"
    );
    for section in [
        "terminal",
        "file",
        "workflow",
        "panel",
        "mcp",
        "evidence",
        "browser",
        "governance",
        "runs",
    ] {
        assert!(
            out.contains(&format!("\"{section}\"")),
            "the bundle lost section {section}"
        );
    }
    // The fixtures travel with it, so a device run needs nothing but the token.
    assert!(
        out.contains("E2E-SESSION-OK"),
        "the bundle lost the fixture table"
    );
    assert!(
        out.contains("prompt_polls"),
        "the bundle lost the budget table"
    );
}

#[test]
fn the_bundle_is_still_one_javascript_file_with_no_imports_of_its_own() {
    // Node 20.18 on the device: a STATIC `import` is what decides the module system, and once a file has loaded
    // as ESM `require` is not available — the suite calls `require('node:fs')`, and the first attempt at this
    // very file died in section three with "require is not defined" because of one static import.
    let out = emitted();
    for line in out.lines() {
        let t = line.trim_start();
        assert!(
            !t.starts_with("import ") && !t.starts_with("import{"),
            "the driver grew a static import: {line}"
        );
        assert!(
            !t.starts_with("export "),
            "the driver grew an export: {line}"
        );
    }
    assert!(
        out.contains("require('node:fs')"),
        "the driver stopped requiring fs"
    );
}
