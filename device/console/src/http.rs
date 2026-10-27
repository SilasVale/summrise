//! The HTTP surface: the operator page, login, enrollment, the device registry, grant signing,
//! permanent-token management and health.
//!
//! One router, built once from a loaded [`Console`]. The handlers are thin on purpose: the decisions
//! live in `auth`, `codes`, `devices` and `grants`, and the router only turns refusals into status
//! codes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Form, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use ed25519_dalek::{Signature, SigningKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Digest;

use crate::audit::{self, Audit};
use crate::auth::{
    bearer_secret, constant_time_eq, new_session_token, random_32, Session, SESSION_LIFETIME_MS,
};
use crate::channels::{ChannelRegistry, Reply, RequestAuth, RequestError};
use crate::codes::{ConsumeError, EnrollmentCodes, IssueError};
use crate::config::{load_grant_key, load_operator_token, Config, StartupError};
use crate::connections;
use crate::denials::Denials;
use crate::devices::{self, Device, DeviceRegistry, RegisterError};
use crate::grants::{self, GrantEnvelope, GrantError, VerifyError};
use crate::hex;
use crate::mcp::DeviceRef;
use crate::tokens::{Token, TokenError, TokenMeta, Tokens};

mod page;

/// How far a device's clock may be from the console's when it registers: five minutes.
const REGISTRATION_SKEW_MS: u64 = 5 * 60 * 1000;

/// How many log lines `GET /audit` and `GET /denials` return when the caller names no limit.
const DEFAULT_LOG_LIMIT: usize = 100;

/// The largest limit the log routes honor: an operator reading an index must not make the console
/// hold an unbounded slice of it.
const MAX_LOG_LIMIT: usize = 1000;

/// How long the console tells an operator's client to wait before retrying a reconnecting device.
const RETRY_AFTER_MS: u64 = 1_000;

/// The largest body the standard MCP endpoint buffers, after the grant has been verified.
///
/// It matches the device's own `/mcp` cap: a body a client can send to one end must not be refused
/// by the other for a reason that is not the tool's.
const MAX_STANDARD_MCP_BODY_BYTES: usize = 8 * 1024 * 1024;

/// How long the console tells an operator's client to wait before retrying a busy device.
const BUSY_RETRY_AFTER_MS: u64 = 250;

/// The clock the console reads, in Unix milliseconds.
///
/// It is a value rather than a direct `SystemTime::now` call so a test can advance it: an expiry that
/// takes a day to observe is an expiry nobody tests.
type Now = Arc<dyn Fn() -> u64 + Send + Sync>;

/// The loaded console. Cloning it is cloning the `Arc`; the router shares this state.
#[derive(Clone)]
pub struct Console {
    inner: Arc<Inner>,
}

struct Inner {
    operator_token: [u8; 32],
    grant_key: SigningKey,
    sessions: Mutex<Vec<Session>>,
    devices: Mutex<DeviceRegistry>,
    /// The operator's permanent API tokens: a credential the console exchanges for a short grant.
    tokens: Mutex<Tokens>,
    /// Saved terminal presets, keyed by device inside the file. Operator convenience, not device state.
    connections: Mutex<connections::Connections>,
    codes: Mutex<EnrollmentCodes>,
    channels: ChannelRegistry,
    audit: Audit,
    denials: Denials,
    mcp_timeout: Duration,
    now: Now,
    started: Instant,
    /// What `/health` reports as this build.
    version: String,
    /// The data directory: the login-code file lives here.
    data: std::path::PathBuf,
    /// When set, a verified Cloudflare Access JWT is an operator session.
    access: Option<crate::access::Access>,
    /// The mesh broker this console asks about a device, as a canonical `http://host:port`.
    ///
    /// `None` means the console has no mesh view; the readiness route reports that rather than a fake
    /// "unreachable". No operator frame moves over the mesh yet.
    mesh_broker: Option<String>,
}

impl Console {
    /// Load the operator secret, the grant key, the registry and the outstanding codes, using the
    /// system clock.
    pub fn load(config: &Config) -> Result<Console, StartupError> {
        Console::load_with_clock(config, system_now_ms)
    }

    /// Load with an injected clock. Tests use this to reach a session's expiry without waiting a day.
    pub fn load_with_clock<F>(config: &Config, now: F) -> Result<Console, StartupError>
    where
        F: Fn() -> u64 + Send + Sync + 'static,
    {
        Console::load_with_clock_and_timeout(config, now, Duration::from_secs(30))
    }

    /// Load with an injected clock and `/mcp` timeout.
    ///
    /// The timeout is the operator's wait for a device answer: 30 seconds in production, shorter in
    /// tests that observe the timeout rather than wait it out.
    pub fn load_with_clock_and_timeout<F>(
        config: &Config,
        now: F,
        mcp_timeout: Duration,
    ) -> Result<Console, StartupError>
    where
        F: Fn() -> u64 + Send + Sync + 'static,
    {
        Console::load_with_clock_timeout_and_audit_cap(
            config,
            now,
            mcp_timeout,
            audit::AUDIT_MAX_BYTES,
        )
    }

    /// Load with every tunable injected: the clock, the `/mcp` bound, and the audit cap.
    ///
    /// The cap is a parameter for the same reason the clock is one: a rotation that takes 8 MiB to
    /// reach is a rotation no test observes, and the discipline it encodes is the device audit's.
    pub fn load_with_clock_timeout_and_audit_cap<F>(
        config: &Config,
        now: F,
        mcp_timeout: Duration,
        audit_max_bytes: u64,
    ) -> Result<Console, StartupError>
    where
        F: Fn() -> u64 + Send + Sync + 'static,
    {
        let operator_token = load_operator_token(&config.operator_token_file)?;
        let grant_key = load_grant_key(&config.grant_key_file)?;
        let devices = DeviceRegistry::load(&config.data)?;
        let tokens = Tokens::load(&config.data)?;
        let connections = connections::Connections::load(&config.data)?;
        let codes = EnrollmentCodes::load(&config.data)?;

        Ok(Console {
            inner: Arc::new(Inner {
                operator_token,
                grant_key,
                sessions: Mutex::new(crate::auth::load_sessions(&config.data, (now)())),
                devices: Mutex::new(devices),
                tokens: Mutex::new(tokens),
                connections: Mutex::new(connections),
                codes: Mutex::new(codes),
                channels: ChannelRegistry::new(),
                audit: Audit::new(&config.data, audit_max_bytes),
                denials: Denials::new(&config.data),
                mcp_timeout,
                now: Arc::new(now),
                started: Instant::now(),
                version: config.version.clone(),
                data: config.data.clone(),
                mesh_broker: config.mesh_broker.clone(),
                access: match &config.access {
                    Some(access) => Some(
                        crate::access::Access::from_jwks_file(
                            &access.jwks,
                            &access.audience,
                            &access.issuer,
                        )
                        .map_err(|error| StartupError::Access(error.to_string()))?,
                    ),
                    None => None,
                },
            }),
        })
    }

    /// Build the router. It carries no socket and no runtime: the caller decides where it listens.
    pub fn router(&self) -> Router {
        Router::new()
            .route("/", get(root_redirect))
            // The terminal emulator, served from the binary: the page needs a real terminal for
            // interactive prompts (sudo, ssh host keys), and fetching it from a CDN would make the
            // console depend on a third party at run time. Two files, no build step.
            // The conventional favicon path: a browser asks for it on every page, and a 404 there is a
            // console error in every page that does not declare an icon. It is the same PNG.
            .route(
                "/favicon.ico",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "image/png")],
                        axum::body::Bytes::from_static(assets::ICON),
                    )
                }),
            )
            .route(
                "/assets/icon.png",
                get(|| async {
                    (
                        [
                            (header::CONTENT_TYPE, "image/png"),
                            (header::CACHE_CONTROL, "public, max-age=86400"),
                        ],
                        axum::body::Bytes::from_static(assets::ICON),
                    )
                }),
            )
            .route(
                "/assets/xterm.js",
                get(|| async {
                    assets::Asset {
                        body: assets::JS,
                        content_type: "text/javascript; charset=utf-8",
                    }
                }),
            )
            .route(
                "/assets/terminal.js",
                get(|| async {
                    assets::Asset {
                        body: assets::TERMINAL,
                        content_type: "text/javascript; charset=utf-8",
                    }
                }),
            )
            .route(
                "/assets/addon-fit.js",
                get(|| async {
                    assets::Asset {
                        body: assets::FIT,
                        content_type: "text/javascript; charset=utf-8",
                    }
                }),
            )
            .route(
                "/assets/xterm.css",
                get(|| async {
                    assets::Asset {
                        body: assets::CSS,
                        content_type: "text/css; charset=utf-8",
                    }
                }),
            )
            .route("/health", get(health))
            .route("/auth", post(authenticate))
            .route("/next", get(next_page))
            .route("/next/auth", post(next_login))
            .route("/next/logout", post(next_logout))
            .route("/next/fleet/:id/name", post(next_rename))
            .route("/next/fleet/:id/remove", post(next_remove))
            .route("/next/enroll", post(next_enroll))
            .route("/next/audit", get(next_audit))
            .route("/next/denials", get(next_denials))
            // The terminal page: one device, one shell, the session's own cookie as the gate.
            .route("/next/terminal/:device", get(next_terminal_page))
            .route("/next/terminal/:device/create", post(next_terminal_create))
            .route("/next/terminal/:device/:id/read", get(next_terminal_read))
            .route(
                "/next/terminal/:device/:id/write",
                post(next_terminal_write),
            )
            .route(
                "/next/terminal/:device/:id/resize",
                post(next_terminal_resize),
            )
            .route(
                "/next/terminal/:device/:id/close",
                post(next_terminal_close),
            )
            // The files page: a server-rendered list, a read view and a text write.
            .route("/next/files/:device", get(next_files_page))
            .route("/next/files/:device/read", get(next_files_read))
            .route("/next/files/:device/write", post(next_files_write))
            // The runs page: one command, its output, and the device's own history.
            .route("/next/runs/:device", get(next_runs_page))
            .route("/next/runs/:device/exec", post(next_runs_exec))
            // The device's own state, and the system page with its process list.
            .route("/next/state/:device", get(next_state_page))
            .route("/next/system/:device", get(next_system_page))
            .route("/next/system/:device/kill", post(next_system_kill))
            // The device's own memory: the entries, an append form, a delete per row.
            .route("/next/memory/:device", get(next_memory_page))
            .route("/next/memory/:device/append", post(next_memory_append))
            .route("/next/memory/:device/delete", post(next_memory_delete))
            // The device's screen, and the watches it runs.
            .route("/next/screen/:device", get(next_screen_page))
            .route("/next/monitors/:device", get(next_monitors_page))
            .route("/next/monitors/:device/add", post(next_monitor_add))
            .route("/next/monitors/:device/remove", post(next_monitor_remove))
            // The device's browser: its targets, a capture, and open/close.
            .route("/next/browser/:device", get(next_browser_page))
            .route("/next/browser/:device/open", post(next_browser_open))
            .route("/next/browser/:device/close", post(next_browser_close))
            // The connect card: a device's permanent tokens, minted once and revocable.
            .route("/next/tokens/:device", get(next_tokens_page))
            .route("/next/tokens/:device/mint", post(next_token_mint))
            .route("/next/tokens/:device/revoke", post(next_token_revoke))
            // One sample of the device's own vitals.
            .route("/next/vitals/:device", get(next_vitals_page))
            .route("/session", get(session_probe))
            .route("/logout", post(logout))
            .route("/enroll", post(enroll))
            // Two namespaces, because two audiences with different gates. `/devices…` is what a
            // DEVICE calls: it registers and then holds a channel, and Cloudflare Access must let it
            // through without an email login (the `console.saisi.online/devices` bypass application).
            // `/fleet…` is what the OPERATOR calls, and it must never share that prefix: a bypassed
            // request carries no `Cf-Access-Jwt-Assertion`, so an operator route under `/devices`
            // answers 401 to a page that Access just signed in — which is exactly what happened
            // (2026-10-24: the list loaded, then the page bounced to the token box).
            .route("/devices", post(register_device))
            .route("/devices/:id/channel", get(device_channel))
            .route("/mesh", get(mesh_status))
            .route("/fleet/:id/mesh", get(device_mesh_readiness))
            .route("/fleet", get(list_devices))
            .route("/fleet/:id/name", post(rename_device))
            .route(
                "/fleet/:id/connections",
                get(list_connections).post(save_connection),
            )
            .route(
                "/fleet/:id/connections/:connection",
                axum::routing::delete(remove_connection),
            )
            .route("/fleet/:id", axum::routing::delete(remove_device))
            // The tool catalogue the grant form reads. It is the device's own registry — the console
            // depends on `summrise-core`, so there is one list, not a copy in the page and another in
            // the device. The page still falls back to its embedded list if this route fails, but the
            // route is what makes that fallback a fallback.
            .route("/tools", get(tool_catalogue))
            .route("/grants", post(create_grant))
            // Permanent tokens an AI client can keep in a fixed configuration. The create response
            // is the only time the secret is visible; the list is metadata only.
            .route("/tokens", get(list_tokens).post(create_token))
            .route("/tokens/:id", axum::routing::delete(revoke_token))
            .route("/tokens/:id/rotate", post(rotate_token))
            .route("/mcp", post(proxy_mcp))
            .route("/mcp/all", post(aggregate_mcp))
            .route("/mcp/:id", post(standard_mcp))
            .route("/audit", get(list_audit))
            // The same read as `/audit`, over the unauthenticated-refusal log. Operator-only: the
            // log holds what probing looks like, and it is surfaced read-only.
            .route("/denials", get(list_denials))
            .layer(axum::middleware::from_fn(no_store))
            .with_state(self.inner.clone())
    }
}

/// The facts one audit line names. One shape, named at the call site: the three `Option`s are
/// different facts, not interchangeable positionals.
struct AuditFacts<'a> {
    kind: &'static str,
    device_id: Option<&'a str>,
    status: StatusCode,
    detail: Option<&'a str>,
    tool: Option<&'a str>,
    token_id: Option<&'a str>,
}

/// The consent the operator gave, as the audit's fixed pair of strings for a grant or a token.
fn consent_detail(allow_destructive: bool) -> &'static str {
    if allow_destructive {
        "allow_destructive=true"
    } else {
        "allow_destructive=false"
    }
}

/// The state axum hands to every handler and extractor.
type AppState = Arc<Inner>;

impl Inner {
    /// Whether this console has a device with `id`: the check every per-device operator route makes
    /// before it touches a store that would happily key on an unknown id.
    fn has_device(&self, id: &str) -> bool {
        lock(&self.devices).contains(id)
    }
}

impl Inner {
    /// The operator behind this request: a live session, or a verified Access identity.
    ///
    /// Access is checked only when the operator configured it, and a token that does not verify falls
    /// through to the session check — the feature can let a request in, never let one skip a check.
    fn operator(&self, headers: &HeaderMap) -> Option<Operator> {
        self.identity(headers).0
    }

    /// The operator, and the reason there is none when Access was the intended identity.
    ///
    /// One verification serves both callers: the routes only need `operator`, and `/session` needs
    /// the report so a page can say *why* it is showing a token box.
    fn identity(&self, headers: &HeaderMap) -> (Option<Operator>, AccessReport) {
        let header = crate::access::token_from_headers(headers).is_some();
        let Some(access) = &self.access else {
            return (
                self.session_operator(headers),
                AccessReport {
                    configured: false,
                    header,
                    error: None,
                },
            );
        };
        let Some(token) = crate::access::token_from_headers(headers) else {
            return (
                self.session_operator(headers),
                AccessReport {
                    configured: true,
                    header: false,
                    error: None,
                },
            );
        };
        match access.verify(token) {
            Ok(claims) => (
                Some(Operator {
                    email: claims.email,
                    via: "access",
                }),
                AccessReport {
                    configured: true,
                    header: true,
                    error: None,
                },
            ),
            Err(error) => (
                self.session_operator(headers),
                AccessReport {
                    configured: true,
                    header: true,
                    error: Some(error.to_string()),
                },
            ),
        }
    }

    /// The operator a live console session names, if any.
    fn session_operator(&self, headers: &HeaderMap) -> Option<Operator> {
        if self.session_valid(headers) {
            return Some(Operator {
                email: None,
                via: "session",
            });
        }
        None
    }

