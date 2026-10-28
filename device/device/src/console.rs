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

use std::fs;
use std::io;
use std::path::PathBuf;
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
    match pinned_console_key() {
        Ok(key) => {
            eprintln!(
                "summrise-device: opening the console channel at {}",
                endpoint.channel_url
            );
            spawn_with_key(endpoint, server, key)
        }
        Err(error) => {
            eprintln!("summrise-device: the console link is disabled: {error}");
            tokio::spawn(async {})
        }
    }
}

/// The testable half of [`spawn`]: one key, one endpoint, one handler.
pub(crate) fn spawn_with_key(
    endpoint: ConsoleEndpoint,
    server: DeviceServer,
    key: VerifyingKey,
) -> JoinHandle<()> {
    tokio::spawn(async move { run(endpoint, server, key).await })
}

/// Connect forever: one socket at a time, with a 1 s → 60 s backoff between attempts.
async fn run(endpoint: ConsoleEndpoint, server: DeviceServer, key: VerifyingKey) {
    let mut attempt: u8 = 1;
    let mut note: Option<RetryNote> = None;
    loop {
        record_attempt(&note);
        match connect(&endpoint, note.as_ref()).await {
            Ok(socket) => {
                // A connection that opened is a success, but a connection that never served a
                // frame is not: a console that accepts and immediately drops must back off like
                // any other failure instead of being retried once a second forever.
                if serve(socket, &endpoint, &server, &key).await {
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

/// How many finished answers may wait for the socket before a task waits for the loop.
const OUTGOING_FRAMES: usize = 64;

/// Serve one socket until it closes, then return whether it carried any frame.
async fn serve(
    mut socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    endpoint: &ConsoleEndpoint,
    server: &DeviceServer,
    key: &VerifyingKey,
) -> bool {
    let mut served = false;
    // The host report rides the same socket as the request/answer traffic: one frame on connect (the
    // first tick fires immediately) and one a minute after. It is unsolicited, which is the point —
    // the console knows what the machine is without asking for a grant.
    let mut host = tokio::time::interval(std::time::Duration::from_secs(60));
    host.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // A frame is answered by its own task, and the answer comes back here: the socket has one owner,
    // so a ping is still answered while a tool is running, and one slow tool cannot stop the channel.
    let (outgoing, mut answers) = tokio::sync::mpsc::channel::<Value>(OUTGOING_FRAMES);
    let in_flight = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_IN_FLIGHT_FRAMES));
    loop {
        tokio::select! {
            _ = host.tick() => {
                if let Err(error) = socket.send(Message::text(crate::host::frame())).await {
                    eprintln!("summrise-device: the host report could not be sent: {error}");
                    break;
                }
            }
            answer = answers.recv() => {
                // `outgoing` is held by this loop, so the channel closes only when the loop does.
                let Some(answer) = answer else {
                    break;
                };
                if let Err(error) = socket.send(Message::text(answer.to_string())).await {
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
                        let endpoint = endpoint.clone();
                        let server = server.clone();
                        let key = *key;
                        let outgoing = outgoing.clone();
                        let in_flight = in_flight.clone();
                        tokio::spawn(async move {
                            // The permit is taken here, not in the read loop: a saturated channel
                            // parks the task, and pings and the host report keep flowing.
                            let Ok(_permit) = in_flight.acquire_owned().await else {
                                return;
                            };
                            if let Some(reply) =
                                handle_frame(text.as_str(), &endpoint, &server, &key).await
                            {
                                let _ = outgoing.send(reply).await;
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
    served
}

/// Handle one socket frame; `None` means the frame was malformed and cannot be correlated.
async fn handle_frame(
    text: &str,
    endpoint: &ConsoleEndpoint,
    server: &DeviceServer,
    key: &VerifyingKey,
) -> Option<Value> {
    let frame: RemoteFrame = match serde_json::from_str(text) {
        Ok(frame) => frame,
        Err(error) => {
            eprintln!("summrise-device: the console sent a malformed frame: {error}");
            return None;
        }
    };

    let request_id = frame.mcp.get("id").cloned().unwrap_or(Value::Null);
    let tool = remote::requested_tool(&frame.mcp).map(str::to_owned);

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
    let response = server.handle(&frame.mcp, caller).await;
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

    async fn send_frame(socket: &mut WebSocketStream<tokio::net::TcpStream>, frame: Value) {
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
    async fn next_frame(socket: &mut WebSocketStream<tokio::net::TcpStream>) -> Value {
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
            signing().verifying_key(),
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
            signing().verifying_key(),
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
            signing().verifying_key(),
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
            signing().verifying_key(),
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
            signing().verifying_key(),
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
            signing().verifying_key(),
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
            signing().verifying_key(),
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
            signing().verifying_key(),
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
            signing().verifying_key(),
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
            signing().verifying_key(),
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
            signing().verifying_key(),
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
            signing().verifying_key(),
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
            signing().verifying_key(),
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
            signing().verifying_key(),
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
