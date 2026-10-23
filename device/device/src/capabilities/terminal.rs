//! The terminal capability (design §4.5): local pseudo-terminals, remote SSH shells and serial
//! lines.
//!
//! One [`PtyRegistry`] per device, one session per `create`. A PTY session owns a portable-pty
//! master, its writer half, the child and a reader thread; an SSH session owns a russh connection,
//! its channel and a reader task (see [`ssh`]); a serial session owns an open port, its writer half
//! and a reader thread. Every reader drains into the same bounded ring buffer, so a line that writes
//! faster than its client reads fills the ring rather than the transport: the device never stops
//! reading, and the far end never blocks on output.
//!
//! `read` takes a **cursor** — the absolute number of bytes the session has produced — and returns
//! the bytes after it and the new cursor. The cursor is the client's resume token: a second call
//! with the returned cursor returns exactly what arrived in between. The ring keeps the newest
//! [`BUFFER_BYTES`]; a cursor older than that window is a typed refusal, because bytes that were
//! dropped cannot be invented.
//!
//! The caps are the capability's, not a client convention: at most [`MAX_SESSIONS`] live sessions,
//! [`BUFFER_BYTES`] retained per session, [`MAX_INPUT_BYTES`] per write, and a session that no client
//! has read, written or resized for [`IDLE_TIMEOUT_MS`] is closed by the sweeper. A closed session —
//! explicitly, by the sweeper, or by the registry being dropped — has its child killed; its id is
//! never reused, and every later operation on it is [`PtyError::UnknownSession`].
//!
//! The state document is the device's, and this capability is part of the device, so `create` and
//! `close` update `state.sessions` themselves. They do it while holding the same lock the audit log
//! uses: the session table and the side-effect record then have one writer, and two creates cannot
//! lose one another's row to a read-modify-write of `state.json`.
//!
//! A session is unsandboxed by design — a PTY's shell runs as the device's user (SYSTEM for the
//! service) — which is why `terminal.create` and `terminal.write` are destructive tools.
//!
//! A serial session is not a child process. There is no process group to signal and no exit to
//! wait for: `close` closes the handle, and the reader stops on the same output flag a PTY's reader
//! does. It also has no timeout of its own — the idle sweeper and an explicit `close` are what end
//! it; the registry's write bound still applies to one blocking write, because that is the shared
//! path, not a serial-specific limit. A serial line has no rows or columns to reshape: `resize`
//! succeeds as a no-op, so a client that resizes every session uniformly does not have to know
//! which kinds have a window.
//!
//! A port belongs to one live session. The registry claims the path before opening it, so a second
//! create on a held port is [`PtyError::PortBusy`] naming the holder instead of a second writer
//! interleaving bytes with the first; the claim follows the session and is released by `close`, the
//! sweeper, or a create that fails before a session is published.
//!
//! A session's kind is part of its type: [`Kind`] has one variant per backend this build can run,
//! and [`Backend`] carries the backend-specific half. `write`, `read` and `close` are shared by
//! every kind; only `resize` and the create path know what a kind is. Adding a kind therefore means
//! a new enum variant and a new create arm, not a new write/read/close path.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use nix::sys::signal::{kill, killpg, Signal};
#[cfg(unix)]
use nix::unistd::Pid;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serialport::{DataBits, Parity as SerialParity, SerialPort, StopBits};
use summrise_core::{SessionKind, SessionRow, SessionState};

use crate::capabilities::ssh;
use crate::store::Store;

/// How many sessions one device may have live at once.
pub const MAX_SESSIONS: usize = 16;

/// How many output bytes one session retains for its client.
pub const BUFFER_BYTES: usize = 1024 * 1024;

/// The largest input one `terminal.write` call may carry.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;

/// How long one `write` may wait before the tool returns a timeout.
///
/// A shell that stops reading its input (a paused child, a full line-discipline queue) makes the
/// write block on the master descriptor. The write runs on a blocking thread and the caller waits
/// with this bound, so one wedged session cannot hold an async worker, let alone the server.
pub const WRITE_TIMEOUT_MS: u64 = 5_000;

/// The slice size one writer task hands to the PTY.
///
/// A single `write_all` of a megabyte can only stop when the whole payload is in the queue. Chunks
/// let the task check its deadline between writes, and they keep one call from asking the kernel to
/// accept more than a terminal's queue can hold at once.
const WRITE_CHUNK_BYTES: usize = 1024;

/// The largest row or column count a PTY may be given.
pub const MAX_DIMENSION: u16 = 1000;

/// The baud rate a serial create uses when the caller names none: the conventional console rate.
pub const DEFAULT_BAUD: u32 = 115_200;

/// The data bits a serial create uses when the caller names none.
pub const DEFAULT_DATA_BITS: u8 = 8;

/// The stop bits a serial create uses when the caller names none.
pub const DEFAULT_STOP_BITS: u8 = 1;

/// How long one serial read may wait for bytes.
///
/// The reader uses it as a poll interval so a close is noticed promptly; `serialport` applies the
/// same timeout to one write, so no single syscall can park a blocking task indefinitely.
const SERIAL_READ_TIMEOUT: Duration = Duration::from_millis(50);

/// How long a session may go without a read, write or resize before the sweeper closes it.
pub const IDLE_TIMEOUT_MS: u64 = 30 * 60 * 1000;

/// How often the sweeper looks for idle sessions.
pub const SWEEP_INTERVAL_MS: u64 = 60 * 1000;

/// The reader's chunk size; one PTY read never asks for more than this.
const READ_CHUNK_BYTES: usize = 8192;

/// How long the reader waits after a `WouldBlock` (the patched ConPTY reader's no-data answer).
const WOULD_BLOCK_POLL_MS: u64 = 10;

/// The device's clock, in Unix milliseconds, as a value a test can replace.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// The limits a registry enforces; they are data so a test does not have to spawn 16 shells to prove
/// the cap, and so a reader can see every cap in one place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most live sessions.
    pub max_sessions: usize,
    /// The output bytes retained per session.
    pub buffer_bytes: usize,
    /// The input bytes one write may carry.
    pub max_input_bytes: usize,
    /// How long one write may wait before it is refused. The blocking thread keeps its gate until it
    /// returns, so a second write to the same session fails fast instead of piling up behind it.
    pub write_timeout_ms: u64,
    /// How long a session may be idle.
    pub idle_timeout_ms: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_sessions: MAX_SESSIONS,
            buffer_bytes: BUFFER_BYTES,
            max_input_bytes: MAX_INPUT_BYTES,
            write_timeout_ms: WRITE_TIMEOUT_MS,
            idle_timeout_ms: IDLE_TIMEOUT_MS,
        }
    }
}

/// A session's id: minted by the registry, stable for the session's life, never reused.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SessionId {
    /// The wire spelling, `session-<number>`.
    text: String,
    /// The number, kept beside the text so ordering does not re-parse it.
    number: u64,
}

impl SessionId {
    /// Mint the id for `number`. Numbering starts at 1 and only ever increases.
    fn mint(number: u64) -> SessionId {
        SessionId {
            text: format!("session-{number}"),
            number,
        }
    }

    /// Parse an id a client sent back. A well-formed but unknown id is not a parse error — it is
    /// [`PtyError::UnknownSession`] from the operation — so this only checks the wire spelling.
    pub fn parse(text: &str) -> Option<SessionId> {
        let digits = text.strip_prefix("session-")?;
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        Some(SessionId {
            text: text.to_owned(),
            number: digits.parse().ok()?,
        })
    }

    /// The wire spelling.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The creation number, used only to order a list deterministically.
    fn number(&self) -> u64 {
        self.number
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// What one live session is attached to.
///
/// This is the registry's own enum, not a free-form string: a session's kind is part of its type,
/// so `list` and the state row cannot disagree about it. A new kind is a new variant here, a new
/// [`Backend`] arm, and a new arm in `create`; the shared write/read/resize/close paths do not
/// change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A pseudo-terminal on this machine.
    Pty,
    /// A remote shell over SSH.
    Ssh,
    /// A serial port named by the caller.
    Serial,
}

impl Kind {
    /// Every kind this build can create, in the order a refusal names them.
    const SUPPORTED: &'static [Kind] = &[Kind::Pty, Kind::Ssh, Kind::Serial];

    /// The wire spelling, which is also the state document's kind spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Pty => "pty",
            Kind::Ssh => "ssh",
            Kind::Serial => "serial",
        }
    }

    /// Parse the `kind` a client sent, refusing one this build cannot create.
    ///
    /// The refusal is typed and names what is supported: asking for `telnet` is not a schema
    /// error, it is a kind this device does not run.
    pub fn parse(text: &str) -> Result<Kind, PtyError> {
        match text {
            "pty" => Ok(Kind::Pty),
            "ssh" => Ok(Kind::Ssh),
            "serial" => Ok(Kind::Serial),
            other => Err(PtyError::UnsupportedKind {
                kind: other.to_owned(),
                supported: Kind::SUPPORTED
                    .iter()
                    .map(|kind| kind.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            }),
        }
    }
}

impl From<Kind> for SessionKind {
    fn from(kind: Kind) -> SessionKind {
        match kind {
            Kind::Pty => SessionKind::Pty,
            Kind::Ssh => SessionKind::Ssh,
            Kind::Serial => SessionKind::Serial,
        }
    }
}

/// What one `create` attaches to.
///
/// The kind is not a separate argument: a target knows its own kind, so a caller cannot ask for a
/// pseudo-terminal and hand it an SSH host.
#[derive(Clone, Copy, Debug)]
pub enum CreateTarget<'a> {
    /// A local shell on a new pseudo-terminal; `None` runs the platform default.
    Pty {
        /// The program to run, or `None` for [`default_shell`].
        shell: Option<&'a str>,
    },
    /// A remote shell over SSH.
    Ssh(SshTarget<'a>),
    /// A serial port the caller named; the capability never enumerates or guesses one.
    Serial(SerialTarget<'a>),
}

/// A serial line: the OS port path and the framing this build will apply to it.
///
/// The path is the caller's. On Windows that is a `COM3`-style name; on Unix any tty path the
/// operator passes, conventionally `/dev/ttyUSB0`. This capability has no port list and never
/// substitutes one: an absent or unknown path is the refusal, not a reason to probe for devices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerialTarget<'a> {
    /// The OS port path, e.g. `COM3` or `/dev/ttyUSB0`.
    pub port: &'a str,
    /// Bits per second. The port rejects a rate it cannot produce; this build does not fall back.
    pub baud: u32,
    /// The parity mode.
    pub parity: Parity,
    /// Data bits per character; `5`, `6`, `7` or `8`.
    pub data_bits: u8,
    /// Stop bits per character; `1` or `2`.
    pub stop_bits: u8,
}

impl<'a> SerialTarget<'a> {
    /// The conventional console default: 115200 baud, 8 data bits, no parity, 1 stop bit.
    pub fn new(port: &'a str) -> SerialTarget<'a> {
        SerialTarget {
            port,
            baud: DEFAULT_BAUD,
            parity: Parity::None,
            data_bits: DEFAULT_DATA_BITS,
            stop_bits: DEFAULT_STOP_BITS,
        }
    }
}

/// The parity of a serial line. The wire spells it in lower case; parsing is case-insensitive
/// because the old agent accepted either and refusing `Even` would only be a spelling test.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Parity {
    /// No parity bit.
    #[default]
    None,
    /// The parity bit makes the number of one bits odd.
    Odd,
    /// The parity bit makes the number of one bits even.
    Even,
}

impl Parity {
    /// Parse the `parity` a client sent, refusing an unrecognized mode instead of defaulting to
    /// none: a typo must not open the port with a wire format that corrupts every frame.
    pub fn parse(text: &str) -> Result<Parity, PtyError> {
        if text.eq_ignore_ascii_case("none") {
            Ok(Parity::None)
        } else if text.eq_ignore_ascii_case("odd") {
            Ok(Parity::Odd)
        } else if text.eq_ignore_ascii_case("even") {
            Ok(Parity::Even)
        } else {
            Err(PtyError::SerialConfig {
                field: "parity",
                value: text.to_owned(),
                accepted: "none, odd or even",
            })
        }
    }

    /// The serialport spelling this build applies.
    fn backend(self) -> SerialParity {
        match self {
            Parity::None => SerialParity::None,
            Parity::Odd => SerialParity::Odd,
            Parity::Even => SerialParity::Even,
        }
    }
}

/// The SSH half of a create.
#[derive(Clone, Copy, Debug)]
pub struct SshTarget<'a> {
    /// The server's name or address.
    pub host: &'a str,
    /// The server's port.
    pub port: u16,
    /// The user the shell runs as.
    pub user: &'a str,
    /// The credentials; `Debug` is redacted by [`ssh::Auth`].
    pub auth: &'a ssh::Auth,
    /// The caller's explicit SHA-256 fingerprint, or `None` for trust on first use.
    pub host_key: Option<&'a str>,
}

impl CreateTarget<'_> {
    /// The kind this target creates.
    pub fn kind(&self) -> Kind {
        match self {
            CreateTarget::Pty { .. } => Kind::Pty,
            CreateTarget::Ssh(_) => Kind::Ssh,
            CreateTarget::Serial(_) => Kind::Serial,
        }
    }

    /// The label `list` reports as the session's shell: the program, `user@host:port`, or the port
    /// path a serial line is attached to.
    pub fn label(&self) -> String {
        match self {
            CreateTarget::Pty { shell } => resolve_shell(*shell),
            CreateTarget::Ssh(target) => {
                format!("{}@{}:{}", target.user, target.host, target.port)
            }
            CreateTarget::Serial(target) => target.port.to_owned(),
        }
    }
}

