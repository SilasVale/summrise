//! The MCP stdio session (design §8.2): one plugin child, newline-delimited JSON-RPC 2.0.
//!
//! The session is deliberately generic over its transport: `tokio::io::duplex` in the tests, the
//! child's stdout/stdin in production. That is the whole difference between the two, so the protocol
//! behaviour — ids, timeouts, malformed lines, a peer that dies mid-call — is proven without
//! spawning a process, and production runs the same code the tests ran.
//!
//! One reader task owns the stream's read half and routes answers by id; requests are written under
//! a write lock and wait on a `oneshot` bounded by the caller's timeout. A timeout removes its
//! waiter, so a late answer is logged as unknown rather than delivered to the next request. A peer
//! that closes its stdout fails every waiter with [`SessionError::Exited`] rather than leaving a call
//! blocked until its 120 s deadline: "the plugin died" is an immediate answer, not a hang.
//!
//! Protocol notes and malformed lines go to the plugin's [`LogSink`]; valid stdout is not logged,
//! because it is JSON-RPC the session consumes and a log that repeats every frame is not read.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{oneshot, Mutex as AsyncMutex};

use super::logs::LogSink;

/// The protocol version this device asks for (design §8.2).
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// The largest stdout line the session will buffer. A plugin that writes more is malformed, and the
/// line is consumed and logged rather than kept: the cap is what stops one bad server from choosing
/// the device's memory ceiling.
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// One tool a plugin advertises, before its namespace is prefixed.
///
/// The annotation hints are carried as the plugin wrote them (MCP makes them optional). The default
/// the device applies when they are absent — destructive unless the plugin said otherwise — is
/// computed in one place, [`PluginTool::destructive`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginTool {
    /// The tool's own name, without the plugin's namespace.
    pub name: String,
    /// A human title, when the plugin supplied one.
    pub title: Option<String>,
    /// What the tool does, when the plugin supplied a sentence.
    pub description: Option<String>,
    /// The JSON Schema object the plugin advertises.
    pub input_schema: Value,
    /// MCP's `readOnlyHint`.
    pub read_only: Option<bool>,
    /// MCP's `destructiveHint`.
    pub destructive: Option<bool>,
    /// MCP's `idempotentHint`.
    pub idempotent: Option<bool>,
    /// MCP's `openWorldHint`.
    pub open_world: Option<bool>,
}

impl PluginTool {
    /// Whether calling this tool is destructive, by MCP's defaults (design §15.7):
    /// `!readOnlyHint && destructiveHint.unwrap_or(true)`.
    pub fn destructive(&self) -> bool {
        !self.read_only.unwrap_or(false) && self.destructive.unwrap_or(true)
    }
}

/// One live session with one plugin child.
///
/// The constructor is generic over the transport; the type itself is not, so the supervisor can hold
/// sessions over different transports (a duplex in a test, a child's pipes in production) in one
/// map. The boxes are the type erasure, not a second implementation.
pub struct Session {
    /// The write half, or `None` after [`Session::close`]. The lock is async because a request may
    /// wait on a pipe the plugin is not reading.
    writer: AsyncMutex<Option<Box<dyn AsyncWrite + Unpin + Send>>>,
    /// The answer each outstanding id is waiting for, with the method name for error sentences.
    waiters: Arc<Mutex<HashMap<u64, Waiter>>>,
    /// The next id to use. MCP ids only have to be unique per connection, and one session is one
    /// connection.
    next_id: AtomicU64,
    /// The protocol notes and the child's replies' malformed leftovers.
    log: LogSink,
    /// Whether the peer has closed its stdout or the session was closed locally.
    closed: Arc<AtomicBool>,
    /// The reader task. Dropping the session aborts it; the stream halves drop with it.
    reader: tokio::task::JoinHandle<()>,
}

/// One outstanding request.
struct Waiter {
    /// The method, so an error answer and a timeout can name what was not answered.
    method: String,
    /// Where the answer goes. Dropping the sender is how the reader says "the peer exited".
    answer: oneshot::Sender<Result<Value, SessionError>>,
}

