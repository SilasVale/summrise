//! HTTP surface served via Tower service (NOT axum route handlers).
//!
//! axum route handlers don't work on cross-compiled Windows. This uses the Tower
//! layer directly — the same layer MCP's StreamableHttpService sits on.
//!
//! Routes:
//!   GET  /                   → minimal status page (no token needed)
//!   GET  /panel, /panel/     → Apple-style terminal panel (token entered in
//!                              the browser, saved to localStorage; no server
//!                              token injection since 1.0.5 — back via
//!                              loopback / gateway-proxy secret / one-time
//!                              panel grant ?grant=, redeemed at the gateway)
//!   GET  /api/events         → SSE event stream
//!   GET  /api/events/poll    → poll events (?after=N)
//!   GET  /api/events/term    → SSE terminal byte stream (TermOutput JSON frames)
//!   GET  /api/status         → system status
//!   GET  /api/spec           → plugin spec
//!   GET  /api/plugins/status → plugin status (playwright-mcp running state)
//!   POST /api/plugins/playwright/start|stop → start/stop playwright-mcp
//!   POST /api/tools/{name}   → generic tool dispatch via PluginRegistry
//!   /mcp (via TokenGate)     → rmcp streamable HTTP server

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::Service;

use crate::plugins::memory::store::MemoryLimits;
use crate::state::AppState;
use summrise_agent_core::{DeviceError, EventBus};
mod panel;
mod parse;
mod sse;

pub use panel::WebPanel;
pub(crate) use panel::{
    panel_content_type, panel_token_response, plausible_grant, redeem_panel_grant, serve_panel_file,
};
pub(crate) use sse::{acquire_sse_guard, sse_stream, sse_term_stream, SseConnectionGuard};

/// Minimal self-contained status page — the device URL answers something readable
/// in a browser. Apple-style light, matching the rest of the Summrise surface.
///
/// THE COLOURS HERE ARE PART OF THE SURFACE, NOT DECORATION (round 236). This string carried the RETIRED accent
/// `#d9480f` long after the stylesheets replaced it, and nobody noticed because nothing compares a colour in a
/// Rust string with a colour in a CSS token. Measured on the rendered pair: `#d9480f` on `#ffefe5` is 3.83, under
/// the 4.5 AA needs for text; `#bf3a0a` on the same chip is 4.90. The code chips on this page were the only
/// sub-AA text in the product, on the first thing anyone sees at a device URL.
/// scripts/test/retired-colours-check.mjs now fails if the retired value comes back anywhere.
const STATUS_PAGE: &str = concat!(
    "<!doctype html><html><head><meta charset=\"utf-8\"><title>summrise-agent</title>",
    "<style>body{background:#f5f5f7;color:#1d1d1f;font-family:-apple-system,'SF Pro Text','PingFang SC','Segoe UI',sans-serif;margin:0;display:flex;justify-content:center;padding:12vh 24px}",
    ".card{background:rgba(255,255,255,.72);backdrop-filter:saturate(180%) blur(20px);-webkit-backdrop-filter:saturate(180%) blur(20px);border:1px solid rgba(0,0,0,.08);border-radius:20px;box-shadow:0 12px 32px rgba(0,0,0,.12);padding:32px;max-width:480px;width:100%}",
    ".mark{display:inline-flex;align-items:center;justify-content:center;width:44px;height:44px;border-radius:10px;background:#1d1d1f;color:#fff;font-weight:700;font-size:22px}",
    "h1{font-size:22px;margin:14px 0 4px;font-weight:650;letter-spacing:-.01em}",
    "p{color:#6e6e73;font-size:13px;margin:4px 0}",
    "code{background:#ffefe5;color:#bf3a0a;padding:1px 6px;border-radius:5px;font-family:ui-monospace,'SF Mono',Consolas,monospace;font-size:12px}",
    "</style></head>",
    "<body><div class=\"card\"><span class=\"mark\">V</span><h1>summrise-agent</h1>",
    "<p>MCP endpoint: <code>/mcp</code></p>",
    "<p>Tool API: <code>/api/tools/{name}</code></p>",
    "<p>Status: <code>/api/status</code></p>",
    "<p>Version: ",
    env!("CARGO_PKG_VERSION"),
    "</p></div></body></html>",
);

/// Build a response with a fallback that can't panic — the builder only fails
/// on invalid status/header constants, which ours never are.
/// Stamp a cache-control header on a response. Token-bearing and panel
/// responses must never be cached — call sites used to inline the same
/// 3-line insert with no-store / no-cache values.
pub(super) fn set_cache_control(resp: &mut Response, value: &'static str) {
    resp.headers_mut().insert(
        axum::http::HeaderName::from_static("cache-control"),
        axum::http::HeaderValue::from_static(value),
    );
}

pub(super) fn built_response(
    status: StatusCode,
    content_type: &'static str,
    body: Body,
) -> Response {
    Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .body(body)
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

// ── Auth helper ────────────────────────────────────────────

/// Read a query parameter by name, splitting on `&` so it works regardless of
/// position (`?after=5&token=x` — the old strip_prefix("token=") only matched
/// when the param came first).
pub(super) fn query_param<'a>(query: Option<&'a str>, key: &str) -> Option<&'a str> {
    query?.split('&').find_map(|pair| {
        pair.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix('='))
    })
}

/// Host header value without its `:port` suffix (trimmed) — shared by the
/// token-injection host allowlist and the loopback check below (the same
/// trim + strip-:port dance was duplicated at both sites).
pub(super) fn host_no_port(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| h.trim().split(':').next().unwrap_or(h.trim())) // strip :port
}

/// Check the Bearer token (Authorization header only). Static (non-async) so
/// it can be called before the Send boundary. The error is boxed — Response
/// is large and only ever handled at the top of handle_request.
///
/// Takes the HEADERS rather than the whole request (SOLID R108): that is all
/// it reads, and it keeps the check usable from a `Send` future — a whole
/// `&Request<Body>` is neither `Send` nor `Sync`, so holding one across an
/// `await` would poison any async caller's future.
///
/// SECURITY (2026-08-12): the ?token= query param was removed — a cross-site
/// page could send a text/plain POST with the token in the URL (no CORS
/// preflight) and bypass auth. Clients use the Authorization header (the
/// panel fetches SSE with fetch(), which sets headers; nothing used the
/// query param).
///
/// AND THIS GATE HAS NO BRUTE-FORCE PENALTY, while `TokenGate` — which guards `/mcp` — has one. Found
/// 2026-09-24 by the agent-web exploration, and the asymmetry is not an oversight so much as a tension
/// between two decisions that were each right:
///
///   * the throttle (`note_auth_failure` / `auth_penalty_remaining_ms`, round 206/207) must SLEEP, so it
///     lives in an `async` block and its input is a `peer` taken from the request's `ConnectInfo`;
///   * THIS function takes HEADERS rather than the request, on purpose, so it stays usable from a `Send`
///     future — see the paragraph above — and it is therefore synchronous and has no peer to charge.
///
/// The consequence is measured rather than argued: `/mcp` pays an exponential backoff for wrong tokens
/// (100 ms doubling to 2 s, per peer) and the `/api/*` surface pays nothing, although this is the gate
/// whose token reaches SYSTEM-level device control. Covering it means threading a `Copy` value — the
/// `IpAddr` — from wherever `ConnectInfo` is still attached, and making this `async` so the owed penalty
/// can be awaited; three call sites, one of which is the generic streaming helper above them. That is a
/// deliberate change on an auth path, not a hurried one, so it is recorded here with its shape instead.
/// AND THIS GATE CARRIES THE PENALTY TOO, since 2026-09-24. It did not, while `TokenGate` — which
/// guards `/mcp` — did: the exponential brute-force backoff (100 ms doubling to 2 s, PER PEER since
/// round 207) was invoked from two sites, both inside `TokenGate`, so the surface whose token reaches
/// SYSTEM-level device control paid nothing for wrong guesses. What made it a tension rather than an
/// oversight is that the throttle must SLEEP, and this function takes HEADERS rather than the request
/// (SOLID R108, so it stays usable from a `Send` future). It is `async` now and takes the peer, which
/// is a `Copy` value read from `ConnectInfo` before any await — the shape the earlier comment predicted.
///
/// NO TOKEN CONFIGURED STILL FAILS CLOSED, and it is charged as a failure like any other guess: a
/// hand-built Config that never got a token must not serve unauthenticated RCE routes, and a caller who
/// reaches that state has guessed wrong about the device as surely as one who sent a bad token.
async fn check_auth(
    peer: Option<std::net::IpAddr>,
    headers: &axum::http::HeaderMap,
    state: &AppState,
) -> Result<(), Box<Response>> {
    /// Refuse: record the failure, pay what is owed, hand back the 401 body. ONE place, because there
    /// are two ways to fail here and both are a caller guessing.
    async fn deny(peer: Option<std::net::IpAddr>, owed: u64) -> Box<Response> {
        note_auth_failure(peer);
        if owed > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(owed)).await;
        }
        Box::new(built_response(
            StatusCode::UNAUTHORIZED,
            "application/json",
            Body::from(r#"{"ok":false,"error":"unauthorized"}"#),
        ))
    }
    // PAY THE PENALTY FIRST (round 206's ordering, adopted here): a caller inside its window does not
    // even have its guess compared.
    let owed = auth_penalty_remaining_ms(peer);
    // Write-through (audit A4): the token comes from the LIVE snapshot, not
    // a boot-time copy — a token rotated via config mutations is visible
    // immediately, without a restart.
    let cfg = state.config_snapshot();
    let Some(ref token) = cfg.server.device_token else {
        // Fail CLOSED: bootstrap guarantees a token on every serving boot
        // (generates + persists fresh installs, recovers quarantined ones,
        // exits when randomness is unavailable) — a missing token here can
        // only come from a hand-built Config, which must never serve
        // unauthenticated RCE routes to the network.
        return Err(deny(peer, owed).await);
    };
    let from_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    // round-116: constant-time compare — the device token is the ONLY gate
    // between an unauthenticated network caller and SYSTEM-level device
    // control; a short-circuiting == leaks the match position via timing
    // (low practical value at 64 hex chars, but the proxy-secret compare in
    // this same file already sets the precedent).
    if token_is_usable(token)
        && from_header.is_some_and(|h| timing_safe_eq(h.as_bytes(), token.as_bytes()))
    {
        note_auth_success(peer);
        return Ok(());
    }
    Err(deny(peer, owed).await)
}

/// Constant-time byte compare — the device token is compared at every
/// /api/* gate; a short-circuiting == leaks the match position via timing
/// (round-116; the proxy-secret check below already used this shape).
/// Is a CONFIGURED token usable as a gate at all?
///
/// `timing_safe_eq(b"", b"")` returns TRUE, so a blank configured token would accept a
/// blank `Authorization: Bearer ` — an open, unauthenticated RCE surface. Both gates in
/// this file failed closed only on *absence* (`None`), never on *blankness*:
///
///   * `check_auth` — `device_token: Some("")` authenticated every `/api/*` route.
///   * `TokenGate`   — the same shape for `/mcp`.
///
/// Round-104 hardened the proxy-secret gate against exactly this ("an EMPTY/absent
/// configured secret must never match"); the two token gates were left relying on
/// `ensure_token()` in ANOTHER crate to normalize blank to `None`. That normalizer is
/// not a defence: a hand-built `Config` reaching the public `bind()` has no normalizer,
/// and a security gate must not depend on a caller having sanitized its input.
fn token_is_usable(token: &str) -> bool {
    !token.trim().is_empty()
}

fn timing_safe_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

// ── Token gate for the MCP route ─────────────────────────────

/// Wraps any Tower service with the same bearer-token check as the API.
/// The MCP endpoint is as sensitive as the API (it drives terminals), so it
/// is never reachable without the token. rmcp has no server-side auth hook,
/// so the check happens here.
///
/// Round-366: reads the token from the LIVE config snapshot per request —
/// like check_auth above — instead of a boot-time clone. The token is
/// minted pre-serve and no production path rotates it today, but a clone
/// would silently split /api vs /mcp auth the day one does (stale-accept
/// the old token on /mcp while rejecting the new one).
#[derive(Clone)]
pub struct TokenGate<S> {
    inner: S,
    state: Arc<AppState>,
}

impl<S> TokenGate<S> {
    pub fn new(inner: S, state: Arc<AppState>) -> Self {
        Self { inner, state }
    }
}

/// The response shape rmcp's StreamableHttpService produces — its body error
/// type is `Infallible`, which differs from axum's `BoxBody` error, so the
/// rejection response is built in the same shape here.
type McpBoxBody = http_body_util::combinators::BoxBody<bytes::Bytes, Infallible>;

/// HOW LONG A FAILED AUTHENTICATION COSTS THE CALLER (rounds 206-207).
///
/// The operator asked what protects MCP if the port is reachable — a VPN, SSH or a bound LAN address all end with THIS gate
/// and a bearer token as the only wall, and MEASURED, the production paths had no throttle of any kind (every `sleep` in
/// this file is in a test). A token is a long secret, so the goal is not to lock anybody out; it is to make guessing cost
/// time instead of being free:
///
/// ```text
///     0 -> 0ms, 1 -> 100ms, 2 -> 200, 3 -> 400, 4 -> 800, 5 -> 1600, >=6 -> 2000, and it stays there.
/// ```
///
/// An honest typo pays 100ms once; a script pays two seconds per attempt after the sixth. Success clears it, so the
/// operator who mistypes and then pastes the right token is never slowed down again.
pub(crate) fn auth_backoff_ms(fails: u32) -> u64 {
    match fails {
        0 => 0,
        1 => 100,
        2 => 200,
        3 => 400,
        4 => 800,
        5 => 1600,
        _ => 2000,
    }
}

/// PER PEER, NOT PER PROCESS (round 207). The first version kept one counter for the whole agent, which meant one caller's
/// failures slowed EVERY other client — a denial of service handed to whoever guesses wrong first, and the integration tests
/// found it before any operator did: one test's 401 made another test's client time out.
///
/// The peer comes from `ConnectInfo`, which the real server attaches and which is ABSENT when there is no socket to ask
/// (in-process tests, and any future non-TCP transport) — and absent is treated as LOCAL, which pays nothing. That is the
/// honest reading: an unknown peer cannot be one of many remote guessers, and penalising it would only slow the operator.
fn peer_key<B>(req: &Request<B>) -> Option<std::net::IpAddr> {
    // FROM THE SOCKET, NEVER FROM A HEADER. The first version read an `x-summrise-peer` header, which a caller sets freely — it
    // could have skipped its own penalty or framed another address with one. `ConnectInfo` is attached by the server from the
    // accepted connection, and its absence (in-process tests, non-TCP transports) means "unknown", which pays nothing.
    req.extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip())
}

fn auth_penalties(
) -> &'static std::sync::Mutex<std::collections::HashMap<std::net::IpAddr, (u32, u64)>> {
    static M: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<std::net::IpAddr, (u32, u64)>>,
    > = std::sync::OnceLock::new();
    M.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Bound the map so a spoofed-source flood cannot grow it without limit: 512 peers is far more than a device's working set.
const AUTH_PEERS_MAX: usize = 512;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Milliseconds still owed by this peer, 0 when none is active (or when the peer is unknown — see `peer_key`).
fn auth_penalty_remaining_ms(peer: Option<std::net::IpAddr>) -> u64 {
    let Some(peer) = peer else { return 0 };
    let Ok(map) = auth_penalties().lock() else {
        return 0;
    };
    map.get(&peer)
        .map(|(_, until)| until.saturating_sub(now_ms()))
        .unwrap_or(0)
}

/// One failed authentication: extend THIS PEER's window.
fn note_auth_failure(peer: Option<std::net::IpAddr>) {
    let Some(peer) = peer else { return };
    let Ok(mut map) = auth_penalties().lock() else {
        return;
    };
    if map.len() >= AUTH_PEERS_MAX && !map.contains_key(&peer) {
        // Full: drop the entries whose windows have already expired rather than growing, and if none have, do nothing —
        // refusing to record is a smaller failure than slowing an innocent peer.
        let now = now_ms();
        map.retain(|_, (_, until)| *until > now);
        if map.len() >= AUTH_PEERS_MAX {
            return;
        }
    }
    let e = map.entry(peer).or_insert((0, 0));
    e.0 = e.0.saturating_add(1);
    e.1 = now_ms().saturating_add(auth_backoff_ms(e.0));
}

/// A caller that authenticated: forgive it, immediately.
fn note_auth_success(peer: Option<std::net::IpAddr>) {
    if let Some(peer) = peer {
        if let Ok(mut map) = auth_penalties().lock() {
            map.remove(&peer);
        }
    }
}

/// WHERE A CLIENT LEARNS HOW TO AUTHENTICATE (round 207, from the research in docs/research/remote-mcp-access.md).
///
/// The MCP specification's own requirement for a 401 is `WWW-Authenticate: Bearer resource_metadata="..."`, pointing at an
/// RFC 9728 protected-resource document. Without it a remote client has nothing to discover from, and this gate can only
/// answer "unauthorized" to a client that was never told what would authorise it.
///
/// The resource identifier is built from the REQUEST (host header + forwarded scheme), because the same agent is reached as
/// 127.0.0.1 by the panel, as a tailnet name, or through a tunnel — and the identifier has to be the one the caller used.
fn resource_base(headers: &axum::http::HeaderMap) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .filter(|v| *v == "http" || *v == "https")
        .unwrap_or("http");
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .filter(|h| !h.is_empty() && h.len() < 128 && !h.contains('/'))
        .unwrap_or("127.0.0.1");
    format!("{scheme}://{host}")
}

fn unauthorized_mcp_response(headers: &axum::http::HeaderMap) -> axum::http::Response<McpBoxBody> {
    let body = http_body_util::Full::new(bytes::Bytes::from_static(
        br#"{"ok":false,"error":"unauthorized"}"#,
    ));
    let resource_metadata = format!(
        "{}/.well-known/oauth-protected-resource",
        resource_base(headers)
    );
    axum::http::Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(
            "WWW-Authenticate",
            format!("Bearer resource_metadata=\"{resource_metadata}\""),
        )
        .header("Content-Type", "application/json")
        .body(http_body_util::combinators::BoxBody::new(body))
        .unwrap_or_else(|_| {
            axum::http::Response::new(http_body_util::combinators::BoxBody::new(
                http_body_util::Full::new(bytes::Bytes::new()),
            ))
        })
}

impl<S, ReqBody> Service<Request<ReqBody>> for TokenGate<S>
where
    S: Service<Request<ReqBody>, Response = axum::http::Response<McpBoxBody>, Error = Infallible>,
    S::Future: Send + 'static,
{
    type Response = axum::http::Response<McpBoxBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        // Live snapshot (see struct docs): a runtime token rotation takes
        // effect on /mcp immediately, exactly like /api/*. Fail closed when
        // no token is configured, mirroring check_auth above.
        let headers_for_401 = req.headers().clone();
        let token = self.state.config_snapshot().server.device_token;
        let Some(token) = token else {
            return Box::pin(async move { Ok(unauthorized_mcp_response(&headers_for_401)) });
        };
        // PAY THE PENALTY FIRST (round 206), so a caller inside the window cannot even have its guess compared.
        let peer = peer_key(&req);
        let owed = auth_penalty_remaining_ms(peer);
        let token_for_check = token.clone();
        let authorized = token_is_usable(&token)
            && req
                .headers()
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .is_some_and(|h| timing_safe_eq(h.as_bytes(), token_for_check.as_bytes()));
        if !authorized {
            note_auth_failure(peer);
            return Box::pin(async move {
                if owed > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(owed)).await;
                }
                Ok(unauthorized_mcp_response(&headers_for_401))
            });
        }
        note_auth_success(peer);
        Box::pin(self.inner.call(req))
    }
}

/// How many run boundaries `/api/operation` carries. Bounded for the same
/// reason the events are: one request must not walk an unbounded log. A reader
/// that needs older boundaries can page with `since_ms` once the runs carry a
/// stamp it can filter on.
const RUNS_IN_TIMELINE: usize = 50;

/// Evidence-feed endpoints (/api/browser/actions, pwshots, pwshot) —
/// extracted from handle_request (round-28 SRP): the READ side of the
/// AI-evidence drawer. Returns None when the path is not one of ours.
async fn handle_browser_evidence(path: &str, query: Option<&str>) -> Option<Response> {
    // Surface audit D#2 (one-browser round): the READ side resolved
    // current_exe()'s parent while the WRITE side (playwright tools)
    // uses the registry install_dir() — the exact 1.2.219 /api/sessions
    // blindness pattern. Same source of truth now.
    //
    // The feed's storage contract (actions.jsonl shape, shot listing,
    // basename guard) is owned by crate::evidence — the producers
    // (browser_run_script, the mcp-client tools) append through the same
    // module, so a shape change has exactly one place to land.
    let pwout = crate::paths::evidence_dir();
    // P2: AI-action timeline — the JSONL written by browser_run_script
    // (one line per execution). Return newest-first, capped at 50.
    if path == "/api/browser/actions" {
        let actions = crate::evidence::recent_actions(&pwout, 50);
        return Some(built_response(
            StatusCode::OK,
            "application/json",
            Body::from(serde_json::json!({"actions": actions}).to_string()),
        ));
    }
    // The device's MERGED operation timeline — terminal audit + browser actions
    // on one ordered axis. Device-level by design (see crate::operation): the
    // embedded browser has no session ownership, and an AI's "one operation"
    // crosses sessions.
    //
    // `since_ms` lets a poller ask only for what it has not seen; `limit` bounds
    // the reply. Both default, so a bare GET returns the recent timeline.
    if path == "/api/operation" {
        let since_ms = query_param(query, "since_ms")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        let limit = query_param(query, "limit")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(200)
            .min(1000);
        let events = crate::operation::merged_operation(
            &crate::paths::sessions_dir(),
            &pwout,
            since_ms,
            limit,
        );
        return Some(built_response(
            StatusCode::OK,
            "application/json",
            Body::from(
                serde_json::json!({
                    "events": events,
                    // The run boundaries the events sit inside. Deliberately a
                    // SEPARATE array rather than a join: the runs log is
                    // best-effort (a device with an unwritable runs dir still
                    // records events), so an event may carry a run_id with no
                    // begin, and a run may exist with no events. Joining here
                    // would hide one of those cases inside the other. Grouping
                    // is the reader's job.
                    "runs": crate::runs::recent(&crate::paths::runs_dir(), RUNS_IN_TIMELINE),
                    "since_ms": since_ms,
                    // The newest stamp in the reply, so a poller can pass it back
                    // as `since_ms` without parsing the events itself.
                    "cursor_ms": events
                        .last()
                        .and_then(|e| e.get("ts_ms"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(since_ms),
                })
                .to_string(),
            ),
        ));
    }
    if path == "/api/browser/pwshots" {
        let shots = crate::evidence::list_shots(&pwout, 40);
        return Some(built_response(
            StatusCode::OK,
            "application/json",
            Body::from(serde_json::json!({"shots": shots}).to_string()),
        ));
    }
    // ANYTHING ELSE IS NOT OURS, and this is the branch the doc above has always claimed exists. It did
    // not: the two `if`s above FALL THROUGH to the screenshot reader, so a path that reached this
    // function without being one of its three routes was read as a pwshot name and answered
    // `400 "bad name"` — a wrong answer that looks like a right one — instead of returning `None` so the
    // caller could 404 it. Found 2026-09-24 by the agent-web exploration, which is also why the caller's
    // `if let Some(...)` read as permanently true.
    if path != "/api/browser/pwshot" {
        return None;
    }
    // /api/browser/pwshot?name=xxx — serve one screenshot (basename only)
    let name = query_param(query, "name").unwrap_or("");
    if !crate::evidence::shot_name_is_safe(name) {
        return Some(built_response(
            StatusCode::BAD_REQUEST,
            "text/plain",
            Body::from("bad name"),
        ));
    }
    Some(match std::fs::read(pwout.join(name)) {
        Ok(bytes) => {
            let mut resp = built_response(StatusCode::OK, "image/png", Body::from(bytes));
            set_cache_control(&mut resp, "no-store");
            resp
        }
        Err(_) => built_response(
            StatusCode::NOT_FOUND,
            "text/plain",
            Body::from("no such shot"),
        ),
    })
}

/// Panel / desktop root page — served with the zero-config token-injection
/// decision (gateway proxy shared secret / loopback / one-time ?grant=
/// redemption). Extracted from handle_request (round-29 SRP).
async fn handle_panel_home(
    state: &AppState,
    path: &str,
    query: Option<&str>,
    host: Option<String>,
    auth_header: Option<String>,
    // From the CONNECTION, not the header — see route_pre_dispatch's note.
    peer_is_loopback: bool,
) -> Response {
    // Write-through (audit A4): one snapshot of the LIVE config for the
    // whole injection decision (proxy_secret + device_token below).
    let cfg = state.config_snapshot();
    let mut resp = serve_panel_file("index.html", "text/html; charset=utf-8");
    set_cache_control(&mut resp, "no-store");
    // Zero-config token injection: embed the device token as a script
    // fragment before </head>. round-102/103: injection requires the
    // gateway proxy's SHARED SECRET (X-Summrise-Auth) — a plain marker
    // header was client-spoofable end-to-end (any curl could set it and
    // read the token; the leaked token grants /api/tools RCE). The
    // secret is generated at agent bootstrap and read by the console;
    // the gateway proxy sends it only for authenticated (admin session
    // or plugin link) requests. Localhost/loopback keeps working for
    // on-device use.
    let secret = cfg.server.proxy_secret.as_deref().unwrap_or("");
    // round-104: an EMPTY/absent configured secret must never match — the
    // old gate accepted an empty header when the secret was "" (quarantine
    // recovery / pre-secret boot), which is exactly the fail-open RCE.
    let via_proxy = !secret.is_empty()
        && auth_header
            .as_deref()
            // Constant-time compare (timing-safe for a 64-hex secret).
            .map(|v| timing_safe_eq(v.as_bytes(), secret.as_bytes()))
            .unwrap_or(false);
    if let Some(ref token) = cfg.server.device_token {
        // EXACT host allowlist. A substring/prefix match here was
        // bypassable — e.g. Host: evil-agent.saisi.online.evil.com matches
        // .contains("agent.saisi.online") and the token is handed to the
        // attacker's page. Only the device's own single-level subdomain
        // (dN.agent.saisi.online — a multi-level attacker subdomain like
        // evil.agent.saisi.online is REJECTED), the apex, or loopback.
        // NOTE: d1.agent.saisi.online has THREE dots — an earlier
        // "count() == 2" check made the subdomain branch unsatisfiable and
        // silently killed token injection for real devices (round-19).
        let host_ok = host
            .as_deref()
            .map(|host| {
                host == "127.0.0.1"
                    || host == "localhost"
                    || host == "agent.saisi.online"
                    || (host.ends_with(".agent.saisi.online")
                        && host.matches('.').count() == 3
                        && host
                            .split('.')
                            .next()
                            .map(|d| d.starts_with("d"))
                            .unwrap_or(false))
            })
            .unwrap_or(false);
        // round-102: token injection only via the gateway proxy OR
        // loopback — a public direct request must NOT receive the token.
        //
        // "LOOPBACK" IS THE PEER, NOT THE HEADER. This used to be decided entirely by the
        // client-supplied `Host`, so ANY local process — including a non-admin one — got
        // the permanent device token from one unauthenticated `curl
        // http://127.0.0.1:<port>/panel/`, which re-opened exactly what the config ACL
        // hardening closed (`paths.rs` grants read only to SYSTEM + Administrators). A
        // `Host: 127.0.0.1` costs an attacker nothing; a loopback SOCKET does not lie.
        //
        // FAIL CLOSED: no `ConnectInfo` extension means we cannot tell where the
        // connection came from, and "cannot tell" is not "loopback". The other two
        // admissions are unaffected — the proxy-secret branch is a real credential and the
        // grant branch is a single-use code, neither of which depends on this.
        let loopback = host
            .as_deref()
            .map(|host| host == "127.0.0.1" || host == "localhost")
            .unwrap_or(false);
        if host_ok && (via_proxy || (loopback && peer_is_loopback)) {
            return panel_token_response(token);
        }
        // One-time panel grant (gateway-issued; the fix the console's
        // openPanel ?token= flow was waiting for): the console mints a
        // 120s single-use grant bound to THIS device and opens
        // /panel/?grant=<code> directly at the device origin — the
        // permanent token never rides in a URL. The grant is redeemed
        // here (Bearer = our own device token) and on success the panel
        // is served with the token injected — EXACTLY the response shape
        // of the authorized injection path above. Panel paths only (the
        // desktop shell never receives grants).
        let panel_path = path == "/panel" || path == "/panel/";
        let grant = query_param(query, "grant").unwrap_or("").trim().to_string();
        if panel_path && plausible_grant(&grant) {
            // Pure-local device (no console binding): ?grant= is simply
            // invalid — fall through to the plain panel, the same
            // readable state a bad/absent token gets.
            if let Some(base) = cfg
                .platform
                .console_url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
            {
                // REFUSE A REPLAY. The gateway is KV, so its delete takes up to ~60 s to
                // be visible everywhere and a claim key does not help (a replay usually
                // arrives at a colo that never read the claim). This process IS strongly
                // consistent, and the replay must come through it — see the guard's own
                // note in panel.rs.
                if crate::web::panel::grant_already_redeemed(&grant) {
                    tracing::warn!(
                        grant_prefix = %&grant[..grant.len().min(8)],
                        "panel grant REPLAY refused -- this device already redeemed it"
                    );
                    return resp;
                }
                if redeem_panel_grant(base, token, &grant).await {
                    // Recorded only on SUCCESS, so a transient gateway failure does not
                    // burn the operator's one legitimate redemption.
                    crate::web::panel::remember_redeemed_grant(&grant);
                    return panel_token_response(token);
                }
            }
            // Redeem failed (network down, expired/consumed/wrong-device
            // grant, gateway 4xx): fall through to the plain panel below.
            // No injection on a maybe — a grant is a claim, not a proof.
        }
    }
    resp
}

// ── Request handler ──────────────────────────────────────────

/// Routes decided BEFORE the API pipeline (SOLID R108).
///
/// Every request either produces a response here, or is not one of these and
/// falls through to `handle_request`'s unconditional auth gate + dispatcher.
/// That split is the point: this function is the ONE enumeration of the
/// pre-dispatch surface, which is exactly the surface that is PUBLIC — the
/// panel/desktop SPA and its assets, the static status page — plus the three
/// streaming routes (SSE ×2, browser evidence) that run their own auth
/// because they never read a body.
///
/// Two consequences worth stating, because they used to depend on statement
/// order inside a 200-line function:
///   * "anything reaching the dispatcher is authenticated" (R102) becomes a
///     property of WHICH FUNCTION a request lands in, not of where the gate
///     happens to sit;
///   * the public surface is auditable by reading this one list, and
///     `deliberately_public_routes_stay_public` pins it from the outside.
///
/// Takes the request's BORROWED PIECES rather than the request itself: a
/// `&Request<Body>` is neither `Send` nor `Sync` (http-body is not), so
/// holding one across an `await` makes this future non-`Send` and breaks the
/// Tower service it is called from. `&Method`, `&str` and `&HeaderMap` all
/// are, so the signature is `Send` by construction.
/// Authenticate, reserve an SSE viewer slot, and build the stream response —
/// the three steps `/api/events` and `/api/events/term` share.
///
/// WHY THIS EXISTS: the two branches in `route_pre_dispatch` were
/// byte-identical apart from which stream function they returned, so every
/// future change to the streaming path had to be made TWICE and could be made
/// once. There is now a single place where an SSE slot is acquired.
///
/// THE VIEWER SLOT IS PASSED TO THE STREAM, which is what makes the 64-viewer
/// cap real (SOLID R128). Until this round the guard was bound to a local and
/// dropped when the response was CONSTRUCTED, so the cap bounded only
/// microseconds of setup — measured: 70 held responses produced zero 503s.
/// Now `acquire_sse_guard()`'s value is MOVED into the streaming task, so it is
/// released when the stream ends (client disconnect, error, or the body being
/// dropped — all three close the mpsc, which the pump detects and breaks on).
///
/// This is the ONE place an SSE slot is acquired, which is why the fix was a
/// single edit here plus the two signatures it forwards to.
async fn sse_route_response<F, Fut>(
    peer: Option<std::net::IpAddr>,
    headers: &axum::http::HeaderMap,
    state: &Arc<AppState>,
    stream: F,
) -> Response
where
    F: FnOnce(Arc<AppState>, SseConnectionGuard) -> Fut,
    Fut: std::future::Future<Output = Response>,
{
    if let Err(resp) = check_auth(peer, headers, state).await {
        return *resp;
    }
    let guard = match acquire_sse_guard() {
        Ok(g) => g,
        Err(resp) => return *resp,
    };
    // The guard is HANDED to the stream, not held here: it must survive until
    // the connection ends, and this function returns long before that does.
    stream(state.clone(), guard).await
}

/// EVERY ROUTE THIS AGENT ANSWERS, AND WHERE IT IS ANSWERED.
///
/// WHY THIS TABLE EXISTS. The surface declared its routes in THREE places: the path literals inside
/// `route_pre_dispatch`, the match arms inside `dispatch`, and a hand-written list in the test
/// `every_dispatch_route_is_auth_gated` that had to be kept in step with both by hand. The list had
/// ALREADY drifted — eight routes lived in `dispatch` and in no list, `/api/run/mark-exit` among
/// them — and the source scan written to catch the next drift started at `dispatch` and then ran on
/// for another twelve hundred lines, because it ended at the next `    async fn` rather than at the
/// end of the function. One fact in three copies is two too many: this is the copy the dispatcher
/// reads through `route_of`, the copy the tests walk, and the copy a reader checks a new route
/// against. It cannot drift from the dispatcher without failing to COMPILE (the match in `dispatch`
/// is exhaustive over `RouteId`), and it cannot drift from the tests without one of them going red.
///
/// ORDER IS PART OF THE CONTRACT, because `route_of` is first-match-wins exactly as the match arms
/// were, and the rows below are in the order those arms were evaluated in. The order is load-bearing
/// where a broader pattern sits under a narrower one: `GET /panel/` is `PanelHome` because the exact
/// row precedes `Prefix "/panel/"` (the same for `/desktop/`), `GET /api/sessions` is the collection
/// while `Under "/api/sessions/"` needs one more character, and the three session actions precede
/// `Prefix "/api/tools/"`'s neighbours in `dispatch`'s arm order. Move a row and `route_of` answers
/// differently with nothing else changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RouteId {
    // ── behind the unconditional auth gate, answered by `dispatch` ──
    Spec,
    Status,
    Sessions,
    SessionEvents,
    Logs,
    Boots,
    VitalsHistory,
    Update,
    Monitors,
    RunMarkExit,
    MonitorAdd,
    MonitorRemove,
    MonitorProbe,
    SessionControl,
    SessionApproval,
    SessionGrants,
    EventsPoll,
    SettingsGet,
    SettingsPut,
    GatewayConnect,
    PluginsStatus,
    PlaywrightStart,
    PlaywrightStop,
    ToolCall,
    // ── answered in `route_pre_dispatch`, which runs its own `check_auth` ──
    EventsStream,
    TermStream,
    PwShots,
    PwShot,
    Actions,
    Operation,
    // ── answered by the MCP `TokenGate` ──
    Mcp,
    // ── public: answered in `route_pre_dispatch` before the gate ──
    PanelHome,
    PanelFile,
    PanelPreflight,
    StatusPage,
    Discovery,
}

/// How a route's path is recognised. Four shapes are enough for every row below, and each one is
/// exactly the guard its match arm used before the table existed.
pub(crate) enum Pattern {
    /// `path` equals this exactly.
    Exact(&'static str),
    /// `path` starts with this AND carries at least one more character.
    Under(&'static str),
    /// `path` starts with this, the bare prefix included.
    Prefix(&'static str),
    /// `path` is `<at><something><ends>` (the session actions). The middle may be empty —
    /// `session_id_from_path` is what rejects an unusable id, as it does today.
    Between {
        at: &'static str,
        ends: &'static str,
    },
}

impl Pattern {
    /// Does `path` match? The METHOD is not consulted here — `route_of` compares it.
    pub(crate) fn matches(&self, path: &str) -> bool {
        match self {
            Pattern::Exact(p) => path == *p,
            // ONE CHARACTER MORE, not one more segment: this is the
            // `p.len() > "/api/sessions/".len()` guard it replaces, so `/api/sessions/` itself is
            // NOT this route (the exact row above it is the collection).
            Pattern::Under(p) => path.len() > p.len() && path.starts_with(p),
            // The bare prefix included — `POST /api/tools/` IS a tool call, as it has always been.
            Pattern::Prefix(p) => path.starts_with(p),
            // BOTH ENDS, and nothing said about the middle: `starts_with` + `ends_with` is exactly
            // the guard the match arms used, including the case where the two spans OVERLAP
            // (`/api/sessions/control` leaves an empty id, which `session_id_from_path` answers for).
            Pattern::Between { at, ends } => path.starts_with(at) && path.ends_with(ends),
        }
    }
}

/// WHERE a route is answered, which is what the tests below ask about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// Before the auth gate, no credential needed.
    Public,
    /// Answered in `route_pre_dispatch`, which runs `check_auth` itself.
    PreDispatchAuthed,
    /// Behind the unconditional auth gate that precedes `dispatch`.
    DispatchGated,
    /// `/mcp`, gated by the MCP `TokenGate` rather than by either of the above.
    Mcp,
}

/// One row. `method` is one of "GET" | "POST" | "PUT" | "OPTIONS" — the strings
/// `handle_request_inner` hands `dispatch`.
pub(crate) struct Route {
    pub method: &'static str,
    pub pattern: Pattern,
    pub id: RouteId,
    /// THE TEST SURFACE OF THE TABLE, and `allow`ed as such rather than given a production reader
    /// it does not have. This column is what lets `every_dispatch_route_is_auth_gated` walk the
    /// table and know, per row, which of the three owners must refuse an anonymous request — and
    /// what lets `the_route_table_has_a_floor_per_stage_and_a_method_vocabulary` notice a whole
    /// STAGE losing its rows, which the first version of that floor did not.
    ///
    /// WHY IT IS NOT READ IN PRODUCTION, deliberately: a `debug_assert!` here was this refactor's
    /// first attempt and it PANICKED a debug build on `OPTIONS /panel/index.html`, a path that
    /// legitimately falls through the pre-dispatch walk while `route_of` still calls it `Public`.
    /// Reading the column in the request path means encoding "where this is normally answered"
    /// as a runtime rule, which is exactly the duplicated classification the auth gate's own
    /// comment warns about. The rule belongs in a test; the compiler confirms the field is used
    /// there (`cfg_attr(not(test))`, because the non-test lib build has no reader).
    #[cfg_attr(not(test), allow(dead_code))]
    pub stage: Stage,
}

/// Every request this agent answers, in the order the match arms are evaluated
/// today. THE ORDER IS PART OF THE CONTRACT — first match wins.
pub(crate) fn routes() -> &'static [Route] {
    /// THE TABLE. Kept inside `routes()` so the function is the only way to read it.
    const ROUTES: &[Route] = &[
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/spec"),
            id: RouteId::Spec,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/status"),
            id: RouteId::Status,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/sessions"),
            id: RouteId::Sessions,
            stage: Stage::DispatchGated,
        },
        // The `Under` row sits DIRECTLY under the exact collection row, deliberately: `/api/sessions`
        // is the list, `/api/sessions/<one more character>` is one session's events.
        Route {
            method: "GET",
            pattern: Pattern::Under("/api/sessions/"),
            id: RouteId::SessionEvents,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/logs"),
            id: RouteId::Logs,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/boots"),
            id: RouteId::Boots,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/vitals/history"),
            id: RouteId::VitalsHistory,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/update"),
            id: RouteId::Update,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/monitors"),
            id: RouteId::Monitors,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "POST",
            pattern: Pattern::Exact("/api/run/mark-exit"),
            id: RouteId::RunMarkExit,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "POST",
            pattern: Pattern::Exact("/api/monitors/add"),
            id: RouteId::MonitorAdd,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "POST",
            pattern: Pattern::Exact("/api/monitors/remove"),
            id: RouteId::MonitorRemove,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "POST",
            pattern: Pattern::Exact("/api/monitors/probe"),
            id: RouteId::MonitorProbe,
            stage: Stage::DispatchGated,
        },
        // The three session ACTIONS, in the order their arms were written, and each one before any
        // broader POST arm — the same reason the guards carried.
        Route {
            method: "POST",
            pattern: Pattern::Between {
                at: "/api/sessions/",
                ends: "/control",
            },
            id: RouteId::SessionControl,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "POST",
            pattern: Pattern::Between {
                at: "/api/sessions/",
                ends: "/approval",
            },
            id: RouteId::SessionApproval,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "POST",
            pattern: Pattern::Between {
                at: "/api/sessions/",
                ends: "/grants",
            },
            id: RouteId::SessionGrants,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/events/poll"),
            id: RouteId::EventsPoll,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/settings"),
            id: RouteId::SettingsGet,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "PUT",
            pattern: Pattern::Exact("/api/settings"),
            id: RouteId::SettingsPut,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "POST",
            pattern: Pattern::Exact("/api/gateway/connect"),
            id: RouteId::GatewayConnect,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/plugins/status"),
            id: RouteId::PluginsStatus,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "POST",
            pattern: Pattern::Exact("/api/plugins/playwright/start"),
            id: RouteId::PlaywrightStart,
            stage: Stage::DispatchGated,
        },
        Route {
            method: "POST",
            pattern: Pattern::Exact("/api/plugins/playwright/stop"),
            id: RouteId::PlaywrightStop,
            stage: Stage::DispatchGated,
        },
        // LAST of the `/api` rows, and a `Prefix`: `/api/tools/` with a name after it is a tool call,
        // and so is the bare `/api/tools/` (the tool name is then empty and the registry says so).
        Route {
            method: "POST",
            pattern: Pattern::Prefix("/api/tools/"),
            id: RouteId::ToolCall,
            stage: Stage::DispatchGated,
        },
        // ── answered in `route_pre_dispatch`, each running `check_auth` itself ──
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/events"),
            id: RouteId::EventsStream,
            stage: Stage::PreDispatchAuthed,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/events/term"),
            id: RouteId::TermStream,
            stage: Stage::PreDispatchAuthed,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/browser/pwshots"),
            id: RouteId::PwShots,
            stage: Stage::PreDispatchAuthed,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/browser/pwshot"),
            id: RouteId::PwShot,
            stage: Stage::PreDispatchAuthed,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/browser/actions"),
            id: RouteId::Actions,
            stage: Stage::PreDispatchAuthed,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/api/operation"),
            id: RouteId::Operation,
            stage: Stage::PreDispatchAuthed,
        },
        // ── the MCP endpoint: axum's nested `TokenGate` claims it ──
        Route {
            method: "GET",
            pattern: Pattern::Exact("/mcp"),
            id: RouteId::Mcp,
            stage: Stage::Mcp,
        },
        Route {
            method: "POST",
            pattern: Pattern::Exact("/mcp"),
            id: RouteId::Mcp,
            stage: Stage::Mcp,
        },
        // ── public ──
        //
        // THE ORDERING HAZARD LIVES HERE: these four exact rows MUST stay above the two `Prefix
        // "/panel/"` / `Prefix "/desktop/"` rows, or `route_of("GET", "/panel/")` answers `PanelFile`
        // instead of the panel HTML — a static asset lookup for the SPA's front door.
        Route {
            method: "GET",
            pattern: Pattern::Exact("/panel"),
            id: RouteId::PanelHome,
            stage: Stage::Public,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/panel/"),
            id: RouteId::PanelHome,
            stage: Stage::Public,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/desktop"),
            id: RouteId::PanelHome,
            stage: Stage::Public,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/desktop/"),
            id: RouteId::PanelHome,
            stage: Stage::Public,
        },
        Route {
            method: "GET",
            pattern: Pattern::Prefix("/panel/"),
            id: RouteId::PanelFile,
            stage: Stage::Public,
        },
        Route {
            method: "GET",
            pattern: Pattern::Prefix("/desktop/"),
            id: RouteId::PanelFile,
            stage: Stage::Public,
        },
        Route {
            method: "OPTIONS",
            pattern: Pattern::Prefix("/panel/"),
            id: RouteId::PanelPreflight,
            stage: Stage::Public,
        },
        Route {
            method: "OPTIONS",
            pattern: Pattern::Prefix("/desktop/"),
            id: RouteId::PanelPreflight,
            stage: Stage::Public,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/"),
            id: RouteId::StatusPage,
            stage: Stage::Public,
        },
        Route {
            method: "GET",
            pattern: Pattern::Exact("/.well-known/oauth-protected-resource"),
            id: RouteId::Discovery,
            stage: Stage::Public,
        },
    ];
    ROUTES
}

