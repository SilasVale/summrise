//! The three tools' surface builders. One module per sweep, because a plan is a transcription of that
//! sweep's own loop structure and the three do not share one.

pub mod console;
pub mod landing;
pub mod panel;

use crate::plan::{PassSpec, Plan, Tool};

/// Build the plan for a tool and a `--passes` spec.
///
/// THE SPEC KEEPS ITS SHAPE, and that is deliberate: the panel's emitter hands the payload the RAW
/// STRING (`report.passes` is that string, and the judge refuses a report that does not say what it
/// covered), while the console and the landing hand over a LIST. The two also disagree about what an
/// empty spec means. Smoothing that over here would change what the sweeps do, and this landing moves
/// the decision rather than changing it.
pub fn build(tool: Tool, passes: Option<&str>) -> Plan {
    match tool {
        Tool::Panel => panel::build(PassSpec::Raw(passes.unwrap_or("all").to_string())),
        Tool::Console => console::build(PassSpec::List(split(passes))),
        Tool::Landing => landing::build(PassSpec::List(split(passes))),
    }
}

/// The console's and the landing's own split: `--passes=a,b` → `["a","b"]`, empties dropped, NO
/// trimming (the panel trims; these do not, and `--passes=a, b` therefore wants only `a`).
fn split(passes: Option<&str>) -> Vec<String> {
    passes
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_and_empty_are_different_inputs_for_the_panel() {
        // The panel's emitter defaults an ABSENT flag to `all` and passes an EMPTY one through as "",
        // which wants nothing. Both are reachable from a caller.
        assert!(build(Tool::Panel, None).wants("pages"));
        assert!(!build(Tool::Panel, Some("")).wants("pages"));
    }

    #[test]
    fn an_empty_spec_wants_everything_on_the_console() {
        // `!PASSES.length` is the console's own first clause. The panel's is not.
        for spec in [None, Some("")] {
            let plan = build(Tool::Console, spec);
            assert!(plan.wants("reflow"));
            assert!(plan.wants("targets"));
        }
    }

    #[test]
    fn only_the_panel_trims_the_entries() {
        assert!(build(Tool::Panel, Some("pages, hover")).wants("hover"));
        assert!(!build(Tool::Console, Some("contrast, targets")).wants("targets"));
        // ... and the untrimmed entry is simply not a pass name, so nothing is wanted by it.
        assert!(build(Tool::Console, Some("contrast")).wants("contrast"));
    }

    #[test]
    fn all_wants_everything_on_every_tool() {
        for tool in [Tool::Panel, Tool::Console, Tool::Landing] {
            let plan = build(tool, Some("all"));
            for name in tool.vocabulary() {
                assert!(plan.wants(name), "{} did not want {name}", tool.name());
            }
        }
    }
}
