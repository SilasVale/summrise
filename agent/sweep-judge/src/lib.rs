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

pub mod contrast;
pub mod hover;
pub mod js;
pub mod marks;
pub mod out;
pub mod paths;
pub mod report;
pub mod summary;
pub mod tools;