/// One live session, as `list` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    /// The session's id.
    pub id: SessionId,
    /// What the session is attached to.
    pub kind: Kind,
    /// When the session was created, on the device's clock.
    pub started_at_ms: u64,
    /// The program running in it, `user@host:port` for a remote shell, or the port path for a
    /// serial line.
    pub shell: String,
    /// The terminal's row count.
    pub rows: u16,
    /// The terminal's column count.
    pub cols: u16,
    /// How many bytes the session has produced; a read from this cursor returns only new output.
    pub cursor: u64,
    /// Whether the far end is still producing and the session has not been closed.
    pub alive: bool,
    /// The child's process id for a PTY; `None` for an SSH or serial session, which has no child.
    pub pid: Option<u32>,
}

/// Why a terminal operation was refused or failed.
///
/// The variants are typed rather than stringly: the MCP surface renders them, and a test names the
/// rule it is proving.
#[derive(Debug)]
pub enum PtyError {
    /// The device already holds [`Limits::max_sessions`] live sessions.
    TooManySessions {
        /// The cap.
        max: usize,
    },
    /// The id is not a live session: it never existed or it has been closed.
    UnknownSession {
        /// The id the caller sent.
        id: String,
    },
    /// The requested kind is a session kind, but this build cannot create it yet.
    UnsupportedKind {
        /// The kind the caller asked for.
        kind: String,
        /// The kinds this build can create.
        supported: String,
    },
    /// The cursor is older than the newest [`Limits::buffer_bytes`] the ring kept.
    CursorTooOld {
        /// The caller's cursor.
        cursor: u64,
        /// The oldest cursor the ring can still serve.
        earliest: u64,
    },
    /// The cursor is past the session's stream; a resume token cannot be ahead of the output.
    CursorAhead {
        /// The caller's cursor.
        cursor: u64,
        /// The session's current end.
        total: u64,
    },
    /// A row or column count is outside `1..=MAX_DIMENSION`.
    InvalidSize {
        /// The requested rows.
        rows: u16,
        /// The requested columns.
        cols: u16,
        /// The largest accepted dimension.
        max: u16,
    },
    /// One write carried more than [`Limits::max_input_bytes`].
    InputTooLarge {
        /// The payload's size.
        bytes: usize,
        /// The cap.
        max: usize,
    },
    /// A write did not finish within [`Limits::write_timeout_ms`]; the shell is not reading its input.
    ///
    /// The blocking task may still be parked on the writer; while it is, [`PtyError::WriteBusy`] is
    /// what a concurrent write gets.
    WriteTimeout {
        /// The session the write was for.
        id: String,
        /// The bound that elapsed.
        timeout_ms: u64,
    },
    /// A previous write to this session is still blocked, so this one was not attempted.
    WriteBusy {
        /// The session the write was for.
        id: String,
    },
    /// portable-pty refused an operation (open, spawn, resize, reader clone).
    Backend(String),
    /// The SSH backend refused the connection or an operation on it.
    Ssh(ssh::SshError),
    /// A serial port could not be opened or configured. `message` is the backend's report, which on
    /// Unix and Windows carries the OS error; it is never replaced with a guess about the cause.
    Serial {
        /// The port path the caller named.
        port: String,
        /// The OS/backend report, including a refused baud or framing ioctl.
        message: String,
    },
    /// Another live session already holds the serial port this create named.
    PortBusy {
        /// The port path the caller named.
        port: String,
        /// The live session that holds it.
        holder: String,
    },
    /// A serial framing value this build does not accept. Refusing is the point: defaulting a typo
    /// to 8N1 would open the port with the wrong wire format and corrupt every frame.
    SerialConfig {
        /// The field, e.g. `baud` or `parity`.
        field: &'static str,
        /// The value the caller gave, as given.
        value: String,
        /// What this build accepts.
        accepted: &'static str,
    },
    /// The operation has no meaning for this session's kind. No current kind uses it — a serial
    /// resize is a successful no-op — but it remains the typed answer for a future kind that
    /// genuinely cannot perform an operation, instead of a stringly refusal.
    #[allow(dead_code)]
    UnsupportedOperation {
        /// The session's kind.
        kind: Kind,
        /// The operation that was refused.
        operation: &'static str,
    },
    /// The session could not be recorded in or removed from the state document.
    State(String),
    /// Writing to the session's input failed.
    Io(io::Error),
}

impl fmt::Display for PtyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PtyError::TooManySessions { max } => {
                write!(
                    f,
                    "the device already holds {max} live sessions; close one first"
                )
            }
            PtyError::UnknownSession { id } => {
                write!(f, "session {id} is unknown or has been closed")
            }
            PtyError::UnsupportedKind { kind, supported } => write!(
                f,
                "session kind {kind:?} is not supported by this build; this device supports: {supported}"
            ),
            PtyError::CursorTooOld { cursor, earliest } => write!(
                f,
                "the cursor {cursor} is older than the retained output, which starts at {earliest}"
            ),
            PtyError::CursorAhead { cursor, total } => {
                write!(f, "the cursor {cursor} is past the session's end ({total})")
            }
            PtyError::InvalidSize { rows, cols, max } => {
                write!(
                    f,
                    "a terminal size is {rows}x{cols}; rows and columns are 1..={max}"
                )
            }
            PtyError::InputTooLarge { bytes, max } => {
                write!(
                    f,
                    "the input is {bytes} bytes; one write carries at most {max}"
                )
            }
            PtyError::WriteTimeout { id, timeout_ms } => write!(
                f,
                "session {id}: the write did not finish within {timeout_ms} ms; the shell is not reading its input"
            ),
            PtyError::WriteBusy { id } => write!(
                f,
                "session {id}: a previous write is still blocked on this shell; this one was not attempted"
            ),
            PtyError::Backend(message) => write!(f, "{message}"),
            PtyError::Ssh(error) => write!(f, "{error}"),
            PtyError::Serial { port, message } => {
                write!(f, "serial port {port} could not be opened: {message}")
            }
            PtyError::PortBusy { port, holder } => write!(
                f,
                "serial port {port} is already open in session {holder}; close that session first"
            ),
            PtyError::SerialConfig {
                field,
                value,
                accepted,
            } => write!(
                f,
                "serial {field} {value:?} is not supported; this build accepts {accepted}"
            ),
            PtyError::UnsupportedOperation { kind, operation } => {
                write!(f, "a {} session has no {operation}", kind.as_str())
            }
            PtyError::State(message) => {
                write!(f, "could not update the state document: {message}")
            }
            PtyError::Io(error) => write!(f, "the session's input failed: {error}"),
        }
    }
}

impl std::error::Error for PtyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PtyError::Io(error) => Some(error),
            PtyError::Ssh(error) => Some(error),
            _ => None,
        }
    }
}

/// The bounded tail of one session's output.
///
/// `total` is the absolute stream length and never decreases; `bytes` holds the newest `cap` of it.
/// The difference between the two is the earliest cursor the ring can still serve.
struct Ring {
    bytes: VecDeque<u8>,
    total: u64,
    cap: usize,
}

impl Ring {
    fn new(cap: usize) -> Ring {
        Ring {
            bytes: VecDeque::with_capacity(cap.min(64 * 1024)),
            total: 0,
            cap,
        }
    }

    /// Append output, dropping the oldest bytes once the cap is reached.
    fn append(&mut self, chunk: &[u8]) {
        self.total = self.total.saturating_add(chunk.len() as u64);
        if self.cap == 0 {
            self.bytes.clear();
            return;
        }
        if chunk.len() >= self.cap {
            self.bytes.clear();
            self.bytes
                .extend(chunk[chunk.len() - self.cap..].iter().copied());
            return;
        }
        let overflow = (self.bytes.len() + chunk.len()).saturating_sub(self.cap);
        if overflow > 0 {
            self.bytes.drain(..overflow);
        }
        self.bytes.extend(chunk.iter().copied());
    }

    /// The absolute stream length: the cursor a client gets after reading everything.
    fn total(&self) -> u64 {
        self.total
    }

    /// The oldest cursor the ring can still serve.
    fn earliest(&self) -> u64 {
        self.total - self.bytes.len() as u64
    }

    /// Copy everything from `cursor` to the current end.
    fn read_from(&self, cursor: u64) -> Result<(Vec<u8>, u64), PtyError> {
        let earliest = self.earliest();
        if cursor > self.total {
            return Err(PtyError::CursorAhead {
                cursor,
                total: self.total,
            });
        }
        if cursor < earliest {
            return Err(PtyError::CursorTooOld { cursor, earliest });
        }
        let skip = (cursor - earliest) as usize;
        Ok((self.bytes.iter().skip(skip).copied().collect(), self.total))
    }
}

/// The reader thread's half of a session: the ring plus the two facts a client can observe.
struct Output {
    ring: Ring,
    /// The child's output ended (a natural exit or a read error), not necessarily a close.
    eof: bool,
    /// The session is being closed; the reader must stop.
    closed: bool,
}

/// One live session.
struct Session {
    id: SessionId,
    kind: Kind,
    /// The program the session runs: the shell of a PTY, or `user@host:port` for SSH.
    shell: String,
    /// When the session was created, on the device's clock.
    started_at_ms: u64,
    /// The backend-specific half. The shared paths reach it through [`Backend::write_handle`] and
    /// [`Backend::close`], so a new kind does not change them.
    backend: Backend,
    /// One write at a time. Set while a writer task owns the input; the task clears it when it
    /// returns. A timed-out blocking task keeps it set, so another write fails fast instead of
    /// parking behind a queue that is not moving.
    write_gate: Arc<AtomicBool>,
    output: Arc<Mutex<Output>>,
    rows: u16,
    cols: u16,
    last_active_ms: u64,
}

/// What one live session runs.
enum Backend {
    /// A portable-pty master and the shell behind it.
    Pty(PtyBackend),
    /// A remote shell over SSH.
    Ssh(SshBackend),
    /// An open serial port.
    Serial(SerialBackend),
}

/// The pseudo-terminal-specific half of a session.
struct PtyBackend {
    /// The master, kept for resize. The reader has its own duplicated descriptor.
    master: Box<dyn MasterPty + Send>,
    /// The input half, shared so a write does not hold the session map while it blocks.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    /// The child, shared with the reader so a natural exit is reaped instead of lingering.
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    /// The direct child's pid, captured at spawn. On Unix portable-pty starts the child with
    /// `setsid()`, so this is also the session id its descendants inherit.
    pid: Option<u32>,
}

/// The SSH-specific half of a session.
struct SshBackend {
    /// The connection and its writing half.
    session: Arc<ssh::SshSession>,
    /// The task draining the channel into the shared ring; aborted when the session is dropped.
    reader: Option<tokio::task::JoinHandle<()>>,
}

/// The serial-specific half of a session.
struct SerialBackend {
    /// The input half, shared so a write does not hold the session map while it blocks. The reader
    /// owns a `try_clone`d handle, so its blocking read never contends with a write for this one.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    /// The claim on this session's port. Dropping the backend releases it, which is what makes
    /// `close`, the sweeper, a failed state write and a failed create all release the port.
    _claim: SerialClaim,
    /// The reader thread, joined by `close`. Its cloned handle shares the port's exclusive flock,
    /// so the lock is gone when `close` returns rather than one poll interval later.
    reader: Option<thread::JoinHandle<()>>,
}

/// One live session's claim on a serial port.
///
/// The registry's claim map is the single source of exclusivity: a port is inserted before it is
/// opened, so a second create on the same path is refused by name instead of opening the line and
/// interleaving bytes with the holder. The guard removes exactly the key it inserted.
struct SerialClaim {
    ports: Arc<Mutex<HashMap<String, SessionId>>>,
    key: String,
}

impl Drop for SerialClaim {
    fn drop(&mut self) {
        lock(&self.ports).remove(&self.key);
    }
}

/// The input half a shared write path sends its payload to.
enum WriteHandle {
    /// A blocking writer: a portable-pty master or a serial port.
    Blocking(Arc<Mutex<Box<dyn Write + Send>>>),
    /// A remote shell's channel.
    Ssh(Arc<ssh::SshSession>),
}

impl Backend {
    /// The input half a write should use.
    fn write_handle(&self) -> WriteHandle {
        match self {
            Backend::Pty(backend) => WriteHandle::Blocking(Arc::clone(&backend.writer)),
            Backend::Ssh(backend) => WriteHandle::Ssh(Arc::clone(&backend.session)),
            Backend::Serial(backend) => WriteHandle::Blocking(Arc::clone(&backend.writer)),
        }
    }

    /// The direct child's pid: a PTY has one; a remote shell and a serial line have none.
    fn pid(&self) -> Option<u32> {
        match self {
            Backend::Pty(backend) => backend.pid,
            Backend::Ssh(_) | Backend::Serial(_) => None,
        }
    }

