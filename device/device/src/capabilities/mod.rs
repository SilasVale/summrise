//! The capability implementations and the tool table the device owns.
//!
//! Whether a tool is destructive — the fact `POST /api/authorize`'s decision needs — is
//! `summrise_core::registry::is_destructive`: one table in `core`, declared by the capabilities
//! themselves (design §4.5). This module no longer keeps a second copy. A name the registry does not
//! know is not a tool the device offers, so the handler denies it instead of asking `core` a question
//! about a capability that does not exist.

pub mod browser;
pub mod files;
pub mod memory;
pub mod monitor;
pub mod runs;
pub mod ssh;
pub mod system;
pub mod terminal;

/// The `destructive` question, answered by the registry and not by this crate.
///
/// Kept as a name the HTTP handler already calls; the value is `core`'s.
pub use summrise_core::registry::is_destructive as destructive;
