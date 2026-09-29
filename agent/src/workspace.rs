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

/// The directory [`ensure_helper`] creates under the login user's home.
const HELPER_DIR: &str = ".summrise/bin";

/// The helper's file name there.
const HELPER_NAME: &str = "summrise-exec-argv";

/// Its mode: runnable by anyone who can reach it, writable only by its owner. The agent uploads as
/// the login user, so the owner bit is the only one it needs.
const HELPER_MODE: u32 = 0o755;

/// Put the helper on `sftp`'s host, and return the ABSOLUTE path it now lives at.
///
/// # Why this exists, and why it returns an absolute path
///
/// The route's exec string names the helper, so the helper has to be there. Staging it over the
/// connection the command will use means a workspace host needs no package manager, no network and no
/// toolchain — the agent carries the bytes and puts them where it will run them.
///
/// The absolute path matters beyond tidiness: with it the exec string contains **no `~` and no
/// expansion at all**, so the one value that reaches the login shell is a plain path and nothing
/// else. Discovering the home directory happens HERE, over a channel that has no shell on it.
///
/// # The upload is skipped when it would be a no-op
///
/// A workspace host is connected to repeatedly. Re-uploading a half-megabyte binary on every command
/// would be a cost with no benefit, so the remote file's size and then its bytes are compared — and
/// the bytes are what decide, because a size that matches is not a file that matches.
pub async fn ensure_helper(
    sftp: &russh_sftp::client::SftpSession,
    bytes: &[u8],
) -> Result<String, DeviceError> {
    use tokio::io::AsyncWriteExt as _;

    let internal = |message: String| DeviceError::Internal { message };

    let home = sftp.canonicalize(".").await.map_err(|e| {
        internal(format!(
            "workspace: cannot resolve the remote home directory: {e}"
        ))
    })?;

    // ONE LEVEL AT A TIME, because SFTP's `mkdir` makes one level and no more — a single call for
    // `~/.summrise/bin` fails when `~/.summrise` does not exist yet, which is the state of every
    // host the first time. That failure was measured, not imagined: it is what a real sshd answered.
    //
    // The call's own result is not the answer either: it fails when the directory is ALREADY there,
    // which is the ordinary case on every run after the first. What decides is whether the directory
    // exists afterwards.
    let mut directory = home.trim_end_matches('/').to_string();
    for component in HELPER_DIR.split('/') {
        directory.push('/');
        directory.push_str(component);
        let _ = sftp.create_dir(&directory).await;
        if sftp.metadata(&directory).await.is_err() {
            return Err(internal(format!(
                "workspace: could not create {directory} on the workspace host"
            )));
        }
    }
    let path = format!("{directory}/{HELPER_NAME}");

    let wanted = sha256_hex(bytes);

    // THE SKIP IS SOUND RATHER THAN CHEAP, and the measurement says that is the right trade here.
    // Reading the file back to hash it costs about what uploading it costs — 1.09 s against 1.03 s
    // for a 4.8 MB helper on a LAN — so a skip that fetched the bytes saves almost nothing, and one
    // that trusted the SIZE would skip a rebuilt binary whose size happened to match. What this buys
    // is that a workspace host never receives a second copy of bytes it already has.
    //
    // WHAT THE NUMBER ACTUALLY ARGUES FOR is not a cheaper skip but FEWER OF THEM: staging belongs
    // once per connection, not once per command. A release helper is also an order of magnitude
    // smaller than the debug build measured, which moves the whole question.
    if let Ok(existing) = sftp.read(&path).await {
        if existing.len() == bytes.len() && sha256_hex(&existing) == wanted {
            // Already the right bytes. Nothing to do, and nothing to report as done.
            return Ok(path);
        }
    }

    {
        // `create` opens with CREATE|TRUNCATE|WRITE; the high-level `write` uses WRITE only and
        // fails with NoSuchFile on a fresh remote path.
        let mut file = sftp
            .create(&path)
            .await
            .map_err(|e| internal(format!("workspace: cannot create {path}: {e}")))?;
        file.write_all(bytes)
            .await
            .map_err(|e| internal(format!("workspace: cannot write {path}: {e}")))?;
        file.flush()
            .await
            .map_err(|e| internal(format!("workspace: cannot flush {path}: {e}")))?;
    }

    sftp.set_metadata(
        &path,
        russh_sftp::protocol::FileAttributes {
            permissions: Some(HELPER_MODE),
            ..Default::default()
        },
    )
    .await
    .map_err(|e| internal(format!("workspace: cannot chmod {path}: {e}")))?;

    // READ BACK AND HASH. An upload that reported success and landed half a binary would surface
    // later as an exec error nobody can place, on a machine nobody is watching — and staging over the
    // same connection is what makes this check cost one round trip.
    let landed = sftp
        .read(&path)
        .await
        .map_err(|e| internal(format!("workspace: cannot read back {path}: {e}")))?;
    if landed.len() != bytes.len() || sha256_hex(&landed) != wanted {
        return Err(internal(format!(
            "workspace: {path} did not land intact ({} bytes sent, {} read back)",
            bytes.len(),
            landed.len()
        )));
    }

    Ok(path)
}

