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
// Portable on purpose: the probe's and the helper channel's tests run in CI's Linux job, where the
// tray itself cannot.
#[cfg_attr(not(windows), allow(dead_code))]
mod health;
#[cfg_attr(not(windows), allow(dead_code))]
mod helper;
#[cfg_attr(not(windows), allow(dead_code))]
mod tray;

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

/// Whether this launch was asked to show the window (`--open`, from the shortcut or the Run key).
#[cfg_attr(not(windows), allow(dead_code))]
fn launches_window(mut args: impl Iterator<Item = String>) -> bool {
    args.any(|argument| argument == "--open")
}

#[cfg(windows)]
fn main() {
    let open = launches_window(std::env::args().skip(1));
    if app::already_running() {
        // The tray already exists; a second launch is the shortcut asking for the window. Open the
        // console in the browser rather than adding a second dot, so the desktop icon does something.
        if open {
            app::open_url(app::CONSOLE);
        }
        return;
    }
    note("starting");
    if let Err(error) = app::run(open) {
        note(&format!("exited: {error}"));
        eprintln!("summrise-shell: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::launches_window;

    #[test]
    fn the_open_parameter_is_the_window_request() {
        assert!(launches_window(["--open".to_owned()].into_iter()));
        assert!(!launches_window([].into_iter()));
        assert!(!launches_window(["--other".to_owned()].into_iter()));
    }
}
