//! `contrast-probe-check` — THE EMITTED PROBE IS THE ARTIFACT THE SWEEPS INJECT, AND IT MUST BE THE CODE
//! THE BROWSER RUNS.
//!
//! WHAT THIS GATE IS FOR, in the `.mjs`'s own header: four rounds of contrast sweeps used an ad-hoc snippet
//! retyped each time, and it was wrong twice. Both defects are assertions here, with the numbers they cost:
//!
//!   1. **TRANSLUCENT BACKGROUNDS.** Reading `rgba(255,255,255,0.07)` as if it were white reported 2.51 for
//!      text that actually sits on a composited `rgb(44,45,49)` and measures 5.49 — twenty of fifty findings
//!      were this.
//!   2. **A SKIP RULE THAT DISABLED THE SWEEP.** Skipping anything with a `background-image` ancestor
//!      skipped EVERY node in the panel and reported `checked=0, underAA=0`, which reads exactly like a pass.
//!
//! THE PROBE'S RULES ARE `agent/tests/contrast_probe.rs` — the pure functions and the gate's own cases for
//! them, numbers included. WHAT IS HERE IS THE FIVE ASSERTIONS ABOUT THE ARTIFACT: `PROBE_SOURCE` is the
//! JavaScript the DESIGN SWEEPS INJECT INTO A BROWSER, and the plan's carve-out list keeps that JavaScript,
//! because `browser_run_script` takes a JS file and the measurement runs in the DOM. So these five are
//! about a JS artifact rather than about a rule.
//!
//! ── MIGRATED FROM `scripts/test/contrast-probe-check.mjs`, WHICH IS DELETED ─────────────────────────
//! The `.mjs` was the LAST of the six `scripts/test/*.mjs` gates, and it is the one whose subject stays
//! JavaScript on purpose. What moved is the gate: the five assertions, the messages and the ok line. What
//! did NOT move, and cannot, is the two questions only the JS engine can answer — does the module PARSE
//! (a stray backtick in the template literal ends it early), and does the EMITTED text COMPILE. Those are
//! asked of Node in one process, and every expected string is compared here.
//!
//! WHY THE FUNCTIONS ARE THE BROWSER'S AND NOT A COPY: `PROBE_SOURCE` embeds them with
//! `Function.prototype.toString()`, so the artifact carries the module's own source text. THAT is the
//! property these five protect, and the assertions below are what says so when the two drift.
//!
//! ── THE MUTATION THAT MUST FAIL THIS GATE ──────────────────────────────────────────────────────────
//! Read this when you change this file: the mutation is how you find out whether the gate can still fail
//! at all. A gate that cannot be broken is worse than no gate.
//!
//! MUTATION: put a backtick in one of the probe's embedded comments, or let a `\\s` in a regex collapse to
//!           `s`.
//! RESULT:   the first is refused by the IMPORT — a backtick inside the template literal ends it, the module
//!           stops parsing, and this gate says so with Node's SyntaxError rather than with a string test.
//!           The second is the one that needs a gate: a collapsed `\\s` still parses, it just matches
//!           something else. exit 1 — "a collapsed \\s reached the emitted source".

mod common;

use common::{repo, Spawn};

/// The five assertions, in the order the `.mjs` made them, and its own count for the ok line.
const ASSERTIONS: usize = 5;

/// One Node process: import the module, serialize the artifact and the functions it must embed, and try both
/// compiles. The `.mjs` did the same work by importing the module and calling `new Function` — the difference
/// is that the answers come back as JSON and the comparisons happen in Rust.
const DRIVER: &str = r#"
import * as m from "./agent/scripts/lib/contrast-probe.mjs";
const out = {
  source: m.PROBE_SOURCE,
  fns: {
    compositeStack: m.compositeStack.toString(),
    contrastRatio: m.contrastRatio.toString(),
    aaThreshold: m.aaThreshold.toString(),
    parseColour: m.parseColour.toString(),
    svgRootPaints: m.svgRootPaints.toString(),
  },
  returnedError: null,
  emittedError: null,
};
try { new Function(`return ${m.PROBE_SOURCE}`); } catch (e) { out.returnedError = String((e && e.message) || e); }
try { new Function(m.PROBE_SOURCE); } catch (e) { out.emittedError = String((e && e.message) || e); }
process.stdout.write(JSON.stringify(out));
"#;

