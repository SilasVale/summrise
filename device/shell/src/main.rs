//! The Summrise tray and window.
//!
//! A Windows user's eye-level view of one device: a dot that says whether the app is serving, and one
//! click to the operator's console (or, when the console is unreachable, the device's own panel).
//!
//! The device itself is two runtimes with no UI: `summrise-launch` supervises `summrise-device`, and
//! every action is granted and audited by the console. This binary is deliberately outside that
//! boundary — it holds no credential and can do nothing but read a loopback health document and open
//! a URL. That is what lets it exist without becoming a second update path or a second authorization
//! surface.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("summrise-shell: the tray and the window are Windows-only");
    std::process::exit(1);
}

#[cfg(windows)]
mod app;
// Portable on purpose: the probe's tests run in CI's Linux job, where the tray cannot.
#[cfg_attr(not(windows), allow(dead_code))]
mod health;

#[cfg(windows)]
fn main() {
    if app::already_running() {
        // The icon is already in the tray; a second one would be a second dot saying the same thing.
        return;
    }
    if let Err(error) = app::run() {
        eprintln!("summrise-shell: {error}");
        std::process::exit(1);
    }
}
