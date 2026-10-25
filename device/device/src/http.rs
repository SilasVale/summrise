//! The loopback HTTP surface: health, state, and the authorization decision.
//!
//! Health is deliberately unauthenticated and answers even when the token file is missing: a
//! supervisor and a human need one endpoint that says the process is up before anything else can be
//! trusted. Everything that reads device state or asks for a decision requires the install token,
//! compared over bytes in constant time and never logged.

use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use summrise_core::{
    authorize, ApprovalState, Ctx, Decision, DenyReason, Subject, ToolCall, Transport,
};

use crate::capabilities::{self, terminal};
use crate::config::{Config, Release};
use crate::helper;
use crate::store::Store;
use crate::watchdog;

/// Build the API's router. Test-only: production always has a console and a mesh slot to fill.
#[cfg(test)]
pub(crate) fn router(config: &Config, store: Store) -> Router {
    router_with(config, store, terminal::Limits::default(), None, None, None)
}

/// A router whose terminal capability uses `limits`. Test-only: an in-process test shortens the
/// write bound instead of waiting the production five seconds.
#[cfg(test)]
pub(crate) fn router_with_limits(
    config: &Config,
    store: Store,
    limits: terminal::Limits,
) -> Router {
    router_with(config, store, limits, None, None, None)
}

fn router_with(
    config: &Config,
    store: Store,
    limits: terminal::Limits,
    console: Option<crate::console::ConsoleEndpoint>,
    mesh: Option<crate::mesh::MeshLink>,
    helper: Option<Arc<helper::Helper>>,
) -> Router {
    let state = AppState {
        token: load_token(&config.token_file),
        store,
        release: config.release(),
        helper,
    };

    // `/mcp` is the same secret as `/api/state`: the gate runs before the MCP service sees the
    // request, so an unauthenticated caller cannot even start a handshake. The body limit is inside
    // the token gate (layers wrap in reverse order of their addition), so an unauthorized caller is
    // refused before its body is collected.
    let mcp = Router::new()
        .route_service("/mcp", crate::mcp::service(config, limits, console, mesh))
        .route_layer(middleware::from_fn(crate::mcp::limit_request_body))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_install_token,
        ));

    Router::new()
        // The device's own panel: the surface a person standing at this machine uses. It needs no
        // token because it needs no network — the listener is loopback, so "can open it" already
        // means "is on this machine". Its data route refuses anything that is not loopback, so a
        // reverse proxy in front of 127.0.0.1 cannot turn it into a remote read.
        .route("/", get(local_panel))
        .route("/api/panel", get(panel_snapshot))
        .route("/api/health", get(health))
        .route("/api/state", get(current_state))
        .route("/api/authorize", post(authorize_request))
        // The per-user helper's registration: a loopback call with the token this boot wrote to
        // `ProgramData`. It is the only route the helper may call, and it only says where to find it.
        .route("/helper", post(register_helper))
        .merge(mcp)
        .with_state(state)
}

/// The `/mcp` token gate: the same bearer check as the JSON routes, before any MCP handling.
async fn require_install_token(
    State(app): State<AppState>,
    request: axum::extract::Request,
    next: middleware::Next,
) -> Response {
    if !app.authorized(request.headers()) {
        return unauthorized();
    }

    next.run(request).await
}

/// Serve the API on `127.0.0.1:<port>` — the loopback interface only, never every interface.
///
/// Binding loopback is the local half of the security model (§9.1): the token makes `Owner` mean
/// something, but only this address keeps the endpoint off the network in the first place.
pub async fn serve(
    config: Config,
    store: Store,
    console: Option<crate::console::ConsoleEndpoint>,
    mesh: Option<crate::mesh::MeshLink>,
    helper: Option<Arc<helper::Helper>>,
) -> io::Result<()> {
    let app = router_with(
        &config,
        store,
        terminal::Limits::default(),
        console,
        mesh,
        helper,
    );
    let listener = bind(&config).await?;
    let address = listener.local_addr()?;

    // F5/I4: the supervisor notices an exit, not a hang. This thread is the half of the recovery the
    // app owns: after two missed self-probes the app exits, the supervisor (which owns the child)
    // sees that exit and restarts it. The SCM's failure actions apply to the supervisor, not to this
    // process, and are reached only if the supervisor gives up and exits nonzero.
    watchdog::spawn(address.port());

    eprintln!("summrise-device: listening on http://{address}");
    // ConnectInfo is what lets `/api/panel` tell a caller standing on this machine from one that
    // reached it through something else. Without it every request looks equally local.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
}

