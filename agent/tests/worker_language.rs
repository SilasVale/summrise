//! WHICH WORKER CONFIGS MAY NAME A JAVASCRIPT ENTRY POINT — a ratchet, and the goal's own completion
//! criterion written where it can fail.
//!
//! THE CRITERION (the standing objective, 2026-10-08): *no production route outside §四's carve-outs is
//! served by JS or TypeScript, and nothing dead in those languages remains.* §四's carve-outs are browser
//! UIs (`agent/resources/panel-react`, `gateway/ui`), the npm CLI package, the Electron shell, the DSH
//! plugin package, and the harnesses that execute a JS/wasm artifact — none of which is a WORKER, which is
//! why a ratchet over `wrangler*.jsonc` can state the criterion mechanically.
//!
//! **THE RULE IS "A TRACKED `.js`/`.ts` FILE", NOT "AN ENTRY ENDING IN .js"** — and that distinction is the
//! whole gate. Every Rust-backed worker in this repository points `main` at `worker/build/index.js` or
//! `build/index.js`: the glue `worker-build` generates from the crate. It is a `.js` path and it is NOT
//! JavaScript anyone maintains. What makes a worker a JavaScript worker is that its entry is a file the
//! repository TRACKS, and `worker_build_commands.rs` reads the same property from the other side (an
//! untracked `main` must carry a `build.command`; a tracked one is source).
//!
//! **A RATCHET RATHER THAN A CHECKLIST, BECAUSE THE LIST IS THE WORK THAT IS LEFT.** Every config whose
//! entry is a tracked JS/TS source is a production worker still running JavaScript; the ones ALLOWED to be
//! in that state are declared below with the item that moves them, and the list may only SHRINK. A
//! declaration that is no longer needed fails too — otherwise the list becomes a place where finished work
//! hides.
//!
//! WHAT IT FOUND ON ITS FIRST RUN (2026-10-08): **`summrise-relay`, the file-relay worker, is production
//! JavaScript and it is in NO item of either plan.** `relay/src/index.js` served the file relay's
//! `/files/*` route (a zone route on the agent hostname, which this gate does not spell out — the
//! production-hostname ratchet in `production_host.rs` counts every literal)
//! (a zone route) and the gateway reaches its upload leg through the `RELAY` service binding. A2 is
//! `proxies/api-relay` (the VPS relay) and P3's second half was the two satellites — so this worker was
//! invisible to the plan and visible to the criterion.
//!
//! **AND IT IS CUT OVER — 2026-10-08, the same day the finding was made.** `relay/wrangler.jsonc` names
//! `worker/build/index.js` now and the JavaScript it replaced is deleted (1,459 lines of source and suite,
//! plus 329 more of recorders and manifest — 1,788 across twelve files), so the entry that stood here is
//! gone and `MAX_STILL_JS` came
//! down with it. **THE DIRECTION OF THAT MOVE IS THE POINT**: the declaration existed for as long as the
//! cutover took, and the second mutation below is the one that made removing it mechanical rather than
//! remembered — a gate whose list can only shrink is a gate that cannot hide finished work.
//!
//! MUTATION: point a Rust-backed config at a tracked JavaScript file — in `proxies/zen-us-proxy/wrangler.jsonc`,
//!           set `"main": "../../gateway/wasm/run-cases.mjs"`.
//! RESULT:   exit 101 —
//!             a worker still runs JavaScript and nothing names the cutover that moves it:
//!               proxies/zen-us-proxy/wrangler.jsonc: name=zen-us-proxy main=../../gateway/wasm/run-cases.mjs
//!               — a production worker whose entry is a tracked JavaScript source, and its name is not
//!               declared in STILL_JS
//!             FIX: cut it over to its Rust module, or declare its name in STILL_JS with the item that will.
//!
//! MUTATION (the direction that makes the list shrink, and the one that fired for real): cut
//!           `summrise-relay` over — set its `main` to `worker/build/index.js` — and LEAVE its declaration
//!           in place.
//! RESULT:   exit 101 —
//!             these declarations are stale — the worker no longer names a JavaScript source:
//!               summrise-relay
//!             FIX: delete the entry (and lower MAX_STILL_JS by one, in the same commit).
//!
//! **AND THE CAP IS AN UPPER BOUND RATHER THAN A COUNTER**: growing the list past `MAX_STILL_JS` fails, and
//! a stale entry fails, so the two together are what make it shrink. Removing an entry WITHOUT lowering the
//! cap is allowed — the cap only has to be lowered when the list would otherwise grow past it — and the
//! first version of this header claimed otherwise, which is a sentence about a gate that its own run does
//! not support.

mod common;

use std::collections::BTreeSet;

/// **THE DECLARED EXCEPTIONS: production workers whose entry is still JavaScript, each with the item that
/// moves it.** The list may only shrink; a new entry needs a sentence saying which cutover removes it.
const STILL_JS: [(&str, &str); 1] = [(
    "summrise-gate",
    "THE CONSOLE WORKER, AND A6 IS ITS ITEM: `src/index.ts` is the /api/* surface the plan moves to \
     wasm in the second phase. Until that cutover the console is TypeScript by design — \
     `gateway/wrangler.jsonc` carries the same sentence next to the name.",
)];

/// The cap follows the list down and never up: a declaration removed without this number moving fails.
const MAX_STILL_JS: usize = 1;

/// A FLOOR, not a claim: the scan must see the configs that exist today.
const MIN_CONFIGS: usize = 6;

