//! `screen`: the declaration. The implementation lives in `device/src/capabilities/screen.rs`.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "The screen capability reads the logged-in user's desktop through the per-user helper. It is the only capability that runs in that session: the service cannot see a desktop it does not own, so the helper takes the picture and the device refuses with a sentence when no helper is running, the session is locked or disconnected, or the helper did not register this boot. A desktop nobody is looking at is not a picture.";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "screen",
    guide: GUIDE,
    tools: &[ToolDef {
        name: "screen.capture",
        destructive: false,
        description: "Capture the logged-in user's screen as a PNG through the per-user helper. A session that is locked, disconnected, or has no helper running is refused with a sentence; a desktop nobody is looking at is not a picture.",
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
