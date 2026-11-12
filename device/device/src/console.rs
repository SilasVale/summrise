//! The console client: the device's one outbound WebSocket to the operator's console.
//!
//! The design's operator path dials out (design §5.2): the device opens one channel to
//! `<console>/devices/<id>/channel`, the console authenticates it with the token registration
//! issued, and every frame on that socket is one MCP exchange plus the grant that authorizes it.
//! There is no inbound port, and the console is optional: without `--console-url` and
//! `--console-token-file` this module is never started and the device behaves exactly as before.
//!
//! A frame is `{"id","mcp":{...},"grant":{"grant":{...},"signature":"..."}}`; the device answers
//! `{"id","result":{...}}` or `{"id","error":{...}}`. The id correlates the answer with the
//! console's waiter, so an answer is never attached to a different frame.
//!
//! The grant is re-verified here, offline, against the console key compiled into the device: the
//! console already verified it when the operator's request arrived, but a frame is only as
//! trustworthy as the endpoint that delivered it, and the device is the last party that can check
//! the bytes for itself. The check is [`crate::remote::admit`] — the same function a mesh channel
//! calls, so the signature, the device, the expiry and the tool scope are one implementation on
//! both remote transports. Only then does the request reach the same `authorize` path the loopback
//! MCP surface uses.
//!
//! The connection is one at a time and reconnects for as long as the process runs: an exponential
//! backoff starts at one second and caps at sixty. The console replaces a device's channel when a
//! new one arrives, so a reconnect after a network change does not need a console-side cleanup.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ed25519_dalek::VerifyingKey;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::ClientRequestBuilder;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use crate::config::{Config, CONSOLE_TOKEN_FLAG, CONSOLE_URL_FLAG};
use crate::mcp::{Arrival, Caller, DeviceServer};
use crate::remote::{self, RemoteFrame};

/// How long one connect attempt may take before it is a failure to retry.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The placeholder that ships when no console grant key was configured at build time.
///
/// All-zero is not a point anybody signs with: [`parse_console_key`] refuses it by name, so a build
/// that pins no console key accepts no console frame. This is the same compile-time seam as the
/// release key: a key the running service could replace would make the signature optional.
pub(crate) const PLACEHOLDER_CONSOLE_GRANT_KEY_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// The console grant public key, embedded at build time from `SUMMRISE_CONSOLE_GRANT_KEY`.
pub(crate) const CONSOLE_GRANT_KEY_HEX: &str = match option_env!("SUMMRISE_CONSOLE_GRANT_KEY") {
    Some(key) => key,
    None => PLACEHOLDER_CONSOLE_GRANT_KEY_HEX,
};

/// Why the console link could not be started.
#[derive(Debug)]
pub(crate) enum ConsoleError {
    /// One of the two console flags was given without the other.
    IncompleteConfig,
    /// The token file could not be read.
    Unreadable {
        /// The path as given.
        path: PathBuf,
        /// The OS error.
        source: io::Error,
    },
    /// The token file is not a registration response or `<id> <token>`.
    MalformedEnrollment {
        /// The path as given.
        path: PathBuf,
        /// What the file must contain.
        expected: &'static str,
    },
    /// The URL is not an absolute `ws`, `wss`, `http` or `https` URL.
    BadUrl(String),
    /// A plaintext `ws://` channel to a host that is not this machine, without the operator's
    /// explicit `allow_plaintext` in `console.json`.
    PlaintextChannel {
        /// The host the URL named.
        host: String,
    },
    /// Windows read the token file's DACL and it allows more than SYSTEM and Administrators.
    #[cfg(windows)]
    Insecure {
        /// The token file as given.
        path: PathBuf,
        /// The read-back's refusal.
        detail: String,
    },
}

impl std::fmt::Display for ConsoleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsoleError::IncompleteConfig => write!(
                f,
                "{CONSOLE_URL_FLAG} and {CONSOLE_TOKEN_FLAG} must be given together"
            ),
            ConsoleError::Unreadable { path, source } => {
                write!(
                    f,
                    "{CONSOLE_TOKEN_FLAG}: cannot read {}: {source}",
                    path.display()
                )
            }
            ConsoleError::MalformedEnrollment { path, expected } => write!(
                f,
                "{CONSOLE_TOKEN_FLAG}: {} must contain {expected}",
                path.display()
            ),
            ConsoleError::BadUrl(url) => {
                write!(
                    f,
                    "{CONSOLE_URL_FLAG}: '{url}' is not a ws:// or wss:// URL"
                )
            }
            ConsoleError::PlaintextChannel { host } => write!(
                f,
                "{CONSOLE_URL_FLAG}: refusing a plaintext ws:// channel to '{host}': the \
                 registration token would cross the network in the clear. Use https:// (wss), or \
                 set \"allow_plaintext\": true in {} for a console on a trusted network.",
                crate::config::CONSOLE_FILE
            ),
            #[cfg(windows)]
            ConsoleError::Insecure { path, detail } => write!(
                f,
                "{CONSOLE_TOKEN_FLAG}: refusing {}: {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ConsoleError {}

/// Why the pinned console key cannot be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyError {
    /// The build carries the all-zero placeholder, so no console frame can be trusted.
    Placeholder,
    /// The configured key is not a valid Ed25519 public key in hex.
    BadKey,
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::Placeholder => write!(
                f,
                "the device was built with the placeholder console grant key (set \
                 SUMMRISE_CONSOLE_GRANT_KEY at build time to trust console frames)"
            ),
            KeyError::BadKey => write!(
                f,
                "the console grant key is not 32 bytes of Ed25519 public key hex"
            ),
        }
    }
}

impl std::error::Error for KeyError {}

/// Everything one console connection needs, loaded once at startup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConsoleEndpoint {
    /// The full socket URL, including `/devices/<id>/channel`.
    pub(crate) channel_url: String,
    /// This device's id: the lowercase hex of its registered Ed25519 public key.
    pub(crate) device_id: String,
    /// The registration token, as lowercase hex.
    pub(crate) token: String,
}

impl ConsoleEndpoint {
    /// Load the endpoint from the device's configuration, or `None` when the link is off.
    pub(crate) fn from_config(config: &Config) -> Result<Option<ConsoleEndpoint>, ConsoleError> {
        let (Some(url), Some(token_file)) = (&config.console_url, &config.console_token_file)
        else {
            return match (&config.console_url, &config.console_token_file) {
                (None, None) => Ok(None),
                _ => Err(ConsoleError::IncompleteConfig),
            };
        };

        // Windows: the registration token is a channel credential. A copy that inherited a looser
        // DACL (a restored backup, a hand copy, a file from a pre-protection install) is refused by
        // the same read-back the writers run, not used because it parses.
        #[cfg(windows)]
        summrise_winsec::verify(token_file).map_err(|detail| ConsoleError::Insecure {
            path: token_file.clone(),
            detail,
        })?;
        let text = fs::read_to_string(token_file).map_err(|source| ConsoleError::Unreadable {
            path: token_file.clone(),
            source,
        })?;
        let (device_id, token) =
            parse_enrollment(&text).ok_or_else(|| ConsoleError::MalformedEnrollment {
                path: token_file.clone(),
                expected: "the registration response ({\"id\",\"token\"}) or `<id> <token>`",
            })?;
        // The release the launcher started us with, not the crate version: a rebuilt binary keeps
        // CARGO_PKG_VERSION at 0.1.0, while the operator knows this machine by the release the
        // launcher chose.
        let version = config
            .release_version
            .clone()
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned());
        let channel_url = channel_url(
            url,
            &device_id,
            &version,
            None,
            config.console_allow_plaintext,
        )?;
        Ok(Some(ConsoleEndpoint {
            channel_url,
            device_id,
            token,
        }))
    }
}

/// The host part of a `host[:port]/path` remainder: what a certificate or a refusal names.
fn host_of(rest: &str) -> &str {
    let authority = rest.split('/').next().unwrap_or(rest);
    // A bracketed IPv6 literal keeps its brackets; strip the port after them.
    if authority.starts_with('[') {
        return authority
            .split_once(']')
            .map(|(host, _)| &authority[..=host.len()])
            .unwrap_or(authority);
    }
    authority.split(':').next().unwrap_or(authority)
}

/// Whether `host` is this machine or a link that is already private: loopback, a private network,
/// a link-local address, the CGNAT range a tailnet uses, or an IPv6 unique-local address.
///
/// Those are the links an operator runs without TLS on purpose — a WireGuard tailnet, a lab LAN —
/// and the token crossing them is not the leak the rule exists for. Everything else (a public
/// address or a host name) needs `"allow_plaintext": true`, because there the wire is the internet.
fn is_loopback_host(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if matches!(bare.to_ascii_lowercase().as_str(), "localhost" | "::1") {
        return true;
    }
    let Ok(address) = bare.parse::<std::net::IpAddr>() else {
        return false;
    };
    match address {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() ||
            // 100.64.0.0/10: the range Tailscale and other CGNAT overlays hand out.
            (v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]))
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local()
        }
    }
}

/// Parse the file registration produced.
///
/// The console's `POST /devices` answer is `{"id","public_key",...,"token"}`; a plain
/// `<id> <token>` (or `<id>:<token>`) is accepted too, so an operator who copied the two values by
/// hand is not forced to write JSON. Both values are 64 hex characters and are canonicalised to
/// lowercase, the form the console registry uses.
fn parse_enrollment(text: &str) -> Option<(String, String)> {
    #[derive(Deserialize)]
    struct Enrollment {
        id: String,
        token: String,
    }

    let trimmed = text.trim();
    let (id, token) = if trimmed.starts_with('{') {
        let enrollment: Enrollment = serde_json::from_str(trimmed).ok()?;
        (enrollment.id, enrollment.token)
    } else {
        let mut parts = trimmed
            .split(|character: char| character.is_whitespace() || character == ':')
            .filter(|part| !part.is_empty());
        (parts.next()?.to_owned(), parts.next()?.to_owned())
    };

    let id = id.trim().to_ascii_lowercase();
    let token = token.trim().to_ascii_lowercase();
    if !is_hex_64(&id) || !is_hex_64(&token) {
        return None;
    }
    Some((id, token))
}

fn is_hex_64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Build the channel URL from the console base and the device id.
fn channel_url(
    base: &str,
    device_id: &str,
    version: &str,
    retry: Option<&RetryNote>,
    allow_plaintext: bool,
) -> Result<String, ConsoleError> {
    let base = base.trim().trim_end_matches('/');
    let (scheme, rest) = base
        .split_once("://")
        .ok_or_else(|| ConsoleError::BadUrl(base.to_owned()))?;
    let scheme = match scheme.to_ascii_lowercase().as_str() {
        "ws" | "http" => "ws",
        "wss" | "https" => "wss",
        _ => return Err(ConsoleError::BadUrl(base.to_owned())),
    };
    if scheme == "ws" && !allow_plaintext {
        let host = host_of(rest);
        if !is_loopback_host(host) {
            return Err(ConsoleError::PlaintextChannel {
                host: host.to_owned(),
            });
        }
    }
    if rest.is_empty()
        || rest.contains(char::is_whitespace)
        || rest.contains('?')
        || rest.contains('#')
    {
        return Err(ConsoleError::BadUrl(base.to_owned()));
    }
    // `?v=` and `?n=` tell the console which build and which machine are on the other end, so the
    // operator's device list can name them without asking the device for a grant first. A console that
    // ignores the parameters answers exactly as before: they are a query string on a route both sides
    // already share.
    let name = host_name()
        .map(|name| format!("&n={}", percent_encode(&name)))
        .unwrap_or_default();
    // `&a=` and `&e=` say how the PREVIOUS attempt ended. A device cannot report its failures while it
    // is failing — the connection that would carry the report is the one that is down — so it tells the
    // console the story the next time it gets through: "this is what it looked like before I arrived".
    let retry = retry
        .map(|note| {
            format!(
                "&a={}&e={}",
                note.attempts,
                percent_encode(&truncate_reason(&note.reason))
            )
        })
        .unwrap_or_default();
    Ok(format!(
        "{scheme}://{rest}/devices/{device_id}/channel?v={}{name}{retry}",
        percent_encode(version)
    ))
}

/// How the attempt before this one ended.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct RetryNote {
    /// How many attempts had failed when this one started (1 means the previous one was the first).
    pub attempts: u8,
    /// The last failure in a sentence, as the device would print it.
    pub reason: String,
}

