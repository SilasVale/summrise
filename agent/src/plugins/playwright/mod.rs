//! Playwright Plugin — browser automation MCP service management.
//!
//! THIS PLUGIN EXPOSES TWO MCP TOOLS, and this header said the opposite for long
//! enough that a reader would have believed it: `tools()` below returns
//! `browser_pw_info` and `browser_run_script` (round-151), and the gateway
//! registers both (`mcp-tools.ts:493,505`). The comment three lines inside this
//! same file says so — the header was the only place still denying it.
//!
//! What IS managed over the admin HTTP surface is the SERVICE
//! (/api/plugins/playwright/start|stop, round-admin-ui), and the browser is
//! otherwise reached through the mcp_client plugin (mcp_client_connect at
//! 127.0.0.1:9229/mcp). That part of the original note was right.
//!
//! The `manager` field is WRITE-ONLY: constructed in `new()` and read nowhere in
//! the crate (`tools::build()` takes no argument), so it is not the seam the old
//! wording implied.

pub mod manager;
pub mod tools;

use summrise_agent_core::{Plugin, ToolDef};

/// Plugin struct — thin facade over the shared `Arc<PlaywrightManager>`
/// (the same Arc lives in AppState, so /api/plugins/status and the HTTP
/// routes see one state machine).
pub struct PlaywrightPlugin {
    pub manager: std::sync::Arc<manager::PlaywrightManager>,
}

impl PlaywrightPlugin {
    pub fn new(manager: std::sync::Arc<manager::PlaywrightManager>) -> Self {
        Self { manager }
    }
}

impl Plugin for PlaywrightPlugin {
    fn name(&self) -> &'static str {
        "playwright"
    }
    fn display_name(&self) -> &'static str {
        "Playwright"
    }
    fn description(&self) -> &'static str {
        "playwright-mcp browser automation"
    }
    fn tools(&self) -> Vec<ToolDef> {
        // round-151: browser_pw_info / browser_run_script — bundled-playwright
        // discovery & execution entry point, so the AI doesn't install it
        // itself. Other browser automation still goes through the mcp_client
        // plugin (127.0.0.1:9229 playwright-mcp).
        tools::build()
    }
}
