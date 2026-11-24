//! The loopback HTTP surface: health, state, and the authorization decision.
//!
//! Health is deliberately unauthenticated and answers even when the token file is missing: a
//! supervisor and a human need one endpoint that says the process is up before anything else can be
//! trusted. Everything that reads device state or asks for a decision requires the install token,
//! compared over bytes in constant time and never logged.

use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{ConnectInfo, Form, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use summrise_core::{
    authorize, ApprovalState, Ctx, Decision, DenyReason, Holder, SessionRef, SessionState, Subject,
    ToolCall, Transport, TxnId, TxnState,
};

#[cfg(test)]
use summrise_core::Goal;

use crate::activity;
use crate::capabilities::{self, browser, terminal};
use crate::config::{Config, Release};
use crate::helper;
use crate::panel;
use crate::plugins::host::PluginHost;
use crate::restart;
use crate::store::{Store, EVENT_TAIL};
use crate::watchdog;

/// Build the API's router. Test-only: production always has a console and a mesh slot to fill.
#[cfg(test)]
pub(crate) fn router(config: &Config, store: Store) -> Router {
    router_with(
        config,
        store,
        terminal::Limits::default(),
        None,
        None,
        None,
        restart::none(),
        Arc::new(PluginHost::load(&config.data)),
        None,
        None,
        "test",
    )
    .0
}

/// A router whose terminal capability uses `limits`. Test-only: an in-process test shortens the
/// write bound instead of waiting the production five seconds.
#[cfg(test)]
pub(crate) fn router_with_limits(
    config: &Config,
    store: Store,
    limits: terminal::Limits,
) -> Router {
    router_with(
        config,
        store,
        limits,
        None,
        None,
        None,
        restart::none(),
        Arc::new(PluginHost::load(&config.data)),
        None,
        None,
        "test",
    )
    .0
}

// The router's dependencies are listed once, here, in the order the features landed; a struct would
// hide the fact that every new capability is one more argument the process owns.
//
// The browser registry is returned beside the router because the run loop, not a request, owns its
// last act: `serve` closes the browser's pages and kills it after the listener stops. Keeping it
// inside the router would make the process exit the only shutdown path, which is the leak this
// return exists to prevent.
#[allow(clippy::too_many_arguments)]
fn router_with(
    config: &Config,
    store: Store,
    limits: terminal::Limits,
    console: Option<crate::console::ConsoleEndpoint>,
    mesh: Option<crate::mesh::MeshLink>,
    helper: Option<Arc<helper::Helper>>,
    restart: restart::Hook,
    plugins: Arc<PluginHost>,
    update_check: Option<Arc<crate::update_check::UpdateChecker>>,
    bus: Option<Arc<summrise_bus::Bus>>,
    txn_boot: &str,
) -> (
    Router,
    Arc<browser::BrowserRegistry>,
    Arc<crate::txns::TxnRegistry>,
) {
    // The MCP service and the panel share one terminal registry, so a session an MCP client opened
    // is the session the panel lists. `/mcp` is the same secret as `/api/state`: the gate runs before
    // the MCP service sees the request, so an unauthenticated caller cannot even start a handshake.
    // One audit for the process: the MCP service writes it, and the panel shares it so a session
    // opened from the device's own page is in the same file as a call an AI client made.
    let audit = std::sync::Arc::new(crate::mcp::Audit::new(&config.data));
    let (mcp_service, terminal, browser, txns) = crate::mcp::service_with_terminal(
        config,
        limits,
        audit.clone(),
        console,
        mesh,
        restart.clone(),
        plugins,
        update_check,
        bus.clone(),
        txn_boot,
    );
    #[cfg(all(test, unix))]
    PANEL_TEST_TERMINAL.with(|slot| *slot.borrow_mut() = Some(Arc::downgrade(&terminal)));
    if let Some(bus) = &bus {
        let _ = bus.publish("device.started", serde_json::json!({ "port": config.port }));
    }
    let state = AppState {
        token: load_token(&config.token_file),
        store,
        release: config.release(),
        helper,
        data: config.data.clone(),
        port: config.port,
        panel_token: mint_panel_token(),
        // This process's id, fixed for the life of the router: a transcript cursor names it so a
        // cursor from another boot cannot address rows in this boot's numbering.
        boot: now_ms().max(0) as u64,
        audit,
        supervised: config.supervised,
        restart,
        terminal,
        txns: Arc::clone(&txns),
        bus,
    };

    // `/mcp` is the same secret as `/api/state`: the gate runs before the MCP service sees the
    // request, so an unauthenticated caller cannot even start a handshake. The body limit is inside
    // the token gate (layers wrap in reverse order of their addition), so an unauthorized caller is
    // refused before its body is collected.
    // The app's restart is one hook: the panel's enrollment and the MCP `host.restart` tool both
    // ask the same closure, so the two paths cannot drift.
    let mcp = Router::new()
        .route_service("/mcp", mcp_service)
        .route_layer(middleware::from_fn(crate::mcp::limit_request_body))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_install_token,
        ));

    let app = Router::new()
        // The device's own panel: the surface a person standing at this machine uses, and the only
        // place this boot's panel token is served. Both the page and its snapshot refuse a
        // non-loopback peer, so a reverse proxy in front of 127.0.0.1 cannot turn either into a
        // remote read.
        .route("/", get(local_panel))
        // The same panel, rendered in Rust, one path ahead of the switch: `/` still serves
        // `panel.html` until a later step flips it.
        .route("/panel", get(panel_page))
        // The operations transcript, the old Electron page moved onto the device: the first paint is
        // server-rendered from the panel's bounded audit tail, and `/api/activity` streams the rows
        // after a cursor for the page's one-second poll. Both are loopback-only; the page carries the
        // panel token in its meta tag and the streaming route requires that token in a header.
        .route("/activity", get(activity_page))
        .route("/api/activity", get(activity_api))
        // The rendered page has no script for enrollment, so the form posts here. The redirect
        // carries the outcome; the page renders it as a banner on the next GET.
        .route("/panel/enroll", post(enroll_from_panel_form))
        // The panel's manual update check: a form POST, loopback plus this boot's panel token. It
        // writes a request file the scheduler consumes; it does not contact the mirror itself.
        .route("/api/panel/update", post(panel_update))
        .route("/api/panel", get(panel_snapshot))
        // One run's facts for the transcript's expand. The journal keeps a capped summary and the
        // runs history keeps no output bodies; this route answers the full command line and the
        // exit facts, and says the byte counts rather than inventing output it does not have.
        .route("/api/runs/:id", get(panel_run))
        // The panel's read-only view of the live terminal sessions. Loopback-only like the page: it
        // names the shell/serial ports a person standing at this machine can already reach.
        .route("/api/terminal", get(panel_terminal))
        // The panel's terminal operations: the same PtyRegistry the MCP's terminal.* tools use, so
        // an operator at the machine can create, type into, read and close a session without a
        // console. Loopback plus the panel token, exactly like the enrollment.
        .route("/api/terminal/create", post(panel_terminal_create))
        .route("/api/terminal/:id/write", post(panel_terminal_write))
        .route("/api/terminal/:id/read", get(panel_terminal_read))
        .route("/api/terminal/:id/close", post(panel_terminal_close))
        .route("/api/terminal/:id/resize", post(panel_terminal_resize))
        // The terminal emulator, embedded: the panel's terminal is the same xterm the console uses.
        .route(
            "/panel/xterm.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    PANEL_XTERM_JS,
                )
            }),
        )
        .route(
            "/panel/addon-fit.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    PANEL_XTERM_FIT,
                )
            }),
        )
        .route(
            "/panel/xterm.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    PANEL_XTERM_CSS,
                )
            }),
        )
        // The panel's own terminal glue: create/read/write/resize/close, and nothing else.
        .route(
            "/panel/panel-terminal.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    PANEL_TERMINAL_JS,
                )
            }),
        )
        // The panel's enrollment: the page a person is looking at posts the code here. The handler
        // demands a loopback peer and this boot's panel token, and the token exists only in that
        // page, so a request from another origin cannot be made to look like the user's.
        .route("/api/enroll", post(enroll_from_panel))
        .route("/api/health", get(health))
        .route("/api/status", get(status))
        .route("/api/state", get(current_state))
        .route("/api/authorize", post(authorize_request))
        // The per-user helper's registration: a loopback call with the token this boot wrote to
        // `ProgramData`. It is the only route the helper may call, and it only says where to find it.
        .route("/helper", post(register_helper))
        .merge(mcp)
        .with_state(state);

    (app, browser, txns)
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
#[allow(clippy::too_many_arguments)] // one argument per capability the process owns, like `router_with`
pub async fn serve(
    config: Config,
    store: Store,
    console: Option<crate::console::ConsoleEndpoint>,
    mesh: Option<crate::mesh::MeshLink>,
    helper: Option<Arc<helper::Helper>>,
    plugins: Arc<PluginHost>,
    update_check: Option<Arc<crate::update_check::UpdateChecker>>,
    bus: Option<Arc<summrise_bus::Bus>>,
    txn_boot: String,
) -> io::Result<()> {
    // A supervised app restarts through the launcher after the answer reaches the page or the MCP
    // client; a standalone process has no supervisor to bring it back, so its handlers never call
    // this hook. One closure serves both the enrollment and `host.restart`.
    let restart = restart::exit_soon();
    let (app, browser, _txns) = router_with(
        &config,
        store,
        terminal::Limits::default(),
        console,
        mesh,
        helper,
        restart,
        plugins,
        update_check,
        bus,
        txn_boot.as_str(),
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
    let served = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    // The interactive stop: Ctrl+C lets the run loop call `PluginHost::stop_all` before the process
    // ends. A SCM stop or a kill is outside any process's destructor; the ownership type covers the
    // browser on that path, and the launcher owns the rest.
    .with_graceful_shutdown(shutdown_signal())
    .await;

    // The browser belongs to the app: close its pages (a page releases its own Web Serial handle on
    // teardown) and then kill its tree, before `main` returns and the process exits. On the abrupt
    // paths this call never runs, and the process-group / job-object ownership is what still ends
    // the browser.
    browser.shutdown().await;

    served
}

/// Wait for an orderly stop.
///
/// On Unix, SIGINT and SIGTERM let the run loop stop the plugin children before the process ends. On
/// Windows a service stop is not a console event this process can observe (the launcher owns that
/// lifecycle), and `ctrl_c` can fail on a process with no console: a failure must not mean "stop
/// now", so the future then never completes and the device keeps serving until it is ended.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(terminate) => terminate,
            Err(_) => {
                if tokio::signal::ctrl_c().await.is_err() {
                    std::future::pending::<()>().await;
                }
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
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
    /// The data directory enrollment writes `console.json` and `console-token.json` under.
    data: PathBuf,
    /// The port the listener binds. The rendered panel prints the MCP endpoint from it; the page
    /// cannot ask the browser its origin because the page is built before a browser sees it.
    port: u16,
    /// This boot's panel token: 64 lowercase hex, minted once when the router is built. It is served
    /// only in the panel's HTML (a meta tag) and never in `/api/panel`, an error body, or a log line.
    panel_token: String,
    /// This boot's id, minted with the router. `/activity` cursors carry it, so a page left open
    /// across a device restart cannot mistake the new process's row numbering for the old one's.
    boot: u64,
    /// The process's one audit, shared with the MCP service: panel operations are recorded there.
    audit: std::sync::Arc<crate::mcp::Audit>,
    /// Whether the launcher started this process with a verified digest. Only then may a panel
    /// enrollment or `host.restart` restart it: a standalone app that exits would not come back.
    supervised: bool,
    /// What to do after a supervised enrollment wrote the console link, and after `host.restart`
    /// was allowed (the real hook exits after a short delay so the answer reaches the caller first;
    /// tests record the call).
    restart: restart::Hook,
    /// The live terminal sessions, shared with the MCP service: the panel lists the same sessions an
    /// MCP client created, because they are the same machine's terminals.
    terminal: Arc<terminal::PtyRegistry>,
    /// The process's transactions, shared with the MCP service: a panel operation is grouped under
    /// the same record a client's call would be.
    txns: Arc<crate::txns::TxnRegistry>,
    /// The process's event journal, when one opened. A panel operation is a call like any other, so
    /// its pair of `call.*` events goes here too: without it the person standing at the machine
    /// would be the one actor whose work the journal cannot show.
    bus: Option<Arc<summrise_bus::Bus>>,
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

    /// Whether the request carries this boot's panel token, compared in constant time over its
    /// decoded bytes. The token is served only in the loopback HTML, and a cross-site page cannot
    /// read it or set this header without a CORS preflight the device never grants.
    fn panel_authorized(&self, headers: &HeaderMap) -> bool {
        let Some(presented) = headers
            .get(PANEL_TOKEN_HEADER)
            .and_then(|value| value.to_str().ok())
        else {
            return false;
        };

        self.panel_token_matches(presented)
    }

    /// The same comparison for a token that travelled in the enrollment form instead of a header:
    /// an HTML form cannot set a custom header, and a cross-site page cannot read the hidden field.
    fn panel_token_matches(&self, presented: &str) -> bool {
        let Some(presented) = decode_hex_32(presented.trim()) else {
            return false;
        };
        let expected =
            decode_hex_32(&self.panel_token).expect("the panel token is minted as 64 hex");

        constant_time_eq(&presented, &expected)
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

/// The document the tray was written against.
///
/// The desktop polls this path every 30 s (`device/desktop-electron/src/main.ts:878`) and hands the
/// raw body to the shell policy's `refreshTrayHealth`, so the shape here is that policy's contract,
/// not this file's taste: `release`, `uptime_secs` and `live_sessions` are read by its committed
/// wasm, and the four fields `Health` already publishes are kept so one document describes the
/// device. `cpu_pct`/`mem_pct` are deliberately absent: the legacy agent sampled them, this process
/// has no sampler, and a number invented here would be worse than the field the policy keeps.
///
/// Readiness, like `/api/health`: a device that cannot read its state document cannot count its
/// sessions or date its start, and a tray must not print a version from a refusal.
async fn status(State(app): State<AppState>) -> Response {
    match app.store.load() {
        Ok(state) => Json(Status {
            status: "ok",
            version: app.release.version.clone(),
            channel: app.release.channel.clone(),
            pid: std::process::id(),
            release: app.release.version.clone(),
            uptime_secs: uptime_secs(&state.service.started_at),
            live_sessions: state
                .sessions
                .iter()
                .filter(|row| !matches!(row.state, SessionState::Reaped | SessionState::Closed))
                .count(),
        })
        .into_response(),
        Err(error) => {
            eprintln!("summrise-device: /api/status is degraded: {error}");
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

/// Seconds since `started_at` (the RFC3339 stamp `refresh_start` writes), or `None` when the
/// document's stamp is not a time. `None` keeps the tray's last known uptime; a zero would read as
/// "it just restarted", which is a different claim.
fn uptime_secs(started_at: &str) -> Option<u64> {
    let started = chrono::DateTime::parse_from_rfc3339(started_at)
        .ok()?
        .timestamp_millis();
    let now = chrono::Utc::now().timestamp_millis();
    (now >= started).then(|| ((now - started) / 1000) as u64)
}

/// `GET /`: the device-local panel, with this boot's panel token in its meta tag.
///
/// Loopback-only by peer address, like `/api/panel`. The page now carries a credential, so a request
/// that reached the listener through something else gets no page rather than the token.
async fn local_panel(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    if !peer.ip().is_loopback() {
        return error_response(StatusCode::FORBIDDEN, "loopback_only", None);
    }

    // The same page `/panel` renders, from the same facts: the HTML file is gone, and there is one
    // place a panel section is written.
    Html(panel::page(&panel_facts(&app)).into_string()).into_response()
}

/// The header the panel's script sends the token in. A cross-site page cannot set a custom header
/// without a CORS preflight, and this API never grants one.
const PANEL_TOKEN_HEADER: &str = "x-summrise-panel";

/// The rendered panel's query: the enrollment form's redirect says whether it worked.
#[derive(Deserialize)]
struct PanelQuery {
    enrolled: Option<String>,
    error: Option<String>,
}

/// `GET /panel`: the panel rendered from [`panel::PanelFacts`] instead of `panel.html`.
///
/// The gate is the same one `GET /` uses — a loopback peer, refused with `loopback_only` — because
/// the page carries this boot's token. A failed enrollment comes back here as `?error=<code>` and a
/// successful one as `?enrolled=1`, so the page is always one server-rendered read.
async fn panel_page(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(query): Query<PanelQuery>,
) -> Response {
    if !peer.ip().is_loopback() {
        return error_response(StatusCode::FORBIDDEN, "loopback_only", None);
    }

    let facts = panel_facts(&app);
    let markup = if let Some(error) = query.error {
        panel::enroll_result(&facts, panel::EnrollOutcome::Failed(error))
    } else if query.enrolled.as_deref() == Some("1") {
        panel::enroll_result(&facts, panel::EnrollOutcome::Enrolled)
    } else {
        panel::page(&facts)
    };

    Html(markup.into_string()).into_response()
}

/// The transcript's query string: the stream's cursor and page size, the transaction it expands,
/// and the session the document is about.
#[derive(Deserialize)]
struct ActivityQuery {
    /// The cursor a poll asks after, as [`activity::cursor_string`] spells it.
    after: Option<String>,
    /// The new-begin page size, clamped to `1..=200`. Late-end updates are outside it.
    limit: Option<u64>,
    /// The transaction the ledger expands. An id this window does not hold expands nothing and is
    /// named as missing; it never falls back to another transaction.
    txn: Option<String>,
    /// The session the page is the document of. [`activity::selected_session`] accepts it only when
    /// the record knows it; an id the record does not know names nothing and is said as missing.
    session: Option<String>,
}

/// `GET /activity`: the transcript's first paint.
///
/// Loopback-only by peer address, like the panel: the page carries this boot's panel token, so a
/// request that arrived through something else gets no page and no token. The token is in the page's
/// meta tag only; the view reads it from there.
async fn activity_page(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(query): Query<ActivityQuery>,
) -> Response {
    if !peer.ip().is_loopback() {
        return error_response(StatusCode::FORBIDDEN, "loopback_only", None);
    }

    let facts = activity_facts(&app, query.txn.as_deref(), query.session.as_deref());
    Html(activity::page(&facts).into_string()).into_response()
}

/// `GET /api/activity?after=<boot>.<seq>&limit=<n>`: the transcript's rows for the page's poll.
///
/// Loopback and this boot's panel token, like the other panel reads. Without a cursor the answer is
/// the newest `limit` rows, oldest first, with `more` saying older rows exist. With a cursor,
/// `next` is the greatest begin-`seq` among the new rows (the caller's cursor when none arrived),
/// `limit` bounds the new begins only, and every row whose `end` landed after the cursor is
/// included outside that cap as an update for a running call. A cursor naming another boot is
/// treated as no cursor, so a page open across a restart starts from the newest window instead of
/// answering nothing. `counts` is always the whole window's counts, so the header does not go
/// stale.
async fn activity_api(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<ActivityQuery>,
) -> Response {
    if !peer.ip().is_loopback() {
        return error_response(StatusCode::FORBIDDEN, "loopback_only", None);
    }
    if !app.panel_authorized(&headers) {
        return error_response(StatusCode::FORBIDDEN, "unauthorized", None);
    }

    let facts = activity_facts(&app, None, None);
    let rows = facts
        .audit
        .as_deref()
        .map(|audit| activity::rows(audit, facts.now_ms))
        .unwrap_or_default();
    let after = query.after.as_deref().and_then(|text| {
        activity::parse_cursor(text)
            .filter(|(boot, _)| *boot == facts.boot)
            .map(|(_, seq)| seq)
    });
    let limit = query
        .limit
        .unwrap_or(activity::DEFAULT_LIMIT as u64)
        .clamp(1, activity::MAX_ROWS as u64) as usize;

    Json(activity::page_of(
        &rows,
        after,
        limit,
        facts.boot,
        facts.truncated,
    ))
    .into_response()
}

/// The transcript's facts: the panel's bounded audit tail, read once per request, plus this boot's
/// token and id and the instant a running call's duration is measured to.
fn activity_facts(
    app: &AppState,
    selected_txn: Option<&str>,
    selected_session: Option<&str>,
) -> activity::ActivityFacts {
    let (audit, truncated) =
        match read_audit_tail(&app.data.join("events.jsonl"), panel::TIMELINE_LINES) {
            Ok(tail) => (Some(tail.lines), tail.truncated),
            // The transcript says the audit could not be read; that is not the empty record.
            Err(_) => (None, false),
        };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0);

    activity::ActivityFacts {
        txns: txn_facts(app, audit.as_deref(), now_ms),
        sessions: session_facts(app, audit.as_deref()),
        selected_txn: selected_txn.map(str::to_owned),
        selected_session: selected_session.map(str::to_owned),
        host: crate::console::host_name().unwrap_or_else(|| "this machine".to_owned()),
        version: app.release.version.clone(),
        channel: app.release.channel.clone(),
        audit,
        truncated,
        now_ms,
        boot: app.boot,
        panel_token: app.panel_token.clone(),
    }
}

/// The live sessions as the pages show them: the registry's holder and goal, the working
/// transaction, and the newest call the bounded record carries for that session.
fn session_facts(app: &AppState, audit: Option<&[serde_json::Value]>) -> Vec<panel::SessionFacts> {
    app.terminal
        .list()
        .into_iter()
        .map(|session| {
            let id = session.id.as_str();
            let open = app
                .txns
                .working_on(&SessionRef::new(id))
                .and_then(|id| app.txns.get(&id));
            let last = audit.and_then(|lines| {
                lines.iter().rev().find_map(|line| {
                    let tool = line.get("tool").and_then(serde_json::Value::as_str)?;
                    if row_session(app, line).as_deref() != Some(id) {
                        return None;
                    }
                    let at_ms = line
                        .get("ts_ms")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0);
                    Some((tool.to_owned(), at_ms))
                })
            });
            panel::SessionFacts {
                kind: session.kind.as_str().to_owned(),
                shell: session.shell,
                id: id.to_owned(),
                holder: panel::holder_label(&session.holder),
                goal: session.goal.as_ref().map(|goal| goal.text().to_owned()),
                goal_by: session
                    .goal
                    .as_ref()
                    .map(|goal| panel::holder_label(goal.by())),
                open_txn: open.as_ref().map(|txn| txn.id().as_str().to_owned()),
                open_purpose: open.as_ref().map(|txn| txn.purpose().to_owned()),
                last_call: last.as_ref().map(|(tool, _)| tool.clone()),
                last_call_at_ms: last.as_ref().map(|(_, at_ms)| *at_ms),
                alive: session.alive,
            }
        })
        .collect()
}

/// The session an audit row worked on, from the row's own fields.
///
/// The retained transaction supplies a session for machine work as well as terminal work. Only
/// terminal tools may fall back to their flattened `session=<id>` argument (including exempt goal
/// operations); another tool's `id` or session-looking command text is never a session reference.
fn row_session(app: &AppState, line: &serde_json::Value) -> Option<String> {
    let tool = line.get("tool").and_then(serde_json::Value::as_str)?;
    // Declarations describe the work; they must not replace its last actual call (the panel
    // commits its implicit transaction immediately after each operation).
    if tool.starts_with("txn.") {
        return None;
    }
    if let Some(session) = line
        .get("txn")
        .and_then(serde_json::Value::as_str)
        .filter(|txn| !txn.is_empty())
        .and_then(|txn| app.txns.get(&TxnId::new(txn)))
        .and_then(|txn| txn.session().map(|session| session.as_str().to_owned()))
    {
        return Some(session);
    }
    if !tool.starts_with("terminal.") {
        return None;
    }
    line.get("args")
        .and_then(serde_json::Value::as_str)
        .and_then(|args| {
            args.split_whitespace()
                .find_map(|part| part.strip_prefix("session="))
                .filter(|session| !session.is_empty())
        })
        .map(str::to_owned)
}

/// The door a row names, as a person reads it: the author of a claim the tail still carries.
fn row_author(line: &serde_json::Value) -> String {
    let transport = line
        .get("transport")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let token = line.get("token_id").and_then(serde_json::Value::as_str);
    match transport {
        "loopback" => "the install door".to_owned(),
        "panel" => "the panel".to_owned(),
        "console" => match token {
            Some("operator") => "the console operator".to_owned(),
            Some(id) => format!("token {id}"),
            None => "the console".to_owned(),
        },
        "mesh" => "a mesh peer".to_owned(),
        _ => "not recorded".to_owned(),
    }
}

/// Every transaction the audit names, with the receipt this process still holds.
///
/// A transaction's group is its row's `txn` field. When the registry has the record, its counters
/// are the device's totals and `retained` is true. When it does not (a restart, or eviction past the
/// retention window), the group survives from the audit with `retained: false`: its counters and its
/// declared ending lived in the receipt and are not invented, but the claim the transaction's own
/// `txn.begin` row still carries in this tail — the purpose and the door — is reported. When that
/// begin row is outside the bounded tail, the purpose falls back to "(purpose not retained)".
fn txn_facts(
    app: &AppState,
    audit: Option<&[serde_json::Value]>,
    now_ms: u64,
) -> Vec<panel::TxnFact> {
    use std::collections::HashMap;

    let lines = audit.unwrap_or(&[]);
    let mut ids: Vec<String> = Vec::new();
    // The declaration's own row, by the call id its ending shares: the purpose and the author of a
    // registry-less transaction are still in the tail when that row is.
    let mut claim_by_call: HashMap<u64, (String, String)> = HashMap::new();
    for line in lines {
        if let Some(txn) = line
            .get("txn")
            .and_then(serde_json::Value::as_str)
            .filter(|txn| !txn.is_empty())
        {
            if !ids.iter().any(|seen| seen == txn) {
                ids.push(txn.to_owned());
            }
        }
        let is_begin = line.get("event").and_then(serde_json::Value::as_str) == Some("begin");
        let is_txn_begin =
            line.get("tool").and_then(serde_json::Value::as_str) == Some("txn.begin");
        if is_begin && is_txn_begin {
            if let (Some(call), Some(purpose)) = (
                line.get("id").and_then(serde_json::Value::as_u64),
                line.get("args")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|args| args.strip_prefix("purpose="))
                    .filter(|purpose| !purpose.is_empty()),
            ) {
                claim_by_call.insert(call, (purpose.to_owned(), row_author(line)));
            }
        }
    }

    let mut facts: Vec<panel::TxnFact> = Vec::with_capacity(ids.len());
    for id in &ids {
        if let Some(txn) = app.txns.get(&TxnId::new(id)) {
            let started = txn.started_at_ms().max(0) as u64;
            let ended = txn.ended_at_ms().map(|ended| ended.max(0) as u64);
            let duration = ended
                .map(|ended| ended.saturating_sub(started))
                .unwrap_or_else(|| now_ms.saturating_sub(started));
            facts.push(panel::TxnFact {
                id: id.clone(),
                purpose: txn.purpose().to_owned(),
                author: panel::holder_label(txn.by()),
                session: txn.session().map(|session| session.as_str().to_owned()),
                state: crate::txns::state_name(txn.state()).to_owned(),
                reason: txn.reason().map(str::to_owned),
                started_at_ms: started,
                ended_at_ms: ended,
                calls: txn.calls(),
                failures: txn.failures(),
                refusals: txn.refusals(),
                duration_ms: duration,
                retained: true,
            });
            continue;
        }

        // The audit keeps the work and the claim its declaration row wrote; the receipt, its counts
        // and its ending are this process's, and they died with it. A declaration whose begin row is
        // outside the bounded tail leaves the claim unretained too.
        let mut started: Option<u64> = None;
        let mut claim: Option<(String, String)> = None;
        for line in lines
            .iter()
            .filter(|line| line.get("txn").and_then(serde_json::Value::as_str) == Some(id.as_str()))
        {
            if let Some(at) = line.get("ts_ms").and_then(serde_json::Value::as_u64) {
                started = Some(started.map_or(at, |seen| seen.min(at)));
            }
            if claim.is_none() {
                claim = line
                    .get("id")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|call| claim_by_call.get(&call))
                    .cloned();
            }
        }
        let Some(started) = started else {
            continue;
        };
        let (purpose, author) = claim.unwrap_or_default();
        facts.push(panel::TxnFact {
            id: id.clone(),
            purpose,
            author,
            session: None,
            state: "unknown".to_owned(),
            reason: None,
            started_at_ms: started,
            ended_at_ms: None,
            calls: 0,
            failures: 0,
            refusals: 0,
            duration_ms: 0,
            retained: false,
        });
    }

    // Stable newest-first, so the ledger and the timeline cannot disagree about order.
    facts.sort_by_key(|fact| std::cmp::Reverse(fact.started_at_ms));
    facts
}

/// The newest `limit` lines of the audit file, and whether older lines exist beyond them.
struct AuditTail {
    lines: Vec<serde_json::Value>,
    truncated: bool,
}

/// Read the audit from the end: the panel shows 200 lines, so a forward read of a 64 MiB file would
/// parse hundreds of thousands of lines and then throw them away.
///
/// A byte window starts at the end and grows until it holds more than `limit` complete nonblank
/// lines (or reaches the start of the file); only then are the last `limit` lines parsed. A window
/// starting within the file omits its first possible fragment. The extra complete line proves a cut.
/// Sparse or blank logs can require reading the full metadata length; this is not a fixed byte cap.
fn read_audit_tail(path: &Path, limit: usize) -> io::Result<AuditTail> {
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};

    /// One read that holds several hundred short audit lines; the window doubles for longer lines.
    const WINDOW: u64 = 64 * 1024;

    let mut file = match File::open(path) {
        Ok(file) => file,
        // A device that has never audited anything has no file; that is an empty record, not an
        // unreadable one.
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(AuditTail {
                lines: Vec::new(),
                truncated: false,
            })
        }
        Err(error) => return Err(error),
    };
    let length = file.metadata()?.len();
    let mut window = WINDOW;
    let (bytes, start) = loop {
        let start = length.saturating_sub(window);
        let mut bytes = vec![0u8; (length - start) as usize];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes);
        let complete_nonblank = text
            .lines()
            .skip(usize::from(start != 0))
            .filter(|line| !line.trim().is_empty())
            .count();
        if start == 0 || complete_nonblank > limit {
            break (bytes, start);
        }
        window = window.saturating_mul(2);
    };

    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text
        .lines()
        .skip(usize::from(start != 0))
        .filter(|line| !line.trim().is_empty())
        .collect();
    // At nonzero start the loop found an extra complete entry; at byte zero this is the full count.
    let truncated = lines.len() > limit;
    if lines.len() > limit {
        lines.drain(..lines.len() - limit);
    }
    let lines = lines
        .into_iter()
        .map(|line| {
            // The same fallback `Store::tail_events` uses: a corrupt line is shown as text rather
            // than taking the whole record with it. The Timeline ignores it; it has no tool.
            serde_json::from_str(line)
                .unwrap_or_else(|_| serde_json::Value::String(line.to_owned()))
        })
        .collect();

    Ok(AuditTail { lines, truncated })
}

