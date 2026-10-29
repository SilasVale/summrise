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

/// How long a held destructive call waits for the operator, and it is deliberately shorter than it
/// looks: the console is served through Cloudflare Access, whose origin bound cuts a request at
/// about 100 s. A ten-minute hold would have the edge drop the client while the console still
/// believed it was waiting, so the answer would be pressed onto a request nobody was holding. Ninety
/// seconds fits inside that bound, and expiry fails CLOSED: an unanswered question is refused, never
/// run.
const APPROVAL_TTL: Duration = Duration::from_secs(90);

/// One call held for approval: what the operator is asked about, and the wire back to the request
/// that is waiting for the answer.
struct Pending {
    row: page::ApprovalRow,
    decide: tokio::sync::oneshot::Sender<bool>,
}

/// The held call's registration, removed when the holding future drops.
///
/// A client that hangs up drops the proxy's future, and with it the waiter the timeout lived in. An
/// entry left behind would sit on the dashboard forever and, worse, answer a call nobody is waiting
/// for. The guard is what makes the map's size follow the requests that exist.
///
/// **A HANG-UP STILL HAS TO CLOSE THE RECORD.** A client that goes away never reaches an `approval`
/// line — the handler is cancelled where it stands — so the audit showed a question that was asked
/// and then nothing, which reads exactly like a console that lost the answer. When the guard is
/// dropped without a decision it writes the missing line (`client left`) from a task of its own,
/// because `Drop` cannot await.
struct Held<'a> {
    app: &'a AppState,
    id: String,
    /// The facts the closing line needs: the handler's locals are gone by the time `Drop` runs.
    device_id: Option<String>,
    tool: Option<String>,
    token_id: Option<String>,
    /// Set by every path that ends the ask on purpose: approved, refused, timed out.
    resolved: bool,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        lock(&self.app.approvals).remove(&self.id);
        if self.resolved {
            return;
        }
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let app = self.app.clone();
        let device_id = self.device_id.clone();
        let tool = self.tool.clone();
        let token_id = self.token_id.clone();
        tokio::spawn(async move {
            app.record_facts(AuditFacts {
                kind: "approval",
                device_id: device_id.as_deref(),
                status: StatusCode::OK,
                detail: Some("client left"),
                tool: tool.as_deref(),
                token_id: token_id.as_deref(),
            })
            .await;
        });
    }
}