    /// Whether the request carries a live session token, compared over bytes in constant time.
    fn session_valid(&self, headers: &HeaderMap) -> bool {
        let Some(presented) = bearer_secret(headers) else {
            return false;
        };

        let now = (self.now)();
        let presented = hash_of(&presented);
        let sessions = lock(&self.sessions);
        let mut matched = false;
        for session in sessions.iter() {
            let token_matches = constant_time_eq(&presented, &session.token_hash);
            let live = now < session.expires_at_ms;
            matched |= token_matches && live;
        }
        matched
    }

    /// Whether this request presents the device token issued at registration.
    fn device_authorized(&self, device_id: &str, headers: &HeaderMap) -> bool {
        let Some(token) = bearer_secret(headers) else {
            return false;
        };
        lock(&self.devices).verify_token(device_id, &token)
    }

    /// Write one audit line for an action that succeeded.
    async fn record(
        &self,
        kind: &'static str,
        device_id: Option<&str>,
        status: StatusCode,
        detail: Option<&str>,
    ) {
        self.record_tool(kind, device_id, status, detail, None, None)
            .await;
    }

    /// Write one audit line from named facts, so the interchangeable `Option`s cannot be transposed.
    async fn record_facts(&self, facts: AuditFacts<'_>) {
        self.audit
            .append(&audit::Entry {
                ts_ms: (self.now)(),
                kind: facts.kind,
                device_id: facts.device_id,
                status: facts.status.as_u16(),
                detail: facts.detail,
                tool: facts.tool,
                token_id: facts.token_id,
            })
            .await;
    }

    /// Write one audit line, naming the tool when the action was a `tools/call` and the permanent
    /// token when one authenticated it. `token_id` is the record's id, never the secret.
    async fn record_tool(
        &self,
        kind: &'static str,
        device_id: Option<&str>,
        status: StatusCode,
        detail: Option<&str>,
        tool: Option<&str>,
        token_id: Option<&str>,
    ) {
        self.record_facts(AuditFacts {
            kind,
            device_id,
            status,
            detail,
            tool,
            token_id,
        })
        .await;
    }

    /// Audit an **authenticated** refusal as `denied` and hand the error back.
    async fn deny(&self, device_id: Option<&str>, error: ApiError) -> ApiError {
        self.record("denied", device_id, error.status, Some(error.code))
            .await;
        error
    }

    /// Record an **unauthenticated** refusal in the denial log and hand the error back.
    ///
    /// These never reach `audit.jsonl`: a caller with no credential must not be able to rotate the
    /// operator's audit history away, so the denial log is a separate, rate-limited file.
    async fn refuse(&self, device_id: Option<&str>, error: ApiError) -> ApiError {
        let detail = error.code;
        self.refuse_as(device_id, error, detail).await
    }

    /// Record an unauthenticated refusal with a reason more specific than its wire code.
    ///
    /// The wire deliberately gives every bad credential `unauthorized`; the denial log is where the
    /// difference ("bad operator token" versus a missing session) is worth keeping.
    async fn refuse_as(
        &self,
        device_id: Option<&str>,
        error: ApiError,
        detail: &'static str,
    ) -> ApiError {
        self.denials
            .append(&audit::Entry {
                ts_ms: (self.now)(),
                kind: "denied",
                device_id,
                status: error.status.as_u16(),
                detail: Some(detail),
                tool: None,
                token_id: None,
            })
            .await;
        error
    }
}

/// A device id as the audit records it: canonical lowercase hex, or absent when the caller named
/// something that is not a 32-byte id at all. The audit is not a place for an arbitrary string.
fn audit_device_id(presented: &str) -> Option<String> {
    hex::decode_32(presented).map(|bytes| hex::encode(&bytes))
}

/// The only fact about a device answer the audit may keep: its numeric JSON-RPC error code.
///
/// A successful result says nothing beyond the `mcp` kind. An error envelope is a JSON-RPC answer
/// and is forwarded whole to the operator, but only its code spans the "no payload body" line: the
/// message and `data` are the operator's content.
fn reply_detail(reply: &Reply) -> Option<String> {
    match reply {
        Reply::Result(_) => None,
        Reply::Error(error) => Some(
            match error
                .get("error")
                .and_then(|body| body.get("code"))
                .and_then(Value::as_i64)
            {
                Some(code) => format!("jsonrpc error {code}"),
                None => "device error".to_owned(),
            },
        ),
    }
}

/// Who the console believes is asking: a session, or an Access identity.
struct Operator {
    /// The verified email, when Access answered.
    email: Option<String>,
    /// `"access"` or `"session"`, for the probe and the audit.
    via: &'static str,
}

/// An operator session, as an extractor: every route that takes this argument is authenticated
/// before its body is read.
pub(crate) struct OperatorSession;

#[axum::async_trait]
impl axum::extract::FromRequestParts<AppState> for OperatorSession {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match state.operator(&parts.headers) {
            Some(_) => Ok(OperatorSession),
            None => Err(state.refuse(None, ApiError::unauthorized()).await),
        }
    }
}

/// A refusal the console renders as `{"error": "<code>"}`.
pub(crate) struct ApiError {
    status: StatusCode,
    code: &'static str,
}

impl ApiError {
    fn unauthorized() -> Self {
        ApiError {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
        }
    }

    fn forbidden(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::FORBIDDEN,
            code,
        }
    }

    fn bad_request(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::BAD_REQUEST,
            code,
        }
    }

    fn not_found(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::NOT_FOUND,
            code,
        }
    }

    fn too_many(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            code,
        }
    }

    fn gateway_timeout(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::GATEWAY_TIMEOUT,
            code,
        }
    }

    fn internal() -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.code }))).into_response()
    }
}

#[derive(Deserialize)]
struct AuthRequest {
    /// The operator token, typed into the page.
    #[serde(default)]
    token: Option<String>,
    /// A one-time code from `--login-link`, which the page takes from the URL.
    #[serde(default)]
    code: Option<String>,
}

#[derive(Deserialize)]
struct RegisterDevice {
    id: String,
    public_key: String,
    /// The one-time code the operator minted; spent by this registration.
    code: String,
    /// The device's clock when it signed, in Unix milliseconds.
    ts_ms: u64,
    /// The Ed25519 signature over `register\n{id}\n{code}\n{ts_ms}`, in hex.
    signature: String,
    /// An optional display name. It is not covered by the signature; it is presentation only.
    #[serde(default)]
    name: Option<String>,
}

#[derive(Serialize)]
struct AuthResponse {
    token: String,
    expires_at_ms: u64,
}

#[derive(Serialize)]
struct EnrollResponse {
    code: String,
    expires_at_ms: u64,
}

#[derive(Serialize)]
struct RegisterResponse {
    #[serde(flatten)]
    device: Device,
    /// The channel token. Returned once, never again; the registry keeps only its hash.
    token: String,
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    devices: usize,
    uptime_s: u64,
    /// The deepest any per-device outbound queue has been since the console started. It is a bound
    /// an operator can see, not a secret.
    max_queued_frames: usize,
    /// The label the deploy passed, or the crate version for a development start.
    version: String,
}

/// The two vendored assets behind `/assets/`.
mod assets {
    use axum::http::header;
    use axum::response::{IntoResponse, Response};

    pub(super) const JS: &str = include_str!("../assets/xterm.js");
    pub(super) const CSS: &str = include_str!("../assets/xterm.css");
    pub(super) const FIT: &str = include_str!("../assets/addon-fit.js");
    /// The terminal page's glue: the loop between xterm and `/next/terminal`.
    pub(super) const TERMINAL: &str = include_str!("../assets/terminal.js");
    /// The brand mark at 128 px: the browser tab's icon and the console's face in a task switcher.
    pub(super) const ICON: &[u8] = include_bytes!("../assets/icon.png");

    /// A tiny responder: the bodies are static, and the content types are the two that matter.
    pub(super) struct Asset {
        pub(super) body: &'static str,
        pub(super) content_type: &'static str,
    }

    impl IntoResponse for Asset {
        fn into_response(self) -> Response {
            (
                [(header::CONTENT_TYPE, self.content_type)],
                [(header::CACHE_CONTROL, "public, max-age=86400")],
                self.body,
            )
                .into_response()
        }
    }
}

/// `GET /`: the console is the Rust page. `web/` is gone, so a bookmark to the old root is sent to
/// the page that replaced it rather than to a 404.
async fn root_redirect() -> Redirect {
    Redirect::to("/next")
}

/// Every answer this console gives is for one request, and a cache that disagrees is how a browser
/// keeps running yesterday's page — or yesterday's `{"authenticated":false}` — after a deploy or a
/// login. One layer says so instead of a list of routes that has to stay complete as routes are added.
async fn no_store(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    // The credential-entry surface must not be frameable: a clickjacked token box is the one way a
    // page this small can leak the operator secret.
    response.headers_mut().insert(
        header::X_FRAME_OPTIONS,
        axum::http::HeaderValue::from_static("DENY"),
    );
    response
}

/// `GET /health`: unauthenticated, and deliberately so — a supervisor and a human need one route
/// that answers before any credential has been presented. It reads only counts, never a secret.
async fn health(State(app): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        devices: lock(&app.devices).len(),
        uptime_s: app.started.elapsed().as_secs(),
        max_queued_frames: app.channels.peak_queued_frames(),
        version: app.version.clone(),
    })
}

/// `POST /auth`: exchange the operator token for a session token.
/// `GET /session`: whether this request already carries an operator identity.
///
/// The page calls it on load: when Access verified the request, there is no token to type, and the
/// answer says so. It never returns a credential — only whether one is needed.
#[derive(serde::Serialize)]
struct SessionProbe {
    authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    via: Option<&'static str>,
    /// What the console made of the Access identity. The page shows this when it has to draw the
    /// login box, so "Access did not sign me in" stops being indistinguishable from "the session
    /// expired an hour ago and this tab never noticed".
    access: AccessReport,
}

/// The three facts that explain a login box that appeared while Access was supposed to be the
/// identity: whether the trust is configured, whether the header arrived, and why it was refused.
#[derive(Serialize)]
struct AccessReport {
    configured: bool,
    header: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn session_probe(State(app): State<AppState>, headers: HeaderMap) -> Json<SessionProbe> {
    let (operator, access) = app.identity(&headers);
    match operator {
        Some(operator) => Json(SessionProbe {
            authenticated: true,
            email: operator.email,
            via: Some(operator.via),
            access,
        }),
        None => Json(SessionProbe {
            authenticated: false,
            email: None,
            via: None,
            access,
        }),
    }
}

/// `POST /logout`: end the operator session this request presents.
///
/// The session list is memory, like the session itself, so removing the matching token is the whole
/// of logout. The token compared is the one the request presents, so another operator's session is
/// untouched. It answers 204 whether or not there was a token to remove: an Access identity has no
/// in-memory token (the identity provider owns that session), and the page clears regardless of the
/// answer. No audit line on purpose — session lifecycle is not a device action.
async fn logout(State(app): State<AppState>, _: OperatorSession, headers: HeaderMap) -> StatusCode {
    if let Some(presented) = bearer_secret(&headers) {
        let token_hash = hash_of(&presented);
        let mut sessions = lock(&app.sessions);
        sessions.retain(|session| !constant_time_eq(&session.token_hash, &token_hash));
        if let Err(error) = crate::auth::save_sessions(&app.data, &sessions) {
            eprintln!("summrise-console: the sessions could not be persisted: {error}");
        }
    }
    StatusCode::NO_CONTENT
}

/// Verify the operator's secret: a typed token, or a one-time login code. One outcome for both
/// callers, `/auth` and the page's form, including the denial line.
async fn operator_from_credentials(
    app: &AppState,
    code: Option<&str>,
    token: Option<&str>,
) -> Result<(), ApiError> {
    let accepted = if let Some(code) = code {
        let now = (app.now)();
        match crate::store::spend_login_code(&app.data, code, now) {
            Ok(()) => true,
            Err(reason) => {
                eprintln!("summrise-console: refused a login code: {reason}");
                return Err(app
                    .refuse_as(None, ApiError::unauthorized(), "bad_login_code")
                    .await);
            }
        }
    } else {
        let Some(presented) = token.and_then(hex::decode_32) else {
            return Err(app
                .refuse_as(None, ApiError::unauthorized(), "bad_operator_token")
                .await);
        };
        constant_time_eq(&presented, &app.operator_token)
    };
    if !accepted {
        return Err(app
            .refuse_as(None, ApiError::unauthorized(), "bad_operator_token")
            .await);
    }
    Ok(())
}

/// Mint one session, drop the expired ones, and return it with the token the caller must present.
///
/// The file is written here rather than on a timer: a session the operator can use must survive the
/// next restart, and a session that is not on disk is one a deploy forgets.
fn new_session(app: &AppState) -> (Session, [u8; 32]) {
    let now = (app.now)();
    let token = new_session_token();
    let session = Session {
        token_hash: hash_of(&token),
        expires_at_ms: now.saturating_add(SESSION_LIFETIME_MS),
        csrf: random_32(),
    };
    {
        let mut sessions = lock(&app.sessions);
        sessions.retain(|session| session.expires_at_ms > now);
        sessions.push(session.clone());
    }
    if let Err(error) = crate::auth::save_sessions(&app.data, &lock(&app.sessions)) {
        eprintln!("summrise-console: the sessions could not be persisted: {error}");
    }
    (session, token)
}

/// The stored form of a session token: its SHA-256 bytes, never the token.
fn hash_of(token: &[u8; 32]) -> [u8; 32] {
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&sha2::Sha256::digest(token));
    hash
}

async fn authenticate(
    State(app): State<AppState>,
    Json(request): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    operator_from_credentials(&app, request.code.as_deref(), request.token.as_deref()).await?;
    // A session is a thing that happened. It has no device, so it goes in as `auth` with the
    // detail a reader needs and nothing about the credential itself.
    let (session, token) = new_session(&app);
    Ok(Json(AuthResponse {
        token: hex::encode(&token),
        expires_at_ms: session.expires_at_ms,
    }))
}

/// The cookie name the page uses. A page cookie, not an API credential: nothing outside `/next`
/// reads it.
const SESSION_COOKIE: &str = "summrise_session";

fn cookie_session(app: &AppState, headers: &HeaderMap) -> Option<Session> {
    let presented = headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().strip_prefix("summrise_session="))
        .find_map(hex::decode_32)?;
    let now = (app.now)();
    let token_hash = hash_of(&presented);
    let sessions = lock(&app.sessions);
    sessions
        .iter()
        .find(|session| {
            constant_time_eq(&session.token_hash, &token_hash) && now < session.expires_at_ms
        })
        .cloned()
}

fn secure_request(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("https"))
}

fn session_cookie(token: &[u8; 32], secure: bool) -> HeaderValue {
    let mut cookie = format!(
        "{SESSION_COOKIE}={}; Path=/next; HttpOnly; SameSite=Lax; Max-Age={}",
        hex::encode(token),
        SESSION_LIFETIME_MS / 1000
    );
    if secure {
        cookie.push_str("; Secure");
    }
    HeaderValue::from_str(&cookie).expect("a valid Set-Cookie")
}

fn clear_session_cookie() -> HeaderValue {
    HeaderValue::from_static("summrise_session=; Path=/next; HttpOnly; SameSite=Lax; Max-Age=0")
}

#[derive(Deserialize)]
struct NextLogin {
    token: String,
}

#[derive(Deserialize)]
struct NextLogout {
    /// Absent is a refusal, not a deserialization error: the page promises 403 for a form that does
    /// not carry its session's CSRF value, and that includes a form with no field at all.
    csrf: Option<String>,
}

