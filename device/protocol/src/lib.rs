//! The wire between a console and a device: the frame a console sends, and the answer it gets back.
//!
//! **ONE TYPE PER FIELD, AND BOTH SIDES READ IT.** The device parses [`Frame`]; the console builds it
//! and parses [`Answer`]. A field renamed here is a field renamed on both sides, and a golden fixture
//! (see `tests/`) fails before a build can ship a wire only one side understands.
//!
//! The frame is `{"id", "mcp"|...}`, and the answer is `{"id", "result"|"error", "journal_seq"?}`.
//! Optional members are omitted, not sent as `null`: a `null` is a value a reader cannot tell from a
//! field it has not learned yet.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

pub use summrise_core::grant::GrantEnvelope;

/// The frame a remote transport carries from a console to a device.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../console/web/src/protocol/")]
pub struct Frame {
    /// The sender's correlation id, echoed in the answer.
    #[ts(type = "number")]
    pub id: u64,
    /// The JSON-RPC request itself, forwarded to the MCP handler unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "unknown")]
    pub mcp: Option<Value>,
    /// "Keep sending this session's output until I say stop."
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub subscribe: Option<Subscribe>,
    /// "Also push this device's own events from this sequence."
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub events: Option<EventsSubscribe>,
    /// "Stop the subscription with this id."
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub unsubscribe: Option<Unsubscribe>,
    /// The grant the sender says authorizes the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "unknown")]
    pub grant: Option<GrantEnvelope>,
    /// The console's permanent-token scope, when the frame carries one instead of a grant.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub token: Option<TokenScope>,
    /// The `trace.call` the console minted for this exchange.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub trace: Option<String>,
}

impl Frame {
    /// A plain call's frame.
    pub fn call(id: u64, mcp: Value) -> Self {
        Self {
            id,
            mcp: Some(mcp),
            subscribe: None,
            events: None,
            unsubscribe: None,
            grant: None,
            token: None,
            trace: None,
        }
    }

    /// A subscription to this device's own events, from `from_seq` (0 replays what is retained).
    pub fn events(id: u64, from_seq: u64) -> Self {
        Self {
            id,
            mcp: None,
            subscribe: None,
            events: Some(EventsSubscribe { from_seq }),
            unsubscribe: None,
            grant: None,
            token: None,
            trace: None,
        }
    }

    /// A subscription to a terminal session, and the `terminal.read` that authorizes it.
    pub fn follow(id: u64, terminal_read: Value, session_id: String, from: u64) -> Self {
        Self {
            id,
            mcp: Some(terminal_read),
            subscribe: Some(Subscribe { session_id, from }),
            events: None,
            unsubscribe: None,
            grant: None,
            token: None,
            trace: None,
        }
    }

    /// The end of the subscription whose frame id was `sub`.
    pub fn unsubscribe(id: u64, sub: u64) -> Self {
        Self {
            id,
            mcp: None,
            subscribe: None,
            events: None,
            unsubscribe: Some(Unsubscribe { sub }),
            grant: None,
            token: None,
            trace: None,
        }
    }
}

/// A subscription: which session to follow, and the byte offset to start from.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../console/web/src/protocol/")]
pub struct Subscribe {
    /// The terminal session id `terminal.create` returned.
    pub session_id: String,
    /// The byte offset the subscriber already has; 0 replays everything retained.
    #[serde(default)]
    #[ts(type = "number")]
    pub from: u64,
}

/// Where a device-event subscription starts.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../console/web/src/protocol/")]
pub struct EventsSubscribe {
    /// The first sequence to deliver; 0 replays whatever the journal still retains.
    #[serde(default)]
    #[ts(type = "number")]
    pub from_seq: u64,
}

/// The end of a subscription.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../console/web/src/protocol/")]
pub struct Unsubscribe {
    /// The id of the `subscribe` frame this ends.
    #[ts(type = "number")]
    pub sub: u64,
}

/// A permanent token's scope, as the console's authenticated channel forwards it.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../console/web/src/protocol/")]
pub struct TokenScope {
    /// The token record's id (never the `srm_` secret), for the audit line.
    pub token_id: String,
    /// The device the console resolved the token to.
    pub device_id: String,
    /// The registry tools the token allows.
    pub scopes: Vec<String>,
    /// Whether the token's record consented to destructive tools.
    pub allow_destructive: bool,
    /// The token's expiry in Unix milliseconds, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub expires_at_ms: Option<u64>,
}

/// The answer a device sends back for a frame.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../console/web/src/protocol/")]
pub struct Answer {
    /// The correlation id the frame carried.
    #[ts(type = "number")]
    pub id: u64,
    /// The machine journal's `seq` for the outcome half, when the device named one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub journal_seq: Option<u64>,
    /// The JSON-RPC body: exactly one of `result` and `error`.
    #[serde(flatten)]
    pub body: Body,
}

/// The JSON-RPC body of an answer.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export, export_to = "../../console/web/src/protocol/")]
pub enum Body {
    /// A successful call's body.
    Result {
        #[ts(type = "unknown")]
        result: Value,
    },
    /// A failed call's body.
    Error {
        #[ts(type = "unknown")]
        error: Value,
    },
}
