//! THE MCP TOOL-FAILURE CODE FAMILY — the stable codes a console MCP client retries on.
//!
//! MOVED FROM `gateway/src/mcp-errors.ts` (16 lines, and the whole of it is these five strings and one
//! constructor). The codes are the DISTINCTION the model needs to retry smartly, and the source says why they
//! exist at all: a flat `-32603 Tool failed: …` string hides whether the device is offline, the session is gone,
//! a call timed out or the browser bridge is busy — so the code rides the JSON-RPC error's `data` (round-55) and
//! `mcp.rs` puts it there.
//!
//! **`ToolError` IS A `Result` ERROR RATHER THAN A THROWN VALUE, AND THAT IS THE ONE STRUCTURAL DIFFERENCE FROM
//! THE SOURCE.** JavaScript's `throw` unwinds to the nearest `catch` wherever it is; Rust has no such thing, so
//! every function on the two dispatch paths returns `Result<_, ToolError>` and the SELF-HEAL in `mcp.rs` is a
//! `match` on the error's code. The mapping back to the wire is one line at the top (`handle`'s `catch` arm).

/// `DEVICE_UNREACHABLE` — the dial itself failed (a tunnel that is down, a hostname that may not be dialled).
pub const DEVICE_UNREACHABLE: &str = "DEVICE_UNREACHABLE";

/// `TIMEOUT` — the device answered nothing inside the budget, or named a timeout of its own (`ssh_timeout`).
pub const TIMEOUT: &str = "TIMEOUT";

/// `SESSION_NOT_FOUND` — the terminal session is gone (usually an agent restart), which is the one code the
/// self-heal retargets on.
pub const SESSION_NOT_FOUND: &str = "SESSION_NOT_FOUND";

/// `SESSION_BUSY` — the device's own `session_busy`, or the browser bridge's per-device in-flight cap.
pub const SESSION_BUSY: &str = "SESSION_BUSY";

/// `TOOL_ERROR` — a device that is UP and answered a failure this side has no better name for, and the
/// registered-but-unrouted programming error.
pub const TOOL_ERROR: &str = "TOOL_ERROR";

/// `ToolErr(code, message)` — `Object.assign(new Error(message), { code })`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    pub code: &'static str,
    pub message: String,
}

impl ToolError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        ToolError {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// **THE TWO SHAPES A DISPATCH PATH CAN FAIL WITH, AND THE CLIENT CAN TELL THEM APART.**
///
/// JavaScript throws both, and `handleMcp`'s catch reads `e.code`: a `ToolErr` carries one and reaches the
/// client as `error.data.code`, while a plain `Error` — the source's `TypeError` from `new URL`, the bridge's
/// `mcp_client_call failed: <status>` — has none, and the client sees no `data` at all. Collapsing them would
/// tell a model to retry against a code that does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolFailure {
    Coded(ToolError),
    Plain(String),
}

impl ToolFailure {
    /// `new Error(message)` — the uncoded arm.
    pub fn plain(message: impl Into<String>) -> Self {
        ToolFailure::Plain(message.into())
    }

    /// `e.message`, whichever arm this is — the text `Tool <name> failed: …` carries.
    pub fn message(&self) -> &str {
        match self {
            ToolFailure::Coded(error) => &error.message,
            ToolFailure::Plain(message) => message,
        }
    }
}

impl From<ToolError> for ToolFailure {
    fn from(error: ToolError) -> Self {
        ToolFailure::Coded(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five codes are WIRE VALUES: they ride `error.data.code` to the client, and a rename is a breaking
    /// change to a contract a model retries on. Pinned as the strings `mcp-errors.ts` exports.
    #[test]
    fn the_codes_are_the_sources_strings() {
        assert_eq!(DEVICE_UNREACHABLE, "DEVICE_UNREACHABLE");
        assert_eq!(TIMEOUT, "TIMEOUT");
        assert_eq!(SESSION_NOT_FOUND, "SESSION_NOT_FOUND");
        assert_eq!(SESSION_BUSY, "SESSION_BUSY");
        assert_eq!(TOOL_ERROR, "TOOL_ERROR");
    }

    /// `ToolErr` carries the message the source's `Error` carried, which is what `Tool <name> failed: <message>`
    /// puts on the wire.
    #[test]
    fn a_tool_error_is_its_message_with_a_code() {
        let e = ToolError::new(SESSION_NOT_FOUND, "Session not found: term-old");
        assert_eq!(e.to_string(), "Session not found: term-old");
        assert_eq!(e.code, SESSION_NOT_FOUND);
    }
}
