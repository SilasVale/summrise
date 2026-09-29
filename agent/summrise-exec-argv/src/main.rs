//! The staged half of the remote subprocess seam.
//!
//! Invoked by `sshd` with a **fixed** command line — the argv it must run arrives on stdin as a
//! length-prefixed frame (see the library for the format and for why it exists at all). On success
//! this process is replaced by the target: the helper never forks, never waits, and never buffers
//! the child's output, because the seam wants the child's own exit status and its own descriptors.
//!
//! Exit codes, chosen to be distinguishable in a caller's log rather than to be pretty:
//!
//! * `125` — the frame was malformed, so nothing was executed. The reason is on stderr.
//! * `126` — the target exists but could not be executed (permissions, a bad interpreter).
//! * `127` — the target was not found on the far side's `PATH`.
//!
//! Anything else is the target's own exit status, because on success there is no helper left to
//! report one.

#[cfg(unix)]
fn main() {
    use std::ffi::OsStr;
    use std::io::Write;
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    // The frame must be read from the RAW descriptor, not from `std::io::stdin()`: that handle is
    // buffered, and a buffered reader would pull bytes belonging to the child's stdin into its own
    // buffer, where the exec'd target can never see them. `File` reads are unbuffered, and
    // `read_frame` stops exactly at the frame's last byte.
    let mut stdin = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(0) });

    let frame = match summrise_exec_argv::read_frame(&mut *stdin) {
        Ok(frame) => frame,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "summrise-exec-argv: {e}");
            std::process::exit(125);
        }
    };

    // The working directory is entered here rather than carried in the exec string, because the exec
    // string passes through the login shell and a workspace path is caller-supplied. A failure is a
    // helper-level failure — the command did not run, and reporting it as if it had would be worse
    // than not running it at all.
    if let Some(directory) = &frame.cwd {
        let path = OsStr::from_bytes(directory);
        if let Err(e) = std::env::set_current_dir(path) {
            let _ = writeln!(
                std::io::stderr(),
                "summrise-exec-argv: cannot enter working directory {path:?}: {e}"
            );
            std::process::exit(126);
        }
    }

    let argv = frame.argv;
    let program = OsStr::from_bytes(&argv[0]);
    let mut command = Command::new(program);
    for element in &argv[1..] {
        command.arg(OsStr::from_bytes(element));
    }

    // `exec` replaces this process, so the target inherits the descriptors `sshd` gave us — with
    // stdin already positioned past the frame. It returns only on failure.
    let error = command.exec();
    let _ = writeln!(
        std::io::stderr(),
        "summrise-exec-argv: cannot execute {:?}: {error}",
        program
    );
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => 127,
        _ => 126,
    };
    std::process::exit(code);
}

/// This helper is a POSIX `execve` shim; there is nothing for it to do on a platform without one.
/// It is compiled on Windows only so that a workspace-wide build does not fail — the binary is never
/// staged onto a Windows machine, where the local subprocess seam already runs natively.
#[cfg(not(unix))]
fn main() {
    eprintln!(
        "summrise-exec-argv: this helper executes an argv via execve and exists only on unix targets; \
         on this platform the local subprocess seam is used instead"
    );
    std::process::exit(125);
}