/// How many sessions the store keeps.
///
/// One operator, a handful of browsers. The page also mints a session for an Access-verified request
/// that carried no cookie, so without a cap a browser that refuses cookies would append one per page
/// load and every request would scan the growing list. The oldest is dropped, which is the one a
/// person is least likely to be using.
const MAX_SESSIONS: usize = 32;

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
    /// The last expression evaluated in a device's browser, and its answer. Console memory only: it
    /// is a form result the next page load shows, not device state and not a file.
    last_evals: Mutex<HashMap<String, page::LastEval>>,
    /// Calls a device refused because the credential did not consent to a destructive tool, held
    /// while the operator answers. Console memory only; a restart refuses them all at once.
    approvals: Mutex<HashMap<String, Pending>>,
    /// Vitals points collected by opening the vitals page: one per visit, capped, memory only.
    vitals: Mutex<HashMap<String, Vec<page::VitalPoint>>>,
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
                last_evals: Mutex::new(HashMap::new()),
                approvals: Mutex::new(HashMap::new()),
                vitals: Mutex::new(HashMap::new()),
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
                "/assets/browser.js",
                get(|| async {
                    assets::Asset {
                        body: assets::BROWSER,
                        content_type: "text/javascript; charset=utf-8",
                    }
                }),
            )
            .route(
                "/assets/approvals.js",
                get(|| async {
                    assets::Asset {
                        body: assets::APPROVALS,
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
            .route("/next/approvals/count", get(next_approvals_count))
            .route("/next/approvals/:id/:decision", post(next_approval_decide))
            .route("/next/fleet/:id/name", post(next_rename))
            .route("/next/fleet/:id/remove", post(next_remove))
            .route("/next/enroll", post(next_enroll))
            .route("/next/audit", get(next_audit))
            .route("/next/denials", get(next_denials))
            // The terminal page: one device, one shell, the session's own cookie as the gate.
            .route("/next/terminal/:device", get(next_terminal_page))
            .route("/next/terminal/:device/create", post(next_terminal_create))
            .route("/next/terminal/:device/:id/end", post(next_terminal_end))
            .route(
                "/next/terminal/:device/connections",
                post(next_terminal_preset_save),
            )
            .route(
                "/next/terminal/:device/connections/:id/remove",
                post(next_terminal_preset_remove),
            )
            .route(
                "/next/terminal/:device/connections/:id/connect",
                post(next_terminal_preset_connect),
            )
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
            .route("/next/files/:device/mkdir", post(next_files_mkdir))
            // The runs page: one command, its output, and the device's own history.
            .route("/next/runs/:device", get(next_runs_page))
            .route("/next/runs/:device/exec", post(next_runs_exec))
            // The device's own state, and the system page with its process list.
            .route("/next/state/:device", get(next_state_page))
            .route("/next/system/:device", get(next_system_page))
            .route("/next/system/:device/kill", post(next_system_kill))
            .route("/next/system/:device/restart", post(next_system_restart))
            .route("/next/system/:device/net", post(next_system_net_test))
            // The device's own memory: the entries, an append form, a delete per row.
            .route("/next/memory/:device", get(next_memory_page))
            .route("/next/memory/:device/append", post(next_memory_append))
            .route("/next/memory/:device/delete", post(next_memory_delete))
            .route("/next/memory/:device/update", post(next_memory_update))
            .route("/next/memory/:device/export", get(next_memory_export))
            // The device's screen, and the watches it runs.
            .route("/next/screen/:device", get(next_screen_page))
            .route("/next/monitors/:device", get(next_monitors_page))
            .route("/next/monitors/:device/add", post(next_monitor_add))
            .route("/next/monitors/:device/probe", post(next_monitor_probe))
            .route("/next/monitors/:device/remove", post(next_monitor_remove))
            // The device's browser: its targets, a capture, and open/close.
            .route("/next/browser/:device", get(next_browser_page))
            .route("/next/browser/:device/frame", get(next_browser_frame))
            .route("/next/browser/:device/open", post(next_browser_open))
            .route("/next/browser/:device/eval", post(next_browser_eval))
            .route(
                "/next/browser/:device/navigate",
                post(next_browser_navigate),
            )
            .route(
                "/next/browser/:device/shutdown",
                post(next_browser_shutdown),
            )
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
            .fallback(page_not_found)
            .layer(axum::middleware::from_fn(no_store))
            .layer(axum::middleware::from_fn_with_state(
                self.inner.clone(),
                access_sign_in,
            ))
            .with_state(self.inner.clone())
    }
}

/// A page route whose device is gone: the same card the fallback serves, so a stale link is never a
/// blank page.
fn no_such_device(app: &AppState) -> Response {
    (
        StatusCode::NOT_FOUND,
        Html(page::not_found(&app.version).into_string()),
    )
        .into_response()
}

/// Any path the router does not own: a 404 in the product's own card, with a way back.
async fn page_not_found(State(app): State<AppState>) -> Response {
    (
        StatusCode::NOT_FOUND,
        Html(page::not_found(&app.version).into_string()),
    )
        .into_response()
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
    /// The devices whose channel is open right now. `devices` is how many the console knows; the
    /// difference is the answer to "is anything connected", which a count of known devices cannot
    /// give — on 2026-10-29 this field said 3 while the one device an agent talked to had been
    /// reconnecting for twenty minutes.
    online: usize,
    uptime_s: u64,
    /// Whether Cloudflare Access is configured as an operator identity. The first thing to check when
    /// the page asks for a token: with this false, it always will.
    access_configured: bool,
    /// The deepest any per-device outbound queue has been since the console started. It is a bound
    /// an operator can see, not a secret.
    max_queued_frames: usize,
    /// The label the deploy passed, or the crate version for a development start.
    version: String,
}

/// The static assets behind `/assets/`: two vendored, two ours.
mod assets {
    use axum::http::header;
    use axum::response::{IntoResponse, Response};

    pub(super) const JS: &str = include_str!("../assets/xterm.js");
    pub(super) const CSS: &str = include_str!("../assets/xterm.css");
    pub(super) const FIT: &str = include_str!("../assets/addon-fit.js");
    /// The terminal page's glue: the loop between xterm and `/next/terminal`.
    pub(super) const TERMINAL: &str = include_str!("../assets/terminal.js");
    /// The approvals badge: the one poll every page carries, because a held call waits ninety seconds.
    pub(super) const APPROVALS: &str = include_str!("../assets/approvals.js");
    /// The browser pane's frame pump: replaces the server-rendered capture as frames arrive.
    pub(super) const BROWSER: &str = include_str!("../assets/browser.js");
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
async fn root_redirect(uri: axum::http::Uri) -> Redirect {
    // The root is what `--login-link https://console.saisi.online` prints, and its code rides in the
    // query: a redirect that drops the query turns a working link into a card with no explanation.
    match uri.query() {
        Some(query) => Redirect::to(&format!("/next?{query}")),
        None => Redirect::to("/next"),
    }
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
    // **THE BROWSER MITIGATES NOTHING IT WAS NOT TOLD.** `X-Frame-Options` was the only one of these
    // the console sent (measured 2026-10-29: `x-frame-options`, `cache-control`, `content-type`).
    // A browser that sniffs is a browser that can run what the console meant as a download; a
    // console page can carry a device id in its path, and a referrer leaks it to whatever the page
    // links to. The CSP line constrains nothing the console does — no frames, no plugins, no base
    // rewrite — so the inline terminal bootstrap keeps working.
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        axum::http::HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        axum::http::HeaderValue::from_static("no-referrer"),
    );
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        axum::http::HeaderValue::from_static(
            "frame-ancestors 'none'; object-src 'none'; base-uri 'self'",
        ),
    );
    response
}

/// Sign the operator in when Cloudflare Access already identified them.
///
/// The page's gate is a cookie and the edge's identity is a header, so without this an operator
/// Access had just authenticated was handed a 64-hex token box for a secret nobody gave them — it
/// lives in a file on the host. A verified header mints the page's session: this request carries it
/// (the handlers read the same header a cookie would have supplied), and the answer sets the cookie
/// for the next one. Injecting it rather than redirecting keeps a browser that refuses cookies from
/// looping between the cookie and the card.
///
/// GET and `/next…` only: a cross-site form POST that the edge happens to authenticate must not be
/// able to mint a session, and the API's bearer tokens are not cookies.
async fn access_sign_in(
    State(app): State<AppState>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let path = request.uri().path();
    let is_page = request.method() == axum::http::Method::GET
        && (path == "/next" || path.starts_with("/next/"));
    if !is_page || cookie_session(&app, request.headers()).is_some() {
        return next.run(request).await;
    }
    let (operator, access) = app.identity(request.headers());
    if operator.map(|operator| operator.via) != Some("access") {
        // A header that was there and did not verify is the case a person cannot see from the
        // page: the card says "did not accept", and this line is what proves the edge delivered
        // anything at all. A header that is absent is not an event — every signed-out visit to a
        // console without Access would write one.
        if access.configured && access.header && access.error.is_some() {
            app.record(
                "auth",
                None,
                StatusCode::UNAUTHORIZED,
                Some("access refused"),
            )
            .await;
        }
        return next.run(request).await;
    }

    let (_session, token) = new_session(&app, "access").await;
    let cookie = format!("{SESSION_COOKIE}={}", hex::encode(&token));
    let combined = match request
        .headers()
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
    {
        Some(existing) => format!("{cookie}; {existing}"),
        None => cookie,
    };
    request.headers_mut().insert(
        header::COOKIE,
        HeaderValue::from_str(&combined).expect("a session cookie is a valid header value"),
    );
    let secure = secure_request(request.headers());
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .append(header::SET_COOKIE, session_cookie(&token, secure));
    response
}

/// `GET /health`: unauthenticated, and deliberately so — a supervisor and a human need one route
/// that answers before any credential has been presented. It reads only counts, never a secret.
async fn health(State(app): State<AppState>) -> Json<Health> {
    let devices = lock(&app.devices);
    let online = devices
        .list()
        .iter()
        .filter(|device| app.channels.online(&device.id))
        .count();
    Json(Health {
        status: "ok",
        devices: devices.len(),
        online,
        uptime_s: app.started.elapsed().as_secs(),
        access_configured: app.access.is_some(),
        max_queued_frames: app.channels.peak_queued_frames(),
        version: app.version.clone(),
    })
}

/// `POST /auth`: exchange the operator token for a session token.
/// `GET /session`: whether this request already carries an operator identity — an Access
/// assertion, a bearer session, or the page's own cookie.
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
    // A page session travels in the cookie, and the probe can be asked with one: `identity` reads
    // the bearer, so the cookie is the fallback that makes `via: "session"` mean what it says.
    let operator = operator.or_else(|| {
        cookie_session(&app, &headers).map(|_| Operator {
            email: None,
            via: "session",
        })
    });
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

/// Remove the session whose token hash is `token_hash`, persisting the removal before adopting it.
///
/// A session that cannot be written out must stay live: adopting the removal in memory and only
/// logging a failed save leaves the signed-out token in `sessions.json`, where the next start
/// re-admits it — a cookie accepted after logout.
fn end_session(app: &AppState, token_hash: &[u8; 32]) -> Result<(), String> {
    let next: Vec<crate::auth::Session> = {
        let sessions = lock(&app.sessions);
        sessions
            .iter()
            .filter(|session| !constant_time_eq(&session.token_hash, token_hash))
            .cloned()
            .collect()
    };
    crate::auth::save_sessions(&app.data, &next).map_err(|error| error.to_string())?;
    *lock(&app.sessions) = next;
    Ok(())
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
        if let Err(error) = end_session(&app, &token_hash) {
            eprintln!("summrise-console: the sessions could not be persisted: {error}");
            return StatusCode::INTERNAL_SERVER_ERROR;
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
/// Mint a session, persist it, and write the audit line that says how it was bought.
///
/// `via` is one of `access`, `token`, `login-link`. The line is written here rather than at each
/// caller so no future way in can forget it: the audit is where "did the edge deliver the header?"
/// gets answered, and an answer with holes is not an answer.
async fn new_session(app: &AppState, via: &'static str) -> (Session, [u8; 32]) {
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
        if sessions.len() > MAX_SESSIONS {
            sessions.sort_by_key(|session| session.expires_at_ms);
            let excess = sessions.len() - MAX_SESSIONS;
            sessions.drain(..excess);
        }
    }
    if let Err(error) = crate::auth::save_sessions(&app.data, &lock(&app.sessions)) {
        eprintln!("summrise-console: the sessions could not be persisted: {error}");
    }
    app.record("auth", None, StatusCode::OK, Some(via)).await;
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
    let (session, token) = new_session(&app, "token").await;
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

/// The clear mirrors [`session_cookie`]'s attributes, `Secure` included: a clear that drops them is
/// a different cookie in every browser that checks.
fn clear_session_cookie(secure: bool) -> HeaderValue {
    let mut cookie = format!("{SESSION_COOKIE}=; Path=/next; HttpOnly; SameSite=Lax; Max-Age=0");
    if secure {
        cookie.push_str("; Secure");
    }
    HeaderValue::from_str(&cookie).expect("a valid Set-Cookie")
}

#[derive(Deserialize)]
struct NextLogin {
    /// The operator token, typed. Absent when the box carried a one-time code instead.
    token: Option<String>,
    /// A one-time code pasted into the box; `--login-link` says its code is for this form.
    code: Option<String>,
}

#[derive(Deserialize)]
struct NextLogout {
    /// Absent is a refusal, not a deserialization error: the page promises 403 for a form that does
    /// not carry its session's CSRF value, and that includes a form with no field at all.
    csrf: Option<String>,
}

/// The audit page's filters, all optional: a GET form, so a narrowed view is a URL.
#[derive(Deserialize)]
struct AuditQuery {
    device: Option<String>,
    kind: Option<String>,
    status: Option<String>,
    tool: Option<String>,
}

/// How far back the audit filter looks before it applies itself: deeper than the page shows, shallow
/// enough that one request does not read the whole file.
const AUDIT_FILTER_WINDOW: usize = 500;

/// `GET /next`: the login form without a live cookie, the page with one.
#[derive(Deserialize, Default)]
struct NextQuery {
    /// The one-time code a `--login-link` put in the URL; spent, never displayed.
    code: Option<String>,
    /// The enrollment code `POST /next/enroll` hands back once, to display.
    enroll: Option<String>,
    error: Option<String>,
    /// What the last decision did ("Approved — …"), so the page says it instead of the row
    /// silently vanishing.
    notice: Option<String>,
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
        // A `--login-link` code arrives in the URL, not a form: spending it here is what makes the
        // printed link work on the page it was minted for. The token box stays underneath it.
        if let Some(code) = query.code.as_deref() {
            return login_with_code(&app, &headers, code).await;
        }
        let (_, access) = app.identity(&headers);
        let note = access_note(&access, &headers);
        return Html(page::login(note.as_deref(), None).into_string()).into_response();
    };
    // Access is the usual signer, and the page says so: the operator was earlier shown a token box
    // for a secret that lives in a file on the host, and the dashboard is where that gets answered.
    let (operator, _) = app.identity(&headers);
    let signer = operator.and_then(|operator| operator.email);
    // The address the operator reached this console on — the one a new device must be able to reach.
    let origin = headers
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .map(|host| {
            let scheme = headers
                .get("x-forwarded-proto")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_else(|| {
                    if host.starts_with("127.0.0.1") || host.starts_with("localhost") {
                        "http"
                    } else {
                        "https"
                    }
                });
            format!("{scheme}://{host}")
        });
    let notes = page::Notes {
        signer: signer.as_deref(),
        enroll: query.enroll.as_deref(),
        error: query.error.as_deref(),
        notice: query.notice.as_deref(),
        origin: origin.as_deref(),
    };
    let approvals = live_approvals(&app);
    Html(
        // The fleet is the page an operator lands on; six lines of what the console just did
        // belong there, read from the same log `/audit` serves.
        page::dashboard(
            &device_views(&app),
            &hex::encode(&session.csrf),
            &app.version,
            notes,
            &app.audit.newest(6).await,
            &approvals,
            (app.now)(),
        )
        .into_string(),
    )
    .into_response()
}

#[derive(Deserialize)]
struct ApprovalDecision {
    csrf: String,
}

/// Refuse every held call for one device: the operator is not answering them, the device is gone.
fn refusals_for(app: &AppState, device_id: &str) {
    let gone: Vec<Pending> = {
        let mut approvals = lock(&app.approvals);
        let ids: Vec<String> = approvals
            .values()
            .filter(|pending| pending.row.device_id == device_id)
            .map(|pending| pending.row.id.clone())
            .collect();
        ids.iter().filter_map(|id| approvals.remove(id)).collect()
    };
    for pending in gone {
        // The waiting gate turns this into the device's own refusal.
        let _ = pending.decide.send(false);
    }
}

/// The held calls that are still worth showing: an entry past its deadline is not one, whatever
/// happened to the future that registered it.
fn live_approvals(app: &AppState) -> Vec<page::ApprovalRow> {
    let now = (app.now)();
    let mut approvals = lock(&app.approvals);
    approvals.retain(|_, pending| {
        pending
            .row
            .created_ms
            .saturating_add(APPROVAL_TTL.as_millis() as u64)
            > now
    });
    let mut rows: Vec<page::ApprovalRow> = approvals
        .values()
        .map(|pending| pending.row.clone())
        .collect();
    rows.sort_by_key(|row| row.created_ms);
    rows
}

/// `GET /next/approvals/count`: how many calls are waiting, for the badge every page carries.
async fn next_approvals_count(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if cookie_session(&app, &headers).is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(json!({ "pending": live_approvals(&app).len() })).into_response()
}

/// `POST /next/approvals/:id/approve` and `/refuse`: answer one held call.
async fn next_approval_decide(
    State(app): State<AppState>,
    Path((id, decision)): Path<(String, String)>,
    headers: HeaderMap,
    Form(form): Form<ApprovalDecision>,
) -> Response {
    if next_session(&app, &headers, Some(&form.csrf)).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    let approve = match decision.as_str() {
        "approve" => true,
        "refuse" => false,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    match lock(&app.approvals).remove(&id) {
        Some(pending) => {
            // The send's result is the only evidence the decision reached a live waiter. A click
            // that lands after the hold resolved (or on a row whose client has gone) would
            // otherwise be reported as done while nothing happened.
            match pending.decide.send(approve) {
                // **THE DECISION IS ANNOUNCED.** A bare `/next` reload makes the row vanish with no
                // sentence; a screen reader heard only "Fleet" again, and the operator had to infer
                // that the approval had been recorded.
                Ok(()) => Redirect::to(if approve {
                    "/next?notice=Approved%20%E2%80%94%20the%20client%27s%20call%20may%20now%20run."
                } else {
                    "/next?notice=Refused%20%E2%80%94%20the%20call%20will%20not%20run."
                })
                .into_response(),
                Err(_) => Redirect::to(&format!(
                    "/next?error={}",
                    page::encode("That call is no longer waiting: it was answered, it expired, or the client stopped waiting.")
                ))
                .into_response(),
            }
        }
        None => Redirect::to(&format!(
            "/next?error={}",
            page::encode("That call is no longer waiting: it was answered, it expired, or the client stopped waiting.")
        ))
        .into_response(),
    }
}

/// Spend a `--login-link` code and answer with the session it buys.
///
/// The redirect is the point: the code must leave the address bar with the request that spent it,
/// and a 303 makes the next request a plain `GET /next` carrying the cookie.
async fn login_with_code(app: &AppState, headers: &HeaderMap, code: &str) -> Response {
    if let Err(reason) = crate::store::spend_login_code(&app.data, code, (app.now)()) {
        eprintln!("summrise-console: refused a login code: {reason}");
        let (_, access) = app.identity(headers);
        let note = access_note(&access, headers);
        return Html(
            page::login(note.as_deref(), Some("That sign-in link was not accepted.")).into_string(),
        )
        .into_response();
    }
    let (_session, token) = new_session(app, "login-link").await;
    let mut response = Redirect::to("/next").into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(&token, secure_request(headers)),
    );
    response
}

/// The one sentence the card can say about the edge when Access is configured: whether this request
/// carried no identity, or one the verifier refused.
///
/// The page's sign-in now depends on that header arriving, and a card that does not say why it
/// appeared cost an operator an hour of looking for a verification bug that did not exist
/// (2026-10-24, recorded in `docs/superpowers/specs/2026-10-23-the-console-gets-a-public-name.md`).
/// The card as the page knows how to explain it: the edge's answer when Access is configured and
/// the request came through it. Every signed-out page uses this, so a bookmarked deep link explains
/// itself as well as the landing page does.
fn login_card(app: &AppState, headers: &HeaderMap) -> Html<String> {
    let (_, access) = app.identity(headers);
    let note = access_note(&access, headers);
    Html(page::login(note.as_deref(), None).into_string())
}

fn access_note(access: &AccessReport, headers: &HeaderMap) -> Option<String> {
    if !access.configured {
        return None;
    }
    match (access.header, access.error.as_deref()) {
        // `cf-ray` is Cloudflare's own stamp on a request it proxied: without it the request did not
        // come through the edge at all (the tailnet address, a smoke run), and sending that operator
        // to sign in at Cloudflare would point at a door that is not on this path.
        (false, _) if headers.contains_key("cf-ray") => Some(
            "Cloudflare Access did not identify this request. If you expected to be signed in, \
             sign in at the edge first, then reload this page."
                .to_owned(),
        ),
        (true, Some(error)) => Some(format!(
            "Cloudflare Access did not accept this request: {error}"
        )),
        // A verified header with no session cannot reach the card: the page's middleware signs it in.
        _ => None,
    }
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
    if removed == Ok(true) {
        // A call held for a machine that no longer exists must not wait out its clock, and
        // approving it could only re-send into a device that is gone.
        refusals_for(&app, &device_id);
    }
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
            Redirect::to(&format!("/next?enroll={code}")).into_response()
        }
        Err(IssueError::TooMany) => Redirect::to("/next?error=too_many_codes").into_response(),
        Err(IssueError::Storage) => Redirect::to("/next?error=code_storage").into_response(),
    }
}

