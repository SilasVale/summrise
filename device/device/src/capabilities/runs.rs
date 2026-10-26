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
//! each stream, and `truncated` says whether either stream produced more. Both pipes are drained
//! concurrently even after their cap is reached, so a command that writes more than the cap finishes
//! instead of blocking on a full pipe. `env` carries at most [`MAX_ENV_VARS`] variables with values of
//! at most [`MAX_ENV_VALUE_BYTES`] bytes, added to the device's own environment. `timeout_ms` defaults
//! to [`DEFAULT_TIMEOUT_MS`] and may not
//! exceed [`MAX_TIMEOUT_MS`]; on expiry the child's **process group** is killed on Unix (the same
//! lesson the PTY capability learned), the child is reaped, and the result is `exit_code: null` with
//! `timed_out: true`.
//!
//! History is one JSON line per run in `<data>/runs.jsonl`, with the output bodies deliberately left
//! out: a row is `{id, command, args, cwd, exit_code, timed_out, duration_ms, started_at_ms,
//! stdout_bytes, stderr_bytes}`. The file holds at most [`HISTORY_LINES`] rows; the next run rotates
//! it to `<data>/runs.1.jsonl` first, so the newest rows are always in the current file. `list` and
//! `get` read the current file and then the rotated one, newest first.
//!
//! A history that cannot be written is not a failed run. The command already ran and its result is
//! returned with `history_recorded: false`; the failure is appended to `<data>/events.jsonl` as a
//! `runs_history_failed` event carrying the io error. Only a refusal before the spawn — validation,
//! a cwd outside the sandbox, or the spawn itself — fails the call and returns no result.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
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

/// How long a run may take when the caller names no timeout.
pub const DEFAULT_TIMEOUT_MS: u64 = 60_000;

/// The longest timeout a caller may ask for.
pub const MAX_TIMEOUT_MS: u64 = 10 * 60 * 1000;

/// The most environment variables one run may set.
pub const MAX_ENV_VARS: usize = 32;

/// The most bytes one environment value may carry.
pub const MAX_ENV_VALUE_BYTES: usize = 4096;

/// How many history rows the current file holds before it is rotated.
pub const HISTORY_LINES: usize = 10_000;

/// How many rows `runs.list` returns when the caller names no limit.
pub const DEFAULT_LIST_LIMIT: usize = 20;

/// The most rows `runs.list` will return.
pub const MAX_LIST_LIMIT: usize = 100;

/// The history file under `<data>`.
pub const HISTORY_FILE: &str = "runs.jsonl";

/// The rotated history file under `<data>`.
pub const ROTATED_HISTORY_FILE: &str = "runs.1.jsonl";

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
/// bytes; `truncated` is true when either stream produced more than its cap. A null `exit_code` with
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
    /// Whether either stream produced more than its cap.
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
    /// How many bytes standard output produced in total.
    pub stdout_bytes: u64,
    /// How many bytes standard error produced in total.
    pub stderr_bytes: u64,
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
    /// The working directory did not resolve to a directory inside the files root.
    Cwd(files::FilesError),
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
            RunsError::InvalidEnv => write!(
                f,
                "an environment name must be non-empty and contain no '=' or NUL; a value contains no NUL"
            ),
            RunsError::Cwd(error) => write!(f, "the working directory: {error}"),
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
        let mut child = command.spawn().map_err(RunsError::Io)?;
        let pid = child.id();

        // Both pipes are drained on their own threads from the first byte: capping what is *kept*
        // must never stop what is *read*, or a child writing past the cap blocks on a full pipe.
        let stdout_cap = self.limits.stdout_bytes;
        let stderr_cap = self.limits.stderr_bytes;
        let stdout_reader = child
            .stdout
            .take()
            .map(|stream| thread::spawn(move || capture(stream, stdout_cap)));
        let stderr_reader = child
            .stderr
            .take()
            .map(|stream| thread::spawn(move || capture(stream, stderr_cap)));

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
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(any(unix, windows)))]
    let _ = pid;

    let _ = child.kill();
}

/// One stream's retained head, its total size, and whether the cap cut it.
#[derive(Default)]
struct Captured {
    /// The head of the stream, at most the cap.
    bytes: Vec<u8>,
    /// Every byte the stream produced, including the ones not retained.
    total: u64,
    /// Whether more bytes arrived than were retained.
    truncated: bool,
}

/// Drain `stream` to EOF, retaining at most `cap` bytes at the head.
///
/// The loop never stops early: once the buffer is full the remaining bytes are counted and dropped,
/// so the child keeps writing and finishes.
fn capture(mut stream: impl Read, cap: usize) -> Captured {
    let mut captured = Captured::default();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                captured.total += read as u64;
                let room = cap.saturating_sub(captured.bytes.len());
                let keep = room.min(read);
                captured.bytes.extend_from_slice(&chunk[..keep]);
                if keep < read {
                    captured.truncated = true;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            // A read error ends this stream's capture; the child's exit status remains the truth
            // about the run, and an output pipe that broke is not a reason to lose that status.
            Err(_) => break,
        }
    }
    captured
}

/// Join a capture thread, treating a panicked reader as an empty stream.
fn join_capture(handle: Option<thread::JoinHandle<Captured>>) -> Captured {
    handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

/// The rows of one history file, newest first.
///
/// A line that does not parse is skipped: a process stopped mid-append can leave a torn final line,
/// and one unreadable row must not make the rest of the history unavailable.
fn read_newest_first(path: &Path) -> Result<Vec<Record>, RunsError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(RunsError::Io(error)),
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
fn count_lines(path: &Path) -> Result<usize, RunsError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text.lines().count()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(RunsError::Io(error)),
    }
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

/// Lock the history mutex, recovering the data from a poisoned lock instead of panicking.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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
    fn a_run_is_recorded_without_its_output_body_and_read_back_by_id() {
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
            !raw.contains("body-not-in-history"),
            "the output body is not stored: {raw}"
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