/// The table's only lookup. `None` is the "not found" answer.
pub(crate) fn route_of(method: &str, path: &str) -> Option<RouteId> {
    routes()
        .iter()
        .find(|r| r.method == method && r.pattern.matches(path))
        .map(|r| r.id)
}

async fn route_pre_dispatch(
    method: &Method,
    path: &str,
    query: Option<&str>,
    headers: &axum::http::HeaderMap,
    state: &Arc<AppState>,
    // The SOCKET's address, for the auth penalty (`check_auth` charges per peer). A SEPARATE value
    // from `peer_is_loopback` below, and deliberately so: `peer_key` reads the same `ConnectInfo` and
    // treats ABSENT as local (nobody to penalise), while the bool treats absent as NOT loopback, which
    // DENIES the loopback token handout. One extension, two questions, two answers.
    peer: Option<std::net::IpAddr>,
    // Whether the SOCKET is loopback — decided at the connection boundary, because a
    // `Host: 127.0.0.1` header costs an attacker nothing and this used to be the only
    // thing standing behind the loopback token handout.
    peer_is_loopback: bool,
) -> Option<Response> {
    // SSE event stream — streaming, handled before body parsing.
    if *method == Method::GET && path == "/api/events" {
        return Some(sse_route_response(peer, headers, state, sse_stream).await);
    }

    // SSE terminal byte stream — streamed TermOutput JSON frames.
    if *method == Method::GET && path == "/api/events/term" {
        return Some(sse_route_response(peer, headers, state, sse_term_stream).await);
    }

    // round-152: AI browser evidence stream — list + fetch screenshots from
    // the pwout dir (browser_run_script & playwright scripts drop screenshots
    // here). The panel polls pwshots and shows new PNGs as the AI works,
    // so a human can see what the AI did without any live frame stream.
    if *method == Method::GET
        && (path == "/api/browser/pwshots"
            || path == "/api/browser/pwshot"
            || path == "/api/browser/actions"
            // The merged operation timeline. Listed HERE so it goes through the
            // check_auth above with the rest: it carries commands, goals and
            // plans from every session, i.e. strictly MORE than the actions feed
            // it sits beside. Reaching handle_browser_evidence without this list
            // would 404; reaching it without the guard would leak the device.
            || path == "/api/operation")
    {
        if let Err(resp) = check_auth(peer, headers, state).await {
            return Some(*resp);
        }
        if let Some(resp) = handle_browser_evidence(path, query).await {
            return Some(resp);
        }
    }

    // Panel / desktop root: served with the zero-config token-injection
    // decision (gateway proxy secret / loopback / one-time grant) — extracted
    // as handle_panel_home (round-29 SRP). Assets are embedded at compile time
    // from resources/panel/; /desktop/ is the Electron full-screen shell and
    // the SPA switches on the path.
    //
    // SECURITY (2026-08-12): the panel previously embedded the device token as
    // window.__PANEL_TOKEN__ for zero-config access. With CORS * on every
    // response, any third-party page could fetch /panel/ and read the token.
    // The token is no longer injected — the user enters it once in the panel
    // (saved to localStorage) instead.
    if *method == Method::GET
        && (path == "/panel" || path == "/panel/" || path == "/desktop" || path == "/desktop/")
    {
        let host = host_no_port(headers).map(|h| h.to_string());
        let auth_header = headers
            .get("x-summrise-auth")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());
        return Some(
            handle_panel_home(state, path, query, host, auth_header, peer_is_loopback).await,
        );
    }
    // THE PREFLIGHT COMES FIRST, AND THE GET-ONLY ROUTE NEVER SAW IT (round 247). A cross-origin asset load is preceded by
    // an OPTIONS with `Access-Control-Request-Private-Network: true`, and it fell through this dispatcher entirely. Answering
    // it is what makes the headers above reachable at all. Scoped exactly like them: the assets, never index.html.
    if *method == Method::OPTIONS && (path.starts_with("/panel/") || path.starts_with("/desktop/"))
    {
        let plen = if path.starts_with("/desktop/") {
            "/desktop/".len()
        } else {
            "/panel/".len()
        };
        let file = path[plen..].split('?').next().unwrap_or("");
        if file != "index.html" {
            let mut resp = built_response(
                StatusCode::NO_CONTENT,
                "text/plain; charset=utf-8",
                Body::empty(),
            );
            for (k, v) in [
                ("Access-Control-Allow-Origin", "*"),
                ("Access-Control-Allow-Private-Network", "true"),
                ("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"),
                ("Access-Control-Allow-Headers", "*"),
                ("Access-Control-Max-Age", "600"),
            ] {
                resp.headers_mut()
                    .insert(k, axum::http::HeaderValue::from_static(v));
            }
            return Some(resp);
        }
    }

    if *method == Method::GET && (path.starts_with("/panel/") || path.starts_with("/desktop/")) {
        // Strip any ?v=… cache-buster before whitelist matching.
        let prefix_len = if path.starts_with("/desktop/") {
            "/desktop/".len()
        } else {
            "/panel/".len()
        };
        let file = path[prefix_len..].split('?').next().unwrap_or("");
        return Some(serve_panel_file(file, panel_content_type(file)));
    }

    // GET non-API — minimal status page: public (no token needed).
    // DISCOVERY, BEFORE THE SPA FALLBACK (round 207): everything that is not /api or /mcp is served the panel HTML below, so
    // this route has to be answered first. It is deliberately UNAUTHENTICATED — it is how a client that has no credential yet
    // learns what would authorise it — and it carries no secret: the resource identifier, the bearer method, and nothing else.
    if *method == Method::GET && path == "/.well-known/oauth-protected-resource" {
        let base = resource_base(headers);
        return Some(built_response(
            StatusCode::OK,
            "application/json",
            Body::from(
                serde_json::json!({
                    "resource": format!("{base}/mcp"),
                    "bearer_methods_supported": ["header"],
                    "resource_documentation": format!("{base}/panel/"),
                })
                .to_string(),
            ),
        ));
    }
    if *method == Method::GET && !path.starts_with("/api") && path != "/mcp" {
        let mut resp = built_response(
            StatusCode::OK,
            "text/html; charset=utf-8",
            Body::from(STATUS_PAGE),
        );
        set_cache_control(&mut resp, "no-cache");
        return Some(resp);
    }

    None
}

/// THE ACCESS LOG (round 29 of the standing goal): one line per request — method, path, status, milliseconds.
///
/// WHY IT EXISTS. The inventory could not answer "does anything still call `/api/events`?" or "is the browser panel
/// density used at all?": the agent kept no record of what was asked for, so a route with no client anywhere in the
/// tree was unmeasurable from the machine that serves it. This is that record, and it is also the only way to tell the
/// three front ends apart in production (the desktop window, a browser panel, the console's proxy, an MCP client).
///
/// WHAT IT DELIBERATELY DOES NOT RECORD: the query string. `?grant=` carries a one-time panel token and other routes
/// take credentials in the query; a log that collects them is a credential store nobody meant to create. The panel and
/// desktop STATIC assets are skipped too — they would be most of the lines and answer nothing.
fn access_log(method: &str, path: &str, status: u16, ms: u128) {
    if path.starts_with("/panel") || path.starts_with("/desktop") {
        return;
    }
    crate::paths::append_log("access.log", &format!("{method} {path} {status} {ms}ms"));
}

pub(super) async fn handle_request(req: Request<Body>, state: Arc<AppState>) -> Response {
    let started = std::time::Instant::now();
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let resp = handle_request_inner(req, state).await;
    access_log(
        &method,
        &path,
        resp.status().as_u16(),
        started.elapsed().as_millis(),
    );
    resp
}

async fn handle_request_inner(req: Request<Body>, state: Arc<AppState>) -> Response {
    // NOTE: no CORS preflight handler — the panel is same-origin (never
    // preflights); cross-origin calls must NOT be allowed, and the gateway
    // proxy adds its own ACAO when required. (The old handler advertised
    // ACAO:null that real responses never granted — dead + misleading.)
    //
    // Hoisted BEFORE the pre-dispatch call so the borrowed pieces (and the
    // `req` borrow they come from) do not straddle an await — see
    // route_pre_dispatch's note on `Send`.
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query_str = req.uri().query().map(|q| q.to_string());
    // Read BEFORE any await and before `req` is consumed. Absent extension => `false` =>
    // the loopback handout is DENIED: "we cannot tell where this connection came from" is
    // not "it came from loopback".
    let peer_is_loopback = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip().is_loopback())
        .unwrap_or(false);
    let peer = peer_key(&req);
    if let Some(resp) = route_pre_dispatch(
        &method,
        &path,
        query_str.as_deref(),
        req.headers(),
        &state,
        peer,
        peer_is_loopback,
    )
    .await
    {
        return resp;
    }

    // ── Auth gate for EVERYTHING below (SOLID R102) ──────────────
    //
    // This gate is UNCONDITIONAL on purpose. It used to be wrapped in a
    // `needs_auth` flag recomputed as `method != GET || path.starts_with(
    // "/api") || path == "/mcp"`, which classified routes a SECOND time —
    // the pre-dispatch routes above (public status page, panel/desktop SPA,
    // the three auth-checked streaming routes) already decide exactly which
    // requests are public, so the flag was provably always true here.
    //
    // A duplicated classification is a security hazard with an asymmetric
    // failure mode: if the two copies ever disagree such that a request
    // reaches this point with the flag false, the gate is SKIPPED and an
    // unauthenticated caller reaches tool dispatch (terminal_execute,
    // system_file_write → SYSTEM-level device control). Gating
    // unconditionally removes the disagreement by construction — anything
    // that falls through to the dispatcher is authenticated, full stop —
    // and the public surfaces above keep their own explicit, tested
    // behaviour. `every_dispatch_route_is_auth_gated` /
    // `deliberately_public_routes_stay_public` pin both halves.
    if let Err(resp) = check_auth(peer, req.headers(), &state).await {
        return *resp;
    }

    // Read body for API requests. >1MB must FAIL LOUDLY, not degrade to an
    // empty body — the old unwrap_or_default() turned an oversized
    // terminal_execute/write into a "successful" call that ran NOTHING
    // (silent data loss; round-59). The gateway's ok/data.ok double-check
    // turns this 413 into a stable error code downstream.
    let body_bytes = match axum::body::to_bytes(req.into_body(), 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            // Distinguish a genuine size violation from a transport error
            // (client dropped mid-body) — both were lumped into 413 +
            // "exceeds limit", lying about a disconnect (round-60).
            // axum::Error is a boxed error; walk the source chain for the
            // LengthLimitError marker (its Display is "length limit exceeded").
            let mut too_large = false;
            let mut src: Option<&(dyn std::error::Error + 'static)> = Some(&e);
            while let Some(s) = src {
                if s.to_string().contains("length limit") {
                    too_large = true;
                    break;
                }
                src = s.source();
            }
            let (status, code, msg) = if too_large {
                (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "payload_too_large",
                    "request body exceeds 1 MB limit",
                )
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    "body_read_error",
                    "failed to read request body",
                )
            };
            return built_response(
                status,
                "application/json",
                Body::from(
                    serde_json::json!({
                        "ok": false,
                        "error": msg,
                        "code": code,
                    })
                    .to_string(),
                ),
            );
        }
    };
    let body_str = String::from_utf8_lossy(&body_bytes).to_string();

    match dispatch(
        &state,
        method.as_str(),
        &path,
        &body_str,
        query_str.as_deref(),
    )
    .await
    {
        Ok(result) => axum::Json(result).into_response(),
        Err(resp) => *resp,
    }
}

/// WHY THIS FUNCTION'S SHAPE IS THE MECHANISM (the route-table round).
///
/// It matches on `route_of(method, path)` — the table's only lookup — and the match is EXHAUSTIVE
/// over `RouteId` with NO WILDCARD ARM. That is the point rather than a style choice: a row added to
/// `routes()` without a handler here does not COMPILE, so the two cannot drift. `None` is the one
/// "not found" answer.
///
/// IT IS AT MODULE SCOPE NOW, and that move is the other half. As a NESTED function it could not be
/// called from a test, so `every_dispatch_route_is_auth_gated` enumerated the routes by reading this
/// file's own source instead — a list kept in step by hand, which had already drifted. This is still
/// called only by `handle_request_inner`, still after the same unconditional auth gate.
///
/// The arms keep their bodies and their comments verbatim. What changed is the selector: the guards
/// that used to decide (`p.starts_with("/api/sessions/") && p.ends_with("/control")`) are now the
/// table's `Pattern`, which is also what the tests walk.
/** Route dispatch: map (method, path) -> handler result or error.
 *  Extracted from handle_request (round-91 SRP) so the main fn stays
 *  auth + body parsing + routing skeleton. */
async fn dispatch(
    state: &AppState,
    method: &str,
    path: &str,
    body_str: &str,
    query_str: Option<&str>,
) -> Result<serde_json::Value, Box<Response>> {
    // NO `debug_assert!` ON THE `stage` COLUMN HERE, AND THE FIRST VERSION OF THIS REFACTOR HAD ONE.
    // It read well — "a `Public` row that reaches the dispatcher means the table and the walk
    // disagree" — and it was WRONG in a way the round's own test had already reasoned about: a request
    // can reach this function legitimately while `route_of` calls it `Public`. `OPTIONS /panel/index.html`
    // is exactly that case, and deliberately so (see the preflight arm: it answers every asset EXCEPT
    // index.html, and what happens to that one is this function's "not found"). Measured consequence:
    // a debug build panicked on that request. `pre_dispatch_stage_agrees_with_the_table` is where this
    // rule lives, which is what that test's own comment decided 240 lines below — "IT IS A TEST RATHER
    // THAN A `debug_assert!` INSIDE THE WALK … without adding a panic to the request path" — and the
    // `OPTIONS` case is now pinned by `a_preflight_for_index_html_falls_through_to_not_found`.
    //
    // The OTHER direction — a `DispatchGated` row answered before the gate, i.e. an auth bypass — is
    // not visible from here (the request never arrives), and is what `every_dispatch_route_is_auth_gated`
    // refuses by sending a tokenless request to every row of the table.
    let result = match route_of(method, path) {
        Some(RouteId::Spec) => api_spec(state),
        Some(RouteId::Status) => api_status(state).await,
        Some(RouteId::Sessions) => api_sessions_list(),
        Some(RouteId::SessionEvents) => match api_session_events(path) {
            Ok(v) => v,
            Err(resp) => return Err(Box::new(*resp)),
        },
        Some(RouteId::Logs) => api_logs(),
        // The device's own restart history — how often this agent has started, and how
        // each run before those starts ended. Separate from `/api/status` on purpose: the
        // status poll runs every 15 s and must stay small, while this list is read when a
        // human asks (the Settings card) or once a minute (the strip's count).
        Some(RouteId::Boots) => api_boots(),
        // The device's vitals HISTORY — the trend behind the two numbers on the strip.
        // Separate from `/api/status` for the same reason `/api/boots` is: the status
        // poll runs every 15 s and must stay small, while a series is read when a human
        // asks for it.
        Some(RouteId::VitalsHistory) => api_vitals_history(),
        // The device's UPDATE STATE — current, what the channel has, whether a rollback
        // pin holds it, whether one is already in flight. Read-only: applying an update
        // goes through the existing tool route (`POST /api/tools/agent_update`), so this
        // adds a view and NOT a second way to install anything.
        Some(RouteId::Update) => api_update(state).await,
        // REACHABILITY — the watched targets, each with its probe series and summary.
        Some(RouteId::Monitors) => crate::monitor::snapshot(),
        // A supervisor that is about to kill this process says so FIRST — and says WHY, because
        // "stopped on purpose" and "replaced by an update" are different verdicts for the next
        // start, and the clock cannot tell them apart (the two durations overlap; see
        // runstate::classify). `reason` defaults to "stop".
        Some(RouteId::RunMarkExit) => {
            let reason = serde_json::from_str::<serde_json::Value>(if body_str.is_empty() {
                "{}"
            } else {
                body_str
            })
            .ok()
            .and_then(|v| v.get("reason").and_then(|r| r.as_str()).map(str::to_string))
            .unwrap_or_else(|| "stop".to_string());
            serde_json::json!({
                "ok": true,
                "reason": reason,
                "marked": crate::runstate::mark_deliberate_stop(&crate::paths::data_dir(), &reason),
            })
        }
        Some(RouteId::MonitorAdd) => api_monitor_add(body_str),
        Some(RouteId::MonitorRemove) => api_monitor_remove(body_str),
        // One probe, now — the panel's "check now" (a target that was just added, or an
        // operator who does not want to wait 15 s for the first reading).
        Some(RouteId::MonitorProbe) => api_monitor_probe(body_str).await,
        // Control handoff (design §D5). MUST be matched before any broader
        // /api/sessions POST arm; the `.ends_with` also keeps it from
        // swallowing a future sibling action on the same collection.
        Some(RouteId::SessionControl) => {
            let sid = session_id_from_path(path)?;
            api_session_control(state, &sid, body_str).await?
        }
        // The gate's decision. Matched before any broader arm, same as the
        // control route above.
        Some(RouteId::SessionApproval) => {
            let sid = session_id_from_path(path)?;
            api_session_approval(state, &sid, body_str).await?
        }
        Some(RouteId::SessionGrants) => {
            let sid = session_id_from_path(path)?;
            api_session_grants(state, &sid, body_str).await?
        }
        Some(RouteId::EventsPoll) => {
            let after: u64 = query_param(query_str, "after")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            api_events_poll(state, after)
        }
        Some(RouteId::SettingsGet) => api_settings_get(state).await,
        Some(RouteId::SettingsPut) => match api_settings_put(state, body_str) {
            Ok(v) => v,
            Err(resp) => return Err(Box::new(*resp)),
        },
        Some(RouteId::GatewayConnect) => match api_gateway_connect(state, body_str).await {
            Ok(v) => v,
            Err(resp) => return Err(Box::new(*resp)),
        },
        Some(RouteId::PluginsStatus) => api_plugins_status(state).await,
        Some(RouteId::PlaywrightStart) => match api_playwright_start(state).await {
            Ok(v) => v,
            Err(resp) => return Err(Box::new(*resp)),
        },
        Some(RouteId::PlaywrightStop) => match api_playwright_stop(state).await {
            Ok(v) => v,
            Err(resp) => return Err(Box::new(*resp)),
        },
        Some(RouteId::ToolCall) => {
            let tool_name = path.strip_prefix("/api/tools/").unwrap_or("");
            api_call_tool(state, tool_name, body_str).await
        }
        // ONE FAILURE, THREE ANSWERS — AND THE ONE BELOW IS DELIBERATE, which is why it is written
        // down rather than tidied. The agent-web exploration listed this as friction: a malformed body
        // on the request-parsing routes is `400 + code` (`parse::invalid_params_response`), a
        // malformed body on the monitor and tool routes is `200 + {ok:false,code}`, an unknown route
        // is `200 + {ok:false,error}` right here, and `/api/run/mark-exit` swallows its parse error
        // entirely.
        //
        // IT CANNOT SIMPLY BE MADE CONSISTENT, and the evidence is a second client:
        // `gateway/src/mcp.ts` records the shape in a comment ("tool errors as HTTP 200 +
        // {ok:false,error}") and compensates for it with `!ok || data.ok === false`, and the
        // console's `callTool` re-checks `ok === false` for the same reason. Answering a failed tool
        // with a non-2xx would break both, so the shape stays.
        //
        // WHAT IS WORTH FIXING IS ON THE CLIENT SIDE: the panel hand-rolls that check in TWELVE places
        // across three dialects (`ok !== true`, `ok === false`, `if (j?.ok)`), when the gateway shows
        // the pattern — ONE predicate, inside the client, applied by every caller. That is a
        // deliberate change to the panel's data layer, not a line here.
        // THE ROWS THIS FUNCTION IS NOT THE OWNER OF, NAMED ONE BY ONE RATHER THAN WILDCARDED.
        //
        // These stages say "answered somewhere else", and in every case that is where they are in
        // production: the `Public` and `PreDispatchAuthed` rows are answered in `route_pre_dispatch`,
        // which runs BEFORE the unconditional auth gate below it (a stream, a static file or the
        // token handout cannot wait for a body read), and `Mcp` is claimed by axum's nested
        // `TokenGate` before this service is ever reached. Reaching here with one of them means the
        // pre-dispatch walk and the table disagree — `pre_dispatch_stage_agrees_with_the_table` is
        // the test that refuses that, and it walks every row.
        //
        // NAMED RATHER THAN WILDCARDED, deliberately: a new row in `routes()` must be given a home
        // in this match, and the compiler is what says so. That is the whole point of the table.
        //
        // The answer stays "not found" because that is what these paths got before the table existed
        // — they fell into the final arm below — and this refactor changes no response.
        Some(
            RouteId::EventsStream
            | RouteId::TermStream
            | RouteId::PwShots
            | RouteId::PwShot
            | RouteId::Actions
            | RouteId::Operation
            | RouteId::Mcp
            | RouteId::PanelHome
            | RouteId::PanelFile
            | RouteId::PanelPreflight
            | RouteId::StatusPage
            | RouteId::Discovery,
        ) => serde_json::json!({"ok": false, "error": "not found"}),
        None => serde_json::json!({"ok": false, "error": "not found"}),
    };
    Ok(result)
}

// ── Generic tool dispatch ────────────────────────────────────

async fn api_call_tool(state: &AppState, tool_name: &str, body: &str) -> serde_json::Value {
    let tool = match state.plugin_registry.find_tool(tool_name) {
        Some(t) => t,
        None => {
            return serde_json::json!({"ok": false, "error": format!("unknown tool: {tool_name}"), "code": "invalid_params"})
        }
    };
    let params: serde_json::Value = if body.is_empty() {
        serde_json::json!({})
    } else {
        match serde_json::from_str(body) {
            Ok(v) => v,
            Err(e) => {
                return serde_json::json!({"ok": false, "error": format!("invalid JSON body: {e}"), "code": "invalid_params"})
            }
        }
    };
    match tool.handler.call(params).await {
        Ok(result) => serde_json::json!({"ok": true, "result": result}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string(), "code": e.code()}),
    }
}

// ── API endpoint handlers ────────────────────────────────────
// Bodies extracted from handle_request's route match so dispatch stays
// auth + routing only. Same contract as before the extraction: a returned
// serde_json::Value is served as `axum::Json(...).into_response()`; an Err
// short-circuits handle_request with the already-built response (status +
// headers preserved verbatim). The Err is boxed like check_auth's — Response
// is large and clippy::result_large_err fires on a plain Result err variant.

/// Session-log reader over paths::sessions_dir().
/// HIGH(audit round): the WRITER (terminal plugin) logs to
/// paths::data_dir()/sessions — on registry-first installs the
/// exe dir is NOT the data dir (d1: D:\Summrise vs C:\ProgramData\
/// Summrise), and these endpoints scanned an empty dir: the audit
/// panel was permanently blind. Read the same dir; also honors
/// the "zero current_exe() guessing outside paths.rs" rule.
fn sessions_logger() -> crate::session_log::SessionLogger {
    let dir = crate::paths::sessions_dir();
    crate::session_log::SessionLogger::new(dir)
}

/// GET /api/sessions — audit trail: session list with terminal state
/// (round-56). The logger lives in the terminal plugin's private field —
/// read the same directory directly (cheap: one file per session).
fn api_sessions_list() -> serde_json::Value {
    let logger = sessions_logger();
    let list: serde_json::Value = logger
        .list_sessions_with_identity()
        .iter()
        .map(|row| match &row.identity {
            Some((kind, label)) => {
                serde_json::json!({ "id": row.sid, "state": row.state, "kind": kind, "label": label })
            }
            // Recorded before identity existed. The KEYS ARE OMITTED rather than
            // sent as `null` or as a placeholder: a consumer must be able to tell
            // "this device does not know" from "this session was a pty".
            None => serde_json::json!({ "id": row.sid, "state": row.state }),
        })
        .collect();
    serde_json::json!({ "ok": true, "sessions": list })
}

/// GET /api/sessions/{sid} — the recorded events of one session (round-68):
/// events_of() existed for /api/sessions but no endpoint called it — the
/// durable audit corpus was write-only, unqueryable by the panel or MCP.
/// This reads the session's jsonl (permanent, survives agent restarts).
fn api_session_events(p: &str) -> Result<serde_json::Value, Box<Response>> {
    // round-87: the old literal "/api/sessions/{sid}" arm never matched a real
    // session id (exact-string match) — the audit endpoint 404'd for every
    // session. Guard-arm route; the sid charset guard now lives in
    // `session_id_from_path`, shared with the control route.
    let sid = session_id_from_path(p)?;
    let logger = sessions_logger();
    // `found` distinguishes "this session recorded nothing" from "there is no
    // readable record for it" — see `SessionLogger::events_of`. `ok` stays true
    // either way: the REQUEST succeeded.
    let rec = logger.events_of(&sid);
    Ok(serde_json::json!({
        "ok": true,
        "id": sid,
        "found": rec.found,
        // WHERE THE RECORD BEGINS. The trail is trimmed to ~2000 lines when a
        // session closes, so a long session's head is discarded by design and
        // the survivors alone cannot say so. `first_seq > 1` is that fact —
        // stated rather than left for a consumer to infer from a seq gap, and
        // about the RECORD rather than about a cause (a trim and a lost write
        // both mean "you are not seeing the beginning").
        "first_seq": rec.first_seq,
        "events": rec.events,
    }))
}

/// `POST /api/sessions/{sid}/approval` — decide the command waiting at the gate.
///
/// Body: `{"id": "<request id>", "approve": true|false}`. The `id` is required and
/// must match the live request, so a panel tab that rendered an OLD prompt cannot
/// approve a DIFFERENT command that arrived after it — the operator's "yes" is
/// always attached to the command they actually read.
///
/// `approve` is required for the same reason `holder` is on the control route: a
/// decision with no direction is not a decision, and defaulting it would mean
/// guessing whether silence meant "run it".
///
/// Nothing is logged to the audit trail here. The handoff is logged because it
/// changes WHO drives; an approval changes only whether one command runs, and
/// that command logs its own `command/start` moments later — so an extra event
/// would duplicate the record rather than complete it.
async fn api_session_approval(
    state: &AppState,
    sid: &str,
    body: &str,
) -> Result<serde_json::Value, Box<Response>> {
    let v = parse::json_body(body, |_| "invalid JSON".to_string())?;
    let id = parse::optional_trimmed_string(&v, "id")
        .ok_or_else(|| parse::invalid_params_response("id is required".to_string()))?;
    let approve = parse::optional_bool(&v, "approve").ok_or_else(|| {
        parse::invalid_params_response("approve is required (true or false)".to_string())
    })?;
    // "and remember this one". Deliberately a BOOLEAN and not a prefix: the
    // prefix is derived server-side from the command the operator was SHOWN (see
    // `term_decide_approval`), so a client cannot widen its own permissions — the
    // most it can do is ask to remember what was already on screen. Absent means
    // "no grant", the conservative default for a request that did not mention it.
    let grant = parse::optional_bool(&v, "grant").unwrap_or(false);

    // The grants in force BEFORE, so the record can name what this decision
    // newly allowed rather than repeating the whole list.
    let grants_before = state
        .terminal_mgr
        .term_approval_grants(sid)
        .await
        .unwrap_or_default();

    // Read the request BEFORE deciding it, because the decision clears it — and
    // the command is what makes the record meaningful. "approved" without the
    // command would say a yes happened without saying what to.
    let decided_command = state
        .terminal_mgr
        .term_pending_approval(sid)
        .await
        .ok()
        .flatten()
        .filter(|p| p.id == id)
        .map(|p| p.command)
        .unwrap_or_default();

    let decided = state
        .terminal_mgr
        .term_decide_approval(sid, &id, approve, grant)
        .await
        .map_err(|e| {
            // A missing session is a client error: the panel can hold a stale sid.
            parse::invalid_params_response(e.to_string())
        })?;

    // Only when a decision actually landed. `decided == false` means the request
    // had already gone — recording a yes that decided nothing would be a false
    // entry in the one log that must not contain them.
    if decided {
        sessions_logger().log_approval(
            sid,
            if approve { "approved" } else { "refused" },
            &decided_command,
        );
        // A grant is a SEPARATE fact from the approval and gets its own event:
        // "the operator said yes to this command" and "this word now runs
        // unasked" are different statements, and a reader chasing why a later
        // command did NOT prompt needs the second one. Only when a grant is
        // genuinely new — re-approving an already-allowed word is not news.
        if approve && grant {
            let after = state
                .terminal_mgr
                .term_approval_grants(sid)
                .await
                .unwrap_or_default();
            if let Some(added) = after.iter().find(|g| !grants_before.contains(g)) {
                sessions_logger().log_approval(sid, "granted", added);
            }
        }
    }

    // Report the grants now in force, so the panel renders them from the
    // decision's own response rather than waiting for its next poll — a grant the
    // operator cannot see immediately is one they cannot judge.
    let grants = state
        .terminal_mgr
        .term_approval_grants(sid)
        .await
        .unwrap_or_default();

    // PUSH on every decision, so the badge clears and the prompt disappears
    // immediately rather than at the next poll — a resolved question that stays
    // on screen is a question the operator will try to answer twice. Emitted for
    // `decided: false` too: "someone already answered" and "it expired" both
    // change what the panel should draw.
    state
        .event_bus
        .emit_term_output(serde_json::json!({ "ev": "sessions-changed" }));

    // `decided: false` is NOT an error at the HTTP level — the session exists and
    // the request was well formed, there was simply nothing waiting. The caller
    // distinguishes it, because "someone already answered" and "it timed out" are
    // both things the operator should be told rather than shown a success.
    Ok(serde_json::json!({
        "ok": true,
        "id": sid,
        "decided": decided,
        "approval_grants": grants,
    }))
}

/// `POST /api/sessions/{sid}/grants` — revoke an approval grant, or all of them.
///
/// Body: `{"grant": "<word>"}` to revoke one, or `{"all": true}` for every grant
/// on the session. A request that names NEITHER is rejected rather than treated
/// as "all": revoking everything is a consequential act that must be asked for
/// explicitly, not the default for a malformed body.
///
/// Every response carries the grants now in force, so the caller renders the
/// server's answer rather than its own guess about what a revoke did.
async fn api_session_grants(
    state: &AppState,
    sid: &str,
    body: &str,
) -> Result<serde_json::Value, Box<Response>> {
    let v = parse::json_body(body, |_| "invalid JSON".to_string())?;
    let all = parse::optional_bool(&v, "all").unwrap_or(false);
    let grant = parse::optional_trimmed_string(&v, "grant");
    if !all && grant.is_none() {
        return Err(parse::invalid_params_response(
            "provide grant (a word) or all (true)".to_string(),
        ));
    }

    // `all` wins when both are given: it is the strictly larger act, so honouring
    // the narrower one would quietly do less than the operator asked.
    let target = if all { None } else { grant.as_deref() };
    let removed = state
        .terminal_mgr
        .term_revoke_grants(sid, target)
        .await
        .map_err(|e| {
            // A missing session is a client error: the panel can hold a stale sid.
            parse::invalid_params_response(e.to_string())
        })?;
    // Taking a permission back is as much evidence as granting it. An empty
    // subject records "all", which is why `approval()` keeps "" meaningful
    // rather than dropping it.
    if removed > 0 {
        sessions_logger().log_approval(sid, "revoked", target.unwrap_or(""));
    }

    let grants = state
        .terminal_mgr
        .term_approval_grants(sid)
        .await
        .unwrap_or_default();
    Ok(serde_json::json!({ "ok": true, "id": sid, "approval_grants": grants }))
}

