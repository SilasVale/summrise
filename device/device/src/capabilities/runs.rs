//! The runs capability (design §4.5): run a command once, capture its output, keep a short history.
//!
//! A run is one process, not a session: it is spawned, its two output streams are drained, and it is
//! reaped before the tool returns. The default form is an argv array — `command` plus `args` — spawned
//! **without a shell**, so a `$`, a `*` or a `;` is an ordinary character and nothing is interpolated.
//! `shell: true` is the explicit exception: the whole `command` string is handed to the platform shell
//! (`sh -c` on Unix, `cmd /C` on Windows), and then the shell, not this capability, parses it. `args`
//! belongs to the argv form and may not be combined with `shell: true`; a shell line that needs
//! arguments puts them in its `command` string.
//!
//! **The working directory is sandboxed; the command is not.** `cwd` is resolved through the same
//! files-root check `files` uses (an absolute path, a `..` component or a symlink that leaves the root
//! is refused before anything is spawned), and it defaults to the files root. That check decides
//! where the process starts. Once it is running it is an ordinary process of the device's user with
//! whatever access that user has: this is not a jail, and `runs.exec` is a destructive tool because
//! the code it runs is arbitrary.
//!
//! Output is bounded: [`MAX_STDOUT_BYTES`] and [`MAX_STDERR_BYTES`] are retained as the **head** of
//! each stream. `truncated` means data may be omitted due to a cap or capture without known clean EOF.
//! Both pipes are drained concurrently even after their cap is reached, so a command that writes more
//! than the cap finishes instead of blocking on a full pipe. History counts are bytes observed before
//! each stream's handoff and can be lower bounds; history has no completeness field.
//! `env` carries at most [`MAX_ENV_VARS`] variables with values of
//! at most [`MAX_ENV_VALUE_BYTES`] bytes, added to the device's own environment. `timeout_ms` defaults
//! to [`DEFAULT_TIMEOUT_MS`] and may not
//! exceed [`MAX_TIMEOUT_MS`]; on expiry the child's **process group** is killed on Unix (the same
//! lesson the PTY capability learned), the child is reaped, and the result is `exit_code: null` with
//! `timed_out: true`.
//!
//! History is one JSON line per run in `<data>/runs.jsonl`, with the output bodies deliberately left
//! out: a row is `{id, command, args, cwd, exit_code, timed_out, duration_ms, started_at_ms,
//! stdout_bytes, stderr_bytes, stdout_preview, stderr_preview}`. The previews are the first
//! [`PREVIEW_CHARS`] characters of each stream with common credential shapes replaced by `***`; the
//! file holds at most [`HISTORY_LINES`] rows, and the next run rotates
//! it to `<data>/runs.1.jsonl` first, so the newest rows are always in the current file. `list` and
//! `get` read the current file and then the rotated one, newest first.
//!
//! A history that cannot be written is not a failed run. The command already ran and its result is
//! returned with `history_recorded: false`; the failure is appended to `<data>/events.jsonl` as a
//! `runs_history_failed` event carrying the io error. Errors before the spawn — validation,
//! a cwd outside the sandbox, or the spawn itself — return no result. Completed nonzero or timed-out
//! runs fail the tool call while retaining their bounded capture in the tool error.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Read, Seek, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::capabilities::files;

/// How many bytes of a command's standard output are retained.
pub const MAX_STDOUT_BYTES: usize = 1024 * 1024;

/// How many bytes of a command's standard error are retained.
pub const MAX_STDERR_BYTES: usize = 1024 * 1024;

/// The declaration's caps: the schema and this implementation read the same numbers.
pub use summrise_core::capabilities::runs::{
    DEFAULT_LIST_LIMIT, DEFAULT_TIMEOUT_MS, MAX_LIST_LIMIT, MAX_TIMEOUT_MS,
};

/// The most environment variables one run may set.
pub const MAX_ENV_VARS: usize = 32;

/// The most bytes one environment value may carry.
pub const MAX_ENV_VALUE_BYTES: usize = 4096;

/// How many history rows the current file holds before it is rotated.
/// The most bytes a command line and its arguments may carry together.
///
/// The history stores the command as it was given, so an unbounded one makes an unbounded row; 8 KiB is
/// several times the longest real command line and far below anything that would turn a listing into a
/// memory event.
pub const MAX_COMMAND_BYTES: usize = 8 * 1024;

pub const HISTORY_LINES: usize = 10_000;

/// The most bytes one history read loads: the newest rows, not the whole file.
///
/// Ten thousand rows of a few hundred bytes each is a few megabytes; a file of rows with long environments
/// or big outputs would otherwise be read whole before every `list`, `get` and `append`.
pub const HISTORY_READ_BYTES: u64 = 8 * 1024 * 1024;

/// The history file under `<data>`.
pub const HISTORY_FILE: &str = "runs.jsonl";

/// The rotated history file under `<data>`.
pub const ROTATED_HISTORY_FILE: &str = "runs.1.jsonl";

/// How many characters of each stream the history keeps. Small on purpose: the history is a record
/// of what ran, not a log store, and every row is read back by a page.
pub const PREVIEW_CHARS: usize = 2_048;

/// How often the parent checks whether the child has exited.
const POLL_INTERVAL_MS: u64 = 10;

/// The limits a registry enforces; they are data so a test can use a small history and a short cap
/// instead of writing ten thousand runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The bytes of stdout retained per run.
    pub stdout_bytes: usize,
    /// The bytes of stderr retained per run.
    pub stderr_bytes: usize,
    /// The timeout used when the request names none.
    pub default_timeout_ms: u64,
    /// The longest timeout a request may name.
    pub max_timeout_ms: u64,
    /// The most environment variables a request may set.
    pub max_env_vars: usize,
    /// The most bytes one environment value may carry.
    pub max_env_value_bytes: usize,
    /// The most bytes a command line and its arguments may carry together.
    pub max_command_bytes: usize,
    /// The rows the current history file holds before rotation.
    pub history_lines: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            stdout_bytes: MAX_STDOUT_BYTES,
            stderr_bytes: MAX_STDERR_BYTES,
            default_timeout_ms: DEFAULT_TIMEOUT_MS,
            max_timeout_ms: MAX_TIMEOUT_MS,
            max_env_vars: MAX_ENV_VARS,
            max_env_value_bytes: MAX_ENV_VALUE_BYTES,
            max_command_bytes: MAX_COMMAND_BYTES,
            history_lines: HISTORY_LINES,
        }
    }
}

/// What `runs.exec` was asked to run, after the wire has been parsed but before validation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecRequest {
    /// The program (`argv[0]`), or the whole shell command line when [`ExecRequest::shell`] is set.
    pub command: String,
    /// The program's arguments; unused when `shell` is set.
    pub args: Vec<String>,
    /// A directory relative to the files root, or `None` for the root itself.
    pub cwd: Option<String>,
    /// The timeout in milliseconds, or `None` for [`Limits::default_timeout_ms`].
    pub timeout_ms: Option<u64>,
    /// Environment variables set for this process, on top of the device's own environment.
    pub env: BTreeMap<String, String>,
    /// Run the command through the platform shell instead of executing it as an argv array.
    pub shell: bool,
}

/// One run's result as the tool returns it.
///
/// `stdout` and `stderr` are the retained heads, decoded as UTF-8 with replacement for invalid
/// bytes; `truncated` is true when either cap was exceeded or capture lacks known clean EOF (including
/// a read error or detected reader panic/poison). A null `exit_code` with
/// `timed_out: true` means the timeout killed the process group; a null `exit_code` with
/// `timed_out: false` means the process ended from a signal rather than an exit status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExecResult {
    /// The run's id, also the key `runs.get` takes.
    pub id: String,
    /// The process's exit status, or `None` when it was killed or timed out.
    pub exit_code: Option<i32>,
    /// Whether the timeout expired and the process group was killed.
    pub timed_out: bool,
    /// How long the run took, in milliseconds.
    pub duration_ms: u64,
    /// The retained head of standard output.
    pub stdout: String,
    /// The retained head of standard error.
    pub stderr: String,
    /// Whether a cap or incomplete capture may have omitted stream data.
    pub truncated: bool,
    /// Whether the history row was written. A false means the command ran and was captured but its
    /// row could not be appended; the failure is in `<data>/events.jsonl` as `runs_history_failed`.
    pub history_recorded: bool,
}

/// One run as the history records it: no output bodies, only their sizes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// The run's id.
    pub id: String,
    /// The program or shell command line that was run.
    pub command: String,
    /// The argv arguments; empty for a shell run.
    pub args: Vec<String>,
    /// The resolved working directory, an absolute path.
    pub cwd: String,
    /// The exit status, or `None` when killed or timed out.
    pub exit_code: Option<i32>,
    /// Whether the timeout killed the process group.
    pub timed_out: bool,
    /// How long the run took, in milliseconds.
    pub duration_ms: u64,
    /// When the run started, in Unix milliseconds.
    pub started_at_ms: u64,
    /// Standard output bytes observed before handoff, possibly a lower bound; no completeness field.
    pub stdout_bytes: u64,
    /// Standard error bytes observed before handoff, possibly a lower bound; no completeness field.
    pub stderr_bytes: u64,
    /// The first [`PREVIEW_CHARS`] characters of standard output, with common credential shapes
    /// replaced by `***`. Empty for rows written before the field existed.
    #[serde(default)]
    pub stdout_preview: String,
    /// The same for standard error.
    #[serde(default)]
    pub stderr_preview: String,
}

