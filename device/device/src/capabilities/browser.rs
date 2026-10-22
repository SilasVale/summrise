//! The browser capability (design §4.5): headless Chrome, driven over CDP.
//!
//! **Why a hand-rolled CDP client.** The capability needs its verbs (list, open, navigate, evaluate,
//! screenshot, close, shutdown) against a browser that is already installed on the device and on
//! the CI runners. Playwright would add a Node runtime and a second browser download for a protocol that is
//! JSON over one WebSocket; this module speaks that protocol directly with `tokio-tungstenite`, so
//! the device remains one Rust process.
//!
//! Chocolatey Chrome and Chromium from the distribution both work: [`discover_chrome`] tries
//! `SUMMRISE_CHROME`, then the conventional Linux and Windows paths. The capability chooses the
//! debugger port itself — bind `127.0.0.1:0`, read the port, release the listener — starts Chrome
//! `--headless=new --remote-debugging-port=<port>` with its own profile under
//! `<data>/chrome-profile/<run>`, and probes `GET /json/version` until Chrome names its browser
//! WebSocket endpoint, bounded by [`READINESS_TIMEOUT_MS`]. No `DevToolsActivePort` file is read or
//! trusted: whether Chrome writes it, or when, is not part of this capability's contract. A port
//! taken between choosing it and Chrome binding it makes Chrome exit (a bind failure); that attempt
//! is retried with a new port and profile, at most [`MAX_LAUNCH_ATTEMPTS`] times.
//!
//! Each launch's profile directory is fresh, which matters when a device was killed: its Chrome may
//! still be running and still hold the old profile's lock. A launch sweeps directories a dead
//! process left behind (by age, or by a lock that names no live process), and a graceful [`Browser`]
//! `Drop` removes its own directory after killing the child — and on Unix its whole process group,
//! which includes the renderers — so dropping the device does not leave a browser behind.
//!
//! **The client is deliberately small.** One command is one `{id, method, params}` message and one
//! matching `{id, result}` reply; events are ignored except for the navigation commit (a main
//! frame's `Page.frameNavigated` or `Page.navigatedWithinDocument`, bound to the navigation by
//! loader id or URL), and then `document.readyState` is polled until the document has finished
//! parsing — a commit only says the document exists, not that its title has been parsed. A fragment
//! navigation has no load event, and an unattributed load event must never end the wait. Every
//! command has a [`COMMAND_TIMEOUT_MS`] bound, and a single message is capped so a broken endpoint
//! cannot make the device allocate without a bound. Page verbs connect to the page's own
//! `webSocketDebuggerUrl`; target verbs use the browser endpoint. Chrome allows both; the page
//! endpoint avoids the attach/session bookkeeping that a flattened browser session would need.
//!
//! **Caps are the capability's.** At most [`MAX_TABS`] pages; an `open` beyond that is refused
//! rather than letting a client fill the device's memory with tabs. An evaluation result is capped
//! at [`EVAL_MAX_BYTES`] and a screenshot at [`SCREENSHOT_MAX_BYTES`] (both the JSON text the
//! client receives, and, for the screenshot, the decoded PNG's length).
//!
//! **Destructive tools.** `open`, `navigate` and `eval` are destructive in the registry's sense:
//! they change what a page holds or run code in it, and no later read can undo the effect. `list`
//! and `screenshot` only observe a page. `close` and `shutdown` are not destructive because they
//! destroy only the session, exactly as `terminal.close` is not: they are the actions a client
//! must be able to take without an approval when a page is wedged, and they create no new effect a
//! reader would have to undo. (A page's unsaved state is the page's, not the device's.)

use std::collections::VecDeque;
use std::fmt;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_with_config, MaybeTlsStream, WebSocketStream};

#[cfg(unix)]
use nix::sys::signal::{kill, killpg, Signal};
#[cfg(unix)]
use nix::unistd::Pid;

/// The most page targets one device may have open at once.
pub const MAX_TABS: usize = 8;

/// The largest evaluation result `browser.eval` returns, as serialized JSON.
pub const EVAL_MAX_BYTES: usize = 1024 * 1024;

/// The largest screenshot `browser.screenshot` returns, as the decoded PNG's length.
pub const SCREENSHOT_MAX_BYTES: usize = 8 * 1024 * 1024;

/// How long one CDP command, one HTTP target-list read, or one CDP connect may take.
pub const COMMAND_TIMEOUT_MS: u64 = 10_000;

/// How long Chrome may take to answer its DevTools HTTP endpoint before an attempt fails.
///
/// Cold CI runners are slow; 30 s is deliberately generous, and this is a launch bound rather than
/// a command bound. The capability's tests shorten it through the registry.
const READINESS_TIMEOUT_MS: u64 = 30_000;

/// How many launch attempts a port taken between choosing it and Chrome binding it may cost.
const MAX_LAUNCH_ATTEMPTS: u8 = 3;

/// How often the readiness probe asks `/json/version` while Chrome is starting.
const PROBE_INTERVAL_MS: u64 = 100;

/// How long one readiness request may take before the probe asks again.
const PROBE_REQUEST_TIMEOUT_MS: u64 = 1_000;

/// How often the ready-state poll asks a committed document whether parsing has finished.
///
/// A commit means the document exists; the parser can still be waiting on a script, so an immediate
/// evaluation may see an empty title. `document.readyState` turns `interactive` when the markup is
/// parsed, which is the first state in which the title is guaranteed.
const READY_STATE_POLL_MS: u64 = 50;

/// How old a leftover profile directory must be before a launch sweeps it.
const PROFILE_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;

/// Chrome's stderr log inside a launch's profile directory.
const CHROME_STDERR: &str = "chrome.stderr";

/// How many bytes of Chrome's stderr a launch failure quotes.
const STDERR_TAIL_BYTES: u64 = 4 * 1024;

/// The largest CDP message the client accepts.
///
/// A screenshot is 8 MiB decoded, which is about 10.7 MiB of base64 plus the JSON envelope; 16 MiB
/// leaves room for that and refuses a message that is not a screenshot at all. Tungstenite enforces
/// the same bound while it reassembles frames, before this module sees the bytes.
const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// The largest `/json/list` body the capability reads. Eight tabs are a few kilobytes.
const MAX_TARGET_LIST_BYTES: usize = 1024 * 1024;

/// How many events are remembered while a command awaits its reply.
///
/// The only event the capability waits for is `Page.loadEventFired`, and it may arrive while the
/// `Page.navigate` reply is still being read. A short queue closes that race without buffering an
/// unbounded event stream.
const MAX_BUFFERED_EVENTS: usize = 16;

/// How much of a JavaScript exception or navigation failure a refusal keeps.
const MAX_ERROR_BYTES: usize = 4 * 1024;

/// How much of a URL a navigation refusal echoes. A URL is client input; the message stays bounded.
const MAX_URL_IN_ERROR_BYTES: usize = 512;

/// The base64 encoding of the eight-byte PNG signature.
///
/// Chrome says `format: "png"` in the request, and this checks the reply is what was asked for.
/// Comparing the encoded prefix avoids decoding a megabyte only to throw it away.
const PNG_BASE64_PREFIX: &str = "iVBORw0KGgo";

/// The Chrome executables [`discover_chrome`] tries, in order, when `SUMMRISE_CHROME` names none.
///
/// Paths for both platforms are listed unconditionally: a path for the other platform simply does
/// not exist, so the check stays one `is_file` per candidate and the function is not a `cfg` maze.
const CHROME_CANDIDATES: &[&str] = &[
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
];

/// One target, as `browser.list` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetInfo {
    /// The target id every other browser tool takes.
    pub id: String,
    /// The document title Chrome reports; empty while a page is still loading.
    pub title: String,
    /// The document URL.
    pub url: String,
    /// Chrome's target type: `page`, `background_page`, `service_worker`, ...
    pub kind: String,
}

/// The value one `browser.eval` produced.
#[derive(Clone, Debug, PartialEq)]
pub struct EvalValue {
    /// The JavaScript type name Chrome reports (`string`, `number`, `object`, `undefined`, ...).
    pub kind: String,
    /// The value itself when `returnByValue` could produce one; `None` for `undefined`, a function
    /// or a value that has no JSON form.
    pub value: Option<Value>,
}

/// One captured screenshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screenshot {
    /// The base64-encoded PNG Chrome returned.
    pub data: String,
    /// The decoded PNG's length in bytes.
    pub bytes: usize,
}

