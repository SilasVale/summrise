//! A WORKER CONFIG WHOSE `main` IS A BUILD ARTIFACT MUST BE BUILT BY SOMETHING.
//!
//! WHY THIS EXISTS (measured 2026-10-08). Both satellite workers' `wrangler.jsonc` name
//! `worker/build/index.js` as their entry point, and that directory is GITIGNORED — each one carries its own
//! `.gitignore` whose only line is `*`. Nothing in the deploy path built it: `./scripts/build.sh proxies`
//! runs `wrangler deploy` and nothing else, so on a clean checkout the deploy stops with
//!
//!     ✘ ERROR The entry-point file at "worker/build/index.js" was not found.
//!
//! — measured by moving both build directories aside and running the dry run. CI only ever got away with it
//! by ORDER: `scripts/test/rust-byte-checks.bash` runs `verify.mjs` earlier in the same job, and that harness
//! builds when `build/` is missing. **A DEPLOY THAT DEPENDS ON A TEST HAVING RUN FIRST IS THE SAME DEFECT
//! CLASS AS A CHECK NOBODY RUNS.** The fix is `"build": { "command": "worker-build --release worker" }` in
//! both configs — the key `gateway/wasm/wrangler.jsonc` already carried, with the crate path as its argument
//! because the satellites keep their crate one directory down.
//!
//! THE RULE IS THE PROPERTY, NOT A LIST OF FILES: **if a worker config's `main` is not a TRACKED file, the
//! config must carry a `build.command`** — the only reason a tracked config names an untracked file is that
//! something is supposed to generate it, and this gate is what makes "something" a fact rather than a hope.
//!
//! `index/`'s two configs are the DECLARED exception, and a declaration carries its reason: `scripts/build.sh`'s
//! `build_index_worker` builds that crate loudly — with its own `worker-build not found` install hint — from
//! `deploy_worker`'s index arm (`scripts/build.sh:192`), before wrangler looks for the main. That is a real
//! mechanism, so it is named rather than assumed. The list may only shrink; a new entry needs a sentence.
//!
//! MUTATION: delete the `"build"` key from `proxies/zen-us-proxy/wrangler.jsonc` and run this gate.
//! RESULT:   exit 101 —
//!             a worker config points at a build artifact that nothing builds:
//!               proxies/zen-us-proxy/wrangler.jsonc: name=zen-us-proxy main=worker/build/index.js — the file
//!               is NOT tracked and this config has no build.command
//!             FIX: give the config `"build": { "command": "worker-build --release [crate-path]" }` …
//!           **AND THE MUTATION IS NOT HYPOTHETICAL: it is the exact state BOTH satellite configs were in
//!           before this gate's own change**, which is why the gate was written in the same round.
//!
//! MUTATION (the scanner's own trap): delete the `if in_string { … continue }` branch from
//!           `common::strip_jsonc_comments`, so a `//` starts a comment wherever it appears.
//! RESULT:   **TWO tests fail, and the second is why the scanner is written by hand.**
//!             the_comment_stripper_keeps_a_string_and_drops_a_comment:
//!               a `//` inside a string was eaten: { "main": "worker
//!             every_config_pointing_at_an_untracked_main_is_built_by_something:
//!               relay/wrangler.jsonc: does not parse after comment stripping (control character
//!               (\u0000-\u001F) found while parsing a string at line 35 column 0)
//!           `relay/wrangler.jsonc` names `https://…` **inside a comment**, which is the shape a regex strip
//!           cuts in half — the same trap that made a `python3 -c` strip unusable for these files. The
//!           hand-written scanner is what keeps a URL in a comment and a `//` in a string apart.

mod common;

use std::collections::BTreeSet;

/// Configs whose artifact is built by `scripts/build.sh` instead of by their own `build.command`.
///
/// A DECLARATION, NOT A PERMISSION: each entry names the mechanism, and the list may only shrink.
const BUILT_BY_BUILD_SH: [(&str, &str); 2] = [
    (
        "summrise-dist",
        "scripts/build.sh's `build_index_worker` builds `index/worker` from `deploy_worker`'s index arm \
         (build.sh:192), loudly, before wrangler looks for a main that would not be there",
    ),
    (
        "summrise-dist-rust",
        "the same crate and the same artifact: `index/wrangler.rust.jsonc` is the second name that same \
         build output is deployed under",
    ),
];

