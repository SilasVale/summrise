//! The HTTP surface: the operator page, login, enrollment, the device registry, grant signing,
//! permanent-token management and health.
//!
//! One router, built once from a loaded [`Console`]. The handlers are thin on purpose: the decisions
//! live in `auth`, `codes`, `devices` and `grants`, and the router only turns refusals into status
//! codes.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
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

/// How far a device's clock may be from the console's when it registers: five minutes.
const REGISTRATION_SKEW_MS: u64 = 5 * 60 * 1000;

/// How many log lines `GET /audit` and `GET /denials` return when the caller names no limit.
const DEFAULT_LOG_LIMIT: usize = 100;

/// The largest limit the log routes honor: an operator reading an index must not make the console
/// hold an unbounded slice of it.
const MAX_LOG_LIMIT: usize = 1000;

/// The header a trusted reverse proxy uses to name the operator it let in.
///
/// Not a credential on its own: the console believes it only when the connection arrives from the
/// address `--trusted-proxy` names, and the proxy overwrites whatever the browser sent. A request
/// from anywhere else is asked for the operator token even when it carries this header.
const PROXY_IDENTITY_HEADER: &str = "x-forwarded-user";

/// How long the console tells an operator's client to wait before retrying a reconnecting device.
const RETRY_AFTER_MS: u64 = 1_000;

/// The React console page, embedded as one self-contained file.
///
/// `/` and `/app` both serve it. The build is committed and gated, so a stale page cannot compile
/// and ship.
const REACT_PAGE: &str = include_str!("../web/dist/index.html");

/// How many sessions the store keeps.
///
/// One operator, a handful of browsers. Signing in mints a session for an Access-verified request
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
    /// The last screen captures, bounded and memory only: an activity row can open the image the
    /// device just sent, and a restart forgets every one of them.
    captures: crate::captures::Captures,
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
    /// When set, a request from exactly this peer is the operator named by [`PROXY_IDENTITY_HEADER`].
    ///
    /// The header alone is never enough: it is what a reverse proxy says, and the address is what
    /// makes the proxy the one entitled to say it.
    trusted_proxy: Option<IpAddr>,
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
                captures: crate::captures::Captures::new(),
                codes: Mutex::new(codes),
                channels: ChannelRegistry::new(),
                audit: Audit::new(&config.data, audit_max_bytes),
                denials: Denials::new(&config.data),
                mcp_timeout,
                now: Arc::new(now),
                started: Instant::now(),
                version: config.version.clone(),
                data: config.data.clone(),
                trusted_proxy: config.trusted_proxy,
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
            .route("/", get(root))
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
            .route("/health", get(health))
            .route("/app", get(react_page))
            .route("/auth", post(authenticate))
            // The one `/next` route that survives: `--login-link` prints a URL whose `code`
            // query the React page does not redeem, and `GET /` still sends that query here.
            // It redeems the code and lands the operator on the React page; there is no page of
            // its own left. Every other `/next` route is gone with the maud surface.
            .route("/next", get(login_link))
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
            // The page's live catalogue: the selected device's own `tools/list`, read over the same
            // channel and operator scope `/mcp` uses. The console keeps no copy and no cache; a
            // device that cannot answer is an offline state, never a fabricated list.
            .route("/fleet/:id/tools", get(live_tool_catalogue))
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
            // **THE TERMINAL'S OUTPUT AS ONE OPEN STREAM.** The page asked for each chunk with a
            // `terminal.read` long-poll, so every answer cost a request and the device had no reader
            // parked between them; a stream keeps the read parked and writes each answer down the
            // connection the moment it lands, which is the shape the device's own panel always had.
            .route("/terminal/:device/:session/stream", get(terminal_stream))
            .route("/audit", get(list_audit))
            // **THE ACTIVITY AS IT HAPPENS.** The page drew "Recent calls" from a fetch, so a call that
            // happened after the last refresh was invisible until the next one — no sense of the AI
            // working now. This is the same index, streamed: a snapshot of the newest lines, then one
            // event per line as it is written. It is the operator's own audit, so it needs the same
            // session `/audit` does.
            .route("/events", get(event_stream))
            // **THE FRAME ITSELF, NOT A DATA URL.** A screenshot can be megabytes; the event carries
            // a same-origin path and this route answers the bytes, behind the same operator gate the
            // page and `/audit` already use. The store is memory only, so an id that a restart or an
            // older capture forgot is a JSON 404, never a broken image.
            .route("/activity/shot/:device/:id", get(activity_shot))
            // The same read as `/audit`, over the unauthenticated-refusal log. Operator-only: the
            // log holds what probing looks like, and it is surfaced read-only.
            .route("/denials", get(list_denials))
            .fallback(not_found)
            .layer(axum::middleware::from_fn(no_store))
            .layer(axum::middleware::from_fn_with_state(
                self.inner.clone(),
                access_sign_in,
            ))
            .with_state(self.inner.clone())
    }
}

/// The extra facts a run's answer carries for the timeline's expanded row.
struct RunEvidence {
    /// The tail of stdout and stderr, when the run wrote anything.
    tail: Option<String>,
    /// The process's exit status, when the device reported one (a timeout has none).
    exit_code: Option<i64>,
    /// Whether the device killed the run at its timeout.
    timed_out: Option<bool>,
}

/// The last lines of a run's own output and its exit facts, bounded, for the timeline's expanded row.
///
/// The console already proxies this answer; showing the caller their own command's output is the
/// evidence a person wants when they click a run. The exit code and timeout are facts the output
/// alone cannot state — a run that printed nothing and exited 1 is not a run that succeeded.
/// Nothing is stored: the audit file still never carries a body.
fn run_evidence(reply: &Reply) -> Option<RunEvidence> {
    const TAIL: usize = 1600;
    let Reply::Result(result) = reply else {
        return None;
    };
    let body = result.get("result").unwrap_or(result);
    let text = body
        .get("content")?
        .as_array()?
        .first()?
        .get("text")?
        .as_str()?;
    let parsed: Value = serde_json::from_str(text).ok()?;
    let exit_code = parsed.get("exit_code").and_then(Value::as_i64);
    let timed_out = parsed.get("timed_out").and_then(Value::as_bool);
    let stdout = parsed
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let stderr = parsed
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut combined = String::new();
    if !stdout.trim().is_empty() {
        combined.push_str(stdout);
    }
    if !stderr.trim().is_empty() {
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str(stderr);
    }
    let tail = if combined.trim().is_empty() {
        None
    } else {
        let length = combined.chars().count();
        Some(if length > TAIL {
            let skip = length - TAIL;
            format!("…{}", combined.chars().skip(skip).collect::<String>())
        } else {
            combined
        })
    };
    Some(RunEvidence {
        tail,
        exit_code,
        timed_out,
    })
}