/// `GET /next/audit`: the audit log, read-only, behind the page's cookie.
async fn next_audit(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return login_card(&app, &headers).into_response();
    };
    // The filter is applied to a deeper read than the page shows, so narrowing never hides a line
    // that is in the log: the page says "N of M" and M is what the console actually holds.
    let total = app.audit.newest(AUDIT_FILTER_WINDOW).await;
    let filter = page::AuditFilter {
        device: query.device.unwrap_or_default(),
        kind: query.kind.unwrap_or_default(),
        status: query.status.unwrap_or_default(),
        tool: query.tool.unwrap_or_default(),
    };
    let rows: Vec<Value> = total
        .iter()
        .filter(|row| {
            let device_ok = filter.device.is_empty()
                || row["device_id"].as_str() == Some(filter.device.as_str());
            let kind_ok =
                filter.kind.is_empty() || row["kind"].as_str() == Some(filter.kind.as_str());
            let status_ok = match row["status"].as_u64() {
                Some(status) if !filter.status.is_empty() => {
                    status.to_string().starts_with(&filter.status)
                }
                _ => filter.status.is_empty(),
            };
            let needle = filter.tool.to_ascii_lowercase();
            let tool_ok = needle.is_empty()
                || row["tool"]
                    .as_str()
                    .is_some_and(|tool| tool.to_ascii_lowercase().contains(&needle))
                || row["detail"]
                    .as_str()
                    .is_some_and(|detail| detail.to_ascii_lowercase().contains(&needle));
            device_ok && kind_ok && status_ok && tool_ok
        })
        .take(DEFAULT_LOG_LIMIT)
        .cloned()
        .collect();
    let devices: Vec<(String, String)> = device_views(&app)
        .into_iter()
        .map(|device| {
            let label = device
                .name
                .clone()
                .unwrap_or_else(|| device.id.chars().take(8).collect());
            (device.id, label)
        })
        .collect();
    let bar = page::audit_filter(
        "/next/audit",
        &devices,
        &filter,
        true,
        page::AUDIT_KINDS.as_slice(),
    );
    Html(
        page::logs(page::Logs {
            rows: &rows,
            title: "Actions",
            total: total.len(),
            filter: Some(bar),
            csrf: &hex::encode(&session.csrf),
            version: &app.version,
            lead: "The console's own record, newest first: the call, its outcome, and a note when \
                   one exists. Times are UTC; a device id is shortened here but named in full by \
                   the line's tooltip. Arguments are not recorded — neither here nor in the \
                   device's own audit.",
            caption: "The console's own audit lines.",
            filtered: filter.is_active(),
            segment: page::LogSegment::Actions,
        })
        .into_string(),
    )
    .into_response()
}

/// `GET /next/denials`: the unauthenticated-refusal log, read-only, behind the page's cookie.
async fn next_denials(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return login_card(&app, &headers).into_response();
    };
    let total = app.denials.newest(AUDIT_FILTER_WINDOW).await;
    let filter = page::AuditFilter {
        device: String::new(),
        kind: query.kind.unwrap_or_default(),
        status: query.status.unwrap_or_default(),
        tool: query.tool.unwrap_or_default(),
    };
    let rows: Vec<Value> = total
        .iter()
        .filter(|row| {
            let kind_ok =
                filter.kind.is_empty() || row["kind"].as_str() == Some(filter.kind.as_str());
            let status_ok = match row["status"].as_u64() {
                Some(status) if !filter.status.is_empty() => {
                    status.to_string().starts_with(&filter.status)
                }
                _ => filter.status.is_empty(),
            };
            let needle = filter.tool.to_ascii_lowercase();
            let detail_ok = needle.is_empty()
                || row["detail"]
                    .as_str()
                    .is_some_and(|detail| detail.to_ascii_lowercase().contains(&needle));
            kind_ok && status_ok && detail_ok
        })
        .take(DEFAULT_LOG_LIMIT)
        .cloned()
        .collect();
    // Denials is not the audit: it is the refusals the console made BEFORE authentication, from
    // its own store, and every row in it has kind `denied`. So it gets its own lead and no kind
    // filter (the audit's twelve-option list offered eleven that could never match).
    let bar = page::audit_filter("/next/denials", &[], &filter, false, &[]);
    Html(
        page::logs(page::Logs {
            rows: &rows,
            title: "Refusals",
            total: total.len(),
            filter: Some(bar),
            csrf: &hex::encode(&session.csrf),
            version: &app.version,
            lead: "Every request the console refused before authentication: a missing session, an \
                   unknown token, an unauthenticated probe. The first hundred in a minute are \
                   listed; the rest are counted. A refusal is not a device call, and the arguments \
                   it carried are not stored.",
            caption: "Refusals recorded before authentication.",
            filtered: filter.is_active(),
            segment: page::LogSegment::Refusals,
        })
        .into_string(),
    )
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
    /// `kind: "ssh"`, instead of a password: an absolute path to an OpenSSH key on the device.
    #[serde(default)]
    key_path: String,
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

/// Turn the one-line target the presets form takes into the draft the connection registry validates.
///
/// `pty` needs no target, `ssh` is `user@host[:port]` (port 22 when absent), `serial` is
/// `path[@baud]`. The registry still owns the shape rules; this only splits the line the operator
/// typed and says which part is missing, so a bad preset is refused at save time with a sentence.
fn draft_from_target(
    name: &str,
    kind: &str,
    target: &str,
) -> Result<connections::Draft, &'static str> {
    let name = name.trim().to_owned();
    let target = target.trim();
    let draft = match kind.trim() {
        "pty" => connections::Draft {
            name,
            kind: "pty".to_owned(),
            host: None,
            port: None,
            user: None,
            port_path: None,
            baud: None,
            key_path: None,
        },
        "ssh" => {
            let (user, rest) = target
                .split_once('@')
                .ok_or("an ssh target is user@host or user@host:port")?;
            let (host, port) = match rest.rsplit_once(':') {
                Some((host, port)) => (
                    host,
                    Some(
                        port.parse::<u16>()
                            .map_err(|_| "the ssh port must be a number")?,
                    ),
                ),
                None => (rest, None),
            };
            connections::Draft {
                name,
                kind: "ssh".to_owned(),
                host: Some(host.trim().to_owned()),
                port: Some(port.unwrap_or(22)),
                user: Some(user.trim().to_owned()),
                port_path: None,
                baud: None,
                key_path: None,
            }
        }
        "serial" => {
            let (path, baud) = match target.split_once('@') {
                Some((path, baud)) => (
                    path,
                    Some(
                        baud.parse::<u32>()
                            .map_err(|_| "the baud must be a number")?,
                    ),
                ),
                None => (target, None),
            };
            connections::Draft {
                name,
                kind: "serial".to_owned(),
                host: None,
                port: None,
                user: None,
                port_path: Some(path.trim().to_owned()),
                baud,
                key_path: None,
            }
        }
        _ => return Err("kind must be pty, ssh or serial"),
    };
    Ok(draft)
}

fn terminal_refusal(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// The page a browser gets when a device call behind a form or a link fails: the sentence the device
/// sent, the console's chrome, and the way back. The XHR endpoints (the terminal's five calls) keep
/// their JSON on purpose — nothing renders a page from them.
async fn refused_page(
    app: &AppState,
    device: &DeviceView,
    session: &crate::auth::Session,
    response: Response,
    active: page::Page,
    back: String,
    back_label: &str,
) -> Response {
    // The status the refusal carried (a device failure is a 502) is kept: a page that reads well
    // and answers 200 tells the log a different story than the device did.
    let status = response.status();
    let sentence = refusal_sentence(response).await;
    (
        status,
        Html(
            page::refusal(
                device,
                &sentence,
                &back,
                back_label,
                active,
                &hex::encode(&session.csrf),
                &app.version,
            )
            .into_string(),
        ),
    )
        .into_response()
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
/// The terminal page's query: a preset the operator asked to prefill, and the sentence a refused
/// rail action (a preset or an end) left behind.
#[derive(Deserialize)]
struct TerminalPresetQuery {
    preset: Option<String>,
    error: Option<String>,
}

async fn next_terminal_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<TerminalPresetQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return login_card(&app, &headers).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // The rail's sessions are a snapshot: the page is server-rendered, and a reload is the refresh.
    // **AN UNREACHABLE DEVICE IS NOT AN IDLE ONE.** The rail used to say "No live session on this
    // device" when the call had failed, which is how a machine that is down reads as a machine with
    // nothing to do. The failure travels into the renderer as an error and suppresses that sentence.
    let (sessions, load_error): (Vec<page::TerminalSession>, Option<&str>) =
        match device_tool(&app, &device_id, "terminal.list", json!({})).await {
            Ok(text) => (
                serde_json::from_str::<Value>(&text)
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
                None,
            ),
            Err(_) => (Vec::new(), Some("The device did not answer.")),
        };
    // WHAT THE CONSOLE DID ON THIS DEVICE LATELY, from the audit log. Sessions do not survive a device
    // restart (today's installs restarted two of them), and an operator who used to watch a session
    // disappear looks at an empty rail and concludes the work is invisible. The last few terminal
    // operations are the record that outlives the session.
    let activity: Vec<page::TerminalActivity> = app
        .audit
        .newest(200)
        .await
        .into_iter()
        .filter(|line| line["device_id"].as_str() == Some(device_id.as_str()))
        .filter(|line| {
            line["tool"]
                .as_str()
                .is_some_and(|tool| tool.starts_with("terminal."))
        })
        .take(5)
        .map(|line| page::TerminalActivity {
            at_ms: line["ts_ms"].as_u64().unwrap_or(0),
            tool: line["tool"].as_str().unwrap_or_default().to_owned(),
            status: line["status"].as_u64().unwrap_or(0),
            arrival: line["arrival"].as_str().unwrap_or_default().to_owned(),
        })
        .collect();
    let presets = lock(&app.connections).list(&device_id);
    let preset = query
        .preset
        .as_ref()
        .and_then(|id| presets.iter().find(|preset| &preset.id == id));
    Html(
        page::terminal(
            &device,
            &hex::encode(&session.csrf),
            &app.version,
            page::TerminalView {
                sessions: &sessions,
                presets: &presets,
                preset,
                activity: &activity,
                error: query.error.as_deref().or(load_error),
            },
        )
        .into_string(),
    )
    .into_response()
}

/// `POST /next/terminal/:device/:id/end`: close one of the device's sessions from the rail.
///
/// The page's own tabs close their session through the JSON route; this is for the session the
/// operator can see but did not open in this browser.
async fn next_terminal_end(
    State(app): State<AppState>,
    Path((device_id, session_id)): Path<(String, String)>,
    headers: HeaderMap,
    Form(form): Form<NextRemove>,
) -> Response {
    if next_session(&app, &headers, Some(&form.csrf)).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    let uri = format!("/next/terminal/{device_id}");
    match terminal_tool(
        &app,
        &device_id,
        "terminal.close",
        json!({ "id": session_id }),
    )
    .await
    {
        Ok(_) => Redirect::to(&uri).into_response(),
        Err(response) => {
            let sentence = refusal_sentence(*response).await;
            Redirect::to(&format!("{uri}?error={}", page::encode(&sentence))).into_response()
        }
    }
}

/// `POST /next/terminal/:device/connections`: save a preset from the toolbar's one-line form.
async fn next_terminal_preset_save(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<PresetSave>,
) -> Response {
    if next_session(&app, &headers, Some(&form.csrf)).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    let uri = format!("/next/terminal/{device_id}");
    let refuse = |sentence: &str| {
        Redirect::to(&format!("{uri}?error={}", page::encode(sentence))).into_response()
    };
    let mut draft = match draft_from_target(&form.name, &form.kind, &form.target) {
        Ok(draft) => draft,
        Err(sentence) => return refuse(sentence),
    };
    draft.key_path = form
        .key_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_owned);
    match lock(&app.connections).add(&device_id, draft) {
        Ok(_) => Redirect::to(&uri).into_response(),
        Err(sentence) => refuse(sentence),
    }
}