/// Extract and validate a session id from `/api/sessions/{sid}[...]`.
///
/// Shared by every session route. The charset guard is not cosmetic: the sid
/// flows into a FILE PATH for the audit endpoints (`events_of` →
/// `{dir}/{sid}.jsonl`), and the forward-slash split alone let a backslash
/// (0x5C, accepted in the request-target by the http crate) traverse on Windows:
/// `/api/sessions/..%5C..%5Cfoo` read `{dir}/../../foo.jsonl` (round-116).
/// Session ids are hex, so anything outside this charset is not a session.
///
/// Extracted from `api_session_events` when the control route became the second
/// consumer: a second hand-written copy of a SECURITY guard is how one of them
/// quietly loses it.
fn session_id_from_path(p: &str) -> Result<String, Box<Response>> {
    let sid = p
        .strip_prefix("/api/sessions/")
        .and_then(|s| s.split('/').next())
        .unwrap_or("")
        .to_string();
    if sid.is_empty() || !sid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(Box::new(built_response(
            StatusCode::BAD_REQUEST,
            "application/json",
            Body::from(r#"{"ok":false,"error":"invalid session id"}"#),
        )));
    }
    Ok(sid)
}

/// `POST /api/sessions/{sid}/control` — hand the session's keyboard to a person
/// or back to the AI (design §D5, the control plane's first piece).
///
/// The human's surface is the PANEL, so this is an HTTP route rather than an MCP
/// tool: the AI learns about a hold the moment it tries to execute (a typed
/// `human_in_control` refusal), which is the information it actually needs. It
/// does not need to poll, so no tool was added — and adding one would have meant
/// mirroring it into the gateway's MCP registry for a consumer that does not
/// exist. `terminal_list` already carries `held_by_human` for anything that does
/// want to look.
///
/// Body: `{"holder": "human" | "ai"}`. Absent/blank is rejected rather than
/// defaulted: guessing which way a malformed request wanted to move the keyboard
/// is exactly the wrong thing to be lenient about.
async fn api_session_control(
    state: &AppState,
    sid: &str,
    body: &str,
) -> Result<serde_json::Value, Box<Response>> {
    let v = parse::json_body(body, |_| "invalid JSON".to_string())?;

    // Both fields are OPTIONAL and at least one is required: this route patches
    // the session's governance, and a request that mentions neither is a client
    // bug rather than a no-op. Absent fields are left alone — never defaulted —
    // so "hand the keyboard back" cannot silently disarm the approval gate, and
    // "arm the gate" cannot silently hand the keyboard over.
    let holder = parse::optional_trimmed_string(&v, "holder");
    let approval = parse::optional_bool(&v, "approval_required");
    // The GOAL is the operator's statement of intent — the design's dispatch beat.
    // Read with the `present` distinction preserved: an ABSENT key leaves the goal
    // alone, while an empty string CLEARS it. Those are different acts and a
    // partial patch must not be able to do the second by accident.
    let goal_present = v.get("goal").is_some();
    let goal = v
        .get("goal")
        .and_then(|g| g.as_str())
        .map(|s| s.to_string());
    if holder.is_none() && approval.is_none() && !goal_present {
        return Err(parse::invalid_params_response(
            "provide holder (\"human\"|\"ai\"), approval_required (bool) and/or goal (string)"
                .to_string(),
        ));
    }
    // A non-string goal is a client bug, not a clear. Treated as absent so the
    // other fields in the same request still apply.
    if v.get("goal").is_some() && goal.is_none() {
        return Err(parse::invalid_params_response(
            "goal must be a string (empty clears it)".to_string(),
        ));
    }

    let human = match holder.as_deref() {
        None => None,
        Some("human") => Some(true),
        Some("ai") => Some(false),
        Some(other) => {
            return Err(parse::invalid_params_response(format!(
                "holder must be \"human\" or \"ai\", got {other:?}"
            )))
        }
    };

    // A missing session is a client error, not a 500: the panel can hold a stale
    // sid after the session was closed or reaped.
    let not_found = |e: DeviceError| parse::invalid_params_response(e.to_string());

    let mut held = None;
    if let Some(h) = human {
        held = Some(
            state
                .terminal_mgr
                .term_set_control(sid, h)
                .await
                .map_err(not_found)?,
        );

        // Record the handoff in the session's audit trail. Logged HERE, at the
        // point the decision is made, rather than inside the manager: the manager
        // owns the in-memory hold and must not grow a dependency on the log, while
        // this is the layer that knows a decision actually happened.
        //
        // Best-effort by design — `log_control` returns nothing, because a log
        // failure must not cost the operator the keyboard. The consequence is that
        // the record can be MISSING an event; it can never invent one.
        sessions_logger().log_control(sid, if h { "human" } else { "ai" });
    }

    let approval_required_before = state.terminal_mgr.term_approval_required(sid).await.ok();
    let mut approval_required = None;
    if let Some(req) = approval {
        let now = state
            .terminal_mgr
            .term_set_approval_required(sid, req)
            .await
            .map_err(not_found)?;
        // ARMING THE GATE IS EVIDENCE. Recorded only when the mode actually
        // CHANGED: a panel that re-sends the same value on every poll would
        // otherwise bury the one transition that matters under a wall of
        // no-ops. The trail is read by a person, and a person wants events.
        if Some(now) != approval_required_before {
            sessions_logger().log_approval(sid, if now { "armed" } else { "disarmed" }, "");
        }
        approval_required = Some(now);
    }

    let mut goal_out = None;
    if let Some(text) = goal {
        let stored = state
            .terminal_mgr
            .term_set_goal(sid, &text)
            .await
            .map_err(not_found)?;
        // Recorded in the audit trail as its own event: the live goal dies with
        // the session, but "this session was FOR x" is history. Logged AFTER the
        // store succeeded, and the CLEARED case logs an empty string rather than
        // nothing — "someone withdrew the objective" is itself a fact a reader
        // needs, and silence would leave the previous goal looking current.
        sessions_logger().log_goal(sid, stored.as_deref().unwrap_or(""));
        goal_out = Some(stored);
    }

    Ok(serde_json::json!({
        "ok": true,
        "id": sid,
        "held_by_human": held,
        "approval_required": approval_required,
        "goal": goal_out,
    }))
}

/// GET /api/logs — the device's own logs, so a remote client can see WHY the
/// agent behaved oddly without asking someone to open files (or guessing a path
/// and `cat`-ing it over a PTY).
///
/// THE PATH IS THE BUG THIS FIXES. The handler used to read
/// `exe_dir()/summrise-update.log`, but layout v2 MOVES that file — along with
/// `agent.log`, `installer.log`, `install-result.txt` and `startup.log` — into
/// `DataDir\logs` (`paths.rs`'s migration list owns that move, and its test
/// pins it). So the route read a path the migration had just emptied and could
/// only ever answer `""` on a v2 device. Every writer had already followed the
/// move: the update swap script writes `{logs}\summrise-update.log`, `filelog.rs`
/// rotates `agent.log` there, and `mcp_client` caps `mcp_diag.log` there.
///
/// `logs_dir()` is the single resolution point, so this reads where the writers
/// write and a future move touches one place again.
///
/// The reply carries the TAIL of each file: an operator wants the newest lines,
/// and `agent.log` is size-rotating so it is bounded but not small. `tail` cuts
/// on a char boundary with a hard byte budget (the naive `&s[len - n..]` panics
/// mid-character, which this crate has paid for three times).
/// How many boots `/api/boots` will return. The file keeps more (see
/// `runstate::BOOT_HISTORY_MAX`); this is what one reply carries, bounded for the same reason
/// every other list route here is.
const BOOTS_IN_REPLY: usize = 50;

/// The window the summary counts over. A DAY is the unit an operator reasons in ("has this
/// thing been stable today?"), and it is long enough that a normal update restart does not
/// read as flapping while a device restarting hourly is unmistakable.
const BOOT_SUMMARY_WINDOW_SECS: u64 = 24 * 60 * 60;

/// `GET /api/boots` — the device's restart history, newest first.
///
/// `boots` is EMPTY, never absent, for an install that has not booted a build that records
/// history: the panel's card then says it has nothing to show rather than rendering a list it
/// cannot explain. Each record is the device's own shape (see `runstate::record_boot`) and is
/// passed through unchanged — this route is a reader, not a second opinion about the format.
fn api_boots() -> serde_json::Value {
    let boots = crate::runstate::recent_boots(&crate::paths::data_dir(), BOOTS_IN_REPLY);
    serde_json::json!({
        "ok": true,
        "boots": boots,
        // The counts the panel's card and its strip key on, computed HERE so "how many restarts,
        // how many of them crashes, in the last day" is ONE rule rather than one per client —
        // and computed against the same clock that wrote the stamps.
        "summary": crate::runstate::summary(&boots, BOOT_SUMMARY_WINDOW_SECS),
    })
}

/// `GET /api/vitals/history` — the agent's own CPU/memory series, oldest first.
///
/// `samples` is EMPTY, never absent, for a host that reports no vitals (every non-Windows
/// build) or for a device that has not been up long enough to have taken a reading: the
/// panel then says the host does not report vitals rather than drawing an empty chart that
/// reads as a broken feature.
///
/// `interval_secs` rides along because the series' SPACING is part of its meaning — a panel
/// that assumed a cadence would draw the same shape for 30 s and 5 min samples, and the
/// chart is the thing an operator reads a rate off.
fn api_vitals_history() -> serde_json::Value {
    let samples = crate::metrics::history();
    let span_secs = match (samples.first(), samples.last()) {
        (Some(a), Some(b)) => b.ts_ms.saturating_sub(a.ts_ms) / 1000,
        _ => 0,
    };
    serde_json::json!({
        "ok": true,
        "interval_secs": crate::metrics::SAMPLE_INTERVAL_SECS,
        "span_secs": span_secs,
        "samples": samples,
    })
}

/// `GET /api/update` — the update state a panel needs, with the update plugin's own rules.
///
/// The route is a THIN READER on purpose: `newer`, the rollback pin and the busy marker are
/// decided inside `plugins::update` (see `update_status`), so the answer here and the tool's
/// behaviour cannot drift apart.
async fn api_update(state: &AppState) -> serde_json::Value {
    let channel = state.config_snapshot().platform.download_url.clone();
    crate::plugins::update::update_status(channel).await
}

/// THE HTTP DOOR'S ERROR ENVELOPE, in one place for the add path: a body this surface could read
/// but not act on is `200 + {"ok":false,"error":<reason>,"code":"invalid_params"}` — the reason
/// rendered verbatim by the panel, the code the machine routes on (see the note on the dispatch
/// table: this shape is a contract two clients already compensate for).
///
/// THE ENVELOPES DIFFER BY DESIGN, THE RULES DO NOT. The MCP door answers
/// `DeviceError::InvalidParams { message: <the same reason> }`; that is the transports' difference,
/// and what both now lack is their own copy of the RULES.
fn invalid_params(reason: impl Into<String>) -> serde_json::Value {
    serde_json::json!({"ok": false, "error": reason.into(), "code": "invalid_params"})
}

/// The monitor form's body — `{"host": "...", "port": 22}` plus optional `path`/`expect` — parsed
/// into the ONE validator's input.
///
/// THIS FUNCTION IS AN ADAPTER AND NOTHING ELSE. It used to carry a second copy of the rules (its
/// own port range check, its own message, and the monitor.rs sentence in a shortened form), so the
/// same mistake answered differently here and through `monitor_add`. What stays here is what is
/// genuinely this door's: the WIRE shape. A missing or non-numeric port is a wire fact — the field
/// is not there — and even that sentence is `monitor::PORT_REQUIRED_REASON`, because `monitor.rs`
/// owns the rules and their wording (including why a missing port is refused rather than defaulted)
/// and a caller must read one answer, not two.
///
/// Kept as its own function rather than folded into `api_monitor_add`: the wire parse is a seam a
/// test can hit without the route's state, and the route then reads as the three lines it is.
fn monitor_form(body: &str) -> Result<crate::monitor::TargetSpec, serde_json::Value> {
    let v: serde_json::Value = serde_json::from_str(if body.is_empty() { "{}" } else { body })
        .map_err(|e| invalid_params(format!("invalid JSON body: {e}")))?;
    let host = v.get("host").and_then(|h| h.as_str()).unwrap_or("");
    let Some(port) = v.get("port").and_then(|p| p.as_u64()) else {
        return Err(invalid_params(crate::monitor::PORT_REQUIRED_REASON));
    };
    // An HTTP path turns the check into a real GET (see `monitor::probe_http`); absent means the
    // TCP probe this instrument started as.
    let path = v.get("path").and_then(|p| p.as_str()).unwrap_or("");
    // The expected text (optional): a page that answers 200 without it counts as down.
    let expect = v.get("expect").and_then(|p| p.as_str()).unwrap_or("");
    crate::monitor::TargetSpec::parse(host, port, path, expect).map_err(invalid_params)
}

fn api_monitor_add(body: &str) -> serde_json::Value {
    let spec = match monitor_form(body) {
        Err(envelope) => return envelope,
        Ok(spec) => spec,
    };
    match crate::monitor::add_target_full(&crate::paths::data_dir(), &spec) {
        Ok(t) => serde_json::json!({"ok": true, "target": t}),
        // The reason goes to the operator verbatim: it is written for a form.
        Err(reason) => invalid_params(reason),
    }
}

fn api_monitor_remove(body: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(if body.is_empty() { "{}" } else { body })
        .unwrap_or_else(|_| serde_json::json!({}));
    let Some(id) = v.get("id").and_then(|i| i.as_str()) else {
        return serde_json::json!({"ok": false, "error": "an id is required", "code": "invalid_params"});
    };
    // HONEST ABOUT WHAT HAPPENED: removing something that was not watched is reported as such
    // rather than as a success (the panel then refreshes to the truth either way).
    serde_json::json!({"ok": true, "removed": crate::monitor::remove_target(&crate::paths::data_dir(), id)})
}

async fn api_monitor_probe(body: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(if body.is_empty() { "{}" } else { body })
        .unwrap_or_else(|_| serde_json::json!({}));
    let Some(id) = v.get("id").and_then(|i| i.as_str()) else {
        return serde_json::json!({"ok": false, "error": "an id is required", "code": "invalid_params"});
    };
    match crate::monitor::probe_once(id).await {
        Some(p) => {
            // THE ROUTE CARRIES THE CRITERION TOO (round 217). The MCP tool returns `expect` and its own
            // comment says why it is load-bearing: "`expect_ok: false` is unreadable without the text the
            // probe wanted, so the target's own expectation travels with the result." This route — the
            // panel's "check now" — omitted it, so ONE probe answered a different shape depending on which
            // door it came through, and neither shape was pinned by a fixture or a test.
            // THE SAME ENVELOPE THE MCP TOOL RETURNS (round 218). One probe, one shape, whichever door
            // it comes through — this was a hand-copy of the tool's block until round 217 made them agree.
            crate::monitor::probe_envelope(id, p)
        }
        None => {
            serde_json::json!({"ok": false, "error": format!("not watching {id}"), "code": "invalid_params"})
        }
    }
}

fn api_logs() -> serde_json::Value {
    let dir = crate::paths::logs_dir();
    // The update log is the one this route was invented for; the others are the
    // agent's own narration (55 `tracing::` sites land in agent.log and nothing
    // read it before), the MCP bridge's diagnostics, and BOOT.
    //
    // `startup.log` IS THE ONE THAT ANSWERS "WHY IS EVERY CLIENT 401ing". The
    // quarantine/rotation narration goes through `out!`/`eout!` (`main.rs`), which
    // writes there — and this doc comment has listed `startup.log` among the files
    // layout v2 moved into this directory since the route was written, while the
    // served set did not include it. So the one place that records a rotated
    // `device_token` was the one place the operator's log card could not read, and
    // the symptom it explains is total and confusing. Names, not paths: adding a
    // fifth is one entry here.
    let read = |name: &str| -> serde_json::Value {
        match std::fs::read_to_string(dir.join(name)) {
            Ok(text) => serde_json::json!({
                "name": name,
                "present": true,
                // 64 KiB per file: four files fit comfortably in one reply
                // while staying far below the clip budget the panel renders.
                "log": crate::text::tail(&text, 64 * 1024),
            }),
            // ABSENT, not empty: "the agent never wrote this" and "it wrote
            // nothing" are different facts and a reader must be able to tell
            // them apart — the same discipline the evidence feed follows.
            Err(_) => serde_json::json!({ "name": name, "present": false, "log": "" }),
        }
    };
    serde_json::json!({
        "ok": true,
        "dir": dir.to_string_lossy(),
        "logs": [
            read("agent.log"),
            read("summrise-update.log"),
            read("mcp_diag.log"),
            read("startup.log"),
        ],
    })
}

/// GET /api/settings — read the runtime-configurable values (round-69).
/// buffer_mb is the per-session output buffer cap — the panel's settings
/// writes it here; it takes effect for NEW output (existing buffers keep
/// their size), persists to config.yaml, survives restarts.
async fn api_settings_get(state: &AppState) -> serde_json::Value {
    // Write-through (audit A4): console_url comes from the LIVE snapshot —
    // the same source the PUT handler persists through update_config (the
    // old disk re-read here was a second source of truth that could
    // disagree with memory).
    let console_url = state.config_snapshot().platform.console_url;
    // Tunnel state: tunnel.yml present + cloudflared running? Lets the
    // Settings page show the persisted state after a refresh (the
    // Gateway card must not blank out once connected).
    let tunnel_configured = crate::paths::tunnel_file().exists();
    // Blocking-subprocess audit: tasklist is a synchronous child
    // process — run it on the blocking pool so the polled-every-15s
    // /api/status sibling handler never stalls the async runtime
    // workers. On non-Windows (dev/CI) tasklist doesn't exist and
    // this degrades to false, as before.
    let tunnel_running = tokio::task::spawn_blocking(|| {
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq cloudflared.exe"])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .to_lowercase()
                    .contains("cloudflared")
            })
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false);
    // Round-358: live memory capacity (Settings page Memory card edits it
    // via PUT below; bytes reported in MiB for the UI).
    let mem = state.memory.limits();
    // USAGE as well as the caps. The operator could lower a cap to below the
    // current contents — after which the device silently starts evicting the
    // OLDEST knowledge — with nothing in the UI able to show it was about to
    // happen. These two numbers come from the SAME store the cap is enforced
    // against, so the meter cannot disagree with the eviction it explains.
    //
    // Reported UNCONDITIONALLY, including at zero, because the caps are: a
    // consumer must be able to divide usage by cap without special-casing an
    // absent field. (Contrast `pending_approvals` on /api/status, which IS
    // omitted at zero — there a non-zero value is an EVENT, whereas here zero
    // is a legitimate reading.)
    let mem_entries = state.memory.len();
    let mem_bytes = state.memory.total_bytes_live();
    serde_json::json!({
        "ok": true,
        "buffer_mb": state.terminal_buf_bytes.load(std::sync::atomic::Ordering::Relaxed) / (1024 * 1024),
        "console_url": console_url,
        "tunnel_configured": tunnel_configured,
        "tunnel_running": tunnel_running,
        "memory_max_entries": mem.max_entries,
        "memory_max_bytes_mb": mem.max_bytes / (1024 * 1024),
        "memory_retention_days": mem.retention_days,
        "memory_entries": mem_entries,
        "memory_bytes": mem_bytes,
    })
}

/// PUT /api/settings — write the runtime-configurable values (round-69).
/// Write-through (audit A4): changes land in the LIVE config AND config.yaml
/// in one update_config step. Err carries the ready-made error response,
/// byte-identical to the pre-extraction early return (including its
/// HTTP-200 Json shape).
fn api_settings_put(state: &AppState, body: &str) -> Result<serde_json::Value, Box<Response>> {
    let v: serde_json::Value = parse::json_body(body, |e| format!("invalid JSON: {e}"))?;
    // stage-n (settings audit): a PUT may legitimately carry ONLY ONE
    // of the keys — the old code reset buffer_mb to 8 whenever it was
    // ABSENT (a console-only save silently clobbered a user's 64).
    // Missing key = leave unchanged; empty console_url string =
    // explicit clear (unchanged semantics).
    let mb = v
        .get("buffer_mb")
        .and_then(|b| b.as_u64())
        .map(|x| (x as usize).clamp(1, 64));
    if let Some(mb) = mb {
        state
            .terminal_buf_bytes
            .store(mb * 1024 * 1024, std::sync::atomic::Ordering::Relaxed);
    }
    // `Option<Option<String>>`: the OUTER option is "the request speaks to
    // console_url" (absent ⇒ leave the binding alone), the inner one is the
    // value (blank ⇒ explicit clear).
    let console_url = v
        .get("console_url")
        .map(|_| parse::optional_trimmed_string(&v, "console_url"));
    /** Parse memory capacity settings from a JSON value.
     *  Returns (entries, bytes, retention, changed) where changed indicates
     *  whether any key was present (absent = leave unchanged). */
    fn parse_memory_settings(
        v: &serde_json::Value,
    ) -> (Option<usize>, Option<usize>, Option<Option<u64>>, bool) {
        let mem_entries = v
            .get("memory_max_entries")
            .and_then(|b| b.as_u64())
            .map(|x| (x as usize).max(1));
        let mem_bytes = v
            .get("memory_max_bytes_mb")
            .and_then(|b| b.as_u64())
            .map(|x| (x as usize).max(1) * 1024 * 1024);
        let mem_retention: Option<Option<u64>> = v.get("memory_retention_days").map(|val| {
            val.as_u64().filter(|&n| n > 0).or_else(|| {
                val.as_str()
                    .and_then(|s| s.trim().parse::<u64>().ok().filter(|&n| n > 0))
            })
        });
        let mem_changed = mem_entries.is_some() || mem_bytes.is_some() || mem_retention.is_some();
        (mem_entries, mem_bytes, mem_retention, mem_changed)
    }

    // Write-through (audit A4): merge onto the CURRENT in-process snapshot
    // and persist via update_config — the runtime buffer cap, the in-process
    // config and config.yaml all move together (the old code rewrote the
    // file from a disk re-read and left state.config stale until restart).
    // Best-effort persist (as before): a read-only install dir must not fail
    // the PUT — the runtime value already took effect. With no config_path
    // (dev invocations), update_config still updates memory and writes
    // nothing.
    let (mem_entries, mem_bytes, mem_retention, mem_changed) = parse_memory_settings(&v);

    if mb.is_some() || console_url.is_some() || mem_changed {
        let mut cfg = state.config_snapshot();
        if let Some(mb) = mb {
            cfg.terminal.buffer_mb = mb as u32;
        }
        if let Some(url) = console_url {
            cfg.platform.console_url = url;
        }
        if mem_changed {
            // Live-first: retune the running store (enforces immediately),
            // then persist the same values so a restart agrees.
            let cur = state.memory.limits();
            let new = MemoryLimits {
                max_entries: mem_entries.unwrap_or(cur.max_entries),
                max_bytes: mem_bytes.unwrap_or(cur.max_bytes),
                retention_days: mem_retention.unwrap_or(cur.retention_days),
            };
            state.memory.set_limits(new);
            cfg.memory.max_entries = Some(new.max_entries);
            cfg.memory.max_bytes = Some(new.max_bytes);
            cfg.memory.retention_days = new.retention_days;
        }
        let _ = state.update_config(cfg, true);
    }
    let mem = state.memory.limits();
    Ok(
        serde_json::json!({ "ok": true, "buffer_mb": mb.unwrap_or_else(|| {
            state.terminal_buf_bytes.load(std::sync::atomic::Ordering::Relaxed) / (1024 * 1024)
        }),
            "memory_max_entries": mem.max_entries,
            "memory_max_bytes_mb": mem.max_bytes / (1024 * 1024),
            "memory_retention_days": mem.retention_days,
        }),
    )
}

/// POST /api/gateway/connect (Settings page card): persist console_url, then
/// register the device with the gateway (reg-key exchange) and optionally
/// provision the free cloudflared tunnel. Returns per-step results so the
/// page can show what happened.
async fn api_gateway_connect(
    state: &AppState,
    body: &str,
) -> Result<serde_json::Value, Box<Response>> {
    // Bare wording here (no serde detail) — documented and preserved.
    let v: serde_json::Value = parse::json_body(body, |_| "invalid JSON".to_string())?;
    // ONE evaluation of the console_url rule. The reported binding and the
    // PERSISTED binding used to be computed by two independent copies of it —
    // they agreed only because both copies happened to match, so editing one
    // would have made the response disagree with what the device stored.
    // `Option<Option<String>>`: outer = "the request speaks to console_url".
    let console_url_patch = v
        .get("console_url")
        .map(|_| parse::optional_trimmed_string(&v, "console_url"));
    let console_url = console_url_patch.clone().flatten();
    let reg_key = parse::optional_trimmed_string(&v, "reg_key");
    let want_tunnel = v.get("tunnel").and_then(|t| t.as_bool()).unwrap_or(false);
    // 1. Persist console_url — write-through (audit A4): merge onto the
    //    CURRENT in-process snapshot and persist via update_config so memory
    //    and config.yaml move together in one step. The HIGH(audit)
    //    device_token-survival invariant holds by construction: the snapshot
    //    always carries the boot device_token (fail-closed auth depends on
    //    it), so the persisted file can NEVER lose it — the old
    //    reload-from-disk-with-stale-fallback could rewrite the file from a
    //    STALE snapshot on a transient load failure.
    //    Only touch console_url when the request actually speaks to it:
    //    absent = keep binding, "" = clear (the partial-PUT semantics
    //    audit flagged — a reg-key-only request used to silently
    //    unbind the gateway).
    //    Best-effort persist (matches the old `let _ =`); with no
    //    config_path (dev/tests) update_config still updates memory and
    //    writes nothing.
    let mut cfg = state.config_snapshot();
    if let Some(val) = console_url_patch {
        cfg.platform.console_url = val;
    }
    let _ = state.update_config(cfg, true);
    // 2. If a reg key was given, exchange it at the gateway for the
    //    Cloudflare API token (the gateway's saved credential) — this
    //    registers the device AND enables tunnel provisioning.
    let mut registered = false;
    let mut cf_token = String::new();
    if let (Some(url), Some(key)) = (console_url.as_deref(), reg_key.as_deref()) {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build();
        if let Ok(client) = client {
            let r = client
                .post(format!(
                    "{}/api/install/tunnel-token",
                    url.trim_end_matches('/')
                ))
                .header("content-type", "application/json")
                // MED(audit round): a key containing \" or , used to
                // corrupt/inject fields in the hand-built JSON body.
                .body(serde_json::json!({ "key": key }).to_string())
                .send()
                .await;
            if let Ok(resp) = r {
                if let Ok(j) = resp.json::<serde_json::Value>().await {
                    if let Some(t) = j.get("apiToken").and_then(|x| x.as_str()) {
                        cf_token = t.to_string();
                        registered = true;
                    }
                }
            }
        }
    }
    // 3. Optional tunnel: write tunnel.yml + spawn cloudflared with
    //    the token (free tier). Best-effort; report the outcome. The
    //    ingress follows the configured bind port (custom ports 502
    //    otherwise).
    let mut tunnel_status = "skipped".to_string();
    if want_tunnel && !cf_token.is_empty() {
        let port = state.config_snapshot().server.port;
        // The proxy fallback's host comes from CONFIGURATION (round 45 of the standing goal): the repository no longer
        // carries a hardcoded production URL, and a device with no download site configured simply has no fallback.
        let download_url = state.config_snapshot().platform.download_url.clone();
        tunnel_status =
            crate::tunnel::provision_tunnel(&cf_token, port, download_url.as_deref()).await;
    } else if want_tunnel {
        tunnel_status = "no cf token (register first or set CLOUDFLARE_API_TOKEN)".to_string();
    }
    Ok(serde_json::json!({
        "ok": true,
        "registered": registered,
        "console_url": console_url,
        "tunnel": tunnel_status,
    }))
}

/// GET /api/plugins/status — plugin management (round-admin-ui): the panel's
/// plugins page polls the playwright-mcp running state here.
async fn api_plugins_status(state: &AppState) -> serde_json::Value {
    let mut obj = serde_json::json!({ "ok": true, "playwright": state.playwright.status().await });
    // P2-4: same boxed manifest as /api/status (advisory, omitted when absent).
    if let Some(boxed) = boxed_versions() {
        obj["boxed_versions"] = boxed;
    }
    obj
}

/// POST /api/plugins/playwright/start — playwright-mcp process control for
/// the panel's plugins page.
async fn api_playwright_start(state: &AppState) -> Result<serde_json::Value, Box<Response>> {
    run_playwright_op(state.playwright.start()).await
}

/// POST /api/plugins/playwright/stop — playwright-mcp process control for
/// the panel's plugins page.
async fn api_playwright_stop(state: &AppState) -> Result<serde_json::Value, Box<Response>> {
    run_playwright_op(state.playwright.stop()).await
}

/// Run a playwright manager op: {ok:true, ...payload} on success, a 500
/// JSON envelope on failure. api_playwright_start/stop used to be two
/// near-identical copies of this shape.
async fn run_playwright_op(
    op: impl std::future::Future<Output = Result<serde_json::Value, impl ToString>>,
) -> Result<serde_json::Value, Box<Response>> {
    match op.await {
        Ok(v) => {
            let mut obj = v.as_object().cloned().unwrap_or_default();
            obj.insert("ok".into(), serde_json::json!(true));
            Ok(serde_json::Value::Object(obj))
        }
        // Dev builds have no bundled node.exe — fail loudly with the
        // path hint instead of pretending the process started.
        Err(e) => Err(Box::new(built_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "application/json",
            Body::from(serde_json::json!({ "ok": false, "error": e.to_string() }).to_string()),
        ))),
    }
}

// ── SSE event stream ─────────────────────────────────────────

/// stage-n SSE audit LOW: bound concurrent SSE connections so a flood of
/// viewers can't exhaust tasks/memory. 64 slots shared across /api/events
/// and /api/events/term; each slot is a permit that releases on drop.
/// P2-4: read the boxed-component version manifest (`summrise setup`/`summrise update`
/// write `<install>/etc/boxed-versions.json`). Returns None when absent or
/// unparseable — advisory only, never fail-closed.
fn boxed_versions() -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(crate::paths::boxed_versions_file()).ok()?;
    serde_json::from_str(&text).ok()
}

async fn api_status(state: &AppState) -> serde_json::Value {
    let serial = state.serial_pool.list_open_ports();
    // stage-n: health diagnostics — uptime (a low value right after an
    // update/crash is a red flag) and the live terminal session count
    // (leaked sessions show up here without needing terminal_history).
    let uptime_secs = state.started_at.elapsed().as_secs();
    let live_sessions = state.terminal_mgr.term_list().await.len();
    let relay_view = state.relay.lock().map(|g| g.clone()).unwrap_or_default();
    // How many commands are WAITING on a human decision, device-wide.
    //
    // Round 14 made the approval gate answerable: a question now outlives the
    // execute that asked it, so an operator who was not watching can still
    // answer inside its TTL. But the push that announces a question
    // (`sessions-changed`) terminates in an OPEN panel's renderer — so every
    // other surface that already polls THIS endpoint (the Electron tray on its
    // 30 s health poll, the console's fleet card) could not say "a decision is
    // waiting", which is the one fact that rework exists to deliver. One count
    // here turns every existing consumer into a notification surface, with no
    // new route and no change to any other endpoint.
    //
    // Deliberately a COUNT and not the requests: the detail already rides
    // `terminal_list`, and repeating it would make this endpoint a second
    // source of truth for governance state. `term_list` is cheap (it projects
    // in-memory rows) and this endpoint is already called on a 30 s cadence.
    let pending_approvals = state
        .terminal_mgr
        .term_list()
        .await
        .iter()
        .filter(|s| s.pending_approval.is_some())
        .count();
    // stage-n: device vitals (Windows: CPU delta + memory; other hosts
    // return None → fields are omitted, endpoint shape stays additive).
    //
    // THE NEWEST SAMPLE, NOT A FRESH ONE. Since the sampler owns the clock
    // (`metrics::spawn_sampler`), taking a reading HERE would shorten the delta window
    // that the sampler's CPU percentage is computed over — the two consumers would be
    // measuring different intervals and the strip's number would disagree with the last
    // point of the chart beside it. One clock, one series, one answer.
    let vitals = crate::metrics::latest();
    let mut out = serde_json::json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        // THE RELAY, AS THE DEVICE SEES IT (round 207). An operator who configured an outbound relay has exactly one question
        // about it — is my agent connected? — and it belongs in the report the panel already reads, not in a log file. A
        // device with no relay says so with a single flag rather than by omission, so the interface can tell "not configured"
        // from "configured and broken".
        "relay": {
            "configured": relay_view.configured,
            "connected": relay_view.last_ok_ms.is_some() && relay_view.consecutive_failures == 0,
            "last_ok_ms": relay_view.last_ok_ms,
            "consecutive_failures": relay_view.consecutive_failures,
            "last_error": relay_view.last_error,
        },
        // WHAT THE DEVICE IS BOUND TO, AS CONFIGURED — and the answer to a question the operator could not answer from any
        // surface: "where is this configured?" `port` was already here and nothing showed it; `host` was not here at all, so
        // the loopback bind existed only in a YAML file on the device. Both are reported now, and the panel shows them beside
        // the address it was actually reached on — which is a DIFFERENT fact (a relayed or tunnelled caller sees its own URL
        // there, and both are true).
        "host": state.config_snapshot().server.host,
        // WHERE EVERY VALUE ON THE SETTINGS PAGE COMES FROM. The operator asked for two things by name — the device token and
        // the config file — and neither was on any surface: the token was buried inside a client snippet, and the file was
        // only discoverable by knowing the install layout. `paths::config_file()` is the same answer the agent itself reads
        // from, so this cannot drift from it.
        "config_path": crate::paths::config_file().display().to_string(),
        "port": state.config_snapshot().server.port,
        "uptime_secs": uptime_secs,
        "live_sessions": live_sessions,
        "serial_ports": serial,
    });
    // Inserted AFTER construction when non-zero, never built as an `Option`
    // inside the `json!` — `json!` renders `None` as `"pending_approvals":
    // null`, not as an omitted key, so the field would be PRESENT on every
    // response and a consumer could not tell "nothing waiting" from "an older
    // agent that never sends this". Exactly the trap round 13 recorded for
    // `runs::clean`, caught here by this change's own test. The `release` field
    // below already used the insert-after shape for the same reason.
    // THE PREVIOUS BOOT'S VERDICT, for every surface that already polls this endpoint.
    //
    // NOT INSIDE THE `pending_approvals` GUARD BELOW — that is where it was first written, and the
    // field was therefore unreachable in the steady state (zero pending approvals), which is the
    // only state a healthy device is ever in. The unit tests covered `last_verdict` in isolation
    // and the endpoint test covered `pending_approvals`; nothing asserted this field ON THE
    // RESPONSE, so every gate stayed green while the feature did nothing. DELIVERED is a claim
    // about the wire, not about the function.
    //
    // Round 254 made `describe_previous` say whether the last run was REPLACED by an update or
    // CRASHED rather than leaving the operator to infer it from three numbers. But the only place
    // that line existed was `logs/startup.log` ON THE DEVICE — a field engineer's audience, not
    // the panel's, and not the console fleet card's. The tray polls THIS endpoint on a 30 s health
    // tick and the console's fleet card reads it too, so one field turns every existing consumer
    // into a surface that can say "the agent crashed last time" with no new route and no change
    // to any other endpoint — the same argument round 14 used for `pending_approvals` above.
    //
    // TWO FIELDS, TWO AUDIENCES, ONE RULE (round 256): `last_boot` is the sentence a human reads
    // (`startup.log`'s own line), and `last_boot_kind` is the same verdict as data —
    // `first-run` / `clean-exit` / `replaced` / `machine-restart` / `crashed` — so the panel and
    // the console can decide whether to SHOUT without matching English. Both come out of
    // `runstate::classify`, so they cannot disagree; `runstate`'s agreement test pins that.
    //
    // ABSENT, NOT NULL, when the install has never booted a build that wrote one: a consumer must
    // be able to tell "no verdict on record" from "a verdict that says nothing". `last_boot_kind`
    // is ABSENT on its own when the file was written by a build that recorded no kind (1.2.366) —
    // then the prose is there and the machine answer honestly is not.
    if let Some(boot) = crate::runstate::last_boot(&crate::paths::data_dir()) {
        out["last_boot"] = serde_json::json!(boot.detail);
        if let Some(kind) = boot.kind {
            out["last_boot_kind"] = serde_json::json!(kind.as_str());
        }
    }
    if pending_approvals > 0 {
        out["pending_approvals"] = serde_json::json!(pending_approvals);
    }
    // round-304: report the npm RELEASE version (written by the swap
    // scripts, agent_update + summrise.js) alongside the Cargo protocol
    // version — /api/status consumers otherwise see 1.0.145 forever
    // while the device runs 1.2.x. Omitted when absent (fresh installs).
    if let Ok(rel) = std::fs::read_to_string(crate::paths::release_marker_file()) {
        let rel = rel.trim();
        if !rel.is_empty() {
            out["release"] = serde_json::json!(rel);
        }
    }
    // P2-4: echo the boxed-component version manifest (written by
    // `summrise setup` / `summrise update` next to the install dir). Omitted when
    // absent (older installs); the file is advisory, never fail-closed.
    if let Some(boxed) = boxed_versions() {
        out["boxed_versions"] = boxed;
    }
    if let Some(cpu) = vitals.as_ref().and_then(|v| v.cpu_pct) {
        out["cpu_pct"] = serde_json::json!(cpu);
    }
    if let Some(mem) = vitals.as_ref().and_then(|v| v.mem_pct) {
        out["mem_pct"] = serde_json::json!(mem);
    }
    if let Some(mb) = vitals.as_ref().and_then(|v| v.mem_total_mb) {
        out["mem_total_mb"] = serde_json::json!(mb);
    }
    // round-103: expose the proxy secret (token-authenticated endpoint) so
    // the console can store it at registration and present X-Summrise-Auth when
    // proxying /panel/ — the agent injects the panel token only for
    // requests carrying the matching secret.
    // Write-through (audit A4): read the LIVE snapshot, not the boot copy.
    let cfg = state.config_snapshot();
    if let Some(sec) = cfg.server.proxy_secret.as_deref() {
        out["proxy_secret"] = serde_json::json!(sec);
    }
    out
}

