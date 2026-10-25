//! The rendezvous broker: an address book and a doorbell over HTTP/JSON.
//!
//! The broker knows four verbs:
//!
//! - `POST /register` — upsert one [`Registration`] by `peer_id`, after
//!   verifying its signature. A forged entry is refused; it never replaces
//!   the good one.
//! - `GET /lookup/:peer_id?now_ms=…` — the peer's fresh candidates and the
//!   registration's `issued_at_ms`.
//! - `POST /request/:target` — leave one signed [`ConnectRequest`] for
//!   `target`, after verifying the requester's signature and that `target`
//!   has registered. A target that never registered is not awaiting, so it
//!   is a 404.
//! - `GET /requests/:target?now_ms=…` — hand the target its unexpired
//!   connect requests **once**. A request past its `ttl_ms` is dropped; a
//!   served request is removed before it is answered.
//!
//! `now_ms` is a query parameter and is still REQUIRED, but the broker does
//! not use it: freshness is the broker's own clock, measured from the moment
//! the broker RECEIVED a registration. It was the caller's clock until
//! 2026-10-25, and that was a defect rather than a test convenience: any
//! unauthenticated `GET /lookup/:id?now_ms=<far future>` swept every other
//! registration out of the table, and a device with a clock five minutes fast
//! did the same by accident. The signed `ttl_ms` still travels and still
//! bounds an entry — from receipt, not from the peer's own stamp — and a
//! registration whose `ttl_ms` exceeds [`MAX_REGISTRATION_TTL_MS`] is refused,
//! because a peer may sign any number it likes and one entry must not hold a
//! slot for a year.
//!
//! **THE REQUEST INBOX IS METADATA ONLY.** A [`ConnectRequest`] holds a
//! requester id, a target id and a relay session id — the three names two
//! peers need in order to meet — plus a signature and its freshness stamps.
//! It has no payload. The broker never stores a frame, a request body meant
//! for a peer, a response, or a tool call, and no route here can return one;
//! a path the router does not know is a 404 from axum itself. When the mesh
//! carries traffic it goes peer-to-peer over QUIC, or through the phase-2b
//! relay as an opaque stream — never through this table. The doc comment is
//! the promise; the `the_broker_has_no_tool_route` test is the proof.
//!
//! TLS is not here on purpose. This broker is the local, phase-2 rendezvous;
//! the VPS relay/broker and its certificate are phase 2b.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::rendezvous::{
    ConnectRequest, LookupResponse, Registration, RendezvousError, RequestsResponse,
};

/// The largest registration or connect-request body the broker will parse,
/// in bytes.
///
/// A registration is a handful of addresses and a signature, and a connect
/// request is three names and two stamps; 64 KiB is already far more than an
/// honest peer sends, and the cap means a client cannot make the broker buffer
/// an arbitrary body before the JSON parser rejects it.
pub const MAX_REGISTRATION_BYTES: usize = 64 * 1024;

/// The most registrations one broker holds, by default.
///
/// A registration is a peer id, a short candidate list, two stamps and a
/// signature — a few hundred bytes of heap each — so this is a memory bound
/// and an admission policy at once. A broker at the cap answers a *new* peer
/// with `429 table_full`; an existing peer can still refresh its own entry,
/// because an upsert does not grow the table. 4096 peers is far more than one
/// phase-2 deployment, while keeping the table small enough that the sweep on
/// every insert and lookup is cheap.
pub const MAX_REGISTRATIONS: usize = 4096;

/// The longest `ttl_ms` a registration may ask for, in milliseconds.
///
/// Five minutes is ten times the default refresh interval (10 s) and ten times
/// the default ttl (30 s): long enough for a slow or briefly disconnected peer
/// to keep its slot, short enough that a signed `u64::MAX` cannot. A peer may
/// sign any ttl it likes; this is the broker's bound on what it will STORE.
pub const MAX_REGISTRATION_TTL_MS: u64 = 5 * 60 * 1000;

/// One stored registration and the broker's receipt time.
///
/// **THE RECEIPT TIME IS THE BROKER'S, NEVER THE PEER'S.** Expiry measures the
/// signed `ttl_ms` from the moment the broker accepted the entry, so a
/// registration dated in the year 3000 cannot hold a slot for longer than any
/// other — and the peer's `issued_at_ms` still travels (it is signed, and a
/// lookup returns it) without being trusted for freshness.
struct Entry {
    registration: Registration,
    received_at_ms: u64,
}

impl Entry {
    /// Whether this entry is past its signed ttl, measured from receipt.
    fn is_stale(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.received_at_ms) > self.registration.ttl_ms
    }
}

/// The broker's clock: milliseconds since the epoch, read only by the broker.
type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

fn system_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// The broker's table: one registration per peer id, with its receipt time.
type Table = Arc<Mutex<HashMap<String, Entry>>>;

/// The broker's connect-request inbox: the requests waiting for each target.
///
/// A queue holds the values as they arrived, each with the broker's RECEIPT time — the same rule the
/// registration table uses (see the module docs): a requester's clock decides nothing, so a peer whose
/// clock is wrong can still leave a note, and one whose clock is far ahead cannot make it live longer.
/// [`ConnectRequest`] deliberately has nowhere to put a payload.
type Inbox = Arc<Mutex<HashMap<String, Vec<QueuedRequest>>>>;

