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

/// The context a migrated capability's tools need. A capability takes exactly what it reads, so a
/// module cannot reach for the device's config, its audit or another capability.
pub struct Ctx<'a> {
    /// The `workspace` root `files` is sandboxed to.
    pub files_root: &'a std::path::Path,
    /// The optional `shared` root.
    pub files_shared_root: Option<&'a std::path::Path>,
    /// This machine's memory store.
    pub memory: &'a memory::Memory,
    /// The live sessions, one registry per device.
    pub terminal: &'a terminal::PtyRegistry,
    /// The one headless browser this machine starts lazily.
    pub browser: &'a browser::BrowserRegistry,
    /// The reachability watches.
    pub monitor: &'a std::sync::Arc<monitor::MonitorRegistry>,
    /// The run history.
    pub runs: &'a std::sync::Arc<runs::Runs>,
    /// The forward rules.
    pub forwards: &'a std::sync::Arc<forward::ForwardRegistry>,
}

/// Whether a migrated capability declares `tool`.
pub fn owns(tool: &str) -> bool {
    files::owns(tool)
        || memory::owns(tool)
        || terminal::owns(tool)
        || browser::owns(tool)
        || monitor::owns(tool)
        || runs::owns(tool)
        || system::owns(tool)
        || forward::owns(tool)
}

/// Every tool a migrated capability declares, for the registry-alignment test.
#[cfg(test)]
pub fn names() -> impl Iterator<Item = &'static str> {
    files::names()
        .chain(memory::names())
        .chain(terminal::names())
        .chain(browser::names())
        .chain(monitor::names())
        .chain(runs::names())
        .chain(system::names())
        .chain(forward::names())
}

/// Run a migrated capability's tool.
pub async fn call(
    tool: &str,
    arguments: serde_json::Map<String, serde_json::Value>,
    ctx: &Ctx<'_>,
) -> Result<String, crate::mcp::ToolError> {
    if files::owns(tool) {
        return files::call(tool, arguments, ctx);
    }
    if memory::owns(tool) {
        return memory::call(ctx.memory, tool, arguments);
    }
    if terminal::owns(tool) {
        return terminal::call(ctx.terminal, tool, arguments).await;
    }
    if browser::owns(tool) {
        return browser::call(ctx.browser, tool, arguments).await;
    }
    if monitor::owns(tool) {
        return monitor::call(ctx.monitor, tool, arguments).await;
    }
    if runs::owns(tool) {
        return runs::call(ctx.runs, tool, arguments).await;
    }
    if system::owns(tool) {
        return system::call(tool, arguments).await;
    }
    if forward::owns(tool) {
        return forward::call(ctx.forwards, tool, arguments);
    }
    Err(crate::mcp::ToolError::Internal(format!(
        "{tool}: no migrated capability declares it"
    )))
}