fn loopback_addr(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

async fn bind(config: &Config) -> io::Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(loopback_addr(config.port)).await
}

/// Everything a handler needs: the state file, the token read once at startup, and the release this
/// process was started as (the fact health reports).
#[derive(Clone)]
struct AppState {
    /// The install token, or `None` when the file was missing or unusable. `None` fails closed:
    /// every protected request is 401 until the token file exists and the app is restarted.
    token: Option<[u8; 32]>,
    store: Store,
    /// The release facts from the launcher's argv, or the crate version for a standalone start.
    release: Release,
    /// This boot's helper identity, or `None` when the helper is not in play. The helper is optional
    /// by design: without it the device still serves every tool, it just has no user-session browser.
    helper: Option<Arc<helper::Helper>>,
}

impl AppState {
    /// Whether the request carries the install token, compared in constant time over its bytes.
    fn authorized(&self, headers: &HeaderMap) -> bool {
        let Some(expected) = self.token else {
            return false;
        };
        let Some(presented) = bearer_token(headers) else {
            return false;
        };

        match decode_hex_32(presented) {
            Some(presented) => constant_time_eq(&presented, &expected),
            None => false,
        }
    }
}

/// Readiness, not liveness: the answer follows the state document on every probe.
///
/// A process whose `state.json` cannot be read is still alive, but it cannot record a session row,
/// reconcile a swap, or repair anything: to the launcher's probes and the app's own watchdog it must
/// not look healthy. Re-reading is the cheaper correct choice here — one small file per probe, and
/// the answer recovers the moment an operator replaces the document, without a restart or a flag
/// that a successful read would have to remember to clear.
async fn health(State(app): State<AppState>) -> Response {
    match app.store.load() {
        Ok(_) => Json(Health {
            status: "ok",
            version: app.release.version.clone(),
            channel: app.release.channel.clone(),
            pid: std::process::id(),
        })
        .into_response(),
        Err(error) => {
            eprintln!("summrise-device: /api/health is degraded: {error}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(DegradedHealth {
                    status: "degraded",
                    reason: format!("state: {error}"),
                }),
            )
                .into_response()
        }
    }
}

/// `GET /`: the device-local panel.
async fn local_panel() -> Html<&'static str> {
    Html(PANEL_PAGE)
}

/// The panel's page, compiled in: the device has no assets directory to lose.
const PANEL_PAGE: &str = include_str!("panel.html");

/// `GET /api/panel`: what this machine is doing, for a caller standing on it.
///
/// Loopback-only by peer address. The shape is deliberately small and read-only: identity, build,
/// how long it has been up, whether the console link is live, and the last few events — the questions
/// `CONTEXT.md` says only the device can answer.
async fn panel_snapshot(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    if !peer.ip().is_loopback() {
        return error_response(StatusCode::FORBIDDEN, "loopback_only", None);
    }
    let state = app.store.load().ok();
    let events = app.store.tail_events(10).unwrap_or_default();
    Json(serde_json::json!({
        "version": app.release.version,
        "channel": app.release.channel,
        "sha256": app.release.sha256,
        "pid": std::process::id(),
        "state": state,
        "events": events,
        // The console link's own story. A device cannot report a failure while it is failing, so this
        // is the last attempt it knows about — and it is the one place that knows it: the panel, on the
        // machine, which is where an operator looks when the console says "offline".
        "console_attempt": crate::console::last_attempt(),
    }))
    .into_response()
}

async fn current_state(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if !app.authorized(&headers) {
        return unauthorized();
    }

    match app.store.load() {
        Ok(state) => Json(state).into_response(),
        Err(error) => {
            // The path and the parse failure are diagnostic; the token is not in either.
            eprintln!("summrise-device: GET /api/state could not read the document: {error}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "state_unavailable",
                Some(error.to_string()),
            )
        }
    }
}

