//! `state`: the declaration. The implementation lives in `device/src/capabilities/state.rs`.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "The state capability answers the console's device page from the device itself: the state document, the readiness answer (`status`, release version, pid, whether the document is poisoned) and the last ten events the device recorded. A poisoned document is a successful call carrying `poisoned: true` and a null state — the failure this tool exists to make visible — not an error the console would render as \"the device is offline\".";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "state",
    guide: GUIDE,
    tools: &[ToolDef {
        name: "state.get",
        destructive: false,
        description: "Read this machine's state document plus its health: status (ok or degraded), release version, pid, whether the state document is poisoned (unreadable), and the last ten events the device recorded. A poisoned document is returned as null state with degraded health rather than a failed call.",
        schema: Some(empty_schema),
    }],
};

fn empty_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}
