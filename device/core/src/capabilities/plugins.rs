//! `plugins`: the declaration. The implementation lives in `device/src/capabilities/plugins.rs`.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};

/// The most lines `plugins.logs` returns, and the default.
pub const MAX_LOG_LINES: usize = 2_000;
pub const DEFAULT_LOG_LINES: usize = 200;

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "The plugins capability is the operator's surface for the plugin host. A plugin is an MCP server installed by path: plugins.inspect reads its plugin.json without starting it, plugins.add validates and starts it and advertises its tools as <namespace>.<tool>, plugins.list shows the installed plugins with their state and tool count, plugins.remove stops one and forgets its record without deleting its directory, plugins.restart resets its start budget, and plugins.logs reads the tail of its log (default 200 lines, at most 2000). A plugin runs as a child process with this machine's permissions.";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "plugins",
    guide: GUIDE,
    tools: &[
        ToolDef {
            name: "plugins.list",
            destructive: false,
            description: "List the installed plugins with their state and tool count; a plugin's tools are advertised only while it is running.",
            schema: Some(list_schema),
        },
        ToolDef {
            name: "plugins.inspect",
            destructive: false,
            description: "Read a plugin directory's plugin.json without starting it or recording it, so a path can be checked before plugins.add.",
            schema: Some(inspect_schema),
        },
        ToolDef {
            name: "plugins.add",
            destructive: true,
            description: "Install the MCP server in a directory: validate plugin.json, start it, and advertise its tools as <namespace>.<tool>; installing registers the path and runs its code with this machine's permissions.",
            schema: Some(add_schema),
        },
        ToolDef {
            name: "plugins.remove",
            destructive: true,
            description: "Remove a plugin by namespace: forget its record and stop its process, never deleting its source directory.",
            schema: Some(remove_schema),
        },
        ToolDef {
            name: "plugins.restart",
            destructive: true,
            description: "Stop and start a plugin now, resetting its restart budget after a failure.",
            schema: Some(restart_schema),
        },
        ToolDef {
            name: "plugins.logs",
            destructive: false,
            description: "Read the last lines of a plugin's log (default 200, at most 2000).",
            schema: Some(logs_schema),
        },
    ],
};

fn list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn inspect_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "The plugin directory to read; it must contain plugin.json. No process is started and no record is written."
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

fn add_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "The plugin directory to install; it must contain plugin.json. Installing registers the path and runs its code with this machine's permissions."
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

fn remove_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "namespace": {
                "type": "string",
                "description": "The namespace plugins.list reports; its source directory is never deleted."
            }
        },
        "required": ["namespace"],
        "additionalProperties": false
    })
}

fn restart_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "namespace": {
                "type": "string",
                "description": "The namespace plugins.list reports; a manual restart resets the start budget."
            }
        },
        "required": ["namespace"],
        "additionalProperties": false
    })
}

fn logs_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "namespace": {
                "type": "string",
                "description": "The namespace plugins.list reports."
            },
            "lines": {
                "type": "integer",
                "minimum": 0,
                "maximum": MAX_LOG_LINES,
                "description": format!("How many trailing lines to return (default {}).", DEFAULT_LOG_LINES)
            }
        },
        "required": ["namespace"],
        "additionalProperties": false
    })
}