async fn authorize_request(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AuthorizeRequest>,
) -> Response {
    if !app.authorized(&headers) {
        return unauthorized();
    }

    // The body names a tool and nothing else (fix design F7). The subject is the install token's
    // owner, the transport is the authenticated loopback channel, `destructive` is the device's table,
    // and the clock is the device's: none of those are expressible on the wire, so a caller cannot
    // assert `owner`, downgrade `destructive`, or move the clock to revive an expired grant.
    let Some(destructive) = capabilities::destructive(&request.tool) else {
        return Json(DecisionDto::from(Decision::Deny {
            reason: DenyReason::DefaultDeny,
        }))
        .into_response();
    };

    let context = Ctx {
        subject: Subject::Owner,
        grant: None,
        transport: Transport::Loopback,
        approval: ApprovalState::NotRequired,
        now_ms: now_ms(),
    };
    let tool = ToolCall::new(request.tool, destructive);

    Json(DecisionDto::from(authorize(&context, &tool))).into_response()
}

/// The device's clock, in Unix milliseconds: the only clock a decision uses.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// The health document: the one answer that needs no token.
#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: String,
    channel: String,
    pid: u32,
}

/// The degraded health answer: the process serves, but it is not ready to do its job.
///
/// `reason` always starts with the subsystem that failed (`state: …`), so a prober can branch on the
/// prefix and an operator reads the concrete error rather than a generic "unhealthy".
#[derive(Serialize)]
struct DegradedHealth {
    status: &'static str,
    reason: String,
}

/// The one error shape the surface has; `detail` is for a human, `error` for a client.
#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// `POST /helper`: the per-user helper says where it listens.
///
/// The body is the helper's port and pid; the token is the one this boot wrote to `ProgramData`, so
/// the only caller that can register is the process running in the logged-in user's session. A
/// registration is not authority: it lets the device *ask* the helper to launch a browser, and the
/// helper still decides what it will do.
#[derive(serde::Deserialize)]
struct HelperRegistration {
    port: u16,
    /// Diagnostic only: the device records nothing about it and decides nothing from it.
    #[allow(dead_code)]
    pid: u32,
}

async fn register_helper(State(app): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(helper) = app.helper.as_ref() else {
        return error_response(StatusCode::NOT_FOUND, "no helper", None);
    };
    if !helper.authorized(&headers) {
        return unauthorized();
    }
    let Ok(registration) = serde_json::from_slice::<HelperRegistration>(&body) else {
        return error_response(StatusCode::BAD_REQUEST, "bad registration", None);
    };
    helper.registered(registration.port, registration.pid);
    StatusCode::NO_CONTENT.into_response()
}

fn unauthorized() -> Response {
    error_response(StatusCode::UNAUTHORIZED, "unauthorized", None)
}

fn error_response(status: StatusCode, error: &'static str, detail: Option<String>) -> Response {
    (status, Json(ErrorBody { error, detail })).into_response()
}

/// The decision request, as it arrives on the wire.
///
/// The body names a tool and nothing else. Identity (`Owner`), transport (`Loopback`), the
/// destructive bit and the clock are established by the device, not by the caller (fix design F7):
/// a request that tries to carry them is not a request this device understands, and serde refuses it
/// rather than silently ignoring the spoofed fields.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorizeRequest {
    tool: String,
}

/// The wire form of a [`Decision`]; `core`'s types stay serde-free.
#[derive(Serialize)]
#[serde(tag = "decision", rename_all = "lowercase")]
enum DecisionDto {
    Allow,
    Ask { prompt: String },
    Deny { reason: DenyReasonDto },
}

#[derive(Serialize)]
struct DenyReasonDto {
    /// A stable, renderable value — the console does not match English to find out why.
    kind: &'static str,
    /// The sentence `core` wrote, for a human reading the response.
    detail: String,
}

