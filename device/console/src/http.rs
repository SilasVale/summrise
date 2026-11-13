//! The control surface: login, enrollment, the device registry, grant signing, permanent-token
//! management and health.
//!
//! It holds the keys (architecture of record, §0.4): the operator secret, the grant key, the
//! session list and the device channels. The page role is [`summrise_console_page`]; this module
//! implements its read-only seam, mounts its routes, and keeps the operator gate in front of the
//! one page route that needs a caller it can authenticate.
//!
//! One router, built once from a loaded [`Console`]. The handlers are thin on purpose: the decisions
//! live in `auth`, `codes`, `devices` and `grants`, and the router only turns refusals into status
//! codes.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, FromRequestParts, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use ed25519_dalek::{Signature, SigningKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Digest;

use crate::audit::{self, Absence, Audit, Caller};
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
use crate::tokens::{Token, TokenError, TokenKind, TokenMeta, Tokens};
use summrise_console_page::{Page, PageSource};

/// How far a device's clock may be from the console's when it registers: five minutes.
const REGISTRATION_SKEW_MS: u64 = 5 * 60 * 1000;

/// How many log lines `GET /audit` and `GET /denials` return when the caller names no limit.
const DEFAULT_LOG_LIMIT: usize = 100;

/// The largest limit the log routes honor: an operator reading an index must not make the console
/// hold an unbounded slice of it.
const MAX_LOG_LIMIT: usize = 1000;

/// The directory under `--data` where the deployment places the release's Windows installer.
///
/// There is no CLI flag for it: the installer is a file the deployment gives the console, and a
/// missing directory is a normal state — a release that carries no setup binary.
const INSTALLER_DIR: &str = "installer";

/// The name every installer candidate carries: `SummriseSetup-<version>.exe`.
const INSTALLER_PREFIX: &str = "SummriseSetup-";
const INSTALLER_SUFFIX: &str = ".exe";

/// The largest installer these routes will serve: 128 MiB.
///
/// A console is a broker, not a file server. A file dropped under the right name is read into
/// memory for its hash; anything larger than this is treated as no installer at all.
const MAX_INSTALLER_BYTES: u64 = 128 * 1024 * 1024;

/// The header a trusted reverse proxy uses to name the operator it let in.
///
/// Not a credential on its own: the console believes it only when the connection arrives from the
/// address `--trusted-proxy` names, and the proxy overwrites whatever the browser sent. A request
/// from anywhere else is asked for the operator token even when it carries this header.
const PROXY_IDENTITY_HEADER: &str = "x-forwarded-user";

/// How long the console tells an operator's client to wait before retrying a reconnecting device.
const RETRY_AFTER_MS: u64 = 1_000;

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
    /// The calls the proxy is still waiting on, for the live stream's running line. Memory only:
    /// it is not the audit and nothing here is written to a file.
    /// Shared with the page role, whose `/events` snapshot names the same in-flight calls.
    running: Arc<crate::running::Running>,
    /// The page role: the built page and the only three reads it is handed. Built at load so the
    /// version is stamped before the first request; the router only mounts it.
    page: Arc<Page>,
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

/// The page role's reads, over the control role's own handles.
///
/// This is the whole surface the page crate is handed: the live index it follows, the calls still
/// in flight, and the clock. It holds no decision and no credential — the gate, the registry and the
/// signing key stay in this module.
struct PageReads {
    audit: Audit,
    running: Arc<crate::running::Running>,
    now: Now,
}

impl PageSource for PageReads {
    fn follow(&self) -> tokio::sync::broadcast::Receiver<Value> {
        self.audit.follow()
    }

    /// The page's snapshot seam is synchronous by design, so this reads the file without awaiting.
    /// It is the same read as `Audit::newest`, including the one rotated generation.
    fn newest(&self, limit: usize) -> Vec<Value> {
        self.audit.newest_blocking(limit)
    }

    fn running(&self, now_ms: u64) -> Vec<Value> {
        self.running.current(now_ms)
    }

    fn now_ms(&self) -> u64 {
        (self.now)()
    }

    fn events_after(&self, device: &str, seq: u64) -> Vec<Value> {
        self.audit.events_after(device, seq)
    }

    fn sentence(&self, line: &Value) -> String {
        crate::activity::line_sentence(line)
    }
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
        // One clock for the console and the channel registry: the audit's lines, timestamped here,
        // are what the fleet subtracts, so the registry must not read the wall clock itself.
        let now: Now = Arc::new(now);
        let audit = Audit::new(&config.data, audit_max_bytes);
        let running = Arc::new(crate::running::Running::new(RUNS_MAX_PROXY_WAIT));

        // The page is stamped with this build's version before it is ever served (the hundred-and-ninety-ninth list).
        // `Page::new` is the one stamp: the page crate fixes the claim before the first request.
        let page = Arc::new(Page::new(
            Arc::new(PageReads {
                audit: audit.clone(),
                running: running.clone(),
                now: now.clone(),
            }),
            &config.version,
        ));
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
                channels: ChannelRegistry::recording(audit.clone(), now.clone()),
                audit,
                denials: Denials::new(&config.data),
                running,
                page,
                mcp_timeout,
                now,
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
    ///
    /// Two roles, one router. The control routes below hold the keys; the page role was built at
    /// load from the only three handles it may hold (the audit, the calls in flight, the clock) and
    /// is mounted from [`summrise_console_page::router`]. Its one gated route, `/events`, is wrapped
    /// here by [`page_operator_gate`]: only the control role can tell an operator from a stranger.
    pub fn router(&self) -> Router {
        let page = self.inner.page.clone();
        let control: Router = Router::new()
            .route("/health", get(health))
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
            // **THE INSTALLER THE CONSOLE WAS GIVEN.** The deployment stages
            // `SummriseSetup-<version>.exe` in `<data>/installer/`, and these two routes turn that
            // file into the Add-a-machine flow's download: the facts a person checks first, then
            // the bytes. The page is the operator's own, so the session `/audit` needs is the gate
            // here too. A console whose deployment staged nothing says so; it does not fail.
            .route("/installer", get(installer_facts))
            .route("/installer/download", get(installer_download))
            // **A RANGE LEAVES AS A FILE.** `/audit?from=&to=` draws a range; this answers the same
            // lines as JSON Lines, oldest first, with the filename and headers that make the file a
            // portable fact instead of a screenshot of a page. Bounded by the same scan rule as the
            // query, and a range the bound cannot prove is refused rather than silently shortened.
            .route("/audit/export", get(export_audit))
            // **WHAT THE WINDOW READS.** `/audit` answers lines; this answers the file those lines
            // came from — its name, size, both ends, the one older generation, and the retention
            // rule as configured. The page states it once beside the cap sentence: that sentence
            // says what is drawn, this fact says what is read.
            .route("/audit/source", get(audit_source))
            // **THE DAY, NOT THE WINDOW.** `/audit` is a bounded window; this is the local calendar
            // day, scanned backwards from the newest line to the same midnight the page's timeline
            // cuts at. The hub's day line and Activity's header read it; the window sentence keeps
            // describing the fetched window.
            .route("/audit/day", get(audit_day))
            // **THE JOIN ACROSS THE TWO RECORDS.** One proxied call is a console line and a machine
            // journal pair that share `trace.call`. Only the machine can say which pairs it still
            // holds, so this route asks its own `journal.get` over the channel, once per Activity
            // window, and relays the answer. It is a read about the window, not an operator action:
            // it writes no audit line and no live event.
            .route("/audit/join", get(audit_join))
            // **WHAT THE MACHINE ITSELF RECORDED.** Activity draws what the console proxied, but a
            // call that never came through here — the device's own panel, a mesh peer, a direct
            // loopback client — lives only in the machine's journal, and after a reload it exists
            // nowhere the page can reach. The machine summarises the window; this route asks for it
            // and relays the rows field by field. It is a read about the window the page draws, not
            // an operator action: it writes no audit line and no live event, and the trace it mints
            // is what keeps the machine's own `call.*` pair for the read out of Activity.
            .route("/journal/read", get(journal_read))
            .route("/journal/day", get(journal_day))
            // **THE FRAME ITSELF, NOT A DATA URL.** A screenshot can be megabytes; the event carries
            // a same-origin path and this route answers the bytes, behind the same operator gate the
            // page and `/audit` already use. The store is memory only, so an id that a restart or an
            // older capture forgot is a JSON 404, never a broken image.
            .route("/activity/shot/:device/:id", get(activity_shot))
            // The same read as `/audit`, over the unauthenticated-refusal log. Operator-only: the
            // log holds what probing looks like, and it is surfaced read-only.
            .route("/denials", get(list_denials))
            .fallback(not_found)
            .with_state(self.inner.clone());
        control
            .merge(summrise_console_page::router(page))
            // The page holds no credential and cannot authenticate anyone. The operator gate belongs
            // to this role's state, and it asks only the page route that needs it.
            .layer(axum::middleware::from_fn_with_state(
                self.inner.clone(),
                page_operator_gate,
            ))
            .layer(axum::middleware::from_fn(no_store))
            .layer(axum::middleware::from_fn_with_state(
                self.inner.clone(),
                access_sign_in,
            ))
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

/// The facts one audit line names. One shape, named at the call site: the `Option`s are different
/// facts, not interchangeable positionals. `caller` is present only on a line a proxied call wrote.
struct AuditFacts<'a> {
    kind: &'static str,
    device_id: Option<&'a str>,
    status: StatusCode,
    detail: Option<&'a str>,
    tool: Option<&'a str>,
    token_id: Option<&'a str>,
    caller: Option<Caller>,
    /// The `trace.call` of the proxied exchange this line records, when the line is one.
    ///
    /// It joins the console's line to the device journal's `call.started`/`call.finished|denied`
    /// pair, which share it. An operator action (a grant, a token, a sign-in) proxied nothing, so
    /// it has none.
    trace: Option<&'a str>,
    /// The machine journal's `seq` for this call's outcome half, when the device named one.
    machine_seq: Option<u64>,
}

/// Who a proxied call belongs to, and the permanent token record that proved it, when one did.
///
/// The two facts travel together because both describe the caller and both go on every line and
/// event the exchange writes. `token_id` is `None` for the operator's session and for a grant, so
/// `caller` is what tells those two apart: the operator page's `/mcp` is the console's own call, a
/// grant on `/mcp/:id` is a client's.
#[derive(Clone, Copy)]
struct CallIdentity<'a> {
    caller: Caller,
    token_id: Option<&'a str>,
    /// True when the console's own poller made the call rather than a person. A poll's successful
    /// read of a pollable tool is not recorded at all; its failure is a `poll` line, and no part of
    /// it reaches the live stream. Only the operator's `/mcp` route and the terminal stream can set
    /// it; an AI client's routes always pass `false`.
    poll: bool,
}

/// One proxied call, as the live stream describes it.
///
/// `detail` is the failure sentence the evidence panel shows — the device's own message, or the
/// console's reason for giving up — and `ok` is not the status: a device's JSON-RPC error is HTTP
/// 200 and still a refusal. The audit line keeps only the numeric code; this is what a person reads.
/// `token_id` is the same record id the audit line carries: the live event is the newest copy of a
/// line, so dropping the token here would make the merge show a tokenless row over a named one.
/// `trace` travels for the same reason: it is the one value that joins the row to the machine's
/// journal `call.*` pair, and the page reads the live copy first.
struct LiveCall<'a> {
    device_id: Option<&'a str>,
    tool: Option<&'a str>,
    sentence: &'a str,
    status: StatusCode,
    ok: bool,
    detail: &'a str,
    ms: u64,
    evidence: Option<Value>,
    caller: Caller,
    token_id: Option<&'a str>,
    /// The `trace.call` this exchange is known by, on the frame, the audit line and the journal.
    trace: Option<&'a str>,
    /// The machine journal's `seq` for this call's outcome half, when the device named one.
    machine_seq: Option<u64>,
}

/// A running call, cleared when the proxy stops waiting.
///
/// `Drop` is the point: an answer, a timeout, a refusal after the frame left, and a client that
/// disconnected mid-wait all clear the announcement the same way, so no exit can leave a phantom.
/// Publishing is a broadcast, which is safe from a destructor.
struct RunningCall<'a> {
    app: &'a Inner,
    id: String,
}

impl Drop for RunningCall<'_> {
    fn drop(&mut self) {
        self.app.finish_call(&self.id);
    }
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
                caller: facts.caller,
                trace: facts.trace,
                machine_seq: facts.machine_seq,
            })
            .await;
        // A call's live event is published by the proxy, which has the call's arguments; everything
        // else (a grant, a token, a refusal) is described from what this line already holds. These
        // events are live too: a row a page just watched arrive is not a row read back from the
        // audit file, even when the tool behind it has no evidence to show. A `poll` line is the
        // exception: the console's own retry is not work to watch, so it stays in the file (the
        // Activity Table draws it) and never arrives as a live event.
        if facts.kind != "mcp" && facts.kind != "poll" {
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
            if let Some(trace) = facts.trace {
                event["trace"] = json!(trace);
            }
            if let Some(machine_seq) = facts.machine_seq {
                event["machine_seq"] = json!(machine_seq);
            }
            if let Some(detail) = facts.detail {
                event["detail"] = json!(detail);
            }
            if let Some(caller) = facts.caller {
                event["caller"] = json!(caller);
            }
            if let Some(token_id) = facts.token_id {
                event["token_id"] = json!(token_id);
            }
            let sentence = crate::activity::line_sentence(&event);
            event["sentence"] = json!(sentence);
            self.audit.publish(event);
        }
    }

    /// Publish one proxied call to the live stream: the sentence, the tool, the device, the outcome,
    /// why it failed, how long the device took, whether the console or a client made the call, the
    /// token record when one authenticated it, and the trace both records are joined by. The newest
    /// rows are these events, so a field the audit line has and the event lacks disappears exactly
    /// where the page reads first.
    fn publish_call(&self, call: LiveCall<'_>) {
        let mut event = json!({
            "ts_ms": (self.now)(),
            "kind": "mcp",
            "status": call.status.as_u16(),
            "ok": call.ok,
            "live": true,
            "ms": call.ms,
            "sentence": call.sentence,
            "caller": call.caller,
        });
        if let Some(device_id) = call.device_id {
            event["device_id"] = json!(device_id);
        }
        if let Some(tool) = call.tool {
            event["tool"] = json!(tool);
        }
        if let Some(token_id) = call.token_id {
            event["token_id"] = json!(token_id);
        }
        if let Some(trace) = call.trace {
            event["trace"] = json!(trace);
        }
        if let Some(machine_seq) = call.machine_seq {
            event["machine_seq"] = json!(machine_seq);
        }
        if !call.detail.is_empty() {
            event["detail"] = json!(call.detail);
        }
        if let Some(evidence) = call.evidence {
            event["evidence"] = evidence;
        }
        self.audit.publish(event);
    }

    /// Publish one finished call unless it was the console's own poll.
    ///
    /// A poll is the console reading the screen it already shows: the record keeps only its failure,
    /// and even that is not work to watch live. Every ordinary call takes [`Inner::publish_call`].
    fn publish_work(&self, poll: bool, call: LiveCall<'_>) {
        if !poll {
            self.publish_call(call);
        }
    }

    /// Announce one proxied call on the live stream, before the wait for the device.
    ///
    /// **THIS IS NOT AN AUDIT LINE.** The frame goes to [`Audit::publish`], which only broadcasts;
    /// nothing is appended to the file, because a call that has not finished is not a record. The
    /// returned guard clears it however the wait ends.
    fn start_call(&self, caller: Caller, device_id: Option<&str>, tool: &str) -> RunningCall<'_> {
        let started = self.running.start((self.now)(), caller, device_id, tool);
        self.audit.publish(json!({
            "started": started.value(),
            "caller": caller,
        }));
        RunningCall {
            app: self,
            id: started.id().to_owned(),
        }
    }

    /// Publish a call's clear and forget it. The snapshot must stop naming the call too, or a page
    /// that reconnects would keep a phantom from the registry's copy.
    fn finish_call(&self, id: &str) {
        self.running.finish(id);
        self.audit.publish(json!({ "finished": { "id": id } }));
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
            caller: None,
            trace: None,
            machine_seq: None,
        })
        .await;
    }

    /// Audit an **authenticated** refusal as `denied` and hand the error back.
    async fn deny(&self, device_id: Option<&str>, error: ApiError) -> ApiError {
        self.deny_from(None, device_id, error).await
    }

    /// [`Inner::deny`] for a route that knows the refusal's audience.
    ///
    /// `None` is an operator action (a grant, a token, a sign-in) or a line older than the field; the
    /// page treats both as "not an AI client's call", never as one.
    async fn deny_from(
        &self,
        caller: Option<Caller>,
        device_id: Option<&str>,
        error: ApiError,
    ) -> ApiError {
        self.record_facts(AuditFacts {
            kind: "denied",
            device_id,
            status: error.status,
            detail: Some(error.code),
            tool: None,
            token_id: None,
            caller,
            trace: None,
            machine_seq: None,
        })
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
                caller: None,
                trace: None,
                machine_seq: None,
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

/// The tool result inside a device answer: MCP's `{content, isError}`.
///
/// A device's answer to `tools/call` is its JSON-RPC response envelope
/// (`{"jsonrpc":"2.0","id":N,"result":{...}}`), and the console forwards that envelope unchanged to
/// `/mcp`, where the page reads `body.result.isError` (`terminal.ts`). The audit must read the same
/// nesting. An unwrapped result is accepted too, so a caller that already picked the result out does
/// not have to rebuild the envelope.
fn tool_result(reply: &Reply) -> Option<&Value> {
    let Reply::Result(response) = reply else {
        return None;
    };
    Some(response.get("result").unwrap_or(response))
}

/// The first text item of a tool result, when it carries one.
///
/// MCP's tool result is `{content, isError}`; a run puts its JSON body in the first text item, and
/// every tool-level refusal puts its sentence there.
fn tool_text(reply: &Reply) -> Option<&str> {
    tool_result(reply)?
        .get("content")?
        .get(0)?
        .get("text")?
        .as_str()
}

/// The run body a `runs.exec` answer carries, when its text is a run's JSON.
fn run_body(reply: &Reply) -> Option<Value> {
    serde_json::from_str(tool_text(reply)?).ok()
}

/// Whether a `runs.exec` answer's own facts say the run failed.
///
/// **ONLY A ZERO EXIT STATUS IS SUCCESS.** A device that predates the `isError` rule answers a
/// failed run as an ordinary value, and the console still has to record it as a failure: the
/// record's whole job is to answer which calls failed. A null exit code is not success either —
/// the timeout killed the process group (`timed_out`), or a signal ended it.
fn run_failed(reply: &Reply) -> bool {
    let Some(run) = run_body(reply) else {
        return false;
    };
    if run
        .get("timed_out")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }
    match run.get("exit_code") {
        Some(exit_code) => exit_code.as_i64() != Some(0),
        None => false,
    }
}

