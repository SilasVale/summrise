//! Run a command on a workspace host.
//!
//! # Why the argv and the working directory travel out of band
//!
//! A subprocess seam promises its caller that **no shell layer exists**: every element of an argv is
//! passed through as an unquoted value, including values a model chose. SSH cannot honour that on
//! its own — an `exec` request carries a *string*, and `sshd` hands that string to the login shell.
//! So this module puts the argv and the `cwd` into a **frame on the channel's stdin**, and the exec
//! string carries nothing but the staged helper's path. The helper reads the frame, enters the
//! directory, and `execve`s.
//!
//! # The one substitution, and why it is not optional
//!
//! `dsh-tool-fs-search` resolves ripgrep **itself** and puts the result straight into `argv[0]` —
//! it never calls the seam's `resolveExecutable` (measured: `grep -c resolveExecutable` over that
//! package is 0). On a Windows DSH host that value is a Windows absolute path such as
//! `C:\…\node_modules\@vscode\ripgrep\bin\rg.exe`, and forwarding it verbatim asks a POSIX target
//! to execute a Windows path. [`target_program`] therefore replaces a **foreign-spelled** program
//! path with its bare name, so the target resolves its own copy from `PATH`.
//!
//! The rule is deliberately narrow — a drive-letter absolute path, nothing else. A bare `rg.exe`
//! with no drive is *not* rewritten, because a POSIX file may legitimately be called that; the
//! limit is stated rather than guessed at.

use summrise_agent_core::DeviceError;
use summrise_exec_argv::write_frame;

use crate::tools::ssh::{ExecOutcome, SshSession};

/// Where the staged helper lives on a workspace host, relative to the login user's home.
///
/// The leading `~` is the ONE deliberate piece of shell interpretation in this module: the exec
/// string is handed to the login shell, so the tilde is expanded there. Everything else in the
/// string is a literal we chose — see [`helper_is_shell_safe`].
pub const HELPER_DEFAULT: &str = "~/.summrise/bin/summrise-exec-argv";

/// One command to run on a workspace host.
pub struct ExecRequest<'a> {
    /// The argv, `argv[0]` first. It never passes through a shell.
    pub argv: &'a [Vec<u8>],
    /// The directory to run in, or `None` to inherit the login shell's own.
    pub cwd: Option<&'a [u8]>,
    /// Bytes of stdout to keep. The stream is still drained past this — see `exec_capture`.
    pub stdout_cap: usize,
    /// The same, for stderr.
    pub stderr_cap: usize,
}

/// Whether `program` is a path spelled for a platform other than the workspace host.
///
/// A drive-letter absolute path (`C:\…`, `C:/…`) is a Windows spelling. The hosts this module
/// drives are POSIX — the helper it stages is an `execve` shim — so that spelling cannot be
/// executed there and must be translated rather than forwarded.
pub fn is_foreign_program_path(program: &[u8]) -> bool {
    program.len() >= 3
        && program[0].is_ascii_alphabetic()
        && program[1] == b':'
        && (program[2] == b'\\' || program[2] == b'/')
}

/// The final component of a program path, under either separator.
pub fn program_basename(program: &[u8]) -> &[u8] {
    match program.iter().rposition(|b| *b == b'/' || *b == b'\\') {
        Some(i) => &program[i + 1..],
        None => program,
    }
}

/// The program to execute on the target.
///
/// A foreign-spelled path becomes its bare name, with a Windows executable suffix removed — the
/// target's own copy is `rg`, not `rg.exe`, and `PATH` is where it is found. Everything else is
/// returned untouched: a path the target can execute is not this function's business to rewrite.
pub fn target_program(program: &[u8]) -> &[u8] {
    if !is_foreign_program_path(program) {
        return program;
    }
    let base = program_basename(program);
    let has_exe_suffix = base.len() > 4 && base[base.len() - 4..].eq_ignore_ascii_case(b".exe");
    if has_exe_suffix {
        &base[..base.len() - 4]
    } else {
        base
    }
}

/// Whether a helper path may be placed in an exec string.
///
/// The exec string is the ONE value in this module that reaches the login shell, so it is the one
/// place where "no shell layer" has to be enforced rather than asserted. The helper's path is a
/// constant we choose, never a model value, and this refuses anything that is not a plain path —
/// which turns the promise into a property of the code.
///
/// `~` is allowed on purpose: it is the single expansion we want, and the helper lives under the
/// login user's home.
pub fn helper_is_shell_safe(helper: &str) -> bool {
    !helper.is_empty()
        && helper.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-' | b'~' | b'+')
        })
}

