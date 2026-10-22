//! The device's decisions as values.
//!
//! `summrise-core` is the half of the device that can be tested anywhere: it opens no socket, reads no
//! file, starts no process and asks no clock. Windows behaviour lives in the two runtimes that depend
//! on it, so a rule that decides who may act, what a session may become, or what the state document
//! means is proven on the build machine before it reaches a device. The update decision is **not**
//! here: it lives in `summrise-release` (`select`), the crate the wired downloader calls.
#![forbid(unsafe_code)]

pub mod auth;
pub mod grant;
pub mod manifest;
pub mod mesh;
pub mod registry;
pub mod session;
pub mod state;
mod update;

pub use auth::{
    authorize, ApprovalState, Ctx, Decision, DenyReason, Grant, Subject, ToolCall, Transport,
};
pub use manifest::{Manifest, Version, VersionError};
pub use mesh::{backoff, GiveUpReason, Path, PathAction, PathEvent, PathState, RelayReason};
pub use registry::{is_destructive, tool, Capability, ToolDef, CAPABILITIES};
pub use session::{transition, TransitionError};
pub use state::{
    Baseline, Install, LastVerdict, RolledBackArtifact, Service, ServiceState, SessionKind,
    SessionRow, SessionState, State, StateError, Update, UpdateOutcome, UpdatePhase,
    VerdictOutcome, MAX_ROLLED_BACK, SCHEMA_VERSION,
};
