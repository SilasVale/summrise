//! THE SUMMRISE CLI'S DECISIONS, IN RUST.
//!
//! The TypeScript at `agent/summrise-agent-npm/src/summrise.ts` is the source this was ported from,
//! and `agent/summrise-agent-npm/test/cli.test.mjs` (68 cases) is the oracle. This crate is the
//! first slice of landing 4 of `docs/superpowers/specs/2026-10-08-every-decision-is-rust-design.md`:
//! the DECISIONS move here, every PLATFORM EFFECT stays behind [`host::Host`], and the npm `bin` is
//! deliberately NOT repointed yet — the port is proven first and cut over after, which is this
//! repository's own pattern for the file relay.
//!
//! **WHAT IS HERE**: command dispatch and argv parsing ([`dispatch`]), version comparison
//! ([`update`]), install/data directory resolution ([`paths`]), the config it reads
//! ([`config`]), component resolution and sha256 verification ([`components`]), the release-channel
//! comparison and the update decision ([`update`]), the status report's content ([`status`]), the
//! machine-readable output ([`monitors`]), and every pure PowerShell-generating helper the oracle
//! tests ([`psgen`], [`ps`]).
//!
//! **WHAT IS NOT**, named rather than implied: the bodies of `setup`/`update`/`rollback`/
//! `uninstall`/`tunnel`/`monitor` beyond their decisions, and the Windows-only staging effects. The
//! report that lands with this slice lists each one with its reason.

pub mod components;
pub mod config;
pub mod dispatch;
pub mod endpoints;
pub mod host;
pub mod monitors;
pub mod paths;
pub mod ps;
pub mod psgen;
pub mod sha256;
pub mod status;
pub mod testing;
pub mod update;
pub mod version;