/// Run one command on `session`'s host and collect its output.
///
/// The frame is built here and travels on stdin; the exec string is `helper` alone.
pub async fn exec_on(
    session: &SshSession,
    helper: &str,
    request: ExecRequest<'_>,
) -> Result<ExecOutcome, DeviceError> {
    if request.argv.is_empty() {
        return Err(DeviceError::Internal {
            message: "workspace exec: an argv needs at least one element (argv[0])".into(),
        });
    }
    if !helper_is_shell_safe(helper) {
        return Err(DeviceError::Internal {
            message: format!(
                "workspace exec: the helper path {helper:?} is not a plain path, and it is the one \
                 value that reaches the login shell"
            ),
        });
    }

    // The substitution happens HERE, on argv[0], and not in `resolveExecutable`: the search tools
    // never call that method, so a backend that relied on it would forward a Windows path.
    let mut argv: Vec<&[u8]> = Vec::with_capacity(request.argv.len());
    argv.push(target_program(&request.argv[0]));
    for element in &request.argv[1..] {
        argv.push(element);
    }

    let mut frame = Vec::new();
    write_frame(&mut frame, request.cwd, &argv).map_err(|e| DeviceError::Internal {
        message: format!("workspace exec: could not build the frame: {e}"),
    })?;

    session
        .exec_capture(helper, &frame, request.stdout_cap, request.stderr_cap)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_program_path_becomes_the_targets_own_name() {
        // The measured case: this is what `@vscode/ripgrep` hands the seam on a Windows host.
        assert_eq!(
            target_program(br"C:\Users\x\node_modules\@vscode\ripgrep\bin\rg.exe"),
            b"rg"
        );
        assert_eq!(target_program(br"C:/tools/rg.exe"), b"rg");
        assert_eq!(target_program(br"D:\a\b\curl.exe"), b"curl");
    }

    #[test]
    fn the_executable_suffix_is_part_of_the_spelling() {
        // The target's copy is `rg`, not `rg.exe`, and PATH is where it is found.
        assert_eq!(target_program(br"C:\bin\MY.EXE"), b"MY");
        assert_eq!(target_program(br"C:\bin\rg.Exe"), b"rg");
        // No suffix to strip: the name is the name.
        assert_eq!(target_program(br"C:\bin\rg"), b"rg");
    }

    #[test]
    fn a_path_the_target_can_execute_is_left_alone() {
        // This function's job is translation, not path rewriting: a native absolute path, a bare
        // name, and a relative path are all the caller's business.
        assert_eq!(target_program(b"/usr/bin/rg"), b"/usr/bin/rg");
        assert_eq!(target_program(b"rg"), b"rg");
        assert_eq!(target_program(b"./build/thing"), b"./build/thing");
        assert_eq!(target_program(b"/opt/my dir/tool"), b"/opt/my dir/tool");
    }

    #[test]
    fn a_bare_windows_suffix_is_not_rewritten_and_that_is_a_stated_limit() {
        // A POSIX file may legitimately be called `rg.exe`, and a bare name carries no evidence of
        // which platform spelled it. Stated here rather than guessed at in the matcher.
        assert_eq!(target_program(b"rg.exe"), b"rg.exe");
    }

    #[test]
    fn the_basename_survives_both_separators_and_neither() {
        assert_eq!(program_basename(br"a\b\c"), b"c");
        assert_eq!(program_basename(b"a/b/c"), b"c");
        assert_eq!(program_basename(b"c"), b"c");
        assert_eq!(program_basename(br"C:\a\"), b"");
    }

    #[test]
    fn a_helper_path_reaches_the_shell_only_if_it_is_a_plain_path() {
        // The helper is a constant we choose; this is what keeps that a property of the code.
        assert!(helper_is_shell_safe(HELPER_DEFAULT));
        assert!(helper_is_shell_safe("/usr/local/bin/summrise-exec-argv"));
        assert!(helper_is_shell_safe(
            "/opt/summrise-v1.2/bin/summrise-exec-argv"
        ));

        // Anything a shell would act on is refused, because this string IS handed to one.
        assert!(!helper_is_shell_safe(""));
        assert!(!helper_is_shell_safe("/tmp/x; rm -rf /"));
        assert!(!helper_is_shell_safe("/tmp/$(id)"));
        assert!(!helper_is_shell_safe("/tmp/a b"));
        assert!(!helper_is_shell_safe("/tmp/a'b"));
        assert!(!helper_is_shell_safe("/tmp/a\"b"));
        assert!(!helper_is_shell_safe("/tmp/a|b"));
        assert!(!helper_is_shell_safe("/tmp/a&b"));
        assert!(!helper_is_shell_safe("/tmp/a>b"));
        assert!(!helper_is_shell_safe("/tmp/a`b"));
    }

    #[test]
    fn the_drive_letter_rule_needs_all_three_characters() {
        assert!(!is_foreign_program_path(b"C:"));
        assert!(!is_foreign_program_path(b"C:x"));
        assert!(!is_foreign_program_path(b":\\x"));
        assert!(!is_foreign_program_path(b"1:\\x"));
        assert!(is_foreign_program_path(br"C:\x"));
        assert!(is_foreign_program_path(b"c:/x"));
    }
}