    /// The kind-specific teardown, called once by [`Session::drop`] after the output is marked
    /// closed.
    fn close(&mut self) {
        match self {
            Backend::Pty(backend) => backend.close(),
            Backend::Ssh(backend) => {
                // Ask the server to close the channel when a runtime is available; the dropped
                // connection handle ends the transport either way.
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    let session = Arc::clone(&backend.session);
                    handle.spawn(async move {
                        let _ = session.close().await;
                    });
                }
                if let Some(reader) = backend.reader.take() {
                    reader.abort();
                }
            }
            Backend::Serial(backend) => {
                // A serial session has no child to kill and no transport to ask: the output's
                // `closed` flag (set by `Session::drop` before this call) stops the reader. Joining
                // it waits at most one `SERIAL_READ_TIMEOUT` and guarantees the reader's cloned
                // handle is closed, so the port's exclusive lock is gone when `close` returns.
                if let Some(reader) = backend.reader.take() {
                    let _ = reader.join();
                }
            }
        }
    }
}

impl PtyBackend {
    /// Kill the shell and the descendants its session id reaches.
    fn close(&mut self) {
        // Killing the direct child is not enough on Unix: a background job (`sleep 30 &`) has the
        // child's session id but its own process group, so it survives both the child's death and a
        // plain `killpg`. The session scan is what closes it — but only while the pid is still ours.
        // A pid the reader reaped earlier may have been recycled, and signalling a recycled process
        // group is a use-after-reap hazard (the old agent's round-50). The reader takes the same
        // child lock when it sees a natural exit, so exactly one of the two paths closes the session.
        let mut child = lock(&self.child);
        match child.try_wait() {
            Ok(None) => {
                close_session(self.pid);
                let _ = child.kill();
                let _ = child.wait();
            }
            Ok(Some(_)) => {
                // `try_wait` just reaped the shell; the pid was ours until this call and cannot have
                // been recycled before the scan that follows.
                close_session(self.pid);
            }
            Err(_) => {
                // The reader reaped the shell earlier; its pid may now belong to another process, so
                // neither its group nor its session id is safe to signal. The reader's natural-exit
                // path killed the session's descendants before reaping it.
            }
        }
    }
}

impl Session {
    /// The current output cursor, as a new client would start from if it wants only new bytes.
    fn cursor(&self) -> u64 {
        lock(&self.output).ring.total()
    }

    /// Whether the session is still producing and has not been closed.
    fn alive(&self) -> bool {
        let output = lock(&self.output);
        !output.eof && !output.closed
    }

    /// The input half a shared write path sends its payload to.
    fn write_handle(&self) -> WriteHandle {
        self.backend.write_handle()
    }

    /// The direct child's pid, if the backend has one.
    fn pid(&self) -> Option<u32> {
        self.backend.pid()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Tell the reader to stop before the backend tears down: on Windows the PTY reader polls
        // this flag and must not keep reading a console that is going away, and the SSH reader task
        // is aborted once the flag is visible.
        lock(&self.output).closed = true;
        self.backend.close();
    }
}

/// The device's terminal sessions: local pseudo-terminals, remote SSH shells and serial lines.
pub struct PtyRegistry {
    sessions: Mutex<HashMap<SessionId, Session>>,
    /// The serial ports live sessions hold, keyed by the OS path (case-folded on Windows). A key is
    /// claimed before the port is opened and released when the session's backend drops, so `close`,
    /// the sweeper and a failed create all free it.
    serial_ports: Arc<Mutex<HashMap<String, SessionId>>>,
    next_number: AtomicU64,
    store: Store,
    /// The SSH host-key pins, one store per device.
    known_hosts: ssh::KnownHosts,
    /// The audit log's write lock: one lock serializes the state document's session rows with the
    /// audit's begin/end lines.
    state_lock: Arc<tokio::sync::Mutex<()>>,
    clock: Clock,
    limits: Limits,
}

impl PtyRegistry {
    /// A registry writing session rows through `store`, serialized by `state_lock`, with `limits`.
    ///
    /// The production caller passes [`Limits::default`]; an in-process test can shorten the write
    /// bound without changing the wire.
    pub fn new(
        store: Store,
        state_lock: Arc<tokio::sync::Mutex<()>>,
        limits: Limits,
    ) -> PtyRegistry {
        PtyRegistry::with_clock(store, state_lock, system_clock(), limits)
    }

    /// The testable constructor: the clock and every cap are arguments.
    pub(crate) fn with_clock(
        store: Store,
        state_lock: Arc<tokio::sync::Mutex<()>>,
        clock: Clock,
        limits: Limits,
    ) -> PtyRegistry {
        PtyRegistry {
            known_hosts: ssh::KnownHosts::at(store.data_dir()),
            sessions: Mutex::new(HashMap::new()),
            serial_ports: Arc::new(Mutex::new(HashMap::new())),
            next_number: AtomicU64::new(1),
            store,
            state_lock,
            clock,
            limits,
        }
    }

    /// Whether `lock` is the lock this registry serializes state rows with.
    ///
    /// Test-only: the MCP server must hand the registry the audit log's lock, and a test names that
    /// invariant rather than trusting a call site.
    #[cfg(test)]
    pub(crate) fn shares_state_lock(&self, lock: &Arc<tokio::sync::Mutex<()>>) -> bool {
        Arc::ptr_eq(&self.state_lock, lock)
    }