/// `GET /next`: the login form without a live cookie, the page with one.
#[derive(Deserialize, Default)]
struct NextQuery {
    code: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct NextRename {
    csrf: String,
    name: String,
}

#[derive(Deserialize)]
struct NextRemove {
    csrf: String,
}

#[derive(Deserialize)]
struct NextEnroll {
    csrf: String,
}

/// The cookie session a `/next` mutation presents, if its CSRF value is this session's.
fn next_session(app: &AppState, headers: &HeaderMap, csrf: Option<&str>) -> Option<Session> {
    let session = cookie_session(app, headers)?;
    let presented = csrf.and_then(hex::decode_32)?;
    constant_time_eq(&presented, &session.csrf).then_some(session)
}

async fn next_page(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<NextQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    Html(
        // The fleet is the page an operator lands on; six lines of what the console just did
        // belong there, read from the same log `/audit` serves.
        page::dashboard(
            &device_views(&app),
            &hex::encode(&session.csrf),
            &app.version,
            query.code.as_deref(),
            query.error.as_deref(),
            &app.audit.newest(6).await,
        )
        .into_string(),
    )
    .into_response()
}

/// `POST /next/fleet/:id/name`: rename a device from the page. The name rules are the API's, and an
/// empty or invalid name is refused rather than silently clearing a label.
async fn next_rename(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<NextRename>,
) -> Response {
    let Some(_session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let trimmed = form.name.trim();
    if trimmed.is_empty() || trimmed.len() > 64 || trimmed.chars().any(|c| c.is_control()) {
        return Redirect::to("/next?error=invalid_name").into_response();
    }
    let renamed = {
        let mut devices = lock(&app.devices);
        devices.rename(&device_id, Some(trimmed.to_owned()))
    };
    match renamed {
        Ok(Some(_)) => Redirect::to("/next").into_response(),
        Ok(None) => Redirect::to("/next?error=no_such_device").into_response(),
        Err(_) => Redirect::to("/next?error=registry_unwritable").into_response(),
    }
}

/// `POST /next/fleet/:id/remove`: forget a device from the page.
async fn next_remove(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<NextRemove>,
) -> Response {
    let Some(_session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let removed = {
        let mut devices = lock(&app.devices);
        devices.remove(&device_id)
    };
    match removed {
        Ok(true) => {
            app.record(
                "device_remove",
                Some(&device_id),
                StatusCode::NO_CONTENT,
                Some("page"),
            )
            .await;
            Redirect::to("/next").into_response()
        }
        Ok(false) => Redirect::to("/next?error=no_such_device").into_response(),
        Err(_) => Redirect::to("/next?error=registry_unwritable").into_response(),
    }
}

/// `POST /next/enroll`: mint a one-time enrollment code and show it once.
async fn next_enroll(
    State(app): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NextEnroll>,
) -> Response {
    let Some(_session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    // The guard ends here: holding it across the audit's await would make the future non-Send.
    let issued = lock(&app.codes).issue((app.now)());
    match issued {
        Ok((code, _)) => {
            // The same action the API's `/enroll` records: a code exists because someone minted it.
            app.record("enroll", None, StatusCode::OK, None).await;
            Redirect::to(&format!("/next?code={code}")).into_response()
        }
        Err(IssueError::TooMany) => Redirect::to("/next?error=too_many_codes").into_response(),
        Err(IssueError::Storage) => Redirect::to("/next?error=code_storage").into_response(),
    }
}

/// `GET /next/audit`: the audit log, read-only, behind the page's cookie.
async fn next_audit(State(app): State<AppState>, headers: HeaderMap) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let rows = app.audit.newest(DEFAULT_LOG_LIMIT).await;
    Html(page::logs(&rows, "Audit", &hex::encode(&session.csrf), &app.version).into_string())
        .into_response()
}

/// `GET /next/denials`: the unauthenticated-refusal log, read-only, behind the page's cookie.
async fn next_denials(State(app): State<AppState>, headers: HeaderMap) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let rows = app.denials.newest(DEFAULT_LOG_LIMIT).await;
    Html(page::logs(&rows, "Denials", &hex::encode(&session.csrf), &app.version).into_string())
        .into_response()
}

/// The terminal page's request bodies. All three are small on purpose: the page sends a session id
/// in the path and nothing else, and the device is the one that decides what the call means.
#[derive(Deserialize)]
struct TerminalCreate {
    #[serde(default = "pty_kind")]
    kind: String,
    rows: Option<u16>,
    cols: Option<u16>,
    /// `kind: "ssh"`.
    #[serde(default)]
    host: String,
    ssh_port: Option<u16>,
    #[serde(default)]
    user: String,
    #[serde(default)]
    password: String,
    /// `kind: "serial"`.
    #[serde(default)]
    serial_port: String,
    baud: Option<u32>,
}

fn pty_kind() -> String {
    "pty".to_owned()
}

#[derive(Deserialize)]
struct TerminalWrite {
    data: String,
}

#[derive(Deserialize)]
struct TerminalResize {
    rows: u16,
    cols: u16,
}

#[derive(Deserialize, Default)]
struct TerminalReadQuery {
    #[serde(default)]
    cursor: u64,
}

/// The page's CSRF value, from the header its fetch sends. A form field is the other spelling; this
/// surface is JSON, so the header is the one that carries it.
fn terminal_csrf(app: &AppState, headers: &HeaderMap) -> Option<Session> {
    let csrf = headers.get("x-csrf").and_then(|value| value.to_str().ok());
    next_session(app, headers, csrf)
}

fn terminal_refusal(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// The sentence a refused [`device_tool`] carried, for a page that renders sentences rather than JSON.
async fn refusal_sentence(response: Response) -> String {
    let (_, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, 64 * 1024).await else {
        return "the device refused the call".to_owned();
    };
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    value
        .get("error")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| "the device refused the call".to_owned())
}

/// One tool call on the operator's own authority, relayed through the device's channel.
///
/// The console session is the credential — the same rule `/mcp` follows: a signed-in operator may
/// use every tool on the named device, destructive ones included. The device's answer is the tool's
/// own text; this function returns it unchanged so a refusal keeps its sentence.
async fn device_tool(
    app: &AppState,
    device_id: &str,
    tool: &str,
    arguments: Value,
) -> Result<String, Box<Response>> {
    let scope = crate::channels::TokenScope {
        token_id: "operator".to_owned(),
        device_id: device_id.to_owned(),
        scopes: vec!["*".to_owned()],
        allow_destructive: true,
        expires_at_ms: None,
    };
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": arguments },
    });
    let reply = match proxy_reply(
        app,
        device_id,
        &RequestAuth::Token(&scope),
        None,
        request,
        StatusCode::BAD_REQUEST,
        None,
    )
    .await
    {
        Ok(reply) => reply,
        Err(response) => return Err(response),
    };
    let result = match reply {
        Reply::Result(result) => result,
        Reply::Error(error) => {
            // The device wraps its answer in the JSON-RPC envelope it would send an MCP client
            // (`{"jsonrpc","id","error":{...}}`), so the sentence is one level deeper.
            let sentence = error
                .get("error")
                .and_then(|body| body.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| error.to_string());
            return Err(Box::new(terminal_refusal(
                StatusCode::BAD_GATEWAY,
                &sentence,
            )));
        }
    };
    // The device's `result` is the JSON-RPC envelope it would send an MCP client, not the bare
    // `{content, isError}`: the tool's text sits under `result.result.content`.
    let body = result.get("result").unwrap_or(&result);
    let text = body
        .get("content")
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if body
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(Box::new(terminal_refusal(StatusCode::BAD_GATEWAY, text)));
    }
    Ok(text.to_owned())
}

/// One `terminal.*` call, whose answer is the tool's JSON text.
async fn terminal_tool(
    app: &AppState,
    device_id: &str,
    tool: &str,
    arguments: Value,
) -> Result<Value, Box<Response>> {
    let text = device_tool(app, device_id, tool, arguments).await?;
    serde_json::from_str(&text).map_err(|_| {
        Box::new(terminal_refusal(
            StatusCode::BAD_GATEWAY,
            "the device's answer was not JSON",
        ))
    })
}

/// `GET /next/terminal/:device`: the page. A cookie is the page's gate; an unknown device is 404.
async fn next_terminal_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // The rail's sessions are a snapshot: the page is server-rendered, and a reload is the refresh.
    // A device that cannot answer gets an empty rail rather than an error page — the terminal itself
    // reports the failure when the operator starts a session.
    let sessions: Vec<page::TerminalSession> =
        match device_tool(&app, &device_id, "terminal.list", json!({})).await {
            Ok(text) => serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| {
                    value.as_array().map(|rows| {
                        rows.iter()
                            .map(|row| page::TerminalSession {
                                id: row["id"].as_str().unwrap_or_default().to_owned(),
                                kind: row["kind"].as_str().unwrap_or_default().to_owned(),
                                started_at_ms: row["started_at_ms"].as_u64().unwrap_or(0),
                            })
                            .collect()
                    })
                })
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };
    Html(
        page::terminal(
            &device,
            &hex::encode(&session.csrf),
            &app.version,
            &sessions,
        )
        .into_string(),
    )
    .into_response()
}

/// `POST /next/terminal/:device/create`: one pty, sized by the browser.
async fn next_terminal_create(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<TerminalCreate>,
) -> Response {
    if terminal_csrf(&app, &headers).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut arguments = match body.kind.as_str() {
        "pty" => json!({ "kind": "pty" }),
        "ssh" => {
            if body.host.trim().is_empty() || body.user.trim().is_empty() {
                return terminal_refusal(StatusCode::BAD_REQUEST, "ssh needs a host and a user");
            }
            if body.password.is_empty() {
                return terminal_refusal(
                    StatusCode::BAD_REQUEST,
                    "ssh needs a password on this page (key files are not offered yet)",
                );
            }
            json!({
                "kind": "ssh",
                "host": body.host.trim(),
                "port": body.ssh_port.unwrap_or(22),
                "user": body.user.trim(),
                "auth": { "kind": "password", "password": body.password },
            })
        }
        "serial" => {
            if body.serial_port.trim().is_empty() {
                return terminal_refusal(StatusCode::BAD_REQUEST, "serial needs a port path");
            }
            json!({
                "kind": "serial",
                "port": body.serial_port.trim(),
                "baud": body.baud.unwrap_or(115_200),
            })
        }
        _ => return terminal_refusal(StatusCode::BAD_REQUEST, "kind must be pty, ssh or serial"),
    };
    if let Some(rows) = body.rows {
        arguments["rows"] = json!(rows);
    }
    if let Some(cols) = body.cols {
        arguments["cols"] = json!(cols);
    }
    match terminal_tool(&app, &device_id, "terminal.create", arguments).await {
        Ok(answer) => Json(json!({
            "id": answer.get("id").cloned().unwrap_or(Value::Null),
            "shell": answer.get("shell").cloned().unwrap_or(Value::Null),
        }))
        .into_response(),
        Err(response) => *response,
    }
}

/// `GET /next/terminal/:device/:id/read?cursor=N`: the output after the cursor.
async fn next_terminal_read(
    State(app): State<AppState>,
    Path((device_id, session_id)): Path<(String, String)>,
    headers: HeaderMap,
    Query(query): Query<TerminalReadQuery>,
) -> Response {
    if cookie_session(&app, &headers).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    match terminal_tool(
        &app,
        &device_id,
        "terminal.read",
        json!({ "id": session_id, "cursor": query.cursor }),
    )
    .await
    {
        Ok(answer) => Json(answer).into_response(),
        Err(response) => *response,
    }
}