/// The credential prefixes a preview must not echo. This is the same list the transcript's command
/// scrubber uses, kept here because the history is written before any reader sees the text.
const PREVIEW_SECRET_PREFIXES: &[&str] = &["srm_", "sk-", "ghp_", "xoxb-", "AKIA", "Bearer "];

/// Replace each token that starts with a credential prefix, up to the next whitespace, with `***`.
fn scrub_preview(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let mut matched = false;
        for prefix in PREVIEW_SECRET_PREFIXES {
            let plen = prefix.chars().count();
            if i + plen <= chars.len() && chars[i..i + plen].iter().copied().eq(prefix.chars()) {
                out.push_str(prefix);
                out.push_str("***");
                i += plen;
                while i < chars.len() && !chars[i].is_whitespace() {
                    i += 1;
                }
                matched = true;
                break;
            }
        }
        if !matched {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// A capped, scrubbed preview of one captured stream.
fn preview_of(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let capped: String = text.chars().take(PREVIEW_CHARS).collect();
    scrub_preview(&capped)
}

/// Why a run was refused or failed.
#[derive(Debug)]
pub enum RunsError {
    /// `command` is empty.
    EmptyCommand,
    /// `shell: true` was combined with `args`; the shell form takes the whole command line in
    /// `command`, and there is no portable way to splice an argv array into a shell string.
    ShellTakesNoArgs,
    /// The timeout is zero or above [`Limits::max_timeout_ms`].
    InvalidTimeout {
        /// The value the caller named.
        timeout_ms: u64,
        /// The largest accepted value.
        max: u64,
    },
    /// The request named more environment variables than the cap.
    TooManyEnv {
        /// How many were named.
        count: usize,
        /// The cap.
        max: usize,
    },
    /// One environment value is longer than the cap.
    EnvValueTooLarge {
        /// Its length in bytes.
        len: usize,
        /// The cap.
        max: usize,
    },
    /// An environment name is empty or contains `=`, or a name or value contains NUL.
    InvalidEnv,
    /// The command and its arguments exceed the cap together.
    CommandTooLarge {
        /// Its length in bytes, arguments included.
        len: usize,
        /// The cap.
        max: usize,
    },
    /// The working directory did not resolve to a directory inside the files root.
    Cwd(files::FilesError),
    /// `command` named a program that does not exist, so the run never started.
    ///
    /// It is named apart from [`RunsError::Io`] because every platform reports this one differently
    /// ("No such file or directory", "The system cannot find the file specified"): the reason a
    /// caller reads must be the device's, not the operating system's.
    ProgramNotFound,
    /// The command could not be spawned or the history could not be read or written.
    Io(io::Error),
    /// `runs.get` named an id no history row carries.
    UnknownRun,
    /// `runs.list` was asked for more rows than the cap.
    LimitTooLarge {
        /// The requested limit.
        limit: usize,
        /// The cap.
        max: usize,
    },
}

impl fmt::Display for RunsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunsError::EmptyCommand => write!(f, "command must not be empty"),
            RunsError::ShellTakesNoArgs => write!(
                f,
                "shell: true takes the whole command line in command; args is the argv form"
            ),
            RunsError::InvalidTimeout { timeout_ms, max } => {
                write!(f, "timeout_ms is 1..={max}, not {timeout_ms}")
            }
            RunsError::TooManyEnv { count, max } => {
                write!(f, "env carries at most {max} variables, not {count}")
            }
            RunsError::EnvValueTooLarge { len, max } => {
                write!(f, "an environment value is {len} bytes; the cap is {max}")
            }
            RunsError::CommandTooLarge { len, max } => {
                write!(f, "the command line is {len} bytes with its arguments; the cap is {max}")
            }
            RunsError::InvalidEnv => write!(
                f,
                "an environment name must be non-empty and contain no '=' or NUL; a value contains no NUL"
            ),
            RunsError::Cwd(error) => write!(f, "the working directory: {error}"),
            RunsError::ProgramNotFound => write!(f, "program not found"),
            RunsError::Io(error) => write!(f, "{error}"),
            RunsError::UnknownRun => write!(f, "no run has that id"),
            RunsError::LimitTooLarge { limit, max } => {
                write!(f, "runs.list returns at most {max} rows, not {limit}")
            }
        }
    }
}

impl std::error::Error for RunsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RunsError::Cwd(error) => Some(error),
            RunsError::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// The device's one runs registry: the files root `cwd` resolves against and the history file.
pub struct Runs {
    /// The root `files` is sandboxed to.
    files_root: PathBuf,
    /// `<data>/runs.jsonl` and `<data>/runs.1.jsonl`.
    data: PathBuf,
    limits: Limits,
    /// Serializes appends, rotation and history reads, so a list never sees a half-rotated pair.
    history_lock: Mutex<()>,
    /// Makes an id unique within the process even when two runs start in the same millisecond.
    next: AtomicU64,
}

impl Runs {
    /// A registry with the production limits.
    pub fn new(data: &Path, files_root: &Path) -> Runs {
        Runs::with_limits(data, files_root, Limits::default())
    }

    /// A registry with the caller's limits; a test uses a small history and cap.
    pub fn with_limits(data: &Path, files_root: &Path, limits: Limits) -> Runs {
        Runs {
            files_root: files_root.to_path_buf(),
            data: data.to_path_buf(),
            limits,
            history_lock: Mutex::new(()),
            next: AtomicU64::new(0),
        }
    }

    /// Run one command and record it in the history.
    pub fn exec(&self, request: ExecRequest) -> Result<ExecResult, RunsError> {
        validate(&request, &self.limits)?;
        let cwd = files::resolve_dir(
            &self.files_root,
            Path::new(request.cwd.as_deref().unwrap_or("")),
        )
        .map_err(RunsError::Cwd)?;
        let timeout_ms = request.timeout_ms.unwrap_or(self.limits.default_timeout_ms);

        let started_at_ms = now_ms();
        // The millisecond plus a process-local counter keeps two runs that start together distinct,
        // while the timestamp is what makes an id from a previous process unlikely to collide.
        let id = format!(
            "run-{started_at_ms}-{}",
            self.next.fetch_add(1, Ordering::Relaxed)
        );

        let mut command = if request.shell {
            platform_shell(&request.command)
        } else {
            let mut command = Command::new(&request.command);
            command.args(&request.args);
            command
        };
        command
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in &request.env {
            command.env(name, value);
        }
        configure_process_group(&mut command);

        let started = Instant::now();
        let mut child = command.spawn().map_err(|error| {
            // A missing program is the one spawn failure every platform spells its own way; name it
            // once, here, so the tool's refusal says the same thing on Windows and Unix.
            if error.kind() == io::ErrorKind::NotFound {
                RunsError::ProgramNotFound
            } else {
                RunsError::Io(error)
            }
        })?;
        let pid = child.id();

        // Both pipes are drained on their own threads from the first byte: capping what is *kept*
        // must never stop what is *read*, or a child writing past the cap blocks on a full pipe.
        let stdout_cap = self.limits.stdout_bytes;
        let stderr_cap = self.limits.stderr_bytes;
        let stdout_reader = child
            .stdout
            .take()
            .map(|stream| spawn_capture(stream, stdout_cap));
        let stderr_reader = child
            .stderr
            .take()
            .map(|stream| spawn_capture(stream, stderr_cap));

        let deadline = started + Duration::from_millis(timeout_ms);
        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        timed_out = true;
                        break None;
                    }
                    thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
                }
                Err(error) => {
                    // The process already ran; a wait that cannot report a status is recorded, not
                    // turned into a failed call. The result's exit_code is null and no timeout is
                    // claimed.
                    record_event(&self.data, "runs_wait_failed", &id, &error);
                    kill_group(pid, &mut child);
                    let _ = child.wait();
                    break None;
                }
            }
        };

        if timed_out {
            kill_group(pid, &mut child);
            let _ = child.wait();
        }

        let stdout = join_capture(stdout_reader);
        let stderr = join_capture(stderr_reader);
        let duration_ms = elapsed_ms(started);
        let exit_code = status.and_then(|status| status.code());

        let record = Record {
            id: id.clone(),
            command: request.command,
            args: request.args,
            cwd: cwd.to_string_lossy().into_owned(),
            exit_code,
            timed_out,
            duration_ms,
            started_at_ms,
            stdout_bytes: stdout.total,
            stderr_bytes: stderr.total,
            stdout_preview: preview_of(&stdout.bytes),
            stderr_preview: preview_of(&stderr.bytes),
        };
        let history_recorded = match self.append(&record) {
            Ok(()) => true,
            Err(error) => {
                record_event(&self.data, "runs_history_failed", &id, &error);
                false
            }
        };

        Ok(ExecResult {
            id,
            exit_code,
            timed_out,
            duration_ms,
            stdout: String::from_utf8_lossy(&stdout.bytes).into_owned(),
            stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
            truncated: stdout.truncated || stderr.truncated,
            history_recorded,
        })
    }

    /// The newest `limit` history rows, across the current file and the rotated one.
    pub fn list(&self, limit: usize) -> Result<Vec<Record>, RunsError> {
        if limit > MAX_LIST_LIMIT {
            return Err(RunsError::LimitTooLarge {
                limit,
                max: MAX_LIST_LIMIT,
            });
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        let _history = lock(&self.history_lock);
        let mut rows = read_newest_first(&self.data.join(HISTORY_FILE))?;
        if rows.len() < limit {
            rows.extend(read_newest_first(&self.data.join(ROTATED_HISTORY_FILE))?);
        }
        rows.truncate(limit);
        Ok(rows)
    }

    /// One history row by id, or [`RunsError::UnknownRun`].
    pub fn get(&self, id: &str) -> Result<Record, RunsError> {
        let _history = lock(&self.history_lock);
        for path in [
            self.data.join(HISTORY_FILE),
            self.data.join(ROTATED_HISTORY_FILE),
        ] {
            for record in read_newest_first(&path)? {
                if record.id == id {
                    return Ok(record);
                }
            }
        }
        Err(RunsError::UnknownRun)
    }

    /// Append one row, rotating a full current file out of the way first.
    fn append(&self, record: &Record) -> Result<(), RunsError> {
        let _history = lock(&self.history_lock);
        fs::create_dir_all(&self.data).map_err(RunsError::Io)?;
        let current = self.data.join(HISTORY_FILE);
        if count_lines(&current)? >= self.limits.history_lines {
            rotate(&current, &self.data.join(ROTATED_HISTORY_FILE))?;
        }

        let mut line = serde_json::to_string(record)
            .map_err(|error| RunsError::Io(io::Error::new(io::ErrorKind::InvalidData, error)))?;
        line.push('\n');
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&current)
            .map_err(RunsError::Io)?;
        file.write_all(line.as_bytes()).map_err(RunsError::Io)?;
        file.flush().map_err(RunsError::Io)
    }
}

