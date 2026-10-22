//! A remote frame and the grant it carries, admitted once for every transport.
//!
//! Two transports deliver the same frame — the console's outbound WebSocket and a mesh channel —
//! and both must reach the same decision about it: the signature against the console key compiled
//! into this device, the device the grant names, its expiry with the clock-skew allowance, and the
//! tool a `tools/call` asks for. The decision lives here, in one function, so a rule fixed on one
//! transport cannot stay broken on the other.
//!
//! The frame is `{"id","mcp":{...},"grant":{"grant":{...},"signature":"..."}}`, exactly what the
//! console sends, and the device answers `{"id","result"|"error"}`. The grant is optional here so
//! that a frame which carries none is a typed refusal the caller can audit, rather than a frame
//! that cannot be correlated with its answer.

use ed25519_dalek::VerifyingKey;
use serde::Deserialize;
use serde_json::{json, Value};

use summrise_core::grant::{self, GrantEnvelope, CLOCK_SKEW_MS};

/// The JSON-RPC code a refusal that happened before the request could run answers with.
///
/// It is the console's own code for "not authorized", and the mesh answers the same one: the
/// caller's fix is the same either way — a grant that names this device and covers the tool.
pub(crate) const GRANT_REFUSAL_CODE: i32 = -32001;

/// The frame a remote transport carries.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct RemoteFrame {
    /// The sender's correlation id, echoed in the answer.
    pub(crate) id: u64,
    /// The JSON-RPC request itself, forwarded to the MCP handler unchanged.
    pub(crate) mcp: Value,
    /// The grant the sender says authorizes the request, when it sent one.
    pub(crate) grant: Option<GrantEnvelope>,
}

/// Why a frame was refused before the request could run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GrantRefusal {
    /// The frame carries no grant at all.
    NoGrant,
    /// The signature is absent, malformed, or not the pinned console key's.
    BadSignature,
    /// The grant is authentic but names a different device.
    DeviceMismatch,
    /// The grant is authentic but has expired, even allowing for clock skew.
    Expired,
    /// The grant is authentic and live, but does not cover the tool the request names.
    ToolNotCovered,
}

impl GrantRefusal {
    /// The closed-set name the audit line records.
    pub(crate) fn error_kind(self) -> &'static str {
        match self {
            GrantRefusal::NoGrant => "no_grant",
            GrantRefusal::BadSignature => "bad_grant",
            GrantRefusal::DeviceMismatch => "grant_device_mismatch",
            GrantRefusal::Expired => "grant_expired",
            GrantRefusal::ToolNotCovered => "tool_not_covered",
        }
    }

    /// The sentence the caller reads.
    pub(crate) fn message(self) -> &'static str {
        match self {
            GrantRefusal::NoGrant => "the frame carries no console grant",
            GrantRefusal::BadSignature => "the console grant signature does not verify",
            GrantRefusal::DeviceMismatch => "the console grant is for a different device",
            GrantRefusal::Expired => "the console grant has expired",
            GrantRefusal::ToolNotCovered => "tool not permitted by grant",
        }
    }
}

/// Admit one remote frame, or say why not.
///
/// The order is the console's, unchanged: the signature first, because nothing in the grant is
/// believed before the bytes are the console's; then the device it names; then its expiry; and last
/// the tool, which is only meaningful once the grant is authentic and live.
pub(crate) fn admit<'a>(
    frame: &'a RemoteFrame,
    device_id: &str,
    key: &VerifyingKey,
    now_ms: i64,
) -> Result<&'a GrantEnvelope, GrantRefusal> {
    let envelope = frame.grant.as_ref().ok_or(GrantRefusal::NoGrant)?;
    grant::verify(&envelope.grant, &envelope.signature, key)
        .map_err(|_| GrantRefusal::BadSignature)?;
    if envelope.grant.device_id != device_id {
        return Err(GrantRefusal::DeviceMismatch);
    }
    // The console signs an expiry from its clock, and the device reads it with a different clock:
    // subtracting `CLOCK_SKEW_MS` from "now" means a grant may run for up to five minutes past its
    // stated expiry. That is the deliberate cost of device clock drift — a device whose clock runs
    // slow must not refuse every frame at the stated instant — and it widens only the time window,
    // never the tools the grant names.
    let expired_at = i64::try_from(envelope.grant.expires_at_ms).unwrap_or(i64::MAX);
    if now_ms.saturating_sub(CLOCK_SKEW_MS) >= expired_at {
        return Err(GrantRefusal::Expired);
    }
    if let Some(tool) = requested_tool(&frame.mcp) {
        if !envelope.grant.tools.iter().any(|allowed| allowed == tool) {
            return Err(GrantRefusal::ToolNotCovered);
        }
    }
    Ok(envelope)
}

/// The tool a JSON-RPC `tools/call` names, when it names one.
///
/// Every other method — `initialize`, `ping`, `tools/list` — names no tool and is admitted by a
/// live grant alone; the grant's tool list is a permission on calls, not on the handshake.
pub(crate) fn requested_tool(request: &Value) -> Option<&str> {
    if request.get("method").and_then(Value::as_str) != Some("tools/call") {
        return None;
    }
    request
        .get("params")
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
}

