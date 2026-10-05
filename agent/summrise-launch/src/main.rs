//! The staged half: start a process with **no console window**, and **do not wait**.
//!
//! Invoked by a scheduled task's action — `SummriseDesktop` used to run `wscript.exe <desktop-pulse.vbs>`
//! and `SummrisePlaywright` used to run `wscript.exe <run-hidden.vbs>` — with the program and its
//! arguments as this process's own arguments. The decision half is in the library; this file is the
//! platform call and the failure record.
//!
//! # Why a GUI subsystem, and why `CREATE_NO_WINDOW` as well
//!
//! `#![windows_subsystem = "windows"]` means **this** process never allocates a console. It does not
//! stop a **child** console application from allocating one, which is the flash the `.vbs` wrappers
//! existed to prevent — so the child is created with `CREATE_NO_WINDOW` too. Two different consoles,
//! two different flags, and a wrapper that sets only the first still flashes.
//!
//! # Why it does not wait
//!
//! `WScript.Shell.Run(command, 0, False)` — the `False` is "do not wait". The scheduled task is
//! finished the moment the target is started, exactly as it was with `wscript.exe`, so a long-running
//! target cannot hold the task open and make the next trigger skip.
//!
//! # Exit codes
//!
//! `0` started · `2` nothing to start · `126` the target could not be started. **`0` DOES NOT MEAN THE
//! TARGET SUCCEEDED** — nothing here waits for it, so nothing here can know.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
use std::io::Write;
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::process::Stdio;

#[cfg(windows)]
use summrise_launch::{failure_line, log_path, parse, Launch};

/// Append one line to `<install>\logs\launcher.log`, and **never fail because of it**.
///
/// A GUI-subsystem process has no console, so this is the only place a failure can be recorded. It is
/// best-effort on purpose: a launcher that refuses to start a process because it could not write a log
/// line about it would be worse than the silence it is trying to replace.
#[cfg(windows)]
fn record(line: &str) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let path: PathBuf = log_path(&exe);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// `CREATE_NO_WINDOW` — the flag that keeps the CHILD from allocating the console this process does
/// not have. `0x0800_0000` is its value; it is spelled out rather than pulled from a crate because
/// this crate has no dependencies, deliberately.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
fn main() {
    use std::os::windows::process::CommandExt;

    let parsed = parse(std::env::args_os().skip(1));
    let launch: Launch = match parsed {
        Ok(l) => l,
        Err(e) => {
            record(&failure_line(None, &e.to_string()));
            std::process::exit(2);
        }
    };

    let spawned = std::process::Command::new(&launch.program)
        .args(&launch.args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    match spawned {
        // **NO `wait()`.** The task is done; the target owns its own life from here.
        Ok(_child) => std::process::exit(0),
        Err(e) => {
            record(&failure_line(Some(&launch), &e.to_string()));
            std::process::exit(126);
        }
    }
}

/// The host build exists so `cargo fmt`/`clippy`/`test` see this crate in the workspace. **IT IS NOT A
/// SHIM FOR THE WINDOWS PATH** — it launches nothing, and says so.
#[cfg(not(windows))]
fn main() {
    eprintln!(
        "summrise-launch is staged onto a Windows machine; on this host it starts nothing. \
         Its decisions are tested in the library."
    );
    std::process::exit(126);
}
