//! Design Plugin — lets an AI SEE the Summrise pages' design.
//!
//! The agent has no browser (CDP/headless-Chrome is retired) and runs as a
//! Windows service (no interactive desktop), so screenshots are impossible.
//! `page_view` instead fetches a page's live HTML/CSS from the agent's own
//! HTTP surface (/panel, /panel/*) — the AI reads the design tokens,
//! structure and styling directly. This is the honest, token-cheap way an AI
//! "sees" the design.

mod tools;

use summrise_agent_core::ToolDef;

/// Plugin struct — stateless; every tool closes over what it needs.
pub struct DesignPlugin {
    /// Console base; `None` = no console configured (saisi decouple) —
    /// `page_view` then errors explicitly instead of a hardcoded host.
    console_url: Option<String>,
    download_url: Option<String>,
    /// The device's own token. `page_view` fetches the panel HTML, which the agent ALREADY
    /// injected this token into — so the redactor needs the VALUE. A pattern guess cannot tell
    /// a credential from a line of panel.js that merely mentions the pattern, and it did not:
    /// round 120 measured it silently rewriting `window.__PANEL_TOKEN__)||""`.
    device_token: Option<String>,
    /// The port this agent actually listens on. The tool used to default to a hardcoded 18080,
    /// which on a custom-port install reads a STRANGER'S service and presents it as the panel.
    local_port: u16,
}

impl DesignPlugin {
    pub fn new(
        console_url: Option<String>,
        download_url: Option<String>,
        device_token: Option<String>,
        local_port: u16,
    ) -> Self {
        Self {
            console_url,
            download_url,
            device_token,
            local_port,
        }
    }
}

impl Default for DesignPlugin {
    fn default() -> Self {
        Self::new(None, None, None, 18080)
    }
}

impl summrise_agent_core::Plugin for DesignPlugin {
    fn name(&self) -> &'static str {
        "design"
    }
    fn display_name(&self) -> &'static str {
        "Design"
    }
    fn description(&self) -> &'static str {
        "Summrise page design inspection — view a page's HTML/CSS to see its design"
    }
    fn tools(&self) -> Vec<ToolDef> {
        vec![tools::page_view(
            self.console_url.clone(),
            self.download_url.clone(),
            self.device_token.clone(),
            self.local_port,
        )]
    }
}
