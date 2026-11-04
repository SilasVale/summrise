//! The operator-action index at `<data>/audit.jsonl`.
//!
//! One line per **authenticated** action: `{"ts_ms","kind","device_id?","status","detail?","tool?","token_id?"}`.
//! `kind` is a closed set — `enroll`, `register`, `grant`, `token`, `token_revoke`, `mcp`, and
//! `denied` for a refusal of a request whose caller proved a credential. `status` is the HTTP status
//! the caller saw, `detail` is a short reason: the console's wire error code for a refusal, or the
//! device's JSON-RPC error code when it answered with one. `tool` is the JSON-RPC tool name for a `tools/call`, and only the name:
//! the arguments are the operator's data and never reach this file. `token_id` is the permanent
//! token *record's* id when a token authenticated an MCP call — never the `srm_` secret; the grant
//! path, the operator's own `/mcp` route and every non-token action write no field.
//!
//! A request that fails authentication never reaches this file. Those refusals are an
//! unauthenticated caller's to generate at will, so they live in [`crate::denials`], rate-limited
//! and capped on their own; an unauthenticated flood therefore cannot rotate the audit history
//! away. Registration is the one proof outside an operator session: a registration that passes its
//! code and signature is an authenticated action, and a failed one is a denial.
//!
//! **No payload body is ever written.** The JSON-RPC request and response are the operator's data;
//! the index records that the exchange happened and whether it succeeded, never what it contained.
//! The one exception the design allows is the numeric error code, which is a fact about the exchange
//! and not its content.
//!
//! The file is capped at [`AUDIT_MAX_BYTES`] with one rotation to `audit.1.jsonl`; the discipline is
//! [`JsonLines`]'s.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::jsonl::JsonLines;

/// The largest the audit file may become before it is rotated to `audit.1.jsonl`.
pub(crate) const AUDIT_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// The current file's name under the data directory.
const AUDIT_FILE: &str = "audit.jsonl";

/// One audit line.
///
/// The absence of a body field is the design: the struct cannot carry one even by accident.
#[derive(Serialize)]
pub(crate) struct Entry<'a> {
    /// The console's clock when the action finished, in Unix milliseconds.
    pub(crate) ts_ms: u64,
    /// `enroll`, `register`, `grant`, `token`, `token_revoke`, `mcp` or `denied`.
    pub(crate) kind: &'static str,
    /// The device the action names, in canonical lowercase hex, when it names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) device_id: Option<&'a str>,
    /// The HTTP status the caller saw.
    pub(crate) status: u16,
    /// Why, for a refusal or a device error: a short code, never a body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<&'a str>,
    /// The tool a `tools/call` named. The name is an authorization fact; the arguments are a body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool: Option<&'a str>,
    /// The permanent token record that authenticated this call, by id — never the secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) token_id: Option<&'a str>,
}

/// The append-only audit index; the denial log reuses [`Entry`] as its line shape.
pub(crate) struct Audit {
    file: JsonLines,
    /// Every line as it is appended, for the live activity stream.
    ///
    /// A broadcast and not a queue: a page that falls behind is told it lagged and asks for the file's
    /// newest lines again, instead of the console holding a per-subscriber backlog nobody bounded.
    live: tokio::sync::broadcast::Sender<Value>,
}

/// How many appended lines the live stream may carry before a slow subscriber is told it lagged.
pub(crate) const LIVE_AUDIT_CAPACITY: usize = 256;

impl Audit {
    /// The audit file under `data`, capped at `max_bytes`.
    pub(crate) fn new(data: &Path, max_bytes: u64) -> Audit {
        let (live, _) = tokio::sync::broadcast::channel(LIVE_AUDIT_CAPACITY);
        Audit {
            file: JsonLines::new(data.join(AUDIT_FILE), max_bytes),
            live,
        }
    }

    /// Append one line.
    ///
    /// The live stream is published by the caller, not here: the sentence a person reads needs the
    /// call's arguments, which this file must never hold. [`Audit::publish`] is the one door.
    pub(crate) async fn append(&self, entry: &Entry<'_>) -> bool {
        self.file.append(entry).await
    }

    /// Publish one live event (already humanised) to every open `/events`. No subscriber is fine: the
    /// stream is for whoever is watching, and nobody watching is not an error.
    pub(crate) fn publish(&self, value: Value) {
        let _ = self.live.send(value);
    }

    /// Follow the audit as it is written. Subscribe BEFORE reading the newest lines: a line written
    /// between the two would otherwise be in neither the snapshot nor the stream.
    pub(crate) fn follow(&self) -> tokio::sync::broadcast::Receiver<Value> {
        self.live.subscribe()
    }

    /// The newest `limit` lines, newest first.
    pub(crate) async fn newest(&self, limit: usize) -> Vec<Value> {
        self.file.newest(limit).await
    }
}