/// The lowercase hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

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

    /// The whole path against a REAL sshd: stage the helper, then run a command through it.
    ///
    /// `#[ignore]`d because it needs a reachable host and a key, which CI has neither of — and a test
    /// that silently skipped itself would be the vacuity this repository keeps finding. The in-process
    /// harness in `tools::ssh` covers the transport; this covers the one thing that harness cannot:
    /// a real SFTP upload landing on a real filesystem, and a real `execve` consuming the frame.
    ///
    /// Run it by hand:
    ///
    /// ```text
    /// SUMMRISE_TEST_SSH=user@host:22122 \
    /// SUMMRISE_TEST_SSH_KEY=/home/me/.ssh/id_ed25519 \
    /// SUMMRISE_TEST_HELPER=target/debug/summrise-exec-argv \
    ///   cargo test -p summrise-agent --features terminal,keyring \
    ///     stage_and_exec_against_a_real_host -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "needs a reachable sshd and a key; run by hand with SUMMRISE_TEST_SSH set"]
    async fn stage_and_exec_against_a_real_host() {
        let target = std::env::var("SUMMRISE_TEST_SSH").expect("SUMMRISE_TEST_SSH=user@host:port");
        let key =
            std::env::var("SUMMRISE_TEST_SSH_KEY").expect("SUMMRISE_TEST_SSH_KEY=/path/to/key");
        let helper_path = std::env::var("SUMMRISE_TEST_HELPER")
            .unwrap_or_else(|_| "target/debug/summrise-exec-argv".to_string());
        let helper_bytes =
            std::fs::read(&helper_path).unwrap_or_else(|e| panic!("read {helper_path}: {e}"));

        let (user, rest) = target.split_once('@').expect("user@host:port");
        let (host, port) = rest.split_once(':').expect("user@host:port");
        let port: u16 = port.parse().expect("a numeric port");

        let session = SshSession::connect(host, port, user, None, Some(&key))
            .await
            .expect("the test host accepts the key");

        // ── staging ──
        let sftp = session.sftp_session().await.expect("an sftp subsystem");
        let started = std::time::Instant::now();
        let staged = ensure_helper(&sftp, &helper_bytes)
            .await
            .expect("the helper lands");
        let first_upload = started.elapsed();
        println!("staged at {staged}");
        println!(
            "MEASURE first staging ({:.0} KiB): {:?}",
            helper_bytes.len() as f64 / 1024.0,
            first_upload
        );
        assert!(
            staged.starts_with('/'),
            "the exec string must carry an absolute path, got {staged:?}"
        );
        assert!(helper_is_shell_safe(&staged), "and a plain one: {staged:?}");

        // A second run must be a no-op rather than an upload — checked by outcome, not by timing:
        // the same path comes back and the bytes are still right.
        let started = std::time::Instant::now();
        let again = ensure_helper(&sftp, &helper_bytes)
            .await
            .expect("staging is idempotent");
        println!(
            "MEASURE second staging (already right): {:?}",
            started.elapsed()
        );
        assert_eq!(again, staged);
        drop(sftp);

        // ── and the command runs through it ──
        let argv = vec![
            b"/bin/echo".to_vec(),
            b"two words".to_vec(),
            b"$(id)".to_vec(),
        ];
        let mut exec_times = Vec::new();
        let mut outcome = None;
        for _ in 0..5 {
            let started = std::time::Instant::now();
            let one = exec_on(
                &session,
                &staged,
                ExecRequest {
                    argv: &argv,
                    cwd: Some(b"/tmp"),
                    stdout_cap: 64 * 1024,
                    stderr_cap: 4096,
                },
            )
            .await
            .expect("the command runs");
            exec_times.push(started.elapsed());
            outcome = Some(one);
        }
        let outcome = outcome.expect("at least one run");
        println!("MEASURE one command over the open session: {exec_times:?}");

        println!("stdout: {:?}", String::from_utf8_lossy(&outcome.stdout));
        assert_eq!(
            String::from_utf8_lossy(&outcome.stdout).trim_end(),
            "two words $(id)",
            "the argv must arrive as three elements with `$(id)` NOT substituted — this is the \
             whole reason the argv travels beside the command instead of inside it"
        );
        assert_eq!(outcome.status, crate::tools::ssh::ExecStatus::Exited(0));
        assert!(!outcome.stdout_overflow);
    }

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