/// Why one request did not produce a result.
///
/// The supervisor adds the plugin's namespace when it turns this into the sentence a caller sees; the
/// session knows the method and the stream, not which plugin owns them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionError {
    /// No answer arrived inside the caller's deadline. The waiter was removed.
    Timeout {
        /// The method that was not answered.
        method: String,
        /// The deadline the caller gave.
        timeout: Duration,
    },
    /// The plugin answered with a JSON-RPC error object.
    Rpc {
        /// The method that was answered.
        method: String,
        /// The error's code, as the plugin wrote it.
        code: i64,
        /// The error's message, as the plugin wrote it.
        message: String,
    },
    /// A `tools/call` result carried `isError: true`; the text is the plugin's own sentence.
    Tool {
        /// The concatenated text blocks, or the raw result when it was not text.
        text: String,
    },
    /// The peer closed its stdout, or the transport failed, while the session was live.
    Exited,
    /// The session was closed locally before the request could be written.
    Closed {
        /// The method that was not sent.
        method: String,
    },
    /// The plugin's answer was JSON but not the shape MCP requires.
    Protocol(String),
    /// Writing the request failed.
    Io(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Timeout { method, timeout } => write!(
                f,
                "{method} did not answer within {} s",
                timeout.as_secs_f64()
            ),
            SessionError::Rpc {
                method,
                code,
                message,
            } => write!(f, "{method} answered JSON-RPC error {code}: {message}"),
            SessionError::Tool { text } => write!(f, "{text}"),
            SessionError::Exited => write!(f, "the plugin process exited"),
            SessionError::Closed { method } => {
                write!(f, "{method} was not sent: the plugin connection is closed")
            }
            SessionError::Protocol(detail) => {
                write!(f, "the plugin spoke the wrong protocol: {detail}")
            }
            SessionError::Io(detail) => write!(f, "the plugin connection failed: {detail}"),
        }
    }
}

impl Session {
    /// Start a session over `stream`, with one reader task routing answers.
    ///
    /// The stream is any duplex transport: `tokio::io::duplex` in tests, a child's joined stdio in
    /// production. It is split into halves; the reader may outlive a caller's timeout, which is what
    /// lets a late answer be logged and discarded.
    pub fn new<S>(stream: S, log: LogSink) -> Session
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (read, write) = tokio::io::split(stream);
        let waiters: Arc<Mutex<HashMap<u64, Waiter>>> = Arc::new(Mutex::new(HashMap::new()));
        let closed = Arc::new(AtomicBool::new(false));

        let reader_waiters = Arc::clone(&waiters);
        let reader_closed = Arc::clone(&closed);
        let reader_log = log.clone();
        let reader = tokio::spawn(async move {
            read_loop(
                BufReader::new(read),
                reader_waiters,
                reader_closed,
                reader_log,
            )
            .await;
        });

