//! `host`: the implementation of `summrise_core::capabilities::host`. The platform facts themselves
//! are `crate::host::facts`; this module is the tool surface and the restart rule.

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::mcp::{parse, ToolError};
use crate::restart::Hook;
use summrise_core::capabilities::host::CAPABILITY;

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

/// Run one of this capability's tools.
pub fn call(
    tool: &str,
    arguments: Map<String, Value>,
    data_dir: &std::path::Path,
    supervised: bool,
    restart: &Hook,
) -> Result<String, ToolError> {
    match tool {
        "host.facts" => {
            // Read-only, and the one tool an operator can call without minting a grant: knowing
            // what machine a device is is context for every decision that follows.
            let _arguments: EmptyArgs = parse(arguments, tool)?;
            Ok(crate::host::facts(data_dir).to_string())
        }
        "host.restart" => {
            let _arguments: EmptyArgs = parse(arguments, tool)?;
            if !supervised {
                return Err(ToolError::Refused(
                    "this device was not started by the launcher; restarting it here would not \
                     bring it back"
                        .to_owned(),
                ));
            }
            let answer = json!({ "restarting": true }).to_string();
            restart();
            Ok(answer)
        }
        other => Err(ToolError::Internal(format!(
            "{other}: the host capability does not declare this tool"
        ))),
    }
}

/// A tool whose input schema declares no properties.
#[allow(dead_code)] // constructed by serde, never by code
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}
