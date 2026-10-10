//! THE PROBES, THE LADDER, AND THE TWO THINGS THAT MUST FAIL LOUDLY.
//!
//! **REVIEW FOCUS 1 IS THE REASON THIS FILE EXISTS.** A page-side payload is JavaScript inside a Rust
//! template, and a probe that stops parsing returns ZERO ROWS — which, on a device, reads exactly like a
//! clean sweep, and which no Rust gate can see because the rows come back as data. The plan names the case in
//! one sentence: *"a stray brace must fail loudly at emit, not return zero rows that read as a clean
//! sweep."* So the emit path PARSES what it assembled, and this file proves it does by handing it a
//! deliberately broken probe and requiring the refusal — naming the probe.
//!
//! **REVIEW FOCUS 5 IS THE OTHER END.** A rows document from an older schema must be an error naming the
//! field rather than a default that silently judges nothing. `tests/rows.rs` is that half.
//!
//! **AND THE VERBATIM PROPERTY IS TRUE BY CONSTRUCTION, WHICH IS STRONGER THAN A TEST.** The probe text is
//! not transcribed into Rust at all: `read_probes()` imports `agent/scripts/lib/design-sweep.mjs` in node and
//! asks it for `fn.toString()` of each registered name, which is the same text `piecesModule` embeds for the
//! four JS emitters. There is nothing to drift, so what this file checks instead is the property that IS a
//! decision — that the emitted program carries each probe BYTE FOR BYTE, and that the registry names real
//! exports.
//!
//! MUTATION: delete the `check_parses` call from `probes::assemble` (leave `fn check_parses` unused).
//! RESULT:   `cargo test -p summrise-design-sweep-driver --test passes` → exit 101, MEASURED:
//!
//! ```text
//! a_broken_probe_is_refused_at_emit_naming_the_probe ... FAILED
//!   a truncated probe was assembled and returned as if it were a program — on a device that is zero rows,
//!   which reads as a clean sweep
//! test result: FAILED. 4 passed; 1 failed
//! ```
//!
//! ...and `the_emitted_program_carries_every_probe_byte_for_byte` still passes, which is the point of having
//! both: the verbatim test says the bytes are right, and the parse test says the bytes are a PROGRAM.

use std::collections::BTreeMap;
use summrise_design_sweep_driver::{assemble, read_probes, steps, PROBES};
use summrise_sweep_judge::canonical_plan;

/// The probe sources, read once for the whole file. A failure here is a FAILURE rather than a skip: a probe
/// that could not be read is a probe the program would embed as nothing, and a suite that skipped would
/// report the same green as a suite that checked.
fn probes() -> BTreeMap<String, String> {
    read_probes().unwrap_or_else(|e| panic!("the page-side probes could not be read: {e}"))
}

#[test]
fn the_registry_names_real_exports() {
    // `read_probes` refuses a registered name the module does not export a non-empty string for, so a
    // renamed probe fails here rather than emitting `undefined` for a page to evaluate as nothing.
    let p = probes();
    for probe in PROBES {
        let src = p
            .get(probe.key)
            .unwrap_or_else(|| panic!("`{}` is registered and produced no source", probe.key));
        assert!(!src.is_empty(), "`{}` produced an empty source", probe.key);
        // **THE FOUR THAT TAKE A ROOT SELECTOR COME BACK AS FUNCTION SOURCE, AND ONE OF THEM IS NOT NAMED
        // AFTER ITS EXPORT.** `namesProbe`, `reflowProbe` and `marksProbe` are declared `export function
        // <name>(root)`, so `toString()` starts with `function <name>(`; `surfaceProbe` is the PRODUCT of
        // `withHelpers(...)`, whose returned function is called `probe` — so an assertion that the source
        // NAMES the export is wrong for exactly one of the four, and it failed here first. What is true of
        // all four is the shape: a function, and not the value `undefined` node would have printed as
        // `null`.
        if probe.is_function() {
            assert!(
                src.starts_with("function ") || src.starts_with("async function "),
                "`{}` (`{}`) is not function source: {}",
                probe.key,
                probe.js_name,
                &src[..src.len().min(120)]
            );
        } else {
            assert!(
                src.starts_with("(") || src.starts_with("(() =>"),
                "`{}` (`{}`) is a `*_SOURCE` and should be the text of a page-side program: {}",
                probe.key,
                probe.js_name,
                &src[..src.len().min(120)]
            );
        }
    }
}