#[test]
fn the_emitted_probe_is_the_code_the_browser_runs() {
    let out = Spawn::new("node")
        .args(["--input-type=module", "-e", DRIVER])
        .cwd(repo())
        .run();
    // THE IMPORT IS THE PARSE CHECK THE `.mjs` RELIED ON, and its own comment says so: "this file IMPORTS
    // the module, so a stray backtick inside the template makes the import throw a SyntaxError and the whole
    // test file fails loudly. I watched it do exactly that twice. A hand-rolled parser for a case the import
    // already catches is a check that can only be wrong."
    assert!(
        out.ok(),
        "contrast-probe: the probe module did not LOAD (rc={}) — a stray backtick inside the template \
         literal ends it early, which is exactly what happened while adding the gradient guard:\n{}",
        out.code(),
        out.both()
    );
    let json: serde_json::Value = serde_json::from_str(out.stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "contrast-probe: the module did not hand back the artifact as JSON ({e}): {}",
            out.stdout
        )
    });
    let source = json["source"]
        .as_str()
        .expect("PROBE_SOURCE must be a string — the sweeps inject it as one");
    let fns = &json["fns"];

    let mut n = 0usize;

    // 1. SYNTACTICALLY VALID. It is a template literal, so a stray BACKTICK inside an embedded comment
    //    terminates it — compiles it here, and says so in words as well, because a `return`-prefixed compile
    //    and a bare one fail differently and the string test names the cause either way.
    n += 1;
    assert_eq!(
        json["returnedError"],
        serde_json::Value::Null,
        "the probe must compile: `new Function('return ' + PROBE_SOURCE)` threw — {}",
        json["returnedError"]
    );
    assert!(
        !source.contains('`'),
        "the probe is a template literal: no backticks inside it"
    );

    // 2. THE REAL FUNCTIONS, NOT A PARAPHRASE. If the module's functions are edited without the probe
    //    following, the browser and the test stop being the same code — which is the failure this whole
    //    module exists to prevent.
    n += 1;
    for (what, key) in [
        ("compositeStack", "compositeStack"),
        ("contrastRatio", "contrastRatio"),
        ("aaThreshold", "aaThreshold"),
        ("parseColour", "parseColour"),
        ("svgRootPaints", "svgRootPaints"),
    ] {
        let text = fns[key]
            .as_str()
            .unwrap_or_else(|| panic!("{what} is not exported by the probe module"));
        assert!(source.contains(text), "{what} not embedded");
    }

    // 3. THE PROBE MEASURES THE SVG ROOT AND SKIPS ONLY ITS CHILDREN.
    n += 1;
    assert!(
        source.contains("el instanceof SVGElement && el.tagName.toLowerCase() !== 'svg'"),
        "the loop must let the svg ROOT through: it is the element that knows the icon's colour \
         (fill: none, stroke: currentColor)"
    );
    assert!(
        !source.contains("el instanceof SVGElement || el.closest('svg')"),
        "a blanket SVG exclusion is back — it takes the root with it and leaves painterOf's fill/stroke \
         branches unreachable"
    );

    // 4. THE EMITTED SOURCE MUST COMPILE AS THE ARTIFACT (round 124). The module can import cleanly while the
    //    string it hands the browser is broken: PROBE_SOURCE is a template literal, so a single backslash in
    //    a regex collapses on the way out. Round 123 shipped exactly that and only found it when the device
    //    refused to evaluate the probe. Importing the module proves nothing about what the browser runs.
    n += 1;
    assert_eq!(
        json["emittedError"],
        serde_json::Value::Null,
        "the EMITTED probe does not compile — the artifact, not the module — {}",
        json["emittedError"]
    );

    // 5. ...AND IT KEEPS ITS REGEX ESCAPES (a collapsed one changes the match, not the parse). A doubled
    //    backslash in the module becomes a single one in the emitted string; if the doubling is forgotten the
    //    regex STILL PARSES — it just matches something else. The selector builder splits a class list on
    //    whitespace, so this checks the content of the artifact, not that it compiles.
    n += 1;
    assert!(
        source.contains(r"split(/\s+/)"),
        "the class-list split lost its \\s escape"
    );
    assert!(
        !source.contains("split(/s+/)"),
        "a collapsed \\s reached the emitted source"
    );

    assert_eq!(
        n, ASSERTIONS,
        "the assertion count is this gate's own report"
    );
    println!(
        "contrast-probe (the emitted probe): ok — {n} assertion(s) about the JS artifact the sweeps \
         inject; the probe's rules are cargo test --test contrast_probe"
    );
}
