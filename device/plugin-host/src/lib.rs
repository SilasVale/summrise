//! The plugin host (design §8.2): a plugin is an MCP server, started as a child process, whose tools
//! are advertised under one namespace behind the device's one endpoint.
//!
//! This crate is the whole supervised subsystem as one compile-time unit: [`logs`] owns the bounded
//! file a plugin writes to, [`session`] owns the stdio JSON-RPC conversation and is generic over its
//! transport, and [`host`] owns lifecycle, restart and the registry file. It names `summrise-core`
//! (the manifest rules) and `summrise-bus` (where lifecycle events are published) and nothing above
//! them; the device app re-exports the crate as its `plugins` module.
//!
//! The protocol implementation is one code path: the tests run the real [`session::Session`] against
//! `tokio::io::duplex`, not a second implementation of the handshake.

mod fsutil;

pub mod host;
pub mod logs;
pub mod session;

// The scripted launcher is also driven by the app's MCP tests, and a dependency's `cfg(test)` is not
// the app's. The feature is enabled from the device's `[dev-dependencies]`, so a shipped build never
// compiles it.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