/// Why a browser operation was refused or failed.
///
/// These are typed rather than stringly so the MCP surface can render them and a test can name the
/// rule it proves.
#[derive(Debug)]
pub enum BrowserError {
    /// Neither `SUMMRISE_CHROME` nor a conventional path names a Chrome executable.
    ChromeNotFound,
    /// The executable exists but the process could not be started.
    Spawn {
        /// The executable that was tried.
        path: String,
        /// The operating system's message.
        message: String,
    },
    /// Chrome started but did not open its debugger endpoint in time.
    Launch(String),
    /// A command did not answer within [`COMMAND_TIMEOUT_MS`].
    Timeout {
        /// The command that was waiting.
        method: &'static str,
        /// The bound that elapsed.
        timeout_ms: u64,
    },
    /// Chrome answered a command with an error.
    Protocol {
        /// The command that failed.
        method: &'static str,
        /// Chrome's message.
        message: String,
    },
    /// The WebSocket transport failed or closed.
    WebSocket {
        /// The transport's message.
        message: String,
    },
    /// The HTTP side of CDP (the target list) failed.
    Http {
        /// The status code, when a response arrived.
        status: Option<u16>,
        /// What went wrong.
        message: String,
    },
    /// One CDP message exceeded [`MAX_MESSAGE_BYTES`].
    MessageTooLarge {
        /// The message's size.
        bytes: usize,
        /// The cap.
        max: usize,
    },
    /// Chrome's HTTP reply exceeded the capability's read bound.
    ResponseTooLarge {
        /// The body's size.
        bytes: usize,
        /// The cap.
        max: usize,
    },
    /// The browser already holds [`MAX_TABS`] pages.
    TooManyTabs {
        /// The cap.
        max: usize,
    },
    /// The target id names no open page.
    UnknownTarget,
    /// The requested URL did not produce a loaded document: Chrome refused it, showed its error
    /// page, or no commit arrived within [`COMMAND_TIMEOUT_MS`].
    Navigation {
        /// The URL that was asked for, bounded for the message.
        url: String,
        /// Chrome's error text, or the deadline that elapsed.
        message: String,
    },
    /// The expression threw.
    Eval {
        /// The exception's description, bounded.
        message: String,
    },
    /// The evaluation result exceeded [`EVAL_MAX_BYTES`].
    ResultTooLarge {
        /// The serialized result's size.
        bytes: usize,
        /// The cap.
        max: usize,
    },
    /// The screenshot exceeded [`SCREENSHOT_MAX_BYTES`].
    ScreenshotTooLarge {
        /// The decoded PNG's size.
        bytes: usize,
        /// The cap.
        max: usize,
    },
    /// Chrome returned something that is not a PNG.
    Screenshot(String),
    /// The profile directory could not be created or used.
    Io(io::Error),
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrowserError::ChromeNotFound => write!(
                f,
                "no Chrome or Chromium was found; set SUMMRISE_CHROME to its executable"
            ),
            BrowserError::Spawn { path, message } => {
                write!(f, "cannot start {path}: {message}")
            }
            BrowserError::Launch(message) => write!(f, "the browser did not start: {message}"),
            BrowserError::Timeout { method, timeout_ms } => {
                write!(f, "{method} did not answer within {timeout_ms} ms")
            }
            BrowserError::Protocol { method, message } => {
                write!(f, "{method} failed: {message}")
            }
            BrowserError::WebSocket { message } => {
                write!(f, "the browser's CDP connection failed: {message}")
            }
            BrowserError::Http { status, message } => match status {
                Some(status) => {
                    write!(
                        f,
                        "cannot read the browser's target list (HTTP {status}): {message}"
                    )
                }
                None => write!(f, "cannot read the browser's target list: {message}"),
            },
            BrowserError::MessageTooLarge { bytes, max } => {
                write!(f, "a CDP message is {bytes} bytes; the cap is {max}")
            }
            BrowserError::ResponseTooLarge { bytes, max } => {
                write!(f, "Chrome's reply is {bytes} bytes; the cap is {max}")
            }
            BrowserError::TooManyTabs { max } => {
                write!(f, "the browser already holds {max} tabs; close one first")
            }
            BrowserError::UnknownTarget => {
                write!(
                    f,
                    "the target is not an open page in this browser; use browser.list"
                )
            }
            BrowserError::Navigation { url, message } => {
                write!(f, "the page {url} did not load: {message}")
            }
            BrowserError::Eval { message } => write!(f, "the expression failed: {message}"),
            BrowserError::ResultTooLarge { bytes, max } => {
                write!(f, "the result is {bytes} bytes; the cap is {max}")
            }
            BrowserError::ScreenshotTooLarge { bytes, max } => {
                write!(f, "the screenshot is {bytes} bytes; the cap is {max}")
            }
            BrowserError::Screenshot(message) => write!(f, "the screenshot was refused: {message}"),
            BrowserError::Io(error) => {
                write!(f, "the browser profile could not be used: {error}")
            }
        }
    }
}

impl std::error::Error for BrowserError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BrowserError::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// The Chrome binary this device would start: `SUMMRISE_CHROME`, then the conventional paths.
///
/// The environment variable wins only when it names an existing file; otherwise the search falls
/// through, so a stale variable on an operator's shell does not hide an installed browser.
pub fn discover_chrome() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os("SUMMRISE_CHROME") {
        let path = PathBuf::from(configured);
        if path.is_file() {
            return Some(path);
        }
    }
    CHROME_CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

/// One target as `/json/list` spells it.
#[derive(Clone, Debug, Deserialize)]
struct TargetEntry {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(rename = "webSocketDebuggerUrl", default)]
    web_socket_url: String,
}

impl From<TargetEntry> for TargetInfo {
    fn from(entry: TargetEntry) -> TargetInfo {
        TargetInfo {
            id: entry.id,
            title: entry.title,
            url: entry.url,
            kind: entry.kind,
        }
    }
}

/// A minimal CDP client: one WebSocket, one outstanding command at a time, id-correlated replies.
struct CdpClient {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    /// Events seen while a command's reply was awaited, so a later `wait_for_event` cannot lose one
    /// that arrived first.
    events: VecDeque<Value>,
}

impl CdpClient {
    /// Connect to one of Chrome's debugger endpoints.
    async fn connect(url: &str) -> Result<CdpClient, BrowserError> {
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE_BYTES))
            .max_frame_size(Some(MAX_MESSAGE_BYTES));
        let connect = connect_async_with_config(url, Some(config), true);
        let (socket, _response) =
            tokio::time::timeout(Duration::from_millis(COMMAND_TIMEOUT_MS), connect)
                .await
                .map_err(|_| BrowserError::Timeout {
                    method: "the CDP connect",
                    timeout_ms: COMMAND_TIMEOUT_MS,
                })?
                .map_err(|error| BrowserError::WebSocket {
                    message: error.to_string(),
                })?;
        Ok(CdpClient {
            socket,
            next_id: 1,
            events: VecDeque::new(),
        })
    }

    /// Send one command and return its `result`, ignoring events until the matching reply.
    async fn command(
        &mut self,
        method: &'static str,
        params: Value,
    ) -> Result<Value, BrowserError> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({ "id": id, "method": method, "params": params }).to_string();

        let round_trip = async {
            self.socket
                .send(Message::text(payload))
                .await
                .map_err(|error| BrowserError::WebSocket {
                    message: error.to_string(),
                })?;
            loop {
                let message = self.next_message().await?;
                if message.get("id").and_then(Value::as_u64) == Some(id) {
                    if let Some(error) = message.get("error") {
                        let message = error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("Chrome gave no message")
                            .to_owned();
                        return Err(BrowserError::Protocol { method, message });
                    }
                    return Ok(message.get("result").cloned().unwrap_or(Value::Null));
                }
                if event_name(&message).is_some() {
                    self.events.push_back(message);
                    if self.events.len() > MAX_BUFFERED_EVENTS {
                        self.events.pop_front();
                    }
                }
            }
        };

        tokio::time::timeout(Duration::from_millis(COMMAND_TIMEOUT_MS), round_trip)
            .await
            .map_err(|_| BrowserError::Timeout {
                method,
                timeout_ms: COMMAND_TIMEOUT_MS,
            })?
    }

    /// Wait for the first of `events`, returning one buffered while a command was in flight.
    ///
    /// `deadline` is the whole wait's bound, not one read's: a stream of unrelated events can keep
    /// the socket busy without extending it. The caller retries with the same deadline after an
    /// event it cannot use.
    async fn wait_for_event(
        &mut self,
        events: &[&'static str],
        deadline: Instant,
    ) -> Result<Value, BrowserError> {
        let timeout_error = || BrowserError::Timeout {
            method: events.first().copied().unwrap_or("the CDP event"),
            timeout_ms: COMMAND_TIMEOUT_MS,
        };
        if let Some(index) = self
            .events
            .iter()
            .position(|message| event_name(message).is_some_and(|name| events.contains(&name)))
        {
            return Ok(self.events.remove(index).unwrap_or(Value::Null));
        }

        let wait = async {
            loop {
                let message = self.next_message().await?;
                if event_name(&message).is_some_and(|name| events.contains(&name)) {
                    return Ok(message);
                }
            }
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(timeout_error());
        }
        tokio::time::timeout(remaining, wait)
            .await
            .map_err(|_| timeout_error())?
    }

    /// The next JSON message; ping/pong frames are handled by tungstenite and skipped.
    async fn next_message(&mut self) -> Result<Value, BrowserError> {
        loop {
            let Some(message) = self.socket.next().await else {
                return Err(BrowserError::WebSocket {
                    message: "the browser closed the CDP connection".to_owned(),
                });
            };
            let message = message.map_err(|error| BrowserError::WebSocket {
                message: error.to_string(),
            })?;
            match message {
                Message::Text(text) => {
                    if text.len() > MAX_MESSAGE_BYTES {
                        return Err(BrowserError::MessageTooLarge {
                            bytes: text.len(),
                            max: MAX_MESSAGE_BYTES,
                        });
                    }
                    return serde_json::from_str(text.as_str()).map_err(|error| {
                        BrowserError::Protocol {
                            method: "the CDP message",
                            message: format!("the reply is not JSON: {error}"),
                        }
                    });
                }
                Message::Binary(bytes) => {
                    return Err(BrowserError::Protocol {
                        method: "the CDP message",
                        message: format!(
                            "a {} byte binary message arrived where JSON was expected",
                            bytes.len()
                        ),
                    })
                }
                Message::Close(_) => {
                    return Err(BrowserError::WebSocket {
                        message: "the browser closed the CDP connection".to_owned(),
                    })
                }
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }
    }
}

/// The event name of a message that is not a reply to a command.
fn event_name(message: &Value) -> Option<&str> {
    if message.get("id").is_some() {
        return None;
    }
    message.get("method").and_then(Value::as_str)
}

/// What a navigation event says about the document this call asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Commit {
    /// The main frame committed the requested document.
    Document,
    /// The main frame committed Chrome's error page (`chrome-error://chromewebdata/`).
    ErrorPage,
    /// Another frame, another navigation, or an event that cannot be bound to this one.
    Other,
}

