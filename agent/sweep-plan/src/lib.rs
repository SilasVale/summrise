//! The design sweeps' PLAN, in Rust.
//!
//! Every sweep tool (`panel-design-sweep.mjs`, `console-design-sweep.mjs`, `landing-design-sweep.mjs`)
//! used to decide, in the payload it injects into a page, WHICH SURFACES to visit and WHICH PASSES to
//! run on each. That is a decision, not a platform call, so it is Rust now (`docs/superpowers/specs/
//! 2026-10-08-every-decision-is-rust-design.md` §2). What stays JavaScript is the driving: navigate,
//! evaluate, sample — a BOUNDARY that decides nothing.
//!
//! Measured before porting: the plan is **static given `--passes`**. `wants()` and the matrices in the
//! payloads read only the pass list and constants, so the plan can be computed at EMIT time and
//! embedded in the emitted bundle rather than recomputed in the browser.
//!
//! The oracle is `parity/compare.mjs`: it executes the PRE-CHANGE payload under a stub browser, takes
//! the ordered trace of what that payload actually asked the browser to do, and compares it case by
//! case with this crate's plan over the same inputs.

pub mod plan;
pub mod tools;

pub use plan::{Plan, Surface, Tool, Viewport};

/// The plan for one tool and one `--passes` spec.
///
/// `passes` is `None` when the flag was absent, which is NOT the same input as `--passes=` — the panel
/// defaults the absent one to `"all"` while the console and the landing default it to the empty list,
/// and those two defaults disagree about an empty list. The distinction is preserved rather than
/// smoothed over, because it is observable today.
pub fn plan_for(tool: Tool, passes: Option<&str>) -> Plan {
    tools::build(tool, passes)
}
