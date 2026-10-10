//! `summrise-design-sweep-driver` — the design sweeps' DRIVER, in Rust, and the line it stops at.
//!
//! **WHAT MOVED, AND WHAT DID NOT, BECAUSE THE LINE IS THE WHOLE DESIGN.** The parent spec MEASURED this
//! question before the plan was written (`docs/superpowers/specs/2026-10-08-every-decision-is-rust-design.md`
//! §4.1): *"the repository has NO Rust browser driver ... a grep for `webSocketDebuggerUrl`, `json/version`
//! and `Input.dispatch` across every crate returns nothing, and no CDP crate is in the lockfile. So 'the
//! Playwright driving becomes Rust' cannot be a port — it would be a NEW browser dependency."* The rule that
//! decides it is the one already in force: launching a browser, dispatching a real mouse press and reading
//! `getComputedStyle` are platform calls; deciding which rung runs, in what order, spending which numbers,
//! and what a control that never changes while pressed MEANS, are decisions.
//!
//! So this crate owns:
//!
//!   * **the STEP LADDER** (`passes.rs`) — the plan's rungs and budgets as a step list, data rather than
//!     generated code, which a fixed JavaScript runtime interprets;
//!   * **the PROBE REGISTRY** (`probes.rs`) — which page-side programs a sweep embeds, read from the
//!     JavaScript at emit time and embedded unchanged, with the assembled program PARSED before it is
//!     returned;
//!   * **the ROWS DOCUMENT** (`rows.rs`) — what a run asked and what came back, refused by field name when a
//!     field is missing.
//!
//! And it does NOT own the passes themselves: `idlePass`, `pressPass`, `ackPass`, `revealPass`, `focusPass`,
//! `motionPass` and `discoverPressTargets` keep their `page.mouse`, their `page.evaluate` and their
//! `page.emulateMedia`, in `agent/scripts/lib/design-sweep.mjs`, where a browser can run them.
//!
//! ```text
//! summrise-design-sweep-driver --emit [--tool panel] [--only a,b] [--surfaces x,y] > driver.js
//! summrise-design-sweep-driver --rows <rows.json>
//! ```
//!
//! **THE EXIT CODE SAYS WHETHER IT COLLECTED, NEVER WHETHER THE PAGE PASSED** — the e2e driver's contract
//! verbatim: `0` the program was assembled and parses (or the rows document reads), `2` a usage error, `1`
//! it could not collect. The verdict is `summrise-sweep-judge`'s, from the rows.

pub mod passes;
pub mod probes;
pub mod rows;

pub use passes::{steps, Step};
pub use probes::{assemble, read_probes, Probe, PROBES, PROBE_MODULE};
pub use rows::{parse, Observation, Rows, SCHEMA};
