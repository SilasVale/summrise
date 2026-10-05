//! The decision half of `summrise-launch`, kept here so it can be tested on a host.
//!
//! # What this replaces, and what it must therefore do
//!
//! Two `.vbs` files, both generated at install time into `<install>\scripts\`, both run by
//! `wscript.exe`:
//!
//! ```text
//! desktop-pulse.vbs   CreateObject("WScript.Shell").Run "powershell … -File ""…\ensure-desktop.ps1""", 0, False
//! run-hidden.vbs      cmd = chr(34) & WScript.Arguments(0) & chr(34) & " " & chr(34) & WScript.Arguments(1) & chr(34)
//!                     For i = 2 To WScript.Arguments.Count - 1 : cmd = cmd & " " & WScript.Arguments(i) : Next
//!                     sh.Run cmd, 0, False
//! ```
//!
//! **THE FACILITY IS ONE WINDOWS CALL**: `WScript.Shell.Run(command, 0, False)` — window style **0**
//! (hidden) and **False** (do not wait). A Rust binary with `#![windows_subsystem = "windows"]` never
//! allocates a console of its own, and `CreateProcessW` with `CREATE_NO_WINDOW` gives the child none
//! either, so the same two properties come out of the platform rather than out of a script host.
//!
//! # The quoting rule is NOT copied, and that is deliberate
//!
//! `run-hidden.vbs` quotes `argv[0]` and `argv[1]` and appends every later argument **raw**. That is a
//! quoting bug wearing a wrapper's clothes: an argument containing a space, or a quote, changes the
//! command line the child parses. `std::process::Command` applies the platform's own quoting rules to
//! every argument, so this port **fixes** it rather than reproducing it — and the fix is named here
//! because "faithful" and "identical" are different words, and a port that copies a bug is not
//! faithful to the thing the bug was in.
//!
//! # Exit codes, chosen to be distinguishable in a caller's log rather than to be pretty
//!
//! * `0`   — the target was STARTED. The launcher does not wait, so this is not its exit status.
//! * `2`   — no target was named.
//! * `126` — the target exists but could not be started.
//!
//! **THE ONE THING THE `.vbs` DID THAT THIS MUST NOT LOSE**: it never showed anything. A
//! GUI-subsystem process has no console to print to, so a failure is written to
//! `<install>\logs\launcher.log` — because a wrapper that fails silently is how a scheduled task
//! stops doing its job for a week without anybody noticing.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// What to start: the program, and the arguments that follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: OsString,
    pub args: Vec<OsString>,
}

/// Why there is nothing to launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// No arguments at all: `summrise-launch` was invoked with nothing to run.
    NoTarget,
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::NoTarget => write!(
                f,
                "summrise-launch was invoked with no arguments, so there is nothing to start"
            ),
        }
    }
}

/// Split an argument list — **already stripped of `argv[0]`** — into a program and its arguments.
///
/// The split is the whole decision: `args[0]` is the program, everything after it is passed through
/// untouched. **NOTHING IS RE-QUOTED HERE**, because there is no command line to build: the arguments
/// reach `CreateProcessW` as an argv, and the platform quotes them once, correctly.
pub fn parse<I>(args: I) -> Result<Launch, LaunchError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut it = args.into_iter();
    match it.next() {
        None => Err(LaunchError::NoTarget),
        Some(program) => Ok(Launch {
            program,
            args: it.collect(),
        }),
    }
}

/// `<exe_dir>\..\logs\launcher.log` — the install layout's own place for this.
///
/// The launcher lives in `<install>\scripts\`, which is layout v2's home for launchers, so the
/// install root is one directory up. **DERIVED FROM THE EXE'S OWN PATH RATHER THAN PASSED IN**: a
/// scheduled task's action carries a program and its arguments and nothing else, so a path the
/// launcher cannot work out for itself is a path it does not have.
pub fn log_path(exe: &Path) -> PathBuf {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let root = dir.parent().unwrap_or(dir);
    root.join("logs").join("launcher.log")
}

/// The line written when something could not be started. One line, no newline, no timestamp —
/// **the caller stamps it**, because a clock read here would be a second opinion about the time.
pub fn failure_line(launch: Option<&Launch>, error: &str) -> String {
    match launch {
        Some(l) => {
            let mut line = format!("could not start {:?}", l.program);
            for a in &l.args {
                line.push(' ');
                line.push_str(&format!("{a:?}"));
            }
            line.push_str(": ");
            line.push_str(error);
            line
        }
        None => format!("nothing to start: {error}"),
    }
}