// ── Event polling ───────────────────────────────────────────

fn api_events_poll(state: &AppState, after: u64) -> serde_json::Value {
    // Atomic snapshot: events + first/last seq under ONE lock — three
    // separate calls could see different snapshots and skip an event forever.
    let (events, first_seq, last_seq) = state.event_bus.poll_after(after);
    serde_json::json!({"ok": true, "events": events, "first_seq": first_seq, "last_seq": last_seq, "epoch": state.event_bus.epoch()})
}

// ── Plugin Spec ───────────────────────────────────────────────

/// ONE PLUGIN'S SPEC OBJECT, in one place so a test can hold it to the fixture (round 224).
///
/// `api_spec` needs an `AppState`, so a test cannot call it; this takes exactly what the object is made of and
/// both sides call it. The fixture `agent/tests/fixtures/plugin-spec.json` lists the keys the panel's
/// `SpecPlugin` interface reads, and `plugin_spec_fixture_matches_the_payload` asserts this function emits them.
fn spec_plugin_object(
    name: &str,
    display_name: &str,
    description: &str,
    tools: Vec<serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "displayName": display_name,
        "description": description,
        "tools": tools,
    })
}

fn api_spec(state: &AppState) -> serde_json::Value {
    let plugins: Vec<serde_json::Value> = state
        .plugin_registry
        .plugins
        .iter()
        .map(|p| {
            let tools: Vec<serde_json::Value> = state
                .plugin_registry
                .plugin_tools(p.name())
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "schema": t.input_schema,
                    })
                })
                .collect();
            spec_plugin_object(p.name(), p.display_name(), p.description(), tools)
        })
        .collect();

    serde_json::json!({"ok": true, "plugins": plugins})
}
#[cfg(test)]
mod tests {
    /// SERIALISES THE TESTS THAT TOUCH THE REAL DATA DIR.
    ///
    /// `paths::data_dir()` is registry-first with no test override, so a handful of endpoint
    /// tests plant files in the LIVE data dir (`logs/last-boot.txt`, `logs/boot-history.jsonl`,
    /// `logs/agent.log`) and restore what they found. Within one binary each of those files has
    /// one owner, but nothing stopped a second copy of the binary — or a future test that picks
    /// the same file — from interleaving with the first, and the symptom is the worst kind:
    /// **three of these failed once under three concurrent `cargo test` runs and every one
    /// passed alone**, which is exactly how a real regression gets dismissed as a flake
    /// (round 257 lost a release build to that reasoning). The lock costs nothing: these tests
    /// do file I/O, not timing.
    /// A TOKIO mutex, not a std one: these tests `.await` while holding it (they call the
    /// request handler), and clippy's `await_holding_lock` refuses a std guard across an await
    /// — CI caught exactly that on the first version of this lock, which is the gate working.
    static DATA_DIR_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Hold the live-data-dir lock for the rest of the test.
    async fn lock_data_dir() -> tokio::sync::MutexGuard<'static, ()> {
        DATA_DIR_TEST_LOCK.lock().await
    }

    use super::panel::{apply_bundle_hash, panel_bundle_hash};
    use super::*;
    use crate::state::AppState;
    use axum::http::Request;
    use summrise_agent_core::Config;

    const TEST_TOKEN: &str = "test-token";

    fn state() -> Arc<AppState> {
        // Fail-closed auth (round-3xx): the default test state carries a
        // token like production always does — tests exercise the gated
        // paths, and the 401 cases build their own tokenless/mismatched
        // configs explicitly.
        let mut cfg = Config::default();
        cfg.server.device_token = Some(TEST_TOKEN.into());
        Arc::new(AppState::new(cfg))
    }

    fn req(method: &str, path: &str) -> Request<Body> {
        req_with_token(method, path, TEST_TOKEN)
    }

    /// An UNAUTHENTICATED request — no Authorization header at all.
    fn req_anon(method: &str, path: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .unwrap()
    }

    /// As [`req_anon`], with a JSON body.
    ///
    /// Exists because the shape that matters for the credential rule is
    /// "a WELL-FORMED request carrying a plausible run id, and no token" — the
    /// old anonymous helper could only send an empty body, so an id could never
    /// appear in the request being refused.
    fn req_anon_with_json(method: &str, path: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn req_with_token(method: &str, path: &str, token: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(path)
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    }

    /// An AUTHENTICATED request with a JSON body — the shape every form-driven route takes
    /// (monitors, gateway connect, settings). The token is the same one `req` uses.
    fn req_with_body(method: &str, path: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(path)
            .header("Authorization", format!("Bearer {TEST_TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    /// A BODY OVER THE CAP IS 413, AND IT SAYS SO IN A `code` RATHER THAN IN PROSE. Round-60's fix drew
    /// the line between a genuine size violation and a transport error — "both were lumped into 413 +
    /// 'exceeds limit', lying about a disconnect" — and NOTHING tested either branch, so the distinction
    /// that fix exists to make was unpinned and its `code` had no consumer anywhere in the repo. This is
    /// the size half; the transport half needs a body whose stream errors, which is a different harness.
    #[tokio::test]
    async fn an_oversized_body_is_413_with_the_payload_too_large_code() {
        let body = "x".repeat(2 * 1024 * 1024);
        let r = handle_request(
            req_with_body("POST", "/api/tools/terminal_list", &body),
            state(),
        )
        .await;
        assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let bytes = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let j: serde_json::Value =
            serde_json::from_slice(&bytes).expect("the refusal must be JSON");
        assert_eq!(j["ok"], serde_json::json!(false));
        assert_eq!(
            j["code"],
            serde_json::json!("payload_too_large"),
            "the code is the machine-readable half, and the reason round-60 split the two branches"
        );
        assert!(
            j["error"].as_str().unwrap_or("").contains("1 MB"),
            "the operator-facing half must name the limit: {j}"
        );
    }

    /// THE OTHER HALF OF ROUND 60'S DISTINCTION: a body that FAILS MID-READ is 400 `body_read_error`,
    /// not 413. Its sibling above covers "too long"; this covers "the transport broke", which is the case
    /// the old code lied about by calling every read failure "exceeds limit". It needs its own harness — a
    /// stream that YIELDS AN ERROR rather than a body that is merely oversized — which is why it is a
    /// separate test rather than a case in the one above.
    #[tokio::test]
    async fn a_body_that_fails_mid_read_is_400_with_the_body_read_error_code() {
        let broken = futures::stream::iter(vec![Err::<bytes::Bytes, _>(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "the client went away mid-body",
        ))]);
        let req = Request::builder()
            .method("POST")
            .uri("/api/tools/terminal_list")
            .header("Authorization", format!("Bearer {TEST_TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from_stream(broken))
            .unwrap();
        let r = handle_request(req, state()).await;
        assert_eq!(
            r.status(),
            StatusCode::BAD_REQUEST,
            "a broken transport is not a size violation — round 60 split these apart"
        );
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let j: serde_json::Value = serde_json::from_slice(&b).expect("the refusal must be JSON");
        assert_eq!(
            j["code"],
            serde_json::json!("body_read_error"),
            "the OTHER code, which nothing produced in a test before this"
        );
    }

    fn req_with_host(path: &str, host: &str) -> Request<Body> {
        // round-102: token injection requires the gateway-proxy marker (or
        // loopback) — the inject cases set it, the must-NOT-inject cases
        // (direct public access) don't.
        req_with_host_proxy(path, host, false)
    }
    fn req_with_host_proxy(path: &str, host: &str, via_proxy: bool) -> Request<Body> {
        let mut b = Request::builder()
            .method("GET")
            .uri(path)
            .header(axum::http::header::HOST, host);
        if via_proxy {
            b = b.header("x-summrise-proxy", "1");
        }
        b.body(Body::empty()).unwrap()
    }

    /// A request WITH a declared peer address, which is what the loopback token handout
    /// now depends on. `None` models "no ConnectInfo was injected" — the case that must
    /// FAIL CLOSED.
    fn req_with_peer(path: &str, host: &str, peer: Option<std::net::IpAddr>) -> Request<Body> {
        let mut r = req_with_host(path, host);
        if let Some(ip) = peer {
            r.extensions_mut()
                .insert(axum::extract::ConnectInfo(std::net::SocketAddr::new(
                    ip, 54321,
                )));
        }
        r
    }
    fn req_with_host_secret(path: &str, host: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(path)
            .header(axum::http::header::HOST, host)
            .header(
                "x-summrise-auth",
                "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            )
            .body(Body::empty())
            .unwrap()
    }
    fn req_with_host_secret_wrong(path: &str, host: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(path)
            .header(axum::http::header::HOST, host)
            .header(
                "x-summrise-auth",
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            )
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn panel_token_injection_host_gate() {
        let mut cfg = Config::default();
        cfg.server.device_token =
            Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into());
        cfg.server.proxy_secret =
            Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into());
        let st = Arc::new(AppState::new(cfg));
        // The device's own subdomain MUST inject when the request carries
        // the gateway's shared secret (X-Summrise-Auth, round-103 — a spoofable
        // marker header was replaced with a constant-time secret check).
        let ok = handle_request(
            req_with_host_secret("/panel/", "d1.agent.saisi.online"),
            st.clone(),
        )
        .await;
        let body = axum::body::to_bytes(ok.into_body(), 1 << 20).await.unwrap();
        assert!(
            String::from_utf8_lossy(&body).contains("window.__PANEL_TOKEN__"),
            "device host with secret must inject"
        );

        // Direct (no secret) on the device host MUST NOT inject — an
        // attacker hitting the enumerable public hostname gets no token.
        let direct = handle_request(
            req_with_host("/panel/", "d1.agent.saisi.online"),
            st.clone(),
        )
        .await;
        let bd = axum::body::to_bytes(direct.into_body(), 1 << 20)
            .await
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&bd).contains("window.__PANEL_TOKEN__"),
            "direct device access must NOT inject (RCE)"
        );

        // A spoofed WRONG secret must not inject either.
        let wrong = handle_request(
            req_with_host_secret_wrong("/panel/", "d1.agent.saisi.online"),
            st.clone(),
        )
        .await;
        let bw = axum::body::to_bytes(wrong.into_body(), 1 << 20)
            .await
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&bw).contains("window.__PANEL_TOKEN__"),
            "wrong secret must NOT inject"
        );

        // Apex with secret + loopback inject.
        let apex = handle_request(
            req_with_host_secret("/panel/", "agent.saisi.online"),
            st.clone(),
        )
        .await;
        let ba = axum::body::to_bytes(apex.into_body(), 1 << 20)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&ba).contains("window.__PANEL_TOKEN__"),
            "apex with secret must inject"
        );
        // Loopback injects — and the peer MUST be loopback, not just the header.
        for h in ["127.0.0.1:18080", "localhost"] {
            let r = handle_request(
                req_with_peer(
                    "/panel/",
                    h,
                    Some(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
                ),
                st.clone(),
            )
            .await;
            let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
            assert!(
                String::from_utf8_lossy(&b).contains("window.__PANEL_TOKEN__"),
                "{h} from a LOOPBACK PEER must inject"
            );
        }

        // THE DEFECT THIS CHECK EXISTS FOR. `Host` is client-supplied and costs an
        // attacker nothing; before this check the header ALONE was the whole gate, so any
        // local process got the permanent device token from an unauthenticated
        // `curl http://127.0.0.1:18080/panel/` — re-opening exactly what the config-file
        // ACL hardening closed (`paths.rs` grants read only to SYSTEM + Administrators).
        for peer in [
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 7)),
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 50)),
        ] {
            let r = handle_request(
                req_with_peer("/panel/", "127.0.0.1:18080", Some(peer)),
                st.clone(),
            )
            .await;
            let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
            assert!(
                !String::from_utf8_lossy(&b).contains("window.__PANEL_TOKEN__"),
                "Host: 127.0.0.1 from a NON-loopback peer ({peer}) must NOT inject"
            );
        }

        // And NO peer information at all must also fail closed: "we cannot tell where
        // this connection came from" is not "it came from loopback".
        let r = handle_request(
            req_with_peer("/panel/", "127.0.0.1:18080", None),
            st.clone(),
        )
        .await;
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        assert!(
            !String::from_utf8_lossy(&b).contains("window.__PANEL_TOKEN__"),
            "a missing ConnectInfo extension must DENY the loopback handout, not allow it"
        );

        // Multi-level attacker subdomain + suffix-spoof MUST NOT inject.
        for h in [
            "evil.agent.saisi.online",
            "agent.saisi.online.evil.com",
            "evil.com",
            "d1.agent.saisi.online.evil.com",
        ] {
            let r = handle_request(req_with_host("/panel/", h), st.clone()).await;
            let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
            assert!(
                !String::from_utf8_lossy(&b).contains("window.__PANEL_TOKEN__"),
                "{h} must NOT inject"
            );
        }
    }

    #[tokio::test]
    async fn panel_token_injection_escapes_script_close() {
        // A non-hex token containing </script> must be escaped, not raw.
        let mut cfg = Config::default();
        cfg.server.device_token = Some("abc</script><script>alert(1)</script>xyz".into());
        let st = Arc::new(AppState::new(cfg));
        let r = handle_request(
            req_with_peer(
                "/panel/",
                "127.0.0.1:18080",
                Some(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
            ),
            st,
        )
        .await;
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let html = String::from_utf8_lossy(&b);
        assert!(
            html.contains("\\u003c/script\\u003e"),
            "must escape </script>: {html}"
        );
    }

    #[tokio::test]
    async fn desktop_route_serves_spa_and_injects_token() {
        // /desktop/ (summrise-desktop shell) serves the same SPA and gets the
        // loopback token injection exactly like /panel/.
        let mut cfg = Config::default();
        cfg.server.device_token = Some("test-token-123".into());
        let st = Arc::new(AppState::new(cfg));
        let r = handle_request(
            req_with_peer(
                "/desktop/",
                "127.0.0.1:18080",
                Some(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
            ),
            st.clone(),
        )
        .await;
        assert_eq!(
            r.status(),
            StatusCode::OK,
            "desktop route must serve the SPA"
        );
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let html = String::from_utf8_lossy(&b);
        assert!(html.contains("id=\"root\""), "desktop SPA html: {html}");
        assert!(
            html.contains("__PANEL_TOKEN__"),
            "loopback token injection on /desktop/: {html}"
        );
        // Static asset route: /desktop/panel.js serves the bundle.
        let r = handle_request(req_with_host("/desktop/panel.js", "127.0.0.1:18080"), st).await;
        assert_eq!(r.status(), StatusCode::OK, "desktop panel.js must serve");
    }

    // ── Panel bundle cache key (content hash, not crate version) ──────
    //
    // Cloudflare overrides no-cache with a 4h Browser-Cache-TTL for .js/.css:
    // the `?v=` on the bundle URLs is the ONLY thing that retires the old
    // panel after an update, so both serve paths (plain + token-injected)
    // must stamp the SAME content hash.

    #[test]
    fn bundle_hash_is_content_key_not_crate_version() {
        // The cache key must move with panel rebuilds, not with the Cargo
        // version (frozen at 1.0.x while the npm release rides 1.2.x — the
        // old `?v=<crate-version>` never changed between releases).
        let h = panel_bundle_hash();
        assert_eq!(h.len(), 16, "FNV-1a-64 hex: {h}");
        assert!(
            h.chars().all(|c| c.is_ascii_hexdigit()),
            "lowercase hex: {h}"
        );
        assert_ne!(h, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn apply_bundle_hash_stamps_once_and_spares_vendor() {
        let html = r#"<link rel="stylesheet" href="vendor/xterm.css"><link rel="stylesheet" href="panel.css"><script type="module" src="panel.js"></script>"#;
        let out = apply_bundle_hash(html);
        let h = panel_bundle_hash();
        assert!(out.contains(&format!("panel.css?v={h}")));
        assert!(out.contains(&format!("panel.js?v={h}")));
        assert!(
            out.contains("vendor/xterm.css"),
            "vendor link untouched: {out}"
        );
        assert!(!out.contains("xterm.css?v="), "no vendor stamp: {out}");
        assert_eq!(
            out.matches("?v=").count(),
            2,
            "exactly one stamp per bundle"
        );
    }

    #[tokio::test]
    async fn panel_plain_and_token_paths_carry_content_hash() {
        let mut cfg = Config::default();
        cfg.server.device_token = Some(TEST_TOKEN.into());
        let st = Arc::new(AppState::new(cfg));
        let h = panel_bundle_hash();
        // Plain panel (non-allowlisted host, no grant → no injection).
        let r = handle_request(req_with_host("/panel/", "d1.example.com:18080"), st.clone()).await;
        assert_eq!(r.status(), StatusCode::OK);
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let html = String::from_utf8_lossy(&b);
        assert!(
            html.contains(&format!("panel.js?v={h}")),
            "plain panel stamps js: {html}"
        );
        assert!(
            html.contains(&format!("panel.css?v={h}")),
            "plain panel stamps css: {html}"
        );
        // Token-injected panel (loopback) — same cache key shape + token.
        let r = handle_request(
            req_with_peer(
                "/panel/",
                "127.0.0.1:18080",
                Some(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
            ),
            st,
        )
        .await;
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let html = String::from_utf8_lossy(&b);
        assert!(html.contains("__PANEL_TOKEN__"));
        assert!(
            html.contains(&format!("panel.js?v={h}")),
            "token panel stamps js: {html}"
        );
        assert!(
            html.contains(&format!("panel.css?v={h}")),
            "token panel stamps css: {html}"
        );
    }

    // ── One-time panel grants (?grant=) ──────────────────────────────────
    //
    // The redeem arm is exercised against a local TCP listener speaking just
    // enough HTTP for the reqwest POST — no real gateway, no external
    // network. Hosts are deliberately NON-allowlisted and non-loopback: on
    // those hosts the ONLY way the token may be injected is a successful
    // grant redemption, which makes the assertions unambiguous.

    const GRANT: &str = "0123456789abcdef0123456789abcdef";

    /// The redeemed-grant guard is ONE process-global, file-backed store — correct for a
    /// device (one process, one set of spent codes) and impossible to parallelise in tests:
    /// a sibling's redemption refuses this one's, and a sibling's RESET deletes the file
    /// this one is asserting on. Both were observed. So every test that touches the guard
    /// holds this for its whole body.
    ///
    /// ASYNC mutex on purpose: these tests await, and a `std::sync::MutexGuard` held across
    /// an await is both a clippy error here and a real hazard.
    static GRANT_TEST_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> =
        std::sync::OnceLock::new();

    fn grant_test_lock() -> &'static tokio::sync::Mutex<()> {
        // `tokio::sync::Mutex::new` is not const in this version, so the lock lives in a
        // OnceLock rather than a bare static.
        GRANT_TEST_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    /// MULTI-shot redeem stub: answers `n` connections with the SAME success body.
    ///
    /// The one-shot stub below cannot test a replay guard, and that is not obvious: with
    /// the guard DISABLED the second redemption simply fails to connect, `redeem_panel_grant`
    /// returns false, and "no token was injected" passes for entirely the wrong reason. It
    /// did — mutation-proving the guard left the suite green. A stub that keeps saying yes
    /// is what makes the guard the only thing that can refuse.
    async fn spawn_redeem_stub_n(n: usize) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            for _ in 0..n {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = [0u8; 4096];
                // Read the request (headers + body) with a bounded wait, then answer.
                let _ = tokio::time::timeout(
                    std::time::Duration::from_millis(500),
                    sock.read(&mut buf),
                )
                .await;
                let body = "{\"ok\":true}";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        // WITH the scheme: the redeem builds "{console_url}/api/..." and reqwest needs it.
        // Returning the bare addr made the first redemption fail outright.
        format!("http://{addr}")
    }

    /// One-shot redeem stub: accepts ONE connection, captures the raw request
    /// bytes, answers `<status_line>` + `<body>` and returns the captured
    /// request via the JoinHandle. Aborting the handle (dropping it) closes
    /// the listener — used to prove NO call was made.
    async fn spawn_redeem_stub(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let handle = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            // Read until the content-length declared by the request is
            // satisfied (headers + JSON body may arrive split); bounded by a
            // per-read timeout so a malformed client can't hang the test.
            let mut data: Vec<u8> = Vec::new();
            loop {
                let mut buf = [0u8; 2048];
                let n = match tokio::time::timeout(
                    std::time::Duration::from_millis(500),
                    sock.read(&mut buf),
                )
                .await
                {
                    Ok(Ok(n)) => n,
                    _ => 0,
                };
                if n == 0 {
                    break;
                }
                data.extend_from_slice(&buf[..n]);
                if let Ok(text) = std::str::from_utf8(&data) {
                    if let Some(i) = text.find("\r\n\r\n") {
                        let len: usize = text[..i]
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse().ok())
                            })
                            .unwrap_or(0);
                        if text[i + 4..].len() >= len {
                            break;
                        }
                    }
                }
            }
            let captured = String::from_utf8_lossy(&data).to_string();
            let resp = format!(
                "{status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
            captured
        });
        (format!("http://{addr}"), handle)
    }

    /// THE REPLAY THE GATEWAY CANNOT STOP. `store/grants.ts` is KV: `get` caches at the edge
    /// and `delete` takes up to ~60 s to be visible everywhere, so a leaked grant URL can be
    /// redeemed a second time. A claim key does not close it (the replay arrives at a colo
    /// that never read the claim) — but the DEVICE does, because it is one strongly
    /// consistent process and the replay must come through it.
    ///
    /// Same request, twice: the first injects, the second must not.
    #[tokio::test]
    async fn panel_grant_is_not_redeemable_twice_by_this_device() {
        let mut cfg = Config::default();
        cfg.server.device_token = Some(TEST_TOKEN.into());
        // MULTI-shot: the gateway keeps saying yes, so the ONLY thing that can refuse the
        // second redemption is the guard. With the one-shot stub this test passed even
        // with the guard disabled — it was measuring the stub, not the guard.
        let base = spawn_redeem_stub_n(4).await;
        cfg.platform.console_url = Some(base);
        let st = Arc::new(AppState::new(cfg));

        // Serialised + clean: the guard is one global file-backed store, and a code spent
        // by an earlier `cargo test` RUN is still spent (the file outlives the binary) —
        // observed, when the first version of this test reused `GRANT` and failed its own
        // "first redemption must inject" assertion.
        let _serial = grant_test_lock().lock().await;
        crate::web::panel::reset_redeemed_grants_for_test();
        let code = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1";
        let path = format!("/panel/?grant={code}");

        let first = handle_request(req_with_host(&path, "d1.example.com:18080"), st.clone()).await;
        let b1 = axum::body::to_bytes(first.into_body(), 1 << 20)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&b1).contains("window.__PANEL_TOKEN__"),
            "the FIRST redemption must inject -- the guard must not break the real flow"
        );

        let second = handle_request(req_with_host(&path, "d1.example.com:18080"), st.clone()).await;
        let b2 = axum::body::to_bytes(second.into_body(), 1 << 20)
            .await
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&b2).contains("window.__PANEL_TOKEN__"),
            "a REPLAYED grant must not inject a second permanent token"
        );
    }

    /// The guard must SURVIVE A RESTART: an agent restart between the two redemptions would
    /// otherwise forget, and the gateway's TTL is 120 s while a restart is ~10 s.
    #[tokio::test]
    async fn redeemed_grant_guard_persists_and_prunes() {
        let _serial = grant_test_lock().lock().await;
        crate::web::panel::reset_redeemed_grants_for_test();
        let code = "fedcba9876543210fedcba9876543210";
        assert!(
            !crate::web::panel::grant_already_redeemed(code),
            "a code this device never redeemed must not be refused"
        );
        crate::web::panel::remember_redeemed_grant(code);
        assert!(
            crate::web::panel::grant_already_redeemed(code),
            "a redeemed code must be refused from then on"
        );
        // Persisted, so a fresh process would also refuse it.
        let on_disk =
            std::fs::read_to_string(crate::paths::etc_dir().join("panel-grants-redeemed.txt"))
                .unwrap_or_default();
        assert!(
            on_disk.contains(code),
            "the redeemed code must be on disk, or an agent restart forgets it: {on_disk:?}"
        );
    }

    #[tokio::test]
    async fn panel_grant_redeem_success_injects_token() {
        // Serialised + clean, or a sibling's redemption refuses this one's and the redeem
        // stub is never contacted — which HANGS a test that awaits it (observed).
        let _serial = grant_test_lock().lock().await;
        crate::web::panel::reset_redeemed_grants_for_test();
        let mut cfg = Config::default();
        cfg.server.device_token = Some(TEST_TOKEN.into());
        let (base, handle) = spawn_redeem_stub("HTTP/1.1 200 OK", "{\"ok\":true}").await;
        cfg.platform.console_url = Some(base);
        let st = Arc::new(AppState::new(cfg));
        // Non-allowlisted, non-loopback host: injection ONLY via the grant.
        let r = handle_request(
            req_with_host(&format!("/panel/?grant={GRANT}"), "d1.example.com:18080"),
            st,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        // Same response shape as the authorized injection path: no-store.
        assert_eq!(
            r.headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok()),
            Some("no-store"),
            "grant-served panel must be no-store"
        );
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let html = String::from_utf8_lossy(&b);
        assert!(
            html.contains("__PANEL_TOKEN__"),
            "successful redeem must inject: {html}"
        );
        // The redeem call carried OUR bearer token + the grant code, and the
        // grant value must not leak beyond the redeem body itself.
        let captured = handle.await.unwrap();
        assert!(
            captured.contains("POST /api/devices/panel-grant/redeem"),
            "captured: {captured}"
        );
        assert!(
            captured.contains(&format!("authorization: Bearer {}", TEST_TOKEN)),
            "captured: {captured}"
        );
        assert!(captured.contains(GRANT), "captured: {captured}");
    }

    #[tokio::test]
    async fn panel_grant_redeem_failure_serves_plain_panel() {
        // Serialised + clean, or a sibling's redemption refuses this one's and the redeem
        // stub is never contacted — which HANGS a test that awaits it (observed).
        let _serial = grant_test_lock().lock().await;
        crate::web::panel::reset_redeemed_grants_for_test();
        let mut cfg = Config::default();
        cfg.server.device_token = Some(TEST_TOKEN.into());
        let (base, handle) =
            spawn_redeem_stub("HTTP/1.1 404 Not Found", "{\"type\":\"error\"}").await;
        cfg.platform.console_url = Some(base);
        let st = Arc::new(AppState::new(cfg));
        let r = handle_request(
            req_with_host(&format!("/panel/?grant={GRANT}"), "d1.example.com:18080"),
            st,
        )
        .await;
        assert_eq!(
            r.status(),
            StatusCode::OK,
            "the panel page itself still serves"
        );
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        assert!(
            !String::from_utf8_lossy(&b).contains("__PANEL_TOKEN__"),
            "a failed redeem must NOT inject (bad-grant = bad-token behavior)"
        );
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn panel_grant_without_console_url_falls_back_to_plain_panel() {
        // Serialised + clean, or a sibling's redemption refuses this one's and the redeem
        // stub is never contacted — which HANGS a test that awaits it (observed).
        let _serial = grant_test_lock().lock().await;
        crate::web::panel::reset_redeemed_grants_for_test();
        // Pure-local device: no console binding → nothing to redeem with, so
        // ?grant= is simply invalid (existing bad-token behavior) and NO
        // network call is possible (no stub exists to answer one).
        let mut cfg = Config::default();
        cfg.server.device_token = Some(TEST_TOKEN.into());
        let st = Arc::new(AppState::new(cfg));
        let r = handle_request(
            req_with_host(&format!("/panel/?grant={GRANT}"), "d1.example.com:18080"),
            st,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        assert!(!String::from_utf8_lossy(&b).contains("__PANEL_TOKEN__"));
    }

    #[tokio::test]
    async fn panel_grant_malformed_code_never_redeems() {
        // The stub answers ok:true to ANYTHING — if the shape check let a
        // malformed grant through, redemption would "succeed" and the token
        // would be injected onto a non-allowlisted host. No injection + no
        // captured request proves the check gates the network call.
        let mut cfg = Config::default();
        cfg.server.device_token = Some(TEST_TOKEN.into());
        let (base, handle) = spawn_redeem_stub("HTTP/1.1 200 OK", "{\"ok\":true}").await;
        cfg.platform.console_url = Some(base);
        let st = Arc::new(AppState::new(cfg));
        let r = handle_request(
            req_with_host("/panel/?grant=not-a-hex-code!", "d1.example.com:18080"),
            st,
        )
        .await;
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        assert!(!String::from_utf8_lossy(&b).contains("__PANEL_TOKEN__"));
        let waited = tokio::time::timeout(std::time::Duration::from_millis(300), handle).await;
        assert!(
            waited.is_err(),
            "no redeem request may be made for a malformed grant"
        );
    }

    #[test]
    fn plausible_grant_shape() {
        assert!(
            plausible_grant(&"a".repeat(32)),
            "gateway-minted shape (32 hex)"
        );
        // WAS: `plausible_grant(&"a".repeat(16))` as "floor accepted" and an uppercase
        // 32 as "uppercase hex ok". Both pin the OLD permissive window, and the gateway
        // rejects both shapes — so each probe cost a redeem round-trip against an
        // unauthenticated, unrate-limited route with a fresh TLS client per attempt.
        assert!(
            !plausible_grant(&"a".repeat(16)),
            "16 chars is not the gateway's shape"
        );
        assert!(
            !plausible_grant("ABCDEF0123456789ABCDEF0123456789"),
            "uppercase is rejected by the gateway's regex (no `i` flag)"
        );
        assert!(!plausible_grant(""), "empty rejected");
        assert!(!plausible_grant("abcdefgh"), "too short rejected");
        assert!(!plausible_grant(&"g".repeat(32)), "non-hex rejected");
        assert!(!plausible_grant(&"a".repeat(129)), "over-long rejected");
    }

    /// The agent's grant shape and the gateway's gate are ONE contract, in two
    /// languages and two repos-in-one. Drift means either probes that cost a
    /// round-trip (agent looser) or codes that never redeem (agent tighter) — and
    /// the looser direction is the expensive one, because the route is
    /// unauthenticated and unrate-limited.
    #[test]
    fn panel_grant_shape_matches_the_gateway() {
        // The gateway's own read gate, quoted from gateway/src/store/grants.ts:59.
        // Kept as a literal so a change there is a change HERE, visibly.
        const GATEWAY_CODE_LEN: usize = 32;
        let minted = "0a1b2c3d4e5f60718293a4b5c6d7e8f9";
        assert_eq!(
            minted.len(),
            GATEWAY_CODE_LEN,
            "the sample is the real width"
        );
        assert!(plausible_grant(minted), "a gateway-minted code must pass");
        assert!(
            !plausible_grant(&minted.to_uppercase()),
            "the gateway's regex is case-SENSITIVE; uppercase must not pass"
        );
        for n in [16usize, 31, 33, 64, 128] {
            assert!(
                !plausible_grant(&"a".repeat(n)),
                "only the gateway's exact width may pass, and {n} is not it"
            );
        }
    }

    #[test]
    fn panel_grant_query_extraction() {
        // Position-independent + ignore surrounding params (the shared
        // query_param helper; grant must not be confused with lookalikes).
        assert_eq!(
            query_param(Some(&format!("grant={GRANT}")), "grant"),
            Some(GRANT)
        );
        assert_eq!(
            query_param(Some(&format!("x=1&grant={GRANT}")), "grant"),
            Some(GRANT)
        );
        assert_eq!(
            query_param(Some("xgrants=1"), "grant"),
            None,
            "prefix must not match"
        );
        assert_eq!(
            query_param(Some("grants=1"), "grant"),
            None,
            "longer key must not match"
        );
        assert_eq!(query_param(None, "grant"), None);
    }

    async fn json_body(resp: Response) -> serde_json::Value {
        let body = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[test]
    /// THE PLUGIN SPEC'S OWN CONTRACT, from the device's side (round 224).
    ///
    /// `monitor-row.json` established the pattern: the fixture lists what the panel's parser reads, the
    /// device asserts it SENDS every one of them, and the panel's test asserts it READS them. /api/spec was
    /// the one payload-bearing route the panel parses without a fixture.
    fn spec_snapshot_pins_every_device_tool_for_the_gateway_contract() {
        // `agent/spec-tools.json` is the machine-readable face of THIS
        // registry. gateway/test/mcp-handler.test.mjs fails when a name in it
        // is neither registered on the console MCP surface nor explicitly
        // listed as not-exposed — because the old gateway contract compared
        // its registry against a hand-typed copy of ITSELF, 21 device tools
        // (the whole system_*/memory_*/mcp_client_* families) stayed invisible
        // to MCP clients with every gate green. Regenerate with:
        //   SUMMRISE_REFRESH_SPEC=1 cargo test --features terminal,keyring spec_snapshot
        //
        // PARAMETER NAMES joined the snapshot for the same reason the tool names
        // did, one drift later: the gateway advertises its OWN inputSchema for
        // every device-direct tool, so a parameter added here is invisible to a
        // console client — it cannot discover it, and a schema-validating client
        // would refuse to send it. `terminal_execute`'s `intent`/`considered`
        // shipped exactly that way and were found by reading, not by a gate.
        // NAMES, `required`, AND TYPES. Names catch a MISSING parameter (an unadvertised one
        // cannot be sent); `required` catches an array that FORBIDS a call the device allows —
        // `terminal_execute`'s `session_id` did exactly that, found by reading, not by a gate;
        // and the declared TYPE catches a schema the gateway would refuse before forwarding.
        // Each was added after the defect it would have caught, which is the only honest reason
        // a field belongs in a contract file.
        //        // DESCRIPTIONS JOINED IT AFTER ONE DRIFTED, and that drift is the reason
        // this file now carries prose: the gateway advertises its OWN copy of
        // every device-direct tool's description, and `monitor_list`'s copy had
        // lost `last_expect_ok` while `drops`' explanation moved onto
        // `last_status` — a model on the console was told a different contract
        // than the device implements, with every contract test green. Nothing can
        // compare prose that lives in only one machine-readable place, so the
        // device's is here now, and a change to it is a committed diff instead of
        // a silent divergence.
        let spec = api_spec(&state());
        let mut entries: Vec<serde_json::Value> = Vec::new();
        for p in spec["plugins"].as_array().unwrap() {
            for t in p["tools"].as_array().unwrap() {
                let mut params: Vec<String> = t["schema"]["properties"]
                    .as_object()
                    .map(|o| o.keys().cloned().collect())
                    .unwrap_or_default();
                params.sort();
                // AND NOW THE TYPES (round 221). The comment above used to say the snapshot
                // carried no types, calling a type difference "a separate question this snapshot
                // is not trying to answer". It is the same class of question `required` was added
                // for — the gateway validates before forwarding, so a schema saying integer where
                // the device wants string refuses a call the device would have served. The question
                // is still separate; it is no longer unanswerable.
                let mut param_types: serde_json::Map<String, serde_json::Value> =
                    serde_json::Map::new();
                if let Some(props) = t["schema"]["properties"].as_object() {
                    for (k, v) in props {
                        if let Some(ty) = v.get("type") {
                            param_types.insert(k.clone(), ty.clone());
                        }
                    }
                }
                // `required` AS WELL AS the parameter names. Names alone let a
                // whole class of contract lie through: the gateway advertised
                // `terminal_execute` as requiring `session_id` where the device
                // makes it optional and requires only `command`, so a
                // schema-validating client was FORBIDDEN a call the device
                // supports. Nothing compared the arrays, so nothing could see it.
                let mut required: Vec<String> = t["schema"]["required"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str())
                            .map(|v| v.to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                required.sort();
                entries.push(serde_json::json!({
                    "name": t["name"].as_str().unwrap(),
                    "plugin": p["name"].as_str().unwrap(),
                    "description": t["description"].as_str().unwrap_or(""),
                    "params": params,
                    "required": required,
                    "param_types": param_types,
                }));
            }
        }
        entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        let rendered = format!(
            "// Device MCP tool inventory (name + owning plugin + description + parameter\n\
             // names + required + declared types),\n\
             // generated from\n\
             // the live PluginRegistry by web::tests::spec_snapshot_pins_every_device_tool_for_the_gateway_contract.\n\
             // The gateway MCP registry contract test reads this file.\n\
             // Do not hand-edit: SUMMRISE_REFRESH_SPEC=1 cargo test spec_snapshot, then commit.\n{}\n",
            serde_json::to_string_pretty(&serde_json::Value::Array(entries)).unwrap()
        );
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/spec-tools.json");
        if std::env::var("SUMMRISE_REFRESH_SPEC").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(path, &rendered).expect("write spec-tools.json");
            return;
        }
        let committed = std::fs::read_to_string(path).unwrap_or_else(|e| {
            panic!("{path} missing ({e}) — run SUMMRISE_REFRESH_SPEC=1 cargo test spec_snapshot")
        });
        assert_eq!(
            committed.trim_end(),
            rendered.trim_end(),
            "{path} is stale vs the live registry — run SUMMRISE_REFRESH_SPEC=1 cargo test spec_snapshot and commit it"
        );
    }

    #[test]
    fn plugin_spec_fixture_matches_the_payload() {
        let raw = include_str!("../../tests/fixtures/plugin-spec.json");
        let fixture: serde_json::Value = serde_json::from_str(raw).expect("the fixture parses");
        let required = fixture["required_by_panel"]
            .as_array()
            .expect("required_by_panel is an array");
        // The fixture's sample plugin must itself carry every key the panel reads — a fixture that omits one
        // would pass this test while the panel rendered an empty label.
        let sample = &fixture["plugin"];
        for key in required {
            let k = key.as_str().expect("a key is a string");
            assert!(
                sample.get(k).is_some(),
                "the fixture omits {k}, which required_by_panel says the panel reads"
            );
        }
        // And what the DEVICE builds for a plugin must carry them too — through the same function the route
        // uses, so this cannot pass while /api/spec sends something else.
        let sample = &fixture["plugin"];
        let built = spec_plugin_object(
            sample["name"].as_str().unwrap(),
            sample["displayName"].as_str().unwrap(),
            sample["description"].as_str().unwrap(),
            sample["tools"].as_array().cloned().unwrap_or_default(),
        );
        for key in required {
            let k = key.as_str().unwrap();
            assert!(
                built.get(k).is_some(),
                "the device does not build {k}, and the panel reads it — the plugin list would render empty"
            );
        }
        // The nested tool list is read too (`tools?: { name }[]`).
        let tools = built["tools"].as_array().expect("tools is an array");
        if let Some(t0) = tools.first() {
            for k in fixture["each_tool"]
                .as_array()
                .expect("each_tool is an array")
            {
                let k = k.as_str().unwrap();
                assert!(t0.get(k).is_some(), "a tool in the spec omits {k}");
            }
        }
    }

    #[tokio::test]
    async fn spec_lists_terminal_plugin() {
        let resp = handle_request(req("GET", "/api/spec"), state()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        // terminal + update + mcp-client + design + playwright + memory + system
        // + runs (the AI-execution identity surface, added with the run feature:
        // a plugin registered but not listed here would keep its tools out of
        // /api/spec, i.e. invisible to every client that discovers through it)
        // + monitor (the reachability watches, round 262 — same rule, one family
        // over: registered but unlisted means invisible to every client that
        // discovers through /api/spec).
        assert_eq!(v["plugins"].as_array().unwrap().len(), 9);
    }

    /// The previous boot's verdict must reach the WIRE, not just the function — and it must say
    /// WHICH verdict, as data.
    ///
    /// **THIS IS THE TEST THE FEATURE DID NOT HAVE, AND ITS ABSENCE IS WHY EVERY GATE STAYED GREEN
    /// WHILE `/api/status` NEVER CARRIED THE FIELD.** The unit tests covered the verdict in
    /// isolation (round trip, empty file, missing file) and the status tests covered
    /// `pending_approvals` — and nothing asserted `last_boot` ON THE RESPONSE. So the block, written
    /// inside the `if pending_approvals > 0` guard, was unreachable in the steady state, which is the
    /// ONLY state a healthy device is ever in. It took a live device and a `curl` to see it.
    /// **DELIVERED is a claim about the wire, not about the function.**
    ///
    /// The second half is round 256's: the sentence alone forces the panel to match English, so the
    /// kind rides beside it — and this asserts the pair on the SAME response.
    /// THE STATUS PAYLOAD'S KEYS, checked against the shared fixture.
    ///
    /// `agent/tests/fixtures/status.json` is read by this test and by the panel's `useAgentVitals`
    /// tests. `/api/status` is the FIRST call the panel makes, and it reads the fields silently: a
    /// renamed `uptime_secs` is a blank strip, a renamed `last_boot_kind` is an "unrecorded" verdict.
    /// This asserts the REAL response's keys — not a reconstruction — so a rename or a field that
    /// stops being sent fails here rather than on an operator's screen.
    #[tokio::test]
    async fn status_carries_the_keys_the_fixture_promises() {
        let raw = include_str!("../../tests/fixtures/status.json");
        let fixture: serde_json::Value = serde_json::from_str(raw).expect("fixture parses");
        let stable: std::collections::BTreeSet<&str> = fixture["stable_keys"]
            .as_array()
            .expect("stable_keys")
            .iter()
            .filter_map(|k| k.as_str())
            .collect();
        let conditional: std::collections::BTreeSet<&str> = fixture["conditional_keys"]
            .as_object()
            .expect("conditional_keys")
            .keys()
            .map(|k| k.as_str())
            .collect();

        let v = json_body(handle_request(req("GET", "/api/status"), state()).await).await;
        let keys: std::collections::BTreeSet<&str> = v
            .as_object()
            .expect("object")
            .keys()
            .map(|k| k.as_str())
            .collect();

        // THE STABLE KEYS ARE ALWAYS THERE. The conditional ones are not asserted for presence: by
        // definition they may be absent, and a fresh test state has no boot verdict, no vitals sample
        // and nothing waiting for approval — which is exactly the steady state a healthy device is in.
        // Their NAMES are still pinned, by the undeclared check below, and the PAIRING rule
        // (`last_boot` with `last_boot_kind`) is the sibling test's job: it writes a verdict first and
        // asserts both halves on the same response.
        for k in stable.iter() {
            assert!(
                keys.contains(k),
                "the fixture promises `{k}` as stable; the response has {keys:?}"
            );
        }
        // A conditional key that IS present must be one the fixture declares — otherwise the contract
        // is describing a payload that no longer exists.
        for k in conditional.iter().filter(|k| keys.contains(**k)) {
            assert!(
                conditional.contains(k),
                "`{k}` is on the response but the fixture calls it unknown"
            );
        }
        // And nothing on the response is unknown to the fixture: a NEW field must be added there (and
        // to the panel's required list if it reads it), or the contract has stopped describing reality.
        let declared: std::collections::BTreeSet<&str> =
            stable.union(&conditional).copied().collect();
        let undeclared: Vec<&&str> = keys.difference(&declared).collect();
        assert!(
            undeclared.is_empty(),
            "the response carries fields the fixture does not declare: {undeclared:?}"
        );
    }

    #[tokio::test]
    async fn status_carries_the_previous_boot_verdict() {
        let _live_dir = lock_data_dir().await;
        use crate::runstate::{save_verdict, BootKind};
        let path = crate::runstate::verdict_path(&crate::paths::data_dir());
        let saved = std::fs::read_to_string(&path).ok();
        if let Some(p) = path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let line =
            "run journal: previous run DID NOT EXIT CLEANLY \u{2014} CRASHED or was killed (test)";
        save_verdict(&crate::paths::data_dir(), BootKind::Crashed, line);

        let v = json_body(handle_request(req("GET", "/api/status"), state()).await).await;
        assert_eq!(
            v["last_boot"].as_str(),
            Some(line),
            "the boot verdict must be ON THE RESPONSE, not merely readable by the function: {v}",
        );
        assert_eq!(
            v["last_boot_kind"].as_str(),
            Some("crashed"),
            "the machine-readable half must ride the same response: {v}",
        );
        // THE REGRESSION ITSELF: this device has nothing waiting for an answer, which is the state
        // the field was invisible in. A future edit that moves the block back under a conditional
        // fails here rather than on a device.
        assert!(
            v.get("pending_approvals").is_none(),
            "this test is only meaningful with zero pending approvals: {v}",
        );
        // And it is a field of the SAME response that already carries the unconditional ones, so a
        // consumer polling for connectivity sees both.
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));

        match saved {
            Some(s) => {
                let _ = std::fs::write(&path, s);
            }
            None => {
                // THE OTHER HALF, IN THE SAME TEST ON PURPOSE: both cases own the same file, so
                // two `#[tokio::test]`s over it would race under cargo's default parallelism and
                // fail intermittently — the kind of flake that gets a test deleted instead of
                // fixed. A device that has never booted a build that writes a verdict must not
                // grow the fields either: "no verdict on record" and "a verdict that says nothing"
                // are different facts, which is why they are inserted conditionally.
                let _ = std::fs::remove_file(&path);
                let v = json_body(handle_request(req("GET", "/api/status"), state()).await).await;
                assert!(v.get("last_boot").is_none(), "{v}");
                assert!(v.get("last_boot_kind").is_none(), "{v}");
            }
        }
    }

    /// `/api/vitals/history` is the TREND behind the strip's two numbers.
    ///
    /// Three properties a consumer depends on, none of which a chart can check for itself:
    /// the series is OLDEST FIRST (the order it must be drawn in), it carries the SPACING
    /// that gives it meaning (`interval_secs` — a shape drawn without its cadence is a lie
    /// about rates), and a host that reports nothing answers with an EMPTY series rather
    /// than an absent field.
    #[tokio::test]
    async fn vitals_history_serves_the_series_oldest_first_with_its_cadence() {
        use crate::metrics::{record_at, Vitals};
        let path_samples = [(1_700_000_000_000u64, 10.0), (1_700_000_030_000, 20.0)];
        for (ts, cpu) in path_samples {
            record_at(
                Vitals {
                    cpu_pct: Some(cpu),
                    mem_pct: Some(50.0),
                    mem_total_mb: Some(8192),
                },
                ts,
            );
        }
        let v = json_body(handle_request(req("GET", "/api/vitals/history"), state()).await).await;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["interval_secs"], 30, "{v}");
        let samples = v["samples"].as_array().expect("a samples array");
        let mine: Vec<&serde_json::Value> = samples
            .iter()
            .filter(|s| s["mem_total_mb"].as_u64() == Some(8192))
            .collect();
        assert_eq!(mine.len(), 2, "both seeded readings are served: {v}");
        assert_eq!(mine[0]["cpu_pct"], 10.0, "oldest first: {v}");
        assert_eq!(mine[1]["cpu_pct"], 20.0, "{v}");
        // The span is derived from the series itself, so a chart's x-axis can be honest
        // without the panel re-deriving it.
        assert!(v["span_secs"].as_u64().unwrap_or(0) >= 30, "{v}");
    }

    /// THE SHAPE IS ALWAYS THERE, whatever the host can measure: `samples` is an ARRAY and
    /// `span_secs` a NUMBER, never an absent field or a null. A chart reads an absent series
    /// and an empty one very differently, and "this host reports no vitals" must arrive as
    /// the second (the emptiness itself is pinned in `metrics`' own tests, where the ring can
    /// be reasoned about without the process-wide series this test shares).
    /// THE MONITOR FORM AND THE SNAPSHOT, end to end through the routes the panel calls.
    ///
    /// Three properties the UI depends on: a form error comes back as a REASON (it is rendered
    /// verbatim), adding the same host:port twice yields ONE target (two rows would be two
    /// probers on one host), and the snapshot always carries a series array per target — the
    /// card draws an empty chart only when it truly has nothing.
    #[tokio::test]
    async fn monitors_add_remove_and_snapshot() {
        // A host that cannot resolve or connect: this test is about the WIRE, and the prober is
        // never invoked (the panel's "check now" is the only caller that probes).
        async fn post(path: &str, body: &str) -> serde_json::Value {
            json_body(handle_request(req_with_body("POST", path, body), state()).await).await
        }
        let v = post("/api/monitors/add", r#"{"host":"192.0.2.77","port":22}"#).await;
        assert_eq!(v["ok"], true, "{v}");
        let id = v["target"]["id"].as_str().expect("an id").to_string();
        assert_eq!(id, "192.0.2.77:22", "{v}");

        // Idempotent: the same target again is the same row, not a second one.
        let again = post("/api/monitors/add", r#"{"host":"192.0.2.77","port":22}"#).await;
        assert_eq!(again["target"]["id"].as_str(), Some(id.as_str()));
        let snap = json_body(handle_request(req("GET", "/api/monitors"), state()).await).await;
        let rows = snap["targets"].as_array().expect("targets");
        assert_eq!(
            rows.iter()
                .filter(|r| r["id"].as_str() == Some(id.as_str()))
                .count(),
            1,
            "one target, one row: {snap}"
        );
        assert!(rows.iter().any(|r| r["id"] == id), "{snap}");
        assert!(snap["interval_secs"].as_u64().unwrap_or(0) > 0, "{snap}");
        // The row carries a summary and a series from the FIRST response — a card that had to
        // wait for a second poll to draw would read as broken.
        let row = rows.iter().find(|r| r["id"] == id).expect("row");
        assert!(row["series"].is_array(), "{row}");
        assert_eq!(
            row["summary"]["probes"], 0,
            "no probes yet is not an error: {row}"
        );
        // `path` is PRESENT AND NULL for a TCP target: the panel builds a row's name from host,
        // port and path, and a snapshot that omitted the field made three different HTTP checks
        // render as three identical names on d1. A component test cannot see a missing
        // server-side field; this can.
        assert!(
            row.get("path").is_some(),
            "the snapshot must carry the path field: {row}"
        );
        assert!(row["path"].is_null(), "{row}");

        // …and a target WITH a path reports it, which is what makes two checks on one port two
        // different rows.
        let with_path = post(
            "/api/monitors/add",
            r#"{"host":"192.0.2.77","port":80,"path":"/health"}"#,
        )
        .await;
        assert_eq!(with_path["target"]["path"], "/health", "{with_path}");
        let snap = json_body(handle_request(req("GET", "/api/monitors"), state()).await).await;
        let row = snap["targets"]
            .as_array()
            .expect("targets")
            .iter()
            .find(|r| r["id"] == "192.0.2.77:80/health")
            .expect("the path-bearing row");
        assert_eq!(row["path"], "/health", "{row}");
        assert_ne!(row["id"], "192.0.2.77:80", "a path is part of the identity");
        let _ = post("/api/monitors/remove", r#"{"id":"192.0.2.77:80/health"}"#).await;
        let _ = post("/api/monitors/remove", r#"{"id":"192.0.2.77:80"}"#).await;

        // A form error is a REASON, not a code.
        let bad = post("/api/monitors/add", r#"{"host":"","port":22}"#).await;
        assert_eq!(bad["ok"], false, "{bad}");
        assert!(
            bad["error"].as_str().unwrap_or("").contains("host"),
            "{bad}"
        );
        let bad = post("/api/monitors/add", r#"{"host":"192.0.2.77"}"#).await;
        assert!(
            bad["error"].as_str().unwrap_or("").contains("port"),
            "{bad}"
        );

        // And removal says whether anything was removed.
        let gone = post("/api/monitors/remove", &format!(r#"{{"id":"{id}"}}"#)).await;
        assert_eq!(gone["removed"], true, "{gone}");
        let gone_again = post("/api/monitors/remove", &format!(r#"{{"id":"{id}"}}"#)).await;
        assert_eq!(
            gone_again["removed"], false,
            "the second removal is a no-op: {gone_again}"
        );
    }

    /// THE HTTP DOOR'S ENVELOPE, through the route the panel actually posts to: a body this
    /// surface could read but not act on is `200 + {"ok":false,"error":<reason>,"code":"invalid_params"}`.
    ///
    /// What is pinned is the TRANSLATION. `monitor_form` had no direct test of its own — the route
    /// test above only ever asked whether SOME error came back — and it used to hold a second copy
    /// of the rules, so the expected strings here are the VALIDATOR'S OWN
    /// (`monitor::TargetSpec::parse`, `monitor::validate_target`, `monitor::PORT_REQUIRED_REASON`):
    /// a sentence re-typed in this file fails this test, which is the whole point of the refactor.
    /// In particular the port message used to be shorter here than through `monitor_add`; the
    /// fuller `monitor.rs` wording is the one both doors now show.
    ///
    /// Every case is refused before the store is reached, so this touches no target list and no
    /// data dir (which is why it needs no serial guard against the monitor's own tests).
    #[tokio::test]
    async fn a_refused_monitor_add_answers_in_the_http_envelope_with_the_validators_own_reason() {
        let expected = [
            // The field is not there at all — a WIRE fact this door establishes, and even so the
            // sentence is monitor.rs's.
            (
                r#"{"host":"192.0.2.77"}"#,
                crate::monitor::PORT_REQUIRED_REASON.to_string(),
            ),
            // A port of zero: the range check `TargetSpec::parse` owns.
            (
                r#"{"host":"192.0.2.77","port":0}"#,
                crate::monitor::PORT_REQUIRED_REASON.to_string(),
            ),
            // Above u16: refused BEFORE the cast, or it would truncate to 0 and answer about the
            // wrong mistake.
            (
                r#"{"host":"192.0.2.77","port":65536}"#,
                "65536 is not a port".to_string(),
            ),
            // The host rules, verbatim from the validator rather than restated here.
            (
                r#"{"host":"","port":22}"#,
                crate::monitor::validate_target("", 22).unwrap_err(),
            ),
            (
                r#"{"host":"a/b","port":22}"#,
                crate::monitor::validate_target("a/b", 22).unwrap_err(),
            ),
            // The refusal `parse` owns: an expectation with no path has no body to read.
            (
                r#"{"host":"192.0.2.77","port":22,"expect":"OpenWrt"}"#,
                crate::monitor::TargetSpec::parse("192.0.2.77", 22, "", "OpenWrt").unwrap_err(),
            ),
        ];
        for (body, reason) in expected {
            let v = json_body(
                handle_request(req_with_body("POST", "/api/monitors/add", body), state()).await,
            )
            .await;
            assert_eq!(v["ok"], false, "{body}: {v}");
            assert_eq!(v["code"], "invalid_params", "{body}: {v}");
            assert_eq!(v["error"].as_str(), Some(reason.as_str()), "{body}: {v}");
        }

        // A body that is not JSON at all is THIS door's wire shape, and it keeps its envelope (and
        // its own sentence: no validator ever saw it).
        let v = json_body(
            handle_request(req_with_body("POST", "/api/monitors/add", "{oops"), state()).await,
        )
        .await;
        assert_eq!(v["code"], "invalid_params", "{v}");
        assert!(
            v["error"]
                .as_str()
                .unwrap_or("")
                .contains("invalid JSON body"),
            "{v}"
        );
    }

    #[tokio::test]
    async fn vitals_history_always_answers_with_a_series_and_a_span() {
        let v = json_body(handle_request(req("GET", "/api/vitals/history"), state()).await).await;
        assert!(v["samples"].is_array(), "{v}");
        assert!(v["span_secs"].is_u64(), "{v}");
        assert_eq!(v["interval_secs"], 30, "{v}");
    }

    #[tokio::test]
    async fn status_ok() {
        let resp = handle_request(req("GET", "/api/status"), state()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    }

    /// `/api/boots` is the DEVICE'S RESTART HISTORY — the pattern, not the last event.
    ///
    /// The verdict file is overwritten at every boot, so this route is the only way any surface
    /// can answer "how often does this agent restart, and how many of those were crashes" — the
    /// question d1's 2026-09-13 incident left nobody able to answer. The test pins the three
    /// things a consumer depends on: the list is NEWEST FIRST, the summary counts the window
    /// the route advertises, and an install with no history answers with an empty list and
    /// zeroes rather than a 404 or a missing field.
    #[tokio::test]
    async fn boots_route_serves_the_restart_history_and_its_summary() {
        let _live_dir = lock_data_dir().await;
        use crate::runstate::{history_path, record_boot, BootKind, RunState};
        let path = history_path(&crate::paths::data_dir());
        let saved = std::fs::read_to_string(&path).ok();
        if let Some(p) = path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        // Seed through the WRITER, not by hand: the route must serve what a boot records.
        let _ = std::fs::remove_file(&path);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let prev = RunState {
            started: now - 500,
            last: now - 100,
            exited: false,
            reason: None,
        };
        record_boot(
            &crate::paths::data_dir(),
            BootKind::Replaced,
            "run journal: previous run DID NOT EXIT CLEANLY \u{2014} REPLACED (test)",
            Some(&prev),
            now - 200,
        );
        record_boot(
            &crate::paths::data_dir(),
            BootKind::Crashed,
            "run journal: previous run DID NOT EXIT CLEANLY \u{2014} CRASHED (test)",
            Some(&prev),
            now,
        );

        let v = json_body(handle_request(req("GET", "/api/boots"), state()).await).await;
        let boots = v["boots"].as_array().expect("a boots array");
        assert_eq!(boots.len(), 2, "{v}");
        assert_eq!(
            boots[0]["kind"].as_str(),
            Some("crashed"),
            "newest first: {v}"
        );
        assert_eq!(boots[1]["kind"].as_str(), Some("replaced"), "{v}");
        assert_eq!(boots[0]["uptime_secs"].as_u64(), Some(400), "{v}");
        assert_eq!(
            v["summary"]["boots"], 2,
            "the summary covers the same records it ships: {v}"
        );
        assert_eq!(v["summary"]["crashes"], 1, "{v}");
        assert_eq!(v["summary"]["window_secs"], 86400, "{v}");

        // And an install that has never booted a recording build: an EMPTY list plus zeroes —
        // the panel's card then says there is nothing to show rather than drawing a blank list
        // whose emptiness it cannot explain.
        let _ = std::fs::remove_file(&path);
        let v = json_body(handle_request(req("GET", "/api/boots"), state()).await).await;
        assert_eq!(v["boots"].as_array().map(|a| a.len()), Some(0), "{v}");
        assert_eq!(v["summary"]["boots"], 0, "{v}");

        match saved {
            Some(s) => {
                let _ = std::fs::write(&path, s);
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// `/api/status` reports a WAITING DECISION, because the push cannot reach
    /// every surface that asks.
    ///
    /// The approval rework made a question outlive the execute that asked it, so
    /// an operator who was away can still answer inside the TTL. The push that
    /// announces one (`sessions-changed`) only reaches an OPEN panel; the tray
    /// and the console fleet card poll `/api/status` instead, and could not say
    /// "a decision is waiting" — the one fact the rework exists to deliver.
    ///
    /// Two properties, and the second is the one a careless implementation gets
    /// wrong: the count APPEARS when a question is open, and it is ABSENT (not
    /// zero) when none is, so a consumer can render a badge without having to
    /// tell "nothing waiting" from "older agent that never sends this field".
    #[cfg(all(feature = "terminal", not(target_os = "windows")))]
    #[tokio::test]
    async fn status_reports_a_waiting_decision_and_omits_it_when_there_is_none() {
        let (st, cfg_path) = state_with_cfg("status-pending", CFG_YAML_TOKEN_ONLY);

        // Idle: the field is ABSENT, not zero.
        let v = json_body(handle_request(req("GET", "/api/status"), st.clone()).await).await;
        assert!(
            v.get("pending_approvals").is_none(),
            "with nothing waiting the field must be OMITTED, so a consumer can \
             tell 'nothing waiting' from 'an older agent that never sends it' — \
             got {v}"
        );

        // Arm a session and start a gated execute; it registers a question.
        let sid = st
            .terminal_mgr
            .term_open(&crate::tools::terminal::TermOpenRequest {
                kind: "pty".into(),
                target: String::new(),
                password: String::new(),
                key_path: String::new(),
                rows: 24,
                cols: 80,
                inject_marker: false,
                data_bits: None,
                parity: None,
                stop_bits: None,
                auto_reconnect: false,
            })
            .await
            .unwrap()
            .0;
        handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/control"),
                r#"{"approval_required":true}"#,
            ),
            st.clone(),
        )
        .await;
        let exec = {
            let (st2, sid2) = (st.clone(), sid.clone());
            tokio::spawn(async move {
                handle_request(
                    req_with_json(
                        "POST",
                        "/api/tools/terminal_execute",
                        &format!(r#"{{"session_id":"{sid2}","command":"echo status-probe"}}"#),
                    ),
                    st2,
                )
                .await
            })
        };
        // Bounded wait for the registration, not a bare check: the execute runs
        // on a spawned task and asserting immediately is a race.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if st
                    .terminal_mgr
                    .term_pending_approval(&sid)
                    .await
                    .unwrap()
                    .is_some()
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the gate must register a question");

        let v = json_body(handle_request(req("GET", "/api/status"), st.clone()).await).await;
        assert_eq!(
            v["pending_approvals"].as_u64(),
            Some(1),
            "a waiting decision must be visible to every surface that polls \
             /api/status — the tray and the console fleet card cannot see the \
             panel's push: {v}"
        );

        // Answer it, so the spawned execute completes and nothing is left
        // registered for the next test in this process.
        let id = st
            .terminal_mgr
            .term_pending_approval(&sid)
            .await
            .unwrap()
            .unwrap()
            .id;
        st.terminal_mgr
            .term_decide_approval(&sid, &id, false, false)
            .await
            .unwrap();
        let _ = exec.await;

        // And it goes back to ABSENT once nothing is waiting.
        let v = json_body(handle_request(req("GET", "/api/status"), st.clone()).await).await;
        assert!(
            v.get("pending_approvals").is_none(),
            "an answered question must clear the count: {v}"
        );

        st.terminal_mgr.term_close(&sid).await.ok();
        let _ = std::fs::remove_file(cfg_path);
    }

    #[tokio::test]
    async fn tool_dispatch_headless() {
        // terminal_list through the registry → headless stub → empty array
        let resp = handle_request(
            req_with_token("POST", "/api/tools/terminal_list", TEST_TOKEN),
            state(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["ok"], true);
        assert_eq!(v["result"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn unknown_tool_reports_error() {
        let resp = handle_request(
            req_with_token("POST", "/api/tools/does_not_exist", TEST_TOKEN),
            state(),
        )
        .await;
        let v = json_body(resp).await;
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains("unknown tool"));
        assert_eq!(v["code"], "invalid_params");
    }

    #[tokio::test]
    async fn tool_error_envelope_survives_the_api_wrapper() {
        // THE DEVICE ERROR CONTRACT, pinned on the /api/tools side (SOLID
        // R101). Two failure families reach a client and this wrapper treats
        // them differently — deliberately, and now on the record:
        //
        //   typed   Err(DeviceError)         → {"ok": false, "error", "code"}
        //   in-band Ok(plugins::tool_error)  → {"ok": true,  "result": {"ok": false, "error"}}
        //
        // The in-band family is the majority of device tools (46 sites). Its
        // OUTER ok is TRUE, so the gateway's round-58 check (`data.ok ===
        // false`) does not classify it as a failure — the message survives as
        // text inside a result the model reads. That is the long-standing MCP
        // behaviour; this test exists so changing it is a visible decision
        // rather than a silent drift.
        let missing = if cfg!(windows) {
            r"C:\summrise-no-such-file-r101"
        } else {
            "/summrise-no-such-file-r101"
        };
        let resp = handle_request(
            req_with_json(
                "POST",
                "/api/tools/system_file_stat",
                &serde_json::json!({ "path": missing }).to_string(),
            ),
            state(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "in-band failures are HTTP 200"
        );
        let v = json_body(resp).await;
        assert_eq!(
            v["ok"], true,
            "the OUTER ok is true for the in-band family — see the doc comment"
        );
        assert!(
            v.get("code").is_none(),
            "in-band failures carry no top-level code"
        );
        assert_eq!(v["result"]["ok"], false, "the tool's own envelope is kept");
        assert!(
            v["result"]["error"]
                .as_str()
                .is_some_and(|e| e.contains("stat ")),
            "the tool's message survives verbatim: {}",
            v["result"]
        );
    }

    #[tokio::test]
    async fn plugins_status_requires_auth() {
        // /api/plugins/* is inside the same auth gate as every other /api/*
        let mut cfg = Config::default();
        cfg.server.device_token = Some("sekret".into());
        let st = Arc::new(AppState::new(cfg));
        let resp = handle_request(req("GET", "/api/plugins/status"), st).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn plugins_status_reports_stopped() {
        let resp = handle_request(req("GET", "/api/plugins/status"), state()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["ok"], true);
        assert_eq!(v["playwright"]["running"], false);
    }

    #[tokio::test]
    async fn plugins_playwright_start_missing_bundle_errors() {
        // Dev builds carry no bundled node.exe under install_dir/playwright/
        // — start must fail loudly (500) with the path hint, not pretend
        // success. The failure happens before any spawn, so no network wait.
        let resp = handle_request(req("POST", "/api/plugins/playwright/start"), state()).await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let v = json_body(resp).await;
        assert_eq!(v["ok"], false);
        let err = v["error"].as_str().unwrap_or_default();
        assert!(
            err.contains("node.exe"),
            "error must name the missing node.exe: {err}"
        );
        assert!(
            err.contains("playwright"),
            "error must point at the playwright bundle: {err}"
        );
    }

    /// The backoff's SHAPE, as a pure function (round 206). The two atomics around it are process-wide and other tests in
    /// this file authenticate, so a test that drove them would race; the state machine is six lines long and visible in
    /// `note_auth_failure`, and the 401 path itself is covered by `auth_401_without_token` below.
    #[test]
    fn auth_backoff_grows_then_holds() {
        assert_eq!(auth_backoff_ms(0), 0, "no failures, no penalty");
        assert_eq!(auth_backoff_ms(1), 100);
        assert_eq!(auth_backoff_ms(2), 200);
        assert_eq!(auth_backoff_ms(3), 400);
        assert_eq!(auth_backoff_ms(5), 1600);
        assert_eq!(auth_backoff_ms(6), 2000);
        assert_eq!(
            auth_backoff_ms(600),
            2000,
            "it HOLDS at two seconds rather than growing without bound: the token is a long secret, so the job is to make \
             guessing cost time, not to lock an operator out"
        );
    }

    /// DISCOVERY IS UNAUTHENTICATED, AND THE 401 POINTS AT IT (round 207). The MCP specification asks a 401 to carry
    /// `WWW-Authenticate: Bearer resource_metadata="..."`; without the document behind that pointer a remote client has
    /// nothing to discover from. Both halves are asserted, because either one alone is useless: a metadata document nobody
    /// is pointed at, or a pointer to a document that 404s (which is what the SPA fallback would have served before this
    /// route was placed above it).
    #[tokio::test]
    async fn mcp_discovery_is_reachable_and_the_401_names_it() {
        let mut cfg = Config::default();
        cfg.server.device_token = Some("sekret".into());
        let st = Arc::new(AppState::new(cfg));

        let doc = handle_request(
            req_anon("GET", "/.well-known/oauth-protected-resource"),
            st.clone(),
        )
        .await;
        assert_eq!(
            doc.status(),
            StatusCode::OK,
            "discovery must not require a credential"
        );
        let body = axum::body::to_bytes(doc.into_body(), 1 << 20)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("/mcp"),
            "the document names the resource: {text}"
        );
        assert!(
            text.contains("bearer_methods_supported"),
            "and how to present a token: {text}"
        );
        assert!(
            !text.contains("authorization_servers"),
            "this agent mints no tokens, so it names no authorization server (RFC 9728 makes the field optional): {text}"
        );

        // THE 401 COMES FROM THE GATE, which is the OWNER of the MCP endpoint (the dispatcher's `/api` auth never sees
        // `/mcp` in production — the MCP router claims it first), so the gate is what this test drives. The inner service
        // is a stand-in that answers "ok": the point here is the gate's own refusal, not what it protects.
        struct OkSvc;
        impl Service<Request<Body>> for OkSvc {
            type Response = axum::http::Response<McpBoxBody>;
            type Error = Infallible;
            type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Infallible>> + Send>>;
            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
                Poll::Ready(Ok(()))
            }
            fn call(&mut self, _req: Request<Body>) -> Self::Future {
                Box::pin(async {
                    Ok(axum::http::Response::new(
                        http_body_util::combinators::BoxBody::new(http_body_util::Full::new(
                            bytes::Bytes::from_static(b"ok"),
                        )),
                    ))
                })
            }
        }

        let mut gate = TokenGate::new(OkSvc, st.clone());
        let denied = gate.call(req_anon("POST", "/mcp")).await.unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        let www = denied
            .headers()
            .get("WWW-Authenticate")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            www.contains("resource_metadata=")
                && www.contains("/.well-known/oauth-protected-resource"),
            "the 401 must point at the document above, got: {www:?}"
        );
    }

    #[tokio::test]
    async fn auth_401_without_token() {
        // NO Authorization header at all (the name says what it means — this
        // test used to send req(), which carries a *wrong* token, so the
        // genuinely-missing-header case had no coverage until R102).
        let mut cfg = Config::default();
        cfg.server.device_token = Some("sekret".into());
        let st = Arc::new(AppState::new(cfg));
        let resp = handle_request(req_anon("GET", "/api/status"), st.clone()).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // A WRONG token must fail identically (same envelope, no oracle).
        let resp = handle_request(req_with_token("GET", "/api/status", "nope"), st).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            json_body(resp).await,
            serde_json::json!({"ok": false, "error": "unauthorized"})
        );
    }

    /// A PATH THAT IS NOT AN EVIDENCE ROUTE IS `None`, NOT A SCREENSHOT — the promise the handler's own
    /// doc has always made and its code did not keep: its two `if`s FALL THROUGH to the pwshot reader, so
    /// anything else reaching it was read as a screenshot name and answered `400 "bad name"`, which is a
    /// wrong answer dressed as a right one. That is also why the caller's `if let Some(...)` looked
    /// permanently true. This pins the INNER half; `pre_dispatch_owns_exactly_the_public_surface` below
    /// pins the outer one, and the defect lived in the seam between them.
    #[tokio::test]
    async fn a_path_that_is_not_an_evidence_route_returns_none() {
        assert!(
            handle_browser_evidence("/api/not-ours", None)
                .await
                .is_none(),
            "an unknown path must fall through to the dispatcher, which 404s it"
        );
        assert!(
            handle_browser_evidence("/api/browser", None)
                .await
                .is_none(),
            "a PREFIX of a real route is not the route"
        );
        // …and the three it does own are answered, whatever their bodies then say.
        assert!(handle_browser_evidence("/api/browser/actions", None)
            .await
            .is_some());
        assert!(handle_browser_evidence("/api/browser/pwshots", None)
            .await
            .is_some());
        assert!(
            handle_browser_evidence("/api/browser/pwshot", Some("name=x.png"))
                .await
                .is_some()
        );
    }

    /// STRUCTURAL PIN for the R108 seam: `route_pre_dispatch` decides whether
    /// a request is answered before the API pipeline at all.
    ///
    /// The two tests around this one pin the CONSEQUENCE (dispatch routes are
    /// gated; public routes are not). This pins the MECHANISM, so the split
    /// cannot quietly rot: a dispatcher route that starts being answered
    /// early would bypass the auth gate entirely, and a public route that
    /// stops being answered early would 401 the panel SPA.
    ///
    /// Note this calls `route_pre_dispatch` directly with NO Authorization
    /// header: the fall-through cases must be `None` *regardless* of auth,
    /// because authenticating is the caller's job, not the router's.
    #[tokio::test]
    async fn pre_dispatch_owns_exactly_the_public_surface() {
        let st = state();
        let method_of = |m: &str| Method::from_bytes(m.as_bytes()).expect("valid method");
        let headers_of = |m: &str, p: &str| req_anon(m, p).headers().clone();

        // Dispatcher routes fall THROUGH. `/api/events` and `/api/events/term`
        // are deliberately absent — they ARE pre-dispatch routes (they stream,
        // so they run their own auth instead of waiting for a body).
        for (m, p) in [
            ("GET", "/api/spec"),
            ("GET", "/api/status"),
            ("GET", "/api/sessions"),
            ("GET", "/api/sessions/some-session-id"),
            ("GET", "/api/logs"),
            ("GET", "/api/boots"),
            ("GET", "/api/vitals/history"),
            ("GET", "/api/update"),
            ("GET", "/api/monitors"),
            ("POST", "/api/run/mark-exit"),
            ("POST", "/api/monitors/add"),
            ("POST", "/api/monitors/remove"),
            ("POST", "/api/monitors/probe"),
            ("GET", "/api/events/poll"),
            ("GET", "/api/settings"),
            ("PUT", "/api/settings"),
            ("POST", "/api/gateway/connect"),
            ("GET", "/api/plugins/status"),
            ("POST", "/api/plugins/playwright/start"),
            ("POST", "/api/plugins/playwright/stop"),
            ("POST", "/api/tools/terminal_list"),
            ("GET", "/mcp"),
            ("POST", "/mcp"),
        ] {
            assert!(
                route_pre_dispatch(&method_of(m), p, None, &headers_of(m, p), &st, None, false)
                    .await
                    .is_none(),
                "{m} {p} is answered BEFORE the auth gate — it would bypass it"
            );
        }

        // Public surfaces are answered HERE, with no token at all.
        for (m, p) in [
            ("GET", "/"),
            ("GET", "/panel"),
            ("GET", "/panel/"),
            ("GET", "/desktop"),
            ("GET", "/desktop/"),
            ("GET", "/panel/panel.js"),
            ("GET", "/desktop/panel.css"),
            ("GET", "/some-unknown-page"),
        ] {
            assert!(
                route_pre_dispatch(&method_of(m), p, None, &headers_of(m, p), &st, None, false)
                    .await
                    .is_some(),
                "{m} {p} must be answered before the gate (documented public surface)"
            );
        }

        // The evidence endpoints run their OWN auth and are pre-dispatch: they
        // must reject an anonymous caller here, not fall through.
        for p in ["/api/browser/pwshots", "/api/browser/actions"] {
            let resp = route_pre_dispatch(
                &method_of("GET"),
                p,
                None,
                &headers_of("GET", p),
                &st,
                None,
                false,
            )
            .await
            .expect("evidence endpoints are answered pre-dispatch");
            assert_eq!(
                resp.status(),
                StatusCode::UNAUTHORIZED,
                "{p} must reject an anonymous caller pre-dispatch"
            );
        }
    }

    /// A path a caller could actually send for `row` — the test-side reading of a [`Pattern`].
    ///
    /// `Under` and `Prefix` rows need a segment under them, because the bare prefix is a DIFFERENT
    /// route or a degenerate one: `/api/sessions/` itself is not `SessionEvents` (the exact
    /// `/api/sessions` row is the collection), and a tool call with an empty tool name says nothing
    /// about the named shape. `Between` rows need an id between their two ends.
    fn probe_path(row: &Route) -> String {
        match row.pattern {
            Pattern::Exact(p) => p.to_string(),
            Pattern::Under(p) => format!("{p}some-session-id"),
            Pattern::Prefix(p) => format!("{p}some-segment"),
            Pattern::Between { at, ends } => format!("{at}some-session-id{ends}"),
        }
    }

    /// THE PRE-DISPATCH AGREEMENT TEST (route-table round): every row of `routes()` is walked through
    /// the REAL `route_pre_dispatch`, and the two must agree about the row's [`Stage`].
    ///
    /// `route_pre_dispatch` keeps its own mechanics — a table cannot express streaming, static files,
    /// content types or body parsing — so its decisions are the one place the table could silently
    /// stop describing the wire. This is that seam's pin, and it covers BOTH directions:
    ///
    ///   * a `Public` or `PreDispatchAuthed` row that stops being answered before the gate would
    ///     401 the panel SPA or the discovery document (and would be answered by `dispatch`, whose
    ///     debug assertion says so out loud);
    ///   * a `DispatchGated` or `Mcp` row that STARTS being answered before the gate is an auth
    ///     bypass — the request never reaches the unconditional gate at all. That is the dangerous
    ///     direction, and it is what this test refuses; `every_dispatch_route_is_auth_gated` is the
    ///     behavioural half, sending a tokenless request to every row.
    ///
    /// IT IS A TEST RATHER THAN A `debug_assert!` INSIDE THE WALK, deliberately: the walk has nine
    /// exit points, so an inline assertion would have to be repeated at each one — and a forgotten
    /// one is a silently loosened check, which is the failure mode this whole round is about. A test
    /// covers every row uniformly, in both profiles, without adding a panic to the request path.
    #[tokio::test]
    async fn pre_dispatch_stage_agrees_with_the_table() {
        let st = state();
        for row in routes() {
            let (m, path) = (row.method, probe_path(row));
            let headers = req_anon(m, &path).headers().clone();
            let answered = route_pre_dispatch(
                &Method::from_bytes(m.as_bytes()).expect("valid method"),
                &path,
                None,
                &headers,
                &st,
                None,
                false,
            )
            .await
            .is_some();
            let table_says_before_the_gate =
                matches!(row.stage, Stage::Public | Stage::PreDispatchAuthed);
            assert_eq!(
                answered,
                table_says_before_the_gate,
                "{m} {path} is stage {:?} in `routes()`, but route_pre_dispatch {} it — the table \
                 and the walk disagree (answered={answered})",
                row.stage,
                if answered {
                    "answered"
                } else {
                    "did not answer"
                }
            );
        }
    }

    /// `route_of` — the table's only lookup — on each of the four `Pattern` shapes, on the methods,
    /// and on the ordering hazard the table's doc comment names.
    ///
    /// The table is the one place the surface's routes are declared; these are its own unit tests, so
    /// a row that stopped matching (a typo, a reordered prefix, a pattern narrowed by accident) fails
    /// here rather than in a device's 404.
    #[test]
    fn route_of_matches_each_pattern_shape() {
        // ── Exact: the whole path, and nothing else ──
        assert_eq!(route_of("GET", "/api/spec"), Some(RouteId::Spec));
        assert_eq!(
            route_of("GET", "/api/spec/"),
            None,
            "an Exact row does not match a trailing slash"
        );

        // ── Under: the prefix AND at least one more character ──
        assert_eq!(
            route_of("GET", "/api/sessions"),
            Some(RouteId::Sessions),
            "the collection is the exact row"
        );
        assert_eq!(
            route_of("GET", "/api/sessions/"),
            None,
            "the bare prefix is NOT `SessionEvents` — `Under` needs one more character, and no \
             other row matches `/api/sessions/`"
        );
        assert_eq!(
            route_of("GET", "/api/sessions/term-0"),
            Some(RouteId::SessionEvents)
        );

        // ── Prefix: the bare prefix included ──
        assert_eq!(
            route_of("POST", "/api/tools/terminal_list"),
            Some(RouteId::ToolCall)
        );
        assert_eq!(
            route_of("POST", "/api/tools/"),
            Some(RouteId::ToolCall),
            "`Prefix` includes its bare prefix: the tool name is then empty and the registry says so"
        );
        assert_eq!(
            route_of("GET", "/api/tools/terminal_list"),
            None,
            "the method is part of the key: `/api/tools/*` is POST-only"
        );

        // ── Between: `<at><something><ends>` ──
        assert_eq!(
            route_of("POST", "/api/sessions/term-0/control"),
            Some(RouteId::SessionControl)
        );
        assert_eq!(
            route_of("POST", "/api/sessions/term-0/approval"),
            Some(RouteId::SessionApproval)
        );
        assert_eq!(
            route_of("POST", "/api/sessions/term-0/grants"),
            Some(RouteId::SessionGrants)
        );
        assert_eq!(
            route_of("GET", "/api/sessions/term-0/control"),
            Some(RouteId::SessionEvents),
            "THE GET ARM IS AN `Under` ON THE COLLECTION, and that is not a new fact: the arm this \
             table replaced was `p.starts_with(\"/api/sessions/\") && p.len() > \"/api/sessions/\".len()`, \
             which matched this path too. Rejecting an id like `term-0/control` is \
             `api_session_events`' job — it parses the remainder — not the table's, so pinning \
             `None` here would have asserted a contract the device never had. (The first version of \
             this test did pin `None`, and the whole suite went red on it: a test written from what \
             the table OUGHT to say rather than from what the old match arm did.)"
        );
        assert_eq!(
            route_of("POST", "/api/sessions/term-0/terminal"),
            None,
            "no action of that name exists — the POST side is three `Between` rows and nothing else"
        );

        // ── the ordering hazard: the EXACT `/panel/` row precedes `Prefix "/panel/"` ──
        assert_eq!(
            route_of("GET", "/panel/"),
            Some(RouteId::PanelHome),
            "ORDER IS THE CONTRACT: with `Prefix \"/panel/\"` present, `/panel/` must still resolve \
             to the panel HTML — move the prefix row up and this answers `PanelFile`"
        );
        assert_eq!(route_of("GET", "/panel"), Some(RouteId::PanelHome));
        assert_eq!(route_of("GET", "/panel/panel.js"), Some(RouteId::PanelFile));
        assert_eq!(
            route_of("OPTIONS", "/panel/panel.js"),
            Some(RouteId::PanelPreflight)
        );

        // ── an unknown path is not found ──
        assert_eq!(route_of("GET", "/api/nope"), None);
        assert_eq!(route_of("POST", "/"), None);
        assert_eq!(route_of("DELETE", "/api/sessions"), None);
    }

    /// The table's floorS (one per stage), and its method vocabulary.
    ///
    /// A table that was emptied — or emptied of one STAGE's rows by an edit that looked local — would
    /// make the walk that reads that stage vacuous, which is exactly the "a gate that cannot fail"
    /// defect this round exists to remove. The counts are FLOORS, not equalities: adding routes must
    /// not require editing these numbers, but losing a stage's rows must fail loudly.
    ///
    /// ONE FLOOR PER STAGE, because the first version floored only `DispatchGated` and the review found
    /// the hole: deleting every `Public` row failed NOTHING. The `Public` half of
    /// `every_dispatch_route_is_auth_gated` walks zero rows and passes, and
    /// `deliberately_public_routes_stay_public` carries its own hand-written list, so the panel SPA, the
    /// status page and the discovery document could all have lost their rows in silence.
    #[test]
    fn the_route_table_has_a_floor_per_stage_and_a_method_vocabulary() {
        for (stage, floor, what) in [
            (
                Stage::DispatchGated,
                24,
                "the auth walk's refusal half and `dispatch`'s arms",
            ),
            (
                Stage::PreDispatchAuthed,
                6,
                "the streaming and evidence routes the walk agrees with",
            ),
            (Stage::Mcp, 2, "both /mcp methods"),
            (
                Stage::Public,
                10,
                "the panel SPA, the status page, the discovery document and the preflight",
            ),
        ] {
            let rows = routes().iter().filter(|r| r.stage == stage).count();
            assert!(
                rows >= floor,
                "`routes()` has {rows} {stage:?} rows and had {floor} when the table was introduced; \
                 that stage covers {what}, and losing its rows makes the walk over it vacuous"
            );
        }
        // AND THE TABLE AS A WHOLE. A row can be lost without dropping a stage below its floor only by
        // ADDING one elsewhere, which is not a loss — so this pair is what makes "a row disappeared"
        // visible from either direction.
        assert!(
            routes().len() >= 42,
            "`routes()` has {} rows; it had 42 when the table was introduced",
            routes().len()
        );

        // A method that is not one of these can never match a request, and `handle_request_inner`
        // only ever produces these four — so a typo here is a route that silently 404s. It would NOT
        // be caught by the auth walk: a `DispatchGated` row with an unmatchable method is probed,
        // falls through to the gate, and 401s exactly as the test expects.
        for row in routes() {
            assert!(
                matches!(row.method, "GET" | "POST" | "PUT" | "OPTIONS"),
                "{:?} has method {:?}, which this surface never dispatches",
                row.id,
                row.method
            );
        }
    }

    /// A PREFLIGHT FOR `index.html` IS ANSWERED "NOT FOUND", AND THAT IS DELIBERATE — the case that
    /// disproved the `debug_assert!` this refactor first put in `dispatch`.
    ///
    /// The preflight arm answers every asset under `/panel/` and `/desktop/` EXCEPT `index.html`, and it
    /// says why in a comment ("the assets, never index.html"). What happens to the excluded one was,
    /// until this test, unspecified: it falls out of `route_pre_dispatch` entirely and lands in the
    /// dispatcher, which answers `{"ok":false,"error":"not found"}` with HTTP 200 — while `route_of`
    /// legitimately calls that path a `Public` row (`OPTIONS` + `Prefix "/panel/"` → `PanelPreflight`).
    ///
    /// So the assertion "a `Public` row must never reach `dispatch`" is FALSE, and a debug build
    /// panicked on this request. The rule now lives in the agreement test, where the request path pays
    /// nothing, and THIS pins the behaviour that made the difference: not a panic, not a 204, but the
    /// same not-found body any unknown path gets.
    #[tokio::test]
    async fn a_preflight_for_index_html_falls_through_to_not_found() {
        let mut cfg = Config::default();
        cfg.server.device_token = Some("sekret".into());
        let st = Arc::new(AppState::new(cfg));
        let resp =
            handle_request(req_with_token("OPTIONS", "/panel/index.html", "sekret"), st).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "an excluded preflight must not be answered as an error either"
        );
        let body = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("not found"),
            "expected the dispatcher's not-found body, got: {text}"
        );
        // AND THE EXCLUDED PATH IS STILL A `Public` ROW, which is the fact that made the debug assertion
        // wrong: the table describes where a path is NORMALLY answered, and this one is answered
        // nowhere on purpose.
        assert_eq!(
            route_of("OPTIONS", "/panel/index.html"),
            Some(RouteId::PanelPreflight)
        );
    }

    /// THE APPROVAL POSTURE IS EVIDENCE — the gap that only a REAL RUN exposed.
    ///
    /// Every unit test passed while arming the gate left no trace at all: the
    /// hold was recorded, the goal was recorded, and the single most
    /// consequential switch in the feature — the one that decides whether
    /// commands run unasked — was invisible. A reader could not tell whether a
    /// command ran because the operator approved it or because the gate was never
    /// on, which is precisely the question the evidence beat exists to answer.
    ///
    /// This drives the REAL routes in order and then reads the trail, because the
    /// defect was not in any one piece: the manager stored the mode correctly, the
    /// route returned it correctly, and only the joined-up history was missing.
    #[cfg(all(feature = "terminal", not(target_os = "windows")))]
    #[tokio::test]
    async fn the_approval_posture_is_recorded_in_the_trail() {
        let (st, cfg_path) = state_with_cfg("approval-audit", CFG_YAML_TOKEN_ONLY);
        let sid = st
            .terminal_mgr
            .term_open(&crate::tools::terminal::TermOpenRequest {
                kind: "pty".into(),
                target: String::new(),
                password: String::new(),
                key_path: String::new(),
                rows: 24,
                cols: 80,
                inject_marker: false,
                data_bits: None,
                parity: None,
                stop_bits: None,
                auto_reconnect: false,
            })
            .await
            .unwrap()
            .0;

        let ctl = |body: &'static str| {
            let st = st.clone();
            let sid = sid.clone();
            async move {
                handle_request(
                    req_with_json("POST", &format!("/api/sessions/{sid}/control"), body),
                    st,
                )
                .await
            }
        };

        // Arm, then arm AGAIN — the second must not add a second event, or a
        // panel polling this route would bury the transition it exists to show.
        assert_eq!(
            ctl(r#"{"approval_required":true}"#).await.status(),
            StatusCode::OK
        );
        assert_eq!(
            ctl(r#"{"approval_required":true}"#).await.status(),
            StatusCode::OK
        );

        // Approve a request WITH a grant, through the real routes.
        let exec = {
            let st2 = st.clone();
            let sid2 = sid.clone();
            tokio::spawn(async move {
                handle_request(
                    req_with_json(
                        "POST",
                        "/api/tools/terminal_execute",
                        &format!(r#"{{"session_id":"{sid2}","command":"echo audited"}}"#),
                    ),
                    st2,
                )
                .await
            })
        };
        let id = {
            let st3 = st.clone();
            let sid3 = sid.clone();
            tokio::time::timeout(std::time::Duration::from_secs(5), async move {
                loop {
                    if let Some(p) = st3.terminal_mgr.term_pending_approval(&sid3).await.unwrap() {
                        break p.id;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("the gate must prompt")
        };
        let resp = handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/approval"),
                &format!(r#"{{"id":"{id}","approve":true,"grant":true}}"#),
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let _ = exec.await;

        // Revoke, then disarm.
        let resp = handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/grants"),
                r#"{"all":true}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            ctl(r#"{"approval_required":false}"#).await.status(),
            StatusCode::OK
        );

        // And the trail tells the whole story, in order.
        let resp = handle_request(req("GET", &format!("/api/sessions/{sid}")), st.clone()).await;
        let events = json_body(resp).await["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let all: Vec<(String, String)> = events
            .iter()
            .filter(|e| e["kind"] == "approval")
            .map(|e| {
                (
                    e["status"].as_str().unwrap_or("").to_string(),
                    e["text"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        // `abandoned` is a TIMING-DEPENDENT extra, and CI is what proved it: the superseded
        // first exec is dropped when the second attempt replaces it, and whether that drop has
        // been recorded by the time the trail is read depends on the scheduler — one run had it
        // between `asked` and `approved`, another did not. The SUBJECT of this test is the
        // posture trail, so the filter is right; the filter is not BLIND, though: when the
        // event is present it must sit exactly between the question and its answer, which is
        // the only place an abandoned decision can honestly be.
        if let Some(ai) = all.iter().position(|(status, _)| status == "abandoned") {
            let asked = all.iter().position(|(status, _)| status == "asked");
            let approved = all.iter().position(|(status, _)| status == "approved");
            assert!(
                asked.is_some() && asked < Some(ai) && Some(ai) < approved,
                "an abandoned decision must sit between the question and the answer: {all:?}"
            );
        }
        let posture: Vec<(String, String)> = all
            .iter()
            .filter(|(status, _)| status != "abandoned")
            .cloned()
            .collect();
        assert_eq!(
            posture,
            vec![
                ("armed".to_string(), String::new()),
                // `asked` comes FIRST among the decision events, and that
                // ordering is the audit's whole value here: it records that a
                // question was PUT to a person before anything answered it.
                // Without it an unanswered or expired gate leaves no trace at
                // all, so a run that stopped because nobody was watching looks
                // identical to one that was never gated.
                ("asked".to_string(), "echo audited".to_string()),
                ("approved".to_string(), "echo audited".to_string()),
                ("granted".to_string(), "echo".to_string()),
                ("revoked".to_string(), String::new()),
                ("disarmed".to_string(), String::new()),
            ],
            "the trail must explain WHY a command ran unasked: arming, the QUESTION, \
             the decision, what it newly allowed, and taking it back. Note `armed` \
             appears ONCE despite two identical requests."
        );

        st.terminal_mgr.term_close(&sid).await.ok();
        let _ = std::fs::remove_file(cfg_path);
    }

    /// A decision that decided NOTHING is not recorded as a decision.
    ///
    /// The stale-id case: an operator clicks "run it" a moment after the request
    /// expired. The route answers honestly (`decided: false`), and the trail must
    /// not gain a yes for a command nobody authorised — a false entry in the one
    /// log that has to be trustworthy.
    #[cfg(all(feature = "terminal", not(target_os = "windows")))]
    #[tokio::test]
    async fn a_decision_that_decided_nothing_leaves_no_record() {
        let (st, cfg_path) = state_with_cfg("approval-noop", CFG_YAML_TOKEN_ONLY);
        let sid = st
            .terminal_mgr
            .term_open(&crate::tools::terminal::TermOpenRequest {
                kind: "pty".into(),
                target: String::new(),
                password: String::new(),
                key_path: String::new(),
                rows: 24,
                cols: 80,
                inject_marker: false,
                data_bits: None,
                parity: None,
                stop_bits: None,
                auto_reconnect: false,
            })
            .await
            .unwrap()
            .0;

        let resp = handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/approval"),
                r#"{"id":"ap-does-not-exist","approve":true,"grant":true}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(json_body(resp).await["decided"], false);

        let resp = handle_request(req("GET", &format!("/api/sessions/{sid}")), st.clone()).await;
        let events = json_body(resp).await["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(
            !events.iter().any(|e| e["kind"] == "approval"),
            "nothing was decided, so nothing may be recorded: {:?}",
            events
                .iter()
                .filter(|e| e["kind"] == "approval")
                .collect::<Vec<_>>()
        );

        st.terminal_mgr.term_close(&sid).await.ok();
        let _ = std::fs::remove_file(cfg_path);
    }

    /// THE OPERATION TIMELINE, end to end: a terminal command and a browser
    /// action written through their REAL producers come back on ONE axis.
    ///
    /// The unit pins prove the merge function; this proves the ROUTE reaches it
    /// with the right directories and the right auth — a route with the wrong
    /// dir or the wrong guard would leave every other test green.
    /// THE OPERATION ROUTE'S ENVELOPE, pinned from this end.
    ///
    /// `useOperationRuns` reads exactly three keys off `/api/operation` — `events`, `runs` and
    /// `cursor_ms` — and reads them defensively: a missing `events` is an empty page rather than an
    /// error, and a `cursor_ms` that is not a number leaves the panel's cursor where it was, so every
    /// poll re-requests the same window for ever. Neither failure looks like a failure.
    ///
    /// GATED ON THE FEATURE, which is what round 105's attempt got wrong: the route answers Internal
    /// without the terminal backend, so a test that asserts a 200 here must not run without it. Unlike
    /// its neighbour below it needs no PTY, so it does not carry the `not(windows)` half.
    #[cfg(feature = "terminal")]
    #[tokio::test]
    async fn the_operation_route_carries_the_envelope_the_panel_reads() {
        let resp = handle_request(req("GET", "/api/operation"), state()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        let keys: std::collections::BTreeSet<&str> = v
            .as_object()
            .expect("object")
            .keys()
            .map(|k| k.as_str())
            .collect();
        for k in ["events", "runs", "cursor_ms"] {
            assert!(
                keys.contains(k),
                "the panel reads `{k}`; the response has {keys:?}"
            );
        }
        assert!(v["events"].is_array(), "events must be a list: {v}");
        assert!(v["runs"].is_array(), "runs must be a list: {v}");
        assert!(
            v["cursor_ms"].is_number(),
            "cursor_ms must be a number — the panel keeps its own cursor and a string here would \
             leave every poll re-requesting the same window: {v}"
        );
    }

    #[cfg(all(feature = "terminal", not(target_os = "windows")))]
    #[tokio::test]
    async fn the_operation_route_merges_terminal_and_browser_records() {
        let (st, cfg_path) = state_with_cfg("operation-e2e", CFG_YAML_TOKEN_ONLY);

        // A real terminal command, through the real tool surface.
        let sid = st
            .terminal_mgr
            .term_open(&crate::tools::terminal::TermOpenRequest {
                kind: "pty".into(),
                target: String::new(),
                password: String::new(),
                key_path: String::new(),
                rows: 24,
                cols: 80,
                inject_marker: false,
                data_bits: None,
                parity: None,
                stop_bits: None,
                auto_reconnect: false,
            })
            .await
            .unwrap()
            .0;
        let resp = handle_request(
            req_with_json(
                "POST",
                "/api/tools/terminal_execute",
                &serde_json::json!({
                    "session_id": sid,
                    // Unique per RUN, not just per session: the sessions dir
                    // persists across runs, so a fixed probe string matches an
                    // older run's event too and the assertion below picks the
                    // wrong one (it did, on the second run).
                    "command": format!("echo operation-probe-{sid}"),
                    "intent": "the terminal half of the timeline",
                })
                .to_string(),
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        // A real browser action, through the shared evidence writer.
        //
        // The dir is created FIRST, as every real producer does before writing
        // evidence. `append_action_line` is best-effort by contract — a missing
        // directory must never fail the tool call that produced the action — so
        // skipping this makes the append a silent no-op and the test would blame
        // the merge for a missing file. (It did, on the first run.)
        let ev_dir = crate::paths::evidence_dir();
        std::fs::create_dir_all(&ev_dir).unwrap();
        crate::evidence::append_action_line(
            &ev_dir,
            crate::now_millis(),
            &serde_json::json!({"script": "mcp: browser_navigate url=http://probe", "exit_code": 0}),
        );

        let resp = handle_request(req("GET", "/api/operation"), st.clone()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        let events = v["events"].as_array().cloned().unwrap_or_default();

        let sources: Vec<&str> = events.iter().filter_map(|e| e["source"].as_str()).collect();
        assert!(
            sources.contains(&"terminal") && sources.contains(&"browser"),
            "the timeline must carry BOTH feeds, got {sources:?}"
        );
        // The terminal half keeps its reasoning AND its session attribution.
        let cmd = events
            .iter()
            .find(|e| e["command"] == format!("echo operation-probe-{sid}"))
            .expect("the command is on the timeline");
        assert_eq!(cmd["intent"], "the terminal half of the timeline");
        assert_eq!(cmd["session"].as_str(), Some(sid.as_str()));
        // The browser half has no session — the browser is device-level.
        let act = events
            .iter()
            .find(|e| e["source"] == "browser")
            .expect("the browser action is on the timeline");
        assert!(act["session"].is_null());

        // Ordered by the explicit millisecond stamp, and the cursor matches the
        // newest entry so a poller can pass it straight back.
        let stamps: Vec<u64> = events.iter().filter_map(|e| e["ts_ms"].as_u64()).collect();
        let mut sorted = stamps.clone();
        sorted.sort_unstable();
        assert_eq!(stamps, sorted, "the timeline must be time-ordered");
        assert_eq!(
            v["cursor_ms"].as_u64(),
            stamps.last().copied(),
            "cursor_ms must be the newest stamp in the reply"
        );

        st.terminal_mgr.term_close(&sid).await.ok();
        let _ = std::fs::remove_file(cfg_path);
    }

    /// RUN IDENTITY, end to end through the REAL tool surface: `run_begin` mints
    /// an id, a command carries it, and `/api/operation` returns both the run
    /// record and the stamped event so a reader can group them.
    ///
    /// This is the test the whole feature exists for. The unit pins prove the
    /// log and the merge separately; only this proves the three layers agree on
    /// ONE string — and the layers are hand-mirrored (a tool schema, an audit
    /// field, an allowlist mapping), which is exactly where this repo has been
    /// bitten before: a name registered on one side and unknown on the other is
    /// silent with every other gate green.
    #[cfg(all(feature = "terminal", not(target_os = "windows")))]
    #[tokio::test]
    async fn a_run_declared_through_the_tool_surface_groups_the_work_it_names() {
        let (st, cfg_path) = state_with_cfg("run-e2e", CFG_YAML_TOKEN_ONLY);

        // 1. Declare the run through the tool the AI actually calls.
        let resp = handle_request(
            req_with_json(
                "POST",
                "/api/tools/run_begin",
                &serde_json::json!({
                    "label": "provision the ONU",
                    "goal": "get it online",
                })
                .to_string(),
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["ok"], true, "run_begin answers the in-band envelope");
        let run_id = v["result"]["run_id"]
            .as_str()
            .expect("run_begin must return the minted id")
            .to_string();
        assert!(run_id.starts_with("run-"), "id shape: {run_id}");

        // 2. Do some work inside it.
        let sid = st
            .terminal_mgr
            .term_open(&crate::tools::terminal::TermOpenRequest {
                kind: "pty".into(),
                target: String::new(),
                password: String::new(),
                key_path: String::new(),
                rows: 24,
                cols: 80,
                inject_marker: false,
                data_bits: None,
                parity: None,
                stop_bits: None,
                auto_reconnect: false,
            })
            .await
            .unwrap()
            .0;
        let probe = format!("echo run-probe-{sid}");
        let resp = handle_request(
            req_with_json(
                "POST",
                "/api/tools/terminal_execute",
                &serde_json::json!({
                    "session_id": sid,
                    "command": probe,
                    "intent": "the work inside the run",
                    "run_id": run_id,
                })
                .to_string(),
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        // 3. A command with NO run id must stay unattributed — the property that
        //    keeps the grouping honest rather than merely plausible.
        let loose = format!("echo loose-{sid}");
        handle_request(
            req_with_json(
                "POST",
                "/api/tools/terminal_execute",
                &serde_json::json!({"session_id": sid, "command": loose}).to_string(),
            ),
            st.clone(),
        )
        .await;

        // 4. Read the timeline the operator reads.
        let resp = handle_request(req("GET", "/api/operation"), st.clone()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;

        let runs = v["runs"].as_array().cloned().unwrap_or_default();
        let run = runs
            .iter()
            .find(|r| r["run_id"] == run_id.as_str() && r["kind"] == "run/begin")
            .expect("the declared run is on the timeline");
        assert_eq!(run["label"], "provision the ONU");
        assert_eq!(run["goal"], "get it online");

        let events = v["events"].as_array().cloned().unwrap_or_default();
        let mine = events
            .iter()
            .find(|e| e["command"] == probe)
            .expect("the command is on the timeline");
        assert_eq!(
            mine["run_id"].as_str(),
            Some(run_id.as_str()),
            "the command must carry the id run_begin minted — if this fails the id \
             is minted and forgotten, and no view can ever group the work"
        );

        let other = events
            .iter()
            .find(|e| e["command"] == loose)
            .expect("the unattributed command is on the timeline too");
        assert!(
            other["run_id"].is_null(),
            "a command sent without a run must NOT be absorbed into one"
        );

        // 5. Close it, and confirm the closure is visible.
        handle_request(
            req_with_json(
                "POST",
                "/api/tools/run_end",
                &serde_json::json!({"run_id": run_id, "outcome": "done"}).to_string(),
            ),
            st.clone(),
        )
        .await;
        let v = json_body(handle_request(req("GET", "/api/operation"), st.clone()).await).await;
        let runs = v["runs"].as_array().cloned().unwrap_or_default();
        assert_eq!(
            runs.iter()
                .find(|r| r["kind"] == "run/end" && r["run_id"] == run_id.as_str())
                .expect("run_end is recorded")["outcome"],
            "done"
        );

        st.terminal_mgr.term_close(&sid).await.ok();
        let _ = std::fs::remove_file(cfg_path);
    }

    /// A run id is a LABEL: presenting one grants NOTHING.
    ///
    /// The behavioural half of the rule `runs.rs` states in prose and pins by
    /// source scan. The device has one token and possession of it IS the identity
    /// (`web/panel.rs`), so an unauthenticated caller who presents a REAL,
    /// well-formed id minted by this very device must still be refused.
    #[tokio::test]
    async fn a_run_id_never_grants_access() {
        let (st, cfg_path) = state_with_cfg("run-auth", CFG_YAML_TOKEN_ONLY);

        let real = crate::runs::begin(&crate::paths::runs_dir(), Some("real"), None);
        let resp = handle_request(
            req_anon_with_json(
                "POST",
                "/api/tools/terminal_execute",
                &serde_json::json!({"command": "id", "session_id": "s1", "run_id": real})
                    .to_string(),
            ),
            st.clone(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "a run id must never stand in for the device token"
        );

        let resp = handle_request(req_anon("GET", "/api/operation"), st.clone()).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let _ = std::fs::remove_file(cfg_path);
    }

    /// The route is AUTH-GATED — it carries every session's commands, goals and
    /// plans, so an anonymous read is the whole device's activity history.
    #[tokio::test]
    async fn the_operation_route_refuses_an_anonymous_read() {
        let (st, cfg_path) = state_with_cfg("operation-auth", CFG_YAML_TOKEN_ONLY);
        let resp = handle_request(req_anon("GET", "/api/operation"), st).await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "the operation timeline must never be readable without a token"
        );
        let _ = std::fs::remove_file(cfg_path);
    }

    /// THE PLAN, end to end: the agent declares it through the TOOL surface, the
    /// operator's view of the session carries it, a command claims a step, and the
    /// trail records both.
    ///
    /// The plan is the AGENT's statement (a tool), the goal is the OPERATOR's (a
    /// control route) — this drives both and shows they land in different places,
    /// which is the distinction the whole feature rests on.
    #[cfg(all(feature = "terminal", not(target_os = "windows")))]
    #[tokio::test]
    async fn a_declared_plan_reaches_the_session_the_trail_and_the_command() {
        let (st, cfg_path) = state_with_cfg("plan-e2e", CFG_YAML_TOKEN_ONLY);
        let sid = st
            .terminal_mgr
            .term_open(&crate::tools::terminal::TermOpenRequest {
                kind: "pty".into(),
                target: String::new(),
                password: String::new(),
                key_path: String::new(),
                rows: 24,
                cols: 80,
                inject_marker: false,
                data_bits: None,
                parity: None,
                stop_bits: None,
                auto_reconnect: false,
            })
            .await
            .unwrap()
            .0;

        let call = |body: serde_json::Value| {
            let st = st.clone();
            async move {
                handle_request(
                    req_with_json("POST", "/api/tools/terminal_plan", &body.to_string()),
                    st,
                )
                .await
            }
        };

        // (a) Omitted `plan` READS without changing anything.
        let resp = call(serde_json::json!({ "session_id": sid })).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            json_body(resp).await["result"]["plan"]
                .as_array()
                .map(|a| a.len()),
            Some(0)
        );

        // (b) Declaring it.
        let resp = call(serde_json::json!({
            "session_id": sid,
            "plan": ["check the ONU is online", "create VLAN 100", "save the config"],
        }))
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["result"]["plan"].as_array().map(|a| a.len()), Some(3));
        assert_eq!(v["result"]["revised"], true);

        // The operator sees it on the session, next to the goal.
        let listed = handle_request(
            req_with_token("POST", "/api/tools/terminal_list", TEST_TOKEN),
            st.clone(),
        )
        .await;
        let body = json_body(listed).await;
        let row = body["result"]
            .as_array()
            .and_then(|a| a.iter().find(|r| r["id"] == sid.as_str()))
            .cloned()
            .expect("session listed");
        assert_eq!(
            row["plan"].as_array().map(|a| a.len()),
            Some(3),
            "plan on session info"
        );

        // (c) A command CLAIMS a step, and the trail links the two.
        let resp = handle_request(
            req_with_json(
                "POST",
                "/api/tools/terminal_execute",
                &serde_json::json!({
                    "session_id": sid,
                    "command": "echo vlan 100",
                    "intent": "carry out step two",
                    "plan_step": 2,
                })
                .to_string(),
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = handle_request(req("GET", &format!("/api/sessions/{sid}")), st.clone()).await;
        let events = json_body(resp).await["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let plan_ev = events
            .iter()
            .find(|e| e["kind"] == "plan")
            .expect("the declaration is recorded");
        assert_eq!(
            plan_ev["status"].as_str(),
            Some("3"),
            "the step count is recorded"
        );
        assert_eq!(
            plan_ev["text"].as_str(),
            Some("1. check the ONU is online\n2. create VLAN 100\n3. save the config"),
            "the plan is recorded numbered, so a reader sees the sequence"
        );

        let start = events
            .iter()
            .find(|e| e["kind"] == "command/start" && e["command"] == "echo vlan 100")
            .expect("the command is recorded");
        assert_eq!(
            start["plan_step"].as_u64(),
            Some(2),
            "the command must say WHICH step it served — without this, 'the plan \
             was followed' is unfalsifiable"
        );

        // (d) An empty array CLEARS it, and the clear is recorded as a plan event
        // with NO text — a withdrawn plan must not read as a blank one.
        let resp = call(serde_json::json!({ "session_id": sid, "plan": [] })).await;
        assert_eq!(
            json_body(resp).await["result"]["plan"]
                .as_array()
                .map(|a| a.len()),
            Some(0)
        );

        let resp = handle_request(req("GET", &format!("/api/sessions/{sid}")), st.clone()).await;
        let events = json_body(resp).await["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let plans: Vec<&serde_json::Value> =
            events.iter().filter(|e| e["kind"] == "plan").collect();
        assert_eq!(plans.len(), 2, "the clear is recorded too");
        assert!(
            plans[1].get("text").is_none(),
            "a cleared plan carries no text"
        );

        st.terminal_mgr.term_close(&sid).await.ok();
        let _ = std::fs::remove_file(cfg_path);
    }

    /// THE GOAL IS THE OPERATOR'S, THE PLAN IS THE AGENT'S — they live on
    /// different surfaces, and a request cannot move one through the other.
    ///
    /// Worth pinning because it is the kind of distinction a later refactor
    /// collapses for convenience ("both are just session state, put them on the
    /// same route"), and the result would be an operator-declared plan or an
    /// agent-declared goal — either of which destroys the comparison between what
    /// was asked for and what was intended.
    #[cfg(all(feature = "terminal", not(target_os = "windows")))]
    #[tokio::test]
    async fn the_control_route_cannot_declare_a_plan() {
        let (st, cfg_path) = state_with_cfg("plan-split", CFG_YAML_TOKEN_ONLY);
        let sid = st
            .terminal_mgr
            .term_open(&crate::tools::terminal::TermOpenRequest {
                kind: "pty".into(),
                target: String::new(),
                password: String::new(),
                key_path: String::new(),
                rows: 24,
                cols: 80,
                inject_marker: false,
                data_bits: None,
                parity: None,
                stop_bits: None,
                auto_reconnect: false,
            })
            .await
            .unwrap()
            .0;

        // The operator's route rejects the agent's field rather than ignoring it.
        let resp = handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/control"),
                r#"{"plan":["sneak a plan in"]}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "the control route takes holder/approval_required/goal — a silently \
             ignored `plan` would look like it worked"
        );
        assert!(
            st.terminal_mgr.term_plan(&sid).await.unwrap().is_empty(),
            "and nothing may have been stored"
        );

        st.terminal_mgr.term_close(&sid).await.ok();
        let _ = std::fs::remove_file(cfg_path);
    }

    /// THE INTENT LAYER, end to end: reasoning posted to the TOOL surface comes
    /// back out of the audit trail beside the command it explains.
    ///
    /// The unit pins prove the logger stores what it is handed; the schema pin
    /// proves the parameter is advertised. Neither proves the two are CONNECTED —
    /// a handler that reads `intent` and never forwards it, or one that forwards
    /// it to the wrong field, would leave every other test green while the
    /// feature did nothing. This drives the real route.
    #[cfg(all(feature = "terminal", not(target_os = "windows")))]
    #[tokio::test]
    async fn intent_posted_to_the_tool_surface_reaches_the_audit_trail() {
        let (st, cfg_path) = state_with_cfg("intent-e2e", CFG_YAML_TOKEN_ONLY);
        let sid = st
            .terminal_mgr
            .term_open(&crate::tools::terminal::TermOpenRequest {
                kind: "pty".into(),
                target: String::new(),
                password: String::new(),
                key_path: String::new(),
                rows: 24,
                cols: 80,
                inject_marker: false,
                data_bits: None,
                parity: None,
                stop_bits: None,
                auto_reconnect: false,
            })
            .await
            .unwrap()
            .0;

        let body = serde_json::json!({
            "session_id": sid,
            "command": "echo intent-probe",
            "intent": "confirm the session is responsive before changing anything",
            "considered": ["skip the check", "reopen the session instead"],
        });
        let resp = handle_request(
            req_with_json("POST", "/api/tools/terminal_execute", &body.to_string()),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = handle_request(req("GET", &format!("/api/sessions/{sid}")), st.clone()).await;
        let events = json_body(resp).await["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let start = events
            .iter()
            .find(|e| e["kind"] == "command/start")
            .expect("the command was recorded");
        assert_eq!(
            start["intent"].as_str(),
            Some("confirm the session is responsive before changing anything"),
            "the reasoning must travel from the tool call to the trail"
        );
        assert_eq!(
            start["considered"].as_array().map(|a| a.len()),
            Some(2),
            "and the branches not taken must survive the trip too"
        );

        // A malformed `considered` is ABSENT, never a failed execute: the
        // reasoning is optional annotation on a command that should still run.
        let body = serde_json::json!({
            "session_id": sid,
            "command": "echo no-intent",
            "considered": "not-an-array",
        });
        let resp = handle_request(
            req_with_json("POST", "/api/tools/terminal_execute", &body.to_string()),
            st.clone(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "a bad annotation must not stop the command the operator asked for"
        );

        st.terminal_mgr.term_close(&sid).await.ok();
        let _ = std::fs::remove_file(cfg_path);
    }

    /// THE DISPATCH BEAT, end to end: the operator states a goal through the
    /// route and it reaches BOTH the session (so the AI reads it off
    /// `terminal_list`) and the audit trail (so a later reader knows what the
    /// session was FOR).
    ///
    /// Both halves matter and neither implies the other: a goal that only lives
    /// in memory answers "what is it doing"; one that only lives in the log
    /// answers "what was it for". The design's dispatch beat needs the first, and
    /// the evidence beat needs the second.
    #[cfg(all(feature = "terminal", not(target_os = "windows")))]
    #[tokio::test]
    async fn a_goal_set_through_the_route_reaches_the_session_and_the_trail() {
        let (st, cfg_path) = state_with_cfg("goal-e2e", CFG_YAML_TOKEN_ONLY);
        let sid = st
            .terminal_mgr
            .term_open(&crate::tools::terminal::TermOpenRequest {
                kind: "pty".into(),
                target: String::new(),
                password: String::new(),
                key_path: String::new(),
                rows: 24,
                cols: 80,
                inject_marker: false,
                data_bits: None,
                parity: None,
                stop_bits: None,
                auto_reconnect: false,
            })
            .await
            .unwrap()
            .0;

        let resp = handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/control"),
                r#"{"goal":"provision the ONU on VLAN 100"}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["goal"], "provision the ONU on VLAN 100");
        // The OTHER fields were not mentioned, so they must read null rather
        // than being defaulted — a partial patch may not clear its neighbours.
        assert!(v["held_by_human"].is_null());
        assert!(v["approval_required"].is_null());

        // (a) The AI can read it off the tool surface it already polls.
        let listed = handle_request(
            req_with_token("POST", "/api/tools/terminal_list", TEST_TOKEN),
            st.clone(),
        )
        .await;
        let body = json_body(listed).await;
        let row = body["result"]
            .as_array()
            .and_then(|a| a.iter().find(|r| r["id"] == sid.as_str()))
            .cloned()
            .expect("session listed");
        assert_eq!(row["goal"], "provision the ONU on VLAN 100");

        // (b) The trail records the STATEMENT, not just the state.
        let resp = handle_request(req("GET", &format!("/api/sessions/{sid}")), st.clone()).await;
        let events = json_body(resp).await["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let goals: Vec<String> = events
            .iter()
            .filter(|e| e["kind"] == "goal")
            .map(|e| e["text"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(goals, vec!["provision the ONU on VLAN 100".to_string()]);

        // Clearing is an EVENT too: a withdrawn objective must not leave the
        // previous one looking current.
        let _ = handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/control"),
                r#"{"goal":""}"#,
            ),
            st.clone(),
        )
        .await;
        let resp = handle_request(req("GET", &format!("/api/sessions/{sid}")), st.clone()).await;
        let events = json_body(resp).await["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let goals: Vec<String> = events
            .iter()
            .filter(|e| e["kind"] == "goal")
            .map(|e| e["text"].as_str().unwrap_or("<null>").to_string())
            .collect();
        assert_eq!(goals.len(), 2, "the clear must be recorded: {goals:?}");
        assert_eq!(goals[1], "", "the clear logs an EMPTY goal, not nothing");

        st.terminal_mgr.term_close(&sid).await.ok();
        let _ = std::fs::remove_file(cfg_path);
    }

    /// A goal that is not a string is rejected rather than silently ignored.
    #[tokio::test]
    async fn a_non_string_goal_is_rejected() {
        let (st, cfg_path) = state_with_cfg("goal-bad", CFG_YAML_TOKEN_ONLY);
        let resp = handle_request(
            req_with_json("POST", "/api/sessions/abc123/control", r#"{"goal":123}"#),
            st,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(
            json_body(resp).await["error"]
                .as_str()
                .unwrap_or("")
                .contains("goal must be a string"),
            "a number where an objective belongs is a client bug, and guessing \
             which way they meant is the one thing this field must not do"
        );
        let _ = std::fs::remove_file(cfg_path);
    }

    /// `POST /api/sessions/{sid}/control` — the control-handoff route.
    ///
    /// The validation pins matter more than the happy path here: this route
    /// moves the keyboard, so a malformed request must be REJECTED rather than
    /// defaulted. Guessing which way an ambiguous body wanted to move it is the
    /// one thing this endpoint must never do.
    #[tokio::test]
    async fn session_control_rejects_a_malformed_request() {
        let (st, cfg_path) = state_with_cfg("ctl-bad", CFG_YAML_TOKEN_ONLY);

        // Unknown holder value.
        let resp = handle_request(
            req_with_json(
                "POST",
                "/api/sessions/abc123/control",
                r#"{"holder":"robot"}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = json_body(resp).await;
        assert_eq!(v["code"], "invalid_params");
        assert!(
            v["error"].as_str().unwrap_or("").contains("robot"),
            "the error must quote the offending value so the panel can show it"
        );

        // Holder absent entirely — must not default to either side.
        let resp = handle_request(
            req_with_json("POST", "/api/sessions/abc123/control", "{}"),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(resp).await["code"], "invalid_params");

        // Invalid JSON.
        let resp = handle_request(
            req_with_json("POST", "/api/sessions/abc123/control", "{oops"),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // A traversing sid is refused by the SHARED guard (round-116's charset
        // rule); this route must not be the one that forgot it.
        let resp = handle_request(
            req_with_json(
                "POST",
                "/api/sessions/..%5C..%5Cfoo/control",
                r#"{"holder":"human"}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "the sid charset guard must apply to the control route too"
        );

        let _ = std::fs::remove_file(cfg_path);
    }

    /// The control route now carries approval mode too — and each field is
    /// INDEPENDENT, which is the property that matters.
    ///
    /// A partial patch must not clear what it did not mention: "hand the keyboard
    /// back" must not silently disarm the gate, and "arm the gate" must not
    /// silently hand the keyboard over. That is the documented incident class this
    /// repo already guards for the settings bodies.
    #[tokio::test]
    async fn session_control_patches_holder_and_approval_independently() {
        let (st, cfg_path) = state_with_cfg("ctl-patch", CFG_YAML_TOKEN_ONLY);

        // A request naming NEITHER field is a client bug, not a silent no-op.
        let resp = handle_request(
            req_with_json("POST", "/api/sessions/abc123/control", "{}"),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // A NON-boolean approval value reads as ABSENT, so a request that only
        // meant to hand over the keyboard still does exactly that — it must not
        // be rejected, and it must not be coerced to true.
        let resp = handle_request(
            req_with_json(
                "POST",
                "/api/sessions/abc123/control",
                r#"{"holder":"ai","approval_required":"yes"}"#,
            ),
            st.clone(),
        )
        .await;
        let v = json_body(resp).await;
        let err = v["error"].as_str().unwrap_or("");
        assert!(
            !err.contains("approval_required"),
            "a non-boolean approval_required must be read as ABSENT, not rejected \
             as a bad value; got: {v}"
        );

        let _ = std::fs::remove_file(cfg_path);
    }

    /// The approval decision route: the validation is what is worth pinning,
    /// because this route decides whether a blocked command runs.
    #[tokio::test]
    async fn session_approval_requires_a_direction_and_an_id() {
        let (st, cfg_path) = state_with_cfg("ap-bad", CFG_YAML_TOKEN_ONLY);

        // No id.
        let resp = handle_request(
            req_with_json(
                "POST",
                "/api/sessions/abc123/approval",
                r#"{"approve":true}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(
            json_body(resp).await["error"]
                .as_str()
                .unwrap_or("")
                .contains("id is required"),
            "the request id is what binds the answer to the command that was read"
        );

        // No direction — must NOT default. A defaulted "yes" would run a command
        // the operator never authorised.
        let resp = handle_request(
            req_with_json("POST", "/api/sessions/abc123/approval", r#"{"id":"ap-1"}"#),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(
            json_body(resp).await["error"]
                .as_str()
                .unwrap_or("")
                .contains("approve is required"),
            "a decision with no direction is not a decision"
        );

        // `false` is a REAL value, not "absent": a deny must be distinguishable
        // from a missing field, or every deny would be read as malformed.
        let resp = handle_request(
            req_with_json(
                "POST",
                "/api/sessions/deadbeef/approval",
                r#"{"id":"ap-1","approve":false}"#,
            ),
            st.clone(),
        )
        .await;
        let err = json_body(resp).await["error"]
            .as_str()
            .unwrap_or("")
            .to_string();
        assert!(
            !err.contains("approve is required"),
            "approve:false must be accepted as a decision; got: {err}"
        );

        let _ = std::fs::remove_file(cfg_path);
    }

    /// A valid request for a session that does not exist is a CLIENT error: not
    /// a success (the operator would believe they hold a dead keyboard) and not
    /// a 500 (it is a stale sid, which is routine).
    #[tokio::test]
    async fn session_control_on_a_missing_session_is_a_client_error() {
        let (st, cfg_path) = state_with_cfg("ctl-missing", CFG_YAML_TOKEN_ONLY);
        let resp = handle_request(
            req_with_json(
                "POST",
                "/api/sessions/deadbeef/control",
                r#"{"holder":"human"}"#,
            ),
            st,
        )
        .await;
        assert_ne!(
            resp.status(),
            StatusCode::OK,
            "holding a session that does not exist must not report success"
        );
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_file(cfg_path);
    }

    /// END-TO-END: a hold reached through the ROUTE stops an execute reached
    /// through the TOOL surface, and the AI sees the typed code.
    ///
    /// The manager pins prove the refusal and the route pins prove the
    /// validation, but neither proves the two are connected — a hold stored
    /// under one session id and looked up under another, or an envelope that
    /// flattened the code to a string, would leave both suites green while the
    /// AI got the wrong answer. This drives the real dispatch and the real tool
    /// handler against a real PTY.
    #[cfg(all(feature = "terminal", not(target_os = "windows")))]
    #[tokio::test]
    async fn a_hold_set_via_the_route_refuses_the_tool_call() {
        let (st, cfg_path) = state_with_cfg("ctl-e2e", CFG_YAML_TOKEN_ONLY);
        let sid = st
            .terminal_mgr
            .term_open(&crate::tools::terminal::TermOpenRequest {
                kind: "pty".into(),
                target: String::new(),
                password: String::new(),
                key_path: String::new(),
                rows: 24,
                cols: 80,
                inject_marker: false,
                data_bits: None,
                parity: None,
                stop_bits: None,
                auto_reconnect: false,
            })
            .await
            .unwrap()
            .0;

        // The operator takes the keyboard, through the HTTP route.
        let resp = handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/control"),
                r#"{"holder":"human"}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(json_body(resp).await["held_by_human"], true);

        // The AI's next execute must be refused, with the code a client routes on.
        let resp = handle_request(
            req_with_json(
                "POST",
                "/api/tools/terminal_execute",
                &format!(r#"{{"session_id":"{sid}","command":"echo hi"}}"#),
            ),
            st.clone(),
        )
        .await;
        let v = json_body(resp).await;
        assert_eq!(v["ok"], false, "a held session must refuse the execute");
        assert_eq!(
            v["code"], "human_in_control",
            "the AI routes on this code; a flattened or wrong one tells it to retry"
        );

        // Hand back, and the same call is allowed through to the lock.
        let resp = handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/control"),
                r#"{"holder":"ai"}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(json_body(resp).await["held_by_human"], false);

        // THE AUDIT TRAIL must record BOTH handoffs, in order, through the real
        // route. Without this the logger's own pin proves the logger works while
        // nothing proves the route calls it — a mutant that always logs "human"
        // passed every test until this assertion existed.
        let resp = handle_request(req("GET", &format!("/api/sessions/{sid}")), st.clone()).await;
        let events = json_body(resp).await["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let controls: Vec<String> = events
            .iter()
            .filter(|e| e["kind"] == "control")
            .map(|e| e["status"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(
            controls,
            vec!["human".to_string(), "ai".to_string()],
            "the route must record the handoff AND the hand-back, in order: {events:?}"
        );
        // `seq` is documented as per-session monotonic. Each route call builds a
        // FRESH logger via `sessions_logger()`, so the second call must still see
        // the first call's event — if the write were left unflushed in a
        // per-instance buffer, both events would claim the same seq and a reader
        // ordering by seq would see a broken trail.
        let seqs: Vec<u64> = events
            .iter()
            .filter(|e| e["kind"] == "control")
            .map(|e| e["seq"].as_u64().unwrap_or(0))
            .collect();
        assert_eq!(seqs.len(), 2);
        assert!(
            seqs[0] < seqs[1],
            "control events must not share a seq (got {seqs:?}) — the trail is \
             ordered by it, and two equal seqs mean one write never reached disk"
        );

        st.terminal_mgr.term_close(&sid).await.ok();
        let _ = std::fs::remove_file(cfg_path);
    }

    /// END-TO-END: arming the gate makes the TOOL surface wait, and a decision
    /// through the ROUTE releases it.
    ///
    /// The manager pins prove the gate refuses and the route pins prove the
    /// parsing; neither proves the two are connected — an execute that never
    /// consults the mode, or a decision that lands on a different session id,
    /// would leave both suites green while an armed session ran commands
    /// unapproved. This drives the real dispatch and the real tool handler.
    #[cfg(all(feature = "terminal", not(target_os = "windows")))]
    #[tokio::test]
    async fn an_armed_session_makes_the_tool_call_wait_for_a_decision() {
        let (st, cfg_path) = state_with_cfg("ap-e2e", CFG_YAML_TOKEN_ONLY);
        let sid = st
            .terminal_mgr
            .term_open(&crate::tools::terminal::TermOpenRequest {
                kind: "pty".into(),
                target: String::new(),
                password: String::new(),
                key_path: String::new(),
                rows: 24,
                cols: 80,
                inject_marker: false,
                data_bits: None,
                parity: None,
                stop_bits: None,
                auto_reconnect: false,
            })
            .await
            .unwrap()
            .0;

        // Arm the gate through the route.
        let resp = handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/control"),
                r#"{"approval_required":true}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["approval_required"], true);
        // The holder was NOT mentioned, so it must be reported as null rather
        // than defaulted — a partial patch may not clear the other field.
        assert!(v["held_by_human"].is_null());
        assert!(!st.terminal_mgr.term_held_by_human(&sid).await.unwrap());

        // Fire the execute; it must BLOCK at the gate.
        let exec = {
            let st2 = st.clone();
            let sid2 = sid.clone();
            tokio::spawn(async move {
                handle_request(
                    req_with_json(
                        "POST",
                        "/api/tools/terminal_execute",
                        &format!(r#"{{"session_id":"{sid2}","command":"echo gated"}}"#),
                    ),
                    st2,
                )
                .await
            })
        };

        // The prompt must become visible while it waits. BOUNDED, so a mutant
        // that removes the gate fails here with a clear message instead of
        // hanging the suite until the harness gives up.
        let st3 = st.clone();
        let sid3 = sid.clone();
        let id = tokio::time::timeout(std::time::Duration::from_secs(5), async move {
            loop {
                if let Some(p) = st3.terminal_mgr.term_pending_approval(&sid3).await.unwrap() {
                    break p.id;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect(
            "no approval prompt ever appeared: the execute did not consult the \
             session's approval mode, so an ARMED session ran the command with \
             nobody asked",
        );
        assert!(!exec.is_finished(), "the execute must still be waiting");

        // ...and a DENY must surface as a refusal, not as a timeout.
        let resp = handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/approval"),
                &format!(r#"{{"id":"{id}","approve":false}}"#),
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(json_body(resp).await["decided"], true);

        let v = json_body(exec.await.unwrap()).await;
        assert_eq!(v["ok"], false);
        assert_eq!(
            v["code"], "approval_denied",
            "the AI must be told the operator refused, not that nobody answered"
        );

        // Disarm so the session can be closed cleanly.
        let _ = handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/control"),
                r#"{"approval_required":false}"#,
            ),
            st.clone(),
        )
        .await;
        st.terminal_mgr.term_close(&sid).await.ok();
        let _ = std::fs::remove_file(cfg_path);
    }

    /// A REFUSED COMMAND MUST NOT WEDGE THE SESSION.
    ///
    /// The gate sits between `term_acquire_execute` and every normal release, and
    /// a refusal propagates with `?` — so unless that path releases too, the
    /// busy flag stays set and EVERY later execute on the session waits out the
    /// 30 s acquire budget and answers `session_busy`. The device looks alive,
    /// the panel shows a live session, and nothing can run in it again.
    ///
    /// This is the assertion the older gate test could not make: it asserts the
    /// refusal code and then CLOSES the session, so the leak is invisible to it.
    /// The same hole exists for a TIMEOUT, which is the outcome an unattended
    /// device gets — so the bug is not an edge case, it is the normal path
    /// whenever nobody is watching.
    ///
    /// The bound is wall-clock on purpose: "did it release" is exactly the
    /// question, and a leaked lock answers it by hanging. Mutation-proven —
    /// removing the release makes this fail with `session_busy` after ~30 s.
    #[cfg(all(feature = "terminal", not(target_os = "windows")))]
    #[tokio::test]
    async fn a_refused_command_leaves_the_session_usable() {
        let (st, cfg_path) = state_with_cfg("approval-release", CFG_YAML_TOKEN_ONLY);
        let sid = st
            .terminal_mgr
            .term_open(&crate::tools::terminal::TermOpenRequest {
                kind: "pty".into(),
                target: String::new(),
                password: String::new(),
                key_path: String::new(),
                rows: 24,
                cols: 80,
                inject_marker: false,
                data_bits: None,
                parity: None,
                stop_bits: None,
                auto_reconnect: false,
            })
            .await
            .unwrap()
            .0;

        // Arm the gate.
        handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/control"),
                r#"{"approval_required":true}"#,
            ),
            st.clone(),
        )
        .await;

        // First command: refused by the operator.
        let exec = {
            let (st2, sid2) = (st.clone(), sid.clone());
            tokio::spawn(async move {
                handle_request(
                    req_with_json(
                        "POST",
                        "/api/tools/terminal_execute",
                        &format!(r#"{{"session_id":"{sid2}","command":"echo refused"}}"#),
                    ),
                    st2,
                )
                .await
            })
        };
        let id = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(p) = st.terminal_mgr.term_pending_approval(&sid).await.unwrap() {
                    break p.id;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the prompt must appear");
        handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/approval"),
                &format!(r#"{{"id":"{id}","approve":false}}"#),
            ),
            st.clone(),
        )
        .await;
        let v = json_body(exec.await.unwrap()).await;
        assert_eq!(
            v["code"], "approval_denied",
            "the refusal itself is correct"
        );

        // THE ASSERTION: the session must still be drivable. Disarm first so the
        // second command is not gated again — the question is the LOCK, not the
        // gate.
        handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/control"),
                r#"{"approval_required":false}"#,
            ),
            st.clone(),
        )
        .await;

        let second = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            handle_request(
                req_with_json(
                    "POST",
                    "/api/tools/terminal_execute",
                    &format!(r#"{{"session_id":"{sid}","command":"echo after-refusal"}}"#),
                ),
                st.clone(),
            ),
        )
        .await
        .expect(
            "a refused command wedged the session: the next execute never \
             finished. The gate returns with `?` between acquire and release, so \
             a refusal (or a timeout) leaves the busy flag set — the failure \
             mode is a session that looks alive and can never run anything again",
        );
        assert_eq!(second.status(), StatusCode::OK);
        let v2 = json_body(second).await;
        assert_eq!(
            v2["ok"], true,
            "a refused command must not poison the session: {v2}"
        );
        assert_ne!(
            v2["code"], "session_busy",
            "the busy flag leaked out of the refusal path"
        );

        st.terminal_mgr.term_close(&sid).await.ok();
        let _ = std::fs::remove_file(cfg_path);
    }

    /// A PARKED command hands the session back, and a later YES still runs it.
    ///
    /// The end-to-end shape of the rework, through the routes the panel and the
    /// AI actually call. Two properties, and each was broken before:
    ///
    ///   * the session is USABLE while a question is open. The gate used to hold
    ///     the execute lock for the whole wait, so an unanswered prompt froze the
    ///     session; now the park releases it. Asserted as a wall-clock bound
    ///     because "did it release" is exactly the question and a leaked lock
    ///     answers it by hanging.
    ///   * a LATE YES still runs the command. The execute has returned by then,
    ///     so without the permit the operator's answer would decide a request
    ///     nobody is waiting on — recorded, and doing nothing.
    #[cfg(all(feature = "terminal", not(target_os = "windows")))]
    #[tokio::test]
    async fn a_parked_command_hands_the_session_back_and_a_late_yes_still_runs_it() {
        let (st, cfg_path) = state_with_cfg("approval-park", CFG_YAML_TOKEN_ONLY);
        let sid = st
            .terminal_mgr
            .term_open(&crate::tools::terminal::TermOpenRequest {
                kind: "pty".into(),
                target: String::new(),
                password: String::new(),
                key_path: String::new(),
                rows: 24,
                cols: 80,
                inject_marker: false,
                data_bits: None,
                parity: None,
                stop_bits: None,
                auto_reconnect: false,
            })
            .await
            .unwrap()
            .0;

        handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/control"),
                r#"{"approval_required":true}"#,
            ),
            st.clone(),
        )
        .await;

        // Drive the gate directly with a SHORT block, so the test exercises the
        // park without waiting the production minute. The route's own budget is
        // covered by the unit tests; what this test is about is what the CALLER
        // gets and what the session looks like afterwards.
        let outcome = st
            .terminal_mgr
            .term_await_approval(&sid, "echo parked", 200)
            .await
            .expect("a park is not an error");
        let crate::tools::terminal::ApprovalOutcome::Parked { id, expires_in_ms } = outcome else {
            panic!("expected a park, got {outcome:?}");
        };
        assert!(
            expires_in_ms > 60_000,
            "the countdown the AI is handed must be the QUESTION's TTL, not the \
             block that just expired — got {expires_in_ms}ms, which would tell a \
             client to give up at the moment the question became answerable"
        );

        // THE SESSION IS USABLE. Disarm so the next command is not gated again:
        // the question here is the LOCK, not the gate.
        handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/control"),
                r#"{"approval_required":false}"#,
            ),
            st.clone(),
        )
        .await;
        let second = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            handle_request(
                req_with_json(
                    "POST",
                    "/api/tools/terminal_execute",
                    &format!(r#"{{"session_id":"{sid}","command":"echo after-park"}}"#),
                ),
                st.clone(),
            ),
        )
        .await
        .expect(
            "a parked command wedged the session: the next execute never \
             finished, so an unanswered question froze the session it asked about",
        );
        assert_eq!(second.status(), StatusCode::OK);
        assert_ne!(
            json_body(second).await["code"],
            "session_busy",
            "the busy flag leaked out of the park path"
        );

        // THE LATE YES STILL RUNS IT. Re-arm, register a question, park on it,
        // then answer — and the same command with the id must go through.
        handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/control"),
                r#"{"approval_required":true}"#,
            ),
            st.clone(),
        )
        .await;
        let outcome = st
            .terminal_mgr
            .term_await_approval(&sid, "echo late-yes", 200)
            .await
            .unwrap();
        let crate::tools::terminal::ApprovalOutcome::Parked { id: late_id, .. } = outcome else {
            panic!("expected a park");
        };

        // The operator answers AFTER the park, through the real route.
        let resp = handle_request(
            req_with_json(
                "POST",
                &format!("/api/sessions/{sid}/approval"),
                &format!(r#"{{"id":"{late_id}","approve":true}}"#),
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            json_body(resp).await["decided"],
            true,
            "a parked question must still be answerable inside its TTL — if this \
             is false the operator's click is silently discarded"
        );

        // The AI re-issues with approval_id: it runs, unasked.
        let run = handle_request(
            req_with_json(
                "POST",
                "/api/tools/terminal_execute",
                &format!(
                    r#"{{"session_id":"{sid}","command":"echo late-yes","approval_id":"{late_id}"}}"#
                ),
            ),
            st.clone(),
        )
        .await;
        let v = json_body(run).await;
        assert_eq!(v["ok"], true, "the permitted command must run: {v}");
        assert_ne!(
            v["state"], "awaiting_approval",
            "a command holding a valid permit must NOT be asked about again — \
             that would throw away the answer the operator already gave"
        );

        let _ = id;
        st.terminal_mgr.term_close(&sid).await.ok();
        let _ = std::fs::remove_file(cfg_path);
    }

    /// `/api/logs` reads WHERE THE WRITERS WRITE.
    ///
    /// The regression this pins is a WIRE-UP bug, not a logic one: the handler
    /// read `exe_dir()/summrise-update.log` while layout v2 had already moved that
    /// file — and `agent.log`, `installer.log`, `install-result.txt`,
    /// `startup.log` with it — into `DataDir\logs`. The route therefore read a
    /// path the migration had emptied and could only ever answer `""`, on every
    /// v2 device, while every writer had followed the move. Nothing failed: the
    /// reply was well-formed and empty.
    ///
    /// So the test plants a marker where the writers ACTUALLY write and asserts
    /// the route returns it — and plants a different one where the buggy code
    /// looked, asserting the route does NOT return that. Asserting only the
    /// first half would pass against a handler that read both.
    #[tokio::test]
    async fn api_logs_reads_the_dir_the_writers_use() {
        let _live_dir = lock_data_dir().await;
        let logs = crate::paths::logs_dir();
        std::fs::create_dir_all(&logs).unwrap();
        let real = logs.join("agent.log");
        let marker = format!("REAL-LOG-MARKER-{}", std::process::id());
        let had_real = std::fs::read_to_string(&real).ok();
        std::fs::write(&real, format!("{marker}\n")).unwrap();

        // The path the buggy code resolved. On a v2 device this file does not
        // exist at all, which is exactly why the route looked empty; the test
        // creates it so the assertion is about WHICH path is read rather than
        // about which file happens to exist.
        let exe = crate::paths::exe_dir();
        let decoy = exe.join("summrise-update.log");
        let had_decoy = std::fs::read_to_string(&decoy).ok();
        let decoy_marker = format!("DECOY-MARKER-{}", std::process::id());
        if std::fs::create_dir_all(&exe).is_ok() {
            let _ = std::fs::write(&decoy, format!("{decoy_marker}\n"));
        }

        let resp = handle_request(req("GET", "/api/logs"), state()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        let body = v.to_string();
        // The body is CLIPPED in the failure messages: a real agent.log plus
        // mcp_diag.log is megabytes, and an assertion that dumps them makes a
        // failing test unreadable exactly when it is needed most.
        assert!(
            body.contains(&marker),
            "the route must read `logs_dir()` — where the update script, \
             filelog.rs and the mcp-client diag writer all put their logs. \
             Reply head: {}",
            crate::text::clip(&body, 400)
        );
        assert!(
            !body.contains(&decoy_marker),
            "the route must NOT read `exe_dir()` — layout v2 MOVES the logs out \
             of there, so that path is empty on every real device and the route \
             silently answered '' forever. Reply head: {}",
            crate::text::clip(&body, 400)
        );

        // STARTUP.LOG IS SERVED, AND IT IS THE ONE THAT EXPLAINS A MASS 401.
        // The doc comment above has listed `startup.log` among the files layout
        // v2 moved into this directory since the route was written, while the
        // served set was three files without it — so the only record of a
        // rotated `device_token` was the one the operator's log card could not
        // read, and the symptom it explains is total (every client 401s) and
        // otherwise unexplained. Asserted by NAME, not by count: a count would
        // pass if a different file were dropped to make room.
        let names: Vec<String> = v["logs"]
            .as_array()
            .expect("logs array")
            .iter()
            .filter_map(|l| l["name"].as_str().map(|n| n.to_string()))
            .collect();
        assert!(
            names.iter().any(|n| n == "startup.log"),
            "api_logs must serve startup.log — it carries the quarantine and \
             token-rotation narration, which is the only account of why every \
             client starts failing auth: {names:?}"
        );

        // Restore whatever was there, so a test run does not leave a fake log.
        match had_real {
            Some(prev) => std::fs::write(&real, prev).unwrap(),
            None => {
                let _ = std::fs::remove_file(&real);
            }
        }
        match had_decoy {
            Some(prev) => std::fs::write(&decoy, prev).unwrap(),
            None => {
                let _ = std::fs::remove_file(&decoy);
            }
        }
    }

    /// THE ROUTE-COVERAGE SECURITY PIN (SOLID R102).
    ///
    /// Every route the web surface dispatches must be reachable ONLY with the
    /// device token: the device token is the single gate between an
    /// unauthenticated network caller and SYSTEM-level device control
    /// (terminal_execute, system_file_write, …). A new route added without
    /// auth fails HERE instead of shipping. Both failure modes are checked —
    /// a missing header and a wrong token — because they take different paths
    /// through `check_auth`.
    ///
    /// THE LIST IS `routes()` NOW, WHICH IS WHAT THIS BUYS OVER THE HAND-WRITTEN LIST IT REPLACES.
    /// The old list had to be kept in step with the dispatcher BY HAND, and it had already drifted:
    /// eight routes lived in `dispatch` and in no list — `/api/update` and `/api/run/mark-exit` among
    /// them, found 2026-09-24 by the agent-web exploration — while this test reported the surface
    /// covered. The two source scans written to catch the next drift were worse than they looked: one
    /// ended at the next `    async fn` instead of at the end of `dispatch`, so it ran on for another
    /// twelve hundred lines, and the sibling in `streaming_routes_share_one_slot_acquisition_point`
    /// asserted a literal that occurred exactly once in the file — inside the assertion itself.
    ///
    /// With the table as the subject there is no second copy left to drift: every row IS this test's
    /// input, the walk cannot be partial, and a route that ships unproven is now IMPOSSIBLE rather
    /// than detected afterwards. The `stage` column is what the walk reads, so a new route's auth
    /// posture is decided when the route is added, in the one place a reader is already looking.
    ///
    /// `/mcp` is still driven through `handle_request` even though axum's `nest_service` normally
    /// keeps it away from this service: the `Mcp` rows are in the table, and asserting them here is
    /// defence-in-depth against a routing change.
    #[tokio::test]
    async fn every_dispatch_route_is_auth_gated() {
        for row in routes() {
            let (m, path) = (row.method, probe_path(row));
            let p = path.as_str();
            match row.stage {
                // Each of the three owners is a REFUSAL for an anonymous caller and for a caller
                // holding the WRONG token — whichever of the three answers, and whichever way it
                // answers (a fast refusal from `route_pre_dispatch`, or the unconditional gate
                // further down).
                Stage::DispatchGated | Stage::PreDispatchAuthed | Stage::Mcp => {
                    let r = handle_request(req_anon(m, p), state()).await;
                    assert_eq!(
                        r.status(),
                        StatusCode::UNAUTHORIZED,
                        "{m} {p} served WITHOUT an Authorization header"
                    );
                    let r =
                        handle_request(req_with_token(m, p, "not-the-device-token"), state()).await;
                    assert_eq!(
                        r.status(),
                        StatusCode::UNAUTHORIZED,
                        "{m} {p} served with a WRONG token"
                    );
                }
                // …and a `Public` row is the mirror image: an anonymous request must NOT be 401, or
                // an auth tightening would lock the panel, the status page or the discovery document
                // out of a device whose owner has the token in hand.
                Stage::Public => {
                    let r = handle_request(req_anon(m, p), state()).await;
                    assert_ne!(
                        r.status(),
                        StatusCode::UNAUTHORIZED,
                        "{m} {p} is a `Public` row and answered 401 without a credential"
                    );
                }
            }
        }
    }

    /// The PUBLIC surface, pinned so an auth tightening cannot silently lock
    /// the panel/status page out (the mirror image of the test above).
    ///
    /// `/` serves a static page naming no device data, and the panel/desktop
    /// SPA is public *by design*: it shows nothing until the user supplies a
    /// token, and the token is injected server-side only for the authorised
    /// paths (loopback / gateway proxy secret / one-time grant) — see
    /// handle_panel_home.
    #[tokio::test]
    async fn deliberately_public_routes_stay_public() {
        for (m, p) in [
            ("GET", "/"),
            ("GET", "/panel"),
            ("GET", "/panel/"),
            ("GET", "/desktop"),
            ("GET", "/desktop/"),
            ("GET", "/panel/panel.js"),
            ("GET", "/desktop/panel.css"),
            ("GET", "/some-unknown-page"),
        ] {
            let r = handle_request(req_anon(m, p), state()).await;
            assert_ne!(
                r.status(),
                StatusCode::UNAUTHORIZED,
                "{m} {p} must stay public (it is a documented public surface)"
            );
        }
    }

    #[tokio::test]
    async fn auth_ok_with_bearer_token() {
        let mut cfg = Config::default();
        cfg.server.device_token = Some("sekret".into());
        let st = Arc::new(AppState::new(cfg));
        let resp = handle_request(req_with_token("GET", "/api/status", "sekret"), st).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn status_page_is_public() {
        // The root page needs no token — it carries no data beyond the version.
        let resp = handle_request(req("GET", "/"), state()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("summrise-agent"));
    }

    #[tokio::test]
    async fn term_sse_requires_auth() {
        let mut cfg = Config::default();
        cfg.server.device_token = Some("sekret".into());
        let st = Arc::new(AppState::new(cfg));
        let resp = handle_request(req("GET", "/api/events/term"), st).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn term_sse_streams_output() {
        use http_body_util::BodyExt;
        let _sse = crate::web::sse::SSE_TEST_LOCK.lock().await;
        let st = state();
        let resp = handle_request(req("GET", "/api/events/term"), st.clone()).await;
        assert_eq!(resp.status(), StatusCode::OK);

        st.event_bus
            .emit_term_output(serde_json::json!({"session_id": "term-0", "data": [104, 105]}));

        // Read just the first frame — the SSE stream never closes, so the whole
        // body can't be drained with to_bytes.
        let mut body = resp.into_body();
        let frame = tokio::time::timeout(std::time::Duration::from_secs(2), body.frame())
            .await
            .expect("SSE frame within timeout")
            .expect("stream produced a frame")
            .expect("frame ok");
        let bytes = frame.into_data().expect("data frame");
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("term-0"),
            "SSE frame missing session id: {text}"
        );
        assert!(text.starts_with("data: "));
    }

    // ── Settings + Gateway-card endpoints (persist paths) ────────────────
    //
    // Offline coverage for GET/PUT /api/settings and the POST
    // /api/gateway/connect persist-only arm. The reg-key exchange and the
    // tunnel provisioning arms need the network and are deliberately NOT
    // tested here.

    const CFG_URL: &str = "https://gw.example";

    /// A config with a device_token and no console binding.
    /// Write-through era: the device_token in the FILE must equal the token
    /// the harness authenticates with — in production AppState::new is
    /// seeded from the very config.yaml at config_path (run_server:
    /// load_config(argv[1])), so the harness mirrors that exactly.
    const CFG_YAML_TOKEN_ONLY: &str = "server:\n  device_token: test-token\nterminal:\n  buffer_mb: 8\nplatform:\n  console_url: null\n";
    /// A config with a device_token, a bound console_url and buffer_mb 8.
    const CFG_YAML_TOKEN_AND_URL: &str = "server:\n  device_token: test-token\nterminal:\n  buffer_mb: 8\nplatform:\n  console_url: https://gw.example\n";

    /// State whose config_path points at a per-test tempdir config.yaml —
    /// mirrors main.rs (persist to the ACTUALLY-LOADED path). The in-memory
    /// snapshot IS the file's config, exactly like the production boot
    /// (AppState::new(load_config(argv[1]))). No global state: every test
    /// owns its directory and removes it.
    fn state_with_cfg(tag: &str, yaml: &str) -> (Arc<AppState>, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("summrise-web-cfg-{}-{tag}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let cfg_path = dir.join("config.yaml");
        std::fs::write(&cfg_path, yaml).unwrap();
        let st = Arc::new(AppState::new(Config::load(&cfg_path).unwrap()));
        *st.config_path.lock().unwrap_or_else(|p| p.into_inner()) = Some(cfg_path.clone());
        (st, cfg_path)
    }

    fn req_with_json(method: &str, path: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(path)
            .header("Authorization", format!("Bearer {TEST_TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn settings_get_shape() {
        let (st, cfg_path) = state_with_cfg("get-shape", CFG_YAML_TOKEN_ONLY);
        let resp = handle_request(req("GET", "/api/settings"), st).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["ok"], true);
        assert!(v["buffer_mb"].is_u64(), "buffer_mb must be a number: {v}");
        assert!(
            v["tunnel_configured"].is_boolean(),
            "tunnel_configured shape: {v}"
        );
        assert!(
            v["tunnel_running"].is_boolean(),
            "tunnel_running shape: {v}"
        );
        assert!(
            v["console_url"].is_null(),
            "unbound config must read back null: {v}"
        );
        // AND THE KEY SET, against the fixture the panel is checked against. The shape assertions
        // above cover the fields someone thought to name; this covers the ones nobody did. The risk on
        // THIS route is not a blank card but a silent WRITE: SettingsPage reads each field behind a
        // typeof guard and falls back to a default, so a renamed `buffer_mb` shows the default and the
        // next Save persists it back.
        let raw = include_str!("../../tests/fixtures/settings.json");
        let fixture: serde_json::Value = serde_json::from_str(raw).expect("fixture parses");
        let promised: std::collections::BTreeSet<&str> = fixture["keys"]
            .as_array()
            .expect("keys")
            .iter()
            .filter_map(|k| k.as_str())
            .collect();
        let keys: std::collections::BTreeSet<&str> = v
            .as_object()
            .expect("object")
            .keys()
            .map(|k| k.as_str())
            .collect();
        let undeclared: Vec<&&str> = keys.difference(&promised).collect();
        assert!(
            undeclared.is_empty(),
            "the response carries fields the fixture does not declare: {undeclared:?}"
        );
        for k in fixture["required_by_panel"].as_array().expect("required") {
            let k = k.as_str().expect("string");
            assert!(
                keys.contains(k),
                "the panel reads `{k}`, which this response does not carry: {keys:?}"
            );
        }
        let _ = std::fs::remove_dir_all(cfg_path.parent().unwrap());
    }

    /// The operator can SET a memory cap; they must be able to SEE the fill.
    ///
    /// `/api/settings` reported `memory_max_entries` / `memory_max_bytes_mb` /
    /// `memory_retention_days` and nothing about usage, so a cap could be
    /// lowered to below the current contents — after which the device begins
    /// silently evicting the OLDEST knowledge — with no way to notice from the
    /// UI. The only accessor that could answer it, `total_bytes_live`, was
    /// documented "Test/reliability hook" and had exactly one caller: its own
    /// test.
    ///
    /// THE ORDER OF THIS WORK MATTERED: shipping a meter before round 20's
    /// ledger fix would have published a number that was WRONG after any edit
    /// (undercount) or soft-delete (overcount) — a meter nobody can trust is
    /// worse than no meter, because it drives decisions. The ledger moves with
    /// the record now, so these two assertions can be exact rather than
    /// approximate.
    #[tokio::test]
    async fn settings_reports_memory_usage_not_just_the_caps() {
        let (st, cfg_path) = state_with_cfg("mem-usage", CFG_YAML_TOKEN_ONLY);

        // MEASURE DELTAS, NOT ABSOLUTES. `AppState::new` builds its memory store
        // on `default_memory_dir()`, a PROCESS-GLOBAL path — every web test in
        // this binary shares one store, so a previous test's records are still
        // there. Asserting an absolute 0 failed exactly that way: a leftover
        // 1 entry / 16 bytes, intermittently, depending on test order.
        //
        // Same trap round 20 hit from the other side (a test reading shared
        // state and calling it its own). There the fix was a unique dir; here
        // the path is not mine to choose, so the assertion measures what THIS
        // test added. What matters is unchanged: usage must be PRESENT as a
        // number even when nothing was added, so a UI can divide it by the cap
        // without special-casing an absent field.
        let before = json_body(handle_request(req("GET", "/api/settings"), st.clone()).await).await;
        let base_entries = before["memory_entries"]
            .as_u64()
            .unwrap_or_else(|| panic!("usage present as a number: {before}"));
        let base_bytes = before["memory_bytes"]
            .as_u64()
            .unwrap_or_else(|| panic!("usage present as a number: {before}"));
        assert!(
            before["memory_max_entries"].is_u64(),
            "usage and cap must be the same kind of number so they can be divided: {before}"
        );

        // Two records of known size: the meter must reflect EXACTLY what the
        // cap is measured against. An edit is included on purpose — it is the
        // write path whose ledger was wrong before round 20.
        let store = st.memory.clone();
        // Ids are unique to THIS test. The store is process-global and keyed by
        // id, so a fixed "m-alpha" left behind by another test would make
        // `insert` an UPDATE — the count would move by 1, not 2, which is
        // exactly how this assertion first failed.
        let uniq = format!("{}-{}", std::process::id(), crate::now_millis());
        let mk = |title: &str, content: &str| crate::plugins::memory::store::MemoryRecord {
            id: format!("m-{uniq}-{title}"),
            title: title.to_string(),
            content: content.to_string(),
            tags: vec![],
            namespace: "shared".to_string(),
            source: "test".to_string(),
            run_id: None,
            created_at: crate::unix_now(),
            updated_at: crate::unix_now(),
            deleted: false,
        };
        // `insert` returns Option (a failed append must be visible); this test store is
        // writable, so unwrap with the reason.
        let a = store
            .insert(mk("alpha", "0123456789"))
            .expect("the record must be PERSISTED in a test store"); // 10 bytes
        store
            .insert(mk("beta", "abc"))
            .expect("the record must be PERSISTED in a test store"); // 3 bytes
        store.update(&a, None, Some("0123456789ABCDEF".into()), None, None, None); // -> 16

        let v = json_body(handle_request(req("GET", "/api/settings"), st.clone()).await).await;
        assert_eq!(
            v["memory_entries"].as_u64(),
            base_entries.checked_add(2),
            "two records added: {v}"
        );
        assert_eq!(
            v["memory_bytes"].as_u64(),
            base_bytes.checked_add(19),
            "16 + 3. The meter must be the SAME ledger the byte cap evicts on — \
             a reader that recomputes it some other way can disagree with the \
             eviction it is supposed to explain: {v}"
        );

        // A soft-delete must leave the meter, not just the list.
        let b = store.list(None, None, 10, false);
        let doomed = b.iter().find(|r| r.title == "beta").unwrap().id.clone();
        store.delete(&doomed);
        let v = json_body(handle_request(req("GET", "/api/settings"), st.clone()).await).await;
        assert_eq!(
            v["memory_entries"].as_u64(),
            base_entries.checked_add(1),
            "tombstones are not live entries: {v}"
        );
        assert_eq!(
            v["memory_bytes"].as_u64(),
            base_bytes.checked_add(16),
            "tombstone bytes leave the meter: {v}"
        );

        let _ = std::fs::remove_dir_all(cfg_path.parent().unwrap());
    }

    /// "THIS SESSION LEFT NO RECORD" AND "I COULD NOT READ THE RECORD" ARE
    /// DIFFERENT ANSWERS.
    ///
    /// `/api/sessions/{sid}` answered `200 {ok:true, events:[]}` for both: the
    /// handler collapsed `read_events`'s `Option` with `unwrap_or_default()`.
    /// A reader therefore cannot tell a session whose file is gone (retention,
    /// a different data dir, a typo'd id) from one that genuinely recorded
    /// nothing — and the panel that consumes this had to word its empty state
    /// to cover both possibilities, which is the honest version of a question
    /// the API should have answered.
    ///
    /// `ok` stays true in both cases: the REQUEST succeeded. What differs is
    /// whether a record exists, which is what `found` reports.
    #[tokio::test]
    async fn session_events_distinguishes_no_record_from_an_empty_one() {
        let (st, cfg_path) = state_with_cfg("events-found", CFG_YAML_TOKEN_ONLY);

        // Nothing on disk for this id: `found` is false, and it is PRESENT so a
        // consumer can branch on it rather than on an absence of events.
        let resp = handle_request(
            req("GET", "/api/sessions/no-such-session-anywhere"),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["ok"], true, "the request succeeded: {v}");
        assert_eq!(
            v["first_seq"], 0,
            "no record means no beginning to report — 0, not a fabricated 1: {v}"
        );
        assert_eq!(
            v["found"], false,
            "a session with no readable record must SAY so rather than \
             answering with an empty list that reads as 'it recorded nothing': {v}"
        );
        assert_eq!(v["events"].as_array().map(|a| a.len()), Some(0), "{v}");

        // Now a session that WAS recorded: `found` true, events present.
        let logger = sessions_logger();
        logger.log_command_start("web-events-found", "echo hi");
        let resp = handle_request(req("GET", "/api/sessions/web-events-found"), st.clone()).await;
        let v = json_body(resp).await;
        assert_eq!(v["found"], true, "a readable record is found: {v}");
        assert!(
            v["events"].as_array().is_some_and(|a| !a.is_empty()),
            "and it carries the events: {v}"
        );
        // A record that was NOT trimmed begins at 1. This is the negative half
        // of the completeness signal: `first_seq > 1` must mean something, and
        // it cannot if every record reports a large number.
        assert_eq!(
            v["first_seq"], 1,
            "an untrimmed record begins at its first event: {v}"
        );

        let _ = std::fs::remove_dir_all(cfg_path.parent().unwrap());
    }

    /// THE ARCHIVE ROW MUST SAY WHAT THE SESSION WAS.
    ///
    /// A recorded session used to be an opaque `term-<hex>-<n>`: the live
    /// kind/label die with the process, the id leaks nothing, and d1's archive
    /// held 620 such rows. The kind and the operator-facing label now ride the
    /// session's version HEADER, so the list route can answer from one line
    /// instead of reading a trail that can be megabytes.
    ///
    /// The two arms are asserted separately because they mean different things:
    /// a known session must NAME itself, and a record from before the field
    /// existed must OMIT the keys — sending `null` or a placeholder would make
    /// "unknown" indistinguishable from a real value at the consumer.
    #[tokio::test]
    async fn the_session_list_names_what_each_session_was() {
        let (st, cfg_path) = state_with_cfg("sess-identity", CFG_YAML_TOKEN_ONLY);
        let dir = crate::paths::sessions_dir();
        let _ = std::fs::create_dir_all(&dir);
        let mk = format!("web-ident-{}-{}", std::process::id(), crate::now_millis());

        // One session that staged its identity, and one that did not.
        let known = format!("{mk}-known");
        let anon = format!("{mk}-anon");
        let logger = sessions_logger();
        logger.remember_identity(&known, "ssh", "ops@box-a");
        logger.log_status(&known, "opened");
        logger.log_status(&anon, "opened");
        logger.flush_all();

        let v = json_body(handle_request(req("GET", "/api/sessions"), st.clone()).await).await;
        let rows = v["sessions"].as_array().expect("sessions array");
        let find = |id: &str| {
            rows.iter()
                .find(|r| r["id"] == id)
                .unwrap_or_else(|| panic!("{id} must be listed"))
                .clone()
        };

        let k = find(&known);
        assert_eq!(k["kind"], "ssh", "the row names the kind: {k}");
        assert_eq!(
            k["label"], "ops@box-a",
            "and the operator-facing label: {k}"
        );

        let a = find(&anon);
        assert!(
            a.get("kind").is_none() && a.get("label").is_none(),
            "an unidentified record must OMIT the keys, so a consumer can tell \
             'the device does not know' from a real value: {a}"
        );

        let _ = std::fs::remove_file(dir.join(format!("{known}.jsonl")));
        let _ = std::fs::remove_file(dir.join(format!("{anon}.jsonl")));
        let _ = std::fs::remove_dir_all(cfg_path.parent().unwrap());
    }

    #[tokio::test]
    async fn settings_put_persists_and_preserves_token() {
        let (st, cfg_path) = state_with_cfg("put-token", CFG_YAML_TOKEN_ONLY);
        let resp = handle_request(
            req_with_json("PUT", "/api/settings", r#"{"buffer_mb": 32}"#),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["ok"], true);
        assert_eq!(v["buffer_mb"].as_u64(), Some(32));
        // The runtime cap took effect immediately (panel hint: applies to NEW
        // output), and the file now carries 8 → 32.
        assert_eq!(
            st.terminal_buf_bytes
                .load(std::sync::atomic::Ordering::Relaxed),
            32 * 1024 * 1024
        );
        let cfg = Config::load(&cfg_path).unwrap();
        assert_eq!(
            cfg.terminal.buffer_mb, 32,
            "PUT must persist to config.yaml"
        );
        // HIGH(audit): a settings write must NEVER drop the device_token —
        // a token-less rewrite makes the next boot mint a NEW token and 401
        // every client (the recorded rotation incident).
        assert_eq!(
            cfg.server.device_token.as_deref(),
            Some(TEST_TOKEN),
            "device_token must survive PUT /api/settings"
        );
        let _ = std::fs::remove_dir_all(cfg_path.parent().unwrap());
    }

    #[tokio::test]
    async fn settings_put_partial_leaves_omitted_keys() {
        let (st, cfg_path) = state_with_cfg("put-partial", CFG_YAML_TOKEN_AND_URL);
        // 1. buffer-only PUT: the bound console_url must survive.
        let resp = handle_request(
            req_with_json("PUT", "/api/settings", r#"{"buffer_mb": 16}"#),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let cfg = Config::load(&cfg_path).unwrap();
        assert_eq!(cfg.terminal.buffer_mb, 16);
        assert_eq!(
            cfg.platform.console_url.as_deref(),
            Some(CFG_URL),
            "buffer-only PUT must not clear console_url"
        );
        // 2. console_url-only PUT: buffer_mb stays 16 (the stage-n settings
        //    audit: the old code reset an ABSENT buffer_mb to 8).
        let resp = handle_request(
            req_with_json(
                "PUT",
                "/api/settings",
                r#"{"console_url": "https://other.example"}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let cfg = Config::load(&cfg_path).unwrap();
        assert_eq!(
            cfg.platform.console_url.as_deref(),
            Some("https://other.example")
        );
        assert_eq!(
            cfg.terminal.buffer_mb, 16,
            "console_url-only PUT must not reset buffer_mb"
        );
        assert_eq!(cfg.server.device_token.as_deref(), Some(TEST_TOKEN));
        let _ = std::fs::remove_dir_all(cfg_path.parent().unwrap());
    }

    #[tokio::test]
    // round-41 audit pin, RESOLVED by product sign-off (2026-09-08):
    // api_settings_put's invalid-JSON envelope now returns HTTP 400, matching
    // api_gateway_connect. The old HTTP-200 Json shape is gone on purpose.
    async fn settings_put_invalid_json_returns_http400_envelope() {
        let (st, _cfg_path) = state_with_cfg("put-bad-json", CFG_YAML_TOKEN_ONLY);
        let resp = handle_request(
            req_with_json("PUT", "/api/settings", "{not json"),
            st.clone(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "unified 400 envelope"
        );
        let v = json_body(resp).await;
        assert_eq!(v["ok"], false);
        assert_eq!(v["code"], "invalid_params");
        assert!(
            v["error"].as_str().unwrap_or("").contains("invalid JSON"),
            "error mentions the parse failure"
        );
    }

    #[tokio::test]
    async fn settings_put_visible_in_memory_and_file() {
        // Audit A4 write-through: a PUT must be visible IN-PROCESS (the live
        // snapshot, no disk reload, no restart — the old state.config was a
        // frozen boot snapshot) AND land in config.yaml. Both sources move
        // together; neither can drift.
        let (st, cfg_path) = state_with_cfg("put-memory", CFG_YAML_TOKEN_ONLY);
        let resp = handle_request(
            req_with_json(
                "PUT",
                "/api/settings",
                r#"{"buffer_mb": 32, "console_url": "https://mem.example"}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        // 1. In-memory visibility: config_snapshot() reflects the change
        //    WITHOUT touching the disk (this assert reads only the RwLock).
        let snap = st.config_snapshot();
        assert_eq!(
            snap.terminal.buffer_mb, 32,
            "in-memory buffer_mb must update"
        );
        assert_eq!(
            snap.platform.console_url.as_deref(),
            Some("https://mem.example"),
            "in-memory console_url must update"
        );
        assert_eq!(
            snap.server.device_token.as_deref(),
            Some(TEST_TOKEN),
            "device_token survives in memory too"
        );
        // 2. The file was written (persist=true) with the same values.
        let disk = Config::load(&cfg_path).unwrap();
        assert_eq!(
            disk.terminal.buffer_mb, 32,
            "persist=true must write config.yaml"
        );
        assert_eq!(
            disk.platform.console_url.as_deref(),
            Some("https://mem.example")
        );
        let _ = std::fs::remove_dir_all(cfg_path.parent().unwrap());
    }

    #[tokio::test]
    async fn settings_memory_roundtrip_live_and_persisted() {
        // Round-358: memory capacity joins GET/PUT /api/settings. This test
        // is the SOLE writer of memory limits in the suite, so its
        // GET-defaults asserts cannot race with another test.
        let (st, cfg_path) = state_with_cfg("put-memlim", CFG_YAML_TOKEN_ONLY);
        // Defaults first (GET shape carries the memory fields).
        let resp = handle_request(req("GET", "/api/settings"), st.clone()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["memory_max_entries"].as_u64(), Some(10_000));
        assert_eq!(v["memory_max_bytes_mb"].as_u64(), Some(64));
        assert!(
            v["memory_retention_days"].is_null(),
            "default retention is keep-forever: {v}"
        );
        // PUT: live limits retune immediately + persist to config.yaml.
        let resp = handle_request(
            req_with_json(
                "PUT",
                "/api/settings",
                r#"{"memory_max_entries": 50, "memory_max_bytes_mb": 16, "memory_retention_days": 30}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["memory_max_entries"].as_u64(), Some(50));
        assert_eq!(v["memory_max_bytes_mb"].as_u64(), Some(16));
        assert_eq!(v["memory_retention_days"].as_u64(), Some(30));
        let live = st.memory.limits();
        assert_eq!(live.max_entries, 50);
        assert_eq!(live.max_bytes, 16 * 1024 * 1024);
        assert_eq!(live.retention_days, Some(30));
        let disk = Config::load(&cfg_path).unwrap();
        assert_eq!(disk.memory.max_entries, Some(50));
        assert_eq!(disk.memory.max_bytes, Some(16 * 1024 * 1024));
        assert_eq!(disk.memory.retention_days, Some(30));
        assert_eq!(disk.server.device_token.as_deref(), Some(TEST_TOKEN));
        // Clear retention back to keep-forever with 0; other keys untouched.
        let resp = handle_request(
            req_with_json("PUT", "/api/settings", r#"{"memory_retention_days": 0}"#),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(st.memory.limits().retention_days, None);
        assert_eq!(
            st.memory.limits().max_entries,
            50,
            "retention-only PUT must not reset entries"
        );
        // Restore shared defaults — AppState::new shares the default memory
        // dir across web tests; a lowered cap must not leak into others.
        let resp = handle_request(
            req_with_json(
                "PUT",
                "/api/settings",
                r#"{"memory_max_entries": 10000, "memory_max_bytes_mb": 64}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let live = st.memory.limits();
        assert_eq!(
            (live.max_entries, live.max_bytes),
            (10_000, 64 * 1024 * 1024)
        );
        let _ = std::fs::remove_dir_all(cfg_path.parent().unwrap());
    }

    #[tokio::test]
    async fn gateway_connect_reports_exactly_what_it_persists() {
        // SOLID R104: the reported `console_url` and the PERSISTED binding are
        // one evaluation of the "trimmed, blank ⇒ unset" rule. They used to be
        // two independent copies that merely happened to agree — editing one
        // would have made the device answer with a binding it had not stored
        // (or stored one it did not report).
        //
        // The tuples also pin a contract detail worth having on the record:
        // the response field ECHOES THE REQUEST, it is not a read-back of the
        // resulting state. A reg-key-only connect therefore answers
        // `console_url: null` while the stored binding is left untouched —
        // which is exactly the "partial update" semantics the handler's
        // comment describes (a reg-key-only request must not unbind).
        for (tag, body, reported, persisted) in [
            (
                "trim",
                r#"{"console_url":"  https://trim.example  "}"#,
                Some("https://trim.example"),
                Some("https://trim.example"),
            ),
            ("clear", r#"{"console_url":""}"#, None, None),
            ("blank", r#"{"console_url":"   "}"#, None, None),
            ("nonstring", r#"{"console_url":42}"#, None, None),
            (
                "regkey-only",
                r#"{"reg_key":"k"}"#,
                None,
                // CFG_YAML_TOKEN_AND_URL's pre-existing binding, KEPT.
                Some("https://gw.example"),
            ),
        ] {
            // Start from a BOUND gateway so the "absent" arm has something to keep.
            let (st, cfg_path) = state_with_cfg(tag, CFG_YAML_TOKEN_AND_URL);
            let resp = handle_request(
                req_with_json("POST", "/api/gateway/connect", body),
                st.clone(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::OK, "{tag}");
            let v = json_body(resp).await;

            assert_eq!(
                v["console_url"].as_str(),
                reported,
                "{tag}: response does not echo the request"
            );
            assert_eq!(
                st.config_snapshot().platform.console_url.as_deref(),
                persisted,
                "{tag}: in-memory binding disagrees with the parsed patch"
            );
            let cfg = Config::load(&cfg_path).unwrap();
            assert_eq!(
                cfg.platform.console_url.as_deref(),
                persisted,
                "{tag}: persisted binding disagrees with the in-memory one"
            );
            let _ = std::fs::remove_dir_all(cfg_path.parent().unwrap());
        }
    }

    #[tokio::test]
    async fn gateway_connect_persist_only_arm() {
        // No reg_key → the config is persisted and the reg-key exchange +
        // tunnel provisioning are SKIPPED (no outbound HTTP on this arm).
        let (st, cfg_path) = state_with_cfg("gw-persist", CFG_YAML_TOKEN_ONLY);
        let resp = handle_request(
            req_with_json(
                "POST",
                "/api/gateway/connect",
                r#"{"console_url": "https://conn.example"}"#,
            ),
            st.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["ok"], true);
        assert_eq!(v["registered"], false, "no reg_key → no registration");
        assert_eq!(v["tunnel"], "skipped");
        assert_eq!(v["console_url"], "https://conn.example");
        // Write-through (audit A4): the binding is ALSO visible in-process —
        // no restart, no disk reload.
        assert_eq!(
            st.config_snapshot().platform.console_url.as_deref(),
            Some("https://conn.example"),
            "gateway connect must update the in-memory config too"
        );
        let cfg = Config::load(&cfg_path).unwrap();
        assert_eq!(
            cfg.platform.console_url.as_deref(),
            Some("https://conn.example"),
            "connect must persist the binding"
        );
        assert_eq!(
            cfg.server.device_token.as_deref(),
            Some(TEST_TOKEN),
            "device_token must survive the gateway-card write too"
        );
        let _ = std::fs::remove_dir_all(cfg_path.parent().unwrap());
    }

    #[tokio::test]
    async fn gateway_connect_reg_key_only_keeps_binding() {
        // reg_key WITHOUT console_url: absent = keep the existing binding
        // (the partial-PUT audit flagged a reg-key-only request silently
        // UNBINDING the gateway). With no console_url the reg-key exchange
        // has nowhere to POST, so this arm stays offline.
        let (st, cfg_path) = state_with_cfg("gw-regkey", CFG_YAML_TOKEN_AND_URL);
        let resp = handle_request(
            req_with_json("POST", "/api/gateway/connect", r#"{"reg_key": "some-key"}"#),
            st,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["ok"], true);
        assert_eq!(v["registered"], false);
        assert!(
            v["console_url"].is_null(),
            "reg-key-only request reports no binding change: {v}"
        );
        let cfg = Config::load(&cfg_path).unwrap();
        assert_eq!(
            cfg.platform.console_url.as_deref(),
            Some(CFG_URL),
            "reg-key-only request must keep the existing binding"
        );
        let _ = std::fs::remove_dir_all(cfg_path.parent().unwrap());
    }

    #[tokio::test]
    // round-41/49 audit pair: api_settings_put returns its invalid-JSON
    // envelope as HTTP 200 (historical round-69 shape, pinned separately)
    // while api_gateway_connect 400s the same class of error. This pin
    // documents the contrast so the OPEN-decision unification is a visible
    // wire change on BOTH endpoints, never a silent drift on one.
    async fn gateway_connect_invalid_json_returns_http400() {
        let (st, _cfg_path) = state_with_cfg("gw-bad-json", CFG_YAML_TOKEN_ONLY);
        let resp = handle_request(
            req_with_json("POST", "/api/gateway/connect", "{not json"),
            st,
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "gateway-connect 400s"
        );
        let v = json_body(resp).await;
        assert_eq!(v["ok"], false);
        assert_eq!(v["code"], "invalid_params");
    }

    #[tokio::test]
    async fn token_rotation_takes_effect_on_api_and_mcp_gate() {
        // Round-366 regression: TokenGate held a BOOT-time token clone while
        // /api/* read the live snapshot, so a runtime rotation stale-accepted
        // the old token on /mcp and rejected the new one. Both gates must
        // read the live snapshot per request.
        use std::convert::Infallible;
        use std::pin::Pin;
        use std::task::{Context, Poll};

        struct OkSvc;
        impl Service<Request<Body>> for OkSvc {
            type Response = axum::http::Response<McpBoxBody>;
            type Error = Infallible;
            type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Infallible>> + Send>>;
            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
                Poll::Ready(Ok(()))
            }
            fn call(&mut self, _req: Request<Body>) -> Self::Future {
                Box::pin(async {
                    Ok(axum::http::Response::new(
                        http_body_util::combinators::BoxBody::new(http_body_util::Full::new(
                            bytes::Bytes::from_static(b"ok"),
                        )),
                    ))
                })
            }
        }

        let st = state();
        assert!(check_auth(None, req("GET", "/api/status").headers(), &st)
            .await
            .is_ok());
        let mut gate = TokenGate::new(OkSvc, st.clone());
        let res = Service::call(&mut gate, req_with_token("POST", "/mcp", TEST_TOKEN))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let mut cfg = st.config_snapshot();
        cfg.server.device_token = Some("rotated-token".into());
        st.update_config(cfg, false).unwrap();

        // /api/* path: old rejected, new accepted.
        assert!(check_auth(
            None,
            req_with_token("GET", "/api/status", TEST_TOKEN).headers(),
            &st
        )
        .await
        .is_err());
        assert!(check_auth(
            None,
            req_with_token("GET", "/api/status", "rotated-token").headers(),
            &st
        )
        .await
        .is_ok());
        // /mcp path: old rejected, new reaches the inner service.
        let res = Service::call(&mut gate, req_with_token("POST", "/mcp", TEST_TOKEN))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let res = Service::call(&mut gate, req_with_token("POST", "/mcp", "rotated-token"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    // SOLID Round-43: the single constant-time compare guarding every /api
    // and /mcp route (round-116) had zero direct pins — only incidental
    // exercise through authed-handler tests. Truth table for the gate the
    // device token depends on.
    #[test]
    fn timing_safe_eq_truth_table() {
        assert!(timing_safe_eq(b"abc", b"abc"));
        assert!(timing_safe_eq(b"", b""));
        assert!(!timing_safe_eq(b"abc", b"abd"), "one bit differs");
        assert!(!timing_safe_eq(b"abc", b"ab"), "length differs (shorter)");
        assert!(!timing_safe_eq(b"ab", b"abc"), "length differs (longer)");
        assert!(!timing_safe_eq(b"", b"a"), "empty vs non-empty");
        // 64-byte token shape: last byte differs.
        let a = vec![0xABu8; 64];
        let mut b = a.clone();
        b[63] ^= 1;
        assert!(!timing_safe_eq(&a, &b));
    }

    /// The SSE viewer cap is REAL, and it is RELEASED (SOLID R124 → R128).
    ///
    /// The R124 pin asserted the OPPOSITE — that 70 held responses produced
    /// ZERO 503s, because the guard was a local dropped when the response was
    /// constructed. That pin did its job: it failed the moment the guard moved
    /// into the streaming task. It is now inverted, and the HOLD half is
    /// phrased to actually discriminate — which took two attempts:
    ///
    ///   * Counting refusals DURING the opens proves nothing about holding.
    ///     The acquires are synchronous, so 70 opens against a 64-slot pool
    ///     refuse 6 of them whether or not the slot survives afterwards; the
    ///     first version of this test therefore PASSED against a mutant that
    ///     dropped the guard at task start (verified by mutation).
    ///   * The discriminating question is asked AFTER the pump tasks have had
    ///     a chance to run: with 64 accepted streams still held, ONE MORE
    ///     request must be refused. A dropped guard frees the pool and that
    ///     request succeeds instead.
    ///
    /// The RELEASE half then pins the other failure mode: dropping the
    /// responses must return the slots, or a long-lived agent would refuse
    /// every viewer forever after 64 total connections.
    #[tokio::test]
    async fn sse_viewer_cap_holds_and_releases_per_connection() {
        // This test DRAINS the process-global pool, so it takes the shared lock
        // every other pool-touching test also takes. Without it the parallel
        // SSE tests receive 503 — the exact starvation the sse.rs NOTE used to
        // forbid by banning a drain test outright, which in turn made the cap's
        // enforcement untestable (and hid the R124 bug).
        let _sse = crate::web::sse::SSE_TEST_LOCK.lock().await;
        let st = state();
        let max = crate::web::sse::SSE_MAX_CONNECTIONS;

        let open = |st: Arc<AppState>| async move {
            let r = Request::builder()
                .method("GET")
                .uri("/api/events")
                .header("Authorization", format!("Bearer {TEST_TOKEN}"))
                .body(Body::empty())
                .unwrap();
            let (parts, _) = r.into_parts();
            route_pre_dispatch(
                &parts.method,
                "/api/events",
                None,
                &parts.headers,
                &st,
                None,
                false,
            )
            .await
            .expect("GET /api/events must be handled by the pre-dispatch layer")
        };

        // Fill the pool, keeping every accepted response alive.
        let mut held = Vec::new();
        let mut refused = None;
        for _ in 0..(max + 6) {
            let resp = open(st.clone()).await;
            if resp.status() != StatusCode::SERVICE_UNAVAILABLE {
                held.push(resp);
            } else if refused.is_none() {
                refused = Some(resp);
            }
        }
        assert_eq!(
            held.len(),
            max,
            "expected the pool to fill to exactly {max}"
        );

        // THE REFUSAL SAYS WHY, IN THE SHAPE THE CLIENT PARSES. The panel's `lib/api.ts` surfaces a body
        // only when it is JSON carrying a string `error` — its comment says "otherwise keep the HTTP
        // status" — so this response used to reach the operator as a bare "HTTP 503" while the most
        // useful sentence on this surface sat in a text/plain body nobody read. The message was always
        // here; the envelope was not.
        let refused = refused.expect("filling the pool past its cap must refuse at least once");
        assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(refused.into_body(), 1 << 20)
            .await
            .unwrap();
        let j: serde_json::Value = serde_json::from_slice(&body).expect("the refusal must be JSON");
        assert_eq!(j["ok"], serde_json::json!(false));
        assert!(
            j["error"]
                .as_str()
                .unwrap_or("")
                .contains("too many SSE viewers"),
            "the operator-facing reason must survive the envelope: {j}"
        );

        // Let every pump task run. A guard that is dropped when the task
        // STARTS would free its slot here.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // ── 1. HOLD ────────────────────────────────────────────────────────
        // With 64 accepted streams still held, one more request must be
        // refused. This is the assertion that a dropped guard cannot satisfy.
        let extra = open(st.clone()).await;
        assert_eq!(
            extra.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "the viewer cap is NOT enforced: a request was ACCEPTED while {max} \
             streams were still open. The guard is not surviving the stream — \
             that is the R124 regression, and it means the documented bound \
             does not exist."
        );
        drop(extra);

        // ── 2. RELEASE ─────────────────────────────────────────────────────
        drop(held);
        let mut freed = false;
        for _ in 0..50 {
            let resp = open(st.clone()).await;
            if resp.status() != StatusCode::SERVICE_UNAVAILABLE {
                freed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(
            freed,
            "slots were NOT released after the streams were dropped — the pool \
             leaks, so an agent that has served {max} connections would refuse \
             every viewer forever. The guard must be released when the \
             streaming task ENDS, promptly (tx.closed()), not at the next \
             heartbeat tick."
        );
    }

    /// Every streaming route must acquire its SSE slot through the ONE shared
    /// path (SOLID R125).
    ///
    /// `sse_route_response` exists so the `_guard` lifetime question has a
    /// single answer. Before the extraction the two branches were
    /// byte-identical apart from their stream function, which meant the
    /// sign-off-pending fix (the guard must be owned by the RESPONSE, not by
    /// the function that builds it — see the ledger's Open threads) would have
    /// had to be applied twice, correctly, in two places. This test keeps that
    /// from regressing: it fails if a streaming route is answered WITHOUT the
    /// shared helper, or if the helper itself stops acquiring a slot.
    ///
    /// It is a SOURCE SCAN, and its limits are stated rather than implied: it
    /// checks shape, not behaviour. The behavioural half is
    /// `sse_viewer_cap_is_not_held_for_the_connection_lifetime` above plus the
    /// auth/stream tests in this module.
    #[test]
    fn streaming_routes_share_one_slot_acquisition_point() {
        const SRC: &str = include_str!("mod.rs");
        // Strip comments before counting. A naive scan counts MENTIONS, and
        // `acquire_sse_guard` is named in this file's docs (including this very
        // test) — the same false-positive trap the boot-surface and module-map
        // gates hit. Only CODE lines may count.
        // Two false-positive sources, both hit while writing this:
        //   * `///` doc comments MENTION the helper by name (including, as it
        //     happens, the comment two screens up);
        //   * this test's own string literals contain "acquire_sse_guard()".
        // So: drop the test module, then strip line comments.
        let production = match SRC.find("#[cfg(test)]") {
            Some(i) => &SRC[..i],
            None => SRC,
        };
        let code: String = production
            .lines()
            .map(|l| match l.find("//") {
                Some(i) => &l[..i],
                None => l,
            })
            .collect::<Vec<_>>()
            .join("\n");
        // The helper is the only place a guard is acquired...
        let acquisitions = code.matches("acquire_sse_guard()").count();
        assert_eq!(
            acquisitions, 1,
            "acquire_sse_guard() must be CALLED in exactly ONE place (the shared \
             helper), found {acquisitions}. If a streaming route grew its own \
             acquisition, the pending guard-lifetime fix now has to be applied \
             twice — and the two copies will drift."
        );
        // ...and that one place is the helper, not some other function that
        // happens to be first in the file.
        let helper_body = code
            .split("async fn sse_route_response")
            .nth(1)
            .and_then(|rest| rest.split("async fn route_pre_dispatch").next())
            .expect("sse_route_response must still exist");
        assert!(
            helper_body.contains("acquire_sse_guard()"),
            "the ONE acquisition is not inside sse_route_response"
        );
        // ...and both streaming routes go through it.
        //
        // COUNTED BY FUNCTION NAME, BECAUSE THE ASSERTION THIS REPLACES COULD NOT FAIL. It asserted
        // that the file CONTAINS `sse_route_response(headers, state, sse_stream)` — and once commit
        // 528e7548 threaded a new first argument (`peer`) through both real calls, that literal
        // occurred exactly ONCE in the whole file: three lines below, inside the assertion itself.
        // `SRC.contains(...)` was satisfied by the very text asking the question, so the two routes
        // this test claims to guard were covered by nothing. A count of the NAME cannot be satisfied
        // that way: there is no call expression in the assertion for it to match.
        //
        // THE MUTATION THAT MUST FAIL: answering a streaming route inline — calling `sse_stream` (or
        // `sse_term_stream`) directly, or copying `sse_route_response`'s body into the branch — removes
        // one of the two calls and drops this count to 1. VERIFIED by replacing `/api/events`' call with
        // a plain `built_response`: exit 101, "production code calls `sse_route_response(` 1 time(s)".
        // The sibling assertion above bit too, on the mutation it exists for — a branch that takes its
        // OWN slot with `acquire_sse_guard()` gives "found 2" (verified the same way).
        //
        // WHAT A COUNT CANNOT SEE, measured rather than assumed. I also routed `/api/events` through a
        // one-line pass-through wrapper that calls the helper, and this test PASSED. That is the right
        // answer, and the reason is worth keeping: the wrapper still acquires the slot through the one
        // helper, so the property this test guards — ONE acquisition point, so the guard's lifetime has
        // one home — still holds. A NAME COUNT sees indirection and cannot see whether the acquisition
        // moved; the assertion above sees the acquisition and cannot see indirection. Together they
        // cover what a text scan can honestly cover, and neither is asked to do the other's job.
        let calls = code.matches("sse_route_response(").count();
        assert_eq!(
            calls, 2,
            "the two streaming routes (/api/events and /api/events/term) must be answered through \
             the shared helper; production code calls `sse_route_response(` {calls} time(s). \
             Answering one of them inline re-opens the two-place fix. (This counts the function \
             NAME on purpose: an assertion built from a full call expression with arguments is the \
             shape that went vacuous here, because the test's own text satisfied it.)"
        );
        // AUTH IS NOT RE-CHECKED HERE, deliberately. The obvious assertion —
        // "the helper body mentions check_auth" — does NOT discriminate: a
        // mutant that replaces the call with `let _ = check_auth(...)` still
        // contains the string and still passes, which I verified by applying
        // it. Keeping a check that cannot fail would be worse than none.
        //
        // Authentication of this path is covered BEHAVIOURALLY, which is
        // stronger: `term_sse_requires_auth` below fails under exactly that
        // mutant (verified), and `route_pre_dispatch`'s own auth coverage
        // (R102's route walk) covers the rest.
    }
    /// The blank-token fail-open, in BOTH gates.
    ///
    /// `timing_safe_eq(b"", b"")` is TRUE, and both gates failed closed only on
    /// ABSENCE (`None`), never on BLANKNESS. So `device_token: Some("")` made
    /// `Authorization: Bearer ` — an empty value — authenticate every `/api/*`
    /// route, and `TokenGate` had the identical shape for `/mcp`. Reachability
    /// today is blocked only by `ensure_token()` in another crate, which is not a
    /// defence: a gate must not depend on its caller having sanitized its input.
    ///
    /// Round-104 hardened the proxy-secret gate against exactly this; these two
    /// were the pair that was missed.
    #[test]
    fn a_blank_configured_token_is_never_usable() {
        assert!(
            !token_is_usable(""),
            "an empty token must never gate anything"
        );
        assert!(
            !token_is_usable("   \t "),
            "whitespace-only is blank too — `Bearer    ` must not authenticate"
        );
        // The real shape must keep working.
        assert!(token_is_usable("a".repeat(64).as_str()));
        assert!(token_is_usable("0f8c1d2e"));
    }

    /// And the two gates must actually USE it — a correct helper nobody calls is
    /// the same defect with better spelling.
    #[test]
    fn both_token_gates_consult_token_is_usable() {
        const SRC: &str = include_str!("mod.rs");
        // Only CODE counts: drop the test module (this test's prose names the
        // helper) and strip line comments. A naive scan counts mentions.
        let production = match SRC.find("#[cfg(test)]") {
            Some(i) => &SRC[..i],
            None => SRC,
        };
        let code: String = production
            .lines()
            .map(|l| match l.find("//") {
                Some(i) => &l[..i],
                None => l,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            code.matches("token_is_usable(").count(),
            3,
            "expected exactly one DEFINITION plus one CALL at each of the two gates \
             (check_auth and TokenGate); a gate that stopped consulting it is the \
             fail-open this test exists to prevent"
        );
    }

    /// A CLAMPED VALUE IS APPLIED, AND THE ENDPOINT SAYS WHICH (round 164).
    ///
    /// `api_settings_put` clamps `buffer_mb` to 1..=64 and persists the clamped value; the
    /// response echoes THAT value, so a client that sent 1000 can see what it got. The bounds
    /// are spelled in four places (this clamp, `state.rs`, `config.rs`'s doc and the panel's
    /// SettingsPage) — this pins the end a caller experiences, so a silent no-op or a rejection
    /// would both be caught.
    #[tokio::test]
    async fn buffer_mb_is_clamped_and_the_effective_value_is_what_sticks() {
        let (st, cfg_path) = state_with_cfg("clamp", CFG_YAML_TOKEN_ONLY);

        let over = handle_request(
            req_with_json("PUT", "/api/settings", r#"{"buffer_mb": 1000}"#),
            st.clone(),
        )
        .await;
        assert_eq!(over.status(), StatusCode::OK);
        assert_eq!(
            st.config_snapshot().terminal.buffer_mb,
            64,
            "an over-range value must clamp to the ceiling, not be rejected or ignored"
        );

        let under = handle_request(
            req_with_json("PUT", "/api/settings", r#"{"buffer_mb": 0}"#),
            st.clone(),
        )
        .await;
        assert_eq!(under.status(), StatusCode::OK);
        assert_eq!(
            st.config_snapshot().terminal.buffer_mb,
            1,
            "and an under-range one to the floor"
        );

        let mid = handle_request(
            req_with_json("PUT", "/api/settings", r#"{"buffer_mb": 32}"#),
            st.clone(),
        )
        .await;
        assert_eq!(mid.status(), StatusCode::OK);
        assert_eq!(
            st.config_snapshot().terminal.buffer_mb,
            32,
            "an in-range value passes through — the clamp is not a constant"
        );
        let _ = std::fs::remove_dir_all(cfg_path.parent().unwrap());
    }
}
