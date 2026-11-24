//! `system`: the declaration. The implementation lives in `device/src/capabilities/system.rs`.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};
/// The longest a network test may take, and how long it takes when the caller names no timeout.
pub const MAX_TIMEOUT_MS: u64 = 60_000;
pub const DEFAULT_NET_TIMEOUT_MS: u64 = 5_000;

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "The system capability looks at and ends processes, and tests one network endpoint. system.process_list lists processes (pid, executable name, resident memory), optionally filtered by name; system.process_kill ends processes by pid or by exact name and says which pids were signalled and which could not be; system.net_test resolves a host name and connects to one TCP port, reporting the addresses, whether the connection succeeded, its latency, and the error when it did not. ICMP is not offered: it needs raw sockets.";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "system",
    guide: GUIDE,
    tools: &[
        ToolDef {
            name: "system.process_list",
            destructive: false,
            description: "List running processes on this machine: pid, executable name and resident memory, optionally filtered by name.",
            schema: Some(process_list_schema),
        },
        ToolDef {
            name: "system.process_kill",
            destructive: true,
            description: "Terminate processes by pid or by exact name; the result names the pids that were signalled and the pids that could not be.",
            schema: Some(process_kill_schema),
        },
        ToolDef {
            name: "system.net_test",
            destructive: false,
            description: "Resolve a host name and connect to one TCP port from this machine, returning the addresses resolved and the connect latency; ICMP needs raw sockets, so it is not offered.",
            schema: Some(net_test_schema),
        },
    ],
};

fn process_list_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Only processes whose executable name contains this text, case-insensitively."
                }
            },
            "additionalProperties": false
    })
}

fn process_kill_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "pid": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "A process id; name a pid or a name, not both."
                },
                "name": {
                    "type": "string",
                    "description": "An exact executable name, case-insensitive; every match is ended."
                }
            },
            "additionalProperties": false
    })
}

fn net_test_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "host": {
                    "type": "string",
                    "description": "A host name or IP address to resolve and connect to."
                },
                "port": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 65535,
                    "description": "The TCP port to connect to."
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_TIMEOUT_MS,
                    "description": format!("The total time to spend resolving and connecting (default {DEFAULT_NET_TIMEOUT_MS}).")
                }
            },
            "required": ["host", "port"],
            "additionalProperties": false
    })
}
