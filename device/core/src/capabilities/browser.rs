//! `browser`: the declaration. The implementation lives in `device/src/capabilities/browser.rs`.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "The browser capability drives one headless Chrome for this machine; the first browser tool call starts it. browser.open opens a tab at a URL and waits for the document to be parsed (at most 8 tabs); browser.navigate moves a tab; browser.eval runs a JavaScript expression in it and returns the value (refused over 1 MiB); browser.screenshot returns a base64 PNG (refused over 8 MiB); browser.cast returns the newest frame of a live screencast as base64 JPEG, which stops itself after 60 s unpolled; browser.list names the targets; browser.close closes one tab without stopping the browser, and browser.shutdown stops the browser itself, after which a later call starts it again.";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "browser",
    guide: GUIDE,
    tools: &[
        ToolDef {
            name: "browser.list",
            destructive: false,
            description: "List the browser's targets (id, title, url, type); the first browser tool call starts headless Chrome.",
            schema: Some(list_schema),
        },
        ToolDef {
            name: "browser.open",
            destructive: true,
            description: "Open a tab at a URL and wait for its document to be parsed; at most 8 tabs, and an open beyond that is refused.",
            schema: Some(open_schema),
        },
        ToolDef {
            name: "browser.navigate",
            destructive: true,
            description: "Navigate one tab to a URL and wait for its document to be parsed.",
            schema: Some(navigate_schema),
        },
        ToolDef {
            name: "browser.eval",
            destructive: true,
            description: "Evaluate a JavaScript expression in one tab and return its value; a result over 1 MiB is refused.",
            schema: Some(eval_schema),
        },
        ToolDef {
            name: "browser.screenshot",
            destructive: false,
            description: "Capture one tab as a base64 PNG; a capture over 8 MiB is refused.",
            schema: Some(screenshot_schema),
        },
        ToolDef {
            name: "browser.cast",
            destructive: false,
            description: "The newest frame of a live screencast of one tab, as a base64 JPEG; a frame over 8 MiB is refused, and the stream stops itself after 60 s unpolled.",
            schema: Some(cast_schema),
        },
        ToolDef {
            name: "browser.close",
            destructive: false,
            description: "Close one tab; the browser stays running and the tab id is not reused.",
            schema: Some(close_schema),
        },
        ToolDef {
            name: "browser.shutdown",
            destructive: false,
            description: "Stop the browser this machine started; a later browser tool call starts it again.",
            schema: Some(shutdown_schema),
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

fn open_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to load; a data: URL works without a network. At most 8 tabs may be open."
                }
            },
            "required": ["url"],
            "additionalProperties": false
    })
}

fn navigate_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "The target id browser.list or browser.open returned."
                },
                "url": {
                    "type": "string",
                    "description": "The URL to navigate the tab to."
                }
            },
            "required": ["target", "url"],
            "additionalProperties": false
    })
}

fn eval_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "The target id browser.list or browser.open returned."
                },
                "expression": {
                    "type": "string",
                    "description": "JavaScript evaluated in the page's current document; its value is returned (at most 1 MiB)."
                }
            },
            "required": ["target", "expression"],
            "additionalProperties": false
    })
}

fn screenshot_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "The target id browser.list or browser.open returned. The result is a base64 PNG of at most 8 MiB."
                }
            },
            "required": ["target"],
            "additionalProperties": false
    })
}

fn cast_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "The target id browser.list or browser.open returned. The result is the newest screencast frame as a base64 JPEG of at most 8 MiB; the stream stops itself after 60 s without a call."
                }
            },
            "required": ["target"],
            "additionalProperties": false
    })
}

fn close_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "The target id browser.list or browser.open returned; the browser stays running."
                }
            },
            "required": ["target"],
            "additionalProperties": false
    })
}

fn shutdown_schema() -> Value {
    json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
    })
}
