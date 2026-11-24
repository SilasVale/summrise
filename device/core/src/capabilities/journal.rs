//! `journal`: the declaration. The implementation lives in `device/src/capabilities/journal.rs`.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "The journal capability reads this machine's own record: the calls and sessions that happened here, including those that never went through the console. journal.read returns a page of rows below a cursor (a row carries a summary, never a payload); journal.get names which asked traces the retained journal still holds, so a page can mark a call that exists in one record and not the other; journal.day counts one day by door and failure, because a day is an aggregate, not a page. A device with no journal answers journal: false rather than failing.";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "journal",
    guide: GUIDE,
    tools: &[
        ToolDef {
            name: "journal.get",
            destructive: false,
            description: "Name which of the asked traces this machine's retained journal still holds, split into retained and missing; a scan that hit its row cap claims no absence beyond it.",
            schema: Some(get_schema),
        },
        ToolDef {
            name: "journal.read",
            destructive: false,
            description: "Return a page of this machine's record below a cursor: rows carry the frame and, on a call, the tool and outcome, never a payload. The answer names the retained bounds and whether more lie below; a device with no journal answers journal: false.",
            schema: Some(read_schema),
        },
        ToolDef {
            name: "journal.day",
            destructive: false,
            description: "Count one day's calls by the door they arrived by and by failure, over an inclusive range the caller names; a device with no journal answers journal: false with zero counts.",
            schema: Some(day_schema),
        },
    ],
};

fn get_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "traces": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 128
                    },
                    "minItems": 1,
                    "maxItems": 512,
                    "description": "The trace ids to look for: the value of a call's `trace.call`."
                }
            },
            "required": ["traces"],
            "additionalProperties": false
    })
}

fn day_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "day_start_ms": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "The first moment of the range, in Unix milliseconds (inclusive)."
                },
                "day_end_ms": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "The moment after the range, in Unix milliseconds (exclusive)."
                }
            },
            "required": ["day_start_ms", "day_end_ms"],
            "additionalProperties": false
    })
}

fn read_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "before_seq": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Answer rows older than this sequence: the last row the caller drew."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "description": "How many rows to answer, newest first."
                }
            },
            "required": ["limit"],
            "additionalProperties": false
    })
}