/// Bind a navigation event to the navigation `Page.navigate` started.
///
/// `Page.frameNavigated` carries the frame as `params.frame` (id, loaderId, url);
/// `Page.navigatedWithinDocument` carries `frameId` and `url` beside each other and has no loader.
/// A cross-document commit matches by loader id — which survives redirects — or, when no loader id
/// was returned, by the requested URL. A fragment matches by frame and URL.
fn commit_of(event: &Value, frame_id: Option<&str>, loader_id: Option<&str>, url: &str) -> Commit {
    let Some(expected_frame) = frame_id else {
        return Commit::Other;
    };
    let Some(params) = event.get("params") else {
        return Commit::Other;
    };
    match event_name(event) {
        Some("Page.frameNavigated") => {
            let Some(frame) = params.get("frame") else {
                return Commit::Other;
            };
            if frame.get("id").and_then(Value::as_str) != Some(expected_frame) {
                return Commit::Other;
            }
            let event_url = frame.get("url").and_then(Value::as_str);
            let loader_matches =
                loader_id.is_some() && frame.get("loaderId").and_then(Value::as_str) == loader_id;
            if event_url.is_some_and(|event_url| event_url.starts_with("chrome-error://"))
                && (loader_matches || event_url == Some(url))
            {
                return Commit::ErrorPage;
            }
            if loader_matches || event_url == Some(url) {
                Commit::Document
            } else {
                Commit::Other
            }
        }
        Some("Page.navigatedWithinDocument") => {
            if params.get("frameId").and_then(Value::as_str) == Some(expected_frame)
                && params.get("url").and_then(Value::as_str) == Some(url)
            {
                Commit::Document
            } else {
                Commit::Other
            }
        }
        _ => Commit::Other,
    }
}

/// A navigation refusal that names the URL it was asked for.
fn navigation_error(url: &str, message: &str) -> BrowserError {
    BrowserError::Navigation {
        url: truncate(url, MAX_URL_IN_ERROR_BYTES),
        message: truncate(message, MAX_ERROR_BYTES),
    }
}

/// Whether a document's `readyState` means its markup has been parsed.
fn parsed(state: &str) -> bool {
    state == "interactive" || state == "complete"
}

/// Poll `document.readyState` until the committed document has finished parsing.
///
/// The commit event says the document exists; the parser may still be waiting on a script, in which
/// case the title is not in the DOM yet. Polling the document itself is the only answer Chrome
/// gives to "has the parser finished". A protocol error is retried (the document can be replaced
/// mid-poll); a transport failure is returned as itself; the deadline is a navigation refusal.
async fn wait_for_ready_state(
    page: &mut CdpClient,
    deadline: Instant,
    url: &str,
) -> Result<(), BrowserError> {
    let expired = || {
        navigation_error(
            url,
            &format!("the document did not finish parsing within {COMMAND_TIMEOUT_MS} ms"),
        )
    };
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(expired());
        }
        let evaluate = page.command(
            "Runtime.evaluate",
            json!({ "expression": "document.readyState", "returnByValue": true }),
        );
        match tokio::time::timeout(remaining, evaluate).await {
            Ok(Ok(reply)) => {
                let state = reply
                    .get("result")
                    .and_then(|result| result.get("value"))
                    .and_then(Value::as_str);
                if state.is_some_and(parsed) {
                    return Ok(());
                }
            }
            Ok(Err(BrowserError::Protocol { .. })) => {}
            Ok(Err(BrowserError::Timeout { .. })) | Err(_) => return Err(expired()),
            Ok(Err(error)) => return Err(error),
        }
        if Instant::now() >= deadline {
            return Err(expired());
        }
        tokio::time::sleep(Duration::from_millis(READY_STATE_POLL_MS)).await;
    }
}

