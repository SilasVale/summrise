//! # summrise-core — the decisions both hosts share
//!
//! **ONE PURE RUST CORE OWNS EVERY DECISION; THE HOSTS EXECUTE IT AND DECIDE NOTHING.**
//! (`docs/superpowers/plans/2026-10-13-one-core-thin-hosts.md` is the plan this crate is the spine of.)
//!
//! Three rules make a module belong here rather than in a host:
//!
//! 1. **IT IS PURE.** Values in, values out, errors as values. No `web_sys`, no `worker`, no `std::fs`, no socket,
//!    no clock it does not take as an argument. That is what lets ONE implementation run in the Cloudflare worker,
//!    in the Windows agent, and — as emitted text — in the console.
//! 2. **IT IS SHARED, OR IT IS ABOUT TO BE.** A rule only one host needs stays in that host until a second host
//!    needs it; a rule TWO hosts already carry (in either language) is a hole, not a bug, and moves here.
//! 3. **IT CARRIES ITS OWN GATE.** Every rule arrives with the test that pins it and, where it was moved rather than
//!    written, the `MUTATION:`/`RESULT:` pair that shows the test can fail.
//!
//! **WHAT IS NOT HERE**: anything that needs a platform handle, and anything true of only one host (the worker's
//! route table, the agent's plugin registry).

pub mod catalogue;
pub mod guard;
pub mod provider_spec;
pub mod text;