/// `POST /next/terminal/:device/connections/:id/remove`: forget one preset.
async fn next_terminal_preset_remove(
    State(app): State<AppState>,
    Path((device_id, id)): Path<(String, String)>,
    headers: HeaderMap,
    Form(form): Form<NextRemove>,
) -> Response {
    if next_session(&app, &headers, Some(&form.csrf)).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    let uri = format!("/next/terminal/{device_id}");
    match lock(&app.connections).remove(&device_id, &id) {
        Ok(true) => Redirect::to(&uri).into_response(),
        Ok(false) => Redirect::to(&format!(
            "{uri}?error={}",
            page::encode("That preset is not on this device.")
        ))
        .into_response(),
        Err(sentence) => {
            Redirect::to(&format!("{uri}?error={}", page::encode(sentence))).into_response()
        }
    }
}

/// `POST /next/terminal/:device/connections/:id/connect`: open a session from a preset.
///
/// Only `pty` and `serial` can connect this way: a preset never stores a credential, so an ssh one
/// fills the form (`?preset=`) and the operator types the password, as the page's form already asks.
async fn next_terminal_preset_connect(
    State(app): State<AppState>,
    Path((device_id, id)): Path<(String, String)>,
    headers: HeaderMap,
    Form(form): Form<NextRemove>,
) -> Response {
    if next_session(&app, &headers, Some(&form.csrf)).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    let uri = format!("/next/terminal/{device_id}");
    let refuse = |sentence: &str| {
        Redirect::to(&format!("{uri}?error={}", page::encode(sentence))).into_response()
    };
    let Some(preset) = lock(&app.connections)
        .list(&device_id)
        .into_iter()
        .find(|preset| preset.id == id)
    else {
        return refuse("That preset is not on this device.");
    };
    let arguments = match preset.kind.as_str() {
        "pty" => json!({ "kind": "pty" }),
        "serial" => json!({
            "kind": "serial",
            "port": preset.port_path,
            "baud": preset.baud.unwrap_or(115_200),
        }),
        "ssh" => {
            // No key on the preset means no credential at all: a password is never stored, so that
            // preset can only fill the form.
            let Some(key_path) = preset.key_path.as_deref() else {
                return refuse("An ssh preset needs its password typed on the page.");
            };
            json!({
                "kind": "ssh",
                "host": preset.host,
                "port": preset.port,
                "user": preset.user,
                "auth": { "kind": "key", "path": key_path },
            })
        }
        _ => return refuse("That preset's kind is unknown."),
    };
    match terminal_tool(&app, &device_id, "terminal.create", arguments).await {
        Ok(answer) => {
            let session = answer.get("id").and_then(Value::as_str).unwrap_or_default();
            Redirect::to(&format!("{uri}?attach={}", page::encode(session))).into_response()
        }
        Err(response) => refuse(&refusal_sentence(*response).await),
    }
}

#[derive(Deserialize)]
struct PresetSave {
    csrf: String,
    name: String,
    kind: String,
    #[serde(default)]
    target: String,
    /// An optional absolute path to a key on the device; only an ssh preset accepts one.
    #[serde(default)]
    key_path: Option<String>,
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
            // A password is typed on the page; a key path names a key the device already has. The
            // path is configuration, which is why a preset may carry it and a password may not.
            let auth = if !body.key_path.trim().is_empty() {
                json!({ "kind": "key", "path": body.key_path.trim() })
            } else if !body.password.is_empty() {
                json!({ "kind": "password", "password": body.password })
            } else {
                return terminal_refusal(
                    StatusCode::BAD_REQUEST,
                    "ssh needs a password typed here or a key path on the device",
                );
            };
            json!({
                "kind": "ssh",
                "host": body.host.trim(),
                "port": body.ssh_port.unwrap_or(22),
                "user": body.user.trim(),
                "auth": auth,
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
    let Some(session_id) = as_session_id(&session_id) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
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

/// Whether a device-supplied id is one shape an id may have: a path segment, not a path.
///
/// The page encodes ids into its own request paths, but the console is the one holding the
/// credential, so it refuses what it would never have produced: `session-3`, `5F61…` and the like.
fn as_session_id(raw: &str) -> Option<&str> {
    let valid = !raw.is_empty()
        && raw.len() <= 64
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then_some(raw)
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
    let Some(session_id) = as_session_id(&session_id) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
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
    let Some(session_id) = as_session_id(&session_id) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
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
    let Some(session_id) = as_session_id(&session_id) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
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
    /// The file a write just replaced, answered once on the page that redirected here.
    wrote: Option<String>,
}

#[derive(Deserialize)]
struct FilesWriteForm {
    csrf: String,
    path: String,
    content: String,
}

/// The page's own rule for a files path, applied before the device's.
///
/// The device canonicalizes under its root and is the authority (it refuses absolute paths, `..`
/// components and escaping symlinks). This is the page's half of the same boundary: the one
/// operator-supplied string that reaches a filesystem is refused here, with a sentence the operator
/// can act on, instead of relying on the far end to say it.
fn page_path(raw: &str) -> Result<String, &'static str> {
    let path = raw.trim().trim_start_matches('/');
    if path.contains('\\') || path.split('/').any(|part| part == "..") {
        return Err("a page path cannot contain '..' or a backslash");
    }
    Ok(path.to_owned())
}

/// `GET /next/files/:device`: the files root, server-rendered.
async fn next_files_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<FilesQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return login_card(&app, &headers).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut error = query.error.clone();
    // An error from the redirect (a refused mkdir, a refused write) says nothing about the path,
    // so the directory is still listed; only a path the console itself refuses skips the call.
    let entries = match page_path(&query.path) {
        Ok(path) => {
            let entries = match device_tool(&app, &device_id, "files.list", json!({ "path": path }))
                .await
            {
                Ok(text) => serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
                Err(_) => {
                    error.get_or_insert_with(|| {
                        "the device did not answer; the console's audit names the reason".to_owned()
                    });
                    Vec::new()
                }
            };
            entries
        }
        Err(sentence) => {
            error = Some(sentence.to_owned());
            Vec::new()
        }
    };
    let path = page_path(&query.path).unwrap_or_default();
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
            query.wrote.as_deref(),
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
        return login_card(&app, &headers).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = match page_path(&query.path) {
        Ok(path) => path,
        Err(sentence) => {
            return (StatusCode::BAD_REQUEST, sentence).into_response();
        }
    };
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
        Err(response) => {
            refused_page(
                &app,
                &device,
                &session,
                *response,
                page::Page::Files,
                format!("/next/files/{device_id}"),
                "Back to the files",
            )
            .await
        }
    }
}

#[derive(Deserialize)]
struct FilesMkdirForm {
    csrf: String,
    /// The folder to create, relative to the device's root.
    path: String,
    /// The listing the operator was looking at; the redirect comes back to it.
    #[serde(default)]
    current: String,
}

/// `POST /next/files/:device/mkdir`: create one directory, then show the listing it was made from.
async fn next_files_mkdir(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<FilesMkdirForm>,
) -> Response {
    if next_session(&app, &headers, Some(&form.csrf)).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    let mut target = format!("/next/files/{device_id}");
    let current = form.current.trim_matches('/');
    if !current.is_empty() {
        target.push_str(&format!("?path={}", page::encode(current)));
    }
    let refuse = |sentence: &str| {
        let separator = if target.contains('?') { '&' } else { '?' };
        Redirect::to(&format!(
            "{target}{separator}error={}",
            page::encode(sentence)
        ))
        .into_response()
    };
    let path = match page_path(&form.path) {
        Ok(path) => path,
        Err(sentence) => return refuse(sentence),
    };
    if path.is_empty() {
        return refuse("A folder needs a name.");
    }
    match device_tool(&app, &device_id, "files.mkdir", json!({ "path": path })).await {
        Ok(_) => Redirect::to(&target).into_response(),
        Err(response) => refuse(&refusal_sentence(*response).await),
    }
}

/// `POST /next/files/:device/write`: write one text file, then show its directory.
async fn next_files_write(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<FilesWriteForm>,
) -> Response {
    let Some(session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return no_such_device(&app);
    };
    let path = match page_path(&form.path) {
        Ok(path) => path,
        Err(sentence) => {
            return Redirect::to(&format!(
                "/next/files/{device_id}?error={}",
                page::encode(sentence)
            ))
            .into_response();
        }
    };
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
                "/next/files/{device_id}?path={}&wrote={}",
                page::encode(directory),
                page::encode(&form.path)
            ))
            .into_response()
        }
        Err(response) => {
            refused_page(
                &app,
                &device,
                &session,
                *response,
                page::Page::Files,
                format!("/next/files/{device_id}"),
                "Back to the files",
            )
            .await
        }
    }
}

