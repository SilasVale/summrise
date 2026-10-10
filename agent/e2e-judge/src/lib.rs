//! **THE DEVICE E2E SUITE'S PLAN AND JUDGE, IN RUST — AND THE DRIVER THAT STAYS JAVASCRIPT.**
//!
//! `agent/scripts/e2e/e2e.js` drives a real Summrise agent on a real device through the product's own HTTP
//! surface: a PTY session, a 300 KB two-page file transfer, the memory tools, the desktop SPA's xterm over
//! CDP 9333, playwright-mcp on two transports, the Evidence drawer's data source, the approval gate, and run
//! identity. It carried **75 `check(name, cond, detail)` calls** and the section order they ran in.
//!
//! **THE SPLIT IS THE ONE THE DESIGN SWEEPS ALREADY MADE.** Their in-page COLLECTOR stayed JavaScript — it
//! performs the platform calls and returns rows — while the JUDGE and the PLAN became Rust
//! (`agent/sweep-judge/`, `agent/sweep-plan/`), because a decision is Rust wherever it sits. The same line
//! falls here:
//!
//! * **the driver is the platform half** — `fetch`, a CDP WebSocket, the `browser_run_script` payload strings,
//!   `require('fs')`, and the polling loops that wait for an answer to arrive. It writes a ROWS document and
//!   decides nothing: no condition, no detail string, no count, no exit code for a verdict.
//! * **the plan is this crate's** — which sections run, in what order, what `--only` may name, and the policy
//!   caps and floors (poll counts, retry counts, the waits between them). It is DATA, and the driver is its
//!   interpreter.
//! * **the judge is this crate's** — the 75 checks, their conditions, the sentences a reader triages a failure
//!   from, the two skip arms, the three numbers and the exit code.
//!
//! **WHY THE SEQUENCING IS A DECISION AND NOT A CONSEQUENCE.** `--only` was validated against a list the driver
//! held, and a name that selected nothing was indistinguishable from success — which is why the guard exists at
//! all. That is a judgement about configuration, and it belongs with the checks, not beside the calls they read.
//! The order is a decision for the same reason: `terminal` is first because everything else needs a PTY to
//! exist, and `governance` and `runs` are last because they are the only two that arm a gate or mint a run id.
//!
//! **THE THREE THINGS A READER SHOULD BE ABLE TO CHECK WITHOUT RUNNING IT:**
//!
//! * `--plan` prints what will run; `--bundle` writes the copy a device fetches, with that plan embedded;
//!   `--rows` judges a recording and exits 0 / 1 / 2.
//! * every check is a pure function over the rows, so all of them are unit-tested against synthetic evidence
//!   in `tests/checks.rs` — no device, no browser and no agent.
//! * the recorded corpus from a real device run (`e2e-rec.json`, 906 KB, recorded 2026-10-09) is the oracle
//!   that proves the port reproduces the JS verdicts, and it is NOT wired yet: it is stranded on a device that
//!   cannot reach the relay, and it is the first thing this landing names as its follow-up. Until it lands, the
//!   verdicts of the seven sections no CI job can run are verified by their own synthetic rows and by nothing
//!   else — which is written down here rather than left to be assumed.

pub mod bundle;
pub mod checks;
pub mod plan;
pub mod rows;
pub mod text;
pub mod verdict;

use rows::{Rows, View};
use verdict::Verdict;

/// The whole judge: a pure function from a recorded run to a verdict.
pub fn judge(doc: &Rows) -> Verdict {
    let mut out = Verdict::default();
    let view = View::new(doc);
    checks::judge(&view, &mut out);
    out
}