/// A fresh profile directory name for one launch.
///
/// The pid and nanosecond stamp make it unique across processes and restarts; the counter makes it
/// unique within a process even if two launches start in the same clock tick.
fn profile_name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    format!(
        "run-{}-{nanos}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// Remove profile directories a previous process could not remove itself.
///
/// This runs **before** the current launch picks its directory, so it can never remove that one. A
/// directory is removed when it is a day old, or when its `SingletonLock` names no live process.
/// On Unix a directory whose lock names a live process is never touched: that is a Chrome the
/// killed device left running, and deleting its profile would break a browser this device can still
/// observe. Windows cannot read a pid from the lock, so only the age rule applies there.
fn sweep_profiles(profiles: &Path) {
    let Ok(entries) = fs::read_dir(profiles) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        let owner = lock_owner_running(&path);
        if owner == Some(true) {
            continue;
        }
        let old = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age >= Duration::from_millis(PROFILE_MAX_AGE_MS));
        let stale_lock = owner == Some(false);
        if old || stale_lock {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

/// Whether the profile's `SingletonLock` names a process this host still has.
///
/// `None` means the platform cannot tell (no `SingletonLock`, or Windows, where only the age rule
/// applies). Chrome writes the lock as a symlink to `<hostname>-<pid>`.
#[cfg(unix)]
fn lock_owner_running(profile: &Path) -> Option<bool> {
    let target = fs::read_link(profile.join("SingletonLock")).ok()?;
    let pid = target
        .to_string_lossy()
        .rsplit('-')
        .next()
        .and_then(|pid| pid.parse::<i32>().ok())?;
    if pid <= 1 {
        return Some(false);
    }
    Some(kill(Pid::from_raw(pid), None).is_ok())
}

/// Windows: the lock format is not a pid, so removal is left to the age rule, the safe direction.
#[cfg(not(unix))]
fn lock_owner_running(_profile: &Path) -> Option<bool> {
    None
}

/// The number of bytes a canonical base64 string decodes to.
fn decoded_base64_len(data: &str) -> usize {
    let padding = data.bytes().rev().take_while(|byte| *byte == b'=').count();
    (data.len() / 4 * 3).saturating_sub(padding.min(2))
}

/// One headless Chrome process and the browser endpoint it is listening on.
struct Browser {
    port: u16,
    /// The browser endpoint Chrome named in `/json/version`, e.g.
    /// `ws://127.0.0.1:<port>/devtools/browser/<uuid>`.
    browser_ws: String,
    child: Child,
    /// This launch's private profile directory; `Drop` removes it after killing the browser.
    profile: PathBuf,
    http: reqwest::Client,
    /// The browser endpoint connection, opened on the first target command.
    socket: Option<CdpClient>,
}

impl Browser {
    /// Start Chrome under `data`'s profile and wait for its DevTools endpoint.
    ///
    /// A launch is attempted with a port this capability chose; if Chrome exits because that port
    /// was taken in the gap, [`next_action`] retries with a new port and profile, up to
    /// [`MAX_LAUNCH_ATTEMPTS`]. Any other failure is returned from its attempt.
    async fn launch(
        data: &Path,
        chrome: Option<&Path>,
        readiness_timeout_ms: u64,
    ) -> Result<Browser, BrowserError> {
        let executable = match chrome {
            Some(path) => path.to_owned(),
            None => discover_chrome().ok_or(BrowserError::ChromeNotFound)?,
        };
        // A killed device leaves its Chrome (and the profile lock) behind. Every launch gets its
        // own directory so an old lock can never wedge this one; the sweep runs before this
        // launch's directory exists, so it can never delete the one about to be created.
        let profiles = data.join("chrome-profile");
        fs::create_dir_all(&profiles).map_err(BrowserError::Io)?;
        sweep_profiles(&profiles);

        let mut attempt = 1;
        loop {
            match launch_once(&executable, &profiles, readiness_timeout_ms).await {
                Ok(browser) => return Ok(browser),
                Err((outcome, error)) => match next_action(outcome, attempt) {
                    LaunchAction::Fail => return Err(error),
                    LaunchAction::Retry(next) => attempt = next,
                },
            }
        }
    }

    /// Whether the browser process is still running; `try_wait` also reaps an exited child.
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// The targets Chrome reports over HTTP, in Chrome's order.
    async fn targets(&self) -> Result<Vec<TargetEntry>, BrowserError> {
        let url = format!("http://127.0.0.1:{}/json/list", self.port);
        let read = async {
            let response =
                self.http
                    .get(&url)
                    .send()
                    .await
                    .map_err(|error| BrowserError::Http {
                        status: None,
                        message: error.to_string(),
                    })?;
            let status = response.status();
            let body = response.text().await.map_err(|error| BrowserError::Http {
                status: Some(status.as_u16()),
                message: error.to_string(),
            })?;
            if !status.is_success() {
                return Err(BrowserError::Http {
                    status: Some(status.as_u16()),
                    message: "the target list was refused".to_owned(),
                });
            }
            Ok(body)
        };

        let body = tokio::time::timeout(Duration::from_millis(COMMAND_TIMEOUT_MS), read)
            .await
            .map_err(|_| BrowserError::Timeout {
                method: "GET /json/list",
                timeout_ms: COMMAND_TIMEOUT_MS,
            })??;
        if body.len() > MAX_TARGET_LIST_BYTES {
            return Err(BrowserError::ResponseTooLarge {
                bytes: body.len(),
                max: MAX_TARGET_LIST_BYTES,
            });
        }
        serde_json::from_str(&body).map_err(|error| BrowserError::Http {
            status: None,
            message: format!("the target list is not JSON: {error}"),
        })
    }

    /// Send one browser-scoped command, opening the browser endpoint on first use.
    async fn browser_command(
        &mut self,
        method: &'static str,
        params: Value,
    ) -> Result<Value, BrowserError> {
        if self.socket.is_none() {
            self.socket = Some(CdpClient::connect(&self.browser_ws).await?);
        }
        self.socket
            .as_mut()
            .expect("the browser endpoint was just connected")
            .command(method, params)
            .await
    }

    /// Connect to one page target's own endpoint.
    async fn page_client(&mut self, target: &str) -> Result<CdpClient, BrowserError> {
        let targets = self.targets().await?;
        let entry = targets
            .iter()
            .find(|entry| {
                entry.id == target && entry.kind == "page" && !entry.web_socket_url.is_empty()
            })
            .ok_or(BrowserError::UnknownTarget)?;
        CdpClient::connect(&entry.web_socket_url).await
    }

    /// Create a tab and wait for its document to be parsed.
    async fn open(&mut self, url: &str) -> Result<String, BrowserError> {
        let pages = self
            .targets()
            .await?
            .iter()
            .filter(|target| target.kind == "page")
            .count();
        if pages >= MAX_TABS {
            return Err(BrowserError::TooManyTabs { max: MAX_TABS });
        }

        // Create with about:blank, then navigate through the page endpoint: the commit wait has to
        // be subscribed before the navigation starts (a target created directly at `url` can finish
        // loading before Page.enable runs, and then there would be no event left to wait for).
        let created = self
            .browser_command("Target.createTarget", json!({ "url": "about:blank" }))
            .await?;
        let target = created
            .get("targetId")
            .and_then(Value::as_str)
            .ok_or_else(|| BrowserError::Protocol {
                method: "Target.createTarget",
                message: "the reply names no target".to_owned(),
            })?
            .to_owned();

        match self.load_target(&target, url).await {
            Ok(()) => Ok(target),
            Err(error) => {
                // The tab exists even though its first navigation failed. Leaving it would let a
                // client fill the tab cap with refused opens, so close what this call created.
                let _ = self
                    .browser_command("Target.closeTarget", json!({ "targetId": &target }))
                    .await;
                Err(error)
            }
        }
    }

    /// Navigate one page and wait for the new document to be parsed.
    async fn navigate(&mut self, target: &str, url: &str) -> Result<(), BrowserError> {
        self.load_target(target, url).await
    }

    /// Load `url` in `target` and return once the new document has finished parsing.
    ///
    /// `Page.navigate` returns when the navigation is *started*, not when it lands, and a
    /// `Page.loadEventFired` carries nothing that ties it to a navigation — the target's previous
    /// document can still fire one. The commit event does identify its navigation:
    /// `Page.frameNavigated` names the frame, the document loader and the URL, and the `loaderId`
    /// `Page.navigate` returned survives redirects. A same-document navigation is reported as
    /// `Page.navigatedWithinDocument`.
    ///
    /// A commit only means the document exists: a parser waiting on a script has not reached the
    /// title yet, so an evaluation immediately after the commit can still read `""` (measured on
    /// the Windows lane). This therefore polls `document.readyState` after the commit until it is
    /// `interactive` (markup parsed, title available) or `complete`, every [`READY_STATE_POLL_MS`],
    /// all within the same deadline.
    ///
    /// The wait ends on the first of:
    /// - this navigation's main frame committing (matching loader, or the requested URL), then the
    ///   document reaching a parsed ready state: return;
    /// - the main frame committing Chrome's error page (`chrome-error://`) for this loader or URL:
    ///   a typed refusal;
    /// - the deadline (commit or parsing): a typed refusal naming the URL. A reply that already
    ///   carries `errorText` (an unsafe or refused port, an unreachable host) is refused before any
    ///   wait.
    async fn load_target(&mut self, target: &str, url: &str) -> Result<(), BrowserError> {
        let mut page = self.page_client(target).await?;
        page.command("Page.enable", json!({})).await?;
        let navigation = page.command("Page.navigate", json!({ "url": url })).await?;

        if let Some(message) = navigation.get("errorText").and_then(Value::as_str) {
            return Err(navigation_error(url, message));
        }
        // A download has no document to wait for.
        if navigation.get("isDownload").and_then(Value::as_bool) == Some(true) {
            return Ok(());
        }

        let frame_id = navigation.get("frameId").and_then(Value::as_str);
        let loader_id = navigation.get("loaderId").and_then(Value::as_str);
        let deadline = Instant::now() + Duration::from_millis(COMMAND_TIMEOUT_MS);
        loop {
            let event = match page
                .wait_for_event(
                    &["Page.frameNavigated", "Page.navigatedWithinDocument"],
                    deadline,
                )
                .await
            {
                Ok(event) => event,
                Err(BrowserError::Timeout { .. }) => {
                    return Err(navigation_error(
                        url,
                        &format!("no document committed within {COMMAND_TIMEOUT_MS} ms"),
                    ))
                }
                Err(error) => return Err(error),
            };
            match commit_of(&event, frame_id, loader_id, url) {
                Commit::Document => return wait_for_ready_state(&mut page, deadline, url).await,
                Commit::ErrorPage => {
                    return Err(navigation_error(
                        url,
                        "the browser committed its error page",
                    ))
                }
                Commit::Other => {}
            }
        }
    }

    /// Evaluate one expression in the page's current document.
    async fn eval(&mut self, target: &str, expression: &str) -> Result<EvalValue, BrowserError> {
        let mut page = self.page_client(target).await?;
        let reply = page
            .command(
                "Runtime.evaluate",
                json!({ "expression": expression, "returnByValue": true }),
            )
            .await?;

        if let Some(exception) = reply.get("exceptionDetails") {
            let message = exception
                .get("exception")
                .and_then(|exception| exception.get("description"))
                .and_then(Value::as_str)
                .or_else(|| exception.get("text").and_then(Value::as_str))
                .unwrap_or("the expression threw");
            return Err(BrowserError::Eval {
                message: truncate(message, MAX_ERROR_BYTES),
            });
        }

        let result = reply.get("result").cloned().unwrap_or(Value::Null);
        let bytes = result.to_string().len();
        if bytes > EVAL_MAX_BYTES {
            return Err(BrowserError::ResultTooLarge {
                bytes,
                max: EVAL_MAX_BYTES,
            });
        }
        Ok(EvalValue {
            kind: result
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            value: result.get("value").cloned(),
        })
    }

    /// Capture one page as a base64 PNG.
    async fn screenshot(&mut self, target: &str) -> Result<Screenshot, BrowserError> {
        let mut page = self.page_client(target).await?;
        let reply = page
            .command("Page.captureScreenshot", json!({ "format": "png" }))
            .await?;
        let data = reply
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| BrowserError::Protocol {
                method: "Page.captureScreenshot",
                message: "the reply carries no image".to_owned(),
            })?
            .to_owned();

        let bytes = decoded_base64_len(&data);
        if bytes > SCREENSHOT_MAX_BYTES {
            return Err(BrowserError::ScreenshotTooLarge {
                bytes,
                max: SCREENSHOT_MAX_BYTES,
            });
        }
        if !data.starts_with(PNG_BASE64_PREFIX) {
            return Err(BrowserError::Screenshot(
                "Chrome's reply is not a PNG".to_owned(),
            ));
        }
        Ok(Screenshot { data, bytes })
    }

    /// Close one page target.
    async fn close(&mut self, target: &str) -> Result<(), BrowserError> {
        // Check first so an unknown id is the typed refusal rather than Chrome's protocol error.
        if !self.targets().await?.iter().any(|entry| entry.id == target) {
            return Err(BrowserError::UnknownTarget);
        }
        let reply = self
            .browser_command("Target.closeTarget", json!({ "targetId": target }))
            .await?;
        if reply.get("success").and_then(Value::as_bool) == Some(true) {
            Ok(())
        } else {
            Err(BrowserError::UnknownTarget)
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        terminate(&mut self.child);
        // Best effort: on Windows a renderer that has not finished exiting can still hold a file.
        // A directory this call cannot remove is swept by a later launch.
        let _ = fs::remove_dir_all(&self.profile);
    }
}

/// Kill a Chrome child and reap it, signalling its whole process group on Unix.
///
/// `try_wait` is the ownership check: a child that already exited was reaped by a previous call,
/// and its pid may have been recycled by the kernel, so its group is not safe to signal. The same
/// reasoning as `terminal.close`'s session close.
fn terminate(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    #[cfg(unix)]
    {
        if let Ok(pid) = i32::try_from(child.id()) {
            let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// One launch attempt: a fresh profile, a port this capability chose, and Chrome answering it.
async fn launch_once(
    executable: &Path,
    profiles: &Path,
    readiness_timeout_ms: u64,
) -> Result<Browser, (LaunchOutcome, BrowserError)> {
    let profile = profiles.join(profile_name());
    fs::create_dir_all(&profile)
        .map_err(|error| (LaunchOutcome::Other, BrowserError::Io(error)))?;

    // Chrome's stderr goes to a file in the profile: a launch that fails can then say why, and a
    // pipe would have to be drained or Chrome would block on a full one. The file lives exactly as
    // long as the profile directory (removed on Drop or on a failed attempt).
    let stderr_log = fs::File::create(profile.join(CHROME_STDERR))
        .map_err(|error| (LaunchOutcome::Other, BrowserError::Io(error)))?;
    let port = free_port().map_err(|error| (LaunchOutcome::Other, BrowserError::Io(error)))?;

    let mut command = Command::new(executable);
    command
        .arg("--headless=new")
        .arg(format!("--remote-debugging-port={port}"))
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-gpu")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_log));
    // Chrome's renderers and helpers inherit the process group. A kill of the group is what makes
    // "shutdown leaves no chrome child" true even when a renderer outlives the browser.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_dir_all(&profile);
            return Err((
                LaunchOutcome::Other,
                BrowserError::Spawn {
                    path: executable.display().to_string(),
                    message: error.to_string(),
                },
            ));
        }
    };

    let http = reqwest::Client::new();
    match probe_browser_ws(&http, &mut child, port, &profile, readiness_timeout_ms).await {
        Ok(browser_ws) => Ok(Browser {
            port,
            browser_ws,
            child,
            profile,
            http,
            socket: None,
        }),
        Err(failure) => {
            // A failed attempt is still a process and a profile this capability started; neither
            // may outlive the error.
            terminate(&mut child);
            let _ = fs::remove_dir_all(&profile);
            Err(failure)
        }
    }
}

/// A free loopback port: bind `127.0.0.1:0`, read what the OS chose, then release it.
///
/// The release is what makes this a hint rather than a reservation: another process can take the
/// port before Chrome binds it, which Chrome reports as a bind failure and [`next_action`] retries.
fn free_port() -> io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    listener.local_addr().map(|address| address.port())
}

/// Ask `GET /json/version` once; `Some(body)` only when it answered 200 with JSON.
async fn probe_version(http: &reqwest::Client, url: &str) -> Option<Value> {
    let response = http.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.bytes().await.ok()?;
    serde_json::from_slice(&body).ok()
}

/// Probe `GET /json/version` until Chrome names its browser endpoint, or the attempt fails.
async fn probe_browser_ws(
    http: &reqwest::Client,
    child: &mut Child,
    port: u16,
    profile: &Path,
    timeout_ms: u64,
) -> Result<String, (LaunchOutcome, BrowserError)> {
    let url = format!("http://127.0.0.1:{port}/json/version");
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let bind_failure = looks_like_bind_failure(&read_tail(
                    &profile.join(CHROME_STDERR),
                    STDERR_TAIL_BYTES,
                ));
                let outcome = if bind_failure {
                    LaunchOutcome::BindFailure
                } else {
                    LaunchOutcome::Exited
                };
                return Err((
                    outcome,
                    BrowserError::Launch(format!(
                        "chrome exited with {status} before it answered {url}; {}",
                        stderr_note(profile)
                    )),
                ));
            }
            Ok(None) => {}
            Err(error) => return Err((LaunchOutcome::Other, BrowserError::Io(error))),
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err((
                LaunchOutcome::Timeout,
                BrowserError::Launch(format!(
                    "chrome did not answer {url} within {timeout_ms} ms; {}",
                    launch_diagnosis(child, profile)
                )),
            ));
        }

        if let Ok(Some(body)) = tokio::time::timeout(
            remaining.min(Duration::from_millis(PROBE_REQUEST_TIMEOUT_MS)),
            probe_version(http, &url),
        )
        .await
        {
            if let Some(endpoint) = body.get("webSocketDebuggerUrl").and_then(Value::as_str) {
                if browser_endpoint(endpoint, port) {
                    return Ok(endpoint.to_owned());
                }
                return Err((
                    LaunchOutcome::Malformed,
                    BrowserError::Launch(format!(
                        "chrome answered {url} with an unexpected webSocketDebuggerUrl: {endpoint}; {}",
                        stderr_note(profile)
                    )),
                ));
            }
        }

        tokio::time::sleep(Duration::from_millis(PROBE_INTERVAL_MS).min(remaining)).await;
    }
}