/// The base64 PNG a `screen.capture` answer carries, if it carries one at all.
///
/// The device's own tool answers a text content item holding `{format, data, width, height}`; an MCP
/// image content item (`content[0].data`) is accepted too, because that is the shape the protocol
/// names for an image and a future device may use it. The base64 is not decoded here: the capture
/// store validates it against the PNG signature before the image gets an id or a URL.
fn screen_png(reply: &Reply) -> Option<String> {
    let Reply::Result(result) = reply else {
        return None;
    };
    let body = result.get("result").unwrap_or(result);
    if body
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let first = body.get("content")?.as_array()?.first()?;
    if let Some(data) = first.get("data").and_then(Value::as_str) {
        return Some(data.to_owned());
    }
    let text = first.get("text")?.as_str()?;
    let parsed: Value = serde_json::from_str(text).ok()?;
    parsed.get("data")?.as_str().map(str::to_owned)
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

/// One proxied call, as the live stream describes it.
///
/// `detail` is the failure sentence the evidence panel shows — the device's own message, or the
/// console's reason for giving up — and `ok` is not the status: a device's JSON-RPC error is HTTP
/// 200 and still a refusal. The audit line keeps only the numeric code; this is what a person reads.
struct LiveCall<'a> {
    device_id: Option<&'a str>,
    tool: Option<&'a str>,
    sentence: &'a str,
    status: StatusCode,
    ok: bool,
    detail: &'a str,
    ms: u64,
    evidence: Option<Value>,
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
    /// The operator behind this request, including the reverse proxy that may have let it in.
    ///
    /// Access is checked only when the operator configured it, and a token that does not verify falls
    /// through to the session check — the feature can let a request in, never let one skip a check.
    fn operator_from(&self, headers: &HeaderMap, peer: Option<IpAddr>) -> Option<Operator> {
        self.identity_from(headers, peer).0
    }

    /// The operator, and the reason there is none when Access was the intended identity.
    ///
    /// One verification serves both callers: the routes only need `operator`, and `/session` needs
    /// the report so the sign-in page can say *why* it is asking for a token.
    fn identity(&self, headers: &HeaderMap) -> (Option<Operator>, AccessReport) {
        self.identity_from(headers, None)
    }

    /// [`Inner::identity`] with the connection's peer address: the only shape that can honor
    /// `--trusted-proxy`, because a header is only as good as the address that sent it.
    fn identity_from(
        &self,
        headers: &HeaderMap,
        peer: Option<IpAddr>,
    ) -> (Option<Operator>, AccessReport) {
        let header = crate::access::token_from_headers(headers).is_some();
        let Some(access) = &self.access else {
            return (
                self.session_operator_from(headers, peer),
                AccessReport {
                    configured: false,
                    header,
                    error: None,
                },
            );
        };
        let Some(token) = crate::access::token_from_headers(headers) else {
            return (
                self.session_operator_from(headers, peer),
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
                self.session_operator_from(headers, peer),
                AccessReport {
                    configured: true,
                    header: true,
                    error: Some(error.to_string()),
                },
            ),
        }
    }

    /// The operator this request names: the reverse proxy's header first, then a live session.
    ///
    /// The proxy is checked first because a deployment that trusts one has already established the
    /// identity at the edge; a browser that also happens to hold a session cookie is the same person.
    fn session_operator_from(&self, headers: &HeaderMap, peer: Option<IpAddr>) -> Option<Operator> {
        if let Some(operator) = self.proxy_operator(headers, peer) {
            return Some(operator);
        }
        // BOTH WAYS IN, ONE SESSION STORE. The header is what an API client presents; the cookie is
        // what the page has, because a page that has to hold its own token in JavaScript loses it on
        // every reload and asks the operator for a 64-character secret again.
        if self.session_valid(headers) || cookie_session(self, headers).is_some() {
            return Some(Operator {
                email: None,
                via: "session",
            });
        }
        None
    }

    /// The operator the reverse proxy named, when it is the proxy that is asking.
    ///
    /// Both facts are required: the peer address must be exactly the address `--trusted-proxy`
    /// names, and the header must be non-empty. Without the flag this is always `None`, so a console
    /// that was never told to trust a header cannot be talked into trusting one.
    fn proxy_operator(&self, headers: &HeaderMap, peer: Option<IpAddr>) -> Option<Operator> {
        let trusted = self.trusted_proxy?;
        if peer != Some(trusted) {
            return None;
        }
        let login = headers.get(PROXY_IDENTITY_HEADER)?.to_str().ok()?.trim();
        if login.is_empty() {
            return None;
        }
        Some(Operator {
            email: Some(login.to_owned()),
            via: "proxy",
        })
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
        let ts_ms = (self.now)();
        self.audit
            .append(&audit::Entry {
                ts_ms,
                kind: facts.kind,
                device_id: facts.device_id,
                status: facts.status.as_u16(),
                detail: facts.detail,
                tool: facts.tool,
                token_id: facts.token_id,
            })
            .await;
        // A call's live event is published by the proxy, which has the call's arguments; everything
        // else (a grant, a token, a refusal) is described from what this line already holds. These
        // events are live too: a row a page just watched arrive is not a row read back from the
        // audit file, even when the tool behind it has no evidence to show.
        if facts.kind != "mcp" {
            let mut event = json!({
                "ts_ms": ts_ms,
                "kind": facts.kind,
                "status": facts.status.as_u16(),
                "live": true,
            });
            if let Some(device_id) = facts.device_id {
                event["device_id"] = json!(device_id);
            }
            if let Some(tool) = facts.tool {
                event["tool"] = json!(tool);
            }
            if let Some(detail) = facts.detail {
                event["detail"] = json!(detail);
            }
            let sentence = crate::activity::line_sentence(&event);
            event["sentence"] = json!(sentence);
            self.audit.publish(event);
        }
    }

    /// Publish one proxied call to the live stream: the sentence, the tool, the device, the outcome,
    /// why it failed, and how long the device took.
    fn publish_call(&self, call: LiveCall<'_>) {
        let mut event = json!({
            "ts_ms": (self.now)(),
            "kind": "mcp",
            "status": call.status.as_u16(),
            "ok": call.ok,
            "live": true,
            "ms": call.ms,
            "sentence": call.sentence,
        });
        if let Some(device_id) = call.device_id {
            event["device_id"] = json!(device_id);
        }
        if let Some(tool) = call.tool {
            event["tool"] = json!(tool);
        }
        if !call.detail.is_empty() {
            event["detail"] = json!(call.detail);
        }
        if let Some(evidence) = call.evidence {
            event["evidence"] = evidence;
        }
        self.audit.publish(event);
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

/// The outcome of a `runs.exec`, as the audit keeps it: the run id, its exit and how long it took.
///
/// The output is deliberately not copied here: the device's history never stored it, and the audit
/// is a record of calls, not a second run log. This is the metadata an operator can use to find the
/// run in the Runs page; `runs.get` is still where the record lives.
fn run_detail(reply: &Reply) -> Option<String> {
    let Reply::Result(result) = reply else {
        return reply_detail(reply);
    };
    let text = result.get("content")?.get(0)?.get("text")?.as_str()?;
    let run: Value = serde_json::from_str(text).ok()?;
    let id = run.get("id")?.as_str()?;
    let elapsed = run.get("duration_ms").and_then(Value::as_u64);
    if run
        .get("timed_out")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Some(match elapsed {
            Some(ms) => format!("run {id} timed out after {ms} ms"),
            None => format!("run {id} timed out"),
        });
    }
    match run.get("exit_code").and_then(Value::as_i64) {
        Some(code) => Some(match elapsed {
            Some(ms) => format!("run {id} exited {code} in {ms} ms"),
            None => format!("run {id} exited {code}"),
        }),
        None => Some(format!("run {id} ended without an exit code")),
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
        // The peer address is what makes a proxy's claim believable. Axum's connect-info service
        // inserts it; a request cannot carry it in its headers, which is the whole point.
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| address.ip());
        match state.operator_from(&parts.headers, peer) {
            Some(_) => Ok(OperatorSession),
            None => Err(state.refuse(None, ApiError::unauthorized()).await),
        }
    }
}

/// A refusal the console renders as `{"error": "<code>"}`, plus an optional sentence a page can
/// show verbatim. The sentence is additive: callers that switch on `error` are unchanged, and a
/// refusal whose next step is not obvious from its code carries the action in words.
pub(crate) struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: Option<String>,
}

impl ApiError {
    fn unauthorized() -> Self {
        ApiError {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: None,
        }
    }