/// The status the audit gives a tool that answered with a failure.
///
/// MCP carries a tool-level failure inside a successful exchange: the result has `isError: true`
/// and the reason in its text, and it is still HTTP 200 because a non-2xx would look like a
/// transport failure to an MCP client. A device that predates that rule answers a refused
/// `tools/call` with a JSON-RPC error envelope instead. Both are a refusal here. **424 Failed
/// Dependency** is this console's word for "the call reached the tool and the tool refused": not a
/// transport answer (5xx), and not a green row.
const TOOL_REFUSED_STATUS: StatusCode = StatusCode::FAILED_DEPENDENCY;

/// Why a `tools/call` is a failure, when it is.
///
/// Two wire shapes mean the same thing and both must be recorded as a failure:
///
/// - MCP's tool-level error: a result with `isError: true`, reason in its text. The result sits
///   inside the device's JSON-RPC response envelope.
/// - An older device's answer: a JSON-RPC error envelope for a `tools/call`. A refusal is the only
///   reason a device has to answer a well-formed tool call with an envelope, so it is a refusal
///   too. A protocol error on `ping`/`initialize`/`tools/list` is not a tool call and keeps 200.
///
/// A `runs.exec` that exited non-zero (or that the timeout killed) is a failure even when a device
/// predates `isError`: the run's own exit facts say so.
///
/// The reason is bounded exactly like the device messages other details keep — the audit never
/// carries a payload body, and a tool's error text is the tool's content.
fn tool_failure(reply: &Reply, tool: Option<&str>) -> Option<String> {
    let tool = tool?;
    if let Reply::Error(_) = reply {
        // The envelope's message is the device's own sentence. Its `data` is still not the audit's:
        // only the message, bounded, is kept.
        return Some(
            error_message(reply)
                .map(bounded_detail)
                .or_else(|| reply_detail(reply))
                .unwrap_or_else(|| "the tool refused".to_owned()),
        );
    }
    let result = tool_result(reply)?;
    let marked = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let failed_run = tool == "runs.exec" && run_failed(reply);
    if !marked && !failed_run {
        return None;
    }
    // A run's own metadata is the better reason when its body parses; otherwise the device's text
    // is already the sentence a client would show.
    let reason = if tool == "runs.exec" {
        run_detail(reply)
    } else {
        None
    }
    .or_else(|| tool_text(reply).map(bounded_detail))
    .unwrap_or_else(|| "the tool refused".to_owned());
    Some(bounded_detail(&reason))
}

/// The outcome of a `runs.exec`, as the audit keeps it: the run id, its exit and how long it took.
///
/// The output is deliberately not copied here: the device's history never stored it, and the audit
/// is a record of calls, not a second run log. This is the metadata an operator can use to find the
/// run in the Runs page; `runs.get` is still where the record lives.
fn run_detail(reply: &Reply) -> Option<String> {
    let Reply::Result(_) = reply else {
        return reply_detail(reply);
    };
    let run = run_body(reply)?;
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
    /// Present only when nothing was written and a retry is safe: the body's `retry_after_ms`
    /// and the HTTP `Retry-After` header carry the same fact.
    retry_after_ms: Option<u64>,
    /// The `WWW-Authenticate` challenge, on the 401s whose credential is a bearer.
    challenge: Option<&'static str>,
}

/// The challenge for a door that wants a bearer and was given none.
const BEARER_CHALLENGE: &str = "Bearer realm=\"summrise-console\"";

/// The challenge for a door that was given a bearer it does not accept.
const INVALID_BEARER_CHALLENGE: &str = "Bearer realm=\"summrise-console\", error=\"invalid_token\"";

impl ApiError {
    fn unauthorized() -> Self {
        ApiError {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: None,
            retry_after_ms: None,
            challenge: None,
        }
    }

    fn forbidden(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::FORBIDDEN,
            code,
            message: None,
            retry_after_ms: None,
            challenge: None,
        }
    }

    fn bad_request(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::BAD_REQUEST,
            code,
            message: None,
            retry_after_ms: None,
            challenge: None,
        }
    }

    fn not_found(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::NOT_FOUND,
            code,
            message: None,
            retry_after_ms: None,
            challenge: None,
        }
    }

    fn too_many(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            code,
            message: None,
            retry_after_ms: None,
            challenge: None,
        }
    }

    fn unavailable(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code,
            message: None,
            retry_after_ms: None,
            challenge: None,
        }
    }

    fn bad_gateway(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::BAD_GATEWAY,
            code,
            message: None,
            retry_after_ms: None,
            challenge: None,
        }
    }

    fn gateway_timeout(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::GATEWAY_TIMEOUT,
            code,
            message: None,
            retry_after_ms: None,
            challenge: None,
        }
    }

    fn internal() -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal",
            message: None,
            retry_after_ms: None,
            challenge: None,
        }
    }

    fn internal_code(code: &'static str) -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code,
            message: None,
            retry_after_ms: None,
            challenge: None,
        }
    }

    /// Attach the sentence a person reads. It names the next action, never only the failure.
    fn saying(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Mark the refusal as safe to retry, and say how long the caller should wait.
    fn retrying(mut self, retry_after_ms: u64) -> Self {
        self.retry_after_ms = Some(retry_after_ms);
        self
    }

    /// The 401 for a bearer door that was given no bearer: a challenge, no `invalid_token`.
    fn challenging(mut self) -> Self {
        self.challenge = Some(BEARER_CHALLENGE);
        self
    }

    /// The 401 for a bearer door that was given a bearer it does not accept.
    fn with_invalid_bearer(mut self) -> Self {
        self.challenge = Some(INVALID_BEARER_CHALLENGE);
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = json!({ "error": self.code });
        if let Some(message) = self.message {
            body["message"] = json!(message);
        }
        if let Some(retry_after_ms) = self.retry_after_ms {
            body["retry_after_ms"] = json!(retry_after_ms);
        }
        let mut response = (self.status, Json(body)).into_response();
        if let Some(retry_after_ms) = self.retry_after_ms {
            // `Retry-After` is in seconds; round up so a 250 ms wait is not advertised as zero.
            let seconds = retry_after_ms.div_ceil(1_000).max(1);
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_str(&seconds.to_string()).expect("a decimal Retry-After"),
            );
        }
        if let Some(challenge) = self.challenge {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static(challenge),
            );
        }
        response
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

/// The operator's gate over the page role's one authenticated route.
///
/// The page cannot authenticate a caller and holds no credential to try: [`summrise_console_page::router`]
/// mounts `/events` without a check, and this layer — installed by the control router, where the
/// identity lives — asks the same [`OperatorSession`] every other operator route asks, on that path
/// only. The page and its assets stay public by design; the audit stream is not.
///
/// The request is rebuilt rather than cloned: `OperatorSession` reads the peer address out of the
/// extensions, so the parts it inspected are the parts the handler receives.
async fn page_operator_gate(
    State(app): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if request.uri().path() != "/events" {
        return next.run(request).await;
    }
    let (mut parts, body) = request.into_parts();
    match OperatorSession::from_request_parts(&mut parts, &app).await {
        Ok(_) => {
            next.run(axum::extract::Request::from_parts(parts, body))
                .await
        }
        Err(refusal) => refusal.into_response(),
    }
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

/// Any path the router does not own. **A TYPED OR SHARED PATH IS A PERSON, NOT A BOT** (the hundred-and-forty-fourth list).
///
/// Measured 2026-11-12 against the live console: `GET /activity` answered **404 with an empty body** — a blank page. The page
/// routes in the URL's HASH (`/#/activity`), which is a fine decision for links this page makes and a trap for every link a
/// person types, pastes or shares: the URL names a destination and the console answers nothing at all. A path that LOOKS like a
/// destination now answers a redirect to `/#<path>`, so the page a person asked for is the page they get.
///
/// **AND EVERYTHING ELSE IS STILL A PLAIN 404.** A client probing an API must not be handed the page: `/api/…`, `/mcp`, the
/// device and audit namespaces and any path whose last segment carries a dot (a file) keep the old answer, which is the
/// honest one.
async fn not_found(uri: axum::http::Uri) -> Response {
    let path = uri.path();
    const NOT_A_PAGE: &[&str] = &[
        "/api",
        "/mcp",
        "/devices",
        "/fleet",
        "/audit",
        "/tokens",
        "/grants",
        "/events",
        "/installer",
        "/mesh",
        "/terminal",
        "/denials",
        "/health",
        "/app",
        "/next",
        "/session",
        "/logout",
        "/enroll",
        "/activity/shot",
        "/favicon",
    ];
    let looks_like_a_page = !NOT_A_PAGE.iter().any(|prefix| path.starts_with(prefix))
        && !path.rsplit('/').next().unwrap_or("").contains('.');
    if !looks_like_a_page {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut response = Response::new(axum::body::Body::empty());
    *response.status_mut() = StatusCode::SEE_OTHER;
    // **THE PAGE'S OWN FRAGMENT SPELLING** (`surface-link.ts`: `#activity`, `#device/<tab>`), not a path with a slash: the
    // first version of this redirect emitted `/#/activity`, the page did not recognise it and silently fell back to `#home`
    // — measured 2026-11-12 in a browser, one round after the redirect landed.
    let fragment = path.trim_start_matches('/');
    match format!("/#{fragment}").parse() {
        Ok(location) => {
            response
                .headers_mut()
                .insert(axum::http::header::LOCATION, location);
            response
        }
        // A path that cannot be a header is not a page this console can name; the plain answer is the safe one.
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// One installer the selection rule found: the file both installer routes answer with.
struct InstallerFile {
    path: PathBuf,
    filename: String,
    version: String,
    size_bytes: u64,
    modified: SystemTime,
}

/// The version an installer's filename carries, or `None` when the name is not the convention.
fn installer_version(filename: &str) -> Option<&str> {
    let version = filename
        .strip_prefix(INSTALLER_PREFIX)?
        .strip_suffix(INSTALLER_SUFFIX)?;
    // An empty middle is not a version, and a route must not report one.
    (!version.is_empty()).then_some(version)
}

/// The newest staged installer under `<data>/installer`, at the production cap.
fn newest_installer(data: &std::path::Path) -> Option<InstallerFile> {
    newest_installer_up_to(data, MAX_INSTALLER_BYTES)
}

/// The newest installer under `<data>/installer` no larger than `max_bytes`.
///
/// One rule serves both routes: a regular file whose name is `SummriseSetup-<version>.exe`, newest
/// modification time first, with the filename as the tie-break so two reads cannot disagree. A
/// missing or unreadable directory, an unreadable entry and an over-cap file are all `None`; the
/// routes then answer "no installer", never a 500. The cap is a parameter for the same reason the
/// audit cap is one: a unit test can exercise it with six bytes instead of a 128 MiB fixture.
fn newest_installer_up_to(data: &std::path::Path, max_bytes: u64) -> Option<InstallerFile> {
    let entries = std::fs::read_dir(data.join(INSTALLER_DIR)).ok()?;
    let mut newest: Option<InstallerFile> = None;
    for entry in entries.flatten() {
        // A directory or a link named like an installer is not one. `file_type` does not follow
        // links, so the console serves a file the deployment staged, not whatever a link names.
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let Ok(filename) = entry.file_name().into_string() else {
            continue;
        };
        let Some(version) = installer_version(&filename).map(str::to_owned) else {
            continue;
        };
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        // The cap is a property of the candidate: a file that is too large is not an installer
        // this console hands out, however new it is.
        if metadata.len() > max_bytes {
            continue;
        }
        // A moment the filesystem cannot state is the oldest possible one, not an invented now.
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        let candidate = InstallerFile {
            path: entry.path(),
            filename,
            version,
            size_bytes: metadata.len(),
            modified,
        };
        let wins = match &newest {
            None => true,
            Some(current) => {
                candidate.modified > current.modified
                    || (candidate.modified == current.modified
                        && candidate.filename > current.filename)
            }
        };
        if wins {
            newest = Some(candidate);
        }
    }
    newest
}

/// `GET /installer`: the release installer the console was given, and the facts a person needs
/// before downloading it.
///
/// The version is the filename's; the size and sha256 are the bytes the download would serve.
/// Nothing is cached and nothing is invented: a missing directory, and a file that vanished or
/// grew between selection and read, are the same `{"available": false}` answer the page can state
/// honestly.
async fn installer_facts(State(app): State<AppState>, _: OperatorSession) -> Json<Value> {
    let Some(installer) = newest_installer(&app.data) else {
        return Json(json!({ "available": false }));
    };
    match tokio::fs::read(&installer.path).await {
        // The facts are the bytes this answer hashed: if the file changed size after selection,
        // describing it would be describing a different file than the one read.
        Ok(bytes)
            if installer.size_bytes <= MAX_INSTALLER_BYTES
                && bytes.len() as u64 == installer.size_bytes =>
        {
            Json(json!({
                "available": true,
                "filename": installer.filename,
                "version": installer.version,
                "size_bytes": installer.size_bytes,
                "sha256": hex::encode(&sha2::Sha256::digest(&bytes)),
            }))
        }
        // The facts must not describe a file the download would refuse.
        _ => Json(json!({ "available": false })),
    }
}

/// `GET /installer/download`: the same file `/installer` described, as an attachment.
///
/// The page's link is same-origin, so the browser sends the HttpOnly session cookie and the gate
/// is the one every operator route has; the global middleware adds `no-store`. No installer is a
/// JSON 404 with a stable code, which the page can tell from an edge refusal.
async fn installer_download(State(app): State<AppState>, _: OperatorSession) -> Response {
    let Some(installer) = newest_installer(&app.data) else {
        return ApiError::not_found("no_installer").into_response();
    };
    let bytes = match tokio::fs::read(&installer.path).await {
        Ok(bytes) if bytes.len() as u64 <= MAX_INSTALLER_BYTES => bytes,
        _ => return ApiError::not_found("no_installer").into_response(),
    };
    let disposition = format!("attachment; filename=\"{}\"", installer.filename);
    let Ok(disposition) = HeaderValue::from_str(&disposition) else {
        // A name that cannot be a header value cannot be handed out safely.
        return ApiError::not_found("no_installer").into_response();
    };
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            ),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        axum::body::Bytes::from(bytes),
    )
        .into_response()
}

/// `GET /health`: unauthenticated, and deliberately so — a supervisor and a human need one route
/// that answers before any credential has been presented. It reads only counts, never a secret.
///
/// It is control, not page: it reports the registry's count, each channel's liveness, the peak
/// outbound queue and whether Access is configured — facts the page role must not hold.
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
/// untouched. It answers 200 with `{"access_logout": <url or null>}`: an Access identity has no
/// in-memory token (the identity provider owns that session), so the URL is how the page ends the
/// edge's session; a request that arrived with a token or a cookie alone gets `null`, because there
/// is no edge session to end. The page clears regardless of the answer. No audit line on purpose —
/// session lifecycle is not a device action.
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
    // The cookie is ours; the identity is the edge's. Behind Access, clearing only ours is undone by
    // the next request the edge authenticates, so the answer names the edge's logout for the page to
    // follow — but only for a request that actually carried an assertion. A token-only operator on an
    // Access-configured console never had an edge session, and sending them to Cloudflare's logout
    // would be a dead end (this is what `/next/logout` did with a redirect before the maud pages were
    // deleted).
    let access_logout = match &app.access {
        Some(access) if crate::access::Access::carried(&headers) => access
            .issuer()
            .map(|issuer| format!("{}/cdn-cgi/access/logout", issuer.trim_end_matches('/'))),
        _ => None,
    };
    let mut response = Json(json!({ "access_logout": access_logout })).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        clear_session_cookie(secure_request(&headers)),
    );
    response
}