/// Whether `endpoint` is the browser WebSocket endpoint of the port this attempt chose.
fn browser_endpoint(endpoint: &str, port: u16) -> bool {
    let Some(rest) = endpoint.strip_prefix("ws://") else {
        return false;
    };
    let Some((host, path)) = rest.split_once('/') else {
        return false;
    };
    (host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}"))
        && path.starts_with("devtools/browser/")
        && path.len() > "devtools/browser/".len()
}

/// Whether Chrome's stderr says it could not bind the port this attempt chose.
///
/// The wording differs by platform and version (`bind() failed`, the POSIX and WinSock "address in
/// use" sentences); any of them means the gap between [`free_port`] and Chrome's bind was lost.
fn looks_like_bind_failure(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    [
        "bind() failed",
        "address already in use",
        "only one usage of each socket address",
        "cannot start http server for devtools",
    ]
    .iter()
    .any(|needle| stderr.contains(needle))
}

/// Why one launch attempt failed; only a lost port is worth another attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LaunchOutcome {
    /// Chrome could not bind the chosen port (another process took it in the gap).
    BindFailure,
    /// Chrome exited for another reason before it answered.
    Exited,
    /// Chrome stayed alive but did not answer within the readiness bound.
    Timeout,
    /// Chrome answered with an endpoint this capability cannot use.
    Malformed,
    /// The attempt failed before a browser existed (spawn, profile, port).
    Other,
}

/// What the launch loop does after an attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LaunchAction {
    /// Try again with this attempt number, a new port and a new profile.
    Retry(u8),
    /// Return the attempt's error.
    Fail,
}

/// Retry only a bind collision, and only while attempts remain.
fn next_action(outcome: LaunchOutcome, attempt: u8) -> LaunchAction {
    if outcome == LaunchOutcome::BindFailure && attempt < MAX_LAUNCH_ATTEMPTS {
        LaunchAction::Retry(attempt + 1)
    } else {
        LaunchAction::Fail
    }
}

/// The last `max` bytes of a file, flattened so one refusal stays one line.
fn read_tail(path: &Path, max: u64) -> String {
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    let length = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    if length > max && file.seek(SeekFrom::Start(length - max)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    if file.take(max).read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Chrome's stderr tail for a launch-failure message, or that it wrote nothing.
fn stderr_note(profile: &Path) -> String {
    let tail = read_tail(&profile.join(CHROME_STDERR), STDERR_TAIL_BYTES);
    if tail.is_empty() {
        "no stderr output".to_owned()
    } else {
        format!("stderr tail: {tail}")
    }
}

/// Whether the child is alive (and its status if not), plus its stderr tail.
///
/// A launch refusal that says only "timed out" leaves the reader guessing. This distinguishes "the
/// process is still running" from a crash, and quotes what Chrome wrote.
fn launch_diagnosis(child: &mut Child, profile: &Path) -> String {
    let state = match child.try_wait() {
        Ok(None) => "chrome is still running".to_owned(),
        Ok(Some(status)) => format!("chrome exited with {status}"),
        Err(error) => format!("chrome's status could not be read: {error}"),
    };
    format!("{state}; {}", stderr_note(profile))
}

/// Keep at most `max` bytes of `text`, on a character boundary, with an ellipsis when cut.
fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

/// The device's one browser: Chrome is started by the first tool call that needs it and is killed
/// when the registry is dropped or `browser.shutdown` runs.
///
/// The lock is held for a whole operation, so two calls cannot race the tab cap or interleave on the
/// one browser endpoint. A command is bounded by [`COMMAND_TIMEOUT_MS`], which is the longest a
/// waiting call can be held behind another.
pub struct BrowserRegistry {
    data: PathBuf,
    /// The discovered executable, or `None` to discover per launch. The tests name one so their
    /// skip decision is the one the launch uses.
    chrome: Option<PathBuf>,
    /// How long Chrome may take to answer `/json/version`; production uses
    /// [`READINESS_TIMEOUT_MS`], a test can shorten it.
    readiness_timeout_ms: u64,
    browser: tokio::sync::Mutex<Option<Browser>>,
}

impl BrowserRegistry {
    /// A registry that launches Chrome from `<data>`'s profile when a tool first needs it.
    pub fn new(data: &Path) -> BrowserRegistry {
        BrowserRegistry {
            data: data.to_owned(),
            chrome: None,
            readiness_timeout_ms: READINESS_TIMEOUT_MS,
            browser: tokio::sync::Mutex::new(None),
        }
    }

    /// A registry that starts exactly `chrome`; the Linux tests use it after one skip decision.
    #[cfg(all(test, target_os = "linux"))]
    fn with_chrome(data: &Path, chrome: PathBuf) -> BrowserRegistry {
        BrowserRegistry {
            data: data.to_owned(),
            chrome: Some(chrome),
            readiness_timeout_ms: READINESS_TIMEOUT_MS,
            browser: tokio::sync::Mutex::new(None),
        }
    }

    /// The same registry with a short readiness bound, so a test does not wait 30 s.
    #[cfg(all(test, target_os = "linux"))]
    fn with_chrome_timeout(
        data: &Path,
        chrome: PathBuf,
        readiness_timeout_ms: u64,
    ) -> BrowserRegistry {
        BrowserRegistry {
            data: data.to_owned(),
            chrome: Some(chrome),
            readiness_timeout_ms,
            browser: tokio::sync::Mutex::new(None),
        }
    }

    /// Start Chrome if it is not running; return the live browser.
    async fn browser(&self) -> Result<tokio::sync::MutexGuard<'_, Option<Browser>>, BrowserError> {
        let mut slot = self.browser.lock().await;
        let running = slot.as_mut().map(Browser::alive).unwrap_or(false);
        if !running {
            // Drop a crashed browser before replacing it: its Drop waits for the process.
            slot.take();
            *slot = Some(
                Browser::launch(
                    &self.data,
                    self.chrome.as_deref(),
                    self.readiness_timeout_ms,
                )
                .await?,
            );
        }
        Ok(slot)
    }

    /// The tabs the browser reports, in Chrome's order. The first call starts Chrome.
    pub async fn list(&self) -> Result<Vec<TargetInfo>, BrowserError> {
        let mut slot = self.browser().await?;
        let browser = slot.as_mut().expect("the registry just made it live");
        Ok(browser
            .targets()
            .await?
            .into_iter()
            .map(TargetInfo::from)
            .collect())
    }

    /// Open a tab at `url` and wait for its document to be parsed; returns the target id.
    pub async fn open(&self, url: &str) -> Result<String, BrowserError> {
        let mut slot = self.browser().await?;
        let browser = slot.as_mut().expect("the registry just made it live");
        browser.open(url).await
    }

    /// Navigate an existing page and wait for the new document to be parsed.
    pub async fn navigate(&self, target: &str, url: &str) -> Result<(), BrowserError> {
        let mut slot = self.browser().await?;
        let browser = slot.as_mut().expect("the registry just made it live");
        browser.navigate(target, url).await
    }

    /// Evaluate `expression` in a page's current document.
    pub async fn eval(&self, target: &str, expression: &str) -> Result<EvalValue, BrowserError> {
        let mut slot = self.browser().await?;
        let browser = slot.as_mut().expect("the registry just made it live");
        browser.eval(target, expression).await
    }

    /// Capture a page as a base64 PNG.
    pub async fn screenshot(&self, target: &str) -> Result<Screenshot, BrowserError> {
        let mut slot = self.browser().await?;
        let browser = slot.as_mut().expect("the registry just made it live");
        browser.screenshot(target).await
    }

    /// Close one page; the browser keeps running (and any other tab stays).
    pub async fn close(&self, target: &str) -> Result<(), BrowserError> {
        let mut slot = self.browser().await?;
        let browser = slot.as_mut().expect("the registry just made it live");
        browser.close(target).await
    }

    /// Stop the browser this registry started; returns whether one was running.
    ///
    /// This is not an error when nothing is running: `shutdown` is the action a client takes to be
    /// sure no browser is left, and "already stopped" is that state, not a failure.
    pub async fn shutdown(&self) -> bool {
        let stopped = self.browser.lock().await.take();
        stopped.is_some()
    }

    /// The running Chrome's pid, for the tests that prove it is reaped.
    #[cfg(all(test, target_os = "linux"))]
    fn pid(&self) -> Option<u32> {
        self.browser
            .try_lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|browser| browser.child.id()))
    }
}

