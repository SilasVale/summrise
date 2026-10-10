//! THE EMITTER — how the plan reaches a device that has no toolchain.
//!
//! **THE SAME SHAPE THE SWEEPS USE, FOR THE SAME REASON.** `sweep-plan.mjs` runs the Rust at EMIT time and the
//! emitted payload carries the plan as data, because the box that renders a page has a browser and no cargo.
//! The box that runs this suite has a device and no cargo either, so `--bundle` writes the driver's own source
//! with the plan already resolved and embedded — and the device runs one file, exactly as it does today.
//!
//! **IT IS A ONE-LINE SUBSTITUTION AND IT REFUSES TO GUESS.** The source carries the marker line with `null`;
//! the emitter requires it EXACTLY ONCE and fails loudly otherwise. String surgery on a source file is only
//! safe when the thing being replaced is unique and the failure is loud — a bundle built from a source whose
//! marker had moved would silently ship a driver with no plan, and the driver refuses to run without one, so
//! the failure would arrive on the device rather than at the emit.
//!
//! **AND IT IS A GATE, NOT A COMMAND SOMEBODY REMEMBERS.** `index/public/summrise-agent/e2e.js` is the copy a
//! device fetches, and it is checked against this function by `tests/bundle.rs`: the emitted bytes must equal
//! the committed ones. A fix that lands in the driver and not in the copy is the failure that gate exists for —
//! the README used to ask a person to remember the `cp`.

use crate::plan::Plan;

/// The line the SOURCE carries. `null` is deliberate: run the source without `--plan` and it refuses rather
/// than inventing a section order.
pub const SOURCE_LINE: &str = "const BUNDLED_PLAN = null; // @e2e-plan";

/// What the emitted line looks like. It names its producer, because the file it lands in is GENERATED — a
/// reader who wants to change the plan is holding the wrong end of the repository.
pub const EMITTED_PREFIX: &str = "const BUNDLED_PLAN = ";
pub const EMITTED_SUFFIX: &str =
    "; // @e2e-plan — written by `summrise-e2e-judge --bundle`; the plan is Rust (agent/e2e-judge/src/plan.rs)";

/// The driver source, with the plan embedded. `source` is the text of `agent/scripts/e2e/e2e.js`.
pub fn bundle(source: &str, plan: &Plan) -> Result<String, String> {
    let hits = source.matches(SOURCE_LINE).count();
    if hits != 1 {
        return Err(format!(
            "the driver source must carry the plan marker line EXACTLY once, and it carries it {hits} time(s):\n  \
             {SOURCE_LINE}\n\
             FIX: restore that line in the driver, or update SOURCE_LINE in agent/e2e-judge/src/bundle.rs to \
             match it. An emitter that substituted into a file it did not recognise would ship a driver with \
             no plan, and the device would find out instead of the build."
        ));
    }
    let json =
        serde_json::to_string(plan).map_err(|e| format!("the plan did not serialize: {e}"))?;
    Ok(source.replace(
        SOURCE_LINE,
        &format!("{EMITTED_PREFIX}{json}{EMITTED_SUFFIX}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::plan_from_cli;

    #[test]
    fn the_plan_is_embedded_as_data_and_the_marker_survives_once() {
        let src = format!("// header\n{SOURCE_LINE}\n// tail\n");
        let plan = plan_from_cli(Some("runs"), false).unwrap();
        let out = bundle(&src, &plan).unwrap();
        assert!(out.contains("\"runs\""), "{out}");
        assert_eq!(out.matches("@e2e-plan").count(), 1);
        assert!(out.starts_with("// header\n"));
        assert!(out.ends_with("// tail\n"));
    }

    #[test]
    fn a_source_whose_marker_moved_is_refused_rather_than_bundled_blind() {
        let e = bundle("// no marker here\n", &Plan::default()).unwrap_err();
        assert!(e.contains("EXACTLY once"), "{e}");
        let two = format!("{SOURCE_LINE}\n{SOURCE_LINE}\n");
        let e = bundle(&two, &Plan::default()).unwrap_err();
        assert!(e.contains("2 time(s)"), "{e}");
    }
}