/// Read every fact the rendered panel shows, from the same sources `/api/panel` reads. One read
/// per request: a browser reload is a fresh look at the machine, and nothing is cached in between.
fn panel_facts(app: &AppState) -> panel::PanelFacts {
    let state = app
        .store
        .load()
        .ok()
        .and_then(|state| serde_json::to_value(state).ok());
    // One bounded read of the device's own audit feeds both the Timeline and the older "Recent
    // events" table. The table shows the last ten of those lines, exactly the tail it showed before;
    // a failed read is still the empty table, while the Timeline says the audit could not be read.
    let (events, audit, audit_truncated) =
        match read_audit_tail(&app.data.join("events.jsonl"), panel::TIMELINE_LINES) {
            Ok(tail) => {
                let start = tail.lines.len().saturating_sub(EVENT_TAIL);
                let events = tail.lines[start..].to_vec();
                (events, Some(tail.lines), tail.truncated)
            }
            Err(_) => (Vec::new(), None, false),
        };
    let sessions = session_facts(app, audit.as_deref());
    let data_dir = app.data.display().to_string();
    let (token_file, token_command) = panel::token_facts(&data_dir);
    let console_url = std::fs::read_to_string(app.data.join("console.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("url")
                .and_then(serde_json::Value::as_str)
                .filter(|url| !url.is_empty())
                .map(str::to_owned)
        });
    let rendered_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0);

    let txns = txn_facts(app, audit.as_deref(), rendered_at_ms);
    panel::PanelFacts {
        version: app.release.version.clone(),
        channel: app.release.channel.clone(),
        sha256: app.release.sha256.clone(),
        pid: std::process::id(),
        data_dir,
        mcp_endpoint: format!("http://127.0.0.1:{}/mcp", app.port),
        token_file,
        token_command,
        state,
        events,
        audit,
        audit_truncated,
        rendered_at_ms,
        console_attempt: crate::console::last_attempt(),
        console_url,
        sessions,
        txns,
        panel_token: app.panel_token.clone(),
    }
}

/// 32 bytes from the operating system's generator, as lowercase hex: the same shape the helper's and
/// the console's tokens have, and for the same reason — a token that is guessable is not a token.
fn mint_panel_token() -> String {
    use rand_core::{OsRng, RngCore};

    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let mut text = String::with_capacity(64);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }

    text
}

/// `GET /api/runs/:id`: one run's record, for the transcript's click-to-expand.
///
/// Loopback and this boot's panel token: the record carries the full command line, which is more
/// than the journal's capped summary, so it must not be readable by a page that lacks the token.
async fn panel_run(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    if !peer.ip().is_loopback() {
        return error_response(StatusCode::FORBIDDEN, "loopback_only", None);
    }
    if !app.panel_authorized(&headers) {
        return error_response(StatusCode::FORBIDDEN, "unauthorized", None);
    }
    let runs = crate::capabilities::runs::Runs::new(&app.data, &app.data.join("workspace"));
    match runs.get(&id) {
        Ok(record) => Json(serde_json::json!({ "ok": true, "run": record })).into_response(),
        Err(crate::capabilities::runs::RunsError::UnknownRun) => {
            error_response(StatusCode::NOT_FOUND, "unknown_run", None)
        }
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "run_unreadable", None),
    }
}

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
    // The same tail `state.get` answers, and the same reading of a failure: null says the device
    // could not read its log, which is not the same claim as "nothing was recorded".
    let events = match app.store.tail_events(EVENT_TAIL) {
        Ok(events) => serde_json::json!(events),
        Err(_) => serde_json::Value::Null,
    };
    Json(serde_json::json!({
        "version": app.release.version,
        "channel": app.release.channel,
        "sha256": app.release.sha256,
        "pid": std::process::id(),
        "state": state,
        "data_dir": app.data.display().to_string(),
        "events": events,
        // The console link's own story. A device cannot report a failure while it is failing, so this
        // is the last attempt it knows about — and it is the one place that knows it: the panel, on the
        // machine, which is where an operator looks when the console says "offline".
        "console_attempt": crate::console::last_attempt(),
    }))
    .into_response()
}

/// The xterm the panel's terminal needs, served from the binary so the page has no CDN dependency.
const PANEL_XTERM_JS: &str = include_str!("../assets/xterm.js");
const PANEL_XTERM_CSS: &str = include_str!("../assets/xterm.css");
const PANEL_XTERM_FIT: &str = include_str!("../assets/addon-fit.js");
/// The panel's own terminal glue, kept apart from the emulator: this is the file that speaks
/// `/api/terminal/*` and knows the page's element ids.
const PANEL_TERMINAL_JS: &str = include_str!("../assets/panel-terminal.js");

/// `GET /api/terminal`: the live terminal sessions, read-only, for the panel's list.
async fn panel_terminal(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    if !peer.ip().is_loopback() {
        return error_response(StatusCode::FORBIDDEN, "loopback_only", None);
    }
    let sessions: Vec<serde_json::Value> = app
        .terminal
        .list()
        .into_iter()
        .map(|session| {
            serde_json::json!({
                "id": session.id.as_str(),
                "kind": session.kind.as_str(),
                "shell": session.shell,
                "started_at_ms": session.started_at_ms,
                "holder": terminal::holder_json(&session.holder),
                "goal": session.goal.as_ref().map(terminal::goal_json),
            })
        })
        .collect();
    Json(serde_json::json!({ "sessions": sessions })).into_response()
}

#[derive(Deserialize)]
struct PanelTerminalCreate {
    kind: String,
    port: Option<String>,
    baud: Option<u32>,
    shell: Option<String>,
}

#[derive(Deserialize)]
struct PanelTerminalResize {
    rows: u16,
    cols: u16,
}

#[derive(Deserialize)]
struct PanelTerminalWrite {
    data: String,
}

#[derive(Deserialize)]
struct PanelTerminalRead {
    cursor: Option<u64>,
    /// How long the read may park waiting for output. The panel asks for 15 s; the device caps it.
    wait_ms: Option<u64>,
}

/// The one gate every panel terminal operation passes: a loopback peer and this boot's token.
fn panel_terminal_allowed(app: &AppState, peer: SocketAddr, headers: &HeaderMap) -> bool {
    peer.ip().is_loopback() && app.panel_authorized(headers)
}

// Per-request, positive test checkpoints; production builds contain neither hooks nor waits.
#[cfg(all(test, unix))]
#[derive(Clone, Debug)]
struct PanelTestEvent {
    tool: &'static str,
    stage: &'static str,
    txn: Option<TxnId>,
    receipt: Option<serde_json::Value>,
}

#[cfg(all(test, unix))]
struct PanelTestControl {
    events: tokio::sync::mpsc::UnboundedSender<PanelTestEvent>,
    gates: std::collections::HashMap<(&'static str, &'static str), Arc<tokio::sync::Semaphore>>,
}

// Weak capture observes the constructor's actual service, without adding a product API or DTO.
#[cfg(all(test, unix))]
thread_local! {
    static PANEL_TEST_TERMINAL: std::cell::RefCell<Option<std::sync::Weak<terminal::PtyRegistry>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, unix))]
tokio::task_local! {
    static PANEL_TEST_CONTROL: Arc<PanelTestControl>;
}

#[cfg(all(test, unix))]
async fn panel_test_checkpoint(
    call: &PanelCall<'_>,
    stage: &'static str,
    receipt: Option<serde_json::Value>,
) {
    if let Ok(control) = PANEL_TEST_CONTROL.try_with(Arc::clone) {
        let _ = control.events.send(PanelTestEvent {
            tool: call.tool,
            stage,
            txn: call.txn.clone(),
            receipt,
        });
        if let Some(gate) = control.gates.get(&(call.tool, stage)) {
            if let Ok(permit) = gate.acquire().await {
                permit.forget();
            }
        }
    }
}

/// One panel operation in flight: the audit's call id, the `trace.call` its journal pair shares,
/// and the tool it is running.
///
/// The three travel together because every ending names all three: the audit's `end` line, the
/// journal's `call.finished`/`call.denied`, and the trace a reader joins them by. The id is the
/// audit's own counter, so one call has one id in both records.
struct PanelCall<'a> {
    id: u64,
    trace: String,
    tool: &'static str,
    txn: Option<TxnId>,
    owns_txn: bool,
    app: &'a AppState,
    /// Consumed once, while the trace's transaction id remains available to audit and publication.
    reserved: bool,
    started: bool,
    /// Capture the actual dispatch result before the first ending await; None remains unknown.
    outcome: Option<bool>,
}

impl PanelCall<'_> {
    fn release(&mut self, reason: Option<&'static str>) -> Option<crate::txns::Receipt> {
        if !std::mem::take(&mut self.reserved) {
            return None;
        }
        self.app.txns.finish_panel(
            &self.app.terminal,
            self.txn
                .as_ref()
                .expect("a reservation carries its resolved transaction"),
            self.owns_txn,
            reason,
            now_ms(),
        )
    }
}

impl Drop for PanelCall<'_> {
    fn drop(&mut self) {
        if self.reserved {
            if !self.started {
                if let Some(txn) = &self.txn {
                    self.app.txns.record_refusal(txn);
                }
            }
            // Known success stays successful if the audit ending is cancelled. Unknown execution
            // is an aborted panel declaration, never an invented dispatch failure or finished row.
            let reason = match self.outcome {
                Some(true) => None,
                Some(false) => Some("the panel operation failed"),
                None => Some("the panel request was cancelled before its outcome was known"),
            };
            let _ = self.release(reason);
        }
    }
}

enum PanelCallError {
    SessionMissing(String),
    Txn(summrise_core::TxnError),
    AuditUnwritable,
}

impl PanelCallError {
    fn code(&self) -> &'static str {
        use summrise_core::TxnError;
        match self {
            Self::SessionMissing(_) => "unknown_session",
            Self::AuditUnwritable => "audit_unwritable",
            Self::Txn(error) => match error {
                TxnError::Unknown => "txn_unknown",
                TxnError::Ended => "txn_ended",
                TxnError::NotAuthor => "txn_not_author",
                TxnError::AlreadyOpen => "txn_already_open",
                TxnError::SessionRequired => "session_required",
                TxnError::SessionMismatch => "txn_session_mismatch",
                TxnError::ScopeMismatch => "txn_scope_mismatch",
                TxnError::TooManyOpen => "too_many_txns",
                TxnError::TooManyText => "arguments",
            },
        }
    }

    fn response(self) -> Response {
        let code = self.code();
        match self {
            Self::AuditUnwritable => panel_not_audited(),
            Self::SessionMissing(id) => error_response(
                StatusCode::BAD_REQUEST,
                code,
                Some(terminal::PtyError::UnknownSession { id }.to_string()),
            ),
            Self::Txn(error) => error_response(
                if error == summrise_core::TxnError::TooManyOpen {
                    StatusCode::CONFLICT
                } else {
                    StatusCode::BAD_REQUEST
                },
                code,
                Some(error.to_string()),
            ),
        }
    }
}

/// Reserve the resolved panel participant before audit awaits. A required operation either has a
/// real transaction or a typed refusal; only Bootstrap create may carry no transaction.
async fn panel_call_begin<'a>(
    app: &'a AppState,
    tool: &'static str,
    session: Option<&terminal::SessionId>,
) -> Result<PanelCall<'a>, PanelCallError> {
    let id = app.audit.next_id();
    let mut call = PanelCall {
        id,
        trace: format!("call-{id}"),
        tool,
        txn: None,
        owns_txn: false,
        app,
        reserved: false,
        started: false,
        outcome: None,
    };
    if let Some(session) = session {
        match panel_transaction(app, session) {
            Ok(txn) => {
                call.txn = Some(txn.id);
                call.owns_txn = txn.owned;
                call.reserved = true;
            }
            Err(error) => {
                let error = match error {
                    crate::txns::PanelAdmissionError::SessionMissing => {
                        PanelCallError::SessionMissing(session.as_str().to_owned())
                    }
                    crate::txns::PanelAdmissionError::Txn(error) => PanelCallError::Txn(error),
                };
                // Refusal does not dispatch. Like the MCP denial path, an unwritable denial
                // cannot turn this typed refusal into an allow or a synthetic working record.
                if app
                    .audit
                    .begin(
                        id,
                        now_ms(),
                        tool,
                        "deny",
                        crate::mcp::Arrival::Panel,
                        None,
                        None,
                        Some(error.code()),
                        None,
                        false,
                    )
                    .await
                {
                    app.audit
                        .end(
                            id,
                            now_ms(),
                            false,
                            Some("refused"),
                            crate::mcp::Arrival::Panel,
                            None,
                            None,
                            None,
                            None,
                            None,
                        )
                        .await;
                }
                publish_panel_call(
                    app,
                    "call.denied",
                    &call.trace,
                    serde_json::json!({"tool":tool,"door":"panel","ok":false,"reason":error.code(),"txn":null}),
                );
                return Err(error);
            }
        }
    }
    if !app
        .audit
        .begin(
            id,
            now_ms(),
            tool,
            "allow",
            crate::mcp::Arrival::Panel,
            None,
            None,
            None,
            call.txn.as_ref().map(TxnId::as_str),
            tool == "terminal.create",
        )
        .await
    {
        if let Some(txn) = &call.txn {
            app.txns.record_refusal(txn);
        }
        let _ = call.release(Some("the panel's authorization line could not be written"));
        publish_panel_call(
            app,
            "call.denied",
            &call.trace,
            serde_json::json!({"tool":tool,"door":"panel","ok":false,"reason":"audit_unwritable","txn":call.txn.as_ref().map(TxnId::as_str)}),
        );
        return Err(PanelCallError::AuditUnwritable);
    }
    if let Some(txn) = &call.txn {
        app.txns.record_call(txn);
    }
    call.started = true;
    publish_panel_call(
        app,
        "call.started",
        &call.trace,
        serde_json::json!({"tool":tool,"door":"panel","txn":call.txn.as_ref().map(TxnId::as_str)}),
    );
    #[cfg(all(test, unix))]
    panel_test_checkpoint(&call, "dispatch", None).await;
    Ok(call)
}

/// The authoritative registry owns both origin and reservation; no unlocked lookup can race the
/// last participant's ending. This checkpoint precedes that authoritative admission in tests.
fn panel_transaction(
    app: &AppState,
    session: &terminal::SessionId,
) -> Result<crate::txns::PanelTxn, crate::txns::PanelAdmissionError> {
    #[cfg(test)]
    after_panel_begin_check();
    app.txns.admit_panel(&app.terminal, session, now_ms())
}

#[cfg(test)]
thread_local! {
    static AFTER_PANEL_BEGIN_CHECK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn after_panel_begin_check() {
    AFTER_PANEL_BEGIN_CHECK.with(|hook| {
        let pause = hook.borrow_mut().take();
        if let Some(pause) = pause {
            pause();
        }
    });
}

async fn panel_call_end(app: &AppState, mut call: PanelCall<'_>, error_kind: Option<&'static str>) {
    call.outcome = Some(error_kind.is_none());
    if error_kind.is_some() {
        if let Some(txn) = &call.txn {
            app.txns.record_failure(txn);
        }
    }
    #[cfg(all(test, unix))]
    panel_test_checkpoint(&call, "before_end", None).await;
    app.audit
        .end(
            call.id,
            now_ms(),
            error_kind.is_none(),
            error_kind,
            crate::mcp::Arrival::Panel,
            None,
            None,
            None,
            None,
            call.txn.as_ref().map(TxnId::as_str),
        )
        .await;
    match error_kind {
        None => publish_panel_call(
            app,
            "call.finished",
            &call.trace,
            serde_json::json!({
                "tool": call.tool,
                "door": "panel",
                "ok": true,
                "txn": call.txn.as_ref().map(TxnId::as_str),
            }),
        ),
        Some(kind) => publish_panel_call(
            app,
            "call.denied",
            &call.trace,
            serde_json::json!({
                "tool": call.tool,
                "door": "panel",
                "ok": false,
                "reason": kind,
                "txn": call.txn.as_ref().map(TxnId::as_str),
            }),
        ),
    }
    let reason = error_kind.map(|_| "the panel operation failed");
    // The registry serializes release + final ending with new admissions. No lock crosses audit
    // or publication, and only the last participant receives the unit's one actual receipt.
    if let Some(receipt) = call.release(reason) {
        #[cfg(all(test, unix))]
        panel_test_checkpoint(&call, "receipt", Some(receipt.to_json())).await;
        panel_txn_audit(app, &receipt.txn, receipt.state, &receipt.purpose).await;
    }
}

/// The durable half of a panel transaction's ending: one begin/end pair the audit can pair, with
/// the purpose on the begin row and the declared state on the end row.
async fn panel_txn_audit(app: &AppState, txn: &TxnId, state: TxnState, purpose: &str) {
    let id = app.audit.next_id();
    let (tool, outcome) = match state {
        TxnState::Committed => ("txn.commit", "committed"),
        TxnState::Aborted => ("txn.abort", "aborted"),
        TxnState::Working => return,
    };
    let args = format!("purpose={purpose}");
    app.audit
        .begin(
            id,
            now_ms(),
            tool,
            "allow",
            crate::mcp::Arrival::Panel,
            None,
            None,
            Some(&args),
            Some(txn.as_str()),
            false,
        )
        .await;
    app.audit
        .end(
            id,
            now_ms(),
            true,
            None,
            crate::mcp::Arrival::Panel,
            None,
            None,
            Some(outcome),
            None,
            Some(txn.as_str()),
        )
        .await;
}

/// Publish one half of a panel call's journal pair, when this process has a journal.
fn publish_panel_call(app: &AppState, kind: &str, trace: &str, payload: serde_json::Value) {
    if let Some(bus) = &app.bus {
        let _ = bus.publish_traced(
            summrise_bus::Plane::Control,
            kind,
            None,
            Some(trace),
            payload,
        );
    }
}

/// What a panel handler answers when its own audit line cannot be written: the operation did not
/// run, and the refusal says why without pretending the device is broken.
fn panel_not_audited() -> Response {
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "audit_unwritable",
        Some(
            "the device's event log could not be written, so the operation was not run".to_owned(),
        ),
    )
}

async fn panel_terminal_create(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<PanelTerminalCreate>,
) -> Response {
    if !panel_terminal_allowed(&app, peer, &headers) {
        return error_response(StatusCode::FORBIDDEN, "panel_only", None);
    }
    let kind = match terminal::Kind::parse(&body.kind) {
        Ok(kind) => kind,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_kind",
                Some(error.to_string()),
            )
        }
    };
    let target = match kind {
        terminal::Kind::Pty => terminal::CreateTarget::Pty {
            shell: body.shell.as_deref(),
        },
        terminal::Kind::Serial => {
            let Some(port) = body.port.as_deref().filter(|port| !port.is_empty()) else {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "serial_needs_port",
                    Some("kind serial needs a port, e.g. COM3".to_owned()),
                );
            };
            let mut serial = terminal::SerialTarget::new(port);
            if let Some(baud) = body.baud {
                serial.baud = baud;
            }
            terminal::CreateTarget::Serial(serial)
        }
        terminal::Kind::Ssh => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "ssh_not_in_panel",
                Some("the panel opens ptys and serial lines only".to_owned()),
            )
        }
    };
    let call = match panel_call_begin(&app, "terminal.create", None).await {
        Ok(call) => call,
        Err(error) => return error.response(),
    };
    match app
        .terminal
        .create(Holder::panel(), None, target, 24, 80)
        .await
    {
        Ok(id) => {
            panel_call_end(&app, call, None).await;
            Json(serde_json::json!({
                "id": id.as_str(),
                "kind": kind.as_str(),
            }))
            .into_response()
        }
        Err(error) => {
            panel_call_end(&app, call, Some("refused")).await;
            error_response(
                StatusCode::BAD_REQUEST,
                "terminal_create_refused",
                Some(error.to_string()),
            )
        }
    }
}

async fn panel_terminal_write(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(body): Json<PanelTerminalWrite>,
) -> Response {
    if !panel_terminal_allowed(&app, peer, &headers) {
        return error_response(StatusCode::FORBIDDEN, "panel_only", None);
    }
    let Some(id) = terminal::SessionId::parse(&id) else {
        return error_response(StatusCode::BAD_REQUEST, "invalid_session", None);
    };
    let call = match panel_call_begin(&app, "terminal.write", Some(&id)).await {
        Ok(call) => call,
        Err(error) => return error.response(),
    };
    match app.terminal.write(&id, body.data.as_bytes()).await {
        Ok(written) => {
            panel_call_end(&app, call, None).await;
            Json(serde_json::json!({ "written": written })).into_response()
        }
        Err(error) => {
            panel_call_end(&app, call, Some("refused")).await;
            error_response(
                StatusCode::BAD_REQUEST,
                "terminal_write_refused",
                Some(error.to_string()),
            )
        }
    }
}

/// `POST /api/terminal/:id/resize`: the browser's grid, so a full-screen program draws correctly.
async fn panel_terminal_resize(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(body): Json<PanelTerminalResize>,
) -> Response {
    if !panel_terminal_allowed(&app, peer, &headers) {
        return error_response(StatusCode::FORBIDDEN, "panel_only", None);
    }
    let Some(id) = terminal::SessionId::parse(&id) else {
        return error_response(StatusCode::BAD_REQUEST, "invalid_session", None);
    };
    let call = match panel_call_begin(&app, "terminal.resize", Some(&id)).await {
        Ok(call) => call,
        Err(error) => return error.response(),
    };
    match app.terminal.resize(&id, body.rows, body.cols).await {
        Ok(()) => {
            panel_call_end(&app, call, None).await;
            Json(serde_json::json!({ "resized": true })).into_response()
        }
        Err(error) => {
            panel_call_end(&app, call, Some("refused")).await;
            error_response(
                StatusCode::BAD_REQUEST,
                "terminal_resize_refused",
                Some(error.to_string()),
            )
        }
    }
}

/// The panel's read answer: the lossy text, and the bytes a terminal replays.
///
/// Both are always present so a client chooses: `text` is what a person reads in a log, and
/// `data_base64` is what an emulator must be fed, because a multi-byte character split by the
/// cursor is two U+FFFD in the text and intact in the bytes.
fn panel_read_answer(answer: &terminal::ReadAnswer) -> serde_json::Value {
    serde_json::json!({
        "text": String::from_utf8_lossy(&answer.data),
        "data_base64": base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &answer.data,
        ),
        "cursor": answer.cursor,
        "alive": answer.alive,
        "exit_code": answer.end.exit_code,
        "signal": answer.end.signal,
    })
}

async fn panel_terminal_read(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(query): Query<PanelTerminalRead>,
) -> Response {
    if !panel_terminal_allowed(&app, peer, &headers) {
        return error_response(StatusCode::FORBIDDEN, "panel_only", None);
    }
    let Some(id) = terminal::SessionId::parse(&id) else {
        return error_response(StatusCode::BAD_REQUEST, "invalid_session", None);
    };
    let call = match panel_call_begin(&app, "terminal.read", Some(&id)).await {
        Ok(call) => call,
        Err(error) => return error.response(),
    };
    let wait = Duration::from_millis(query.wait_ms.unwrap_or(0).min(30_000));
    match app
        .terminal
        .read_wait(&id, query.cursor.unwrap_or(0), wait)
        .await
    {
        Ok(answer) => {
            panel_call_end(&app, call, None).await;
            Json(panel_read_answer(&answer)).into_response()
        }
        Err(error) => {
            panel_call_end(&app, call, Some("refused")).await;
            error_response(
                StatusCode::BAD_REQUEST,
                "terminal_read_refused",
                Some(error.to_string()),
            )
        }
    }
}

async fn panel_terminal_close(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    if !panel_terminal_allowed(&app, peer, &headers) {
        return error_response(StatusCode::FORBIDDEN, "panel_only", None);
    }
    let Some(id) = terminal::SessionId::parse(&id) else {
        return error_response(StatusCode::BAD_REQUEST, "invalid_session", None);
    };
    let call = match panel_call_begin(&app, "terminal.close", Some(&id)).await {
        Ok(call) => call,
        Err(error) => return error.response(),
    };
    match app.terminal.close(&id).await {
        Ok(()) => {
            // The transaction ends here too: the close is this request's operation, and
            // The final registry release keeps the snapshot when the session is gone.
            panel_call_end(&app, call, None).await;
            Json(serde_json::json!({ "closed": true })).into_response()
        }
        Err(error) => {
            panel_call_end(&app, call, Some("refused")).await;
            error_response(
                StatusCode::BAD_REQUEST,
                if matches!(&error, terminal::PtyError::UnknownSession { .. }) {
                    "unknown_session"
                } else {
                    "terminal_close_refused"
                },
                Some(error.to_string()),
            )
        }
    }
}

/// The panel's enrollment request, as the page sends it. `force` and `allow_plaintext` are the two
/// operator decisions the form exposes; the URL/code rules are `enroll`'s, not duplicated here.
#[derive(Deserialize)]
struct PanelEnroll {
    console_url: String,
    code: String,
    force: Option<bool>,
    allow_plaintext: Option<bool>,
}

