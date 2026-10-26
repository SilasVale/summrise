//! The MCP surface (design §4.5): the capability registry, spoken over streamable HTTP.
//!
//! The endpoint is mounted at `/mcp` behind the install token (the same `Authorization: Bearer`
//! check as `/api/state`), and it has no tool list of its own: `tools/list` is a reading of
//! `summrise_core::registry::CAPABILITIES`, so a tool that no capability declares is not advertised.
//!
//! Every `tools/call` walks the same path the HTTP authorization endpoint walks: resolve the tool in
//! the registry (absent → deny), build the device's `Ctx` (`Owner`, because the install token was
//! presented), ask `authorize`, and only then call the capability. The audit writes a `begin` line
//! (`{id, ts_ms, event, tool, decision}`) **before** dispatch and an `end` line (`{id, ts_ms, event,
//! ok, error_kind}`) after it: a session close or a crash between them leaves the begin line, which
//! is the honest record of a side effect that may have happened. The tool name is recorded (cut to
//! a bounded length); no argument value is ever logged.
//!
//! The request body is bounded before rmcp parses it: [`MAX_BODY_BYTES`] is checked from
//! `Content-Length` and enforced while collecting the body, so an oversized request is 413 rather
//! than an allocation. The per-tool caps (1 MiB file, 1 MiB serialized memory entry) then apply to a
//! payload the process has already agreed to hold.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{header, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, ErrorCode, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool, ToolsCapability,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData as McpError, ServerHandler};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use summrise_core::grant::{Grant as ConsoleGrant, CLOCK_SKEW_MS};
use summrise_core::{
    authorize, registry, ApprovalState, Ctx, Decision, Subject, ToolCall, ToolDef, Transport,
};
use tokio::io::AsyncWriteExt;

use crate::capabilities::{browser, files, memory, monitor, runs, ssh, system, terminal};
use crate::config::{Config, Release};
use crate::store::Store;

/// The base64 engine the file tools use: the standard alphabet, padded, as every client library emits.
const BASE64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;
use base64::Engine as _;

/// The largest payload one `files.read` or `files.write` call moves.
///
/// The surface gives the capability this cap and the capability enforces it — a read from
/// metadata, before the file is opened. There is no second size check here.
const MAX_FILE_BYTES: usize = 1024 * 1024;

/// The largest request body `/mcp` accepts, before rmcp parses any JSON.
///
/// 8 MiB leaves room for JSON escaping: a 1 MiB text can serialize to ~6 MiB, so every payload the
/// per-tool caps admit fits. A 10 GB `Content-Length` is refused from the header, and an oversized
/// chunked body fails while it is collected; neither reaches serde or a capability.
pub(crate) const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

/// How many entries `memory.search` returns when the caller names no limit.
const DEFAULT_SEARCH_RESULTS: usize = 10;

/// How many trailing lines `terminal.screen` returns when the caller names no count.
const DEFAULT_SCREEN_LINES: usize = 60;

/// How long `system.net_test` waits when the caller names no timeout.
const DEFAULT_NET_TIMEOUT_MS: u64 = 5_000;

/// The terminal size `terminal.create` uses when the caller names none.
const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;

/// The largest the audit file may become before it is rotated to `events.1.jsonl`.
const AUDIT_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// How much of a tool name an audit line keeps.
///
/// Registry names are short; only an unknown name can be long, and the body limit bounds it. Capping
/// the recorded name keeps one line bounded even then, so the audit file's own cap means something.
const MAX_AUDIT_TOOL_BYTES: usize = 128;

/// The device's MCP handler: one session per client, one registry for every session.
#[derive(Clone)]
pub(crate) struct DeviceServer {
    /// The root `files` is sandboxed to.
    files_root: PathBuf,
    /// The state document; `state.get` is the console's device page (there is no console route).
    store: Store,
    /// The release facts `state.get`'s health half reports, the same ones `/api/health` uses.
    release: Release,
    /// The append-only memory; stateless, so every session shares the one capability.
    memory: Arc<memory::Memory>,
    /// The live pseudo-terminals. Shared: an MCP session may create a PTY that another client reads,
    /// and the device (not an HTTP session) owns a child.
    terminal: Arc<terminal::PtyRegistry>,
    /// The browser. Shared like the PTY registry, and lazy: nothing starts Chrome until a browser
    /// tool is called, and dropping the last server (the service) kills it.
    browser: Arc<browser::BrowserRegistry>,
    /// The reachability monitors. Shared like the PTY registry: one set of watches with one probe
    /// loop per target, whichever MCP session asks.
    monitor: Arc<monitor::MonitorRegistry>,
    /// The run history, and through it the files root `runs.exec`'s cwd resolves against.
    runs: Arc<runs::Runs>,
    /// The per-call audit log; shared so concurrent sessions serialize their appends.
    audit: Arc<Audit>,
}

/// Who is asking the handler to run a call.
///
/// The loopback HTTP surface proves the install token, so it is [`Caller::Owner`]. A remote frame
/// — the console's channel or a mesh peer's — carries a grant its transport has already verified
/// offline; it is [`Caller::Granted`], and the handler applies the same [`authorize`] rules a
/// loopback request would, with the grant as the subject's permission rather than an unexamined
/// claim from the wire.
#[derive(Clone, Copy)]
pub(crate) enum Caller<'a> {
    /// The install token was presented on loopback.
    Owner,
    /// A remote frame carried this verified grant, over `arrival`.
    Granted {
        /// The grant whose signature the transport verified.
        grant: &'a ConsoleGrant,
        /// The wire it arrived on, which the audit line records.
        arrival: Arrival,
        /// The calling channel's pinned public key in hex, when the wire authenticates a peer.
        /// The console's outbound WebSocket proves the console, not a peer, so only a mesh channel
        /// fills this; the audit line omits it elsewhere.
        peer: Option<&'a str>,
    },
}

/// How a call reached the handler, as the audit line records it.
///
/// It is not `core`'s [`Transport`]: that type answers the authorization question — a loopback
/// socket or a remote frame — and this one names the actual wire, which is the fact an operator
/// reading the log needs. A mesh channel authorizes as [`Transport::Relay`], because the mesh is
/// the relay-or-direct path that variant names, while the line says `mesh`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arrival {
    /// The loopback HTTP surface, with the install token.
    Loopback,
    /// The console's outbound WebSocket.
    Console,
    /// A mesh channel: a peer reached this device directly or through the relay.
    Mesh,
}

impl Arrival {
    /// The value the audit line carries.
    fn as_str(self) -> &'static str {
        match self {
            Arrival::Loopback => "loopback",
            Arrival::Console => "console",
            Arrival::Mesh => "mesh",
        }
    }

    /// How `core`'s authorization reads this arrival.
    fn transport(self) -> Transport {
        match self {
            Arrival::Loopback => Transport::Loopback,
            Arrival::Console => Transport::Console,
            // A mesh peer is remote however it was reached, so an absent grant is `NoGrant` rather
            // than the loopback default deny.
            Arrival::Mesh => Transport::Relay,
        }
    }
}

impl<'a> Caller<'a> {
    /// How this caller's call arrived.
    fn arrival(self) -> Arrival {
        match self {
            Caller::Owner => Arrival::Loopback,
            Caller::Granted { arrival, .. } => arrival,
        }
    }

    /// The calling channel's pinned public key in hex, when the wire authenticates one.
    fn peer(self) -> Option<&'a str> {
        match self {
            Caller::Owner => None,
            Caller::Granted { peer, .. } => peer,
        }
    }
}

/// The MCP service the HTTP router mounts at `/mcp`.
///
/// Stateful sessions and SSE responses: the standard streamable-HTTP shape, which an rmcp client
/// speaks without configuration. The allowed hosts are rmcp's loopback default, matching the only
/// address this device listens on. `limits` is the terminal capability's; production passes
/// [`terminal::Limits::default`], and an in-process test shortens the write bound.
pub(crate) fn service(
    config: &Config,
    limits: terminal::Limits,
    console: Option<crate::console::ConsoleEndpoint>,
    mesh: Option<crate::mesh::MeshLink>,
) -> StreamableHttpService<DeviceServer, LocalSessionManager> {
    let server = DeviceServer::new(config, limits);
    // The sweeper is the device's, not a client's: it holds a weak reference, so the children are
    // closed when the service (and its registry) is dropped.
    server.terminal.spawn_sweeper();
    // The monitors' probe loops belong to the registry, not to an MCP session; a loaded watch
    // starts probing at startup, before any client asks for its status.
    server.monitor.spawn();
    // The console link reaches the same handler the loopback surface does, so a call and its audit
    // line are the same code whichever transport delivered it.
    if let Some(endpoint) = console {
        crate::console::spawn(endpoint, server.clone());
    }
    // A mesh peer reaches the same handler too, over the device's own identity instead of the
    // console's WebSocket. Both are armed only when the operator configured them.
    if let Some(link) = mesh {
        crate::mesh::spawn(link, server.clone());
    }
    StreamableHttpService::new(
        move || Ok(server.clone()),
        Default::default(),
        StreamableHttpServerConfig::default(),
    )
}

/// Refuse a request body larger than [`MAX_BODY_BYTES`] before the MCP service sees it.
///
/// `axum::extract::DefaultBodyLimit` is an extension the axum extractors consult; rmcp reads the
/// body itself, so that layer would not apply here. This middleware checks `Content-Length` first
/// (a 10 GB header is refused without reading a byte) and collects the body with the same limit,
/// handing rmcp an in-memory body only after it fits.
pub(crate) async fn limit_request_body(request: Request<Body>, next: Next) -> Response {
    if let Some(declared) = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        if declared > MAX_BODY_BYTES as u64 {
            return payload_too_large(declared);
        }
    }

    let (parts, body) = request.into_parts();
    let bytes: Bytes = match axum::body::to_bytes(body, MAX_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => return payload_too_large(0),
    };
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

fn payload_too_large(declared: u64) -> Response {
    let detail = if declared > 0 {
        format!("the request body is {declared} bytes; the MCP limit is {MAX_BODY_BYTES}")
    } else {
        format!("the request body exceeds the MCP limit of {MAX_BODY_BYTES} bytes")
    };
    (StatusCode::PAYLOAD_TOO_LARGE, detail).into_response()
}

impl DeviceServer {
    pub(crate) fn new(config: &Config, limits: terminal::Limits) -> DeviceServer {
        let audit = Arc::new(Audit::new(&config.data));
        DeviceServer {
            files_root: config.files_root.clone(),
            store: Store::new(&config.install, &config.data, config.release()),
            release: config.release(),
            memory: Arc::new(memory::Memory::new(&config.data)),
            terminal: Arc::new(terminal::PtyRegistry::new(
                Store::new(&config.install, &config.data, config.release()),
                audit.write_lock(),
                limits,
            )),
            browser: Arc::new(browser::BrowserRegistry::new(&config.data)),
            monitor: Arc::new(monitor::MonitorRegistry::new(&config.data)),
            runs: Arc::new(runs::Runs::new(&config.data, &config.files_root)),
            audit,
        }
    }

    /// Log the authorization half of a call before anything can have a side effect.
    ///
    /// The caller awaits this before `dispatch`; a session close or process stop after dispatch but
    /// before the `end` line leaves this line on disk, which is the honest record. `false` means the
    /// line could not be written, and an allowed call must not run without it.
    async fn audit_begin(
        &self,
        id: u64,
        ts_ms: i64,
        tool: &str,
        decision: &str,
        arrival: Arrival,
        peer: Option<&str>,
    ) -> bool {
        self.audit
            .begin(id, ts_ms, tool, decision, arrival, peer)
            .await
    }

    /// Log the outcome half of a call, with a machine-readable kind and no argument value.
    async fn audit_end(
        &self,
        id: u64,
        ok: bool,
        error_kind: Option<&'static str>,
        arrival: Arrival,
        peer: Option<&str>,
    ) {
        self.audit
            .end(id, crate::http::now_ms(), ok, error_kind, arrival, peer)
            .await;
    }

    fn next_call_id(&self) -> u64 {
        self.audit.next_id()
    }

    /// Log a refusal that happened before a call was dispatched: a bad grant, a tool outside it,
    /// or another transport-level authorization failure.
    pub(crate) async fn audit_refused(
        &self,
        tool: &str,
        error_kind: &'static str,
        arrival: Arrival,
        peer: Option<&str>,
    ) {
        let id = self.next_call_id();
        let now_ms = crate::http::now_ms();
        self.audit_begin(id, now_ms, tool, "deny", arrival, peer)
            .await;
        self.audit_end(id, false, Some(error_kind), arrival, peer)
            .await;
    }