/// Reject a request before anything is spawned.
fn validate(request: &ExecRequest, limits: &Limits) -> Result<(), RunsError> {
    if request.command.is_empty() {
        return Err(RunsError::EmptyCommand);
    }
    if request.shell && !request.args.is_empty() {
        return Err(RunsError::ShellTakesNoArgs);
    }
    if let Some(timeout_ms) = request.timeout_ms {
        if timeout_ms == 0 || timeout_ms > limits.max_timeout_ms {
            return Err(RunsError::InvalidTimeout {
                timeout_ms,
                max: limits.max_timeout_ms,
            });
        }
    }
    let command_bytes: usize = request.command.len()
        + request
            .args
            .iter()
            .map(|argument| argument.len() + 1)
            .sum::<usize>();
    if command_bytes > limits.max_command_bytes {
        return Err(RunsError::CommandTooLarge {
            len: command_bytes,
            max: limits.max_command_bytes,
        });
    }
    if request.env.len() > limits.max_env_vars {
        return Err(RunsError::TooManyEnv {
            count: request.env.len(),
            max: limits.max_env_vars,
        });
    }
    for (name, value) in &request.env {
        if name.is_empty() || name.contains('=') || name.contains('\0') || value.contains('\0') {
            return Err(RunsError::InvalidEnv);
        }
        if value.len() > limits.max_env_value_bytes {
            return Err(RunsError::EnvValueTooLarge {
                len: value.len(),
                max: limits.max_env_value_bytes,
            });
        }
    }
    Ok(())
}

/// The platform shell that `shell: true` runs, as a command that will parse the whole line.
fn platform_shell(line: &str) -> Command {
    #[cfg(unix)]
    {
        let mut command = Command::new("sh");
        command.args(["-c", line]);
        command
    }
    #[cfg(windows)]
    {
        let mut command = Command::new("cmd");
        command.args(["/C", line]);
        command
    }
    #[cfg(not(any(unix, windows)))]
    {
        let mut command = Command::new(line);
        command
    }
}

/// Put the child in its own process group, so a timeout can reach more than the direct child.
fn configure_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        // Safe, stable `std` API: the child becomes the leader of a new group whose id is its pid.
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = command;
    }
}

/// Kill the child's process group and the child itself; both calls are best-effort.
fn kill_group(pid: u32, child: &mut Child) {
    #[cfg(unix)]
    {
        // pid 0 means "this process group" to kill(2); pid 1 is init. Neither is a run we started.
        if pid > 1 {
            let group = nix::unistd::Pid::from_raw(pid as i32);
            let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
            let _ = nix::sys::signal::kill(group, nix::sys::signal::Signal::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        // A new Windows process group does not give a kill-tree; `taskkill /T` is the platform's
        // own answer. The direct kill below is the fallback when the process already ended.
        // `CREATE_NO_WINDOW`: the app normally runs under the service, where there is no desktop, but a
        // `--enroll` or a debugging run from a console session would otherwise flash a taskkill window
        // for every timed-out run.
        let mut command = Command::new("taskkill");
        command.args(["/F", "/T", "/PID", &pid.to_string()]);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
        let _ = command.status();
    }
    #[cfg(not(any(unix, windows)))]
    let _ = pid;

    let _ = child.kill();
}

/// One stream's retained head, observed byte count, and possible omission of data.
#[derive(Default)]
struct Captured {
    /// The head of the stream, at most the cap.
    bytes: Vec<u8>,
    /// Bytes published before handoff, including drained excess; possibly a lower bound.
    total: u64,
    /// The cap or incomplete capture may have omitted stream data.
    truncated: bool,
}

#[derive(Default)]
struct CaptureState {
    captured: Captured,
    eof: bool,
    sealed: bool,
}

struct CaptureTask {
    handle: thread::JoinHandle<()>,
    state: Arc<Mutex<CaptureState>>,
}

fn spawn_capture(stream: impl Read + Send + 'static, cap: usize) -> CaptureTask {
    let state = Arc::new(Mutex::new(CaptureState::default()));
    let reader_state = Arc::clone(&state);
    let handle = thread::spawn(move || capture(stream, cap, reader_state));
    CaptureTask { handle, state }
}

/// Drain outside the lock, publishing bounded heads and observed totals until handoff.
/// After sealing, continue draining without refilling the transferred head or changing its count.
fn capture(mut stream: impl Read, cap: usize, state: Arc<Mutex<CaptureState>>) {
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => {
                let mut state = lock(&state);
                if !state.sealed {
                    state.eof = true;
                }
                break;
            }
            Ok(read) => {
                let mut state = lock(&state);
                if state.sealed {
                    continue;
                }
                let captured = &mut state.captured;
                captured.total += read as u64;
                let room = cap.saturating_sub(captured.bytes.len());
                let keep = room.min(read);
                captured.bytes.extend_from_slice(&chunk[..keep]);
                if keep < read {
                    captured.truncated = true;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            // No clean EOF: the child's exit status still describes execution, while handoff
            // marks the capture incomplete and preserves every previously published read.
            Err(_) => break,
        }
    }
}

/// How long each capture thread may keep the call after the child has been reaped.
///
/// A descendant may hold an inherited write end indefinitely. Preserve the existing per-stream
/// two-second grace (stdout then stderr), so both streams can add about four seconds to the call.
const CAPTURE_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// Seal and transfer published progress, joining only a reader that has finished.
/// An unfinished detached reader retains its read handle, thread and small state until it exits;
/// this bounded handoff does not cancel a blocking read or promise leak-free cancellation.
fn join_capture(task: Option<CaptureTask>) -> Captured {
    let Some(task) = task else {
        return Captured {
            truncated: true,
            ..Captured::default()
        };
    };
    let deadline = Instant::now() + CAPTURE_GRACE;
    while !task.handle.is_finished() {
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let panicked = task.handle.is_finished() && task.handle.join().is_err();
    // Inspect poison here rather than hiding it through the recovering lock helper. Recheck EOF
    // under this final lock: it may have arrived as the grace expired. This lock is the snapshot
    // boundary; chunks read but not yet published are deliberately outside the returned count.
    let (mut state, poisoned) = match task.state.lock() {
        Ok(state) => (state, false),
        Err(error) => (error.into_inner(), true),
    };
    state.sealed = true;
    let mut captured = std::mem::take(&mut state.captured);
    captured.truncated |= !state.eof || panicked || poisoned;
    captured
}

/// The rows of one history file, newest first.
///
/// A line that does not parse is skipped: a process stopped mid-append can leave a torn final line,
/// and one unreadable row must not make the rest of the history unavailable.
fn read_newest_first(path: &Path) -> Result<Vec<Record>, RunsError> {
    let text = match read_capped(path, HISTORY_READ_BYTES)? {
        Some(text) => text,
        None => return Ok(Vec::new()),
    };
    let mut records: Vec<Record> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    records.reverse();
    Ok(records)
}

/// How many rows a history file holds; an absent file is empty.
///
/// Counted by scanning in chunks rather than by reading the file: this runs before every append, and the
/// file is the one thing in the data directory that grows to megabytes.
fn count_lines(path: &Path) -> Result<usize, RunsError> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(RunsError::Io(error)),
    };
    let mut buffer = [0u8; 64 * 1024];
    let mut lines = 0;
    let mut last_byte = b'\n';
    loop {
        let read = file.read(&mut buffer).map_err(RunsError::Io)?;
        if read == 0 {
            break;
        }
        for byte in &buffer[..read] {
            if *byte == b'\n' {
                lines += 1;
            }
            last_byte = *byte;
        }
    }
    // A file whose last line has no newline still holds a row (a crash between the two writes).
    if last_byte != b'\n' {
        lines += 1;
    }
    Ok(lines)
}

/// Read at most `cap` bytes of `path`, starting from the end, and drop the partial first line.
///
/// **THE HISTORY IS APPEND-ONLY AND NEWEST-LAST, SO THE TAIL IS WHAT A READER WANTS.** Reading the whole
/// file put up to `HISTORY_LINES` rows of commands and output into memory on every `list` and `get`; the
/// cap keeps the newest rows, which are the ones the page shows, and a file smaller than the cap reads
/// exactly as before.
fn read_capped(path: &Path, cap: u64) -> Result<Option<String>, RunsError> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(RunsError::Io(error)),
    };
    let len = file.metadata().map_err(RunsError::Io)?.len();
    let mut text = String::new();
    if len <= cap {
        file.read_to_string(&mut text).map_err(RunsError::Io)?;
        return Ok(Some(text));
    }
    file.seek(io::SeekFrom::Start(len - cap))
        .map_err(RunsError::Io)?;
    file.read_to_string(&mut text).map_err(RunsError::Io)?;
    // The first line is a fragment: its beginning is before the window.
    match text.find('\n') {
        Some(at) => {
            text.drain(..=at);
        }
        None => text.clear(),
    }
    Ok(Some(text))
}