fn is_js_source(main: &str) -> bool {
    let lower = main.to_ascii_lowercase();
    [".js", ".mjs", ".cjs", ".ts", ".tsx"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// (configs whose entry is a tracked JS/TS source, configs read).
fn scan() -> (Vec<(String, String, String)>, usize) {
    let tracked: BTreeSet<String> = common::git_ls_files_all().into_iter().collect();
    let mut js_workers = Vec::new();
    let mut seen = 0;
    for (path, text) in common::worker_configs() {
        let parsed: serde_json::Value =
            match serde_json::from_str(&common::strip_jsonc_comments(&text)) {
                Ok(v) => v,
                Err(e) => {
                    js_workers.push((
                        path,
                        "(unparseable)".to_string(),
                        format!("does not parse after comment stripping: {e}"),
                    ));
                    continue;
                }
            };
        let Some(main) = parsed.get("main").and_then(|m| m.as_str()) else {
            continue;
        };
        seen += 1;
        if !is_js_source(main) {
            continue;
        }
        // **THE ENTRY IS JAVASCRIPT ONLY IF THE REPOSITORY TRACKS IT.** `worker/build/index.js` is
        // `worker-build`'s glue for a Rust crate; `src/index.ts` is source. Same path shape, different
        // meaning, and only the index can tell them apart.
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if !tracked.contains(common::resolve(dir, main).as_str()) {
            continue;
        }
        let name = parsed
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("(no name)")
            .to_string();
        js_workers.push((path, name, main.to_string()));
    }
    (js_workers, seen)
}

#[test]
fn only_declared_workers_still_name_a_javascript_source() {
    let (found, seen) = scan();
    let declared: BTreeSet<&str> = STILL_JS.iter().map(|(n, _)| *n).collect();
    let live: BTreeSet<&str> = found
        .iter()
        .map(|(_, n, _)| n.as_str())
        .filter(|n| declared.contains(n))
        .collect();

    assert!(
        STILL_JS.len() <= MAX_STILL_JS,
        "the declared list is {} entries and its cap is {MAX_STILL_JS} — it may only shrink",
        STILL_JS.len()
    );
    assert!(
        seen >= MIN_CONFIGS,
        "this scan read only {seen} config(s) with a `main` (floor {MIN_CONFIGS}) — a gate that looked at \
         almost nothing must not pass"
    );

    let undeclared: Vec<String> = found
        .iter()
        .filter(|(_, name, _)| !declared.contains(name.as_str()))
        .map(|(path, name, main)| {
            format!(
                "{path}: name={name} main={main} — a production worker whose entry is a tracked JavaScript \
                 source, and its name is not declared in STILL_JS"
            )
        })
        .collect();
    assert!(
        undeclared.is_empty(),
        "a worker still runs JavaScript and nothing names the cutover that moves it:\n  {}\n\
         FIX: cut it over to its Rust module, or declare its name in STILL_JS with the item that will.",
        undeclared.join("\n  ")
    );

    // **THE OTHER DIRECTION, AND IT IS WHAT MAKES THE LIST SHRINK.** A declaration whose worker no longer
    // names a JavaScript source is finished work wearing an exception: it keeps `MAX_STILL_JS` high and
    // hides the cutover that happened. The cap must follow it down IN THE SAME COMMIT.
    let stale: Vec<&str> = STILL_JS
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !live.contains(n))
        .collect();
    assert!(
        stale.is_empty(),
        "these declarations are stale — the worker no longer names a JavaScript source:\n  {}\n\
         FIX: delete the entry (and lower MAX_STILL_JS by one, in the same commit).",
        stale.join("\n  ")
    );

    println!(
        "worker-language: {seen} config(s) with a main, {} still JavaScript ({} declared), 0 undeclared",
        found.len(),
        STILL_JS.len()
    );
}

/// The predicate's own proof, in the shape the gate actually meets it: the same `.js` ending means two
/// different things, and only the tracked-file test separates them.
#[test]
fn a_generated_glue_file_is_not_a_javascript_worker() {
    let tracked: BTreeSet<String> = common::git_ls_files_all().into_iter().collect();
    // The Rust-backed workers' entries: `.js` paths that `worker-build` generates, and NOT tracked.
    // **`relay` JOINED THIS LIST WITH ITS CUTOVER (2026-10-08)** — it stood in the other list below
    // while `src/index.js` was its entry, and moving it here is the premise change this test exists to
    // make visible rather than silent.
    for (dir, main) in [
        ("proxies/zen-us-proxy", "worker/build/index.js"),
        ("proxies/zen-go-proxy", "worker/build/index.js"),
        ("index", "worker/build/index.js"),
        ("gateway/wasm", "build/index.js"),
        ("relay", "worker/build/index.js"),
    ] {
        assert!(
            is_js_source(main),
            "{main} ends in .js — the extension test alone cannot tell"
        );
        assert!(
            !tracked.contains(common::resolve(dir, main).as_str()),
            "{dir}/{main} is TRACKED, so this test's premise moved and the gate would flag it"
        );
    }
    // ...and the one declared worker's entry IS a tracked source. **A LOOP UNTIL 2026-10-08**, when
    // `relay` left it (and `clippy::single_element_loop` refused the one-element version — the lint is
    // what noticed, which is worth keeping in mind the next time this list is expected to shrink).
    let (dir, main) = ("gateway", "src/index.ts");
    assert!(is_js_source(main));
    assert!(
        tracked.contains(common::resolve(dir, main).as_str()),
        "{dir}/{main} should be a tracked source"
    );
}