impl Drop for BrowserRegistry {
    fn drop(&mut self) {
        // `get_mut` needs `&mut self`, so no lock can be held here; dropping the Browser kills and
        // reaps Chrome, including on a device that is shutting down.
        self.browser.get_mut().take();
    }
}

#[cfg(all(test, target_os = "linux"))]
/// Find Chrome, or make the skip visible and let the caller return.
///
/// A missing Chrome is a failed lane in CI (the runner image has it) and a skip on a
/// developer's box. libtest captures `println!` from a passing test, so the skip is written to
/// the process's stderr descriptor: a plain `cargo test` shows it, not only `--nocapture`. The
/// line is printed once per process however many tests then return.
pub(crate) fn chrome_or_skip() -> Option<PathBuf> {
    match discover_chrome() {
        Some(chrome) => Some(chrome),
        None => {
            if std::env::var_os("CI").is_some() {
                panic!(
                    "browser.cdp is tested against a real Chrome, and CI must have one; \
                     set SUMMRISE_CHROME to the executable"
                );
            }
            static ANNOUNCED: std::sync::Once = std::sync::Once::new();
            ANNOUNCED.call_once(|| {
                let line = "SKIP browser.cdp: no Chrome/Chromium was found (set \
                            SUMMRISE_CHROME); the browser tests did not run\n";
                if fs::write("/dev/stderr", line).is_err() {
                    eprintln!("{line}");
                }
            });
            None
        }
    }
}