/// **THE VERBATIM PROPERTY, ASSERTED RATHER THAN ASSUMED.** Every probe the registry names appears in the
/// assembled program byte for byte.
#[test]
fn the_emitted_program_carries_every_probe_byte_for_byte() {
    let plan = canonical_plan();
    let step_list = steps(&plan, &["panel-light-Terminal".to_string()]).unwrap();
    let code = assemble(
        &serde_json::to_string(&plan).unwrap(),
        &serde_json::to_string(&step_list).unwrap(),
        &probes(),
    )
    .expect("the program assembles");
    for probe in PROBES {
        let src = &probes()[probe.key];
        assert!(
            code.contains(src.as_str()),
            "the assembled program does not carry `{}` unchanged — a page evaluating a different probe than \
             the one this checkout has is a measurement nobody can reproduce",
            probe.key
        );
    }
    // The declarations are named the way `piecesModule` names them, so a payload reads them by the same key.
    for probe in PROBES {
        assert!(
            code.contains(&format!("const {} = ", probe.key)),
            "{}",
            probe.key
        );
    }
}

/// **REVIEW FOCUS 1.** A probe with a stray brace must be refused AT EMIT, and the refusal must name it.
#[test]
fn a_broken_probe_is_refused_at_emit_naming_the_probe() {
    let plan = canonical_plan();
    let step_list = steps(&plan, &["panel-light-Terminal".to_string()]).unwrap();
    let plan_json = serde_json::to_string(&plan).unwrap();
    let steps_json = serde_json::to_string(&step_list).unwrap();
    // Truncate the `REFLOW` probe — which is the shape the plan names, and the shape a bad edit produces:
    // a template literal that lost its tail.
    let mut broken = probes();
    let full = broken["REFLOW"].clone();
    broken.insert("REFLOW".to_string(), full[..full.len() / 2].to_string());
    let e = assemble(&plan_json, &steps_json, &broken).expect_err(
        "a truncated probe was assembled and returned as if it were a program — on a device that is zero \
         rows, which reads as a clean sweep",
    );
    assert!(
        e.contains("REFLOW"),
        "the refusal must NAME the probe a reader has to fix: {e}"
    );
    assert!(e.contains("does not parse"), "{e}");
}

/// A probe the registry names and the map does not carry is refused too — otherwise the program would embed
/// nothing for it and the page would evaluate `undefined`.
#[test]
fn a_missing_probe_is_refused_rather_than_embedded_as_nothing() {
    let plan = canonical_plan();
    let step_list = steps(&plan, &["panel-light-Terminal".to_string()]).unwrap();
    let mut short = probes();
    short.remove("MARKS");
    let e = assemble(
        &serde_json::to_string(&plan).unwrap(),
        &serde_json::to_string(&step_list).unwrap(),
        &short,
    )
    .expect_err("a program missing a probe must not be returned");
    assert!(e.contains("MARKS"), "{e}");
}

/// The emitted program is the plan and the step list as DATA, and it exports the probes under the keys a
/// payload reads.
#[test]
fn the_program_is_the_plan_and_the_steps_as_data() {
    let plan = canonical_plan();
    let step_list = steps(
        &plan,
        &[
            "panel-light-Terminal".to_string(),
            "desktop-dark-History".to_string(),
        ],
    )
    .unwrap();
    let code = assemble(
        &serde_json::to_string(&plan).unwrap(),
        &serde_json::to_string(&step_list).unwrap(),
        &probes(),
    )
    .unwrap();
    assert!(code.contains("const PLAN = {"), "the plan travels as data");
    assert!(
        code.contains("const STEPS = ["),
        "the step list travels as data"
    );
    assert!(
        code.contains("panel-light-Terminal.timing"),
        "the step ids are in the program"
    );
    assert!(
        code.contains("module.exports = { PLAN, STEPS"),
        "the payload reads them by name"
    );
    // Two surfaces, every rung: the step count is a product, not a coincidence.
    assert_eq!(step_list.len(), 2 * plan.sections.len());
}
