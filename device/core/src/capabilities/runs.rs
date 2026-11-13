//! `runs`: the declaration. The implementation lives in `device/src/capabilities/runs.rs`.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};
/// How long a run may take when the caller names no timeout, and the ceiling it may ask for.
pub const DEFAULT_TIMEOUT_MS: u64 = 60_000;
pub const MAX_TIMEOUT_MS: u64 = 10 * 60 * 1000;
/// How many history rows a list returns by default, and the most it may ask for.
pub const DEFAULT_LIST_LIMIT: usize = 20;
pub const MAX_LIST_LIMIT: usize = 100;

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "The runs capability runs one command and keeps its history. runs.exec takes an argv array (or a shell line with shell true), an optional cwd inside the files root, a timeout (default 60 s, at most 10 min) and up to 32 environment variables; stdout and stderr keep 1 MiB each, and a run that does not start or does not exit zero is a refusal naming why. runs.list returns the recent rows newest first (default 20, at most 100; output bodies are not stored, and the history keeps 10 000 rows per file), and runs.get returns one row by id.";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "runs",
    guide: GUIDE,
    tools: &[
        ToolDef {
            name: "runs.exec",
            destructive: true,
            description: "Run a command once and return its captured output; command and args are an argv array with no shell unless shell is true. cwd must be inside the files root (where the process starts, not a jail), stdout and stderr keep 1 MiB each, timeout_ms defaults to 60 seconds and is at most 10 minutes, and env is at most 32 variables of 4 KiB each.",
            schema: Some(exec_schema),
        },
        ToolDef {
            name: "runs.list",
            destructive: false,
            description: "List the recent runs newest first; the history keeps 10 000 rows per file with the overflow in runs.1.jsonl, stores no output bodies, and limit defaults to 20 and is at most 100.",
            schema: Some(list_schema),
        },
        ToolDef {
            name: "runs.get",
            destructive: false,
            description: "Return one run's history row by id; output bodies are not stored.",
            schema: Some(get_schema),
        },
    ],
};

fn exec_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The program to run (argv[0]); with shell true, the whole line the platform shell parses."
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Arguments passed as an argv array, never word-split or expanded; empty when shell is true."
                },
                "cwd": {
                    "type": "string",
                    "description": "A directory relative to the files root where the process starts; it constrains the start, not what the process can reach. Defaults to the root."
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_TIMEOUT_MS,
                    "description": format!("How long the command may run (default {} ms). On expiry the process group is killed and timed_out is true.", DEFAULT_TIMEOUT_MS)
                },
                "env": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "description": "Environment variables for this process, added to the device's own (at most 32, each value at most 4096 bytes)."
                },
                "shell": {
                    "type": "boolean",
                    "description": "Run command through the platform shell (sh -c on Unix, cmd /C on Windows); false runs it as an argv array with no shell."
                }
            },
            "required": ["command"],
            "additionalProperties": false
    })
}

fn list_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": MAX_LIST_LIMIT,
                    "description": format!("How many history rows to return, newest first (default {}).", DEFAULT_LIST_LIMIT)
                }
            },
            "additionalProperties": false
    })
}

fn get_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "A run id runs.exec or runs.list returned; output bodies are not stored."
                }
            },
            "required": ["id"],
            "additionalProperties": false
    })
}