#[derive(Deserialize, Default)]
struct RunsQuery {
    error: Option<String>,
    /// A history row to open.
    run: Option<String>,
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
        return login_card(&app, &headers).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (rows, load_error): (Vec<Value>, Option<&str>) =
        match device_tool(&app, &device_id, "runs.list", json!({ "limit": 20 })).await {
            Ok(text) => (
                serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
                None,
            ),
            Err(_) => (Vec::new(), Some("The device did not answer.")),
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
                id: row["id"].as_str().unwrap_or_default().to_owned(),
                command_line: format!("{command} {args}").trim().to_owned(),
                started_at_ms: row["started_at_ms"].as_u64().unwrap_or(0),
                duration_ms: row["duration_ms"].as_u64().unwrap_or(0),
                exit_code: row["exit_code"].as_i64(),
                timed_out: row["timed_out"].as_bool().unwrap_or(false),
            }
        })
        .collect();
    let mut error = query.error.clone();
    let detail = match query.run.as_deref() {
        Some(id) => match device_tool(&app, &device_id, "runs.get", json!({ "id": id })).await {
            Ok(text) => serde_json::from_str::<Value>(&text).ok().map(|row| {
                let args = row["args"]
                    .as_array()
                    .map(|args| {
                        args.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                page::RunDetail {
                    id: row["id"].as_str().unwrap_or_default().to_owned(),
                    command: row["command"].as_str().unwrap_or_default().to_owned(),
                    args,
                    cwd: row["cwd"].as_str().unwrap_or_default().to_owned(),
                    started_at_ms: row["started_at_ms"].as_u64().unwrap_or(0),
                    duration_ms: row["duration_ms"].as_u64().unwrap_or(0),
                    exit_code: row["exit_code"].as_i64(),
                    timed_out: row["timed_out"].as_bool().unwrap_or(false),
                    stdout_bytes: row["stdout_bytes"].as_u64().unwrap_or(0),
                    stderr_bytes: row["stderr_bytes"].as_u64().unwrap_or(0),
                }
            }),
            Err(response) => {
                error.get_or_insert_with(|| "that run is not in the device's history".to_owned());
                let _ = response;
                None
            }
        },
        None => None,
    };
    Html(
        page::runs(
            &device,
            &rows,
            detail.as_ref(),
            &hex::encode(&session.csrf),
            &app.version,
            error.as_deref().or(load_error),
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
        Err(response) => {
            refused_page(
                &app,
                &device,
                &session,
                *response,
                page::Page::Runs,
                format!("/next/runs/{device_id}"),
                "Back to the runs",
            )
            .await
        }
    }
}

#[derive(Deserialize, Default)]
struct SystemQuery {
    #[serde(default)]
    name: String,
    killed: Option<u64>,
    failed: Option<u64>,
    /// `asked` after a successful restart request, or the device's sentence when it refused.
    restart: Option<String>,
    /// The summary of a reachability test, or the device's sentence when it was refused.
    net: Option<String>,
}

#[derive(Deserialize)]
struct SystemKillForm {
    csrf: String,
    pid: Option<u64>,
    #[serde(default)]
    name: String,
}

/// `GET /next/state/:device`: the state document lives on the System page now; old links go there.
async fn next_state_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if cookie_session(&app, &headers).is_none() {
        return login_card(&app, &headers).into_response();
    }
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    // The system page shows both the processes and this document, and the rail's State entry is
    // gone; a bookmark to the old address lands where the document now lives.
    Redirect::to(&format!("/next/system/{device_id}")).into_response()
}

/// `GET /next/system/:device`: the process list and the kill form.
async fn next_system_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<SystemQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return login_card(&app, &headers).into_response();
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
    let (processes, load_error): (Vec<Value>, Option<&str>) =
        match device_tool(&app, &device_id, "system.process_list", arguments).await {
            Ok(text) => (
                serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
                None,
            ),
            Err(_) => (Vec::new(), Some("The device did not answer.")),
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
    }
    .or_else(|| query.net.clone())
    .or_else(|| {
        query.restart.as_deref().map(|restart| match restart {
            "asked" => "The device is restarting: it reports itself offline while the launcher \
                            brings it back. After three consecutive restarts the launcher gives up \
                            and it stays down."
                .to_owned(),
            sentence => sentence.to_owned(),
        })
    });
    // A failure to list processes is a notice, and the renderer suppresses the "listed no
    // processes" empty state when a notice is present.
    let notice = load_error.map(str::to_owned).or(notice);
    // The state document is part of this page now; a device that cannot answer it is a page
    // without that card, not an error page (the processes already said whether it answered).
    let state = match device_tool(&app, &device_id, "state.get", json!({})).await {
        Ok(text) => serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|value| serde_json::to_string_pretty(&value).ok())
            .or(Some(text)),
        Err(_) => None,
    };
    Html(
        page::system(
            &device,
            &processes,
            &name,
            &hex::encode(&session.csrf),
            &app.version,
            notice.as_deref(),
            state.as_deref(),
        )
        .into_string(),
    )
    .into_response()
}

#[derive(Deserialize)]
struct NetTestForm {
    csrf: String,
    host: String,
    port: u16,
}

/// `POST /next/system/:device/net`: ask the device whether it can reach a host:port.
///
/// The answer is a summary, not the raw sample: the page's notice slot is one sentence, and what an
/// operator asked is "yes or no, and how long".
async fn next_system_net_test(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<NetTestForm>,
) -> Response {
    if next_session(&app, &headers, Some(&form.csrf)).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    let host = form.host.trim().to_owned();
    let uri = format!("/next/system/{device_id}");
    let target = format!("{host}:{}", form.port);
    match device_tool(
        &app,
        &device_id,
        "system.net_test",
        json!({ "host": host, "port": form.port }),
    )
    .await
    {
        Ok(text) => {
            let report: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let addresses = report["addresses"]
                .as_array()
                .map(|list| list.len())
                .unwrap_or(0);
            // The device reports the address that connected, not a boolean: `connected` is a
            // string on success and null on failure. Reading it as a bool made every success look
            // like a failure, which is what the live check caught.
            let summary = match report["connected"].as_str() {
                Some(address) => format!(
                    "{target} is reachable — {addresses} address(es), connected to {address} in {} ms",
                    report["latency_ms"].as_u64().unwrap_or(0)
                ),
                None => {
                    let reason = report["error"].as_str().unwrap_or("no connection");
                    format!("{target} did not connect: {reason}")
                }
            };
            Redirect::to(&format!("{uri}?net={}", page::encode(&summary))).into_response()
        }
        Err(response) => {
            let sentence = refusal_sentence(*response).await;
            Redirect::to(&format!("{uri}?net={}", page::encode(&sentence))).into_response()
        }
    }
}

/// `POST /next/system/:device/restart`: restart the app the launcher supervises.
///
/// The device answers `{"restarting": true}` before it goes down, so the redirect lands; the next
/// page load will find it offline until the launcher brings it back.
async fn next_system_restart(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<NextRemove>,
) -> Response {
    if next_session(&app, &headers, Some(&form.csrf)).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    let uri = format!("/next/system/{device_id}");
    match device_tool(&app, &device_id, "host.restart", json!({})).await {
        Ok(_) => Redirect::to(&format!("{uri}?restart=asked")).into_response(),
        Err(response) => {
            let sentence = refusal_sentence(*response).await;
            Redirect::to(&format!("{uri}?restart={}", page::encode(&sentence))).into_response()
        }
    }
}

/// `POST /next/system/:device/kill`: end a process by pid or by exact name.
async fn next_system_kill(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<SystemKillForm>,
) -> Response {
    let Some(session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return no_such_device(&app);
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
            // The device answers `killed` as a list of pids (`KillReport.killed: Vec<u32>`); reading it
            // as a number made every successful kill render "ended 0 process(es)".
            let killed = answer["killed"]
                .as_array()
                .map_or(0, |pids| pids.len() as u64);
            let failed = answer["failed"]
                .as_array()
                .map(|failed| failed.len())
                .unwrap_or(0);
            Redirect::to(&format!(
                "/next/system/{device_id}?killed={killed}&failed={failed}"
            ))
            .into_response()
        }
        Err(response) => {
            refused_page(
                &app,
                &device,
                &session,
                *response,
                page::Page::System,
                format!("/next/system/{device_id}"),
                "Back to the system page",
            )
            .await
        }
    }
}

#[derive(Deserialize, Default)]
struct MemoryQuery {
    #[serde(default)]
    tag: String,
    /// A text search: when present, `memory.search` answers instead of `memory.list`.
    #[serde(default)]
    q: String,
    /// The entry the edit form is filled from.
    edit: Option<String>,
    /// A sentence after an update.
    notice: Option<String>,
}

