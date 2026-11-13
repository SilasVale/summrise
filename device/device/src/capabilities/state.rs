//! `state`: the implementation of `summrise_core::capabilities::state`. The document is the device's
//! own store; this module is the readiness answer and the event tail the page draws.

use serde_json::{json, Map, Value};

use crate::mcp::ToolError;
use crate::store::{Store, EVENT_TAIL};
use summrise_core::capabilities::state::CAPABILITY;

/// Whether `tool` is one this capability declares.
pub fn owns(tool: &str) -> bool {
    CAPABILITY
        .tools
        .iter()
        .any(|declared| declared.name == tool)
}

/// Every tool this capability declares, for the registry-alignment test.
#[cfg(test)]
pub fn names() -> impl Iterator<Item = &'static str> {
    CAPABILITY.tools.iter().map(|tool| tool.name)
}

/// Read the device's own state document.
pub fn call(
    tool: &str,
    _arguments: Map<String, Value>,
    store: &Store,
    version: &str,
    bus: Option<&summrise_bus::Bus>,
) -> Result<String, ToolError> {
    match tool {
        "state.get" => {
            // A poisoned document is the failure this tool exists to make visible, so it is a
            // successful call carrying `poisoned: true` and a null state, not an error the console
            // would render as "the device is offline".
            let (state, health) = match store.load() {
                Ok(state) => (
                    json!(state),
                    json!({
                        "status": "ok",
                        "version": version,
                        "pid": std::process::id(),
                        "poisoned": false,
                    }),
                ),
                Err(error) => (
                    Value::Null,
                    json!({
                        "status": "degraded",
                        "version": version,
                        "pid": std::process::id(),
                        "poisoned": true,
                        "reason": format!("state: {error}"),
                    }),
                ),
            };
            // Best-effort: the document still answers when the log cannot be read, but the field is
            // `null` — "the device cannot say" — not `[]`, which would claim it recorded nothing.
            let events = match store.tail_events(EVENT_TAIL) {
                Ok(events) => json!(events),
                Err(_) => Value::Null,
            };
            // The record's facts are the device page's answer to "is my history here": where it
            // lives, how big it is, and the sequence bounds. `null` on a process with no record,
            // never an invented zero.
            let record = match bus.and_then(|bus| bus.facts().ok()) {
                Some(facts) => json!(facts),
                None => Value::Null,
            };
            Ok(json!({
                "state": state,
                "health": health,
                "events": events,
                "record": record,
            })
            .to_string())
        }
        other => Err(ToolError::Internal(format!(
            "{other}: the state capability does not declare this tool"
        ))),
    }
}