/// `POST /api/enroll`: enrol this machine with the one-time code the console minted for it.
///
/// Three gates, in this order: the peer must be loopback (the listener already binds there; this also
/// refuses a request that arrived through something else), the header must carry this boot's panel
/// token, and only then does `enroll` parse the URL and the code. The body is parsed by the extractor
/// before those checks, but nothing is contacted, signed, or written until all three pass.
///
/// The answer is the device id and nothing else: the console token a registration returns is written
/// to disk and never echoed.
async fn enroll_from_panel(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<PanelEnroll>,
) -> Response {
    if !peer.ip().is_loopback() {
        return error_response(StatusCode::FORBIDDEN, "loopback_only", None);
    }
    if !app.panel_authorized(&headers) {
        return error_response(StatusCode::FORBIDDEN, "unauthorized", None);
    }

    let invocation = crate::enroll::EnrollInvocation {
        console_url: request.console_url,
        code: request.code,
        data: app.data.clone(),
        force: request.force.unwrap_or(false),
        allow_plaintext: request.allow_plaintext.unwrap_or(false),
    };
    match crate::enroll::enroll(&invocation).await {
        Ok(enrollment) => {
            // The app builds its console link at boot. Under the launcher, exit so the supervisor
            // restarts it with the link that was just written; standalone, say so instead of dying.
            //
            // The answer is built before the hook is asked. The hook's delay is what lets the tiny
            // response reach the page; the handler cannot observe the body's flush itself, so the
            // delay is the stated mechanism, not a guarantee.
            let restarting = app.supervised;
            let answer = Json(serde_json::json!({ "id": enrollment.id, "restarting": restarting }))
                .into_response();
            if restarting {
                (app.restart)();
            }
            answer
        }
        // The operator reads the device's own sentence; the panel invents none.
        Err(error) => sentence_response(StatusCode::BAD_REQUEST, &error.to_string()),
    }
}

/// The rendered panel's enrollment form. A browser form cannot set the JSON route's header, so the
/// token travels in a hidden field; `force` and `allow_plaintext` are checkboxes, whose submitted
/// value is `on` (or absent), not a JSON boolean.
#[derive(Deserialize)]
struct PanelEnrollForm {
    /// Optional so a missing field is the page's `usage` banner, not axum's plain 422: every
    /// failure of this form has a redirect to explain it.
    console_url: Option<String>,
    code: Option<String>,
    force: Option<String>,
    allow_plaintext: Option<String>,
    /// This boot's panel token, rendered into the form. A cross-site page cannot read it.
    panel_token: Option<String>,
}

/// The rendered panel's "Check for updates now" form. Only the panel token travels; the source and
/// the release policy stay in the scheduler.
#[derive(Deserialize)]
struct PanelUpdateForm {
    /// This boot's panel token, rendered into the form. A cross-site page cannot read it.
    panel_token: Option<String>,
}

/// `POST /api/panel/update`: ask the scheduler to check now.
///
/// The gates are the enrollment form's: loopback and this boot's panel token. The answer is a
/// redirect so a reload does not repeat the request; the scheduler consumes the file within one
/// poll and writes the result into `state.json`, which the Updates card reads.
async fn panel_update(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<PanelUpdateForm>,
) -> Response {
    if !peer.ip().is_loopback() {
        return error_response(StatusCode::FORBIDDEN, "loopback_only", None);
    }
    let authorized = app.panel_authorized(&headers)
        || form
            .panel_token
            .as_deref()
            .is_some_and(|presented| app.panel_token_matches(presented));
    if !authorized {
        return error_response(StatusCode::FORBIDDEN, "unauthorized", None);
    }
    match crate::update_source::UpdateSource::load(&app.data) {
        Ok(Some(_)) => match std::fs::write(
            app.data.join(crate::update_source::REQUEST_FILE),
            b"manual\n",
        ) {
            Ok(()) => Redirect::to("/panel#updates").into_response(),
            Err(error) => sentence_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("the update request could not be written: {error}"),
            ),
        },
        Ok(None) => sentence_response(
            StatusCode::BAD_REQUEST,
            "no update source is configured on this device",
        ),
        Err(error) => sentence_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("the update source could not be read: {error}"),
        ),
    }
}

/// `POST /panel/enroll`: the form half of `/api/enroll`, with the same gates and the same
/// [`crate::enroll::enroll`] call. The answer is a redirect, not a body, so a reload cannot repeat
/// the write and the page remains a server-rendered read.
async fn enroll_from_panel_form(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<PanelEnrollForm>,
) -> Response {
    if !peer.ip().is_loopback() {
        return error_response(StatusCode::FORBIDDEN, "loopback_only", None);
    }
    let authorized = app.panel_authorized(&headers)
        || form
            .panel_token
            .as_deref()
            .is_some_and(|presented| app.panel_token_matches(presented));
    if !authorized {
        return error_response(StatusCode::FORBIDDEN, "unauthorized", None);
    }

    let console_url = form.console_url.as_deref().unwrap_or("").trim().to_owned();
    let code = form.code.as_deref().unwrap_or("").trim().to_owned();
    if console_url.is_empty() || code.is_empty() {
        return Redirect::to("/panel?error=usage").into_response();
    }

    let invocation = crate::enroll::EnrollInvocation {
        console_url,
        code,
        data: app.data.clone(),
        force: checked(form.force.as_deref()),
        allow_plaintext: checked(form.allow_plaintext.as_deref()),
    };
    match crate::enroll::enroll(&invocation).await {
        Ok(_) => {
            // The answer is built before the hook is asked, exactly like `/api/enroll`: the hook's
            // delay is what lets the browser receive the redirect and issue the follow-up GET.
            let restarting = app.supervised;
            let response = Redirect::to("/panel?enrolled=1").into_response();
            if restarting {
                (app.restart)();
            }
            response
        }
        Err(error) => {
            Redirect::to(&format!("/panel?error={}", enroll_error_code(&error))).into_response()
        }
    }
}

/// A checkbox's form value: present and not an explicit off value. The page submits `on`; `0` and
/// `false` are the spellings a hand-written client might send.
fn checked(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        let value = value.trim();
        !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
    })
}

/// The stable code a failed form enrollment redirects with. The page maps the code to a sentence;
/// `/api/enroll` keeps returning `EnrollError`'s own Display text unchanged.
fn enroll_error_code(error: &crate::enroll::EnrollError) -> &'static str {
    use crate::enroll::EnrollError;

    match error {
        EnrollError::Bootstrap(_) => "bootstrap",
        EnrollError::Usage => "usage",
        EnrollError::BadUrl(_) => "bad_url",
        EnrollError::DataDir { .. } => "data_dir",
        EnrollError::Identity(_) => "identity",
        EnrollError::Console { .. } => "console_unreachable",
        EnrollError::Status { .. } => "console_status",
        EnrollError::Refused { .. } => "console_refused",
        EnrollError::BadResponse { .. } => "bad_response",
        EnrollError::ConsoleFile { .. } => "console_file",
        EnrollError::TokenConflict { .. } => "token_conflict",
        EnrollError::Write { .. } => "write",
        #[cfg(windows)]
        EnrollError::Protect { .. } => "protect",
    }
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

/// `GET /api/status`: the lifecycle facts the desktop tray's 30 s poll parses.
#[derive(Serialize)]
struct Status {
    status: &'static str,
    version: String,
    channel: String,
    pid: u32,
    /// The same release under the name the tray's policy reads (`release`, the legacy body's key).
    release: String,
    /// Seconds since the start recorded in `state.json`, or `null` when that stamp is not a time.
    uptime_secs: Option<u64>,
    /// Rows the state document carries that are not `reaped` or `closed`.
    live_sessions: usize,
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

/// An error body whose sentence is owned: enrollment refusals are `EnrollError`'s Display, and the
/// panel shows that sentence rather than a second text.
fn sentence_response(status: StatusCode, sentence: &str) -> Response {
    (status, Json(serde_json::json!({ "error": sentence }))).into_response()
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

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
            files_shared_root: None,
            release_version: release_version.map(str::to_owned),
            release_sha: release_sha.map(str::to_owned),
            release_channel,
            supervised: release_sha.is_some(),
            console_url: None,
            console_token_file: None,
            console_allow_plaintext: false,
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
            self.app_with(None, restart::none())
        }

        /// The same router with an event journal attached, for the tests that read its pair back.
        fn app_with_bus(&self, bus: Option<Arc<summrise_bus::Bus>>) -> Router {
            router_with(
                &self.config,
                self.store.clone(),
                terminal::Limits::default(),
                None,
                None,
                None,
                restart::none(),
                Arc::new(PluginHost::load(&self.config.data)),
                None,
                bus,
                "test",
            )
            .0
        }

        /// The router and the transaction registry it shares with its MCP service, for the test
        /// that reads a panel transaction's record after the page's calls.
        fn app_and_txns(&self) -> (Router, Arc<crate::txns::TxnRegistry>) {
            let (app, _browser, txns) = router_with(
                &self.config,
                self.store.clone(),
                terminal::Limits::default(),
                None,
                None,
                None,
                restart::none(),
                Arc::new(PluginHost::load(&self.config.data)),
                None,
                None,
                "test",
            );
            (app, txns)
        }

        /// The same router with a helper identity, for the registration test.
        fn app_with_helper(&self, helper: Arc<crate::helper::Helper>) -> Router {
            self.app_with(Some(helper), restart::none())
        }

        /// The same router with a recorded restart hook, for the supervised-enrolment test.
        fn app_with_restart(&self, restart: crate::restart::Hook) -> Router {
            self.app_with(None, restart)
        }

