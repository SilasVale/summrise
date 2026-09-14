//! Monitor Plugin — reachability, as MCP tools.
//!
//! Tools: `monitor_list`, `monitor_add`, `monitor_remove`, `monitor_probe`.
//!
//! WHY THEY EXIST. The device has watched `host:port` targets since round 261 — an operator
//! asks the panel for a watch and the panel draws it — but **the AI could not see any of it**:
//! the surface was a route and a card, and the AI's only channel to this device is MCP. So an
//! AI could reboot an ONU and then have no ground truth about whether it came back, while the
//! device held a minute-by-minute record of exactly that. These four tools close the gap in the
//! direction that matters: the caller IS the AI, and this is the same instrument object, not a
//! second copy of it (`crate::monitor` owns the targets, the probes and the summaries).
//!
//! THE SHAPE OF THE WORK THEY ENABLE, which is why there are four rather than one:
//!   * `monitor_list` — what is being watched, with each target's summary and recent series.
//!     This is the "what happened while I was doing something else" call.
//!   * `monitor_probe` — probe NOW, synchronously, and get the result. The pair
//!     `monitor_probe` → act → `monitor_probe` is how an AI states a before/after instead of
//!     inferring it from a 15 s timer.
//!   * `monitor_add` / `monitor_remove` — the AI can leave a watch behind for the operator
//!     (the list is persisted, so a watch an AI adds outlives both of them).
//!
//! Stateless — the targets live in `crate::monitor` (persisted under DataDir).

pub mod tools;

use vale_agent_core::{Plugin, ToolDef};

/// Plugin struct — stateless; the tools close over nothing.
pub struct MonitorPlugin;

impl Plugin for MonitorPlugin {
    fn name(&self) -> &'static str {
        "monitor"
    }
    fn display_name(&self) -> &'static str {
        "Reachability"
    }
    fn description(&self) -> &'static str {
        "Watch host:port targets over TCP — what is up, what is down, and since when"
    }
    fn tools(&self) -> Vec<ToolDef> {
        tools::build()
    }
}