    /// Handle one JSON-RPC exchange from a remote frame, with the caller's context.
    ///
    /// The success value is a complete JSON-RPC response, not a bare result: the console and the
    /// mesh both forward the body to an MCP client unchanged. A tool-level refusal is a `CallToolResult` with
    /// `isError: true` (the MCP shape); an authorization decision is a JSON-RPC error.
    pub(crate) async fn handle(&self, request: &Value, caller: Caller<'_>) -> Value {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str);
        let Some(method) = method else {
            return jsonrpc_error(id, ErrorCode::INVALID_REQUEST.0, "invalid request");
        };

        match method {
            "initialize" => jsonrpc_result(
                id,
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": {
                        "name": "summrise-device",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                }),
            ),
            "ping" => jsonrpc_result(id, json!({})),
            "tools/list" => jsonrpc_result(id, json!({ "tools": registry_tools() })),
            "tools/call" => {
                let params = request.get("params");
                let name = params
                    .and_then(|params| params.get("name"))
                    .and_then(Value::as_str)
                    .filter(|name| !name.is_empty());
                let Some(name) = name else {
                    return jsonrpc_error(
                        id,
                        ErrorCode::INVALID_PARAMS.0,
                        "invalid params: tools/call requires params.name",
                    );
                };
                let arguments = params
                    .and_then(|params| params.get("arguments"))
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();

                match self.execute_call(name, arguments, caller).await {
                    Ok(result) => match serde_json::to_value(&result) {
                        Ok(result) => jsonrpc_result(id, result),
                        Err(error) => jsonrpc_error(
                            id,
                            ErrorCode::INTERNAL_ERROR.0,
                            &format!("the tool result could not be serialized: {error}"),
                        ),
                    },
                    // A capability refusal is MCP's tool-level error, not a protocol failure.
                    Err(CallFailure::Refused(message)) => {
                        let result = CallToolResult::error(vec![ContentBlock::text(message)]);
                        match serde_json::to_value(&result) {
                            Ok(result) => jsonrpc_result(id, result),
                            Err(error) => jsonrpc_error(
                                id,
                                ErrorCode::INTERNAL_ERROR.0,
                                &format!("the tool refusal could not be serialized: {error}"),
                            ),
                        }
                    }
                    Err(failure) => jsonrpc_error(id, failure.code(), failure.message()),
                }
            }
            _ => jsonrpc_error(
                id,
                ErrorCode::METHOD_NOT_FOUND.0,
                &format!("method not found: {method}"),
            ),
        }
    }

    /// The one authorization-and-dispatch path every surface uses.
    ///
    /// The caller supplies the identity; this method builds the [`Ctx`], makes the audit line the
    /// decision implies, and only then reaches [`Self::dispatch`].
    async fn execute_call(
        &self,
        name: &str,
        arguments: Map<String, Value>,
        caller: Caller<'_>,
    ) -> Result<CallToolResult, CallFailure> {
        let now_ms = crate::http::now_ms();
        let id = self.next_call_id();
        let arrival = caller.arrival();
        let peer = caller.peer();
        let Some(definition) = registry::tool(name) else {
            self.audit_begin(id, now_ms, name, "deny", arrival, peer)
                .await;
            self.audit_end(id, false, Some("unknown_tool"), arrival, peer)
                .await;
            return Err(CallFailure::Arguments(format!("unknown tool: {name}")));
        };

        // The policy grant is built here, beside the `Ctx` that borrows it, so the signed wire type
        // is never mistaken for the decision's narrow view of it.
        let policy = match caller {
            Caller::Owner => None,
            Caller::Granted { grant, .. } => Some(summrise_core::Grant::from(grant)),
        };
        let context = match caller {
            Caller::Owner => Ctx {
                subject: Subject::Owner,
                grant: None,
                transport: arrival.transport(),
                approval: ApprovalState::NotRequired,
                now_ms,
            },
            Caller::Granted { grant, .. } => Ctx {
                subject: Subject::Key {
                    // `authorize` reads the grant, not this. The id is the honest name of the door
                    // the call came through, so a surface that renders the subject cannot report a
                    // mesh peer as the console.
                    id: arrival.as_str().to_owned(),
                    scopes: grant.tools.clone(),
                },
                grant: policy.as_ref(),
                transport: arrival.transport(),
                approval: ApprovalState::NotRequired,
                now_ms: now_ms.saturating_sub(CLOCK_SKEW_MS),
            },
        };
        let call = ToolCall::new(name.to_owned(), definition.destructive);
        match authorize(&context, &call) {
            Decision::Allow => {}
            Decision::Deny { reason } => {
                let detail = reason.to_string();
                self.audit_begin(id, now_ms, name, "deny", arrival, peer)
                    .await;
                self.audit_end(id, false, Some("denied"), arrival, peer)
                    .await;
                return Err(CallFailure::Denied(detail));
            }
            Decision::Ask { prompt } => {
                self.audit_begin(id, now_ms, name, "ask", arrival, peer)
                    .await;
                self.audit_end(id, false, Some("approval_required"), arrival, peer)
                    .await;
                // Through the console there is no interactive approval yet, so the only way to
                // consent is the signature. Say that in the refusal rather than leaving the
                // operator with a prompt no surface can answer.
                let prompt = match caller {
                    Caller::Granted { .. } => format!(
                        "{prompt}; this grant did not consent to destructive tools, so mint a \
                         grant with allow_destructive=true"
                    ),
                    Caller::Owner => prompt,
                };
                return Err(CallFailure::Approval(prompt));
            }
        }

        // I1: the authorization line is on disk before the capability can touch anything. If the
        // session is closed or the process stops between here and the `end` line, the begin line is
        // what the audit keeps; it says the call was allowed, not that it failed. An unwritable log
        // refuses the call before dispatch: no side effect runs without its authorization line.
        if !self
            .audit_begin(id, now_ms, name, "allow", arrival, peer)
            .await
        {
            return Err(CallFailure::Internal(
                "the audit log is not writable; the capability was not run".to_owned(),
            ));
        }

        match self.dispatch(name, arguments).await {
            Ok(text) => {
                self.audit_end(id, true, None, arrival, peer).await;
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
            }
            Err(ToolError::Arguments(message)) => {
                self.audit_end(id, false, Some("arguments"), arrival, peer)
                    .await;
                Err(CallFailure::Arguments(message))
            }
            Err(ToolError::Refused(message)) => {
                self.audit_end(id, false, Some("refused"), arrival, peer)
                    .await;
                Err(CallFailure::Refused(message))
            }
            Err(ToolError::Internal(message)) => {
                self.audit_end(id, false, Some("internal"), arrival, peer)
                    .await;
                Err(CallFailure::Internal(message))
            }
        }
    }

    /// Route one already-authorized call to its capability.
    ///
    /// The name was resolved against the registry by the caller; [`handler`] is the one table that
    /// gives it an implementation, and its `None` arm is a registry entry nobody dispatches — a bug
    /// the server reports instead of hiding. The terminal tools are async because create and close
    /// take the audit/state lock the session rows are written under; `dispatch` is therefore async
    /// even though the files and memory arms do no awaiting.
    async fn dispatch(
        &self,
        name: &str,
        arguments: Map<String, Value>,
    ) -> Result<String, ToolError> {
        let Some(handler) = handler(name) else {
            return Err(ToolError::Internal(format!(
                "{name} is in the registry but has no implementation"
            )));
        };

        match handler {
            Handler::FilesRead => {
                let arguments: ReadArgs = parse(arguments, name)?;
                let bytes =
                    files::read(&self.files_root, Path::new(&arguments.path), MAX_FILE_BYTES)
                        .map_err(|error| refused(name, error))?;
                match arguments.encoding {
                    Encoding::Utf8 => String::from_utf8(bytes).map_err(|_| {
                        ToolError::Refused(format!(
                            "{name}: the file is not UTF-8 text; ask for encoding \"base64\""
                        ))
                    }),
                    Encoding::Base64 => Ok(BASE64.encode(&bytes)),
                }
            }
            Handler::FilesWrite => {
                let arguments: WriteArgs = parse(arguments, name)?;
                let bytes = match arguments.encoding {
                    Encoding::Utf8 => arguments.content.clone().into_bytes(),
                    Encoding::Base64 => BASE64.decode(&arguments.content).map_err(|_| {
                        ToolError::Refused(format!("{name}: the content is not valid base64"))
                    })?,
                };
                files::write(
                    &self.files_root,
                    Path::new(&arguments.path),
                    &bytes,
                    MAX_FILE_BYTES,
                )
                .map_err(|error| refused(name, error))?;
                Ok(json!({
                    "path": arguments.path,
                    "bytes": bytes.len(),
                })
                .to_string())
            }
            Handler::FilesMkdir => {
                let arguments: PathArgs = parse(arguments, name)?;
                files::mkdir(&self.files_root, Path::new(&arguments.path))
                    .map_err(|error| refused(name, error))?;
                Ok(json!({ "path": arguments.path, "created": true }).to_string())
            }
            Handler::FilesList => {
                let arguments: PathArgs = parse(arguments, name)?;
                let entries = files::list(&self.files_root, Path::new(&arguments.path))
                    .map_err(|error| refused(name, error))?;
                let entries: Vec<Value> = entries
                    .iter()
                    .map(|entry| {
                        json!({
                            "name": entry.name,
                            "kind": match entry.kind {
                                files::Kind::File => "file",
                                files::Kind::Directory => "directory",
                                files::Kind::Other => "other",
                            },
                            "len": entry.len,
                        })
                    })
                    .collect();
                Ok(Value::Array(entries).to_string())
            }
            Handler::MemoryAppend => {
                let arguments: AppendArgs = parse(arguments, name)?;
                let entry = self
                    .memory
                    .append(&arguments.text, &arguments.tags)
                    .map_err(|error| refused(name, error))?;
                Ok(json!({ "appended": true, "id": entry.id }).to_string())
            }
            Handler::MemoryUpdate => {
                let arguments: UpdateArgs = parse(arguments, name)?;
                let entry = self
                    .memory
                    .update(&arguments.id, &arguments.text, &arguments.tags)
                    .map_err(|error| refused(name, error))?;
                Ok(json!({
                    "id": entry.id,
                    "text": entry.text,
                    "tags": entry.tags,
                })
                .to_string())
            }
            Handler::MemoryDelete => {
                let arguments: DeleteArgs = parse(arguments, name)?;
                self.memory
                    .delete(&arguments.id)
                    .map_err(|error| refused(name, error))?;
                Ok(json!({ "deleted": true, "id": arguments.id }).to_string())
            }
            Handler::MemorySearch => {
                let arguments: SearchArgs = parse(arguments, name)?;
                let limit = arguments.limit.unwrap_or(DEFAULT_SEARCH_RESULTS);
                let found = self
                    .memory
                    .search(&arguments.query, limit)
                    .map_err(|error| refused(name, error))?;
                let found: Vec<Value> = found
                    .iter()
                    .map(|entry| json!({ "id": entry.id, "text": entry.text, "tags": entry.tags }))
                    .collect();
                Ok(Value::Array(found).to_string())
            }
            Handler::MemoryList => {
                let arguments: MemoryListArgs = parse(arguments, name)?;
                let limit = arguments.limit.unwrap_or(DEFAULT_SEARCH_RESULTS);
                let found = self
                    .memory
                    .list(limit, arguments.tag.as_deref())
                    .map_err(|error| refused(name, error))?;
                let found: Vec<Value> = found
                    .iter()
                    .map(|entry| json!({ "id": entry.id, "text": entry.text, "tags": entry.tags }))
                    .collect();
                Ok(Value::Array(found).to_string())
            }
            Handler::MemoryExport => {
                let _arguments: EmptyArgs = parse(arguments, name)?;
                let export = self
                    .memory
                    .export(memory::MAX_EXPORT_BYTES)
                    .map_err(|error| refused(name, error))?;
                Ok(json!({
                    "text": export.text,
                    "entries": export.entries,
                    "truncated": export.truncated,
                    "total_bytes": export.total_bytes,
                })
                .to_string())
            }
            Handler::TerminalCreate => {
                let arguments: PtyCreateArgs = parse(arguments, name)?;
                // The kind is the capability's to validate, and its refusal is typed: a client
                // asking for a kind this build does not run learns which kinds it does. The shape
                // of the rest of the arguments depends on the kind, so a pty with an SSH host (or
                // an ssh without one) is an arguments error, not a silently ignored field.
                let kind =
                    terminal::Kind::parse(&arguments.kind).map_err(|error| refused(name, error))?;
                // A serial target borrows its path, and the target outlives this match's arms, so
                // the owned path must live in the outer scope. Only the serial arm initializes it.
                let serial_port: String;
                let target = match kind {
                    terminal::Kind::Pty => {
                        reject_serial_framing(&arguments, name)?;
                        if arguments.host.is_some()
                            || arguments.port.is_some()
                            || arguments.user.is_some()
                            || arguments.auth.is_some()
                            || arguments.host_key.is_some()
                        {
                            return Err(ToolError::Arguments(format!(
                                "{name}: kind pty takes shell, rows and cols; host, port, user, auth and host_key are for kind ssh"
                            )));
                        }
                        terminal::CreateTarget::Pty {
                            shell: arguments.shell.as_deref(),
                        }
                    }
                    terminal::Kind::Ssh => {
                        reject_serial_framing(&arguments, name)?;
                        if arguments.shell.is_some() {
                            return Err(ToolError::Arguments(format!(
                                "{name}: kind ssh takes host, port, user, auth and optional host_key; shell is for kind pty"
                            )));
                        }
                        let required = |field: &str| {
                            ToolError::Arguments(format!(
                                "{name}: {field} is required when kind is ssh"
                            ))
                        };
                        let host = arguments.host.as_deref().ok_or_else(|| required("host"))?;
                        let port = match arguments.port.as_ref() {
                            Some(PortArg::Ssh(port)) => *port,
                            Some(PortArg::Serial(_)) => {
                                return Err(ToolError::Arguments(format!(
                                    "{name}: kind ssh names its TCP port as a number, not a port path"
                                )));
                            }
                            None => return Err(required("port")),
                        };
                        let user = arguments.user.as_deref().ok_or_else(|| required("user"))?;
                        let auth = arguments.auth.as_ref().ok_or_else(|| required("auth"))?;
                        terminal::CreateTarget::Ssh(terminal::SshTarget {
                            host,
                            port,
                            user,
                            auth,
                            host_key: arguments.host_key.as_deref(),
                        })
                    }
                    terminal::Kind::Serial => {
                        if arguments.shell.is_some()
                            || arguments.host.is_some()
                            || arguments.user.is_some()
                            || arguments.auth.is_some()
                            || arguments.host_key.is_some()
                        {
                            return Err(ToolError::Arguments(format!(
                                "{name}: kind serial takes port and optional baud, parity, data_bits and stop_bits; shell, host, user, auth and host_key are for kind pty or ssh"
                            )));
                        }
                        let port = match arguments.port.as_ref() {
                            Some(PortArg::Serial(port)) if !port.is_empty() => port.clone(),
                            Some(PortArg::Serial(_)) => {
                                return Err(ToolError::Arguments(format!(
                                    "{name}: kind serial needs a non-empty port path"
                                )));
                            }
                            Some(PortArg::Ssh(_)) => {
                                return Err(ToolError::Arguments(format!(
                                    "{name}: kind serial names an OS port path as a string, not a TCP port number"
                                )));
                            }
                            None => {
                                return Err(ToolError::Arguments(format!(
                                    "{name}: port is required when kind is serial; it names the OS port, e.g. COM3 or /dev/ttyUSB0"
                                )));
                            }
                        };
                        let parity =
                            terminal::Parity::parse(arguments.parity.as_deref().unwrap_or("none"))
                                .map_err(|error| refused(name, error))?;
                        serial_port = port;
                        terminal::CreateTarget::Serial(terminal::SerialTarget {
                            baud: arguments.baud.unwrap_or(terminal::DEFAULT_BAUD),
                            parity,
                            data_bits: arguments.data_bits.unwrap_or(terminal::DEFAULT_DATA_BITS),
                            stop_bits: arguments.stop_bits.unwrap_or(terminal::DEFAULT_STOP_BITS),
                            ..terminal::SerialTarget::new(&serial_port)
                        })
                    }
                };
                let label = target.label();
                let id = self
                    .terminal
                    .create(
                        target,
                        arguments.rows.unwrap_or(DEFAULT_ROWS),
                        arguments.cols.unwrap_or(DEFAULT_COLS),
                    )
                    .await
                    .map_err(|error| refused(name, error))?;
                Ok(json!({
                    "id": id.as_str(),
                    "kind": kind.as_str(),
                    "shell": label,
                })
                .to_string())
            }
            Handler::TerminalWrite => {
                let arguments: PtyWriteArgs = parse(arguments, name)?;
                let id = session_id(name, &arguments.id)?;
                let written = self
                    .terminal
                    .write(&id, arguments.data.as_bytes())
                    .await
                    .map_err(|error| refused(name, error))?;
                Ok(json!({ "written": written }).to_string())
            }
            Handler::TerminalRead => {
                let arguments: PtyReadArgs = parse(arguments, name)?;
                let id = session_id(name, &arguments.id)?;
                let (bytes, cursor) = self
                    .terminal
                    .read(&id, arguments.cursor.unwrap_or(0))
                    .map_err(|error| refused(name, error))?;
                // The wire is JSON, so the bytes are rendered as UTF-8. This is lossy on purpose: a
                // read can split a multi-byte sequence at the cursor, and the client's cursor is
                // already past it, so the replacement character is the honest answer rather than a
                // retry the capability cannot prove is safe. A byte-stream client (the panel's
                // terminal) reads the same ring through its own transport later.
                Ok(json!({
                    "data": String::from_utf8_lossy(&bytes),
                    "cursor": cursor,
                })
                .to_string())
            }
            Handler::TerminalResize => {
                let arguments: PtyResizeArgs = parse(arguments, name)?;
                let id = session_id(name, &arguments.id)?;
                self.terminal
                    .resize(&id, arguments.rows, arguments.cols)
                    .await
                    .map_err(|error| refused(name, error))?;
                Ok(json!({
                    "resized": true,
                    "rows": arguments.rows,
                    "cols": arguments.cols,
                })
                .to_string())
            }
            Handler::TerminalClose => {
                let arguments: PtyIdArgs = parse(arguments, name)?;
                let id = session_id(name, &arguments.id)?;
                self.terminal
                    .close(&id)
                    .await
                    .map_err(|error| refused(name, error))?;
                Ok(json!({ "closed": true }).to_string())
            }
            Handler::TerminalList => {
                let sessions: Vec<Value> = self
                    .terminal
                    .list()
                    .iter()
                    .map(|session| {
                        // Discovery only: the kind and when it started. Size, cursor, pid and
                        // liveness belong to one session's own calls, and a client that needs them
                        // reads the session, not the list.
                        json!({
                            "id": session.id.as_str(),
                            "kind": session.kind.as_str(),
                            "started_at_ms": session.started_at_ms,
                        })
                    })
                    .collect();
                Ok(Value::Array(sessions).to_string())
            }
            Handler::TerminalScreen => {
                let arguments: ScreenArgs = parse(arguments, name)?;
                let id = session_id(name, &arguments.id)?;
                let lines = arguments
                    .lines
                    .unwrap_or(DEFAULT_SCREEN_LINES)
                    .min(terminal::MAX_SCREEN_LINES);
                let text = self
                    .terminal
                    .screen(&id, lines)
                    .map_err(|error| refused(name, error))?;
                Ok(json!({
                    "id": arguments.id,
                    "text": text,
                    "lines": text.lines().count(),
                })
                .to_string())
            }
            Handler::BrowserList => {
                let targets = self
                    .browser
                    .list()
                    .await
                    .map_err(|error| refused(name, error))?;
                let targets: Vec<Value> = targets
                    .iter()
                    .map(|target| {
                        json!({
                            "id": target.id,
                            "title": target.title,
                            "url": target.url,
                            "type": target.kind,
                        })
                    })
                    .collect();
                Ok(Value::Array(targets).to_string())
            }
            Handler::BrowserOpen => {
                let arguments: BrowserOpenArgs = parse(arguments, name)?;
                let target = self
                    .browser
                    .open(&arguments.url)
                    .await
                    .map_err(|error| refused(name, error))?;
                Ok(json!({ "target": target, "url": arguments.url }).to_string())
            }
            Handler::BrowserNavigate => {
                let arguments: BrowserNavigateArgs = parse(arguments, name)?;
                self.browser
                    .navigate(&arguments.target, &arguments.url)
                    .await
                    .map_err(|error| refused(name, error))?;
                Ok(json!({ "navigated": true, "url": arguments.url }).to_string())
            }
            Handler::BrowserEval => {
                let arguments: BrowserEvalArgs = parse(arguments, name)?;
                let result = self
                    .browser
                    .eval(&arguments.target, &arguments.expression)
                    .await
                    .map_err(|error| refused(name, error))?;
                let mut value = json!({ "type": result.kind });
                if let Some(result) = result.value {
                    value["value"] = result;
                }
                Ok(value.to_string())
            }
            Handler::BrowserScreenshot => {
                let arguments: BrowserTargetArgs = parse(arguments, name)?;
                let screenshot = self
                    .browser
                    .screenshot(&arguments.target)
                    .await
                    .map_err(|error| refused(name, error))?;
                Ok(json!({
                    "format": "png",
                    "data": screenshot.data,
                    "bytes": screenshot.bytes,
                })
                .to_string())
            }
            Handler::BrowserClose => {
                let arguments: BrowserTargetArgs = parse(arguments, name)?;
                self.browser
                    .close(&arguments.target)
                    .await
                    .map_err(|error| refused(name, error))?;
                Ok(json!({ "closed": true }).to_string())
            }
            Handler::BrowserShutdown => {
                let stopped = self.browser.shutdown().await;
                Ok(json!({ "stopped": stopped }).to_string())
            }
            Handler::MonitorAdd => {
                let arguments: MonitorAddArgs = parse(arguments, name)?;
                let target = self
                    .monitor
                    .add(monitor::TargetSpec {
                        kind: arguments.kind,
                        host: arguments.host,
                        port: arguments.port,
                        path: arguments.path,
                        expect: arguments.expect,
                        interval_ms: arguments.interval_ms,
                    })
                    .map_err(|error| refused(name, error))?;
                Ok(json!(target).to_string())
            }
            Handler::MonitorRemove => {
                let arguments: MonitorIdArgs = parse(arguments, name)?;
                let removed = self
                    .monitor
                    .remove(&arguments.id)
                    .map_err(|error| refused(name, error))?;
                Ok(json!({ "removed": removed }).to_string())
            }
            Handler::MonitorList => {
                let targets: Vec<Value> = self
                    .monitor
                    .list()
                    .iter()
                    .map(|target| {
                        json!({
                            "id": target.id,
                            "kind": target.kind,
                            "host": target.host,
                            "port": target.port,
                            "path": target.path,
                            "expect": target.expect,
                            "interval_ms": target.interval_ms,
                            "summary": self.monitor.summary(&target.id).unwrap_or_default(),
                        })
                    })
                    .collect();
                Ok(Value::Array(targets).to_string())
            }
            Handler::MonitorProbe => {
                let arguments: MonitorIdArgs = parse(arguments, name)?;
                let sample = self
                    .monitor
                    .probe(&arguments.id)
                    .await
                    .map_err(|error| refused(name, error))?;
                Ok(json!(sample).to_string())
            }
            Handler::MonitorStatus => {
                let arguments: MonitorIdArgs = parse(arguments, name)?;
                let summary = self.monitor.summary(&arguments.id).ok_or_else(|| {
                    refused(
                        name,
                        monitor::MonitorError::UnknownTarget {
                            id: arguments.id.clone(),
                        },
                    )
                })?;
                Ok(json!(summary).to_string())
            }
            Handler::RunsExec => {
                let arguments: RunsExecArgs = parse(arguments, name)?;
                // The run is a blocking wait for a child process; it belongs on the blocking pool,
                // not on a reactor worker that other MCP calls share.
                let runs = Arc::clone(&self.runs);
                let request = runs::ExecRequest {
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
                        ToolError::Internal(format!("{name}: the run task failed: {error}"))
                    })?
                    .map_err(|error| refused(name, error))?;
                Ok(json!(result).to_string())
            }
            Handler::RunsList => {
                let arguments: RunsListArgs = parse(arguments, name)?;
                let rows = self
                    .runs
                    .list(arguments.limit.unwrap_or(runs::DEFAULT_LIST_LIMIT))
                    .map_err(|error| refused(name, error))?;
                Ok(json!(rows).to_string())
            }
            Handler::RunsGet => {
                let arguments: RunsIdArgs = parse(arguments, name)?;
                let row = self
                    .runs
                    .get(&arguments.id)
                    .map_err(|error| refused(name, error))?;
                Ok(json!(row).to_string())
            }
            Handler::SystemProcessList => {
                let arguments: ProcessListArgs = parse(arguments, name)?;
                let found = system::processes(arguments.name.as_deref())
                    .map_err(|error| refused(name, error))?;
                let found: Vec<Value> = found
                    .iter()
                    .map(|process| {
                        json!({
                            "pid": process.pid,
                            "name": process.name,
                            "memory_kb": process.memory_kb,
                        })
                    })
                    .collect();
                Ok(Value::Array(found).to_string())
            }
            Handler::SystemProcessKill => {
                let arguments: ProcessKillArgs = parse(arguments, name)?;
                let report = system::kill(arguments.pid, arguments.name.as_deref())
                    .map_err(|error| refused(name, error))?;
                let failed: Vec<Value> = report
                    .failed
                    .iter()
                    .map(|(pid, reason)| json!({ "pid": pid, "reason": reason }))
                    .collect();
                Ok(json!({ "killed": report.killed, "failed": failed }).to_string())
            }
            Handler::SystemNetTest => {
                let arguments: NetTestArgs = parse(arguments, name)?;
                let timeout = Duration::from_millis(
                    arguments
                        .timeout_ms
                        .unwrap_or(DEFAULT_NET_TIMEOUT_MS)
                        .clamp(1, system::MAX_TIMEOUT_MS),
                );
                let report = system::net_test(&arguments.host, arguments.port, timeout)
                    .map_err(|error| refused(name, error))?;
                Ok(json!({
                    "addresses": report.addresses,
                    "connected": report.connected,
                    "latency_ms": report.latency_ms,
                    "error": report.error,
                })
                .to_string())
            }
            Handler::HostFacts => {
                // Read-only, and the one tool an operator can call without minting a grant: knowing
                // what machine a device is is context for every decision that follows.
                Ok(crate::host::facts().to_string())
            }
            Handler::ScreenCapture => {
                // The one tool that runs where this process cannot: the picture comes from the
                // per-user helper, and "no helper" is a sentence rather than a black frame.
                let Some(helper) = crate::helper::current() else {
                    return Err(refused(
                        name,
                        "no per-user helper is running on this device",
                    ));
                };
                let Some(port) = helper.port() else {
                    return Err(refused(
                        name,
                        "the per-user helper has not registered this boot",
                    ));
                };
                let capture = crate::helper::capture(port, helper.token())
                    .await
                    .map_err(|error| refused(name, error))?;
                Ok(json!({
                    "format": "png",
                    "data": capture.png,
                    "width": capture.width,
                    "height": capture.height,
                })
                .to_string())
            }
            Handler::StateGet => {
                // The console's device page: the document an operator would read from
                // `/api/state`, plus the readiness answer `/api/health` would give, and the last ten
                // events the device recorded — the same tail the local panel shows. A poisoned
                // document is the failure this tool exists to make visible, so it is a successful
                // call carrying `poisoned: true` and a null state, not an error the console would
                // render as "the device is offline".
                let (state, health) = match self.store.load() {
                    Ok(state) => (
                        json!(state),
                        json!({
                            "status": "ok",
                            "version": self.release.version,
                            "pid": std::process::id(),
                            "poisoned": false,
                        }),
                    ),
                    Err(error) => (
                        Value::Null,
                        json!({
                            "status": "degraded",
                            "version": self.release.version,
                            "pid": std::process::id(),
                            "poisoned": true,
                            "reason": format!("state: {error}"),
                        }),
                    ),
                };
                // Best-effort, like the local panel: a device that cannot read its own event log
                // still answers the document, and the console shows an empty tail rather than an
                // offline device. A poisoned document still returns whatever events exist.
                let events = self.store.tail_events(10).unwrap_or_default();
                Ok(json!({ "state": state, "health": health, "events": events }).to_string())
            }
        }
    }
}

