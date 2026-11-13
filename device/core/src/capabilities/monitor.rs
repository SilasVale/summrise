//! `monitor`: the declaration. The implementation lives in `device/src/capabilities/monitor.rs`.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};
/// The fastest a monitor may probe.
pub const MIN_INTERVAL_MS: u64 = 5000;

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "The monitor capability watches a host:port and keeps the last 60 probes. monitor.add takes a kind (tcp or http), host, port and interval (at least 5 s), plus an http path and the body text it must contain; at most 16 targets. monitor.remove stops a watch and discards its series; monitor.list shows every watch with its summary; monitor.probe probes one now and records the sample; monitor.status returns one watch's summary (up_now, since_ms, up_pct, drops, last_status).";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "monitor",
    guide: GUIDE,
    tools: &[
        ToolDef {
            name: "monitor.add",
            destructive: false,
            description: "Watch a host:port and persist the watch; tcp connects, http GETs path and is up on a status below 500 whose body contains expect when set. At most 16 targets, an interval of at least 5000 ms, and the last 60 samples are summarized.",
            schema: Some(add_schema),
        },
        ToolDef {
            name: "monitor.remove",
            destructive: false,
            description: "Stop watching a target by id and discard its in-memory series; removed says whether a watch existed.",
            schema: Some(remove_schema),
        },
        ToolDef {
            name: "monitor.list",
            destructive: false,
            description: "List the watched targets with their settings and summary: up_now, since_ms, up_pct, drops and last_status from the last 60 probes.",
            schema: Some(list_schema),
        },
        ToolDef {
            name: "monitor.probe",
            destructive: false,
            description: "Probe one watched target now and return the sample {ts, ok, latency_ms}; it is recorded like a scheduled probe.",
            schema: Some(probe_schema),
        },
        ToolDef {
            name: "monitor.status",
            destructive: false,
            description: "Return one target's summary over its last 60 probes: up_now, since_ms, up_pct, drops and last_status.",
            schema: Some(status_schema),
        },
    ],
};

fn add_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["tcp", "http"],
                    "description": "tcp makes a TCP connection; http performs a GET of path."
                },
                "host": {
                    "type": "string",
                    "minLength": 1,
                    "description": "IP address or name, e.g. \"192.168.1.1\" or \"example.com\"; no scheme and no brackets."
                },
                "port": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 65535,
                    "description": "The TCP port the service listens on."
                },
                "path": {
                    "type": "string",
                    "description": "For an http target, the path to GET (default \"/\"); it is normalized to start with a slash."
                },
                "expect": {
                    "type": "string",
                    "description": "For an http target, text the body MUST contain. A page that answers 200 without it is down."
                },
                "interval_ms": {
                    "type": "integer",
                    "minimum": MIN_INTERVAL_MS,
                    "description": format!("How often to probe, in milliseconds; the minimum is {}.", MIN_INTERVAL_MS)
                }
            },
            "required": ["kind", "host", "port", "interval_ms"],
            "additionalProperties": false
    })
}

fn remove_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The target id monitor.list returned. Its in-memory series is discarded."
                }
            },
            "required": ["id"],
            "additionalProperties": false
    })
}

fn list_schema() -> Value {
    json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
    })
}

fn probe_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The target id monitor.list returned; the probe is recorded in its series."
                }
            },
            "required": ["id"],
            "additionalProperties": false
    })
}

fn status_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The target id monitor.list returned; its summary covers the last 60 probes."
                }
            },
            "required": ["id"],
            "additionalProperties": false
    })
}