/// The note the link is currently carrying, for the device's own panel.
///
/// One process, one console link, so a static is the honest shape: plumbing an `Arc` through the MCP
/// service to the HTTP layer would be three signatures to say "there is one of these".
static LAST_ATTEMPT: std::sync::OnceLock<std::sync::Mutex<Option<RetryNote>>> =
    std::sync::OnceLock::new();

/// How the previous attempt ended, as the panel shows it.
pub fn last_attempt() -> Option<RetryNote> {
    LAST_ATTEMPT
        .get()
        .and_then(|cell| cell.lock().ok().and_then(|guard| guard.clone()))
}

fn record_attempt(note: &Option<RetryNote>) {
    let cell = LAST_ATTEMPT.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(mut guard) = cell.lock() {
        *guard = note.clone();
    }
}

/// A reason travels in a query string and is displayed to an operator: one line, bounded, no control
/// characters.
fn truncate_reason(reason: &str) -> String {
    let cleaned: String = reason
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.len() <= 160 {
        cleaned
    } else {
        format!("{}…", &cleaned[..160])
    }
}

/// The machine's own name, as the operator would recognise it in a list.
///
/// Windows names a machine with `COMPUTERNAME` (`desktop-9vh92bj`); the variable may also be unset in
/// a service that inherited nothing, in which case the console shows no name rather than a guess.
fn host_name() -> Option<String> {
    for key in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

/// Percent-encode the characters a query string cannot carry, so a name with a space or `&` survives.
fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

/// Parse the compile-time console key, refusing the placeholder by name.
fn parse_console_key(text: Option<&str>) -> Result<VerifyingKey, KeyError> {
    let text = text.ok_or(KeyError::Placeholder)?;
    let raw = decode_hex_32(text).ok_or(KeyError::BadKey)?;
    if raw == [0u8; 32] {
        return Err(KeyError::Placeholder);
    }
    VerifyingKey::from_bytes(&raw).map_err(|_| KeyError::BadKey)
}

fn decode_hex_32(text: &str) -> Option<[u8; 32]> {
    let digits = text.trim().as_bytes();
    if digits.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (byte, pair) in out.iter_mut().zip(digits.as_chunks::<2>().0) {
        *byte = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(out)
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// The compile-time key, or the refusal a placeholder build ships with.
pub(crate) fn pinned_console_key() -> Result<VerifyingKey, KeyError> {
    parse_console_key(Some(CONSOLE_GRANT_KEY_HEX))
}

/// Start the console link, or do nothing (and say why) when the build pins no usable key.
pub(crate) fn spawn(endpoint: ConsoleEndpoint, server: DeviceServer) -> JoinHandle<()> {
    // The channel is authenticated by this device's registration token. A pinned key only adds a
    // signature over console frames; a build without one must keep the channel (a permission limit
    // that can strand a machine is worse than an unverified frame on a channel we already trust).
    let key = match pinned_console_key() {
        Ok(key) => Some(key),
        Err(error) => {
            eprintln!("summrise-device: console frames are unsigned: {error}");
            None
        }
    };
    eprintln!(
        "summrise-device: opening the console channel at {}",
        endpoint.channel_url
    );
    spawn_with_key(endpoint, server, key)
}

/// The testable half of [`spawn`]: one key, one endpoint, one handler.
pub(crate) fn spawn_with_key(
    endpoint: ConsoleEndpoint,
    server: DeviceServer,
    key: Option<VerifyingKey>,
) -> JoinHandle<()> {
    tokio::spawn(async move { run(endpoint, server, key).await })
}

/// Connect forever: one socket at a time, with a 1 s → 60 s backoff between attempts.
async fn run(endpoint: ConsoleEndpoint, server: DeviceServer, key: Option<VerifyingKey>) {
    let mut attempt: u8 = 1;
    let mut note: Option<RetryNote> = None;
    loop {
        record_attempt(&note);
        match connect(&endpoint, note.as_ref()).await {
            Ok(socket) => {
                // The channel coming back is the machine's wake signal: while it was away the
                // process kept running, so no start fired and the schedule may have fired into
                // sleep. The checker decides whether this connection is a wake (first after a
                // start, or more than five minutes away) and its own guard decides whether to
                // check; the check is a task so a slow mirror never delays the channel.
                if let Some(check) = server.update_checker() {
                    drop(check.clone().note_channel_connected());
                }
                // A connection that opened is a success, but a connection that never served a
                // frame is not: a console that accepts and immediately drops must back off like
                // any other failure instead of being retried once a second forever.
                if serve(socket, &endpoint, &server, key.as_ref()).await {
                    attempt = 1;
                    note = None;
                } else {
                    note = Some(RetryNote {
                        attempts: attempt,
                        reason: "the console accepted the channel and then dropped it".to_owned(),
                    });
                }
            }
            Err(ConnectError::Refused(status)) => {
                // A refusal is the console's answer, not a network problem: the token this device
                // registered with is gone (the operator revoked it). Retrying cannot fix that, and a
                // loop that retried for ever would bury the one sentence the operator needs.
                eprintln!(
                    "summrise-device: the console refused this device (HTTP {status}); it will not retry. \
                     Re-enroll with `summrise-device --enroll <console-url> <code> --force`.",
                );
                return;
            }
            Err(ConnectError::Other(error)) => {
                eprintln!(
                    "summrise-device: cannot reach the console at {}: {error}",
                    endpoint.channel_url
                );
                note = Some(RetryNote {
                    attempts: attempt,
                    reason: error,
                });
            }
        }

        let delay = summrise_core::mesh::backoff(attempt);
        tokio::time::sleep(Duration::from_millis(delay)).await;
        attempt = attempt.saturating_add(1);
    }
}

/// One authenticated WebSocket.
/// Why a channel could not be opened.
enum ConnectError {
    /// The console answered with an authentication refusal: retrying cannot help.
    Refused(u16),
    /// Anything else — a network fault, a handshake timeout, a proxy in the way.
    Other(String),
}

/// The status of an authentication refusal, if this error is one.
fn refusal_status(error: &tokio_tungstenite::tungstenite::Error) -> Option<u16> {
    match error {
        tokio_tungstenite::tungstenite::Error::Http(response)
            if matches!(response.status().as_u16(), 401 | 403) =>
        {
            Some(response.status().as_u16())
        }
        _ => None,
    }
}

async fn connect(
    endpoint: &ConsoleEndpoint,
    retry: Option<&RetryNote>,
) -> Result<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, ConnectError> {
    // The retry note is appended here rather than baked into the endpoint: it changes on every attempt
    // and the endpoint does not.
    let mut url = endpoint.channel_url.clone();
    if let Some(note) = retry {
        // The note is a parameter like any other. The separator is computed rather than assumed: an
        // endpoint built by hand (the tests build one) may carry no query string at all.
        let separator = if url.contains('?') { '&' } else { '?' };
        url.push_str(&format!(
            "{separator}a={}&e={}",
            note.attempts,
            percent_encode(&truncate_reason(&note.reason))
        ));
    }
    let uri = url
        .parse()
        .map_err(|error| ConnectError::Other(format!("the channel URL is not a URI: {error}")))?;
    let request = ClientRequestBuilder::new(uri)
        .with_header("Authorization", format!("Bearer {}", endpoint.token));
    let connect = connect_async(request);
    match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Err(_) => Err(ConnectError::Other(format!(
            "the handshake did not finish inside {} ms",
            CONNECT_TIMEOUT.as_millis()
        ))),
        Ok(Err(error)) => match refusal_status(&error) {
            Some(status) => Err(ConnectError::Refused(status)),
            None => Err(ConnectError::Other(error.to_string())),
        },
        Ok(Ok((socket, _response))) => Ok(socket),
    }
}

/// The most frames the channel runs at once.
///
/// One machine, one console: the bound keeps a console that pipelines requests from spawning an
/// unbounded number of blocking commands, while still letting a slow tool be one call rather than
/// the whole channel. Measured on 2026-10-27 (14rjcr8): one `runs.exec` — an `scp` — ran 420 s, and
/// every frame behind it waited; the console recorded 504 for `host.facts`, `terminal.list` and the
/// run itself. A serial loop turned one slow command into an unanswerable device.
const MAX_IN_FLIGHT_FRAMES: usize = 16;

/// How long a frame waits for one of the [`MAX_IN_FLIGHT_FRAMES`] permits before the device says
/// it is busy.
///
/// A request that cannot get a permit is not a request the device failed: the channel is running
/// every call it agreed to run. Parking it for as long as a slow tool takes would make a saturated
/// device indistinguishable from one that accepted the frame and forgot it — and the console's own
/// wait for an answer is 30 s, after which the operator reads a timeout with no reason. Five
/// seconds is longer than any handoff measured here (at most 2 of the 16 were ever in flight as of
/// 2026-11-06) and short enough that the retry the sentence asks for is still useful.
const IN_FLIGHT_WAIT: Duration = Duration::from_secs(5);

/// The JSON-RPC code a frame answers when no permit was free inside [`IN_FLIGHT_WAIT`].
///
/// It is not [`remote::GRANT_REFUSAL_CODE`]: the frame's grant may be perfectly good, the device
/// just could not run the call now. The console forwards this envelope to its client unchanged, so
/// the page reads [`DEVICE_BUSY_MESSAGE`] rather than a 30 s timeout.
const DEVICE_BUSY_CODE: i32 = -32003;

/// The sentence a client reads when the channel is at [`MAX_IN_FLIGHT_FRAMES`].
const DEVICE_BUSY_MESSAGE: &str =
    "the device is busy: too many requests are already running; retry shortly";

/// How many finished frames may wait for the socket before a task waits for the loop.
const OUTGOING_FRAMES: usize = 64;

/// How often a follow waiting for its frame to be written checks whether the console has fallen
/// out of the ring's window.
///
/// The wait is a loop of these checks rather than an unbounded await: a console that stopped
/// reading is turned into the one sentence its paged catch-up can recover from, instead of a task
/// parked for as long as the socket stays open.
const FOLLOW_GAP_CHECK: Duration = Duration::from_millis(250);

/// How many terminal subscriptions one channel may hold at once.
///
/// A subscription is a read parked on this device's ring for as long as a page is open, so the cap
/// is a page-lifetime bound rather than a per-request one; sixteen is more terminals than one
/// operator watches and far under the console's own stream cap.
const MAX_SUBSCRIPTIONS: usize = 16;

/// How long one follow read parks before the loop looks at its stop signal again.
const FOLLOW_WAIT: Duration = Duration::from_secs(2);

/// The most output one pushed frame carries.
///
/// The console hands the page a frame at a time anyway; keeping a frame small keeps the WebSocket
/// (and a relay in front of it) from carrying a megabyte burst as one message.
const FOLLOW_CHUNK: usize = 32 * 1024;

/// One frame on its way to the console.
///
/// **A FOLLOW PUSH IS RELEASED BY THE WRITER, NOT BY THE QUEUE.** A tool answer is done once it is
/// in the queue; a follow's push carries the sender that says the socket took it, and the follow
/// waits for that before producing its next chunk. That one-frame window is what bounds a lagging
/// follow's outbound queue: a console that stops reading can make the writer wait, but it cannot
/// make the follow stack megabytes of pushed frames in front of an answer.
enum Outbound {
    /// A tool answer, a subscribe ack, a refusal, or the sentence that stops a subscription.
    Answer(Value),
    /// One followed terminal frame, and the release the follow waits on.
    Push {
        value: Value,
        written: tokio::sync::oneshot::Sender<()>,
    },
}

impl Outbound {
    /// The frame the socket writes, and the release a follow waits on when it is a push.
    fn release(self) -> (Value, Option<tokio::sync::oneshot::Sender<()>>) {
        match self {
            Outbound::Answer(value) => (value, None),
            Outbound::Push { value, written } => (value, Some(written)),
        }
    }
}

/// Serve one socket until it closes, then return whether it carried any frame.
///
/// The socket is generic over the WebSocket stream so a test can drive this loop over an in-memory
/// pipe with a reader it controls. That is not cosmetic: a loopback socket buffers megabytes before
/// it pushes back (measured: 57 32 KiB frames accepted by an unread peer), so a test on a real
/// socket cannot make the writer wait the way a console that stops reading does.
async fn serve<S>(
    socket: S,
    endpoint: &ConsoleEndpoint,
    server: &DeviceServer,
    key: Option<&VerifyingKey>,
) -> bool
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error>
        + futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    let in_flight = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_IN_FLIGHT_FRAMES));
    serve_with_in_flight(socket, endpoint, server, key, in_flight).await
}

