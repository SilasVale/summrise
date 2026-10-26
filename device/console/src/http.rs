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
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use ed25519_dalek::{Signature, SigningKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::audit::{self, Audit};
use crate::auth::{
    bearer_secret, constant_time_eq, new_session_token, random_32, Session, SESSION_LIFETIME_MS,
};
use crate::channels::{ChannelRegistry, Reply, RequestError};
use crate::codes::{ConsumeError, EnrollmentCodes, IssueError};
use crate::config::{load_grant_key, load_operator_token, Config, StartupError};
use crate::connections;
use crate::denials::Denials;
use crate::devices::{self, Device, DeviceRegistry, RegisterError};
use crate::grants::{self, GrantEnvelope, GrantError, VerifyError};
use crate::hex;
use crate::tokens::{TokenError, TokenMeta, Tokens};

/// How far a device's clock may be from the console's when it registers: five minutes.
const REGISTRATION_SKEW_MS: u64 = 5 * 60 * 1000;

/// How many audit lines `GET /audit` returns when the caller names no limit.
const DEFAULT_AUDIT_LIMIT: usize = 100;

/// The largest limit `GET /audit` honors: an operator reading the index must not make the console
/// hold an unbounded slice of it.
const MAX_AUDIT_LIMIT: usize = 1000;

/// How long the console tells an operator's client to wait before retrying a reconnecting device.
const RETRY_AFTER_MS: u64 = 1_000;

/// The largest body the standard MCP endpoint buffers, after the grant has been verified.
///
/// It matches the device's own `/mcp` cap: a body a client can send to one end must not be refused
/// by the other for a reason that is not the tool's.
const MAX_STANDARD_MCP_BODY_BYTES: usize = 8 * 1024 * 1024;

/// How long the console tells an operator's client to wait before retrying a busy device.
const BUSY_RETRY_AFTER_MS: u64 = 250;

/// The operator page, embedded at compile time so the binary carries its own UI.
///
/// The page is one file: its styles and its script are inline, there is no build step, and it holds
/// no secret. The operator types the token into it, and the token stays in the page's memory.
const OPERATOR_PAGE: &str = include_str!("../web/dist/index.html");

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
                sessions: Mutex::new(Vec::new()),
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
            .route("/", get(operator_page))
            // The terminal emulator, served from the binary: the page needs a real terminal for
            // interactive prompts (sudo, ssh host keys), and fetching it from a CDN would make the
            // console depend on a third party at run time. Two files, no build step.
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
            .route("/mcp/:id", post(standard_mcp))
            .route("/audit", get(list_audit))
            .layer(axum::middleware::from_fn(no_store))
            .with_state(self.inner.clone())
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
        let sessions = lock(&self.sessions);
        let mut matched = false;
        for session in sessions.iter() {
            let token_matches = constant_time_eq(&presented, &session.token);
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
        self.audit
            .append(&audit::Entry {
                ts_ms: (self.now)(),
                kind,
                device_id,
                status: status.as_u16(),
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

/// `GET /`: the operator page. It is static HTML with no secret in it; the page asks for the token
/// and keeps the session it gets back in memory.
async fn operator_page() -> Html<&'static str> {
    Html(OPERATOR_PAGE)
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
        let mut sessions = lock(&app.sessions);
        sessions.retain(|session| !constant_time_eq(&presented, &session.token));
    }
    StatusCode::NO_CONTENT
}

async fn authenticate(
    State(app): State<AppState>,
    Json(request): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    // Two ways in, one outcome: a typed token, or a one-time code the operator printed on the host
    // (`--login-link`). The code is short-lived and spent on first use, so a link that leaked into a
    // chat log stops working as soon as it is used or after two minutes.
    let accepted = if let Some(code) = request.code.as_deref() {
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
        let presented = match request.token.as_deref().and_then(hex::decode_32) {
            Some(presented) => presented,
            None => {
                return Err(app
                    .refuse_as(None, ApiError::unauthorized(), "bad_operator_token")
                    .await)
            }
        };
        constant_time_eq(&presented, &app.operator_token)
    };
    if !accepted {
        return Err(app
            .refuse_as(None, ApiError::unauthorized(), "bad_operator_token")
            .await);
    }

    let now = (app.now)();
    let token = new_session_token();
    let expires_at_ms = now.saturating_add(SESSION_LIFETIME_MS);
    {
        let mut sessions = lock(&app.sessions);
        sessions.retain(|session| session.expires_at_ms > now);
        sessions.push(Session {
            token,
            expires_at_ms,
        });
    }

    Ok(Json(AuthResponse {
        token: hex::encode(&token),
        expires_at_ms,
    }))
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

    // The audit keeps the consent the operator granted, as a fixed pair of strings: it is an
    // authorization fact, not a payload, and the flag is what a later review needs to find.
    let detail = if signed.grant.allow_destructive {
        "allow_destructive=true"
    } else {
        "allow_destructive=false"
    };
    app.record(
        "grant",
        Some(signed.grant.device_id.as_str()),
        StatusCode::OK,
        Some(detail),
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
    device_id: String,
    tools: Vec<String>,
    /// The operator's explicit consent to destructive tools under this token. Absent means `false`:
    /// consent is a deliberate `true`, never a default.
    #[serde(default)]
    allow_destructive: bool,
}

/// The create answer: the public facts, plus the one and only copy of the secret a client gets.
#[derive(Serialize)]
struct CreatedToken {
    #[serde(flatten)]
    meta: TokenMeta,
    /// The plaintext token. Shown once; the console stores only its SHA-256.
    token: String,
}

/// `GET /tokens`: the operator's permanent tokens, newest first, without hashes or secrets.
async fn list_tokens(State(app): State<AppState>, _: OperatorSession) -> Json<Vec<TokenMeta>> {
    Json(lock(&app.tokens).list())
}

/// `POST /tokens`: mint one permanent token for one device and a fixed tool set.
///
/// The answer is the only time the plaintext exists outside the operator's client: the registry
/// keeps its SHA-256, the list reports metadata only, and the audit line names the device and never
/// the token.
async fn create_token(
    State(app): State<AppState>,
    _: OperatorSession,
    Json(request): Json<CreateToken>,
) -> Result<(StatusCode, Json<CreatedToken>), ApiError> {
    let audit_id = audit_device_id(&request.device_id);
    let device_known = app.has_device(&request.device_id);
    let created = lock(&app.tokens).create(
        crate::tokens::Draft {
            name: request.name,
            device_id: request.device_id,
            tools: request.tools,
            allow_destructive: request.allow_destructive,
        },
        device_known,
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
                TokenError::TooManyTokens => ApiError::too_many("too_many_tokens"),
                TokenError::Storage => ApiError::internal(),
            };
            return Err(app.deny(audit_id.as_deref(), refusal).await);
        }
    };

    // The consent is an authorization fact, as it is on a grant: a later review must find it.
    let detail = if meta.allow_destructive {
        "allow_destructive=true"
    } else {
        "allow_destructive=false"
    };
    app.record(
        "token",
        Some(&meta.device_id),
        StatusCode::CREATED,
        Some(detail),
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
    let detail = if meta.allow_destructive {
        "allow_destructive=true"
    } else {
        "allow_destructive=false"
    };
    app.record(
        "token",
        Some(&meta.device_id),
        StatusCode::CREATED,
        Some(detail),
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
    grant: GrantEnvelope,
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

/// Mint the grant one permanent token authorizes for this request.
///
/// The scope is the token's, never the request's: the record's device, tools and destructive consent
/// are what get signed, for [`crate::tokens::MINTED_GRANT_LIFETIME_S`] seconds. The caller of this
/// route therefore cannot widen what the token may reach, and the device verifies this grant as it
/// verifies any other.
///
/// Why a presented API token did not become a grant.
enum TokenRefusal {
    /// The credential itself does not resolve: unknown, revoked, or not mintable. The caller is not
    /// authenticated, so this is a denial-log line, never the operator's audit.
    Unauthenticated(&'static str),
    /// A live token for another device. The caller *is* authenticated, so the operator's audit gets
    /// the refusal — a credential used against the wrong machine is exactly what an audit is for.
    ForeignDevice,
}

/// The grant one permanent token request is answered with, and the record that authenticated it.
struct MintedGrant {
    envelope: GrantEnvelope,
    /// The token record's id: the audit names this, never the `srm_` secret.
    token_id: String,
}

/// `Err` is the refusal kind. The wire answer is always `unauthorized`: a caller holding a wrong,
/// foreign or revoked token learns only that it was refused.
///
/// A successful signing also marks the token used, best-effort: the in-memory last use moves on
/// every exchange, the file at the store's own throttle, and a failed write does not fail this call.
fn mint_token_grant(
    app: &AppState,
    device_id: &str,
    token: &str,
) -> Result<MintedGrant, TokenRefusal> {
    let hash = crate::tokens::hash_token(token);
    let Some(record) = lock(&app.tokens).find_by_hash(&hash) else {
        return Err(TokenRefusal::Unauthenticated("unknown_api_token"));
    };
    if record.device_id != device_id {
        return Err(TokenRefusal::ForeignDevice);
    }

    let now = (app.now)();
    let signed = grants::sign(
        &app.grant_key,
        device_id,
        record.tools,
        crate::tokens::MINTED_GRANT_LIFETIME_S,
        record.allow_destructive,
        now,
    )
    .map_err(|_| TokenRefusal::Unauthenticated("api_token_unusable"))?;

    // The exchange happened, so the record says when. This is after a successful signature for
    // exactly this device: an unknown, revoked, foreign or unmintable token never gets here.
    lock(&app.tokens).mark_used(&hash, now);

    Ok(MintedGrant {
        envelope: GrantEnvelope {
            grant: signed.grant,
            signature: hex::encode(&signed.signature.to_bytes()),
        },
        token_id: record.id,
    })
}

/// Forward one MCP exchange to a device and record the authenticated outcome.
///
/// The caller has already checked the device and verified the grant. `invalid_params_status` is the
/// HTTP status a malformed `tools/call` gets: the custom route answers 400, the standard route
/// answers 200 with a JSON-RPC error, and both are the same authorization decision. `filter` is the
/// grant's tool list when the request is `tools/list` on the standard route; the custom route passes
/// `None` and keeps its unfiltered answer. `token_id` is the permanent token's record id when one
/// authenticated this exchange, and is written on every line this function records.
async fn forward(
    app: &AppState,
    device_id: &str,
    grant: &GrantEnvelope,
    token_id: Option<&str>,
    request: Value,
    invalid_params_status: StatusCode,
    filter: Option<&[String]>,
) -> Response {
    let audit_id = audit_device_id(device_id);
    let request_id = request.get("id").cloned().unwrap_or(Value::Null);
    let tool = match requested_tool(&request) {
        Ok(tool) => tool.map(str::to_owned),
        Err(()) => {
            app.record_tool(
                "denied",
                audit_id.as_deref(),
                invalid_params_status,
                Some("invalid_tool_call"),
                None,
                token_id,
            )
            .await;
            return jsonrpc_error(
                invalid_params_status,
                request_id,
                -32602,
                "invalid params: tools/call requires params.name",
                None,
            );
        }
    };
    let is_tools_list = request.get("method").and_then(Value::as_str) == Some("tools/list");

    if let Some(tool) = tool.as_deref() {
        if !grant.grant.tools.iter().any(|allowed| allowed == tool) {
            app.record_tool(
                "denied",
                audit_id.as_deref(),
                StatusCode::FORBIDDEN,
                Some("tool_not_permitted"),
                Some(tool),
                token_id,
            )
            .await;
            return jsonrpc_error(
                StatusCode::FORBIDDEN,
                request_id,
                -32001,
                "tool not permitted by grant",
                Some(json!({ "tool": tool })),
            );
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
                app.channels.request_within(device_id, request, grant, wait),
            )
            .await
        }
        // Everything else answers in milliseconds; the general bound stays in charge of both layers.
        None => {
            tokio::time::timeout(
                app.mcp_timeout,
                app.channels.request(device_id, request, grant),
            )
            .await
        }
    };

    match answer {
        Err(_) => {
            app.record_tool(
                "denied",
                audit_id.as_deref(),
                StatusCode::GATEWAY_TIMEOUT,
                Some("device timeout"),
                tool.as_deref(),
                token_id,
            )
            .await;
            ApiError::gateway_timeout("device timeout").into_response()
        }
        Ok(Err(error)) => {
            let failure = failure_for(error);
            app.record_tool(
                "denied",
                audit_id.as_deref(),
                failure.status,
                Some(failure.detail),
                tool.as_deref(),
                token_id,
            )
            .await;
            failure.into_response()
        }
        Ok(Ok(mut reply)) => {
            if let (Some(allowed), true) = (filter, is_tools_list) {
                if let Reply::Result(result) = &mut reply {
                    crate::mcp::filter_tools(result, allowed);
                }
            }
            let detail = reply_detail(&reply);
            app.record_tool(
                "mcp",
                audit_id.as_deref(),
                StatusCode::OK,
                detail.as_deref(),
                tool.as_deref(),
                token_id,
            )
            .await;
            match reply {
                Reply::Result(result) => (StatusCode::OK, Json(result)).into_response(),
                Reply::Error(error) => (StatusCode::OK, Json(error)).into_response(),
            }
        }
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
    if let Err(error) = verify_presented(&app, &body.device_id, &body.grant) {
        return app.deny(audit_id.as_deref(), error).await.into_response();
    }

    forward(
        &app,
        &body.device_id,
        &body.grant,
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
/// permanent token, found by hash, checked against this path's device and exchanged for a
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
    // and a permanent token mints an envelope with the token's scope. A token failure is a
    // pre-authentication refusal, so it goes to the denial log, never to the audit. Only a token
    // gives the forwarded lines a token id; a grant has no permanent record to name.
    let (grant, token_id) = match bearer {
        crate::mcp::BearerKind::Grant(grant) => {
            if let Err(error) = verify_presented(&app, &device_id, &grant) {
                return app.refuse(audit_id.as_deref(), error).await.into_response();
            }
            (grant, None)
        }
        crate::mcp::BearerKind::Token(token) => match mint_token_grant(&app, &device_id, &token) {
            Ok(minted) => (minted.envelope, Some(minted.token_id)),
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
                &grant,
                token_id.as_deref(),
                request,
                StatusCode::OK,
                Some(&grant.grant.tools),
            )
            .await
        }
        "tools/call" => {
            forward(
                &app,
                &device_id,
                &grant,
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

async fn list_devices(State(app): State<AppState>, _: OperatorSession) -> Json<Vec<DeviceView>> {
    let devices = lock(&app.devices).list();
    let devices = devices
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
        .collect();
    Json(devices)
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

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<String>,
}

/// `GET /audit`: the operator's own actions, newest first, one JSON line each.
///
/// `limit` is a string here so a malformed value is this service's JSON refusal rather than axum's
/// plain-text query rejection. Missing means [`DEFAULT_AUDIT_LIMIT`]; a value above
/// [`MAX_AUDIT_LIMIT`] is clamped, because the index is not an unbounded read.
async fn list_audit(
    State(app): State<AppState>,
    _: OperatorSession,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Vec<Value>>, ApiError> {
    let limit = match query.limit {
        None => DEFAULT_AUDIT_LIMIT,
        Some(text) => text
            .parse::<usize>()
            .map_err(|_| ApiError::bad_request("invalid_limit"))?,
    };

    Ok(Json(app.audit.newest(limit.min(MAX_AUDIT_LIMIT)).await))
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
}