/// Move a full current history aside, replacing any older rotated file.
fn rotate(current: &Path, rotated: &Path) -> Result<(), RunsError> {
    if !current.exists() {
        return Ok(());
    }
    match fs::remove_file(rotated) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(RunsError::Io(error)),
    }
    fs::rename(current, rotated).map_err(RunsError::Io)
}

/// Append one event to `<data>/events.jsonl`, best effort: a device that cannot write its event log
/// still returns the result the event would have accompanied.
fn record_event(data: &Path, event: &str, id: &str, error: &impl fmt::Display) {
    let line = serde_json::json!({
        "ts_ms": now_ms(),
        "event": event,
        "id": id,
        "error": error.to_string(),
    });
    let mut text = line.to_string();
    text.push('\n');
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data.join("events.jsonl"))
    {
        let _ = file.write_all(text.as_bytes());
        let _ = file.flush();
    }
}

/// The device's wall clock, in Unix milliseconds.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// How long a run took, in milliseconds.
fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

/// Lock shared history or capture data, recovering poison instead of panicking.
/// Capture handoff separately inspects poison because recovery alone cannot establish completeness.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(all(test, unix))]
pub(crate) mod open_pipe_fixture {
    use super::*;
    use crate::test_support::{process_is_dead, TempDir};
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::mpsc;

    // Payloads stay in the environment, so command/argv history cannot contain the markers.
    const SCRIPT: &str = r#"
printf '%s\n' "$$" > "$PIPE_PARENT" || { printf 'no-holder\n' > "$PIPE_REGISTRATION_FAILED"; exit 125; }
sh -c 'if [ "${PIPE_PAUSE_REGISTRATION:-}" = 1 ]; then kill -STOP "$$"; fi; printf ready > "$PIPE_READY"; IFS= read -r release < "$PIPE_CONTROL"; printf "%s" "$PIPE_LATE_OUT"; printf "%s" "$PIPE_LATE_ERR" >&2' &
holder=$!
if [ -n "${PIPE_RESCUE_PID:-}" ]; then
    printf '%s %s\n' "$$" "$holder" > "$PIPE_RESCUE_PID" || { kill -KILL "$holder"; wait "$holder"; printf 'holder-reaped\n' > "$PIPE_REGISTRATION_FAILED"; exit 125; }
fi
if ! printf '%s\n' "$holder" > "$PIPE_HOLDER" || ! IFS= read -r registered < "$PIPE_HOLDER" || [ "$registered" != "$holder" ]; then
    kill -KILL "$holder" 2>/dev/null
    wait "$holder" 2>/dev/null
    printf 'holder-reaped\n' > "$PIPE_REGISTRATION_FAILED"
    exit 125
fi
printf '%s' "$PIPE_OUT"
printf '%s' "$PIPE_ERR" >&2
exit "$PIPE_EXIT"
"#;

    /// Owns only this fixture's private FIFO, PID files and worker. Installed before any spawn.
    pub(crate) struct OpenPipe<T> {
        _dir: TempDir,
        parent: PathBuf,
        holder: PathBuf,
        registration_failed: PathBuf,
        ready: PathBuf,
        control: Option<fs::File>,
        setup_child: Option<Child>,
        worker: Option<thread::JoinHandle<()>>,
        receiver: mpsc::Receiver<T>,
        started: Instant,
        pub(crate) request: ExecRequest,
    }

