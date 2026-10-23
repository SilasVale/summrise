//! The operator-side console: one service that authenticates the operator, keeps the device
//! registry, signs the grants a device verifies offline, and forwards an operator's MCP exchange
//! over the device's own outbound channel.
//!
//! The console is deliberately small. It is a broker, not a data path (design §5.2): it holds the
//! grant key, so a compromised console can mint grants, and the only thing that keeps that key
//! meaningful is that this service stays the operator's own. Storage is two atomic JSON files
//! (`devices.json` and `codes.json`) plus two append-only JSONL indexes (`audit.jsonl` for
//! authenticated actions and `denials.jsonl` for failed authentication); there is still no database.
//!
//! It also serves one embedded page at `/` (`page.html`, no build step): the operator's face on the
//! same routes. The page holds no secret and keeps the session it is given in memory.
//!
//! Everything testable is here in the library; `main.rs` is only the command line and the listener.

#![forbid(unsafe_code)]

mod audit;
mod auth;
mod channels;
mod codes;
mod config;
mod denials;
mod devices;
mod grants;
mod hex;
mod http;
mod jsonl;
mod setup;
mod store;

pub use config::{Config, ConfigError, StartupError, BIND_FLAG, DEFAULT_BIND, DEFAULT_PORT};
pub use devices::Device;
pub use grants::{
    canonical_message, Grant, GrantEnvelope, MAX_GRANT_LIFETIME_S, MIN_GRANT_LIFETIME_S,
};
pub use http::Console;
pub use setup::{run as setup, Setup, GRANT_SEED_FILE, OPERATOR_TOKEN_FILE};