/// Serializes the tests that launch Chrome.
///
/// The capability supports concurrent browsers, but the test process should not prove it by
/// starting four Chromes on a small CI runner: the launches starve each other and every one hits
/// the launch deadline. Production has no such gate.
#[cfg(all(test, target_os = "linux"))]
static LAUNCH_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Take the browser-launch gate for one test, and hold it until the test ends.
#[cfg(all(test, target_os = "linux"))]
pub(crate) async fn launch_gate() -> tokio::sync::MutexGuard<'static, ()> {
    LAUNCH_GATE.lock().await
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use serde_json::json;

    use crate::test_support::TempDir;

    use super::*;

    const FIRST_TITLE: &str = "SUMMRISE_CDP_OK";
    const SECOND_TITLE: &str = "SUMMRISE_CDP_SECOND";

    fn data_url(title: &str) -> String {
        format!("data:text/html,<title>{title}</title>")
    }

    /// A temp directory and a registry that will run `chrome`, or `None` after a visible skip.
    fn fixture(label: &str) -> Option<(TempDir, PathBuf, BrowserRegistry)> {
        let chrome = chrome_or_skip()?;
        let dir = TempDir::new(label);
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data directory is creatable");
        let registry = BrowserRegistry::with_chrome(&data, chrome);
        Some((dir, data, registry))
    }

    /// The pids whose command line names this profile: Chrome's children inherit the flag.
    fn processes_with_profile(profile: &Path) -> Vec<u32> {
        let needle = profile.to_string_lossy().into_owned();
        let mut found = Vec::new();
        let Ok(entries) = fs::read_dir("/proc") else {
            return found;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            if pid == std::process::id() {
                continue;
            }
            let Ok(cmdline) = fs::read(entry.path().join("cmdline")) else {
                continue;
            };
            if String::from_utf8_lossy(&cmdline).contains(&needle) {
                found.push(pid);
            }
        }
        found
    }

    /// Wait for every process named by `profile` to be gone; returns what is left after the bound.
    async fn leftover_chrome(profile: &Path) -> Vec<u32> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let left = processes_with_profile(profile);
            if left.is_empty() || Instant::now() >= deadline {
                return left;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The number of `page` targets Chrome currently lists.
    async fn page_count(registry: &BrowserRegistry) -> usize {
        registry
            .list()
            .await
            .expect("the list reads")
            .iter()
            .filter(|target| target.kind == "page")
            .count()
    }

    /// Wait until Chrome lists `expected` pages; the target list lags a close by a moment.
    async fn pages_settle(registry: &BrowserRegistry, expected: usize) -> usize {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let pages = page_count(registry).await;
            if pages == expected || Instant::now() >= deadline {
                return pages;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    #[tokio::test]
    async fn a_launch_lists_one_page_and_shutdown_stops_chrome() {
        let _gate = launch_gate().await;
        let Some((_dir, data, registry)) = fixture("browser-launch") else {
            return;
        };
        let profile = data.join("chrome-profile");

        let targets = registry.list().await.expect("the browser starts and lists");
        let pages: Vec<&TargetInfo> = targets
            .iter()
            .filter(|target| target.kind == "page")
            .collect();
        assert_eq!(
            pages.len(),
            1,
            "a fresh headless Chrome has one page target: {targets:?}"
        );
        assert!(!pages[0].id.is_empty(), "a target has an id");

        let pid = registry.pid().expect("the browser is running");
        assert!(registry.shutdown().await, "a running browser was stopped");
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "shutdown reaps the browser process"
        );
        assert_eq!(
            leftover_chrome(&profile).await,
            Vec::<u32>::new(),
            "shutdown leaves no chrome process behind"
        );
        assert!(
            !registry.shutdown().await,
            "shutting down a stopped browser is not an error"
        );

        let target = registry
            .open(&data_url(FIRST_TITLE))
            .await
            .expect("a tool call after shutdown starts Chrome again on the same profile");
        assert!(!target.is_empty());
        registry.shutdown().await;

        let directories: Vec<String> = fs::read_dir(&profile)
            .expect("the profile root exists")
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            directories,
            Vec::<String>::new(),
            "a graceful shutdown removes the profile directory it created"
        );
    }

    #[tokio::test]
    async fn open_eval_navigate_screenshot_and_close_round_trip() {
        let _gate = launch_gate().await;
        let Some((_dir, data, registry)) = fixture("browser-round-trip") else {
            return;
        };

        let target = registry
            .open(&data_url(FIRST_TITLE))
            .await
            .expect("a data URL opens");
        assert!(
            registry
                .list()
                .await
                .expect("the list reads")
                .iter()
                .any(|entry| entry.id == target && entry.kind == "page"),
            "the opened tab is a listed page"
        );

        let title = registry
            .eval(&target, "document.title")
            .await
            .expect("the title evaluates");
        assert_eq!(title.kind, "string");
        assert_eq!(
            title.value,
            Some(json!(FIRST_TITLE)),
            "open waited for the load"
        );

        registry
            .navigate(&target, &data_url(SECOND_TITLE))
            .await
            .expect("the tab navigates");
        let title = registry
            .eval(&target, "document.title")
            .await
            .expect("the new title evaluates");
        assert_eq!(
            title.value,
            Some(json!(SECOND_TITLE)),
            "navigate waited for the second document"
        );

        let shot = registry
            .screenshot(&target)
            .await
            .expect("the tab is captured");
        assert!(
            shot.data.starts_with("iVBORw0KGgo"),
            "a base64 PNG starts with the PNG magic: {:?}",
            &shot.data[..shot.data.len().min(16)]
        );
        assert!(
            shot.bytes > 100,
            "a real screenshot is more than 100 bytes: {}",
            shot.bytes
        );

        let error = registry
            .eval(&target, "a_function_that_does_not_exist()")
            .await
            .expect_err("a thrown expression is a refusal");
        assert!(matches!(error, BrowserError::Eval { .. }), "{error:?}");

        let pages = page_count(&registry).await;
        let error = registry
            .open("http://[")
            .await
            .expect_err("Chrome refuses an invalid URL");
        assert!(matches!(error, BrowserError::Protocol { .. }), "{error:?}");
        assert_eq!(
            pages_settle(&registry, pages).await,
            pages,
            "an open whose load failed leaves no tab behind"
        );

        registry.close(&target).await.expect("the tab closes");
        assert_eq!(
            pages_settle(&registry, pages - 1).await,
            pages - 1,
            "a closed tab is not listed"
        );

        let error = registry
            .eval(&target, "document.title")
            .await
            .expect_err("a closed target is gone");
        assert!(matches!(error, BrowserError::UnknownTarget), "{error:?}");

        let error = registry
            .close("not-a-target")
            .await
            .expect_err("an unknown target cannot be closed");
        assert!(matches!(error, BrowserError::UnknownTarget), "{error:?}");

        assert!(registry.shutdown().await, "the browser was running");
        assert_eq!(
            leftover_chrome(&data.join("chrome-profile")).await,
            Vec::<u32>::new(),
            "shutdown leaves no chrome child behind"
        );
    }

    #[tokio::test]
    async fn the_ninth_tab_is_refused() {
        let _gate = launch_gate().await;
        let Some((_dir, _data, registry)) = fixture("browser-tabs-cap") else {
            return;
        };

        let mut pages = registry
            .list()
            .await
            .expect("the browser starts and lists")
            .iter()
            .filter(|target| target.kind == "page")
            .count();
        while pages < MAX_TABS {
            registry
                .open(&data_url(&format!("SUMMRISE_TAB_{pages}")))
                .await
                .unwrap_or_else(|error| panic!("tab {pages}: {error}"));
            pages += 1;
        }

        let error = registry
            .open(&data_url("SUMMRISE_TAB_ONE_TOO_MANY"))
            .await
            .expect_err("one tab over the cap is refused");
        assert!(
            matches!(error, BrowserError::TooManyTabs { max: MAX_TABS }),
            "{error:?}"
        );
        let pages = registry
            .list()
            .await
            .expect("the list reads")
            .iter()
            .filter(|target| target.kind == "page")
            .count();
        assert_eq!(pages, MAX_TABS, "a refused open creates no tab");

        registry.shutdown().await;
    }

    #[tokio::test]
    async fn fragment_navigations_return_without_waiting_for_a_load_event() {
        let _gate = launch_gate().await;
        let Some((_dir, _data, registry)) = fixture("browser-fragment") else {
            return;
        };
        let base = data_url("SUMMRISE_FRAGMENT");
        let target = registry.open(&base).await.expect("the document opens");

        for hash in ["#one", "#two"] {
            let started = Instant::now();
            registry
                .navigate(&target, &format!("{base}{hash}"))
                .await
                .unwrap_or_else(|error| panic!("fragment navigation {hash}: {error}"));
            let elapsed = started.elapsed();
            assert!(
                elapsed < Duration::from_secs(5),
                "a same-document navigation must not wait for a load event: {hash} took {elapsed:?}"
            );
        }

        let hash = registry
            .eval(&target, "location.hash")
            .await
            .expect("the current hash evaluates");
        assert_eq!(hash.value, Some(json!("#two")));

        registry.shutdown().await;
    }

    #[tokio::test]
    async fn a_launch_after_a_leaked_browser_uses_a_fresh_profile() {
        let _gate = launch_gate().await;
        let Some(chrome) = chrome_or_skip() else {
            return;
        };
        let dir = TempDir::new("browser-fresh-profile");
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data directory is creatable");
        let profiles = data.join("chrome-profile");

        // First launch, with a tab that a second browser must not inherit.
        let first = BrowserRegistry::with_chrome(&data, chrome.clone());
        let leaked_tab = first
            .open(&data_url("SUMMRISE_LEAKED_TAB"))
            .await
            .expect("the first browser opens a tab");
        let mut leaked_ids: Vec<String> = first
            .list()
            .await
            .expect("the first browser lists")
            .into_iter()
            .map(|target| target.id)
            .collect();
        leaked_ids.push(leaked_tab);

        // Take the browser out of the registry and never drop it: this is the device process that
        // was killed while Chrome kept running and kept its profile's lock.
        let browser = first
            .browser
            .lock()
            .await
            .take()
            .expect("the first browser is running");
        let leaked_pid = browser.child.id();
        std::mem::forget(browser);

        // A failed assertion must not leave the leaked browser running: its own Drop was what was
        // suppressed, so this guard owns the group-kill instead.
        struct LeakedGroup(Option<u32>);
        impl Drop for LeakedGroup {
            fn drop(&mut self) {
                if let Some(pid) = self.0 {
                    let _ = killpg(Pid::from_raw(pid as i32), Signal::SIGKILL);
                }
            }
        }
        let mut leaked = LeakedGroup(Some(leaked_pid));

        // Every launch gets its own profile, so the locked old one cannot wedge this one.
        let second = BrowserRegistry::with_chrome(&data, chrome);
        let targets = second
            .list()
            .await
            .expect("a launch succeeds while the leaked browser still runs");
        assert_eq!(
            targets
                .iter()
                .filter(|target| target.kind == "page")
                .count(),
            1,
            "the second browser has its own single page: {targets:?}"
        );
        assert!(
            !targets.iter().any(|target| leaked_ids.contains(&target.id)),
            "the leaked browser's tabs are not the second browser's: {targets:?}"
        );
        second.shutdown().await;

        // The leaked Chrome is this test's own child; close its group so nothing is left running.
        killpg(Pid::from_raw(leaked_pid as i32), Signal::SIGKILL)
            .expect("the leaked group is killed");
        leaked.0 = None;
        assert_eq!(
            leftover_chrome(&profiles).await,
            Vec::<u32>::new(),
            "the test leaves no chrome behind"
        );
    }

    #[tokio::test]
    async fn dropping_the_registry_kills_chrome_and_nothing_launches_before_a_call() {
        let _gate = launch_gate().await;
        let Some(chrome) = chrome_or_skip() else {
            return;
        };
        let dir = TempDir::new("browser-drop");
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data directory is creatable");
        let profile = data.join("chrome-profile");

        {
            let registry = BrowserRegistry::with_chrome(&data, chrome.clone());
            assert_eq!(
                processes_with_profile(&profile),
                Vec::<u32>::new(),
                "a registry launches nothing until a tool call needs a browser"
            );
            registry.list().await.expect("the browser starts");
        }

        assert_eq!(
            leftover_chrome(&profile).await,
            Vec::<u32>::new(),
            "dropping the registry kills the browser and its children"
        );
    }

    #[tokio::test]
    async fn open_returns_after_the_first_load() {
        let _gate = launch_gate().await;
        let Some((_dir, _data, registry)) = fixture("browser-open-loaded") else {
            return;
        };

        let started = Instant::now();
        let target = registry
            .open(&data_url(FIRST_TITLE))
            .await
            .expect("a data URL opens");
        let elapsed = started.elapsed();
        let title = registry
            .eval(&target, "document.title")
            .await
            .expect("the title evaluates immediately after open");
        assert_eq!(
            title.value,
            Some(json!(FIRST_TITLE)),
            "open returns only after the new document's title is available"
        );
        let state = registry
            .eval(&target, "document.readyState")
            .await
            .expect("readyState evaluates immediately after open");
        assert!(
            matches!(
                state.value.as_ref().and_then(Value::as_str),
                Some("interactive" | "complete")
            ),
            "open waits for the document to finish parsing: {state:?}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "open must stay well under the command timeout: {elapsed:?}"
        );

        let started = Instant::now();
        let blank = registry
            .open("about:blank")
            .await
            .expect("about:blank opens");
        let href = registry
            .eval(&blank, "location.href")
            .await
            .expect("the href evaluates immediately after open");
        assert_eq!(href.value, Some(json!("about:blank")));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "open of about:blank must stay well under the command timeout"
        );

        registry.shutdown().await;
    }

    #[tokio::test]
    async fn a_url_that_does_not_load_is_a_typed_refusal_and_leaves_no_tab() {
        let _gate = launch_gate().await;
        let Some((_dir, _data, registry)) = fixture("browser-open-failed") else {
            return;
        };
        registry.list().await.expect("the browser starts");
        let pages = page_count(&registry).await;

        for attempt in 0..2 {
            let started = Instant::now();
            let error = registry
                .open("http://127.0.0.1:1/")
                .await
                .expect_err("a URL that never loads is refused");
            assert!(
                matches!(error, BrowserError::Navigation { .. }),
                "attempt {attempt}: {error:?}"
            );
            assert!(
                error.to_string().contains("http://127.0.0.1:1/"),
                "attempt {attempt}: the refusal names the URL: {error}"
            );
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "attempt {attempt}: a refused load is reported within the bound: {:?}",
                started.elapsed()
            );
            assert_eq!(
                pages_settle(&registry, pages).await,
                pages,
                "attempt {attempt}: a failed open closes its tab, so the count does not grow"
            );
        }

        registry.shutdown().await;
    }

    /// Serve one page whose parser blocks on a slow script, with the title after that script.
    ///
    /// The title is deliberately after the script: at commit the parser has not reached it, so a
    /// tool that returns at the commit gives the caller an empty title. This is the state the
    /// Windows lane saw; the 600 ms script keeps it observable for long enough on any host.
    async fn slow_parsing_page() -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the page server binds");
        let port = listener
            .local_addr()
            .expect("the page server is bound")
            .port();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buffer = [0u8; 4096];
                    let read = socket.read(&mut buffer).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..read]);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let (body, content_type) = if path == "/slow.js" {
                        tokio::time::sleep(Duration::from_millis(600)).await;
                        ("/* slow */", "application/javascript")
                    } else {
                        (
                            "<script src=\"/slow.js\"></script><title>SUMMRISE_CDP_SLOW</title>",
                            "text/html",
                        )
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        port
    }

    #[tokio::test]
    async fn open_waits_for_a_document_that_is_still_parsing() {
        let _gate = launch_gate().await;
        let Some((_dir, _data, registry)) = fixture("browser-open-parsing") else {
            return;
        };
        let port = slow_parsing_page().await;

        let started = Instant::now();
        let target = registry
            .open(&format!("http://127.0.0.1:{port}/page"))
            .await
            .expect("the page opens");
        let elapsed = started.elapsed();
        let title = registry
            .eval(&target, "document.title")
            .await
            .expect("the title evaluates immediately after open");
        assert_eq!(
            title.value,
            Some(json!("SUMMRISE_CDP_SLOW")),
            "open returns only after the parser has reached the title"
        );
        let state = registry
            .eval(&target, "document.readyState")
            .await
            .expect("readyState evaluates immediately after open");
        assert!(
            matches!(
                state.value.as_ref().and_then(Value::as_str),
                Some("interactive" | "complete")
            ),
            "open waits for the document to finish parsing: {state:?}"
        );
        assert!(
            elapsed >= Duration::from_millis(500),
            "open actually waited for the slow script: {elapsed:?}"
        );

        registry.shutdown().await;
    }

    /// The argument the fake-chrome probe passes; the generated script exits before its body.
    const FAKE_CHROME_PROBE: &str = "--summrise-probe";

    /// An executable shell script standing in for Chrome.
    fn fake_chrome(dir: &TempDir, body: &str) -> PathBuf {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join("fake-chrome");
        // The generated script exits before its body when probed, so the helper can exec it here
        // without running the scenario (writing the counter, sleeping, exiting 7, ...).
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"{FAKE_CHROME_PROBE}\" ]; then exit 0; fi\n{body}\n"
        );
        {
            let mut file = fs::File::create(&path).expect("the fake chrome is creatable");
            file.write_all(script.as_bytes())
                .expect("the fake chrome is written");
            file.flush().expect("the fake chrome is flushed");
            file.sync_all().expect("the fake chrome reaches the disk");
            // The handle is dropped here, before anything execs the file. A write handle still
            // open anywhere is what Linux reports as ETXTBSY at exec time.
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("the fake chrome is executable");

        // Even with the handle closed, a loaded runner can still report ETXTBSY (a forked child
        // may have inherited a write handle). Probe the program until the kernel accepts it; the
        // probe argument makes the script exit without its body. A genuinely broken fake (no
        // interpreter, no execute bit) fails here with its own error.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match Command::new(&path).arg(FAKE_CHROME_PROBE).status() {
                Ok(_) => break,
                Err(error) if executable_busy(&error) && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => panic!("the fake chrome cannot be executed: {error}"),
            }
        }
        path
    }

    /// Whether an exec failed because the file still had a write handle (Linux ETXTBSY).
    fn executable_busy(error: &std::io::Error) -> bool {
        error.kind() == std::io::ErrorKind::ExecutableFileBusy || error.raw_os_error() == Some(26)
    }

    #[test]
    fn the_fake_chrome_probe_runs_nothing() {
        let dir = TempDir::new("browser-fake-chrome");
        let marker = dir.join("marker");
        let chrome = fake_chrome(&dir, &format!("echo ran > \"{}\"", marker.display()));
        let status = Command::new(&chrome)
            .arg(FAKE_CHROME_PROBE)
            .status()
            .expect("the fake chrome can be probed");
        assert!(status.success(), "the probe exits 0");
        assert!(
            !marker.exists(),
            "the probe argument must not run the fake chrome's body"
        );
    }

    #[tokio::test]
    async fn a_launch_failure_reports_the_exit_status_and_stderr_tail() {
        let dir = TempDir::new("browser-launch-exit");
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data directory is creatable");
        let chrome = fake_chrome(&dir, "echo SUMMRISE_EXIT_MARKER >&2\nexit 7");
        let registry = BrowserRegistry::with_chrome(&data, chrome);

        let error = registry
            .list()
            .await
            .expect_err("the fake browser exits before opening a port");
        let message = error.to_string();
        assert!(
            message.contains("exit status: 7"),
            "the exit status is named: {message}"
        );
        assert!(
            message.contains("SUMMRISE_EXIT_MARKER"),
            "the stderr tail is quoted: {message}"
        );
    }

    #[tokio::test]
    async fn a_bind_failure_is_retried_and_stops_after_three_attempts() {
        let dir = TempDir::new("browser-launch-retry");
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data directory is creatable");
        let counter = dir.join("attempts");
        let chrome = fake_chrome(
            &dir,
            &format!(
                "echo 'bind() failed: Address already in use' >&2\necho attempt >> \"{}\"\nexit 1",
                counter.display()
            ),
        );
        let registry = BrowserRegistry::with_chrome(&data, chrome);

        let error = registry
            .list()
            .await
            .expect_err("a port that cannot be bound is a launch failure");
        let message = error.to_string();
        let attempts = fs::read_to_string(&counter)
            .unwrap_or_default()
            .lines()
            .count();
        assert_eq!(
            attempts, 3,
            "a bind failure is retried up to three attempts: {message}"
        );
        assert!(message.contains("exit status: 1"), "{message}");
        assert!(message.contains("bind() failed"), "{message}");
    }

    #[test]
    fn the_retry_decision_retries_only_a_bind_failure() {
        assert_eq!(
            next_action(LaunchOutcome::BindFailure, 1),
            LaunchAction::Retry(2)
        );
        assert_eq!(
            next_action(LaunchOutcome::BindFailure, 2),
            LaunchAction::Retry(3)
        );
        assert_eq!(
            next_action(LaunchOutcome::BindFailure, MAX_LAUNCH_ATTEMPTS),
            LaunchAction::Fail,
            "the third attempt is the last"
        );
        for outcome in [
            LaunchOutcome::Timeout,
            LaunchOutcome::Exited,
            LaunchOutcome::Malformed,
            LaunchOutcome::Other,
        ] {
            assert_eq!(
                next_action(outcome, 1),
                LaunchAction::Fail,
                "{outcome:?} is not worth another port"
            );
        }
    }

    #[test]
    fn a_bind_failure_is_recognised_from_chrome_stderr() {
        for stderr in [
            "[1:2:ERROR:socket_win.cc(56)] bind() failed: Only one usage of each socket address \
             (protocol/network address/port) is normally permitted. (10048)",
            "socket_posix.cc: bind() failed: Address already in use (98)",
            "Cannot start http server for devtools.",
        ] {
            assert!(looks_like_bind_failure(stderr), "{stderr}");
        }
        assert!(!looks_like_bind_failure(
            "GPU process isn't usable. Goodbye."
        ));
    }

    #[tokio::test]
    async fn a_launch_probe_timeout_names_the_port_and_the_live_child() {
        let dir = TempDir::new("browser-probe-timeout");
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data directory is creatable");
        let chrome = fake_chrome(&dir, "echo SUMMRISE_PROBE_MARKER >&2\nsleep 30");
        let registry = BrowserRegistry::with_chrome_timeout(&data, chrome, 1_000);

        let started = Instant::now();
        let error = registry
            .list()
            .await
            .expect_err("a browser that never answers is a launch failure");
        let elapsed = started.elapsed();
        let message = error.to_string();

        assert!(
            elapsed < Duration::from_secs(5),
            "the configurable readiness bound is used: {elapsed:?}"
        );
        assert!(
            message.contains("chrome is still running"),
            "the timeout says the child is alive: {message}"
        );
        assert!(
            message.contains("SUMMRISE_PROBE_MARKER"),
            "the timeout quotes the stderr tail: {message}"
        );
        let port = message.split("127.0.0.1:").nth(1).and_then(|rest| {
            rest.split(|character: char| !character.is_ascii_digit())
                .next()
                .and_then(|digits| digits.parse::<u16>().ok())
        });
        assert!(
            port.is_some_and(|port| port > 1024),
            "the chosen port is named in the failure: {message}"
        );
    }

    #[tokio::test]
    async fn a_launch_sweeps_dead_profiles_and_keeps_a_live_one() {
        let _gate = launch_gate().await;
        use std::os::unix::fs::symlink;

        let Some(chrome) = chrome_or_skip() else {
            return;
        };
        let dir = TempDir::new("browser-sweep");
        let data = dir.join("data");
        let profiles = data.join("chrome-profile");
        fs::create_dir_all(&profiles).expect("the profile root is creatable");

        // A lock naming a pid that cannot exist: the device was killed and this profile is dead.
        let stale = profiles.join("run-stale");
        fs::create_dir_all(&stale).expect("the stale profile is creatable");
        symlink("host-999999999", stale.join("SingletonLock"))
            .expect("the stale lock is creatable");

        // No lock, but a day old: a failed launch's leftover.
        let old = profiles.join("run-old");
        fs::create_dir_all(&old).expect("the old profile is creatable");
        fs::File::open(&old)
            .expect("the old profile opens")
            .set_modified(SystemTime::now() - Duration::from_secs(25 * 60 * 60))
            .expect("the old profile's mtime is settable");

        // A lock naming this live test process: never swept, whoever owns it.
        let live = profiles.join("run-live");
        fs::create_dir_all(&live).expect("the live profile is creatable");
        symlink(
            format!("host-{}", std::process::id()),
            live.join("SingletonLock"),
        )
        .expect("the live lock is creatable");

        let registry = BrowserRegistry::with_chrome(&data, chrome);
        registry.list().await.expect("the browser starts");

        assert!(!stale.exists(), "a stale lock is swept");
        assert!(!old.exists(), "a profile older than a day is swept");
        assert!(
            live.exists(),
            "a profile whose lock names a live process is kept"
        );
        let directories: Vec<String> = fs::read_dir(&profiles)
            .expect("the profile root reads")
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            directories.len(),
            2,
            "only the live profile and this launch's remain: {directories:?}"
        );

        registry.shutdown().await;
        assert!(
            live.exists(),
            "shutdown touches only the profile it created"
        );
        assert_eq!(
            fs::read_dir(&profiles)
                .expect("the profile root reads")
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .count(),
            1,
            "this launch's profile is gone and the live one is untouched"
        );
    }

    #[tokio::test]
    async fn a_missing_chrome_binary_is_a_typed_refusal() {
        let dir = TempDir::new("browser-missing-chrome");
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data directory is creatable");
        let registry =
            BrowserRegistry::with_chrome(&data, PathBuf::from("/nonexistent/summrise-chrome"));

        let error = registry
            .list()
            .await
            .expect_err("there is no browser to start");

        assert!(matches!(error, BrowserError::Spawn { .. }), "{error:?}");
        assert!(
            error.to_string().contains("cannot start"),
            "the refusal names the failed start: {error}"
        );
    }
}
