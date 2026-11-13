//! The capability implementations and the tool table the device owns.
//!
//! Whether a tool is destructive — the fact `POST /api/authorize`'s decision needs — is
//! `summrise_core::registry::is_destructive`: one table in `core`, declared by the capabilities
//! themselves (design §4.5). This module no longer keeps a second copy. A name the registry does not
//! know is not a tool the device offers, so the handler denies it instead of asking `core` a question
//! about a capability that does not exist.

pub mod browser;
pub mod files;
pub mod forward;
pub mod memory;
pub mod monitor;
pub(crate) mod owned;
pub mod plugins;
pub mod runs;
pub mod ssh;
pub mod system;
pub mod terminal;

/// The `destructive` question, answered by the registry and not by this crate.
///
/// Kept as a name the HTTP handler already calls; the value is `core`'s.
pub use summrise_core::registry::is_destructive as destructive;

/// Whether a migrated capability declares `tool`.
pub fn owns(tool: &str) -> bool {
    files::owns(tool)
}

/// Every tool a migrated capability declares, for the registry-alignment test.
#[cfg(test)]
pub fn names() -> impl Iterator<Item = &'static str> {
    files::names()
}

/// Run a migrated capability's tool; the roots are the device's resolved paths.
pub fn call(
    tool: &str,
    arguments: serde_json::Map<String, serde_json::Value>,
    workspace: &std::path::Path,
    shared: Option<&std::path::Path>,
) -> Result<String, crate::mcp::ToolError> {
    files::call(tool, arguments, workspace, shared)
}