/// `POST /next/terminal/:device/:id/write`: keystrokes.
async fn next_terminal_write(
    State(app): State<AppState>,
    Path((device_id, session_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<TerminalWrite>,
) -> Response {
    if terminal_csrf(&app, &headers).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    match terminal_tool(
        &app,
        &device_id,
        "terminal.write",
        json!({ "id": session_id, "data": body.data }),
    )
    .await
    {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(response) => *response,
    }
}

/// `POST /next/terminal/:device/:id/resize`: the browser's grid.
async fn next_terminal_resize(
    State(app): State<AppState>,
    Path((device_id, session_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<TerminalResize>,
) -> Response {
    if terminal_csrf(&app, &headers).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    match terminal_tool(
        &app,
        &device_id,
        "terminal.resize",
        json!({ "id": session_id, "rows": body.rows, "cols": body.cols }),
    )
    .await
    {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(response) => *response,
    }
}

/// `POST /next/terminal/:device/:id/close`: end the shell.
async fn next_terminal_close(
    State(app): State<AppState>,
    Path((device_id, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if terminal_csrf(&app, &headers).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    match terminal_tool(
        &app,
        &device_id,
        "terminal.close",
        json!({ "id": session_id }),
    )
    .await
    {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(response) => *response,
    }
}

#[derive(Deserialize, Default)]
struct FilesQuery {
    #[serde(default)]
    path: String,
    error: Option<String>,
}

#[derive(Deserialize)]
struct FilesWriteForm {
    csrf: String,
    path: String,
    content: String,
}

/// `GET /next/files/:device`: the files root, server-rendered.
async fn next_files_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<FilesQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = query.path.trim_matches('/').to_owned();
    let mut error = query.error.clone();
    let entries = match device_tool(&app, &device_id, "files.list", json!({ "path": path })).await {
        Ok(text) => serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
        Err(_) => {
            error =
                Some("the device did not answer; the console's audit names the reason".to_owned());
            Vec::new()
        }
    };
    let entries: Vec<page::FileEntry> = entries
        .iter()
        .map(|entry| page::FileEntry {
            name: entry["name"].as_str().unwrap_or_default().to_owned(),
            kind: entry["kind"].as_str().unwrap_or("other").to_owned(),
            len: entry["len"].as_u64().unwrap_or(0),
        })
        .collect();
    Html(
        page::files(
            &device,
            &path,
            &entries,
            &hex::encode(&session.csrf),
            &app.version,
            error.as_deref(),
        )
        .into_string(),
    )
    .into_response()
}

/// `GET /next/files/:device/read?path=`: one file, as text.
async fn next_files_read(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<FilesQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = query.path.trim_matches('/').to_owned();
    match device_tool(&app, &device_id, "files.read", json!({ "path": path })).await {
        Ok(text) => Html(
            page::file_text(
                &device,
                &path,
                &text,
                &hex::encode(&session.csrf),
                &app.version,
            )
            .into_string(),
        )
        .into_response(),
        Err(response) => *response,
    }
}

/// `POST /next/files/:device/write`: write one text file, then show its directory.
async fn next_files_write(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<FilesWriteForm>,
) -> Response {
    let Some(_session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let path = form.path.trim().trim_start_matches('/').to_owned();
    if path.is_empty() {
        return Redirect::to(&format!("/next/files/{device_id}?error=empty_path")).into_response();
    }
    match device_tool(
        &app,
        &device_id,
        "files.write",
        json!({ "path": path, "content": form.content }),
    )
    .await
    {
        Ok(_) => {
            let directory = path.rsplit_once('/').map(|(head, _)| head).unwrap_or("");
            Redirect::to(&format!(
                "/next/files/{device_id}?path={}",
                page::encode(directory)
            ))
            .into_response()
        }
        Err(response) => *response,
    }
}

#[derive(Deserialize, Default)]
struct RunsQuery {
    error: Option<String>,
}

#[derive(Deserialize)]
struct RunsExecForm {
    csrf: String,
    command: String,
    #[serde(default)]
    args: String,
    #[serde(default)]
    cwd: String,
    timeout_ms: Option<u64>,
    shell: Option<String>,
}

/// The answer `runs.exec` gives: the status and the output the device captured for it.
#[derive(Deserialize)]
struct RunAnswer {
    id: String,
    exit_code: Option<i64>,
    timed_out: bool,
    duration_ms: u64,
    stdout: String,
    stderr: String,
    truncated: bool,
}

/// `GET /next/runs/:device`: the form and the device's own history.
async fn next_runs_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<RunsQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let rows = match device_tool(&app, &device_id, "runs.list", json!({ "limit": 20 })).await {
        Ok(text) => serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let rows: Vec<page::RunRow> = rows
        .iter()
        .map(|row| {
            let command = row["command"].as_str().unwrap_or_default();
            let args = row["args"]
                .as_array()
                .map(|args| {
                    args.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            page::RunRow {
                command_line: format!("{command} {args}").trim().to_owned(),
                started_at_ms: row["started_at_ms"].as_u64().unwrap_or(0),
                duration_ms: row["duration_ms"].as_u64().unwrap_or(0),
                exit_code: row["exit_code"].as_i64(),
                timed_out: row["timed_out"].as_bool().unwrap_or(false),
            }
        })
        .collect();
    Html(
        page::runs(
            &device,
            &rows,
            &hex::encode(&session.csrf),
            &app.version,
            query.error.as_deref(),
        )
        .into_string(),
    )
    .into_response()
}

/// `POST /next/runs/:device/exec`: one command, answered with its own output.
async fn next_runs_exec(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<RunsExecForm>,
) -> Response {
    let Some(session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let command = form.command.trim().to_owned();
    if command.is_empty() {
        return Redirect::to(&format!("/next/runs/{device_id}?error=empty_command"))
            .into_response();
    }
    let args: Vec<String> = form
        .args
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    let command_line = format!("{command} {}", args.join(" ")).trim().to_owned();
    let timeout = form.timeout_ms.unwrap_or(30_000).clamp(1_000, 600_000);
    let mut arguments = json!({
        "command": command,
        "args": args,
        "timeout_ms": timeout,
        "shell": form.shell.is_some(),
    });
    let cwd = form.cwd.trim();
    if !cwd.is_empty() {
        arguments["cwd"] = json!(cwd);
    }
    match device_tool(&app, &device_id, "runs.exec", arguments).await {
        Ok(text) => match serde_json::from_str::<RunAnswer>(&text) {
            Ok(answer) => {
                let answer = page::RunResult {
                    id: answer.id,
                    exit_code: answer.exit_code,
                    timed_out: answer.timed_out,
                    duration_ms: answer.duration_ms,
                    stdout: answer.stdout,
                    stderr: answer.stderr,
                    truncated: answer.truncated,
                };
                Html(
                    page::run_result(
                        &device,
                        &command_line,
                        &answer,
                        &hex::encode(&session.csrf),
                        &app.version,
                    )
                    .into_string(),
                )
                .into_response()
            }
            Err(_) => Redirect::to(&format!(
                "/next/runs/{device_id}?error=the_device_answer_was_not_a_run"
            ))
            .into_response(),
        },
        Err(response) => *response,
    }
}

#[derive(Deserialize, Default)]
struct SystemQuery {
    #[serde(default)]
    name: String,
    killed: Option<u64>,
    failed: Option<u64>,
}

#[derive(Deserialize)]
struct SystemKillForm {
    csrf: String,
    pid: Option<u64>,
    #[serde(default)]
    name: String,
}

/// `GET /next/state/:device`: the device's own state document, pretty-printed.
async fn next_state_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match device_tool(&app, &device_id, "state.get", json!({})).await {
        Ok(text) => {
            let pretty = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| serde_json::to_string_pretty(&value).ok())
                .unwrap_or(text);
            Html(
                page::state_document(&device, &pretty, &hex::encode(&session.csrf), &app.version)
                    .into_string(),
            )
            .into_response()
        }
        Err(response) => *response,
    }
}

/// `GET /next/system/:device`: the process list and the kill form.
async fn next_system_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<SystemQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let name = query.name.trim().to_owned();
    let mut arguments = json!({});
    if !name.is_empty() {
        arguments["name"] = json!(name);
    }
    let processes = match device_tool(&app, &device_id, "system.process_list", arguments).await {
        Ok(text) => serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let processes: Vec<page::ProcessRow> = processes
        .iter()
        .map(|process| page::ProcessRow {
            pid: process["pid"].as_u64().unwrap_or(0),
            name: process["name"].as_str().unwrap_or_default().to_owned(),
            memory_kb: process["memory_kb"].as_u64().unwrap_or(0),
        })
        .collect();
    let notice = match (query.killed, query.failed) {
        (Some(killed), Some(failed)) => Some(format!(
            "the device ended {killed} process(es); {failed} could not be ended"
        )),
        _ => None,
    };
    Html(
        page::system(
            &device,
            &processes,
            &name,
            &hex::encode(&session.csrf),
            &app.version,
            notice.as_deref(),
        )
        .into_string(),
    )
    .into_response()
}

/// `POST /next/system/:device/kill`: end a process by pid or by exact name.
async fn next_system_kill(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<SystemKillForm>,
) -> Response {
    let Some(_session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let name = form.name.trim();
    let mut arguments = json!({});
    if let Some(pid) = form.pid {
        arguments["pid"] = json!(pid);
    } else if !name.is_empty() {
        arguments["name"] = json!(name);
    } else {
        return Redirect::to(&format!("/next/system/{device_id}?failed=0")).into_response();
    }
    match device_tool(&app, &device_id, "system.process_kill", arguments).await {
        Ok(text) => {
            let answer: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let killed = answer["killed"].as_u64().unwrap_or(0);
            let failed = answer["failed"]
                .as_array()
                .map(|failed| failed.len())
                .unwrap_or(0);
            Redirect::to(&format!(
                "/next/system/{device_id}?killed={killed}&failed={failed}"
            ))
            .into_response()
        }
        Err(response) => *response,
    }
}

#[derive(Deserialize, Default)]
struct MemoryQuery {
    #[serde(default)]
    tag: String,
}

#[derive(Deserialize)]
struct MemoryAppendForm {
    csrf: String,
    text: String,
    #[serde(default)]
    tags: String,
}

#[derive(Deserialize)]
struct MemoryDeleteForm {
    csrf: String,
    id: String,
}

/// `GET /next/memory/:device`: the entries, newest first, and the forms that change them.
async fn next_memory_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<MemoryQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let tag = query.tag.trim().to_owned();
    let mut arguments = json!({ "limit": 50 });
    if !tag.is_empty() {
        arguments["tag"] = json!(tag);
    }
    let entries = match device_tool(&app, &device_id, "memory.list", arguments).await {
        Ok(text) => serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let entries: Vec<page::MemoryRow> = entries
        .iter()
        .map(|entry| page::MemoryRow {
            id: entry["id"].as_str().unwrap_or_default().to_owned(),
            text: entry["text"].as_str().unwrap_or_default().to_owned(),
            tags: entry["tags"]
                .as_array()
                .map(|tags| {
                    tags.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default(),
        })
        .collect();
    Html(
        page::memory(
            &device,
            &entries,
            &tag,
            &hex::encode(&session.csrf),
            &app.version,
            None,
        )
        .into_string(),
    )
    .into_response()
}

/// `POST /next/memory/:device/append`: keep one fact on the device.
async fn next_memory_append(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<MemoryAppendForm>,
) -> Response {
    let Some(_session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let text = form.text.trim().to_owned();
    if text.is_empty() {
        return Redirect::to(&format!("/next/memory/{device_id}")).into_response();
    }
    let tags: Vec<String> = form
        .tags
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect();
    match device_tool(
        &app,
        &device_id,
        "memory.append",
        json!({ "text": text, "tags": tags }),
    )
    .await
    {
        Ok(_) => Redirect::to(&format!("/next/memory/{device_id}")).into_response(),
        Err(response) => *response,
    }
}

/// `POST /next/memory/:device/delete`: retire one entry.
async fn next_memory_delete(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<MemoryDeleteForm>,
) -> Response {
    let Some(_session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    match device_tool(&app, &device_id, "memory.delete", json!({ "id": form.id })).await {
        Ok(_) => Redirect::to(&format!("/next/memory/{device_id}")).into_response(),
        Err(response) => *response,
    }
}

#[derive(Deserialize)]
struct MonitorAddForm {
    csrf: String,
    kind: String,
    host: String,
    port: u16,
    interval_ms: Option<u64>,
    #[serde(default)]
    path: String,
    #[serde(default)]
    expect: String,
}

#[derive(Deserialize)]
struct MonitorRemoveForm {
    csrf: String,
    id: String,
}

/// `GET /next/screen/:device`: one capture, inline as a data URL.
async fn next_screen_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match device_tool(&app, &device_id, "screen.capture", json!({})).await {
        Ok(text) => {
            let capture: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let format = capture["format"].as_str().unwrap_or("png");
            let data = capture["data"].as_str().unwrap_or_default();
            let width = capture["width"].as_u64().unwrap_or(0);
            let height = capture["height"].as_u64().unwrap_or(0);
            let url = format!("data:image/{format};base64,{data}");
            let detail = format!("{width}×{height} png, captured now");
            Html(
                page::screen(
                    &device,
                    Some(&url),
                    &detail,
                    &hex::encode(&session.csrf),
                    &app.version,
                )
                .into_string(),
            )
            .into_response()
        }
        Err(response) => {
            let sentence = refusal_sentence(*response).await;
            Html(
                page::screen(
                    &device,
                    None,
                    &sentence,
                    &hex::encode(&session.csrf),
                    &app.version,
                )
                .into_string(),
            )
            .into_response()
        }
    }
}

/// `GET /next/monitors/:device`: the watches, their newest state, and the add form.
async fn next_monitors_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut error = None;
    let targets = match device_tool(&app, &device_id, "monitor.list", json!({})).await {
        Ok(text) => serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
        Err(response) => {
            error = Some(refusal_sentence(*response).await);
            Vec::new()
        }
    };
    let rows: Vec<page::MonitorRow> = targets
        .iter()
        .map(|target| {
            let host = target["host"].as_str().unwrap_or_default();
            let port = target["port"].as_u64().unwrap_or(0);
            page::MonitorRow {
                id: target["id"].as_str().unwrap_or_default().to_owned(),
                kind: target["kind"].as_str().unwrap_or_default().to_owned(),
                target: format!("{host}:{port}"),
                interval_ms: target["interval_ms"].as_u64().unwrap_or(0),
                up_now: target["summary"]["up_now"].as_bool(),
                up_pct: target["summary"]["up_pct"].as_u64(),
                drops: target["summary"]["drops"].as_u64(),
                last_status: target["summary"]["last_status"]
                    .as_u64()
                    .and_then(|status| u16::try_from(status).ok()),
            }
        })
        .collect();
    Html(
        page::monitors(
            &device,
            &rows,
            &hex::encode(&session.csrf),
            &app.version,
            error.as_deref(),
        )
        .into_string(),
    )
    .into_response()
}

/// `POST /next/monitors/:device/add`: watch one target.
async fn next_monitor_add(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<MonitorAddForm>,
) -> Response {
    let Some(_session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let kind = form.kind.trim();
    if kind != "tcp" && kind != "http" {
        return Redirect::to(&format!("/next/monitors/{device_id}?error=kind")).into_response();
    }
    let mut arguments = json!({
        "kind": kind,
        "host": form.host.trim(),
        "port": form.port,
        "interval_ms": form.interval_ms.unwrap_or(15_000).clamp(5_000, 3_600_000),
    });
    let path = form.path.trim();
    let expect = form.expect.trim();
    if kind == "http" {
        if !path.is_empty() {
            arguments["path"] = json!(path);
        }
        if !expect.is_empty() {
            arguments["expect"] = json!(expect);
        }
    }
    match device_tool(&app, &device_id, "monitor.add", arguments).await {
        Ok(_) => Redirect::to(&format!("/next/monitors/{device_id}")).into_response(),
        Err(response) => *response,
    }
}

/// `POST /next/monitors/:device/remove`: stop watching one target.
async fn next_monitor_remove(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<MonitorRemoveForm>,
) -> Response {
    let Some(_session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    match device_tool(&app, &device_id, "monitor.remove", json!({ "id": form.id })).await {
        Ok(_) => Redirect::to(&format!("/next/monitors/{device_id}")).into_response(),
        Err(response) => *response,
    }
}

#[derive(Deserialize)]
struct BrowserUrlForm {
    csrf: String,
    url: String,
}

#[derive(Deserialize)]
struct BrowserTargetForm {
    csrf: String,
    target: String,
}

/// `GET /next/browser/:device`: the targets, and a capture of the first.
async fn next_browser_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut error = None;
    let targets = match device_tool(&app, &device_id, "browser.list", json!({})).await {
        Ok(text) => serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
        Err(response) => {
            error = Some(refusal_sentence(*response).await);
            Vec::new()
        }
    };
    let rows: Vec<page::BrowserRow> = targets
        .iter()
        .map(|target| page::BrowserRow {
            id: target["id"].as_str().unwrap_or_default().to_owned(),
            title: target["title"].as_str().unwrap_or_default().to_owned(),
            url: target["url"].as_str().unwrap_or_default().to_owned(),
            kind: target["type"].as_str().unwrap_or_default().to_owned(),
        })
        .collect();
    let mut shot = None;
    if let Some(first) = rows.first() {
        if let Ok(text) = device_tool(
            &app,
            &device_id,
            "browser.screenshot",
            json!({ "target": first.id }),
        )
        .await
        {
            let capture: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let format = capture["format"].as_str().unwrap_or("png");
            if let Some(data) = capture["data"].as_str() {
                shot = Some(format!("data:image/{format};base64,{data}"));
            }
        }
    }
    Html(
        page::browser(
            &device,
            &rows,
            shot.as_deref(),
            &hex::encode(&session.csrf),
            &app.version,
            error.as_deref(),
        )
        .into_string(),
    )
    .into_response()
}

/// `POST /next/browser/:device/open`: open a page on the device.
async fn next_browser_open(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<BrowserUrlForm>,
) -> Response {
    let Some(_session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    match device_tool(
        &app,
        &device_id,
        "browser.open",
        json!({ "url": form.url.trim() }),
    )
    .await
    {
        Ok(_) => Redirect::to(&format!("/next/browser/{device_id}")).into_response(),
        Err(response) => *response,
    }
}

/// `POST /next/browser/:device/close`: close one target.
async fn next_browser_close(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<BrowserTargetForm>,
) -> Response {
    let Some(_session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    match device_tool(
        &app,
        &device_id,
        "browser.close",
        json!({ "target": form.target }),
    )
    .await
    {
        Ok(_) => Redirect::to(&format!("/next/browser/{device_id}")).into_response(),
        Err(response) => *response,
    }
}

#[derive(Deserialize)]
struct TokenMintForm {
    csrf: String,
    name: String,
}

#[derive(Deserialize)]
struct TokenRevokeForm {
    csrf: String,
    id: String,
}

/// Every tool the registry declares: what a "one token, everything" token names.
fn every_tool() -> Vec<String> {
    summrise_core::registry::CAPABILITIES
        .iter()
        .flat_map(|capability| capability.tools.iter().map(|tool| tool.name.to_owned()))
        .collect()
}

/// `GET /next/tokens/:device`: the tokens that name this device, and the mint form.
async fn next_tokens_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let rows: Vec<page::TokenRow> = lock(&app.tokens)
        .list()
        .into_iter()
        .filter(|token| token.device_id == device_id)
        .map(|token| page::TokenRow {
            id: token.id,
            name: token.name,
            tools: token.tools.len() as u64,
            destructive: token.allow_destructive,
            created_ms: token.created_at_ms,
        })
        .collect();
    Html(
        page::tokens(
            &device,
            &rows,
            &hex::encode(&session.csrf),
            &app.version,
            None,
        )
        .into_string(),
    )
    .into_response()
}

/// `POST /next/tokens/:device/mint`: one permanent token, every tool, destructive allowed.
async fn next_token_mint(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<TokenMintForm>,
) -> Response {
    let Some(session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let known: std::collections::BTreeSet<String> = lock(&app.devices)
        .list()
        .into_iter()
        .map(|device| device.id)
        .collect();
    let created = lock(&app.tokens).create(
        crate::tokens::Draft {
            name: form.name,
            scope: crate::tokens::TokenScope::Device(device_id.clone()),
            tools: every_tool(),
            allow_destructive: true,
            expires_in_s: None,
        },
        |id| known.contains(id),
        (app.now)(),
    );
    let (meta, secret) = match created {
        Ok(created) => created,
        Err(_) => {
            return Redirect::to(&format!("/next/tokens/{device_id}?error=refused")).into_response()
        }
    };
    let scheme = if secure_request(&headers) {
        "https"
    } else {
        "http"
    };
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("127.0.0.1");
    let config = format!(
        "- id: mcp-summrise\n  name: '@deepseek-ai/dsh-mcp-client'\n  config:\n    serverName: summrise\n    transport: streamable-http\n    url: {scheme}://{host}/mcp/{device_id}\n    headers:\n      Authorization: Bearer {secret}"
    );
    Html(
        page::token_created(
            &device,
            &meta.name,
            &secret,
            &config,
            &hex::encode(&session.csrf),
            &app.version,
        )
        .into_string(),
    )
    .into_response()
}

/// `POST /next/tokens/:device/revoke`: retire one token.
async fn next_token_revoke(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<TokenRevokeForm>,
) -> Response {
    let Some(_session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let _ = lock(&app.tokens).revoke(&form.id);
    Redirect::to(&format!("/next/tokens/{device_id}")).into_response()
}

/// `GET /next/vitals/:device`: one sample of what the device reports about itself.
async fn next_vitals_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let facts = match device_tool(&app, &device_id, "host.facts", json!({})).await {
        Ok(text) => serde_json::from_str::<Value>(&text).unwrap_or(Value::Null),
        Err(response) => return *response,
    };
    let disks = facts["disks"]
        .as_array()
        .map(|disks| {
            disks
                .iter()
                .map(|disk| page::VitalsDisk {
                    name: disk["name"].as_str().unwrap_or_default().to_owned(),
                    total: disk["total_bytes"].as_u64().unwrap_or(0),
                    free: disk["free_bytes"].as_u64().unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();
    let sample = page::Vitals {
        machine: facts["machine"].as_str().unwrap_or_default().to_owned(),
        os: format!(
            "{} {}",
            facts["os"]["name"].as_str().unwrap_or_default(),
            facts["os"]["version"].as_str().unwrap_or_default()
        )
        .trim()
        .to_owned(),
        cores: facts["cpu"]["cores"].as_u64().unwrap_or(0),
        uptime_s: facts["uptime_s"].as_u64().unwrap_or(0),
        memory_total: facts["memory"]["total_bytes"].as_u64().unwrap_or(0),
        memory_available: facts["memory"]["available_bytes"].as_u64().unwrap_or(0),
        disks,
        version: facts["update"]["current"]["version"]
            .as_str()
            .unwrap_or("—")
            .to_owned(),
        channel: facts["update"]["current"]["channel"]
            .as_str()
            .unwrap_or("—")
            .to_owned(),
    };
    Html(page::vitals(&device, &sample, &hex::encode(&session.csrf), &app.version).into_string())
        .into_response()
}

/// `POST /next/auth`: the page's sign-in. The same operator secret as `/auth`, the same denial
/// line; the answer is a cookie and a redirect, not JSON.
async fn next_login(
    State(app): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NextLogin>,
) -> Response {
    if operator_from_credentials(&app, None, Some(&form.token))
        .await
        .is_err()
    {
        return (
            StatusCode::UNAUTHORIZED,
            Html(page::login(Some("That token was not accepted.")).into_string()),
        )
            .into_response();
    }
    let (_session, token) = new_session(&app);
    let mut response = Redirect::to("/next").into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(&token, secure_request(&headers)),
    );
    response
}

/// `POST /next/logout`: end the session the cookie names, if the form echoes its CSRF value.
async fn next_logout(
    State(app): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NextLogout>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Redirect::to("/next").into_response();
    };
    let Some(csrf) = form.csrf.as_deref().and_then(hex::decode_32) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if !constant_time_eq(&csrf, &session.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    {
        let mut sessions = lock(&app.sessions);
        sessions.retain(|entry| !constant_time_eq(&entry.token_hash, &session.token_hash));
    }
    if let Err(error) = crate::auth::save_sessions(&app.data, &lock(&app.sessions)) {
        eprintln!("summrise-console: the sessions could not be persisted: {error}");
    }
    let mut response = Redirect::to("/next").into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, clear_session_cookie());
    response
}

/// `POST /enroll`: the operator mints one one-time code for a device.
async fn enroll(
    State(app): State<AppState>,
    _: OperatorSession,
) -> Result<Json<EnrollResponse>, ApiError> {
    let issued = lock(&app.codes).issue((app.now)());
    let (code, expires_at_ms) = match issued {
        Ok(issued) => issued,
        Err(IssueError::TooMany) => {
            return Err(app.deny(None, ApiError::too_many("too_many_codes")).await)
        }
        Err(IssueError::Storage) => return Err(app.deny(None, ApiError::internal()).await),
    };

    app.record("enroll", None, StatusCode::OK, None).await;

    Ok(Json(EnrollResponse {
        code,
        expires_at_ms,
    }))
}

/// `POST /devices`: a device proves possession of a key and a one-time code, then receives the token
/// its channel uses.
///
/// The checks run in this order, and the code is spent only after the signature verifies:
///
/// 1. `public_key` is a valid Ed25519 point and `id` decodes to the same 32 bytes (400);
/// 2. `ts_ms` is within five minutes of the console's clock (403 `stale_timestamp`);
/// 3. the signature verifies over exactly `register\n{id}\n{code}\n{ts_ms}` (403 `bad_signature`);
/// 4. the code exists and has not expired (403 `unknown_code`), and is consumed atomically.
///
/// The id is canonicalised to lowercase hex, but the signed message uses the id exactly as the
/// device presented it, so a device that signs its canonical id always matches.
async fn register_device(
    State(app): State<AppState>,
    Json(request): Json<RegisterDevice>,
) -> Result<Json<RegisterResponse>, ApiError> {
    let now = (app.now)();
    let presented_id = audit_device_id(&request.id);

    let verifying = match devices::verifying_key(&request.public_key) {
        Some(verifying) => verifying,
        None => {
            return Err(app
                .refuse(
                    presented_id.as_deref(),
                    ApiError::bad_request("invalid_public_key"),
                )
                .await)
        }
    };
    let key_bytes = verifying.to_bytes();
    if hex::decode_32(&request.id) != Some(key_bytes) {
        return Err(app
            .refuse(
                presented_id.as_deref(),
                ApiError::bad_request("id_mismatch"),
            )
            .await);
    }

    if request.ts_ms.abs_diff(now) > REGISTRATION_SKEW_MS {
        return Err(app
            .refuse(
                presented_id.as_deref(),
                ApiError::forbidden("stale_timestamp"),
            )
            .await);
    }

    let signature =
        match hex::decode_64(&request.signature).map(|bytes| Signature::from_bytes(&bytes)) {
            Some(signature) => signature,
            None => {
                return Err(app
                    .refuse(
                        presented_id.as_deref(),
                        ApiError::bad_request("invalid_signature"),
                    )
                    .await)
            }
        };
    let message = format!(
        "register\n{}\n{}\n{}",
        request.id, request.code, request.ts_ms
    );
    if verifying
        .verify_strict(message.as_bytes(), &signature)
        .is_err()
    {
        return Err(app
            .refuse(
                presented_id.as_deref(),
                ApiError::forbidden("bad_signature"),
            )
            .await);
    }

    let consumed = lock(&app.codes).consume(&request.code, now);
    match consumed {
        Ok(()) => {}
        Err(ConsumeError::Unknown) => {
            return Err(app
                .refuse(presented_id.as_deref(), ApiError::forbidden("unknown_code"))
                .await)
        }
        Err(ConsumeError::Storage) => {
            return Err(app
                .refuse(presented_id.as_deref(), ApiError::internal())
                .await)
        }
    }

    let token = random_32();
    let registered =
        lock(&app.devices).register(&request.id, &request.public_key, request.name, &token, now);
    let device = match registered {
        Ok(device) => device,
        Err(error) => {
            let refusal = match error {
                RegisterError::InvalidPublicKey => ApiError::bad_request("invalid_public_key"),
                RegisterError::IdMismatch => ApiError::bad_request("id_mismatch"),
                RegisterError::Conflict => ApiError::bad_request("device_conflict"),
                RegisterError::Storage => ApiError::internal(),
            };
            return Err(app.refuse(presented_id.as_deref(), refusal).await);
        }
    };

    app.record("register", Some(&device.id), StatusCode::OK, None)
        .await;

    Ok(Json(RegisterResponse {
        device,
        token: hex::encode(&token),
    }))
}

#[derive(Deserialize)]
struct CreateGrant {
    device_id: String,
    tools: Vec<String>,
    expires_in_s: u64,
    /// The operator's explicit consent to destructive tools. Absent means `false`: consent is a
    /// deliberate `true`, never a default.
    #[serde(default)]
    allow_destructive: bool,
}

/// `POST /grants`: the operator asks the console to sign what a request may do on one device.
async fn create_grant(
    State(app): State<AppState>,
    _: OperatorSession,
    Json(request): Json<CreateGrant>,
) -> Result<Json<GrantEnvelope>, ApiError> {
    let device_id = audit_device_id(&request.device_id);
    if !lock(&app.devices).contains(&request.device_id) {
        return Err(app
            .deny(device_id.as_deref(), ApiError::not_found("unknown_device"))
            .await);
    }

    let signed = grants::sign(
        &app.grant_key,
        &request.device_id,
        request.tools,
        request.expires_in_s,
        request.allow_destructive,
        (app.now)(),
    );
    let signed = match signed {
        Ok(signed) => signed,
        Err(error) => {
            let refusal = match error {
                GrantError::EmptyTools => ApiError::bad_request("empty_tools"),
                GrantError::UnknownTool => ApiError::bad_request("unknown_tool"),
                GrantError::ExpiryTooShort => ApiError::bad_request("expiry_too_short"),
                GrantError::ExpiryTooLong => ApiError::bad_request("expiry_too_long"),
            };
            return Err(app.deny(device_id.as_deref(), refusal).await);
        }
    };

    app.record(
        "grant",
        Some(signed.grant.device_id.as_str()),
        StatusCode::OK,
        Some(consent_detail(signed.grant.allow_destructive)),
    )
    .await;

    Ok(Json(GrantEnvelope {
        grant: signed.grant,
        signature: hex::encode(&signed.signature.to_bytes()),
    }))
}

#[derive(Deserialize)]
struct CreateToken {
    name: String,
    /// One device, for the per-device endpoint. Exactly one of this and `device_ids` is required.
    #[serde(default)]
    device_id: Option<String>,
    /// Several devices, for the aggregate endpoint. Exactly one of this and `device_id` is required.
    #[serde(default)]
    device_ids: Option<Vec<String>>,
    tools: Vec<String>,
    /// The operator's explicit consent to destructive tools under this token. Absent means `false`:
    /// consent is a deliberate `true`, never a default.
    #[serde(default)]
    allow_destructive: bool,
    /// The optional lifetime in seconds. Absent (or `null`) means the token never expires, which is
    /// the default and stays so.
    #[serde(default)]
    expires_in_s: Option<u64>,
}

/// The create answer: the public facts, plus the one and only copy of the secret a client gets.
#[derive(Serialize)]
struct CreatedToken {
    #[serde(flatten)]
    meta: TokenMeta,
    /// The plaintext token. Shown once; the console stores only its SHA-256.
    token: String,
}

/// `GET /tokens`: the operator's tokens, newest first, without hashes or secrets.
async fn list_tokens(State(app): State<AppState>, _: OperatorSession) -> Json<Vec<TokenMeta>> {
    Json(lock(&app.tokens).list())
}

/// `POST /tokens`: mint one token for one device and a fixed tool set, permanent unless the
/// request gives it a lifetime.
///
/// The answer is the only time the plaintext exists outside the operator's client: the registry
/// keeps its SHA-256, the list reports metadata only, and the audit line names the device and never
/// the token.
async fn create_token(
    State(app): State<AppState>,
    _: OperatorSession,
    Json(request): Json<CreateToken>,
) -> Result<(StatusCode, Json<CreatedToken>), ApiError> {
    // The failure audit names the requested device when the request named one; an aggregate token
    // names none, so its refusal is a scope fact, not a device fact.
    let audit_id = request.device_id.as_deref().and_then(audit_device_id);
    let scope = match (request.device_id, request.device_ids) {
        (Some(device_id), None) => crate::tokens::TokenScope::Device(device_id),
        (None, Some(device_ids)) => crate::tokens::TokenScope::Devices(device_ids),
        (Some(_), Some(_)) => {
            return Err(app
                .deny(
                    audit_id.as_deref(),
                    ApiError::bad_request("ambiguous_scope"),
                )
                .await)
        }
        (None, None) => {
            return Err(app
                .deny(
                    audit_id.as_deref(),
                    ApiError::bad_request("device_required"),
                )
                .await)
        }
    };
    // Resolve the registry before taking the token lock: the closure must not hold a second lock.
    let known: std::collections::BTreeSet<String> = lock(&app.devices)
        .list()
        .into_iter()
        .map(|device| device.id)
        .collect();
    let created = lock(&app.tokens).create(
        crate::tokens::Draft {
            name: request.name,
            scope,
            tools: request.tools,
            allow_destructive: request.allow_destructive,
            expires_in_s: request.expires_in_s,
        },
        |id| known.contains(id),
        (app.now)(),
    );

    let (meta, token) = match created {
        Ok(created) => created,
        Err(error) => {
            let refusal = match error {
                TokenError::NameEmpty => ApiError::bad_request("name_empty"),
                TokenError::NameTooLong => ApiError::bad_request("invalid_name"),
                TokenError::EmptyTools => ApiError::bad_request("empty_tools"),
                TokenError::UnknownTool => ApiError::bad_request("unknown_tool"),
                TokenError::UnknownDevice => ApiError::not_found("unknown_device"),
                TokenError::EmptyDevices => ApiError::bad_request("empty_devices"),
                TokenError::DuplicateDevice => ApiError::bad_request("duplicate_device"),
                TokenError::TooManyTokens => ApiError::too_many("too_many_tokens"),
                TokenError::ExpiryTooShort => ApiError::bad_request("expiry_too_short"),
                TokenError::ExpiryTooLong => ApiError::bad_request("expiry_too_long"),
                TokenError::Storage => ApiError::internal(),
            };
            return Err(app.deny(audit_id.as_deref(), refusal).await);
        }
    };

    // The consent is an authorization fact, as it is on a grant: a later review must find it.
    let created_device = (!meta.device_id.is_empty()).then_some(meta.device_id.as_str());
    app.record(
        "token",
        created_device,
        StatusCode::CREATED,
        Some(consent_detail(meta.allow_destructive)),
    )
    .await;

    Ok((StatusCode::CREATED, Json(CreatedToken { meta, token })))
}

/// `POST /tokens/:id/rotate`: replace one token's secret in one write, keeping its scope.
///
/// The answer is the create shape: the new record's public facts and the one and only copy of the
/// new secret. The audit tells the replacement as two facts of the same action — a creation with the
/// replacement's consent and a revocation of the old record (`rotated`) — both naming the device.
async fn rotate_token(
    State(app): State<AppState>,
    Path(id): Path<String>,
    _: OperatorSession,
) -> Result<(StatusCode, Json<CreatedToken>), ApiError> {
    let rotated = lock(&app.tokens)
        .rotate(&id, (app.now)())
        .map_err(|_| ApiError::internal())?;
    let Some((meta, token)) = rotated else {
        return Err(ApiError::not_found("no_such_token"));
    };

    // The consent is an authorization fact, as it is on a creation: a later review must find it.
    app.record(
        "token",
        Some(&meta.device_id),
        StatusCode::CREATED,
        Some(consent_detail(meta.allow_destructive)),
    )
    .await;
    // The old record is gone from the file before this response is built: the same device's
    // revocation is the withdrawal half of the replacement, not a second operator action.
    app.record(
        "token_revoke",
        Some(&meta.device_id),
        StatusCode::CREATED,
        Some("rotated"),
    )
    .await;

    Ok((StatusCode::CREATED, Json(CreatedToken { meta, token })))
}

/// `DELETE /tokens/:id`: revoke one token. The record is removed, so its next request is 401.
async fn revoke_token(
    State(app): State<AppState>,
    Path(id): Path<String>,
    _: OperatorSession,
) -> Result<StatusCode, ApiError> {
    let removed = lock(&app.tokens)
        .revoke(&id)
        .map_err(|_| ApiError::internal())?;
    let Some(token) = removed else {
        return Err(ApiError::not_found("no_such_token"));
    };
    app.record(
        "token_revoke",
        Some(&token.device_id),
        StatusCode::NO_CONTENT,
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /devices/{id}/channel`: the device dials in, and this socket becomes its one live channel.
///
/// Authentication is the token registration returned. An unregistered id and a wrong token are the
/// same 401 on purpose: the route does not confirm which devices exist to someone without a token.
/// A second channel for the same device replaces this one; `channels` owns that rule.
async fn device_channel(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !app.device_authorized(&device_id, &headers) {
        let audited = audit_device_id(&device_id);
        return app
            .refuse_as(
                audited.as_deref(),
                ApiError::unauthorized(),
                "bad_device_token",
            )
            .await
            .into_response();
    }

    lock(&app.devices).touch(&device_id, (app.now)());

    // `?v=` is the device volunteering its version, so the operator's device list can name it
    // without asking the device for a grant first. Bounded and character-checked: it is displayed.
    if let Some(version) = params.get("v") {
        let plausible = !version.is_empty()
            && version.len() <= 32
            && version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
        if plausible {
            app.channels.announce_version(&device_id, version);
        }
    }
    // How the attempt before this one ended. A device cannot report a failure over the connection that
    // failed, so it tells the console on the next success — the note is one attempt behind by nature.
    let attempts = params
        .get("a")
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    let reason = params
        .get("e")
        .filter(|value| value.len() <= 200 && !value.chars().any(|c| c.is_control()))
        .cloned()
        .unwrap_or_default();
    if attempts > 0 || !reason.is_empty() {
        app.channels
            .announce_retry(&device_id, crate::channels::RetryNote { attempts, reason });
    }

    if let Some(name) = params.get("n") {
        let plausible =
            !name.is_empty() && name.len() <= 64 && !name.chars().any(|c| c.is_control());
        if plausible {
            app.channels.announce_name(&device_id, name);
            // A machine that has never been renamed should be called what it calls itself, not by a
            // digest the operator cannot read. An operator's rename always wins: the announcement
            // only fills a name that is absent.
            let has_name = lock(&app.devices)
                .get(&device_id)
                .map(|device| device.name.is_some())
                .unwrap_or(true);
            if !has_name {
                let _ = lock(&app.devices).rename(&device_id, Some(name.to_owned()));
            }
        }
    }

    upgrade.on_upgrade(move |socket| async move {
        app.channels.serve(device_id, socket).await;
    })
}

#[derive(Deserialize)]
struct McpProxy {
    device_id: String,
    /// The legacy signed grant. When absent, the operator's session is the authorization: the full
    /// tool set on the named device, destructive allowed.
    #[serde(default)]
    grant: Option<GrantEnvelope>,
    request: Value,
}

/// The tool a JSON-RPC request names, when it is a `tools/call`.
///
/// `Ok(None)` is any other method; `Err(())` is a `tools/call` whose `params.name` is missing or not
/// a non-empty string, which is a malformed JSON-RPC request rather than an authorization decision.
fn requested_tool(request: &Value) -> Result<Option<&str>, ()> {
    if request.get("method").and_then(Value::as_str) != Some("tools/call") {
        return Ok(None);
    }
    match request.get("params").and_then(|params| params.get("name")) {
        Some(Value::String(name)) if !name.is_empty() => Ok(Some(name)),
        _ => Err(()),
    }
}

/// A JSON-RPC 2.0 error answer, with the request's own id when it has one.
fn jsonrpc_error(
    status: StatusCode,
    id: Value,
    code: i64,
    message: &str,
    data: Option<Value>,
) -> Response {
    let error = match data {
        Some(data) => json!({ "code": code, "message": message, "data": data }),
        None => json!({ "code": code, "message": message }),
    };
    (
        status,
        Json(json!({ "jsonrpc": "2.0", "id": id, "error": error })),
    )
        .into_response()
}

/// How long `runs.exec` may take, above the general `/mcp` bound.
///
/// The device runs the command, and the console's own 30-second bound would turn every run longer than
/// that into a 504 while the command kept running and its history row was still written. The wait
/// follows the run's requested timeout (60 seconds when absent, the device's 1 ms..10 min range) plus a
/// grace for the frame, so a client sees the answer it asked to wait for. Every other tool answers in
/// milliseconds, and `None` leaves the general bound in charge.
fn run_wait(request: &Value) -> Option<Duration> {
    let name = request
        .get("params")
        .and_then(|params| params.get("name"))
        .and_then(|name| name.as_str())?;
    if name != "runs.exec" {
        return None;
    }
    let requested = request
        .get("params")
        .and_then(|params| params.get("arguments"))
        .and_then(|arguments| arguments.get("timeout_ms"))
        .and_then(|timeout| timeout.as_u64())
        .unwrap_or(60_000);
    let clamped = requested.clamp(1, RUNS_MAX_TIMEOUT_MS);
    Some(Duration::from_millis(clamped) + RUNS_WAIT_GRACE)
}

/// The device's own cap on `runs.exec`, mirrored so the wait cannot exceed it.
const RUNS_MAX_TIMEOUT_MS: u64 = 10 * 60 * 1000;

/// The round-trip grace added to a run's wait: the channel gets this much longer than the run itself.
const RUNS_WAIT_GRACE: Duration = Duration::from_secs(5);

/// The proxy's backstop around a run: the channel's bound plus room for the frame's round trip.
const RUNS_BACKSTOP_GRACE: Duration = Duration::from_secs(10);

/// Verify a presented grant: its signature, the device it names and its expiry.
///
/// It records nothing on purpose. The custom route's caller has already proved an operator session,
/// so its refusals are audited; the standard route has no session, so a refusal before a verified
/// grant is a denial. Which log a refusal belongs to is the caller's decision, not this function's.
fn verify_presented(
    app: &AppState,
    device_id: &str,
    grant: &GrantEnvelope,
) -> Result<(), ApiError> {
    match grants::verify(&app.grant_key, grant, device_id, (app.now)()) {
        Ok(()) => Ok(()),
        Err(VerifyError::BadSignature) => Err(ApiError::forbidden("invalid_grant_signature")),
        Err(VerifyError::DeviceMismatch) => Err(ApiError::forbidden("grant_device_mismatch")),
        Err(VerifyError::Expired) => Err(ApiError::forbidden("grant_expired")),
    }
}

/// Whether this console has the device, separate from the signature check so each route can decide
/// which refusal is a denial and which is an audited action.
fn ensure_registered(app: &AppState, device_id: &str) -> Result<(), ApiError> {
    if lock(&app.devices).contains(device_id) {
        Ok(())
    } else {
        Err(ApiError::not_found("unknown_device"))
    }
}

/// Mint the grant one token authorizes for this request.
///
/// The scope is the token's, never the request's: the record's device, tools and destructive consent
/// are what get signed, for [`crate::tokens::MINTED_GRANT_LIFETIME_S`] seconds. The caller of this
/// route therefore cannot widen what the token may reach, and the device verifies this grant as it
/// verifies any other.
///
/// Why a presented API token did not become a grant.
enum TokenRefusal {
    /// The credential itself does not resolve: unknown, revoked, expired, or not mintable. The
    /// caller is not authenticated, so this is a denial-log line, never the operator's audit.
    Unauthenticated(&'static str),
    /// A live token for another device. The caller *is* authenticated, so the operator's audit gets
    /// the refusal — a credential used against the wrong machine is exactly what an audit is for.
    ForeignDevice,
}

/// The one wire answer for an aggregate token that failed lookup or minting: 401, with the denial
/// detail the refusal names. A wrong, foreign or revoked credential is not distinguished on the wire.
async fn refuse_token(app: &AppState, refusal: TokenRefusal) -> Response {
    let detail = match refusal {
        TokenRefusal::Unauthenticated(detail) => detail,
        TokenRefusal::ForeignDevice => "foreign_api_token",
    };
    app.refuse_as(None, ApiError::unauthorized(), detail)
        .await
        .into_response()
}

/// The scope one token request is answered with, and the record that authenticated it.
struct MintedScope {
    scope: crate::channels::TokenScope,
    /// The token record's id: the audit names this, never the `srm_` secret.
    token_id: String,
}

/// `Err` is the refusal kind. The wire answer is always `unauthorized`: a caller holding a wrong,
/// foreign or revoked token learns only that it was refused.
///
/// A successful signing also marks the token used, best-effort: the in-memory last use moves on
/// every exchange, the file at the store's own throttle, and a failed write does not fail this call.
fn live_token(app: &AppState, token: &str) -> Result<Token, TokenRefusal> {
    let hash = crate::tokens::hash_token(token);
    let Some(record) = lock(&app.tokens).find_by_hash(&hash) else {
        return Err(TokenRefusal::Unauthenticated("unknown_api_token"));
    };
    // An expired token is a credential failure, checked before the scope: a caller cannot learn
    // which device it belongs to with a credential that is no longer valid.
    let now = (app.now)();
    if record
        .expires_at_ms
        .is_some_and(|expires_at| now >= expires_at)
    {
        return Err(TokenRefusal::Unauthenticated("expired_api_token"));
    }
    Ok(record)
}

/// Sign a grant from the record's own scope and mark the token used.
/// The token record's own scope, as the console forwards it: no signature, because the channel is
/// the authentication. Marking the token used happens here, after the device check.
fn token_scope(app: &AppState, record: &Token, device_id: &str) -> MintedScope {
    lock(&app.tokens).mark_used(&record.hash, (app.now)());

    MintedScope {
        scope: crate::channels::TokenScope {
            token_id: record.id.clone(),
            device_id: device_id.to_owned(),
            scopes: record.tools.clone(),
            allow_destructive: record.allow_destructive,
            expires_at_ms: record.expires_at_ms,
        },
        token_id: record.id.clone(),
    }
}

/// A device token on `/mcp/:id`: one device only, never an aggregate record.
fn mint_token_scope(
    app: &AppState,
    device_id: &str,
    token: &str,
) -> Result<MintedScope, TokenRefusal> {
    let record = live_token(app, token)?;
    if record.device_ids.is_some() || record.device_id != device_id {
        return Err(TokenRefusal::ForeignDevice);
    }
    Ok(token_scope(app, &record, device_id))
}

/// An aggregate token on `/mcp/all`: the path is not a device, so the requested device is checked
/// against the record's list before anything is signed.
fn mint_aggregate_scope(
    app: &AppState,
    device_id: &str,
    token: &str,
) -> Result<MintedScope, TokenRefusal> {
    let record = live_token(app, token)?;
    let Some(allowed) = record.device_ids.as_ref() else {
        return Err(TokenRefusal::Unauthenticated("not_an_aggregate_token"));
    };
    if !allowed.iter().any(|id| id == device_id) {
        return Err(TokenRefusal::ForeignDevice);
    }
    Ok(token_scope(app, &record, device_id))
}

/// Forward one MCP exchange to a device and record the authenticated outcome.
///
/// The caller has already checked the device and verified the grant. `invalid_params_status` is the
/// HTTP status a malformed `tools/call` gets: the custom route answers 400, the standard route
/// answers 200 with a JSON-RPC error, and both are the same authorization decision. `filter` is the
/// grant's tool list when the request is `tools/list` on the standard route; the custom route passes
/// `None` and keeps its unfiltered answer. `token_id` is the permanent token's record id when one
/// authenticated this exchange, and is written on every line this function records.
async fn proxy_reply(
    app: &AppState,
    device_id: &str,
    auth: &RequestAuth<'_>,
    token_id: Option<&str>,
    request: Value,
    invalid_params_status: StatusCode,
    filter: Option<&[String]>,
) -> Result<Reply, Box<Response>> {
    let audit_id = audit_device_id(device_id);
    let request_id = request.get("id").cloned().unwrap_or(Value::Null);
    let tool = match requested_tool(&request) {
        Ok(tool) => tool.map(str::to_owned),
        Err(()) => {
            app.record_facts(AuditFacts {
                kind: "denied",
                device_id: audit_id.as_deref(),
                status: invalid_params_status,
                detail: Some("invalid_tool_call"),
                tool: None,
                token_id,
            })
            .await;
            return Err(Box::new(jsonrpc_error(
                invalid_params_status,
                request_id,
                -32602,
                "invalid params: tools/call requires params.name",
                None,
            )));
        }
    };
    let is_tools_list = request.get("method").and_then(Value::as_str) == Some("tools/list");

    if let Some(tool) = tool.as_deref() {
        // A `*` is a wildcard, as it is on the device: the operator's own scope is `["*"]`, and an
        // exact-equality check here silently refused every tool call that presented no grant.
        if !auth
            .tools()
            .iter()
            .any(|allowed| summrise_core::auth::pattern_matches(allowed, tool))
        {
            app.record_facts(AuditFacts {
                kind: "denied",
                device_id: audit_id.as_deref(),
                status: StatusCode::FORBIDDEN,
                detail: Some("tool_not_permitted"),
                tool: Some(tool),
                token_id,
            })
            .await;
            return Err(Box::new(jsonrpc_error(
                StatusCode::FORBIDDEN,
                request_id,
                -32001,
                "tool not permitted by grant",
                Some(json!({ "tool": tool })),
            )));
        }
    }

    // A run blocks on the device for its own timeout (the device's cap is ten minutes), so both bounds
    // follow it: the channel's wait for the answer and the proxy's backstop around it. Every other
    // tool keeps `mcp_timeout`, and a device that has not answered in it is one to give up on.
    let answer = match run_wait(&request) {
        // A run blocks on the device for its own timeout, so the channel waits that long and the
        // proxy's backstop sits a little above it.
        Some(wait) => {
            tokio::time::timeout(
                wait + RUNS_BACKSTOP_GRACE,
                app.channels.request_within(device_id, request, auth, wait),
            )
            .await
        }
        // Everything else answers in milliseconds; the general bound stays in charge of both layers.
        None => {
            tokio::time::timeout(
                app.mcp_timeout,
                app.channels.request(device_id, request, auth),
            )
            .await
        }
    };

    match answer {
        Err(_) => {
            app.record_facts(AuditFacts {
                kind: "denied",
                device_id: audit_id.as_deref(),
                status: StatusCode::GATEWAY_TIMEOUT,
                detail: Some("device timeout"),
                tool: tool.as_deref(),
                token_id,
            })
            .await;
            Err(Box::new(
                ApiError::gateway_timeout("device timeout").into_response(),
            ))
        }
        Ok(Err(error)) => {
            let failure = failure_for(error);
            app.record_facts(AuditFacts {
                kind: "denied",
                device_id: audit_id.as_deref(),
                status: failure.status,
                detail: Some(failure.detail),
                tool: tool.as_deref(),
                token_id,
            })
            .await;
            Err(Box::new(failure.into_response()))
        }
        Ok(Ok(mut reply)) => {
            if let (Some(allowed), true) = (filter, is_tools_list) {
                if let Reply::Result(result) = &mut reply {
                    crate::mcp::filter_tools(result, allowed);
                }
            }
            let detail = reply_detail(&reply);
            app.record_facts(AuditFacts {
                kind: "mcp",
                device_id: audit_id.as_deref(),
                status: StatusCode::OK,
                detail: detail.as_deref(),
                tool: tool.as_deref(),
                token_id,
            })
            .await;
            Ok(reply)
        }
    }
}

/// Forward one MCP exchange and turn the reply into the HTTP response.
///
/// A device `error` envelope is HTTP 200 with the JSON-RPC body unchanged: an MCP client treats a
/// non-2xx as a transport failure and would lose the error's code and message.
async fn forward(
    app: &AppState,
    device_id: &str,
    auth: &RequestAuth<'_>,
    token_id: Option<&str>,
    request: Value,
    invalid_params_status: StatusCode,
    filter: Option<&[String]>,
) -> Response {
    match proxy_reply(
        app,
        device_id,
        auth,
        token_id,
        request,
        invalid_params_status,
        filter,
    )
    .await
    {
        Ok(Reply::Result(result)) => (StatusCode::OK, Json(result)).into_response(),
        Ok(Reply::Error(error)) => (StatusCode::OK, Json(error)).into_response(),
        Err(response) => *response,
    }
}

/// `POST /mcp`: verify the operator's grant, enforce the tool it permits, then forward one MCP
/// exchange over the device channel.
///
/// The console verifies its own signature, which is what makes this route refuse a grant altered in
/// transit. It also enforces the grant's tools before the frame leaves: a `tools/call` naming a tool
/// the grant does not cover is a 403 JSON-RPC error, audited with the tool name and `denied`. The
/// verified grant itself travels with every frame, so the device can re-verify the same bytes offline
/// and apply its own authorization rules as well; the proxy's check is defence in depth, not the
/// only one.
///
/// A device `error` envelope is an MCP-level answer, not a transport failure: it is returned as HTTP
/// 200 with the JSON-RPC body unchanged, because an MCP client treats a non-2xx as a transport
/// failure and would lose the error's `code`/`message`/`data`.
///
/// A device that is between connections (the request waits [`crate::channels::RECONNECT_WAIT`] for
/// the next one) returns 503 with `retry_after_ms`, because nothing was written and the operator's
/// client can retry. A request whose channel closed after the frame was written returns 502 with no
/// retry hint: the outcome is unknown, and a retry could execute it twice. 504 remains the request
/// the device took and did not answer.
///
/// This route's refusals are audited: the operator session has already proved a credential. The
/// standard route (`/mcp/:id`) shares the same three pieces and sends its pre-authentication refusals
/// to the denial log instead.
async fn proxy_mcp(
    State(app): State<AppState>,
    _: OperatorSession,
    Json(body): Json<McpProxy>,
) -> Response {
    let audit_id = audit_device_id(&body.device_id);

    if let Err(error) = ensure_registered(&app, &body.device_id) {
        return app.deny(audit_id.as_deref(), error).await.into_response();
    }
    // The operator's session is the credential. A legacy client may still present a grant, verified
    // exactly as before; a client that presents none gets the operator's own authority: every tool on
    // the named device, destructive allowed. "I have a token" means "I may do everything".
    let legacy = match &body.grant {
        Some(grant) => {
            if let Err(error) = verify_presented(&app, &body.device_id, grant) {
                return app.deny(audit_id.as_deref(), error).await.into_response();
            }
            Some(grant)
        }
        None => None,
    };
    let operator_scope = crate::channels::TokenScope {
        token_id: "operator".to_owned(),
        device_id: body.device_id.clone(),
        scopes: vec!["*".to_owned()],
        allow_destructive: true,
        expires_at_ms: None,
    };
    let auth = match legacy {
        Some(grant) => RequestAuth::Grant(grant),
        None => RequestAuth::Token(&operator_scope),
    };
    forward(
        &app,
        &body.device_id,
        &auth,
        None,
        body.request,
        StatusCode::BAD_REQUEST,
        None,
    )
    .await
}

/// `POST /mcp/:id`: the standard MCP transport an AI client connects to.
///
/// The bearer is one of two credentials: a grant envelope, verified exactly as it always was, or a
/// token, found by hash, checked for expiry and against this path's device and exchanged for a
/// sixty-second grant with the token's tools and consent. The device only ever sees a grant.
///
/// Either credential is checked before the device lookup (no registry oracle) and before the body is
/// read (no 8 MiB buffer for an unauthenticated caller). Everything before a verified grant or a
/// valid token goes to the denial log; everything after it is an audited action.
async fn standard_mcp(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    request: axum::extract::Request,
) -> Response {
    let audit_id = audit_device_id(&device_id);
    let (parts, body) = request.into_parts();
    let headers = parts.headers;

    if !crate::mcp::content_type_is_json(&headers) {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(json!({ "error": "unsupported_media_type" })),
        )
            .into_response();
    }
    if !crate::mcp::accept_allows_json(&headers) {
        return (
            StatusCode::NOT_ACCEPTABLE,
            Json(json!({ "error": "not_acceptable" })),
        )
            .into_response();
    }

    let Some(bearer) = crate::mcp::bearer_from_headers(&headers) else {
        return app
            .refuse(audit_id.as_deref(), ApiError::unauthorized())
            .await
            .into_response();
    };
    // Two bearer types, one grant from here on: a presented envelope is verified exactly as before,
    // and a token mints an envelope with the token's scope. A token failure is a
    // pre-authentication refusal, so it goes to the denial log, never to the audit. Only a token
    // gives the forwarded lines a token id; a grant has no permanent record to name.
    enum BearerAuth {
        Grant(GrantEnvelope),
        Token(crate::channels::TokenScope),
    }
    let (bearer_auth, token_id) = match bearer {
        crate::mcp::BearerKind::Grant(grant) => {
            if let Err(error) = verify_presented(&app, &device_id, &grant) {
                return app.refuse(audit_id.as_deref(), error).await.into_response();
            }
            (BearerAuth::Grant(grant), None)
        }
        crate::mcp::BearerKind::Token(token) => match mint_token_scope(&app, &device_id, &token) {
            Ok(minted) => (BearerAuth::Token(minted.scope), Some(minted.token_id)),
            Err(TokenRefusal::Unauthenticated(detail)) => {
                return app
                    .refuse_as(audit_id.as_deref(), ApiError::unauthorized(), detail)
                    .await
                    .into_response()
            }
            Err(TokenRefusal::ForeignDevice) => {
                app.record(
                    "denied",
                    audit_id.as_deref(),
                    StatusCode::UNAUTHORIZED,
                    Some("api_token_device_mismatch"),
                )
                .await;
                return ApiError::unauthorized().into_response();
            }
        },
    };
    let auth = match &bearer_auth {
        BearerAuth::Grant(grant) => RequestAuth::Grant(grant),
        BearerAuth::Token(scope) => RequestAuth::Token(scope),
    };
    if let Err(error) = ensure_registered(&app, &device_id) {
        return app.deny(audit_id.as_deref(), error).await.into_response();
    }

    // The grant is verified, so this buffer can only be filled by a caller who proved a credential.
    let bytes = match axum::body::to_bytes(body, MAX_STANDARD_MCP_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({ "error": "payload_too_large" })),
            )
                .into_response()
        }
    };
    let request: Value = match serde_json::from_slice(&bytes) {
        Ok(request) => request,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(crate::mcp::jsonrpc_error_body(
                    Value::Null,
                    -32700,
                    "parse error",
                )),
            )
                .into_response()
        }
    };
    let Some(object) = request.as_object() else {
        return invalid_protocol_request(Value::Null, "invalid request");
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return invalid_protocol_request(Value::Null, "invalid request");
    }
    let id = object.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = object
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return invalid_protocol_request(id, "invalid request");
    };
    if object.get("id").is_none() {
        // A notification. `notifications/initialized` is the only one with meaning here; the rest
        // are dropped, as JSON-RPC says a notification gets no answer.
        return StatusCode::ACCEPTED.into_response();
    }

    match method.as_str() {
        "initialize" => (
            StatusCode::OK,
            Json(crate::mcp::jsonrpc_result(
                id,
                crate::mcp::initialize_result(&device_id, &app.version),
            )),
        )
            .into_response(),
        "ping" => (
            StatusCode::OK,
            Json(crate::mcp::jsonrpc_result(id, json!({}))),
        )
            .into_response(),
        "tools/list" => {
            forward(
                &app,
                &device_id,
                &auth,
                token_id.as_deref(),
                request,
                StatusCode::OK,
                Some(auth.tools()),
            )
            .await
        }
        "tools/call" => {
            forward(
                &app,
                &device_id,
                &auth,
                token_id.as_deref(),
                request,
                StatusCode::OK,
                None,
            )
            .await
        }
        _ => (
            StatusCode::OK,
            Json(crate::mcp::jsonrpc_error_body(
                id,
                -32601,
                "method not found",
            )),
        )
            .into_response(),
    }
}

/// Where one `device` argument points inside an aggregate token's scope.
#[derive(Clone, Debug, PartialEq, Eq)]
enum DeviceResolution {
    Resolved(String),
    OutOfScope,
    Ambiguous,
}

/// Resolve the `device` argument against the token's allowed ids and the registered names.
///
/// An exact id wins. A name is matched only among the **allowed** devices, so a name that exists
/// outside the scope is an out-of-scope request, not a hint; a name shared by two allowed devices is
/// ambiguous rather than resolved by order.
fn resolve_aggregate_device(
    allowed: &[String],
    devices: &[DeviceRef],
    requested: &str,
) -> DeviceResolution {
    if allowed.iter().any(|id| id == requested) {
        return DeviceResolution::Resolved(requested.to_owned());
    }
    let matches: Vec<&String> = devices
        .iter()
        .filter(|device| allowed.contains(&device.id) && device.name.as_deref() == Some(requested))
        .map(|device| &device.id)
        .collect();
    match matches.as_slice() {
        [] => DeviceResolution::OutOfScope,
        [id] => DeviceResolution::Resolved((*id).clone()),
        _ => DeviceResolution::Ambiguous,
    }
}

/// Remove `params.arguments.device` from a `tools/call`, returning it when it is a non-empty string.
fn take_aggregate_device(request: &mut Value) -> Result<String, ()> {
    let Some(arguments) = request
        .get_mut("params")
        .and_then(|params| params.get_mut("arguments"))
        .and_then(Value::as_object_mut)
    else {
        return Err(());
    };
    match arguments.remove("device") {
        Some(Value::String(device)) if !device.is_empty() => Ok(device),
        _ => Err(()),
    }
}

/// The token's allowed devices that are still registered, as `(id, name)`.
fn aggregate_devices(app: &AppState, allowed: &[String]) -> Vec<DeviceRef> {
    let devices = lock(&app.devices).list();
    allowed
        .iter()
        .filter_map(|id| {
            devices
                .iter()
                .find(|device| &device.id == id)
                .map(|device| DeviceRef {
                    id: device.id.clone(),
                    name: device.name.clone(),
                })
        })
        .collect()
}

/// `POST /mcp/all`: one standard MCP transport for every device an aggregate token names.
///
/// The bearer is an aggregate token only. `initialize` names the token's devices, `tools/list` is
/// answered by the first allowed device that has a channel and its schemas gain the required
/// `device` property, and `tools/call` resolves that property, strips it, and forwards an ordinary
/// per-device grant. The device never sees the token.
async fn aggregate_mcp(State(app): State<AppState>, request: axum::extract::Request) -> Response {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;

    if !crate::mcp::content_type_is_json(&headers) {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(json!({ "error": "unsupported_media_type" })),
        )
            .into_response();
    }
    if !crate::mcp::accept_allows_json(&headers) {
        return (
            StatusCode::NOT_ACCEPTABLE,
            Json(json!({ "error": "not_acceptable" })),
        )
            .into_response();
    }

    let Some(bearer) = crate::mcp::bearer_from_headers(&headers) else {
        return app
            .refuse(None, ApiError::unauthorized())
            .await
            .into_response();
    };
    let token = match bearer {
        crate::mcp::BearerKind::Token(token) => token,
        crate::mcp::BearerKind::Grant(_) => {
            return app
                .refuse_as(None, ApiError::unauthorized(), "aggregate_requires_token")
                .await
                .into_response()
        }
    };
    let record = match live_token(&app, &token) {
        Ok(record) => record,
        Err(refusal) => return refuse_token(&app, refusal).await,
    };
    let Some(allowed) = record.device_ids.clone() else {
        return app
            .refuse_as(None, ApiError::unauthorized(), "not_an_aggregate_token")
            .await
            .into_response();
    };

    // The token is authenticated, so this buffer can only be filled by a caller who proved one.
    let bytes = match axum::body::to_bytes(body, MAX_STANDARD_MCP_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({ "error": "payload_too_large" })),
            )
                .into_response()
        }
    };
    let mut request: Value = match serde_json::from_slice(&bytes) {
        Ok(request) => request,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(crate::mcp::jsonrpc_error_body(
                    Value::Null,
                    -32700,
                    "parse error",
                )),
            )
                .into_response()
        }
    };
    let Some(object) = request.as_object() else {
        return invalid_protocol_request(Value::Null, "invalid request");
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return invalid_protocol_request(Value::Null, "invalid request");
    }
    let id = object.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = object
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return invalid_protocol_request(id, "invalid request");
    };
    if object.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    match method.as_str() {
        "initialize" => {
            let devices = aggregate_devices(&app, &allowed);
            (
                StatusCode::OK,
                Json(crate::mcp::jsonrpc_result(
                    id,
                    crate::mcp::aggregate_initialize_result(&devices, &app.version),
                )),
            )
                .into_response()
        }
        "ping" => (
            StatusCode::OK,
            Json(crate::mcp::jsonrpc_result(id, json!({}))),
        )
            .into_response(),
        "tools/list" => {
            let Some(device_id) = allowed.iter().find(|id| app.channels.online(id)).cloned() else {
                return failure_for(RequestError::Offline).into_response();
            };
            let granted = match mint_aggregate_scope(&app, &device_id, &token) {
                Ok(granted) => granted,
                Err(refusal) => return refuse_token(&app, refusal).await,
            };
            let auth = RequestAuth::Token(&granted.scope);
            match proxy_reply(
                &app,
                &device_id,
                &auth,
                Some(&granted.token_id),
                request,
                StatusCode::OK,
                Some(auth.tools()),
            )
            .await
            {
                Ok(Reply::Result(mut result)) => {
                    let devices = aggregate_devices(&app, &allowed);
                    crate::mcp::add_device_argument(&mut result, &devices);
                    (StatusCode::OK, Json(result)).into_response()
                }
                Ok(Reply::Error(error)) => (StatusCode::OK, Json(error)).into_response(),
                Err(response) => *response,
            }
        }
        "tools/call" => {
            let requested = match take_aggregate_device(&mut request) {
                Ok(device) => device,
                Err(()) => {
                    app.record_facts(AuditFacts {
                        kind: "denied",
                        device_id: None,
                        status: StatusCode::BAD_REQUEST,
                        detail: Some("missing_device"),
                        tool: None,
                        token_id: Some(&record.id),
                    })
                    .await;
                    return jsonrpc_error(
                        StatusCode::BAD_REQUEST,
                        id,
                        -32602,
                        "invalid params: tools/call requires params.arguments.device",
                        None,
                    );
                }
            };
            let devices = aggregate_devices(&app, &allowed);
            match resolve_aggregate_device(&allowed, &devices, &requested) {
                DeviceResolution::Resolved(device_id) => {
                    if let Err(error) = ensure_registered(&app, &device_id) {
                        return app
                            .deny(audit_device_id(&device_id).as_deref(), error)
                            .await
                            .into_response();
                    }
                    let granted = match mint_aggregate_scope(&app, &device_id, &token) {
                        Ok(granted) => granted,
                        Err(refusal) => return refuse_token(&app, refusal).await,
                    };
                    let auth = RequestAuth::Token(&granted.scope);
                    forward(
                        &app,
                        &device_id,
                        &auth,
                        Some(&granted.token_id),
                        request,
                        StatusCode::OK,
                        None,
                    )
                    .await
                }
                DeviceResolution::OutOfScope | DeviceResolution::Ambiguous => {
                    // The audit records the `device` value as sent: a name is a legal input, and an
                    // ambiguous one is exactly what an operator needs to see. The wire answer stays
                    // the plan's one scope refusal, so the error code cannot probe for a device
                    // outside the token.
                    app.record_facts(AuditFacts {
                        kind: "denied",
                        device_id: Some(requested.as_str()),
                        status: StatusCode::FORBIDDEN,
                        detail: Some("device_not_permitted"),
                        tool: None,
                        token_id: Some(&record.id),
                    })
                    .await;
                    jsonrpc_error(
                        StatusCode::FORBIDDEN,
                        id,
                        -32001,
                        "device not permitted by token",
                        Some(json!({ "device": requested })),
                    )
                }
            }
        }
        _ => (
            StatusCode::OK,
            Json(crate::mcp::jsonrpc_error_body(
                id,
                -32601,
                "method not found",
            )),
        )
            .into_response(),
    }
}

/// The 400 every malformed JSON-RPC request gets.
fn invalid_protocol_request(id: Value, message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(crate::mcp::jsonrpc_error_body(id, -32600, message)),
    )
        .into_response()
}

/// `DELETE /devices/:id`: revoke a machine. The operator's answer to a stolen or retired box.
async fn remove_device(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    _: OperatorSession,
) -> Result<StatusCode, ApiError> {
    let removed = {
        let mut devices = lock(&app.devices);
        devices
            .remove(&device_id)
            .map_err(|_| ApiError::bad_request("registry_unwritable"))?
    };
    if !removed {
        return Err(ApiError::not_found("no_such_device"));
    }
    // The device's permanent tokens are scoped to it; a removed machine leaves no token that only a
    // hand-edit could clear. A failed write is logged rather than failing the removal: the tokens can
    // no longer reach a registered device either way.
    let revoked = lock(&app.tokens).revoke_for_device(&device_id);
    match revoked {
        Ok(revoked) => {
            for _ in revoked {
                app.record(
                    "token_revoke",
                    Some(&device_id),
                    StatusCode::NO_CONTENT,
                    Some("device_removed"),
                )
                .await;
            }
        }
        Err(_) => eprintln!(
            "summrise-console: the tokens of removed device {device_id} could not be revoked"
        ),
    }

    // The registration is gone, so the device's next connect is refused; closing the live channel
    // makes the revocation immediate instead of waiting for a disconnect.
    app.channels.disconnect(&device_id);
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /devices/:id/name`: the operator's label for a machine, or `null` to clear it.
#[derive(Deserialize)]
struct RenameDevice {
    name: Option<String>,
}

/// `GET /fleet/:id/connections`: this device's saved terminal presets.
async fn list_connections(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    _: OperatorSession,
) -> Result<Json<Vec<connections::Connection>>, ApiError> {
    if !app.has_device(&device_id) {
        return Err(ApiError::not_found("no_such_device"));
    }
    Ok(Json(lock(&app.connections).list(&device_id)))
}

/// `POST /fleet/:id/connections`: save one preset.
async fn save_connection(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    _: OperatorSession,
    Json(draft): Json<connections::Draft>,
) -> Result<(StatusCode, Json<connections::Connection>), ApiError> {
    if !app.has_device(&device_id) {
        return Err(ApiError::not_found("no_such_device"));
    }
    let saved = lock(&app.connections)
        .add(&device_id, draft)
        .map_err(ApiError::bad_request)?;
    Ok((StatusCode::CREATED, Json(saved)))
}

/// `DELETE /fleet/:id/connections/:connection`: forget one preset.
async fn remove_connection(
    State(app): State<AppState>,
    Path((device_id, connection_id)): Path<(String, String)>,
    _: OperatorSession,
) -> Result<StatusCode, ApiError> {
    if !app.has_device(&device_id) {
        return Err(ApiError::not_found("no_such_device"));
    }
    let removed = lock(&app.connections)
        .remove(&device_id, &connection_id)
        .map_err(ApiError::bad_request)?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("no_such_connection"))
    }
}

async fn rename_device(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    session: OperatorSession,
    Json(body): Json<RenameDevice>,
) -> Result<Json<DeviceView>, ApiError> {
    let name = match body.name {
        Some(name) => {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                None
            } else if trimmed.len() > 64 || trimmed.chars().any(|c| c.is_control()) {
                return Err(ApiError::bad_request("invalid_name"));
            } else {
                Some(trimmed.to_owned())
            }
        }
        None => None,
    };

    let renamed = {
        let mut devices = lock(&app.devices);
        devices
            .rename(&device_id, name)
            .map_err(|_| ApiError::bad_request("registry_unwritable"))?
    };
    let Some(device) = renamed else {
        return Err(ApiError::not_found("no_such_device"));
    };

    // Not a device action: the label lives in this console's registry, so it is not written to the
    // device audit. `session` is what proves the caller is the operator.
    let _ = session;

    Ok(Json(DeviceView {
        online: app.channels.online(&device.id),
        version: app.channels.version(&device.id),
        host: app.channels.host(&device.id),
        name: device.name.or_else(|| app.channels.name(&device.id)),
        last_attempt: app.channels.retry(&device.id).map(|note| RetryView {
            attempts: note.attempts,
            reason: note.reason,
        }),
        id: device.id,
        public_key: device.public_key,
        registered_at_ms: device.registered_at_ms,
        last_seen_ms: device.last_seen_ms,
    }))
}

/// `GET /devices`: the registry, for the operator.
/// `GET /tools`: the registry, as the grant form reads it.
async fn tool_catalogue(_: OperatorSession) -> Json<Vec<ToolView>> {
    let tools = summrise_core::registry::CAPABILITIES
        .iter()
        .flat_map(|capability| capability.tools)
        .map(|tool| ToolView {
            name: tool.name,
            destructive: tool.destructive,
            description: tool.description,
        })
        .collect();
    Json(tools)
}

/// One tool as `/tools` serves it.
#[derive(serde::Serialize)]
struct ToolView {
    name: &'static str,
    destructive: bool,
    description: &'static str,
}

/// `GET /mesh`: whether this console has a mesh view at all.
///
/// It answers 200 in both cases: "the operator did not configure it" is a fact about this console, not a
/// failure of the request.
async fn mesh_status(State(app): State<AppState>, _: OperatorSession) -> Json<serde_json::Value> {
    Json(json!({
        "configured": app.mesh_broker.is_some(),
        "broker": app.mesh_broker,
    }))
}

/// `GET /fleet/:id/mesh`: ask the broker whether this device is registered **right now**.
///
/// This contacts the broker, never the device: no operator frame and no tool call moves on this route. A
/// device the broker does not know is `reachable: false` (200); a broker that does not answer in time is
/// 504, another broker refusal is 502.
async fn device_mesh_readiness(
    State(app): State<AppState>,
    _: OperatorSession,
    Path(device_id): Path<String>,
) -> Response {
    let Some(broker) = app.mesh_broker.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "configured": false, "error": "no_mesh_broker" })),
        )
            .into_response();
    };
    // The registry is the authority on whether this id exists: a broker answer for an id the console
    // never registered is not this console's business to report.
    if !lock(&app.devices).contains(&device_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "unknown_device" })),
        )
            .into_response();
    }
    match summrise_mesh::lookup_candidates(&broker, &device_id, (app.now)(), Duration::from_secs(3))
        .await
    {
        Ok(candidates) => (
            StatusCode::OK,
            Json(json!({
                "configured": true,
                "reachable": true,
                "candidates": candidates.len(),
            })),
        )
            .into_response(),
        Err(summrise_mesh::RendezvousError::UnknownPeer(_)) => (
            StatusCode::OK,
            Json(json!({
                "configured": true,
                "reachable": false,
                "candidates": 0,
            })),
        )
            .into_response(),
        Err(summrise_mesh::RendezvousError::Timeout) => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(json!({ "configured": true, "error": "broker_timeout" })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "configured": true,
                "error": "broker_unreachable",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

fn device_views(app: &AppState) -> Vec<DeviceView> {
    lock(&app.devices)
        .list()
        .into_iter()
        .map(|device| DeviceView {
            online: app.channels.online(&device.id),
            version: app.channels.version(&device.id),
            host: app.channels.host(&device.id),
            last_attempt: app.channels.retry(&device.id).map(|note| RetryView {
                attempts: note.attempts,
                reason: note.reason,
            }),
            // A name given at registration wins; the machine's own name is a fallback for the
            // devices that never gave one, which is all of them so far.
            name: device.name.or_else(|| app.channels.name(&device.id)),
            id: device.id,
            public_key: device.public_key,
            registered_at_ms: device.registered_at_ms,
            last_seen_ms: device.last_seen_ms,
        })
        .collect()
}

async fn list_devices(State(app): State<AppState>, _: OperatorSession) -> Json<Vec<DeviceView>> {
    Json(device_views(&app))
}

/// A registered device as the operator's list sees it: the registry's facts plus the channel's.
#[derive(serde::Serialize)]
struct RetryView {
    attempts: u8,
    reason: String,
}

#[derive(serde::Serialize)]
struct DeviceView {
    id: String,
    public_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    registered_at_ms: u64,
    last_seen_ms: u64,
    /// Whether a channel is open right now.
    online: bool,
    /// The version the device announced on its channel, when it announced one.
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    /// How the attempt before the last one ended, when the device has said since.
    #[serde(skip_serializing_if = "Option::is_none")]
    last_attempt: Option<RetryView>,
    /// The machine facts the device last sent over its channel: the operator reads what the device is,
    /// including which sessions are disconnected, without minting a grant to ask.
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<serde_json::Value>,
}

/// How a transport failure is answered, and whether the operator may retry.
struct TransportFailure {
    status: StatusCode,
    error: &'static str,
    /// Present only when nothing was written and a retry is safe.
    retry_after_ms: Option<u64>,
    /// The audit detail: the same fact the wire error names.
    detail: &'static str,
}

/// Map the channel's failure onto the proxy's status contract.
///
/// `503` is reserved for "nothing was written": a device between channels, or a full queue. A frame
/// that was written and never answered is `502` with no retry hint, because the device may have
/// executed it. A socket that did not accept a frame inside its bound is `504`.
fn failure_for(error: RequestError) -> TransportFailure {
    match error {
        RequestError::Offline => TransportFailure {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error: "device reconnecting",
            retry_after_ms: Some(RETRY_AFTER_MS),
            detail: "device reconnecting",
        },
        RequestError::Disconnected => TransportFailure {
            status: StatusCode::BAD_GATEWAY,
            error: "device disconnected",
            retry_after_ms: None,
            detail: "device disconnected",
        },
        RequestError::Busy => TransportFailure {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error: "device busy",
            retry_after_ms: Some(BUSY_RETRY_AFTER_MS),
            detail: "device busy",
        },
        RequestError::SendTimeout => TransportFailure {
            status: StatusCode::GATEWAY_TIMEOUT,
            error: "device timeout",
            retry_after_ms: None,
            detail: "device timeout",
        },
        RequestError::ReplyTimeout => TransportFailure {
            status: StatusCode::GATEWAY_TIMEOUT,
            error: "device did not answer",
            retry_after_ms: None,
            detail:
                "the device accepted the request and did not answer inside the console's bound; \
                     it may still be working on it",
        },
    }
}

impl TransportFailure {
    fn into_response(self) -> Response {
        let body = match self.retry_after_ms {
            Some(retry_after_ms) => {
                json!({ "error": self.error, "retry_after_ms": retry_after_ms })
            }
            None => json!({ "error": self.error }),
        };
        if self.retry_after_ms.is_some() {
            let mut headers = HeaderMap::new();
            headers.insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
            (self.status, headers, Json(body)).into_response()
        } else {
            (self.status, Json(body)).into_response()
        }
    }
}

/// The `?limit=` shape shared by the two log routes.
///
/// `limit` is a string here so a malformed value is this service's JSON refusal rather than axum's
/// plain-text query rejection.
#[derive(Deserialize)]
struct LogQuery {
    limit: Option<String>,
}

/// The clamped limit both log routes read.
///
/// Missing means [`DEFAULT_LOG_LIMIT`]; a value above [`MAX_LOG_LIMIT`] is clamped, because neither
/// index is an unbounded read.
fn log_limit(limit: Option<String>) -> Result<usize, ApiError> {
    let limit = match limit {
        None => DEFAULT_LOG_LIMIT,
        Some(text) => text
            .parse::<usize>()
            .map_err(|_| ApiError::bad_request("invalid_limit"))?,
    };

    Ok(limit.min(MAX_LOG_LIMIT))
}

/// `GET /audit`: the operator's own actions, newest first, one JSON line each.
async fn list_audit(
    State(app): State<AppState>,
    _: OperatorSession,
    Query(query): Query<LogQuery>,
) -> Result<Json<Vec<Value>>, ApiError> {
    Ok(Json(app.audit.newest(log_limit(query.limit)?).await))
}

/// `GET /denials`: unauthenticated refusals, newest first, one JSON line each.
///
/// Operator-only and read-only. The log's write path keeps its rate limit, cap and rotation; this
/// reads it under the same append lock, so a concurrent write is not torn.
async fn list_denials(
    State(app): State<AppState>,
    _: OperatorSession,
    Query(query): Query<LogQuery>,
) -> Result<Json<Vec<Value>>, ApiError> {
    Ok(Json(app.denials.newest(log_limit(query.limit)?).await))
}

/// Lock a mutex, ignoring poisoning: a panic in one handler must not stop the process from serving
/// the rest, and every value guarded here is replaced rather than repaired.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn system_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `runs.exec` wait follows the run; everything else leaves the general bound in charge.
    #[test]
    fn a_run_waits_for_its_own_timeout_and_other_tools_do_not() {
        let call = |name: &str, arguments: Value| {
            json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "tools/call",
                "params": { "name": name, "arguments": arguments }
            })
        };

        // Absent timeout_ms: the device's default 60 s plus the grace.
        assert_eq!(
            run_wait(&call("runs.exec", json!({"command": "ver"}))),
            Some(Duration::from_secs(65))
        );
        // A requested timeout is honoured, so a 45 s run is not cut at 30 s.
        assert_eq!(
            run_wait(&call(
                "runs.exec",
                json!({"command": "ver", "timeout_ms": 45_000})
            )),
            Some(Duration::from_secs(50))
        );
        // The device's cap is mirrored: asking for a day waits ten minutes, not a day.
        assert_eq!(
            run_wait(&call(
                "runs.exec",
                json!({"command": "ver", "timeout_ms": 86_400_000})
            )),
            Some(Duration::from_secs(605))
        );
        // Anything that is not a run, or is malformed, leaves the general bound alone.
        assert_eq!(run_wait(&call("memory.list", json!({}))), None);
        assert_eq!(
            run_wait(&json!({"method": "initialize", "params": {}})),
            None
        );
        assert_eq!(
            run_wait(&json!({"method": "tools/call", "params": {"name": "runs.exec"}})),
            Some(Duration::from_secs(65))
        );
    }

    #[test]
    fn a_tool_call_is_recognized_only_for_tools_call() {
        let call = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "files.read", "arguments": {} },
        });
        assert_eq!(requested_tool(&call), Ok(Some("files.read")));

        let other = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
        assert_eq!(requested_tool(&other), Ok(None));

        let missing = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "arguments": {} },
        });
        assert_eq!(requested_tool(&missing), Err(()));

        let empty = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "" },
        });
        assert_eq!(requested_tool(&empty), Err(()));
    }

    #[test]
    fn transport_failures_map_to_the_status_contract() {
        let offline = failure_for(RequestError::Offline);
        assert_eq!(offline.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(offline.error, "device reconnecting");
        assert_eq!(offline.retry_after_ms, Some(1_000));

        let disconnected = failure_for(RequestError::Disconnected);
        assert_eq!(disconnected.status, StatusCode::BAD_GATEWAY);
        assert_eq!(disconnected.retry_after_ms, None);

        let busy = failure_for(RequestError::Busy);
        assert_eq!(busy.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(busy.error, "device busy");
        assert_eq!(busy.retry_after_ms, Some(250));

        let send_timeout = failure_for(RequestError::SendTimeout);
        assert_eq!(send_timeout.status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(send_timeout.retry_after_ms, None);
    }

    #[test]
    fn only_retryable_failures_carry_a_retry_after_header() {
        for error in [RequestError::Offline, RequestError::Busy] {
            let response = failure_for(error).into_response();
            assert!(
                response.headers().contains_key(header::RETRY_AFTER),
                "a safe retry names its delay"
            );
        }
        for error in [
            RequestError::Disconnected,
            RequestError::SendTimeout,
            RequestError::ReplyTimeout,
        ] {
            let response = failure_for(error).into_response();
            assert!(
                !response.headers().contains_key(header::RETRY_AFTER),
                "an unknown or timed-out outcome must not invite a blind retry"
            );
        }
    }

    fn device(id: &str, name: Option<&str>) -> DeviceRef {
        DeviceRef {
            id: id.to_owned(),
            name: name.map(str::to_owned),
        }
    }

    #[test]
    fn a_device_name_resolves_only_within_the_scope() {
        let devices = vec![
            device("a", Some("ALPHA")),
            device("b", Some("BETA")),
            device("c", Some("GAMMA")),
        ];
        let allowed = vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            resolve_aggregate_device(&allowed, &devices, "b"),
            DeviceResolution::Resolved("b".to_owned())
        );
        assert_eq!(
            resolve_aggregate_device(&allowed, &devices, "BETA"),
            DeviceResolution::Resolved("b".to_owned())
        );
        assert_eq!(
            resolve_aggregate_device(&allowed, &devices, "GAMMA"),
            DeviceResolution::OutOfScope
        );
        assert_eq!(
            resolve_aggregate_device(&allowed, &devices, "nope"),
            DeviceResolution::OutOfScope
        );
    }

    #[test]
    fn a_shared_device_name_is_ambiguous() {
        let devices = vec![device("a", Some("SAME")), device("b", Some("SAME"))];
        assert_eq!(
            resolve_aggregate_device(&["a".into(), "b".into()], &devices, "SAME"),
            DeviceResolution::Ambiguous
        );
        // The same name outside the scope is not a match at all.
        assert_eq!(
            resolve_aggregate_device(&["a".into()], &devices, "SAME"),
            DeviceResolution::Resolved("a".to_owned())
        );
    }

    #[test]
    fn the_aggregate_strips_the_device_before_forwarding() {
        let mut request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "files.read", "arguments": {"path": "C:\\x", "device": "a"}}});
        assert_eq!(take_aggregate_device(&mut request), Ok("a".to_owned()));
        assert!(request["params"]["arguments"].get("device").is_none());
        assert_eq!(request["params"]["arguments"]["path"], json!("C:\\x"));
    }

    #[test]
    fn a_missing_or_empty_device_is_refused_before_forwarding() {
        for arguments in [json!({}), json!({"device": ""}), json!({"device": 7})] {
            let mut request =
                json!({"method": "tools/call", "params": {"name": "x", "arguments": arguments}});
            assert_eq!(take_aggregate_device(&mut request), Err(()));
        }
    }
}