/// A JSON-RPC error envelope for a refusal the handler did not produce.
pub(crate) fn jsonrpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    use super::*;
    use summrise_core::grant::{canonical_message, Grant};

    const SIGNING_SEED: [u8; 32] = [0x51; 32];

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn device_id() -> String {
        "ab".repeat(32)
    }

    fn signing() -> SigningKey {
        SigningKey::from_bytes(&SIGNING_SEED)
    }

    fn now_ms() -> i64 {
        crate::http::now_ms()
    }

    fn signed_by(key: &SigningKey, grant: Grant) -> GrantEnvelope {
        let signature = key.sign(canonical_message(&grant).as_bytes());
        GrantEnvelope {
            grant,
            signature: hex(&signature.to_bytes()),
        }
    }

    fn grant_for(device_id: &str, tools: &[&str], expires_at_ms: u64) -> GrantEnvelope {
        signed_by(
            &signing(),
            Grant {
                device_id: device_id.to_owned(),
                tools: tools.iter().map(|tool| (*tool).to_owned()).collect(),
                expires_at_ms,
                allow_destructive: false,
            },
        )
    }

    fn call(name: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": name, "arguments": {} },
        })
    }

    fn remote_frame(mcp: Value, grant: Option<GrantEnvelope>) -> RemoteFrame {
        RemoteFrame { id: 7, mcp, grant }
    }

    #[test]
    fn a_grant_the_pinned_key_signed_for_this_device_admits_its_tool() {
        let grant = grant_for(&device_id(), &["state.get"], now_ms() as u64 + 60_000);
        let frame = remote_frame(call("state.get"), Some(grant.clone()));

        assert_eq!(
            admit(&frame, &device_id(), &signing().verifying_key(), now_ms()),
            Ok(&grant)
        );
    }

    #[test]
    fn a_frame_with_no_grant_is_refused_by_name() {
        let frame = remote_frame(call("state.get"), None);

        assert_eq!(
            admit(&frame, &device_id(), &signing().verifying_key(), now_ms()),
            Err(GrantRefusal::NoGrant)
        );
    }

    #[test]
    fn a_grant_signed_by_another_key_is_refused() {
        let other = SigningKey::from_bytes(&[0x77; 32]);
        let grant = signed_by(
            &other,
            Grant {
                device_id: device_id(),
                tools: vec!["state.get".to_owned()],
                expires_at_ms: now_ms() as u64 + 60_000,
                allow_destructive: false,
            },
        );
        let frame = remote_frame(call("state.get"), Some(grant));

        assert_eq!(
            admit(&frame, &device_id(), &signing().verifying_key(), now_ms()),
            Err(GrantRefusal::BadSignature)
        );
    }

    #[test]
    fn a_grant_for_another_device_is_refused() {
        let grant = grant_for(&"cd".repeat(32), &["state.get"], now_ms() as u64 + 60_000);
        let frame = remote_frame(call("state.get"), Some(grant));

        assert_eq!(
            admit(&frame, &device_id(), &signing().verifying_key(), now_ms()),
            Err(GrantRefusal::DeviceMismatch)
        );
    }

    #[test]
    fn an_expired_grant_is_refused_and_the_skew_does_not_revive_it() {
        let stale = grant_for(&device_id(), &["state.get"], (now_ms() - 600_000) as u64);
        let frame = remote_frame(call("state.get"), Some(stale));
        assert_eq!(
            admit(&frame, &device_id(), &signing().verifying_key(), now_ms()),
            Err(GrantRefusal::Expired)
        );

        // Inside the skew: the console's clock may be ahead of this device's, so a grant that has
        // just expired by the device's reading is still admitted. The window is the deliberate
        // cost of two clocks, and it never widens the tools.
        let recent = grant_for(&device_id(), &["state.get"], (now_ms() - 60_000) as u64);
        let frame = remote_frame(call("state.get"), Some(recent));
        assert!(
            admit(&frame, &device_id(), &signing().verifying_key(), now_ms()).is_ok(),
            "a minute past the stated expiry is inside the five-minute allowance"
        );
    }

    #[test]
    fn a_tool_outside_the_grant_is_refused_by_name() {
        let grant = grant_for(&device_id(), &["files.read"], now_ms() as u64 + 60_000);
        let frame = remote_frame(call("state.get"), Some(grant));

        assert_eq!(
            admit(&frame, &device_id(), &signing().verifying_key(), now_ms()),
            Err(GrantRefusal::ToolNotCovered)
        );
    }

    #[test]
    fn a_method_that_names_no_tool_is_admitted_by_a_live_grant_alone() {
        let grant = grant_for(&device_id(), &["files.read"], now_ms() as u64 + 60_000);
        for mcp in [
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        ] {
            let frame = remote_frame(mcp.clone(), Some(grant.clone()));
            assert!(
                admit(&frame, &device_id(), &signing().verifying_key(), now_ms()).is_ok(),
                "{mcp}"
            );
        }
    }

    #[test]
    fn only_a_tools_call_with_a_non_empty_name_names_a_tool() {
        assert_eq!(requested_tool(&call("files.read")), Some("files.read"));
        assert_eq!(
            requested_tool(&call("")),
            None,
            "an empty name names nothing"
        );
        assert_eq!(
            requested_tool(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": { "name": 7 },
            })),
            None,
            "a name that is not a string names nothing"
        );
        assert_eq!(
            requested_tool(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })),
            None
        );
        assert_eq!(requested_tool(&json!({ "id": 1 })), None);
    }

    #[test]
    fn every_refusal_has_a_distinct_audit_kind_and_a_sentence() {
        let refusals = [
            GrantRefusal::NoGrant,
            GrantRefusal::BadSignature,
            GrantRefusal::DeviceMismatch,
            GrantRefusal::Expired,
            GrantRefusal::ToolNotCovered,
        ];
        let mut kinds: Vec<&str> = refusals
            .iter()
            .map(|refusal| refusal.error_kind())
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(
            kinds.len(),
            refusals.len(),
            "the audit kinds are a closed set with no collisions: {kinds:?}"
        );
        for refusal in refusals {
            assert!(
                !refusal.message().is_empty(),
                "{refusal:?} needs a sentence"
            );
        }
    }
}
