//! Design Plugin — lets an AI SEE the Summrise pages' design.
//!
//! The agent has no browser (CDP/headless-Chrome is retired) and runs as a
//! Windows service (no interactive desktop), so screenshots are impossible.
//! `page_view` instead fetches a page's live HTML/CSS from the agent's own
//! HTTP surface (/panel, /panel/*) — the AI reads the design tokens,
//! structure and styling directly. This is the honest, token-cheap way an AI
//! "sees" the design.

mod tools;

use crate::state::ConfigHandle;
use summrise_agent_core::ToolDef;

/// Plugin struct — every tool reads the LIVE config at the point of use.
pub struct DesignPlugin {
    /// ONE handle where this plugin used to take FOUR boot clones (`console_url`, `download_url`,
    /// `device_token`, `local_port`) — each read per call now, never cached into a field ("for convenience"
    /// is exactly the copy that went stale). The four facts, and why each must be live:
    ///
    ///   * `console_url` — Console base; `None` = no console configured (saisi decouple) — `page_view` then
    ///     errors explicitly instead of a hardcoded host. `PUT /api/settings` can repoint the device, and
    ///     the old clone kept fetching the console the OPERATOR had already moved away from.
    ///   * `download_url` — the download site a remote page is read from, changeable the same way.
    ///   * `device_token` — The device's own token. `page_view` fetches the panel HTML, which the agent
    ///     ALREADY injected this token into — so the redactor needs the VALUE. A pattern guess cannot tell a
    ///     credential from a line of panel.js that merely mentions the pattern, and it did not: round 120
    ///     measured it silently rewriting `window.__PANEL_TOKEN__)||""`. A boot clone is the other half of
    ///     the same defect: after a rotation it holds the WRONG value, redacts nothing, and says so with a
    ///     successful response.
    ///   * `local_port` — The port this agent actually listens on. The tool used to default to a hardcoded
    ///     18080, which on a custom-port install reads a STRANGER'S service and presents it as the panel.
    config: ConfigHandle,
}

impl DesignPlugin {
    /// ONE argument where this used to take four (`console_url`, `download_url`, `device_token`,
    /// `local_port`): the values are not the plugin's to hold, they are the device's to change.
    pub fn new(config: ConfigHandle) -> Self {
        Self { config }
    }
}

impl Default for DesignPlugin {
    fn default() -> Self {
        Self::new(ConfigHandle::default())
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
        vec![tools::page_view(self.config.clone())]
    }
}