/// Why one call failed, in the shape each surface renders.
///
/// `Arguments` and `Internal` are protocol errors on both surfaces; `Denied` and `Approval` are the
/// two authorization answers, with the codes the console path shares with an operator's client;
/// `Refused` is a capability's answer and stays a tool-level `isError` result where MCP is spoken.
enum CallFailure {
    Arguments(String),
    Refused(String),
    Internal(String),
    Denied(String),
    Approval(String),
}

impl CallFailure {
    /// The JSON-RPC code an authorization or protocol error carries.
    fn code(&self) -> i32 {
        match self {
            CallFailure::Arguments(_) => ErrorCode::INVALID_PARAMS.0,
            CallFailure::Internal(_) => ErrorCode::INTERNAL_ERROR.0,
            CallFailure::Denied(_) => -32001,
            CallFailure::Approval(_) => -32002,
            CallFailure::Refused(_) => ErrorCode::INTERNAL_ERROR.0,
        }
    }

    fn message(&self) -> &str {
        match self {
            CallFailure::Arguments(message)
            | CallFailure::Refused(message)
            | CallFailure::Internal(message)
            | CallFailure::Denied(message)
            | CallFailure::Approval(message) => message,
        }
    }

    /// The rmcp error the loopback MCP surface returns.
    fn into_mcp_error(self) -> McpError {
        match self {
            CallFailure::Arguments(message) => McpError::invalid_params(message, None),
            CallFailure::Internal(message) => McpError::internal_error(message, None),
            CallFailure::Denied(message) => McpError::new(ErrorCode(-32001), message, None),
            CallFailure::Approval(prompt) => McpError::new(
                ErrorCode(-32002),
                format!("approval required: {prompt}"),
                None,
            ),
            CallFailure::Refused(message) => McpError::internal_error(message, None),
        }
    }
}

/// A JSON-RPC 2.0 success answer.
fn jsonrpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// A JSON-RPC 2.0 error answer.
fn jsonrpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

/// One implementation of one registry tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handler {
    FilesRead,
    FilesWrite,
    FilesMkdir,
    FilesList,
    MemoryAppend,
    MemoryUpdate,
    MemoryDelete,
    MemorySearch,
    MemoryList,
    MemoryExport,
    TerminalCreate,
    TerminalWrite,
    TerminalRead,
    TerminalResize,
    TerminalClose,
    TerminalList,
    TerminalScreen,
    BrowserList,
    BrowserOpen,
    BrowserNavigate,
    BrowserEval,
    BrowserScreenshot,
    BrowserClose,
    BrowserShutdown,
    MonitorAdd,
    MonitorRemove,
    MonitorList,
    MonitorProbe,
    MonitorStatus,
    RunsExec,
    RunsList,
    RunsGet,
    SystemProcessList,
    SystemProcessKill,
    SystemNetTest,
    HostFacts,
    ScreenCapture,
    StateGet,
}

/// The names this surface implements, and their handlers.
///
/// This is the one table that turns a registry name into code. Adding a tool to `CAPABILITIES`
/// without a row here fails `every_registry_tool_has_an_implementation`; adding a row without a
/// registry entry fails `every_implementation_is_registered`. `tools/list` cannot advertise a tool
/// the surface cannot run without that test going red.
const HANDLERS: &[(&str, Handler)] = &[
    ("files.read", Handler::FilesRead),
    ("files.write", Handler::FilesWrite),
    ("files.mkdir", Handler::FilesMkdir),
    ("files.list", Handler::FilesList),
    ("memory.append", Handler::MemoryAppend),
    ("memory.update", Handler::MemoryUpdate),
    ("memory.delete", Handler::MemoryDelete),
    ("memory.search", Handler::MemorySearch),
    ("memory.list", Handler::MemoryList),
    ("memory.export", Handler::MemoryExport),
    ("terminal.create", Handler::TerminalCreate),
    ("terminal.write", Handler::TerminalWrite),
    ("terminal.read", Handler::TerminalRead),
    ("terminal.resize", Handler::TerminalResize),
    ("terminal.close", Handler::TerminalClose),
    ("terminal.list", Handler::TerminalList),
    ("terminal.screen", Handler::TerminalScreen),
    ("browser.list", Handler::BrowserList),
    ("browser.open", Handler::BrowserOpen),
    ("browser.navigate", Handler::BrowserNavigate),
    ("browser.eval", Handler::BrowserEval),
    ("browser.screenshot", Handler::BrowserScreenshot),
    ("browser.close", Handler::BrowserClose),
    ("browser.shutdown", Handler::BrowserShutdown),
    ("monitor.add", Handler::MonitorAdd),
    ("monitor.remove", Handler::MonitorRemove),
    ("monitor.list", Handler::MonitorList),
    ("monitor.probe", Handler::MonitorProbe),
    ("monitor.status", Handler::MonitorStatus),
    ("runs.exec", Handler::RunsExec),
    ("runs.list", Handler::RunsList),
    ("runs.get", Handler::RunsGet),
    ("system.process_list", Handler::SystemProcessList),
    ("system.process_kill", Handler::SystemProcessKill),
    ("system.net_test", Handler::SystemNetTest),
    ("host.facts", Handler::HostFacts),
    ("screen.capture", Handler::ScreenCapture),
    ("state.get", Handler::StateGet),
];

/// The handler for `name`, or `None` when the surface has no implementation.
fn handler(name: &str) -> Option<Handler> {
    HANDLERS
        .iter()
        .find(|(declared, _)| *declared == name)
        .map(|(_, handler)| *handler)
}

/// Whether the surface can run `name`. Read by the registry-alignment test.
#[cfg(test)]
fn dispatch_supports(name: &str) -> bool {
    handler(name).is_some()
}

impl ServerHandler for DeviceServer {
    fn get_info(&self) -> ServerInfo {
        let mut tools_capability = ToolsCapability::default();
        // The registry is a compile-time table: the list cannot change while the process serves.
        tools_capability.list_changed = Some(false);

        let mut info = ServerInfo::default();
        info.instructions = Some(
            "Summrise device: files inside the device's files root, its append-only memory, \
             terminal sessions (local PTY or SSH), a headless browser, reachability monitors, and
             one-shot command runs with a short history."
                .into(),
        );
        info.capabilities = ServerCapabilities::builder()
            .enable_tools_with(tools_capability)
            .build();
        let mut server_info = Implementation::default();
        server_info.name = "summrise-device".into();
        server_info.title = Some("Summrise Device".into());
        server_info.version = env!("CARGO_PKG_VERSION").into();
        info.server_info = server_info;
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(registry_tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let name = request.name.as_ref().to_owned();
        let arguments = request.arguments.unwrap_or_default();
        match self.execute_call(&name, arguments, Caller::Owner).await {
            Ok(result) => Ok(result),
            Err(CallFailure::Refused(message)) => {
                Ok(CallToolResult::error(vec![ContentBlock::text(message)]))
            }
            Err(failure) => Err(failure.into_mcp_error()),
        }
    }
}

/// Every tool the registry declares, in registry order: the whole MCP surface.
fn registry_tools() -> Vec<Tool> {
    registry::CAPABILITIES
        .iter()
        .flat_map(|capability| capability.tools)
        .map(|definition| {
            let mut tool = Tool::default();
            tool.name = definition.name.into();
            tool.description = Some(definition.description.into());
            tool.input_schema = Arc::new(
                input_schema(definition)
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            );
            tool
        })
        .collect()
}

/// The input schema for one registry tool.
///
/// Keyed by the registry's name and never by a list of its own: a tool the registry does not declare
/// is never asked for a schema, and a declared tool with no entry gets the empty object schema
/// rather than disappearing from the list.
fn input_schema(definition: &ToolDef) -> Value {
    match definition.name {
        "files.read" => json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "A path relative to the device's files root, e.g. notes/todo.txt."
                },
                "encoding": {
                    "type": "string",
                    "enum": ["utf8", "base64"],
                    "description": "How to return the bytes. utf8 (the default) is the file's text and fails on a file that is not text; base64 returns the bytes encoded, for a file that is not text."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
        "files.write" => json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "A path relative to the device's files root; its parent must exist (create it with files.mkdir)."
                },
                "content": {
                    "type": "string",
                    "description": "The bytes to write, at most 1 MiB, encoded as the `encoding` says. It replaces the file's bytes."
                },
                "encoding": {
                    "type": "string",
                    "enum": ["utf8", "base64"],
                    "description": "How `content` encodes the bytes: utf8 (the default) for text, base64 for anything else. The 1 MiB limit applies to the decoded bytes."
                }
            },
            "required": ["path", "content"],
            "additionalProperties": false
        }),
        "files.mkdir" => json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "One directory relative to the files root, e.g. notes; its parent must exist. It never overwrites or deletes."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
        "files.list" => json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "A directory relative to the files root; \"\" lists the root. Returns at most 1000 entries, sorted; a larger directory is an error."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
        "memory.append" => json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "minLength": 1,
                    "description": "The fact to remember; an empty text is refused."
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Free-form tags; a search matches them as well as the text. At most 32 tags of 128 bytes each."
                }
            },
            "required": ["text"],
            "additionalProperties": false
        }),
        "memory.update" => json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The id memory.list, memory.search or memory.append returned."
                },
                "text": {
                    "type": "string",
                    "minLength": 1,
                    "description": "The entry's new text; it replaces what the entry said. An empty text is refused: a delete retires an entry."
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "The entry's complete new tag list, not a patch: at most 32 tags of 128 bytes each, and [] clears them."
                }
            },
            "required": ["id", "text", "tags"],
            "additionalProperties": false
        }),
        "memory.delete" => json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The id memory.list, memory.search or memory.append returned."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        }),
        "memory.search" => json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Case-insensitive substring matched against text and tags."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": memory::MAX_RESULTS,
                    "description": format!("How many entries to return, newest first (default {DEFAULT_SEARCH_RESULTS}).")
                }
            },
            "required": ["query"],
            "additionalProperties": false
        }),
        "memory.list" => json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": memory::MAX_RESULTS,
                    "description": format!("How many entries to return, newest first (default {DEFAULT_SEARCH_RESULTS}).")
                },
                "tag": {
                    "type": "string",
                    "description": "Only entries carrying exactly this tag, matched case-insensitively."
                }
            },
            "additionalProperties": false
        }),
        "memory.export" => json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
        "terminal.create" => json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["pty", "ssh", "serial"],
                    "description": "The kind of session to start. pty runs a local shell on a new pseudo-terminal; ssh opens a remote shell whose host key is pinned on first use; serial opens the OS port path the caller names at 115200 8N1 unless framing is given."
                },
                "shell": {
                    "type": "string",
                    "description": "For kind pty: the program to run. The default is bash on Linux/macOS and pwsh on Windows. This is one program path, not a command line with arguments."
                },
                "host": {
                    "type": "string",
                    "description": "For kind ssh: the server's DNS name or IP address."
                },
                "port": {
                    "oneOf": [
                        {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 65535,
                            "description": "For kind ssh: the server's TCP port."
                        },
                        {
                            "type": "string",
                            "minLength": 1,
                            "description": "For kind serial: the OS port path, e.g. COM3 on Windows or /dev/ttyUSB0 on Unix. The capability never enumerates or guesses a port."
                        }
                    ]
                },
                "user": {
                    "type": "string",
                    "description": "For kind ssh: the remote user whose shell runs."
                },
                "auth": {
                    "type": "object",
                    "properties": {
                        "kind": {
                            "type": "string",
                            "enum": ["password", "key"],
                            "description": "password sends a password; key presents an OpenSSH private key."
                        },
                        "password": {
                            "type": "string",
                            "description": "For kind password: the password. Never logged."
                        },
                        "path": {
                            "type": "string",
                            "description": "For kind key: an absolute path to an OpenSSH private key. Never logged; an unreadable or relative path is refused."
                        }
                    },
                    "required": ["kind"],
                    "additionalProperties": false
                },
                "host_key": {
                    "type": "string",
                    "description": "For kind ssh: the SHA-256 fingerprint the server's key must match, e.g. SHA256:... . When given, no trust-on-first-use is performed, and a mismatch is refused before authentication."
                },
                "baud": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": u32::MAX,
                    "description": format!("For kind serial: bits per second (default {}). A rate the port rejects is an error, not a fallback.", terminal::DEFAULT_BAUD)
                },
                "parity": {
                    "type": "string",
                    "enum": ["none", "odd", "even"],
                    "description": "For kind serial: the parity mode (default none)."
                },
                "data_bits": {
                    "type": "integer",
                    "enum": [5, 6, 7, 8],
                    "description": format!("For kind serial: data bits per character (default {}).", terminal::DEFAULT_DATA_BITS)
                },
                "stop_bits": {
                    "type": "integer",
                    "enum": [1, 2],
                    "description": format!("For kind serial: stop bits per character (default {}).", terminal::DEFAULT_STOP_BITS)
                },
                "rows": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": terminal::MAX_DIMENSION,
                    "description": format!("Terminal rows (default {DEFAULT_ROWS}).")
                },
                "cols": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": terminal::MAX_DIMENSION,
                    "description": format!("Terminal columns (default {DEFAULT_COLS}).")
                }
            },
            "required": ["kind"],
            "additionalProperties": false
        }),
        "terminal.write" => json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The id terminal.create returned."
                },
                "data": {
                    "type": "string",
                    "description": "UTF-8 text for the session's input; write \"\\n\" to run a command. At most 1 MiB per call, and a write that cannot finish in 5 seconds is refused."
                }
            },
            "required": ["id", "data"],
            "additionalProperties": false
        }),
        "terminal.read" => json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The id terminal.create returned."
                },
                "cursor": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Read output after this cursor. 0 (the default) reads all retained output; the returned cursor is the next call's value."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        }),
        "terminal.resize" => json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The id terminal.create returned."
                },
                "rows": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": terminal::MAX_DIMENSION,
                    "description": "The new row count."
                },
                "cols": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": terminal::MAX_DIMENSION,
                    "description": "The new column count."
                }
            },
            "required": ["id", "rows", "cols"],
            "additionalProperties": false
        }),
        "terminal.close" => json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The id terminal.create returned. Its shell is killed and the id is not reused."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        }),
        "terminal.list" => json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
        "terminal.screen" => json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "A session id from terminal.create or terminal.list."
                },
                "lines": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": terminal::MAX_SCREEN_LINES,
                    "description": format!("How many trailing lines to return (default {DEFAULT_SCREEN_LINES}).")
                }
            },
            "required": ["id"],
            "additionalProperties": false
        }),
        "browser.list" => json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
        "browser.open" => json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to load; a data: URL works without a network. At most 8 tabs may be open."
                }
            },
            "required": ["url"],
            "additionalProperties": false
        }),
        "browser.navigate" => json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "The target id browser.list or browser.open returned."
                },
                "url": {
                    "type": "string",
                    "description": "The URL to navigate the tab to."
                }
            },
            "required": ["target", "url"],
            "additionalProperties": false
        }),
        "browser.eval" => json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "The target id browser.list or browser.open returned."
                },
                "expression": {
                    "type": "string",
                    "description": "JavaScript evaluated in the page's current document; its value is returned (at most 1 MiB)."
                }
            },
            "required": ["target", "expression"],
            "additionalProperties": false
        }),
        "browser.screenshot" => json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "The target id browser.list or browser.open returned. The result is a base64 PNG of at most 8 MiB."
                }
            },
            "required": ["target"],
            "additionalProperties": false
        }),
        "browser.close" => json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "The target id browser.list or browser.open returned; the browser stays running."
                }
            },
            "required": ["target"],
            "additionalProperties": false
        }),
        "browser.shutdown" => json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
        "monitor.add" => json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["tcp", "http"],
                    "description": "tcp makes a TCP connection; http performs a GET of path."
                },
                "host": {
                    "type": "string",
                    "minLength": 1,
                    "description": "IP address or name, e.g. \"192.168.1.1\" or \"example.com\"; no scheme and no brackets."
                },
                "port": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 65535,
                    "description": "The TCP port the service listens on."
                },
                "path": {
                    "type": "string",
                    "description": "For an http target, the path to GET (default \"/\"); it is normalized to start with a slash."
                },
                "expect": {
                    "type": "string",
                    "description": "For an http target, text the body MUST contain. A page that answers 200 without it is down."
                },
                "interval_ms": {
                    "type": "integer",
                    "minimum": monitor::MIN_INTERVAL_MS,
                    "description": format!("How often to probe, in milliseconds; the minimum is {}.", monitor::MIN_INTERVAL_MS)
                }
            },
            "required": ["kind", "host", "port", "interval_ms"],
            "additionalProperties": false
        }),
        "monitor.remove" => json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The target id monitor.list returned. Its in-memory series is discarded."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        }),
        "monitor.list" => json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
        "monitor.probe" => json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The target id monitor.list returned; the probe is recorded in its series."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        }),
        "monitor.status" => json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The target id monitor.list returned; its summary covers the last 60 probes."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        }),
        "runs.exec" => json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The program to run (argv[0]); with shell true, the whole line the platform shell parses."
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Arguments passed as an argv array, never word-split or expanded; empty when shell is true."
                },
                "cwd": {
                    "type": "string",
                    "description": "A directory relative to the files root where the process starts; it constrains the start, not what the process can reach. Defaults to the root."
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": runs::MAX_TIMEOUT_MS,
                    "description": format!("How long the command may run (default {} ms). On expiry the process group is killed and timed_out is true.", runs::DEFAULT_TIMEOUT_MS)
                },
                "env": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "description": "Environment variables for this process, added to the device's own (at most 32, each value at most 4096 bytes)."
                },
                "shell": {
                    "type": "boolean",
                    "description": "Run command through the platform shell (sh -c on Unix, cmd /C on Windows); false runs it as an argv array with no shell."
                }
            },
            "required": ["command"],
            "additionalProperties": false
        }),
        "runs.list" => json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": runs::MAX_LIST_LIMIT,
                    "description": format!("How many history rows to return, newest first (default {}).", runs::DEFAULT_LIST_LIMIT)
                }
            },
            "additionalProperties": false
        }),
        "runs.get" => json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "A run id runs.exec or runs.list returned; output bodies are not stored."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        }),
        "system.process_list" => json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Only processes whose executable name contains this text, case-insensitively."
                }
            },
            "additionalProperties": false
        }),
        "system.process_kill" => json!({
            "type": "object",
            "properties": {
                "pid": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "A process id; name a pid or a name, not both."
                },
                "name": {
                    "type": "string",
                    "description": "An exact executable name, case-insensitive; every match is ended."
                }
            },
            "additionalProperties": false
        }),
        "system.net_test" => json!({
            "type": "object",
            "properties": {
                "host": {
                    "type": "string",
                    "description": "A host name or IP address to resolve and connect to."
                },
                "port": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 65535,
                    "description": "The TCP port to connect to."
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": system::MAX_TIMEOUT_MS,
                    "description": format!("The total time to spend resolving and connecting (default {DEFAULT_NET_TIMEOUT_MS}).")
                }
            },
            "required": ["host", "port"],
            "additionalProperties": false
        }),
        "host.facts" => json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
        "screen.capture" => json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
        "state.get" => json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
        _ => json!({ "type": "object" }),
    }
}

