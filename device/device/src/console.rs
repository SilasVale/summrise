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
        let channel_url = channel_url(url, &device_id)?;
        Ok(Some(ConsoleEndpoint {
            channel_url,
            device_id,
            token,
        }))
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
fn channel_url(base: &str, device_id: &str) -> Result<String, ConsoleError> {
    let base = base.trim().trim_end_matches('/');
    let (scheme, rest) = base
        .split_once("://")
        .ok_or_else(|| ConsoleError::BadUrl(base.to_owned()))?;
    let scheme = match scheme.to_ascii_lowercase().as_str() {
        "ws" | "http" => "ws",
        "wss" | "https" => "wss",
        _ => return Err(ConsoleError::BadUrl(base.to_owned())),
    };
    if rest.is_empty()
        || rest.contains(char::is_whitespace)
        || rest.contains('?')
        || rest.contains('#')
    {
        return Err(ConsoleError::BadUrl(base.to_owned()));
    }
    // `?v=` tells the console which build is on the other end, so the operator's device list can name
    // it without asking the device for a grant first. A console that ignores the parameter answers
    // exactly as before: it is a query string on a route both sides already share.
    Ok(format!(
        "{scheme}://{rest}/devices/{device_id}/channel?v={}",
        env!("CARGO_PKG_VERSION")
    ))
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
    loop {
        match connect(&endpoint).await {
            Ok(socket) => {
                // A connection that opened is a success, but a connection that never served a
                // frame is not: a console that accepts and immediately drops must back off like
                // any other failure instead of being retried once a second forever.
                if serve(socket, &endpoint, &server, &key).await {
                    attempt = 1;
                }
            }
            Err(error) => {
                eprintln!(
                    "summrise-device: cannot reach the console at {}: {error}",
                    endpoint.channel_url
                );
            }
        }

        let delay = summrise_core::mesh::backoff(attempt);
        tokio::time::sleep(Duration::from_millis(delay)).await;
        attempt = attempt.saturating_add(1);
    }
}

/// One authenticated WebSocket.
async fn connect(
    endpoint: &ConsoleEndpoint,
) -> Result<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, String> {
    let uri = endpoint
        .channel_url
        .parse()
        .map_err(|error| format!("the channel URL is not a URI: {error}"))?;
    let request = ClientRequestBuilder::new(uri)
        .with_header("Authorization", format!("Bearer {}", endpoint.token));
    let connect = connect_async(request);
    match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Err(_) => Err(format!(
            "the handshake did not finish inside {} ms",
            CONNECT_TIMEOUT.as_millis()
        )),
        Ok(Err(error)) => Err(error.to_string()),
        Ok(Ok((socket, _response))) => Ok(socket),
    }
}

/// Serve one socket until it closes, then return whether it carried any frame.
async fn serve(
    mut socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    endpoint: &ConsoleEndpoint,
    server: &DeviceServer,
    key: &VerifyingKey,
) -> bool {
    let mut served = false;
    while let Some(message) = socket.next().await {
        match message {
            Ok(Message::Text(text)) => {
                served = true;
                if let Some(reply) = handle_frame(text.as_str(), endpoint, server, key).await {
                    if let Err(error) = socket.send(Message::text(reply.to_string())).await {
                        eprintln!("summrise-device: the console channel write failed: {error}");
                        break;
                    }
                }
            }
            Ok(Message::Ping(payload)) => {
                let _ = socket.send(Message::Pong(payload)).await;
            }
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => {}
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

    let envelope = match remote::admit(&frame, &endpoint.device_id, key, crate::http::now_ms()) {
        Ok(envelope) => envelope,
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

    let response = server
        .handle(
            &frame.mcp,
            Caller::Granted {
                grant: &envelope.grant,
                arrival: Arrival::Console,
                peer: None,
            },
        )
        .await;
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
            console_url: Some("ws://unused".to_owned()),
            console_token_file: Some(dir.join("console")),
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

    async fn next_frame(socket: &mut WebSocketStream<tokio::net::TcpStream>) -> Value {
        loop {
            match socket
                .next()
                .await
                .expect("the socket is open")
                .expect("a frame")
            {
                WsMessage::Text(text) => {
                    return serde_json::from_str(text.as_str()).expect("the frame is JSON")
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
            let expected = expected.replace("0.0.0-test", env!("CARGO_PKG_VERSION"));
            assert_eq!(channel_url(base, "ab").expect(base), expected, "{base}");
        }

        for bad in [
            "",
            "console.example.test",
            "ftp://host",
            "ws://",
            "ws://host/a b",
        ] {
            assert!(channel_url(bad, "ab").is_err(), "{bad:?} must be refused");
        }
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
            console_url: Some("https://console.example.test".to_owned()),
            console_token_file: Some(token_file),
            mesh_broker: None,
            mesh_relay: None,
            mesh_stun: None,
        };

        let loaded = ConsoleEndpoint::from_config(&config)
            .expect("the registration file loads")
            .expect("both console facts are present");
        assert_eq!(loaded.device_id, device_id());
        assert_eq!(loaded.token, token());
        assert_eq!(
            loaded.channel_url,
            format!(
                "wss://console.example.test/devices/{}/channel?v={}",
                device_id(),
                env!("CARGO_PKG_VERSION")
            )
        );

        config.console_url = None;
        config.console_token_file = None;
        assert!(
            matches!(ConsoleEndpoint::from_config(&config), Ok(None)),
            "no console facts means the feature is off, not an error"
        );
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
        assert_eq!(
            second.uri,
            format!("/devices/{}/channel", device_id()),
            "the reconnect dials the same channel"
        );
        handle.abort();
    }
}
