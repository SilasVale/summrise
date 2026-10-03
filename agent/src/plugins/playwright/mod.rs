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

/// **ONE FIXED PORT, SO THE TESTS THAT TOUCH IT MUST NOT RUN AT THE SAME TIME.**
///
/// `manager.rs`'s `status_tracks_fresh_external_and_released` BINDS 9229 to exercise the "an instance we
/// did not spawn" branch, and every status assertion in this crate PROBES it — so a suite that ran them in
/// parallel made them flake each other. The test's own comment has said so since round-382 ("parallel
/// tests holding it would flake each other"); what it ALSO assumed was that 9229 starts free, and that
/// stopped being true on CI (measured 2026-10-02: `cargo test -p summrise-agent` failed as the first
/// command of the agent job's Tests step with exit 101, twice in a row, on a tree whose only change was
/// inside `gateway/wasm/` — which those tests never load).
///
/// A LOCK RATHER THAN A WIDER ASSUMPTION: the tests still assert what they always did, and they now take
/// turns. `tokio`'s mutex because the guard is held across `await` (the probes are async).
pub static PORT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