        /// The one place the router arguments are listed; a new dependency is one edit.
        fn app_with(
            &self,
            helper: Option<Arc<crate::helper::Helper>>,
            restart: crate::restart::Hook,
        ) -> Router {
            router_with(
                &self.config,
                self.store.clone(),
                terminal::Limits::default(),
                None,
                None,
                helper,
                restart,
                Arc::new(PluginHost::load(&self.config.data)),
                None,
                None,
                "test",
            )
            .0
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

    #[cfg(unix)]
    mod panel_calls {
        use super::*;
        use summrise_core::{HolderKind, MAX_OPEN_TXNS};
        use tokio::sync::{mpsc, Semaphore};

        const DEADLINE: Duration = Duration::from_secs(10);

        struct RouteFixture {
            app: Router,
            txns: Arc<crate::txns::TxnRegistry>,
            terminal: Arc<terminal::PtyRegistry>,
            token: String,
            fixture: Fixture,
        }

        fn peer(mut request: Request<Body>) -> Request<Body> {
            request
                .extensions_mut()
                .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 23456))));
            request
        }

        impl RouteFixture {
            async fn new(label: &str) -> Self {
                let fixture = fixture(label, None);
                fixture.store.save(&fixture.store.initial()).unwrap();
                let (app, txns) = fixture.app_and_txns();
                let terminal = PANEL_TEST_TERMINAL
                    .with(|slot| {
                        slot.borrow_mut()
                            .take()
                            .and_then(|terminal| terminal.upgrade())
                    })
                    .expect("the actual router retains its real terminal service");
                let (status, html) = send_raw(app.clone(), peer(get("/panel", None))).await;
                assert_eq!(status, StatusCode::OK);
                let token = panel_meta(&html).expect("the actual panel supplies its boot token");
                Self {
                    app,
                    txns,
                    terminal,
                    token,
                    fixture,
                }
            }

            fn request(&self, method: &str, path: &str, body: Option<Value>) -> Request<Body> {
                let mut request = Request::builder()
                    .method(method)
                    .uri(path)
                    .header("x-summrise-panel", &self.token);
                let payload = if let Some(body) = body {
                    request = request.header(header::CONTENT_TYPE, "application/json");
                    Body::from(body.to_string())
                } else {
                    Body::empty()
                };
                peer(request.body(payload).unwrap())
            }

            async fn open(&self) -> String {
                let (status, value) = send(
                    self.app.clone(),
                    self.request(
                        "POST",
                        "/api/terminal/create",
                        Some(json!({"kind":"pty","shell":"/bin/cat"})),
                    ),
                )
                .await;
                assert_eq!(status, StatusCode::OK, "{value}");
                value["id"].as_str().unwrap().to_owned()
            }

            fn audit(&self) -> Vec<Value> {
                fs::read_to_string(self.fixture.config.data.join("events.jsonl"))
                    .unwrap_or_default()
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect()
            }
        }

        struct Events {
            rx: mpsc::UnboundedReceiver<PanelTestEvent>,
            seen: Vec<PanelTestEvent>,
        }

        impl Events {
            async fn wait(&mut self, tool: &str, stage: &str) -> Result<PanelTestEvent, String> {
                if let Some(event) = self
                    .seen
                    .iter()
                    .find(|event| event.tool == tool && event.stage == stage)
                {
                    return Ok(event.clone());
                }
                tokio::time::timeout(DEADLINE, async {
                    while let Some(event) = self.rx.recv().await {
                        let found = event.tool == tool && event.stage == stage;
                        self.seen.push(event.clone());
                        if found {
                            return Ok(event);
                        }
                    }
                    Err("the checkpoint channel closed".to_owned())
                })
                .await
                .map_err(|_| format!("positive checkpoint {tool}/{stage} never arrived"))?
            }

            fn drain(&mut self) {
                while let Ok(event) = self.rx.try_recv() {
                    self.seen.push(event);
                }
            }
        }

        fn control(holds: &[(&'static str, &'static str)]) -> (Arc<PanelTestControl>, Events) {
            let (events, rx) = mpsc::unbounded_channel();
            let gates = holds
                .iter()
                .map(|key| (*key, Arc::new(Semaphore::new(0))))
                .collect();
            (
                Arc::new(PanelTestControl { events, gates }),
                Events {
                    rx,
                    seen: Vec::new(),
                },
            )
        }

        fn release_all(control: &PanelTestControl) {
            for gate in control.gates.values() {
                gate.add_permits(16);
            }
        }

        async fn controlled(
            app: Router,
            request: Request<Body>,
            control: Arc<PanelTestControl>,
        ) -> (StatusCode, Value) {
            PANEL_TEST_CONTROL.scope(control, send(app, request)).await
        }

        async fn join_route(
            mut pending: tokio::task::JoinHandle<(StatusCode, Value)>,
        ) -> Result<(StatusCode, Value), String> {
            match tokio::time::timeout(DEADLINE, &mut pending).await {
                Ok(result) => result.map_err(|error| error.to_string()),
                Err(_) => {
                    pending.abort();
                    let _ = pending.await;
                    Err("the real route did not finish before its deadline".to_owned())
                }
            }
        }

        #[tokio::test]
        async fn panel_calls_full_capacity_denies_all_required_routes_before_io() {
            let fixture = RouteFixture::new("panel-calls-full-capacity").await;
            for _ in 0..MAX_OPEN_TXNS {
                fixture
                    .txns
                    .begin(
                        Holder::install(),
                        "occupied device work",
                        None,
                        None,
                        now_ms(),
                    )
                    .unwrap();
            }
            // Bootstrap is legal even while the working-record cap is full.
            let session = fixture.open().await;
            let (control, mut events) = control(&[]);
            let mut statuses = Vec::new();
            let mut errors = Vec::new();
            for (method, path, body) in [
                (
                    "POST",
                    format!("/api/terminal/{session}/write"),
                    Some(json!({"data":"forbidden payload\n"})),
                ),
                (
                    "GET",
                    format!("/api/terminal/{session}/read?cursor=0&wait_ms=0"),
                    None,
                ),
                (
                    "POST",
                    format!("/api/terminal/{session}/resize"),
                    Some(json!({"rows":37,"cols":101})),
                ),
                (
                    "POST",
                    format!("/api/terminal/{session}/close"),
                    Some(json!({})),
                ),
            ] {
                let (status, value) = controlled(
                    fixture.app.clone(),
                    fixture.request(method, &path, body),
                    Arc::clone(&control),
                )
                .await;
                statuses.push(status);
                errors.push(value["error"].clone());
            }
            events.drain();
            let (_, list) = send(fixture.app.clone(), peer(get("/api/terminal", None))).await;
            let row = list["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == session);
            let untagged = fixture
                .audit()
                .into_iter()
                .filter(|line| {
                    line["event"] == "begin"
                        && line["decision"] == "allow"
                        && [
                            "terminal.write",
                            "terminal.read",
                            "terminal.resize",
                            "terminal.close",
                        ]
                        .contains(&line["tool"].as_str().unwrap_or_default())
                        && line["txn"].is_null()
                })
                .count();
            println!(
                "PANEL_CAPACITY {}",
                json!({"statuses":statuses.iter().map(|status|status.as_u16()).collect::<Vec<_>>(),"errors":errors,"dispatches":events.seen.iter().filter(|event|event.stage=="dispatch").count(),"untagged_allows":untagged})
            );
            assert_eq!(
                statuses,
                vec![StatusCode::CONFLICT; 4],
                "all four protected production routes must reject capacity"
            );
            assert_eq!(errors, vec![json!("too_many_txns"); 4]);
            assert!(
                events.seen.iter().all(|event| event.stage != "dispatch"),
                "no terminal operation was dispatched: {:?}",
                events.seen
            );
            assert_eq!(
                untagged, 0,
                "a required route must not write an untagged allow"
            );
            assert!(
                row.is_some(),
                "capacity refusal keeps the public session row"
            );
            let native = fixture.terminal.list();
            let native = native
                .iter()
                .find(|row| row.id.as_str() == session)
                .expect("capacity refusal keeps the real native session");
            assert_eq!(native.rows, 24);
            assert_eq!(native.cols, 80);
            assert_eq!(fixture.txns.working_count(), MAX_OPEN_TXNS);
            assert!(fixture.txns.get(&TxnId::new("txn-test-21")).is_none());
        }

        #[tokio::test]
        async fn panel_calls_failed_resize_receipt_counts_one_call_and_one_failure() {
            let fixture = RouteFixture::new("panel-calls-failed-resize").await;
            let session = fixture.open().await;
            let (control, mut events) = control(&[]);
            let (status, _) = controlled(
                fixture.app.clone(),
                fixture.request(
                    "POST",
                    &format!("/api/terminal/{session}/resize"),
                    Some(json!({"rows":0,"cols":80})),
                ),
                control,
            )
            .await;
            events.drain();
            assert_eq!(status, StatusCode::BAD_REQUEST);
            let receipts: Vec<_> = events
                .seen
                .iter()
                .filter_map(|event| event.receipt.as_ref())
                .collect();
            assert_eq!(receipts.len(), 1);
            println!("PANEL_RESIZE_RECEIPT {}", receipts[0]);
            assert_eq!(receipts[0]["observed"]["calls"], 1);
            assert_eq!(receipts[0]["observed"]["failures"], 1);
            assert_eq!(receipts[0]["state"], "aborted");
        }

        struct Overlap {
            mid_state: TxnState,
            receipt: Value,
            ending_rows: usize,
            first_status: StatusCode,
            last_status: StatusCode,
        }

        async fn overlap_case(
            creator_read: bool,
            creator_first: bool,
            failed_borrower: bool,
        ) -> Result<Overlap, String> {
            let fixture = RouteFixture::new("panel-calls-overlap").await;
            let session = fixture.open().await;
            let creator_tool = if creator_read {
                "terminal.read"
            } else {
                "terminal.write"
            };
            let borrower_tool = if failed_borrower {
                "terminal.resize"
            } else if creator_read {
                "terminal.write"
            } else {
                "terminal.read"
            };
            let (control, mut events) =
                control(&[(creator_tool, "before_end"), (borrower_tool, "before_end")]);
            let creator_request = if creator_read {
                fixture.request(
                    "GET",
                    &format!("/api/terminal/{session}/read?cursor=0&wait_ms=15000"),
                    None,
                )
            } else {
                fixture.request(
                    "POST",
                    &format!("/api/terminal/{session}/write"),
                    Some(json!({"data":"lease marker\n"})),
                )
            };
            let mut creator = Some(tokio::spawn(controlled(
                fixture.app.clone(),
                creator_request,
                Arc::clone(&control),
            )));
            let mut borrower = None;
            let result = async {
                let admitted = events.wait(creator_tool, "dispatch").await?;
                let txn = admitted.txn.ok_or("the creator must have a transaction")?;
                let borrower_request = if failed_borrower {
                    fixture.request(
                        "POST",
                        &format!("/api/terminal/{session}/resize"),
                        Some(json!({"rows":0,"cols":80})),
                    )
                } else if creator_read {
                    fixture.request(
                        "POST",
                        &format!("/api/terminal/{session}/write"),
                        Some(json!({"data":"lease marker\n"})),
                    )
                } else {
                    fixture.request(
                        "GET",
                        &format!("/api/terminal/{session}/read?cursor=0&wait_ms=0"),
                        None,
                    )
                };
                borrower = Some(tokio::spawn(controlled(
                    fixture.app.clone(),
                    borrower_request,
                    Arc::clone(&control),
                )));
                let joined = events.wait(borrower_tool, "dispatch").await?;
                if joined.txn.as_ref() != Some(&txn) {
                    return Err(
                        "overlapping requests did not join the same real transaction".to_owned(),
                    );
                }
                events.wait(creator_tool, "before_end").await?;
                events.wait(borrower_tool, "before_end").await?;
                let first_tool = if creator_first {
                    creator_tool
                } else {
                    borrower_tool
                };
                control.gates[&(first_tool, "before_end")].add_permits(1);
                let first = if creator_first {
                    creator.take().unwrap()
                } else {
                    borrower.take().unwrap()
                };
                let (first_status, _) = join_route(first).await?;
                let mid_state = fixture
                    .txns
                    .get(&txn)
                    .ok_or("the shared record vanished")?
                    .state();
                let last_tool = if creator_first {
                    borrower_tool
                } else {
                    creator_tool
                };
                control.gates[&(last_tool, "before_end")].add_permits(1);
                let last = if creator_first {
                    borrower.take().unwrap()
                } else {
                    creator.take().unwrap()
                };
                let (last_status, _) = join_route(last).await?;
                events.drain();
                let receipts: Vec<_> = events
                    .seen
                    .iter()
                    .filter_map(|event| event.receipt.clone())
                    .collect();
                if receipts.len() != 1 {
                    return Err(format!(
                        "expected exactly one actual ending receipt, got {}",
                        receipts.len()
                    ));
                }
                let ending_rows = fixture
                    .audit()
                    .iter()
                    .filter(|line| {
                        line["event"] == "begin"
                            && ["txn.commit", "txn.abort"]
                                .contains(&line["tool"].as_str().unwrap_or_default())
                            && line["txn"] == txn.as_str()
                    })
                    .count();
                Ok(Overlap {
                    mid_state,
                    receipt: receipts[0].clone(),
                    ending_rows,
                    first_status,
                    last_status,
                })
            }
            .await;
            release_all(&control);
            for pending in [creator, borrower].into_iter().flatten() {
                let _ = join_route(pending).await;
            }
            result
        }

        #[tokio::test]
        async fn panel_calls_both_completion_orders_keep_all_admitted_observations() {
            let mut results = Vec::new();
            for case in [
                (true, false, false),
                (false, true, false),
                (false, true, true),
                (false, false, true),
            ] {
                results.push((case, overlap_case(case.0, case.1, case.2).await));
            }
            for (case, result) in &results {
                if let Ok(observed) = result {
                    println!(
                        "PANEL_INTERLEAVE {}",
                        json!({"creator_read":case.0,"creator_first":case.1,"failed_borrower":case.2,"mid_state":format!("{:?}",observed.mid_state),"receipt":observed.receipt,"ending_rows":observed.ending_rows})
                    );
                }
            }
            for ((_, creator_first, failed), result) in results {
                let observed = result.unwrap_or_else(|error| {
                    panic!("controlled production interleave failed: {error}")
                });
                assert_eq!(
                    observed.mid_state,
                    TxnState::Working,
                    "the first finisher must leave the shared unit open"
                );
                assert_eq!(observed.receipt["observed"]["calls"], 2);
                assert_eq!(
                    observed.receipt["observed"]["failures"],
                    if failed { 1 } else { 0 }
                );
                assert_eq!(
                    observed.receipt["state"],
                    if failed { "aborted" } else { "committed" }
                );
                assert_eq!(observed.ending_rows, 1);
                assert_eq!(
                    observed.first_status,
                    if failed && !creator_first {
                        StatusCode::BAD_REQUEST
                    } else {
                        StatusCode::OK
                    }
                );
                assert_eq!(
                    observed.last_status,
                    if failed && creator_first {
                        StatusCode::BAD_REQUEST
                    } else {
                        StatusCode::OK
                    }
                );
            }
        }

        #[tokio::test]
        async fn panel_calls_borrow_explicit_units_even_by_panel_at_full_capacity() {
            for by in [
                Holder {
                    kind: HolderKind::Token,
                    token_id: Some("srm_external".to_owned()),
                },
                Holder::panel(),
            ] {
                let fixture = RouteFixture::new("panel-calls-external").await;
                let session = fixture.open().await;
                let external = fixture
                    .txns
                    .begin(
                        by,
                        "explicit external work",
                        Some(SessionRef::new(&session)),
                        None,
                        now_ms(),
                    )
                    .unwrap();
                for _ in 1..MAX_OPEN_TXNS {
                    fixture
                        .txns
                        .begin(
                            Holder::install(),
                            "occupied device work",
                            None,
                            None,
                            now_ms(),
                        )
                        .unwrap();
                }
                let (status, value) = send(
                    fixture.app.clone(),
                    fixture.request(
                        "POST",
                        &format!("/api/terminal/{session}/resize"),
                        Some(json!({"rows":31,"cols":91})),
                    ),
                )
                .await;
                assert_eq!(status, StatusCode::OK, "{value}");
                let record = fixture.txns.get(external.id()).unwrap();
                assert_eq!(
                    record.state(),
                    TxnState::Working,
                    "an explicit record never becomes panel-auto-owned"
                );
                assert_eq!(record.calls(), 1);
                assert_eq!(fixture.txns.working_count(), MAX_OPEN_TXNS);
            }
        }

        #[tokio::test]
        async fn panel_calls_unknown_close_has_typed_absence_before_and_after_admission() {
            let fixture = RouteFixture::new("panel-calls-unknown-close").await;
            let session = fixture.open().await;
            let (control, mut events) = control(&[("terminal.close", "dispatch")]);
            let pending = tokio::spawn(controlled(
                fixture.app.clone(),
                fixture.request(
                    "POST",
                    &format!("/api/terminal/{session}/close"),
                    Some(json!({})),
                ),
                Arc::clone(&control),
            ));
            let reached = events.wait("terminal.close", "dispatch").await;
            let actual_close = send(
                fixture.app.clone(),
                fixture.request(
                    "POST",
                    &format!("/api/terminal/{session}/close"),
                    Some(json!({})),
                ),
            )
            .await;
            release_all(&control);
            let resumed = join_route(pending).await;
            reached.expect("the first close was admitted before the real competing close");
            assert_eq!(actual_close.0, StatusCode::OK);
            let (status, value) = resumed.unwrap();
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(value["error"], "unknown_session");
            let before = fixture.audit().len();
            let (status, value) = send(
                fixture.app.clone(),
                fixture.request(
                    "POST",
                    &format!("/api/terminal/{session}/close"),
                    Some(json!({})),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(value["error"], "unknown_session");
            assert!(fixture.audit()[before..]
                .iter()
                .all(|line| line["event"] != "begin" || line["decision"] != "allow"));
            assert_eq!(fixture.txns.working_count(), 0);
        }

        #[tokio::test]
        async fn panel_calls_audit_failure_never_dispatches_or_leaks_a_reserved_unit() {
            let fixture = RouteFixture::new("panel-calls-audit-failure").await;
            let session = fixture.open().await;
            let path = fixture.fixture.config.data.join("events.jsonl");
            let backup = fixture
                .fixture
                .config
                .data
                .join("owned-events.before-block");
            assert_eq!(path.parent().unwrap(), fixture.fixture.config.data);
            fs::rename(&path, &backup).unwrap();
            fs::create_dir(&path).unwrap();
            let (control, mut events) = control(&[]);
            let (status, value) = controlled(
                fixture.app.clone(),
                fixture.request(
                    "POST",
                    &format!("/api/terminal/{session}/write"),
                    Some(json!({"data":"must not dispatch\n"})),
                ),
                control,
            )
            .await;
            events.drain();
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(value["error"], "audit_unwritable");
            assert!(events.seen.iter().all(|event| event.stage != "dispatch"));
            let record = fixture.txns.get(&TxnId::new("txn-test-1")).unwrap();
            assert_eq!(
                (record.calls(), record.failures(), record.refusals()),
                (0, 0, 1)
            );
            assert_eq!(record.state(), TxnState::Aborted);
            assert_eq!(fixture.txns.working_count(), 0);
        }

        #[tokio::test]
        async fn panel_calls_cancel_after_allow_releases_unit_without_inventing_failure() {
            let fixture = RouteFixture::new("panel-calls-cancel").await;
            let session = fixture.open().await;
            let (control, mut events) = control(&[("terminal.read", "dispatch")]);
            let pending = tokio::spawn(controlled(
                fixture.app.clone(),
                fixture.request(
                    "GET",
                    &format!("/api/terminal/{session}/read?cursor=0&wait_ms=15000"),
                    None,
                ),
                Arc::clone(&control),
            ));
            let reached = events.wait("terminal.read", "dispatch").await;
            pending.abort();
            release_all(&control);
            let joined = pending.await;
            let event = reached.expect("a positive allow-before-dispatch checkpoint");
            assert!(joined.unwrap_err().is_cancelled());
            let record = fixture.txns.get(&event.txn.unwrap()).unwrap();
            assert_eq!(record.state(), TxnState::Aborted);
            assert_eq!((record.calls(), record.failures()), (1, 0));
            assert_eq!(fixture.txns.working_count(), 0);
        }

        #[tokio::test]
        async fn panel_calls_known_outcome_cancellation_preserves_observed_results() {
            let mut observations = Vec::new();
            for (rows, expected_state, expected_failures, expected_native) in [
                (33, TxnState::Committed, 0, (33, 99)),
                (0, TxnState::Aborted, 1, (24, 80)),
            ] {
                let fixture = RouteFixture::new("panel-calls-known-outcome-cancel").await;
                let session = fixture.open().await;
                let (control, mut events) = control(&[("terminal.resize", "before_end")]);
                let pending = tokio::spawn(controlled(
                    fixture.app.clone(),
                    fixture.request(
                        "POST",
                        &format!("/api/terminal/{session}/resize"),
                        Some(json!({"rows":rows,"cols":99})),
                    ),
                    Arc::clone(&control),
                ));
                let reached = events.wait("terminal.resize", "before_end").await;
                // Observe the real dispatch's metadata and counters while its ending is parked.
                let native = fixture
                    .terminal
                    .list()
                    .into_iter()
                    .find(|row| row.id.as_str() == session)
                    .map(|row| (row.rows, row.cols));
                let before = reached
                    .as_ref()
                    .ok()
                    .and_then(|event| event.txn.as_ref())
                    .and_then(|id| fixture.txns.get(id))
                    .map(|record| (record.calls(), record.failures(), record.refusals()));
                pending.abort();
                release_all(&control);
                let joined = pending.await;
                let event = reached.expect("the actual resize returned before the ending fence");
                let txn = event
                    .txn
                    .expect("the admitted actual resize carries a transaction");
                let record = fixture
                    .txns
                    .get(&txn)
                    .expect("Drop keeps the actual ended record");
                let slot_clear = fixture
                    .txns
                    .working_on(&SessionRef::new(&session))
                    .is_none();
                let audit = fixture.audit();
                let begin = audit
                    .iter()
                    .find(|line| {
                        line["event"] == "begin"
                            && line["decision"] == "allow"
                            && line["tool"] == "terminal.resize"
                            && line["txn"] == txn.as_str()
                    })
                    .expect("the allow audit preceded the actual resize");
                let call_tails = audit
                    .iter()
                    .filter(|line| line["event"] == "end" && line["id"] == begin["id"])
                    .count();
                let unit_tails = audit
                    .iter()
                    .filter(|line| {
                        ["txn.commit", "txn.abort"]
                            .contains(&line["tool"].as_str().unwrap_or_default())
                            && line["txn"] == txn.as_str()
                    })
                    .count();
                let cancelled = joined
                    .as_ref()
                    .err()
                    .is_some_and(tokio::task::JoinError::is_cancelled);
                let actual = json!({"rows_requested":rows,"native_before_cancel":native,"counts_before_cancel":before,"state":format!("{:?}",record.state()),"calls":record.calls(),"failures":record.failures(),"refusals":record.refusals(),"working":fixture.txns.working_count(),"slot_clear":slot_clear,"retained":fixture.txns.retained_count(),"call_tails":call_tails,"unit_tails":unit_tails,"joined_cancelled":cancelled});
                observations.push((
                    expected_state,
                    expected_failures,
                    expected_native,
                    native,
                    before,
                    record,
                    fixture.txns.working_count(),
                    slot_clear,
                    fixture.txns.retained_count(),
                    call_tails,
                    unit_tails,
                    cancelled,
                    actual,
                ));
            }
            // Both positive fences are reached and both tasks are cancelled/joined before assertions.
            for observation in &observations {
                println!("PANEL_KNOWN_CANCEL {}", observation.12);
            }
            for (
                expected_state,
                expected_failures,
                expected_native,
                native,
                before,
                record,
                working,
                slot_clear,
                retained,
                call_tails,
                unit_tails,
                cancelled,
                _,
            ) in observations
            {
                assert!(cancelled);
                assert_eq!(native, Some(expected_native));
                assert_eq!(before, Some((1, expected_failures, 0)));
                assert_eq!(record.state(), expected_state);
                assert_eq!(
                    (record.calls(), record.failures(), record.refusals()),
                    (1, expected_failures, 0)
                );
                assert_eq!(working, 0);
                assert!(slot_clear);
                assert_eq!(
                    retained, 1,
                    "the actual registry retains precisely one declaration"
                );
                assert_eq!(call_tails, 0, "Drop does not invent a completed call audit");
                assert_eq!(
                    unit_tails, 0,
                    "Drop does not invent a durable transaction ending audit"
                );
            }
        }
    }

    #[cfg(unix)]
    mod close_goal {
        use super::*;
        use std::sync::mpsc;

        struct GoalFixture {
            app: Arc<AppState>,
            _fixture: Fixture,
        }

        impl GoalFixture {
            fn new(label: &str) -> Self {
                let fixture = fixture(label, None);
                fixture.store.save(&fixture.store.initial()).unwrap();
                let app = Arc::new(AppState {
                    token: None,
                    store: fixture.store.clone(),
                    release: fixture.config.release(),
                    helper: None,
                    data: fixture.config.data.clone(),
                    port: 0,
                    panel_token: "test-panel-token".to_owned(),
                    boot: 1,
                    audit: Arc::new(crate::mcp::Audit::new(&fixture.config.data)),
                    supervised: false,
                    restart: restart::none(),
                    terminal: Arc::new(terminal::PtyRegistry::new(
                        fixture.store.clone(),
                        Arc::new(tokio::sync::Mutex::new(())),
                        terminal::Limits::default(),
                    )),
                    txns: Arc::new(crate::txns::TxnRegistry::new("close-panel")),
                    bus: None,
                });
                Self {
                    app,
                    _fixture: fixture,
                }
            }

            async fn registered_session(&self) -> terminal::SessionId {
                self.app
                    .terminal
                    .create(
                        Holder::panel(),
                        Some("umbrella goal"),
                        terminal::CreateTarget::Pty {
                            shell: Some("/bin/true"),
                        },
                        24,
                        80,
                    )
                    .await
                    .expect("the actual registered session opens")
            }

            async fn end_during_close(&self, state: TxnState) -> Value {
                let session = self.registered_session().await;
                let open =
                    panel_transaction(&self.app, &session).expect("the actual panel helper begins");
                assert!(open.owned);
                let app = Arc::clone(&self.app);
                let (checked, checked_rx) = mpsc::channel();
                let (release, release_rx) = mpsc::channel();
                let ending = std::thread::spawn(move || {
                    crate::txns::AFTER_GOAL_PRESENCE_CHECK.with(|hook| {
                        *hook.borrow_mut() = Some(Box::new(move || {
                            checked
                                .send(())
                                .expect("the scheduler observes panel presence");
                            release_rx
                                .recv()
                                .expect("the scheduler releases panel ending");
                        }));
                    });
                    let goal =
                        crate::txns::goal_now(&app.terminal, &app.txns.get(&open.id).unwrap());
                    // The actual panel metadata helper feeds the actual registry ending/receipt,
                    // the same production boundary panel_call_end uses for both ending states.
                    app.txns.end(
                        &open.id,
                        &Holder::panel(),
                        state,
                        (state == TxnState::Aborted).then_some("the panel operation failed"),
                        goal,
                        20,
                    )
                });
                let checked = checked_rx.recv_timeout(Duration::from_secs(10));
                let closed = self.app.terminal.close(&session).await;
                let released = release.send(());
                let ending = ending.join();
                checked.expect("panel ending reached the old presence/goal boundary");
                closed.expect("the real registered session closes");
                released.expect("panel ending resumes after close");
                ending
                    .expect("the panel ending returns")
                    .expect("the panel may end its own transaction")
                    .to_json()
            }
        }

        #[tokio::test]
        async fn close_goal_panel_commit_during_close_keeps_the_original_goal() {
            let fixture = GoalFixture::new("close-goal-panel-commit");
            let receipt = fixture.end_during_close(TxnState::Committed).await;
            assert_eq!(receipt["state"], "committed");
            assert_eq!(receipt["goal_at_start"]["text"], "umbrella goal");
            assert!(
                receipt.get("goal_at_end").is_none(),
                "a panel close is not a goal clear: {receipt}"
            );
        }

        #[tokio::test]
        async fn close_goal_panel_abort_during_close_keeps_the_original_goal() {
            let fixture = GoalFixture::new("close-goal-panel-abort");
            let receipt = fixture.end_during_close(TxnState::Aborted).await;
            assert_eq!(receipt["state"], "aborted");
            assert_eq!(receipt["reason"], "the panel operation failed");
            assert_eq!(receipt["goal_at_start"]["text"], "umbrella goal");
            assert!(
                receipt.get("goal_at_end").is_none(),
                "a panel close is not a goal clear: {receipt}"
            );
        }

        #[tokio::test]
        async fn close_goal_panel_implicit_begin_after_close_leaves_no_record_or_id() {
            let fixture = GoalFixture::new("close-goal-panel-begin");
            let session = fixture.registered_session().await;
            let app = Arc::clone(&fixture.app);
            let named = session.clone();
            let (checked, checked_rx) = mpsc::channel();
            let (release, release_rx) = mpsc::channel();
            let beginning = std::thread::spawn(move || {
                AFTER_PANEL_BEGIN_CHECK.with(|hook| {
                    *hook.borrow_mut() = Some(Box::new(move || {
                        checked
                            .send(())
                            .expect("the scheduler observes the panel begin check");
                        release_rx
                            .recv()
                            .expect("the scheduler releases panel begin");
                    }));
                });
                panel_transaction(&app, &named)
            });
            let checked = checked_rx.recv_timeout(Duration::from_secs(10));
            let closed = fixture.app.terminal.close(&session).await;
            let released = release.send(());
            let beginning = beginning.join();
            checked.expect("the panel begin reached its old check/goal boundary");
            closed.expect("the actual registered session closes");
            released.expect("the panel begin resumes after close");
            assert!(
                matches!(
                    beginning.expect("the panel begin returns"),
                    Err(crate::txns::PanelAdmissionError::SessionMissing)
                ),
                "a closed session must not admit an implicit panel begin"
            );
            assert_eq!(fixture.app.txns.working_count(), 0);
            assert_eq!(
                fixture
                    .app
                    .txns
                    .working_on(&SessionRef::new(session.as_str())),
                None
            );
            let next_session = fixture.registered_session().await;
            let next = panel_transaction(&fixture.app, &next_session)
                .expect("a registered session still begins");
            assert_eq!(
                next.id.as_str(),
                "txn-close-panel-1",
                "the missing session minted no id"
            );
            assert!(next.owned);
        }

        #[tokio::test]
        async fn close_goal_panel_explicit_clear_and_missing_session_remain_distinct() {
            for state in [TxnState::Committed, TxnState::Aborted] {
                for clear in [true, false] {
                    let fixture = GoalFixture::new("close-goal-panel-clear-or-missing");
                    let session = fixture.registered_session().await;
                    let open = panel_transaction(&fixture.app, &session).unwrap();
                    if clear {
                        fixture
                            .app
                            .terminal
                            .set_goal(&session, None, &Holder::panel(), 30)
                            .unwrap();
                    } else {
                        fixture.app.terminal.close(&session).await.unwrap();
                    }
                    let goal = crate::txns::goal_now(
                        &fixture.app.terminal,
                        &fixture.app.txns.get(&open.id).unwrap(),
                    );
                    let receipt = fixture
                        .app
                        .txns
                        .end(&open.id, &Holder::panel(), state, None, goal, 40)
                        .unwrap()
                        .to_json();
                    assert_eq!(receipt["goal_at_start"]["text"], "umbrella goal");
                    assert_eq!(
                        receipt.get("goal_at_end").is_some(),
                        clear,
                        "only an explicit clear adds the field: {receipt}"
                    );
                    if clear {
                        assert!(receipt["goal_at_end"].is_null());
                    }
                }
            }
        }

        #[tokio::test]
        async fn close_goal_panel_begin_keeps_existing_winner_ownership_and_present_empty_goal() {
            let fixture = GoalFixture::new("close-goal-panel-reuse");
            let own_session = fixture.registered_session().await;
            let own = panel_transaction(&fixture.app, &own_session).unwrap();
            let reused = panel_transaction(&fixture.app, &own_session).unwrap();
            assert_eq!(reused.id, own.id);
            assert!(
                reused.owned,
                "the panel's existing transaction stays panel-owned"
            );
            let client_session = fixture.registered_session().await;
            let by = Holder {
                kind: summrise_core::HolderKind::Token,
                token_id: Some("srm_client".to_owned()),
            };
            let client = fixture
                .app
                .txns
                .begin(
                    by,
                    "client work",
                    Some(SessionRef::new(client_session.as_str())),
                    fixture.app.terminal.goal_of(&client_session),
                    1,
                )
                .unwrap();
            let reused = panel_transaction(&fixture.app, &client_session).unwrap();
            assert_eq!(reused.id, *client.id());
            assert!(
                !reused.owned,
                "the panel must not end another author's existing record"
            );
            let empty_session = fixture.registered_session().await;
            fixture
                .app
                .terminal
                .set_goal(&empty_session, None, &Holder::panel(), 30)
                .unwrap();
            let empty = panel_transaction(&fixture.app, &empty_session)
                .expect("a present session without a goal is valid");
            assert!(fixture
                .app
                .txns
                .get(&empty.id)
                .unwrap()
                .goal_at_start()
                .is_none());
            assert_eq!(fixture.app.txns.working_count(), 3);
        }
    }

    // A missing case or a fixture served as placeholder HTML would hide a whole browser scenario.
    #[tokio::test]
    async fn device_ui_fixture_manifest_serves_current_renderers() {
        let fixture = fixture("device-ui-contract", Some(&token_hex()));
        let app = device_ui_fixture_app(
            &fixture,
            Arc::new(Mutex::new(json!({"panel":"/panel","activity":"/activity"}))),
        );
        let (status, manifest) = send(app.clone(), get("/__ui-fixtures/manifest", None)).await;
        assert_eq!(status, StatusCode::OK, "browser matrix manifest must exist");
        let cases = manifest["cases"].as_array().expect("named browser cases");
        assert_eq!(cases.len(), 15);
        for case in cases {
            let (status, html) =
                send_raw(app.clone(), get(case["url"].as_str().unwrap(), None)).await;
            assert_eq!(status, StatusCode::OK, "{}", case["name"]);
            assert!(html.contains("<!DOCTYPE html>"));
            assert!(html.contains("summrise-panel"));
            assert_eq!(
                html.matches("<h1").count(),
                1,
                "{} keeps an honest document title",
                case["name"]
            );
            assert!(html.contains(if case["kind"] == "panel" {
                "id=\"content\""
            } else {
                "id=\"main\""
            }));
            let marker = match case["name"].as_str().unwrap() {
                "panel-normal" => "https://console.example.test",
                "panel-first" => "id=\"enroll-form\"",
                "panel-empty" => "No call has been recorded on this device.",
                "panel-unreadable" => "id=\"timeline-unreadable\"",
                "panel-bounded-long" => "The newest 200 lines; older lines are not shown.",
                "activity-working" => "data-state=\"working\"",
                "activity-refused" => "class=\"current-refusal\"",
                "activity-closed-failure" => "data-goal=\"unavailable\"",
                "activity-aborted" => "data-state=\"aborted\"",
                "activity-orphan" => "data-retained=\"false\"",
                "activity-no-goal" => "data-goal=\"none\"",
                "activity-no-session-empty" => "data-state=\"empty\"",
                "activity-no-session-recorded" => "data-state=\"rows\"",
                "activity-unreadable" => "data-state=\"unreadable\"",
                "activity-multi-long" => "session-fixture-second-distinguishable",
                other => panic!("unexpected browser case {other}"),
            };
            assert!(
                html.contains(marker),
                "{} must render its named scenario",
                case["name"]
            );
        }
        assert_eq!(
            send_raw(fixture.app(), get("/__ui-fixtures/manifest", None))
                .await
                .0,
            StatusCode::NOT_FOUND,
            "fixture routes are opt-in test utilities, never production routes"
        );
    }

    const DEVICE_UI_CASES: &[(&str, &str)] = &[
        ("panel-normal", "panel"),
        ("panel-first", "panel"),
        ("panel-empty", "panel"),
        ("panel-unreadable", "panel"),
        ("panel-bounded-long", "panel"),
        ("activity-working", "activity"),
        ("activity-refused", "activity"),
        ("activity-closed-failure", "activity"),
        ("activity-aborted", "activity"),
        ("activity-orphan", "activity"),
        ("activity-no-goal", "activity"),
        ("activity-no-session-empty", "activity"),
        ("activity-no-session-recorded", "activity"),
        ("activity-unreadable", "activity"),
        ("activity-multi-long", "activity"),
    ];
    const UI_FIXTURE_NOW: u64 = 1_790_000_600_000;

    fn device_ui_session(id: &str) -> panel::SessionFacts {
        panel::SessionFacts {
            kind: "pty".into(),
            shell: "/bin/bash".into(),
            id: id.into(),
            holder: "fixture author".into(),
            goal: Some("Make the machine's work understandable 让设备工作清楚可见".into()),
            goal_by: Some("fixture author".into()),
            open_txn: Some("txn-fixture-current".into()),
            open_purpose: Some("Inspect the isolated workspace".into()),
            last_call: Some("files.read".into()),
            last_call_at_ms: Some(UI_FIXTURE_NOW - 1_000),
            alive: true,
        }
    }

    fn device_ui_txn(id: &str, state: &str) -> panel::TxnFact {
        panel::TxnFact {
            id: id.into(),
            purpose: "Inspect the isolated workspace and report observed results".into(),
            author: "fixture author".into(),
            session: Some("session-fixture-current".into()),
            state: state.into(),
            reason: (state == "aborted").then(|| "The attempt did not achieve its purpose".into()),
            started_at_ms: UI_FIXTURE_NOW - 60_000,
            ended_at_ms: (state != "working").then_some(UI_FIXTURE_NOW - 1_000),
            calls: 1,
            failures: 0,
            refusals: 0,
            duration_ms: 59_000,
            retained: true,
        }
    }

    fn device_ui_audit(txn: &str, failed: bool, refused: bool) -> Vec<Value> {
        vec![
            json!({"id":1,"ts_ms":UI_FIXTURE_NOW-2_000,"event":"begin","tool":"runs.exec",
                "decision":if refused {"deny"} else {"allow"},"transport":"loopback","txn":txn,
                "args":"fixture command; isolated temporary workspace"}),
            json!({"id":1,"ts_ms":UI_FIXTURE_NOW-1_000,"event":"end","ok":!failed,
                "error_kind":if refused {"no_grant"} else if failed {"tool_failed"} else {""},
                "transport":"loopback","txn":txn}),
        ]
    }

    // These are facts supplied to the production renderer, never simulated successful backends.
    fn device_ui_panel_case(name: &str) -> panel::PanelFacts {
        let data_dir = "/tmp/summrise-ui-fixture".to_owned();
        let (token_file, token_command) = panel::token_facts(&data_dir);
        let mut facts = panel::PanelFacts {
            version: "2.0.15".into(),
            channel: "canary".into(),
            sha256: "b".repeat(64),
            pid: 4242,
            data_dir,
            mcp_endpoint: "http://127.0.0.1:18080/mcp".into(),
            token_file,
            token_command,
            state: Some(
                json!({"schema":1,"update":{"phase":"idle","to":null,"outcome":null,"lastResultAtMs":null}}),
            ),
            events: Vec::new(),
            audit: Some(device_ui_audit("txn-fixture-current", false, false)),
            audit_truncated: false,
            rendered_at_ms: UI_FIXTURE_NOW,
            console_attempt: None,
            console_url: Some("https://console.example.test".into()),
            sessions: vec![device_ui_session("session-fixture-current")],
            txns: vec![device_ui_txn("txn-fixture-current", "working")],
            panel_token: "f".repeat(64),
        };
        match name {
            "panel-first" => {
                facts.console_url = None;
                facts.sessions.clear();
                facts.txns.clear();
                facts.audit = Some(Vec::new());
            }
            "panel-empty" => {
                facts.sessions.clear();
                facts.txns.clear();
                facts.audit = Some(Vec::new());
            }
            "panel-unreadable" => {
                facts.state = None;
                facts.audit = None;
                facts.sessions.clear();
                facts.txns.clear();
            }
            "panel-bounded-long" => {
                let long = "VeryLongWorkspaceSegment超长工作目录".repeat(12);
                facts.data_dir = format!("/tmp/{long}/summrise");
                (facts.token_file, facts.token_command) = panel::token_facts(&facts.data_dir);
                facts.console_url = Some(format!("https://console.example.test/{long}"));
                facts.sessions[0].goal = Some(long.clone());
                facts.sessions[0].holder = long.clone();
                facts.sessions[0].shell = format!("/tmp/{long}/shell");
                facts.txns[0].purpose = long;
                facts.txns[0].calls = 100;
                facts.txns[0].failures = 8;
                facts.audit = Some(
                    (0..100u64)
                        .flat_map(|id| {
                            let mut pair =
                                device_ui_audit("txn-fixture-current", id % 13 == 0, false);
                            for row in &mut pair {
                                row["id"] = json!(id);
                                row["ts_ms"] = json!(UI_FIXTURE_NOW - 200_000 + id * 1_000);
                            }
                            pair
                        })
                        .collect(),
                );
                facts.audit_truncated = true;
            }
            _ => {}
        }
        facts.events = facts
            .audit
            .as_ref()
            .map(|rows| rows.iter().rev().take(EVENT_TAIL).cloned().collect())
            .unwrap_or_default();
        facts
    }

    fn device_ui_activity_case(name: &str) -> activity::ActivityFacts {
        let txn = "txn-fixture-current";
        let mut facts = activity::ActivityFacts {
            audit: Some(device_ui_audit(txn, false, false)),
            truncated: false,
            now_ms: UI_FIXTURE_NOW,
            boot: 1,
            panel_token: "f".repeat(64),
            txns: vec![device_ui_txn(txn, "working")],
            sessions: vec![device_ui_session("session-fixture-current")],
            selected_txn: Some(txn.into()),
            selected_session: Some("session-fixture-current".into()),
            host: "isolated UI fixture".into(),
            version: "2.0.15".into(),
            channel: "canary".into(),
        };
        match name {
            "activity-refused" => {
                facts.audit = Some(device_ui_audit(txn, true, true));
                facts.txns[0].calls = 0;
                facts.txns[0].refusals = 1;
            }
            "activity-closed-failure" => {
                facts.audit = Some(device_ui_audit(txn, true, false));
                facts.txns[0] = device_ui_txn(txn, "committed");
                facts.txns[0].failures = 1;
                facts.sessions.clear();
            }
            "activity-aborted" => {
                facts.txns[0] = device_ui_txn(txn, "aborted");
            }
            "activity-orphan" => {
                // The HTTP fact reader retains the audit's transaction id after its registry
                // receipt is gone; the renderer does not derive this fact itself.
                facts.txns = vec![panel::TxnFact {
                    id: txn.into(),
                    state: "unknown".into(),
                    started_at_ms: UI_FIXTURE_NOW - 60_000,
                    retained: false,
                    ..panel::TxnFact::default()
                }];
                facts.sessions.clear();
                facts.selected_session = None;
            }
            "activity-no-goal" => {
                facts.sessions[0].goal = None;
                facts.sessions[0].goal_by = None;
            }
            "activity-no-session-empty"
            | "activity-no-session-recorded"
            | "activity-unreadable" => {
                facts.txns.clear();
                facts.sessions.clear();
                facts.selected_session = None;
                facts.selected_txn = None;
                facts.audit = match name {
                    "activity-unreadable" => None,
                    "activity-no-session-empty" => Some(Vec::new()),
                    _ => Some(vec![
                        json!({"id":7,"event":"begin","ts_ms":UI_FIXTURE_NOW-1_000,"tool":"host.facts","decision":"allow","transport":"loopback"}),
                        json!({"id":7,"event":"end","ts_ms":UI_FIXTURE_NOW,"ok":true}),
                    ]),
                };
            }
            "activity-multi-long" => {
                let long =
                    "InspectLongWorkspaceAndExplainObservedResults检查超长工作目录与实际结果"
                        .repeat(10);
                facts.sessions[0].goal = Some(long.clone());
                facts.sessions[0].holder = long.clone();
                facts.txns[0].purpose = long;
                let mut second = device_ui_session("session-fixture-second-distinguishable");
                second.goal = Some("Review the other session's goal 审阅另一会话".into());
                second.open_txn = None;
                second.open_purpose = None;
                facts.sessions.push(second);
                let mut old = device_ui_txn("txn-fixture-earlier", "committed");
                old.failures = 1;
                old.started_at_ms -= 120_000;
                facts.txns.push(old);
                facts.audit.as_mut().unwrap().extend(
                    device_ui_audit("txn-fixture-earlier", true, false)
                        .into_iter()
                        .map(|mut row| {
                            row["id"] = json!(2);
                            row
                        }),
                );
            }
            _ => {}
        }
        facts
    }

    fn device_ui_fixture_app(fixture: &Fixture, live: Arc<Mutex<Value>>) -> Router {
        let manifest = axum::routing::get(move || {
            let live = live.lock().expect("fixture manifest").clone();
            async move {
                let cases: Vec<Value> = DEVICE_UI_CASES
                    .iter()
                    .map(|(name, kind)| {
                        json!({
                            "name": name,
                            "url": format!("/__ui-fixtures/{name}"),
                            "kind": kind,
                            "expectedH1Count": 1,
                        })
                    })
                    .collect();
                Json(json!({"cases": cases, "live": live}))
            }
        });
        fixture
            .app()
            .route("/__ui-fixtures/manifest", manifest)
            .route(
                "/__ui-fixtures/:name",
                axum::routing::get(device_ui_fixture_page),
            )
    }

    async fn device_ui_fixture_page(
        axum::extract::Path(name): axum::extract::Path<String>,
    ) -> Response {
        let Some((_, kind)) = DEVICE_UI_CASES.iter().find(|(case, _)| *case == name) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let html = if *kind == "panel" {
            panel::page(&device_ui_panel_case(&name))
        } else {
            activity::page(&device_ui_activity_case(&name))
        };
        Html(html.into_string()).into_response()
    }

    /// Abort only the temporary listener this test created, including on a failed assertion.
    struct DeviceUiServer(PanelServer);

    impl Drop for DeviceUiServer {
        fn drop(&mut self) {
            self.0._handle.abort();
        }
    }

    async fn device_ui_close_sessions(server: &PanelServer, panel_token: &str) {
        let http = client();
        let response = http
            .get(format!("{}/api/terminal", server.base_url))
            .send()
            .await
            .expect("fixture session list");
        let sessions: Value =
            serde_json::from_str(&response.text().await.expect("fixture sessions text"))
                .expect("fixture sessions JSON");
        for session in sessions["sessions"].as_array().expect("fixture sessions") {
            let id = session["id"].as_str().expect("fixture session id");
            let closed = http
                .post(format!("{}/api/terminal/{id}/close", server.base_url))
                .header(PANEL_TOKEN_HEADER, panel_token)
                .send()
                .await
                .expect("temporary session closes");
            assert_eq!(
                closed.status(),
                StatusCode::OK,
                "only an owned fixture session is closed"
            );
        }
    }

    struct DeviceUiSeedTask(tokio::task::JoinHandle<Value>);

    impl Drop for DeviceUiSeedTask {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    async fn device_ui_seed_task<F>(
        server: &PanelServer,
        panel_token: &str,
        seed: F,
    ) -> Result<Value, tokio::task::JoinError>
    where
        F: std::future::Future<Output = Value> + Send + 'static,
    {
        let mut task = DeviceUiSeedTask(tokio::spawn(seed));
        let result = (&mut task.0).await;
        if result.is_err() {
            // Keep the owned listener alive until all partially seeded sessions have closed.
            device_ui_close_sessions(server, panel_token).await;
        }
        result
    }

    #[tokio::test]
    async fn device_ui_seed_failure_closes_its_owned_sessions() {
        let install_token = mint_panel_token();
        let fixture = fixture("device-ui-seed-failure", Some(&install_token));
        let server = DeviceUiServer(PanelServer::start(fixture.app()).await);
        let http = client();
        let panel_token = served_token(&http, &server.0).await;
        let base_url = server.0.base_url.clone();
        let failed = device_ui_seed_task(&server.0, &panel_token, async move {
            let mut mcp = McpClient::start(&base_url, &install_token).await;
            mcp.call("terminal.create", json!({"kind":"pty"})).await;
            panic!("controlled owned browser-seed preparation failure");
        })
        .await;
        let (_, text) = get_text(&http, &format!("{}/api/terminal", server.0.base_url)).await;
        let after_failure: Value = serde_json::from_str(&text).unwrap();
        device_ui_close_sessions(&server.0, &panel_token).await;
        assert!(failed.unwrap_err().is_panic());
        assert_eq!(
            after_failure["sessions"],
            json!([]),
            "failed preparation must close its actual temporary sessions before returning"
        );
    }

    async fn device_ui_seed(server: &PanelServer, install_token: &str) -> Value {
        let panel_token = served_token(&client(), server).await;
        let base_url = server.base_url.clone();
        let install_token = install_token.to_owned();
        match device_ui_seed_task(server, &panel_token, async move {
            device_ui_seed_calls(&base_url, &install_token).await
        })
        .await
        {
            Ok(seed) => seed,
            Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
            Err(error) => panic!("owned browser seed task failed: {error}"),
        }
    }

    async fn device_ui_seed_calls(base_url: &str, install_token: &str) -> Value {
        let mut mcp = McpClient::start(base_url, install_token).await;
        let mut session_ids = Vec::new();
        let mut txn_ids = Vec::new();
        for (index, goal) in [
            "Inspect the isolated browser workspace",
            "Review the second session's observations",
        ]
        .iter()
        .enumerate()
        {
            let created = mcp
                .call("terminal.create", json!({"kind":"pty","goal":goal}))
                .await;
            let id = created["id"]
                .as_str()
                .expect("actual temporary session")
                .to_owned();
            let begun = mcp.call("txn.begin", json!({"session":id,"purpose":format!("Prepare browser acceptance session {}",index+1)})).await;
            let txn = begun["txn"]
                .as_str()
                .expect("actual retained transaction")
                .to_owned();
            // The install door names the terminal session explicitly for session-bound work.
            mcp.call("terminal.read", json!({"id":id,"cursor":0,"txn":txn}))
                .await;
            if index == 0 {
                let receipt = mcp.call("txn.commit", json!({"txn":txn})).await;
                assert_eq!(receipt["state"], "committed");
            }
            session_ids.push(id);
            txn_ids.push(txn);
        }
        // A machine call from the install door requires its own device-scope transaction.
        let begun = mcp
            .call(
                "txn.begin",
                json!({"purpose":"Prepare the browser's actual machine-run detail"}),
            )
            .await;
        let run_txn = begun["txn"]
            .as_str()
            .expect("actual device-scope transaction")
            .to_owned();
        let run = mcp
            .call(
                "runs.exec",
                json!({"command":"echo SUMMRISE_UI_SEEDED_RUN","shell":true,"txn":run_txn}),
            )
            .await;
        let run_id = run["id"].as_str().expect("actual local run id").to_owned();
        let receipt = mcp.call("txn.commit", json!({"txn":run_txn})).await;
        assert_eq!(receipt["state"], "committed");
        json!({
            "panel":"/panel","root":"/","activity":"/activity",
            "sessionIds":session_ids,"txnIds":txn_ids,"runId":run_id,"runTxnId":run_txn,
            "receipt":format!("/activity?session={}&txn={}",session_ids[0],txn_ids[0]),
            "invalidSerialPort":"/__ui-fixtures/invalid-serial-port",
            "enrollConsoleUrl":"not-a-url","enrollCode":"e".repeat(64),
            "ptyMarker":format!("SUMMRISE_UI_{}", &mint_panel_token()[..16]),
            "controlledFailures":"CDP response interception only; no serial hardware or remote console"
        })
    }

    #[tokio::test]
    async fn device_ui_browser_seed_preserves_real_transaction_scopes() {
        let install_token = mint_panel_token();
        let fixture = fixture("device-ui-seed-scopes", Some(&install_token));
        fixture.saved_state();
        let (app, txns) = fixture.app_and_txns();
        let server = DeviceUiServer(PanelServer::start(app).await);
        let http = client();
        let panel_token = served_token(&http, &server.0).await;
        let seed = device_ui_seed(&server.0, &install_token).await;
        let (_, text) = get_text(&http, &format!("{}/api/terminal", server.0.base_url)).await;
        let sessions: Value = serde_json::from_str(&text).expect("actual seeded session list");
        let ids = seed["sessionIds"].as_array().expect("two seeded sessions");
        let txn_ids = seed["txnIds"]
            .as_array()
            .expect("two session-bound records");
        assert_eq!(ids.len(), 2);
        assert_eq!(txn_ids.len(), 2);
        let records: Vec<_> = txn_ids
            .iter()
            .map(|id| {
                txns.get(&TxnId::new(id.as_str().unwrap()))
                    .expect("actual session record")
            })
            .collect();
        let run_txn = txns
            .get(&TxnId::new(
                seed["runTxnId"]
                    .as_str()
                    .expect("actual machine run transaction"),
            ))
            .expect("retained machine receipt");
        let run = http
            .get(format!(
                "{}/api/runs/{}",
                server.0.base_url,
                seed["runId"].as_str().expect("actual run id")
            ))
            .header(PANEL_TOKEN_HEADER, &panel_token)
            .send()
            .await
            .expect("actual run detail");
        let run_status = run.status();
        let run: Value = serde_json::from_str(&run.text().await.unwrap()).unwrap();
        let audit: Vec<Value> = fs::read_to_string(fixture.config.data.join("events.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let (activity_status, activity) = get_text(
            &http,
            &format!(
                "{}/activity?session={}",
                server.0.base_url,
                ids[0].as_str().unwrap()
            ),
        )
        .await;
        device_ui_close_sessions(&server.0, &panel_token).await;
        for (index, goal) in [
            "Inspect the isolated browser workspace",
            "Review the second session's observations",
        ]
        .iter()
        .enumerate()
        {
            let id = ids[index].as_str().unwrap();
            let session = sessions["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["id"] == id)
                .expect("actual live seeded session");
            assert_eq!(session["goal"]["text"], *goal);
            assert_eq!(records[index].session().map(SessionRef::as_str), Some(id));
            assert_eq!(records[index].goal_at_start().map(Goal::text), Some(*goal));
            assert_eq!(
                records[index].state(),
                if index == 0 {
                    TxnState::Committed
                } else {
                    TxnState::Working
                }
            );
            assert_eq!(
                (
                    records[index].calls(),
                    records[index].failures(),
                    records[index].refusals()
                ),
                (1, 0, 0)
            );
        }
        assert!(run_txn.is_device_scope());
        assert_eq!(run_txn.state(), TxnState::Committed);
        assert_eq!(
            (run_txn.calls(), run_txn.failures(), run_txn.refusals()),
            (1, 0, 0)
        );
        assert_eq!(run_status, StatusCode::OK);
        assert_eq!(run["run"]["id"], seed["runId"]);
        assert_eq!(activity_status, StatusCode::OK);
        assert!(
            activity.contains(&format!("data-run=\"{}\"", seed["runId"].as_str().unwrap())),
            "the actual machine run remains in the selected session page's global disclosure"
        );
        assert!(audit.iter().any(|row| row["event"] == "begin"
            && row["tool"] == "runs.exec"
            && row["txn"] == seed["runTxnId"]));
        assert!(
            audit.len() < panel::TIMELINE_LINES,
            "the actual run stays in the global browser feed window"
        );
        assert_eq!(
            seed["receipt"],
            format!(
                "/activity?session={}&txn={}",
                ids[0].as_str().unwrap(),
                txn_ids[0].as_str().unwrap()
            )
        );
    }

    // This gate catches backend/view drift: its browser must type into a real PTY, navigate actual
    // session/receipt queries and exercise the existing protected panel forms. Failure fixtures are
    // separately named in the manifest and must never be reported as remote operator successes.
    #[tokio::test]
    #[ignore = "browser runtime required; CI invokes device_ui_browser_acceptance"]
    async fn device_ui_browser_acceptance() {
        let install_token = mint_panel_token();
        let fixture = fixture("device-ui-browser", Some(&install_token));
        fixture.saved_state();
        let live = Arc::new(Mutex::new(
            json!({"panel":"/panel","root":"/","activity":"/activity"}),
        ));
        let server = DeviceUiServer(
            PanelServer::start(device_ui_fixture_app(&fixture, Arc::clone(&live))).await,
        );
        let http = client();
        let panel_token = served_token(&http, &server.0).await;
        let seed = device_ui_seed(&server.0, &install_token).await;
        *live.lock().expect("browser live handoff") = seed;

        // Verify security on the actual router before the browser touches any controls.
        for path in ["/api/terminal/create", "/api/panel/update", "/panel/enroll"] {
            let request = http.post(format!("{}{path}", server.0.base_url));
            let response = if path == "/api/terminal/create" {
                request
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(json!({"kind":"pty"}).to_string())
                    .send()
                    .await
            } else {
                request.form(&[("panel_token", "")]).send().await
            }
            .expect("token refusal answers");
            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "protected action {path}"
            );
        }
        let response = http
            .get(format!("{}/api/activity", server.0.base_url))
            .send()
            .await
            .expect("activity refuses missing token");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let out = std::env::var_os("SUMMRISE_DEVICE_UI_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("CARGO_TARGET_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(std::env::temp_dir)
                    .join("device-ui-browser-acceptance")
            });
        fs::create_dir_all(&out).expect("owned browser evidence directory");
        let script =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/device-ui-browser.mjs");
        let result = tokio::time::timeout(
            Duration::from_secs(480),
            tokio::process::Command::new("node")
                .arg(script)
                .arg("--base-url")
                .arg(&server.0.base_url)
                .arg("--out")
                .arg(&out)
                .kill_on_drop(true)
                .output(),
        )
        .await;

        // Close only this fixture's sessions, even when the browser reports a failure.
        device_ui_close_sessions(&server.0, &panel_token).await;
        let result = result
            .expect("device UI browser gate exceeded eight minutes")
            .expect("Node must be installed for the browser gate");
        fs::write(out.join("driver.stdout.log"), &result.stdout).expect("driver stdout evidence");
        fs::write(out.join("driver.stderr.log"), &result.stderr).expect("driver stderr evidence");
        fs::write(
            out.join("driver.exit"),
            format!("{}\n", result.status.code().unwrap_or(-1)),
        )
        .expect("driver exit evidence");
        assert!(
            result.status.success(),
            "device UI browser gate failed ({:?}); evidence: {}",
            result.status.code(),
            out.display()
        );
        println!("Device UI browser evidence: {}", out.display());
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
        crate::ensure_state(&fixture.store, true, 1_794_045_000_000)
            .expect("startup writes the release facts");

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
        crate::ensure_state(&fixture.store, true, 1_794_045_000_000)
            .expect("a clean start writes the document");
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

    // ── /api/status: the document the desktop tray's poll parses ────────────────────────────────────
    //
    // The tray (`device/desktop-electron/src/main.ts:878`) fetches this path every 30 s and hands the
    // raw body to the shell policy's `refreshTrayHealth`. The field names are the policy's, not this
    // file's taste: `release`, `uptime_secs`, `live_sessions` are what its committed wasm reads
    // (`test/shell-policy-wasm.test.mjs` pins the same literal from the JavaScript side).

    #[tokio::test]
    async fn the_status_route_answers_the_lifecycle_document_the_tray_parses() {
        let fixture = fixture("status-tray", Some(&token_hex()));

        // The tray sends the install's bearer when it has one (`authHeaders()`); the route is tokenless
        // like `/api/health`, so a bearer must not change the answer, and its absence must not either.
        let (status, body) = send(fixture.app(), get("/api/status", Some(&token_hex()))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            send(fixture.app(), get("/api/status", None)).await.0,
            StatusCode::OK,
            "a tokenless loopback poll is the tray with no token configured"
        );

        assert_eq!(body["status"], "ok");
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(
            body["release"], body["version"],
            "the policy reads `release`"
        );
        assert_eq!(body["channel"], "stable");
        assert_eq!(body["pid"].as_u64(), Some(u64::from(std::process::id())));
        let uptime = body["uptime_secs"]
            .as_u64()
            .expect("a fresh process has an uptime");
        assert!(
            uptime <= 2,
            "no state file means this process started now: {body}"
        );
        assert_eq!(body["live_sessions"], 0);

        let mut keys: Vec<&str> = body
            .as_object()
            .expect("a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "channel",
                "live_sessions",
                "pid",
                "release",
                "status",
                "uptime_secs",
                "version"
            ],
            "the tray's policy reads these seven and nothing else is promised: {body}"
        );
    }

    #[tokio::test]
    async fn the_status_route_reads_the_start_time_and_sessions_the_state_document_carries() {
        let fixture = fixture("status-facts", None);
        let mut state = fixture.store.load().expect("the fixture state loads");
        let started = chrono::Utc::now() - chrono::Duration::seconds(204);
        state.service.started_at = started.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        for (id, row_state) in [
            ("session-1", SessionState::Attached),
            ("ssh-1", SessionState::Detached),
            ("serial-old", SessionState::Reaped),
            ("pty-old", SessionState::Closed),
        ] {
            state.sessions.push(SessionRow {
                id: id.to_owned(),
                kind: SessionKind::Pty,
                state: row_state,
            });
        }
        fixture.store.save(&state).expect("the fixture state saves");

        let (status, body) = send(fixture.app(), get("/api/status", None)).await;

        assert_eq!(status, StatusCode::OK);
        let uptime = body["uptime_secs"]
            .as_u64()
            .unwrap_or_else(|| panic!("the recorded start is parsed: {body}"));
        assert!(
            (203..=206).contains(&uptime),
            "about 204 s since the recorded start: {body}"
        );
        assert_eq!(
            body["live_sessions"], 2,
            "a reaped or closed row is not a live session: {body}"
        );
    }

    #[tokio::test]
    async fn the_status_route_refuses_when_the_state_document_is_unreadable() {
        let fixture = fixture("status-degraded", Some(&token_hex()));
        crate::ensure_state(&fixture.store, true, 1_794_045_000_000)
            .expect("a clean start writes the document");
        fs::write(fixture.store.state_path(), "{ not a document")
            .expect("the corrupt fixture is written");

        let (status, body) = send(fixture.app(), get("/api/status", None)).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "degraded");
        assert!(
            body.get("release").is_none() && body.get("version").is_none(),
            "a refusal must not carry a version the tray would print: {body}"
        );
        let reason = body["reason"].as_str().expect("the failure is named");
        assert!(
            reason.starts_with("state: ") && reason.contains("state file is corrupt"),
            "{reason}"
        );
    }

    #[tokio::test]
    async fn a_status_route_with_an_unreadable_start_time_still_answers_the_rest() {
        let fixture = fixture("status-no-start", None);
        let mut state = fixture.store.load().expect("the fixture state loads");
        state.service.started_at = "not a time".to_owned();
        fixture.store.save(&state).expect("the fixture state saves");

        let (status, body) = send(fixture.app(), get("/api/status", None)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        assert!(
            body["uptime_secs"].is_null(),
            "an unreadable start time is omitted, not invented: {body}"
        );
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

    // ---- the panel's enroll form -----------------------------------------------------------

    /// A loopback server that serves a router with `ConnectInfo`, as production does: the panel's
    /// handlers read the peer address, and a oneshot router has no address to report.
    struct PanelServer {
        base_url: String,
        _handle: tokio::task::JoinHandle<()>,
    }

    impl PanelServer {
        async fn start(app: Router) -> PanelServer {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("a loopback listener");
            let address = listener.local_addr().expect("the panel's address");
            let handle = tokio::spawn(async move {
                let _ = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await;
            });
            PanelServer {
                base_url: format!("http://{address}"),
                _handle: handle,
            }
        }
    }

    /// What the mock console has seen, and the tokens it has issued.
    struct ConsoleState {
        tokens: AtomicU64,
        seen: Mutex<Vec<Value>>,
    }

    /// The console's `POST /devices`, as the plan's mock: it echoes the device id the registration
    /// named and issues the next 64-hex token. The signature is `enroll`'s own test; the panel only
    /// has to reach that route through the real enrollment.
    async fn mock_register(
        State(state): State<Arc<ConsoleState>>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        state
            .seen
            .lock()
            .expect("the console's request list")
            .push(body.clone());
        let token = format!("{:064x}", state.tokens.fetch_add(1, Ordering::SeqCst) + 1);

        Json(json!({ "id": body["id"].clone(), "token": token }))
    }

    struct MockConsole {
        base_url: String,
        state: Arc<ConsoleState>,
        _handle: tokio::task::JoinHandle<()>,
    }

    impl MockConsole {
        async fn start() -> MockConsole {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("a loopback listener");
            let address = listener.local_addr().expect("the console's address");
            let state = Arc::new(ConsoleState {
                tokens: AtomicU64::new(0),
                seen: Mutex::new(Vec::new()),
            });
            let console = Router::new()
                .route("/devices", axum::routing::post(mock_register))
                .with_state(Arc::clone(&state));
            let handle = tokio::spawn(async move {
                let _ = axum::serve(listener, console).await;
            });
            MockConsole {
                base_url: format!("http://{address}"),
                state,
                _handle: handle,
            }
        }

        fn requests(&self) -> Vec<Value> {
            self.state
                .seen
                .lock()
                .expect("the console's request list")
                .clone()
        }

        fn token(&self, index: u64) -> String {
            format!("{:064x}", index + 1)
        }
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .expect("a test HTTP client")
    }

    async fn get_text(client: &reqwest::Client, url: &str) -> (StatusCode, String) {
        let response = client.get(url).send().await.expect("the device answers");
        let status = response.status();
        let text = response.text().await.expect("the answer is text");

        (status, text)
    }

    /// The panel token the served page carries, extracted the way the page's own script reads it.
    fn panel_meta(content: &str) -> Option<String> {
        const OPEN: &str = r#"<meta name="summrise-panel" content=""#;
        let start = content.find(OPEN)? + OPEN.len();
        let rest = &content[start..];
        let end = rest.find('"')?;

        Some(rest[..end].to_owned())
    }

    async fn served_token(client: &reqwest::Client, server: &PanelServer) -> String {
        let (status, html) = get_text(client, &server.base_url).await;
        assert_eq!(status, StatusCode::OK);

        panel_meta(&html).expect("the panel carries this boot's token in its meta tag")
    }

    async fn post_enroll(
        client: &reqwest::Client,
        base_url: &str,
        token: Option<&str>,
        body: Value,
    ) -> (StatusCode, Value, String) {
        let mut request = client
            .post(format!("{base_url}/api/enroll"))
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            request = request.header("x-summrise-panel", token);
        }
        let response = request
            .body(serde_json::to_vec(&body).expect("the request body is JSON"))
            .send()
            .await
            .expect("the device answers");
        let status = response.status();
        let text = response.text().await.expect("the answer is text");
        let answer = serde_json::from_str(&text).unwrap_or(Value::Null);

        (status, answer, text)
    }

    fn post_enroll_request(token: Option<&str>, body: Value) -> Request<Body> {
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/enroll")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            request = request.header("x-summrise-panel", token);
        }

        request
            .body(Body::from(body.to_string()))
            .expect("the request builds")
    }

    #[tokio::test]
    async fn the_panel_page_carries_a_fresh_token_and_the_enroll_form() {
        let fixture = fixture("panel-page", None);
        let client = client();
        // Two separately built routers: each one mints its own boot token.
        let first = PanelServer::start(fixture.app()).await;
        let second = PanelServer::start(fixture.app()).await;

        let (status, html) = get_text(&client, &first.base_url).await;
        assert_eq!(status, StatusCode::OK);
        let token = panel_meta(&html).expect("GET / carries the panel token meta tag");
        assert_eq!(token.len(), 64, "the token is 32 bytes as hex: {token}");
        assert!(
            token
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "the token is lowercase hex: {token}"
        );
        // The form is a plain POST now: the page carries the token in a hidden field because a form
        // cannot set `x-summrise-panel`, and the answer is a redirect with the outcome.
        assert!(
            html.contains("action=\"/panel/enroll\""),
            "the enroll form posts to the panel's own route"
        );
        assert!(
            html.contains("name=\"panel_token\""),
            "the form carries this boot's token in a field"
        );
        for id in [
            "enroll-console",
            "enroll-code",
            "enroll-force",
            "enroll-plaintext",
            "enroll-submit",
            "enroll-status",
            "local-endpoint",
            "local-token-file",
            "local-token-command",
            "local-config",
            "copy-local",
            "local-copy-status",
        ] {
            assert!(
                html.contains(&format!("id=\"{id}\"")),
                "the form carries #{id}: {html}"
            );
        }
        assert!(
            html.contains(r#"role="status""#),
            "the status line is announced to a screen reader"
        );
        assert!(
            html.contains(r#"src="/panel/panel-terminal.js""#),
            "the terminal's glue is an asset now; the page no longer carries its own script"
        );
        assert!(
            html.contains("Connect a client"),
            "the panel explains the local MCP path"
        );
        assert!(
            html.contains("Copy local MCP config"),
            "the local config is copyable"
        );
        assert!(
            !html.contains("__PANEL_TOKEN__"),
            "the served page carries the token, not the placeholder"
        );

        let (other_status, other_html) = get_text(&client, &second.base_url).await;
        assert_eq!(other_status, StatusCode::OK);
        let other = panel_meta(&other_html).expect("the second server carries its own token");
        assert_ne!(token, other, "a second boot gets a second token");
    }

    #[tokio::test]
    async fn the_panel_lists_the_live_terminal_sessions() {
        let fixture = fixture("panel-terminal", None);
        let client = client();
        let server = PanelServer::start(fixture.app()).await;

        let (status, html) = get_text(&client, &server.base_url).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            html.contains("id=\"terminal-sessions\""),
            "the panel's terminal card carries the live list"
        );

        let (status, body) = get_text(&client, &format!("{}/api/terminal", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        let list: serde_json::Value =
            serde_json::from_str(&body).expect("the terminal list is JSON");
        assert_eq!(list["sessions"], serde_json::json!([]));
    }

    /// The panel's Timeline is built from `panel_facts`, and an orphan there is a `retained: false`
    /// fact: the page must say the receipt is gone and print none of the zero counters it carries.
    #[tokio::test]
    async fn the_panel_timeline_says_an_orphan_receipt_is_not_retained() {
        let fixture = fixture("panel-orphan-receipt", None);
        write_audit(&fixture, &orphan_audit());
        let client = client();
        let server = PanelServer::start(fixture.app()).await;

        let (status, html) = get_text(&client, &format!("{}/panel", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            html.contains("fix the parser"),
            "the claim from the audit's begin row is still in the tail: {html}"
        );
        assert!(
            html.contains("receipt not retained"),
            "the production fact for an orphan says its receipt is gone: {html}"
        );
        assert!(
            !html.contains("0 calls · 0 failures · 0 refused"),
            "the unretained fact's zero counters are not printed as a receipt: {html}"
        );
    }

    /// The ledger is built from the same facts: an orphan keeps the claim its `txn.begin` row
    /// carries in this tail, while its receipt (counts and declared ending) is not retained.
    #[tokio::test]
    async fn the_activity_ledger_shows_an_orphans_claim_but_not_its_receipt() {
        let fixture = fixture("activity-orphan-purpose", None);
        write_audit(&fixture, &orphan_audit());
        let client = client();
        let server = PanelServer::start(fixture.app()).await;

        let (status, html) = get_text(&client, &format!("{}/activity", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        // The orphan is not a session's work, so the page draws it in the collapsed secondary
        // section; the claim and the absent receipt are read out of that section.
        let start = html
            .find(r#"class="older-activity""#)
            .expect("the secondary section is drawn");
        let end = html
            .find(r#"class="transcript""#)
            .expect("the call list follows the secondary section");
        let ledger = &html[start..end];

        assert!(
            ledger.contains("fix the parser") && !ledger.contains("(purpose not retained)"),
            "the claim from the audit's own begin row survives the restart: {ledger}"
        );
        assert!(
            ledger.contains("receipt not retained")
                && !ledger.contains("Committed.")
                && !ledger.contains("Aborted."),
            "a missing receipt declares no ending: {ledger}"
        );
        assert!(
            ledger.contains(r#"data-group="unscoped""#)
                && !ledger.contains(r#"data-group="machine""#),
            "an orphan's scope is unknown, so it is not placed by guess: {ledger}"
        );
    }

    /// When the `txn.begin` row has left the bounded tail, nothing here can report the claim: the
    /// ledger says the purpose is not retained, and still invents no receipt.
    #[tokio::test]
    async fn an_orphan_whose_begin_left_the_tail_says_its_purpose_is_not_retained() {
        let fixture = fixture("activity-orphan-begin-gone", None);
        let at = 1_761_566_400_000u64;
        // The id arrives on a call row; its `txn.begin` row is older than this tail.
        write_audit(
            &fixture,
            &[
                json!({
                    "id": 2, "ts_ms": at + 1_000, "event": "begin", "tool": "files.read",
                    "decision": "allow", "transport": "panel", "txn": "txn-gone"
                }),
                json!({
                    "id": 2, "ts_ms": at + 1_100, "event": "end", "ok": true,
                    "transport": "panel", "txn": "txn-gone"
                }),
            ],
        );
        let client = client();
        let server = PanelServer::start(fixture.app()).await;

        let (status, html) = get_text(&client, &format!("{}/activity", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        // The orphan is not a session's work, so the page draws it in the collapsed secondary
        // section; the claim and the absent receipt are read out of that section.
        let start = html
            .find(r#"class="older-activity""#)
            .expect("the secondary section is drawn");
        let end = html
            .find(r#"class="transcript""#)
            .expect("the call list follows the secondary section");
        let ledger = &html[start..end];

        assert!(
            ledger.contains("(purpose not retained)"),
            "a claim outside the tail is not invented: {ledger}"
        );
        assert!(
            ledger.contains("receipt not retained"),
            "the receipt is still not retained when the claim is gone: {ledger}"
        );
        assert!(
            !ledger.contains("Committed.") && !ledger.contains("Aborted."),
            "a missing receipt declares no ending: {ledger}"
        );
    }

    /// A panel operation audits `args: None`, so the session card's "last call" cannot come from a
    /// `session=` argument. It is attributed by the row's `txn` to the registry's session.
    #[tokio::test]
    async fn the_session_card_finds_a_call_the_panel_wrote() {
        let fixture = fixture("panel-session-last-call", None);
        let client = client();
        let server = PanelServer::start(fixture.app()).await;
        let token = served_token(&client, &server).await;

        let response = client
            .post(format!("{}/api/terminal/create", server.base_url))
            .header("x-summrise-panel", &token)
            .header("content-type", "application/json")
            .body(json!({ "kind": "pty" }).to_string())
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::OK);
        let created: Value =
            serde_json::from_str(&response.text().await.expect("the create answer is text"))
                .expect("the create answer is JSON");
        let id = created["id"].as_str().expect("a session id").to_owned();

        let response = client
            .post(format!("{}/api/terminal/{id}/write", server.base_url))
            .header("x-summrise-panel", &token)
            .header("content-type", "application/json")
            .body(json!({ "data": "echo panel\n" }).to_string())
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::OK);

        let (status, html) = get_text(&client, &format!("{}/panel", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        let marker = format!(r#"class="session-card" data-session="{id}""#);
        let start = html.find(&marker).expect("the panel lists the session");
        let end = start
            + html[start..]
                .find("</article>")
                .expect("the session card closes");
        let card = &html[start..end];

        assert!(
            card.contains("Last call") && card.contains("terminal.write"),
            "the panel's own write is the session's last call: {card}"
        );
        assert!(
            !card.contains("No call recorded in this window."),
            "the card no longer claims the window holds no call: {card}"
        );
    }

    /// One orphan transaction as the audit writes it: a `txn.begin` row whose end mints the id, then
    /// a call that rides it. No registry record exists, so `txn_facts` answers `retained: false`.
    fn orphan_audit() -> Vec<Value> {
        let at = 1_761_566_400_000u64;
        vec![
            json!({
                "id": 1, "ts_ms": at, "event": "begin", "tool": "txn.begin",
                "decision": "allow", "transport": "panel", "args": "purpose=fix the parser"
            }),
            json!({
                "id": 1, "ts_ms": at + 5, "event": "end", "ok": true,
                "outcome": "started", "transport": "panel", "txn": "txn-gone"
            }),
            json!({
                "id": 2, "ts_ms": at + 1_000, "event": "begin", "tool": "files.read",
                "decision": "allow", "transport": "panel", "txn": "txn-gone"
            }),
            json!({
                "id": 2, "ts_ms": at + 1_100, "event": "end", "ok": true,
                "transport": "panel", "txn": "txn-gone"
            }),
        ]
    }

    #[tokio::test]
    async fn the_panel_terminal_operations_need_the_panel_token_and_work() {
        let fixture = fixture("panel-terminal-ops", None);
        let client = client();
        let server = PanelServer::start(fixture.app()).await;
        let token = served_token(&client, &server).await;

        // Without the token: refused before any session exists.
        let response = client
            .post(format!("{}/api/terminal/create", server.base_url))
            .header("content-type", "application/json")
            .body(json!({ "kind": "pty" }).to_string())
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // With it: a pty, a write, a read and a close.
        let response = client
            .post(format!("{}/api/terminal/create", server.base_url))
            .header("x-summrise-panel", &token)
            .header("content-type", "application/json")
            .body(json!({ "kind": "pty" }).to_string())
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::OK);
        let text = response.text().await.expect("the create answer");
        let created: Value = serde_json::from_str(&text).expect("the create answer is JSON");
        let id = created["id"].as_str().expect("a session id").to_owned();

        // The panel is a door of its own: the session says so, and the panel's own list answers it.
        let (status, body) = get_text(&client, &format!("{}/api/terminal", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        let list: Value = serde_json::from_str(&body).expect("the terminal list is JSON");
        let session = list["sessions"]
            .as_array()
            .and_then(|sessions| sessions.iter().find(|session| session["id"] == id.as_str()))
            .unwrap_or_else(|| panic!("the panel lists the session it opened: {list}"));
        assert_eq!(
            session["holder"],
            json!({ "kind": "panel", "token_id": null }),
            "a session opened at the panel is held by the panel door: {session}"
        );
        assert!(
            session["goal"].is_null(),
            "the panel declares no goal when it opens a session: {session}"
        );

        for (method, path, body) in [
            (
                "write",
                format!("/api/terminal/{id}/write"),
                Some(json!({ "data": "exit\n" })),
            ),
            ("read", format!("/api/terminal/{id}/read?cursor=0"), None),
            (
                "close",
                format!("/api/terminal/{id}/close"),
                Some(json!({})),
            ),
        ] {
            let request = if method == "read" {
                client.get(format!("{}{path}", server.base_url))
            } else {
                client.post(format!("{}{path}", server.base_url))
            };
            let request = match body {
                Some(body) => request
                    .header("content-type", "application/json")
                    .body(body.to_string()),
                None => request,
            };
            let response = request
                .header("x-summrise-panel", &token)
                .send()
                .await
                .expect("the device answers");
            assert_eq!(response.status(), StatusCode::OK, "{method} {path}");
        }

        // The panel's terminal operations are audited like an MCP call: a session opened at the
        // device's own page must be findable in events.jsonl, or the log answers "what ran here"
        // with everything except what the person standing at the machine did.
        let lines: Vec<Value> = std::fs::read_to_string(fixture.config.data.join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("an audit line is JSON"))
            .collect();
        let panel_ops: Vec<&Value> = lines
            .iter()
            .filter(|line| {
                line["transport"] == "panel"
                    && line["decision"] == "allow"
                    && line["tool"]
                        .as_str()
                        .is_some_and(|tool| tool.starts_with("terminal."))
            })
            .collect();
        assert_eq!(
            panel_ops.len(),
            4,
            "create, write, read and close each wrote their authorization line: {lines:?}"
        );
        // Creating the session is the Bootstrap call: it has no transaction because it is what
        // makes one possible. Every other panel operation opens a transaction of its own, and the
        // request that opened it ends it before answering.
        assert!(
            panel_ops[0]["txn"].is_null() && panel_ops[0]["bootstrap"] == true,
            "the create is the Bootstrap call: {lines:?}"
        );
        let mut endings = 0;
        for line in &panel_ops[1..] {
            let txn = line["txn"]
                .as_str()
                .unwrap_or_else(|| panic!("a panel operation names its transaction: {lines:?}"));
            let ending = lines
                .iter()
                .find(|other| {
                    other["tool"] == "txn.commit"
                        && other["txn"] == json!(txn)
                        && other["event"] == "begin"
                })
                .and_then(|begin| {
                    lines
                        .iter()
                        .find(|end| end["event"] == "end" && end["id"] == begin["id"])
                })
                .unwrap_or_else(|| {
                    panic!("the request that opened {txn} also recorded its ending: {lines:?}")
                });
            assert_eq!(
                ending["outcome"], "committed",
                "the panel's ending declares the state: {ending}"
            );
            endings += 1;
        }
        assert_eq!(
            endings, 3,
            "write, read and close each ended their own record"
        );
        assert_eq!(panel_ops[0]["tool"], "terminal.create", "{lines:?}");
        assert_eq!(panel_ops[1]["tool"], "terminal.write", "{lines:?}");
        assert_eq!(panel_ops[2]["tool"], "terminal.read", "{lines:?}");
        assert_eq!(panel_ops[3]["tool"], "terminal.close", "{lines:?}");
        assert!(
            lines.iter().all(|line| line["token_id"].is_null()),
            "a panel call carries no token id: {lines:?}"
        );
    }

    /// **THE PANEL'S TRANSACTION IS ITS OWN.** A panel operation is gated like any other machine
    /// call, and the panel must not borrow a client's record: it begins `by: panel` for the session
    /// it acts on, writes the id into every audit row, and commits it when it closes the session.
    ///
    /// Ruling 2 makes that transaction **session-bound**, not device-scope: every panel operation
    /// after the create acts on a session, and the gate refuses a device-scope transaction on a
    /// session call (`txn_scope_mismatch`). "Of its own" is the property this test pins: the author
    /// is the panel door, not the caller that opened the session.
    #[tokio::test]
    async fn the_panel_uses_a_session_bound_transaction_of_its_own() {
        let fixture = fixture("panel-txn", None);
        let (app, txns) = fixture.app_and_txns();
        let client = client();
        let server = PanelServer::start(app).await;
        let token = served_token(&client, &server).await;

        let response = client
            .post(format!("{}/api/terminal/create", server.base_url))
            .header("x-summrise-panel", &token)
            .header("content-type", "application/json")
            .body(json!({ "kind": "pty" }).to_string())
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::OK);
        let created: Value = serde_json::from_str(&response.text().await.unwrap())
            .expect("the create answer is JSON");
        let id = created["id"].as_str().expect("a session id").to_owned();

        for (method, path, body) in [
            (
                "write",
                format!("/api/terminal/{id}/write"),
                Some(json!({ "data": "echo panel\n" })),
            ),
            ("read", format!("/api/terminal/{id}/read?cursor=0"), None),
            (
                "close",
                format!("/api/terminal/{id}/close"),
                Some(json!({})),
            ),
        ] {
            let request = if method == "read" {
                client.get(format!("{}{path}", server.base_url))
            } else {
                client.post(format!("{}{path}", server.base_url))
            };
            let request = match body {
                Some(body) => request
                    .header("content-type", "application/json")
                    .body(body.to_string()),
                None => request,
            };
            let response = request
                .header("x-summrise-panel", &token)
                .send()
                .await
                .expect("the device answers");
            assert_eq!(response.status(), StatusCode::OK, "{method} {path}");
        }

        let lines: Vec<Value> = std::fs::read_to_string(fixture.config.data.join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("an audit line is JSON"))
            .collect();
        let panel_ops: Vec<&Value> = lines
            .iter()
            .filter(|line| {
                line["transport"] == "panel"
                    && line["decision"] == "allow"
                    && line["tool"]
                        .as_str()
                        .is_some_and(|tool| tool.starts_with("terminal."))
            })
            .collect();
        assert_eq!(panel_ops.len(), 4, "{lines:?}");
        assert!(
            panel_ops[0]["txn"].is_null(),
            "the create is Bootstrap and rides no transaction: {lines:?}"
        );
        for line in &panel_ops[1..] {
            let txn_id = line["txn"]
                .as_str()
                .unwrap_or_else(|| panic!("each panel operation names its transaction: {lines:?}"));
            let record = txns
                .get(&TxnId::new(txn_id))
                .unwrap_or_else(|| panic!("the transaction is in the shared registry: {txn_id}"));
            assert_eq!(
                record.by(),
                &Holder::panel(),
                "the author is the panel door, not the client"
            );
            assert_eq!(
                record.session().map(SessionRef::as_str),
                Some(id.as_str()),
                "the transaction is bound to the session it acted on"
            );
            assert!(
                record.purpose().starts_with("your panel on"),
                "the purpose is the panel's own: {}",
                record.purpose()
            );
            assert_eq!(
                record.state(),
                TxnState::Committed,
                "the request committed the record it opened"
            );
            assert_eq!(
                record.calls(),
                1,
                "each panel request is one call in its own unit"
            );
        }
    }

    /// **AN UNAUDITED PANEL CALL LEAVES NO WORKING RECORD.** `panel_call_begin` opens the record
    /// before it writes the authorization line; when that line cannot be written the call must not
    /// run, so the record must not stay working either. There is no audit row to pair (the write is
    /// what failed), so the registry is the evidence: nothing working, and the record that was
    /// opened is aborted with the reason.
    #[tokio::test]
    async fn an_unwritable_panel_audit_leaves_no_working_transaction() {
        let fixture = fixture("panel-audit-unwritable", None);
        let (app, txns) = fixture.app_and_txns();
        let client = client();
        let server = PanelServer::start(app).await;
        let token = served_token(&client, &server).await;

        let response = client
            .post(format!("{}/api/terminal/create", server.base_url))
            .header("x-summrise-panel", &token)
            .header("content-type", "application/json")
            .body(json!({ "kind": "pty" }).to_string())
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::OK);
        let created: Value = serde_json::from_str(&response.text().await.unwrap())
            .expect("the create answer is JSON");
        let id = created["id"].as_str().expect("a session id").to_owned();

        // Make the log unwritable the way an occupied path makes it: `events.jsonl` becomes a
        // directory, so the next append fails and the panel must refuse the call it was opening.
        let audit = fixture.config.data.join("events.jsonl");
        std::fs::remove_file(&audit).expect("the create wrote the audit file");
        std::fs::create_dir(&audit).expect("the audit path is now unwritable");

        let response = client
            .post(format!("{}/api/terminal/{id}/write", server.base_url))
            .header("x-summrise-panel", &token)
            .header("content-type", "application/json")
            .body(json!({ "data": "must not run\n" }).to_string())
            .send()
            .await
            .expect("the device answers");
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "an unaudited operation is refused"
        );
        let body = response.text().await.expect("the refusal is readable");
        assert!(
            body.contains("audit_unwritable") || body.contains("could not be written"),
            "the refusal names the audit: {body}"
        );

        assert_eq!(
            txns.working_count(),
            0,
            "the record begun before the failed write was aborted, not left working"
        );
        let opened = txns
            .get(&TxnId::new("txn-test-1"))
            .expect("the panel's first record is retained");
        assert_eq!(opened.state(), TxnState::Aborted, "it was aborted");
        assert!(
            opened
                .reason()
                .is_some_and(|reason| reason.contains("authorization line")),
            "the reason names what failed: {:?}",
            opened.reason()
        );
        assert!(
            std::fs::read_dir(&audit)
                .expect("the unwritable path is a directory")
                .next()
                .is_none(),
            "no audit row was fabricated where the log could not be written"
        );
    }

    /// **NO PANEL TRANSACTION OUTLIVES ITS REQUEST, AND A STALE ID BEGINS NONE.** A transaction
    /// on a session that has gone would be a working record nothing can end; a refused operation
    /// would leave the record it opened working. The first is refused before a record exists, the
    /// second aborts the record it opened, and either way the registry has nothing left working.
    #[tokio::test]
    async fn a_closed_and_a_refused_panel_session_leave_no_open_transaction() {
        let fixture = fixture("panel-txn-no-leak", None);
        let (app, txns) = fixture.app_and_txns();
        let client = client();
        let server = PanelServer::start(app).await;
        let token = served_token(&client, &server).await;

        let create = |kind: &'static str| {
            let client = client.clone();
            let server = &server;
            let token = token.clone();
            async move {
                let response = client
                    .post(format!("{}/api/terminal/create", server.base_url))
                    .header("x-summrise-panel", &token)
                    .header("content-type", "application/json")
                    .body(json!({ "kind": kind }).to_string())
                    .send()
                    .await
                    .expect("the device answers");
                assert_eq!(response.status(), StatusCode::OK);
                let created: Value = serde_json::from_str(&response.text().await.unwrap())
                    .expect("the create answer is JSON");
                created["id"].as_str().expect("a session id").to_owned()
            }
        };

        // A live session that is closed first: a write afterwards names no live session, so the
        // panel begins no transaction at all (the capability's own refusal is the answer).
        let closed = create("pty").await;
        let response = client
            .post(format!("{}/api/terminal/{closed}/close", server.base_url))
            .header("x-summrise-panel", &token)
            .header("content-type", "application/json")
            .body(json!({}).to_string())
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::OK);
        let response = client
            .post(format!("{}/api/terminal/{closed}/write", server.base_url))
            .header("x-summrise-panel", &token)
            .header("content-type", "application/json")
            .body(json!({ "data": "too late\n" }).to_string())
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            txns.working_count(),
            0,
            "a stale session begins no record to leak"
        );

        // A live session whose operation is refused: the record the request opened is aborted and
        // its ending is on the audit, so the group is closed rather than orphaned.
        let live = create("pty").await;
        let response = client
            .post(format!("{}/api/terminal/{live}/resize", server.base_url))
            .header("x-summrise-panel", &token)
            .header("content-type", "application/json")
            .body(json!({ "rows": 0, "cols": 0 }).to_string())
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            txns.working_count(),
            0,
            "a refused operation aborts the record it opened"
        );

        let lines: Vec<Value> = std::fs::read_to_string(fixture.config.data.join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("an audit line is JSON"))
            .collect();
        let resize = lines
            .iter()
            .find(|line| line["tool"] == "terminal.resize" && line["event"] == "begin")
            .expect("the refused resize was authorized first");
        let txn = resize["txn"]
            .as_str()
            .expect("the resize rode a transaction");
        let abort = lines
            .iter()
            .find(|line| line["tool"] == "txn.abort" && line["txn"] == json!(txn))
            .expect("the abort is on the audit");
        assert_eq!(abort["event"], "begin", "{abort}");
        assert_eq!(
            abort["args"], "purpose=your panel on session-2",
            "the purpose names the panel's own unit: {abort}"
        );
        let abort_end = lines
            .iter()
            .find(|line| line["event"] == "end" && line["id"] == abort["id"])
            .expect("the abort has an end line");
        assert_eq!(abort_end["outcome"], "aborted", "{abort_end}");
        assert_eq!(
            txns.get(&TxnId::new(txn)).map(|record| record.state()),
            Some(TxnState::Aborted),
            "the receipt is aborted, not left working"
        );
    }

    /// A panel operation is a call in the journal's sense too: the person at the machine is not the
    /// one actor whose work the record cannot show.
    #[tokio::test]
    async fn a_panel_operation_is_a_journal_call_pair_sharing_one_trace() {
        let fixture = fixture("panel-journal-calls", None);
        let bus = Arc::new(
            summrise_bus::Bus::open(fixture.config.data.join("journal"))
                .expect("the journal opens"),
        );
        let client = client();
        let server = PanelServer::start(fixture.app_with_bus(Some(Arc::clone(&bus)))).await;
        let token = served_token(&client, &server).await;

        let response = client
            .post(format!("{}/api/terminal/create", server.base_url))
            .header("x-summrise-panel", &token)
            .header("content-type", "application/json")
            .body(json!({ "kind": "pty" }).to_string())
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::OK);
        let created: Value = serde_json::from_str(&response.text().await.unwrap())
            .expect("the create answer is JSON");
        let id = created["id"].as_str().expect("a session id").to_owned();
        let response = client
            .post(format!("{}/api/terminal/{id}/close", server.base_url))
            .header("x-summrise-panel", &token)
            .header("content-type", "application/json")
            .body(json!({}).to_string())
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::OK);

        let mut subscription = bus.subscribe(1).expect("the journal replays");
        let mut calls = Vec::new();
        while let Some(event) = subscription.try_next() {
            if event.kind.starts_with("call.") {
                calls.push(event);
            }
        }
        assert_eq!(
            calls
                .iter()
                .map(|event| event.kind.as_str())
                .collect::<Vec<_>>(),
            vec![
                "call.started",
                "call.finished",
                "call.started",
                "call.finished"
            ],
            "create and close are two calls, each a pair: {calls:?}"
        );
        assert_eq!(calls[0].payload["tool"], "terminal.create");
        assert_eq!(calls[2].payload["tool"], "terminal.close");
        let first = calls[0].trace.clone().expect("the pair carries a trace");
        assert_eq!(calls[1].trace, Some(first.clone()), "one trace per call");
        assert_ne!(
            calls[2].trace,
            Some(first),
            "a second call has its own trace, not the first's"
        );
        assert_eq!(calls[0].payload["door"], "panel");
        assert_eq!(
            calls[3].payload["door"], "panel",
            "the finished half of the close names the door too: {:?}",
            calls[3].payload
        );
    }

    #[tokio::test]
    async fn the_panel_snapshot_does_not_carry_the_panel_token() {
        let fixture = fixture("panel-snapshot-token", None);
        let client = client();
        let server = PanelServer::start(fixture.app()).await;
        let token = served_token(&client, &server).await;

        let (status, body) = get_text(&client, &format!("{}/api/panel", server.base_url)).await;

        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains("\"version\""),
            "the snapshot is the panel's JSON: {body}"
        );
        assert!(
            body.contains("\"data_dir\""),
            "the local section needs the data directory: {body}"
        );
        assert!(
            !body.contains(&token),
            "the token the HTML carries is not in the snapshot: {body}"
        );
    }

    #[tokio::test]
    async fn the_runs_route_answers_the_full_command_for_the_transcript() {
        let fixture = fixture("panel-run", None);
        let record = serde_json::json!({
            "id": "run-1",
            "command": "cmd /c echo srm_deadbeef0123456789",
            "args": [],
            "cwd": "C:\\ProgramData\\Summrise\\workspace",
            "exit_code": 0,
            "timed_out": false,
            "duration_ms": 42,
            "started_at_ms": 1,
            "stdout_bytes": 26,
            "stderr_bytes": 0
        });
        std::fs::write(
            fixture.config.data.join("runs.jsonl"),
            format!("{record}\n"),
        )
        .expect("the run history is written");

        let client = client();
        let server = PanelServer::start(fixture.app()).await;
        let token = served_token(&client, &server).await;

        let anonymous = client
            .get(format!("{}/api/runs/run-1", server.base_url))
            .send()
            .await
            .expect("the anonymous request runs");
        assert_eq!(
            anonymous.status(),
            StatusCode::FORBIDDEN,
            "a page without this boot's token must not read a command line"
        );

        let answer = client
            .get(format!("{}/api/runs/run-1", server.base_url))
            .header("x-summrise-panel", &token)
            .send()
            .await
            .expect("the authorized request runs");
        assert_eq!(answer.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&answer.text().await.expect("the answer is text"))
                .expect("the answer is JSON");
        assert_eq!(body["run"]["command"], "cmd /c echo srm_deadbeef0123456789");
        assert_eq!(body["run"]["exit_code"], 0);
        assert_eq!(
            body["run"]["stdout_bytes"], 26,
            "the history says the size, not the body"
        );
    }

    /// The value of one HTML attribute, for the transcript page's cursor and boot facts.
    fn page_attribute(html: &str, name: &str) -> Option<String> {
        let opening = format!(r#"{name}=""#);
        let start = html.find(&opening)? + opening.len();
        let rest = &html[start..];
        let end = rest.find('"')?;

        Some(rest[..end].to_owned())
    }

    /// The transcript's audit pair for a tool call, as `events.jsonl` spells it.
    fn call_begin(id: u64, at: u64, tool: &str, args: Option<&str>) -> Value {
        json!({
            "id": id,
            "ts_ms": at,
            "event": "begin",
            "tool": tool,
            "decision": "allow",
            "transport": "loopback",
            "args": args,
        })
    }

    fn call_end(id: u64, at: u64, ok: bool, run_id: Option<&str>) -> Value {
        json!({
            "id": id,
            "ts_ms": at,
            "event": "end",
            "ok": ok,
            "error_kind": if ok { None } else { Some("refused") },
            "transport": "loopback",
            "run_id": run_id,
        })
    }

    fn write_audit(fixture: &Fixture, lines: &[Value]) {
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        fs::write(fixture.config.data.join("events.jsonl"), text)
            .expect("the audit fixture is written");
    }

    async fn activity_query(
        client: &reqwest::Client,
        base_url: &str,
        token: &str,
        query: &str,
    ) -> (StatusCode, Value) {
        let response = client
            .get(format!("{base_url}/api/activity{query}"))
            .header("x-summrise-panel", token)
            .send()
            .await
            .expect("the activity route answers");
        let status = response.status();
        let text = response.text().await.expect("the answer is text");

        (
            status,
            serde_json::from_str(&text).expect("the answer is JSON"),
        )
    }

    async fn activity_answer(
        client: &reqwest::Client,
        base_url: &str,
        token: &str,
        after: &str,
        limit: u64,
    ) -> (StatusCode, Value) {
        activity_query(
            client,
            base_url,
            token,
            &format!("?after={after}&limit={limit}"),
        )
        .await
    }

    #[tokio::test]
    async fn the_transcript_page_is_loopback_only_and_needs_the_panel_token() {
        let fixture = fixture("activity-loopback", None);
        write_audit(
            &fixture,
            &[
                call_begin(1, 1_761_566_401_000, "files.list", None),
                call_end(1, 1_761_566_401_100, true, None),
            ],
        );
        let app = fixture.app();
        let remote = SocketAddr::from(([203, 0, 113, 10], 40_123));

        // The page carries this boot's token, so a request that reached the listener through
        // something else gets no page.
        let mut request = get("/activity", None);
        request.extensions_mut().insert(ConnectInfo(remote));
        let response = app
            .clone()
            .oneshot(request)
            .await
            .expect("the router answers");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the refusal is readable");
        let refusal: Value = serde_json::from_slice(&bytes).expect("the refusal is JSON");
        assert_eq!(refusal, json!({ "error": "loopback_only" }));

        let mut request = get("/activity", None);
        request
            .extensions_mut()
            .insert(ConnectInfo(loopback_addr(0)));
        let response = app
            .clone()
            .oneshot(request)
            .await
            .expect("the router answers");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the page is readable");
        let html = String::from_utf8_lossy(&bytes).into_owned();
        let token = panel_meta(&html).expect("the transcript carries this boot's token");
        assert_eq!(token.len(), 64, "the token is 32 bytes as hex: {token}");
        assert!(
            token
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "the token is lowercase hex: {token}"
        );
        assert_eq!(
            html.matches(&token).count(),
            1,
            "the page carries the token only in its meta tag"
        );

        // The streaming route is the other half of the same page: without the panel token it is
        // refused even on loopback.
        let mut request = get("/api/activity", None);
        request
            .extensions_mut()
            .insert(ConnectInfo(loopback_addr(0)));
        let (status, answer) = send(app.clone(), request).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(answer, json!({ "error": "unauthorized" }));

        // The right token on the wrong peer is still refused: the token exists only on this machine.
        let mut request = Request::builder()
            .method("GET")
            .uri("/api/activity")
            .header("x-summrise-panel", &token)
            .body(Body::empty())
            .expect("the request builds");
        request.extensions_mut().insert(ConnectInfo(remote));
        let (status, answer) = send(app, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(answer, json!({ "error": "loopback_only" }));
    }

    #[tokio::test]
    async fn the_transcript_run_detail_names_the_session_and_the_command() {
        let fixture = fixture("activity-run-detail", None);
        write_audit(
            &fixture,
            &[
                call_begin(
                    1,
                    1_761_566_401_000,
                    "terminal.write",
                    Some("session=session-1"),
                ),
                call_end(1, 1_761_566_401_100, true, None),
                call_begin(2, 1_761_566_402_000, "runs.exec", Some("echo srm_deadbeef")),
                call_end(2, 1_761_566_402_040, true, Some("run-1")),
            ],
        );
        let record = serde_json::json!({
            "id": "run-1",
            "command": "cmd /c echo srm_deadbeef0123456789",
            "args": [],
            "cwd": "C:\\ProgramData\\Summrise\\workspace",
            "exit_code": 0,
            "timed_out": false,
            "duration_ms": 42,
            "started_at_ms": 1_761_566_402_000u64,
            "stdout_bytes": 26,
            "stderr_bytes": 0
        });
        std::fs::write(
            fixture.config.data.join("runs.jsonl"),
            format!("{record}\n"),
        )
        .expect("the run history is written");

        let client = client();
        let server = PanelServer::start(fixture.app()).await;
        let (status, html) = get_text(&client, &format!("{}/activity", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        let token = panel_meta(&html).expect("the page carries the panel token");

        assert!(
            html.contains("session-1"),
            "the terminal call names its session: {html}"
        );
        assert!(
            html.contains(r#"data-run="run-1""#),
            "the run call names the run its end wrote: {html}"
        );

        let answer = client
            .get(format!("{}/api/runs/run-1", server.base_url))
            .header("x-summrise-panel", &token)
            .send()
            .await
            .expect("the run detail route answers");
        assert_eq!(answer.status(), StatusCode::OK);
        let body: Value = serde_json::from_str(&answer.text().await.expect("the answer is text"))
            .expect("the answer is JSON");
        assert_eq!(
            body["run"]["command"], "cmd /c echo srm_deadbeef0123456789",
            "the expand reads the command the audit deliberately does not carry: {body}"
        );
    }

    #[tokio::test]
    async fn the_activity_route_streams_rows_after_a_cursor_and_reports_more() {
        let fixture = fixture("activity-cursor", None);
        let at = 1_761_566_401_000;
        write_audit(
            &fixture,
            &[
                call_begin(1, at, "runs.exec", Some("echo one")),
                call_end(1, at + 100, true, Some("run-1")),
                call_begin(2, at + 1_000, "files.read", Some("notes.txt")),
                call_end(2, at + 1_100, true, None),
                call_begin(3, at + 2_000, "terminal.write", Some("session=session-1")),
                call_end(3, at + 2_100, true, None),
            ],
        );
        let client = client();
        let server = PanelServer::start(fixture.app()).await;
        let (status, html) = get_text(&client, &format!("{}/activity", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        let token = panel_meta(&html).expect("the page carries the panel token");
        let boot = page_attribute(&html, "data-boot").expect("the page carries this boot's id");

        let (status, first) =
            activity_answer(&client, &server.base_url, &token, &format!("{boot}.0"), 2).await;
        assert_eq!(status, StatusCode::OK);
        let rows = first["rows"].as_array().expect("the page carries rows");
        assert_eq!(rows.len(), 2, "the limit is the page size: {first}");
        assert_eq!(rows[0]["tool"], "runs.exec", "oldest first: {first}");
        assert_eq!(rows[1]["tool"], "files.read", "oldest first: {first}");
        assert_eq!(first["more"], true, "a third row is not in this page");

        let next = first["next"].as_u64().expect("the page carries its cursor");
        let (status, second) = activity_answer(
            &client,
            &server.base_url,
            &token,
            &format!("{boot}.{next}"),
            2,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let rows = second["rows"].as_array().expect("the page carries rows");
        assert_eq!(
            rows.last().map(|row| row["tool"].clone()),
            Some(json!("terminal.write")),
            "the rest of the record ends with the third call: {second}"
        );
        assert!(
            rows.iter().any(|row| row["id"] == json!(3)),
            "the third call is not skipped: {second}"
        );
        assert_eq!(second["more"], false, "nothing follows the last row");

        // A cursor from another boot is not this boot's numbering: the route answers the newest
        // window rather than nothing for a page whose cursor is stale.
        let stale = boot
            .parse::<u64>()
            .expect("the boot id is decimal")
            .saturating_add(1);
        let (status, restarted) =
            activity_answer(&client, &server.base_url, &token, &format!("{stale}.0"), 2).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            !restarted["rows"]
                .as_array()
                .expect("the page carries rows")
                .is_empty(),
            "a stale cursor still answers rows: {restarted}"
        );
    }

    #[tokio::test]
    async fn the_activity_route_needs_the_panel_token() {
        let fixture = fixture("activity-token", None);
        write_audit(
            &fixture,
            &[
                call_begin(1, 1_761_566_401_000, "files.list", None),
                call_end(1, 1_761_566_401_100, true, None),
                call_begin(2, 1_761_566_402_000, "runs.exec", Some("false")),
                call_end(2, 1_761_566_402_100, false, None),
                call_begin(3, 1_761_566_403_000, "terminal.write", None),
            ],
        );
        let client = client();
        let server = PanelServer::start(fixture.app()).await;
        let (status, html) = get_text(&client, &format!("{}/activity", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        let token = panel_meta(&html).expect("the page carries the panel token");

        let anonymous = client
            .get(format!("{}/api/activity", server.base_url))
            .send()
            .await
            .expect("the anonymous request runs");
        assert_eq!(anonymous.status(), StatusCode::FORBIDDEN);

        let wrong = format!(
            "{}{}",
            if token.starts_with('0') { '1' } else { '0' },
            &token[1..]
        );
        let refused = client
            .get(format!("{}/api/activity", server.base_url))
            .header("x-summrise-panel", &wrong)
            .send()
            .await
            .expect("the refused request runs");
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);

        let answer = client
            .get(format!("{}/api/activity", server.base_url))
            .header("x-summrise-panel", &token)
            .send()
            .await
            .expect("the authorized request runs");
        assert_eq!(answer.status(), StatusCode::OK);
        let text = answer.text().await.expect("the answer is text");
        let body: Value = serde_json::from_str(&text).expect("the answer is JSON");
        let object = body.as_object().expect("the answer is an object");
        assert_eq!(
            object.len(),
            4,
            "the answer is exactly rows, next, more and counts: {text}"
        );
        for key in ["rows", "next", "more", "counts"] {
            assert!(object.contains_key(key), "the answer carries {key}: {text}");
        }
        assert_eq!(body["counts"]["total"], 3, "{text}");
        assert_eq!(body["counts"]["failed"], 1, "{text}");
        assert_eq!(body["counts"]["groups"]["files"], 1, "{text}");
        assert_eq!(body["counts"]["groups"]["runs"], 1, "{text}");
        assert_eq!(body["counts"]["groups"]["terminal"], 1, "{text}");
        assert_eq!(body["counts"]["groups"]["other"], 0, "{text}");
        let rows = body["rows"].as_array().expect("the answer carries rows");
        assert_eq!(rows.len(), 3, "{text}");
        assert!(
            rows[0]["end_seq"].is_u64(),
            "a completed row carries its end sequence: {text}"
        );
        assert!(
            rows[1]["end_seq"].is_u64(),
            "a completed row carries its end sequence: {text}"
        );
        assert_eq!(
            rows[2]["end_seq"],
            Value::Null,
            "a running row's end sequence is null: {text}"
        );
        assert!(
            !text.contains(&token),
            "the page token never travels in the body: {text}"
        );
    }

    /// `?txn=<id>` names the expanded entry; without it, the newest entry is the default (no
    /// working record exists on this fixture).
    #[tokio::test]
    async fn the_activity_page_expands_the_transaction_the_query_names() {
        let fixture = fixture("activity-txn", None);
        let at = 1_761_566_400_000u64;
        let lines = vec![
            json!({
                "id": 1, "ts_ms": at, "event": "begin", "tool": "txn.begin",
                "decision": "allow", "transport": "loopback", "args": "purpose=fix the parser"
            }),
            json!({
                "id": 1, "ts_ms": at + 5, "event": "end", "ok": true,
                "outcome": "started", "transport": "loopback", "txn": "txn-1"
            }),
            json!({
                "id": 2, "ts_ms": at + 1_000, "event": "begin", "tool": "files.read",
                "decision": "allow", "transport": "loopback", "txn": "txn-1"
            }),
            json!({
                "id": 2, "ts_ms": at + 1_100, "event": "end", "ok": true,
                "transport": "loopback", "txn": "txn-1"
            }),
            json!({
                "id": 3, "ts_ms": at + 30_000, "event": "begin", "tool": "txn.begin",
                "decision": "allow", "transport": "loopback", "args": "purpose=verify the suite"
            }),
            json!({
                "id": 3, "ts_ms": at + 30_005, "event": "end", "ok": true,
                "outcome": "started", "transport": "loopback", "txn": "txn-2"
            }),
            json!({
                "id": 4, "ts_ms": at + 31_000, "event": "begin", "tool": "files.list",
                "decision": "allow", "transport": "loopback", "txn": "txn-2"
            }),
            json!({
                "id": 4, "ts_ms": at + 31_100, "event": "end", "ok": true,
                "transport": "loopback", "txn": "txn-2"
            }),
        ];
        write_audit(&fixture, &lines);
        let client = client();
        let server = PanelServer::start(fixture.app()).await;

        let (status, default) = get_text(&client, &format!("{}/activity", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            default.contains(r#"data-txn="txn-2" data-expanded="true""#),
            "the newest entry is the default: {default}"
        );

        let (status, selected) =
            get_text(&client, &format!("{}/activity?txn=txn-1", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            selected.contains(r#"data-txn="txn-1" data-expanded="true""#),
            "the query chooses the expanded transaction: {selected}"
        );
        assert!(
            !selected.contains(r#"data-txn="txn-2" data-expanded="true""#),
            "only one entry is expanded: {selected}"
        );
    }

    /// A real MCP client for the route tests: the streamable transport's handshake, then tool calls.
    ///
    /// The activity page reads a session's goal from the live registry, and `terminal.create` /
    /// `terminal.goal` are the surfaces that write one; a route test that needs a goal must
    /// therefore be a client of `/mcp`.
    struct McpClient {
        http: reqwest::Client,
        url: String,
        token: String,
        session: String,
        protocol: String,
        next_id: u64,
    }

    impl McpClient {
        async fn start(base_url: &str, token: &str) -> McpClient {
            let http = client();
            let url = format!("{base_url}/mcp");
            let response = http
                .post(&url)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(
                    json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "initialize",
                        "params": {
                            "protocolVersion": "2025-06-18",
                            "capabilities": {},
                            "clientInfo": { "name": "activity-route-test", "version": "0.1.0" }
                        }
                    })
                    .to_string(),
                )
                .send()
                .await
                .expect("the MCP handshake answers");
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "the handshake is answered"
            );
            let session = response
                .headers()
                .get("mcp-session-id")
                .expect("initialize binds a session")
                .to_str()
                .expect("the session id is ASCII")
                .to_owned();
            let message = mcp_message(response).await;
            let protocol = message["result"]["protocolVersion"]
                .as_str()
                .expect("the handshake names a protocol")
                .to_owned();
            let mcp = McpClient {
                http,
                url,
                token: token.to_owned(),
                session,
                protocol,
                next_id: 2,
            };
            let initialized = mcp
                .post(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
                .await;
            assert_eq!(
                initialized.status(),
                StatusCode::ACCEPTED,
                "the initialized notification is accepted"
            );
            mcp
        }

        async fn post(&self, body: Value) -> reqwest::Response {
            self.http
                .post(&self.url)
                .header("authorization", format!("Bearer {}", self.token))
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("mcp-session-id", &self.session)
                .header("mcp-protocol-version", &self.protocol)
                .body(body.to_string())
                .send()
                .await
                .expect("the MCP server answers")
        }

        /// One tool call; its answer's text is the tool's JSON document.
        async fn call(&mut self, tool: &str, arguments: Value) -> Value {
            let id = self.next_id;
            self.next_id += 1;
            let response = self
                .post(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "tools/call",
                    "params": { "name": tool, "arguments": arguments }
                }))
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{tool} is answered");
            let message = mcp_message(response).await;
            assert_eq!(
                message["result"]["isError"], false,
                "{tool} succeeded: {message}"
            );
            let text = message["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_else(|| panic!("{tool} answers text: {message}"));
            serde_json::from_str(text).expect("the tool's text is JSON")
        }
    }

    /// The JSON-RPC message in a streamable-HTTP answer: its last non-empty SSE `data:` line.
    async fn mcp_message(response: reqwest::Response) -> Value {
        let body = response.text().await.expect("the MCP answer is text");
        let data = body
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .rfind(|data| !data.is_empty())
            .unwrap_or_else(|| panic!("no JSON-RPC message in {body:?}"));
        serde_json::from_str(data).expect("the JSON-RPC data is JSON")
    }

    /// The document is the session the query names: its goal is the title, and a session the record
    /// does not know is named as missing rather than silently replaced by another.
    #[tokio::test]
    async fn the_activity_page_is_the_document_of_the_session_the_query_names() {
        let fixture = fixture("activity-session-query", Some(&token_hex()));
        let client = client();
        let server = PanelServer::start(fixture.app()).await;
        let mut mcp = McpClient::start(&server.base_url, &token_hex()).await;
        let first = mcp
            .call(
                "terminal.create",
                json!({ "kind": "pty", "goal": "read the installer's config" }),
            )
            .await;
        let first = first["id"].as_str().expect("a session id").to_owned();
        let second = mcp
            .call(
                "terminal.create",
                json!({ "kind": "pty", "goal": "ship the spine this week" }),
            )
            .await;
        let second = second["id"].as_str().expect("a session id").to_owned();
        assert_ne!(first, second, "two sessions were opened");

        let (status, html) = get_text(
            &client,
            &format!("{}/activity?session={second}", server.base_url),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let goal_at = html
            .find(r#"class="goal""#)
            .expect("the goal block is drawn");
        let ledger_at = html
            .find(r#"id="ledger""#)
            .expect("the selected session's ledger is drawn");
        let goal_block = &html[goal_at..ledger_at];
        assert!(
            goal_block.contains(&format!(r#"data-session="{second}""#)),
            "the query selects its session: {goal_block}"
        );
        assert!(
            goal_block.contains("ship the spine this week"),
            "the selected session's goal is the title: {goal_block}"
        );
        assert!(
            !goal_block.contains("read the installer's config"),
            "another session's goal is never the title: {goal_block}"
        );

        let (status, html) = get_text(
            &client,
            &format!("{}/activity?session=nope", server.base_url),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            html.contains(r#"data-missing-session="nope""#),
            "the page names the session the query asked for: {html}"
        );
        assert!(
            html.contains("Session nope is not in this record."),
            "the page says it is not in this record: {html}"
        );
        assert!(
            !html.contains(r#"data-goal="set""#),
            "a session the record does not know substitutes no other session: {html}"
        );
    }

    fn activity_app(fixture: &Fixture, device: &crate::mcp::DeviceServer) -> Router {
        let state = AppState {
            token: None,
            store: fixture.store.clone(),
            release: fixture.config.release(),
            helper: None,
            data: fixture.config.data.clone(),
            port: 0,
            panel_token: "test-panel-token".to_owned(),
            boot: 1,
            audit: Arc::new(crate::mcp::Audit::new(&fixture.config.data)),
            supervised: false,
            restart: restart::none(),
            terminal: device.terminal_registry(),
            txns: Arc::clone(device.txns()),
            bus: None,
        };
        Router::new()
            .route("/activity", axum::routing::get(activity_page))
            .with_state(state)
    }

    async fn client_session_call(
        device: &crate::mcp::DeviceServer,
        tool: &str,
        arguments: Value,
    ) -> Value {
        let scopes = vec!["*".to_owned()];
        let reply = device
            .handle(
                &json!({
                    "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": { "name": tool, "arguments": arguments }
                }),
                crate::mcp::Caller::Console {
                    token_id: "session-client",
                    scopes: &scopes,
                    allow_destructive: true,
                    arrival: crate::mcp::Arrival::Console,
                    trace: None,
                },
            )
            .await;
        assert_eq!(reply["result"]["isError"], false, "{tool}: {reply}");
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    /// Run the real dispatcher and serve its retained audit through the normal activity route.
    /// This keeps client policy, child outcomes, receipts and the page's facts on the same registries.
    #[tokio::test]
    async fn activity_process_effects_follow_real_execution_and_policy() {
        let fixture = fixture("activity-real-processes", None);
        let device = crate::mcp::DeviceServer::new(&fixture.config, terminal::Limits::default());
        let scopes = vec!["*".to_owned()];
        let caller = crate::mcp::Caller::Console {
            token_id: "process-client",
            scopes: &scopes,
            allow_destructive: true,
            arrival: crate::mcp::Arrival::Console,
            trace: None,
        };
        let create = device
            .handle(
                &json!({
                    "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": { "name": "terminal.create", "arguments": { "kind": "pty" } }
                }),
                caller,
            )
            .await;
        let created: Value =
            serde_json::from_str(create["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        let session = created["id"]
            .as_str()
            .expect("client creates a real session");
        let server = PanelServer::start(activity_app(&fixture, &device)).await;
        let http = client();
        let timeout_command = if cfg!(windows) {
            "ping -n 5 127.0.0.1 >NUL"
        } else {
            "sleep 5"
        };
        let denied_scopes = vec!["files.*".to_owned()];
        let cases = [
            (
                "nonzero",
                json!({"command": "exit 7", "shell": true}),
                true,
                true,
                1,
                1,
                0,
            ),
            (
                "timeout",
                json!({"command": timeout_command, "shell": true, "timeout_ms": 50}),
                true,
                true,
                1,
                1,
                0,
            ),
            (
                "missing-program",
                json!({"command": "summrise-no-such-process-round27"}),
                true,
                true,
                0,
                1,
                0,
            ),
            (
                "invalid-cwd",
                json!({"command": "unused", "cwd": "../"}),
                true,
                true,
                0,
                1,
                0,
            ),
            ("arguments", json!({"command": 7}), true, true, 0, 1, 0),
            (
                "approval",
                json!({"command": "exit 0", "shell": true}),
                true,
                false,
                0,
                0,
                1,
            ),
            (
                "denied",
                json!({"command": "exit 0", "shell": true}),
                false,
                true,
                0,
                0,
                1,
            ),
        ];
        for (label, mut arguments, covered, consent, processes, calls, refusals) in cases {
            let begun = device.handle(&json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": { "name": "txn.begin", "arguments": { "session": session, "purpose": label } }
            }), caller).await;
            let begun: Value =
                serde_json::from_str(begun["result"]["content"][0]["text"].as_str().unwrap())
                    .unwrap();
            let txn = begun["txn"].as_str().unwrap();
            arguments["txn"] = json!(txn);
            let reply = device
                .handle(
                    &json!({
                        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                        "params": { "name": "runs.exec", "arguments": arguments }
                    }),
                    crate::mcp::Caller::Console {
                        token_id: "process-client",
                        scopes: if covered { &scopes } else { &denied_scopes },
                        allow_destructive: consent,
                        arrival: crate::mcp::Arrival::Console,
                        trace: None,
                    },
                )
                .await;
            if label == "arguments" {
                assert_eq!(reply["error"]["code"], -32602, "{label}: {reply}");
            } else if refusals == 0 {
                assert_eq!(reply["result"]["isError"], true, "{label}: {reply}");
            } else {
                assert!(reply["error"].is_object(), "{label}: {reply}");
            }
            let committed = device
                .handle(
                    &json!({
                        "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                        "params": { "name": "txn.commit", "arguments": { "txn": txn } }
                    }),
                    caller,
                )
                .await;
            let receipt: Value =
                serde_json::from_str(committed["result"]["content"][0]["text"].as_str().unwrap())
                    .unwrap();
            assert_eq!(receipt["observed"]["calls"], calls, "{label}: {receipt}");
            assert_eq!(
                receipt["observed"]["failures"],
                if refusals == 0 { 1 } else { 0 },
                "{label}: {receipt}"
            );
            assert_eq!(
                receipt["observed"]["refusals"], refusals,
                "{label}: {receipt}"
            );
            let (status, html) = get_text(
                &http,
                &format!("{}/activity?session={session}&txn={txn}", server.base_url),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            let entry = html
                .split(&format!(r#"data-txn="{txn}" data-expanded="true""#))
                .nth(1)
                .expect("the requested entry is expanded");
            let entry = entry.split("</article>").next().unwrap();
            assert!(
                entry.contains(&format!(
                    "files 0 written · processes {processes} run · forwarding rules recorded 0 · windows 0"
                )),
                "{label}: {entry}"
            );
            assert!(
                entry.contains(&format!(
                    "Observed in this window: 1 call · 1 failure · {refusals} refused"
                )),
                "{label}: {entry}"
            );
        }
    }

    #[tokio::test]
    async fn activity_closed_session_goal_is_unavailable() {
        let fixture = fixture("activity-closed-goal", Some(&token_hex()));
        let http = client();
        let server = PanelServer::start(fixture.app()).await;
        let mut mcp = McpClient::start(&server.base_url, &token_hex()).await;
        let created = mcp
            .call(
                "terminal.create",
                json!({"kind": "pty", "goal": "original session goal"}),
            )
            .await;
        let session = created["id"].as_str().unwrap();
        let begun = mcp
            .call(
                "txn.begin",
                json!({"session": session, "purpose": "retained purpose"}),
            )
            .await;
        let txn = begun["txn"].as_str().unwrap();
        mcp.call("terminal.write", json!({"id": session, "data": "\n"}))
            .await;
        mcp.call("terminal.close", json!({"id": session})).await;
        let receipt = mcp.call("txn.commit", json!({"txn": txn})).await;
        assert_eq!(receipt["goal_at_start"]["text"], "original session goal");

        let (status, html) = get_text(
            &http,
            &format!("{}/activity?session={session}&txn={txn}", server.base_url),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let goal = html
            .split(r#"class="goal""#)
            .nth(1)
            .unwrap()
            .split(r#"id="ledger""#)
            .next()
            .unwrap();
        assert!(goal.contains(r#"data-goal="unavailable""#), "{goal}");
        assert!(
            goal.contains("The goal for this session is not retained."),
            "{goal}"
        );
        assert!(
            !goal.contains("No goal set")
                && !goal.contains("original session goal")
                && !goal.contains("retained purpose"),
            "{goal}"
        );
        assert!(
            html.contains(&format!(r#"data-txn="{txn}" data-expanded="true""#)),
            "the session's ledger remains selected"
        );
        assert!(html.contains("retained purpose"));
    }

    #[tokio::test]
    async fn activity_selects_session_with_newest_transaction_backed_machine_work() {
        let fixture = fixture("activity-machine-recency", None);
        let device = crate::mcp::DeviceServer::new(&fixture.config, terminal::Limits::default());
        let http = client();
        let server = PanelServer::start(activity_app(&fixture, &device)).await;
        let first = client_session_call(
            &device,
            "terminal.create",
            json!({"kind": "pty", "goal": "newest machine work"}),
        )
        .await;
        let first = first["id"].as_str().unwrap();
        let second = client_session_call(
            &device,
            "terminal.create",
            json!({"kind": "pty", "goal": "older terminal work"}),
        )
        .await;
        let second = second["id"].as_str().unwrap();
        let older = client_session_call(
            &device,
            "txn.begin",
            json!({"session": second, "purpose": "terminal work"}),
        )
        .await;
        let older = older["txn"].as_str().unwrap();
        client_session_call(&device, "terminal.read", json!({"id": second})).await;
        let newest = client_session_call(
            &device,
            "txn.begin",
            json!({"session": first, "purpose": "machine work"}),
        )
        .await;
        let newest = newest["txn"].as_str().unwrap();
        client_session_call(&device, "files.list", json!({"path": "", "txn": newest})).await;
        // An unrelated id/session-looking string in a run command cannot move it to session B.
        client_session_call(&device, "runs.exec", json!({"command": format!("echo id={second} session={second}"), "shell": true, "txn": newest})).await;
        let (_, working) = get_text(&http, &format!("{}/activity", server.base_url)).await;
        let goal = working
            .split(r#"class="goal""#)
            .nth(1)
            .unwrap()
            .split(r#"id="ledger""#)
            .next()
            .unwrap();
        assert!(
            goal.contains(&format!(r#"data-session="{first}""#)),
            "working selection: {goal}"
        );

        client_session_call(&device, "txn.commit", json!({"txn": older})).await;
        client_session_call(&device, "txn.commit", json!({"txn": newest})).await;
        let (_, committed) = get_text(&http, &format!("{}/activity", server.base_url)).await;
        let goal = committed
            .split(r#"class="goal""#)
            .nth(1)
            .unwrap()
            .split(r#"id="ledger""#)
            .next()
            .unwrap();
        assert!(
            goal.contains(&format!(r#"data-session="{first}""#)),
            "closed transaction selection: {goal}"
        );
        assert!(goal.contains("newest machine work") && !goal.contains("older terminal work"));
        // Exempt terminal operations still attribute their explicit session without a txn.
        client_session_call(
            &device,
            "terminal.goal",
            json!({"id": second, "action": "clear"}),
        )
        .await;
        let (_, terminal) = get_text(&http, &format!("{}/activity", server.base_url)).await;
        let goal = terminal
            .split(r#"class="goal""#)
            .nth(1)
            .unwrap()
            .split(r#"id="ledger""#)
            .next()
            .unwrap();
        assert!(
            goal.contains(&format!(r#"data-session="{second}""#)),
            "terminal fallback: {goal}"
        );
        assert!(
            goal.contains("No goal set for this session."),
            "known goal clear: {goal}"
        );
    }

    #[tokio::test]
    async fn the_activity_page_ignores_a_theme_query() {
        let fixture = fixture("activity-one-palette", None);
        let client = client();
        let server = PanelServer::start(fixture.app()).await;

        let (status, plain) = get_text(&client, &format!("{}/activity", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);

        let (status, asked) =
            get_text(&client, &format!("{}/activity?theme=dark", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            asked, plain,
            "a theme query changes no byte of the page: the product has one palette"
        );
        assert!(
            !asked.contains("data-theme"),
            "the page declares no theme attribute"
        );
    }

    #[tokio::test]
    async fn the_activity_route_returns_the_newest_rows_without_a_cursor() {
        let fixture = fixture("activity-newest", None);
        let at = 1_761_566_400_000u64;
        let mut lines = Vec::with_capacity(120);
        for index in 1..=60u64 {
            let start = at + index * 1_000;
            lines.push(call_begin(index, start, "files.list", None));
            lines.push(call_end(index, start + 100, true, None));
        }
        write_audit(&fixture, &lines);
        let client = client();
        let server = PanelServer::start(fixture.app()).await;
        let token = served_token(&client, &server).await;

        // No cursor: the newest 50 rows by default, oldest first, and older rows remain.
        let (status, answer) = activity_query(&client, &server.base_url, &token, "").await;
        assert_eq!(status, StatusCode::OK);
        let rows = answer["rows"].as_array().expect("the answer carries rows");
        assert_eq!(rows.len(), 50, "the default page is 50 rows: {answer}");
        assert_eq!(
            rows[0]["id"], 11,
            "the newest window, still oldest first: {answer}"
        );
        assert_eq!(rows[49]["id"], 60, "{answer}");
        assert_eq!(answer["more"], true, "ten older rows remain");
        assert_eq!(
            answer["counts"]["total"], 60,
            "the counts cover the window, not the page"
        );

        // limit=0 clamps up to one row: the newest.
        let (status, one) = activity_query(&client, &server.base_url, &token, "?limit=0").await;
        assert_eq!(status, StatusCode::OK);
        let rows = one["rows"].as_array().expect("the answer carries rows");
        assert_eq!(rows.len(), 1, "{one}");
        assert_eq!(rows[0]["id"], 60, "the newest row: {one}");

        // limit=999 is bounded by this window's 60 rows; the 200 cap is unit-tested.
        let (status, all) = activity_query(&client, &server.base_url, &token, "?limit=999").await;
        assert_eq!(status, StatusCode::OK);
        let rows = all["rows"].as_array().expect("the answer carries rows");
        assert_eq!(rows.len(), 60, "{all}");
        assert_eq!(rows[0]["id"], 1, "{all}");
        assert_eq!(rows[59]["id"], 60, "{all}");
        assert_eq!(all["more"], false, "{all}");
    }

    #[tokio::test]
    async fn the_panel_snapshot_never_carries_the_install_token() {
        let token = token_hex();
        let fixture = fixture("panel-snapshot-install-token", Some(&token));
        let client = client();
        let server = PanelServer::start(fixture.app()).await;

        let (status, body) = get_text(&client, &format!("{}/api/panel", server.base_url)).await;

        assert_eq!(status, StatusCode::OK);
        assert!(
            !body.contains(&token),
            "the install token is the owner credential and is named by file, not printed: {body}"
        );
    }

    #[tokio::test]
    async fn the_enroll_endpoint_needs_the_panel_token_and_writes_nothing_without_it() {
        let fixture = fixture("panel-enroll-no-token", None);
        let console = MockConsole::start().await;
        let client = client();
        let server = PanelServer::start(fixture.app()).await;
        let token = served_token(&client, &server).await;
        let wrong = format!(
            "{}{}",
            if token.starts_with('0') { '1' } else { '0' },
            &token[1..]
        );
        let body = json!({ "console_url": console.base_url, "code": "a".repeat(64) });

        for presented in [None, Some(wrong.as_str()), Some("not-hex")] {
            let (status, answer, text) =
                post_enroll(&client, &server.base_url, presented, body.clone()).await;

            assert_eq!(status, StatusCode::FORBIDDEN, "{presented:?}: {text}");
            assert_eq!(
                answer,
                json!({ "error": "unauthorized" }),
                "{presented:?}: {text}"
            );
        }

        assert!(
            !fixture.config.data.join("console.json").exists(),
            "a refused request arms no console link"
        );
        assert!(
            !fixture.config.data.join("console-token.json").exists(),
            "a refused request writes no token"
        );
        assert!(
            console.requests().is_empty(),
            "a refused request never reaches the console"
        );
    }

    #[tokio::test]
    async fn the_panel_enrols_this_machine_and_answers_its_id_and_whether_it_restarts() {
        let fixture = fixture("panel-enroll-ok", None);
        let console = MockConsole::start().await;
        let client = client();
        let calls = Arc::new(AtomicU64::new(0));
        let recorder = calls.clone();
        let hook: crate::restart::Hook = Arc::new(move || {
            recorder.fetch_add(1, Ordering::SeqCst);
        });
        let server = PanelServer::start(fixture.app_with_restart(hook)).await;
        let token = served_token(&client, &server).await;
        let code = "a".repeat(64);

        let (status, answer, text) = post_enroll(
            &client,
            &server.base_url,
            Some(&token),
            json!({ "console_url": console.base_url.clone(), "code": code.clone() }),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{text}");
        let id = answer["id"]
            .as_str()
            .expect("the answer names the device id")
            .to_owned();
        assert_eq!(id.len(), 64, "the id is the identity's public key in hex");
        assert_eq!(
            answer.as_object().map(|object| object.len()),
            Some(2),
            "the answer is the id and the restart fact: {text}"
        );
        assert_eq!(
            answer["restarting"], false,
            "a standalone start is not restarted: it would not come back"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a standalone start never asks the restart hook"
        );
        assert!(
            !text.contains(&console.token(0)),
            "the console token never comes back in the answer: {text}"
        );

        let token_file: Value = serde_json::from_slice(
            &fs::read(fixture.config.data.join("console-token.json"))
                .expect("the registration response is written"),
        )
        .expect("the token file is JSON");
        assert_eq!(token_file["id"], id);
        assert_eq!(token_file["token"], console.token(0));

        let link: Value = serde_json::from_slice(
            &fs::read(fixture.config.data.join("console.json"))
                .expect("the console link is written"),
        )
        .expect("the console link is JSON");
        assert_eq!(link["url"], console.base_url);

        let registrations = console.requests();
        assert_eq!(registrations.len(), 1, "one registration was sent");
        assert_eq!(registrations[0]["id"], id);
        assert_eq!(registrations[0]["code"], code);
        assert_eq!(
            registrations[0]["signature"].as_str().map(str::len),
            Some(128),
            "the registration is signed with the device identity"
        );
    }

    #[tokio::test]
    async fn a_supervised_enrolment_asks_for_a_restart_after_writing_the_link() {
        let fixture = fixture_with_release(
            "panel-enroll-restart",
            None,
            Some("2.0.3"),
            Some("b1946ac92492d2347c6235b4d2611184"),
            Some(Channel::Stable),
        );
        let console = MockConsole::start().await;
        let client = client();
        let calls = Arc::new(AtomicU64::new(0));
        let link_seen = Arc::new(AtomicU64::new(0));
        let data = fixture.config.data.clone();
        let recorder = calls.clone();
        let link = link_seen.clone();
        let hook: crate::restart::Hook = Arc::new(move || {
            recorder.fetch_add(1, Ordering::SeqCst);
            if data.join("console.json").exists() {
                link.fetch_add(1, Ordering::SeqCst);
            }
        });
        let server = PanelServer::start(fixture.app_with_restart(hook)).await;
        let token = served_token(&client, &server).await;

        let (status, answer, text) = post_enroll(
            &client,
            &server.base_url,
            Some(&token),
            json!({ "console_url": console.base_url.clone(), "code": "a".repeat(64) }),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{text}");
        assert_eq!(
            answer["restarting"], true,
            "a supervised start asks for the restart"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the hook is asked exactly once"
        );
        assert_eq!(
            link_seen.load(Ordering::SeqCst),
            1,
            "the console link existed when the hook was asked, not merely when the answer arrived"
        );
    }

    #[tokio::test]
    async fn a_second_panel_enrolment_needs_force_and_then_replaces_the_token() {
        let fixture = fixture("panel-enroll-conflict", None);
        let console = MockConsole::start().await;
        let client = client();
        let server = PanelServer::start(fixture.app()).await;
        let token = served_token(&client, &server).await;
        let token_path = fixture.config.data.join("console-token.json");

        let (status, _, text) = post_enroll(
            &client,
            &server.base_url,
            Some(&token),
            json!({ "console_url": console.base_url.clone(), "code": "a".repeat(64) }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let before = fs::read(&token_path).expect("the first registration is on disk");

        let (status, answer, text) = post_enroll(
            &client,
            &server.base_url,
            Some(&token),
            json!({ "console_url": console.base_url.clone(), "code": "b".repeat(64) }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{text}");
        let sentence = answer["error"]
            .as_str()
            .expect("the refusal is the device's sentence");
        assert!(
            sentence.contains("already exists"),
            "the sentence names the conflict: {sentence}"
        );
        assert!(
            sentence.contains("--force"),
            "the sentence names the flag the form maps: {sentence}"
        );
        assert_eq!(
            fs::read(&token_path).expect("the first registration is kept"),
            before,
            "a refused replacement leaves the old token alone"
        );
        assert_eq!(
            console.requests().len(),
            1,
            "a refused replacement does not spend the second code"
        );

        let (status, answer, text) = post_enroll(
            &client,
            &server.base_url,
            Some(&token),
            json!({
                "console_url": console.base_url.clone(),
                "code": "b".repeat(64),
                "force": true,
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{text}");
        assert_eq!(answer["id"].as_str().map(str::len), Some(64));
        assert_eq!(
            console.requests().len(),
            2,
            "the forced run reaches the console"
        );
        let after: Value = serde_json::from_slice(
            &fs::read(&token_path).expect("the replaced registration is on disk"),
        )
        .expect("the token file is JSON");
        assert_eq!(
            after["token"],
            console.token(1),
            "force replaces the token with the new registration"
        );
    }

    #[tokio::test]
    async fn a_non_loopback_peer_cannot_read_the_panel_or_enrol() {
        let fixture = fixture("panel-enroll-non-loopback", None);
        let console = MockConsole::start().await;
        let app = fixture.app();
        let remote = SocketAddr::from(([203, 0, 113, 10], 40_123));

        // The token is only reachable on loopback: reading the same router from a remote peer gets
        // no page, because the page carries this boot's credential.
        let mut local = get("/", None);
        local.extensions_mut().insert(ConnectInfo(loopback_addr(0)));
        let response = app
            .clone()
            .oneshot(local)
            .await
            .expect("the router answers");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the page is readable");
        let html = String::from_utf8_lossy(&bytes).into_owned();
        let token = panel_meta(&html).expect("the loopback read carries the token");

        let mut request = get("/", None);
        request.extensions_mut().insert(ConnectInfo(remote));
        let response = app
            .clone()
            .oneshot(request)
            .await
            .expect("the router answers");
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "the page carries this boot's token; a non-loopback reader gets no page"
        );

        let mut request = post_enroll_request(
            Some(&token),
            json!({ "console_url": console.base_url, "code": "a".repeat(64) }),
        );
        request.extensions_mut().insert(ConnectInfo(remote));
        let (status, answer) = send(app, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            answer,
            json!({ "error": "loopback_only" }),
            "the peer is refused even with the right token"
        );
        assert!(
            !fixture.config.data.join("console.json").exists(),
            "a non-loopback request arms no console link"
        );
        assert!(
            !fixture.config.data.join("console-token.json").exists(),
            "a non-loopback request writes no token"
        );
        assert!(
            console.requests().is_empty(),
            "a non-loopback request never reaches the console"
        );
    }

    #[test]
    fn panel_terminal_read_projects_live_and_ended_snapshot_facts() {
        for (alive, exit_code, signal) in [
            (true, None, None),
            (false, Some(7), None),
            (false, None, Some("SIGTERM")),
            (false, None, None),
        ] {
            let answer = terminal::ReadAnswer {
                data: b"same snapshot\n".to_vec(),
                cursor: 73,
                alive,
                end: terminal::EndFacts {
                    exit_code,
                    signal: signal.map(str::to_owned),
                },
            };
            let wire = panel_read_answer(&answer);
            assert_eq!(
                wire["alive"],
                json!(alive),
                "the snapshot's liveness must reach the view"
            );
            assert_eq!(wire.get("exit_code"), Some(&json!(exit_code)));
            assert_eq!(wire.get("signal"), Some(&json!(signal)));
            assert_eq!(wire["cursor"], 73);
            assert_eq!(wire["text"], "same snapshot\n");
        }
    }

    #[tokio::test]
    async fn panel_terminal_read_exposes_a_real_temporary_pty_exit() {
        let fixture = fixture("panel-read-exit-projection", None);
        let http = client();
        let server = DeviceUiServer(PanelServer::start(fixture.app()).await);
        let token = served_token(&http, &server.0).await;
        let created = http
            .post(format!("{}/api/terminal/create", server.0.base_url))
            .header(PANEL_TOKEN_HEADER, &token)
            .header(header::CONTENT_TYPE, "application/json")
            .body(json!({"kind":"pty"}).to_string())
            .send()
            .await
            .expect("temporary PTY create");
        assert_eq!(created.status(), StatusCode::OK);
        let created: Value = serde_json::from_str(&created.text().await.unwrap()).unwrap();
        let id = created["id"].as_str().expect("temporary PTY id");
        // THE LINE ENDS WITH CRLF, BECAUSE CR IS THE RETURN KEY ON EVERY PLATFORM. A Unix pty
        // takes a bare LF as the end of a line, so `exit 7\n` runs the command there. A Windows
        // ConPTY console does not: its input pipe carries KEYSTROKES, and the console runs a
        // line only when it is given the RETURN key. A bare LF is not that key — PowerShell
        // draws its `>>` continuation prompt and cmd.exe echoes the text and waits — so the
        // command never runs, the shell never exits, and the session is alive for ever. The
        // capability's own Windows round trip (capabilities/terminal.rs) and the launcher's
        // ConPTY round trip both type CRLF for the same reason, and both pass on Windows.
        //
        // MEASURED ON A REAL WINDOWS DEVICE, not inferred: `exit 7\n` left the session alive
        // with no exit code at all, and `exit 7\r\n` ended it with exit code 7 and no signal.
        let written = http
            .post(format!("{}/api/terminal/{id}/write", server.0.base_url))
            .header(PANEL_TOKEN_HEADER, &token)
            .header(header::CONTENT_TYPE, "application/json")
            .body(json!({"data":"exit 7\r\n"}).to_string())
            .send()
            .await
            .expect("temporary PTY exit command");
        assert_eq!(written.status(), StatusCode::OK);
        // The wait is for the session's END, not for one field of it: a Windows ConPTY round trip
        // (spawn, write, the shell's own exit, the reader's EOF) is far slower than a Unix pty,
        // and the same capability's own round-trip test waits twenty seconds for exactly this
        // observation. The budget is not what the earlier failure was: that run never typed a
        // RETURN, so the shell never ran the command and never ended — no wait could have
        // answered it.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        let mut cursor = 0;
        let ended = loop {
            let read = http
                .get(format!(
                    "{}/api/terminal/{id}/read?cursor={cursor}&wait_ms=250",
                    server.0.base_url
                ))
                .header(PANEL_TOKEN_HEADER, &token)
                .send()
                .await
                .expect("temporary PTY read");
            assert_eq!(read.status(), StatusCode::OK);
            let read: Value = serde_json::from_str(&read.text().await.unwrap()).unwrap();
            if read.get("alive").is_none()
                || read["alive"] == false
                || tokio::time::Instant::now() >= deadline
            {
                break read;
            }
            cursor = read["cursor"].as_u64().expect("absolute stream cursor");
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let closed = http
            .post(format!("{}/api/terminal/{id}/close", server.0.base_url))
            .header(PANEL_TOKEN_HEADER, &token)
            .send()
            .await
            .expect("only the temporary PTY closes");
        assert_eq!(closed.status(), StatusCode::OK);
        assert_eq!(
            ended["alive"], false,
            "the real ended session must reach the view"
        );
        assert_eq!(
            ended["exit_code"], 7,
            "the child's observed exit code must reach the view"
        );
        assert_eq!(ended.get("signal"), Some(&Value::Null));
    }

    #[test]
    fn a_split_character_survives_the_read_answer_as_bytes() {
        // Two reads whose boundary falls inside one three-byte character. The text fields are
        // lossy on purpose (an operator reading a log wants words, not bytes), and the base64
        // fields are what a terminal replays: joined, they are the character again.
        let character = "中".as_bytes();
        let first = panel_read_answer(&terminal::ReadAnswer {
            data: character[..1].to_vec(),
            cursor: 1,
            alive: true,
            end: terminal::EndFacts::default(),
        });
        let second = panel_read_answer(&terminal::ReadAnswer {
            data: character[1..].to_vec(),
            cursor: 3,
            alive: true,
            end: terminal::EndFacts::default(),
        });
        assert!(
            first["text"].as_str().expect("text").contains('\u{fffd}'),
            "the split half is the replacement character in the text"
        );
        let mut joined = Vec::new();
        for answer in [&first, &second] {
            let encoded = answer["data_base64"].as_str().expect("the bytes");
            joined.extend(
                base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)
                    .expect("valid base64"),
            );
        }
        assert_eq!(joined, character, "the bytes rejoin into the character");
        assert_eq!(second["cursor"].as_u64(), Some(3));
    }

    #[test]
    fn the_panel_terminal_attaches_a_session_that_is_already_running() {
        // The route answers `{sessions:[…]}`; a reload must use that shape rather than leave the
        // canvas empty while the server-rendered card says a session exists.
        assert!(PANEL_TERMINAL_JS.contains("attachExisting"));
        assert!(PANEL_TERMINAL_JS.contains("answer.sessions"));
        assert!(PANEL_TERMINAL_JS.contains("alive === false"));
        assert!(
            PANEL_TERMINAL_JS.contains("wait_ms=15000"),
            "the panel read long-polls instead of asking again every 150 ms"
        );
    }

    #[test]
    fn the_audit_tail_skips_long_trailing_blank_windows() {
        let fixture = fixture("panel-audit-tail-trailing-blanks", None);
        let path = fixture.config.data.join("events.jsonl");

        for count in [0usize, 1, 200, 201] {
            let mut text = String::new();
            for id in 0..count {
                text.push_str(&format!("{}\n", json!({"id": id})));
            }
            text.push_str(&"\n".repeat(65_536));
            fs::write(&path, text).expect("the trailing-blank fixture is written");

            let tail = read_audit_tail(&path, 200).expect("the audit is readable");
            let expected: Vec<Value> = (count.saturating_sub(200)..count)
                .map(|id| json!({"id": id}))
                .collect();
            assert_eq!(tail.lines, expected);
            assert_eq!(tail.truncated, count > 200);

            let zero = read_audit_tail(&path, 0).expect("the audit is readable at limit zero");
            assert!(zero.lines.is_empty());
            assert_eq!(zero.truncated, count > 0);
        }
    }

    #[test]
    fn the_audit_tail_counts_interspersed_nonblank_lines() {
        let fixture = fixture("panel-audit-tail-interspersed-blanks", None);
        let path = fixture.config.data.join("events.jsonl");
        let mut text = String::new();
        for id in 0..250 {
            text.push_str(&format!(
                "{}\n",
                json!({"id": id, "padding": "x".repeat(600)})
            ));
            text.push_str(&" \t\n".repeat(4));
        }
        fs::write(&path, text).expect("the interspersed-blank fixture is written");

        let tail = read_audit_tail(&path, 200).expect("the audit is readable");
        let expected: Vec<Value> = (50..250)
            .map(|id| json!({"id": id, "padding": "x".repeat(600)}))
            .collect();
        assert_eq!(tail.lines, expected);
        assert!(tail.truncated);
    }

    #[test]
    fn the_audit_tail_preserves_malformed_and_unterminated_entries() {
        let fixture = fixture("panel-audit-tail-malformed-unterminated", None);
        let path = fixture.config.data.join("events.jsonl");

        for older in [false, true] {
            let mut text = String::new();
            if older {
                text.push_str("{\"id\":0}\n");
            }
            text.push_str("  damaged audit  \n");
            text.push_str(&"\n".repeat(65_536));
            text.push_str("{\"id\":2}");
            fs::write(&path, text).expect("the malformed and unterminated fixture is written");

            let tail = read_audit_tail(&path, 2).expect("the audit is readable");
            assert_eq!(
                tail.lines,
                vec![json!("  damaged audit  "), json!({"id": 2})]
            );
            assert_eq!(tail.truncated, older);
        }
    }

    #[test]
    fn the_audit_tail_is_bounded_and_reports_the_cut() {
        let fixture = fixture("panel-audit-tail", None);
        let path = fixture.config.data.join("events.jsonl");

        let tail = read_audit_tail(&path, 200).expect("a missing audit is an empty record");
        assert!(tail.lines.is_empty());
        assert!(!tail.truncated);

        fs::write(&path, b"").expect("the empty audit fixture is written");
        for limit in [200, 0] {
            let tail = read_audit_tail(&path, limit).expect("an empty audit is readable");
            assert!(tail.lines.is_empty());
            assert!(!tail.truncated);
        }

        // A file larger than the first read window: the reader must find the tail without parsing
        // the whole file, and it must know the lines before it are real.
        let mut text = String::new();
        for index in 0..5_000u64 {
            text.push_str(&format!(
                "{{\"id\":{index},\"event\":\"begin\",\"tool\":\"host.facts\"}}\n"
            ));
        }
        fs::write(&path, &text).expect("the audit fixture is written");
        let tail = read_audit_tail(&path, 200).expect("the audit is readable");
        assert_eq!(tail.lines.len(), 200);
        assert!(tail.truncated);
        assert_eq!(tail.lines[0]["id"], json!(4_800));
        assert_eq!(tail.lines[199]["id"], json!(4_999));

        // Exactly the bound is not a cut: the page must not say older lines are missing when none
        // are.
        let text: String = (0..200u64)
            .map(|index| {
                format!("{{\"id\":{index},\"event\":\"begin\",\"tool\":\"host.facts\"}}\n")
            })
            .collect();
        fs::write(&path, text).expect("the exact-bound fixture is written");
        let tail = read_audit_tail(&path, 200).expect("the audit is readable");
        assert_eq!(tail.lines.len(), 200);
        assert!(!tail.truncated);
        assert_eq!(tail.lines[0]["id"], json!(0));
    }

    #[tokio::test]
    async fn the_panel_timeline_reads_the_device_own_audit() {
        let fixture = fixture("panel-timeline", None);
        let mut text = String::new();
        for index in 0..250u64 {
            let at = 1_761_566_400_000 + index * 1_000;
            text.push_str(&format!(
                "{}\n{}\n",
                json!({
                    "id": index,
                    "ts_ms": at,
                    "event": "begin",
                    "tool": if index % 2 == 0 { "host.facts" } else { "runs.exec" },
                    "decision": "allow",
                    "transport": "loopback"
                }),
                json!({
                    "id": index,
                    "ts_ms": at + 500,
                    "event": "end",
                    "ok": index % 13 != 0,
                    "transport": "loopback"
                })
            ));
        }
        fs::write(fixture.config.data.join("events.jsonl"), text)
            .expect("the audit fixture is written");
        let server = PanelServer::start(fixture.app()).await;
        let (status, html) = get_text(&client(), &format!("{}/panel", server.base_url)).await;

        assert_eq!(status, StatusCode::OK);
        assert!(
            html.contains("The newest 200 lines; older lines are not shown."),
            "the route tells the page the read was cut: {html}"
        );
        let newest = if 249 % 2 == 0 {
            "host.facts"
        } else {
            "runs.exec"
        };
        assert!(
            html.contains(&format!(r#"<span class="timeline-tool">{newest}</span>"#)),
            "the newest call is in the record: {html}"
        );
        assert!(
            html.contains(r#"<span class="chip fail">failed</span>"#),
            "the audit's `ok: false` is a failed row: {html}"
        );
        assert!(html.contains(r#"<span class="timeline-caller">loopback</span>"#));
    }

    #[tokio::test]
    async fn the_rust_panel_renders_the_facts_the_html_page_shows() {
        let mut fixture = fixture_with_release(
            "panel-rust-page",
            None,
            Some("2.0.3"),
            Some("b1946ac92492d2347c6235b4d2611184"),
            Some(Channel::Stable),
        );
        fixture.config.port = 18_080;
        fixture.saved_state();
        let client = client();
        let server = PanelServer::start(fixture.app()).await;

        let (status, html) = get_text(&client, &format!("{}/panel", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        let token = panel_meta(&html).expect("GET /panel carries the panel token meta tag");
        assert_eq!(token.len(), 64, "the token is 32 bytes as hex: {token}");

        for fact in [
            "2.0.3",
            "stable",
            "b1946ac92492d2347c6235b4d2611184",
            "http://127.0.0.1:18080/mcp",
            "Connect a client",
            "Console",
            "Diagnostics",
            "Recent events",
            "schema",
            "lastVerdict",
            "<title>Summrise — this machine</title>",
        ] {
            assert!(
                html.contains(fact),
                "{fact:?} is missing from /panel: {html}"
            );
        }
        assert!(
            html.contains("pid "),
            "the build section prints the process: {html}"
        );
        // The command is PowerShell on Windows and a shell on Linux; the test asks the same
        // function the page asked instead of assuming the separator (this assertion is why the
        // Windows job was red while every Linux suite passed).
        let (token_file, command) = panel::token_facts(&fixture.config.data.display().to_string());
        assert!(
            html.contains(&command.replace('"', "&quot;")),
            "the read command names the token file ({command}): {html}"
        );
        assert!(
            html.contains(&token_file),
            "the token path is printed: {html}"
        );
        assert!(html.contains(r#"action="/panel/enroll""#));
        assert!(html.contains(r#"name="panel_token""#));

        // A live session is read from the same registry `/api/terminal` reads, and the page is
        // rendered at request time rather than cached: one created after the first GET shows up.
        let response = client
            .post(format!("{}/api/terminal/create", server.base_url))
            .header("x-summrise-panel", &token)
            .header("content-type", "application/json")
            .body(json!({ "kind": "pty" }).to_string())
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::OK);
        let created: Value =
            serde_json::from_str(&response.text().await.expect("the create answer is text"))
                .expect("the create answer is JSON");
        let id = created["id"].as_str().expect("a session id").to_owned();

        let (status, html) = get_text(&client, &format!("{}/panel", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        let start = html
            .find("id=\"terminal-sessions\"")
            .expect("the sessions list");
        // The card's own close, not a later card's title: the page's card order is the panel's to
        // decide, and the session list only has to be inside the Terminal card.
        let end = start
            + html[start..]
                .find("</section>")
                .expect("the terminal card closes");
        let sessions = &html[start..end];
        assert!(
            sessions.contains(&id) && sessions.contains("pty"),
            "the live session is in its section: {sessions}"
        );

        let _ = client
            .post(format!("{}/api/terminal/{id}/close", server.base_url))
            .header("x-summrise-panel", &token)
            .send()
            .await;
    }

    #[tokio::test]
    async fn the_rust_panel_refuses_a_non_loopback_peer() {
        let fixture = fixture("panel-rust-non-loopback", None);
        let app = fixture.app();
        let remote = SocketAddr::from(([203, 0, 113, 10], 40_123));

        let mut request = get("/panel", None);
        request.extensions_mut().insert(ConnectInfo(remote));
        let (status, answer) = send(app.clone(), request).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            answer,
            json!({ "error": "loopback_only" }),
            "the rendered page carries the token, so a remote peer gets no page"
        );

        let mut request = Request::builder()
            .method("POST")
            .uri("/panel/enroll")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(
                "console_url=http%3A%2F%2F127.0.0.1%3A1&code=a&panel_token=b",
            ))
            .expect("the request builds");
        request.extensions_mut().insert(ConnectInfo(remote));
        let (status, answer) = send(app, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(answer, json!({ "error": "loopback_only" }));
    }

    #[tokio::test]
    async fn the_panel_enroll_form_redirects_with_the_outcome() {
        let fixture = fixture("panel-form-enroll", None);
        let console = MockConsole::start().await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("a client that leaves the redirect visible");
        let server = PanelServer::start(fixture.app()).await;
        let (status, html) = get_text(&client, &format!("{}/panel", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        let token = panel_meta(&html).expect("the form carries this boot's token");
        let code = "a".repeat(64);

        // A form with the wrong hidden token must not reach the console.
        let wrong = format!(
            "{}{}",
            if token.starts_with('0') { '1' } else { '0' },
            &token[1..]
        );
        let response = client
            .post(format!("{}/panel/enroll", server.base_url))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(format!(
                "console_url={}&code={code}&panel_token={wrong}",
                console.base_url
            ))
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(
            console.requests().is_empty(),
            "a form without this boot's token writes nothing and sends nothing"
        );

        let response = client
            .post(format!("{}/panel/enroll", server.base_url))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(format!(
                "console_url={}&code={code}&panel_token={token}",
                console.base_url
            ))
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/panel?enrolled=1")
        );
        assert_eq!(console.requests().len(), 1, "the real enrollment ran");

        let (status, page) =
            get_text(&client, &format!("{}/panel?enrolled=1", server.base_url)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(page.contains(r#"class="enroll-status ok""#));
        assert!(page.contains("Enrolled."));

        // A second enrollment is a conflict until the form's force box is ticked; the redirect says
        // which code the page should explain.
        let response = client
            .post(format!("{}/panel/enroll", server.base_url))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(format!(
                "console_url={}&code={}&panel_token={token}",
                console.base_url,
                "b".repeat(64)
            ))
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/panel?error=token_conflict")
        );

        let (status, page) = get_text(
            &client,
            &format!("{}/panel?error=token_conflict", server.base_url),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(page.contains(r#"class="enroll-status error""#));
        assert!(page.contains("Enrolment failed (token_conflict)"));
        assert!(page.contains("a console token already exists"));

        // The force checkbox is a form field, not a JSON boolean: `on` must reach `enroll`'s force.
        let response = client
            .post(format!("{}/panel/enroll", server.base_url))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(format!(
                "console_url={}&code={}&force=on&allow_plaintext=on&panel_token={token}",
                console.base_url,
                "c".repeat(64)
            ))
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/panel?enrolled=1"),
            "force rotates the existing enrollment"
        );
        assert_eq!(
            console.requests().len(),
            2,
            "the checked boxes reach the real enrollment"
        );

        // A field the form needs but did not carry is still a redirect, so the page can explain it.
        let response = client
            .post(format!("{}/panel/enroll", server.base_url))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(format!("panel_token={token}"))
            .send()
            .await
            .expect("the device answers");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/panel?error=usage")
        );
        assert_eq!(
            console.requests().len(),
            2,
            "an incomplete form never reaches the console"
        );
    }

    #[test]
    fn a_checkbox_is_true_when_present_and_false_when_absent() {
        assert!(!checked(None));
        assert!(!checked(Some("")));
        assert!(!checked(Some("0")));
        assert!(!checked(Some("false")));
        assert!(checked(Some("on")));
        assert!(checked(Some("1")));
    }

    #[tokio::test]
    async fn the_panel_serves_its_own_terminal_assets() {
        // The terminal is xterm now, embedded in this binary; a build without the assets is a page
        // whose terminal never opens. These are the three routes the page asks for.
        let fixture = fixture("panel-assets", None);
        for (path, content_type, marker) in [
            (
                "/panel/xterm.js",
                "text/javascript; charset=utf-8",
                "Terminal",
            ),
            (
                "/panel/addon-fit.js",
                "text/javascript; charset=utf-8",
                "FitAddon",
            ),
            ("/panel/xterm.css", "text/css; charset=utf-8", "xterm"),
            (
                "/panel/panel-terminal.js",
                "text/javascript; charset=utf-8",
                "term-host",
            ),
        ] {
            let response = fixture
                .app()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .body(Body::empty())
                        .expect("a request"),
                )
                .await
                .expect("the asset answers");
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(
                response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok()),
                Some(content_type),
                "{path}"
            );
            let body = axum::body::to_bytes(response.into_body(), 8 << 20)
                .await
                .expect("a body");
            assert!(
                body.windows(marker.len())
                    .any(|window| window == marker.as_bytes()),
                "{path} carries its marker"
            );
        }
    }
}