/// One waiting connect request and when the broker accepted it.
struct QueuedRequest {
    request: ConnectRequest,
    received_at_ms: u64,
}

impl QueuedRequest {
    /// Whether this note is past its signed ttl, measured from receipt.
    fn is_expired(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.received_at_ms) > self.request.ttl_ms
    }
}

/// A local rendezvous broker.
///
/// Cloning shares the table and the inbox: every router made from a clone sees
/// the same registrations and the same pending requests.
#[derive(Clone)]
pub struct Broker {
    table: Table,
    inbox: Inbox,
    /// The registration cap this broker enforces; [`MAX_REGISTRATIONS`]
    /// unless [`Broker::with_capacity`] chose another.
    max_registrations: usize,
    /// The broker's OWN clock. Never the caller's: see the module docs.
    clock: Clock,
}

impl Default for Broker {
    fn default() -> Self {
        Self {
            table: Table::default(),
            inbox: Inbox::default(),
            max_registrations: MAX_REGISTRATIONS,
            clock: Arc::new(system_now_ms),
        }
    }
}

impl std::fmt::Debug for Broker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Broker").finish_non_exhaustive()
    }
}

impl Broker {
    /// An empty broker with the default cap.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty broker whose clock is `clock`.
    ///
    /// The seam a test moves time with, and the reason no test has to sleep for
    /// a ttl to elapse. Production uses [`system_now_ms`].
    pub fn with_clock(clock: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        Self {
            clock: Arc::new(clock),
            ..Self::default()
        }
    }

    /// The broker's current time, in milliseconds since the epoch.
    fn now(&self) -> u64 {
        (self.clock)()
    }

    /// An empty broker that admits at most `max_registrations` peers.
    ///
    /// The cap is a deployment knob and a test seam: a test does not need to
    /// generate thousands of identities to prove the refusal.
    pub fn with_capacity(max_registrations: usize) -> Self {
        Self {
            max_registrations,
            ..Self::default()
        }
    }

    /// The axum router, ready for `axum::serve` or `tower::ServiceExt`.
    pub fn router(&self) -> Router {
        Router::new()
            .route("/register", post(register))
            .route("/lookup/:peer_id", get(lookup))
            .route("/request/:target", post(leave_request))
            .route("/requests/:target", get(take_requests))
            .layer(DefaultBodyLimit::max(MAX_REGISTRATION_BYTES))
            .with_state(self.clone())
    }

    /// Serve until the listener fails.
    pub async fn serve(self, listener: TcpListener) -> std::io::Result<()> {
        axum::serve(listener, self.router()).await
    }
}

/// The shape of every error this API returns, matching the device API's body.
#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
    detail: String,
}

type ApiError = (StatusCode, Json<ErrorBody>);

fn api_error(status: StatusCode, error: &'static str, detail: impl Into<String>) -> ApiError {
    (
        status,
        Json(ErrorBody {
            error,
            detail: detail.into(),
        }),
    )
}

fn not_found(detail: impl Into<String>) -> ApiError {
    api_error(StatusCode::NOT_FOUND, "unknown_peer", detail)
}

fn table_full(limit: usize) -> ApiError {
    api_error(
        StatusCode::TOO_MANY_REQUESTS,
        "table_full",
        format!("the broker already holds its maximum of {limit} registrations"),
    )
}

/// Whether `peer_id` is exactly one lowercase-hex Ed25519 public key.
///
/// The broker indexes by the string, so this is what makes the key canonical:
/// a case or whitespace variant is refused at the door instead of becoming a
/// second, unreachable entry for the same key. 64 characters is the hex
/// spelling of 32 bytes.
fn is_canonical_peer_id(peer_id: &str) -> bool {
    peer_id.len() == 64
        && peer_id
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn malformed_peer_id(peer_id: &str) -> ApiError {
    api_error(
        StatusCode::BAD_REQUEST,
        "malformed_peer_id",
        format!("{peer_id:?} is not a peer id; it must be exactly 64 lowercase hex characters"),
    )
}

fn poisoned_table() -> ApiError {
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "broker_state",
        "the broker's registration table is poisoned",
    )
}

fn poisoned_inbox() -> ApiError {
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "broker_state",
        "the broker's connect-request inbox is poisoned",
    )
}