    fn forbidden(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::FORBIDDEN,
            code,
            message: None,
        }
    }

    fn bad_request(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::BAD_REQUEST,
            code,
            message: None,
        }
    }

    fn not_found(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::NOT_FOUND,
            code,
            message: None,
        }
    }

    fn too_many(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            code,
            message: None,
        }
    }

    fn unavailable(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code,
            message: None,
        }
    }

    fn gateway_timeout(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::GATEWAY_TIMEOUT,
            code,
            message: None,
        }
    }

    fn internal() -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal",
            message: None,
        }
    }

    fn internal_code(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code,
            message: None,
        }
    }

    /// Attach the sentence a person reads. It names the next action, never only the failure.
    fn saying(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = json!({ "error": self.code });
        if let Some(message) = self.message {
            body["message"] = json!(message);
        }
        (self.status, Json(body)).into_response()
    }
}

#[derive(Deserialize)]
struct AuthRequest {
    /// The operator token, typed into the sign-in form.
    #[serde(default)]
    token: Option<String>,
    /// A one-time code from `--login-link`, which the link puts in the URL.
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
    /// the sign-in asks for a token: with this false, it always will.
    access_configured: bool,
    /// The deepest any per-device outbound queue has been since the console started. It is a bound
    /// an operator can see, not a secret.
    max_queued_frames: usize,
    /// The label the deploy passed, or the crate version for a development start.
    version: String,
}

/// The static assets behind `/assets/`.
mod assets {
    /// The brand mark at 128 px: the browser tab's icon and the console's face in a task switcher.
    /// The React build already points at `/assets/icon.png`, so this route is the page's, not the
    /// deleted surface's.
    pub(super) const ICON: &[u8] = include_bytes!("../assets/icon.png");
}

/// `GET /`: the React workspace. A `code` query still goes to `/next`, because `--login-link`
/// puts a one-time code there and the React page does not redeem it; dropping the query would
/// turn a working link into a page with no explanation. Every other query stays on the React
/// page: a cache-bust or a shared link must not become a redirect.
async fn root(uri: axum::http::Uri) -> Response {
    let login_code = uri.query().is_some_and(|query| {
        query
            .split('&')
            .any(|part| part == "code" || part.starts_with("code="))
    });
    if login_code {
        Redirect::to(&format!("/next?{}", uri.query().unwrap_or_default())).into_response()
    } else {
        Html(REACT_PAGE).into_response()
    }
}

#[derive(Deserialize)]
struct LoginLinkQuery {
    /// The one-time code `--login-link` put in the URL; spent, never displayed.
    code: Option<String>,
}

/// `GET /next`: the one route of the deleted maud surface that still has a caller.
///
/// `--login-link` prints `<public-url>?code=…`, and `GET /` forwards that `code` query here
/// because the React page does not redeem it. The redemption is the whole route now: it spends
/// the code, sets the same cookie the React page reads, and lands on `/`. A request without a
/// code (a stale bookmark) simply goes home. There is no page here, and nothing else under
/// `/next` exists.
async fn login_link(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LoginLinkQuery>,
) -> Response {
    let Some(code) = query.code.as_deref() else {
        return Redirect::to("/").into_response();
    };
    if let Err(reason) = crate::store::spend_login_code(&app.data, code, (app.now)()) {
        eprintln!("summrise-console: refused a login code: {reason}");
        return (
            StatusCode::UNAUTHORIZED,
            "That sign-in link was not accepted.",
        )
            .into_response();
    }
    let (_session, token) = new_session(&app, "login-link").await;
    let mut response = Redirect::to("/").into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(&token, secure_request(&headers)),
    );
    response
}

/// Every answer this console gives is for one request, and a cache that disagrees is how a browser
/// keeps running yesterday's page — or yesterday's `{"authenticated":false}` — after a deploy or a
/// login. One layer says so instead of a list of routes that has to stay complete as routes are added.
///
/// The one exception is a screen capture: it answers its own `private, max-age=60`, because the image
/// is the operator's own and a minute of browser cache saves the second fetch that opening the same
/// row again would cost.
async fn no_store(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let is_shot = request.uri().path().starts_with("/activity/shot/");
    let mut response = next.run(request).await;
    // Only a served image keeps its own header. A 401 or a 404 on the same path is no-store like
    // everything else, so a refusal is never what a browser shows after the operator signs in.
    if !(is_shot && response.status() == StatusCode::OK) {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        );
    }
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
/// The React page reads the Access header on `/session` directly, but `/next` still benefits: an
/// old bookmark arrives at the login-link shim already identified, and this mints the same cookie
/// the shim would otherwise make the browser earn again. The answer sets the cookie for the next
/// request; injecting it rather than redirecting keeps a browser that refuses cookies from
/// looping between the cookie and the page.
///
/// GET `/next` only: a cross-site form POST that the edge happens to authenticate must not be able
/// to mint a session, and the API's bearer tokens are not cookies.
async fn access_sign_in(
    State(app): State<AppState>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let path = request.uri().path();
    let is_page = request.method() == axum::http::Method::GET && path == "/next";
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
/// `GET /app`: the React console page, one embedded file. It carries no data of its own; the page
/// authenticates with the console's own session and calls the JSON routes that already exist.
async fn react_page() -> Html<&'static str> {
    Html(REACT_PAGE)
}

/// Any path the router does not own: a 404. The React page owns its own routes, so the console
/// does not render a fallback page for a path it does not serve.
async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

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