    /// Start the idle sweeper and the one startup reconciliation.
    ///
    /// The task holds only a weak reference, so dropping the registry ends the task at its next tick
    /// instead of keeping the device (and its children) alive. A registry built outside a Tokio
    /// runtime gets no sweeper; the tests drive `sweep_idle` themselves.
    pub fn spawn_sweeper(self: &Arc<Self>) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            eprintln!(
                "summrise-device: no Tokio runtime; the terminal sweeper is not running in this process"
            );
            return;
        };
        let weak = Arc::downgrade(self);
        handle.spawn(async move {
            // A previous process may have left rows behind. Reconcile before the first sweep so a
            // reader of /api/state sees live sessions, not a dead run's.
            if let Some(registry) = weak.upgrade() {
                registry.reconcile_state().await;
            }
            let mut ticker = tokio::time::interval(Duration::from_millis(SWEEP_INTERVAL_MS));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let Some(registry) = weak.upgrade() else {
                    break;
                };
                registry.sweep_idle().await;
            }
        });
    }

    /// Start a session for `target`, record it in the state document and return its id.
    ///
    /// The caller resolves the kind with [`Kind::parse`] while building the target, so a kind this
    /// build cannot create never reaches this method. The cap is
    /// checked before anything is opened and again before the session is published. The state row
    /// is written before the session is published to the registry: if the document cannot be
    /// updated the session is dropped (killing a PTY's child, closing an SSH connection) and the
    /// create fails, so a live session can never be invisible to `/api/state`.
    ///
    /// The SSH connect is awaited before the audit/state lock is taken: a handshake that takes
    /// seconds must not stop the audit log, and the second cap check keeps a concurrent create
    /// from pushing the registry past its limit.
    pub async fn create(
        &self,
        target: CreateTarget<'_>,
        rows: u16,
        cols: u16,
    ) -> Result<SessionId, PtyError> {
        validate_size(rows, cols)?;
        if lock(&self.sessions).len() >= self.limits.max_sessions {
            return Err(PtyError::TooManySessions {
                max: self.limits.max_sessions,
            });
        }

        let kind = target.kind();
        let session = match target {
            CreateTarget::Pty { shell } => self.spawn_pty(&resolve_shell(shell), rows, cols)?,
            CreateTarget::Ssh(target) => self.spawn_ssh(&target, rows, cols).await?,
            CreateTarget::Serial(target) => self.spawn_serial(&target, rows, cols).await?,
        };
        let id = session.id.clone();

        let _state = self.state_lock.lock().await;
        let mut sessions = lock(&self.sessions);
        if sessions.len() >= self.limits.max_sessions {
            drop(sessions);
            drop(_state);
            drop(session);
            return Err(PtyError::TooManySessions {
                max: self.limits.max_sessions,
            });
        }
        let row = SessionRow {
            id: id.as_str().to_owned(),
            kind: SessionKind::from(kind),
            state: SessionState::Attached,
        };
        // A failed save returns through `?`, dropping the not-yet-published session instead of
        // leaving a live one the state document does not know about.
        self.append_row(&row)?;
        sessions.insert(id.clone(), session);

        Ok(id)
    }

    /// Write UTF-8 bytes to a session's input.
    ///
    /// A PTY write is a blocking syscall: a shell that stops reading fills the line-discipline
    /// queue until it blocks. A serial write is blocking for the same reason: a flow-controlled
    /// line can fill the driver's buffer. So a blocking write runs on a blocking thread and this
    /// caller waits at most [`Limits::write_timeout_ms`]; the session map is never held while it
    /// blocks, and the async worker is free even when the far end is wedged. An SSH write is async
    /// and bounded by the same deadline.
    ///
    /// A timed-out PTY task keeps the session's write gate set until it returns, so a second write
    /// fails fast with [`PtyError::WriteBusy`] instead of parking another task behind the same
    /// queue. The task writes in [`WRITE_CHUNK_BYTES`] chunks and checks its deadline between them,
    /// so a large payload does not become one unbounded `write_all`.
    pub async fn write(&self, id: &SessionId, bytes: &[u8]) -> Result<usize, PtyError> {
        if bytes.len() > self.limits.max_input_bytes {
            return Err(PtyError::InputTooLarge {
                bytes: bytes.len(),
                max: self.limits.max_input_bytes,
            });
        }

        let (writer, gate) = {
            let mut sessions = lock(&self.sessions);
            let session = sessions.get_mut(id).ok_or_else(|| unknown(id))?;
            session.last_active_ms = (self.clock)();
            (session.write_handle(), Arc::clone(&session.write_gate))
        };

        if gate.swap(true, Ordering::SeqCst) {
            return Err(PtyError::WriteBusy {
                id: id.as_str().to_owned(),
            });
        }

        let timeout_ms = self.limits.write_timeout_ms;
        let payload = bytes.to_vec();
        match writer {
            WriteHandle::Blocking(writer) => {
                let task_writer = Arc::clone(&writer);
                let task_gate = Arc::clone(&gate);
                let task = tokio::task::spawn_blocking(move || {
                    // Whatever happens to this task — success, I/O error, deadline, panic — the
                    // gate is cleared only when the task is really done with the writer.
                    let _gate = WriteGate(task_gate);
                    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
                    let mut writer = lock(&task_writer);
                    write_chunks(&mut **writer, &payload, deadline)
                });

                match tokio::time::timeout(Duration::from_millis(timeout_ms), task).await {
                    Ok(Ok(Ok(()))) => Ok(bytes.len()),
                    Ok(Ok(Err(error))) if error.kind() == io::ErrorKind::TimedOut => {
                        Err(PtyError::WriteTimeout {
                            id: id.as_str().to_owned(),
                            timeout_ms,
                        })
                    }
                    Ok(Ok(Err(error))) => Err(PtyError::Io(error)),
                    Ok(Err(error)) => Err(PtyError::Backend(format!(
                        "session {}: the writer task failed: {error}",
                        id.as_str()
                    ))),
                    Err(_) => Err(PtyError::WriteTimeout {
                        id: id.as_str().to_owned(),
                        timeout_ms,
                    }),
                }
            }
            WriteHandle::Ssh(session) => {
                let task_gate = Arc::clone(&gate);
                let deadline = Instant::now() + Duration::from_millis(timeout_ms);
                let write = async move {
                    // The gate follows the write future: on success, error, or cancellation by the
                    // outer timeout, the next write is allowed to try.
                    let _gate = WriteGate(task_gate);
                    session.write(&payload, deadline).await
                };
                match tokio::time::timeout(Duration::from_millis(timeout_ms), write).await {
                    Ok(Ok(written)) => Ok(written),
                    Ok(Err(ssh::SshError::WriteTimeout)) => Err(PtyError::WriteTimeout {
                        id: id.as_str().to_owned(),
                        timeout_ms,
                    }),
                    Ok(Err(error)) => Err(PtyError::Ssh(error)),
                    Err(_) => Err(PtyError::WriteTimeout {
                        id: id.as_str().to_owned(),
                        timeout_ms,
                    }),
                }
            }
        }
    }

    /// Copy a session's output from `cursor` to the current end.
    pub fn read(&self, id: &SessionId, cursor: u64) -> Result<(Vec<u8>, u64), PtyError> {
        let mut sessions = lock(&self.sessions);
        let session = sessions.get_mut(id).ok_or_else(|| unknown(id))?;
        session.last_active_ms = (self.clock)();
        let read = lock(&session.output).ring.read_from(cursor);
        drop(sessions);
        read
    }

    /// Resize the terminal; a local shell receives `SIGWINCH` and reflows, an SSH shell gets a
    /// window-change request. A serial line has no window, so its resize is a successful no-op.
    ///
    /// The PTY's resize is synchronous and runs under the session map; SSH's awaits the channel, so
    /// it runs after the map is released and the dimensions are updated afterwards.
    pub async fn resize(&self, id: &SessionId, rows: u16, cols: u16) -> Result<(), PtyError> {
        validate_size(rows, cols)?;
        let ssh = {
            let mut sessions = lock(&self.sessions);
            let session = sessions.get_mut(id).ok_or_else(|| unknown(id))?;
            session.last_active_ms = (self.clock)();
            match &session.backend {
                Backend::Pty(backend) => {
                    backend
                        .master
                        .resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        })
                        .map_err(|error| {
                            PtyError::Backend(format!("cannot resize the pseudo-terminal: {error}"))
                        })?;
                    None
                }
                Backend::Ssh(backend) => Some(Arc::clone(&backend.session)),
                Backend::Serial(_) => {
                    // A line has no window to reflow. This is a successful no-op rather than an
                    // error: a client that resizes every session uniformly must not have to know
                    // which kinds have a window, and the old backend no-opped it too.
                    None
                }
            }
        };
        if let Some(session) = ssh {
            session.resize(rows, cols).await.map_err(PtyError::Ssh)?;
        }
        if let Some(session) = lock(&self.sessions).get_mut(id) {
            session.rows = rows;
            session.cols = cols;
        }
        Ok(())
    }

    /// Close a session, kill its child and remove its row from the state document.
    pub async fn close(&self, id: &SessionId) -> Result<(), PtyError> {
        let state_guard = self.state_lock.lock().await;
        let session = lock(&self.sessions).remove(id).ok_or_else(|| unknown(id))?;
        let result = self.remove_rows(&[id.as_str().to_owned()]);
        // Drop the session outside the state lock: the child's exit and the reader's wake-up do not
        // need to hold the audit lock.
        drop(state_guard);
        drop(session);
        result
    }

    /// The live sessions, oldest first.
    pub fn list(&self) -> Vec<SessionInfo> {
        let sessions = lock(&self.sessions);
        let mut infos: Vec<SessionInfo> = sessions
            .values()
            .map(|session| SessionInfo {
                id: session.id.clone(),
                kind: session.kind,
                started_at_ms: session.started_at_ms,
                shell: session.shell.clone(),
                rows: session.rows,
                cols: session.cols,
                cursor: session.cursor(),
                alive: session.alive(),
                pid: session.pid(),
            })
            .collect();
        infos.sort_by_key(|info| info.id.number());
        infos
    }

    /// Close every session that has been idle for the configured timeout.
    ///
    /// This is the sweeper's body, public so a test can inject a clock and call it exactly. It takes
    /// the audit/state lock because it edits `state.sessions`, and it drops the closed sessions after
    /// releasing that lock so a child's exit does not delay an audit line.
    pub async fn sweep_idle(&self) {
        let state_guard = self.state_lock.lock().await;
        let now = (self.clock)();
        let expired: Vec<Session> = {
            let mut sessions = lock(&self.sessions);
            let ids: Vec<SessionId> = sessions
                .iter()
                .filter(|(_, session)| {
                    now.saturating_sub(session.last_active_ms) >= self.limits.idle_timeout_ms
                })
                .map(|(id, _)| id.clone())
                .collect();
            ids.iter().filter_map(|id| sessions.remove(id)).collect()
        };
        if expired.is_empty() {
            return;
        }

        let ids: Vec<String> = expired
            .iter()
            .map(|session| session.id.as_str().to_owned())
            .collect();
        if let Err(error) = self.remove_rows(&ids) {
            eprintln!("summrise-device: the sweeper could not update the state document: {error}");
        }
        drop(state_guard);
        drop(expired);
    }

    /// Remove rows for terminal sessions this registry is not running.
    ///
    /// A process that died (or was stopped) leaves its `state.sessions` rows behind; the next start's
    /// registry knows which ids are live, so every other `pty`, `ssh` or `serial` row is a dead run's.
    /// Rows of other kinds belong to capabilities this registry does not own and are left alone.
    pub async fn reconcile_state(&self) {
        let _state = self.state_lock.lock().await;
        let live: HashSet<String> = lock(&self.sessions)
            .keys()
            .map(|id| id.as_str().to_owned())
            .collect();
        let mut state = match self.store.load() {
            Ok(state) => state,
            Err(error) => {
                eprintln!(
                    "summrise-device: cannot reconcile the terminal rows in the state document: {error}"
                );
                return;
            }
        };
        let before = state.sessions.len();
        state.sessions.retain(|row| {
            !matches!(
                row.kind,
                SessionKind::Pty | SessionKind::Ssh | SessionKind::Serial
            ) || live.contains(&row.id)
        });
        if state.sessions.len() == before {
            return;
        }
        if let Err(error) = self.store.save(&state) {
            eprintln!("summrise-device: cannot save the reconciled terminal rows: {error}");
        }
    }

    /// Open the PTY, spawn the shell, and start the reader thread.
    fn spawn_pty(&self, shell: &str, rows: u16, cols: u16) -> Result<Session, PtyError> {
        let number = self.next_number.fetch_add(1, Ordering::SeqCst);
        let id = SessionId::mint(number);
        let started_at_ms = (self.clock)();

        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| {
                PtyError::Backend(format!("cannot open a pseudo-terminal: {error}"))
            })?;

        // The shell inherits the device's environment and working directory; no capability-level
        // sandbox applies to a terminal.
        let command = CommandBuilder::new(shell);
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| PtyError::Backend(format!("cannot start {shell}: {error}")))?;

        // The child holds its own slave descriptor. Keeping this one would keep the master's reader
        // from ever seeing EOF after the shell exits (the old agent's round-87 trap): drop it here.
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().map_err(|error| {
            PtyError::Backend(format!("cannot read the pseudo-terminal: {error}"))
        })?;
        let writer = pair.master.take_writer().map_err(|error| {
            PtyError::Backend(format!("cannot write the pseudo-terminal: {error}"))
        })?;

        let output = Arc::new(Mutex::new(Output {
            ring: Ring::new(self.limits.buffer_bytes),
            eof: false,
            closed: false,
        }));
        // portable-pty's Unix spawn calls setsid(), so this pid is the child's session and process
        // group id; the descendants a close has to reach inherit it.
        let pid = child.process_id();
        let child = Arc::new(Mutex::new(child));
        let thread_output = Arc::clone(&output);
        let thread_child = Arc::clone(&child);
        let thread_name = format!("summrise-pty-reader-{id}");
        let thread = thread::Builder::new().name(thread_name);
        if let Err(error) = thread.spawn(move || {
            drain_reader(reader, thread_output, thread_child, pid);
        }) {
            let _ = lock(&child).kill();
            return Err(PtyError::Backend(format!(
                "cannot start the session's reader thread: {error}"
            )));
        }

        Ok(Session {
            id,
            kind: Kind::Pty,
            shell: shell.to_owned(),
            started_at_ms,
            backend: Backend::Pty(PtyBackend {
                master: pair.master,
                writer: Arc::new(Mutex::new(writer)),
                child,
                pid,
            }),
            write_gate: Arc::new(AtomicBool::new(false)),
            output,
            rows,
            cols,
            last_active_ms: started_at_ms,
        })
    }

    /// Connect one SSH session and start the task that drains it into the shared ring.
    ///
    /// The task is the SSH counterpart of [`spawn_pty`]'s reader thread: the same [`Ring`], the same
    /// caps, and the same `eof`/`closed` facts. It holds only the output half and the ring; the
    /// session's connection handle stays in [`SshBackend`], and dropping the session aborts the
    /// task after marking the output closed.
    async fn spawn_ssh(
        &self,
        target: &SshTarget<'_>,
        rows: u16,
        cols: u16,
    ) -> Result<Session, PtyError> {
        let number = self.next_number.fetch_add(1, Ordering::SeqCst);
        let id = SessionId::mint(number);
        let started_at_ms = (self.clock)();

        let mut session = ssh::connect(
            target.host,
            target.port,
            target.user,
            target.auth,
            rows,
            cols,
            target.host_key,
            &self.known_hosts,
        )
        .await
        .map_err(PtyError::Ssh)?;
        let Some(mut reader) = session.take_reader() else {
            return Err(PtyError::Backend(
                "the SSH session has no output half".to_owned(),
            ));
        };
        let label = session.label();
        let session = Arc::new(session);

        let output = Arc::new(Mutex::new(Output {
            ring: Ring::new(self.limits.buffer_bytes),
            eof: false,
            closed: false,
        }));
        let task_output = Arc::clone(&output);
        let reader_task = tokio::spawn(async move {
            while let Some(chunk) = reader.next_chunk().await {
                let mut output = lock(&task_output);
                if output.closed {
                    return;
                }
                output.ring.append(&chunk);
            }
            lock(&task_output).eof = true;
        });

        Ok(Session {
            id,
            kind: Kind::Ssh,
            shell: label,
            started_at_ms,
            backend: Backend::Ssh(SshBackend {
                session,
                reader: Some(reader_task),
            }),
            write_gate: Arc::new(AtomicBool::new(false)),
            output,
            rows,
            cols,
            last_active_ms: started_at_ms,
        })
    }

    /// Open one serial port and start the thread that drains it into the shared ring.
    ///
    /// The open is blocking (a flaky adapter can take seconds to answer), so it runs on the
    /// blocking pool and the caller waits without occupying a worker. The reader gets a cloned
    /// handle: its blocking read then never holds the writer's mutex, and a write cannot wait for a
    /// read's poll interval.
    ///
    /// Framing is validated before the port is touched, and a value the port itself rejects comes
    /// back as [`PtyError::Serial`] with the backend's message: this build never retries a rejected
    /// mode with a default one, because that would corrupt every frame on a line configured wrong.
    async fn spawn_serial(
        &self,
        target: &SerialTarget<'_>,
        rows: u16,
        cols: u16,
    ) -> Result<Session, PtyError> {
        let number = self.next_number.fetch_add(1, Ordering::SeqCst);
        let id = SessionId::mint(number);
        let started_at_ms = (self.clock)();
        let (data_bits, parity, stop_bits) = serial_framing(target)?;

        // Claim the path before opening it: two creates racing for the same line must not both
        // apply termios settings and then interleave bytes. The map is the only exclusivity; the
        // guard below frees it on every path out of this function, including a failed open.
        let key = serial_port_key(target.port);
        {
            let mut ports = lock(&self.serial_ports);
            if let Some(holder) = ports.get(&key) {
                return Err(PtyError::PortBusy {
                    port: target.port.to_owned(),
                    holder: holder.as_str().to_owned(),
                });
            }
            ports.insert(key.clone(), id.clone());
        }
        let claim = SerialClaim {
            ports: Arc::clone(&self.serial_ports),
            key,
        };

        let port = target.port.to_owned();
        let baud = target.baud;
        let opened = tokio::task::spawn_blocking(move || {
            open_serial(&port, baud, data_bits, parity, stop_bits)
        })
        .await
        .map_err(|error| PtyError::Backend(format!("the serial open task failed: {error}")))?;
        let port = opened?;
        let reader = port.try_clone().map_err(|error| PtyError::Serial {
            port: target.port.to_owned(),
            message: format!("cannot clone the opened handle for the reader: {error}"),
        })?;
        // The trait object is a `Write` (and `Send`) through its supertraits; the shared write path
        // only needs that half, and the reader owns the cloned one.
        let writer: Box<dyn Write + Send> = port;

        let output = Arc::new(Mutex::new(Output {
            ring: Ring::new(self.limits.buffer_bytes),
            eof: false,
            closed: false,
        }));
        let thread_output = Arc::clone(&output);
        let thread_name = format!("summrise-serial-reader-{id}");
        let thread = thread::Builder::new().name(thread_name);
        let reader = thread
            .spawn(move || {
                drain_serial(reader, thread_output);
            })
            .map_err(|error| {
                PtyError::Backend(format!("cannot start the session's reader thread: {error}"))
            })?;

        Ok(Session {
            id,
            kind: Kind::Serial,
            shell: target.port.to_owned(),
            started_at_ms,
            backend: Backend::Serial(SerialBackend {
                writer: Arc::new(Mutex::new(writer)),
                _claim: claim,
                reader: Some(reader),
            }),
            write_gate: Arc::new(AtomicBool::new(false)),
            output,
            rows,
            cols,
            last_active_ms: started_at_ms,
        })
    }

    /// Add one session row to the state document.
    fn append_row(&self, row: &SessionRow) -> Result<(), PtyError> {
        let mut state = self
            .store
            .load()
            .map_err(|error| PtyError::State(error.to_string()))?;
        state.sessions.retain(|existing| existing.id != row.id);
        state.sessions.push(row.clone());
        self.store
            .save(&state)
            .map_err(|error| PtyError::State(error.to_string()))
    }

    /// Remove the named rows from the state document.
    fn remove_rows(&self, ids: &[String]) -> Result<(), PtyError> {
        let mut state = self
            .store
            .load()
            .map_err(|error| PtyError::State(error.to_string()))?;
        state.sessions.retain(|row| !ids.contains(&row.id));
        self.store
            .save(&state)
            .map_err(|error| PtyError::State(error.to_string()))
    }
}

impl Drop for PtyRegistry {
    fn drop(&mut self) {
        // Dropping the device must not leave shells running. The registry is gone, so no other thread
        // can be using these sessions; each Drop kills and reaps its child.
        let sessions = self
            .sessions
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner());
        sessions.clear();
    }
}