/// Why a call did not produce a successful text result.
enum ToolError {
    /// The arguments do not match the tool's input schema.
    Arguments(String),
    /// The capability refused or failed; the sentence is for the client to render.
    Refused(String),
    /// The registry declares a tool this surface cannot run.
    Internal(String),
}

/// Parse a call's arguments, naming the argument schema rather than echoing a value.
///
/// `serde_json`'s own message quotes the offending value (`invalid type: string
/// "SENTINELVALUE"`), and a client can put a secret in the wrong field. Only the error category is
/// reported, so neither the audit nor the response can carry an argument value.
fn parse<T: DeserializeOwned>(arguments: Map<String, Value>, tool: &str) -> Result<T, ToolError> {
    serde_json::from_value(Value::Object(arguments)).map_err(|error| {
        let reason = match error.classify() {
            serde_json::error::Category::Data => {
                "the arguments do not match the tool's input schema"
            }
            serde_json::error::Category::Syntax => "the arguments are not valid JSON",
            serde_json::error::Category::Eof => "the arguments are incomplete JSON",
            serde_json::error::Category::Io => "the arguments could not be read",
        };
        ToolError::Arguments(format!("{tool}: {reason}"))
    })
}

/// Refuse the serial framing fields on a kind that has no serial line.
///
/// Ignoring them would make `{"kind":"pty","baud":9600}` look like a successful create while the
/// caller believes it configured something; every kind-specific field is checked rather than
/// silently dropped.
fn reject_serial_framing(arguments: &PtyCreateArgs, name: &str) -> Result<(), ToolError> {
    if arguments.baud.is_some()
        || arguments.parity.is_some()
        || arguments.data_bits.is_some()
        || arguments.stop_bits.is_some()
    {
        return Err(ToolError::Arguments(format!(
            "{name}: baud, parity, data_bits and stop_bits are for kind serial"
        )));
    }
    Ok(())
}

/// A capability error as the tool-level refusal the client sees.
fn refused(tool: &str, error: impl std::fmt::Display) -> ToolError {
    ToolError::Refused(format!("{tool}: {error}"))
}

/// The arguments `files.read` and `files.list` take.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathArgs {
    path: String,
}

/// The arguments `files.write` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    path: String,
    content: String,
    /// How `content` encodes the bytes: text (the default) or base64.
    #[serde(default)]
    encoding: Encoding,
}

/// The two encodings the file tools accept and return.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Encoding {
    /// UTF-8 text: the default, and what a human-readable file is.
    #[default]
    Utf8,
    /// Base64, for bytes that are not text.
    Base64,
}

/// The arguments `files.read` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    path: String,
    /// How to return the bytes: text (the default) or base64.
    #[serde(default)]
    encoding: Encoding,
}

/// The arguments `memory.append` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppendArgs {
    text: String,
    /// Tags are optional: a plain note is still a fact worth remembering.
    #[serde(default)]
    tags: Vec<String>,
}

/// The arguments `memory.update` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateArgs {
    id: String,
    text: String,
    /// The complete replacement, not a patch: an update that omitted tags would silently clear
    /// them, so the schema requires the field and names [] as the way to clear.
    tags: Vec<String>,
}

/// The arguments `memory.delete` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteArgs {
    id: String,
}

/// The arguments `memory.search` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    query: String,
    limit: Option<usize>,
}

/// The arguments `memory.list` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryListArgs {
    limit: Option<usize>,
    tag: Option<String>,
}

/// A tool whose input schema declares no properties.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

/// The arguments `terminal.screen` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScreenArgs {
    id: String,
    lines: Option<usize>,
}

/// The arguments `system.process_list` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessListArgs {
    name: Option<String>,
}

/// The arguments `system.process_kill` takes: one target, a pid or a name.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessKillArgs {
    pid: Option<u32>,
    name: Option<String>,
}

/// The arguments `system.net_test` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NetTestArgs {
    host: String,
    port: u16,
    timeout_ms: Option<u64>,
}

/// The arguments `terminal.create` takes; only the kind is required, because every other field has
/// a platform default.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PtyCreateArgs {
    kind: String,
    /// The local program for `kind: "pty"`.
    shell: Option<String>,
    /// The remote target for `kind: "ssh"`.
    host: Option<String>,
    /// The remote TCP port for `kind: "ssh"`, or the OS port path for `kind: "serial"`. The two
    /// kinds name different things with one field, so a JSON number and a JSON string are both
    /// accepted here and each kind refuses the other's form by name.
    port: Option<PortArg>,
    user: Option<String>,
    auth: Option<ssh::Auth>,
    host_key: Option<String>,
    /// The serial framing; all optional, default 115200 8N1.
    baud: Option<u32>,
    parity: Option<String>,
    data_bits: Option<u8>,
    stop_bits: Option<u8>,
    rows: Option<u16>,
    cols: Option<u16>,
}

/// The `port` a create names: a TCP port for SSH, a path for serial.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum PortArg {
    /// `kind: "ssh"` names a TCP port by number.
    Ssh(u16),
    /// `kind: "serial"` names an OS port path, e.g. `COM3` or `/dev/ttyUSB0`.
    Serial(String),
}

/// The arguments `terminal.write` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PtyWriteArgs {
    id: String,
    data: String,
}

/// The arguments `terminal.read` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PtyReadArgs {
    id: String,
    cursor: Option<u64>,
}

/// The arguments `terminal.resize` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PtyResizeArgs {
    id: String,
    rows: u16,
    cols: u16,
}

/// The arguments `terminal.close` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PtyIdArgs {
    id: String,
}

/// The arguments `browser.open` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserOpenArgs {
    url: String,
}

/// The arguments `browser.close` and `browser.screenshot` take.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserTargetArgs {
    target: String,
}

/// The arguments `browser.navigate` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserNavigateArgs {
    target: String,
    url: String,
}

/// The arguments `browser.eval` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserEvalArgs {
    target: String,
    expression: String,
}

/// The arguments `monitor.add` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MonitorAddArgs {
    kind: monitor::Kind,
    host: String,
    port: u16,
    path: Option<String>,
    expect: Option<String>,
    interval_ms: u64,
}

/// The arguments `monitor.remove`, `monitor.probe` and `monitor.status` take.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MonitorIdArgs {
    id: String,
}

/// The arguments `runs.exec` takes.
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunsListArgs {
    limit: Option<usize>,
}

/// The arguments `runs.get` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunsIdArgs {
    id: String,
}

/// Parse the session id a client sent back, refusing a spelling this device never mints.
///
/// The id is not echoed in the refusal for the same reason no other argument value is: a client can
/// put anything in the field, and the response must not become a reflection.
fn session_id(tool: &str, text: &str) -> Result<terminal::SessionId, ToolError> {
    terminal::SessionId::parse(text).ok_or_else(|| {
        ToolError::Refused(format!(
            "{tool}: the id is not a session id; use one terminal.list returned"
        ))
    })
}

/// One `begin` line: the authorization decision was made and the capability is about to run.
#[derive(Serialize)]
struct AuditBegin<'a> {
    id: u64,
    ts_ms: i64,
    event: &'static str,
    tool: &'a str,
    decision: &'a str,
    /// How the call arrived. Recorded on every line, because two calls with the same tool and the
    /// same decision are not the same event when one came from the loopback socket and one from a
    /// peer: only this field tells an operator which door was used.
    transport: &'static str,
    /// The mesh caller's pinned public key in hex. Absent on every other wire, where the transport
    /// authenticates the console or the local owner rather than a peer.
    #[serde(skip_serializing_if = "Option::is_none")]
    peer: Option<&'a str>,
}

/// One `end` line: the call returned. `error_kind` is a closed set of names, never a message and
/// never an argument value.
#[derive(Serialize)]
struct AuditEnd<'a> {
    id: u64,
    ts_ms: i64,
    event: &'static str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'a str>,
    transport: &'static str,
    /// The mesh caller's pinned public key in hex; absent on the console and loopback wires.
    #[serde(skip_serializing_if = "Option::is_none")]
    peer: Option<&'a str>,
}

/// The append-only event log at `<data>/events.jsonl`.
///
/// Two lines per call: `begin` is written before dispatch, `end` after. Each line is one
/// `write_all` on an append handle while this lock is held, so two sessions cannot interleave halves
/// of a line. It is also the lock `terminal` takes when it writes `state.sessions`: the
/// session table and the side-effect record have one writer, so a create's row and its audit line
/// cannot be reordered by a concurrent close.
///
/// A failed write goes to stderr and never fails the call: after dispatch the capability
/// has already run, and a lost audit line must not turn a completed side effect into a request the
/// client retries.
///
/// The file is capped at [`AUDIT_MAX_BYTES`]. When the next line would cross the cap, the current
/// file is renamed to `events.1.jsonl` (replacing any previous `.1`) and a new `events.jsonl`
/// starts. One generation is kept, so disk use is at most about twice the cap; lines older than the
/// previous generation are discarded by design. If rotation fails, the line is still appended: a
/// growing audit is a worse warning than a missing one. A failed `begin` returns `false` and the
/// caller refuses the call; a failed `end` is reported and the call still returns, because after
/// dispatch the capability has already run.
struct Audit {
    path: PathBuf,
    rotated: PathBuf,
    max_bytes: u64,
    next_id: AtomicU64,
    /// Shared with the terminal capability: see the struct's doc comment.
    append: Arc<tokio::sync::Mutex<()>>,
}

impl Audit {
    fn new(data: &Path) -> Audit {
        Audit::with_cap(data.join("events.jsonl"), AUDIT_MAX_BYTES)
    }

    fn with_cap(path: PathBuf, max_bytes: u64) -> Audit {
        Audit {
            rotated: path.with_extension("1.jsonl"),
            path,
            max_bytes,
            next_id: AtomicU64::new(1),
            append: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// The lock a capability that writes the state document shares with this log.
    fn write_lock(&self) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(&self.append)
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Write the authorization line. `false` means no line was written.
    async fn begin(
        &self,
        id: u64,
        ts_ms: i64,
        tool: &str,
        decision: &str,
        arrival: Arrival,
        peer: Option<&str>,
    ) -> bool {
        self.append_line(&AuditBegin {
            id,
            ts_ms,
            event: "begin",
            tool: audit_tool(tool),
            decision,
            transport: arrival.as_str(),
            peer,
        })
        .await
    }

    async fn end(
        &self,
        id: u64,
        ts_ms: i64,
        ok: bool,
        error_kind: Option<&str>,
        arrival: Arrival,
        peer: Option<&str>,
    ) {
        let _written = self
            .append_line(&AuditEnd {
                id,
                ts_ms,
                event: "end",
                ok,
                error_kind,
                transport: arrival.as_str(),
                peer,
            })
            .await;
    }

    async fn append_line<T: Serialize>(&self, entry: &T) -> bool {
        let mut line = match serde_json::to_string(entry) {
            Ok(line) => line,
            Err(error) => {
                eprintln!("summrise-device: could not serialize an audit line: {error}");
                return false;
            }
        };
        line.push('\n');

        let _writing = self.append.lock().await;
        if let Some(parent) = self.path.parent() {
            if let Err(error) = tokio::fs::create_dir_all(parent).await {
                eprintln!(
                    "summrise-device: could not create the audit directory {}: {error}",
                    parent.display()
                );
                return false;
            }
        }
        if let Err(error) = self.rotate_if_needed(line.len() as u64).await {
            eprintln!(
                "summrise-device: could not rotate {}: {error}",
                self.path.display()
            );
        }
        let result = async {
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .await?;
            file.write_all(line.as_bytes()).await?;
            // `tokio::fs::File::write_all` returns once the bytes are in its blocking task's
            // buffer, not once the OS write has completed. `flush` waits for that operation, so an
            // awaited `begin` really is a line on the file before dispatch can run.
            file.flush().await
        }
        .await;
        if let Err(error) = result {
            eprintln!(
                "summrise-device: could not append to {}: {error}",
                self.path.display()
            );
            return false;
        }
        true
    }

    /// Rotate the current file to `events.1.jsonl` when the next line would cross the cap.
    async fn rotate_if_needed(&self, incoming: u64) -> std::io::Result<()> {
        let current = match tokio::fs::metadata(&self.path).await {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if current == 0 || current.saturating_add(incoming) <= self.max_bytes {
            return Ok(());
        }

        match tokio::fs::remove_file(&self.rotated).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        tokio::fs::rename(&self.path, &self.rotated).await
    }
}

/// At most [`MAX_AUDIT_TOOL_BYTES`] of a tool name, cut on a character boundary.
///
/// The registry's names are short; only an unknown name can be long. Truncating it keeps one audit
/// line bounded, which is what makes the file cap meaningful.
fn audit_tool(tool: &str) -> &str {
    if tool.len() <= MAX_AUDIT_TOOL_BYTES {
        return tool;
    }
    let mut end = MAX_AUDIT_TOOL_BYTES;
    while !tool.is_char_boundary(end) {
        end -= 1;
    }
    &tool[..end]
}

#[cfg(test)]
mod tests {
    use std::fs;
    // The PTY tests are Unix (or Linux) only, and they are the only users of these on Windows.
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
    use serde_json::{json, Value};
    use tokio::net::TcpListener;
    use tower::ServiceExt;

    use base64::Engine as _;

    use summrise_core::CAPABILITIES;

    use super::BASE64;

    #[cfg(target_os = "linux")]
    use crate::capabilities::browser;
    use crate::capabilities::{monitor, runs, terminal};
    use crate::config::Config;
    use crate::store::Store;
    use crate::test_support::TempDir;

    use super::{
        dispatch_supports, handler, registry_tools, HANDLERS, MAX_BODY_BYTES, MAX_FILE_BYTES,
    };

    const TOKEN: [u8; 32] = [0x6b; 32];

    fn token_hex() -> String {
        TOKEN.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// A running device on an ephemeral loopback port, with the real router (token gate included).
    struct Device {
        mcp_url: String,
        config: Config,
        /// Kept alive so every path the config names exists for the whole test.
        _dir: TempDir,
        /// The serving task; the test runtime drops it.
        _server: tokio::task::JoinHandle<()>,
    }

    async fn start(label: &str) -> Device {
        start_with_limits(label, terminal::Limits::default()).await
    }

    /// The same device, with the terminal capability's limits replaced.
    async fn start_with_limits(label: &str, limits: terminal::Limits) -> Device {
        let dir = TempDir::new(label);
        let install = dir.join("install");
        let data = dir.join("data");
        let files_root = dir.join("workspace");
        for path in [&install, &data, &files_root] {
            fs::create_dir_all(path).expect("the fixture directory is creatable");
        }
        let token_file = dir.join("token");
        fs::write(&token_file, token_hex()).expect("the token fixture is written");

        let config = Config {
            install,
            data,
            port: 0,
            token_file,
            files_root,
            release_version: None,
            release_sha: None,
            release_channel: None,
            console_url: None,
            console_token_file: None,
            mesh_broker: None,
            mesh_relay: None,
            mesh_stun: None,
        };
        let store = Store::new(&config.install, &config.data, config.release());
        let app = crate::http::router_with_limits(&config, store, limits);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let address = listener.local_addr().expect("the bound address");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Device {
            mcp_url: format!("http://{address}/mcp"),
            config,
            _dir: dir,
            _server: server,
        }
    }

    /// The smallest streamable-HTTP MCP client this test needs: one JSON-RPC message per POST,
    /// the session id from `initialize`, SSE framing read as the server sends it.
    struct Client {
        http: reqwest::Client,
        url: String,
        token: Option<String>,
        session: Option<String>,
        protocol_version: Option<String>,
        next_id: u64,
    }

    impl Client {
        fn with_token(device: &Device, token: Option<String>) -> Client {
            Client {
                http: reqwest::Client::new(),
                url: device.mcp_url.clone(),
                token,
                session: None,
                protocol_version: None,
                next_id: 1,
            }
        }

        fn authorized(device: &Device) -> Client {
            Client::with_token(device, Some(token_hex()))
        }

        async fn try_post(&self, body: Value) -> reqwest::Result<reqwest::Response> {
            let mut request = self
                .http
                .post(&self.url)
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "application/json, text/event-stream")
                .body(body.to_string());
            if let Some(token) = &self.token {
                request = request.header(AUTHORIZATION, format!("Bearer {token}"));
            }
            if let Some(session) = &self.session {
                request = request.header("mcp-session-id", session);
            }
            if let Some(version) = &self.protocol_version {
                request = request.header("mcp-protocol-version", version);
            }
            request.send().await
        }

        async fn post(&self, body: Value) -> reqwest::Response {
            self.try_post(body).await.expect("the MCP server answers")
        }

        async fn initialize(&mut self) -> Value {
            let id = self.take_id();
            let response = self
                .post(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": { "name": "device-test", "version": "0.1.0" }
                    }
                }))
                .await;
            assert_eq!(response.status(), 200, "the handshake is answered");
            let session = response
                .headers()
                .get("mcp-session-id")
                .expect("initialize binds a session")
                .to_str()
                .expect("the session id is ASCII")
                .to_owned();
            let message = message_json(response).await;
            self.session = Some(session);
            self.protocol_version = message["result"]["protocolVersion"]
                .as_str()
                .map(str::to_owned);

            let initialized = self
                .post(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
                .await;
            assert_eq!(
                initialized.status(),
                202,
                "the initialized notification is accepted"
            );

            message
        }

        async fn request(&mut self, method: &str, params: Value) -> reqwest::Response {
            let id = self.take_id();
            self.post(json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params
            }))
            .await
        }