/// [`serve`] with its in-flight semaphore supplied.
///
/// The semaphore is a parameter so a test can take every permit before the loop opens and prove the
/// busy answer directly; in a real saturation the moment a permit frees is the running tools'
/// business, which is exactly the timing a bound's test must not depend on. Production calls
/// [`serve`], which builds a fresh semaphore per socket, so the serving loop itself is unchanged.
async fn serve_with_in_flight<S>(
    mut socket: S,
    endpoint: &ConsoleEndpoint,
    server: &DeviceServer,
    key: Option<&VerifyingKey>,
    in_flight: std::sync::Arc<tokio::sync::Semaphore>,
) -> bool
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error>
        + futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    let mut served = false;
    // The host report rides the same socket as the request/answer traffic: one frame on connect (the
    // first tick fires immediately) and one a minute after. It is unsolicited, which is the point —
    // the console knows what the machine is without asking for a grant.
    let mut host = tokio::time::interval(std::time::Duration::from_secs(60));
    host.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // A frame is answered by its own task, and the answer comes back here: the socket has one owner,
    // so a ping is still answered while a tool is running, and one slow tool cannot stop the channel.
    let (outgoing, mut answers) = tokio::sync::mpsc::channel::<Outbound>(OUTGOING_FRAMES);
    // The stop signal of every live subscription on this socket. Dropping a sender stops its task;
    // dropping the whole map (the socket closed) stops them all.
    let streams: Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<()>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    loop {
        tokio::select! {
            _ = host.tick() => {
                if let Err(error) = socket.send(Message::text(crate::host::frame(server.store().data_dir()))).await {
                    eprintln!("summrise-device: the host report could not be sent: {error}");
                    break;
                }
            }
            answer = answers.recv() => {
                // `outgoing` is held by this loop, so the channel closes only when the loop does.
                let Some(answer) = answer else {
                    break;
                };
                let (value, written) = answer.release();
                let sent = socket.send(Message::text(value.to_string())).await;
                // Release the follow's next chunk only once this frame is on the wire (or failed):
                // this ack is the write bound, and a follow may have at most one frame in the queue.
                if let Some(written) = written {
                    let _ = written.send(());
                }
                if let Err(error) = sent {
                    eprintln!("summrise-device: the console channel write failed: {error}");
                    break;
                }
            }
            message = socket.next() => {
                let Some(message) = message else {
                    break;
                };
                match message {
                    Ok(Message::Text(text)) => {
                        served = true;
                        let parsed: Option<remote::RemoteFrame> = serde_json::from_str(&text).ok();
                        match parsed {
                            Some(frame) if frame.subscribe.is_some() => {
                                // A subscription is a long-lived frame, so it is counted against
                                // its own cap rather than the in-flight semaphore a one-shot call
                                // takes.
                                if lock_streams(&streams).len() >= MAX_SUBSCRIPTIONS {
                                    let refusal = json!({
                                        "id": frame.id,
                                        "error": json!({
                                            "jsonrpc": "2.0",
                                            "id": frame.id,
                                            "error": {
                                                "code": -32000,
                                                "message": "this device already holds the maximum number of terminal subscriptions",
                                            },
                                        }),
                                    });
                                    let outgoing = outgoing.clone();
                                    tokio::spawn(async move {
                                        let _ = outgoing.send(Outbound::Answer(refusal)).await;
                                    });
                                } else {
                                    spawn_follow(
                                        frame,
                                        endpoint.clone(),
                                        server.clone(),
                                        key.copied(),
                                        outgoing.clone(),
                                        Arc::clone(&streams),
                                    );
                                }
                                continue;
                            }
                            Some(frame) if frame.events.is_some() => {
                                // A device-event subscription shares the terminal subscriptions'
                                // cap: both are one long-lived read on one socket.
                                if lock_streams(&streams).len() >= MAX_SUBSCRIPTIONS {
                                    let refusal = json!({
                                        "id": frame.id,
                                        "error": json!({
                                            "jsonrpc": "2.0",
                                            "id": frame.id,
                                            "error": {
                                                "code": -32000,
                                                "message": "this device already holds the maximum number of subscriptions",
                                            },
                                        }),
                                    });
                                    let outgoing = outgoing.clone();
                                    tokio::spawn(async move {
                                        let _ = outgoing.send(Outbound::Answer(refusal)).await;
                                    });
                                } else {
                                    spawn_events(
                                        frame,
                                        server.bus(),
                                        outgoing.clone(),
                                        Arc::clone(&streams),
                                    );
                                }
                                continue;
                            }
                            Some(frame) if frame.unsubscribe.is_some() => {
                                let sub = frame.unsubscribe.as_ref().map(|u| u.sub).unwrap_or(0);
                                let stopped = lock_streams(&streams).remove(&sub).is_some();
                                let reply = json!({
                                    "id": frame.id,
                                    "result": { "unsubscribed": stopped },
                                });
                                let outgoing = outgoing.clone();
                                tokio::spawn(async move {
                                    let _ = outgoing.send(Outbound::Answer(reply)).await;
                                });
                                continue;
                            }
                            _ => {}
                        }
                        let endpoint = endpoint.clone();
                        let server = server.clone();
                        let key = key.copied();
                        let outgoing = outgoing.clone();
                        let in_flight = in_flight.clone();
                        tokio::spawn(async move {
                            // The permit is taken here, not in the read loop: a saturated channel
                            // parks the task, and pings and the host report keep flowing. The wait
                            // for the permit is bounded: a full channel answers the frame instead
                            // of parking it until the console gives up.
                            let _permit =
                                match tokio::time::timeout(IN_FLIGHT_WAIT, in_flight.acquire_owned())
                                    .await
                                {
                                    Ok(Ok(permit)) => permit,
                                    // The semaphore is only closed with this loop, and the loop
                                    // owns the socket an answer would go out on.
                                    Ok(Err(_)) => return,
                                    Err(_) => {
                                        if let Some((reply, tool)) = busy_answer(&text) {
                                            // A refusal is a call, not a gap: the begin and end
                                            // lines say the console was answered, and why.
                                            server
                                                .audit_refused(
                                                    &tool,
                                                    "busy",
                                                    Arrival::Console,
                                                    None,
                                                )
                                                .await;
                                            let _ = outgoing
                                                .send(Outbound::Answer(reply))
                                                .await;
                                        }
                                        return;
                                    }
                                };
                            if let Some(reply) =
                                handle_frame(text.as_str(), &endpoint, &server, key.as_ref()).await
                            {
                                let _ = outgoing.send(Outbound::Answer(reply)).await;
                            }
                        });
                    }
                    Ok(Message::Ping(payload)) => {
                        let _ = socket.send(Message::Pong(payload)).await;
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        }
    }
    // The socket is gone: every follow task on it stops now rather than discovering it on its next
    // push. Dropping the senders is the whole mechanism.
    lock_streams(&streams).clear();
    served
}

/// Lock the subscription map, treating a poisoned lock as the map (a panic elsewhere must not take
/// the channel down with it).
fn lock_streams(
    streams: &Mutex<HashMap<u64, tokio::sync::oneshot::Sender<()>>>,
) -> std::sync::MutexGuard<'_, HashMap<u64, tokio::sync::oneshot::Sender<()>>> {
    streams
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Follow the device's event journal from a sequence, pushing one frame per event until stopped.
fn spawn_events(
    frame: remote::RemoteFrame,
    bus: Option<Arc<summrise_bus::Bus>>,
    outgoing: tokio::sync::mpsc::Sender<Outbound>,
    streams: Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<()>>>>,
) {
    tokio::spawn(async move {
        let id = frame.id;
        let from_seq = frame.events.map(|events| events.from_seq).unwrap_or(0);
        let Some(bus) = bus else {
            let _ = outgoing
                .send(Outbound::Answer(json!({
                    "id": id,
                    "error": {
                        "code": -32000,
                        "message": "this device has no event journal",
                    },
                })))
                .await;
            return;
        };
        let mut subscription = match bus.subscribe(from_seq) {
            Ok(subscription) => subscription,
            Err(error) => {
                let _ = outgoing
                    .send(Outbound::Answer(json!({
                        "id": id,
                        "error": { "code": -32000, "message": error.to_string() },
                    })))
                    .await;
                return;
            }
        };
        let (stop, mut stopped) = tokio::sync::oneshot::channel::<()>();
        lock_streams(&streams).insert(id, stop);
        loop {
            let next = tokio::select! {
                _ = &mut stopped => break,
                next = subscription.next() => next,
            };
            let Some(event) = next else {
                break;
            };
            let pushed = json!({ "id": id, "evt": { "sub": id, "event": event } });
            if outgoing.send(Outbound::Answer(pushed)).await.is_err() {
                break;
            }
        }
        lock_streams(&streams).remove(&id);
    });
}

/// Follow one terminal session for a subscription: one audited read for the ack, then pushed frames.
///
/// **THE ONE AUDITED CALL IS THE SUBSCRIBE.** The frame carries a `terminal.read`, and this runs it
/// exactly as a poll would — same admission, same `server.handle`, same audit line — then keeps
/// reading the ring on the session's own notifier for as long as the console holds the subscription.
/// A frame per chunk would otherwise be a tool call (and two audit lines) per chunk.
fn spawn_follow(
    frame: remote::RemoteFrame,
    endpoint: ConsoleEndpoint,
    server: DeviceServer,
    key: Option<VerifyingKey>,
    outgoing: tokio::sync::mpsc::Sender<Outbound>,
    streams: Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<()>>>>,
) {
    tokio::spawn(async move {
        let id = frame.id;
        let Some(subscribe) = frame.subscribe.clone() else {
            return;
        };
        let request_id = frame
            .mcp
            .as_ref()
            .and_then(|mcp| mcp.get("id"))
            .cloned()
            .unwrap_or(Value::Null);
        let admitted = match remote::admit(
            &frame,
            &endpoint.device_id,
            key.as_ref(),
            crate::http::now_ms(),
            remote::Transport::Console,
            server.nonces(),
        ) {
            Ok(admitted) => admitted,
            Err(refusal) => {
                server
                    .audit_refused(
                        "terminal.read",
                        refusal.error_kind(),
                        Arrival::Console,
                        None,
                    )
                    .await;
                let error = json!({
                    "id": id,
                    "error": remote::jsonrpc_error(
                        request_id,
                        remote::GRANT_REFUSAL_CODE,
                        refusal.message(),
                    ),
                });
                let _ = outgoing.send(Outbound::Answer(error)).await;
                return;
            }
        };
        let caller = match &admitted {
            remote::Admitted::Grant(envelope) => Caller::Granted {
                grant: &envelope.grant,
                arrival: Arrival::Console,
                peer: None,
            },
            remote::Admitted::Token(scope) => Caller::Console {
                token_id: &scope.token_id,
                scopes: &scope.scopes,
                allow_destructive: scope.allow_destructive,
                arrival: Arrival::Console,
            },
        };
        // The ack is the first read: it carries the retained output from the cursor the console
        // named, and it is the one audited call this subscription makes.
        let Some(mcp) = frame.mcp.as_ref() else {
            let _ = outgoing
                .send(Outbound::Answer(
                    json!({ "term": { "sub": id, "error": "the subscription carried no request" } }),
                ))
                .await;
            return;
        };
        let response = server.handle(mcp, caller).await;
        let first = if response.get("error").is_some() {
            json!({ "id": id, "error": response })
        } else {
            json!({ "id": id, "result": response })
        };
        if outgoing.send(Outbound::Answer(first)).await.is_err() {
            return;
        }
        let mut cursor = read_cursor(&response).unwrap_or(subscribe.from);
        let terminal = server.terminal_registry();
        let Some(session) = crate::capabilities::terminal::SessionId::parse(&subscribe.session_id)
        else {
            let _ = outgoing
                .send(Outbound::Answer(json!({
                    "term": { "sub": id, "error": "the id is not a session id" }
                })))
                .await;
            return;
        };
        let (stop, mut stopped) = tokio::sync::oneshot::channel::<()>();
        lock_streams(&streams).insert(id, stop);
        loop {
            let answer = tokio::select! {
                _ = &mut stopped => break,
                answer = terminal.read_wait(&session, cursor, FOLLOW_WAIT) => answer,
            };
            match answer {
                Ok(answer) => {
                    if !answer.data.is_empty() {
                        let mut offset = 0usize;
                        for chunk in utf8_chunks(&answer.data, FOLLOW_CHUNK) {
                            // Every chunk of one read carries that read's end; its start is what a
                            // lag check asks the ring about, because the console's cursor cannot be
                            // ahead of the first chunk it has not been given.
                            let from = cursor + offset as u64;
                            offset += chunk.len();
                            let pushed = json!({
                                "term": {
                                    "sub": id,
                                    "session": subscribe.session_id,
                                    "data": String::from_utf8_lossy(chunk),
                                    "cursor": answer.cursor,
                                    "alive": answer.alive,
                                }
                            });
                            match push_follow_frame(
                                &outgoing,
                                &terminal,
                                &session,
                                pushed,
                                from,
                                &mut stopped,
                            )
                            .await
                            {
                                FollowPush::Written => {}
                                FollowPush::Cut(sentence) => {
                                    let _ = outgoing
                                        .send(Outbound::Answer(
                                            json!({ "term": { "sub": id, "error": sentence } }),
                                        ))
                                        .await;
                                    lock_streams(&streams).remove(&id);
                                    return;
                                }
                                FollowPush::Stopped | FollowPush::Gone => {
                                    lock_streams(&streams).remove(&id);
                                    return;
                                }
                            }
                        }
                    }
                    cursor = answer.cursor;
                    if !answer.alive {
                        let _ = outgoing
                            .send(Outbound::Answer(json!({
                                "term": {
                                    "sub": id,
                                    "alive": false,
                                    "cursor": cursor,
                                    "exit_code": answer.end.exit_code,
                                    "signal": answer.end.signal,
                                }
                            })))
                            .await;
                        break;
                    }
                }
                Err(error) => {
                    let _ = outgoing
                        .send(Outbound::Answer(
                            json!({ "term": { "sub": id, "error": error.to_string() } }),
                        ))
                        .await;
                    break;
                }
            }
        }
        lock_streams(&streams).remove(&id);
    });
}

/// What waiting for one followed frame's write decided.
enum FollowPush {
    /// The socket writer took the frame.
    Written,
    /// The subscription was stopped while the frame waited.
    Stopped,
    /// The socket writer is gone; the channel is closing.
    Gone,
    /// The console fell out of the ring's window: this sentence stops the subscription.
    Cut(String),
}

/// Hand one followed frame to the socket writer and wait for it to be written.
///
/// **THIS WAIT IS THE LAG BOUND.** A follow may have one frame in the outbound queue at a time, so
/// a console that stops reading cannot make the follow stack output in front of another call's
/// answer. While the frame waits, the ring is checked every [`FOLLOW_GAP_CHECK`]: when the console
/// is so far behind that the ring has dropped the bytes this frame starts at, the subscription is
/// cut with the ring's own refusal — the sentence the console's paged catch-up already parses to
/// resume from the oldest byte the device still has. A merely slow console is waited for, because
/// the frame's queue is bounded and no output is lost.
async fn push_follow_frame(
    outgoing: &tokio::sync::mpsc::Sender<Outbound>,
    terminal: &crate::capabilities::terminal::PtyRegistry,
    session: &crate::capabilities::terminal::SessionId,
    value: Value,
    from: u64,
    stopped: &mut tokio::sync::oneshot::Receiver<()>,
) -> FollowPush {
    let (written, mut acknowledged) = tokio::sync::oneshot::channel::<()>();
    if outgoing
        .send(Outbound::Push { value, written })
        .await
        .is_err()
    {
        return FollowPush::Gone;
    }
    loop {
        tokio::select! {
            _ = &mut *stopped => return FollowPush::Stopped,
            acknowledged = &mut acknowledged => {
                return match acknowledged {
                    Ok(()) => FollowPush::Written,
                    Err(_) => FollowPush::Gone,
                };
            }
            _ = tokio::time::sleep(FOLLOW_GAP_CHECK) => {
                // Reading one byte at this cursor is the cheapest question the ring answers with
                // its window: `Ok` means the frame is still inside it, `CursorTooOld` names the
                // oldest byte the console can page from.
                if let Err(error) = terminal.read_with_limit(session, from, Some(1)) {
                    return FollowPush::Cut(error.to_string());
                }
            }
        }
    }
}

/// The cursor a read answer ended at, from the MCP envelope `server.handle` returns.
fn read_cursor(response: &Value) -> Option<u64> {
    let text = response
        .get("result")?
        .get("content")?
        .as_array()?
        .first()?
        .get("text")?
        .as_str()?;
    let answer: Value = serde_json::from_str(text).ok()?;
    answer.get("cursor").and_then(Value::as_u64)
}

/// Split bytes into frames without cutting a UTF-8 sequence in half.
fn utf8_chunks(bytes: &[u8], max: usize) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        let mut end = (start + max).min(bytes.len());
        if end < bytes.len() {
            // A continuation byte at the cut means the sequence started before it: back up to the
            // lead byte so one frame never ends mid-character.
            while end > start && (bytes[end] & 0xC0) == 0x80 {
                end -= 1;
            }
            if end == start {
                end = (start + max).min(bytes.len());
            }
        }
        out.push(&bytes[start..end]);
        start = end;
    }
    out
}

