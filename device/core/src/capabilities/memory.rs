//! `memory`: the declaration. The implementation lives in `device/src/capabilities/memory.rs`.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};

/// The most entries one search or list returns; the implementation enforces it too.
pub const MAX_RESULTS: usize = 100;

/// The default when the caller names no limit.
pub const DEFAULT_RESULTS: usize = 10;

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "\
The memory capability is this machine's append-only store of facts: one JSONL line per operation, \
replayed backwards into the live view. `memory.append` returns the id every other memory tool names. \
An update replaces the text and the complete tag list (send `[]` to clear the tags); a delete retires \
the entry and a second delete refuses. A search is a case-insensitive substring over text and tags; \
a list enumerates what is there, newest first, optionally filtered to one exact tag. An export is the \
whole store as JSONL, capped at 2 MiB, and says whether the cap cut it.";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "memory",
    guide: GUIDE,
    tools: &[
        ToolDef {
            name: "memory.append",
            destructive: true,
            description: "Append one entry to this machine's append-only memory and return its id; the id is what memory.update and memory.delete name.",
            schema: Some(append_schema),
        },
        ToolDef {
            name: "memory.update",
            destructive: true,
            description: "Replace the text and tags of the entry an id names; both are required, tags is the complete new list (send [] to clear them), validation is exactly memory.append's, and an id that is not live is refused as no_such_entry.",
            schema: Some(update_schema),
        },
        ToolDef {
            name: "memory.delete",
            destructive: true,
            description: "Retire one remembered entry by id; every later read omits it, and a second delete is refused as no_such_entry.",
            schema: Some(delete_schema),
        },
        ToolDef {
            name: "memory.search",
            destructive: false,
            description: "Search this machine's memory, newest entries first.",
            schema: Some(search_schema),
        },
        ToolDef {
            name: "memory.list",
            destructive: false,
            description: "Enumerate remembered entries, newest first, optionally only those carrying one tag; a search matches text, this lists what is there.",
            schema: Some(list_schema),
        },
        ToolDef {
            name: "memory.export",
            destructive: false,
            description: "Export the whole store as JSONL text, up to 2 MiB; the result names the entry count and whether the cap cut it.",
            schema: Some(export_schema),
        },
    ],
};

fn append_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "text": {
                "type": "string",
                "minLength": 1,
                "description": "The fact to remember; an empty text is refused."
            },
            "tags": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Free-form tags; a search matches them as well as the text. At most 32 tags of 128 bytes each."
            }
        },
        "required": ["text"],
        "additionalProperties": false
    })
}

fn update_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {
                "type": "string",
                "description": "The id memory.list, memory.search or memory.append returned."
            },
            "text": {
                "type": "string",
                "minLength": 1,
                "description": "The entry's new text; it replaces what the entry said. An empty text is refused: a delete retires an entry."
            },
            "tags": {
                "type": "array",
                "items": { "type": "string" },
                "description": "The entry's complete new tag list, not a patch: at most 32 tags of 128 bytes each, and [] clears them."
            }
        },
        "required": ["id", "text", "tags"],
        "additionalProperties": false
    })
}

fn delete_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {
                "type": "string",
                "description": "The id memory.list, memory.search or memory.append returned."
            }
        },
        "required": ["id"],
        "additionalProperties": false
    })
}

fn search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Case-insensitive substring matched against text and tags."
            },
            "limit": {
                "type": "integer",
                "minimum": 0,
                "maximum": MAX_RESULTS,
                "description": format!("How many entries to return, newest first (default {DEFAULT_RESULTS}).")
            }
        },
        "required": ["query"],
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
                "maximum": MAX_RESULTS,
                "description": format!("How many entries to return, newest first (default {DEFAULT_RESULTS}).")
            },
            "tag": {
                "type": "string",
                "description": "Only entries carrying exactly this tag, matched case-insensitively."
            }
        },
        "additionalProperties": false
    })
}

fn export_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}
