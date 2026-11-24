//! `host`: the declaration. The implementation lives in `device/src/capabilities/host.rs`.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "The host capability is about the machine itself. host.facts reads what the machine is: operating system and build, host name, uptime, CPU cores, memory and disk totals, the Windows sessions and their state, and whether the per-user helper is registered; facts that cannot be read are zero or empty, never an error. host.restart restarts the app through its launcher: every session ends and the machine goes offline briefly, the launcher allows three consecutive restarts, and a standalone process (one the launcher did not start) refuses, because the restart would not bring it back.";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "host",
    guide: GUIDE,
    tools: &[
        ToolDef {
            name: "host.facts",
            destructive: false,
            description: "Read what this machine is running on: operating system and build, host name, uptime, CPU cores, memory and disk totals, and the Windows sessions with their state (active, connected, disconnected, idle). Also whether the per-user helper is registered, its pid and session. Facts that cannot be read are zero or empty, never an error.",
            schema: Some(facts_schema),
        },
        ToolDef {
            name: "host.restart",
            destructive: true,
            description: "Restart this machine's app through its launcher. Terminals, browser tabs and runs on this machine end, and the machine goes offline briefly. The launcher allows three consecutive restarts, so repeated restarts can leave the device down; a standalone process refuses.",
            schema: Some(restart_schema),
        },
    ],
};

fn facts_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn restart_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}
