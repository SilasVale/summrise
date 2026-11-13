//! `files`: the declaration. The implementation lives in `device/src/capabilities/files.rs`.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "\
The files capability works inside the device's files roots. Every path is relative to `workspace` \
(the default) or `shared` (only when the operator configured it); an absolute path, a `..` \
component, or a symlink that resolves outside the root is refused. `files.write` does not create \
missing parents: make one with `files.mkdir`. A read returns at most 1 MiB; ask for \
`encoding: \"base64\"` when the file is not text. A directory with more than 1000 entries is \
refused rather than truncated.";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "files",
    guide: GUIDE,
    tools: &[
        ToolDef {
            name: "files.read",
            destructive: false,
            description: "Read a file inside one of this machine's files roots: workspace by default, or shared when the operator configured it (the optional root argument).",
            schema: Some(read_schema),
        },
        ToolDef {
            name: "files.write",
            destructive: true,
            description: "Write a file inside one of this machine's files roots; its parent must exist. Pass root: \"shared\" to use the operator-configured second root.",
            schema: Some(write_schema),
        },
        ToolDef {
            name: "files.mkdir",
            destructive: false,
            description: "Create one directory inside one of this machine's files roots; never overwrites. Pass root: \"shared\" to use the operator-configured second root.",
            schema: Some(mkdir_schema),
        },
        ToolDef {
            name: "files.list",
            destructive: false,
            description: "List a directory inside one of this machine's files roots, sorted, at most 1000 entries. Pass root: \"shared\" to use the operator-configured second root.",
            schema: Some(list_schema),
        },
    ],
};

fn read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "A path relative to the device's files root, e.g. notes/todo.txt."
            },
            "root": {
                "type": "string",
                "enum": ["workspace", "shared"],
                "description": "Which configured root the path is relative to. workspace is the default; shared exists only when the operator configured it."
            },
            "encoding": {
                "type": "string",
                "enum": ["utf8", "base64"],
                "description": "How to return the bytes. utf8 (the default) is the file's text and fails on a file that is not text; base64 returns the bytes encoded, for a file that is not text."
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

fn write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "A path relative to the device's files root; its parent must exist (create it with files.mkdir)."
            },
            "root": {
                "type": "string",
                "enum": ["workspace", "shared"],
                "description": "Which configured root the path is relative to. workspace is the default; shared exists only when the operator configured it."
            },
            "content": {
                "type": "string",
                "description": "The bytes to write, at most 1 MiB, encoded as the `encoding` says. It replaces the file's bytes."
            },
            "encoding": {
                "type": "string",
                "enum": ["utf8", "base64"],
                "description": "How `content` encodes the bytes: utf8 (the default) for text, base64 for anything else. The 1 MiB limit applies to the decoded bytes."
            }
        },
        "required": ["path", "content"],
        "additionalProperties": false
    })
}

fn mkdir_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "One directory relative to the files root, e.g. notes; its parent must exist. It never overwrites or deletes."
            },
            "root": {
                "type": "string",
                "enum": ["workspace", "shared"],
                "description": "Which configured root the path is relative to. workspace is the default; shared exists only when the operator configured it."
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

fn list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "A directory relative to the files root; \"\" lists the root. Returns at most 1000 entries, sorted; a larger directory is an error."
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}