        Session {
            writer: AsyncMutex::new(Some(Box::new(write))),
            waiters,
            next_id: AtomicU64::new(1),
            log,
            closed,
            reader,
        }
    }

    /// Whether the peer is gone (or the session was closed). The supervisor reads this instead of
    /// waiting for a call to discover it.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// The MCP handshake: `initialize`, read the negotiated version, then `notifications/initialized`.
    ///
    /// The version the plugin answers is returned and logged, not compared against this build's: the
    /// spec accepts an older server if it answers `initialize`, and the negotiated fact is what the
    /// operator needs when a plugin misbehaves.
    pub async fn initialize(&self, timeout: Duration) -> Result<String, SessionError> {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {
                        "name": "summrise",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                }),
                timeout,
            )
            .await?;
        let version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                SessionError::Protocol(
                    "initialize answered without a protocolVersion string".to_owned(),
                )
            })?
            .to_owned();
        self.notify("notifications/initialized", json!({})).await?;
        self.log
            .line(format!("summrise: negotiated MCP protocol {version}"));
        Ok(version)
    }

    /// Read the plugin's catalogue.
    pub async fn list_tools(&self, timeout: Duration) -> Result<Vec<PluginTool>, SessionError> {
        let result = self.request("tools/list", json!({}), timeout).await?;
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                SessionError::Protocol("tools/list answered without a tools array".to_owned())
            })?;
        tools.iter().map(parse_tool).collect()
    }

    /// Call one plugin tool by its own (un-namespaced) name.
    ///
    /// Text content is concatenated for the device's text result; any other content (an image, a
    /// resource) is returned as the raw JSON so a client can still read it. `isError: true` is a
    /// tool-level failure rather than a successful text, so the device does not report success for a
    /// plugin's refusal.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
        timeout: Duration,
    ) -> Result<String, SessionError> {
        let result = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
                timeout,
            )
            .await?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(SessionError::Tool {
                text: text_of(&result),
            });
        }
        let Some(content) = result.get("content").and_then(Value::as_array) else {
            return Ok(result.to_string());
        };
        let mut texts = Vec::new();
        for block in content {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => match block.get("text").and_then(Value::as_str) {
                    Some(text) => texts.push(text.to_owned()),
                    None => return Ok(result.to_string()),
                },
                _ => return Ok(result.to_string()),
            }
        }
        Ok(texts.join("\n"))
    }

    /// The health check. A plugin that answers `ping` is alive; the supervisor is the one that counts
    /// misses and restarts.
    pub async fn ping(&self, timeout: Duration) -> Result<(), SessionError> {
        self.request("ping", json!({}), timeout).await.map(|_| ())
    }

    /// Close the child's stdin and wait `grace` for it to exit on its own.
    ///
    /// The child process is the launcher's, not the session's: the supervisor kills it after this
    /// returns if it is still there. Closing stdin is the polite half, and a plugin that reads until
    /// EOF exits here.
    pub async fn shutdown(&self, grace: Duration) {
        self.close().await;
        tokio::time::sleep(grace).await;
    }

    /// Close the write half, which closes the child's stdin once the write half drops.
    pub async fn close(&self) {
        self.closed.store(true, Ordering::Release);
        let mut writer = self.writer.lock().await;
        if let Some(mut writer) = writer.take() {
            // A pipe whose reader is gone fails this immediately; a peer that is still there sees
            // EOF when the half is dropped. Either answer is fine — the kill follows.
            let _ = tokio::time::timeout(Duration::from_secs(1), writer.shutdown()).await;
        }
    }

    /// Send one request and wait for its answer, bounded by `timeout`.
    async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, SessionError> {
        if self.is_closed() {
            return Err(SessionError::Closed {
                method: method.to_owned(),
            });
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (answer, received) = oneshot::channel();
        self.waiters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                id,
                Waiter {
                    method: method.to_owned(),
                    answer,
                },
            );

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let mut bytes = match serde_json::to_vec(&request) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.forget(id);
                return Err(SessionError::Protocol(format!(
                    "the request could not be encoded: {error}"
                )));
            }
        };
        bytes.push(b'\n');
        if let Err(error) = self.send(&bytes).await {
            self.forget(id);
            return Err(error);
        }

        match tokio::time::timeout(timeout, received).await {
            Ok(Ok(answer)) => answer,
            // The reader dropped the sender: the peer is gone and every waiter was failed.
            Ok(Err(_)) => Err(SessionError::Exited),
            Err(_) => {
                self.forget(id);
                Err(SessionError::Timeout {
                    method: method.to_owned(),
                    timeout,
                })
            }
        }
    }

    /// Send one notification: no id, and no answer is expected.
    async fn notify(&self, method: &str, params: Value) -> Result<(), SessionError> {
        if self.is_closed() {
            return Err(SessionError::Closed {
                method: method.to_owned(),
            });
        }
        let message = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let mut bytes = serde_json::to_vec(&message)
            .map_err(|error| SessionError::Protocol(error.to_string()))?;
        bytes.push(b'\n');
        self.send(&bytes).await
    }

    /// Write bytes and flush; the newline is the frame boundary.
    async fn send(&self, bytes: &[u8]) -> Result<(), SessionError> {
        let mut writer = self.writer.lock().await;
        let Some(writer) = writer.as_mut() else {
            return Err(SessionError::Exited);
        };
        writer
            .write_all(bytes)
            .await
            .map_err(|error| SessionError::Io(error.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|error| SessionError::Io(error.to_string()))
    }

    /// Remove a waiter that will never be answered by this request.
    fn forget(&self, id: u64) {
        self.waiters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // The supervisor closes the session before dropping it; this is the last resort for a
        // session dropped in a panic or a test. It never blocks.
        self.reader.abort();
    }
}

