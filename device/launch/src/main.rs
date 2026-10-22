//! `summrise-launch` — the Windows entry point.
//!
//! The whole supervisor lives in [`win`], behind `cfg(windows)`. The Linux build of this package is
//! deliberately inert: its job is to compile the pure half and run its tests, not to pretend a
//! Windows service can be exercised from a build machine. `cargo xwin check` is what proves the
//! other half compiles.

#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    win::entry()
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!(
        "summrise-launch: the supervisor is a Windows service; this build carries the decisions and their tests"
    );
    std::process::ExitCode::from(1)
}