/// The answer a frame gets when it waited [`IN_FLIGHT_WAIT`] for a permit and the channel was full.
///
/// The shape is the one a refused frame answers with — `{"id","error":jsonrpc_error(...)}` — so the
/// console's waiter is released and the page reads a sentence instead of timing out. The frame id
/// still correlates it. A frame that does not parse cannot be correlated, so it gets no answer (the
/// handler would not have answered it either). The returned tool is what the audit line names.
fn busy_answer(text: &str) -> Option<(Value, String)> {
    let frame: RemoteFrame = serde_json::from_str(text).ok()?;
    let request_id = frame
        .mcp
        .as_ref()
        .and_then(|mcp| mcp.get("id"))
        .cloned()
        .unwrap_or(Value::Null);
    let tool = frame
        .mcp
        .as_ref()
        .and_then(remote::requested_tool)
        .unwrap_or("(unknown)")
        .to_owned();
    let answer = json!({
        "id": frame.id,
        "error": remote::jsonrpc_error(request_id, DEVICE_BUSY_CODE, DEVICE_BUSY_MESSAGE),
    });
    Some((answer, tool))
}

/// Handle one socket frame; `None` means the frame was malformed and cannot be correlated.
async fn handle_frame(
    text: &str,
    endpoint: &ConsoleEndpoint,
    server: &DeviceServer,
    key: Option<&VerifyingKey>,
) -> Option<Value> {
    let frame: RemoteFrame = match serde_json::from_str(text) {
        Ok(frame) => frame,
        Err(error) => {
            eprintln!("summrise-device: the console sent a malformed frame: {error}");
            return None;
        }
    };

    let request_id = frame
        .mcp
        .as_ref()
        .and_then(|mcp| mcp.get("id"))
        .cloned()
        .unwrap_or(Value::Null);
    let tool = frame
        .mcp
        .as_ref()
        .and_then(remote::requested_tool)
        .map(str::to_owned);

    let admitted = match remote::admit(
        &frame,
        &endpoint.device_id,
        key,
        crate::http::now_ms(),
        remote::Transport::Console,
        server.nonces(),
    ) {
        Ok(admitted) => admitted,
        Err(refusal) => {
            server
                .audit_refused(
                    tool.as_deref().unwrap_or("(unknown)"),
                    refusal.error_kind(),
                    Arrival::Console,
                    None,
                )
                .await;
            return Some(json!({
                "id": frame.id,
                "error": remote::jsonrpc_error(
                    request_id,
                    remote::GRANT_REFUSAL_CODE,
                    refusal.message(),
                ),
            }));
        }
    };

    let caller = match &admitted {
        remote::Admitted::Grant(envelope) => Caller::Granted {
            grant: &envelope.grant,
            arrival: Arrival::Console,
            peer: None,
        },
        remote::Admitted::Token(scope) => Caller::Console {
            token_id: &scope.token_id,
            scopes: &scope.scopes,
            allow_destructive: scope.allow_destructive,
            arrival: Arrival::Console,
        },
    };
    let Some(mcp) = frame.mcp.as_ref() else {
        eprintln!("summrise-device: a frame carried neither a request nor a subscription");
        return None;
    };
    let response = server.handle(mcp, caller).await;
    if response.get("error").is_some() {
        Some(json!({ "id": frame.id, "error": response }))
    } else {
        Some(json!({ "id": frame.id, "result": response }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
    use tokio_tungstenite::tungstenite::protocol::Role;
    use tokio_tungstenite::tungstenite::Message as WsMessage;
    use tokio_tungstenite::{accept_hdr_async, WebSocketStream};

    use super::*;
    use crate::capabilities::terminal;
    use crate::test_support::TempDir;
    use summrise_core::grant::{canonical_message, Grant, GrantEnvelope};

    const SIGNING_SEED: [u8; 32] = [0x51; 32];
    const TOKEN: [u8; 32] = [0x6b; 32];

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn device_id() -> String {
        "ab".repeat(32)
    }

    fn token() -> String {
        hex(&TOKEN)
    }

    fn signing() -> SigningKey {
        SigningKey::from_bytes(&SIGNING_SEED)
    }

    fn endpoint(url: &str) -> ConsoleEndpoint {
        ConsoleEndpoint {
            channel_url: format!("{url}/devices/{}/channel", device_id()),
            device_id: device_id(),
            token: token(),
        }
    }

    fn now_ms() -> i64 {
        crate::http::now_ms()
    }

    fn grant(tools: &[&str], expires_at_ms: u64) -> GrantEnvelope {
        grant_allowing(tools, expires_at_ms, false)
    }

    /// A grant whose signed `allow_destructive` is exactly `allow_destructive`.
    fn grant_allowing(
        tools: &[&str],
        expires_at_ms: u64,
        allow_destructive: bool,
    ) -> GrantEnvelope {
        signed_by(
            &signing(),
            Grant {
                device_id: device_id(),
                tools: tools.iter().map(|tool| (*tool).to_owned()).collect(),
                expires_at_ms,
                allow_destructive,
                // One nonce per grant, as the console's signer does it: a frame built twice in a test
                // must carry the same nonce for the replay check to have something to compare.
                nonce: "5a5a".repeat(8),
            },
        )
    }

    fn signed_by(key: &SigningKey, grant: Grant) -> GrantEnvelope {
        let signature = key.sign(canonical_message(&grant).as_bytes());
        GrantEnvelope {
            grant,
            signature: hex(&signature.to_bytes()),
        }
    }

    /// A handler with its own data directory and files root.
    fn fixture(label: &str) -> (DeviceServer, Config, TempDir) {
        let dir = TempDir::new(label);
        let install = dir.join("install");
        let data = dir.join("data");
        let files_root = dir.join("workspace");
        for path in [&install, &data, &files_root] {
            fs::create_dir_all(path).expect("the fixture directory is creatable");
        }
        let config = Config {
            install,
            data,
            port: 0,
            token_file: dir.join("token"),
            files_root,
            files_shared_root: None,
            release_version: None,
            release_sha: None,
            release_channel: None,
            supervised: false,
            console_url: Some("ws://unused".to_owned()),
            console_token_file: Some(dir.join("console")),
            console_allow_plaintext: true,
            mesh_broker: None,
            mesh_relay: None,
            mesh_stun: None,
        };
        let server = DeviceServer::new(&config, terminal::Limits::default());
        (server, config, dir)
    }

    fn audit_lines(config: &Config) -> Vec<Value> {
        match fs::read_to_string(config.data.join("events.jsonl")) {
            Ok(contents) => contents
                .lines()
                .map(|line| serde_json::from_str(line).expect("an audit line is JSON"))
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("the audit log is readable: {error}"),
        }
    }

    /// One connection the fake console accepted, with the handshake facts the device sent.
    struct Connection {
        uri: String,
        authorization: Option<String>,
        socket: WebSocketStream<tokio::net::TcpStream>,
    }

    /// A console-shaped listener on loopback: it accepts, records, and hands the socket to the test.
    struct FakeConsole {
        url: String,
        connections: mpsc::UnboundedReceiver<Connection>,
        _task: JoinHandle<()>,
    }

    impl FakeConsole {
        #[allow(clippy::result_large_err)] // the handshake callback's error type is tungstenite's
        async fn start() -> FakeConsole {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("a loopback listener");
            let url = format!("ws://{}", listener.local_addr().expect("the bound address"));
            let (sender, connections) = mpsc::unbounded_channel();
            let task = tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        break;
                    };
                    let sender = sender.clone();
                    tokio::spawn(async move {
                        let seen = Arc::new(Mutex::new(None));
                        let seen_in = Arc::clone(&seen);
                        let Ok(socket) = accept_hdr_async(
                            stream,
                            move |request: &Request, response: Response| {
                                let authorization = request
                                    .headers()
                                    .get("authorization")
                                    .and_then(|value| value.to_str().ok())
                                    .map(str::to_owned);
                                *seen_in.lock().expect("the handshake lock") =
                                    Some((request.uri().to_string(), authorization));
                                Ok(response)
                            },
                        )
                        .await
                        else {
                            return;
                        };
                        let (uri, authorization) = seen
                            .lock()
                            .expect("the handshake lock")
                            .take()
                            .expect("seen");
                        let _ = sender.send(Connection {
                            uri,
                            authorization,
                            socket,
                        });
                    });
                }
            });
            FakeConsole {
                url,
                connections,
                _task: task,
            }
        }

        async fn next(&mut self) -> Connection {
            tokio::time::timeout(Duration::from_secs(5), self.connections.recv())
                .await
                .expect("the device connects")
                .expect("the listener is alive")
        }
    }

    async fn send_frame<S>(socket: &mut S, frame: Value)
    where
        S: futures_util::Sink<WsMessage, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    {
        socket
            .send(WsMessage::text(frame.to_string()))
            .await
            .expect("the frame is sent");
    }

    /// The next frame that answers a request.
    ///
    /// The host report is unsolicited and rides the same socket, so it can land before or after any
    /// answer — the console keys it by `host`, not by `id`. A test that reads "the next frame" after
    /// sending a request wants the answer and must not depend on which of the two the loop flushed
    /// first.
    async fn next_frame<S>(socket: &mut S) -> Value
    where
        S: futures_util::Stream<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>>
            + Unpin,
    {
        loop {
            match socket
                .next()
                .await
                .expect("the socket is open")
                .expect("a frame")
            {
                WsMessage::Text(text) => {
                    let frame: Value =
                        serde_json::from_str(text.as_str()).expect("the frame is JSON");
                    if frame.get("host").is_some() {
                        continue;
                    }
                    return frame;
                }
                WsMessage::Close(_) => panic!("the device closed the channel"),
                _ => {}
            }
        }
    }

    fn read_call(id: u64, path: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": "files.read", "arguments": { "path": path } },
        })
    }

    fn state_call(id: u64) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": "state.get", "arguments": {} },
        })
    }

    fn write_call(id: u64, path: &str) -> Value {
        write_call_with(id, path, "must not land")
    }

    fn write_call_with(id: u64, path: &str, content: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": "files.write",
                "arguments": { "path": path, "content": content }
            },
        })
    }

    #[test]
    fn the_pinned_key_refuses_the_placeholder_and_a_malformed_key() {
        assert_eq!(
            parse_console_key(Some(PLACEHOLDER_CONSOLE_GRANT_KEY_HEX)),
            Err(KeyError::Placeholder)
        );
        assert_eq!(parse_console_key(None), Err(KeyError::Placeholder));
        assert_eq!(parse_console_key(Some("not-hex")), Err(KeyError::BadKey));
        assert!(
            parse_console_key(Some(&hex(&signing().verifying_key().to_bytes()))).is_ok(),
            "a real key is accepted"
        );
    }

    /// A refusal is final; a network fault is not. The classifier is the difference between a device
    /// that re-enrolls and one that retries a dead token for ever.
    #[test]
    fn an_authentication_refusal_is_told_apart_from_a_network_fault() {
        for status in [401u16, 403] {
            let response = tokio_tungstenite::tungstenite::http::Response::builder()
                .status(status)
                .body(None)
                .expect("a response builds");
            let error = tokio_tungstenite::tungstenite::Error::Http(Box::new(response));
            assert_eq!(refusal_status(&error), Some(status));
        }

        let response = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(502)
            .body(None)
            .expect("a response builds");
        let error = tokio_tungstenite::tungstenite::Error::Http(Box::new(response));
        assert_eq!(
            refusal_status(&error),
            None,
            "a gateway fault is worth retrying"
        );
        assert_eq!(
            refusal_status(&tokio_tungstenite::tungstenite::Error::ConnectionClosed),
            None
        );
    }

    #[test]
    fn the_channel_url_accepts_the_console_spellings_and_refuses_the_rest() {
        for (base, expected) in [
            (
                "ws://127.0.0.1:8080",
                "ws://127.0.0.1:8080/devices/ab/channel?v=0.0.0-test",
            ),
            (
                "http://127.0.0.1:8080/",
                "ws://127.0.0.1:8080/devices/ab/channel?v=0.0.0-test",
            ),
            (
                "https://console.example.test",
                "wss://console.example.test/devices/ab/channel?v=0.0.0-test",
            ),
            (
                "wss://console.example.test/console",
                "wss://console.example.test/console/devices/ab/channel?v=0.0.0-test",
            ),
        ] {
            let actual = channel_url(base, "ab", "0.0.0-test", None, true).expect(base);
            assert!(actual.starts_with(expected), "{base}: {actual}");
            // The machine's own name follows the version, encoded so a space or `&` survives.
            if let Some(name) = host_name() {
                assert_eq!(
                    actual,
                    format!("{expected}&n={}", percent_encode(&name)),
                    "{base}: the name is announced with the version"
                );
            }
        }

        for bad in [
            "",
            "console.example.test",
            "ftp://host",
            "ws://",
            "ws://host/a b",
        ] {
            assert!(
                channel_url(bad, "ab", "0.0.0-test", None, true).is_err(),
                "{bad:?} must be refused"
            );
        }
    }

    /// Plaintext to another machine carries the registration token in the clear; to this machine it
    /// never crosses a wire. The operator's `allow_plaintext` is the only way past the rule.
    #[test]
    fn a_plaintext_channel_is_loopback_or_explicitly_allowed() {
        for base in [
            "ws://127.0.0.1:19000",
            "http://127.0.0.1:19000/console",
            "ws://localhost:19000",
            "ws://[::1]:19000",
            // A tailnet / LAN link is private transport, so it needs no allowance: the deployed
            // device reaches its console this way (100.74.250.98), and refusing it rolled a working
            // install back once.
            "ws://100.74.250.98:19000",
            "ws://10.1.2.3:19000",
            "ws://192.168.1.5:19000",
            "ws://[fd7a:115c:a1e0::1]:19000",
            "https://console.example.test",
            "wss://console.example.test",
        ] {
            assert!(
                channel_url(base, "ab", "0.0.0-test", None, false).is_ok(),
                "{base:?} must be accepted without an allowance"
            );
        }

        for base in [
            "ws://8.8.8.8:19000",
            "http://console.example.test",
            "ws://203.0.113.10:19000",
        ] {
            let error = channel_url(base, "ab", "0.0.0-test", None, false)
                .expect_err("plaintext off this machine is refused by default");
            assert!(
                matches!(error, ConsoleError::PlaintextChannel { .. }),
                "{base}: {error:?}"
            );
            let allowed = channel_url(base, "ab", "0.0.0-test", None, true)
                .expect("an explicit allowance is honoured");
            assert!(
                allowed.starts_with("ws://"),
                "{base}: an allowed plaintext base still becomes a ws channel: {allowed}"
            );
        }

        let error = channel_url("ws://8.8.8.8:19000", "ab", "0.0.0-test", None, false)
            .expect_err("refused");
        assert!(
            error.to_string().contains("8.8.8.8"),
            "the refusal names the host: {error}"
        );
    }

    #[test]
    fn the_enrollment_file_accepts_the_registration_response_and_two_plain_values() {
        let json = format!(r#"{{"id":"{}","token":"{}"}}"#, device_id(), token());
        assert_eq!(
            parse_enrollment(&json),
            Some((device_id(), token())),
            "the console's own registration response is the primary shape"
        );
        for text in [
            format!("{}\n{}\n", device_id(), token()),
            format!("{} {}", device_id(), token()),
            format!("{}{}{}", device_id(), ":", token()),
        ] {
            assert_eq!(parse_enrollment(&text), Some((device_id(), token())));
        }
        for bad in [
            "",
            "abc",
            &"ab".repeat(31),
            &format!("{} {}", device_id(), "zz"),
        ] {
            assert_eq!(parse_enrollment(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_endpoint_loads_the_registration_file_and_builds_the_channel_url() {
        let dir = TempDir::new("console-enrollment");
        let token_file = dir.join("console.json");
        fs::write(
            &token_file,
            serde_json::json!({
                "id": device_id(),
                "public_key": device_id(),
                "name": "the test device",
                "registered_at_ms": 1,
                "last_seen_ms": 1,
                "token": token(),
            })
            .to_string(),
        )
        .expect("the registration response is written");
        #[cfg(windows)]
        summrise_winsec::protect_file(&token_file)
            .expect("the fixture token file is protected the way enrollment writes it");
        let mut config = Config {
            install: dir.join("install"),
            data: dir.join("data"),
            port: 0,
            token_file: dir.join("token"),
            files_root: dir.join("workspace"),
            files_shared_root: None,
            release_version: None,
            release_sha: None,
            release_channel: None,
            supervised: false,
            console_url: Some("https://console.example.test".to_owned()),
            console_token_file: Some(token_file),
            console_allow_plaintext: true,
            mesh_broker: None,
            mesh_relay: None,
            mesh_stun: None,
        };

        let loaded = ConsoleEndpoint::from_config(&config)
            .expect("the registration file loads")
            .expect("both console facts are present");
        assert_eq!(loaded.device_id, device_id());
        assert_eq!(loaded.token, token());
        let mut expected = format!(
            "wss://console.example.test/devices/{}/channel?v={}",
            device_id(),
            env!("CARGO_PKG_VERSION")
        );
        // The machine's name rides with the version wherever the machine has one. An expectation that
        // hardcodes its absence passes on a runner that inherits neither `COMPUTERNAME` nor `HOSTNAME`
        // (the Linux job does not) and fails on every Windows machine, where `COMPUTERNAME` is always
        // set — which is how this test went red on 2026-10-24 (run 40080082612) after 98 commits of
        // nobody running it.
        if let Some(name) = host_name() {
            expected.push_str(&format!("&n={}", percent_encode(&name)));
        }
        assert_eq!(loaded.channel_url, expected);

        config.console_url = None;
        config.console_token_file = None;
        assert!(
            matches!(ConsoleEndpoint::from_config(&config), Ok(None)),
            "no console facts means the feature is off, not an error"
        );
    }

    #[tokio::test]
    async fn device_events_follow_from_the_cursor() {
        let dir = std::env::temp_dir().join(format!(
            "summrise-console-events-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let bus = Arc::new(
            summrise_bus::Bus::open(dir.join("journal").join("000001.seg"))
                .expect("the journal opens"),
        );
        bus.publish("a", json!({})).unwrap();
        bus.publish("b", json!({})).unwrap();

        let (outgoing, mut frames) = mpsc::channel(8);
        let streams = Arc::new(Mutex::new(HashMap::new()));
        let frame: remote::RemoteFrame =
            serde_json::from_value(json!({ "id": 7, "events": { "from_seq": 2 } }))
                .expect("the frame parses");
        spawn_events(
            frame,
            Some(Arc::clone(&bus)),
            outgoing,
            Arc::clone(&streams),
        );

        let first = frames.recv().await.expect("one event").release().0;
        assert_eq!(first["evt"]["sub"], 7);
        assert_eq!(first["evt"]["event"]["seq"], 2);
        assert_eq!(first["evt"]["event"]["type"], "b");

        for _ in 0..100 {
            if lock_streams(&streams).contains_key(&7) {
                break;
            }
            tokio::task::yield_now().await;
        }
        let stop = lock_streams(&streams)
            .remove(&7)
            .expect("the subscription is registered");
        let _ = stop.send(());
    }

    /// The channel coming back is the machine's wake, and the console is what tells the checker.
    ///
    /// The unit tests prove the guard; this proves the wire from the socket to it: a connection the
    /// console accepted asks for exactly one check, without waiting for a frame.
    #[tokio::test]
    async fn a_console_connection_asks_the_update_checker() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let (server, _config, _dir) = fixture("console-wake");
        let probes = Arc::new(AtomicUsize::new(0));
        let checker =
            crate::update_check::test_checker(server.store().clone(), Arc::clone(&probes));
        let server = server.with_update_check(Some(checker));
        let mut console = FakeConsole::start().await;
        let _handle = spawn_with_key(
            endpoint(&console.url),
            server,
            Some(signing().verifying_key()),
        );
        let _connection = console.next().await;

        tokio::time::timeout(Duration::from_secs(5), async {
            while probes.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the accepted channel is a wake");
        assert_eq!(probes.load(Ordering::SeqCst), 1);
    }

    /// A slow tool is one call, not the whole channel.
    ///
    /// On 2026-10-27 an `scp` through `runs.exec` ran 420 s on 14rjcr8, and the serial loop let every
    /// later frame wait behind it: the console recorded 504 for `host.facts`, `terminal.list` and the
    /// run itself while the device was alive and answering nothing. The second call here must answer
    /// while the first still runs.
    #[tokio::test]
    async fn a_slow_call_does_not_hold_the_channel() {
        let (server, _config, _dir) = fixture("console-concurrent");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;

        // Four seconds, whichever platform: long enough that a serial channel cannot answer the
        // second frame before the first ends, short enough for a test suite.
        #[cfg(windows)]
        let slow = json!({ "command": "cmd", "args": ["/C", "ping", "-n", "5", "127.0.0.1"] });
        #[cfg(not(windows))]
        let slow = json!({ "command": "sleep", "args": ["4"] });

        let mut slow_arguments = slow;
        slow_arguments["timeout_ms"] = json!(10_000);
        send_frame(
            &mut connection.socket,
            json!({
                "id": 7,
                "mcp": json!({
                    "jsonrpc": "2.0",
                    "id": 70,
                    "method": "tools/call",
                    "params": { "name": "runs.exec", "arguments": slow_arguments },
                }),
                "grant": grant_allowing(&["runs.exec"], now_ms() as u64 + 60_000, true),
            }),
        )
        .await;
        send_frame(
            &mut connection.socket,
            json!({
                "id": 8,
                "mcp": state_call(71),
                "grant": grant(&["state.get"], now_ms() as u64 + 60_000),
            }),
        )
        .await;

        let answer =
            tokio::time::timeout(Duration::from_secs(3), next_frame(&mut connection.socket))
                .await
                .expect("the device answers the second call while the first is still running");
        assert_eq!(answer["id"], 8, "the fast call answers first: {answer}");
        let slow_answer = next_frame(&mut connection.socket).await;
        assert_eq!(
            slow_answer["id"], 7,
            "the slow call answers when it ends: {slow_answer}"
        );
        handle.abort();
    }

    /// A full channel answers "the device is busy" inside [`IN_FLIGHT_WAIT`] instead of parking
    /// the frame for ever.
    ///
    /// With all sixteen permits held, a fast tool's frame used to wait for one to be released —
    /// indistinguishable to the console from a frame the device accepted and dropped, and past the
    /// console's own 30 s wait a timeout instead of a reason. The answer must be the transport's
    /// error envelope, with its audit begin and end; once a permit is free the same frame runs.
    ///
    /// Mutation proof: with the acquire unbounded again, this test times out at
    /// `IN_FLIGHT_WAIT + 2 s` waiting for the answer, because the permits are held for the whole
    /// test.
    #[tokio::test]
    async fn a_full_channel_answers_busy_instead_of_parking_the_frame() {
        let (server, config, _dir) = fixture("console-in-flight-busy");
        // Hold all sixteen permits before the socket opens. A real saturation would be sixteen
        // parked calls, but then the fixture's own session output would decide when a permit
        // frees, and this test would be measuring timing rather than a bound.
        let in_flight = Arc::new(tokio::sync::Semaphore::new(MAX_IN_FLIGHT_FRAMES));
        let held = Arc::clone(&in_flight)
            .acquire_many_owned(MAX_IN_FLIGHT_FRAMES as u32)
            .await
            .expect("every permit is free in a fresh channel");
        let (mut console, serve) = piped_console_with(&server, 1024 * 1024, in_flight).await;

        send_frame(
            &mut console,
            json!({
                "id": 300,
                "mcp": state_call(2_000),
                "grant": grant_once(&["state.get"], "b900"),
            }),
        )
        .await;
        let answer = tokio::time::timeout(
            IN_FLIGHT_WAIT + Duration::from_secs(2),
            next_frame(&mut console),
        )
        .await
        .expect("a full channel answers inside the bound instead of parking the frame");
        assert_eq!(answer["id"], 300, "{answer}");
        assert_eq!(
            answer["error"]["error"]["code"],
            json!(DEVICE_BUSY_CODE),
            "busy is its own code, not a grant refusal: {answer}"
        );
        assert_eq!(
            answer["error"]["error"]["message"],
            json!(DEVICE_BUSY_MESSAGE),
            "the client reads the sentence, not a timeout: {answer}"
        );

        // The refusal is a call with its end, as every answer is: the console's timeline must show
        // this request was answered, and why.
        let lines = audit_lines(&config);
        let begin = lines
            .iter()
            .find(|line| line["event"] == "begin" && line["tool"] == "state.get")
            .unwrap_or_else(|| panic!("the busy frame has a begin line: {lines:?}"));
        assert_eq!(begin["decision"], "deny");
        let id = begin["id"].as_u64().expect("the call id");
        let end = lines
            .iter()
            .find(|line| line["event"] == "end" && line["id"] == json!(id))
            .unwrap_or_else(|| panic!("the busy frame has its end line: {lines:?}"));
        assert_eq!(end["ok"], false);
        assert_eq!(end["error_kind"], "busy");

        // The normal path is untouched: once a permit is free the same frame runs and answers as
        // the transport always did.
        drop(held);
        send_frame(
            &mut console,
            json!({
                "id": 301,
                "mcp": state_call(2_001),
                "grant": grant_once(&["state.get"], "b901"),
            }),
        )
        .await;
        let answer = tokio::time::timeout(Duration::from_secs(5), next_frame(&mut console))
            .await
            .expect("a frame that gets a permit runs");
        assert_eq!(answer["id"], 301, "{answer}");
        assert_eq!(answer["result"]["id"], 2_001, "{answer}");
        serve.abort();
    }

    /// A subscription follows a live session: one audited read as the ack, then pushed frames
    /// until the console says stop.
    #[tokio::test]
    async fn a_subscription_pushes_a_live_session_and_stops_when_told() {
        let (server, _config, _dir) = fixture("console-subscribe-live");
        let mut console = FakeConsole::start().await;
        let registry = server.terminal_registry();
        let session = registry
            .create(
                crate::capabilities::terminal::CreateTarget::Pty { shell: None },
                24,
                80,
            )
            .await
            .expect("a local shell");
        let session_id = session.as_str().to_owned();
        let _handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;

        send_frame(
            &mut connection.socket,
            json!({
                "id": 31,
                "mcp": json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "tools/call",
                    "params": { "name": "terminal.read", "arguments": { "id": session_id, "cursor": 0 } },
                }),
                "subscribe": { "session_id": session_id, "from": 0 },
                "grant": grant(&["terminal.read"], now_ms() as u64 + 60_000),
            }),
        )
        .await;
        let ack = next_frame(&mut connection.socket).await;
        assert_eq!(ack["id"], 31, "the ack echoes the subscribe id: {ack}");

        // Something for the follow loop to push: input goes to the registry directly, so this test
        // measures the push and not the write tool's authorization.
        registry
            .write(&session, b"echo subscribe-marker\n")
            .await
            .expect("the shell takes input");

        let mut pushed = String::new();
        for _ in 0..12 {
            let frame = next_frame(&mut connection.socket).await;
            if let Some(term) = frame.get("term") {
                assert_eq!(
                    term["sub"], 31,
                    "a pushed frame names its subscription: {frame}"
                );
                pushed.push_str(term["data"].as_str().unwrap_or_default());
                if pushed.contains("subscribe-marker") {
                    break;
                }
            }
        }
        assert!(
            pushed.contains("subscribe-marker"),
            "the pushed output: {pushed:?}"
        );

        send_frame(
            &mut connection.socket,
            json!({ "id": 33, "unsubscribe": { "sub": 31 } }),
        )
        .await;
        let mut stopped = false;
        for _ in 0..8 {
            let frame = next_frame(&mut connection.socket).await;
            if frame["id"] == 33 {
                assert_eq!(
                    frame["result"]["unsubscribed"],
                    json!(true),
                    "the live subscription was stopped: {frame}"
                );
                stopped = true;
                break;
            }
        }
        assert!(stopped, "the unsubscribe was answered");
        let _ = registry.close(&session).await;
    }

    /// A subscribe for a session that is not there is refused in the ack, and the follow loop says
    /// so once instead of pushing forever.
    #[tokio::test]
    async fn a_subscription_to_an_unknown_session_is_refused() {
        let (server, _config, _dir) = fixture("console-subscribe-unknown");
        let mut console = FakeConsole::start().await;
        let _handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;
        send_frame(
            &mut connection.socket,
            json!({
                "id": 41,
                "mcp": json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "tools/call",
                    "params": { "name": "terminal.read", "arguments": { "id": "session-999", "cursor": 0 } },
                }),
                "subscribe": { "session_id": "session-999", "from": 0 },
                "grant": grant(&["terminal.read"], now_ms() as u64 + 60_000),
            }),
        )
        .await;
        let ack = next_frame(&mut connection.socket).await;
        assert_eq!(ack["id"], 41);
        let text = ack["result"]["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        assert!(
            text.contains("unknown") || text.contains("session"),
            "the refusal is the read's own sentence: {ack}"
        );
        let pushed = next_frame(&mut connection.socket).await;
        assert_eq!(pushed["term"]["sub"], 41);
        assert!(
            pushed["term"]["error"].is_string(),
            "the follow loop reports the end once: {pushed}"
        );
    }

    /// A channel whose console end is an in-memory pipe of `capacity` bytes.
    ///
    /// The pipe is the reader a test controls: once the test stops reading, the device's next write
    /// that does not fit blocks exactly as a console that stopped reading its socket does. A
    /// loopback socket is not usable for this — measured on 2026-11-06, the kernel accepted 57
    /// 32 KiB frames for an unread peer before the writer waited — so a queue bound would be
    /// invisible behind the kernel's own buffer.
    async fn piped_console(
        server: &DeviceServer,
        capacity: usize,
    ) -> (WebSocketStream<tokio::io::DuplexStream>, JoinHandle<bool>) {
        let in_flight = Arc::new(tokio::sync::Semaphore::new(MAX_IN_FLIGHT_FRAMES));
        piped_console_with(server, capacity, in_flight).await
    }

    /// [`piped_console`] with the in-flight semaphore supplied, so a test can hold every permit.
    async fn piped_console_with(
        server: &DeviceServer,
        capacity: usize,
        in_flight: Arc<tokio::sync::Semaphore>,
    ) -> (WebSocketStream<tokio::io::DuplexStream>, JoinHandle<bool>) {
        let (device_side, test_side) = tokio::io::duplex(capacity);
        let endpoint = endpoint("ws://pipe");
        let key = signing().verifying_key();
        let server = server.clone();
        let handle = tokio::spawn(async move {
            let socket = WebSocketStream::from_raw_socket(device_side, Role::Server, None).await;
            serve_with_in_flight(socket, &endpoint, &server, Some(&key), in_flight).await
        });
        let console = WebSocketStream::from_raw_socket(test_side, Role::Client, None).await;
        (console, handle)
    }

    /// A grant the test signs for one frame, with a nonce of its own.
    ///
    /// The shared [`grant`] helper reuses one nonce, which the device's replay log reads as the
    /// same consent; a test that sends more than one granted frame needs a nonce per frame.
    fn grant_once(tools: &[&str], nonce: &str) -> GrantEnvelope {
        signed_by(
            &signing(),
            Grant {
                device_id: device_id(),
                tools: tools.iter().map(|tool| (*tool).to_owned()).collect(),
                expires_at_ms: now_ms() as u64 + 60_000,
                allow_destructive: false,
                nonce: nonce.to_owned(),
            },
        )
    }

    /// A subscribe frame: a `terminal.read` from `from`, carried as a subscription.
    fn subscribe_frame(frame_id: u64, request_id: u64, session: &str, from: u64) -> Value {
        json!({
            "id": frame_id,
            "mcp": json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "tools/call",
                "params": { "name": "terminal.read", "arguments": { "id": session, "cursor": from } },
            }),
            "subscribe": { "session_id": session, "from": from },
            "grant": grant_once(&["terminal.read"], &format!("0{frame_id:02x}")),
        })
    }

    /// A lagging console cannot make a follow queue output ahead of another call's answer, and a
    /// follow it has left behind the ring is cut with the ring's own sentence.
    ///
    /// Measured on 9VH (0.3.282) after a >1 MiB pty flood: a quiet pty's `terminal.read` held the
    /// MCP call 45 s while `terminal.list` answered in 3 s. The read's own wait is bounded, so the
    /// answers were stuck in one outbound queue behind the flood's pushed frames. Here the pipe
    /// holds three chunks, so a fourth write waits exactly as a console that stopped reading does;
    /// the tool call is sent while it waits, and the ring then moves past the frame the writer is
    /// holding.
    #[tokio::test]
    async fn a_lagging_console_does_not_hold_another_calls_answer() {
        let (server, _config, _dir) = fixture("console-follow-lag");
        let registry = server.terminal_registry();
        let session = registry
            .create(terminal::CreateTarget::Pty { shell: None }, 24, 80)
            .await
            .expect("a local shell");
        let session_id = session.as_str().to_owned();
        let (mut console, _serve) = piped_console(&server, 96 * 1024).await;

        send_frame(&mut console, subscribe_frame(51, 1, &session_id, 0)).await;
        let ack = next_frame(&mut console).await;
        assert_eq!(ack["id"], 51, "the subscribe is acked: {ack}");

        // One read's worth of flood: the follow hands it over a frame at a time, and the pipe
        // holds three 32 KiB frames before the writer has to wait.
        registry
            .push_output_for_test(&session, &vec![b'a'; 512 * 1024])
            .expect("the ring takes output");
        tokio::time::sleep(Duration::from_millis(150)).await;

        // A call whose answer must not queue behind the flood. It cannot even be read until the
        // writer takes the frame it is waiting on.
        send_frame(
            &mut console,
            json!({
                "id": 52,
                "mcp": json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": { "name": "terminal.list", "arguments": {} },
                }),
                "grant": grant_once(&["terminal.list"], "a52"),
            }),
        )
        .await;

        // The ring moves past the frame the writer is holding: the console is out of the window
        // now, so no later frame can catch it up. The follow must say so and stop.
        registry
            .push_output_for_test(&session, &vec![b'b'; 1024 * 1024])
            .expect("the ring takes output");
        tokio::time::sleep(Duration::from_millis(700)).await;

        let mut seen = 0usize;
        let mut data_frames = 0usize;
        let mut answer_at = None;
        let mut cut_at = None;
        let mut cut = String::new();
        while seen < 24 && (answer_at.is_none() || cut_at.is_none()) {
            let Ok(frame) =
                tokio::time::timeout(Duration::from_secs(2), next_frame(&mut console)).await
            else {
                break;
            };
            seen += 1;
            if frame["id"] == 52 {
                answer_at = Some(seen);
            }
            if let Some(term) = frame.get("term") {
                if term["data"].as_str().is_some_and(|data| !data.is_empty()) {
                    data_frames += 1;
                }
                if let Some(sentence) = term["error"].as_str() {
                    cut = sentence.to_owned();
                    cut_at = Some(seen);
                }
            }
        }
        assert!(
            data_frames > 0,
            "the flood's frames were in front of the answer, not skipped: {seen} frames"
        );
        assert!(
            cut_at.is_some(),
            "the follow was cut once the ring passed its frame: {cut:?}"
        );
        assert!(
            cut.contains("older than the retained output"),
            "the cut is the ring's own sentence, the one the console's paged catch-up reads: {cut}"
        );
        assert!(
            answer_at.is_some(),
            "the tool answer arrived within the first {seen} frames"
        );
        assert!(
            answer_at.expect("the answer arrived") <= 8,
            "the answer queued behind the flood: frame {answer_at:?} of {seen}"
        );
        let _ = registry.close(&session).await;
    }

    /// A console that keeps up keeps the follow's frames, their order and their size.
    #[tokio::test]
    async fn a_console_that_keeps_up_keeps_the_follows_frames_and_order() {
        let (server, _config, _dir) = fixture("console-follow-order");
        let registry = server.terminal_registry();
        let session = registry
            .create(terminal::CreateTarget::Pty { shell: None }, 24, 80)
            .await
            .expect("a local shell");
        let session_id = session.as_str().to_owned();
        let (mut console, _serve) = piped_console(&server, 1024 * 1024).await;

        send_frame(&mut console, subscribe_frame(61, 1, &session_id, 0)).await;
        let ack = next_frame(&mut console).await;
        assert_eq!(ack["id"], 61, "the subscribe is acked: {ack}");

        // 96 KiB in one append is three full chunks: the framing the console already knows.
        let payload: String = (0..96 * 1024)
            .map(|index| (b'a' + (index % 26) as u8) as char)
            .collect();
        registry
            .push_output_for_test(&session, payload.as_bytes())
            .expect("the ring takes output");

        let mut text = String::new();
        let mut sizes = Vec::new();
        let mut cursors = Vec::new();
        while text.len() < payload.len() {
            let frame = tokio::time::timeout(Duration::from_secs(5), next_frame(&mut console))
                .await
                .expect("the follow pushes the output");
            let term = frame
                .get("term")
                .unwrap_or_else(|| panic!("a pushed frame: {frame}"));
            assert_eq!(
                term["sub"], 61,
                "a pushed frame names its subscription: {frame}"
            );
            assert!(
                term.get("error").is_none(),
                "the follow stays live: {frame}"
            );
            let data = term["data"].as_str().expect("a pushed chunk is text");
            assert!(
                data.len() <= FOLLOW_CHUNK,
                "a frame carries at most one chunk: {}",
                data.len()
            );
            text.push_str(data);
            sizes.push(data.len());
            cursors.push(term["cursor"].as_u64().expect("the frame names its cursor"));
        }
        assert_eq!(text, payload, "every byte arrives, in order");
        assert_eq!(
            sizes,
            vec![FOLLOW_CHUNK, FOLLOW_CHUNK, FOLLOW_CHUNK],
            "the chunks are the ones the console already reads"
        );
        assert!(
            cursors.windows(2).all(|pair| pair[0] <= pair[1]),
            "the cursor never goes backwards: {cursors:?}"
        );

        // And the follow is still live: the next output is pushed, not polled.
        //
        // **THE FRAME IS CHECKED BY CURSOR, NOT BY CONTENT.** A `pty` session prints its shell's
        // prompt on its own schedule, and this test read the prompt as the next chunk —
        // `left: String("zss@zss:~$ "), right: "after\n"` — because the prompt landed between the
        // subscribe ack and this append. That is the MACHINE under test, not the follow. What the
        // follow promises is the next chunk AFTER the last cursor it pushed, and the chunk's text is
        // whatever the session produced next.
        let last = *cursors.last().expect("at least one pushed frame");
        registry
            .push_output_for_test(&session, b"after\n")
            .expect("the ring takes output");
        let frame = tokio::time::timeout(Duration::from_secs(5), next_frame(&mut console))
            .await
            .expect("the follow is still live");
        let cursor = frame["term"]["cursor"]
            .as_u64()
            .unwrap_or_else(|| panic!("a pushed frame names its cursor: {frame}"));
        assert!(
            cursor > last,
            "the follow pushed the chunk after {last}, not one before it: {frame}"
        );
        let data = frame["term"]["data"]
            .as_str()
            .expect("a pushed chunk is text");
        assert!(
            data == "after\n" || !data.is_empty(),
            "the chunk after the last cursor is the session's next output, not empty: {frame}"
        );
        if data != "after\n" {
            // The prompt arrived first. The FOLLOW is still what is being asserted: consume up to a
            // few more frames and require `after\n` among them, in order.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                let frame = tokio::time::timeout_at(deadline, next_frame(&mut console))
                    .await
                    .expect("the follow is still live")
                    .clone();
                if frame["term"]["data"] == "after\n" {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "the appended bytes never arrived: the frames stopped at {frame}"
                );
            }
        }
        let _ = registry.close(&session).await;
    }

    /// The ring's own refusal from a lag cut names a cursor the console can page to and resume
    /// from: the subscription after the cut follows the session again.
    #[tokio::test]
    async fn the_cursor_from_a_lag_cut_resumes_the_subscription() {
        let (server, _config, _dir) = fixture("console-follow-resume");
        let registry = server.terminal_registry();
        let session = registry
            .create(terminal::CreateTarget::Pty { shell: None }, 24, 80)
            .await
            .expect("a local shell");
        let session_id = session.as_str().to_owned();
        let (mut console, _serve) = piped_console(&server, 96 * 1024).await;

        send_frame(&mut console, subscribe_frame(71, 1, &session_id, 0)).await;
        let ack = next_frame(&mut console).await;
        assert_eq!(ack["id"], 71, "the subscribe is acked: {ack}");

        registry
            .push_output_for_test(&session, &vec![b'a'; 512 * 1024])
            .expect("the ring takes output");
        tokio::time::sleep(Duration::from_millis(150)).await;
        registry
            .push_output_for_test(&session, &vec![b'b'; 1024 * 1024])
            .expect("the ring takes output");
        tokio::time::sleep(Duration::from_millis(700)).await;

        // Drain until the follow is cut; the sentence names the oldest byte still retained.
        let mut earliest = None;
        let mut data_frames = 0usize;
        while earliest.is_none() {
            let frame = tokio::time::timeout(Duration::from_secs(5), next_frame(&mut console))
                .await
                .expect("the follow is cut");
            if let Some(term) = frame.get("term") {
                if term["data"].as_str().is_some_and(|data| !data.is_empty()) {
                    data_frames += 1;
                }
                earliest = term
                    .get("error")
                    .and_then(Value::as_str)
                    .and_then(|sentence| sentence.split("which starts at ").nth(1))
                    .and_then(|number| number.parse::<u64>().ok());
            }
        }
        assert!(
            data_frames > 0,
            "the flood reached the pipe before the ring passed it"
        );
        let earliest = earliest.expect("the cut names where the retained output starts");

        // What the console's paged catch-up does: page the ring to its end, then subscribe there.
        let end = registry
            .read(&session, earliest)
            .expect("the cursor the cut reported is still served")
            .cursor;
        send_frame(&mut console, subscribe_frame(72, 2, &session_id, end)).await;
        let resumed = next_frame(&mut console).await;
        assert_eq!(resumed["id"], 72, "the resubscribe is acked: {resumed}");
        assert!(
            resumed.get("error").is_none(),
            "the cursor is served: {resumed}"
        );
        let text = resumed["result"]["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        let answer: Value = serde_json::from_str(text).expect("the ack is the read's own JSON");
        assert_eq!(
            answer["cursor"],
            json!(end),
            "the ack resumes at the cursor it asked from: {answer}"
        );

        registry
            .push_output_for_test(&session, b"resumed-after-cut\n")
            .expect("the ring takes output");
        let mut pushed = String::new();
        while !pushed.contains("resumed-after-cut") {
            let frame = tokio::time::timeout(Duration::from_secs(5), next_frame(&mut console))
                .await
                .expect("the resumed follow pushes");
            if let Some(term) = frame.get("term") {
                pushed.push_str(term["data"].as_str().unwrap_or_default());
            }
        }
        assert!(
            pushed.contains("resumed-after-cut"),
            "the subscription follows again: {pushed:?}"
        );
        let _ = registry.close(&session).await;
    }

    #[test]
    fn utf8_chunks_never_cut_a_character_in_half() {
        // "héllo" as bytes: the é is two bytes; a cut at 2 would split it.
        let text = "héllo";
        let chunks = utf8_chunks(text.as_bytes(), 2);
        let rebuilt: Vec<u8> = chunks
            .iter()
            .flat_map(|chunk| chunk.iter().copied())
            .collect();
        assert_eq!(rebuilt, text.as_bytes(), "every byte survives");
        for chunk in &chunks {
            assert!(
                std::str::from_utf8(chunk).is_ok(),
                "a chunk is valid UTF-8 on its own: {chunk:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_granted_files_read_is_executed_over_the_channel() {
        let (server, config, _dir) = fixture("console-read");
        fs::write(
            config.files_root.join("notes.txt"),
            "hello over the channel",
        )
        .expect("the fixture file is written");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );

        let mut connection = console.next().await;
        assert_eq!(
            connection.uri,
            format!("/devices/{}/channel", device_id()),
            "the device dials the channel its registration names"
        );
        assert_eq!(
            connection.authorization,
            Some(format!("Bearer {}", token())),
            "the registration token is the only credential on the channel"
        );

        send_frame(
            &mut connection.socket,
            json!({
                "id": 7,
                "mcp": read_call(99, "notes.txt"),
                "grant": grant(&["files.read"], now_ms() as u64 + 60_000),
            }),
        )
        .await;

        let reply = next_frame(&mut connection.socket).await;
        assert_eq!(reply["id"], 7, "the frame id is echoed: {reply}");
        assert_eq!(reply["result"]["id"], 99);
        assert_eq!(
            reply["result"]["result"]["content"][0]["text"], "hello over the channel",
            "{reply}"
        );
        let lines = audit_lines(&config);
        assert_eq!(lines[0]["decision"], "allow");
        handle.abort();
    }

    /// A captured frame is one consent: the same signed grant sent twice runs once, and the second
    /// arrival is refused by name. Without the nonce this is the call that ran twice.
    #[tokio::test]
    async fn the_same_signed_frame_twice_runs_once() {
        let (server, config, _dir) = fixture("console-replay");
        fs::write(config.files_root.join("notes.txt"), "read once")
            .expect("the fixture file is written");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;
        let frame = json!({
            "id": 31,
            "mcp": read_call(130, "notes.txt"),
            "grant": grant(&["files.read"], now_ms() as u64 + 60_000),
        });

        send_frame(&mut connection.socket, frame.clone()).await;
        let first = next_frame(&mut connection.socket).await;
        assert_eq!(first["result"]["id"], 130, "{first}");

        send_frame(&mut connection.socket, frame).await;
        let second = next_frame(&mut connection.socket).await;
        assert_eq!(
            second["error"]["error"]["code"], -32001,
            "the replayed frame is refused: {second}"
        );
        assert!(
            second["error"]["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("already been used")),
            "{second}"
        );

        let lines = audit_lines(&config);
        assert_eq!(lines.len(), 4, "two calls, two lines each: {lines:?}");
        assert_eq!(lines[0]["decision"], "allow");
        assert_eq!(lines[1]["ok"], true);
        assert_eq!(lines[2]["decision"], "deny");
        assert_eq!(lines[3]["error_kind"], "replay");
        handle.abort();
    }

    #[tokio::test]
    async fn a_console_call_is_audited_as_the_console_transport() {
        let (server, config, _dir) = fixture("console-audit-transport");
        fs::write(config.files_root.join("notes.txt"), "over the channel")
            .expect("the fixture file is written");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;

        send_frame(
            &mut connection.socket,
            json!({
                "id": 21,
                "mcp": read_call(120, "notes.txt"),
                "grant": grant(&["files.read"], now_ms() as u64 + 60_000),
            }),
        )
        .await;

        let reply = next_frame(&mut connection.socket).await;
        assert_eq!(reply["result"]["id"], 120, "{reply}");
        let lines = audit_lines(&config);
        assert_eq!(lines.len(), 2, "one call is a begin and an end: {lines:?}");
        for line in &lines {
            assert_eq!(
                line["transport"], "console",
                "a console call is not a loopback one and the log must say so: {lines:?}"
            );
            assert!(
                line.get("peer").is_none(),
                "the console WebSocket authenticates the console, not a mesh peer: {lines:?}"
            );
        }
        handle.abort();
    }

    #[tokio::test]
    async fn a_granted_state_get_is_the_console_device_page() {
        let (server, config, _dir) = fixture("console-state-get");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;

        send_frame(
            &mut connection.socket,
            json!({
                "id": 9,
                "mcp": state_call(101),
                "grant": grant(&["state.get"], now_ms() as u64 + 60_000),
            }),
        )
        .await;

        let reply = next_frame(&mut connection.socket).await;
        assert_eq!(reply["id"], 9, "the frame id is echoed: {reply}");
        assert_eq!(reply["result"]["id"], 101);
        assert!(
            reply["result"]["result"]["isError"] != true,
            "the granted read succeeds: {reply}"
        );
        let document: Value = serde_json::from_str(
            reply["result"]["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_else(|| panic!("state.get returns text: {reply}")),
        )
        .expect("state.get returns one JSON document");
        assert_eq!(
            document["state"]["install"]["dir"],
            config.install.display().to_string(),
            "the console reads the same document /api/state would answer: {document}"
        );
        assert_eq!(document["health"]["status"], "ok", "{document}");
        assert_eq!(
            document["health"]["poisoned"], false,
            "a readable document is not poisoned: {document}"
        );
        let lines = audit_lines(&config);
        assert_eq!(lines[0]["decision"], "allow");
        assert_eq!(lines[0]["tool"], "state.get");
        handle.abort();
    }

    #[tokio::test]
    async fn an_ungranted_state_get_is_refused_before_it_runs() {
        let (server, config, _dir) = fixture("console-state-get-outside");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;

        send_frame(
            &mut connection.socket,
            json!({
                "id": 10,
                "mcp": state_call(102),
                "grant": grant(&["files.read"], now_ms() as u64 + 60_000),
            }),
        )
        .await;

        let reply = next_frame(&mut connection.socket).await;
        assert_eq!(reply["id"], 10);
        assert_eq!(
            reply["error"]["error"]["code"], -32001,
            "a tool outside the grant is refused: {reply}"
        );
        let lines = audit_lines(&config);
        assert_eq!(lines[0]["decision"], "deny");
        assert_eq!(lines[0]["tool"], "state.get");
        handle.abort();
    }

    #[tokio::test]
    async fn a_tool_outside_the_grant_is_refused_before_it_runs_and_audited_deny() {
        let (server, config, _dir) = fixture("console-outside");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;

        send_frame(
            &mut connection.socket,
            json!({
                "id": 8,
                "mcp": write_call(100, "denied.txt"),
                "grant": grant(&["files.read"], now_ms() as u64 + 60_000),
            }),
        )
        .await;

        let reply = next_frame(&mut connection.socket).await;
        assert_eq!(reply["id"], 8);
        assert_eq!(reply["error"]["error"]["code"], -32001, "{reply}");
        assert!(
            !config.files_root.join("denied.txt").exists(),
            "a tool outside the grant must not reach the capability"
        );
        let lines = audit_lines(&config);
        assert_eq!(lines[0]["decision"], "deny");
        assert_eq!(lines[0]["tool"], "files.write");
        assert_eq!(lines[1]["ok"], false);
        handle.abort();
    }

    #[tokio::test]
    async fn a_grant_that_consents_to_destruction_runs_files_write() {
        let (server, config, _dir) = fixture("console-destructive-allowed");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;

        send_frame(
            &mut connection.socket,
            json!({
                "id": 12,
                "mcp": write_call_with(104, "consented.txt", "written with consent"),
                "grant": grant_allowing(
                    &["files.write"],
                    now_ms() as u64 + 60_000,
                    true,
                ),
            }),
        )
        .await;

        let reply = next_frame(&mut connection.socket).await;
        assert_eq!(reply["id"], 12);
        assert_eq!(reply["result"]["id"], 104, "{reply}");
        assert_eq!(
            fs::read_to_string(config.files_root.join("consented.txt"))
                .expect("a consented destructive call writes"),
            "written with consent"
        );
        assert_eq!(audit_lines(&config)[0]["decision"], "allow");
        handle.abort();
    }

    #[tokio::test]
    async fn a_grant_without_destructive_consent_says_how_to_mint_one() {
        let (server, config, _dir) = fixture("console-destructive-refused");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;

        send_frame(
            &mut connection.socket,
            json!({
                "id": 13,
                "mcp": write_call(105, "unconsented.txt"),
                "grant": grant_allowing(
                    &["files.write"],
                    now_ms() as u64 + 60_000,
                    false,
                ),
            }),
        )
        .await;

        let reply = next_frame(&mut connection.socket).await;
        assert_eq!(reply["id"], 13);
        assert_eq!(
            reply["error"]["error"]["code"], -32002,
            "an unconsented destructive call asks, it is not a grant-scope denial: {reply}"
        );
        assert!(
            reply["error"]["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("allow_destructive")),
            "the refusal names the fix: {reply}"
        );
        assert!(
            !config.files_root.join("unconsented.txt").exists(),
            "a destructive call without consent must not touch the filesystem"
        );
        let lines = audit_lines(&config);
        assert_eq!(lines[0]["decision"], "ask");
        assert_eq!(lines[1]["error_kind"], "approval_required");
        handle.abort();
    }

    #[tokio::test]
    async fn a_destructive_flag_tampered_after_signing_is_refused() {
        let (server, config, _dir) = fixture("console-destructive-tampered");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;

        let mut envelope = grant_allowing(&["files.write"], now_ms() as u64 + 60_000, true);
        envelope.grant.allow_destructive = false;
        send_frame(
            &mut connection.socket,
            json!({
                "id": 14,
                "mcp": write_call(106, "tampered.txt"),
                "grant": envelope,
            }),
        )
        .await;

        let reply = next_frame(&mut connection.socket).await;
        assert_eq!(reply["id"], 14);
        assert_eq!(reply["error"]["error"]["code"], -32001, "{reply}");
        assert!(
            reply["error"]["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("signature")),
            "{reply}"
        );
        assert!(
            !config.files_root.join("tampered.txt").exists(),
            "a grant whose flag was altered is not a grant"
        );
        handle.abort();
    }

    #[tokio::test]
    async fn an_expired_grant_is_refused() {
        let (server, config, _dir) = fixture("console-expired");
        fs::write(config.files_root.join("notes.txt"), "too late").expect("the fixture is written");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;

        send_frame(
            &mut connection.socket,
            json!({
                "id": 9,
                "mcp": read_call(101, "notes.txt"),
                "grant": grant(&["files.read"], now_ms() as u64 - 10 * 60 * 1_000),
            }),
        )
        .await;

        let reply = next_frame(&mut connection.socket).await;
        assert_eq!(reply["id"], 9);
        assert_eq!(reply["error"]["error"]["code"], -32001, "{reply}");
        assert!(
            reply["error"]["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("expired")),
            "{reply}"
        );
        assert_eq!(audit_lines(&config)[0]["decision"], "deny");
        handle.abort();
    }

    #[tokio::test]
    async fn a_grant_for_another_device_is_refused() {
        let (server, config, _dir) = fixture("console-device-mismatch");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;

        let mut wrong = grant(&["files.read"], now_ms() as u64 + 60_000);
        wrong.grant.device_id = "cd".repeat(32);
        let wrong = signed_by(&signing(), wrong.grant);
        send_frame(
            &mut connection.socket,
            json!({ "id": 10, "mcp": read_call(102, "notes.txt"), "grant": wrong }),
        )
        .await;

        let reply = next_frame(&mut connection.socket).await;
        assert_eq!(reply["id"], 10);
        assert_eq!(reply["error"]["error"]["code"], -32001, "{reply}");
        assert!(
            reply["error"]["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("different device")),
            "{reply}"
        );
        assert_eq!(audit_lines(&config)[0]["decision"], "deny");
        handle.abort();
    }

    #[tokio::test]
    async fn a_grant_signed_by_another_key_is_refused() {
        let (server, config, _dir) = fixture("console-bad-signature");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );
        let mut connection = console.next().await;

        let other = SigningKey::from_bytes(&[0x77; 32]);
        let bad = signed_by(
            &other,
            Grant {
                device_id: device_id(),
                tools: vec!["files.read".to_owned()],
                expires_at_ms: now_ms() as u64 + 60_000,
                allow_destructive: false,
                nonce: String::new(),
            },
        );
        send_frame(
            &mut connection.socket,
            json!({ "id": 11, "mcp": read_call(103, "notes.txt"), "grant": bad }),
        )
        .await;

        let reply = next_frame(&mut connection.socket).await;
        assert_eq!(reply["id"], 11);
        assert_eq!(reply["error"]["error"]["code"], -32001, "{reply}");
        assert!(
            reply["error"]["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("signature")),
            "{reply}"
        );
        assert_eq!(audit_lines(&config)[0]["decision"], "deny");
        handle.abort();
    }

    #[tokio::test]
    async fn the_client_reconnects_after_a_dropped_socket() {
        let (server, _config, _dir) = fixture("console-reconnect");
        let mut console = FakeConsole::start().await;
        let handle = spawn_with_key(
            endpoint(&console.url),
            server.clone(),
            Some(signing().verifying_key()),
        );

        let first = console.next().await;
        assert_eq!(
            first.uri,
            format!("/devices/{}/channel", device_id()),
            "the first connection is the channel"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(200), console.next())
                .await
                .is_err(),
            "the link keeps one connection at a time"
        );

        drop(first);
        let second = tokio::time::timeout(Duration::from_secs(3), console.next())
            .await
            .expect("the device reconnects within the backoff");
        let path = second.uri.split('?').next().unwrap_or_default();
        assert_eq!(
            path,
            format!("/devices/{}/channel", device_id()),
            "the reconnect dials the same channel"
        );
        // And it carries the story of the drop: the console can only learn why a channel died from the
        // attempt that comes after it.
        assert!(
            second.uri.contains("a=1") && second.uri.contains("e="),
            "the reconnect says how the last attempt ended: {}",
            second.uri
        );
        handle.abort();
    }
}
