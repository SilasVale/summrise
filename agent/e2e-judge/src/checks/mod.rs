//! THE NINE SECTIONS, AND THE ONE THING THAT IS NOT A SECTION.
//!
//! The dispatcher walks the PLAN's order — not a fixed list — so `--only` and `--no-browser` decide what is
//! judged by deciding what was recorded, and a recording is judged against the plan it carries. Each section
//! function stops where its observations stop: the driver could not keep driving, and the suite's early returns
//! used to express exactly that by never registering the check.
//!
//! **`driver.abort` IS THE SUITE'S ONE NON-SECTION CHECK.** The JS driver wrapped every section in a single
//! `try/catch` and pushed `{name:'suite', pass:false, detail}` — so a call that failed ended the run, and the
//! checks registered before it still counted. That behaviour is preserved deliberately rather than improved
//! into nine per-section failures: the recorded corpus is a run of this suite, and a port that changes what a
//! failed run REPORTS changes the oracle it is verified against.

pub mod browser;
pub mod evidence;
pub mod file;
pub mod governance;
pub mod mcp;
pub mod panel;
pub mod runs;
pub mod terminal;
pub mod workflow;

use crate::plan::Section;
use crate::rows::View;
use crate::verdict::{Check, Verdict};

/// Judge every section the run's own plan selected, in the plan's order, and then the run's abort if it had
/// one.
///
/// **AND A SECTION THAT RECORDED SOMETHING THE JUDGE COULD NOT READ IS A FAILURE, NOT A SILENCE.** This is the
/// one failure mode a two-language split invites: rename an observation id in the driver and every check that
/// depended on it stops being registered — the run reports `N/N passed` with a smaller N, and nothing says so.
/// The guard is narrow on purpose: it fires only when the document CONTAINS observations under that section's
/// prefix and the section still produced no checks, which cannot happen unless the two halves disagree about a
/// name. A section the run never reached has no observations, and `suite` already names why.
pub fn judge(rows: &View, out: &mut Verdict) {
    for section in rows.plan().sections.clone() {
        let before = out.checks.len();
        match section {
            Section::Terminal => terminal::judge(rows, out),
            Section::File => file::judge(rows, out),
            Section::Workflow => workflow::judge(rows, out),
            Section::Panel => panel::judge(rows, out),
            Section::Mcp => mcp::judge(rows, out),
            Section::Evidence => evidence::judge(rows, out),
            Section::Browser => browser::judge(rows, out),
            Section::Governance => governance::judge(rows, out),
            Section::Runs => runs::judge(rows, out),
        }
        if out.checks.len() == before {
            let prefix = section.prefix();
            let recorded = rows
                .doc
                .observations
                .iter()
                .filter(|o| o.id.starts_with(prefix))
                .count();
            if recorded > 0 {
                out.push(Check::fail(
                    format!("{}: the judge produced no checks", section.name()),
                    format!(
                        "the run recorded {recorded} observation(s) under `{prefix}` and the judge read none of \
                         them: the driver and the judge disagree about an observation id, so every check in \
                         this section silently stopped existing. FIX: compare the ids in \
                         agent/scripts/e2e/e2e.js against agent/e2e-judge/src/checks/."
                    ),
                ));
            }
        }
    }

    if let Some(message) = rows.got("driver.abort").failure() {
        out.push(Check::fail("suite", message));
    }
}
