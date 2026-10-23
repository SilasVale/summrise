//! The operator-action index at `<data>/audit.jsonl`.
//!
//! One line per **authenticated** action: `{"ts_ms","kind","device_id?","status","detail?","tool?"}`.
//! `kind` is a closed set — `enroll`, `register`, `grant`, `mcp`, and `denied` for a refusal of a
//! request whose caller proved a credential. `status` is the HTTP status the caller saw, `detail` is
//! a short reason: the console's wire error code for a refusal, or the device's JSON-RPC error code
//! when it answered with one. `tool` is the JSON-RPC tool name for a `tools/call`, and only the name:
//! the arguments are the operator's data and never reach this file.
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
    /// `enroll`, `register`, `grant`, `mcp` or `denied`.
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
}

/// The append-only audit index; the denial log reuses [`Entry`] as its line shape.
pub(crate) struct Audit {
    file: JsonLines,
}

impl Audit {
    /// The audit file under `data`, capped at `max_bytes`.
    pub(crate) fn new(data: &Path, max_bytes: u64) -> Audit {
        Audit {
            file: JsonLines::new(data.join(AUDIT_FILE), max_bytes),
        }
    }

    /// Append one line.
    pub(crate) async fn append(&self, entry: &Entry<'_>) -> bool {
        self.file.append(entry).await
    }

    /// The newest `limit` lines, newest first.
    pub(crate) async fn newest(&self, limit: usize) -> Vec<Value> {
        self.file.newest(limit).await
    }
}
