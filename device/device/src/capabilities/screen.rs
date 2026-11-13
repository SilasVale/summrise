//! `screen`: the implementation of `summrise_core::capabilities::screen`. The picture comes from the
//! per-user helper; this module is the refusal rules and the wire shape.

use serde_json::{json, Map, Value};

use crate::mcp::ToolError;
use summrise_core::capabilities::screen::CAPABILITY;

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

/// Capture the logged-in user's desktop.
pub async fn call(tool: &str, _arguments: Map<String, Value>) -> Result<String, ToolError> {
    match tool {
        "screen.capture" => {
            // The one tool that runs where this process cannot: the picture comes from the
            // per-user helper, and "no helper" is a sentence rather than a black frame.
            let Some(helper) = crate::helper::current() else {
                return Err(ToolError::Refused(format!(
                    "{tool}: no per-user helper is running on this device"
                )));
            };
            let Some(port) = helper.port() else {
                return Err(ToolError::Refused(format!(
                    "{tool}: the per-user helper has not registered this boot"
                )));
            };
            let capture = crate::helper::capture(port, helper.token())
                .await
                .map_err(|error| ToolError::Refused(format!("{tool}: {error}")))?;
            Ok(json!({
                "format": "png",
                "data": capture.png,
                "width": capture.width,
                "height": capture.height,
            })
            .to_string())
        }
        other => Err(ToolError::Internal(format!(
            "{other}: the screen capability does not declare this tool"
        ))),
    }
}
