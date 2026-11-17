//! The device's decisions as values.
//!
//! `summrise-core` is the half of the device that can be tested anywhere: it opens no socket, reads no
//! file, starts no process and asks no clock. Windows behaviour lives in the two runtimes that depend
//! on it, so a rule that decides who may act, what a session may become, or what the state document
//! means is proven on the build machine before it reaches a device. The update decision is **not**
//! here: it lives in `summrise-release` (`select`), the crate the wired downloader calls.
#![forbid(unsafe_code)]

pub mod auth;
pub mod capabilities;
pub mod event;
pub mod grant;
pub mod manifest;
pub mod mesh;
pub mod plugins;
pub mod registry;
pub mod repair_task;
pub mod session;
pub mod state;
pub mod txn;
mod update;

pub use auth::{
    authorize, ApprovalState, Ctx, Decision, DenyReason, Grant, Subject, ToolCall, Transport,
};
pub use event::{Family, RowKind};
pub use manifest::{Manifest, Version, VersionError};
pub use mesh::{backoff, GiveUpReason, Path, PathAction, PathEvent, PathState, RelayReason};
pub use registry::{
    is_destructive, session_arg, tool, txn_policy, Capability, ToolDef, TxnPolicy, BOOTSTRAP_TXNS,
    CAPABILITIES, EXEMPT_READ_TXNS, WORKING_TXN_ARG,
};
pub use session::{
    may_write_goal, transition, validate_goal, Goal, GoalError, Holder, HolderKind,
    TransitionError, GOAL_MAX_CHARS,
};
pub use state::{
    Baseline, Install, LastVerdict, RolledBackArtifact, Service, ServiceState, SessionKind,
    SessionRow, SessionState, State, StateError, Update, UpdateCheck, UpdateCheckTrigger,
    UpdateOutcome, UpdatePhase, VerdictOutcome, MAX_ROLLED_BACK, SCHEMA_VERSION,
};
pub use txn::{
    may_begin, may_end, SessionRef, Txn, TxnError, TxnId, TxnState, MAX_OPEN_TXNS,
    MAX_RETAINED_TXNS, TXN_PURPOSE_MAX_CHARS, TXN_REASON_MAX_CHARS,
};