/// `POST /register` — verify one registration and upsert it by peer id.
async fn register(
    State(broker): State<Broker>,
    body: Result<Json<Registration>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(registration) = body.map_err(|rejection| {
        api_error(
            StatusCode::BAD_REQUEST,
            "malformed_body",
            rejection.body_text(),
        )
    })?;
    if !is_canonical_peer_id(&registration.peer_id) {
        return Err(malformed_peer_id(&registration.peer_id));
    }
    registration.verify_self().map_err(|error| match error {
        RendezvousError::BadSignature => api_error(
            StatusCode::FORBIDDEN,
            "bad_signature",
            "the registration's signature does not verify",
        ),
        other => api_error(
            StatusCode::BAD_REQUEST,
            "malformed_registration",
            other.to_string(),
        ),
    })?;

    // THE PEER CHOOSES ITS TTL IN THE SIGNED BYTES, so the broker is the one that must refuse an
    // unreasonable one: a `u64::MAX` would hold a slot against every other peer for ever.
    if registration.ttl_ms > MAX_REGISTRATION_TTL_MS {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "ttl_too_long",
            format!(
                "ttl_ms {} exceeds the broker's maximum of {MAX_REGISTRATION_TTL_MS} ms",
                registration.ttl_ms
            ),
        ));
    }

    let mut table = broker.table.lock().map_err(|_| poisoned_table())?;
    // FRESHNESS IS THE BROKER'S. This used to sweep with the registering peer's own stamp, so a peer
    // whose clock was in the future evicted live entries; the receipt time removes the peer from the
    // question entirely.
    let now_ms = broker.now();
    table.retain(|_, entry| !entry.is_stale(now_ms));
    if !table.contains_key(&registration.peer_id) && table.len() >= broker.max_registrations {
        return Err(table_full(broker.max_registrations));
    }
    table.insert(
        registration.peer_id.clone(),
        Entry {
            registration,
            received_at_ms: now_ms,
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

/// The clock the caller supplies. **IT IS REQUIRED BY THE WIRE AND IGNORED BY THE BROKER** — the name
/// keeps the parameter in the URL, and the leading underscore says the value is deliberately unused;
/// freshness comes from the broker's own clock (see the module docs).
#[derive(Debug, Deserialize)]
struct ClockQuery {
    #[serde(rename = "now_ms")]
    _now_ms: u64,
}

/// `GET /lookup/:peer_id?now_ms=…` — the fresh candidates and their stamp.
async fn lookup(
    State(broker): State<Broker>,
    Path(peer_id): Path<String>,
    query: Result<Query<ClockQuery>, QueryRejection>,
) -> Result<Json<LookupResponse>, ApiError> {
    let Query(_query) = query.map_err(|rejection| {
        api_error(
            StatusCode::BAD_REQUEST,
            "malformed_query",
            rejection.body_text(),
        )
    })?;

    if !is_canonical_peer_id(&peer_id) {
        return Err(malformed_peer_id(&peer_id));
    }
    let mut table = broker.table.lock().map_err(|_| poisoned_table())?;
    // A lookup is where a stale entry would otherwise be hidden forever. The sweep uses the broker's
    // clock; the caller's `now_ms` cannot make another peer's registration disappear.
    table.retain(|_, entry| !entry.is_stale(broker.now()));
    let Some(entry) = table.get(&peer_id) else {
        return Err(not_found(format!("no registration for {peer_id}")));
    };
    Ok(Json(LookupResponse {
        issued_at_ms: entry.registration.issued_at_ms,
        candidates: entry.registration.candidates.clone(),
    }))
}

/// `POST /request/:target` — verify a connect request and leave it for its
/// target.
///
/// A request that names a target which never registered is a 404: there is no
/// serving session to hand it to, and the requester should treat the peer as
/// absent. The body's `target_id` must agree with the path, so one note cannot
/// be addressed to two peers.
async fn leave_request(
    State(broker): State<Broker>,
    Path(target): Path<String>,
    body: Result<Json<ConnectRequest>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(request) = body.map_err(|rejection| {
        api_error(
            StatusCode::BAD_REQUEST,
            "malformed_body",
            rejection.body_text(),
        )
    })?;
    if !is_canonical_peer_id(&target) {
        return Err(malformed_peer_id(&target));
    }
    request.verify_self().map_err(|error| match error {
        RendezvousError::BadSignature => api_error(
            StatusCode::FORBIDDEN,
            "bad_signature",
            "the connect request's signature does not verify",
        ),
        other => api_error(
            StatusCode::BAD_REQUEST,
            "malformed_request",
            other.to_string(),
        ),
    })?;
    if request.target_id != target {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "malformed_request",
            "the connect request's target_id does not match the path",
        ));
    }

    let now_ms = broker.now();
    {
        let mut table = broker.table.lock().map_err(|_| poisoned_table())?;
        table.retain(|_, entry| !entry.is_stale(now_ms));
        if !table.contains_key(&target) {
            return Err(not_found(format!(
                "no registration for {target}; a peer that never registered is not awaiting a connect"
            )));
        }
    }

    let mut inbox = broker.inbox.lock().map_err(|_| poisoned_inbox())?;
    inbox.entry(target).or_default().push(QueuedRequest {
        request,
        received_at_ms: now_ms,
    });
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /requests/:target?now_ms=…` — hand the target its pending requests
/// once.
///
/// The queue is removed before it is filtered and answered, so two pollers
/// cannot both see the same note. Requests past their ttl are dropped rather
/// than returned; a registered peer with nothing waiting gets an empty list,
/// which is not an error.
async fn take_requests(
    State(broker): State<Broker>,
    Path(target): Path<String>,
    query: Result<Query<ClockQuery>, QueryRejection>,
) -> Result<Json<RequestsResponse>, ApiError> {
    let Query(_query) = query.map_err(|rejection| {
        api_error(
            StatusCode::BAD_REQUEST,
            "malformed_query",
            rejection.body_text(),
        )
    })?;

    if !is_canonical_peer_id(&target) {
        return Err(malformed_peer_id(&target));
    }
    let now_ms = broker.now();
    {
        let mut table = broker.table.lock().map_err(|_| poisoned_table())?;
        table.retain(|_, entry| !entry.is_stale(now_ms));
        if !table.contains_key(&target) {
            return Err(not_found(format!("no registration for {target}")));
        }
    }

    let mut inbox = broker.inbox.lock().map_err(|_| poisoned_inbox())?;
    let waiting = inbox.remove(&target).unwrap_or_default();
    let requests = waiting
        .into_iter()
        .filter(|queued| !queued.is_expired(now_ms))
        .map(|queued| queued.request)
        .collect();
    Ok(Json(RequestsResponse { requests }))
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::{to_bytes, Body};
    use axum::http::{header, Request};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use crate::keys::Identity;
    use crate::rendezvous::{Candidate, CandidateKind};

    const ISSUED: u64 = 1_000;
    const TTL: u64 = 30_000;

    fn app() -> Router {
        Broker::new().router()
    }

    /// A broker whose clock a test moves by hand.
    ///
    /// `now_ms` used to do this on the wire; the broker now owns the clock, so a test owns the
    /// BROKER's clock instead — and no test ever sleeps for a ttl to elapse.
    fn clocked(now: u64) -> (Broker, Arc<std::sync::atomic::AtomicU64>) {
        let cell = Arc::new(std::sync::atomic::AtomicU64::new(now));
        let reader = Arc::clone(&cell);
        (
            Broker::with_clock(move || reader.load(std::sync::atomic::Ordering::SeqCst)),
            cell,
        )
    }

    fn candidate(kind: CandidateKind, addr: &str) -> Candidate {
        Candidate::new(kind, addr, 900)
    }

    fn register_request(registration: &Registration) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/register")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(registration).expect("a registration is JSON"),
            ))
            .expect("the request builds")
    }

    fn register_raw(registration: &Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/register")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(registration.to_string()))
            .expect("the request builds")
    }

    fn lookup_request(peer_id: &str, now_ms: u64) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(format!("/lookup/{peer_id}?now_ms={now_ms}"))
            .body(Body::empty())
            .expect("the request builds")
    }

    fn lookup_without_clock(peer_id: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(format!("/lookup/{peer_id}"))
            .body(Body::empty())
            .expect("the request builds")
    }

    fn leave_request_request(request: &ConnectRequest) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(format!("/request/{}", request.target_id))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(request).expect("a connect request is JSON"),
            ))
            .expect("the request builds")
    }

    fn take_requests_request(target: &str, now_ms: u64) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(format!("/requests/{target}?now_ms={now_ms}"))
            .body(Body::empty())
            .expect("the request builds")
    }

    async fn leave_request(app: &Router, request: &ConnectRequest) -> (StatusCode, Value) {
        send(app.clone(), leave_request_request(request)).await
    }

    async fn take_requests(app: &Router, target: &str, now_ms: u64) -> (StatusCode, Value) {
        send(app.clone(), take_requests_request(target, now_ms)).await
    }

    async fn register_empty(app: &Router, identity: &Identity) -> String {
        let peer_id = identity.public_hex();
        let registration = Registration::sign(identity, Vec::new(), ISSUED, TTL);
        assert_eq!(register(app, &registration).await.0, StatusCode::NO_CONTENT);
        peer_id
    }

    async fn send(app: Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = app.oneshot(request).await.expect("the router answers");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the body is readable");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("the body is JSON")
        };
        (status, value)
    }

    async fn register(app: &Router, registration: &Registration) -> (StatusCode, Value) {
        send(app.clone(), register_request(registration)).await
    }

    async fn lookup(app: &Router, peer_id: &str, now_ms: u64) -> (StatusCode, Value) {
        send(app.clone(), lookup_request(peer_id, now_ms)).await
    }

    #[tokio::test]
    async fn a_registration_round_trips_the_candidates_in_order() {
        let app = app();
        let identity = Identity::generate();
        let candidates = vec![
            candidate(CandidateKind::Udp, "10.0.0.1:5000"),
            candidate(CandidateKind::Tcp, "10.0.0.1:5001"),
        ];
        let registration = Registration::sign(&identity, candidates, ISSUED, TTL);

        let (status, _) = register(&app, &registration).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, body) = lookup(&app, &identity.public_hex(), ISSUED).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            json!({
                "issued_at_ms": ISSUED,
                "candidates": [
                    { "kind": "udp", "addr": "10.0.0.1:5000", "seen_at_ms": 900 },
                    { "kind": "tcp", "addr": "10.0.0.1:5001", "seen_at_ms": 900 }
                ]
            }),
            "the candidates come back in the order they were published"
        );
    }

    #[tokio::test]
    async fn a_registration_is_upserted_not_appended() {
        let app = app();
        let identity = Identity::generate();
        let peer_id = identity.public_hex();

        let first = Registration::sign(
            &identity,
            vec![candidate(CandidateKind::Udp, "10.0.0.1:5000")],
            ISSUED,
            TTL,
        );
        let second = Registration::sign(
            &identity,
            vec![candidate(CandidateKind::Udp, "10.0.0.2:6000")],
            ISSUED + 10,
            TTL,
        );
        assert_eq!(register(&app, &first).await.0, StatusCode::NO_CONTENT);
        assert_eq!(register(&app, &second).await.0, StatusCode::NO_CONTENT);

        let (status, body) = lookup(&app, &peer_id, ISSUED + 10).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["candidates"],
            json!([{ "kind": "udp", "addr": "10.0.0.2:6000", "seen_at_ms": 900 }])
        );
        assert_eq!(body["issued_at_ms"], json!(ISSUED + 10));
    }

    #[tokio::test]
    async fn a_registration_past_its_ttl_is_not_returned() {
        let (broker, clock) = clocked(ISSUED);
        let app = broker.router();
        let identity = Identity::generate();
        let registration = Registration::sign(
            &identity,
            vec![candidate(CandidateKind::Udp, "10.0.0.1:5000")],
            ISSUED,
            500,
        );
        assert_eq!(
            register(&app, &registration).await.0,
            StatusCode::NO_CONTENT
        );

        clock.store(ISSUED + 500, std::sync::atomic::Ordering::SeqCst);
        let (status, _) = lookup(&app, &identity.public_hex(), ISSUED + 500).await;
        assert_eq!(status, StatusCode::OK, "exactly at the ttl is still fresh");

        clock.store(ISSUED + 501, std::sync::atomic::Ordering::SeqCst);
        let (status, body) = lookup(&app, &identity.public_hex(), ISSUED + 501).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], json!("unknown_peer"));
    }

    /// **THE DEFECT THIS FIXES, AS A TEST.** The sweep used to run on the CALLER's `now_ms`, so one
    /// unauthenticated `GET /lookup/:id?now_ms=<an hour ahead>` deleted every other registration — and a
    /// device whose clock was merely fast did it by accident, once a minute. Freshness is the broker's
    /// clock now; the caller's number is still required by the wire and is not read.
    #[tokio::test]
    async fn a_lookup_from_a_clock_far_ahead_cannot_evict_a_live_registration() {
        let (broker, clock) = clocked(ISSUED);
        let app = broker.router();
        let identity = Identity::generate();
        let registration = Registration::sign(
            &identity,
            vec![candidate(CandidateKind::Udp, "10.0.0.1:5000")],
            ISSUED,
            TTL,
        );
        assert_eq!(
            register(&app, &registration).await.0,
            StatusCode::NO_CONTENT
        );

        let (status, _) = lookup(&app, &identity.public_hex(), ISSUED + 3_600_000).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "a caller's clock cannot age another peer's registration"
        );
        let (status, body) = lookup(&app, &identity.public_hex(), ISSUED).await;
        assert_eq!(status, StatusCode::OK, "and the entry is still there");
        assert_eq!(
            body["candidates"].as_array().map(Vec::len),
            Some(1),
            "with what the peer published: {body}"
        );
        assert_eq!(
            clock.load(std::sync::atomic::Ordering::SeqCst),
            ISSUED,
            "the broker never advanced its clock, because it never read the caller's"
        );
    }

    /// A peer signs its own `ttl_ms`, so the broker is the one that must bound it: `u64::MAX` would hold
    /// one of the 4096 slots for ever.
    #[tokio::test]
    async fn a_registration_ttl_above_the_ceiling_is_refused() {
        let app = app();
        let identity = Identity::generate();
        let registration =
            Registration::sign(&identity, Vec::new(), ISSUED, MAX_REGISTRATION_TTL_MS + 1);
        let (status, body) = register(&app, &registration).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], json!("ttl_too_long"));
        let (status, _) = lookup(&app, &identity.public_hex(), ISSUED).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a refused registration must not occupy a slot"
        );
    }

    /// **A STAMP IN THE FUTURE IS NOT A LONGER LIFE.** Expiry is measured from when the broker RECEIVED
    /// the entry, so a peer cannot pin a slot by signing a date in the year 3000.
    #[tokio::test]
    async fn a_registration_dated_in_the_future_expires_from_receipt() {
        let (broker, clock) = clocked(ISSUED);
        let app = broker.router();
        let identity = Identity::generate();
        let far_future = u64::MAX / 2;
        let registration = Registration::sign(&identity, Vec::new(), far_future, TTL);
        assert_eq!(
            register(&app, &registration).await.0,
            StatusCode::NO_CONTENT
        );

        clock.store(ISSUED + TTL + 1, std::sync::atomic::Ordering::SeqCst);
        let (status, _) = lookup(&app, &identity.public_hex(), ISSUED + TTL + 1).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "the signed stamp does not extend the life the broker granted"
        );
    }

    #[tokio::test]
    async fn a_bad_signature_is_403_and_does_not_overwrite_a_good_registration() {
        let app = app();
        let identity = Identity::generate();
        let peer_id = identity.public_hex();
        let good = Registration::sign(
            &identity,
            vec![candidate(CandidateKind::Udp, "10.0.0.1:5000")],
            ISSUED,
            TTL,
        );
        assert_eq!(register(&app, &good).await.0, StatusCode::NO_CONTENT);

        let mut forged = Registration::sign(
            &identity,
            vec![candidate(CandidateKind::Udp, "10.9.9.9:6666")],
            ISSUED + 10,
            TTL,
        );
        forged.sig[0] ^= 0x01;
        let (status, body) = register(&app, &forged).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], json!("bad_signature"));

        let (status, body) = lookup(&app, &peer_id, ISSUED).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["candidates"],
            json!([{ "kind": "udp", "addr": "10.0.0.1:5000", "seen_at_ms": 900 }]),
            "the refused registration must not replace the accepted one"
        );
    }

    #[tokio::test]
    async fn an_unknown_peer_is_not_found() {
        let app = app();
        let identity = Identity::generate();

        let (status, body) = lookup(&app, &identity.public_hex(), ISSUED).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], json!("unknown_peer"));
    }

    #[tokio::test]
    async fn two_peers_do_not_clobber_each_other() {
        let app = app();
        let first = Identity::generate();
        let second = Identity::generate();

        let first_registration = Registration::sign(
            &first,
            vec![candidate(CandidateKind::Udp, "10.0.0.1:5000")],
            ISSUED,
            TTL,
        );
        let second_registration = Registration::sign(
            &second,
            vec![candidate(CandidateKind::Tcp, "10.0.0.2:6000")],
            ISSUED,
            TTL,
        );
        assert_eq!(
            register(&app, &first_registration).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            register(&app, &second_registration).await.0,
            StatusCode::NO_CONTENT
        );

        let (_, first_body) = lookup(&app, &first.public_hex(), ISSUED).await;
        let (_, second_body) = lookup(&app, &second.public_hex(), ISSUED).await;
        assert_eq!(
            first_body["candidates"],
            json!([{ "kind": "udp", "addr": "10.0.0.1:5000", "seen_at_ms": 900 }])
        );
        assert_eq!(
            second_body["candidates"],
            json!([{ "kind": "tcp", "addr": "10.0.0.2:6000", "seen_at_ms": 900 }])
        );
    }

    #[tokio::test]
    async fn a_full_table_refuses_a_new_peer_but_an_existing_peer_can_refresh() {
        let broker = Broker::with_capacity(2);
        let app = broker.router();
        let first = Identity::generate();
        let second = Identity::generate();
        let third = Identity::generate();

        for identity in [&first, &second] {
            let registration = Registration::sign(identity, Vec::new(), ISSUED, TTL);
            assert_eq!(
                register(&app, &registration).await.0,
                StatusCode::NO_CONTENT
            );
        }

        let third_registration = Registration::sign(&third, Vec::new(), ISSUED, TTL);
        let (status, body) = register(&app, &third_registration).await;
        assert_eq!(
            status,
            StatusCode::TOO_MANY_REQUESTS,
            "the table's cap must refuse a new peer"
        );
        assert_eq!(body["error"], json!("table_full"));

        // An upsert of an admitted peer does not need a new slot, so a full
        // table can still be kept fresh.
        let refreshed = Registration::sign(&first, Vec::new(), ISSUED + 1, TTL);
        assert_eq!(
            register(&app, &refreshed).await.0,
            StatusCode::NO_CONTENT,
            "a full table must not refuse a refresh of an existing peer"
        );
        assert_eq!(
            broker.table.lock().expect("the table").len(),
            2,
            "the refused registration must not have grown the table"
        );
        let (status, _) = lookup(&app, &third.public_hex(), ISSUED).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "the refused peer is absent");
    }

    #[tokio::test]
    async fn an_expired_registration_is_swept_when_a_new_one_is_inserted() {
        let (broker, clock) = clocked(ISSUED);
        let app = broker.router();
        let expired = Identity::generate();
        let fresh = Identity::generate();

        let expired_registration = Registration::sign(&expired, Vec::new(), ISSUED, 500);
        assert_eq!(
            register(&app, &expired_registration).await.0,
            StatusCode::NO_CONTENT
        );
        // Five seconds after RECEIPT the first registration is far past its 500 ms ttl. The broker's
        // clock is what moves; the fresh peer's own stamp is deliberately later and irrelevant.
        clock.store(ISSUED + 5_000, std::sync::atomic::Ordering::SeqCst);
        let fresh_registration = Registration::sign(&fresh, Vec::new(), ISSUED + 5_000, TTL);
        assert_eq!(
            register(&app, &fresh_registration).await.0,
            StatusCode::NO_CONTENT
        );

        assert!(
            broker
                .table
                .lock()
                .expect("the table")
                .get(&expired.public_hex())
                .is_none(),
            "the sweep on insert must remove the expired entry, not hide it"
        );
        let (status, _) = lookup(&app, &expired.public_hex(), ISSUED + 5_000).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_expired_registration_is_swept_by_a_lookup() {
        let (broker, clock) = clocked(ISSUED);
        let app = broker.router();
        let expired = Identity::generate();
        let live = Identity::generate();

        let expired_registration = Registration::sign(&expired, Vec::new(), ISSUED, 100);
        assert_eq!(
            register(&app, &expired_registration).await.0,
            StatusCode::NO_CONTENT
        );
        let live_registration = Registration::sign(&live, Vec::new(), ISSUED, TTL);
        assert_eq!(
            register(&app, &live_registration).await.0,
            StatusCode::NO_CONTENT
        );

        clock.store(ISSUED + 101, std::sync::atomic::Ordering::SeqCst);
        let (status, _) = lookup(&app, &live.public_hex(), ISSUED + 101).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            broker
                .table
                .lock()
                .expect("the table")
                .get(&expired.public_hex())
                .is_none(),
            "a lookup must sweep what its clock says is expired"
        );
    }

    #[tokio::test]
    async fn a_peer_id_that_is_not_64_lowercase_hex_is_400() {
        let app = app();
        let identity = Identity::generate();
        let canonical = identity.public_hex();
        let uppercase = canonical.to_ascii_uppercase();
        let too_long = format!("{canonical}0");

        // The body is checked before its signature, so a case variant cannot
        // become a second table key under the same key.
        let mut registration = Registration::sign(&identity, Vec::new(), ISSUED, TTL);
        registration.peer_id = uppercase.clone();
        let (status, body) = register(&app, &registration).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "an uppercase peer id is malformed, not an unknown peer"
        );
        assert_eq!(body["error"], json!("malformed_peer_id"));

        for candidate in [
            uppercase.as_str(),
            &canonical[..63],
            too_long.as_str(),
            "not-a-key",
        ] {
            let (status, body) = lookup(&app, candidate, ISSUED).await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "{candidate:?} must be a malformed peer id"
            );
            assert_eq!(body["error"], json!("malformed_peer_id"));
        }

        let requester = Identity::generate();
        let request =
            ConnectRequest::sign(&requester, uppercase.clone(), "relay-session", ISSUED, TTL);
        assert_eq!(
            leave_request(&app, &request).await.0,
            StatusCode::BAD_REQUEST,
            "a connect request addressed to a non-canonical id is malformed"
        );
        assert_eq!(
            take_requests(&app, &uppercase, ISSUED).await.0,
            StatusCode::BAD_REQUEST,
            "an inbox poll for a non-canonical id is malformed"
        );
    }

    #[tokio::test]
    async fn a_malformed_body_is_400() {
        let app = app();

        let broken = Request::builder()
            .method("POST")
            .uri("/register")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{"))
            .expect("the request builds");
        assert_eq!(
            send(app.clone(), broken).await.0,
            StatusCode::BAD_REQUEST,
            "invalid JSON is a malformed body"
        );

        assert_eq!(
            send(app.clone(), register_raw(&json!({ "peer_id": "half" })))
                .await
                .0,
            StatusCode::BAD_REQUEST,
            "a body missing the registration's fields is malformed"
        );

        let identity = Identity::generate();
        let mut bad_peer_id = Registration::sign(
            &identity,
            vec![candidate(CandidateKind::Udp, "10.0.0.1:5000")],
            ISSUED,
            TTL,
        );
        bad_peer_id.peer_id = "not-a-key".to_owned();
        assert_eq!(
            register(&app, &bad_peer_id).await.0,
            StatusCode::BAD_REQUEST,
            "a peer id that is not a public key cannot be verified"
        );

        let mut bad_signature_length = Registration::sign(
            &identity,
            vec![candidate(CandidateKind::Udp, "10.0.0.1:5000")],
            ISSUED,
            TTL,
        );
        bad_signature_length.sig.truncate(63);
        assert_eq!(
            register(&app, &bad_signature_length).await.0,
            StatusCode::BAD_REQUEST,
            "a signature of the wrong length is malformed, not a valid signature"
        );
    }

    #[tokio::test]
    async fn a_lookup_without_a_clock_is_400() {
        let app = app();
        let identity = Identity::generate();

        let (status, _) = send(app, lookup_without_clock(&identity.public_hex())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn a_connect_request_is_delivered_once_and_removed() {
        let (broker, _clock) = clocked(ISSUED);
        let app = broker.router();
        let target = Identity::generate();
        let target_id = register_empty(&app, &target).await;
        let requester = Identity::generate();
        let request = ConnectRequest::sign(&requester, &target_id, "relay-session", ISSUED, TTL);

        let (status, _) = leave_request(&app, &request).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, body) = take_requests(&app, &target_id, ISSUED).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["requests"],
            json!([serde_json::to_value(&request).expect("a request is JSON")]),
            "the target takes exactly the request that was left"
        );

        let (status, body) = take_requests(&app, &target_id, ISSUED).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["requests"],
            json!([]),
            "the inbox is one-shot, not a mailbox"
        );
    }

    #[tokio::test]
    async fn a_connect_request_past_its_ttl_is_dropped() {
        let (broker, clock) = clocked(ISSUED);
        let app = broker.router();
        let target = Identity::generate();
        let target_id = register_empty(&app, &target).await;
        let requester = Identity::generate();

        let fresh = ConnectRequest::sign(&requester, &target_id, "fresh", ISSUED, 500);
        assert_eq!(leave_request(&app, &fresh).await.0, StatusCode::NO_CONTENT);
        clock.store(ISSUED + 500, std::sync::atomic::Ordering::SeqCst);
        let (_, body) = take_requests(&app, &target_id, ISSUED + 500).await;
        assert_eq!(
            body["requests"],
            json!([serde_json::to_value(&fresh).expect("a request is JSON")]),
            "exactly at the ttl is still fresh"
        );

        let expired = ConnectRequest::sign(&requester, &target_id, "expired", ISSUED, 500);
        assert_eq!(
            leave_request(&app, &expired).await.0,
            StatusCode::NO_CONTENT
        );
        // THE TTL RUNS FROM RECEIPT, not from the signed stamp: the note was accepted at +500, so it is
        // stale one millisecond past +1,000 — not at +501, which is only one millisecond of inbox life.
        clock.store(ISSUED + 1_001, std::sync::atomic::Ordering::SeqCst);
        let (status, body) = take_requests(&app, &target_id, ISSUED + 1_001).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["requests"],
            json!([]),
            "a request past its ttl measured from receipt is dropped, not returned"
        );

        clock.store(ISSUED, std::sync::atomic::Ordering::SeqCst);
        let (_, body) = take_requests(&app, &target_id, ISSUED).await;
        assert_eq!(
            body["requests"],
            json!([]),
            "the expired request was removed, not hidden until later"
        );
    }

    #[tokio::test]
    async fn a_bad_request_signature_is_403_and_does_not_consume_another_request() {
        let (broker, _clock) = clocked(ISSUED);
        let app = broker.router();
        let target = Identity::generate();
        let target_id = register_empty(&app, &target).await;
        let honest = Identity::generate();
        let good = ConnectRequest::sign(&honest, &target_id, "honest", ISSUED, TTL);
        assert_eq!(leave_request(&app, &good).await.0, StatusCode::NO_CONTENT);

        let forger = Identity::generate();
        let mut forged = ConnectRequest::sign(&forger, &target_id, "forged", ISSUED, TTL);
        forged.sig[0] ^= 0x01;
        let (status, body) = leave_request(&app, &forged).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], json!("bad_signature"));

        let (status, body) = take_requests(&app, &target_id, ISSUED).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["requests"],
            json!([serde_json::to_value(&good).expect("a request is JSON")]),
            "the refused request must not consume the one already waiting"
        );
    }

    #[tokio::test]
    async fn a_request_for_a_peer_that_never_registered_is_404() {
        let app = app();
        let stranger = Identity::generate().public_hex();
        let requester = Identity::generate();
        let request = ConnectRequest::sign(&requester, &stranger, "relay-session", ISSUED, TTL);

        let (status, body) = leave_request(&app, &request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], json!("unknown_peer"));

        let (status, body) = take_requests(&app, &stranger, ISSUED).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], json!("unknown_peer"));
    }

    #[tokio::test]
    async fn a_malformed_connect_request_is_400() {
        let app = app();
        let target = Identity::generate();
        let target_id = register_empty(&app, &target).await;

        let broken = Request::builder()
            .method("POST")
            .uri(format!("/request/{target_id}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{"))
            .expect("the request builds");
        assert_eq!(
            send(app.clone(), broken).await.0,
            StatusCode::BAD_REQUEST,
            "invalid JSON is a malformed body"
        );

        let incomplete = Request::builder()
            .method("POST")
            .uri(format!("/request/{target_id}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "requester_id": "half" }).to_string()))
            .expect("the request builds");
        assert_eq!(
            send(app.clone(), incomplete).await.0,
            StatusCode::BAD_REQUEST,
            "a body missing the request's fields is malformed"
        );

        let requester = Identity::generate();
        let mut bad_requester =
            ConnectRequest::sign(&requester, &target_id, "relay-session", ISSUED, TTL);
        bad_requester.requester_id = "not-a-key".to_owned();
        assert_eq!(
            leave_request(&app, &bad_requester).await.0,
            StatusCode::BAD_REQUEST,
            "a requester id that is not a public key cannot be verified"
        );

        let mut bad_signature_length =
            ConnectRequest::sign(&requester, &target_id, "relay-session", ISSUED, TTL);
        bad_signature_length.sig.truncate(63);
        assert_eq!(
            leave_request(&app, &bad_signature_length).await.0,
            StatusCode::BAD_REQUEST,
            "a signature of the wrong length is malformed, not a valid signature"
        );

        let without_clock = Request::builder()
            .method("GET")
            .uri(format!("/requests/{target_id}"))
            .body(Body::empty())
            .expect("the request builds");
        assert_eq!(
            send(app.clone(), without_clock).await.0,
            StatusCode::BAD_REQUEST,
            "a request poll without the caller's clock is malformed"
        );

        let elsewhere = Identity::generate().public_hex();
        let addressed_elsewhere =
            ConnectRequest::sign(&requester, &elsewhere, "relay-session", ISSUED, TTL);
        let mismatched = Request::builder()
            .method("POST")
            .uri(format!("/request/{target_id}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&addressed_elsewhere).expect("a request is JSON"),
            ))
            .expect("the request builds");
        assert_eq!(
            send(app, mismatched).await.0,
            StatusCode::BAD_REQUEST,
            "one note cannot name a target other than the path"
        );
    }

    #[tokio::test]
    async fn the_broker_has_no_tool_route() {
        let app = app();

        for path in ["/tool/echo", "/tools", "/payload", "/invoke"] {
            let get_request = Request::builder()
                .method("GET")
                .uri(path)
                .body(Body::empty())
                .expect("the request builds");
            let (status, _) = send(app.clone(), get_request).await;
            assert_eq!(
                status,
                StatusCode::NOT_FOUND,
                "GET {path} must not reach a tool"
            );

            let post_request = Request::builder()
                .method("POST")
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "tool": "echo" }).to_string()))
                .expect("the request builds");
            let (status, _) = send(app.clone(), post_request).await;
            assert_eq!(
                status,
                StatusCode::NOT_FOUND,
                "POST {path} must not carry a payload"
            );
        }
    }
}
