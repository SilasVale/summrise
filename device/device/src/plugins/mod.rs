//! The plugin host (design §8.2): a plugin is an MCP server, started as a child process, whose tools
//! are advertised under one namespace behind the device's one endpoint.
//!
//! The module is layered so each layer can be proven without the next: [`logs`] owns the bounded file
//! a plugin writes to, [`session`] owns the stdio JSON-RPC conversation and is generic over its
//! transport, and the supervisor (added with the host) owns lifecycle, restart and the registry file.
//! The protocol implementation is one code path: the tests run the real [`session::Session`] against
//! `tokio::io::duplex`, not a second implementation of the handshake.

pub mod host;
pub mod logs;
pub mod session;

#[cfg(test)]
pub(crate) mod test_support;