#[derive(Deserialize)]
struct MemoryAppendForm {
    csrf: String,
    text: String,
    #[serde(default)]
    tags: String,
    /// Set by the edit form; an append leaves it absent.
    #[serde(default)]
    id: Option<String>,
    /// The list the edit was opened from, so the save lands back on it.
    #[serde(default)]
    q: String,
    #[serde(default)]
    tag: String,
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
        return login_card(&app, &headers).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let tag = query.tag.trim().to_owned();
    let search = query.q.trim().to_owned();
    let (entries, load_error): (Vec<Value>, Option<&str>) = if search.is_empty() {
        let mut arguments = json!({ "limit": 50 });
        if !tag.is_empty() {
            arguments["tag"] = json!(tag);
        }
        match device_tool(&app, &device_id, "memory.list", arguments).await {
            Ok(text) => (
                serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
                None,
            ),
            Err(_) => (Vec::new(), Some("The device did not answer.")),
        }
    } else {
        match device_tool(
            &app,
            &device_id,
            "memory.search",
            json!({ "query": search, "limit": 50 }),
        )
        .await
        {
            Ok(text) => (
                serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
                None,
            ),
            Err(_) => (Vec::new(), Some("The device did not answer.")),
        }
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
    let editing = query
        .edit
        .as_deref()
        .and_then(|id| entries.iter().find(|entry| entry.id == id).cloned());
    Html(
        page::memory(
            &device,
            &entries,
            page::MemoryLook {
                tag: &tag,
                query: &search,
                editing: editing.as_ref(),
            },
            &hex::encode(&session.csrf),
            &app.version,
            query.notice.as_deref().or(load_error),
        )
        .into_string(),
    )
    .into_response()
}

/// `POST /next/memory/:device/update`: rewrite one entry's text and complete tag list.
async fn next_memory_update(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<MemoryAppendForm>,
) -> Response {
    let Some(session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return no_such_device(&app);
    };
    let Some(id) = form.id.clone() else {
        return Redirect::to(&format!("/next/memory/{device_id}")).into_response();
    };
    let text = form.text.trim().to_owned();
    if text.is_empty() {
        return Redirect::to(&format!(
            "/next/memory/{device_id}?notice={}",
            page::encode("An entry's text cannot be emptied; delete it instead.")
        ))
        .into_response();
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
        "memory.update",
        json!({ "id": id, "text": text, "tags": tags }),
    )
    .await
    {
        Ok(_) => {
            let mut query = format!("notice={}", page::encode("The entry is rewritten."));
            if !form.q.trim().is_empty() {
                query.push_str(&format!("&q={}", page::encode(form.q.trim())));
            }
            if !form.tag.trim().is_empty() {
                query.push_str(&format!("&tag={}", page::encode(form.tag.trim())));
            }
            Redirect::to(&format!("/next/memory/{device_id}?{query}")).into_response()
        }
        Err(response) => {
            refused_page(
                &app,
                &device,
                &session,
                *response,
                page::Page::Memory,
                format!("/next/memory/{device_id}"),
                "Back to the memory",
            )
            .await
        }
    }
}

/// `GET /next/memory/:device/export`: the whole store as JSONL, as a download.
async fn next_memory_export(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(_session) = cookie_session(&app, &headers) else {
        return login_card(&app, &headers).into_response();
    };
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    match device_tool(&app, &device_id, "memory.export", json!({})).await {
        Ok(text) => {
            // The device answers an envelope (`{text, entries, truncated, total_bytes}`); the
            // download is the JSONL inside it, not the envelope around it.
            let report: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let body = report["text"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| text.clone());
            let mut builder = Response::builder()
                .header(header::CONTENT_TYPE, "application/x-ndjson; charset=utf-8")
                .header(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"memory-{device_id}.jsonl\""),
                );
            // JSONL has nowhere to put a note, so a cut export says so in a header.
            if report["truncated"].as_bool() == Some(true) {
                builder = builder.header("x-memory-truncated", "true");
            }
            builder
                .body(axum::body::Body::from(body))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(response) => *response,
    }
}

/// `POST /next/memory/:device/append`: keep one fact on the device.
async fn next_memory_append(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<MemoryAppendForm>,
) -> Response {
    let Some(session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return no_such_device(&app);
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
        Err(response) => {
            refused_page(
                &app,
                &device,
                &session,
                *response,
                page::Page::Memory,
                format!("/next/memory/{device_id}"),
                "Back to the memory",
            )
            .await
        }
    }
}

/// `POST /next/memory/:device/delete`: retire one entry.
async fn next_memory_delete(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<MemoryDeleteForm>,
) -> Response {
    let Some(session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return no_such_device(&app);
    };
    match device_tool(&app, &device_id, "memory.delete", json!({ "id": form.id })).await {
        Ok(_) => Redirect::to(&format!("/next/memory/{device_id}")).into_response(),
        Err(response) => {
            refused_page(
                &app,
                &device,
                &session,
                *response,
                page::Page::Memory,
                format!("/next/memory/{device_id}"),
                "Back to the memory",
            )
            .await
        }
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

/// `POST /next/monitors/:device/probe`: probe one watched target now, then show its summary.
async fn next_monitor_probe(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<MonitorRemoveForm>,
) -> Response {
    if next_session(&app, &headers, Some(&form.csrf)).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    let uri = format!("/next/monitors/{device_id}");
    match device_tool(&app, &device_id, "monitor.probe", json!({ "id": form.id })).await {
        Ok(_) => Redirect::to(&uri).into_response(),
        Err(response) => {
            let sentence = refusal_sentence(*response).await;
            Redirect::to(&format!("{uri}?error={}", page::encode(&sentence))).into_response()
        }
    }
}

/// `GET /next/screen/:device`: one capture, inline as a data URL.
async fn next_screen_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return login_card(&app, &headers).into_response();
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
#[derive(Deserialize)]
struct MonitorQuery {
    error: Option<String>,
}

async fn next_monitors_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<MonitorQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return login_card(&app, &headers).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut error = query.error.clone();
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
                // HOW LONG THE CURRENT STATE HAS LASTED is the question an operator asks first about a
                // target that is down ("since when?"), and the device already answers it.
                since_ms: target["summary"]["since_ms"].as_u64(),
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
            (app.now)(),
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
    let Some(session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return no_such_device(&app);
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
        Err(response) => {
            refused_page(
                &app,
                &device,
                &session,
                *response,
                page::Page::Monitors,
                format!("/next/monitors/{device_id}"),
                "Back to the monitors",
            )
            .await
        }
    }
}

/// `POST /next/monitors/:device/remove`: stop watching one target.
async fn next_monitor_remove(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<MonitorRemoveForm>,
) -> Response {
    let Some(session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return no_such_device(&app);
    };
    match device_tool(&app, &device_id, "monitor.remove", json!({ "id": form.id })).await {
        Ok(_) => Redirect::to(&format!("/next/monitors/{device_id}")).into_response(),
        Err(response) => {
            refused_page(
                &app,
                &device,
                &session,
                *response,
                page::Page::Monitors,
                format!("/next/monitors/{device_id}"),
                "Back to the monitors",
            )
            .await
        }
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
#[derive(Deserialize)]
struct BrowserFrameQuery {
    target: String,
    /// `0` when the pane learned this device has no live stream, so the console stops asking for
    /// one on every frame; absent or `1` tries the stream first.
    #[serde(default)]
    stream: Option<String>,
}

/// `GET /next/browser/:device/frame`: the newest screencast frame, as JSON, for the pane's pump.
///
/// This is the device's `browser.cast`, not a fresh screenshot: Chrome pushes frames to a live
/// stream on the device, and each poll takes the newest one — so the device encodes once per painted
/// frame instead of once per request, and a slow console loses frames rather than falling behind.
async fn next_browser_frame(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<BrowserFrameQuery>,
) -> Response {
    if cookie_session(&app, &headers).is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    if query.target.trim().is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let target = query.target.trim().to_owned();
    let wants_stream = query.stream.as_deref() != Some("0");
    // The live stream is the better answer, but a device build older than `browser.cast` has to keep
    // working: the pane then gets one screenshot per poll, which is what it had before. The answer
    // says which one it was, so the pane can slow down instead of asking a device without a stream
    // five times a second.
    let streamed = if wants_stream {
        match device_tool(
            &app,
            &device_id,
            "browser.cast",
            json!({ "target": target }),
        )
        .await
        {
            Ok(text) => Some(text),
            Err(response) => {
                let sentence = refusal_sentence(*response).await;
                // Only "this device has no such tool" is a reason to fall back; a transient
                // failure must reach the caller, because a silent fallback makes the pane latch
                // itself down to screenshots for the life of the tab.
                if sentence.contains("unknown tool") {
                    None
                } else {
                    return (StatusCode::BAD_GATEWAY, Json(json!({ "error": sentence })))
                        .into_response();
                }
            }
        }
    } else {
        None
    };
    let (captured, streaming) = match streamed {
        Some(text) => (text, true),
        None => {
            match device_tool(
                &app,
                &device_id,
                "browser.screenshot",
                json!({ "target": target }),
            )
            .await
            {
                Ok(text) => (text, false),
                Err(response) => return *response,
            }
        }
    };
    let capture: Value = serde_json::from_str(&captured).unwrap_or(Value::Null);
    match capture["data"].as_str() {
        Some(data) => Json(json!({
            "format": capture["format"].as_str().unwrap_or("jpeg"),
            "data": data,
            "width": capture["width"],
            "height": capture["height"],
            "at_ms": capture["at_ms"],
            "stream": streaming,
        }))
        .into_response(),
        None => StatusCode::BAD_GATEWAY.into_response(),
    }
}

#[derive(Deserialize)]
struct BrowserQuery {
    error: Option<String>,
}

async fn next_browser_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<BrowserQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return login_card(&app, &headers).into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut error = query.error.clone();
    // **THE PAGE DOES NOT WAIT TEN SECONDS FOR A LIST.** On the live device this call sits at the
    // command bound while the device's browser starts; three seconds is long enough for an answer
    // and short enough that the page still opens, with the list named unknown rather than empty.
    let listed = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        device_tool(&app, &device_id, "browser.list", json!({})),
    )
    .await;
    let targets = match listed {
        Ok(Ok(text)) => serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
        Ok(Err(response)) => {
            error = Some(refusal_sentence(*response).await);
            Vec::new()
        }
        Err(_) => {
            // **NO EXPLANATION, BECAUSE THE ONE I WROTE WAS WRONG.** This bound was added while the
            // page measured 10 s; the measurement was taken through the device's own browser, whose
            // tool calls serialize with the page's `browser.list`. Fetched with a session cookie and
            // no device browser in the way, the page answers in 34 ms. The bound stays as a
            // backstop; the excuse for it does not.
            error = Some(
                "The device did not answer within 3 seconds; its browser targets are unknown."
                    .to_owned(),
            );
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
        // **THE THUMBNAIL MUST NOT HOLD THE PAGE.** On a live device this call has been measured at
        // the full command bound (10 s) before the page was sent; the capture is one target's
        // still, and the pane's pump loads a live frame a moment later. A short bound keeps the
        // page responsive and lets the pump do its job.
        let capture = tokio::time::timeout(
            std::time::Duration::from_millis(1_500),
            device_tool(
                &app,
                &device_id,
                "browser.screenshot",
                json!({ "target": first.id }),
            ),
        )
        .await;
        if let Ok(Ok(text)) = capture {
            let capture: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let format = capture["format"].as_str().unwrap_or("png");
            if let Some(data) = capture["data"].as_str() {
                shot = Some(format!("data:image/{format};base64,{data}"));
            }
        }
    }
    let last_eval = lock(&app.last_evals).get(&device.id).cloned();
    Html(
        page::browser(
            &device,
            &rows,
            shot.as_deref(),
            &hex::encode(&session.csrf),
            &app.version,
            error.as_deref(),
            last_eval.as_ref(),
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
    let Some(session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return no_such_device(&app);
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
        Err(response) => {
            refused_page(
                &app,
                &device,
                &session,
                *response,
                page::Page::Screen,
                format!("/next/browser/{device_id}"),
                "Back to the screen",
            )
            .await
        }
    }
}

#[derive(Deserialize)]
struct BrowserNavigateForm {
    csrf: String,
    target: String,
    url: String,
}

/// `POST /next/browser/:device/navigate`: send one target to a URL.
async fn next_browser_navigate(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<BrowserNavigateForm>,
) -> Response {
    if next_session(&app, &headers, Some(&form.csrf)).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    let uri = format!("/next/browser/{device_id}");
    let url = form.url.trim().to_owned();
    if url.is_empty() {
        return Redirect::to(&format!("{uri}?error={}", page::encode("A URL is needed.")))
            .into_response();
    }
    match device_tool(
        &app,
        &device_id,
        "browser.navigate",
        json!({ "target": form.target, "url": url }),
    )
    .await
    {
        Ok(_) => Redirect::to(&uri).into_response(),
        Err(response) => {
            let sentence = refusal_sentence(*response).await;
            Redirect::to(&format!("{uri}?error={}", page::encode(&sentence))).into_response()
        }
    }
}

/// `POST /next/browser/:device/shutdown`: end the browser process and every tab in it.
async fn next_browser_shutdown(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<NextRemove>,
) -> Response {
    if next_session(&app, &headers, Some(&form.csrf)).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    let uri = format!("/next/browser/{device_id}");
    match device_tool(&app, &device_id, "browser.shutdown", json!({})).await {
        Ok(_) => Redirect::to(&uri).into_response(),
        Err(response) => {
            let sentence = refusal_sentence(*response).await;
            Redirect::to(&format!("{uri}?error={}", page::encode(&sentence))).into_response()
        }
    }
}

#[derive(Deserialize)]
struct BrowserEvalForm {
    csrf: String,
    target: String,
    expression: String,
}

/// `POST /next/browser/:device/eval`: evaluate one expression in a target, and keep the answer for
/// the page that follows.
async fn next_browser_eval(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<BrowserEvalForm>,
) -> Response {
    if next_session(&app, &headers, Some(&form.csrf)).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.has_device(&device_id) {
        return no_such_device(&app);
    }
    let uri = format!("/next/browser/{device_id}");
    let expression = form.expression.trim().to_owned();
    if expression.is_empty() {
        return Redirect::to(&format!(
            "{uri}?error={}",
            page::encode("An expression is needed.")
        ))
        .into_response();
    }
    match device_tool(
        &app,
        &device_id,
        "browser.eval",
        json!({ "target": form.target, "expression": expression }),
    )
    .await
    {
        Ok(text) => {
            let value: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!(text));
            let mut rendered =
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
            const LIMIT: usize = 4_000;
            if rendered.chars().count() > LIMIT {
                rendered = rendered.chars().take(LIMIT).collect::<String>() + "\n… (truncated)";
            }
            lock(&app.last_evals).insert(
                device_id,
                page::LastEval {
                    target: form.target,
                    expression,
                    value: rendered,
                },
            );
            Redirect::to(&uri).into_response()
        }
        Err(response) => {
            let sentence = refusal_sentence(*response).await;
            Redirect::to(&format!("{uri}?error={}", page::encode(&sentence))).into_response()
        }
    }
}

/// `POST /next/browser/:device/close`: close one target.
async fn next_browser_close(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<BrowserTargetForm>,
) -> Response {
    let Some(session) = next_session(&app, &headers, Some(&form.csrf)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(device) = device_views(&app)
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return no_such_device(&app);
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
        Err(response) => {
            refused_page(
                &app,
                &device,
                &session,
                *response,
                page::Page::Screen,
                format!("/next/browser/{device_id}"),
                "Back to the screen",
            )
            .await
        }
    }
}

#[derive(Deserialize)]
struct TokenMintForm {
    csrf: String,
    name: String,
    /// Present only when the operator ticked the consent box. A checkbox that is not ticked posts
    /// nothing, so absent is "no destructive consent" rather than a parse error.
    #[serde(default)]
    allow_destructive: Option<String>,
    /// Seconds of life, from the card's select; absent (or empty) is the permanent token the form
    /// was born making.
    #[serde(default)]
    expires_in_s: Option<String>,
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

/// The one refusal the tokens page has to explain: a lifetime outside the store's bounds, or the
/// store's cap. The code travels in the URL; the page turns it into a sentence.
#[derive(serde::Deserialize)]
struct TokensQuery {
    error: Option<String>,
}

/// `GET /next/tokens/:device`: the tokens that name this device, and the mint form.
async fn next_tokens_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<TokensQuery>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return login_card(&app, &headers).into_response();
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
            expires_at_ms: token.expires_at_ms,
        })
        .collect();
    // **A REFUSAL THE PAGE NEVER SAID.** The mint handler redirected with `?error=refused` and
    // this handler ignored the query, so a token that was not created looked like a form that did
    // nothing. The code is mapped here, where the sentence belongs.
    let error = match query.error.as_deref() {
        Some("expiry") => {
            Some("That lifetime is not between one hour and 365 days; the token was not created.")
        }
        Some("refused") => {
            Some("The console did not create that token: it holds at most 64, so revoke one first.")
        }
        _ => None,
    };
    Html(
        page::tokens(
            &device,
            &rows,
            &hex::encode(&session.csrf),
            &app.version,
            error,
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
    let expires_in_s = match form.expires_in_s.as_deref() {
        None | Some("") => None,
        Some(value) => match value.parse::<u64>() {
            Ok(seconds) => Some(seconds),
            // A value the form never offers is a hand-made request; the store's own bounds are the
            // authority, and this refusal is the same sentence the operator would get from it.
            Err(_) => {
                return Redirect::to(&format!("/next/tokens/{device_id}?error=expiry"))
                    .into_response()
            }
        },
    };
    let created = lock(&app.tokens).create(
        crate::tokens::Draft {
            name: form.name,
            scope: crate::tokens::TokenScope::Device(device_id.clone()),
            tools: every_tool(),
            allow_destructive: form.allow_destructive.is_some(),
            expires_in_s,
        },
        |id| known.contains(id),
        (app.now)(),
    );
    let (meta, secret) = match created {
        Ok(created) => created,
        Err(
            crate::tokens::TokenError::ExpiryTooShort | crate::tokens::TokenError::ExpiryTooLong,
        ) => {
            return Redirect::to(&format!("/next/tokens/{device_id}?error=expiry")).into_response()
        }
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
    // The page names a device; a token that does not reach it is not this page's to revoke. Without
    // this, a revoke form on device A could destroy device B's token.
    let belongs = lock(&app.tokens).list().into_iter().any(|token| {
        token.id == form.id
            && (token.device_id == device_id
                || token
                    .device_ids
                    .as_ref()
                    .is_some_and(|ids| ids.contains(&device_id)))
    });
    if !belongs {
        return StatusCode::NOT_FOUND.into_response();
    }
    let _ = lock(&app.tokens).revoke(&form.id);
    Redirect::to(&format!("/next/tokens/{device_id}")).into_response()
}

/// A percentage, rounded, and zero rather than a division by zero when the device reported nothing.
fn percent_of(used: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    ((used as f64 / total as f64) * 100.0).round() as u64
}

/// `GET /next/vitals/:device`: one sample of what the device reports about itself.
async fn next_vitals_page(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return login_card(&app, &headers).into_response();
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
    // One point per visit, capped: the trend card is the page's own history and says so. A full
    // ring drops the oldest, which is the least relevant point on screen.
    const MAX_TREND: usize = 60;
    let memory_total = facts["memory"]["total_bytes"].as_u64().unwrap_or(0);
    let memory_available = facts["memory"]["available_bytes"].as_u64().unwrap_or(0);
    let memory_pct = percent_of(memory_total.saturating_sub(memory_available), memory_total);
    let disk_total: u64 = facts["disks"]
        .as_array()
        .map(|disks| {
            disks
                .iter()
                .filter_map(|disk| disk["total_bytes"].as_u64())
                .sum()
        })
        .unwrap_or(0);
    let disk_free: u64 = facts["disks"]
        .as_array()
        .map(|disks| {
            disks
                .iter()
                .filter_map(|disk| disk["free_bytes"].as_u64())
                .sum()
        })
        .unwrap_or(0);
    let disk_pct = percent_of(disk_total.saturating_sub(disk_free), disk_total);
    let trend = {
        let mut trends = lock(&app.vitals);
        let points = trends.entry(device_id.clone()).or_default();
        // No totals is no reading: the facts may be the platform's error shape, or a build with no
        // disks. Recording ("0%, 0%") would draw a line claiming the machine is empty.
        if memory_total > 0 || disk_total > 0 {
            points.push(page::VitalPoint {
                memory_pct,
                disk_pct,
            });
            if points.len() > MAX_TREND {
                let excess = points.len() - MAX_TREND;
                points.drain(..excess);
            }
        }
        points.clone()
    };
    let generation = |name: &str| {
        let generation = &facts["update"][name];
        generation["version"]
            .as_str()
            .map(|version| page::Generation {
                version: version.to_owned(),
                channel: generation["channel"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            })
    };
    let swap = {
        let swap = &facts["update"]["last_swap"];
        swap["outcome"].as_str().map(|outcome| page::SwapView {
            outcome: outcome.to_owned(),
            version: swap["version"].as_str().map(str::to_owned),
            at_ms: swap["at_ms"].as_u64().unwrap_or(0),
            reason: swap["reason"].as_str().map(str::to_owned),
        })
    };
    let repairs = facts["update"]["repairs"]
        .as_array()
        .map(|repairs| {
            repairs
                .iter()
                .filter_map(|repair| {
                    Some(page::RepairView {
                        from: repair["from"].as_str()?.to_owned(),
                        at_ms: repair["at_ms"].as_u64().unwrap_or(0),
                        // An empty string is not a digest: it rendered a separator with nothing
                        // after it on the live page.
                        sha256: repair["sha256"]
                            .as_str()
                            .filter(|digest| !digest.is_empty())
                            .map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let restarts = facts["restarts"]
        .as_array()
        .map(|restarts| {
            restarts
                .iter()
                .filter_map(|restart| {
                    Some(page::RestartView {
                        at_ms: restart["at_ms"].as_u64()?,
                        version: restart["version"].as_str()?.to_owned(),
                        channel: restart["channel"].as_str().unwrap_or_default().to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let shell = facts["shell"]
        .get("sha256")
        .and_then(Value::as_str)
        .map(|digest| page::ShellView {
            sha256: digest.to_owned(),
            bytes: facts["shell"]["bytes"].as_u64().unwrap_or(0),
        });
    let install = page::InstallView {
        current: generation("current"),
        previous: generation("previous"),
        pending: generation("pending"),
        swap,
        repairs,
        restarts,
        shell,
    };
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
        cpu_percent: facts["cpu"]["percent_used"].as_u64(),
        uptime_s: facts["uptime_s"].as_u64().unwrap_or(0),
        memory_total: facts["memory"]["total_bytes"].as_u64().unwrap_or(0),
        memory_available: facts["memory"]["available_bytes"].as_u64().unwrap_or(0),
        trend,
        install,
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
    if operator_from_credentials(&app, form.code.as_deref(), form.token.as_deref())
        .await
        .is_err()
    {
        // Never the attempt itself: a line that records what was typed is a line that leaks it.
        app.record(
            "auth",
            None,
            StatusCode::UNAUTHORIZED,
            Some("token refused"),
        )
        .await;
        let (_, access) = app.identity(&headers);
        let note = access_note(&access, &headers);
        return (
            StatusCode::UNAUTHORIZED,
            Html(
                page::login(
                    note.as_deref(),
                    Some("That token or sign-in code was not accepted."),
                )
                .into_string(),
            ),
        )
            .into_response();
    }
    let (_session, token) = new_session(&app, "token").await;
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
    // Ours is the cookie; the identity is Access's. Clearing only the cookie would be undone by the
    // next request the edge authenticates, so behind Access sign-out ends the edge's session too.
    // Without Access there is no edge to sign out of, and the card is the way back in.
    let target = match app.access.as_ref().and_then(crate::access::Access::issuer) {
        Some(issuer) => format!("{}/cdn-cgi/access/logout", issuer.trim_end_matches('/')),
        None => "/next".to_owned(),
    };
    if let Some(session) = cookie_session(&app, &headers) {
        let Some(csrf) = form.csrf.as_deref().and_then(hex::decode_32) else {
            return StatusCode::FORBIDDEN.into_response();
        };
        if !constant_time_eq(&csrf, &session.csrf) {
            return StatusCode::FORBIDDEN.into_response();
        }
        if let Err(error) = end_session(&app, &session.token_hash) {
            // Keep the cookie: the session is still live, so clearing it would lie.
            eprintln!("summrise-console: the sessions could not be persisted: {error}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    // A tab whose cookie is already gone still gets to ask the edge to forget it — otherwise the
    // button does nothing, because the next request would be signed straight back in. That path
    // needs no CSRF value: ending a session is not a security boundary, and the edge's own logout
    // is a URL anyone can send.
    let mut response = Redirect::to(&target).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        clear_session_cookie(secure_request(&headers)),
    );
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
            // The channel is live memory; the registry is what survives a restart, and an offline
            // device is read from the registry.
            let _ = lock(&app.devices).record_version(&device_id, version);
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
    // **THE CONSOLE MUST ANSWER BEFORE THE PROXY IN FRONT OF IT GIVES UP.** The operator's console is
    // reached through Cloudflare Access, whose origin timeout is about a hundred seconds: a run that
    // is proxied for ten minutes is cut there and the operator sees Cloudflare's HTML error instead of
    // this console's answer (measured 2026-10-28: two `browser.eval`/`runs.exec` calls, CF 504 pages
    // with ray ids). Ninety seconds leaves room for the frame's round trip and the answer's write; a
    // run that needs longer keeps running on the device and is collected through `runs.list`.
    Some((Duration::from_millis(clamped) + RUNS_WAIT_GRACE).min(RUNS_MAX_PROXY_WAIT))
}

/// The device's own cap on `runs.exec`, mirrored so the wait cannot exceed it.
const RUNS_MAX_TIMEOUT_MS: u64 = 10 * 60 * 1000;

/// The longest a proxied call may hold the operator's HTTP request open.
const RUNS_MAX_PROXY_WAIT: Duration = Duration::from_secs(90);

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
/// The JSON-RPC code a device's error envelope carries, when it is one.
fn error_code(reply: &Reply) -> Option<i64> {
    match reply {
        Reply::Error(error) => error
            .get("error")
            .and_then(|body| body.get("code"))
            .and_then(Value::as_i64),
        Reply::Result(_) => None,
    }
}

/// What a held call will run, as the operator should read it: the arguments, pretty-printed and cut.
fn arguments_summary(request: &Value) -> String {
    let arguments = request
        .get("params")
        .and_then(|params| params.get("arguments"))
        .cloned()
        .unwrap_or(Value::Null);
    let text = serde_json::to_string_pretty(&arguments).unwrap_or_default();
    let mut summary: String = text.chars().take(400).collect();
    if text.chars().count() > 400 {
        summary.push('…');
    }
    summary
}

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
    // An approval is answered by re-sending this exact call, so it is kept beside the one that left.
    let held = request.clone();

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
            // A device answers -32002 when a destructive call's credential did not consent to it.
            // Through the console that call is HELD here instead of returned: the console is the
            // only surface where a person can say yes, and the client's request stays open while
            // they decide. An unanswered question is refused — expiry fails closed, as the legacy
            // gate did.
            // Only a credential that did NOT consent is held. A consenting token answered -32002
            // is a device refusing for its own reasons (a damaged or hostile build), and asking the
            // operator to authorize what they already authorized would stall the page for the whole
            // deadline.
            let askable = match auth {
                RequestAuth::Token(scope) if !scope.allow_destructive => Some(scope),
                _ => None,
            };
            if error_code(&reply) == Some(-32002) {
                if let Some(scope) = askable {
                    let consenting = crate::channels::TokenScope {
                        token_id: scope.token_id.clone(),
                        device_id: scope.device_id.clone(),
                        scopes: scope.scopes.clone(),
                        allow_destructive: true,
                        expires_at_ms: scope.expires_at_ms,
                    };
                    let id = hex::encode(&random_32()[..8]);
                    let (decide, answer) = tokio::sync::oneshot::channel();
                    let mut registration = Held {
                        app,
                        id: id.clone(),
                        device_id: audit_id.clone(),
                        tool: tool.clone(),
                        token_id: token_id.map(str::to_owned),
                        resolved: false,
                    };
                    // Resolved now, while the credential exists: the card outlives a revocation,
                    // and the operator still deserves to know whose call they are answering.
                    let token_name = token_id.and_then(|id| lock(&app.tokens).find_by_id(id));
                    let asked_ms = (app.now)();
                    lock(&app.approvals).insert(
                        id.clone(),
                        Pending {
                            row: page::ApprovalRow {
                                id: id.clone(),
                                device_id: device_id.to_owned(),
                                tool: tool.clone().unwrap_or_default(),
                                arguments: arguments_summary(&held),
                                created_ms: asked_ms,
                                // The deadline travels with the answer's data rather than with the
                                // page: the row says when it stops being askable, and the page's
                                // script turns that into the seconds the operator has left.
                                expires_ms: asked_ms
                                    .saturating_add(APPROVAL_TTL.as_millis() as u64),
                                token_name: token_name.as_ref().map(|token| token.name.clone()),
                                token_id: token_id.map(str::to_owned),
                            },
                            decide,
                        },
                    );
                    app.record_facts(AuditFacts {
                        kind: "approval",
                        device_id: audit_id.as_deref(),
                        status: StatusCode::OK,
                        detail: Some("asked"),
                        tool: tool.as_deref(),
                        token_id,
                    })
                    .await;
                    match tokio::time::timeout(APPROVAL_TTL, answer).await {
                        Ok(Ok(true)) => {
                            registration.resolved = true;
                            app.record_facts(AuditFacts {
                                kind: "approval",
                                device_id: audit_id.as_deref(),
                                status: StatusCode::OK,
                                detail: Some("approved"),
                                tool: tool.as_deref(),
                                token_id,
                            })
                            .await;
                            // The consent is for THIS call, held in the console, and it is not
                            // written anywhere: the token keeps saying no to the next one.
                            //
                            // The credential is re-read, not remembered: the 90 s the operator had
                            // to decide is long enough for a token to be revoked or to expire, and
                            // the device cannot know about a revocation — it only checks expiry. A
                            // consent to a credential that no longer exists is not a consent.
                            let still_live = lock(&app.tokens)
                                .find_by_id(&consenting.token_id)
                                .is_some_and(|record| {
                                    record
                                        .expires_at_ms
                                        .is_none_or(|expires| (app.now)() < expires)
                                });
                            if !still_live {
                                app.record_facts(AuditFacts {
                                    kind: "approval",
                                    device_id: audit_id.as_deref(),
                                    status: StatusCode::UNAUTHORIZED,
                                    detail: Some("token gone"),
                                    tool: tool.as_deref(),
                                    token_id,
                                })
                                .await;
                                reply = Reply::Error(json!({
                                    "code": -32001,
                                    "message": "this call was approved, but the token that asked for it is no longer valid: it was refused, not run.",
                                }));
                                return Ok(reply);
                            }
                            //
                            // The bound is the first dispatch's, not the general one: a run holds
                            // its request for the command's own timeout, and approving one must not
                            // make the console give up while the device is still running it.
                            let bound = run_wait(&held)
                                .map(|wait| wait + RUNS_BACKSTOP_GRACE)
                                .unwrap_or(app.mcp_timeout);
                            match app
                                .channels
                                .request_within(
                                    device_id,
                                    held.clone(),
                                    &RequestAuth::Token(&consenting),
                                    bound,
                                )
                                .await
                            {
                                Ok(retried) => reply = retried,
                                Err(_) => {
                                    // The frame may have reached the device before the channel
                                    // failed, so the call may have run. Returning the original
                                    // "needs approval" would invite a retry that runs it TWICE; the
                                    // only honest answer is that the outcome is unknown.
                                    app.record_facts(AuditFacts {
                                        kind: "approval",
                                        device_id: audit_id.as_deref(),
                                        status: StatusCode::GATEWAY_TIMEOUT,
                                        detail: Some("approved, but the device did not answer"),
                                        tool: tool.as_deref(),
                                        token_id,
                                    })
                                    .await;
                                    return Err(Box::new(ApiError::gateway_timeout(
                                        "approved, but the device did not answer: the outcome is                                          unknown, and retrying may run it twice",
                                    )
                                    .into_response()));
                                }
                            }
                        }
                        Ok(_) => {
                            registration.resolved = true;
                            app.record_facts(AuditFacts {
                                kind: "approval",
                                device_id: audit_id.as_deref(),
                                status: StatusCode::OK,
                                detail: Some("refused"),
                                tool: tool.as_deref(),
                                token_id,
                            })
                            .await;
                        }
                        Err(_) => {
                            registration.resolved = true;
                            app.record_facts(AuditFacts {
                                kind: "approval",
                                device_id: audit_id.as_deref(),
                                status: StatusCode::OK,
                                detail: Some("timed out"),
                                tool: tool.as_deref(),
                                token_id,
                            })
                            .await;
                        }
                    }
                    drop(registration);
                }
            }
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
    // Its held calls go with it: a removed machine has nothing left to approve.
    refusals_for(&app, &device_id);
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

    let outage = app.channels.last_outage(&device.id);
    Ok(Json(DeviceView {
        online: app.channels.online(&device.id),
        version: app.channels.version(&device.id),
        host: app.channels.host(&device.id),
        name: device.name.or_else(|| app.channels.name(&device.id)),
        last_attempt: app.channels.retry(&device.id).map(|note| RetryView {
            attempts: note.attempts,
            reason: note.reason,
        }),
        last_outage: outage
            .and_then(|outage| outage.duration_ms)
            .map(|duration_ms| OutageView {
                started_ms: outage.map(|outage| outage.started_ms).unwrap_or(0),
                duration_ms,
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
            // The live channel wins; the registry's last announced version is the offline answer.
            version: app
                .channels
                .version(&device.id)
                .or_else(|| device.last_version.clone()),
            host: app.channels.host(&device.id),
            last_attempt: app.channels.retry(&device.id).map(|note| RetryView {
                attempts: note.attempts,
                reason: note.reason,
            }),
            last_outage: app
                .channels
                .last_outage(&device.id)
                .and_then(|outage| outage.duration_ms)
                .map(|duration_ms| OutageView {
                    started_ms: app
                        .channels
                        .last_outage(&device.id)
                        .map(|outage| outage.started_ms)
                        .unwrap_or(0),
                    duration_ms,
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
    /// The last time this device's channel went away and came back, when the console saw both ends.
    /// `online` answers "is it up now"; this answers "was it away, and for how long" — the question
    /// an operator asks when they come back to a fleet where nothing looks wrong any more.
    #[serde(skip_serializing_if = "Option::is_none")]
    last_outage: Option<OutageView>,
    /// The machine facts the device last sent over its channel: the operator reads what the device is,
    /// including which sessions are disconnected, without minting a grant to ask.
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<serde_json::Value>,
}

/// One finished outage, as a page reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct OutageView {
    pub(crate) started_ms: u64,
    pub(crate) duration_ms: u64,
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

    /// The page refuses a traversal itself; the device's canonicalization is the second half of the
    /// boundary, not the only half.
    #[test]
    fn a_page_path_is_refused_before_the_device_sees_it() {
        assert_eq!(page_path("logs/agent.log").as_deref(), Ok("logs/agent.log"));
        assert_eq!(page_path("/logs").as_deref(), Ok("logs"));
        assert_eq!(page_path("").as_deref(), Ok(""));
        for bad in ["../etc/passwd", "a/../../b", "..\\windows", "C:\\x", "a\\b"] {
            assert!(page_path(bad).is_err(), "{bad} must be refused");
        }
    }

    /// The presets form takes one line; the parts it produces are the registry's draft fields.
    #[test]
    fn a_target_line_becomes_the_draft_the_registry_validates() {
        let ssh = draft_from_target("edge", "ssh", "root@10.0.0.5:2222").expect("ssh parses");
        assert_eq!(ssh.name, "edge");
        assert_eq!(ssh.kind, "ssh");
        assert_eq!(ssh.user.as_deref(), Some("root"));
        assert_eq!(ssh.host.as_deref(), Some("10.0.0.5"));
        assert_eq!(ssh.port, Some(2222));

        let default_port = draft_from_target("edge", "ssh", "root@10.0.0.5").expect("ssh parses");
        assert_eq!(default_port.port, Some(22));

        let serial = draft_from_target("console", "serial", "COM3@9600").expect("serial parses");
        assert_eq!(serial.port_path.as_deref(), Some("COM3"));
        assert_eq!(serial.baud, Some(9600));

        let plain = draft_from_target("console", "serial", "COM3").expect("serial parses");
        assert_eq!(plain.baud, None);

        let pty = draft_from_target("shell", "pty", "ignored").expect("pty parses");
        assert_eq!(pty.kind, "pty");
        assert_eq!(pty.host, None);

        // A serial path may contain anything (a `/dev/serial/by-id/...` path has colons); only the
        // length rule applies, and the device owns whether the OS has that port.
        for (kind, target) in [
            ("ssh", "no-at-sign"),
            ("ssh", "root@host:not-a-port"),
            ("telnet", "x"),
        ] {
            assert!(
                draft_from_target("name", kind, target).is_err(),
                "{kind} {target} must be refused"
            );
        }
    }

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
        // A ten-minute run is NOT waited for end to end: the proxy in front of the console gives up
        // at about a hundred seconds, so the console answers first and the run is collected later.
        assert_eq!(
            run_wait(&call(
                "runs.exec",
                json!({"command": "ver", "timeout_ms": 86_400_000})
            )),
            Some(RUNS_MAX_PROXY_WAIT)
        );
        assert!(
            RUNS_MAX_PROXY_WAIT < Duration::from_secs(100),
            "the bound must sit under the proxy's ceiling"
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