/// One line from the peer's stdout, or the fact that it was too long to keep.
enum ReadLine {
    Text(String),
    Oversized,
}

/// Read one newline-terminated line with a cap, consuming the whole line either way.
///
/// `read_line` would grow the buffer for as long as a malformed peer kept writing; this reads through
/// the line to its newline but stops retaining bytes at [`MAX_LINE_BYTES`], so the next frame is still
/// aligned and the memory ceiling is the device's.
async fn read_line_bounded<R>(
    reader: &mut BufReader<R>,
    line: &mut Vec<u8>,
) -> std::io::Result<Option<ReadLine>>
where
    R: AsyncRead + Unpin,
{
    line.clear();
    let mut oversized = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if line.is_empty() && !oversized {
                return Ok(None);
            }
            return Ok(Some(if oversized {
                ReadLine::Oversized
            } else {
                ReadLine::Text(String::from_utf8_lossy(line).into_owned())
            }));
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.unwrap_or(available.len());
        if !oversized {
            if line.len() + take > MAX_LINE_BYTES {
                oversized = true;
                line.clear();
            } else {
                line.extend_from_slice(&available[..take]);
            }
        }
        let consumed = newline.map_or(available.len(), |index| index + 1);
        reader.consume(consumed);
        if newline.is_some() {
            return Ok(Some(if oversized {
                ReadLine::Oversized
            } else {
                ReadLine::Text(String::from_utf8_lossy(line).into_owned())
            }));
        }
    }
}

/// The reader task: route answers by id, log everything that is not an answer.
async fn read_loop<R>(
    mut reader: BufReader<R>,
    waiters: Arc<Mutex<HashMap<u64, Waiter>>>,
    closed: Arc<AtomicBool>,
    log: LogSink,
) where
    R: AsyncRead + Unpin,
{
    let mut line = Vec::new();
    loop {
        match read_line_bounded(&mut reader, &mut line).await {
            Ok(None) => break,
            Ok(Some(ReadLine::Oversized)) => log.line(format!(
                "summrise: skipped an stdout line longer than {MAX_LINE_BYTES} bytes"
            )),
            Ok(Some(ReadLine::Text(text))) => {
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                let message: Value = match serde_json::from_str(text) {
                    Ok(message) => message,
                    Err(error) => {
                        log.line(format!(
                            "summrise: skipped a malformed stdout line: {error}"
                        ));
                        continue;
                    }
                };
                route(&message, &waiters, &log);
            }
            Err(error) => {
                log.line(format!(
                    "summrise: reading the plugin's stdout failed: {error}"
                ));
                break;
            }
        }
    }

    // The peer is gone. Fail every waiter now: a call in flight is a sentence, not a 120 s hang.
    closed.store(true, Ordering::Release);
    let waiters = waiters
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .drain()
        .collect::<Vec<_>>();
    if !waiters.is_empty() {
        log.line(format!(
            "summrise: the plugin's stdout closed with {} request(s) in flight",
            waiters.len()
        ));
    }
    for (_, waiter) in waiters {
        let _ = waiter.answer.send(Err(SessionError::Exited));
    }
}

/// Route one decoded stdout message.
fn route(message: &Value, waiters: &Arc<Mutex<HashMap<u64, Waiter>>>, log: &LogSink) {
    let id = message.get("id").and_then(Value::as_u64);
    let method = message.get("method").and_then(Value::as_str);
    match (id, method) {
        (Some(id), None) => {
            let waiter = waiters
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&id);
            let Some(waiter) = waiter else {
                log.line(format!(
                    "summrise: ignored an answer for id {id}, which nobody was waiting for"
                ));
                return;
            };
            let answer = match (message.get("error"), message.get("result")) {
                (Some(error), _) => Err(SessionError::Rpc {
                    method: waiter.method,
                    code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                    message: error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("no message")
                        .to_owned(),
                }),
                (None, Some(result)) => Ok(result.clone()),
                (None, None) => Err(SessionError::Protocol(
                    "a response carried neither result nor error".to_owned(),
                )),
            };
            let _ = waiter.answer.send(answer);
        }
        // A notification is the plugin talking when nobody asked; MCP allows it. Logging it is all
        // the device promises, and the next answer on the stream is unaffected.
        (None, Some(method)) => log.line(format!("summrise: ignored a notification: {method}")),
        // A server-to-client request would need a capability this client never advertised. Log it,
        // but do not answer: the plugin should not have sent it, and answering would pretend the
        // device implements a feature it does not.
        (Some(id), Some(method)) => log.line(format!(
            "summrise: ignored a request from the plugin: {method} (id {id})"
        )),
        (None, None) => log.line("summrise: ignored an stdout line with neither id nor method"),
    }
}

