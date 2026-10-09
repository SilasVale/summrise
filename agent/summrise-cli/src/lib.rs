//! THE SUMMRISE CLI, IN RUST — THE WHOLE THING, NOT THE DECISIONS ONLY.
//!
//! The TypeScript at `agent/summrise-agent-npm/src/summrise.ts` is the source this was ported from,
//! and `agent/summrise-agent-npm/test/cli.test.mjs` (68 cases) was the oracle while both existed.
//! This crate is landing 4 of
//! `docs/superpowers/specs/2026-10-08-every-decision-is-rust-design.md`: **4a** moved the decisions,
//! **4b** moved the platform half (`monitor`, `uninstall`, `run`, `tunnel`, the staging and the
//! swap), repointed the npm `bin` at this binary and deleted the TypeScript.
//!
//! **WHAT IS HERE**: command dispatch and argv parsing ([`dispatch`]), version comparison
//! ([`update`]), install/data directory resolution ([`paths`]), the config it reads ([`config`]),
//! component resolution and sha256 verification ([`components`]), the release-channel comparison and
//! the update decision ([`update`]), the status report's content ([`status`]), the machine-readable
//! output ([`monitors`]), every pure PowerShell-generating helper the oracle tests ([`psgen`],
//! [`ps`]), the device and gateway doors ([`device`]), the reachability instrument ([`monitor`]), the
//! install ([`setup`]), the staging and the swap ([`swap`]), the tunnel ([`tunnel`]) and the
//! teardown ([`uninstall`]).
//!
//! **EVERY PLATFORM EFFECT IS BEHIND [`host::Host`].** There is deliberately no
//! "run this command line" method on it: `ps()` in the TypeScript existed because a PowerShell script
//! had to be passed as ONE argv element, and the `psArgv` incident is what happens when a shell
//! re-parses it. A trait that offers only `run(argv)`, plus two NAMED operations for the
//! `cmd.exe` builtins that take no argv, makes that defect unrepresentable rather than merely tested
//! for — and the oracle's two source-scanning cases ("every `ps()` script is passed as argv", "every
//! interpolated value sits inside a double-quoted region of the cmd line") are replaced by the
//! construction.

pub mod components;
pub mod config;
pub mod device;
pub mod dispatch;
pub mod endpoints;
pub mod host;
pub mod monitor;
pub mod monitors;
pub mod paths;
pub mod ps;
pub mod psgen;
pub mod rollback;
pub mod setup;
pub mod sha256;
pub mod status;
pub mod swap;
pub mod testing;
pub mod tunnel;
pub mod uninstall;
pub mod update;
pub mod version;