/// True when the string looks like it came from `wscript.exe` running one of the two wrappers this
/// binary replaces. Used by the installer's migration to decide whether a scheduled task still points
/// at the old path — **and it is here, in the tested half, rather than in a PowerShell one-liner.**
pub fn is_the_old_wrapper(action: &OsStr) -> bool {
    let s = action.to_string_lossy().to_ascii_lowercase();
    s.ends_with("wscript.exe") || s.ends_with(".vbs")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_first_argument_is_the_program_and_the_rest_are_its_arguments() {
        let l = parse(v(&[
            "powershell.exe",
            "-NoProfile",
            "-File",
            r"C:\x\ensure-desktop.ps1",
        ]))
        .expect("a target");
        assert_eq!(l.program, OsString::from("powershell.exe"));
        assert_eq!(
            l.args,
            v(&["-NoProfile", "-File", r"C:\x\ensure-desktop.ps1"])
        );
    }

    #[test]
    fn no_arguments_is_an_error_and_not_an_empty_launch() {
        // **THE CASE THE `.vbs` SILENTLY IGNORED**: `sh.Run ""` raises nothing a scheduler records.
        assert_eq!(parse(Vec::<OsString>::new()), Err(LaunchError::NoTarget));
    }

    #[test]
    fn an_argument_containing_a_space_or_a_quote_survives_untouched() {
        // **THIS IS THE QUOTING BUG THE `.vbs` HAD.** It quoted argv[0] and argv[1] and appended the
        // rest raw, so a third argument like `a b` reached the child as two arguments. Here the
        // argument is carried as ONE `OsString` all the way to `CreateProcessW`, which quotes it.
        let l = parse(v(&[
            "node.exe",
            "playwright-mcp",
            "--config",
            r#"{"a": "b c"}"#,
            "with space",
        ]))
        .expect("a target");
        assert_eq!(l.args.len(), 4);
        assert_eq!(l.args[2], OsString::from(r#"{"a": "b c"}"#));
        assert_eq!(l.args[3], OsString::from("with space"));
    }

    #[test]
    fn the_log_sits_beside_the_install_root_not_inside_scripts() {
        // **BUILT WITH `push`, NOT WRITTEN AS A WINDOWS LITERAL.** The first version asserted against
        // `r"C:\\Program Files\\..."`, which on the LINUX host that runs `cargo test` is a SINGLE path
        // component — backslash is not a separator there — so `parent()` answered `""` and the test
        // failed while the CODE was right. A test that only holds on one platform reports the host,
        // not the behaviour.
        let exe = PathBuf::from("install")
            .join("scripts")
            .join("summrise-launch.exe");
        assert_eq!(
            log_path(&exe),
            PathBuf::from("install").join("logs").join("launcher.log")
        );
        // And the Windows shape is asserted by its COMPONENTS rather than by its spelling.
        let root = Path::new(r"C:\Program Files\Summrise");
        let win = root.join("scripts").join("summrise-launch.exe");
        assert_eq!(log_path(&win), root.join("logs").join("launcher.log"));
    }

    #[test]
    fn the_old_wrapper_is_recognised_in_both_of_its_shapes() {
        // The scheduled task's action is `wscript.exe` with the `.vbs` as its ARGUMENT, so the
        // migration has to recognise either one — a device that was half-migrated has one of each.
        assert!(is_the_old_wrapper(OsStr::new(
            r"C:\Windows\System32\wscript.exe"
        )));
        assert!(is_the_old_wrapper(OsStr::new(
            r"C:\Program Files\Summrise\scripts\desktop-pulse.vbs"
        )));
        assert!(is_the_old_wrapper(OsStr::new("WSCRIPT.EXE")));
        assert!(!is_the_old_wrapper(OsStr::new(
            r"C:\Program Files\Summrise\scripts\summrise-launch.exe"
        )));
    }

    #[test]
    fn a_failure_line_names_the_program_and_the_arguments() {
        let l = parse(v(&["node.exe", "--version"])).expect("a target");
        let line = failure_line(Some(&l), "The system cannot find the file specified.");
        assert!(line.contains("node.exe"), "{line}");
        assert!(line.contains("--version"), "{line}");
        assert!(line.contains("cannot find the file"), "{line}");
        // And the no-target case says so rather than printing an empty program.
        let line = failure_line(None, "no arguments");
        assert!(line.starts_with("nothing to start:"), "{line}");
    }
}