    impl<T: Send + 'static> OpenPipe<T> {
        pub(crate) fn start(
            label: &str,
            stdout: &str,
            stderr: &str,
            exit: i32,
            run: impl FnOnce(ExecRequest) -> T + Send + 'static,
        ) -> Self {
            let dir = TempDir::new(label);
            let fifo = dir.join("release.fifo");
            let (sender, receiver) = mpsc::channel();
            let mut owned = Self {
                parent: dir.join("parent.pid"),
                holder: dir.join("holder.pid"),
                registration_failed: dir.join("registration.failed"),
                ready: dir.join("ready"),
                _dir: dir,
                control: None,
                setup_child: None,
                worker: None,
                receiver,
                started: Instant::now(),
                request: ExecRequest::default(),
            };
            owned.setup_child = Some(
                Command::new("mkfifo")
                    .arg(&fifo)
                    .spawn()
                    .expect("SETUP: mkfifo exists"),
            );
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if let Some(status) = owned
                    .setup_child
                    .as_mut()
                    .unwrap()
                    .try_wait()
                    .expect("SETUP: wait mkfifo")
                {
                    assert!(status.success(), "SETUP: mkfifo failed: {status}");
                    owned.setup_child.take(); // try_wait reaped the direct child.
                    break;
                }
                assert!(Instant::now() < deadline, "SETUP: mkfifo deadline");
                thread::sleep(Duration::from_millis(10));
            }
            // O_RDWR avoids a blocking open, including teardown before the holder reaches read.
            owned.control = Some(
                fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .custom_flags(nix::libc::O_NONBLOCK)
                    .open(&fifo)
                    .expect("SETUP: open private FIFO"),
            );
            owned.request = ExecRequest {
                command: "sh".to_owned(),
                args: vec!["-c".to_owned(), SCRIPT.to_owned()],
                timeout_ms: Some(30_000),
                env: BTreeMap::from([
                    (
                        "PIPE_PARENT".to_owned(),
                        owned.parent.to_string_lossy().into_owned(),
                    ),
                    (
                        "PIPE_HOLDER".to_owned(),
                        owned.holder.to_string_lossy().into_owned(),
                    ),
                    (
                        "PIPE_REGISTRATION_FAILED".to_owned(),
                        owned.registration_failed.to_string_lossy().into_owned(),
                    ),
                    (
                        "PIPE_READY".to_owned(),
                        owned.ready.to_string_lossy().into_owned(),
                    ),
                    (
                        "PIPE_CONTROL".to_owned(),
                        fifo.to_string_lossy().into_owned(),
                    ),
                    ("PIPE_PAUSE_REGISTRATION".to_owned(), String::new()),
                    ("PIPE_RESCUE_PID".to_owned(), String::new()),
                    ("PIPE_OUT".to_owned(), stdout.to_owned()),
                    ("PIPE_ERR".to_owned(), stderr.to_owned()),
                    ("PIPE_EXIT".to_owned(), exit.to_string()),
                    ("PIPE_LATE_OUT".to_owned(), "late-owned-stdout".to_owned()),
                    ("PIPE_LATE_ERR".to_owned(), "late-owned-stderr".to_owned()),
                ]),
                ..ExecRequest::default()
            };
            let request = owned.request.clone();
            owned.worker = Some(thread::spawn(move || {
                let _ = sender.send(run(request));
            }));
            owned
        }

        pub(crate) fn wait_ready(&self) -> Result<u32, String> {
            let deadline = self.started + Duration::from_secs(5);
            loop {
                if self.registration_failed.exists() {
                    return Err("SETUP: parent could not publish holder registration".to_owned());
                }
                if self.ready.exists() {
                    if let Some(pid) = self.holder_pid() {
                        if !process_is_dead(pid) {
                            return Ok(pid);
                        }
                        return Err("SETUP: holder stopped before result".to_owned());
                    }
                    // The holder can signal readiness before its parent publishes $!.
                }
                if Instant::now() >= deadline {
                    return Err("SETUP: holder readiness deadline".to_owned());
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        pub(crate) fn receive(&self) -> Result<T, String> {
            let remaining =
                (self.started + Duration::from_secs(15)).saturating_duration_since(Instant::now());
            self.receiver
                .recv_timeout(remaining)
                .map_err(|error| format!("WATCHDOG: {error}"))
        }

        pub(crate) fn holder_pid(&self) -> Option<u32> {
            read_pid(&self.holder)
        }
        pub(crate) fn elapsed(&self) -> Duration {
            self.started.elapsed()
        }
    }

    fn read_pid(path: &Path) -> Option<u32> {
        fs::read_to_string(path)
            .ok()?
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|pid| *pid > 1)
    }

    impl<T> OpenPipe<T> {
        pub(crate) fn cleanup(&mut self) -> Result<(), String> {
            if let Some(control) = self.control.as_mut() {
                let _ = control.write_all(b"release\n");
            }
            if let Some(mut child) = self.setup_child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            let graceful = Instant::now() + Duration::from_secs(3);
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let parent = read_pid(&self.parent);
                let holder = read_pid(&self.holder);
                let finished = self
                    .worker
                    .as_ref()
                    .is_none_or(|worker| worker.is_finished());
                // An absent expected PID is unknown, never evidence that an actor stopped.
                // A parent receipt is written only before any fork or after kill + wait.
                let failure = fs::read_to_string(&self.registration_failed).unwrap_or_default();
                let never_started = self.worker.is_none() && parent.is_none() && holder.is_none();
                let no_holder = parent.is_none() && holder.is_none() && failure == "no-holder\n";
                let holder_stopped = holder
                    .map(process_is_dead)
                    .unwrap_or(failure == "holder-reaped\n");
                let stopped = never_started
                    || no_holder
                    || (parent.is_some_and(process_is_dead) && holder_stopped);
                if stopped && finished {
                    self.control.take();
                    if let Some(worker) = self.worker.take() {
                        worker
                            .join()
                            .map_err(|_| "fixture worker panicked".to_owned())?;
                        eprintln!("owned pipe cleanup: parent={parent:?} holder={holder:?} stopped=true worker_joined=true elapsed={:?}", self.started.elapsed());
                    }
                    return Ok(());
                }
                if Instant::now() >= graceful {
                    // Signal only the private recorded group after an owned member confirms it.
                    if let Some(group) = parent {
                        for pid in parent
                            .into_iter()
                            .chain(holder)
                            .filter(|pid| !process_is_dead(*pid))
                        {
                            use nix::sys::signal::{killpg, Signal};
                            use nix::unistd::{getpgid, Pid};
                            if getpgid(Some(Pid::from_raw(pid as i32))).ok()
                                == Some(Pid::from_raw(group as i32))
                            {
                                let _ = killpg(Pid::from_raw(group as i32), Signal::SIGKILL);
                                break;
                            }
                        }
                    }
                }
                if Instant::now() >= deadline {
                    return Err(format!("fixture cleanup deadline: parent={parent:?} holder={holder:?} stopped={stopped} worker_finished={finished}"));
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    impl<T> Drop for OpenPipe<T> {
        fn drop(&mut self) {
            if let Err(error) = self.cleanup() {
                eprintln!("{error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::path::PathBuf;

    #[cfg(unix)]
    use super::*;
    #[cfg(unix)]
    use crate::test_support::{wait_until_dead, TempDir};

    /// A data directory, a files root and a registry over both.
    #[cfg(unix)]
    fn fixture(label: &str) -> (TempDir, PathBuf, PathBuf, Runs) {
        let dir = TempDir::new(label);
        let data = dir.join("data");
        let root = dir.join("root");
        fs::create_dir_all(&data).expect("the data directory is creatable");
        fs::create_dir_all(&root).expect("the files root is creatable");
        let runs = Runs::new(&data, &root);
        (dir, data, root, runs)
    }

    /// An argv request with the defaults.
    #[cfg(unix)]
    fn argv(command: &str, args: &[&str]) -> ExecRequest {
        ExecRequest {
            command: command.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            ..ExecRequest::default()
        }
    }

    #[cfg(unix)]
    /// A command line is bounded before anything is spawned: the history stores it, so an unbounded one
    /// is an unbounded row.
    #[cfg(unix)]
    #[test]
    fn a_command_line_over_the_cap_is_refused_before_the_spawn() {
        let limits = Limits::default();
        let long = "x".repeat(limits.max_command_bytes + 1);
        let request = ExecRequest {
            command: long,
            ..ExecRequest::default()
        };
        match validate(&request, &limits) {
            Err(RunsError::CommandTooLarge { len, max }) => {
                assert!(len > max);
                assert_eq!(max, limits.max_command_bytes);
            }
            other => panic!("a huge command must be refused, got {other:?}"),
        }

        // Arguments count too, and the separator between them.
        let argument = "y".repeat(limits.max_command_bytes);
        let request = ExecRequest {
            command: "cmd".to_owned(),
            args: vec![argument],
            ..ExecRequest::default()
        };
        assert!(matches!(
            validate(&request, &limits),
            Err(RunsError::CommandTooLarge { .. })
        ));
    }

    /// The history read keeps the newest rows when the file is larger than the window.
    #[cfg(unix)]
    #[test]
    fn a_history_read_keeps_the_tail_when_the_file_exceeds_the_window() {
        let dir = TempDir::new("runs-history-window");
        let path = dir.join("runs.jsonl");
        let mut body = String::new();
        for index in 0..50 {
            body.push_str(&format!("{{\"index\":{index}}}\n"));
        }
        fs::write(&path, &body).expect("the history is writable");
        // A window that can hold only the last few rows: the first line is a fragment and is dropped.
        let text = read_capped(&path, 40)
            .expect("the read succeeds")
            .expect("a file");
        assert!(
            !text.starts_with("{\"index\":49"),
            "the fragment is dropped: {text}"
        );
        assert!(
            text.trim_end().ends_with("\"index\":49}"),
            "the tail survives: {text}"
        );
        assert!(
            !text.contains("\"index\":0}"),
            "the head is not read: {text}"
        );
        assert_eq!(
            text.lines().count(),
            3,
            "three whole rows fit in forty bytes"
        );
    }

    // A real reader error must retain the prefix but cannot establish clean EOF.
    #[cfg(unix)]
    #[test]
    fn pipe_capture_read_error_preserves_prefix_and_marks_incomplete() {
        struct FailsAfterPrefix(bool);
        impl Read for FailsAfterPrefix {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if self.0 {
                    return Err(io::Error::other("owned read failure"));
                }
                self.0 = true;
                buffer[..6].copy_from_slice(b"prefix");
                Ok(6)
            }
        }
        let captured = join_capture(Some(spawn_capture(FailsAfterPrefix(false), 128)));
        assert_eq!(
            (captured.bytes, captured.total, captured.truncated),
            (b"prefix".to_vec(), 6, true)
        );
    }

    // Interrupted is retryable; a later read zero makes an uncapped capture complete.
    #[cfg(unix)]
    #[test]
    fn pipe_capture_clean_eof_after_interruption_is_complete() {
        struct InterruptedThenEof(u8);
        impl Read for InterruptedThenEof {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                self.0 += 1;
                match self.0 {
                    1 => Err(io::Error::from(io::ErrorKind::Interrupted)),
                    2 => {
                        buffer[..2].copy_from_slice(b"ok");
                        Ok(2)
                    }
                    _ => Ok(0),
                }
            }
        }
        let captured = join_capture(Some(spawn_capture(InterruptedThenEof(0), 128)));
        assert_eq!(
            (captured.bytes, captured.total, captured.truncated),
            (b"ok".to_vec(), 2, false)
        );
    }

    // A panic after a successful read cannot discard that read or claim clean EOF.
    #[cfg(unix)]
    #[test]
    fn pipe_capture_reader_panic_preserves_published_prefix() {
        struct PanicsAfterPrefix(bool);
        impl Read for PanicsAfterPrefix {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                assert!(!self.0, "owned reader panic");
                self.0 = true;
                buffer[..6].copy_from_slice(b"prefix");
                Ok(6)
            }
        }
        let captured = join_capture(Some(spawn_capture(PanicsAfterPrefix(false), 128)));
        assert_eq!(
            (captured.bytes, captured.total, captured.truncated),
            (b"prefix".to_vec(), 6, true)
        );
    }

    // Poison after a published clean EOF still means completeness cannot be trusted.
    #[cfg(unix)]
    #[test]
    fn pipe_capture_poisoned_state_keeps_prefix_and_marks_incomplete() {
        let state = Arc::new(Mutex::new(CaptureState {
            captured: Captured {
                bytes: b"prefix".to_vec(),
                total: 6,
                truncated: false,
            },
            eof: true,
            sealed: false,
        }));
        let reader_state = Arc::clone(&state);
        let poisoner = thread::spawn(move || {
            let _guard = reader_state.lock().unwrap();
            panic!("owned capture state poison");
        });
        assert!(poisoner.join().is_err(), "the state is actually poisoned");
        // A separately successful reader handle isolates poison from reader panic and missing EOF.
        let handle = thread::spawn(|| {});
        let captured = join_capture(Some(CaptureTask { handle, state }));
        assert_eq!(
            (captured.bytes, captured.total, captured.truncated),
            (b"prefix".to_vec(), 6, true)
        );
    }

    #[cfg(unix)]
    #[test]
    fn pipe_capture_missing_reader_is_incomplete() {
        let captured = join_capture(None);
        assert_eq!(
            (captured.bytes, captured.total, captured.truncated),
            (Vec::<u8>::new(), 0, true)
        );
    }

    // A delayed holder must be accounted for before cleanup can close its only release writer.
    #[cfg(target_os = "linux")]
    #[test]
    fn pipe_capture_cleanup_stops_holder_delayed_before_registration() {
        use crate::test_support::process_is_dead;
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::{getpgid, Pid};

        // Independent of OpenPipe's registration: installed first, and rescues the real actor
        // before any assertion, including on the old helper's false-success RED path.
        struct Rescue {
            _dir: TempDir,
            pid_file: PathBuf,
        }
        impl Rescue {
            fn pids(&self) -> Option<(u32, u32)> {
                let text = fs::read_to_string(&self.pid_file).ok()?;
                let mut fields = text.split_whitespace();
                let group = fields.next()?.parse::<u32>().ok().filter(|pid| *pid > 1)?;
                let holder = fields.next()?.parse::<u32>().ok().filter(|pid| *pid > 1)?;
                Some((group, holder))
            }
            fn stop(&self) -> Result<(), String> {
                let deadline = Instant::now() + Duration::from_secs(3);
                loop {
                    if let Some((group, holder)) = self.pids() {
                        if process_is_dead(holder) {
                            return Ok(());
                        }
                        if getpgid(Some(Pid::from_raw(holder as i32))).ok()
                            != Some(Pid::from_raw(group as i32))
                        {
                            return Err(
                                "rescue refused a PID outside its recorded group".to_owned()
                            );
                        }
                        let _ = kill(Pid::from_raw(holder as i32), Signal::SIGKILL);
                    }
                    if Instant::now() >= deadline {
                        return Err("independent rescue deadline".to_owned());
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
        impl Drop for Rescue {
            fn drop(&mut self) {
                if let Err(error) = self.stop() {
                    eprintln!("{error}");
                }
            }
        }
        let rescue_dir = TempDir::new("pipe-registration-rescue");
        let rescue = Rescue {
            pid_file: rescue_dir.join("owned.pid"),
            _dir: rescue_dir,
        };
        let rescue_path = rescue.pid_file.to_string_lossy().into_owned();
        let (_dir, _data, _root, runs) = fixture("pipe-delayed-registration");
        let mut pipe = super::open_pipe_fixture::OpenPipe::start(
            "pipe-delayed-control",
            "",
            "",
            0,
            move |mut request| {
                request
                    .env
                    .insert("PIPE_PAUSE_REGISTRATION".to_owned(), "1".to_owned());
                request
                    .env
                    .insert("PIPE_RESCUE_PID".to_owned(), rescue_path);
                runs.exec(request)
            },
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let paused = loop {
            if let Some((_, holder)) = rescue.pids() {
                let stat = fs::read_to_string(format!("/proc/{holder}/stat")).unwrap_or_default();
                if stat
                    .rsplit_once(')')
                    .is_some_and(|(_, fields)| fields.split_whitespace().next() == Some("T"))
                {
                    break Ok(holder);
                }
            }
            if Instant::now() >= deadline {
                break Err("SETUP: holder did not stop before registration");
            }
            thread::sleep(Duration::from_millis(10));
        };
        let received = pipe.receive();
        let registered = pipe.holder_pid();
        let cleanup = pipe.cleanup();
        let live_after_cleanup = rescue
            .pids()
            .is_some_and(|(_, holder)| !process_is_dead(holder));
        let rescued = rescue.stop();
        // Exercise final Drop while the independent guard still owns its rescue record.
        drop(pipe);
        eprintln!("delayed-registration: paused={paused:?} registered={registered:?} cleanup={cleanup:?} holder_live_after_cleanup={live_after_cleanup} independent_rescue={rescued:?}");
        assert!(rescued.is_ok(), "{rescued:?}");
        paused.expect("controlled actual holder stop");
        received
            .expect("bounded public exec receipt")
            .expect("sh runs");
        assert!(cleanup.is_ok() && !live_after_cleanup,
            "cleanup must stop every spawned holder before success: cleanup={cleanup:?}, holder_live={live_after_cleanup}, registered={registered:?}");

        // A real failed PID-file publication must reap the forked holder and surface setup failure.
        let rescue_dir = TempDir::new("pipe-registration-failure-rescue");
        let rescue = Rescue {
            pid_file: rescue_dir.join("owned.pid"),
            _dir: rescue_dir,
        };
        let rescue_path = rescue.pid_file.to_string_lossy().into_owned();
        let (_dir, _data, root, runs) = fixture("pipe-registration-failure");
        let mut pipe = super::open_pipe_fixture::OpenPipe::start(
            "pipe-registration-failure-control",
            "",
            "",
            0,
            move |mut request| {
                request
                    .env
                    .insert("PIPE_RESCUE_PID".to_owned(), rescue_path);
                // An existing directory deterministically rejects the parent's PID-file write.
                request.env.insert(
                    "PIPE_HOLDER".to_owned(),
                    root.to_string_lossy().into_owned(),
                );
                runs.exec(request)
            },
        );
        let received = pipe.receive();
        let readiness = pipe.wait_ready();
        let cleanup = pipe.cleanup();
        let holder_stopped = rescue.pids().is_some_and(|(_, pid)| process_is_dead(pid));
        let rescued = rescue.stop();
        drop(pipe);
        eprintln!("registration-write-failure: readiness={readiness:?} cleanup={cleanup:?} holder_stopped_before_rescue={holder_stopped} independent_rescue={rescued:?}");
        assert!(rescued.is_ok(), "{rescued:?}");
        assert!(readiness.is_err(), "failed publication is a setup failure");
        assert_eq!(
            received
                .expect("bounded failure receipt")
                .expect("sh runs")
                .exit_code,
            Some(125)
        );
        assert!(
            cleanup.is_ok() && holder_stopped,
            "parent must reap the unregistered holder: {cleanup:?}"
        );
    }

    // This catches loss of a reader's published head/count when inherited pipes outlive the child.
    #[cfg(unix)]
    #[test]
    fn pipe_capture_success_keeps_capped_heads_and_observed_history_without_eof() {
        let (_dir, data, root, _) = fixture("runs-open-pipe");
        let runs = Arc::new(Runs::with_limits(
            &data,
            &root,
            Limits {
                stdout_bytes: 16,
                stderr_bytes: 128,
                ..Limits::default()
            },
        ));
        let worker_runs = Arc::clone(&runs);
        let mut pipe = super::open_pipe_fixture::OpenPipe::start(
            "runs-pipe-control",
            "0123456789abcdefMORE",
            "sk-pipe_secret err-before",
            0,
            move |request| worker_runs.exec(request),
        );
        let ready = pipe.wait_ready();
        let received = pipe.receive();
        let elapsed = pipe.elapsed();
        let live = pipe
            .holder_pid()
            .is_some_and(|pid| !crate::test_support::process_is_dead(pid));
        let request = pipe.request.clone();
        let cleanup = pipe.cleanup();
        eprintln!("external success result: elapsed={elapsed:?} holder_live_before_release={live} cleanup={cleanup:?}");
        assert!(cleanup.is_ok(), "{cleanup:?}");
        ready.expect("SETUP: the holder reached its private control read");
        let result = received
            .expect("result within the fixture watchdog")
            .expect("sh runs");
        assert!(
            live,
            "the inherited holder must remain live until result receipt"
        );
        assert!(elapsed < Duration::from_secs(15));
        assert_eq!(
            (&*result.stdout, &*result.stderr, result.truncated),
            ("0123456789abcdef", "sk-pipe_secret err-before", true),
            "public prefix/flag tuple: {result:?}"
        );
        assert_eq!(result.exit_code, Some(0));
        assert!(!result.timed_out);
        assert!(result.history_recorded);
        assert!(result.duration_ms < 15_000);
        let row = runs.get(&result.id).expect("get retains one row");
        assert_eq!(runs.list(5).unwrap(), vec![row.clone()]);
        assert_eq!((row.stdout_bytes, row.stderr_bytes), (20, 25));
        assert_eq!(row.stdout_preview, "0123456789abcdef");
        assert_eq!(row.stderr_preview, "sk-*** err-before");
        assert_eq!(row.command, request.command);
        assert_eq!(row.args, request.args);
        assert_eq!(row.cwd, fs::canonicalize(root).unwrap().to_string_lossy());
        assert_eq!(row.duration_ms, result.duration_ms);
        let saved = serde_json::to_value(row).unwrap();
        for field in ["env", "stdout", "stderr"] {
            assert!(saved.get(field).is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    fn echo_hello_exits_zero_with_its_standard_output() {
        let (_dir, _data, _root, runs) = fixture("runs-echo");

        let result = runs.exec(argv("echo", &["hello"])).expect("echo runs");

        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout, "hello\n");
        assert_eq!(result.stderr, "");
        assert!(!result.timed_out);
        assert!(!result.truncated);
        assert!(
            result.duration_ms < 30_000,
            "the duration is measured in milliseconds, not seconds: {}",
            result.duration_ms
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_default_form_does_not_interpolate_shell_metacharacters() {
        let (_dir, _data, _root, runs) = fixture("runs-no-shell");

        let result = runs
            .exec(argv("printf", &["%s", "$HOME `id` *"]))
            .expect("printf runs");

        assert_eq!(
            result.stdout, "$HOME `id` *",
            "without shell: true the argument is passed literally, never expanded"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_nonzero_exit_keeps_its_status_and_standard_error() {
        let (_dir, _data, _root, runs) = fixture("runs-exit");

        let result = runs
            .exec(argv("sh", &["-c", "echo err >&2; exit 3"]))
            .expect("sh runs");

        assert_eq!(result.exit_code, Some(3));
        assert_eq!(result.stderr, "err\n");
        assert!(!result.timed_out);
        assert!(!result.truncated);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_timeout_kills_the_process_group_and_reports_no_exit_code() {
        let (_dir, _data, root, runs) = fixture("runs-timeout");

        let result = runs
            .exec(ExecRequest {
                command: "sh".to_owned(),
                args: vec![
                    "-c".to_owned(),
                    "echo $$ > child.pid; exec sleep 5".to_owned(),
                ],
                // Two seconds, not 200 ms: the assertion needs the shell to have spawned and
                // written its pid before the deadline. A loaded runner can miss a 200 ms window and
                // then fail on the missing file rather than on the timeout's behaviour.
                timeout_ms: Some(2_000),
                ..ExecRequest::default()
            })
            .expect("sh runs");

        assert!(result.timed_out, "the 2 s timeout expired: {result:?}");
        assert_eq!(result.exit_code, None, "a killed process has no exit code");

        // The pid in the file is the shell's — and after `exec`, the sleep's. It must be gone:
        // killing only the direct child would leave a process group member behind.
        let pid: i32 = fs::read_to_string(root.join("child.pid"))
            .expect("the child wrote its pid")
            .trim()
            .parse()
            .expect("the pid is a number");
        assert!(
            wait_until_dead(pid as u32, Duration::from_secs(3)).await,
            "the process group is still running after a timeout (pid {pid})"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_timeout_reaches_a_background_process_in_the_group_too() {
        let (_dir, _data, root, runs) = fixture("runs-timeout-group");

        // The background sleep is a different process in the shell's process group. Its output is
        // redirected so it cannot keep the pipes open: if only the direct child were killed, it
        // would outlive the call and this test would see it.
        //
        // The foreground `exec sleep` (not `wait`) is deliberate: `wait` can return early on a shell
        // that reports a failed background spawn, and then `timed_out` is false for a reason that is
        // not the timeout. `exec` leaves one long-lived child in place. Two seconds, not 500 ms,
        // gives a loaded runner time to spawn the shell and for it to write the two pid files before
        // the deadline; a 500 ms deadline could otherwise fire before the writes and fail differently.
        let result = runs
            .exec(ExecRequest {
                command: "sh".to_owned(),
                args: vec![
                    "-c".to_owned(),
                    "sleep 5 >/dev/null 2>&1 & echo $! > background.pid; echo $$ > child.pid; exec sleep 5"
                        .to_owned(),
                ],
                timeout_ms: Some(2_000),
                ..ExecRequest::default()
            })
            .expect("sh runs");

        assert!(result.timed_out, "the 2 s timeout expired: {result:?}");
        let read_pid = |name: &str| -> i32 {
            fs::read_to_string(root.join(name))
                .expect("the child wrote its pid")
                .trim()
                .parse()
                .expect("the pid is a number")
        };
        let shell = read_pid("child.pid");
        let background = read_pid("background.pid");
        assert_ne!(shell, background, "the background job is its own process");
        for (label, pid) in [("shell", shell), ("background sleep", background)] {
            assert!(
                wait_until_dead(pid as u32, Duration::from_secs(3)).await,
                "the {label} (pid {pid}) must not be running after the timeout"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn output_over_the_cap_keeps_its_head_and_the_child_still_finishes() {
        let (_dir, _data, _root, runs) = fixture("runs-stdout-cap");

        let result = runs
            .exec(argv("sh", &["-c", "head -c 2097152 /dev/zero"]))
            .expect("the writer runs");

        assert_eq!(
            result.exit_code,
            Some(0),
            "the child finished because both pipes stayed drained: {:?}",
            result.exit_code
        );
        assert!(result.truncated, "2 MiB is over the 1 MiB stdout cap");
        assert_eq!(result.stdout.len(), MAX_STDOUT_BYTES);
        assert!(
            result.stdout.bytes().all(|byte| byte == 0),
            "the retained bytes are the head of what the child wrote"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stderr_is_capped_by_the_same_rule() {
        let (_dir, _data, _root, runs) = fixture("runs-stderr-cap");

        let result = runs
            .exec(argv("sh", &["-c", "head -c 2097152 /dev/zero >&2"]))
            .expect("the writer runs");

        assert_eq!(result.exit_code, Some(0));
        assert!(result.truncated);
        assert_eq!(result.stderr.len(), MAX_STDERR_BYTES);
    }

    #[cfg(unix)]
    #[test]
    fn a_cwd_outside_the_files_root_is_refused_before_the_command_runs() {
        let (dir, _data, root, runs) = fixture("runs-cwd-outside");
        let outside = dir.join("outside");
        fs::create_dir_all(&outside).expect("the directory outside the root");

        for cwd in ["../outside", "nested/../../outside"] {
            let error = runs
                .exec(ExecRequest {
                    command: "touch".to_owned(),
                    args: vec!["ran".to_owned()],
                    cwd: Some(cwd.to_owned()),
                    ..ExecRequest::default()
                })
                .expect_err("a cwd that leaves the root is refused");
            assert!(
                matches!(error, RunsError::Cwd(files::FilesError::Parent)),
                "{cwd}: {error:?}"
            );
        }

        let absolute = outside.to_str().expect("the temp path is UTF-8");
        let error = runs
            .exec(ExecRequest {
                command: "touch".to_owned(),
                args: vec!["ran".to_owned()],
                cwd: Some(absolute.to_owned()),
                ..ExecRequest::default()
            })
            .expect_err("an absolute cwd is outside the sandbox by definition");
        assert!(
            matches!(error, RunsError::Cwd(files::FilesError::Absolute)),
            "{error:?}"
        );

        assert!(!outside.join("ran").exists());
        assert!(!root.join("ran").exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_history_preview_is_capped_and_never_echoes_a_credential() {
        let (_dir, _data, _root, runs) = fixture("runs-preview");
        let long = format!("srm_deadbeef0123456789 {}", "x".repeat(PREVIEW_CHARS * 2));

        let result = runs
            .exec(argv("printf", &["%s", &long]))
            .expect("printf runs");
        assert_eq!(result.stdout, long, "the answer is exact, not the preview");

        let row = runs.get(&result.id).expect("the row reads back");
        assert!(
            row.stdout_preview.chars().count() <= PREVIEW_CHARS,
            "the preview is capped at {PREVIEW_CHARS} characters"
        );
        assert!(
            row.stdout_preview.contains("srm_***"),
            "the credential shape is replaced: {}",
            row.stdout_preview
        );
        assert!(
            !row.stdout_preview.contains("srm_deadbeef"),
            "the token itself is never written to the history"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_cwd_inside_the_files_root_is_where_the_command_starts() {
        let (_dir, _data, root, runs) = fixture("runs-cwd-inside");
        let inside = root.join("nested");
        fs::create_dir_all(&inside).expect("the nested directory");

        let result = runs
            .exec(ExecRequest {
                command: "pwd".to_owned(),
                cwd: Some("nested".to_owned()),
                ..ExecRequest::default()
            })
            .expect("pwd runs");

        let expected = fs::canonicalize(&inside).expect("the directory canonicalizes");
        assert_eq!(
            result.stdout.trim_end(),
            expected.to_string_lossy(),
            "the command starts in the resolved directory"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_run_is_recorded_with_its_result_and_read_back_by_id() {
        let (_dir, data, root, runs) = fixture("runs-history");

        let mut env = BTreeMap::new();
        env.insert("RUN_BODY".to_owned(), "body-not-in-history\n".to_owned());
        let with_body = runs
            .exec(ExecRequest {
                command: "sh".to_owned(),
                args: vec!["-c".to_owned(), "printf %s \"$RUN_BODY\"".to_owned()],
                env,
                ..ExecRequest::default()
            })
            .expect("the writer runs");
        assert_eq!(with_body.stdout, "body-not-in-history\n");

        assert_eq!(
            runs.get(&with_body.id)
                .expect("the row reads back")
                .stdout_preview,
            "body-not-in-history\n",
            "a short body is in the history as its preview, and the full answer is on the result"
        );

        let second = runs.exec(argv("echo", &["two"])).expect("echo runs");

        let rows = runs.list(20).expect("the history lists");
        assert_eq!(rows.len(), 2, "one call is one row");
        assert_eq!(rows[0].id, second.id, "list is newest first");
        assert_eq!(rows[1].id, with_body.id);
        assert_eq!(rows[0].command, "echo");
        assert_eq!(rows[0].args, ["two"]);
        assert_eq!(rows[0].exit_code, Some(0));
        assert!(!rows[0].timed_out);
        assert_eq!(rows[0].stdout_bytes, 4, "\"two\\n\" is four bytes");
        assert_eq!(rows[0].stderr_bytes, 0);
        assert!(rows[0].started_at_ms > 0);
        assert_eq!(
            rows[0].cwd,
            fs::canonicalize(&root)
                .expect("the root canonicalizes")
                .to_string_lossy()
                .into_owned()
        );

        assert_eq!(
            runs.get(&second.id).expect("the row is gettable"),
            rows[0],
            "get returns the same history row list returns"
        );
        assert!(
            matches!(runs.get("run-unknown"), Err(RunsError::UnknownRun)),
            "an id no row carries is an honest refusal"
        );

        let raw = fs::read_to_string(data.join(HISTORY_FILE)).expect("the history is on disk");
        assert!(
            raw.contains("\"stdout_preview\""),
            "the history carries a capped preview, not a body-only row: {raw}"
        );
        assert_eq!(raw.lines().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_history_write_failure_keeps_the_result_and_records_an_event() {
        use std::os::unix::fs::PermissionsExt;

        let (_dir, data, _root, runs) = fixture("runs-history-failure");
        let history = data.join(HISTORY_FILE);
        fs::write(&history, "").expect("the history fixture is writable");
        fs::set_permissions(&history, fs::Permissions::from_mode(0o500))
            .expect("the history fixture becomes read-only");

        let result = runs
            .exec(argv("echo", &["still-reported"]))
            .expect("a completed run is not failed by its history");

        assert_eq!(
            result.exit_code,
            Some(0),
            "the command still reports its status"
        );
        assert_eq!(
            result.stdout, "still-reported\n",
            "the output is still returned"
        );
        assert!(!result.history_recorded, "the history write failed");
        assert_eq!(
            fs::read_to_string(&history).expect("the read-only file is readable"),
            "",
            "a refused append leaves no torn row"
        );

        let events = fs::read_to_string(data.join("events.jsonl"))
            .expect("the history failure is recorded in the event log");
        let event: serde_json::Value =
            serde_json::from_str(events.lines().next().expect("one event line"))
                .expect("the event is JSON");
        assert_eq!(event["event"], "runs_history_failed");
        assert_eq!(event["id"], result.id);
        assert!(
            event["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty()),
            "the event carries the io error: {event}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn history_rotates_at_the_cap_and_keeps_the_older_rows_readable() {
        let dir = TempDir::new("runs-rotation");
        let data = dir.join("data");
        let root = dir.join("root");
        fs::create_dir_all(&data).expect("the data directory");
        fs::create_dir_all(&root).expect("the files root");
        let limits = Limits {
            history_lines: 3,
            ..Limits::default()
        };
        let runs = Runs::with_limits(&data, &root, limits);

        let mut ids = Vec::new();
        for index in 0..4 {
            let result = runs
                .exec(argv("echo", &[&index.to_string()]))
                .expect("echo runs");
            ids.push(result.id);
        }

        assert_eq!(
            fs::read_to_string(data.join(HISTORY_FILE))
                .expect("the current file exists")
                .lines()
                .count(),
            1,
            "the fourth row rotated the first three"
        );
        assert_eq!(
            fs::read_to_string(data.join(ROTATED_HISTORY_FILE))
                .expect("the rotated file exists")
                .lines()
                .count(),
            3
        );

        let rows = runs.list(10).expect("the history lists");
        assert_eq!(
            rows.len(),
            4,
            "list reads the current file and then the rotated one"
        );
        assert_eq!(rows[0].id, ids[3]);
        assert_eq!(rows[3].id, ids[0]);
        assert_eq!(
            runs.get(&ids[0])
                .expect("an old row is still gettable")
                .args,
            ["0"],
            "get searches the rotated file too"
        );
    }

    #[cfg(unix)]
    #[test]
    fn list_honours_its_limit_and_refuses_more_than_one_hundred() {
        let (_dir, _data, _root, runs) = fixture("runs-list-limit");
        for index in 0..3 {
            runs.exec(argv("echo", &[&index.to_string()]))
                .expect("echo runs");
        }

        assert_eq!(runs.list(2).expect("two rows").len(), 2);
        assert!(runs.list(0).expect("zero is legal").is_empty());
        assert!(
            matches!(
                runs.list(MAX_LIST_LIMIT + 1),
                Err(RunsError::LimitTooLarge {
                    limit: 101,
                    max: 100
                })
            ),
            "a limit over the cap is refused, not silently clamped"
        );
    }

    #[cfg(unix)]
    #[test]
    fn env_is_passed_to_the_child_and_capped() {
        let (_dir, _data, _root, runs) = fixture("runs-env");

        let mut env = BTreeMap::new();
        env.insert("RUN_MARK".to_owned(), "present".to_owned());
        let result = runs
            .exec(ExecRequest {
                command: "sh".to_owned(),
                args: vec!["-c".to_owned(), "printf %s \"$RUN_MARK\"".to_owned()],
                env,
                ..ExecRequest::default()
            })
            .expect("the child sees its environment");
        assert_eq!(result.stdout, "present");

        let mut too_many = BTreeMap::new();
        for index in 0..=MAX_ENV_VARS {
            too_many.insert(format!("K{index}"), "v".to_owned());
        }
        let error = runs
            .exec(ExecRequest {
                command: "true".to_owned(),
                env: too_many,
                ..ExecRequest::default()
            })
            .expect_err("a map over the count is refused");
        assert!(
            matches!(
                error,
                RunsError::TooManyEnv {
                    count: 33,
                    max: MAX_ENV_VARS
                }
            ),
            "{error:?}"
        );

        let mut too_large = BTreeMap::new();
        too_large.insert("BIG".to_owned(), "x".repeat(MAX_ENV_VALUE_BYTES + 1));
        let error = runs
            .exec(ExecRequest {
                command: "true".to_owned(),
                env: too_large,
                ..ExecRequest::default()
            })
            .expect_err("a value over the cap is refused");
        assert!(
            matches!(
                error,
                RunsError::EnvValueTooLarge {
                    len: 4097,
                    max: 4096
                }
            ),
            "{error:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_invalid_timeout_or_an_empty_command_is_refused_before_spawning() {
        let (_dir, _data, _root, runs) = fixture("runs-validation");

        for timeout_ms in [0, MAX_TIMEOUT_MS + 1] {
            let error = runs
                .exec(ExecRequest {
                    command: "echo".to_owned(),
                    timeout_ms: Some(timeout_ms),
                    ..ExecRequest::default()
                })
                .expect_err("the timeout is outside the accepted range");
            assert!(
                matches!(
                    error,
                    RunsError::InvalidTimeout {
                        timeout_ms: given,
                        max: MAX_TIMEOUT_MS
                    } if given == timeout_ms
                ),
                "{error:?}"
            );
        }

        assert!(matches!(
            runs.exec(ExecRequest::default()),
            Err(RunsError::EmptyCommand)
        ));

        let error = runs
            .exec(ExecRequest {
                command: "echo hi".to_owned(),
                args: vec!["unexpected".to_owned()],
                shell: true,
                ..ExecRequest::default()
            })
            .expect_err("the shell form and the argv form are not mixed");
        assert!(matches!(error, RunsError::ShellTakesNoArgs), "{error:?}");
    }

    #[cfg(unix)]
    #[test]
    fn shell_true_runs_the_command_through_the_platform_shell() {
        let (_dir, _data, _root, runs) = fixture("runs-shell");

        let result = runs
            .exec(ExecRequest {
                command: "printf shell-through-%s \"$0\"; true".to_owned(),
                shell: true,
                ..ExecRequest::default()
            })
            .expect("the shell runs the line");

        assert_eq!(result.exit_code, Some(0));
        assert_eq!(
            result.stdout, "shell-through-sh",
            "the platform shell parsed the line, so $0 expanded"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// The declaration's implementation: the device side of `summrise_core::capabilities::runs`.
// ---------------------------------------------------------------------------------------------

use serde_json::{json, Map, Value};
use std::sync::Arc;

use crate::mcp::{parse, ToolError};
use summrise_core::capabilities::runs::CAPABILITY;

/// Whether `tool` is one this capability declares.
pub fn owns(tool: &str) -> bool {
    CAPABILITY
        .tools
        .iter()
        .any(|declared| declared.name == tool)
}

/// Every tool this capability declares, for the registry-alignment test.
#[cfg(test)]
pub fn names() -> impl Iterator<Item = &'static str> {
    CAPABILITY.tools.iter().map(|tool| tool.name)
}

/// Run one of this capability's tools against the run history.
pub async fn call(
    runs: &Arc<Runs>,
    tool: &str,
    arguments: Map<String, Value>,
) -> Result<String, ToolError> {
    match tool {
        "runs.exec" => {
            let arguments: RunsExecArgs = parse(arguments, tool)?;
            // The run is a blocking wait for a child process; it belongs on the blocking pool, not
            // on a reactor worker that other MCP calls share.
            let runs = Arc::clone(runs);
            let request = ExecRequest {
                command: arguments.command,
                args: arguments.args,
                cwd: arguments.cwd,
                timeout_ms: arguments.timeout_ms,
                env: arguments.env,
                shell: arguments.shell,
            };
            let result = tokio::task::spawn_blocking(move || runs.exec(request))
                .await
                .map_err(|error| {
                    ToolError::Internal(format!("{tool}: the run task failed: {error}"))
                })?
                .map_err(|error| refused(tool, error))?;
            // **A RUN THAT DID NOT EXIT ZERO IS A FAILURE, EVEN WHEN THE TOOL RETURNED A VALUE.**
            // The run happened (its history row is written), but the call's outcome is the
            // process's, and the "failed" word the console draws comes from here.
            if result.exit_code != Some(0) {
                return Err(ToolError::ExecutedFailure {
                    message: format!("{tool}: {}", run_failure_sentence(&result)),
                    data: json!(result),
                });
            }
            Ok(json!(result).to_string())
        }
        "runs.list" => {
            let arguments: RunsListArgs = parse(arguments, tool)?;
            let rows = runs
                .list(arguments.limit.unwrap_or(DEFAULT_LIST_LIMIT))
                .map_err(|error| refused(tool, error))?;
            Ok(json!(rows).to_string())
        }
        "runs.get" => {
            let arguments: RunsIdArgs = parse(arguments, tool)?;
            let row = runs
                .get(&arguments.id)
                .map_err(|error| refused(tool, error))?;
            Ok(json!(row).to_string())
        }
        other => Err(ToolError::Internal(format!(
            "{other}: the runs capability does not declare this tool"
        ))),
    }
}

fn run_failure_sentence(run: &ExecResult) -> String {
    if run.timed_out {
        return format!("run {} timed out after {} ms", run.id, run.duration_ms);
    }
    match run.exit_code {
        Some(code) => format!("run {} exited {code} in {} ms", run.id, run.duration_ms),
        None => format!("run {} ended without an exit code", run.id),
    }
}

fn refused(tool: &str, error: impl std::fmt::Display) -> ToolError {
    ToolError::Refused(format!("{tool}: {error}"))
}

/// The arguments `runs.exec` takes.
#[allow(dead_code)] // constructed by serde, never by code
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunsExecArgs {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    timeout_ms: Option<u64>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    shell: bool,
}

/// The arguments `runs.list` takes.
#[allow(dead_code)] // constructed by serde, never by code
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunsListArgs {
    limit: Option<usize>,
}

/// The arguments `runs.get` takes.
#[allow(dead_code)] // constructed by serde, never by code
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunsIdArgs {
    id: String,
}
