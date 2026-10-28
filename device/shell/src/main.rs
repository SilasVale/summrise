//! The Summrise tray and window.
//!
//! A Windows user's eye-level view of one device: a dot that says whether the app is serving, and one
//! click to the operator's console (or, when the console is unreachable, the device's own panel).
//!
//! **This binary is installer-owned.** The canary update swaps `summrise-device.exe` and nothing else, so
//! a change here reaches a machine through `SummriseSetup`, not through a release document; docs/RELEASE.md
//! carries the procedure and the reason. Its startup lines go to `%LOCALAPPDATA%\Summrise\shell.log`,
//! because a tray that does not appear is otherwise a process with no message.
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
// Portable on purpose: the probe's and the helper channel's tests run in CI's Linux job, where the
// tray itself cannot.
#[cfg_attr(not(windows), allow(dead_code))]
mod health;
#[cfg_attr(not(windows), allow(dead_code))]
mod helper;

/// A GUI-subsystem process has no console, so a failure that would otherwise be an `eprintln!` goes
/// to a file beside the browser profile: the tray is either there or it is not, and this says which.
pub fn note(line: &str) {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else {
        return;
    };
    let directory = std::path::PathBuf::from(base).join("Summrise");
    let _ = std::fs::create_dir_all(&directory);
    let path = directory.join("shell.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = writeln!(file, "{line}");
    }
}

#[cfg(windows)]
fn main() {
    // A DIAGNOSTIC THAT OPENS THE WINDOW AND EXITS. The tray menu is a click on a machine this project
    // is often not sitting at, so "Open console does nothing" was reported twice without a way to
    // separate the menu wiring from the window: `--check-window` builds the same window and WebView2
    // view the menu builds, waits for the page to load, and answers on the log and the exit code.
    if std::env::args().any(|argument| argument == "--check-window") {
        let code = app::check_window();
        std::process::exit(code);
    }
    if app::already_running() {
        // A second instance is the session-reconnect task asking for the icon back, not a user
        // double-clicking: signal the running instance to re-add the tray, and only then exit.
        if app::signal_wake() {
            note("another instance owns the tray; asked it to re-add the icon");
        } else {
            note("another instance owns the tray and did not answer the wake signal");
        }
        return;
    }
    note("starting");
    if let Err(error) = app::run() {
        note(&format!("exited: {error}"));
        eprintln!("summrise-shell: {error}");
        std::process::exit(1);
    }
}