        /// One `tools/call`, returning the JSON-RPC envelope (a result or an error).
        async fn call(&mut self, name: &str, arguments: Value) -> Value {
            let response = self
                .request(
                    "tools/call",
                    json!({ "name": name, "arguments": arguments }),
                )
                .await;
            assert_eq!(response.status(), 200, "a tool call is answered");
            message_json(response).await
        }

        fn take_id(&mut self) -> u64 {
            let id = self.next_id;
            self.next_id += 1;
            id
        }
    }

    /// The one JSON-RPC message an SSE response carries; the priming event has no data.
    async fn message_json(response: reqwest::Response) -> Value {
        let body = response.text().await.expect("the SSE body is readable");
        let data = body
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .rfind(|data| !data.is_empty())
            .unwrap_or_else(|| panic!("no JSON-RPC message in {body:?}"));
        serde_json::from_str(data).expect("the SSE data is JSON")
    }

    fn text_of(message: &Value) -> &str {
        message["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("a text result: {message}"))
    }

    fn audit_lines(device: &Device) -> Vec<Value> {
        let path = device.config.data.join("events.jsonl");
        match fs::read_to_string(&path) {
            Ok(contents) => contents
                .lines()
                .map(|line| serde_json::from_str(line).expect("an audit line is JSON"))
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("the audit log is readable: {error}"),
        }
    }

    #[test]
    fn every_registry_tool_is_advertised_with_its_description_and_a_schema() {
        let tools = registry_tools();

        let registry: Vec<&str> = CAPABILITIES
            .iter()
            .flat_map(|capability| capability.tools)
            .map(|tool| tool.name)
            .collect();
        let advertised: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        assert_eq!(
            advertised, registry,
            "tools/list is a reading of the registry, in its order"
        );

        for tool in &tools {
            assert!(
                tool.description
                    .as_deref()
                    .is_some_and(|text| !text.is_empty()),
                "{} has no description",
                tool.name
            );
            assert_eq!(
                tool.input_schema["type"], "object",
                "{}'s input schema is an object",
                tool.name
            );
            assert!(
                tool.input_schema["properties"].is_object(),
                "{}'s input schema names its arguments",
                tool.name
            );
        }

        let schema = |name: &str| {
            tools
                .iter()
                .find(|tool| tool.name == name)
                .unwrap_or_else(|| panic!("{name} is advertised"))
                .input_schema
                .clone()
        };
        assert_eq!(schema("files.read")["required"], json!(["path"]));
        assert_eq!(
            schema("files.write")["required"],
            json!(["path", "content"]),
            "a write needs both the destination and its bytes"
        );
        assert_eq!(
            schema("files.mkdir")["required"],
            json!(["path"]),
            "a mkdir names the one directory it creates"
        );
        assert_eq!(schema("files.list")["required"], json!(["path"]));
        assert_eq!(schema("memory.append")["required"], json!(["text"]));
        assert_eq!(
            schema("memory.append")["properties"]["tags"]["type"],
            "array"
        );
        assert_eq!(
            schema("memory.append")["properties"]["text"]["minLength"],
            1,
            "an empty text is not a fact to remember"
        );
        assert!(
            schema("memory.append")["properties"]["text"]["description"]
                .as_str()
                .is_some_and(|description| description.contains("empty")),
            "the append schema says an empty text is refused"
        );
        assert_eq!(
            schema("memory.update")["required"],
            json!(["id", "text", "tags"]),
            "an update names the entry and the whole replacement"
        );
        assert_eq!(
            schema("memory.update")["properties"]["tags"]["type"],
            "array",
            "an update carries the complete tag list, not a patch"
        );
        assert_eq!(
            schema("memory.update")["properties"]["text"]["minLength"],
            1,
            "an empty text cannot clear an entry, because a delete is how it is retired"
        );
        assert!(
            schema("memory.update")["properties"]["text"]["description"]
                .as_str()
                .is_some_and(|description| description.contains("empty")),
            "the update schema says an empty text is refused"
        );
        assert_eq!(
            schema("memory.delete")["required"],
            json!(["id"]),
            "a delete names the one entry it retires"
        );
        assert_eq!(
            schema("memory.search")["required"],
            json!(["query"]),
            "memory.search's limit is optional"
        );
        assert_eq!(
            schema("memory.search")["properties"]["limit"]["type"],
            "integer"
        );
        // A create names the kind it starts; the size and shell are optional, because the
        // capability supplies the platform defaults.
        assert_eq!(
            schema("terminal.create")["required"],
            json!(["kind"]),
            "terminal.create names the kind of session it starts"
        );
        assert_eq!(
            schema("terminal.create")["properties"]["kind"]["enum"],
            json!(["pty", "ssh", "serial"]),
            "every session kind is on the wire"
        );
        assert_eq!(
            schema("terminal.create")["properties"]["shell"]["type"],
            "string"
        );
        assert_eq!(
            schema("terminal.create")["properties"]["port"]["oneOf"],
            json!([
                {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 65535,
                    "description": "For kind ssh: the server's TCP port."
                },
                {
                    "type": "string",
                    "minLength": 1,
                    "description": "For kind serial: the OS port path, e.g. COM3 on Windows or /dev/ttyUSB0 on Unix. The capability never enumerates or guesses a port."
                }
            ]),
            "ssh names a TCP port by number and serial names an OS path by string"
        );
        for field in ["baud", "parity", "data_bits", "stop_bits"] {
            assert!(
                schema("terminal.create")["properties"][field].is_object(),
                "kind serial's framing field {field} is on the wire"
            );
        }
        assert_eq!(
            schema("terminal.write")["required"],
            json!(["id", "data"]),
            "a write names its session and its bytes"
        );
        assert_eq!(
            schema("terminal.read")["required"],
            json!(["id"]),
            "terminal.read's cursor is optional (0 means the retained output)"
        );
        assert_eq!(
            schema("terminal.read")["properties"]["cursor"]["type"],
            "integer"
        );
        assert_eq!(
            schema("terminal.resize")["required"],
            json!(["id", "rows", "cols"])
        );
        assert_eq!(schema("terminal.close")["required"], json!(["id"]));
        assert_eq!(
            schema("terminal.list")["properties"],
            json!({}),
            "a list has no arguments"
        );
        assert_eq!(
            schema("browser.open")["required"],
            json!(["url"]),
            "an open names the URL it loads"
        );
        assert_eq!(
            schema("browser.navigate")["required"],
            json!(["target", "url"])
        );
        assert_eq!(
            schema("browser.eval")["required"],
            json!(["target", "expression"]),
            "an evaluation names both the tab and the expression"
        );
        assert_eq!(schema("browser.screenshot")["required"], json!(["target"]));
        assert_eq!(schema("browser.close")["required"], json!(["target"]));
        assert_eq!(
            schema("browser.list")["properties"],
            json!({}),
            "a list has no arguments"
        );
        assert_eq!(
            schema("browser.shutdown")["properties"],
            json!({}),
            "a shutdown has no arguments"
        );
        assert_eq!(
            schema("monitor.add")["required"],
            json!(["kind", "host", "port", "interval_ms"]),
            "an add names what to probe and how often"
        );
        assert_eq!(
            schema("monitor.add")["properties"]["kind"]["enum"],
            json!(["tcp", "http"]),
            "the only probe kinds are tcp and http"
        );
        assert_eq!(
            schema("monitor.add")["properties"]["interval_ms"]["minimum"],
            monitor::MIN_INTERVAL_MS,
            "the schema states the capability's minimum interval"
        );
        assert_eq!(
            schema("monitor.list")["properties"],
            json!({}),
            "a list has no arguments"
        );
        for name in ["monitor.remove", "monitor.probe", "monitor.status"] {
            assert_eq!(
                schema(name)["required"],
                json!(["id"]),
                "{name} names its target"
            );
        }
        assert_eq!(
            schema("runs.exec")["required"],
            json!(["command"]),
            "a run names the program; every other field has a default"
        );
        assert_eq!(
            schema("runs.exec")["properties"]["timeout_ms"]["maximum"],
            runs::MAX_TIMEOUT_MS,
            "the schema states the capability's maximum timeout"
        );
        assert_eq!(
            schema("runs.exec")["properties"]["shell"]["type"],
            "boolean",
            "shell is the explicit opt-in to shell parsing"
        );
        assert_eq!(
            schema("runs.list")["properties"]["limit"]["maximum"],
            runs::MAX_LIST_LIMIT,
            "the schema states the history list's cap"
        );
        assert_eq!(schema("runs.get")["required"], json!(["id"]));
        assert_eq!(
            schema("state.get")["properties"],
            json!({}),
            "state.get takes no arguments: the device reads its own document"
        );
    }

    #[tokio::test]
    async fn initialize_negotiates_the_protocol_version() {
        let device = start("mcp-initialize").await;
        let mut client = Client::authorized(&device);

        let message = client.initialize().await;

        assert_eq!(
            message["result"]["protocolVersion"], "2025-06-18",
            "a version this server knows is the version it answers: {message}"
        );
        assert_eq!(message["result"]["serverInfo"]["name"], "summrise-device");
        assert_eq!(
            message["result"]["serverInfo"]["version"],
            env!("CARGO_PKG_VERSION")
        );
        assert!(
            message["result"]["capabilities"]["tools"].is_object(),
            "the server advertises tools: {message}"
        );
        assert_eq!(
            message["result"]["capabilities"]["tools"]["listChanged"], false,
            "the registry does not change while the process serves"
        );
    }

    #[tokio::test]
    async fn the_tool_list_is_exactly_the_registrys_tools() {
        let device = start("mcp-tools-list").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let response = client.request("tools/list", json!({})).await;
        assert_eq!(response.status(), 200);
        let message = message_json(response).await;
        let tools = message["result"]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("tools/list answers tools: {message}"));

        let names: Vec<&str> = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("a tool name"))
            .collect();
        assert_eq!(
            names,
            [
                "files.read",
                "files.write",
                "files.mkdir",
                "files.list",
                "memory.append",
                "memory.update",
                "memory.delete",
                "memory.search",
                "memory.list",
                "memory.export",
                "terminal.create",
                "terminal.write",
                "terminal.read",
                "terminal.resize",
                "terminal.close",
                "terminal.list",
                "terminal.screen",
                "browser.list",
                "browser.open",
                "browser.navigate",
                "browser.eval",
                "browser.screenshot",
                "browser.close",
                "browser.shutdown",
                "monitor.add",
                "monitor.remove",
                "monitor.list",
                "monitor.probe",
                "monitor.status",
                "runs.exec",
                "runs.list",
                "runs.get",
                "system.process_list",
                "system.process_kill",
                "system.net_test",
                "host.facts",
                "screen.capture",
                "state.get",
            ],
            "the wire surface is the registry's surface"
        );
        for tool in tools {
            let name = tool["name"].as_str().expect("a tool name");
            let declared =
                summrise_core::registry::tool(name).expect("the tool is in the registry");
            assert_eq!(
                tool["description"], declared.description,
                "{name}'s description is the registry's sentence, not a second copy"
            );
            assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
        }
    }

    #[tokio::test]
    async fn state_get_returns_the_document_and_health_over_mcp() {
        let device = start("mcp-state-get").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let reply = client.call("state.get", json!({})).await;

        assert!(reply.get("error").is_none(), "{reply}");
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        let document: Value =
            serde_json::from_str(text_of(&reply)).expect("state.get returns one JSON document");
        assert_eq!(
            document["state"]["schema"],
            summrise_core::SCHEMA_VERSION,
            "the state half is the document /api/state would answer: {document}"
        );
        assert_eq!(
            document["state"]["install"]["dir"],
            device.config.install.display().to_string()
        );
        assert_eq!(document["health"]["status"], "ok");
        assert_eq!(
            document["health"]["version"],
            env!("CARGO_PKG_VERSION"),
            "health names the release this process was started as"
        );
        assert_eq!(
            document["health"]["pid"].as_u64(),
            Some(u64::from(std::process::id()))
        );
        assert_eq!(
            document["health"]["poisoned"], false,
            "a readable document is not poisoned: {document}"
        );
        let events = document["events"]
            .as_array()
            .unwrap_or_else(|| panic!("state.get answers an event tail: {document}"));
        assert_eq!(
            events.len(),
            1,
            "this call is audited before it runs, so it is the quiet device's only event: {document}"
        );
        assert_eq!(events[0]["event"], "begin", "{document}");
        assert_eq!(events[0]["tool"], "state.get", "{document}");
    }

    #[tokio::test]
    async fn state_get_returns_the_event_tail_oldest_first() {
        let device = start("mcp-state-events").await;
        fs::write(
            device.config.data.join("events.jsonl"),
            "{\"ts_ms\":1,\"kind\":\"repaired\",\"from\":\"baseline\"}\n\
             {\"ts_ms\":2,\"kind\":\"state_quarantined\"}\n\
             a line the device wrote but did not encode as JSON\n",
        )
        .expect("the event fixture is written");
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let reply = client.call("state.get", json!({})).await;

        assert!(reply.get("error").is_none(), "{reply}");
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        let document: Value =
            serde_json::from_str(text_of(&reply)).expect("state.get returns one JSON document");
        let events = document["events"]
            .as_array()
            .unwrap_or_else(|| panic!("state.get answers an event tail: {document}"));
        assert_eq!(
            events[..3],
            [
                json!({ "ts_ms": 1, "kind": "repaired", "from": "baseline" }),
                json!({ "ts_ms": 2, "kind": "state_quarantined" }),
                json!("a line the device wrote but did not encode as JSON"),
            ],
            "the tail is the device's lines, oldest first, and a line that is not JSON survives as text: {document}"
        );
        assert_eq!(
            events.len(),
            4,
            "the device wrote three lines and audited this call: {document}"
        );
        assert_eq!(
            events[3]["tool"], "state.get",
            "the call reading the tail is its newest event: {document}"
        );
    }

    #[tokio::test]
    async fn state_get_answers_an_empty_tail_without_an_event_log() {
        let device = start("mcp-state-no-events").await;
        assert!(
            !device.config.data.join("events.jsonl").exists(),
            "the fixture starts without an event log"
        );
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let reply = client.call("state.get", json!({})).await;

        assert!(reply.get("error").is_none(), "{reply}");
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        let document: Value =
            serde_json::from_str(text_of(&reply)).expect("state.get returns one JSON document");
        let events = document["events"]
            .as_array()
            .unwrap_or_else(|| panic!("state.get answers an event tail: {document}"));
        assert_eq!(
            events.len(),
            1,
            "a missing log contributes nothing; this call's own audit line is the only event: {document}"
        );
        assert_eq!(events[0]["event"], "begin", "{document}");
        assert_eq!(events[0]["tool"], "state.get", "{document}");
        assert_eq!(document["health"]["status"], "ok", "{document}");
    }

    #[tokio::test]
    async fn state_get_survives_an_unreadable_event_log() {
        let device = start("mcp-state-events-unreadable").await;
        // A log that is not UTF-8 is unwritable as text but still appendable, so the call reaches
        // the handler and the best-effort read is the thing under test.
        fs::write(device.config.data.join("events.jsonl"), [0xff, 0xfe])
            .expect("the unreadable fixture is written");
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let reply = client.call("state.get", json!({})).await;

        assert!(reply.get("error").is_none(), "{reply}");
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        let document: Value =
            serde_json::from_str(text_of(&reply)).expect("state.get returns one JSON document");
        assert_eq!(
            document["events"],
            json!([]),
            "an unreadable event log is an empty tail, not a failed call: {document}"
        );
        assert_eq!(
            document["health"]["status"], "ok",
            "the document half still answers: {document}"
        );
    }

    #[tokio::test]
    async fn state_get_reports_a_poisoned_document_without_failing_the_call() {
        let device = start("mcp-state-poisoned").await;
        fs::write(
            device.config.data.join("state.json"),
            "{ this is not a state document",
        )
        .expect("the corrupt fixture is written");
        fs::write(
            device.config.data.join("events.jsonl"),
            "{\"ts_ms\":7,\"kind\":\"state_quarantined\"}\n",
        )
        .expect("the event fixture is written");
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let reply = client.call("state.get", json!({})).await;

        assert!(reply.get("error").is_none(), "{reply}");
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        let document: Value =
            serde_json::from_str(text_of(&reply)).expect("the failure is a document, not an error");
        assert_eq!(
            document["state"],
            Value::Null,
            "an unreadable document is reported as absent, not invented: {document}"
        );
        assert_eq!(
            document["health"]["status"], "degraded",
            "health is the same readiness answer /api/health gives: {document}"
        );
        assert_eq!(document["health"]["poisoned"], true);
        assert!(
            document["health"]["reason"]
                .as_str()
                .is_some_and(|reason| reason.starts_with("state:")),
            "the reason names the poisoned subsystem: {document}"
        );
        assert_eq!(
            document["events"][0],
            json!({ "ts_ms": 7, "kind": "state_quarantined" }),
            "a poisoned document does not hide the events the device still has: {document}"
        );
        assert_eq!(
            document["events"][1]["tool"], "state.get",
            "the events half still answers even though the document half is poisoned: {document}"
        );
    }

    #[tokio::test]
    async fn a_file_written_through_mcp_reads_back_through_mcp() {
        let device = start("mcp-files-round-trip").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        // The root exists (the process creates it at startup); `notes` does not until mkdir.
        assert!(
            !device.config.files_root.join("notes").exists(),
            "the fixture starts without the directory the client is about to create"
        );
        let made = client.call("files.mkdir", json!({ "path": "notes" })).await;
        assert!(made.get("error").is_none(), "{made}");
        assert_eq!(made["result"]["isError"], false, "{made}");

        let written = client
            .call(
                "files.write",
                json!({ "path": "notes/todo.txt", "content": "buy milk" }),
            )
            .await;
        assert!(written.get("error").is_none(), "{written}");
        assert_eq!(written["result"]["isError"], false, "{written}");

        // Binary in, binary out: the same path, the bytes carried as base64.
        let binary = vec![0u8, 159, 146, 150, 0, 255, 10, 13];
        let encoded = BASE64.encode(&binary);
        let written = client
            .call(
                "files.write",
                json!({ "path": "notes/blob.bin", "content": encoded, "encoding": "base64" }),
            )
            .await;
        assert!(written.get("error").is_none(), "{written}");
        let decoded = client
            .call(
                "files.read",
                json!({ "path": "notes/blob.bin", "encoding": "base64" }),
            )
            .await;
        let returned = BASE64
            .decode(text_of(&decoded))
            .expect("the answer is base64");
        assert_eq!(returned, binary, "the bytes survive the round trip");

        // A text read of binary is refused with the remedy in the message, not silently mangled.
        let refused = client
            .call("files.read", json!({ "path": "notes/blob.bin" }))
            .await;
        assert_eq!(refused["result"]["isError"], true, "{refused}");
        assert!(
            text_of(&refused).contains("base64"),
            "the refusal names the encoding to ask for: {}",
            text_of(&refused)
        );

        let read = client
            .call("files.read", json!({ "path": "notes/todo.txt" }))
            .await;
        assert_eq!(text_of(&read), "buy milk");

        let listed = client.call("files.list", json!({ "path": "notes" })).await;
        let entries: Value = serde_json::from_str(text_of(&listed)).expect("list is JSON");
        let todo = entries
            .as_array()
            .expect("list is an array")
            .iter()
            .find(|entry| entry["name"] == "todo.txt")
            .expect("the text file is listed");
        assert_eq!(todo["kind"], "file");
        assert_eq!(todo["len"], 8);
    }

    #[tokio::test]
    async fn memory_appended_through_mcp_is_found_by_search() {
        let device = start("mcp-memory-round-trip").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let appended = client
            .call(
                "memory.append",
                json!({ "text": "the device remembers this", "tags": ["arch"] }),
            )
            .await;
        assert!(appended.get("error").is_none(), "{appended}");
        let appended: Value = serde_json::from_str(text_of(&appended)).expect("append is JSON");
        assert_eq!(appended["appended"], true);
        let id = appended["id"]
            .as_str()
            .expect("append answers the id it minted")
            .to_owned();
        assert_eq!(id.len(), 16, "an id is 8 random bytes as hex: {id}");

        let found = client
            .call("memory.search", json!({ "query": "remembers" }))
            .await;
        let entries: Value = serde_json::from_str(text_of(&found)).expect("search is JSON");
        assert_eq!(entries[0]["id"], id, "search names the entry it returns");
        assert_eq!(entries[0]["text"], "the device remembers this");
        assert_eq!(entries[0]["tags"], json!(["arch"]));

        let hits: Vec<String> = fs::read_to_string(device.config.data.join("memory.jsonl"))
            .expect("the append is on disk")
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(hits.len(), 1, "append-only: one entry, one line");
    }

    #[tokio::test]
    async fn memory_entries_are_corrected_and_retired_by_id_through_mcp() {
        let device = start("mcp-memory-edit").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let appended = client
            .call(
                "memory.append",
                json!({ "text": "first version", "tags": ["draft"] }),
            )
            .await;
        let appended: Value = serde_json::from_str(text_of(&appended)).expect("append is JSON");
        let id = appended["id"]
            .as_str()
            .expect("append answers the id it minted")
            .to_owned();

        let updated = client
            .call(
                "memory.update",
                json!({ "id": id, "text": "second version", "tags": ["final"] }),
            )
            .await;
        assert!(updated.get("error").is_none(), "{updated}");
        assert_eq!(updated["result"]["isError"], false, "{updated}");
        let updated: Value = serde_json::from_str(text_of(&updated)).expect("update is JSON");
        assert_eq!(
            updated,
            json!({ "id": id, "text": "second version", "tags": ["final"] }),
            "an update answers exactly the entry it wrote, and cannot change its name"
        );

        let old = client
            .call("memory.search", json!({ "query": "first version" }))
            .await;
        let old: Value = serde_json::from_str(text_of(&old)).expect("search is JSON");
        assert!(
            old.as_array().expect("search is an array").is_empty(),
            "the superseded text is not returned: {old}"
        );
        let found = client
            .call("memory.search", json!({ "query": "second version" }))
            .await;
        let found: Value = serde_json::from_str(text_of(&found)).expect("search is JSON");
        assert_eq!(found[0]["id"], id);
        assert_eq!(found[0]["tags"], json!(["final"]));

        let listed = client.call("memory.list", json!({})).await;
        let listed: Value = serde_json::from_str(text_of(&listed)).expect("list is JSON");
        assert_eq!(listed[0]["id"], id, "list names the entry it returns");
        assert_eq!(listed[0]["text"], "second version");

        let deleted = client.call("memory.delete", json!({ "id": id })).await;
        assert!(deleted.get("error").is_none(), "{deleted}");
        assert_eq!(deleted["result"]["isError"], false, "{deleted}");
        let deleted: Value = serde_json::from_str(text_of(&deleted)).expect("delete is JSON");
        assert_eq!(deleted, json!({ "deleted": true, "id": id }));

        let listed = client.call("memory.list", json!({})).await;
        let listed: Value = serde_json::from_str(text_of(&listed)).expect("list is JSON");
        assert!(
            listed.as_array().expect("list is an array").is_empty(),
            "a retired entry is gone from every read: {listed}"
        );

        let again = client.call("memory.delete", json!({ "id": id })).await;
        assert_eq!(again["result"]["isError"], true, "{again}");
        assert!(
            text_of(&again).contains(&id),
            "the refusal names the id that is no longer live: {}",
            text_of(&again)
        );
    }

    #[tokio::test]
    async fn an_update_of_an_unknown_id_is_a_tool_error_that_names_it() {
        let device = start("mcp-memory-no-such-entry").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let refused = client
            .call(
                "memory.update",
                json!({ "id": "no-such-id", "text": "nothing", "tags": [] }),
            )
            .await;

        assert!(refused.get("error").is_none(), "the tool ran: {refused}");
        assert_eq!(refused["result"]["isError"], true, "{refused}");
        assert!(
            text_of(&refused).contains("no-such-id"),
            "a refusal names the id the caller named: {}",
            text_of(&refused)
        );

        let refused = client
            .call("memory.delete", json!({ "id": "also-missing" }))
            .await;
        assert_eq!(refused["result"]["isError"], true, "{refused}");
        assert!(
            text_of(&refused).contains("also-missing"),
            "a delete refusal names its id too: {}",
            text_of(&refused)
        );
    }

    #[tokio::test]
    async fn monitor_tools_round_trip_through_mcp() {
        let device = start("mcp-monitor").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let port = listener.local_addr().expect("the bound address").port();

        let added = client
            .call(
                "monitor.add",
                json!({
                    "kind": "tcp",
                    "host": "127.0.0.1",
                    "port": port,
                    "interval_ms": monitor::MIN_INTERVAL_MS,
                }),
            )
            .await;
        assert!(added.get("error").is_none(), "{added}");
        let target: Value = serde_json::from_str(text_of(&added)).expect("the target is JSON");
        let id = target["id"]
            .as_str()
            .expect("the target has an id")
            .to_owned();
        assert_eq!(target["kind"], "tcp");

        let probed = client.call("monitor.probe", json!({ "id": id })).await;
        let sample: Value = serde_json::from_str(text_of(&probed)).expect("the sample is JSON");
        assert_eq!(sample["ok"], true, "a listening socket answers: {sample}");
        assert!(
            sample["latency_ms"].as_u64().is_some(),
            "a connection that happened has a latency: {sample}"
        );
        assert!(sample["ts"].as_u64().is_some());

        let listed = client.call("monitor.list", json!({})).await;
        let targets: Value = serde_json::from_str(text_of(&listed)).expect("the list is JSON");
        let rows = targets.as_array().expect("the list is an array");
        assert_eq!(rows.len(), 1, "one watch is one row");
        assert_eq!(rows[0]["id"], id);
        assert_eq!(rows[0]["kind"], "tcp");
        assert_eq!(rows[0]["port"], port);
        assert_eq!(rows[0]["interval_ms"], monitor::MIN_INTERVAL_MS);
        assert_eq!(
            rows[0]["summary"]["up_now"], true,
            "the manual probe is in the target's series: {targets}"
        );

        let status = client.call("monitor.status", json!({ "id": id })).await;
        let summary: Value = serde_json::from_str(text_of(&status)).expect("the summary is JSON");
        assert_eq!(summary, rows[0]["summary"], "status is the list's summary");

        let removed = client.call("monitor.remove", json!({ "id": id })).await;
        let removed: Value = serde_json::from_str(text_of(&removed)).expect("the result is JSON");
        assert_eq!(removed["removed"], true);
        let listed = client.call("monitor.list", json!({})).await;
        let targets: Value = serde_json::from_str(text_of(&listed)).expect("the list is JSON");
        assert!(targets.as_array().expect("an array").is_empty());

        let again = client.call("monitor.remove", json!({ "id": id })).await;
        let again: Value = serde_json::from_str(text_of(&again)).expect("the result is JSON");
        assert_eq!(
            again["removed"], false,
            "removing a watch that is not there is an honest no-op"
        );
        drop(listener);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runs_exec_list_and_get_round_trip_through_mcp() {
        let device = start("mcp-runs").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let executed = client
            .call("runs.exec", json!({ "command": "echo", "args": ["hello"] }))
            .await;
        assert!(executed.get("error").is_none(), "{executed}");
        assert_eq!(executed["result"]["isError"], false, "{executed}");
        let result: Value = serde_json::from_str(text_of(&executed)).expect("exec is JSON");
        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["stdout"], "hello\n");
        assert_eq!(result["timed_out"], false);
        assert_eq!(result["truncated"], false);
        assert_eq!(
            result["history_recorded"], true,
            "a writable device records every completed run"
        );
        let id = result["id"].as_str().expect("the run has an id").to_owned();

        let listed = client.call("runs.list", json!({ "limit": 5 })).await;
        let rows: Value = serde_json::from_str(text_of(&listed)).expect("list is JSON");
        let rows = rows.as_array().expect("the history is an array");
        assert_eq!(rows.len(), 1, "one run is one row");
        assert_eq!(rows[0]["id"], id);
        assert_eq!(rows[0]["command"], "echo");
        assert_eq!(rows[0]["args"], json!(["hello"]));
        assert_eq!(rows[0]["stdout_bytes"], 6);
        assert!(
            rows[0].get("stdout").is_none(),
            "a history row never carries the output body: {}",
            rows[0]
        );

        let got = client.call("runs.get", json!({ "id": id })).await;
        let row: Value = serde_json::from_str(text_of(&got)).expect("get is JSON");
        assert_eq!(row, rows[0], "get returns the row list returned");

        let unknown = client
            .call("runs.get", json!({ "id": "run-unknown" }))
            .await;
        assert_eq!(unknown["result"]["isError"], true, "{unknown}");
        assert!(
            text_of(&unknown).contains("no run has that id"),
            "an unknown id is a tool-level refusal: {unknown}"
        );

        let outside = client
            .call(
                "runs.exec",
                json!({ "command": "touch", "args": ["ran"], "cwd": "../" }),
            )
            .await;
        assert_eq!(outside["result"]["isError"], true, "{outside}");
        assert!(
            text_of(&outside).contains("the working directory"),
            "the refusal names the working directory: {outside}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_pty_session_round_trips_through_mcp() {
        let device = start("mcp-pty-round-trip").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let created = client
            .call(
                "terminal.create",
                json!({ "kind": "pty", "shell": "/bin/sh", "rows": 24, "cols": 80 }),
            )
            .await;
        assert_eq!(created["result"]["isError"], false, "{created}");
        let created: Value = serde_json::from_str(text_of(&created)).expect("create is JSON");
        assert_eq!(created["kind"], "pty", "create reports the kind it started");
        assert_eq!(created["shell"], "/bin/sh");
        let id = created["id"]
            .as_str()
            .expect("create answers the id")
            .to_owned();

        // The device is the only writer of state.json: a live session is in it immediately.
        let saved: Value = serde_json::from_str(
            &fs::read_to_string(device.config.data.join("state.json"))
                .expect("the create wrote the state document"),
        )
        .expect("the state document is JSON");
        assert!(
            saved["sessions"]
                .as_array()
                .expect("sessions is an array")
                .iter()
                .any(|row| row["id"].as_str() == Some(id.as_str())
                    && row["kind"] == "pty"
                    && row["state"] == "attached"),
            "the live session is in the state document: {saved}"
        );

        // The command's text does not contain its output (`SUMMRISE$((6*7))`), so finding
        // `SUMMRISE42` proves the shell executed it rather than the PTY echoing it.
        let written = client
            .call(
                "terminal.write",
                json!({ "id": id, "data": "echo SUMMRISE$((6*7))\n" }),
            )
            .await;
        assert_eq!(written["result"]["isError"], false, "{written}");

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut cursor = 0u64;
        let mut output = String::new();
        while Instant::now() < deadline {
            let read = client
                .call("terminal.read", json!({ "id": id, "cursor": cursor }))
                .await;
            assert_eq!(read["result"]["isError"], false, "{read}");
            let body: Value = serde_json::from_str(text_of(&read)).expect("read is JSON");
            output.push_str(body["data"].as_str().expect("data is a string"));
            let next = body["cursor"].as_u64().expect("cursor is a number");
            assert!(next >= cursor, "a cursor never moves backwards");
            cursor = next;
            if output.contains("SUMMRISE42") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            output.contains("SUMMRISE42"),
            "the shell's output arrives through MCP: {output:?}"
        );

        let resized = client
            .call(
                "terminal.resize",
                json!({ "id": id, "rows": 40, "cols": 120 }),
            )
            .await;
        assert_eq!(resized["result"]["isError"], false, "{resized}");
        let resized: Value = serde_json::from_str(text_of(&resized)).expect("resize is JSON");
        assert_eq!(resized["rows"], 40);
        assert_eq!(resized["cols"], 120);

        let listed = client.call("terminal.list", json!({})).await;
        let sessions: Value = serde_json::from_str(text_of(&listed)).expect("list is JSON");
        let session = sessions
            .as_array()
            .and_then(|sessions| sessions.first())
            .unwrap_or_else(|| panic!("the live session is listed: {sessions}"));
        assert_eq!(session["id"], id);
        assert_eq!(session["kind"], "pty", "the list names the session's kind");
        assert!(
            session["started_at_ms"]
                .as_u64()
                .is_some_and(|started| started > 0),
            "the list names when the session started: {session}"
        );
        assert_eq!(
            session.as_object().expect("a list row is an object").len(),
            3,
            "the list row is exactly {{id, kind, started_at_ms}}: {session}"
        );

        let closed = client.call("terminal.close", json!({ "id": id })).await;
        assert_eq!(closed["result"]["isError"], false, "{closed}");

        let listed = client.call("terminal.list", json!({})).await;
        let sessions: Value = serde_json::from_str(text_of(&listed)).expect("list is JSON");
        assert_eq!(sessions, json!([]), "a closed session is not listed");

        let saved: Value = serde_json::from_str(
            &fs::read_to_string(device.config.data.join("state.json"))
                .expect("the close updated the state document"),
        )
        .expect("the state document is JSON");
        assert_eq!(
            saved["sessions"],
            json!([]),
            "a closed session's row is gone: {saved}"
        );

        let after = client
            .call("terminal.read", json!({ "id": id, "cursor": cursor }))
            .await;
        assert_eq!(after["result"]["isError"], true, "{after}");
        assert!(
            text_of(&after).contains("unknown or has been closed"),
            "a closed id is a typed refusal: {after}"
        );
    }

    #[tokio::test]
    async fn an_ssh_session_round_trips_through_mcp() {
        use std::time::{Duration, Instant};

        use crate::capabilities::ssh::test_support::TestServer;

        let server = TestServer::start_password("mcp-password").await;
        let device = start("mcp-ssh-round-trip").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let created = client
            .call(
                "terminal.create",
                json!({
                    "kind": "ssh",
                    "host": "127.0.0.1",
                    "port": server.port(),
                    "user": "tester",
                    "auth": { "kind": "password", "password": "mcp-password" },
                    "rows": 24,
                    "cols": 80,
                }),
            )
            .await;
        assert_eq!(created["result"]["isError"], false, "{created}");
        let created: Value = serde_json::from_str(text_of(&created)).expect("create is JSON");
        assert_eq!(created["kind"], "ssh", "create reports the kind it started");
        assert_eq!(
            created["shell"],
            format!("tester@127.0.0.1:{}", server.port()),
            "the label names the remote target"
        );
        let id = created["id"]
            .as_str()
            .expect("create answers the id")
            .to_owned();

        // The first connection pinned the host key in the device's own data directory.
        let known = fs::read_to_string(device.config.data.join("known_hosts.json"))
            .expect("the first SSH connection wrote the known-hosts store");
        assert!(
            known.contains(server.fingerprint()),
            "the store pins the key the device checked: {known}"
        );

        let saved: Value = serde_json::from_str(
            &fs::read_to_string(device.config.data.join("state.json"))
                .expect("the create wrote the state document"),
        )
        .expect("the state document is JSON");
        assert!(
            saved["sessions"]
                .as_array()
                .expect("sessions is an array")
                .iter()
                .any(|row| row["id"].as_str() == Some(id.as_str())
                    && row["kind"] == "ssh"
                    && row["state"] == "attached"),
            "the live SSH session is in the state document: {saved}"
        );

        let written = client
            .call(
                "terminal.write",
                json!({ "id": id, "data": "echo MCP-SSH-MARK\n" }),
            )
            .await;
        assert_eq!(written["result"]["isError"], false, "{written}");

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut cursor = 0u64;
        let mut output = String::new();
        while Instant::now() < deadline {
            let read = client
                .call("terminal.read", json!({ "id": id, "cursor": cursor }))
                .await;
            assert_eq!(read["result"]["isError"], false, "{read}");
            let body: Value = serde_json::from_str(text_of(&read)).expect("read is JSON");
            output.push_str(body["data"].as_str().expect("data is a string"));
            cursor = body["cursor"].as_u64().expect("cursor is a number");
            if output.contains("MCP-SSH-MARK") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            output.contains("MCP-SSH-MARK"),
            "the remote output arrives through MCP: {output:?}"
        );

        let resized = client
            .call(
                "terminal.resize",
                json!({ "id": id, "rows": 40, "cols": 120 }),
            )
            .await;
        assert_eq!(resized["result"]["isError"], false, "{resized}");

        let listed = client.call("terminal.list", json!({})).await;
        let sessions: Value = serde_json::from_str(text_of(&listed)).expect("list is JSON");
        let session = sessions
            .as_array()
            .and_then(|sessions| sessions.first())
            .unwrap_or_else(|| panic!("the live session is listed: {sessions}"));
        assert_eq!(session["id"], id);
        assert_eq!(session["kind"], "ssh", "the list names the session's kind");

        let closed = client.call("terminal.close", json!({ "id": id })).await;
        assert_eq!(closed["result"]["isError"], false, "{closed}");
        let listed = client.call("terminal.list", json!({})).await;
        let sessions: Value = serde_json::from_str(text_of(&listed)).expect("list is JSON");
        assert_eq!(sessions, json!([]), "a closed session is not listed");

        server.stop().await;
    }

    #[tokio::test]
    async fn creating_an_unimplemented_session_kind_is_a_typed_refusal() {
        let device = start("mcp-terminal-kind").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let refused = client
            .call("terminal.create", json!({ "kind": "telnet" }))
            .await;

        assert!(
            refused.get("error").is_none(),
            "the call routed to the capability, so the refusal is a tool result: {refused}"
        );
        assert_eq!(refused["result"]["isError"], true, "{refused}");
        let refusal = text_of(&refused);
        assert!(
            refusal.contains("not supported"),
            "the refusal is the capability's typed not-supported error: {refusal:?}"
        );
        assert!(
            refusal.contains("telnet")
                && refusal.contains("pty")
                && refusal.contains("ssh")
                && refusal.contains("serial"),
            "it names the kind it refused and the kinds it supports: {refusal:?}"
        );
        assert!(
            audit_lines(&device)
                .iter()
                .filter(|line| line["event"] == "end")
                .all(|line| line["ok"] == false && line["error_kind"] == "refused"),
            "the refusal is audited as a refused call"
        );
    }

    #[tokio::test]
    async fn an_ssh_create_missing_its_target_is_an_arguments_error() {
        let device = start("mcp-terminal-ssh-args").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        // A missing field is malformed arguments, so it is a JSON-RPC invalid-params error rather
        // than a tool result: nothing reached the capability.
        let refused = client
            .call("terminal.create", json!({ "kind": "ssh" }))
            .await;
        assert_eq!(refused["error"]["code"], -32602, "{refused}");
        let refusal = refused["error"]["message"]
            .as_str()
            .expect("the error has a message");
        assert!(
            refusal.contains("host is required"),
            "the refusal names the missing field: {refusal:?}"
        );
        assert!(
            refusal.contains("kind is ssh"),
            "the refusal names the kind that needs it: {refusal:?}"
        );

        let with_pty_shell = client
            .call(
                "terminal.create",
                json!({ "kind": "ssh", "shell": "/bin/sh" }),
            )
            .await;
        assert_eq!(with_pty_shell["error"]["code"], -32602, "{with_pty_shell}");

        // A pty cannot be given the remote half's fields: ignoring them would make a typo look
        // like a successful local session.
        let pty_with_host = client
            .call(
                "terminal.create",
                json!({ "kind": "pty", "host": "example.test" }),
            )
            .await;
        assert_eq!(pty_with_host["error"]["code"], -32602, "{pty_with_host}");
    }

    #[tokio::test]
    async fn a_serial_create_requires_a_port_path_and_keeps_the_kinds_apart() {
        let device = start("mcp-terminal-serial-args").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        // An absent port names the field and the kind that needs it.
        let missing = client
            .call("terminal.create", json!({ "kind": "serial" }))
            .await;
        assert_eq!(missing["error"]["code"], -32602, "{missing}");
        let refusal = missing["error"]["message"]
            .as_str()
            .expect("the error has a message");
        assert!(
            refusal.contains("port is required") && refusal.contains("kind is serial"),
            "the refusal names the missing field and its kind: {refusal:?}"
        );

        // A serial port is a path, never a TCP port number.
        let numeric = client
            .call("terminal.create", json!({ "kind": "serial", "port": 22 }))
            .await;
        assert_eq!(numeric["error"]["code"], -32602, "{numeric}");
        let refusal = numeric["error"]["message"]
            .as_str()
            .expect("the error has a message");
        assert!(
            refusal.contains("port path as a string"),
            "the refusal says what a serial port is: {refusal:?}"
        );

        // The serial framing fields are not silently ignored on a kind that has no serial line.
        for kind in ["pty", "ssh"] {
            let framed = client
                .call("terminal.create", json!({ "kind": kind, "baud": 9600 }))
                .await;
            assert_eq!(framed["error"]["code"], -32602, "{kind}: {framed}");
            let refusal = framed["error"]["message"]
                .as_str()
                .expect("the error has a message");
            assert!(
                refusal.contains("for kind serial"),
                "{kind}'s refusal says where the framing belongs: {refusal:?}"
            );
        }

        // An absent path is the capability's typed refusal, not an arguments error: the call
        // reached the serial backend, which is where the OS message lives.
        let absent = device.config.data.join("no-such-serial-port");
        let absent = absent.to_str().expect("the fixture path is UTF-8");
        let refused = client
            .call(
                "terminal.create",
                json!({ "kind": "serial", "port": absent }),
            )
            .await;
        assert_eq!(refused["result"]["isError"], true, "{refused}");
        let refusal = text_of(&refused);
        assert!(
            refusal.contains(absent) && refusal.contains("could not be opened"),
            "the refusal names the port and keeps the OS report: {refusal:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_serial_session_round_trips_through_mcp() {
        use std::time::{Duration, Instant};

        use nix::poll::{poll, PollFd, PollFlags};

        let device = start("mcp-serial-round-trip").await;
        let pair = nix::pty::openpty(None, None).expect("a pty pair is created");
        let path = nix::unistd::ttyname(pair.slave).expect("the pty slave has a path");
        let port = path.to_str().expect("the pty path is UTF-8").to_owned();
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let created = client
            .call(
                "terminal.create",
                json!({
                    "kind": "serial",
                    "port": port,
                    "baud": 115200,
                    "parity": "none",
                    "data_bits": 8,
                    "stop_bits": 1,
                }),
            )
            .await;
        assert_eq!(created["result"]["isError"], false, "{created}");
        let created: Value = serde_json::from_str(text_of(&created)).expect("create is JSON");
        assert_eq!(
            created["kind"], "serial",
            "create reports the kind it started"
        );
        assert_eq!(created["shell"], port, "the label names the port it opened");
        let id = created["id"]
            .as_str()
            .expect("create answers the id")
            .to_owned();

        let saved: Value = serde_json::from_str(
            &fs::read_to_string(device.config.data.join("state.json"))
                .expect("the create wrote the state document"),
        )
        .expect("the state document is JSON");
        assert!(
            saved["sessions"]
                .as_array()
                .expect("sessions is an array")
                .iter()
                .any(|row| row["id"].as_str() == Some(id.as_str())
                    && row["kind"] == "serial"
                    && row["state"] == "attached"),
            "the live serial session is in the state document: {saved}"
        );

        // Device -> line: the bytes written through MCP reach the far end of the pty.
        let written = client
            .call(
                "terminal.write",
                json!({ "id": id, "data": "MCP-SERIAL-DOWN\n" }),
            )
            .await;
        assert_eq!(written["result"]["isError"], false, "{written}");
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut received = Vec::new();
        while Instant::now() < deadline
            && !String::from_utf8_lossy(&received).contains("MCP-SERIAL-DOWN")
        {
            let mut fds = [PollFd::new(pair.master, PollFlags::POLLIN)];
            if poll(&mut fds, 100).expect("polling the pty master") == 0 {
                continue;
            }
            let mut buffer = [0u8; 256];
            let read =
                nix::unistd::read(pair.master, &mut buffer).expect("the far end reads the line");
            received.extend_from_slice(&buffer[..read]);
        }
        assert!(
            String::from_utf8_lossy(&received).contains("MCP-SERIAL-DOWN"),
            "the session's write reached the line: {:?}",
            String::from_utf8_lossy(&received)
        );

        // Line -> device: the far end's answer arrives through terminal.read.
        nix::unistd::write(pair.master, b"MCP-SERIAL-UP\n").expect("the far end writes the line");
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut cursor = 0u64;
        let mut output = String::new();
        while Instant::now() < deadline {
            let read = client
                .call("terminal.read", json!({ "id": id, "cursor": cursor }))
                .await;
            assert_eq!(read["result"]["isError"], false, "{read}");
            let body: Value = serde_json::from_str(text_of(&read)).expect("read is JSON");
            output.push_str(body["data"].as_str().expect("data is a string"));
            cursor = body["cursor"].as_u64().expect("cursor is a number");
            if output.contains("MCP-SERIAL-UP") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            output.contains("MCP-SERIAL-UP"),
            "the line's answer arrives through MCP: {output:?}"
        );

        // A serial line has no window; a uniform client's resize is a successful no-op, not an
        // error it would have to special-case.
        let resized = client
            .call(
                "terminal.resize",
                json!({ "id": id, "rows": 40, "cols": 120 }),
            )
            .await;
        assert_eq!(resized["result"]["isError"], false, "{resized}");
        let resized: Value = serde_json::from_str(text_of(&resized)).expect("resize is JSON");
        assert_eq!(resized["rows"], 40);
        assert_eq!(resized["cols"], 120);

        let listed = client.call("terminal.list", json!({})).await;
        let sessions: Value = serde_json::from_str(text_of(&listed)).expect("list is JSON");
        let session = sessions
            .as_array()
            .and_then(|sessions| sessions.first())
            .unwrap_or_else(|| panic!("the live session is listed: {sessions}"));
        assert_eq!(session["id"], id);
        assert_eq!(
            session["kind"], "serial",
            "the list names the session's kind"
        );

        let closed = client.call("terminal.close", json!({ "id": id })).await;
        assert_eq!(closed["result"]["isError"], false, "{closed}");
        let listed = client.call("terminal.list", json!({})).await;
        let sessions: Value = serde_json::from_str(text_of(&listed)).expect("list is JSON");
        assert_eq!(sessions, json!([]), "a closed session is not listed");

        let after = client
            .call("terminal.read", json!({ "id": id, "cursor": cursor }))
            .await;
        assert_eq!(after["result"]["isError"], true, "{after}");
        assert!(
            text_of(&after).contains("unknown or has been closed"),
            "a closed id is a typed refusal: {after}"
        );

        let _ = nix::unistd::close(pair.master);
        let _ = nix::unistd::close(pair.slave);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn browser_tools_round_trip_through_mcp() {
        if browser::chrome_or_skip().is_none() {
            return;
        }
        let _gate = browser::launch_gate().await;

        let device = start("mcp-browser-round-trip").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let opened = client
            .call(
                "browser.open",
                json!({ "url": "data:text/html,<title>SUMMRISE_MCP_TITLE</title>" }),
            )
            .await;
        assert_eq!(opened["result"]["isError"], false, "{opened}");
        let opened: Value = serde_json::from_str(text_of(&opened)).expect("open is JSON");
        let target = opened["target"]
            .as_str()
            .expect("open answers the target")
            .to_owned();

        let evaluated = client
            .call(
                "browser.eval",
                json!({ "target": target, "expression": "document.title" }),
            )
            .await;
        let evaluated: Value = serde_json::from_str(text_of(&evaluated)).expect("eval is JSON");
        assert_eq!(evaluated["type"], "string");
        assert_eq!(evaluated["value"], "SUMMRISE_MCP_TITLE");

        let listed = client.call("browser.list", json!({})).await;
        let targets: Value = serde_json::from_str(text_of(&listed)).expect("list is JSON");
        assert!(
            targets
                .as_array()
                .expect("a list is an array")
                .iter()
                .any(|entry| entry["id"] == target.as_str() && entry["type"] == "page"),
            "the opened tab is listed: {targets}"
        );

        let captured = client
            .call("browser.screenshot", json!({ "target": target }))
            .await;
        let captured: Value = serde_json::from_str(text_of(&captured)).expect("shot is JSON");
        assert_eq!(captured["format"], "png");
        assert!(
            captured["data"]
                .as_str()
                .is_some_and(|data| data.starts_with("iVBORw0KGgo")),
            "the screenshot is base64 PNG"
        );
        assert!(
            captured["bytes"].as_u64().is_some_and(|bytes| bytes > 100),
            "a real screenshot is more than 100 bytes: {captured}"
        );

        let closed = client
            .call("browser.close", json!({ "target": target }))
            .await;
        assert_eq!(closed["result"]["isError"], false, "{closed}");

        let stopped = client.call("browser.shutdown", json!({})).await;
        let stopped: Value = serde_json::from_str(text_of(&stopped)).expect("shutdown is JSON");
        assert_eq!(stopped["stopped"], true, "shutdown stopped the browser");
    }

    // Linux: the stopped state is observed through `/proc/<pid>/stat`. The bound itself is portable;
    // the observation is not.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_stalled_pty_write_is_bounded_and_the_api_keeps_answering() {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        let device = start_with_limits(
            "mcp-pty-write-stall",
            terminal::Limits {
                write_timeout_ms: 250,
                ..terminal::Limits::default()
            },
        )
        .await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let created = client
            .call(
                "terminal.create",
                json!({ "kind": "pty", "shell": "/bin/sh" }),
            )
            .await;
        assert_eq!(created["result"]["isError"], false, "{created}");
        let created: Value = serde_json::from_str(text_of(&created)).expect("create is JSON");
        let id = created["id"]
            .as_str()
            .expect("create answers the id")
            .to_owned();

        // `terminal.list` reports no pid by design, so the shell names itself through the session
        // it serves; the echoed command's `$$` is skipped because it is not a number.
        let pid = session_pid(&mut client, &id).await;
        kill(Pid::from_raw(pid), Signal::SIGSTOP).expect("the shell is stopped");
        // Wait for the stop to be observable before the write: a signal is delivered asynchronously,
        // and a shell still running for a few more instructions could drain the input.
        let mut stopped = false;
        for _ in 0..100 {
            let stat = fs::read_to_string(format!("/proc/{pid}/stat"))
                .expect("the stopped shell's stat is readable");
            let state = stat
                .rsplit_once(')')
                .expect("a stat line has a comm")
                .1
                .split_whitespace()
                .next()
                .expect("a stat line has a state");
            if state == "T" {
                stopped = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(stopped, "the shell is stopped before the write");

        // The PTY's buffered input path may absorb the whole megabyte inside the shortened bound, or
        // the bound may fire first. Both are valid; what must never pass is a call that hangs, so
        // the write and the health probe share a wall-clock deadline.
        let started = Instant::now();
        let data = "x".repeat(1024 * 1024);
        let write = client.call("terminal.write", json!({ "id": id, "data": data }));
        let health_url = format!("{}/api/health", device.mcp_url.trim_end_matches("/mcp"));
        let health = async {
            // Let the write handler take its blocking task first; after 100 ms the write is either
            // still in flight or already done, and health must answer either way.
            tokio::time::sleep(Duration::from_millis(100)).await;
            tokio::time::timeout(Duration::from_secs(2), reqwest::get(&health_url))
                .await
                .expect("health answers while or after the write")
                .expect("the health request completes")
        };
        let (message, response) = tokio::time::timeout(Duration::from_secs(6), async {
            tokio::join!(write, health)
        })
        .await
        .expect("a stalled write must return, not hang");
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "the call is bounded, not tied to the shell's throughput: {:?}",
            started.elapsed()
        );

        assert!(
            response.status().is_success(),
            "the device still serves /api/health around a stalled write: {}",
            response.status()
        );
        match message["result"]["isError"].as_bool() {
            Some(true) => {
                let refusal = text_of(&message);
                assert!(
                    refusal.contains("did not finish") || refusal.contains("still blocked"),
                    "a refusal is the typed write error: {message}"
                );
                assert!(
                    refusal.contains(&id),
                    "the refusal names the session: {message}"
                );
            }
            Some(false) => {
                let body: Value =
                    serde_json::from_str(text_of(&message)).expect("a write result is JSON");
                assert_eq!(
                    body["written"],
                    1024 * 1024,
                    "a completed write reports every byte it accepted: {message}"
                );
            }
            other => panic!("a tool result is an isError boolean, not {other:?}: {message}"),
        }

        // Resume the shell so any parked task can finish, then close the session.
        kill(Pid::from_raw(pid), Signal::SIGCONT).expect("the shell resumes");
        let closed = client.call("terminal.close", json!({ "id": id })).await;
        assert_eq!(closed["result"]["isError"], false, "{closed}");
    }

    /// The pid of the shell behind `id`, discovered through the session itself.
    ///
    /// `terminal.list` reports `{id, kind, started_at_ms}` and no process id, so the test asks the
    /// shell to print its own pid and reads the answer through `terminal.read`.
    #[cfg(target_os = "linux")]
    async fn session_pid(client: &mut Client, id: &str) -> i32 {
        let written = client
            .call(
                "terminal.write",
                json!({ "id": id, "data": "echo PID=$$\n" }),
            )
            .await;
        assert_eq!(written["result"]["isError"], false, "{written}");

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut cursor = 0u64;
        let mut output = String::new();
        while Instant::now() < deadline {
            let read = client
                .call("terminal.read", json!({ "id": id, "cursor": cursor }))
                .await;
            assert_eq!(read["result"]["isError"], false, "{read}");
            let body: Value = serde_json::from_str(text_of(&read)).expect("read is JSON");
            output.push_str(body["data"].as_str().expect("data is a string"));
            cursor = body["cursor"].as_u64().expect("cursor is a number");
            if let Some(pid) = first_number_after(&output, "PID=") {
                return pid as i32;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the shell never printed its pid; output so far: {output:?}");
    }

    /// The first ASCII number written immediately after `marker` anywhere in `text`.
    #[cfg(target_os = "linux")]
    fn first_number_after(text: &str, marker: &str) -> Option<u32> {
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

    #[test]
    fn the_pty_registry_writes_its_state_rows_under_the_audits_lock() {
        let dir = TempDir::new("mcp-pty-lock");
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
            console_url: None,
            console_token_file: None,
            mesh_broker: None,
            mesh_relay: None,
            mesh_stun: None,
        };

        let server = super::DeviceServer::new(&config, terminal::Limits::default());

        assert!(
            server
                .terminal
                .shares_state_lock(&server.audit.write_lock()),
            "create/close must serialize state.sessions with the audit's lines"
        );
    }

    #[tokio::test]
    async fn an_unknown_tool_is_an_mcp_error() {
        let device = start("mcp-unknown-tool").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let message = client
            .call("files.delete", json!({ "path": "notes/todo.txt" }))
            .await;

        assert!(
            message.get("result").is_none(),
            "an unknown tool has no result and no data: {message}"
        );
        assert_eq!(message["error"]["code"], -32602, "{message}");
        assert!(
            message["error"]["message"]
                .as_str()
                .is_some_and(|text| text.contains("files.delete")),
            "{message}"
        );
    }

    #[tokio::test]
    async fn a_call_appends_a_begin_line_before_its_end_line() {
        let device = start("mcp-audit-one").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;
        // Neither the handshake nor the listing is a tool call: the log records calls only.
        let _ = client.request("tools/list", json!({})).await;

        let message = client
            .call(
                "files.write",
                json!({ "path": "audit.txt", "content": "x" }),
            )
            .await;
        assert_eq!(message["result"]["isError"], false, "{message}");

        let lines = audit_lines(&device);
        assert_eq!(lines.len(), 2, "one call, a begin and an end: {lines:?}");

        let begin = &lines[0];
        assert_eq!(begin["event"], "begin");
        assert_eq!(begin["tool"], "files.write");
        assert_eq!(begin["decision"], "allow");
        assert!(
            begin["ok"].is_null(),
            "the authorization line carries no outcome yet: {begin:?}"
        );
        assert!(
            begin["ts_ms"].as_i64().is_some_and(|ts| ts > 0),
            "the line carries the device's clock: {begin:?}"
        );

        let end = &lines[1];
        assert_eq!(end["event"], "end");
        assert_eq!(end["ok"], true);
        assert!(
            end.get("error_kind").is_none(),
            "a successful call has no error kind: {end:?}"
        );
        assert_eq!(begin["id"], end["id"], "the two lines name one call");
        assert!(
            end["ts_ms"].as_i64() >= begin["ts_ms"].as_i64(),
            "the end cannot precede the begin: {lines:?}"
        );
    }

    #[tokio::test]
    async fn every_audit_line_names_the_transport_the_call_arrived_on() {
        let device = start("mcp-audit-transport").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let message = client.call("state.get", json!({})).await;
        assert!(message.get("error").is_none(), "{message}");

        let lines = audit_lines(&device);
        assert_eq!(lines.len(), 2, "one call is a begin and an end: {lines:?}");
        for line in &lines {
            assert_eq!(
                line["transport"], "loopback",
                "a line that does not say how the call arrived cannot be read back: {lines:?}"
            );
            assert!(
                line.get("peer").is_none(),
                "the loopback owner is not a mesh peer: {lines:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_denied_call_is_audited_with_ok_false_and_no_detail() {
        let device = start("mcp-audit-denied").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        // The pre-release `terminal.pty.*` names are deliberately not aliases: calling one is an
        // unknown tool, and that refusal is what this audit test records.
        let message = client.call("terminal.pty.create", json!({})).await;
        assert!(message.get("error").is_some(), "{message}");

        let lines = audit_lines(&device);
        assert_eq!(
            lines.len(),
            2,
            "a refused call is still a begin and an end: {lines:?}"
        );
        assert_eq!(lines[0]["event"], "begin");
        assert_eq!(lines[0]["tool"], "terminal.pty.create");
        assert_eq!(lines[0]["decision"], "deny");
        assert_eq!(lines[0]["ok"], Value::Null);
        assert_eq!(lines[1]["event"], "end");
        assert_eq!(lines[1]["ok"], false);
        assert_eq!(
            lines[1]["error_kind"], "unknown_tool",
            "the reason is a closed kind, not a message: {lines:?}"
        );
        assert!(
            lines[1].get("detail").is_none(),
            "the audit does not carry a message or an argument: {lines:?}"
        );
    }

    #[tokio::test]
    async fn a_refused_call_is_audited_as_refused() {
        let device = start("mcp-read-cap").await;
        let path = device.config.files_root.join("huge.bin");
        fs::File::create(&path)
            .expect("the sparse fixture is creatable")
            .set_len((MAX_FILE_BYTES + 1) as u64)
            .expect("the sparse length is settable");
        let mut client = Client::authorized(&device);
        client.initialize().await;

        let message = client
            .call("files.read", json!({ "path": "huge.bin" }))
            .await;

        assert!(message.get("error").is_none(), "the tool ran: {message}");
        assert_eq!(message["result"]["isError"], true, "{message}");
        assert!(
            text_of(&message).contains("cap"),
            "the refusal names the cap: {message}"
        );

        let lines = audit_lines(&device);
        assert_eq!(lines.len(), 2, "a failed call is still one call: {lines:?}");
        assert_eq!(lines[0]["event"], "begin");
        assert_eq!(lines[0]["decision"], "allow");
        assert_eq!(lines[1]["event"], "end");
        assert_eq!(lines[1]["ok"], false, "a refused read is a failed call");
        assert_eq!(lines[1]["error_kind"], "refused");
    }

    #[tokio::test]
    async fn the_audit_never_records_an_argument_value() {
        let device = start("mcp-audit-no-echo").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        // The review's probe: a wrongly-typed value used to appear in serde's error message, which
        // the old audit stored as `detail`. The value is a sentinel, not a secret.
        let message = client
            .call(
                "memory.search",
                json!({ "query": "x", "limit": "SENTINELVALUE" }),
            )
            .await;
        assert_eq!(message["error"]["code"], -32602, "{message}");
        assert!(
            !message.to_string().contains("SENTINELVALUE"),
            "the response names the schema failure, not the value: {message}"
        );

        let raw = fs::read_to_string(device.config.data.join("events.jsonl"))
            .expect("the audit file is readable");
        assert!(
            !raw.contains("SENTINELVALUE"),
            "the audit line records the error kind, not the argument: {raw}"
        );
        let lines = audit_lines(&device);
        let end = lines
            .iter()
            .find(|line| line["event"] == "end")
            .expect("the call has an end line");
        assert_eq!(end["ok"], false);
        assert_eq!(end["error_kind"], "arguments");
    }

    #[tokio::test]
    async fn a_begin_line_survives_a_call_that_never_wrote_its_end() {
        let dir = TempDir::new("audit-begin-only");
        let audit = super::Audit::new(&dir.join("data"));

        audit
            .begin(
                7,
                123,
                "files.write",
                "allow",
                super::Arrival::Loopback,
                None,
            )
            .await;

        // This is the crash/session-close window: the side effect may have happened, and the only
        // surviving line is the authorization line. It must be there, on its own.
        let raw = fs::read_to_string(dir.join("data").join("events.jsonl"))
            .expect("the begin line is on disk");
        let line: Value = serde_json::from_str(raw.trim()).expect("the line is JSON");
        assert_eq!(line["id"], 7);
        assert_eq!(line["event"], "begin");
        assert_eq!(line["tool"], "files.write");
        assert_eq!(line["decision"], "allow");
    }

    #[tokio::test]
    async fn an_unwritable_audit_returns_false_for_begin() {
        let dir = TempDir::new("audit-unwritable");
        fs::write(dir.join("blocked"), b"a file, not a directory").expect("the blocking file");
        let audit = super::Audit::new(&dir.join("blocked"));

        assert!(
            !audit
            .begin(1, 1, "files.write", "allow", super::Arrival::Loopback, None)
            .await,
            "the caller must be able to refuse the call when its authorization line cannot be written"
        );
    }

    #[tokio::test]
    async fn the_audit_rotates_once_at_its_cap_and_replaces_the_previous_generation() {
        let dir = TempDir::new("audit-rotation");
        let audit = super::Audit::with_cap(dir.join("events.jsonl"), 400);

        for id in 1..=40 {
            audit
                .begin(
                    id,
                    id as i64,
                    "files.read",
                    "allow",
                    super::Arrival::Loopback,
                    None,
                )
                .await;
            audit
                .end(id, id as i64, true, None, super::Arrival::Loopback, None)
                .await;
        }

        let current =
            fs::read_to_string(dir.join("events.jsonl")).expect("the current audit file exists");
        let rotated =
            fs::read_to_string(dir.join("events.1.jsonl")).expect("the rotated audit file exists");

        assert!(
            current.contains("\"id\":40"),
            "the newest line stays in the current file"
        );
        assert!(
            !rotated.contains("\"id\":1"),
            "the first generation was replaced by a later rotation, not accumulated: {rotated}"
        );
        assert!(
            current.lines().count() < 40 && rotated.lines().count() < 40,
            "neither file holds the whole history: {current:?} / {rotated:?}"
        );
        for line in current.lines().chain(rotated.lines()) {
            serde_json::from_str::<Value>(line).expect("every audit line is JSON");
        }
    }

    #[tokio::test]
    async fn a_body_over_the_limit_is_refused_before_any_parse() {
        let device = start("mcp-body-limit").await;
        let mut client = Client::authorized(&device);
        client.initialize().await;

        // The marker is the side effect the refused call would have made.
        let marker = device.config.files_root.join("too-big.txt");
        let refused = client
            .try_post(json!({
                "jsonrpc": "2.0",
                "id": 99,
                "method": "tools/call",
                "params": {
                    "name": "files.write",
                    "arguments": {
                        "path": "too-big.txt",
                        "content": "x".repeat(MAX_BODY_BYTES + 1),
                    }
                }
            }))
            .await;

        match refused {
            Ok(response) => assert_eq!(
                response.status(),
                StatusCode::PAYLOAD_TOO_LARGE,
                "an oversized body is refused as 413"
            ),
            Err(error) => {
                // Windows reports the same refusal as a transport abort: the server answers and
                // closes while the client is still writing the 9 MiB request. The Linux contract
                // stays pinned to the 413 response.
                #[cfg(windows)]
                assert!(
                    error.is_request() || error.is_connect() || error.is_body(),
                    "the abort is the refusal's transport report, not a timeout or a builder error: {error}"
                );
                #[cfg(not(windows))]
                panic!("the MCP server answers 413 on Linux: {error}");
            }
        }

        assert!(
            !marker.exists(),
            "the capability never ran: {} must not exist",
            marker.display()
        );
        assert!(
            audit_lines(&device).is_empty(),
            "not even a begin line exists: rmcp never parsed the request"
        );
    }

    #[tokio::test]
    async fn an_oversized_content_length_is_refused_without_reading_the_body() {
        let device = start("mcp-content-length").await;
        // Ten gigabytes declared, zero bytes sent: the refusal must come from the header alone.
        let declared = 10_u64 * 1024 * 1024 * 1024;
        let app = crate::http::router(
            &device.config,
            Store::new(
                &device.config.install,
                &device.config.data,
                device.config.release(),
            ),
        );
        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(header::CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {}", token_hex()))
            .header(header::CONTENT_LENGTH, declared.to_string())
            .body(Body::empty())
            .expect("the request builds");

        let response = app.oneshot(request).await.expect("the router answers");

        assert_eq!(
            response.status(),
            StatusCode::PAYLOAD_TOO_LARGE,
            "a declared body over the cap is refused from the header"
        );
        assert!(audit_lines(&device).is_empty());
    }

    #[test]
    fn every_registry_tool_has_an_implementation_and_every_implementation_is_registered() {
        let mut registry: Vec<&str> = CAPABILITIES
            .iter()
            .flat_map(|capability| capability.tools)
            .map(|tool| tool.name)
            .collect();
        for name in &registry {
            assert!(
                dispatch_supports(name),
                "{name} is advertised but has no dispatch row; add it to HANDLERS"
            );
            assert!(handler(name).is_some());
        }

        for (name, _) in HANDLERS {
            assert!(
                summrise_core::registry::tool(name).is_some(),
                "{name} has an implementation but is not in the registry"
            );
            assert!(dispatch_supports(name));
        }

        let mut implementations: Vec<&str> = HANDLERS.iter().map(|(name, _)| *name).collect();
        implementations.sort_unstable();
        registry.sort_unstable();
        assert_eq!(
            implementations, registry,
            "the registry and the dispatch table must name the same tools"
        );
    }

    #[tokio::test]
    async fn the_mcp_route_without_the_install_token_is_401() {
        let device = start("mcp-no-token").await;

        for token in [None, Some("00".repeat(32))] {
            let client = Client::with_token(&device, token.clone());
            let response = client
                .post(json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": { "name": "intruder", "version": "0" }
                    }
                }))
                .await;

            assert_eq!(
                response.status(),
                401,
                "no token (or a wrong one: {token:?}) is refused before MCP sees the request"
            );
            let body: Value =
                serde_json::from_str(&response.text().await.expect("the 401 body is readable"))
                    .expect("the 401 body is JSON");
            assert_eq!(body, json!({ "error": "unauthorized" }));
        }
    }

    /// A handler with its own data directory, for the console path that does not speak HTTP.
    fn console_server(label: &str) -> (super::DeviceServer, Config, TempDir) {
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
            console_url: None,
            console_token_file: None,
            mesh_broker: None,
            mesh_relay: None,
            mesh_stun: None,
        };
        let server = super::DeviceServer::new(&config, terminal::Limits::default());
        (server, config, dir)
    }

    #[tokio::test]
    async fn a_loopback_context_is_still_the_owner_through_the_console_handler() {
        let (server, config, _dir) = console_server("console-owner");

        let reply = server
            .handle(
                &json!({
                    "jsonrpc": "2.0",
                    "id": 4,
                    "method": "tools/call",
                    "params": {
                        "name": "files.write",
                        "arguments": { "path": "owner.txt", "content": "owner" }
                    }
                }),
                super::Caller::Owner,
            )
            .await;

        assert_eq!(reply["result"]["isError"], false, "{reply}");
        assert_eq!(
            fs::read_to_string(config.files_root.join("owner.txt"))
                .expect("the owner's write reached the files root"),
            "owner"
        );
        let lines = fs::read_to_string(config.data.join("events.jsonl"))
            .expect("the audit log exists")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("a JSON line"))
            .collect::<Vec<_>>();
        assert_eq!(lines[0]["decision"], "allow");
    }

    #[tokio::test]
    async fn a_granted_destructive_call_asks_before_it_runs() {
        let (server, config, _dir) = console_server("console-ask");
        let grant = summrise_core::grant::Grant {
            device_id: "ab".repeat(32),
            tools: vec!["files.write".to_owned()],
            expires_at_ms: (crate::http::now_ms() + 60_000) as u64,
            allow_destructive: false,
        };

        let reply = server
            .handle(
                &json!({
                    "jsonrpc": "2.0",
                    "id": 5,
                    "method": "tools/call",
                    "params": {
                        "name": "files.write",
                        "arguments": { "path": "asked.txt", "content": "not yet" }
                    }
                }),
                super::Caller::Granted {
                    grant: &grant,
                    arrival: super::Arrival::Console,
                    peer: None,
                },
            )
            .await;

        assert_eq!(
            reply["error"]["code"],
            json!(-32002),
            "an unanswered approval is its own JSON-RPC code: {reply}"
        );
        assert!(
            reply["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("allow_destructive")),
            "the refusal names how to consent: {reply}"
        );
        assert!(
            !config.files_root.join("asked.txt").exists(),
            "a call that needs approval must not touch the filesystem"
        );
        let lines = fs::read_to_string(config.data.join("events.jsonl"))
            .expect("the audit log exists")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("a JSON line"))
            .collect::<Vec<_>>();
        assert_eq!(lines[0]["decision"], "ask");
        assert_eq!(lines[1]["error_kind"], "approval_required");
    }

    #[tokio::test]
    async fn a_grant_that_consents_runs_the_destructive_call() {
        let (server, config, _dir) = console_server("console-destructive-allow");
        let grant = summrise_core::grant::Grant {
            device_id: "ab".repeat(32),
            tools: vec!["files.write".to_owned()],
            expires_at_ms: (crate::http::now_ms() + 60_000) as u64,
            allow_destructive: true,
        };

        let reply = server
            .handle(
                &json!({
                    "jsonrpc": "2.0",
                    "id": 6,
                    "method": "tools/call",
                    "params": {
                        "name": "files.write",
                        "arguments": { "path": "consented.txt", "content": "written" }
                    }
                }),
                super::Caller::Granted {
                    grant: &grant,
                    arrival: super::Arrival::Console,
                    peer: None,
                },
            )
            .await;

        assert_eq!(reply["id"], 6);
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        assert_eq!(
            fs::read_to_string(config.files_root.join("consented.txt"))
                .expect("the consented write reached the files root"),
            "written"
        );
        let lines = fs::read_to_string(config.data.join("events.jsonl"))
            .expect("the audit log exists")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("a JSON line"))
            .collect::<Vec<_>>();
        assert_eq!(lines[0]["decision"], "allow");
    }
}