async fn session_probe(
    State(app): State<AppState>,
    headers: HeaderMap,
    peer: Option<ConnectInfo<SocketAddr>>,
) -> Json<SessionProbe> {
    let (operator, access) =
        app.identity_from(&headers, peer.map(|ConnectInfo(address)| address.ip()));
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
async fn logout(State(app): State<AppState>, _: OperatorSession, headers: HeaderMap) -> Response {
    // Either credential identifies the session to end; the cookie is also cleared so the browser
    // stops presenting a session the console has already forgotten.
    let token_hash = bearer_secret(&headers)
        .map(|presented| hash_of(&presented))
        .or_else(|| cookie_session(&app, &headers).map(|session| session.token_hash));
    if let Some(token_hash) = token_hash {
        if let Err(error) = end_session(&app, &token_hash) {
            eprintln!("summrise-console: the sessions could not be persisted: {error}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        clear_session_cookie(secure_request(&headers)),
    );
    response
}

/// Verify the operator's secret: a typed token, or a one-time login code. `/auth` is the caller,
/// and the denial line is written here so every sign-in failure records the same thing.
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
    headers: HeaderMap,
    Json(request): Json<AuthRequest>,
) -> Result<Response, ApiError> {
    operator_from_credentials(&app, request.code.as_deref(), request.token.as_deref()).await?;
    // A session is a thing that happened. It has no device, so it goes in as `auth` with the
    // detail a reader needs and nothing about the credential itself.
    let (session, token) = new_session(&app, "token").await;
    // THE COOKIE IS THE SESSION, and the token in the body is for clients that already speak the
    // header. Keeping a bearer in the page's JavaScript was the half-step the React port shipped
    // with: it costs a re-typed operator token on every reload, and the design's target is an
    // HttpOnly cookie the page cannot read and does not have to remember.
    let mut response = Json(AuthResponse {
        token: hex::encode(&token),
        expires_at_ms: session.expires_at_ms,
    })
    .into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(&token, secure_request(&headers)),
    );
    Ok(response)
}

/// The cookie name the page uses. The console's own session, for the page and for the JSON API.
const SESSION_COOKIE: &str = "summrise_session";

fn cookie_session(app: &Inner, headers: &HeaderMap) -> Option<Session> {
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
        "{SESSION_COOKIE}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
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
    let mut cookie = format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    if secure {
        cookie.push_str("; Secure");
    }
    HeaderValue::from_str(&cookie).expect("a valid Set-Cookie")
}

fn terminal_refusal(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// The sentence a refused [`device_tool`] carried, for the terminal stream's `fault` event.
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

// ---- one open stream per terminal ---------------------------------------------------------------

/// How many terminal streams one console carries at once.
///
/// A stream is a parked `terminal.read` and one HTTP connection; the cap is what keeps a page that
/// reloads in a loop, or a dozen browser tabs, from parking a hundred device reads at once.
pub(crate) const MAX_TERMINAL_STREAMS: usize = 64;

/// How long one stream's device read parks before its answer comes back empty.
///
/// It has to fit inside [`crate::channels::REPLY_TIMEOUT`] (30 s) or every quiet stream would turn
/// into a 504 the moment the device went quiet; 20 s leaves the channel time to answer.
const TERMINAL_STREAM_WAIT_MS: u64 = 20_000;

/// The pause between reads for a device older than `wait_ms`.
///
/// A read with no wait returns whatever is buffered and returns at once, so a stream against such a
/// device would spin; this is the page's old 250 ms poll, tightened because the console — not a
/// browser over a WAN — is the caller.
const TERMINAL_STREAM_GAP: Duration = Duration::from_millis(120);

/// The live terminal streams, process-wide. A slot is held by the stream itself and released when
/// the client disconnects or the session ends.
static TERMINAL_STREAMS: AtomicUsize = AtomicUsize::new(0);

/// A slot in the stream pool, released on drop.
struct StreamSlot;

impl StreamSlot {
    fn acquire() -> Option<Self> {
        let previous = TERMINAL_STREAMS.fetch_add(1, Ordering::SeqCst);
        if previous < MAX_TERMINAL_STREAMS {
            Some(Self)
        } else {
            TERMINAL_STREAMS.fetch_sub(1, Ordering::SeqCst);
            None
        }
    }
}

impl Drop for StreamSlot {
    fn drop(&mut self) {
        TERMINAL_STREAMS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// `GET /terminal/:device/:session/stream?from=N`: the session's output as server-sent events.
///
/// Three events cross it. `output` carries the bytes since the cursor, exactly what `terminal.read`
/// answered; `end` says the session ended (with the device's exit facts when it gave them); `fault`
/// carries the sentence a refused read produced, which is the same one the page showed when a poll
/// failed. A stream that ends without `end` is the link, not the session, and the page reconnects
/// from its cursor.
///
/// **THE CURSOR IS THE PAGE'S**, not the console's: `from` is where its xterm's output ends, so a
/// reconnect after a dropped connection replays only what was missed and never duplicates a byte.
async fn terminal_stream(
    State(app): State<AppState>,
    _: OperatorSession,
    Path((device_id, session_id)): Path<(String, String)>,
    Query(query): Query<TerminalStreamQuery>,
) -> Response {
    let Some(session_id) = as_session_id(&session_id) else {
        return (
            StatusCode::BAD_REQUEST,
            "a session id is letters, digits, - and _",
        )
            .into_response();
    };
    if let Err(error) = ensure_registered(&app, &device_id) {
        return app
            .deny(audit_device_id(&device_id).as_deref(), error)
            .await
            .into_response();
    }
    let Some(slot) = StreamSlot::acquire() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::RETRY_AFTER, HeaderValue::from_static("2"))],
            "too many terminal streams are open",
        )
            .into_response();
    };

    // **A DEVICE THAT CAN STREAM IS FOLLOWED, NOT POLLED.** The capability is advertised in its host
    // frame; a device that does not name it answers no subscribe frame at all, so the console would
    // wait out its reply bound on every attach. Without the capability, the parked-read pump below is
    // the stream, and the page cannot tell the two apart.
    if app.channels.has_cap(&device_id, "terminal.stream") {
        let scope = operator_scope(&device_id);
        let read = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "terminal.read",
                "arguments": { "id": session_id, "cursor": query.from },
            },
        });
        if let Ok((sub, stream, ack)) = app
            .channels
            .subscribe(
                &device_id,
                session_id,
                query.from,
                read,
                &RequestAuth::Token(&scope),
            )
            .await
        {
            return subscription_stream(app, device_id, sub, stream, ack, slot);
        }
    }

    let state = TerminalStreamState {
        app,
        device: device_id,
        session: session_id.to_owned(),
        cursor: query.from,
        wait: true,
        done: false,
        _slot: slot,
    };
    // `unfold` is the loop: one item per event, and the state carries the cursor (and the slot)
    // from one item to the next. Nothing is written to the device here that the page could not have
    // asked for with `terminal.read` — the read, its authorization and its audit are the same ones.
    let events = futures_util::stream::unfold(state, |mut state| async move {
        loop {
            if state.done {
                return None;
            }
            let mut arguments = json!({ "id": state.session, "cursor": state.cursor });
            if state.wait {
                arguments["wait_ms"] = json!(TERMINAL_STREAM_WAIT_MS);
            }
            match terminal_tool(&state.app, &state.device, "terminal.read", arguments).await {
                Ok(answer) => {
                    if let Some(next) = answer.get("cursor").and_then(Value::as_u64) {
                        state.cursor = next;
                    }
                    let data = answer
                        .get("data")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    if !data.is_empty() {
                        let payload = json!({ "data": data, "cursor": state.cursor });
                        return Some((
                            Ok::<Event, Infallible>(
                                Event::default().event("output").data(payload.to_string()),
                            ),
                            state,
                        ));
                    }
                    if answer.get("alive").and_then(Value::as_bool) == Some(false) {
                        let payload = json!({
                            "cursor": state.cursor,
                            "exit_code": answer.get("exit_code").cloned().unwrap_or(Value::Null),
                            "signal": answer.get("signal").cloned().unwrap_or(Value::Null),
                        });
                        state.done = true;
                        return Some((
                            Ok(Event::default().event("end").data(payload.to_string())),
                            state,
                        ));
                    }
                    if !state.wait {
                        // No wait field was accepted, so the read returned at once and empty.
                        tokio::time::sleep(TERMINAL_STREAM_GAP).await;
                    }
                }
                Err(response) => {
                    let status = response.status();
                    let sentence = refusal_sentence(*response).await;
                    // A device from before `wait_ms`: drop the field once and keep the stream, the
                    // same fallback the page made when it was the one calling `terminal.read`.
                    if state.wait && refuses_wait_ms(&sentence) {
                        state.wait = false;
                        continue;
                    }
                    let payload = json!({ "sentence": sentence, "status": status.as_u16() });
                    state.done = true;
                    return Some((
                        Ok(Event::default().event("fault").data(payload.to_string())),
                        state,
                    ));
                }
            }
        }
    });

    let mut response = Sse::new(events)
        .keep_alive(KeepAlive::default().interval(Duration::from_secs(15)))
        .into_response();
    // A reverse proxy that buffers would hold every event until the stream ended, which is the one
    // way this route could be worse than the polling it replaces. `no-store` (the console's
    // middleware) is not enough: nginx honors this header and nothing else.
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

/// The stream's one query parameter.
#[derive(Deserialize)]
struct TerminalStreamQuery {
    /// The byte offset the page's terminal already shows; a reconnect resumes there.
    #[serde(default)]
    from: u64,
}

/// The cursor and the slot, carried from one event to the next.
struct TerminalStreamState {
    app: AppState,
    device: String,
    session: String,
    cursor: u64,
    /// Whether the device accepts `wait_ms`; a refusal switches this off for the stream's life.
    wait: bool,
    /// Set once a terminal event has been produced, so the next poll of the stream ends it.
    done: bool,
    /// Released when the stream is dropped.
    _slot: StreamSlot,
}