impl From<Decision> for DecisionDto {
    fn from(decision: Decision) -> Self {
        match decision {
            Decision::Allow => DecisionDto::Allow,
            Decision::Ask { prompt } => DecisionDto::Ask { prompt },
            Decision::Deny { reason } => {
                let kind = match &reason {
                    DenyReason::Anonymous => "anonymous",
                    DenyReason::NoGrant => "no_grant",
                    DenyReason::GrantExpired { .. } => "grant_expired",
                    DenyReason::ToolNotCovered { .. } => "tool_not_covered",
                    DenyReason::DefaultDeny => "default_deny",
                };
                DecisionDto::Deny {
                    reason: DenyReasonDto {
                        kind,
                        detail: reason.to_string(),
                    },
                }
            }
        }
    }
}

/// Read the token once at startup.
///
/// A missing or unusable file is a warning, not a fatal error: health must still answer so a
/// supervisor can report that the process is up. The warning names the path and never the contents.
fn load_token(path: &Path) -> Option<[u8; 32]> {
    match std::fs::read(path) {
        Ok(raw) => match parse_token_file(&raw) {
            Some(token) => Some(token),
            None => {
                eprintln!(
                    "summrise-device: the token file {} does not hold 32 hex-encoded bytes; protected endpoints will answer 401",
                    path.display()
                );
                None
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            eprintln!(
                "summrise-device: the token file {} is missing; protected endpoints will answer 401 until it exists and the app is restarted",
                path.display()
            );
            None
        }
        Err(error) => {
            eprintln!(
                "summrise-device: the token file {} cannot be read ({error}); protected endpoints will answer 401",
                path.display()
            );
            None
        }
    }
}

fn parse_token_file(raw: &[u8]) -> Option<[u8; 32]> {
    let text = std::str::from_utf8(raw).ok()?.trim();

    decode_hex_32(text)
}

fn decode_hex_32(text: &str) -> Option<[u8; 32]> {
    let bytes = text.as_bytes();
    if bytes.len() != 64 {
        return None;
    }

    let mut decoded = [0u8; 32];
    for (decoded_byte, pair) in decoded.iter_mut().zip(bytes.as_chunks::<2>().0) {
        *decoded_byte = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }

    Some(decoded)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Compare two byte strings without an early exit.
///
/// The length comparison is public (a 32-byte token has a public length); the bytes are folded with
/// `|` so the loop runs the same number of times whatever the first difference is.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right.iter()) {
        difference |= left ^ right;
    }

    difference == 0
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }

    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use axum::body::Body;
    use axum::http::{header, Request};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use summrise_core::{Decision, DenyReason, SessionKind, SessionRow, SessionState};
    use summrise_release::Channel;

    use crate::test_support::TempDir;

    const TOKEN: [u8; 32] = [0x5a; 32];

    fn token_hex() -> String {
        TOKEN.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    struct Fixture {
        /// Kept alive so the config's paths exist for the whole test.
        _dir: TempDir,
        config: Config,
        store: Store,
    }

    fn fixture(label: &str, token_file: Option<&str>) -> Fixture {
        fixture_with_release(label, token_file, None, None, None)
    }

    fn fixture_with_release(
        label: &str,
        token_file: Option<&str>,
        release_version: Option<&str>,
        release_sha: Option<&str>,
        release_channel: Option<Channel>,
    ) -> Fixture {
        let dir = TempDir::new(label);
        let install = dir.join("install");
        let data = dir.join("data");
        fs::create_dir_all(&install).expect("the install directory is creatable");
        fs::create_dir_all(&data).expect("the data directory is creatable");

        let token_path = dir.join("token");
        if let Some(contents) = token_file {
            fs::write(&token_path, contents).expect("the token fixture is written");
        }
        // A real directory under the fixture: a capability built from this config must be able to
        // root itself here, so the test does not hide a path a later task would resolve.
        let files_root = dir.join("workspace");
        fs::create_dir_all(&files_root).expect("the files root is creatable");

        let config = Config {
            install,
            data,
            port: 0,
            token_file: token_path,
            files_root,
            release_version: release_version.map(str::to_owned),
            release_sha: release_sha.map(str::to_owned),
            release_channel,
            console_url: None,
            console_token_file: None,
            mesh_broker: None,
            mesh_relay: None,
            mesh_stun: None,
        };
        let store = Store::new(&config.install, &config.data, config.release());
        Fixture {
            _dir: dir,
            config,
            store,
        }
    }

    impl Fixture {
        fn app(&self) -> Router {
            router(&self.config, self.store.clone())
        }

        /// The same router with a helper identity, for the registration test.
        fn app_with_helper(&self, helper: Arc<crate::helper::Helper>) -> Router {
            router_with(
                &self.config,
                self.store.clone(),
                terminal::Limits::default(),
                None,
                None,
                Some(helper),
            )
        }

        fn saved_state(&self) -> summrise_core::State {
            let mut state = self.store.initial();
            state.sessions = vec![SessionRow {
                id: "session-1".to_owned(),
                kind: SessionKind::Pty,
                state: SessionState::Attached,
            }];
            self.store.save(&state).expect("the state saves");
            state
        }
    }

    fn get(uri: &str, bearer: Option<&str>) -> Request<Body> {
        let mut request = Request::builder().method("GET").uri(uri);
        if let Some(token) = bearer {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        request.body(Body::empty()).expect("the request builds")
    }

    fn post(uri: &str, bearer: Option<&str>, body: Value) -> Request<Body> {
        let mut request = Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = bearer {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        request
            .body(Body::from(body.to_string()))
            .expect("the request builds")
    }

    #[tokio::test]
    async fn the_helper_registers_only_with_this_boots_token() {
        let fixture = fixture("helper-registration", None);
        let helper = Arc::new(crate::helper::Helper::with_token("c".repeat(64)));
        let app = fixture.app_with_helper(helper.clone());
        let registration = json!({ "port": 40001, "pid": 1 });

        // No token, and a token from another boot: both are just unauthenticated callers.
        let (status, _) = send(app.clone(), post("/helper", None, registration.clone())).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = send(
            app.clone(),
            post("/helper", Some(&"d".repeat(64)), registration.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(helper.port(), None);

        // This boot's token registers the port and nothing else.
        let (status, _) = send(app, post("/helper", Some(&"c".repeat(64)), registration)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(helper.port(), Some(40001));
    }

    #[tokio::test]
    async fn without_a_helper_identity_the_registration_route_is_absent() {
        let fixture = fixture("helper-absent", None);
        let (status, _) = send(
            fixture.app(),
            post("/helper", None, json!({ "port": 40001, "pid": 1 })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    async fn send(app: Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = app.oneshot(request).await.expect("the router answers");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the body is readable");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("the body is JSON")
        };
        (status, value)
    }

    /// Send a request whose rejection body is axum's plain text, not this API's JSON error shape.
    async fn send_raw(app: Router, request: Request<Body>) -> (StatusCode, String) {
        let response = app.oneshot(request).await.expect("the router answers");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the body is readable");
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    fn authorize_body(tool: &str) -> Value {
        json!({ "tool": tool })
    }

    #[test]
    fn the_listening_address_is_loopback_only() {
        let address = loopback_addr(18_080);

        assert_eq!(address.ip(), std::net::IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(address.port(), 18_080);
        assert!(
            !address.ip().is_unspecified(),
            "a device API must never listen on 0.0.0.0"
        );
    }

    #[test]
    fn a_token_file_is_32_hex_bytes_with_a_tolerated_newline() {
        assert_eq!(parse_token_file(token_hex().as_bytes()), Some(TOKEN));
        assert_eq!(
            parse_token_file(format!("{}\r\n", token_hex()).as_bytes()),
            Some(TOKEN),
            "a trailing newline is how editors and installers leave the file"
        );
        assert_eq!(
            parse_token_file(token_hex().to_uppercase().as_bytes()),
            Some(TOKEN),
            "hex decoding is case-insensitive"
        );

        assert_eq!(parse_token_file(b""), None);
        assert_eq!(parse_token_file(b"abc"), None);
        assert_eq!(parse_token_file(&[b'g'; 64]), None);
        assert_eq!(parse_token_file(&[0xff; 64]), None, "not UTF-8");
    }

    #[test]
    fn a_decision_is_rendered_as_the_wire_document() {
        let allow = serde_json::to_value(DecisionDto::from(Decision::Allow))
            .expect("a decision serializes");
        assert_eq!(allow, json!({ "decision": "allow" }));

        let ask = serde_json::to_value(DecisionDto::from(Decision::Ask {
            prompt: "'terminal.create' is destructive and needs approval".to_owned(),
        }))
        .expect("a decision serializes");
        assert_eq!(
            ask,
            json!({
                "decision": "ask",
                "prompt": "'terminal.create' is destructive and needs approval"
            })
        );

        let deny = serde_json::to_value(DecisionDto::from(Decision::Deny {
            reason: DenyReason::NoGrant,
        }))
        .expect("a decision serializes");
        assert_eq!(deny["decision"], "deny");
        assert_eq!(deny["reason"]["kind"], "no_grant");

        let cases = [
            (DenyReason::Anonymous, "anonymous"),
            (DenyReason::DefaultDeny, "default_deny"),
            (
                DenyReason::GrantExpired {
                    expires_at_ms: 1,
                    now_ms: 2,
                },
                "grant_expired",
            ),
            (
                DenyReason::ToolNotCovered {
                    tool: "files.write".to_owned(),
                },
                "tool_not_covered",
            ),
        ];
        for (reason, kind) in cases {
            let value = serde_json::to_value(DecisionDto::from(Decision::Deny { reason }))
                .expect("a decision serializes");
            assert_eq!(value["reason"]["kind"], kind);
        }
    }

    #[test]
    fn the_comparison_does_not_stop_at_the_first_different_byte() {
        // A wrong token must fail; a correct one must pass. The timing property itself is asserted
        // by the implementation (one branch at the end), not by a clock in a test.
        assert!(constant_time_eq(&TOKEN, &TOKEN));
        assert!(!constant_time_eq(&TOKEN, &[0x00; 32]));
        assert!(!constant_time_eq(&TOKEN, &[0x5a; 31]));
        assert!(!constant_time_eq(&TOKEN, &[]));
        assert!(constant_time_eq(&[], &[]));
    }

    #[tokio::test]
    async fn the_server_binds_the_loopback_interface() {
        let fixture = fixture("bind", None);

        let listener = bind(&fixture.config).await.expect("the port is bindable");
        let address = listener
            .local_addr()
            .expect("the bound address is readable");

        assert_eq!(
            address.ip(),
            std::net::IpAddr::V4(Ipv4Addr::LOCALHOST),
            "the device API binds 127.0.0.1, never a network interface: {address}"
        );
    }

    #[tokio::test]
    async fn health_answers_without_a_token_file() {
        let fixture = fixture("health", None);

        let (status, body) = send(fixture.app(), get("/api/health", None)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(
            body["channel"], "stable",
            "a standalone start opted into nothing, so it follows stable"
        );
        assert_eq!(
            body["pid"].as_u64(),
            Some(u64::from(std::process::id())),
            "health names the process the operator would restart"
        );
    }

    #[tokio::test]
    async fn health_reports_the_release_version_not_the_crate_version() {
        let fixture = fixture_with_release(
            "health-release",
            None,
            Some("9.9.9"),
            Some("b1946ac92492d2347c6235b4d2611184"),
            None,
        );
        assert_ne!(env!("CARGO_PKG_VERSION"), "9.9.9");

        let (status, body) = send(fixture.app(), get("/api/health", None)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["version"], "9.9.9",
            "health names the release the launcher started, not this build's compile time: {body}"
        );
    }

    #[tokio::test]
    async fn health_reports_the_channel_the_sidecar_named() {
        let fixture = fixture_with_release(
            "health-channel",
            None,
            Some("9.9.9"),
            Some("b1946ac92492d2347c6235b4d2611184"),
            Some(Channel::Canary),
        );

        let (status, body) = send(fixture.app(), get("/api/health", None)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["channel"], "canary",
            "a canary build must not be labelled stable by health either: {body}"
        );
    }

    #[tokio::test]
    async fn state_follows_the_release_flags() {
        let fixture = fixture_with_release(
            "state-release",
            Some(&token_hex()),
            Some("9.9.9"),
            Some("b1946ac92492d2347c6235b4d2611184"),
            Some(Channel::Canary),
        );
        crate::ensure_state(&fixture.store, true).expect("startup writes the release facts");

        let (status, body) = send(fixture.app(), get("/api/state", Some(&token_hex()))).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["install"]["version"], "9.9.9");
        assert_eq!(
            body["install"]["channel"], "canary",
            "the state document records the channel the launcher booted: {body}"
        );
        assert_eq!(body["baseline"]["version"], "9.9.9");
        assert_eq!(
            body["baseline"]["sha256"],
            "b1946ac92492d2347c6235b4d2611184"
        );
        assert_eq!(body["lastVerdict"]["detail"], "started 9.9.9");
    }

    #[tokio::test]
    async fn state_without_a_token_is_401() {
        let fixture = fixture("state-no-token", None);

        let (status, body) = send(fixture.app(), get("/api/state", None)).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, json!({ "error": "unauthorized" }));
    }

    #[tokio::test]
    async fn state_with_a_wrong_token_is_401() {
        let fixture = fixture("state-wrong-token", Some(&token_hex()));
        let wrong = "00".repeat(32);

        let (status, body) = send(fixture.app(), get("/api/state", Some(&wrong))).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, json!({ "error": "unauthorized" }));
    }

    #[tokio::test]
    async fn state_with_a_malformed_bearer_token_is_401() {
        let fixture = fixture("state-malformed-token", Some(&token_hex()));

        for presented in ["not-hex", &"5a".repeat(31), ""] {
            let (status, _) = send(fixture.app(), get("/api/state", Some(presented))).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "'{presented}'");
        }
    }

    #[tokio::test]
    async fn state_with_the_install_token_returns_the_saved_document() {
        let fixture = fixture("state-ok", Some(&format!("{}\n", token_hex())));
        let state = fixture.saved_state();

        let (status, body) = send(fixture.app(), get("/api/state", Some(&token_hex()))).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::to_value(&state).expect("a state is JSON"));
    }

    #[tokio::test]
    async fn state_with_no_file_yet_returns_the_initial_document() {
        let fixture = fixture("state-absent", Some(&token_hex()));

        let (status, body) = send(fixture.app(), get("/api/state", Some(&token_hex()))).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["schema"], 1);
        assert_eq!(body["install"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(body["sessions"], json!([]));
        assert_eq!(body["update"]["phase"], "idle");
    }

    #[tokio::test]
    async fn a_corrupt_state_file_is_a_500_not_a_fresh_document() {
        let fixture = fixture("state-corrupt", Some(&token_hex()));
        fs::write(fixture.store.state_path(), "{ not a document").expect("the fixture is written");

        let (status, body) = send(fixture.app(), get("/api/state", Some(&token_hex()))).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"], "state_unavailable");
    }

    /// C2: health is a readiness answer. A device that cannot read its state document cannot record
    /// a session, so it must not answer `ok` to the launcher's probes or to its own watchdog while
    /// `/api/state` reports the same failure.
    #[tokio::test]
    async fn a_corrupt_state_file_at_runtime_makes_health_degraded_and_names_the_state_error() {
        let fixture = fixture("health-degraded", Some(&token_hex()));
        // A clean start first: at startup a torn document is quarantined and rebuilt (see
        // `a_corrupt_state_file_is_quarantined_and_rebuilt`). This tests the other half — the
        // document is corrupted while the device runs, a torn write or a disk that lied — and the
        // API must say so instead of inventing a document.
        crate::ensure_state(&fixture.store, true).expect("a clean start writes the document");
        fs::write(fixture.store.state_path(), "{ not a document")
            .expect("the corrupt fixture is written");

        let (status, body) = send(fixture.app(), get("/api/health", None)).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "degraded");
        let reason = body["reason"]
            .as_str()
            .unwrap_or_else(|| panic!("degraded health names the failure: {body}"));
        assert!(
            reason.starts_with("state: "),
            "the reason is the state's error: {reason}"
        );
        assert!(
            reason.contains("state file is corrupt"),
            "the reason names the document, not a generic refusal: {reason}"
        );

        let (status, body) = send(fixture.app(), get("/api/state", Some(&token_hex()))).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"], "state_unavailable");
    }

    /// The answer follows the file: once the operator replaces the corrupt document, health is
    /// `ok` again without a restart. A pinned poison flag would leave the device degraded forever.
    #[tokio::test]
    async fn health_recovers_when_the_state_file_is_replaced_with_a_valid_one() {
        let fixture = fixture("health-recovered", Some(&token_hex()));
        fs::write(fixture.store.state_path(), "{ not a document")
            .expect("the corrupt fixture is written");

        let (status, _) = send(fixture.app(), get("/api/health", None)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

        let state = fixture.store.initial();
        fixture
            .store
            .save(&state)
            .expect("a valid document replaces the corrupt one");

        let (status, body) = send(fixture.app(), get("/api/health", None)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn authorize_requires_the_install_token() {
        let fixture = fixture("authorize-no-token", Some(&token_hex()));
        let body = authorize_body("terminal.create");

        let (status, response) = send(fixture.app(), post("/api/authorize", None, body)).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(response, json!({ "error": "unauthorized" }));
    }

    #[tokio::test]
    async fn the_mcp_token_gate_runs_before_the_body_limit() {
        let fixture = fixture("mcp-token-before-limit", Some(&token_hex()));
        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(header::CONTENT_TYPE, "application/json")
            .header(
                header::CONTENT_LENGTH,
                (crate::mcp::MAX_BODY_BYTES + 1).to_string(),
            )
            .body(Body::empty())
            .expect("the request builds");

        let response = fixture
            .app()
            .oneshot(request)
            .await
            .expect("the router answers");

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "an unauthenticated oversized body is refused by the token gate, not collected first"
        );
    }

    #[tokio::test]
    async fn authorize_allows_a_known_tool_as_the_owner() {
        let fixture = fixture("authorize-owner", Some(&token_hex()));

        // The registry is the table the device serves, so iterating it is what keeps this test from
        // sampling: a tool added to `CAPABILITIES` is covered the day it is added, and a tool the
        // registry declares but authorize refuses fails here.
        for tool in summrise_core::registry::CAPABILITIES
            .iter()
            .flat_map(|capability| capability.tools)
            .map(|definition| definition.name)
        {
            let (status, response) = send(
                fixture.app(),
                post("/api/authorize", Some(&token_hex()), authorize_body(tool)),
            )
            .await;

            assert_eq!(status, StatusCode::OK, "{tool}");
            assert_eq!(
                response,
                json!({ "decision": "allow" }),
                "the install token is the owner, and rule 1 precedes the destructive question: {tool}"
            );
        }
    }

    #[tokio::test]
    async fn authorize_denies_an_unknown_tool() {
        let fixture = fixture("authorize-unknown", Some(&token_hex()));

        for tool in [
            "",
            "terminal",
            // The pre-release names are not aliases: the tools are terminal.create and friends now.
            "terminal.pty",
            "terminal.pty.create",
            "terminalx.pty",
            "browser.delete",
            "files.delete",
            "shell",
        ] {
            let (status, response) = send(
                fixture.app(),
                post("/api/authorize", Some(&token_hex()), authorize_body(tool)),
            )
            .await;

            assert_eq!(status, StatusCode::OK, "{tool:?}");
            assert_eq!(response["decision"], "deny", "{tool:?}");
            assert_eq!(
                response["reason"]["kind"], "default_deny",
                "{} is not in the device's table: {response}",
                tool
            );
        }
    }

    #[tokio::test]
    async fn authorize_rejects_a_spoofed_owner_or_destructive_or_clock_field() {
        let fixture = fixture("authorize-spoofed", Some(&token_hex()));
        let spoofed = [
            json!({ "tool": "terminal.create", "subject": { "kind": "owner" } }),
            json!({ "tool": "terminal.create", "transport": "relay" }),
            json!({ "tool": "terminal.create", "destructive": false }),
            json!({ "tool": "terminal.create", "approval": "granted" }),
            json!({ "tool": "terminal.create", "now_ms": 1 }),
        ];

        for body in spoofed {
            let (status, text) = send_raw(
                fixture.app(),
                post("/api/authorize", Some(&token_hex()), body.clone()),
            )
            .await;

            assert_eq!(
                status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "the body {body} carries a fact the device establishes, not the caller: {text}"
            );
            assert!(
                text.contains("unknown field"),
                "the deserializer is what rejects {body}: {text}"
            );
        }
    }
}
