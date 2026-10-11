//! `summrise-sweep-judge` — the design sweeps' judge, in Rust.
//!
//! It replaces the `--judge` half of `agent/scripts/{panel,console,landing}-design-sweep.mjs`: a PURE
//! FUNCTION over a JSON report that never touches a browser and never touches Playwright. The `--emit` half
//! of each tool stays where it is, because a platform executes it.
//!
//! ```text
//! summrise-sweep-judge --tool <panel|console|landing> [--expect=<csv>] <report.json>
//! ```
//!
//! Exit codes, exactly as the JavaScript's: `0` no findings, `1` findings (or a report that could not be
//! read), `2` a usage error. The findings go to STDERR and the `note:` lines to STDOUT, which is the split
//! `console.error` / `console.log` produced.
//!
//! **THREE MODULES HERE ARE THE SWEEPS' *RULES* RATHER THAN ITS REPORTS** (landing 2, the design sweeps'
//! split): `press` is `pressDelta`, the rule a pressed control is judged by; `loudness` is `loudnessOf`,
//! the rule the graphics probe evaluates in the page; and `waivers` is `DECORATIVE_WAIVERS`, the policy
//! list two instruments read. They are here rather than in the driver because they are JUDGEMENTS over a
//! row — which is this crate's whole criterion — and because the JavaScript twins they were differentially
//! checked against are the ones a browser still runs.

pub mod contrast;
pub mod hover;
pub mod js;
pub mod loudness;
pub mod marks;
pub mod out;
pub mod paths;
pub mod plan;
pub mod press;
pub mod report;
pub mod summary;
pub mod tools;
pub mod waivers;

pub use loudness::{loudness_of, Loudness};
pub use plan::{canonical_plan, only, Section, SweepPlan};
pub use press::{press_delta, KEYS as PRESS_KEYS};
pub use waivers::{decorative, is_waived, waived_at, Decorative};