/// The text of a `tools/call` result: text blocks concatenated, or the raw JSON for any other shape.
fn text_of(result: &Value) -> String {
    let Some(content) = result.get("content").and_then(Value::as_array) else {
        return result.to_string();
    };
    let mut texts = Vec::new();
    for block in content {
        match block.get("text").and_then(Value::as_str) {
            Some(text) => texts.push(text.to_owned()),
            None => return result.to_string(),
        }
    }
    texts.join("\n")
}

/// Parse one entry of `tools/list`.
fn parse_tool(tool: &Value) -> Result<PluginTool, SessionError> {
    let name = tool
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| SessionError::Protocol("a plugin tool has no name".to_owned()))?
        .to_owned();
    let input_schema = tool
        .get("inputSchema")
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object" }));
    if !input_schema.is_object() {
        return Err(SessionError::Protocol(format!(
            "tool {name} has a non-object inputSchema"
        )));
    }
    let annotations = tool.get("annotations");
    let hint = |key: &str| {
        annotations
            .and_then(|annotations| annotations.get(key))
            .and_then(Value::as_bool)
    };
    Ok(PluginTool {
        name,
        title: tool.get("title").and_then(Value::as_str).map(str::to_owned),
        description: tool
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        input_schema,
        read_only: hint("readOnlyHint"),
        destructive: hint("destructiveHint"),
        idempotent: hint("idempotentHint"),
        open_world: hint("openWorldHint"),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    use serde_json::json;
    use tokio::io::{duplex, AsyncWriteExt, BufReader};

    use super::*;

    /// A duplex peer: answers each request with whatever `answer` returns.
    ///
    /// The script is a closure rather than a canned sequence so a test can prove the request the
    /// session actually sent (the id, the method, the arguments) and not just its result.
    struct Peer {
        session: Session,
        requests: Arc<Mutex<Vec<Value>>>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for Peer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    fn peer(answer: impl Fn(&Value) -> Option<Value> + Send + 'static) -> Peer {
        let (session_side, peer_side) = duplex(64 * 1024);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        let log_lines = Arc::new(Mutex::new(Vec::new()));
        let session = Session::new(session_side, LogSink::to_memory(Arc::clone(&log_lines)));

        let task = tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(peer_side);
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
                    continue;
                };
                if let Ok(mut seen) = seen.lock() {
                    seen.push(message.clone());
                }
                if message.get("id").is_none() {
                    continue;
                }
                if let Some(response) = answer(&message) {
                    let bytes = format!("{response}\n");
                    if write.write_all(bytes.as_bytes()).await.is_err() {
                        break;
                    }
                }
            }
        });

        Peer {
            session,
            requests,
            task,
        }
    }

    fn reply(request: &Value, result: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": request["id"], "result": result })
    }

    #[tokio::test]
    async fn initialize_list_and_call_round_trip_through_a_duplex() {
        let peer = peer(|request| match request["method"].as_str() {
            Some("initialize") => Some(reply(
                request,
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "hello", "version": "0.1.0" }
                }),
            )),
            Some("tools/list") => Some(reply(
                request,
                json!({
                    "tools": [{
                        "name": "greet",
                        "title": "Greet someone",
                        "description": "Say hello to a name.",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "name": { "type": "string" } },
                            "required": ["name"]
                        },
                        "annotations": {
                            "readOnlyHint": true,
                            "destructiveHint": false,
                            "idempotentHint": true,
                            "openWorldHint": false
                        }
                    }]
                }),
            )),
            Some("tools/call") => Some(reply(
                request,
                json!({
                    "content": [{ "type": "text", "text": "hi Bob" }],
                    "isError": false
                }),
            )),
            _ => None,
        });

        let version = peer
            .session
            .initialize(Duration::from_secs(1))
            .await
            .expect("the handshake answers");
        assert_eq!(version, PROTOCOL_VERSION);

        let tools = peer
            .session
            .list_tools(Duration::from_secs(1))
            .await
            .expect("the catalogue answers");
        assert_eq!(tools.len(), 1);
        let tool = &tools[0];
        assert_eq!(
            tool.name, "greet",
            "the name is the plugin's own, un-namespaced"
        );
        assert_eq!(tool.title.as_deref(), Some("Greet someone"));
        assert_eq!(tool.description.as_deref(), Some("Say hello to a name."));
        assert_eq!(tool.input_schema["required"], json!(["name"]));
        assert_eq!(tool.read_only, Some(true));
        assert_eq!(tool.destructive, Some(false));
        assert_eq!(tool.idempotent, Some(true));
        assert_eq!(tool.open_world, Some(false));
        assert!(!tool.destructive(), "the plugin said it is read-only");

        let text = peer
            .session
            .call_tool("greet", json!({ "name": "Bob" }), Duration::from_secs(1))
            .await
            .expect("the call answers");
        assert_eq!(text, "hi Bob");

        let seen = peer.requests.lock().unwrap().clone();
        let methods: Vec<&str> = seen
            .iter()
            .map(|message| message["method"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(
            methods,
            [
                "initialize",
                "notifications/initialized",
                "tools/list",
                "tools/call"
            ],
            "the handshake sends initialized before the catalogue"
        );
        let call = seen.last().expect("the call was seen");
        assert_eq!(call["params"]["name"], "greet");
        assert_eq!(call["params"]["arguments"], json!({ "name": "Bob" }));
    }

    #[tokio::test]
    async fn a_malformed_line_and_a_notification_do_not_wedge_the_next_answer() {
        let (session_side, peer_side) = duplex(64 * 1024);
        let log_lines = Arc::new(Mutex::new(Vec::new()));
        let session = Session::new(session_side, LogSink::to_memory(Arc::clone(&log_lines)));

        let peer_task = tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(peer_side);
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("one request");
            let request: Value = serde_json::from_str(line.trim()).expect("the request is JSON");
            // Garbage, a notification, then the real answer: the session must skip the first two and
            // still resolve the waiter.
            write
                .write_all(b"{ not json\n")
                .await
                .expect("the malformed line is written");
            write
                .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n")
                .await
                .expect("the notification is written");
            let answer = format!(
                "{}\n",
                json!({ "jsonrpc": "2.0", "id": request["id"], "result": {} })
            );
            write
                .write_all(answer.as_bytes())
                .await
                .expect("the answer");
        });

        assert!(
            session.ping(Duration::from_secs(1)).await.is_ok(),
            "the valid answer after the noise resolves the request"
        );
        peer_task.await.expect("the peer ran");

        let log = log_lines.lock().unwrap().clone();
        assert!(
            log.iter()
                .any(|line| line.contains("malformed stdout line")),
            "the malformed line is logged: {log:?}"
        );
        assert!(
            log.iter()
                .any(|line| line.contains("ignored a notification")),
            "the notification is logged: {log:?}"
        );
    }

    #[tokio::test]
    async fn a_timeout_removes_its_waiter_and_the_next_request_still_works() {
        let (session_side, peer_side) = duplex(64 * 1024);
        let session = Session::new(
            session_side,
            LogSink::to_memory(Arc::new(Mutex::new(Vec::new()))),
        );
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&seen);

        let peer_task = tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(peer_side);
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let request: Value = serde_json::from_str(line.trim()).expect("a request");
                let index = counter.fetch_add(1, Ordering::SeqCst);
                if index == 0 {
                    // Do not answer the first request at all.
                    continue;
                }
                let answer = format!(
                    "{}\n",
                    json!({ "jsonrpc": "2.0", "id": request["id"], "result": {} })
                );
                write
                    .write_all(answer.as_bytes())
                    .await
                    .expect("the answer");
            }
        });

        let error = session
            .ping(Duration::from_millis(50))
            .await
            .expect_err("the first request is not answered");
        assert!(
            matches!(error, SessionError::Timeout { ref method, .. } if method == "ping"),
            "{error:?}"
        );

        session
            .ping(Duration::from_secs(1))
            .await
            .expect("the session is still usable after a timeout");

        peer_task.abort();
        assert_eq!(seen.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_peer_that_dies_mid_call_fails_it_promptly() {
        let (session_side, peer_side) = duplex(64 * 1024);
        let session = Session::new(
            session_side,
            LogSink::to_memory(Arc::new(Mutex::new(Vec::new()))),
        );
        let peer_task = tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(peer_side);
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("one request");
            // Drop the transport without answering: the call must not hang for its deadline.
            let _ = write.write_all(b"").await;
        });

        let started = Instant::now();
        let error = session
            .call_tool("greet", json!({}), Duration::from_secs(30))
            .await
            .expect_err("the peer closed rather than answering");
        assert!(matches!(error, SessionError::Exited), "{error:?}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "a dead peer fails the call now, not at the 30 s deadline: {:?}",
            started.elapsed()
        );

        peer_task.abort();
        assert!(session.is_closed(), "the session learns the peer is gone");
    }

    #[tokio::test]
    async fn an_older_protocol_version_is_accepted_and_reported() {
        let peer = peer(|request| {
            (request["method"] == "initialize").then(|| {
                reply(
                    request,
                    json!({ "protocolVersion": "2024-11-05", "capabilities": {} }),
                )
            })
        });

        let version = peer
            .session
            .initialize(Duration::from_secs(1))
            .await
            .expect("an older server that answers initialize is accepted");
        assert_eq!(version, "2024-11-05");
    }

    #[tokio::test]
    async fn a_tool_error_result_is_an_error_and_non_text_content_is_raw_json() {
        let peer = peer(|request| match request["method"].as_str() {
            Some("tools/call") => match request["params"]["name"].as_str() {
                Some("fail") => Some(reply(
                    request,
                    json!({
                        "content": [{ "type": "text", "text": "nope" }],
                        "isError": true
                    }),
                )),
                Some("image") => Some(reply(
                    request,
                    json!({
                        "content": [{ "type": "image", "data": "AAAA", "mimeType": "image/png" }]
                    }),
                )),
                _ => None,
            },
            _ => None,
        });

        let error = peer
            .session
            .call_tool("fail", json!({}), Duration::from_secs(1))
            .await
            .expect_err("isError is a failure, not text");
        assert_eq!(
            error,
            SessionError::Tool {
                text: "nope".to_owned()
            }
        );

        let text = peer
            .session
            .call_tool("image", json!({}), Duration::from_secs(1))
            .await
            .expect("an image result is still a result");
        assert!(
            text.contains("\"type\":\"image\""),
            "non-text content is the raw JSON: {text}"
        );
    }

    #[tokio::test]
    async fn a_tools_list_without_a_tools_array_is_a_protocol_error() {
        let peer = peer(|request| {
            (request["method"] == "tools/list").then(|| reply(request, json!({ "notTools": [] })))
        });

        let error = peer
            .session
            .list_tools(Duration::from_secs(1))
            .await
            .expect_err("a catalogue without tools is not an empty catalogue");
        assert!(matches!(error, SessionError::Protocol(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_tool_without_a_name_is_a_protocol_error() {
        let peer = peer(|request| {
            (request["method"] == "tools/list").then(|| {
                reply(
                    request,
                    json!({ "tools": [{ "description": "nameless", "inputSchema": {} }] }),
                )
            })
        });

        let error = peer
            .session
            .list_tools(Duration::from_secs(1))
            .await
            .expect_err("a tool nobody can call is not a tool");
        assert!(matches!(error, SessionError::Protocol(_)), "{error:?}");
    }

    #[tokio::test]
    async fn annotations_that_are_absent_are_destructive_by_the_mcp_default() {
        let peer = peer(|request| {
            (request["method"] == "tools/list").then(|| {
                reply(
                    request,
                    json!({
                        "tools": [{ "name": "run", "inputSchema": { "type": "object" } }]
                    }),
                )
            })
        });

        let tools = peer
            .session
            .list_tools(Duration::from_secs(1))
            .await
            .expect("the catalogue answers");
        assert!(
            tools[0].destructive(),
            "an unannotated tool is destructive: the default is a denial, not a permission"
        );
    }
}