/// The declaration list is a debt with owners: it may only shrink, and growing it needs a sentence here.
const MAX_DECLARED: usize = 2;

/// A FLOOR, not a claim: the numbers are the tree's business and may move; what must not happen is this scan
/// reading almost nothing and passing because it looked at almost nothing.
const MIN_CONFIGS: usize = 6;

// THE JSONC READER, THE CONFIG LIST AND THE PATH RESOLVER ARE `common`'s — one owner, because
// `worker_language.rs` asks the same questions of the same files from the other side (a TRACKED entry is
// source; an untracked one is a build artifact that needs a `build.command`). The string-aware comment
// strip lives there with them, and its own test moved with it.

/// (offenders, configs read).
fn offenders(tracked: &[String]) -> (Vec<String>, usize) {
    let set: BTreeSet<&str> = tracked.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    let mut seen = 0;
    for (path, text) in common::worker_configs() {
        let parsed: serde_json::Value =
            match serde_json::from_str(&common::strip_jsonc_comments(&text)) {
                Ok(v) => v,
                Err(e) => {
                    out.push(format!(
                        "{path}: does not parse after comment stripping ({e})"
                    ));
                    continue;
                }
            };
        let Some(main) = parsed.get("main").and_then(|m| m.as_str()) else {
            continue;
        };
        seen += 1;
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if set.contains(common::resolve(dir, main).as_str()) {
            continue;
        }
        let built = parsed
            .get("build")
            .and_then(|b| b.get("command"))
            .and_then(|c| c.as_str())
            .is_some_and(|c| !c.trim().is_empty());
        if built {
            continue;
        }
        let name = parsed
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("(no name)");
        if BUILT_BY_BUILD_SH.iter().any(|(n, _)| *n == name) {
            continue;
        }
        out.push(format!(
            "{path}: name={name} main={main} — the file is NOT tracked and this config has no build.command"
        ));
    }
    (out, seen)
}

#[test]
fn every_config_pointing_at_an_untracked_main_is_built_by_something() {
    let tracked = common::git_ls_files_all();
    let (bad, seen) = offenders(&tracked);

    assert!(
        BUILT_BY_BUILD_SH.len() <= MAX_DECLARED,
        "the declared list grew past its cap: {} > {MAX_DECLARED} — a new entry needs a sentence saying which \
         mechanism builds that config's main, in the same commit",
        BUILT_BY_BUILD_SH.len()
    );
    assert!(
        seen >= MIN_CONFIGS,
        "this scan read only {seen} config(s) with a `main` (floor {MIN_CONFIGS}) — a gate that looked at \
         almost nothing must not pass"
    );
    assert!(
        bad.is_empty(),
        "a worker config points at a build artifact that nothing builds:\n  {}\n\
         FIX: give the config `\"build\": {{ \"command\": \"worker-build --release [crate-path]\" }}` — the shape \
         `gateway/wasm/wrangler.jsonc` and both satellite configs carry — or declare its name in \
         BUILT_BY_BUILD_SH with the mechanism that builds it instead.",
        bad.join("\n  ")
    );
    println!(
        "worker-build-commands: {seen} config(s) with a main, {} declared as built by build.sh, 0 offenders",
        BUILT_BY_BUILD_SH.len()
    );
}

/// The scanner's own trap, pinned: a `//` inside a string is NOT a comment, and a `//` outside one IS.
#[test]
fn the_comment_stripper_keeps_a_string_and_drops_a_comment() {
    let kept = common::strip_jsonc_comments(r#"{ "main": "worker//build/index.js" }"#);
    assert!(
        kept.contains("worker//build/index.js"),
        "a `//` inside a string was eaten: {kept}"
    );
    let dropped =
        common::strip_jsonc_comments("{ // https://example.test/x\n  \"main\": \"a.js\" }");
    assert!(
        !dropped.contains("example.test"),
        "a comment survived the strip: {dropped}"
    );
    assert!(
        dropped.contains("\"main\""),
        "the strip ate the code after the comment: {dropped}"
    );
    // An ESCAPED quote does not end the string, so the `//` after it is still inside one.
    let escaped = common::strip_jsonc_comments(r#"{ "note": "say \"//\" here", "main": "a.js" }"#);
    assert!(
        escaped.contains("\"main\": \"a.js\""),
        "an escaped quote ended the string early: {escaped}"
    );
}