/// One terminal stream streamed by the device: the ack already in hand, then pushed frames.
struct TerminalSubscription {
    app: AppState,
    device: String,
    sub: u64,
    /// The pushed frames, in order.
    stream: tokio::sync::mpsc::Receiver<Value>,
    /// The ack's read, emitted as the stream's first event.
    first: Option<Value>,
    done: bool,
    /// Released when the stream is dropped, after the subscription is stopped.
    _slot: StreamSlot,
}

impl Drop for TerminalSubscription {
    fn drop(&mut self) {
        // The page went away (or the session ended): stop the device following, and drop the sink
        // so any frame already in flight is discarded instead of queued for nobody.
        self.app.channels.unsubscribe(&self.device, self.sub);
    }
}

/// Serve a subscribed terminal as SSE.
fn subscription_stream(
    app: AppState,
    device: String,
    sub: u64,
    stream: tokio::sync::mpsc::Receiver<Value>,
    ack: Reply,
    slot: StreamSlot,
) -> Response {
    let first = match &ack {
        Reply::Result(result) => result_answer(result),
        // A refused subscribe is the same sentence a refused read showed; there is no stream to
        // serve, so the connection carries the fault and ends.
        Reply::Error(error) => Some(json!({ "sentence": reply_error_sentence(error) })),
    };
    let state = TerminalSubscription {
        app,
        device,
        sub,
        stream,
        first,
        done: false,
        _slot: slot,
    };
    let events = futures_util::stream::unfold(state, |mut state| async move {
        if state.done {
            return None;
        }
        if let Some(first) = state.first.take() {
            let event = if first.get("sentence").is_some() {
                state.done = true;
                let payload = json!({
                    "sentence": first
                        .get("sentence")
                        .and_then(Value::as_str)
                        .unwrap_or("the device refused the subscription"),
                    "status": 502,
                });
                Event::default().event("fault").data(payload.to_string())
            } else {
                match read_event(&first) {
                    Some(event) => event,
                    None => return None,
                }
            };
            return Some((Ok::<Event, Infallible>(event), state));
        }
        match state.stream.recv().await {
            Some(frame) => {
                if let Some(sentence) = frame.get("error").and_then(Value::as_str) {
                    state.done = true;
                    let payload = json!({ "sentence": sentence, "status": 502 });
                    return Some((
                        Ok::<Event, Infallible>(
                            Event::default().event("fault").data(payload.to_string()),
                        ),
                        state,
                    ));
                }
                read_event(&frame).map(|event| (Ok::<Event, Infallible>(event), state))
            }
            None => None,
        }
    });
    let mut response = Sse::new(events)
        .keep_alive(KeepAlive::default().interval(Duration::from_secs(15)))
        .into_response();
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

/// One read-shaped answer (`{data, cursor, alive, exit_code, signal}`) as an SSE event.
fn read_event(answer: &Value) -> Option<Event> {
    let cursor = answer.get("cursor").and_then(Value::as_u64);
    let data = answer
        .get("data")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !data.is_empty() {
        let payload = json!({ "data": data, "cursor": cursor });
        return Some(Event::default().event("output").data(payload.to_string()));
    }
    if answer.get("alive").and_then(Value::as_bool) == Some(false) {
        let payload = json!({
            "cursor": cursor,
            "exit_code": answer.get("exit_code").cloned().unwrap_or(Value::Null),
            "signal": answer.get("signal").cloned().unwrap_or(Value::Null),
        });
        return Some(Event::default().event("end").data(payload.to_string()));
    }
    // An empty read that is still alive carries no event; the caller asks again.
    None
}

/// The read answer inside one MCP tool result.
///
/// A device wraps its answer in the JSON-RPC envelope it would send an MCP client, so the tool's
/// text sits under `result.result.content`; a bare `{content, isError}` is accepted too.
fn result_answer(result: &Value) -> Option<Value> {
    let body = result.get("result").unwrap_or(result);
    let text = body
        .get("content")?
        .as_array()?
        .first()?
        .get("text")?
        .as_str()?;
    serde_json::from_str(text).ok()
}

/// The sentence inside a device's JSON-RPC error envelope.
fn reply_error_sentence(error: &Value) -> String {
    error
        .get("error")
        .and_then(|body| body.get("message"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| error.to_string())
}

/// Whether a refusal is the device not knowing `wait_ms`, which is a version fact, not a fault.
fn refuses_wait_ms(sentence: &str) -> bool {
    let lower = sentence.to_ascii_lowercase();
    [
        "wait_ms",
        "unknown field",
        "unknown argument",
        "additional propert",
        "input schema",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
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

    // The static registry is accepted offline, exactly as before. A name outside it is a plugin
    // tool, and a plugin tool is a device fact: only the device that advertises it can vouch for it.
    let plugin_tools: Vec<String> = request
        .tools
        .iter()
        .filter(|tool| summrise_core::registry::tool(tool).is_none())
        .cloned()
        .collect();
    let mut advertised_tools = std::collections::BTreeSet::new();
    if let Some(tool) = plugin_tools.first() {
        match &scope {
            // An aggregate token's scope is one list applied to every device, and v1 keeps it on
            // the static registry. A plugin tool is per-device, so this refusal says so.
            crate::tokens::TokenScope::Devices(_) => {
                let refusal = ApiError::bad_request("unknown_tool").saying(format!(
                    "plugin tools are per-device in v1; mint a token for the device that advertises \
                     {tool}"
                ));
                return Err(app.deny(audit_id.as_deref(), refusal).await);
            }
            crate::tokens::TokenScope::Device(device_id) => {
                if !known.contains(device_id) {
                    return Err(app
                        .deny(audit_id.as_deref(), ApiError::not_found("unknown_device"))
                        .await);
                }
                // The same read the catalogue route makes: the device's own `tools/list`, with the
                // operator's scope. No cache, so the check sees what the device advertises now.
                let live = match read_live_tools(&app, device_id).await {
                    Ok(live) => live,
                    Err(LiveToolsError::Unavailable) => {
                        let refusal = ApiError::bad_request("unknown_tool").saying(format!(
                            "the machine is not connected, so the console cannot check {tool}; \
                             connect the machine, then mint the token"
                        ));
                        return Err(app.deny(audit_id.as_deref(), refusal).await);
                    }
                    Err(LiveToolsError::Timeout) => {
                        let refusal = ApiError::bad_request("unknown_tool").saying(format!(
                            "the device did not answer while the console checked {tool}; connect \
                             the machine, then mint the token"
                        ));
                        return Err(app.deny(audit_id.as_deref(), refusal).await);
                    }
                    Err(LiveToolsError::Malformed) => {
                        let refusal = ApiError::bad_request("unknown_tool").saying(
                            "the device sent a tool list this console cannot read; update the \
                             device, then mint the token",
                        );
                        return Err(app.deny(audit_id.as_deref(), refusal).await);
                    }
                };
                let advertised: std::collections::BTreeSet<&str> = live
                    .iter()
                    .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                    .collect();
                for tool in &plugin_tools {
                    if !advertised.contains(tool.as_str()) {
                        let refusal = ApiError::bad_request("unknown_tool").saying(format!(
                            "the device does not advertise {tool}; connect it and try again"
                        ));
                        return Err(app.deny(audit_id.as_deref(), refusal).await);
                    }
                    advertised_tools.insert(tool.clone());
                }
            }
        }
    }

    let created = lock(&app.tokens).create(
        crate::tokens::Draft {
            name: request.name,
            scope,
            tools: request.tools,
            advertised_tools,
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
                TokenError::UnknownTool => ApiError::bad_request("unknown_tool").saying(
                    "that tool is not in this build's registry; pick a tool the device advertises",
                ),
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
/// The arguments object of a `tools/call`, or null for any other method. The live stream describes
/// a call from these; nothing here is written to the audit file.
fn requested_arguments(request: &Value) -> &Value {
    request
        .get("params")
        .and_then(|params| params.get("arguments"))
        .unwrap_or(&Value::Null)
}

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

/// The operator's own scope on one device: every tool, destructive allowed.
///
/// The operator session is the credential on `/mcp`, and the live catalogue is the same operator
/// asking the same device a read-only question. One shape, built once, so the catalogue cannot
/// become a second authorization path with weaker rules than the proxy.
fn operator_scope(device_id: &str) -> crate::channels::TokenScope {
    crate::channels::TokenScope {
        token_id: "operator".to_owned(),
        device_id: device_id.to_owned(),
        scopes: vec!["*".to_owned()],
        allow_destructive: true,
        expires_at_ms: None,
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

/// The message a device's JSON-RPC error carries, when it is one.
///
/// The console forwards the error envelope whole, so the message sits under `error`; the direct
/// shape is accepted too, because a caller reading only the message should not depend on the wrap.
fn error_message(reply: &Reply) -> Option<&str> {
    let Reply::Error(error) = reply else {
        return None;
    };
    error
        .get("error")
        .and_then(|body| body.get("message"))
        .or_else(|| error.get("message"))
        .and_then(Value::as_str)
}

/// A device's message, bounded for one row's evidence: a panel is not a log.
fn bounded_detail(message: &str) -> String {
    message.chars().take(64).collect()
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
    let started = Instant::now();
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
    // The sentence a person reads, built from the call's own arguments — never a guess. It is made
    // once, before the wait, so every outcome publishes the same description of the same call.
    let sentence = match tool.as_deref() {
        Some(tool) => crate::activity::describe(tool, requested_arguments(&request)),
        None => "Listed the machine's tools".to_owned(),
    };
    let publish_sentence = sentence.clone();
    // What the page shows when the row is opened: the call's own arguments, bounded, plus the run's
    // output tail when there is one. Built once, before the wait, like the sentence.
    let mut evidence = tool
        .as_deref()
        .and_then(|tool| crate::activity::detail(tool, requested_arguments(&request)));

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
            app.publish_call(LiveCall {
                device_id: audit_id.as_deref(),
                tool: tool.as_deref(),
                sentence: &publish_sentence,
                status: StatusCode::GATEWAY_TIMEOUT,
                ok: false,
                detail: "device timeout",
                ms: started.elapsed().as_millis() as u64,
                evidence: evidence.clone(),
            });
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
            app.publish_call(LiveCall {
                device_id: audit_id.as_deref(),
                tool: tool.as_deref(),
                sentence: &publish_sentence,
                status: failure.status,
                ok: false,
                detail: failure.detail,
                ms: started.elapsed().as_millis() as u64,
                evidence: evidence.clone(),
            });
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
            // There is no ninety-second hold: the token's consent was the decision, and a call it
            // did not authorize is refused now, with a sentence that says how to authorize it.
            if error_code(&reply) == Some(-32002) {
                if let Reply::Error(error) = &mut reply {
                    if let Some(inner) = error.get_mut("error") {
                        inner["message"] = json!(
                            "this token does not allow destructive calls; mint one with destructive consent to run it"
                        );
                    }
                }
                app.publish_call(LiveCall {
                    device_id: audit_id.as_deref(),
                    tool: tool.as_deref(),
                    sentence: &publish_sentence,
                    status: StatusCode::FORBIDDEN,
                    ok: false,
                    detail: "destructive_without_consent",
                    ms: started.elapsed().as_millis() as u64,
                    evidence: evidence.clone(),
                });
                app.record_facts(AuditFacts {
                    kind: "denied",
                    device_id: audit_id.as_deref(),
                    status: StatusCode::FORBIDDEN,
                    detail: Some("destructive_without_consent"),
                    tool: tool.as_deref(),
                    token_id,
                })
                .await;
                return Ok(reply);
            }
            if let (Some(allowed), true) = (filter, is_tools_list) {
                if let Reply::Result(result) = &mut reply {
                    crate::mcp::filter_tools(result, allowed);
                }
            }
            // The audit keeps the outcome as a code; the live event also carries the device's own
            // sentence, because the panel is where a person reads why a call was refused.
            let detail = if tool.as_deref() == Some("runs.exec") {
                run_detail(&reply)
            } else {
                reply_detail(&reply)
            };
            let (ok, live_detail) = match &reply {
                Reply::Result(_) => (true, detail.clone().unwrap_or_default()),
                Reply::Error(_) => (
                    false,
                    error_message(&reply)
                        .map(bounded_detail)
                        .unwrap_or_else(|| detail.clone().unwrap_or_default()),
                ),
            };
            if tool.as_deref() == Some("runs.exec") {
                // The evidence object is the page's, not the audit's: a run with no arguments still
                // gets one so its exit facts have somewhere to live.
                if let Some(run) = run_evidence(&reply) {
                    let evidence = evidence.get_or_insert_with(|| json!({}));
                    if let Some(object) = evidence.as_object_mut() {
                        if let Some(output) = run.tail {
                            object.insert("output".to_owned(), json!(output));
                        }
                        if let Some(exit_code) = run.exit_code {
                            object.insert("exit_code".to_owned(), json!(exit_code));
                        }
                        if let Some(timed_out) = run.timed_out {
                            object.insert("timed_out".to_owned(), json!(timed_out));
                        }
                    }
                }
            }
            // A screenshot is evidence a row can open, not a data URL in an event: the image stays
            // in the console's memory, and the event names the path and the moment. A device answer
            // that is not a PNG is still the device's result, but it gets no `shot` it cannot serve.
            if ok && tool.as_deref() == Some("screen.capture") {
                if let (Some(device), Some(encoded)) = (audit_id.as_deref(), screen_png(&reply)) {
                    let captured_at_ms = (app.now)();
                    if let Some(image_id) = app.captures.remember(device, &encoded) {
                        let evidence = evidence.get_or_insert_with(|| json!({}));
                        if let Some(object) = evidence.as_object_mut() {
                            object.insert(
                                "shot".to_owned(),
                                json!(format!("/activity/shot/{device}/{image_id}")),
                            );
                            object.insert("captured_at_ms".to_owned(), json!(captured_at_ms));
                        }
                    }
                }
            }
            app.publish_call(LiveCall {
                device_id: audit_id.as_deref(),
                tool: tool.as_deref(),
                sentence: &publish_sentence,
                status: StatusCode::OK,
                ok,
                detail: &live_detail,
                ms: started.elapsed().as_millis() as u64,
                evidence,
            });
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
    let operator_scope = operator_scope(&body.device_id);
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

/// `GET /fleet/:id/tools`: the selected device's live `tools/list`, classified by the console.
///
/// The device is the authority on what it advertises. The console adds only the two facts a page
/// cannot derive: where the tool comes from (`builtin`, or the plugin namespace) and, for a
/// built-in, the capability group the registry declares it under. A read never falls back to the
/// static registry: an offline device is an offline state, not a shorter list.
async fn live_tool_catalogue(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    _: OperatorSession,
) -> Response {
    if !app.has_device(&device_id) {
        return ApiError::not_found("unknown_device").into_response();
    }
    match read_live_tools(&app, &device_id).await {
        Ok(tools) => match catalogue_from_tools(&tools) {
            Ok(tools) => (StatusCode::OK, Json(json!({ "tools": tools }))).into_response(),
            Err(()) => LiveToolsError::Malformed.refusal().into_response(),
        },
        Err(error) => error.refusal().into_response(),
    }
}

/// One tool as `GET /fleet/:id/tools` serves it: the device's advertisement, plus the console's
/// source/capability classification. Field names are the fixed wire contract the page reads.
#[derive(Debug, PartialEq, serde::Serialize)]
struct LiveToolView {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
    source: String,
    capability: String,
    destructive: bool,
}

/// Why a live `tools/list` read produced no list.
#[derive(Debug, PartialEq, Eq)]
enum LiveToolsError {
    /// No channel, a queue at its cap, or a channel that closed: nothing was read.
    Unavailable,
    /// The device took the frame and did not answer inside the console's bound.
    Timeout,
    /// The device answered with something that is not a tool list.
    Malformed,
}

impl LiveToolsError {
    /// The catalogue route's answer: 503 offline, 504 timeout, 500 malformed, each with a sentence.
    fn refusal(self) -> ApiError {
        match self {
            LiveToolsError::Unavailable => ApiError::unavailable("device reconnecting")
                .saying("this machine is not connected; connect it, then try again"),
            LiveToolsError::Timeout => ApiError::gateway_timeout("device timeout")
                .saying("the device did not answer; it may still be working, so try again"),
            LiveToolsError::Malformed => ApiError::internal_code("malformed_tools").saying(
                "the device sent a tool list this console cannot read; update the device, then try again",
            ),
        }
    }
}

/// Send `tools/list` over the device's channel with the operator's own scope and return the raw
/// tool objects. The same helper answers the token form's plugin-tool check, so both read the one
/// list the device actually advertises.
async fn read_live_tools(app: &AppState, device_id: &str) -> Result<Vec<Value>, LiveToolsError> {
    // A registered but disconnected device is offline now; asking the channel would wait out the
    // reconnect bound before saying the same thing.
    if !app.channels.online(device_id) {
        return Err(LiveToolsError::Unavailable);
    }

    let scope = operator_scope(device_id);
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let answer = tokio::time::timeout(
        app.mcp_timeout,
        app.channels
            .request(device_id, request, &RequestAuth::Token(&scope)),
    )
    .await;

    let reply = match answer {
        Err(_) => return Err(LiveToolsError::Timeout),
        Ok(Err(RequestError::Offline | RequestError::Busy | RequestError::Disconnected)) => {
            return Err(LiveToolsError::Unavailable)
        }
        Ok(Err(RequestError::SendTimeout | RequestError::ReplyTimeout)) => {
            return Err(LiveToolsError::Timeout)
        }
        Ok(Ok(reply)) => reply,
    };

    let Reply::Result(response) = reply else {
        // A JSON-RPC error envelope for `tools/list` is not a list. It is the device's answer, so
        // it is malformed upstream rather than a transport failure.
        return Err(LiveToolsError::Malformed);
    };
    response
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .cloned()
        .ok_or(LiveToolsError::Malformed)
}

/// Map the device's advertised tools onto the catalogue contract, or `Err(())` when one entry is
/// not a tool object with a non-empty name.
fn catalogue_from_tools(tools: &[Value]) -> Result<Vec<LiveToolView>, ()> {
    tools
        .iter()
        .map(|tool| live_tool_view(tool).ok_or(()))
        .collect()
}

/// One advertised tool, classified. `None` when the entry cannot be a tool.
fn live_tool_view(tool: &Value) -> Option<LiveToolView> {
    let name = tool.get("name")?.as_str()?;
    if name.is_empty() {
        return None;
    }
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    // MCP requires `inputSchema`; a device that omits it still advertises a callable object, so
    // the contract's field gets the empty object schema rather than disappearing.
    let input_schema = tool
        .get("inputSchema")
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
    let (source, capability, destructive) = match summrise_core::registry::tool(name) {
        Some(definition) => (
            "builtin".to_owned(),
            registry_capability(name).unwrap_or_default().to_owned(),
            definition.destructive,
        ),
        None => {
            // A plugin tool is `<namespace>.<X>` (spec §8.2). The first segment is the capability
            // the page groups it under and the source the row carries.
            let namespace = name.split('.').next().unwrap_or(name);
            (
                format!("plugin:{namespace}"),
                namespace.to_owned(),
                plugin_tool_destructive(tool),
            )
        }
    };
    Some(LiveToolView {
        name: name.to_owned(),
        description,
        input_schema,
        source,
        capability,
        destructive,
    })
}

/// The capability whose registry entry declares `name`.
fn registry_capability(name: &str) -> Option<&'static str> {
    summrise_core::registry::CAPABILITIES
        .iter()
        .find(|capability| capability.tools.iter().any(|tool| tool.name == name))
        .map(|capability| capability.name)
}

/// A plugin tool's destructive flag, by MCP's own default: a read-only tool is not destructive, and
/// a tool that omits `destructiveHint` is destructive. This is the device's rule, restated here
/// because the console classifies the row the device advertises.
fn plugin_tool_destructive(tool: &Value) -> bool {
    let annotations = tool.get("annotations");
    let read_only = annotations
        .and_then(|annotations| annotations.get("readOnlyHint"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let destructive_hint = annotations
        .and_then(|annotations| annotations.get("destructiveHint"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    !read_only && destructive_hint
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
    // The same sentence the live stream carries, so a fetched window and a streamed line read the
    // same way. A stored line never had the call's arguments, so this is the general sentence for
    // its kind rather than the call's own.
    let lines = app
        .audit
        .newest(log_limit(query.limit)?)
        .await
        .into_iter()
        .map(|mut line| {
            let sentence = crate::activity::line_sentence(&line);
            line["sentence"] = json!(sentence);
            line
        })
        .collect();
    Ok(Json(lines))
}

/// `GET /activity/shot/:device/:id`: one remembered screen capture, exactly as the device sent it.
///
/// The event's `evidence.shot` names this path, and the operator's session is the gate: the image is
/// their own screen, so an unauthenticated read gets no bytes. An id that was never stored, has been
/// evicted, or belongs to another device is a JSON 404 — a page can say "this frame is gone" instead
/// of drawing a broken image.
async fn activity_shot(
    State(app): State<AppState>,
    _: OperatorSession,
    Path((device_id, image_id)): Path<(String, String)>,
) -> Response {
    match app.captures.get(&device_id, &image_id) {
        Some(png) => (
            [
                (
                    header::CONTENT_TYPE,
                    axum::http::HeaderValue::from_static("image/png"),
                ),
                (
                    header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("private, max-age=60"),
                ),
            ],
            axum::body::Bytes::copy_from_slice(&png),
        )
            .into_response(),
        None => ApiError::not_found("shot_not_found").into_response(),
    }
}

/// `GET /events`: the audit as a stream of server-sent events.
///
/// A `snapshot` carries the newest lines so the page opens with context, then each appended line is
/// an `activity` event. A slow client is told it `lagged` rather than given a growing backlog: it
/// refetches `/audit`, which is the same index from the same file.
async fn event_stream(State(app): State<AppState>, _: OperatorSession) -> Response {
    // Subscribe BEFORE reading the newest lines: a line written between the two would otherwise be in
    // neither the snapshot nor the stream.
    let live = app.audit.follow();
    let recent: Vec<Value> = app
        .audit
        .newest(40)
        .await
        .into_iter()
        .map(|mut line| {
            let sentence = crate::activity::line_sentence(&line);
            line["sentence"] = json!(sentence);
            line
        })
        .collect();
    // Oldest first, like the file: a page prepends each live event and gets a newest-first list.
    let recent: Vec<Value> = recent.into_iter().rev().collect();
    let snapshot = futures_util::stream::once(async move {
        let body = json!({ "lines": recent });
        Ok::<Event, Infallible>(Event::default().event("snapshot").data(body.to_string()))
    });
    let live_events = futures_util::stream::unfold(live, |mut live| async move {
        match live.recv().await {
            Ok(value) => Some((
                Ok::<Event, Infallible>(Event::default().event("activity").data(value.to_string())),
                live,
            )),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                let body = json!({ "dropped": dropped });
                Some((
                    Ok::<Event, Infallible>(
                        Event::default().event("lagged").data(body.to_string()),
                    ),
                    live,
                ))
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
        }
    });
    let mut response = Sse::new(futures_util::StreamExt::chain(snapshot, live_events))
        .keep_alive(KeepAlive::default().interval(Duration::from_secs(15)))
        .into_response();
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
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

    #[tokio::test]
    async fn root_sends_only_login_codes_to_the_login_link() {
        use axum::http::{StatusCode, Uri};

        let page = root(Uri::from_static("/")).await;
        assert_eq!(page.status(), StatusCode::OK);

        // A cache-bust or any other query is still the React page.
        let cache_bust = root(Uri::from_static("/?v=236")).await;
        assert_eq!(cache_bust.status(), StatusCode::OK);

        let code = root(Uri::from_static("/?code=abc&next=%2F")).await;
        assert_eq!(code.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            code.headers().get("location").unwrap(),
            "/next?code=abc&next=%2F"
        );
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
    fn a_screen_capture_is_read_from_a_text_item_or_an_image_item() {
        // The device's own tool answers a text item holding JSON; an MCP image item is the protocol's
        // other shape. Both carry the same base64, and neither is decoded until the capture store
        // checks the PNG signature.
        let text = Reply::Result(json!({
            "content": [{
                "type": "text",
                "text": json!({ "format": "png", "data": "QUJD", "width": 1, "height": 1 })
                    .to_string(),
            }],
            "isError": false,
        }));
        assert_eq!(screen_png(&text).as_deref(), Some("QUJD"));

        let image = Reply::Result(json!({
            "content": [{ "type": "image", "data": "QUJD", "mimeType": "image/png" }],
        }));
        assert_eq!(screen_png(&image).as_deref(), Some("QUJD"));

        let nested = Reply::Result(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "content": [{ "type": "image", "data": "QUJD" }] },
        }));
        assert_eq!(screen_png(&nested).as_deref(), Some("QUJD"));

        assert_eq!(screen_png(&Reply::Error(json!({ "error": {} }))), None);
        let failed = Reply::Result(json!({
            "content": [{ "type": "image", "data": "QUJD" }],
            "isError": true,
        }));
        assert_eq!(screen_png(&failed), None);
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

    #[test]
    fn a_run_records_its_outcome_in_the_audit_detail() {
        let result =
            |text: &str| Reply::Result(json!({"content": [{"type": "text", "text": text}]}));
        assert_eq!(
            run_detail(&result(
                r#"{"id":"r-1","exit_code":0,"timed_out":false,"duration_ms":42}"#
            ))
            .as_deref(),
            Some("run r-1 exited 0 in 42 ms")
        );
        assert_eq!(
            run_detail(&result(
                r#"{"id":"r-2","exit_code":null,"timed_out":true,"duration_ms":60000}"#
            ))
            .as_deref(),
            Some("run r-2 timed out after 60000 ms")
        );
        assert_eq!(
            run_detail(&result(
                r#"{"id":"r-3","exit_code":null,"timed_out":false,"duration_ms":7}"#
            ))
            .as_deref(),
            Some("run r-3 ended without an exit code")
        );
        // An error keeps the existing sentence; a reply that is not a run stays detail-less.
        assert_eq!(
            run_detail(&Reply::Error(json!({"error": {"code": -32603}}))).as_deref(),
            Some("jsonrpc error -32603")
        );
        assert_eq!(run_detail(&result("not json")), None);
    }

    #[test]
    fn a_run_carries_its_exit_facts_and_output_tail_as_evidence() {
        let result =
            |text: &str| Reply::Result(json!({"content": [{"type": "text", "text": text}]}));
        let evidence = run_evidence(&result(
            r#"{"id":"r-1","exit_code":3,"timed_out":false,"stdout":"boom\n","stderr":""}"#,
        ))
        .expect("a run answer parses");
        assert_eq!(evidence.exit_code, Some(3));
        assert_eq!(evidence.timed_out, Some(false));
        assert_eq!(evidence.tail.as_deref(), Some("boom\n"));

        // A run that printed nothing is still a fact: its exit code is what the panel says.
        let quiet = run_evidence(&result(
            r#"{"id":"r-2","exit_code":null,"timed_out":true,"stdout":"","stderr":""}"#,
        ))
        .expect("a run answer parses");
        assert_eq!(quiet.exit_code, None);
        assert_eq!(quiet.timed_out, Some(true));
        assert_eq!(quiet.tail, None);

        // A device error carries no run body, and a message is bounded for one row.
        assert!(run_evidence(&Reply::Error(json!({"error": {"code": -32001}}))).is_none());
        assert_eq!(bounded_detail(&"x".repeat(100)), "x".repeat(64));
    }

    /// The live catalogue's classification: the device's advertisement plus the console's source,
    /// capability and destructive rule. Built-ins come from the registry; a plugin tool's
    /// destructive flag is MCP's default, and its namespace is the first name segment.
    #[test]
    fn the_live_catalogue_classifies_builtins_and_plugin_tools() {
        let tools = vec![
            json!({
                "name": "terminal.create",
                "description": "Start a session.",
                "inputSchema": { "type": "object", "properties": {} },
            }),
            json!({
                "name": "hello.greet",
                "description": "Greet someone.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "who": { "type": "string" } },
                    "required": ["who"],
                },
                "annotations": { "destructiveHint": false },
            }),
            json!({
                "name": "hello.peek",
                "annotations": { "readOnlyHint": true, "destructiveHint": true },
            }),
            json!({ "name": "hello.boom" }),
        ];

        let views = catalogue_from_tools(&tools).expect("every entry is a tool");
        let served = serde_json::to_value(&views).expect("the views serialize");
        assert_eq!(
            served,
            json!([
                {
                    "name": "terminal.create",
                    "description": "Start a session.",
                    "inputSchema": { "type": "object", "properties": {} },
                    "source": "builtin",
                    "capability": "terminal",
                    "destructive": true,
                },
                {
                    "name": "hello.greet",
                    "description": "Greet someone.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "who": { "type": "string" } },
                        "required": ["who"],
                    },
                    "source": "plugin:hello",
                    "capability": "hello",
                    "destructive": false,
                },
                {
                    // A read-only tool is not destructive even when the other hint says it is.
                    "name": "hello.peek",
                    "description": "",
                    "inputSchema": { "type": "object", "properties": {} },
                    "source": "plugin:hello",
                    "capability": "hello",
                    "destructive": false,
                },
                {
                    // MCP's default for a plugin that says nothing: destructive.
                    "name": "hello.boom",
                    "description": "",
                    "inputSchema": { "type": "object", "properties": {} },
                    "source": "plugin:hello",
                    "capability": "hello",
                    "destructive": true,
                },
            ]),
            "the wire contract is six fields, exactly"
        );
    }

    /// A tool list the page cannot render is a 500 upstream fault, not a partial list: a tool with
    /// no name would otherwise reach the page as a nameless row.
    #[test]
    fn a_tool_without_a_name_makes_the_whole_list_malformed() {
        assert!(catalogue_from_tools(&[json!({ "name": "" })]).is_err());
        assert!(catalogue_from_tools(&[json!({ "description": "no name" })]).is_err());
        assert!(catalogue_from_tools(&[json!({ "name": 7 })]).is_err());
        assert_eq!(
            catalogue_from_tools(&[]).expect("an empty list is valid"),
            Vec::new()
        );
    }
}