/// The reader thread: drain the master into the ring, notice a natural exit, and stop on close.
fn drain_reader(
    mut reader: Box<dyn Read + Send>,
    output: Arc<Mutex<Output>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    pid: Option<u32>,
) {
    let mut buffer = [0u8; READ_CHUNK_BYTES];
    let mut child_exited = false;
    loop {
        if lock(&output).closed {
            return;
        }
        match reader.read(&mut buffer) {
            Ok(0) => {
                child_exited = true;
                break;
            }
            Ok(n) => lock(&output).ring.append(&buffer[..n]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                // The patched ConPTY reader reports no data this way, and the pipe never EOFs while
                // the HPCON lives. Poll the child instead: a natural exit ends the drain after the
                // bytes already buffered have been read.
                if child_exited {
                    break;
                }
                let mut child = lock(&child);
                match child.try_wait() {
                    Ok(Some(_)) | Err(_) => child_exited = true,
                    Ok(None) => {
                        drop(child);
                        thread::sleep(Duration::from_millis(WOULD_BLOCK_POLL_MS));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => {
                child_exited = true;
                break;
            }
        }
    }

    if child_exited {
        // A natural exit is reaped here; waiting for the sweeper would leave a zombie for up to half
        // an hour. Before reaping, close the session's descendants while the pid is still the
        // shell's — but only if this is a natural exit, not an explicit close: the child lock makes
        // the check and the close one step, and `Session::drop` takes the same lock before its
        // `try_wait`, so exactly one path closes the session while the pid is still ours.
        let mut child = lock(&child);
        if !lock(&output).closed {
            close_session(pid);
        }
        let _ = child.try_wait();
    }
    let mut output = lock(&output);
    output.eof = true;
}

/// The reader thread of a serial session: drain the port into the ring and stop when it closes.
///
/// `serialport` reports a read timeout as [`io::ErrorKind::TimedOut`] on Unix and as a zero-byte
/// read on Windows, and the port's timeout is [`SERIAL_READ_TIMEOUT`]; either is "no data this
/// poll", not an end. A zero-byte read on Unix is the far end's EOF (the pty master closed), which
/// ends the drain exactly as a PTY reader's EOF does.
fn drain_serial(mut reader: Box<dyn SerialPort>, output: Arc<Mutex<Output>>) {
    let mut buffer = [0u8; READ_CHUNK_BYTES];
    loop {
        if lock(&output).closed {
            return;
        }
        match reader.read(&mut buffer) {
            Ok(0) => {
                #[cfg(windows)]
                continue;
                #[cfg(not(windows))]
                break;
            }
            Ok(n) => lock(&output).ring.append(&buffer[..n]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => break,
        }
    }
    lock(&output).eof = true;
}

/// Open a serial port with the framing the caller asked for.
///
/// `serialport` opens and configures in one call, so an open error can be a path the OS cannot
/// open *or* a line mode the port refuses. Both are [`PtyError::Serial`] with the backend's own
/// message, which is where the OS error lives; this build does not retry with a default mode.
fn open_serial(
    port: &str,
    baud: u32,
    data_bits: DataBits,
    parity: SerialParity,
    stop_bits: StopBits,
) -> Result<Box<dyn SerialPort>, PtyError> {
    // Every caller reaches this from `spawn_blocking`, so the wait below never blocks a reactor.
    let deadline = Instant::now() + SERIAL_LOCK_PATIENCE;
    loop {
        let attempt = serialport::new(port, baud)
            .data_bits(data_bits)
            .parity(parity)
            .stop_bits(stop_bits)
            .timeout(SERIAL_READ_TIMEOUT)
            .open();
        match attempt {
            Ok(port) => return Ok(port),
            // The port is not in use; its last holder is still dying. Wait for it, inside the
            // bound, instead of telling the operator a free port is busy.
            Err(error) if is_exclusive_refusal(&error) && Instant::now() < deadline => {
                thread::sleep(SERIAL_LOCK_RETRY);
            }
            Err(error) => {
                return Err(PtyError::Serial {
                    port: port.to_owned(),
                    message: error.to_string(),
                })
            }
        }
    }
}

/// Whether the open was refused by the port's exclusivity rather than by the device being gone.
///
/// The refusal has two spellings on Linux and they are one condition: a tty whose `TIOCEXCL` a
/// dying holder still has set refuses the `open` with `EBUSY` ("Device or resource busy"), and one
/// whose `flock` is still held refuses with serialport's "Unable to acquire exclusive lock on
/// serial port". `serialport::ErrorKind` has no `Busy`, and the flock refusal is `NoDevice` — the
/// same kind a genuinely missing device has — so the description is the only signal that tells
/// "wait a moment" from "this port will never open".
fn is_exclusive_refusal(error: &serialport::Error) -> bool {
    const REFUSALS: [&str; 2] = ["exclusive lock", "busy"];
    REFUSALS
        .iter()
        .any(|spelling| error.description.contains(spelling))
}

/// How long a fresh open waits for a port whose exclusive lock a dying holder has not released yet.
///
/// `flock` belongs to the open file description, and `fork` copies the descriptor table: a child
/// forked while a serial session held the port keeps that description — and therefore the lock —
/// until it execs, because `O_CLOEXEC` acts at exec and not at fork. A PTY shell or a `runs.exec`
/// child forked in that window keeps a just-closed port locked for milliseconds, so an immediate
/// re-create would report a busy port that is, in fact, free. The wait is short and bounded: a port
/// another program really holds is still refused, a quarter of a second later.
const SERIAL_LOCK_PATIENCE: Duration = Duration::from_millis(250);

/// How long one retry waits before it asks for the lock again.
const SERIAL_LOCK_RETRY: Duration = Duration::from_millis(5);

/// The key a serial port is claimed under.
///
/// A Windows `COM` name is case-insensitive, so `COM3` and `com3` are one port; a Unix path is the
/// operator's exact path, and two spellings are two different files.
fn serial_port_key(port: &str) -> String {
    #[cfg(windows)]
    {
        port.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        port.to_owned()
    }
}

/// Reject a framing value this build does not accept, before a port is opened.
fn validate_serial(target: &SerialTarget<'_>) -> Result<(), PtyError> {
    if target.baud == 0 {
        return Err(PtyError::SerialConfig {
            field: "baud",
            value: target.baud.to_string(),
            accepted: "a positive rate",
        });
    }
    if !(5..=8).contains(&target.data_bits) {
        return Err(PtyError::SerialConfig {
            field: "data_bits",
            value: target.data_bits.to_string(),
            accepted: "5, 6, 7 or 8",
        });
    }
    if !(1..=2).contains(&target.stop_bits) {
        return Err(PtyError::SerialConfig {
            field: "stop_bits",
            value: target.stop_bits.to_string(),
            accepted: "1 or 2",
        });
    }
    Ok(())
}

/// Convert a validated target to the backend's framing types.
fn serial_framing(
    target: &SerialTarget<'_>,
) -> Result<(DataBits, SerialParity, StopBits), PtyError> {
    validate_serial(target)?;
    let data_bits = match target.data_bits {
        5 => DataBits::Five,
        6 => DataBits::Six,
        7 => DataBits::Seven,
        8 => DataBits::Eight,
        // `validate_serial` refused everything else.
        other => {
            return Err(PtyError::SerialConfig {
                field: "data_bits",
                value: other.to_string(),
                accepted: "5, 6, 7 or 8",
            });
        }
    };
    let stop_bits = match target.stop_bits {
        1 => StopBits::One,
        2 => StopBits::Two,
        other => {
            return Err(PtyError::SerialConfig {
                field: "stop_bits",
                value: other.to_string(),
                accepted: "1 or 2",
            });
        }
    };
    Ok((data_bits, target.parity.backend(), stop_bits))
}

/// Clears a session's write gate when its blocking writer task ends, including on panic.
struct WriteGate(Arc<AtomicBool>);

impl Drop for WriteGate {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Write `bytes` in chunks, stopping as soon as `deadline` has passed.
///
/// `write_all` on a PTY master blocks when the line-discipline queue is full, so the deadline is
/// checked between chunks rather than inside a syscall. The caller's `tokio::time::timeout` bounds
/// the call; this check keeps an abandoned task from writing megabytes after the client was told the
/// write did not finish.
fn write_chunks(
    writer: &mut (dyn Write + Send),
    bytes: &[u8],
    deadline: Instant,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the write deadline elapsed",
            ));
        }
        let end = (offset + WRITE_CHUNK_BYTES).min(bytes.len());
        writer.write_all(&bytes[offset..end])?;
        writer.flush()?;
        offset = end;
    }
    Ok(())
}

/// Close the session a child led, when the platform has a way to reach its descendants.
///
/// Windows terminates the direct child only; the ConPTY console's teardown is the platform's job.
/// The shell in a Unix child is a session leader (portable-pty calls `setsid()`), so this is where
/// the `sleep 30 &` case is handled.
#[cfg(unix)]
fn close_session(pid: Option<u32>) {
    if let Some(pid) = pid {
        kill_session(pid);
    }
}

/// Windows: closing the session is the direct child's kill, which `Session::drop` already does.
#[cfg(not(unix))]
fn close_session(_pid: Option<u32>) {}

/// Kill every process in the PTY child's session on Unix.
///
/// portable-pty starts the child with `setsid()`. That makes the child a session leader, and its
/// descendants inherit the session id even when the shell's job control puts a background job in a
/// new process group (the `sleep 30 &` case): `killpg(child)` kills the shell but not that job.
/// Linux exposes session membership through `/proc/<pid>/stat`, so members are signalled
/// individually; on other Unix only the process group can be reached, and a background group (or a
/// descendant that called `setsid` itself) can outlive the close.
#[cfg(unix)]
fn kill_session(session: u32) {
    // pid 0 means "this process group" to kill(2); pid 1 is init. Neither is a session we started.
    if session <= 1 {
        return;
    }

    let leader = Pid::from_raw(session as i32);
    let _ = killpg(leader, Signal::SIGKILL);

    #[cfg(target_os = "linux")]
    for pid in session_members(session) {
        // The scan can see this process if a test's own session id somehow matches; never signal it.
        if pid != std::process::id() {
            let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
        }
    }

    // The group kill already covers the shell; this covers a backend that does not call `setsid`
    // (then the leader is still the process to signal) and the non-Linux case.
    let _ = kill(leader, Signal::SIGKILL);
}

/// The pids in `session` according to `/proc/<pid>/stat` (Linux only).
#[cfg(target_os = "linux")]
fn session_members(session: u32) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut members = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // `pid (comm) state ppid pgrp session ...`; comm may contain spaces, so everything before its
        // closing parenthesis is skipped. A comm containing `) ` can still confuse the split — that is
        // the kernel's own ambiguous format — but the session id must still match, so the only
        // possible failure is a missed member, never a signal to a process from another session.
        let Some((_, fields)) = stat.rsplit_once(')') else {
            continue;
        };
        let mut fields = fields.split_whitespace();
        let _state = fields.next();
        let _ppid = fields.next();
        let _pgrp = fields.next();
        let Some(sid) = fields.next().and_then(|field| field.parse::<u32>().ok()) else {
            continue;
        };
        if sid == session {
            members.push(pid);
        }
    }
    members
}

/// A typed refusal for an id the caller sent.
fn unknown(id: &SessionId) -> PtyError {
    PtyError::UnknownSession {
        id: id.as_str().to_owned(),
    }
}

/// Reject a terminal size outside `1..=MAX_DIMENSION`.
fn validate_size(rows: u16, cols: u16) -> Result<(), PtyError> {
    if rows == 0 || cols == 0 || rows > MAX_DIMENSION || cols > MAX_DIMENSION {
        return Err(PtyError::InvalidSize {
            rows,
            cols,
            max: MAX_DIMENSION,
        });
    }
    Ok(())
}

/// The shell a create runs: the caller's non-empty program, or the platform default.
///
/// This is the single place the default is chosen, so the MCP surface can report the same shell the
/// capability started instead of guessing at it.
pub fn resolve_shell(shell: Option<&str>) -> String {
    shell
        .filter(|program| !program.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(default_shell)
}

/// The shell a create starts when the client names none.
///
/// `pwsh` is the only shell the old agent supports on Windows (Windows PowerShell 5.1's PSReadLine
/// re-echoes console input and draws continuation prompts); on Unix the conventional `bash`. The MCP
/// surface calls this so the shell it reports is exactly the shell the capability started.
pub fn default_shell() -> String {
    #[cfg(windows)]
    {
        // `pwsh` first, because it is the better terminal. But a Windows device without PowerShell 7
        // is a device that cannot open a terminal at all, which the canary proved: `terminal.create`
        // answered "cannot start pwsh ... (os error 2)". Windows PowerShell 5.1 re-echoes input and
        // draws continuation prompts, and `cmd.exe` is worse still — a bad terminal beats no
        // terminal, and the MCP surface reports the shell it actually started, so the client is
        // never guessing.
        let path = std::env::var_os("PATH").unwrap_or_default();
        let directories: Vec<std::path::PathBuf> = std::env::split_paths(&path).collect();
        let extensions = [".exe", ".cmd", ".bat", ""];
        for candidate in ["pwsh.exe", "pwsh", "powershell.exe", "cmd.exe"] {
            if find_program(candidate, &directories, &extensions).is_some() {
                return candidate.to_owned();
            }
        }
        "cmd.exe".to_owned()
    }
    #[cfg(not(windows))]
    {
        "bash".to_owned()
    }
}

/// The first `directories/<name><extension>` that is a file.
///
/// Compiled on Windows (where the shell fallback uses it) and in tests (where the rule is tested);
/// a Unix release has no fallback to make.
///
/// A pure function so the fallback rule is testable without a machine that happens to have (or lack)
/// a given shell: the caller supplies the directories and the extension list.
#[cfg(any(windows, test))]
pub fn find_program(
    name: &str,
    directories: &[std::path::PathBuf],
    extensions: &[&str],
) -> Option<std::path::PathBuf> {
    if name.contains(std::path::MAIN_SEPARATOR) {
        let direct = std::path::PathBuf::from(name);
        return direct.is_file().then_some(direct);
    }
    for directory in directories {
        for extension in extensions {
            // A name that already carries the extension is not given a second one.
            let file = if extension.is_empty() || name.ends_with(extension) {
                directory.join(name)
            } else {
                directory.join(format!("{name}{extension}"))
            };
            if file.is_file() {
                return Some(file);
            }
        }
    }
    None
}

/// The device's wall clock, in Unix milliseconds.
fn system_clock() -> Clock {
    Arc::new(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or(0)
    })
}