/// The sentence a refused operator token needs: what to type, and where the right value lives.
fn bad_operator_token() -> ApiError {
    ApiError::unauthorized().saying(
        "that token was not accepted; the operator token is the 64 hex characters in the file \
         named by --operator-token-file",
    )
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
                    .refuse_as(
                        None,
                        ApiError::unauthorized().saying(
                            "that sign-in code was not accepted; it may have expired or already \
                             been used. Mint a new one with --login-link",
                        ),
                        "bad_login_code",
                    )
                    .await);
            }
        }
    } else {
        let Some(presented) = token.and_then(hex::decode_32) else {
            return Err(app
                .refuse_as(None, bad_operator_token(), "bad_operator_token")
                .await);
        };
        constant_time_eq(&presented, &app.operator_token)
    };
    if !accepted {
        return Err(app
            .refuse_as(None, bad_operator_token(), "bad_operator_token")
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
    poll: bool,
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
        CallIdentity {
            caller: Caller::Console,
            token_id: None,
            poll,
        },
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
    poll: bool,
) -> Result<Value, Box<Response>> {
    let text = device_tool(app, device_id, tool, arguments, poll).await?;
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
            .deny_from(
                Some(Caller::Console),
                audit_device_id(&device_id).as_deref(),
                error,
            )
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
        // The subscribe's one audited read is a call like any other: its trace goes on the frame so
        // the device's journal pair names it, and the relay drops that pair rather than drawing the
        // console's own read twice.
        let trace = app.channels.mint_trace();
        if let Ok((sub, stream, ack)) = app
            .channels
            .subscribe(
                &device_id,
                session_id,
                query.from,
                read,
                &RequestAuth::Token(&scope),
                &trace,
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
            match terminal_tool(&state.app, &state.device, "terminal.read", arguments, true).await {
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
    /// The ack's read, consumed as the stream's first frame.
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
        Reply::Error(error) => Some(json!({ "error": reply_error_sentence(error) })),
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
        loop {
            if state.done {
                return None;
            }
            // The ack is the read the device already answered; every later frame is pushed. A quiet
            // follow pushes nothing, so a receive that finds no frame is the one true end.
            let frame = match state.first.take() {
                Some(frame) => frame,
                None => match state.stream.recv().await {
                    Some(frame) => frame,
                    None => return None,
                },
            };
            match frame_step(&frame) {
                FrameStep::Send(event) => return Some((Ok::<Event, Infallible>(event), state)),
                FrameStep::Last(event) => {
                    state.done = true;
                    return Some((Ok::<Event, Infallible>(event), state));
                }
                // An empty read that is still alive is not an end: wait for the next frame.
                FrameStep::Skip => {}
            }
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

/// What one frame from a subscribed device contributes to the stream.
enum FrameStep {
    /// Send this event and keep following.
    Send(Event),
    /// Send this event and end the stream: an `end` or a `fault`.
    Last(Event),
    /// An empty read that is still alive: no event; wait for the next frame.
    Skip,
}

/// One frame from the device as the stream's next step.
///
/// A pushed `error` is the follow's fault; every other frame is the read-shaped answer the ack and
/// the pushes share.
fn frame_step(frame: &Value) -> FrameStep {
    if let Some(sentence) = frame.get("error").and_then(Value::as_str) {
        let payload = json!({ "sentence": sentence, "status": 502 });
        return FrameStep::Last(Event::default().event("fault").data(payload.to_string()));
    }
    read_event(frame)
}

/// One read-shaped answer (`{data, cursor, alive, exit_code, signal}`) as the stream's next step.
///
/// **An empty read that is still alive is not an end.** The subscription's ack for an up-to-date
/// cursor is exactly that frame — and the device sends no frame at all for a quiet follow — so it
/// is [`FrameStep::Skip`]; only `alive: false` ends the session.
fn read_event(answer: &Value) -> FrameStep {
    let cursor = answer.get("cursor").and_then(Value::as_u64);
    let data = answer
        .get("data")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !data.is_empty() {
        let payload = json!({ "data": data, "cursor": cursor });
        return FrameStep::Send(Event::default().event("output").data(payload.to_string()));
    }
    if answer.get("alive").and_then(Value::as_bool) == Some(false) {
        let payload = json!({
            "cursor": cursor,
            "exit_code": answer.get("exit_code").cloned().unwrap_or(Value::Null),
            "signal": answer.get("signal").cloned().unwrap_or(Value::Null),
        });
        return FrameStep::Last(Event::default().event("end").data(payload.to_string()));
    }
    FrameStep::Skip
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
    /// True when the console's own page is polling, not a person: a read of the same screen the
    /// console already shows. Honored only on the operator route and only when no grant is
    /// presented, so an older client that sends a grant cannot hide its call behind the marker.
    #[serde(default)]
    poll: bool,
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

/// Whether a tool's answer is the screen or facts the console already shows.
///
/// A poll of one of these is not a new fact about the machine: the terminal stream already draws
/// the same bytes, the Session card the same list, and the System page the same facts. The five are
/// the read-only tools the page polls; a call that a person makes with the same tool is recorded
/// because the page does not mark it `poll`. `terminal.create`, `terminal.write`, `terminal.close`
/// and every other tool stay work even when they come from the page.
fn is_poll_read(tool: Option<&str>) -> bool {
    matches!(
        tool,
        Some("terminal.read" | "terminal.screen" | "terminal.list" | "state.get" | "host.facts")
    )
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

/// How long a call that legitimately parks on the device may take, above the general `/mcp` bound.
///
/// Two tools park by design. `runs.exec` runs the command, and the console's own 30-second bound
/// would turn every run longer than that into a 504 while the command kept running and its history
/// row was still written. `terminal.read` waits `wait_ms` for output, and the device's own cap on
/// that wait is 30 seconds — exactly the console's general bound, so a legal long poll would race
/// the device's deadline and be cut by the console instead of returning its wait. In both cases the
/// bound follows the argument the client sent, plus a grace for the frame and the answer, so a
/// client sees the answer it asked to wait for. Every other tool answers in milliseconds, and
/// `None` leaves the general bound in charge.
fn call_wait(request: &Value) -> Option<Duration> {
    let name = request
        .get("params")
        .and_then(|params| params.get("name"))
        .and_then(|name| name.as_str())?;
    let arguments = request
        .get("params")
        .and_then(|params| params.get("arguments"));
    match name {
        "runs.exec" => {
            let requested = arguments
                .and_then(|arguments| arguments.get("timeout_ms"))
                .and_then(|timeout| timeout.as_u64())
                .unwrap_or(60_000);
            let clamped = requested.clamp(1, RUNS_MAX_TIMEOUT_MS);
            // **THE CONSOLE MUST ANSWER BEFORE THE PROXY IN FRONT OF IT GIVES UP.** The operator's
            // console is reached through Cloudflare Access, whose origin timeout is about a hundred
            // seconds: a run that is proxied for ten minutes is cut there and the operator sees
            // Cloudflare's HTML error instead of this console's answer (measured 2026-10-28: two
            // `browser.eval`/`runs.exec` calls, CF 504 pages with ray ids). Ninety seconds leaves
            // room for the frame's round trip and the answer's write; a run that needs longer keeps
            // running on the device and is collected through `runs.list`.
            Some((Duration::from_millis(clamped) + RUNS_WAIT_GRACE).min(RUNS_MAX_PROXY_WAIT))
        }
        "terminal.read" => {
            // **THE DEVICE CAPS THE WAIT, SO THE CONSOLE MUST MIRROR THE CAP.** A `wait_ms` larger
            // than the device honours would otherwise make the console wait longer than any answer
            // can take; clamping keeps the console's bound tied to the wait that can actually
            // happen. Zero (or an absent/other-typed field) is not a long poll and keeps the
            // general bound.
            let requested = arguments
                .and_then(|arguments| arguments.get("wait_ms"))
                .and_then(|wait| wait.as_u64())
                .unwrap_or(0);
            let wait = requested.min(TERMINAL_MAX_WAIT_MS);
            (wait > 0).then(|| Duration::from_millis(wait) + TERMINAL_WAIT_GRACE)
        }
        _ => None,
    }
}

/// The device's own cap on `runs.exec`, mirrored so the wait cannot exceed it.
const RUNS_MAX_TIMEOUT_MS: u64 = 10 * 60 * 1000;

/// The longest a proxied call may hold the operator's HTTP request open.
const RUNS_MAX_PROXY_WAIT: Duration = Duration::from_secs(90);

/// The round-trip grace added to a run's wait: the channel gets this much longer than the run itself.
const RUNS_WAIT_GRACE: Duration = Duration::from_secs(5);

/// The device's own cap on `terminal.read`'s `wait_ms`, mirrored so the bound cannot exceed the
/// wait the device will ever honour (device/device/src/mcp.rs clamps `wait_ms` to 30 s).
const TERMINAL_MAX_WAIT_MS: u64 = 30_000;

/// The round-trip grace added to a read's wait: the ring copy and the answer's return must fit
/// inside the channel's bound as well as the proxy's.
const TERMINAL_WAIT_GRACE: Duration = Duration::from_secs(10);

/// The proxy's backstop around a call that parks: the channel's bound plus room for the frame's
/// round trip and the answer's write.
const WAIT_BACKSTOP_GRACE: Duration = Duration::from_secs(10);

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
        Err(unknown_device(device_id))
    }
}

/// The 404 for a door that names no machine: a wrong id, not a credential problem.
fn unknown_device(device_id: &str) -> ApiError {
    ApiError::not_found("unknown_device").saying(format!(
        "there is no machine {} in this console's fleet; check the id on the Machines page",
        short_device_id(device_id)
    ))
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
    /// caller is not authenticated, so this is a denial-log line, never the operator's audit. No
    /// door is named: an unauthenticated caller does not get the token's scope.
    Unauthenticated(&'static str),
    /// A live token presented at the other kind of endpoint. Carries the endpoint the token opens
    /// (e.g. `/mcp/all` or `/mcp/4c718d6e44c8…`), so the 401 can name the door instead of saying
    /// only "unauthorized".
    WrongDoor(String),
    /// A live device token for another machine. The caller *is* authenticated, so the operator's
    /// audit gets the refusal — a credential used against the wrong machine is what an audit is for.
    /// Carries the endpoint the token does open.
    ForeignDevice(String),
}

/// A device id as the page writes it: the first twelve characters and an ellipsis when cut.
///
/// The 401 names the endpoint a presented token opens; a 64-hex id in a sentence is unreadable, and
/// this is the same short form the token list's Scope cell prints.
fn short_device_id(device_id: &str) -> String {
    let mut chars = device_id.chars();
    let short: String = chars.by_ref().take(12).collect();
    if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}

/// The endpoint a token opens, as a 401 names it.
fn token_door(record: &Token) -> String {
    match record.kind() {
        TokenKind::Fleet => "/mcp/all".to_owned(),
        TokenKind::Device => device_door(&record.device_id),
    }
}

/// One device's MCP door, as a refusal names it.
fn device_door(device_id: &str) -> String {
    format!("/mcp/{}", short_device_id(device_id))
}

/// The 401 a bearer door gives a request whose header is missing or not a bearer.
///
/// The door and the credential it wants are in the sentence; the challenge is the HTTP half of the
/// same fact. A malformed bearer gets `invalid_token`; an absent one gets the bare challenge,
/// because no credential was presented.
fn bearer_unauthorized(
    door: &str,
    problem: crate::mcp::BearerProblem,
    credential: &str,
) -> ApiError {
    match problem {
        crate::mcp::BearerProblem::Missing => ApiError::unauthorized()
            .saying(format!(
                "this is the MCP door at {door}; the request carried no bearer token. Send \
                 {credential} as \"Authorization: Bearer …\""
            ))
            .challenging(),
        crate::mcp::BearerProblem::NotBearer => ApiError::unauthorized()
            .saying(format!(
                "this is the MCP door at {door}; the Authorization header is not a Bearer token. \
                 Send {credential} as \"Authorization: Bearer …\""
            ))
            .challenging(),
        crate::mcp::BearerProblem::Unreadable => ApiError::unauthorized()
            .saying(format!(
                "this is the MCP door at {door}; the bearer value is neither a grant nor a token \
                 this console issued. Send {credential} as \"Authorization: Bearer …\""
            ))
            .with_invalid_bearer(),
    }
}

/// The 401 for a bearer that named itself but was not accepted.
fn invalid_bearer(door: &str, reason: &str) -> ApiError {
    ApiError::unauthorized()
        .saying(format!("this is the MCP door at {door}; {reason}"))
        .with_invalid_bearer()
}

/// The one wire answer for an aggregate token that failed lookup or minting: 401, with the denial
/// detail the refusal names. A wrong, foreign or revoked credential is not distinguished on the
/// wire; a live token of the other kind is, because the caller already proved that credential and
/// needs to know the door it opens.
async fn refuse_token(app: &AppState, refusal: TokenRefusal) -> Response {
    let (detail, error) = match refusal {
        TokenRefusal::Unauthenticated(detail) => {
            let reason = match detail {
                "expired_api_token" => {
                    "the bearer token has expired; mint a new fleet token for /mcp/all"
                }
                _ => {
                    "the bearer token is not one this console issued, or it was revoked; mint a \
                     new fleet token for /mcp/all"
                }
            };
            (detail, invalid_bearer("/mcp/all", reason))
        }
        TokenRefusal::WrongDoor(door) => (
            "not_an_aggregate_token",
            invalid_bearer(
                "/mcp/all",
                &format!("this token is for {door}; /mcp/all takes a fleet token"),
            ),
        ),
        // The aggregate route reaches this only for a token whose list no longer contains the
        // requested device. That is not a wrong door, so it stays in the fleet's own words.
        TokenRefusal::ForeignDevice(door) => (
            "foreign_api_token",
            invalid_bearer(
                "/mcp/all",
                &format!(
                    "this token is for {door}; the requested machine is not in its fleet. Mint a \
                     token that includes it"
                ),
            ),
        ),
    };
    app.refuse_as(None, error, detail).await.into_response()
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

/// A device token on `/mcp/:id`: one device only, never a fleet record.
///
/// The caller has already looked the record up, because it also has to check the registry between
/// authentication and the door decision.
fn mint_token_scope_from(
    app: &AppState,
    device_id: &str,
    record: Token,
) -> Result<MintedScope, TokenRefusal> {
    match record.kind() {
        // The token authenticated; it is simply the wrong kind for this URL. Naming its own door
        // makes the first failing call say where the credential belongs.
        TokenKind::Fleet => Err(TokenRefusal::WrongDoor(token_door(&record))),
        TokenKind::Device if record.device_id != device_id => {
            Err(TokenRefusal::ForeignDevice(token_door(&record)))
        }
        TokenKind::Device => Ok(token_scope(app, &record, device_id)),
    }
}

/// An aggregate token on `/mcp/all`: the path is not a device, so the requested device is checked
/// against the record's list before anything is signed.
fn mint_aggregate_scope_from(
    app: &AppState,
    device_id: &str,
    record: &Token,
) -> Result<MintedScope, TokenRefusal> {
    let Some(allowed) = record.device_ids.as_ref() else {
        return Err(TokenRefusal::WrongDoor(token_door(record)));
    };
    if !allowed.iter().any(|id| id == device_id) {
        return Err(TokenRefusal::ForeignDevice(token_door(record)));
    }
    Ok(token_scope(app, record, device_id))
}

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

/// Forward one MCP exchange to a device and record the authenticated outcome.
///
/// The caller has already checked the device and verified the grant. `invalid_params_status` is the
/// HTTP status a malformed `tools/call` gets: the custom route answers 400, the standard route
/// answers 200 with a JSON-RPC error, and both are the same authorization decision. `filter` is the
/// grant's tool list when the request is `tools/list` on the standard route; the custom route passes
/// `None` and keeps its unfiltered answer. `identity` names whose call it is — the page's own `/mcp`
/// or the terminal stream, against an AI client on `/mcp/:id` or `/mcp/all` — and the permanent
/// token's record id when one authenticated the exchange. Both facts are written on every line and
/// event this function records, except a successful poll: a `poll` read of a screen tool is the
/// console redrawing what it already shows and writes neither, while a failed one writes a `poll`
/// line (no live event) so the table keeps the machine's silence visible without counting it as the
/// AI's failure.
async fn proxy_reply(
    app: &AppState,
    device_id: &str,
    auth: &RequestAuth<'_>,
    identity: CallIdentity<'_>,
    request: Value,
    invalid_params_status: StatusCode,
    filter: Option<&[String]>,
) -> Result<Reply, Box<Response>> {
    let audit_id = audit_device_id(device_id);
    let poll = identity.poll;
    // The one `trace.call` this exchange is known by: the frame carries it to the device, the audit
    // line carries it to the record, and the device journal's pair carries it on the machine. A
    // refused or timed-out exchange keeps it too, so a reader can still join the pair that exists.
    let trace = app.channels.mint_trace();
    let started = Instant::now();
    let request_id = request.get("id").cloned().unwrap_or(Value::Null);
    let tool = match requested_tool(&request) {
        Ok(tool) => tool.map(str::to_owned),
        Err(()) => {
            app.record_facts(AuditFacts {
                kind: if poll { "poll" } else { "denied" },
                device_id: audit_id.as_deref(),
                status: invalid_params_status,
                detail: Some("invalid_tool_call"),
                tool: None,
                token_id: identity.token_id,
                caller: Some(identity.caller),
                trace: Some(&trace),
                machine_seq: None,
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
                kind: if poll { "poll" } else { "denied" },
                device_id: audit_id.as_deref(),
                status: StatusCode::FORBIDDEN,
                detail: Some("tool_not_permitted"),
                tool: Some(tool),
                token_id: identity.token_id,
                caller: Some(identity.caller),
                trace: Some(&trace),
                machine_seq: None,
            })
            .await;
            return Err(Box::new(jsonrpc_error(
                StatusCode::FORBIDDEN,
                request_id,
                -32001,
                &format!(
                    "tool {tool} is not permitted by the presented credential; include it in \
                     the token or grant, then try again"
                ),
                Some(json!({ "tool": tool })),
            )));
        }
    }

    // **ANNOUNCE THE CALL BEFORE THE WAIT.** A run can keep the device five minutes while the proxy
    // gives up at ninety seconds; without this frame the band reads "Quiet" for exactly the call a
    // person most needs to see. The guard clears it on every exit.
    let _running = match tool.as_deref() {
        Some(tool) if !poll => Some(app.start_call(identity.caller, audit_id.as_deref(), tool)),
        _ => None,
    };

    // **A CALL THAT PARKS IS WAITED FOR.** A run blocks on the device for its own timeout (the
    // device's cap is ten minutes) and a read long-polls for its `wait_ms` (capped at thirty
    // seconds), so both bounds follow the argument the client sent: the channel's wait for the
    // answer and the proxy's backstop around it. Without this the console's own thirty-second bound
    // would cut a legal read at exactly the wait the device was allowed to spend. Every other tool
    // keeps `mcp_timeout`, and a device that has not answered in it is one to give up on.
    let answer = match call_wait(&request) {
        Some(wait) => {
            tokio::time::timeout(
                wait + WAIT_BACKSTOP_GRACE,
                app.channels
                    .request_within_acknowledged(device_id, request, auth, &trace, wait),
            )
            .await
        }
        // Everything else answers in milliseconds; the general bound stays in charge of both layers.
        None => {
            tokio::time::timeout(
                app.mcp_timeout,
                app.channels
                    .request_acknowledged(device_id, request, auth, &trace),
            )
            .await
        }
    };

    match answer {
        Err(_) => {
            app.publish_work(
                poll,
                LiveCall {
                    device_id: audit_id.as_deref(),
                    tool: tool.as_deref(),
                    sentence: &publish_sentence,
                    status: StatusCode::GATEWAY_TIMEOUT,
                    ok: false,
                    detail: "device timeout",
                    ms: started.elapsed().as_millis() as u64,
                    evidence: evidence.clone(),
                    caller: identity.caller,
                    token_id: identity.token_id,
                    trace: Some(&trace),
                    machine_seq: None,
                },
            );
            app.record_facts(AuditFacts {
                kind: if poll { "poll" } else { "denied" },
                device_id: audit_id.as_deref(),
                status: StatusCode::GATEWAY_TIMEOUT,
                detail: Some("device timeout"),
                tool: tool.as_deref(),
                token_id: identity.token_id,
                caller: Some(identity.caller),
                trace: Some(&trace),
                machine_seq: None,
            })
            .await;
            Err(Box::new(
                ApiError::gateway_timeout("device timeout").into_response(),
            ))
        }
        Ok(Err(error)) => {
            let now = (app.now)();
            let failure = failure_for(error, app.link_state(device_id, now), now);
            app.publish_work(
                poll,
                LiveCall {
                    device_id: audit_id.as_deref(),
                    tool: tool.as_deref(),
                    sentence: &publish_sentence,
                    status: failure.status(),
                    ok: false,
                    detail: failure.detail,
                    ms: started.elapsed().as_millis() as u64,
                    evidence: evidence.clone(),
                    caller: identity.caller,
                    token_id: identity.token_id,
                    trace: Some(&trace),
                    machine_seq: None,
                },
            );
            app.record_facts(AuditFacts {
                kind: if poll { "poll" } else { "denied" },
                device_id: audit_id.as_deref(),
                status: failure.status(),
                detail: Some(failure.detail),
                tool: tool.as_deref(),
                token_id: identity.token_id,
                caller: Some(identity.caller),
                trace: Some(&trace),
                machine_seq: None,
            })
            .await;
            Err(Box::new(failure.into_response()))
        }
        Ok(Ok(answered)) => {
            // **THE MACHINE'S OWN POSITION FOR THIS CALL (round 9).** The frame named the outcome
            // half's journal `seq`; the audit line below records it, so the row can be read back
            // from the machine by its own sequence rather than guessed from two clocks.
            let machine_seq = answered.journal_seq;
            let mut reply = answered.reply;
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
                app.publish_work(
                    poll,
                    LiveCall {
                        device_id: audit_id.as_deref(),
                        tool: tool.as_deref(),
                        sentence: &publish_sentence,
                        status: StatusCode::FORBIDDEN,
                        ok: false,
                        detail: "destructive_without_consent",
                        ms: started.elapsed().as_millis() as u64,
                        evidence: evidence.clone(),
                        caller: identity.caller,
                        token_id: identity.token_id,
                        trace: Some(&trace),
                        machine_seq,
                    },
                );
                app.record_facts(AuditFacts {
                    kind: if poll { "poll" } else { "denied" },
                    device_id: audit_id.as_deref(),
                    status: StatusCode::FORBIDDEN,
                    detail: Some("destructive_without_consent"),
                    tool: tool.as_deref(),
                    token_id: identity.token_id,
                    caller: Some(identity.caller),
                    trace: Some(&trace),
                    machine_seq,
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
            // **A TOOL THAT ANSWERED IS NOT NECESSARILY A SUCCESS.** MCP puts the failure in the
            // result (`isError: true` with its reason), and a run says so with its exit code. A
            // device older than that rule refuses a `tools/call` with a JSON-RPC error envelope.
            // All three become a non-200 audit line; the HTTP answer to the caller is untouched,
            // and a protocol error on ping/initialize/tools/list stays a 200 answer.
            let tool_failure = tool_failure(&reply, tool.as_deref());
            let (ok, status, audit_detail, live_detail) = match &reply {
                Reply::Result(_) => match tool_failure {
                    Some(reason) => (false, TOOL_REFUSED_STATUS, Some(reason.clone()), reason),
                    None => (
                        true,
                        StatusCode::OK,
                        detail.clone(),
                        detail.clone().unwrap_or_default(),
                    ),
                },
                Reply::Error(_) => match tool_failure {
                    Some(reason) => (false, TOOL_REFUSED_STATUS, Some(reason.clone()), reason),
                    None => (
                        false,
                        StatusCode::OK,
                        detail.clone(),
                        error_message(&reply)
                            .map(bounded_detail)
                            .unwrap_or_else(|| detail.clone().unwrap_or_default()),
                    ),
                },
            };
            // The row's words match the form's for the one failure a person causes by typing a
            // command: "Could not run the command", with the device's reason one click away.
            let publish_sentence = if tool.as_deref() == Some("runs.exec") && !ok {
                "Could not run the command".to_owned()
            } else {
                publish_sentence
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
            // A successful poll of a screen read is the console redrawing what it already has.
            // It is neither an audit line nor a live event; a failed poll still writes its `poll`
            // line below, so the operator can find the machine's silence in the Activity Table.
            let silent_poll = poll && ok && is_poll_read(tool.as_deref());
            if !silent_poll {
                app.publish_work(
                    poll,
                    LiveCall {
                        device_id: audit_id.as_deref(),
                        tool: tool.as_deref(),
                        sentence: &publish_sentence,
                        status,
                        ok,
                        detail: &live_detail,
                        ms: started.elapsed().as_millis() as u64,
                        evidence,
                        caller: identity.caller,
                        token_id: identity.token_id,
                        trace: Some(&trace),
                        machine_seq,
                    },
                );
                app.record_facts(AuditFacts {
                    kind: if poll { "poll" } else { "mcp" },
                    device_id: audit_id.as_deref(),
                    status,
                    detail: audit_detail.as_deref(),
                    tool: tool.as_deref(),
                    token_id: identity.token_id,
                    caller: Some(identity.caller),
                    trace: Some(&trace),
                    machine_seq,
                })
                .await;
            }
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
    identity: CallIdentity<'_>,
    request: Value,
    invalid_params_status: StatusCode,
    filter: Option<&[String]>,
) -> Response {
    match proxy_reply(
        app,
        device_id,
        auth,
        identity,
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
/// the next one) returns 503, because nothing was written and the operator's client can retry. The
/// sentence and the retry hint come from [`LinkState`]: "reconnecting" (with `retry_after_ms`) while
/// the last frame is recent, otherwise "not connected since <age>" with no hint. A request whose
/// channel closed after the frame was written returns 502 with no retry hint: the outcome is
/// unknown, and a retry could execute it twice. 504 remains the request the device took and did not
/// answer.
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
        return app
            .deny_from(Some(Caller::Console), audit_id.as_deref(), error)
            .await
            .into_response();
    }
    // The operator's session is the credential. A legacy client may still present a grant, verified
    // exactly as before; a client that presents none gets the operator's own authority: every tool on
    // the named device, destructive allowed. "I have a token" means "I may do everything".
    let legacy = match &body.grant {
        Some(grant) => {
            if let Err(error) = verify_presented(&app, &body.device_id, grant) {
                return app
                    .deny_from(Some(Caller::Console), audit_id.as_deref(), error)
                    .await
                    .into_response();
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
        CallIdentity {
            caller: Caller::Console,
            token_id: None,
            // The page's own automatic reads say so. A legacy grant on this route is a client that
            // still uses the operator transport; it must not be able to hide behind the marker.
            poll: body.poll && body.grant.is_none(),
        },
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
/// valid token goes to the denial log; everything after it is an audited action. A 401 names this
/// door and the reason (no bearer, unknown or revoked, expired, the other door's token, another
/// machine's token) and carries `WWW-Authenticate`; once a token has authenticated, an id that names
/// no machine is a 404 `unknown_device`, not another token blamed for the wrong door.
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

    let door = device_door(&device_id);
    let Some(bearer) = crate::mcp::bearer_from_headers(&headers) else {
        let problem = crate::mcp::bearer_problem(&headers);
        let error = bearer_unauthorized(&door, problem, "the device token (srm_…)");
        return app.refuse(audit_id.as_deref(), error).await.into_response();
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
        crate::mcp::BearerKind::Token(token) => {
            // The credential is looked up before the registry, so an unauthenticated caller cannot
            // use this route to learn which ids exist. Once it *is* authenticated, a missing machine
            // is a 404 rather than another token blamed for the wrong door.
            let record = match live_token(&app, &token) {
                Ok(record) => record,
                Err(TokenRefusal::Unauthenticated(detail)) => {
                    let reason = match detail {
                        "expired_api_token" => {
                            format!("the bearer token has expired; mint a new one for {door}")
                        }
                        _ => format!(
                            "the bearer token is not one this console issued, or it was revoked; \
                             mint a new token for {door}"
                        ),
                    };
                    return app
                        .refuse_as(audit_id.as_deref(), invalid_bearer(&door, &reason), detail)
                        .await
                        .into_response();
                }
                Err(_) => {
                    return app
                        .refuse_as(
                            audit_id.as_deref(),
                            invalid_bearer(
                                &door,
                                &format!(
                                    "the bearer token was not accepted; mint a new one for {door}"
                                ),
                            ),
                            "unknown_api_token",
                        )
                        .await
                        .into_response();
                }
            };
            if let Err(error) = ensure_registered(&app, &device_id) {
                return app
                    .deny_from(Some(Caller::Client), audit_id.as_deref(), error)
                    .await
                    .into_response();
            }
            match mint_token_scope_from(&app, &device_id, record) {
                Ok(minted) => (BearerAuth::Token(minted.scope), Some(minted.token_id)),
                // A live token of the other kind is authenticated, so its refusal may name the door
                // it opens. The audit keeps the difference from a plain wrong-device attempt.
                Err(TokenRefusal::WrongDoor(token_door)) => {
                    app.record_facts(AuditFacts {
                        kind: "denied",
                        device_id: audit_id.as_deref(),
                        status: StatusCode::UNAUTHORIZED,
                        detail: Some("api_token_wrong_endpoint"),
                        tool: None,
                        token_id: None,
                        caller: Some(Caller::Client),
                        trace: None,
                        machine_seq: None,
                    })
                    .await;
                    return invalid_bearer(
                        &door,
                        &format!(
                            "this token is for {token_door}; a device endpoint takes a device \
                             token for that machine"
                        ),
                    )
                    .into_response();
                }
                Err(TokenRefusal::ForeignDevice(token_door)) => {
                    app.record_facts(AuditFacts {
                        kind: "denied",
                        device_id: audit_id.as_deref(),
                        status: StatusCode::UNAUTHORIZED,
                        detail: Some("api_token_device_mismatch"),
                        tool: None,
                        token_id: None,
                        caller: Some(Caller::Client),
                        trace: None,
                        machine_seq: None,
                    })
                    .await;
                    return invalid_bearer(
                        &door,
                        &format!(
                            "this token is for {token_door}; it is not accepted at another \
                             machine's endpoint"
                        ),
                    )
                    .into_response();
                }
                Err(TokenRefusal::Unauthenticated(detail)) => {
                    // `live_token` already succeeded, so this arm is unreachable in the current
                    // split; it still names the door rather than falling back to a bare 401.
                    return app
                        .refuse_as(
                            audit_id.as_deref(),
                            invalid_bearer(
                                &door,
                                &format!("the bearer token could not be re-read; mint a new one for {door}"),
                            ),
                            detail,
                        )
                        .await
                        .into_response();
                }
            }
        }
    };
    let auth = match &bearer_auth {
        BearerAuth::Grant(grant) => RequestAuth::Grant(grant),
        BearerAuth::Token(scope) => RequestAuth::Token(scope),
    };
    if let Err(error) = ensure_registered(&app, &device_id) {
        return app
            .deny_from(Some(Caller::Client), audit_id.as_deref(), error)
            .await
            .into_response();
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
                CallIdentity {
                    caller: Caller::Client,
                    token_id: token_id.as_deref(),
                    poll: false,
                },
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
                CallIdentity {
                    caller: Caller::Client,
                    token_id: token_id.as_deref(),
                    poll: false,
                },
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
    /// A registered machine, but not one this token lists.
    OutOfScope,
    /// A name or id no machine in the fleet has: a typo or a removed machine, not a scope refusal.
    Unknown,
    Ambiguous,
}

/// Resolve the `device` argument against the token's allowed ids and the registered names.
///
/// An exact id wins. A name is matched only among the **allowed** devices, so a name that exists
/// outside the scope is an out-of-scope request, not a hint; a name shared by two allowed devices is
/// ambiguous rather than resolved by order. A value that names no machine at all is [`Unknown`]:
/// the operator needs to check the id, not mint another token.
fn resolve_aggregate_device(
    allowed: &[String],
    in_scope: &[DeviceRef],
    fleet: &[DeviceRef],
    requested: &str,
) -> DeviceResolution {
    if allowed.iter().any(|id| id == requested) {
        return DeviceResolution::Resolved(requested.to_owned());
    }
    let matches: Vec<&String> = in_scope
        .iter()
        .filter(|device| device.name.as_deref() == Some(requested))
        .map(|device| &device.id)
        .collect();
    match matches.as_slice() {
        [id] => DeviceResolution::Resolved((*id).clone()),
        [] => {
            let exists = fleet
                .iter()
                .any(|device| device.id == requested || device.name.as_deref() == Some(requested));
            if exists {
                DeviceResolution::OutOfScope
            } else {
                DeviceResolution::Unknown
            }
        }
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

/// Every registered machine, as `(id, name)`: the fleet an aggregate argument is checked against.
///
/// The scope check uses this only to tell "not in this token's fleet" from "no such machine"; the
/// resolved id still has to come from the token's own list.
fn fleet_devices(app: &AppState) -> Vec<DeviceRef> {
    lock(&app.devices)
        .list()
        .into_iter()
        .map(|device| DeviceRef {
            id: device.id,
            name: device.name,
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
        let problem = crate::mcp::bearer_problem(&headers);
        let error = bearer_unauthorized("/mcp/all", problem, "a fleet token (srm_…)");
        return app.refuse(None, error).await.into_response();
    };
    let token = match bearer {
        crate::mcp::BearerKind::Token(token) => token,
        crate::mcp::BearerKind::Grant(_) => {
            return app
                .refuse_as(
                    None,
                    invalid_bearer(
                        "/mcp/all",
                        "a grant is not accepted here; /mcp/all takes a fleet token",
                    ),
                    "aggregate_requires_token",
                )
                .await
                .into_response()
        }
    };
    let record = match live_token(&app, &token) {
        Ok(record) => record,
        Err(refusal) => return refuse_token(&app, refusal).await,
    };
    let Some(allowed) = record.device_ids.clone() else {
        return refuse_token(&app, TokenRefusal::WrongDoor(token_door(&record))).await;
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
                // No machine in this token's fleet is present. A fleet list has no single age to
                // state, so it says the fleet's fact and leaves the retry open: any machine
                // connecting changes it.
                return ApiError::unavailable("devices not connected")
                    .saying(
                        "no machine in this token's fleet is connected; connect one of them, then \
                         try again",
                    )
                    .retrying(RETRY_AFTER_MS)
                    .into_response();
            };
            let granted = match mint_aggregate_scope_from(&app, &device_id, &record) {
                Ok(granted) => granted,
                Err(refusal) => return refuse_token(&app, refusal).await,
            };
            let auth = RequestAuth::Token(&granted.scope);
            match proxy_reply(
                &app,
                &device_id,
                &auth,
                CallIdentity {
                    caller: Caller::Client,
                    token_id: Some(&granted.token_id),
                    poll: false,
                },
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
                        caller: Some(Caller::Client),
                        trace: None,
                        machine_seq: None,
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
            let fleet = fleet_devices(&app);
            let resolution = resolve_aggregate_device(&allowed, &devices, &fleet, &requested);
            match &resolution {
                DeviceResolution::Resolved(device_id) => {
                    if let Err(error) = ensure_registered(&app, device_id) {
                        return app
                            .deny_from(
                                Some(Caller::Client),
                                audit_device_id(device_id).as_deref(),
                                error,
                            )
                            .await
                            .into_response();
                    }
                    let granted = match mint_aggregate_scope_from(&app, device_id, &record) {
                        Ok(granted) => granted,
                        Err(refusal) => return refuse_token(&app, refusal).await,
                    };
                    let auth = RequestAuth::Token(&granted.scope);
                    forward(
                        &app,
                        device_id,
                        &auth,
                        CallIdentity {
                            caller: Caller::Client,
                            token_id: Some(&granted.token_id),
                            poll: false,
                        },
                        request,
                        StatusCode::OK,
                        None,
                    )
                    .await
                }
                DeviceResolution::Unknown => {
                    // A machine that does not exist is not a scope refusal: minting another token
                    // would not help. The distinct error and 404 say to check the id instead.
                    app.record_facts(AuditFacts {
                        kind: "denied",
                        device_id: Some(requested.as_str()),
                        status: StatusCode::NOT_FOUND,
                        detail: Some("unknown_device"),
                        tool: None,
                        token_id: Some(&record.id),
                        caller: Some(Caller::Client),
                        trace: None,
                        machine_seq: None,
                    })
                    .await;
                    jsonrpc_error(
                        StatusCode::NOT_FOUND,
                        id,
                        -32602,
                        &format!(
                            "unknown device: {requested} is not in this console's fleet; check \
                             the id on the Machines page"
                        ),
                        Some(json!({ "device": requested })),
                    )
                }
                DeviceResolution::OutOfScope | DeviceResolution::Ambiguous => {
                    // The audit records the `device` value as sent: a name is a legal input, and an
                    // ambiguous one is exactly what an operator needs to see. The wire code stays
                    // the plan's one scope refusal, so it cannot probe for a device outside the
                    // token; the sentence still has to say which of the two happened.
                    app.record_facts(AuditFacts {
                        kind: "denied",
                        device_id: Some(requested.as_str()),
                        status: StatusCode::FORBIDDEN,
                        detail: Some("device_not_permitted"),
                        tool: None,
                        token_id: Some(&record.id),
                        caller: Some(Caller::Client),
                        trace: None,
                        machine_seq: None,
                    })
                    .await;
                    let message = match &resolution {
                        DeviceResolution::Ambiguous => format!(
                            "device name {requested} is ambiguous: it matches more than one \
                             machine in this token's fleet; use the machine id instead"
                        ),
                        _ => format!(
                            "device {requested} is not permitted by this token; it is not in the \
                             token's fleet. Mint a token that includes it, or call a machine the \
                             token lists"
                        ),
                    };
                    jsonrpc_error(
                        StatusCode::FORBIDDEN,
                        id,
                        -32001,
                        &message,
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

    // The rename answers with the row the page replaces, so it carries the same day the list
    // serves: the local calendar day, the same rule, and a machine with no line today still gets no
    // `today`.
    let window = app.audit.fleet_window().await;
    let day = app.audit.day((app.now)()).await;
    Ok(Json(DeviceView {
        online: app.channels.online(&device.id),
        version: app.channels.version(&device.id),
        host: app.channels.host(&device.id),
        update: app.channels.update(&device.id),
        name: device.name.or_else(|| app.channels.name(&device.id)),
        last_attempt: app.channels.retry(&device.id).map(|note| RetryView {
            attempts: note.attempts,
            reason: note.reason,
        }),
        today: day_view(day.devices.get(&device.id), day.since_ms, day.bounded),
        last_outage: outage_view(window.boundaries.last_outages.get(&device.id).copied()),
        last_left_ms: window.boundaries.last_lefts.get(&device.id).copied(),
        channel_activity: channel_activity_view(
            window.boundaries.activity.get(&device.id).copied(),
            window.read,
        ),
        last_frame_ms: app.channels.last_frame(&device.id),
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
/// static registry: an offline device is an offline state, not a shorter list. The refusal's
/// sentence is the MCP door's own [`LinkState`] sentence, so one silence reads one way everywhere.
async fn live_tool_catalogue(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    _: OperatorSession,
) -> Response {
    if !app.has_device(&device_id) {
        return unknown_device(&device_id).into_response();
    }
    match read_live_tools(&app, &device_id).await {
        Ok(tools) => match catalogue_from_tools(&tools) {
            Ok(tools) => (StatusCode::OK, Json(json!({ "tools": tools }))).into_response(),
            Err(()) => LiveToolsError::Malformed
                .refusal(&app, &device_id)
                .into_response(),
        },
        Err(error) => error.refusal(&app, &device_id).into_response(),
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
    ///
    /// The offline sentence comes from the same [`LinkState`] the MCP door uses, so the operator's
    /// read and an AI client's refused call never describe one silence two ways.
    fn refusal(self, app: &AppState, device_id: &str) -> ApiError {
        match self {
            LiveToolsError::Unavailable => {
                let now = (app.now)();
                LinkRefusal::from_state(app.link_state(device_id, now), now).into_api_error()
            }
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
    // `tools/list` is not a call: no capability runs, and no journal pair is written for it. The
    // frame still carries a trace, so every frame this console sends has one shape.
    let trace = app.channels.mint_trace();
    let answer = tokio::time::timeout(
        app.mcp_timeout,
        app.channels
            .request(device_id, request, &RequestAuth::Token(&scope), &trace),
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

/// One pass over the audit's newest lines, shared by every row: the file is read once per fleet
/// call, and the boundary maps and day counts are built after the read returns, so the writer is not
/// held while the answer is serialised.
async fn device_views(app: &AppState) -> Vec<DeviceView> {
    let window = app.audit.fleet_window().await;
    // `today` is the day read, not the window read: the newest 2,000 lines are still the absence
    // read and the timeline's window, but a count that shrinks while the day runs is not a day.
    let day = app.audit.day((app.now)()).await;
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
            update: app.channels.update(&device.id),
            last_attempt: app.channels.retry(&device.id).map(|note| RetryView {
                attempts: note.attempts,
                reason: note.reason,
            }),
            today: day_view(day.devices.get(&device.id), day.since_ms, day.bounded),
            last_outage: outage_view(window.boundaries.last_outages.get(&device.id).copied()),
            last_left_ms: window.boundaries.last_lefts.get(&device.id).copied(),
            channel_activity: channel_activity_view(
                window.boundaries.activity.get(&device.id).copied(),
                window.read,
            ),
            last_frame_ms: app.channels.last_frame(&device.id),
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
    Json(device_views(&app).await)
}

/// A registered device as the operator's list sees it: the registry's facts plus the channel's.
#[derive(serde::Serialize)]
struct RetryView {
    attempts: u8,
    reason: String,
}

/// One machine's day as `/fleet` serves it: the AI's calls in the local calendar day, how many of
/// them failed, and where the day's coverage starts when the file cannot prove all of it.
#[derive(serde::Serialize)]
struct DayView {
    client_calls: u64,
    failed: u64,
    /// The local time the read's coverage starts at, when a rotation (or the scan bound) means the
    /// file does not hold the whole day. Omitted for a whole day.
    #[serde(skip_serializing_if = "Option::is_none")]
    since_ms: Option<u64>,
    /// The scan hit its line bound before midnight: the numbers are the newest lines only.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    bounded: bool,
    /// The line bound, when it was hit. The page states how many lines the count covers.
    #[serde(skip_serializing_if = "Option::is_none")]
    scan_limit: Option<usize>,
}

/// The day of one machine, or `None` when today's read names it nowhere.
fn day_view(
    day: Option<&crate::audit::DayBuckets>,
    since_ms: Option<u64>,
    bounded: bool,
) -> Option<DayView> {
    day.map(|day| DayView {
        client_calls: day.client_calls,
        failed: day.failed,
        since_ms,
        bounded,
        scan_limit: bounded.then_some(crate::audit::DAY_SCAN_LIMIT),
    })
}

/// `GET /audit/day`: the local calendar day's counts, by the page's four buckets.
#[derive(serde::Deserialize)]
struct AuditDayQuery {
    /// One machine's day; absent is the fleet's, including lines that name no device.
    device: Option<String>,
}

/// One day as the route serves it: the same numbers the hub's sentence and Activity's header read,
/// so two surfaces of one day cannot count two different sets of lines.
#[derive(serde::Serialize)]
struct DaySummary {
    client_calls: u64,
    failed: u64,
    console: u64,
    console_failed: u64,
    poll: u64,
    poll_failed: u64,
    unattributed: u64,
    unattributed_failed: u64,
    /// **THE MIDNIGHT THIS ANSWER CUT AT (the forty-fourth list).** The page's timeline grouped its
    /// rows into days at the BROWSER's midnight while this read cut at the CONSOLE HOST's; they
    /// agree only when both are the same zone. Publishing the moment lets the timeline label the
    /// interval the count is of, on any browser.
    day_start_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    since_ms: Option<u64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    bounded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    scan_limit: Option<usize>,
    /// How many lines the backwards scan examined. The day is bounded by lines, and this is the
    /// cost the bound paid: a live day is the number of lines after midnight, not the whole file.
    lines_read: usize,
}

/// `GET /audit/day?device=`: the local calendar day, counted from the file rather than from the
/// newest window.
///
/// A line at local midnight or later is today; the scan stops at the first older line. A file that
/// starts after midnight (one rotation keeps the older generation) is partial, and the answer says
/// `since_ms` instead of presenting it as the whole day. A scan that hits its line bound says so.
async fn audit_day(
    State(app): State<AppState>,
    _: OperatorSession,
    Query(query): Query<AuditDayQuery>,
) -> Json<DaySummary> {
    let read = app.audit.day((app.now)()).await;
    let day = match query.device.as_deref() {
        Some(device) => read.devices.get(device).copied().unwrap_or_default(),
        None => read.all,
    };
    Json(DaySummary {
        client_calls: day.client_calls,
        failed: day.failed,
        console: day.console,
        console_failed: day.console_failed,
        poll: day.poll,
        poll_failed: day.poll_failed,
        unattributed: day.unattributed,
        unattributed_failed: day.unattributed_failed,
        day_start_ms: read.day_start_ms,
        since_ms: read.since_ms,
        bounded: read.bounded,
        scan_limit: read.bounded.then_some(crate::audit::DAY_SCAN_LIMIT),
        lines_read: read.lines_read,
    })
}

#[derive(serde::Serialize)]
struct DeviceView {
    id: String,
    public_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    registered_at_ms: u64,
    last_seen_ms: u64,
    /// The AI's calls this machine made in the local calendar day, and how many of them failed:
    /// the fleet's comparison line. Omitted when the day holds no line naming the machine, so a
    /// page draws nothing rather than a zero the read cannot prove. A partial day says `since_ms`.
    #[serde(skip_serializing_if = "Option::is_none")]
    today: Option<DayView>,
    /// When the console last saw any frame from this device, in Unix milliseconds. `online` is this
    /// moment inside the presence window; a channel that exists but has been silent past it is
    /// offline. Omitted when the console has seen no frame since it started: a device it never heard
    /// has no moment to serve, and a zero or a socket's open time would be an invented one.
    #[serde(skip_serializing_if = "Option::is_none")]
    last_frame_ms: Option<u64>,
    /// When the current silence started: the newest `device.left` that no `device.joined` closed.
    ///
    /// The measured DESKTOP-14RJCR8 case: a console restarted mid-outage has no `last_frame_ms`, and
    /// the registry's `last_seen_ms` is the previous channel's open (08:06:04), older than the close
    /// the dead process wrote (08:18:06). This is the newest evidence the console has, so an offline
    /// row can be aged from it. Omitted when the newest boundary for this device is a join (the
    /// silence ended) or when the window holds no boundary at all.
    #[serde(skip_serializing_if = "Option::is_none")]
    last_left_ms: Option<u64>,
    /// Whether a frame arrived on a live channel inside the presence window.
    online: bool,
    /// The version the device announced on its channel, when it announced one.
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    /// How the attempt before the last one ended, when the device has said since.
    #[serde(skip_serializing_if = "Option::is_none")]
    last_attempt: Option<RetryView>,
    /// The last time this device's channel went away and came back, when the audit proves both ends.
    /// `online` answers "is it up now"; this answers "was it away, and for how long" — the question
    /// an operator asks when they come back to a fleet where nothing looks wrong any more. It is
    /// derived from the audit, so a console restarted mid-outage still has the pair.
    #[serde(skip_serializing_if = "Option::is_none")]
    last_outage: Option<OutageView>,
    /// The joins and leaves the audit's newest window holds for this machine, with the lines it
    /// read and the span those lines cover. `last_attempt` is the device's own retry note, and only
    /// this console process has it — a console restarted since the device last connected has
    /// forgotten it. The audit file has not, so this is the durable half of the same link-health
    /// story, read from the window the absence pair above comes from.
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_activity: Option<ChannelActivityView>,
    /// The machine facts the device last sent over its channel: the operator reads what the device is,
    /// including which sessions are disconnected, without minting a grant to ask.
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<serde_json::Value>,
    /// The device's own update story, as the last frame carried it: the version it runs, the verdict
    /// of its last start and the updater's last answer. Opaque here — the page reads the device's
    /// fields and words — and omitted for a device that never sent one, so "never said" is not an
    /// empty section the page would render as a fact.
    #[serde(skip_serializing_if = "Option::is_none")]
    update: Option<serde_json::Value>,
}

/// One finished outage, as a page reads it.
///
/// Only a pair the audit proves is a view: a device that has dropped and not come back has no
/// `back_at_ms`, and a window with no join is no absence at all. The two names are the two a menu
/// line compares: how long it was gone, and when it came back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct OutageView {
    pub(crate) away_ms: u64,
    pub(crate) back_at_ms: u64,
}

/// The fleet's view of an audit absence: `None` when the newest lines hold no join with a left
/// before it, so the page never renders a duration that is still running (or a zero standing in for
/// "never dropped").
fn outage_view(absence: Option<Absence>) -> Option<OutageView> {
    absence.map(|absence| OutageView {
        away_ms: absence.away_ms,
        back_at_ms: absence.back_at_ms,
    })
}

/// One machine's join/leave tally, and the window of lines it covers.
///
/// The window is served with the counts because a number without its window would read as all
/// time: 24 joins is a different link on a 1,000-line window than on a 2,000,000-line file, and the
/// page must be able to say which one it drew. `window` is the read's own length — never the cap it
/// asked for — and `span_ms` is the distance between the window's newest and oldest moments, so the
/// tally can be read as a rate. A window whose ends carry no moment carries no `span_ms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ChannelActivityView {
    pub(crate) joined: u64,
    pub(crate) left: u64,
    pub(crate) window: usize,
    /// The window's newest-to-oldest span in milliseconds. Omitted when an end carries no `ts_ms`,
    /// so the page states the lines it read and invents no duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) span_ms: Option<u64>,
}

/// The wire view of [`crate::audit::ChannelActivity`]: `None` for a device the window holds no
/// boundary for, so a page draws no count rather than a zero. `read` is the window the counts came
/// from, and both its facts travel with them so the sentence cannot pair a count with another read.
fn channel_activity_view(
    activity: Option<crate::audit::ChannelActivity>,
    read: crate::audit::WindowFacts,
) -> Option<ChannelActivityView> {
    activity.map(|activity| ChannelActivityView {
        joined: activity.joined,
        left: activity.left,
        window: read.lines,
        span_ms: read.span_ms,
    })
}

/// How a transport failure is answered, and whether the operator may retry.
struct TransportFailure {
    error: ApiError,
    /// The audit detail: the same fact the wire error names.
    detail: &'static str,
}

/// What the console can prove about a device it could not reach, from the device's last frame.
///
/// This is the ONE derivation both the operator's tool read and the MCP door use, so the two
/// never tell different stories about the same silence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinkState {
    /// The device was heard inside [`crate::channels::PRESENCE_WINDOW`]: its channel is between
    /// connections, and the next reconnect attempt is still recent. Nothing is known to be lost.
    Reconnecting,
    /// The last frame (or, after a console restart, the last accepted channel) is older than that
    /// window. The silence is the fact, and an age can be stated. `None` is a device the console
    /// has never heard: there is no moment to age.
    NotConnected { since_ms: Option<u64> },
}

/// How long after its last frame the console still calls a missing device "reconnecting".
///
/// It is the presence window itself: that is the moment the fleet stops counting the device as
/// online, so it is also the longest the console can say "it was just here" without inventing a
/// recovery it cannot observe. Past it, the sentence is the age of the silence.
const RECONNECT_GRACE_MS: u64 = crate::channels::PRESENCE_WINDOW.as_millis() as u64;

impl Inner {
    /// What the console can say about this device's link now, from its last frame.
    ///
    /// The frame is the evidence. A console restarted mid-outage has none, so the registry's last
    /// accepted channel is the fallback: the device's first frame arrives on the channel it opened,
    /// and the open is a known moment. A device the console has never seen has no moment; it reads
    /// as not connected since the console started rather than inventing a frame time.
    fn link_state(&self, device_id: &str, now_ms: u64) -> LinkState {
        let since_ms = self.channels.last_frame(device_id).or_else(|| {
            lock(&self.devices)
                .get(device_id)
                .map(|device| device.last_seen_ms)
        });
        match since_ms {
            Some(since_ms) if now_ms.saturating_sub(since_ms) <= RECONNECT_GRACE_MS => {
                LinkState::Reconnecting
            }
            since_ms => LinkState::NotConnected { since_ms },
        }
    }
}

/// One refusal for a missing device, as both doors render it.
struct LinkRefusal {
    error: &'static str,
    message: String,
    retry_after_ms: Option<u64>,
    /// The audit detail: the same fact the wire error names.
    detail: &'static str,
}

impl LinkRefusal {
    fn from_state(state: LinkState, now_ms: u64) -> Self {
        match state {
            LinkState::Reconnecting => LinkRefusal {
                error: "device reconnecting",
                message: "this machine is reconnecting; try again in a moment".to_owned(),
                retry_after_ms: Some(RETRY_AFTER_MS),
                detail: "device reconnecting",
            },
            LinkState::NotConnected { since_ms: None } => LinkRefusal {
                error: "device not connected",
                message: "this machine is not connected; the console has not heard from it since \
                          it started. Connect it, then try again"
                    .to_owned(),
                retry_after_ms: None,
                detail: "device not connected",
            },
            LinkState::NotConnected {
                since_ms: Some(since_ms),
            } => LinkRefusal {
                error: "device not connected",
                message: format!(
                    "this machine is not connected since {} ago; connect it, then try again",
                    age_sentence(now_ms.saturating_sub(since_ms))
                ),
                retry_after_ms: None,
                detail: "device not connected",
            },
        }
    }

    fn into_api_error(self) -> ApiError {
        let error = ApiError::unavailable(self.error).saying(self.message);
        match self.retry_after_ms {
            Some(retry_after_ms) => error.retrying(retry_after_ms),
            None => error,
        }
    }
}

/// A silence in the fleet's own units: seconds, minutes, then hours and minutes, then days.
///
/// The page's `seenAgo` uses the same units; a console sentence and the row above it must not
/// disagree about how long five hours is.
fn age_sentence(elapsed_ms: u64) -> String {
    let seconds = (elapsed_ms + 500) / 1_000;
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = (elapsed_ms + 30_000) / 60_000;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    let rest = minutes % 60;
    if hours < 24 {
        return if rest == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {rest:02}m")
        };
    }
    let days = hours / 24;
    let rest = hours % 24;
    if rest == 0 {
        format!("{days}d")
    } else {
        format!("{days}d {rest}h")
    }
}

/// Map the channel's failure onto the proxy's status contract.
///
/// `503` is reserved for "nothing was written": a device between channels, or a full queue. A frame
/// that was written and never answered is `502` with no retry hint, because the device may have
/// executed it. A socket that did not accept a frame inside its bound is `504`.
///
/// `link` is read only for [`RequestError::Offline`]; every other failure has its own state.
fn failure_for(error: RequestError, link: LinkState, now_ms: u64) -> TransportFailure {
    let (error, detail) = match error {
        RequestError::Offline => {
            let refusal = LinkRefusal::from_state(link, now_ms);
            let detail = refusal.detail;
            (refusal.into_api_error(), detail)
        }
        RequestError::Disconnected => (
            ApiError::bad_gateway("device disconnected"),
            "device disconnected",
        ),
        RequestError::Busy => (
            ApiError::unavailable("device busy").retrying(BUSY_RETRY_AFTER_MS),
            "device busy",
        ),
        RequestError::SendTimeout => (
            ApiError::gateway_timeout("device timeout"),
            "device timeout",
        ),
        RequestError::ReplyTimeout => (
            ApiError::gateway_timeout("device did not answer"),
            "the device accepted the request and did not answer inside the console's bound; \
             it may still be working on it",
        ),
    };
    TransportFailure { error, detail }
}

impl TransportFailure {
    fn status(&self) -> StatusCode {
        self.error.status
    }

    fn into_response(self) -> Response {
        self.error.into_response()
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

/// The `?limit=&device=` shape `/audit` reads, and the optional `?from=&to=` range.
///
/// `device` is optional on purpose: without it the route is the fleet-wide window it has always
/// been, and a page that names one machine reads that machine's own newest lines instead. `from`
/// and `to` arrive together or not at all: one endpoint is not a range.
#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<String>,
    device: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

/// What `/audit` was asked to answer: the newest window, or a chosen range.
enum AuditWindow {
    Newest,
    /// `[from_ms, to_ms)`.
    Range {
        from_ms: u64,
        to_ms: u64,
    },
}

/// Parse the range pair. Both absent is the window this route has always served; anything else
/// must name a moment twice and the second must be later than the first.
fn audit_window(from: Option<&str>, to: Option<&str>) -> Result<AuditWindow, ApiError> {
    let invalid = || ApiError::bad_request("invalid_range");
    match (from, to) {
        (None, None) => Ok(AuditWindow::Newest),
        (Some(from), Some(to)) => {
            let from_ms = from.parse::<u64>().map_err(|_| invalid())?;
            let to_ms = to.parse::<u64>().map_err(|_| invalid())?;
            // `[from, to)` is empty at equality and reversed after it; neither is a range a caller
            // can mean, and a page that asks for one has a bug worth naming.
            if from_ms >= to_ms {
                return Err(invalid());
            }
            Ok(AuditWindow::Range { from_ms, to_ms })
        }
        _ => Err(invalid()),
    }
}

/// A range's answer: the lines, and the facts the page needs to state the bounds it drew.
///
/// An object rather than the bare array `/audit` has always served: a range's answer has to say
/// whether the scan bound cut it and where the file's coverage starts, and an array cannot carry
/// either without making every line the envelope. A request with no `from`/`to` keeps the array.
#[derive(serde::Serialize)]
struct AuditRangeView {
    lines: Vec<Value>,
    /// How many lines the backwards scan examined. The cost, not a count of matches.
    lines_read: usize,
    /// The moment coverage starts at, when the current file starts after `from`.
    #[serde(skip_serializing_if = "Option::is_none")]
    since_ms: Option<u64>,
    /// The scan stopped at its line bound before reaching `from`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    bounded: bool,
    /// The bound's size, named when it was hit.
    #[serde(skip_serializing_if = "Option::is_none")]
    scan_limit: Option<usize>,
    /// The range held more matching lines than `limit`; `lines` is its newest prefix.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    at_limit: bool,
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

/// `GET /audit/source`: the file the audit reads, its size and the moments its lines span.
///
/// A separate route rather than fields on `/audit`: that answer is a bare array of lines and the
/// page reads it as one. This fact is about the reader, not about a window or a day, so neither
/// route's answer has to become about both. Every figure comes from the file or from the writer's
/// configuration; what could not be read is omitted rather than served as zero.
async fn audit_source(
    State(app): State<AppState>,
    _: OperatorSession,
) -> Json<crate::jsonl::SourceFacts> {
    Json(app.audit.source().await)
}

/// `GET /audit`: the operator's own actions, newest first, one JSON line each.
///
/// `?device=` narrows the read to one machine's lines. The filter runs during the scan, not over a
/// pre-cut fleet window, so another machine's traffic cannot push this machine's lines out.
///
/// `?from=&to=` answers a time range instead of the newest window: the lines whose `ts_ms` is in
/// `[from, to)`, newest first, from the current file's backwards scan. The answer is an object that
/// states the scan's bound and where coverage starts; no `from`/`to` keeps the bare array this route
/// has always served.
async fn list_audit(
    State(app): State<AppState>,
    _: OperatorSession,
    Query(query): Query<AuditQuery>,
) -> Result<Response, ApiError> {
    let limit = log_limit(query.limit)?;
    let window = audit_window(query.from.as_deref(), query.to.as_deref())?;
    match window {
        AuditWindow::Newest => {
            let lines = match query.device.as_deref() {
                Some(device) => app.audit.newest_for(device, limit).await,
                None => app.audit.newest(limit).await,
            };
            Ok(Json(with_sentences(lines)).into_response())
        }
        AuditWindow::Range { from_ms, to_ms } => {
            let read = app.audit.range(from_ms, to_ms).await;
            // The device is a selection over the range the scan read, not part of the scan's cut:
            // `/audit/day?device=` selects the same way from the day it read.
            let mut lines: Vec<Value> = read
                .lines
                .into_iter()
                .filter(|line| match query.device.as_deref() {
                    Some(device) => line.get("device_id").and_then(Value::as_str) == Some(device),
                    None => true,
                })
                .collect();
            // The window's limit is the page's draw size, not the range's definition: say when it
            // cut the answer rather than serving the newest prefix as the whole range.
            let at_limit = lines.len() > limit;
            lines.truncate(limit);
            Ok(Json(AuditRangeView {
                lines: with_sentences(lines),
                lines_read: read.lines_read,
                since_ms: read.since_ms,
                bounded: read.bounded,
                scan_limit: read.bounded.then_some(crate::audit::DAY_SCAN_LIMIT),
                at_limit,
            })
            .into_response())
        }
    }
}

/// `GET /audit/export?from=&to=&device=`: one answered range as a JSON Lines download.
///
/// The body is the range query's own lines — the same selection, the same `sentence` field — in
/// chronological order: the table draws newest first because a person reads the present at the top,
/// and a file is read oldest first because it is a history. The filename names the two requested
/// bounds and, when one machine was asked for, its short id. `X-Audit-Range` states the bounds, and
/// `X-Audit-Since` states the moment the current file's coverage begins when the range starts older
/// than the file. A range the 50,000-line scan bound cannot prove is `413` with `X-Audit-Bounded`
/// naming the bound: a file that begins in the middle of its own range would be a partial history
/// wearing the range's name, so it is refused rather than shortened without a word.
async fn export_audit(
    State(app): State<AppState>,
    _: OperatorSession,
    Query(query): Query<AuditQuery>,
) -> Response {
    let window = match audit_window(query.from.as_deref(), query.to.as_deref()) {
        Ok(AuditWindow::Range { from_ms, to_ms }) => (from_ms, to_ms),
        // An export is a file with a name and two bounds; the newest window has neither. A caller
        // that asks for one is told that a range is two moments, exactly as `/audit` answers it.
        Ok(AuditWindow::Newest) => {
            return ApiError::bad_request("invalid_range").into_response();
        }
        Err(error) => return error.into_response(),
    };
    let (from_ms, to_ms) = window;
    let from = match export_instant(from_ms) {
        Ok(from) => from,
        Err(error) => return error.into_response(),
    };
    let to = match export_instant(to_ms) {
        Ok(to) => to,
        Err(error) => return error.into_response(),
    };

    let read = app.audit.range(from_ms, to_ms).await;
    let range_header = format!("from={from_ms}; to={to_ms}");
    if read.bounded {
        let message = format!(
            "the range needs more than the {}-line scan bound; ask for a narrower range",
            thousands(crate::audit::DAY_SCAN_LIMIT)
        );
        let mut response = (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "error": "range_too_large", "message": message })),
        )
            .into_response();
        insert_audit_header(response.headers_mut(), "x-audit-range", &range_header);
        insert_audit_header(
            response.headers_mut(),
            "x-audit-bounded",
            &crate::audit::DAY_SCAN_LIMIT.to_string(),
        );
        return response;
    }
    let since_ms = read.since_ms;
    let mut lines: Vec<Value> = read
        .lines
        .into_iter()
        .filter(|line| match query.device.as_deref() {
            Some(device) => line.get("device_id").and_then(Value::as_str) == Some(device),
            None => true,
        })
        .collect();
    // The scan reads backwards, so the vector is newest first; reversing puts the file in the order
    // a history is read without re-sorting equal moments, which keeps the file's own order.
    lines.reverse();
    let lines = with_sentences(lines);
    let mut body = String::new();
    for line in &lines {
        body.push_str(&line.to_string());
        body.push('\n');
    }

    let mut response = (StatusCode::OK, body).into_response();
    let filename = export_filename(from, to, query.device.as_deref());
    insert_audit_header(
        response.headers_mut(),
        header::CONTENT_TYPE.as_str(),
        "application/x-ndjson",
    );
    insert_audit_header(
        response.headers_mut(),
        header::CONTENT_DISPOSITION.as_str(),
        &format!("attachment; filename=\"{filename}\""),
    );
    insert_audit_header(response.headers_mut(), "x-audit-range", &range_header);
    if let Some(since_ms) = since_ms {
        insert_audit_header(
            response.headers_mut(),
            "x-audit-since",
            &since_ms.to_string(),
        );
    }
    response
}

/// A moment the export can name, or the range refusal when the number is not one.
///
/// The query parses `u64`; chrono names an instant as `i64` milliseconds, and a number outside that
/// range is not a date any filename could state. Refusing it keeps the filename from inventing one.
fn export_instant(ms: u64) -> Result<chrono::DateTime<chrono::Utc>, ApiError> {
    use chrono::{TimeZone, Utc};

    let ms = i64::try_from(ms).map_err(|_| ApiError::bad_request("invalid_range"))?;
    Utc.timestamp_millis_opt(ms)
        .single()
        .ok_or_else(|| ApiError::bad_request("invalid_range"))
}

/// The download's filename: `summrise-audit-<from>_to_<to>[_<device short>].ndjson`.
///
/// The moments are basic ISO 8601 in UTC, milliseconds kept, so two ranges inside one second do not
/// share a name. Colons are not legal in a Windows filename; the basic form has none.
fn export_filename(
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    device: Option<&str>,
) -> String {
    let compact = |at: chrono::DateTime<chrono::Utc>| {
        format!(
            "{}.{:03}Z",
            at.format("%Y%m%dT%H%M%S"),
            at.timestamp_subsec_millis()
        )
    };
    let suffix = device
        .and_then(export_device_short)
        .map(|short| format!("_{short}"))
        .unwrap_or_default();
    format!(
        "summrise-audit-{}_to_{}{suffix}.ndjson",
        compact(from),
        compact(to)
    )
}

/// The device's short id as a filename part: its first twelve characters while they are the
/// canonical id's alphabet. A caller-supplied string outside that alphabet is not put in a name the
/// browser writes to disk.
fn export_device_short(device: &str) -> Option<String> {
    let short: String = device.chars().take(12).collect();
    (!short.is_empty()
        && short
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-'))
    .then_some(short)
}

/// Put one ASCII header on a response. Every value here is built from numbers or a sanitised device
/// id, so a value the type rejects is a bug in this file, not a caller's input.
fn insert_audit_header(headers: &mut HeaderMap, name: &'static str, value: &str) {
    let value = HeaderValue::from_str(value).expect("an audit export header is ASCII");
    headers.insert(name, value);
}

/// `50000` as `50,000`: the number a person reads in a refusal.
fn thousands(count: usize) -> String {
    let digits = count.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    grouped
}

/// The same sentence the live stream carries, so a fetched window and a streamed line read the
/// same way. A stored line never had the call's arguments, so this is the general sentence for its
/// kind rather than the call's own.
fn with_sentences(lines: Vec<Value>) -> Vec<Value> {
    lines
        .into_iter()
        .map(|mut line| {
            let sentence = crate::activity::line_sentence(&line);
            line["sentence"] = json!(sentence);
            line
        })
        .collect()
}

/// The most traces one join may ask about.
///
/// The page marks one Activity window and asks about the traces it drew, at most this many. The
/// device accepts a larger list than this, but nothing the console draws ever needs one: the bound
/// keeps a single read from asking a machine to scan for an unbounded set of names, and the refusal
/// is the console's own sentence rather than a device's.
const MAX_JOIN_TRACES: usize = 128;

/// The longest one trace may be. The console mints every trace on a proxied call, so a caller
/// asking about a longer string is not asking about a call this console recorded.
const MAX_JOIN_TRACE_CHARS: usize = 128;

/// The `?device=&traces=` shape `/audit/join` reads.
///
/// `traces` arrives as one comma-separated string because it is a GET whose answer is a read, and
/// repeated parameters would need a decoder this route does not have. The order is the caller's
/// and is kept: the device answers in it, and a page that renders the mark keeps its own order
/// too — the arrays are its own traces back, not a set.
#[derive(Deserialize)]
struct JoinQuery {
    device: Option<String>,
    traces: Option<String>,
}

/// `GET /audit/join?device=&traces=`: which of these calls the machine's journal still holds.
///
/// One proxied call is two records that share `trace.call`: the console's audit line and the
/// machine's `call.started`/`call.finished|denied` pair. Only the machine can say which pairs its
/// journal still retains — an interval would be a guess, because a rotated journal is not
/// contiguous with a rotated audit file. So this route asks the device's own `journal.get` over
/// the channel, once per window, and relays the answer unchanged.
///
/// **IT WRITES NO AUDIT LINE AND NO LIVE EVENT.** A join is not an operator action; it is the page
/// asking a question about the window it is already drawing. A read that audited itself would grow
/// the very window it describes, and one Activity row per window would be the console narrating
/// its own reading. The frame still carries a trace the console minted, so the console's relay
/// drops the `call.*` pair the machine writes for the read — the machine's journal records that it
/// was read, and Activity does not grow a row for it.
async fn audit_join(
    State(app): State<AppState>,
    _: OperatorSession,
    Query(query): Query<JoinQuery>,
) -> Response {
    let Some(presented) = query.device else {
        return ApiError::bad_request("device_required")
            .saying("name the machine whose journal to read; ?device= takes its id")
            .into_response();
    };
    let Some(device_id) = audit_device_id(&presented) else {
        return unknown_device(&presented).into_response();
    };
    if !app.has_device(&device_id) {
        return unknown_device(&presented).into_response();
    }
    let traces = match join_traces(query.traces.as_deref().unwrap_or_default()) {
        Ok(traces) => traces,
        Err(error) => return error.into_response(),
    };
    let asked = traces.len();
    let outcome = read_journal_join(&app, &device_id, &traces).await;
    let mut view = JoinView {
        device: device_id.clone(),
        asked,
        joined: Vec::new(),
        missing: Vec::new(),
        unknown: false,
        journal: false,
        bounded: false,
        oldest_seq: 0,
        last_seq: 0,
        // The moment the answer was built, on the console's clock: it dates the read, not the
        // machine's journal, whose two ends are the seq bounds below.
        read_at_ms: (app.now)(),
        reason: None,
    };
    match outcome {
        Ok(facts) => {
            view.joined = facts.joined;
            view.missing = facts.missing;
            view.journal = facts.journal;
            view.bounded = facts.bounded;
            view.oldest_seq = facts.oldest_seq;
            view.last_seq = facts.last_seq;
        }
        Err(JoinUnreadable::Transport(error)) => {
            return error.refusal(&app, &device_id).into_response();
        }
        // A device that answered but could not answer THIS question leaves every row unanswered.
        // The page draws no claim about a machine it could not ask, and `reason` is for the operator
        // and the tests: the page ignores it and reads `unknown` alone.
        Err(JoinUnreadable::Unanswered(reason)) => {
            view.unknown = true;
            view.reason = Some(reason);
        }
    }
    Json(view).into_response()
}

/// One join as `/audit/join` serves it: the fixed wire contract the page reads.
///
/// `device` is the canonical id, not the one the caller presented, so the page's own key matches
/// the audit lines' `device_id`. `read_at_ms` is the console's clock when the answer was built;
/// it dates the read, not the machine's journal, whose bounds are `oldest_seq`/`last_seq`.
#[derive(serde::Serialize)]
struct JoinView {
    device: String,
    asked: usize,
    joined: Vec<String>,
    missing: Vec<String>,
    unknown: bool,
    journal: bool,
    bounded: bool,
    oldest_seq: u64,
    last_seq: u64,
    read_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

/// The device's own facts about a join, already relayed unchanged.
struct JoinFacts {
    joined: Vec<String>,
    missing: Vec<String>,
    journal: bool,
    bounded: bool,
    oldest_seq: u64,
    last_seq: u64,
}

/// Why a join has no facts: the device could not be reached, or answered something else.
enum JoinUnreadable {
    /// The machine could not be asked. This is a link failure, not an answer: it is refused with the
    /// same 503/504 sentence every other per-device read uses, so one silence reads one way.
    Transport(LiveToolsError),
    /// The device answered, and its answer is not a join: an older build that does not offer the
    /// tool, or a result this console cannot read. The route answers 200 with `unknown`.
    Unanswered(&'static str),
}

/// Parse the comma-separated trace list, or the 400 that names the rule it broke.
///
/// The order is the caller's and duplicates are kept: the device answers in that order, and the page
/// deduplicates its own side because it is the page's window that repeats a trace. Every rule here
/// is a bound the device also enforces; the console states it first so the operator reads one
/// sentence about one request, not a device refusal relayed through two layers.
fn join_traces(text: &str) -> Result<Vec<String>, ApiError> {
    let traces: Vec<&str> = text.split(',').map(str::trim).collect();
    if traces.len() > MAX_JOIN_TRACES {
        return Err(ApiError::bad_request("invalid_traces").saying(format!(
            "one join may ask about at most {MAX_JOIN_TRACES} traces; ask about a shorter window"
        )));
    }
    if let Some(long) = traces
        .iter()
        .find(|trace| trace.chars().count() > MAX_JOIN_TRACE_CHARS)
    {
        return Err(ApiError::bad_request("invalid_traces").saying(format!(
            "each trace is at most {MAX_JOIN_TRACE_CHARS} characters, and one of these is {}",
            long.chars().count()
        )));
    }
    if traces.iter().all(|trace| trace.is_empty()) {
        return Err(ApiError::bad_request("invalid_traces").saying(
            "name at least one trace to join; ?traces= is a comma-separated list of the calls the \
             window drew",
        ));
    }
    if traces.iter().any(|trace| trace.is_empty()) {
        return Err(ApiError::bad_request("invalid_traces")
            .saying("every trace must be a non-empty string, and this list has an empty entry"));
    }
    Ok(traces.into_iter().map(str::to_owned).collect())
}

/// Ask the device's `journal.get` over its channel, under the operator's own scope.
///
/// Not through [`proxy_reply`]: that path is the operator's proxied call, and it writes the audit
/// line and the live event this read must not write. The frame is built here, with a minted trace so
/// the device's journal pair for it is recognised by the relay and not drawn twice.
async fn read_journal_join(
    app: &AppState,
    device_id: &str,
    traces: &[String],
) -> Result<JoinFacts, JoinUnreadable> {
    // A registered but disconnected device is offline now; asking the channel would wait out the
    // reconnect bound before saying the same thing.
    if !app.channels.online(device_id) {
        return Err(JoinUnreadable::Transport(LiveToolsError::Unavailable));
    }

    let scope = operator_scope(device_id);
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": "journal.get", "arguments": { "traces": traces } },
    });
    // The trace is minted for the same reason `/fleet/:id/tools` mints one: the console's relay
    // drops a `call.*` pair it recognises as its own, so this read stays out of Activity.
    let trace = app.channels.mint_trace();
    let answer = tokio::time::timeout(
        app.mcp_timeout,
        app.channels
            .request(device_id, request, &RequestAuth::Token(&scope), &trace),
    )
    .await;

    let reply = match answer {
        Err(_) => return Err(JoinUnreadable::Transport(LiveToolsError::Timeout)),
        Ok(Err(RequestError::Offline | RequestError::Busy | RequestError::Disconnected)) => {
            return Err(JoinUnreadable::Transport(LiveToolsError::Unavailable))
        }
        Ok(Err(RequestError::SendTimeout | RequestError::ReplyTimeout)) => {
            return Err(JoinUnreadable::Transport(LiveToolsError::Timeout))
        }
        Ok(Ok(reply)) => reply,
    };

    let Reply::Result(_) = reply else {
        // A JSON-RPC error is the device's answer. It is how a build that predates `journal.get`
        // says so, and the page must read that as unknown rather than as every row missing.
        return Err(JoinUnreadable::Unanswered(join_refusal(&reply)));
    };
    // MCP carries a tool-level failure inside a successful exchange: a result with `isError: true`
    // and the reason in its text. That is a refusal, not a body this console failed to parse, and
    // the text is a sentence rather than the join object.
    if tool_result(&reply)
        .and_then(|result| result.get("isError"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Err(JoinUnreadable::Unanswered("refused"));
    }
    join_facts(&reply).ok_or(JoinUnreadable::Unanswered("malformed"))
}

/// Which of the two refusals a device's JSON-RPC error is: the tool is absent, or it said no.
///
/// A build older than `journal.get` answers one of two ways: `-32601` when it does not dispatch
/// `tools/call` at all, or `-32602` with "unknown tool: journal.get" when it dispatches calls but
/// does not list this one. Both mean the machine was never able to answer; everything else — a
/// grant that does not cover the tool, a device that declined the read — is an answer that leaves
/// the question open. Both are `unknown`; only their `reason` differs.
fn join_refusal(reply: &Reply) -> &'static str {
    let names_a_missing_tool = error_message(reply).is_some_and(|message| {
        let message = message.to_ascii_lowercase();
        message.contains("unknown tool") || message.contains("no such tool")
    });
    if error_code(reply) == Some(-32601) || names_a_missing_tool {
        "unknown_tool"
    } else {
        "refused"
    }
}

/// The join object inside a `tools/call` answer's first text item, when it is one.
///
/// The device's text is a JSON object with the six facts below; every field is required, because a
/// missing `traces` or `missing` would otherwise become an empty array and the page would mark every
/// row as gone on a device that simply did not say.
fn join_facts(reply: &Reply) -> Option<JoinFacts> {
    let body: Value = serde_json::from_str(tool_text(reply)?).ok()?;
    Some(JoinFacts {
        joined: string_array(body.get("traces")?)?,
        missing: string_array(body.get("missing")?)?,
        journal: body.get("journal")?.as_bool()?,
        bounded: body.get("bounded")?.as_bool()?,
        oldest_seq: body.get("oldest_seq")?.as_u64()?,
        last_seq: body.get("last_seq")?.as_u64()?,
    })
}

/// A JSON array of strings as a `Vec<String>`, or `None` when one entry is not a string.
fn string_array(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|entry| entry.as_str().map(str::to_owned))
        .collect()
}

/// The most rows one read may ask a machine to summarise.
///
/// The page draws one window of the machine's record, and this is the window: the device accepts
/// exactly this many, so the console states the bound itself rather than relaying a device's refusal.
/// A larger read would make the console hold an unbounded slice of another machine's journal.
const MAX_JOURNAL_ROWS: usize = 200;

/// The longest range `/journal/day` will ask a machine to count.
///
/// A local day is 23–25 hours and the page's cut-based block is 24; the bound is the device's own,
/// stated here first so the operator reads one sentence about one request.
const MAX_DAY_SPAN_MS: u64 = 26 * 60 * 60 * 1000;

/// The `?device=&before=&limit=` shape `/journal/read` reads.
///
/// `before` and `limit` are strings so a malformed value is this service's JSON refusal with a
/// sentence, not axum's plain-text query rejection. `before` is the `seq` of the last row the page
/// already drew; absent means the page asks for the newest rows.
#[derive(Deserialize)]
struct JournalReadQuery {
    device: Option<String>,
    before: Option<String>,
    limit: Option<String>,
}

/// The `?device=&from=&to=` shape `/journal/day` reads.
///
/// Both moments are strings so a malformed value is this service's JSON refusal with a sentence,
/// not axum's plain-text query rejection. The device keeps no timezone: the console names the two
/// instants of the day it counts.
#[derive(Deserialize)]
struct JournalDayQuery {
    device: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

/// `GET /journal/read?device=&before=&limit=`: the window the machine's own journal holds.
///
/// Activity is drawn from two records. The console's audit is what came through the proxy; the
/// machine's journal is everything the machine ran, including the calls that never reached the
/// console at all. Only the machine can summarise the second, so this route asks its own
/// `journal.read` over the channel and relays the window, one bounded page at a time.
///
/// **IT WRITES NO AUDIT LINE AND NO LIVE EVENT**, for the join's reason: a read about the window the
/// page draws must not grow that window. The frame still carries a trace the console minted, so the
/// relay drops the `call.*` pair the machine writes for the read.
async fn journal_read(
    State(app): State<AppState>,
    _: OperatorSession,
    Query(query): Query<JournalReadQuery>,
) -> Response {
    let Some(presented) = query.device else {
        return ApiError::bad_request("device_required")
            .saying("name the machine whose journal to read; ?device= takes its id")
            .into_response();
    };
    let Some(device_id) = audit_device_id(&presented) else {
        return unknown_device(&presented).into_response();
    };
    if !app.has_device(&device_id) {
        return unknown_device(&presented).into_response();
    }
    let limit = match journal_limit(query.limit.as_deref()) {
        Ok(limit) => limit,
        Err(error) => return error.into_response(),
    };
    let before = match journal_before(query.before.as_deref()) {
        Ok(before) => before,
        Err(error) => return error.into_response(),
    };

    let outcome = read_journal_rows(&app, &device_id, before, limit).await;
    let mut view = JournalReadView {
        device: device_id.clone(),
        journal: false,
        oldest_seq: 0,
        last_seq: 0,
        rows: Vec::new(),
        more: false,
        // The moment the answer was built, on the console's clock: it dates the read, not the rows
        // the machine recorded, whose own times are in each row's `ts`.
        read_at_ms: (app.now)(),
        unknown: false,
        reason: None,
    };
    match outcome {
        Ok(window) => {
            view.journal = window.journal;
            view.oldest_seq = window.oldest_seq;
            view.last_seq = window.last_seq;
            view.rows = window.rows;
            view.more = window.more;
        }
        Err(JournalUnreadable::Transport(error)) => {
            return error.refusal(&app, &device_id).into_response();
        }
        // A machine that answered but could not answer THIS question leaves the window unanswered.
        // The page draws no claim about a record it could not read, and `reason` is for the operator
        // and the tests; the page reads `unknown` alone. **AN EMPTY WINDOW HERE WOULD BE DRAWN AS
        // "THE MACHINE RECORDED NOTHING"**, which is the one thing this route must not say.
        Err(JournalUnreadable::Unanswered(reason)) => {
            view.unknown = true;
            view.reason = Some(reason);
        }
    }
    Json(view).into_response()
}

/// `GET /journal/day?device=&from=&to=`: the machine's own count of a day.
///
/// The console's day sentence counts its audit; a client may call the machine directly, and those
/// calls exist only in its journal. A page of rows cannot answer for a day, so this route asks for
/// the aggregate — per-door call and failure counts inside the range — and merges it into the
/// sentence. **IT WRITES NO AUDIT LINE AND NO LIVE EVENT**, and its frame carries a minted trace so
/// the relay drops the pair the read provokes.
async fn journal_day(
    State(app): State<AppState>,
    _: OperatorSession,
    Query(query): Query<JournalDayQuery>,
) -> Response {
    let Some(presented) = query.device else {
        return ApiError::bad_request("device_required")
            .saying("name the machine whose day to count; ?device= takes its id")
            .into_response();
    };
    let Some(device_id) = audit_device_id(&presented) else {
        return unknown_device(&presented).into_response();
    };
    if !app.has_device(&device_id) {
        return unknown_device(&presented).into_response();
    }
    let from = match journal_day_moment(
        query.from.as_deref(),
        "from",
        "invalid_day_from",
        "?from= is the day's first moment, in Unix milliseconds",
    ) {
        Ok(from) => from,
        Err(error) => return error.into_response(),
    };
    let to = match journal_day_moment(
        query.to.as_deref(),
        "to",
        "invalid_day_to",
        "?to= is the moment after the day, in Unix milliseconds",
    ) {
        Ok(to) => to,
        Err(error) => return error.into_response(),
    };
    if to <= from {
        return ApiError::bad_request("invalid_day_range")
            .saying("to must be after from: the range is half-open, [from, to)")
            .into_response();
    }
    if to - from > MAX_DAY_SPAN_MS {
        return ApiError::bad_request("invalid_day_span")
            .saying(format!(
                "the range must be no more than {} hours apart",
                MAX_DAY_SPAN_MS / 3_600_000
            ))
            .into_response();
    }

    let outcome = read_journal_day(&app, &device_id, from, to).await;
    let mut view = JournalDayView {
        device: device_id.clone(),
        journal: false,
        day_start_ms: from,
        day_end_ms: to,
        oldest_seq: 0,
        oldest_ts: None,
        last_seq: 0,
        scanned: 0,
        more: false,
        calls: DayCalls::zero(),
        // The moment the answer was built, on the console's clock: it dates the read, not the calls
        // the machine counted.
        read_at_ms: (app.now)(),
        unknown: false,
        reason: None,
    };
    match outcome {
        Ok(day) => {
            view.journal = day.journal;
            view.day_start_ms = day.day_start_ms;
            view.day_end_ms = day.day_end_ms;
            view.oldest_seq = day.oldest_seq;
            view.oldest_ts = day.oldest_ts;
            view.last_seq = day.last_seq;
            view.scanned = day.scanned;
            view.more = day.more;
            view.calls = day.calls;
        }
        Err(JournalUnreadable::Transport(error)) => {
            return error.refusal(&app, &device_id).into_response();
        }
        // A machine that answered but could not answer THIS question leaves the day unanswered. The
        // page draws no claim about a day it could not read, and never a zero it did not measure.
        Err(JournalUnreadable::Unanswered(reason)) => {
            view.unknown = true;
            view.reason = Some(reason);
        }
    }
    Json(view).into_response()
}

/// One day as `/journal/day` serves it: the fixed wire contract the page reads.
///
/// `device` is the canonical id, and `read_at_ms` is the console's clock when the answer was built.
/// `scanned` and `more` are the read's bounds, so a page can say "at least" when the machine's scan
/// was cut rather than print a precise number the machine did not prove.
#[derive(serde::Serialize)]
struct JournalDayView {
    device: String,
    journal: bool,
    day_start_ms: u64,
    day_end_ms: u64,
    oldest_seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    oldest_ts: Option<String>,
    last_seq: u64,
    scanned: u64,
    more: bool,
    calls: DayCalls,
    read_at_ms: u64,
    unknown: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

/// One door's count inside a day: the calls that finished and how many of them failed.
#[derive(serde::Serialize, Clone, Copy)]
struct DayCallCount {
    calls: u64,
    failed: u64,
}

/// The five doors a journal row can name; an unrecognized door is `unknown`.
#[derive(serde::Serialize)]
struct DayCalls {
    console: DayCallCount,
    panel: DayCallCount,
    mesh: DayCallCount,
    loopback: DayCallCount,
    unknown: DayCallCount,
}

impl DayCalls {
    fn zero() -> Self {
        let zero = DayCallCount {
            calls: 0,
            failed: 0,
        };
        Self {
            console: zero,
            panel: zero,
            mesh: zero,
            loopback: zero,
            unknown: zero,
        }
    }
}

/// The device's own day, already relayed field by field.
struct JournalDayFacts {
    journal: bool,
    day_start_ms: u64,
    day_end_ms: u64,
    oldest_seq: u64,
    oldest_ts: Option<String>,
    last_seq: u64,
    scanned: u64,
    more: bool,
    calls: DayCalls,
}

/// One window as `/journal/read` serves it: the fixed wire contract the page reads.
///
/// `device` is the canonical id, not the one the caller presented, so the page's own key matches the
/// audit lines' `device_id`. `read_at_ms` is the console's clock when the answer was built.
#[derive(serde::Serialize)]
struct JournalReadView {
    device: String,
    journal: bool,
    oldest_seq: u64,
    last_seq: u64,
    rows: Vec<MachineRow>,
    more: bool,
    read_at_ms: u64,
    unknown: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

/// One row of the machine's record, as the page draws it: a summary, never a payload.
///
/// Every optional field is omitted rather than defaulted when the device did not send the type the
/// contract names, because a page reading `ok` as a boolean or `seq` as a number must never be handed
/// a string or a blank. `payload_bytes` says how much the row's payload held — the only fact about a
/// payload this route relays.
#[derive(serde::Serialize)]
struct MachineRow {
    seq: u64,
    #[serde(rename = "type")]
    kind: String,
    ts: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    door: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload_bytes: Option<u64>,
}

/// The device's own window, already relayed field by field.
struct JournalFacts {
    journal: bool,
    oldest_seq: u64,
    last_seq: u64,
    rows: Vec<MachineRow>,
    more: bool,
}

/// Why a window has no facts: the machine could not be reached, or answered something else.
enum JournalUnreadable {
    /// The machine could not be asked. This is a link failure, not an answer: it is refused with the
    /// same 503/504 sentence every other per-device read uses, so one silence reads one way.
    Transport(LiveToolsError),
    /// The device answered, and its answer is not a window: an older build that does not offer the
    /// tool, a refusal, or a result this console cannot read. The route answers 200 with `unknown`.
    Unanswered(&'static str),
}

/// Parse `?limit=`, or the 400 that names the rule it broke.
///
/// Missing means [`MAX_JOURNAL_ROWS`]: the page draws one window, and the newest page is what it
/// draws. The bound is the device's own, stated here first so the operator reads one sentence about
/// one request rather than a device refusal relayed through two layers.
fn journal_limit(text: Option<&str>) -> Result<usize, ApiError> {
    let Some(text) = text else {
        return Ok(MAX_JOURNAL_ROWS);
    };
    let invalid = || {
        ApiError::bad_request("invalid_limit").saying(format!(
            "limit must be a whole number from 1 to {MAX_JOURNAL_ROWS}; ?limit= is how many rows of \
             the machine's journal to read"
        ))
    };
    match text.parse::<usize>() {
        Ok(limit) if (1..=MAX_JOURNAL_ROWS).contains(&limit) => Ok(limit),
        _ => Err(invalid()),
    }
}

/// Parse `?before=`: the sequence of the last row the page already drew, or `None` for the newest.
///
/// A sequence number starts at one, so zero names no row; a page that asks for rows before it would
/// otherwise get an empty window that reads as "the machine recorded nothing".
fn journal_before(text: Option<&str>) -> Result<Option<u64>, ApiError> {
    let Some(text) = text else {
        return Ok(None);
    };
    match text.parse::<u64>() {
        Ok(before) if before > 0 => Ok(Some(before)),
        _ => Err(ApiError::bad_request("invalid_before").saying(
            "before must be a positive sequence number: ?before= is the seq of the last row already \
             drawn, and the answer is the older rows",
        )),
    }
}

/// Parse `?from=` or `?to=`: a whole Unix-millisecond moment, or the 400 that names the rule.
///
/// The two fields have their own error codes because a page that asked badly should learn which end
/// of the range it asked badly about. The moment is required: a day without a start is not a range.
fn journal_day_moment(
    text: Option<&str>,
    field: &str,
    code: &'static str,
    saying: &'static str,
) -> Result<u64, ApiError> {
    let sentence = format!("{field} is required: {saying}, a whole number of Unix milliseconds");
    let Some(text) = text else {
        return Err(ApiError::bad_request(code).saying(sentence));
    };
    text.parse::<u64>()
        .map_err(|_| ApiError::bad_request(code).saying(sentence))
}

/// Ask the machine's `journal.read` for one window: the row count, and the exclusive cursor when the
/// page is paging older than a row it already drew.
async fn read_journal_rows(
    app: &AppState,
    device_id: &str,
    before: Option<u64>,
    limit: usize,
) -> Result<JournalFacts, JournalUnreadable> {
    let mut arguments = json!({ "limit": limit });
    if let Some(before) = before {
        arguments["before_seq"] = json!(before);
    }
    let body = ask_journal(app, device_id, "journal.read", arguments).await?;
    journal_facts(&body).ok_or(JournalUnreadable::Unanswered("malformed"))
}

/// Ask the machine's `journal.day` for the day's per-door counts.
async fn read_journal_day(
    app: &AppState,
    device_id: &str,
    from: u64,
    to: u64,
) -> Result<JournalDayFacts, JournalUnreadable> {
    let body = ask_journal(
        app,
        device_id,
        "journal.day",
        json!({ "day_start_ms": from, "day_end_ms": to }),
    )
    .await?;
    journal_day_facts(&body).ok_or(JournalUnreadable::Unanswered("malformed"))
}

/// Ask one of the journal's read tools over the machine's channel, under the operator's own scope.
///
/// Not through [`proxy_reply`]: that path is the operator's proxied call, and it writes the audit
/// line and the live event these reads must not write. The frame is built here, with a minted trace
/// so the machine's journal pair for it is recognised by the relay and not drawn twice.
async fn ask_journal(
    app: &AppState,
    device_id: &str,
    tool: &'static str,
    arguments: Value,
) -> Result<Value, JournalUnreadable> {
    // A registered but disconnected device is offline now; asking the channel would wait out the
    // reconnect bound before saying the same thing.
    if !app.channels.online(device_id) {
        return Err(JournalUnreadable::Transport(LiveToolsError::Unavailable));
    }

    let scope = operator_scope(device_id);
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": arguments },
    });
    // The trace is minted for the same reason `/fleet/:id/tools` mints one: the console's relay
    // drops a `call.*` pair it recognises as its own, so this read stays out of Activity.
    let trace = app.channels.mint_trace();
    let answer = tokio::time::timeout(
        app.mcp_timeout,
        app.channels
            .request(device_id, request, &RequestAuth::Token(&scope), &trace),
    )
    .await;

    let reply = match answer {
        Err(_) => return Err(JournalUnreadable::Transport(LiveToolsError::Timeout)),
        Ok(Err(RequestError::Offline | RequestError::Busy | RequestError::Disconnected)) => {
            return Err(JournalUnreadable::Transport(LiveToolsError::Unavailable))
        }
        Ok(Err(RequestError::SendTimeout | RequestError::ReplyTimeout)) => {
            return Err(JournalUnreadable::Transport(LiveToolsError::Timeout))
        }
        Ok(Ok(reply)) => reply,
    };

    let Reply::Result(_) = reply else {
        // A JSON-RPC error is the device's answer. It is how a build that predates the tool says so,
        // and the page must read that as unknown rather than as a machine that recorded nothing.
        return Err(JournalUnreadable::Unanswered(journal_refusal(&reply, tool)));
    };
    // MCP carries a tool-level failure inside a successful exchange: a result with `isError: true`
    // and the reason in its text. That is a refusal, not a body this console failed to parse, and
    // the text is a sentence rather than the document.
    if tool_result(&reply)
        .and_then(|result| result.get("isError"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Err(JournalUnreadable::Unanswered("refused"));
    }
    let text = tool_text(&reply).ok_or(JournalUnreadable::Unanswered("malformed"))?;
    serde_json::from_str(text).map_err(|_| JournalUnreadable::Unanswered("malformed"))
}

fn journal_refusal(reply: &Reply, tool: &str) -> &'static str {
    let names_a_missing_tool = error_message(reply).is_some_and(|message| {
        let message = message.to_ascii_lowercase();
        message.contains(&tool.to_ascii_lowercase())
            && (message.contains("unknown tool") || message.contains("no such tool"))
    });
    if error_code(reply) == Some(-32601) || names_a_missing_tool {
        "unknown_tool"
    } else {
        "refused"
    }
}

/// The device's day, field by field, or `None` when the answer is not the shape the route serves.
///
/// Every field is required: a missing `calls` would otherwise become five zeros and the page would
/// print a precise day the machine never measured. `oldest_ts` is optional because a journal that
/// retained nothing has no oldest moment to name.
fn journal_day_facts(body: &Value) -> Option<JournalDayFacts> {
    Some(JournalDayFacts {
        journal: body.get("journal")?.as_bool()?,
        day_start_ms: body.get("day_start_ms")?.as_u64()?,
        day_end_ms: body.get("day_end_ms")?.as_u64()?,
        oldest_seq: body.get("oldest_seq")?.as_u64()?,
        oldest_ts: body
            .get("oldest_ts")
            .and_then(Value::as_str)
            .map(str::to_owned),
        last_seq: body.get("last_seq")?.as_u64()?,
        scanned: body.get("scanned")?.as_u64()?,
        more: body.get("more")?.as_bool()?,
        calls: day_calls(body.get("calls")?)?,
    })
}

/// The five doors' counts, each one a pair of whole numbers.
fn day_calls(value: &Value) -> Option<DayCalls> {
    let count = |door: &str| -> Option<DayCallCount> {
        let entry = value.get(door)?;
        Some(DayCallCount {
            calls: entry.get("calls")?.as_u64()?,
            failed: entry.get("failed")?.as_u64()?,
        })
    };
    Some(DayCalls {
        console: count("console")?,
        panel: count("panel")?,
        mesh: count("mesh")?,
        loopback: count("loopback")?,
        unknown: count("unknown")?,
    })
}

/// The window object inside a `tools/call` answer's first text item, when it is one.
///
/// The four envelope facts are required, because a missing `rows` would otherwise become an empty
/// window and the page would draw that answer as a machine with nothing retained. The rows inside it
/// are read one field at a time: each field is copied only when it has the type the contract names,
/// and a row with no `seq`, `ts` or `type` is dropped rather than drawn as blanks.
fn journal_facts(body: &Value) -> Option<JournalFacts> {
    let rows = body
        .get("rows")?
        .as_array()?
        .iter()
        .filter_map(machine_row)
        .collect();
    Some(JournalFacts {
        journal: body.get("journal")?.as_bool()?,
        oldest_seq: body.get("oldest_seq")?.as_u64()?,
        last_seq: body.get("last_seq")?.as_u64()?,
        rows,
        more: body.get("more")?.as_bool()?,
    })
}

/// One row of the machine's record, field by field, or `None` when it lacks its own identity.
fn machine_row(row: &Value) -> Option<MachineRow> {
    Some(MachineRow {
        seq: row.get("seq")?.as_u64()?,
        ts: row.get("ts")?.as_str()?.to_owned(),
        kind: row.get("type")?.as_str()?.to_owned(),
        trace: row.get("trace").and_then(Value::as_str).map(str::to_owned),
        tool: row.get("tool").and_then(Value::as_str).map(str::to_owned),
        ok: row.get("ok").and_then(Value::as_bool),
        reason: row.get("reason").and_then(Value::as_str).map(str::to_owned),
        door: row.get("door").and_then(Value::as_str).map(str::to_owned),
        payload_bytes: row.get("payload_bytes").and_then(Value::as_u64),
    })
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

    /// **A TYPED PATH LANDS ON THE PAGE IT NAMES; AN API PATH STAYS A 404** (the hundred-and-forty-fourth list). Measured
    /// live 2026-11-12: `GET /activity` answered 404 with an empty body, because this console routes in the hash.
    #[tokio::test]
    async fn a_typed_path_lands_on_its_page_and_an_api_path_stays_a_404() {
        use axum::http::{StatusCode, Uri};

        let typed = not_found(Uri::from_static("/activity")).await;
        assert_eq!(typed.status(), StatusCode::SEE_OTHER);
        assert_eq!(typed.headers().get("location").unwrap(), "/#activity");

        // A nested destination keeps its whole path; nothing is guessed away.
        let nested = not_found(Uri::from_static("/device/abc")).await;
        assert_eq!(nested.headers().get("location").unwrap(), "/#device/abc");

        // The API, the transport and files keep the honest answer.
        for path in [
            "/api/nope",
            "/mcp",
            "/fleet/x/y",
            "/files/a.png",
            "/activity/shot/d/1",
        ] {
            let answer = not_found(Uri::from_static(path)).await;
            assert_eq!(answer.status(), StatusCode::NOT_FOUND, "{path}");
        }
    }

    /// A parked call's wait follows the argument the client sent; everything else leaves the
    /// general bound in charge.
    #[test]
    fn a_parked_call_waits_for_its_own_bound_and_other_tools_do_not() {
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
            call_wait(&call("runs.exec", json!({"command": "ver"}))),
            Some(Duration::from_secs(65))
        );
        // A requested timeout is honoured, so a 45 s run is not cut at 30 s.
        assert_eq!(
            call_wait(&call(
                "runs.exec",
                json!({"command": "ver", "timeout_ms": 45_000})
            )),
            Some(Duration::from_secs(50))
        );
        // A ten-minute run is NOT waited for end to end: the proxy in front of the console gives up
        // at about a hundred seconds, so the console answers first and the run is collected later.
        assert_eq!(
            call_wait(&call(
                "runs.exec",
                json!({"command": "ver", "timeout_ms": 86_400_000})
            )),
            Some(RUNS_MAX_PROXY_WAIT)
        );
        assert!(
            RUNS_MAX_PROXY_WAIT < Duration::from_secs(100),
            "the bound must sit under the proxy's ceiling"
        );
        // **A READ'S WAIT IS THE DEVICE'S WAIT.** A 1.5 s long poll is not cut at the console's
        // general 30 s bound, and a 30 s one (the device's cap) gets the grace that keeps the
        // console from racing the device's own deadline.
        assert_eq!(
            call_wait(&call(
                "terminal.read",
                json!({"id": "session-1", "wait_ms": 1_500})
            )),
            Some(Duration::from_millis(11_500))
        );
        assert_eq!(
            call_wait(&call(
                "terminal.read",
                json!({"id": "session-1", "wait_ms": 30_000})
            )),
            Some(Duration::from_secs(40))
        );
        // The device clamps a larger wait to 30 s, so the console's bound must not grow past it.
        assert_eq!(
            call_wait(&call(
                "terminal.read",
                json!({"id": "session-1", "wait_ms": 86_400_000})
            )),
            Some(Duration::from_secs(40))
        );
        // No wait (or a malformed one) is not a long poll: the general bound stays in charge.
        assert_eq!(
            call_wait(&call("terminal.read", json!({"id": "session-1"}))),
            None
        );
        assert_eq!(
            call_wait(&call(
                "terminal.read",
                json!({"id": "session-1", "wait_ms": 0})
            )),
            None
        );
        assert_eq!(
            call_wait(&call(
                "terminal.read",
                json!({"id": "session-1", "wait_ms": "1000"})
            )),
            None
        );
        // Anything that is not a run or a read, or is malformed, leaves the general bound alone.
        assert_eq!(call_wait(&call("memory.list", json!({}))), None);
        assert_eq!(
            call_wait(&json!({"method": "initialize", "params": {}})),
            None
        );
        assert_eq!(
            call_wait(&json!({"method": "tools/call", "params": {"name": "runs.exec"}})),
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
        let now = 1_000_000_000_000;
        let offline = failure_for(
            RequestError::Offline,
            LinkState::NotConnected {
                since_ms: Some(now - 5 * 3_600_000),
            },
            now,
        );
        assert_eq!(offline.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(offline.error.code, "device not connected");
        assert_eq!(offline.error.retry_after_ms, None);

        let reconnecting = failure_for(RequestError::Offline, LinkState::Reconnecting, now);
        assert_eq!(reconnecting.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(reconnecting.error.code, "device reconnecting");
        assert_eq!(reconnecting.error.retry_after_ms, Some(1_000));

        let disconnected = failure_for(RequestError::Disconnected, LinkState::Reconnecting, now);
        assert_eq!(disconnected.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(disconnected.error.retry_after_ms, None);

        let busy = failure_for(RequestError::Busy, LinkState::Reconnecting, now);
        assert_eq!(busy.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(busy.error.code, "device busy");
        assert_eq!(busy.error.retry_after_ms, Some(250));

        let send_timeout = failure_for(RequestError::SendTimeout, LinkState::Reconnecting, now);
        assert_eq!(send_timeout.status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(send_timeout.error.retry_after_ms, None);
    }

    #[test]
    fn only_retryable_failures_carry_a_retry_after_header() {
        let now = 1_000_000_000_000;
        for error in [RequestError::Offline, RequestError::Busy] {
            let response = failure_for(error, LinkState::Reconnecting, now).into_response();
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
            let response = failure_for(error, LinkState::Reconnecting, now).into_response();
            assert!(
                !response.headers().contains_key(header::RETRY_AFTER),
                "an unknown or timed-out outcome must not invite a blind retry"
            );
        }
        // A long silence has no retry promise: the age is the answer, not a delay.
        let offline = failure_for(
            RequestError::Offline,
            LinkState::NotConnected {
                since_ms: Some(now - 5 * 3_600_000),
            },
            now,
        );
        assert!(!offline
            .into_response()
            .headers()
            .contains_key(header::RETRY_AFTER));
    }

    #[test]
    fn the_link_sentence_has_one_state_and_the_age_of_the_last_frame() {
        let now = 1_000_000_000_000;
        let reconnecting = LinkRefusal::from_state(LinkState::Reconnecting, now);
        assert_eq!(
            reconnecting.message,
            "this machine is reconnecting; try again in a moment"
        );
        assert_eq!(reconnecting.detail, "device reconnecting");

        let away = LinkRefusal::from_state(
            LinkState::NotConnected {
                since_ms: Some(now - ((5 * 60 + 21) * 60_000)),
            },
            now,
        );
        assert_eq!(
            away.message,
            "this machine is not connected since 5h 21m ago; connect it, then try again"
        );
        assert_eq!(away.detail, "device not connected");
        assert_eq!(away.retry_after_ms, None);

        let never = LinkRefusal::from_state(LinkState::NotConnected { since_ms: None }, now);
        assert_eq!(
            never.message,
            "this machine is not connected; the console has not heard from it since it started. \
             Connect it, then try again"
        );
    }

    #[test]
    fn an_age_is_written_in_the_fleet_units() {
        assert_eq!(age_sentence(15_000), "15s");
        assert_eq!(age_sentence(7 * 60_000), "7m");
        assert_eq!(age_sentence((60 + 49) * 60_000), "1h 49m");
        assert_eq!(age_sentence(2 * 3_600_000), "2h");
        assert_eq!(age_sentence((24 + 5) * 3_600_000), "1d 5h");
    }

    fn device(id: &str, name: Option<&str>) -> DeviceRef {
        DeviceRef {
            id: id.to_owned(),
            name: name.map(str::to_owned),
        }
    }

    #[test]
    fn a_device_name_resolves_only_within_the_scope() {
        let fleet = vec![
            device("a", Some("ALPHA")),
            device("b", Some("BETA")),
            device("c", Some("GAMMA")),
        ];
        let allowed = vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            resolve_aggregate_device(&allowed, &fleet[..2], &fleet, "b"),
            DeviceResolution::Resolved("b".to_owned())
        );
        assert_eq!(
            resolve_aggregate_device(&allowed, &fleet[..2], &fleet, "BETA"),
            DeviceResolution::Resolved("b".to_owned())
        );
        assert_eq!(
            resolve_aggregate_device(&allowed, &fleet[..2], &fleet, "GAMMA"),
            DeviceResolution::OutOfScope
        );
        assert_eq!(
            resolve_aggregate_device(&allowed, &fleet[..2], &fleet, "nope"),
            DeviceResolution::Unknown
        );
    }

    #[test]
    fn an_unknown_machine_is_not_a_scope_refusal() {
        let fleet = vec![device("a", Some("ALPHA"))];
        assert_eq!(
            resolve_aggregate_device(&["a".into()], &fleet, &fleet, "deadbeef"),
            DeviceResolution::Unknown
        );
        // A machine that exists outside the token is the scope's refusal, not a typo.
        let fleet = vec![device("a", Some("ALPHA")), device("b", Some("BETA"))];
        assert_eq!(
            resolve_aggregate_device(&["a".into()], &fleet[..1], &fleet, "BETA"),
            DeviceResolution::OutOfScope
        );
    }

    #[test]
    fn a_shared_device_name_is_ambiguous() {
        let fleet = vec![device("a", Some("SAME")), device("b", Some("SAME"))];
        assert_eq!(
            resolve_aggregate_device(&["a".into(), "b".into()], &fleet, &fleet, "SAME"),
            DeviceResolution::Ambiguous
        );
        // The same name outside the scope is not a match at all.
        assert_eq!(
            resolve_aggregate_device(&["a".into()], &fleet[..1], &fleet, "SAME"),
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
        // The wire shape: the tool result is inside the device's JSON-RPC response.
        let result = |text: &str| {
            Reply::Result(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {"content": [{"type": "text", "text": text}]},
            }))
        };
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
        // The bare result is still read, for a forwarding path that does not wrap it.
        assert_eq!(
            run_detail(&Reply::Result(json!({
                "content": [{"type": "text", "text": r#"{"id":"r-4","exit_code":0}"#}],
            })))
            .as_deref(),
            Some("run r-4 exited 0")
        );
        // An error keeps the existing sentence; a reply that is not a run stays detail-less.
        assert_eq!(
            run_detail(&Reply::Error(json!({"error": {"code": -32603}}))).as_deref(),
            Some("jsonrpc error -32603")
        );
        assert_eq!(run_detail(&result("not json")), None);
    }

    #[test]
    fn a_tool_result_that_failed_is_read_from_iserror_or_the_run_exit() {
        let tool = |text: &str, is_error: bool| {
            Reply::Result(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "content": [{ "type": "text", "text": text }],
                    "isError": is_error,
                },
            }))
        };
        // MCP's own marker: the device says the tool refused, and its text is the reason.
        assert_eq!(
            tool_failure(
                &tool("runs.exec: program not found", true),
                Some("runs.exec")
            )
            .as_deref(),
            Some("runs.exec: program not found")
        );
        assert_eq!(
            tool_failure(&tool("files: no such file", true), Some("files.read")).as_deref(),
            Some("files: no such file")
        );
        // A run's exit code is the fact an older device still answers with.
        assert_eq!(
            tool_failure(
                &tool(
                    r#"{"id":"r-2","exit_code":2,"timed_out":false,"duration_ms":7}"#,
                    false
                ),
                Some("runs.exec")
            )
            .as_deref(),
            Some("run r-2 exited 2 in 7 ms")
        );
        assert_eq!(
            tool_failure(
                &tool(
                    r#"{"id":"r-3","exit_code":null,"timed_out":true,"duration_ms":10}"#,
                    false
                ),
                Some("runs.exec")
            )
            .as_deref(),
            Some("run r-3 timed out after 10 ms")
        );
        // Zero is the only success, and a result that is not a tool failure stays detail-less.
        assert_eq!(
            tool_failure(
                &tool(r#"{"id":"r-1","exit_code":0,"timed_out":false}"#, false),
                Some("runs.exec")
            ),
            None
        );
        assert_eq!(
            tool_failure(&tool(r#"{"content":"hi"}"#, false), Some("files.read")),
            None
        );
        // A reason is bounded like every other detail: the audit never carries a body.
        let long = "x".repeat(200);
        assert_eq!(
            tool_failure(&tool(&long, true), Some("files.read"))
                .expect("a failure")
                .chars()
                .count(),
            64
        );
    }

    /// **THE FALLBACK THE FLEET NEEDS.** A device that predates `isError` refuses a `tools/call`
    /// with a JSON-RPC error envelope. That is a refusal, not a green answer; its message is the
    /// reason and its `data` is not the audit's. A protocol error on ping/initialize/tools/list is
    /// not a tool call at all, so it is not a tool refusal.
    #[test]
    fn a_tool_call_error_envelope_is_a_refusal_with_its_bounded_message() {
        let envelope = |message: &str| {
            Reply::Error(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": { "code": -32603, "message": message, "data": {"detail": "SECRET DATA"} },
            }))
        };
        assert_eq!(
            tool_failure(&envelope("runs.exec: program not found"), Some("runs.exec")).as_deref(),
            Some("runs.exec: program not found")
        );
        assert_eq!(
            tool_failure(&envelope("files: no such file"), Some("files.read")).as_deref(),
            Some("files: no such file")
        );
        // Not a tool call: ping, initialize and tools/list keep their own 200 answer.
        assert_eq!(
            tool_failure(&envelope("runs.exec: program not found"), None),
            None
        );
        // The message is bounded exactly like a tool result's text.
        assert_eq!(
            tool_failure(&envelope(&"y".repeat(200)), Some("files.read"))
                .expect("a refusal")
                .chars()
                .count(),
            64
        );
        // A message-less envelope still names a refusal rather than returning None.
        assert_eq!(
            tool_failure(
                &Reply::Error(json!({"error": {"code": -32603}})),
                Some("files.read")
            )
            .as_deref(),
            Some("jsonrpc error -32603")
        );
    }

    #[test]
    fn a_run_carries_its_exit_facts_and_output_tail_as_evidence() {
        let result = |text: &str| {
            Reply::Result(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {"content": [{"type": "text", "text": text}]},
            }))
        };
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

    // ---- the release installer -----------------------------------------------------------------

    /// A uniquely named directory under the system temp directory, removed when the test ends.
    struct InstallerDir(PathBuf);

    impl InstallerDir {
        fn new(label: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "summrise-console-installer-{}-{}-{label}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the installer fixture directory is creatable");
            Self(path)
        }

        /// One file in `<data>/installer` with an explicit modification time, so "newest" is the
        /// test's fact and not the filesystem's clock.
        fn put(&self, name: &str, bytes: &[u8], modified_s: u64) {
            let directory = self.0.join(INSTALLER_DIR);
            std::fs::create_dir_all(&directory).expect("the installer directory is creatable");
            let path = directory.join(name);
            std::fs::write(&path, bytes).expect("the installer fixture is written");
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .expect("the installer fixture opens");
            file.set_times(
                std::fs::FileTimes::new()
                    .set_modified(UNIX_EPOCH + Duration::from_secs(modified_s)),
            )
            .expect("the fixture's mtime is settable");
        }
    }

    impl Drop for InstallerDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The version is the filename's middle: the prefix and suffix are the convention, the middle
    /// is the release, and a name that is not the convention carries no version to report.
    #[test]
    fn an_installer_name_carries_its_version_and_nothing_else() {
        assert_eq!(
            installer_version("SummriseSetup-0.3.280.exe"),
            Some("0.3.280")
        );
        assert_eq!(
            installer_version("SummriseSetup-0.3.273-ux43.exe"),
            Some("0.3.273-ux43")
        );
        assert_eq!(installer_version("SummriseSetup-.exe"), None);
        assert_eq!(installer_version("summrisesetup-0.3.280.exe"), None);
        assert_eq!(installer_version("SummriseSetup-0.3.280.exe.tmp"), None);
        assert_eq!(installer_version("other-0.3.280.exe"), None);
    }

    /// The selection is one rule both routes share: the newest mtime wins, a tie is broken by the
    /// filename so two reads cannot disagree, and only a regular file whose name is the convention
    /// is a candidate.
    #[test]
    fn the_selection_is_the_newest_installer_by_mtime() {
        let dir = InstallerDir::new("selection");
        assert!(newest_installer_up_to(&dir.0, MAX_INSTALLER_BYTES).is_none());

        dir.put("notes.txt", b"not an installer", 5_000);
        assert!(
            newest_installer_up_to(&dir.0, MAX_INSTALLER_BYTES).is_none(),
            "an unrelated file is not the release's installer"
        );

        dir.put("SummriseSetup-9.9.9.exe", b"the older release", 1_000);
        dir.put("SummriseSetup-0.0.1.exe", b"the newer release", 2_000);
        let chosen =
            newest_installer_up_to(&dir.0, MAX_INSTALLER_BYTES).expect("one installer is staged");
        assert_eq!(chosen.filename, "SummriseSetup-0.0.1.exe");
        assert_eq!(chosen.version, "0.0.1");
        assert_eq!(chosen.size_bytes, b"the newer release".len() as u64);

        // The same moment: the greater filename wins, deterministically.
        dir.put("SummriseSetup-0.0.2.exe", b"the tie", 2_000);
        let tied = newest_installer_up_to(&dir.0, MAX_INSTALLER_BYTES).expect("one installer");
        assert_eq!(tied.filename, "SummriseSetup-0.0.2.exe");
    }

    /// A directory named like an installer is not one: the rule serves a release file, not whatever
    /// the data directory happens to hold under the right name.
    #[test]
    fn a_directory_named_like_an_installer_is_not_one() {
        let dir = InstallerDir::new("directory");
        let installers = dir.0.join(INSTALLER_DIR);
        std::fs::create_dir_all(&installers).expect("the installer directory is creatable");
        std::fs::create_dir(installers.join("SummriseSetup-9.9.9.exe"))
            .expect("the fixture directory is creatable");
        assert!(newest_installer_up_to(&dir.0, MAX_INSTALLER_BYTES).is_none());
    }

    /// The cap is a parameter so a test can reach it with six bytes rather than a 128 MiB fixture;
    /// the newest file over it is not served, the older file under it is, and with nothing under it
    /// there is no installer. The production cap is the ruling's 128 MiB.
    #[test]
    fn a_candidate_over_the_cap_is_not_served() {
        let dir = InstallerDir::new("cap");
        dir.put("SummriseSetup-9.9.9.exe", b"sixteen bytes...", 2_000);
        dir.put("SummriseSetup-0.0.1.exe", b"older", 1_000);

        let chosen =
            newest_installer_up_to(&dir.0, 5).expect("the older file is under the five-byte cap");
        assert_eq!(chosen.filename, "SummriseSetup-0.0.1.exe");
        assert!(
            newest_installer_up_to(&dir.0, 4).is_none(),
            "with every candidate over the cap there is no installer"
        );
        assert_eq!(MAX_INSTALLER_BYTES, 128 * 1024 * 1024);
    }
}