/// Lock a mutex, recovering the data from a poisoned lock instead of panicking a second time.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::io::RawFd;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use summrise_core::{SessionKind, SessionState};

    use crate::config::Release;
    use crate::test_support::{process_is_dead, wait_until_dead, TempDir};

    use super::*;

    /// How long a test waits for a shell to produce its marker.
    const MARKER_TIMEOUT: Duration = Duration::from_secs(10);

    struct Fixture {
        /// Kept alive so the store's paths exist for the whole test; declared before the registry so
        /// it drops after it (the registry kills children, which writes nothing).
        _dir: TempDir,
        store: Store,
        now: Arc<AtomicU64>,
        registry: PtyRegistry,
    }

    impl Fixture {
        fn new(label: &str) -> Fixture {
            Fixture::with_limits(label, Limits::default())
        }

        fn with_limits(label: &str, limits: Limits) -> Fixture {
            let dir = TempDir::new(label);
            let data = dir.join("data");
            fs::create_dir_all(&data).expect("the data directory is creatable");
            let store = Store::new(dir.join("install"), &data, Release::standalone());
            // A device always has a document before it serves; the tests that do not care still want
            // a file to load, not the "absent document" convenience.
            store
                .save(&store.initial())
                .expect("the fixture state saves");

            let now = Arc::new(AtomicU64::new(1_000));
            let clock_now = Arc::clone(&now);
            let clock: Clock = Arc::new(move || clock_now.load(Ordering::SeqCst));
            let registry = PtyRegistry::with_clock(
                store.clone(),
                Arc::new(tokio::sync::Mutex::new(())),
                clock,
                limits,
            );

            Fixture {
                _dir: dir,
                store,
                now,
                registry,
            }
        }

        fn advance(&self, milliseconds: u64) {
            self.now.fetch_add(milliseconds, Ordering::SeqCst);
        }

        fn saved_sessions(&self) -> Vec<SessionRow> {
            self.store
                .load()
                .expect("the state document loads")
                .sessions
        }
    }

    /// A pseudo-terminal pair standing in for a serial line.
    ///
    /// A pty is not a UART: it has no line speed, and on Linux the termios ioctl that would apply
    /// one is accepted as a no-op. That is why these tests need no hardware. `serialport` opens and
    /// configures a port in one call, so on a platform whose pty refuses the ioctl the create is a
    /// typed [`PtyError::Serial`] naming the path and the OS report — the tests below match that
    /// case explicitly rather than skipping, because a test that silently proves nothing is worse
    /// than one that says why it cannot run.
    #[cfg(target_os = "linux")]
    struct PtyLine {
        master: RawFd,
        slave: RawFd,
        path: PathBuf,
    }

    #[cfg(target_os = "linux")]
    impl PtyLine {
        fn new() -> PtyLine {
            let pair = nix::pty::openpty(None, None).expect("a pty pair is created");
            let path = nix::unistd::ttyname(pair.slave).expect("the pty slave has a path");
            PtyLine {
                master: pair.master,
                slave: pair.slave,
                path,
            }
        }

        /// The path `terminal.create` names as the serial port.
        fn port(&self) -> &str {
            self.path.to_str().expect("the pty path is UTF-8")
        }

        /// Write from the far end of the line, as the device under test would answer.
        fn write(&self, bytes: &[u8]) {
            nix::unistd::write(self.master, bytes).expect("the far end writes the line");
        }

        /// Read the far end until `marker` arrives or the test's deadline expires.
        fn read_until_marker(&self, marker: &str) -> String {
            let deadline = Instant::now() + MARKER_TIMEOUT;
            let mut text = String::new();
            while Instant::now() < deadline && !text.contains(marker) {
                let mut fds = [nix::poll::PollFd::new(
                    self.master,
                    nix::poll::PollFlags::POLLIN,
                )];
                match nix::poll::poll(&mut fds, 100) {
                    Ok(0) => continue,
                    Ok(_) => {}
                    Err(error) => panic!("polling the pty master: {error}"),
                }
                let mut buffer = [0u8; 256];
                let read = nix::unistd::read(self.master, &mut buffer)
                    .expect("the far end reads the line");
                text.push_str(&String::from_utf8_lossy(&buffer[..read]));
            }
            assert!(
                text.contains(marker),
                "the line carried {marker:?} to its far end within {MARKER_TIMEOUT:?}: {text:?}"
            );
            text
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for PtyLine {
        fn drop(&mut self) {
            let _ = nix::unistd::close(self.master);
            let _ = nix::unistd::close(self.slave);
        }
    }

    /// Read until `marker` appears or the deadline expires, returning everything accumulated.
    async fn read_until(registry: &PtyRegistry, id: &SessionId, marker: &str) -> String {
        let deadline = Instant::now() + MARKER_TIMEOUT;
        let mut cursor = 0;
        let mut text = String::new();
        while Instant::now() < deadline {
            let (bytes, next) = registry.read(id, cursor).expect("a live session reads");
            if next > cursor {
                text.push_str(&String::from_utf8_lossy(&bytes));
                cursor = next;
            }
            if text.contains(marker) {
                return text;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("{marker:?} did not arrive within {MARKER_TIMEOUT:?}; output so far: {text:?}");
    }

    /// Read until `marker` is followed by ASCII digits anywhere in the output.
    ///
    /// The echoed command contains the marker too (with `$$`/`$!` after it), and an interactive
    /// shell may print a prompt between the echo and the command's output. Neither position is a
    /// contract, so this scans for the first `marker<digits>` anywhere instead of anchoring on a
    /// line shape.
    async fn read_number(registry: &PtyRegistry, id: &SessionId, marker: &str) -> u32 {
        let deadline = Instant::now() + MARKER_TIMEOUT;
        let mut cursor = 0;
        let mut text = String::new();
        while Instant::now() < deadline {
            let (bytes, next) = registry.read(id, cursor).expect("a live session reads");
            if next > cursor {
                text.push_str(&String::from_utf8_lossy(&bytes));
                cursor = next;
            }
            if let Some(number) = number_after(&text, marker) {
                return number;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!(
            "{marker}<number> did not arrive within {MARKER_TIMEOUT:?}; output so far: {text:?}"
        );
    }

    /// The first number written immediately after `marker` anywhere in `text`.
    fn number_after(text: &str, marker: &str) -> Option<u32> {
        let mut rest = text;
        while let Some(index) = rest.find(marker) {
            let after = &rest[index + marker.len()..];
            let digits: String = after
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .collect();
            if !digits.is_empty() {
                return digits.parse().ok();
            }
            rest = after;
        }
        None
    }

    #[tokio::test]
    async fn a_shell_answers_a_marker_and_the_cursor_resumes() {
        let fixture = Fixture::new("pty-round-trip");
        let id = fixture
            .registry
            .create(
                CreateTarget::Pty {
                    shell: Some("/bin/sh"),
                },
                24,
                80,
            )
            .await
            .expect("a shell starts");
        let info = &fixture.registry.list()[0];
        assert_eq!(info.shell, "/bin/sh");
        assert_eq!(info.kind, Kind::Pty, "the list names the session's kind");
        assert_eq!(
            info.started_at_ms, 1_000,
            "the list names when the session started"
        );

        // The command's text does not contain its output (`SUMMRISE$((6*7))`), so the marker proves
        // the shell executed it rather than the PTY echoing it. The prompt is not part of the marker.
        fixture
            .registry
            .write(&id, b"echo SUMMRISE$((6*7))\n")
            .await
            .expect("the command is written");
        let text = read_until(&fixture.registry, &id, "SUMMRISE42").await;
        assert!(
            text.contains("SUMMRISE42"),
            "the shell's output is the session's output: {text:?}"
        );

        // The cursor is a resume token: reading from an old cursor returns the retained tail again,
        // and reading from the newest cursor returns nothing new.
        let (all, end) = fixture
            .registry
            .read(&id, 0)
            .expect("the retained output is readable from the start");
        assert!(String::from_utf8_lossy(&all).contains("SUMMRISE42"));
        let (empty, same) = fixture.registry.read(&id, end).expect("a caught-up read");
        assert!(empty.is_empty());
        assert_eq!(same, end);

        fixture
            .registry
            .resize(&id, 40, 120)
            .await
            .expect("a live session resizes");
        assert_eq!(fixture.registry.list()[0].rows, 40);
        assert_eq!(fixture.registry.list()[0].cols, 120);

        fixture
            .registry
            .close(&id)
            .await
            .expect("the session closes");

        assert!(matches!(
            fixture.registry.read(&id, end),
            Err(PtyError::UnknownSession { .. })
        ));
        assert!(matches!(
            fixture.registry.write(&id, b"x").await,
            Err(PtyError::UnknownSession { .. })
        ));
        assert!(matches!(
            fixture.registry.resize(&id, 20, 20).await,
            Err(PtyError::UnknownSession { .. })
        ));
        assert!(matches!(
            fixture.registry.close(&id).await,
            Err(PtyError::UnknownSession { .. })
        ));
        assert!(
            fixture.registry.list().is_empty(),
            "a closed session is not listed"
        );
    }

    #[tokio::test]
    async fn the_list_reports_the_kind_and_the_creation_time() {
        let fixture = Fixture::new("pty-list-shape");
        let id = fixture
            .registry
            .create(
                CreateTarget::Pty {
                    shell: Some("/bin/sh"),
                },
                24,
                80,
            )
            .await
            .expect("a shell starts");

        // Activity moves the idle clock but not the creation time: the list's started_at_ms is the
        // moment the session was made, not the moment it was last used.
        fixture.advance(7_000);
        fixture.registry.read(&id, 0).expect("a live session reads");

        let info = &fixture.registry.list()[0];
        assert_eq!(info.id, id);
        assert_eq!(info.kind, Kind::Pty);
        assert_eq!(
            info.started_at_ms, 1_000,
            "reading a session does not move its creation time"
        );

        fixture
            .registry
            .close(&id)
            .await
            .expect("the session closes");
    }

    #[test]
    fn a_kind_this_build_cannot_create_is_a_typed_refusal() {
        let fixture = Fixture::new("pty-unsupported-kind");
        let error = Kind::parse("telnet").expect_err("telnet is not a session kind");

        match &error {
            PtyError::UnsupportedKind { kind, supported } => {
                assert_eq!(kind, "telnet", "the refusal names the kind it was given");
                assert!(
                    supported.contains("pty")
                        && supported.contains("ssh")
                        && supported.contains("serial"),
                    "the refusal names every supported kind: {supported}"
                );
            }
            other => panic!("an unsupported kind is a typed refusal, not {other:?}"),
        }
        let text = error.to_string();
        assert!(text.contains("not supported"), "{text}");
        assert!(
            text.contains("pty") && text.contains("ssh") && text.contains("serial"),
            "the refusal names what is supported: {text}"
        );
        assert!(
            fixture.registry.list().is_empty(),
            "a refused create publishes no session"
        );
        assert_eq!(Kind::parse("ssh").expect("ssh is supported"), Kind::Ssh);
        assert_eq!(
            Kind::parse("serial").expect("serial is supported"),
            Kind::Serial
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_serial_session_round_trips_through_a_pseudo_terminal() {
        let fixture = Fixture::new("serial-round-trip");
        let line = PtyLine::new();
        let port = line.port().to_owned();

        let opened = fixture
            .registry
            .create(CreateTarget::Serial(SerialTarget::new(&port)), 24, 80)
            .await;
        let id = match opened {
            Ok(id) => id,
            Err(PtyError::Serial {
                port: refused,
                message,
            }) => panic!(
                "serialport refused to configure the pty at {refused}: {message} \
                 (a pty is not a UART; this test needs a platform whose pty accepts the termios ioctl)"
            ),
            Err(other) => panic!("a serial session opens on the pty slave: {other}"),
        };

        let info = &fixture.registry.list()[0];
        assert_eq!(info.kind, Kind::Serial, "the list names the serial kind");
        assert_eq!(info.shell, port, "the list names the port it opened");
        assert_eq!(info.pid, None, "a serial session is not a child process");
        assert!(info.alive, "a freshly opened serial line is alive");

        let row = fixture
            .saved_sessions()
            .into_iter()
            .find(|row| row.id == id.as_str())
            .expect("the live serial session is in the state document");
        assert_eq!(row.kind, SessionKind::Serial);
        assert_eq!(row.state, SessionState::Attached);

        // Device -> line: write on the session, read at the far end of the pty.
        fixture
            .registry
            .write(&id, b"SUMMRISE-DOWN\n")
            .await
            .expect("the bytes go to the line");
        line.read_until_marker("SUMMRISE-DOWN");

        // Line -> device: write at the far end, read through the session's ring.
        line.write(b"SUMMRISE-UP\n");
        let text = read_until(&fixture.registry, &id, "SUMMRISE-UP").await;
        assert!(
            text.contains("SUMMRISE-UP"),
            "the line's answer is the session's output: {text:?}"
        );

        // A serial line has no window: resizing it succeeds as a no-op, so a client that resizes
        // every session uniformly does not have to special-case serial.
        fixture
            .registry
            .resize(&id, 40, 120)
            .await
            .expect("a serial resize is a successful no-op");
        assert_eq!(
            fixture.registry.list()[0].kind,
            Kind::Serial,
            "the no-op did not turn the session into another kind"
        );

        fixture
            .registry
            .close(&id)
            .await
            .expect("the serial session closes");
        assert!(
            fixture.registry.list().is_empty(),
            "a closed serial session is not listed"
        );
        assert!(
            matches!(
                fixture.registry.read(&id, 0),
                Err(PtyError::UnknownSession { .. })
            ),
            "a closed serial session cannot be read"
        );
        assert!(
            matches!(
                fixture.registry.write(&id, b"x").await,
                Err(PtyError::UnknownSession { .. })
            ),
            "a closed serial session cannot be written"
        );
        assert!(
            fixture.saved_sessions().is_empty(),
            "a closed serial session's state row is gone"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_missing_serial_port_is_a_typed_refusal_naming_it() {
        let fixture = Fixture::new("serial-missing-port");
        let missing = fixture._dir.join("no-such-serial-port");
        assert!(!missing.exists(), "the fixture path is genuinely absent");
        let port = missing.to_str().expect("the fixture path is UTF-8");

        let error = fixture
            .registry
            .create(CreateTarget::Serial(SerialTarget::new(port)), 24, 80)
            .await
            .expect_err("there is no device at that path");

        match &error {
            PtyError::Serial {
                port: refused,
                message,
            } => {
                assert_eq!(refused, port, "the refusal names the port it tried");
                assert!(
                    !message.is_empty(),
                    "the refusal keeps the OS report instead of replacing it"
                );
                let message = message.to_lowercase();
                assert!(
                    message.contains("no such file") || message.contains("not found"),
                    "the OS says the path does not exist: {message}"
                );
            }
            other => panic!("a missing port is the typed serial refusal, not {other:?}"),
        }
        let text = error.to_string();
        assert!(
            text.contains(port),
            "the rendered refusal names the port: {text}"
        );
        assert!(
            fixture.registry.list().is_empty(),
            "a refused create publishes no session"
        );
        assert!(
            fixture.saved_sessions().is_empty(),
            "a refused create publishes no state row"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_second_create_on_a_held_serial_port_is_refused_and_close_releases_it() {
        let fixture = Fixture::new("serial-port-busy");
        let line = PtyLine::new();
        let port = line.port();
        let first = fixture
            .registry
            .create(CreateTarget::Serial(SerialTarget::new(port)), 24, 80)
            .await
            .expect("the first session opens the port");

        let refused = fixture
            .registry
            .create(CreateTarget::Serial(SerialTarget::new(port)), 24, 80)
            .await
            .expect_err("the port already has a session");
        match &refused {
            PtyError::PortBusy {
                port: refused_port,
                holder,
            } => {
                assert_eq!(refused_port, port, "the refusal names the port it tried");
                assert_eq!(
                    holder,
                    first.as_str(),
                    "the refusal names the session that holds it"
                );
            }
            other => panic!("a held port is the typed busy refusal, not {other:?}"),
        }
        let text = refused.to_string();
        assert!(
            text.contains(port) && text.contains(first.as_str()),
            "the rendered refusal names the port and its holder: {text}"
        );
        assert_eq!(
            fixture.registry.list().len(),
            1,
            "the refused create did not publish a second session"
        );
        assert_eq!(
            fixture.registry.list()[0].id,
            first,
            "the first session still holds the port"
        );
        assert!(
            fixture
                .saved_sessions()
                .iter()
                .filter(|row| row.kind == SessionKind::Serial)
                .count()
                == 1,
            "the refused create wrote no second state row: {:?}",
            fixture.saved_sessions()
        );

        // The claim follows the session: close releases it and the same port opens again.
        fixture
            .registry
            .close(&first)
            .await
            .expect("the holder closes");
        let second = fixture
            .registry
            .create(CreateTarget::Serial(SerialTarget::new(port)), 24, 80)
            .await
            .expect("the closed port is free again");
        assert_ne!(second, first, "the second session has its own id");
        fixture
            .registry
            .close(&second)
            .await
            .expect("the second session closes");
    }

    #[test]
    fn serial_framing_the_build_does_not_support_is_refused_not_defaulted() {
        // A typo in a wire format must not open a port with the default and corrupt every frame.
        let target = SerialTarget {
            data_bits: 9,
            ..SerialTarget::new("/dev/null")
        };
        match validate_serial(&target) {
            Err(PtyError::SerialConfig {
                field,
                value,
                accepted,
            }) => {
                assert_eq!(field, "data_bits");
                assert_eq!(value, "9");
                assert!(accepted.contains('8'), "{accepted}");
            }
            other => panic!("data_bits 9 is a typed refusal, not {other:?}"),
        }

        let target = SerialTarget {
            stop_bits: 3,
            ..SerialTarget::new("/dev/null")
        };
        assert!(
            matches!(
                validate_serial(&target),
                Err(PtyError::SerialConfig {
                    field: "stop_bits",
                    ..
                })
            ),
            "stop bits 3 is refused"
        );

        let target = SerialTarget {
            baud: 0,
            ..SerialTarget::new("/dev/null")
        };
        assert!(
            matches!(
                validate_serial(&target),
                Err(PtyError::SerialConfig { field: "baud", .. })
            ),
            "a zero baud rate is refused"
        );

        match Parity::parse("MARK") {
            Err(PtyError::SerialConfig {
                field,
                value,
                accepted,
            }) => {
                assert_eq!(field, "parity");
                assert_eq!(value, "MARK", "the refusal keeps the value it was given");
                assert!(
                    accepted.contains("odd") && accepted.contains("none"),
                    "{accepted}"
                );
            }
            other => panic!("parity MARK is a typed refusal, not {other:?}"),
        }
        // The old agent's spelling is case-insensitive; the wire's lowercase form is accepted too.
        assert_eq!(
            Parity::parse("EVEN").expect("parity is case-insensitive"),
            Parity::Even
        );
        assert_eq!(
            Parity::parse("none").expect("no parity is the default"),
            Parity::None
        );

        validate_serial(&SerialTarget::new("/dev/null")).expect("115200 8N1 is accepted");
    }

    /// A lock that is still dying is waited out, not reported as a busy port.
    ///
    /// `flock` belongs to the open file description and `fork` copies the descriptor table, so a
    /// child forked while a session held the port keeps that description — and its lock — until it
    /// execs, because `O_CLOEXEC` acts at exec and not at fork. Another capability's child (a PTY
    /// shell, a `runs.exec`) forked in that window keeps a just-closed port locked for
    /// milliseconds, and the operator must not be told the port is busy.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_lock_that_is_still_dying_is_waited_out() {
        let fixture = Fixture::new("serial-dying-lock");
        let line = PtyLine::new();
        // The dying holder: an exclusive lock on the port that a concurrent close has not released
        // yet.
        let holder = open_serial(
            line.port(),
            115_200,
            DataBits::Eight,
            SerialParity::None,
            StopBits::One,
        )
        .expect("the holder takes the lock");
        let released = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            drop(holder);
        });

        let session = fixture
            .registry
            .create(CreateTarget::Serial(SerialTarget::new(line.port())), 24, 80)
            .await
            .expect("the create waits out the lock instead of refusing a free port");
        released.await.expect("the holder drops");
        fixture
            .registry
            .close(&session)
            .await
            .expect("the session closes");
    }

    /// A port another program really holds is still refused, after a bounded wait.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_port_held_for_longer_than_the_patience_is_still_refused() {
        let fixture = Fixture::new("serial-held-long");
        let line = PtyLine::new();
        let _holder = open_serial(
            line.port(),
            115_200,
            DataBits::Eight,
            SerialParity::None,
            StopBits::One,
        )
        .expect("the holder takes the lock");

        let started = Instant::now();
        let error = fixture
            .registry
            .create(CreateTarget::Serial(SerialTarget::new(line.port())), 24, 80)
            .await
            .expect_err("a port another holder keeps is still refused");
        let waited = started.elapsed();
        assert!(
            matches!(error, PtyError::Serial { .. }),
            "the refusal is the serial open's own: {error}"
        );
        assert!(
            waited >= SERIAL_LOCK_PATIENCE,
            "the wait is the bounded patience, not an instant refusal: {waited:?}"
        );
        assert!(
            waited < SERIAL_LOCK_PATIENCE * 4,
            "and it does not outlast the bound: {waited:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn the_sweeper_closes_an_idle_serial_session() {
        let fixture = Fixture::new("serial-sweeper");
        let line = PtyLine::new();
        let id = fixture
            .registry
            .create(CreateTarget::Serial(SerialTarget::new(line.port())), 24, 80)
            .await
            .expect("a serial session opens");

        fixture.advance(IDLE_TIMEOUT_MS - 1);
        fixture.registry.sweep_idle().await;
        assert_eq!(
            fixture.registry.list().len(),
            1,
            "one millisecond short of the timeout is not idle"
        );

        fixture.advance(1);
        fixture.registry.sweep_idle().await;
        assert!(
            fixture.registry.list().is_empty(),
            "the idle serial session is closed by the same sweeper"
        );
        assert!(matches!(
            fixture.registry.read(&id, 0),
            Err(PtyError::UnknownSession { .. })
        ));
        assert!(
            fixture
                .saved_sessions()
                .iter()
                .all(|row| row.id != id.as_str()),
            "the swept serial session's state row is gone: {:?}",
            fixture.saved_sessions()
        );

        // The sweep released the port's claim too, not just its session row.
        let again = fixture
            .registry
            .create(CreateTarget::Serial(SerialTarget::new(line.port())), 24, 80)
            .await
            .expect("the swept port is free again");
        fixture
            .registry
            .close(&again)
            .await
            .expect("the reopened session closes");
    }

    #[tokio::test]
    async fn the_seventeenth_live_session_is_refused() {
        let fixture = Fixture::new("pty-cap");
        let mut created = Vec::new();
        for index in 0..MAX_SESSIONS {
            let id = fixture
                .registry
                .create(
                    CreateTarget::Pty {
                        shell: Some("/bin/sh"),
                    },
                    24,
                    80,
                )
                .await
                .unwrap_or_else(|error| panic!("session {index}: {error}"));
            created.push(id);
        }
        assert_eq!(fixture.registry.list().len(), MAX_SESSIONS);

        let refused = fixture
            .registry
            .create(
                CreateTarget::Pty {
                    shell: Some("/bin/sh"),
                },
                24,
                80,
            )
            .await
            .expect_err("the cap is enforced before a process is spawned");
        assert!(
            matches!(refused, PtyError::TooManySessions { max: MAX_SESSIONS }),
            "{refused}"
        );
        // The cap is the registry's, not the PTY's: a serial create is refused before the port is
        // named, let alone opened.
        let refused_serial = fixture
            .registry
            .create(
                CreateTarget::Serial(SerialTarget::new("/dev/no-such-serial-port")),
                24,
                80,
            )
            .await
            .expect_err("the cap applies to every session kind");
        assert!(
            matches!(
                refused_serial,
                PtyError::TooManySessions { max: MAX_SESSIONS }
            ),
            "{refused_serial}"
        );
        assert_eq!(
            fixture.registry.list().len(),
            MAX_SESSIONS,
            "the refused create added nothing"
        );

        for id in created {
            fixture
                .registry
                .close(&id)
                .await
                .expect("the session closes");
        }
    }

    #[tokio::test]
    async fn the_sweeper_closes_a_session_idle_for_the_timeout() {
        let fixture = Fixture::new("pty-sweeper");
        let id = fixture
            .registry
            .create(
                CreateTarget::Pty {
                    shell: Some("/bin/sh"),
                },
                24,
                80,
            )
            .await
            .expect("a shell starts");

        fixture.advance(IDLE_TIMEOUT_MS - 1);
        fixture.registry.sweep_idle().await;
        assert_eq!(
            fixture.registry.list().len(),
            1,
            "one millisecond short of the timeout is not idle"
        );

        fixture.advance(1);
        fixture.registry.sweep_idle().await;
        assert!(
            fixture.registry.list().is_empty(),
            "the idle session is closed"
        );
        assert!(matches!(
            fixture.registry.read(&id, 0),
            Err(PtyError::UnknownSession { .. })
        ));
        assert!(
            fixture
                .saved_sessions()
                .iter()
                .all(|row| row.id != id.as_str()),
            "the swept session's state row is gone: {:?}",
            fixture.saved_sessions()
        );
    }

    #[tokio::test]
    async fn the_state_document_lists_a_live_session_and_forgets_it_on_close() {
        let fixture = Fixture::new("pty-state");
        let id = fixture
            .registry
            .create(
                CreateTarget::Pty {
                    shell: Some("/bin/sh"),
                },
                24,
                80,
            )
            .await
            .expect("a shell starts");

        let rows = fixture.saved_sessions();
        let row = rows
            .iter()
            .find(|row| row.id == id.as_str())
            .unwrap_or_else(|| panic!("the live session is in the state document: {rows:?}"));
        assert_eq!(row.kind, SessionKind::Pty);
        assert_eq!(row.state, SessionState::Attached);

        fixture
            .registry
            .close(&id)
            .await
            .expect("the session closes");
        assert!(
            fixture
                .saved_sessions()
                .iter()
                .all(|row| row.id != id.as_str()),
            "a closed session is not in the state document"
        );
    }

    #[test]
    fn an_empty_shell_names_the_platform_default() {
        // A pure test: the default program is the platform's, and spawning it would only add an
        // environment assumption (a runner without `bash`) to a rule about argument resolution.
        assert_eq!(resolve_shell(None), default_shell());
        assert_eq!(resolve_shell(Some("")), default_shell());
        assert_eq!(resolve_shell(Some("/bin/sh")), "/bin/sh");
    }

    #[tokio::test]
    async fn a_closed_id_is_never_reused() {
        let fixture = Fixture::new("pty-id-reuse");
        let first = fixture
            .registry
            .create(
                CreateTarget::Pty {
                    shell: Some("/bin/sh"),
                },
                24,
                80,
            )
            .await
            .expect("the first shell starts");
        fixture
            .registry
            .close(&first)
            .await
            .expect("the first session closes");
        let second = fixture
            .registry
            .create(
                CreateTarget::Pty {
                    shell: Some("/bin/sh"),
                },
                24,
                80,
            )
            .await
            .expect("the second shell starts");

        assert_ne!(second, first, "an id names one session for all time");
        assert!(matches!(
            fixture.registry.read(&first, 0),
            Err(PtyError::UnknownSession { .. })
        ));
    }

    #[tokio::test]
    async fn reconcile_removes_rows_left_by_a_dead_process() {
        let fixture = Fixture::new("pty-reconcile");
        let mut state = fixture.store.load().expect("the fixture state loads");
        state.sessions.push(SessionRow {
            id: "session-999".to_owned(),
            kind: SessionKind::Pty,
            state: SessionState::Attached,
        });
        state.sessions.push(SessionRow {
            id: "ssh-1".to_owned(),
            kind: SessionKind::Ssh,
            state: SessionState::Attached,
        });
        state.sessions.push(SessionRow {
            id: "serial-1".to_owned(),
            kind: SessionKind::Serial,
            state: SessionState::Attached,
        });
        fixture.store.save(&state).expect("the stale state saves");

        fixture.registry.reconcile_state().await;

        let rows = fixture.saved_sessions();
        assert!(
            rows.iter().all(|row| row.id != "session-999"),
            "a pty row no live registry owns is a dead run's: {rows:?}"
        );
        assert!(
            rows.iter().all(|row| row.id != "ssh-1"),
            "an ssh row is this registry's to reconcile too: {rows:?}"
        );
        assert!(
            rows.iter().all(|row| row.id != "serial-1"),
            "a serial row is this registry's to reconcile too now that it owns serial sessions: {rows:?}"
        );
    }

    #[tokio::test]
    async fn dropping_the_registry_kills_the_child() {
        let fixture = Fixture::new("pty-drop");
        let id = fixture
            .registry
            .create(
                CreateTarget::Pty {
                    shell: Some("/bin/sh"),
                },
                24,
                80,
            )
            .await
            .expect("a shell starts");
        fixture
            .registry
            .write(&id, b"echo PID=$$\n")
            .await
            .expect("the command is written");
        // The echoed command contains `PID=$$` too; the number parser skips it and finds the
        // output's `PID=<digits>` wherever an interactive shell put its prompt.
        let pid = read_number(&fixture.registry, &id, "PID=").await;

        drop(fixture);

        assert!(
            wait_until_dead(pid, Duration::from_secs(10)).await,
            "the registry drop left process {pid} running"
        );
    }

    // Linux: the test proves the shell is stopped through `/proc/<pid>/stat`. The capability's
    // bound itself is portable; the observation is not.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_write_to_a_stopped_shell_is_bounded_without_wedging_the_registry() {
        let fixture = Fixture::with_limits(
            "pty-write-timeout",
            Limits {
                write_timeout_ms: 250,
                ..Limits::default()
            },
        );
        let id = fixture
            .registry
            .create(
                CreateTarget::Pty {
                    shell: Some("/bin/sh"),
                },
                24,
                80,
            )
            .await
            .expect("a shell starts");
        let pid = fixture.registry.list()[0].pid.expect("the child has a pid");
        kill(Pid::from_raw(pid as i32), Signal::SIGSTOP).expect("the shell is stopped");
        tokio::time::sleep(Duration::from_millis(100)).await;
        let state = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .expect("the stopped child's stat is readable");
        let state = state
            .rsplit_once(')')
            .expect("a stat line has a comm")
            .1
            .split_whitespace()
            .next()
            .expect("a stat line has a state");
        assert_eq!(state, "T", "the shell is stopped before the write");

        // The stopped shell may absorb the 1 MiB through the PTY's buffered input path before the
        // bound, or the bound may fire first. Both outcomes satisfy the capability; what must never
        // happen is a call that hangs, so the whole pair is wrapped in a wall-clock deadline.
        let payload = vec![b'x'; MAX_INPUT_BYTES];
        let started = Instant::now();
        // `join!` polls the first future far enough to take the gate and spawn its blocking task,
        // then polls the second, which either fails fast or follows a first write that already
        // finished.
        let pair = tokio::time::timeout(Duration::from_secs(6), async {
            let first = fixture.registry.write(&id, &payload);
            let second = fixture.registry.write(&id, b"y");
            tokio::join!(first, second)
        })
        .await
        .expect("a stalled write must return, not hang");
        let (first, second) = pair;
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "the call is bounded, not tied to the shell's throughput: {:?}",
            started.elapsed()
        );

        match first {
            Ok(written) => assert_eq!(
                written, MAX_INPUT_BYTES,
                "a completed write reports every byte it accepted"
            ),
            Err(PtyError::WriteTimeout {
                id: timed_out,
                timeout_ms,
            }) => {
                assert_eq!(timed_out, id.as_str(), "the timeout names the session");
                assert_eq!(timeout_ms, 250, "the timeout reports the configured bound");
            }
            Err(other) => panic!(
                "a stopped shell must either absorb the write or return the typed timeout: {other}"
            ),
        }
        match second {
            Ok(written) => assert_eq!(written, 1, "the second write followed the first"),
            Err(PtyError::WriteBusy { id: busy }) => {
                assert_eq!(busy, id.as_str(), "the busy refusal names the session");
            }
            Err(other) => panic!("the second write must be bounded, not hang: {other}"),
        }

        // The registry is still usable while a blocking task is parked: its map lock is not the
        // writer's lock.
        assert_eq!(fixture.registry.list().len(), 1);

        // Resume the shell so any parked task can finish, then close the session.
        kill(Pid::from_raw(pid as i32), Signal::SIGCONT).expect("the shell resumes");
        fixture
            .registry
            .close(&id)
            .await
            .expect("the session closes");
    }

    // Linux: the session scan that reaches a background job in its own process group is the Linux
    // `/proc` implementation. On other Unix only the process group can be signalled, which is the
    // documented limitation of the fallback, so this test would fail there by design.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn closing_a_session_kills_a_background_job_in_its_own_process_group() {
        let fixture = Fixture::new("pty-descendants");
        let id = fixture
            .registry
            .create(
                CreateTarget::Pty {
                    shell: Some("/bin/sh"),
                },
                24,
                80,
            )
            .await
            .expect("a shell starts");
        let shell = fixture.registry.list()[0].pid.expect("the child has a pid");
        fixture
            .registry
            .write(&id, b"sleep 30 & echo BG=$!\n")
            .await
            .expect("the background job starts");
        // The echoed command contains `BG=$!` too; the number parser skips it and finds the
        // output's `BG=<digits>` wherever an interactive shell put its prompt.
        let background = read_number(&fixture.registry, &id, "BG=").await as i32;

        assert!(
            kill(Pid::from_raw(background), None::<Signal>).is_ok(),
            "the background job is running before the close"
        );

        fixture
            .registry
            .close(&id)
            .await
            .expect("the session closes");

        assert!(
            wait_until_dead(background as u32, Duration::from_secs(3)).await,
            "the background job {background} survived close of session {} (shell {shell})",
            id.as_str()
        );
        assert!(
            wait_until_dead(shell, Duration::from_secs(3)).await,
            "the direct child {shell} survived close of session {}",
            id.as_str()
        );
    }

    // Linux: the zombie is observed through `/proc/<pid>/stat`. The close path itself is Unix; the
    // observation is Linux.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn closing_after_a_natural_shell_exit_closes_its_background_jobs() {
        let fixture = Fixture::new("pty-natural-exit");
        let id = fixture
            .registry
            .create(
                CreateTarget::Pty {
                    shell: Some("/bin/sh"),
                },
                24,
                80,
            )
            .await
            .expect("a shell starts");
        let shell = fixture.registry.list()[0].pid.expect("the child has a pid");
        fixture
            .registry
            .write(&id, b"sleep 30 & echo BG=$!\n")
            .await
            .expect("the background job starts");
        let background = read_number(&fixture.registry, &id, "BG=").await as i32;
        assert!(
            kill(Pid::from_raw(background), None::<Signal>).is_ok(),
            "the background job is running before the shell exits"
        );

        fixture
            .registry
            .write(&id, b"exit\n")
            .await
            .expect("the exit command is written");

        // The background job still holds the PTY slave, so the reader does not see EOF; the shell is
        // a zombie until close. That is the state `try_wait` must reap, and the pid is still ours at
        // that point, so close can kill the session's other members.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut zombie = false;
        while Instant::now() < deadline {
            if let Ok(stat) = std::fs::read_to_string(format!("/proc/{shell}/stat")) {
                let state = stat
                    .rsplit_once(')')
                    .and_then(|(_, fields)| fields.split_whitespace().next());
                if state == Some("Z") {
                    zombie = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(zombie, "the shell exited and waits to be reaped");
        assert!(
            process_is_dead(shell),
            "a zombie answers kill(0) but is not running"
        );

        fixture
            .registry
            .close(&id)
            .await
            .expect("the session closes");

        assert!(
            wait_until_dead(background as u32, Duration::from_secs(3)).await,
            "the background job {background} survived close after the shell exited"
        );
    }

    #[test]
    fn the_ring_keeps_the_newest_cap_bytes_and_refuses_lost_cursors() {
        let mut ring = Ring::new(8);
        ring.append(b"0123456789");

        assert_eq!(ring.total(), 10);
        assert_eq!(ring.earliest(), 2);
        assert!(
            matches!(
                ring.read_from(0),
                Err(PtyError::CursorTooOld {
                    cursor: 0,
                    earliest: 2
                })
            ),
            "bytes the ring dropped are not invented"
        );
        assert!(matches!(
            ring.read_from(11),
            Err(PtyError::CursorAhead {
                cursor: 11,
                total: 10
            })
        ));

        assert_eq!(
            ring.read_from(2).expect("the retained tail reads"),
            (b"23456789".to_vec(), 10)
        );
        assert_eq!(
            ring.read_from(9)
                .expect("a mid-tail cursor reads only what follows"),
            (b"9".to_vec(), 10)
        );
        assert_eq!(
            ring.read_from(10)
                .expect("a caught-up cursor reads nothing"),
            (Vec::new(), 10)
        );
    }

    #[test]
    fn a_number_is_found_wherever_the_prompt_put_it() {
        // The shape a CI runner produced: the shell echoed the command, then printed its prompt, then
        // ran it. The echo must be skipped (`$$` is not a number) and the prompt must not be required
        // to be absent.
        assert_eq!(
            number_after("$ echo PID=$$\r\n$ PID=3769\r\n$ ", "PID="),
            Some(3769)
        );
        assert_eq!(number_after("$ echo BG=$!\r\nBG=42\r\n$ ", "BG="), Some(42));
        assert_eq!(number_after("PID=3769", "PID="), Some(3769));
        assert_eq!(number_after("no marker here", "PID="), None);
        assert_eq!(number_after("PID=$! still not a number", "PID="), None);
        // A marker followed by a prompt or a word is not a number; only `<marker><digits>` counts.
        assert_eq!(number_after("PID= value 7", "PID="), None);
    }

    #[test]
    fn a_size_of_zero_or_beyond_the_cap_is_refused() {
        for (rows, cols) in [
            (0, 80),
            (24, 0),
            (MAX_DIMENSION + 1, 80),
            (24, MAX_DIMENSION + 1),
        ] {
            let error = validate_size(rows, cols).expect_err("that size is outside the cap");
            assert!(
                matches!(error, PtyError::InvalidSize { .. }),
                "{rows}x{cols}: {error:?}"
            );
        }
        validate_size(1, 1).expect("the smallest terminal is legal");
        validate_size(MAX_DIMENSION, MAX_DIMENSION).expect("the largest terminal is legal");
    }

    #[test]
    fn the_shell_fallback_finds_the_first_candidate_that_exists() {
        let root = std::env::temp_dir().join(format!("summrise-shell-{}", std::process::id()));
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir_all(&first).expect("first");
        std::fs::create_dir_all(&second).expect("second");
        std::fs::write(second.join("powershell.exe"), b"stub").expect("stub");
        let directories = [first.clone(), second.clone()];

        assert_eq!(
            find_program("powershell.exe", &directories, &[".exe", ""]),
            Some(second.join("powershell.exe")),
            "a shell that exists in a later directory is found"
        );
        assert_eq!(
            find_program("pwsh.exe", &directories, &[".exe", ""]),
            None,
            "a shell that exists nowhere is not invented"
        );

        // A name that already carries its extension is not given a second one.
        assert_eq!(
            find_program("powershell.exe", &directories, &[".exe"]),
            Some(second.join("powershell.exe"))
        );

        // A path is checked as given, not searched for.
        assert_eq!(
            find_program(
                &second.join("powershell.exe").to_string_lossy(),
                &[],
                &[".exe"]
            ),
            Some(second.join("powershell.exe"))
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
