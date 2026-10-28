//! The rendezvous: a signed address book, a signed doorbell, and the client
//! that speaks to it.
//!
//! A rendezvous answers one question: **"where did this peer say it was, and
//! when?"** It is not a message queue, a proxy, or a control plane. It stores
//! candidates — addresses to try — and, for a peer that is not in `connect`,
//! one **connect request**: a signed note asking a second peer to meet the
//! requester at a named relay session. That note carries three names and two
//! stamps, never a frame, a payload, or a tool call, and the broker hands it
//! out only once. Everything the peers actually say to each other travels over
//! QUIC, or through the relay's opaque splice.
//!
//! The only thing a peer can learn here is a hint about where to dial, or a
//! relay session id to meet at; the pinned identity in [`crate::endpoint`] and
//! the hello inside [`crate::relay_tls`] decide whether the meeting reached the
//! right machine.
//!
//! **The peer id is the public key.** The signed hello names an arbitrary
//! control-plane id because a resolver stands behind it. A registration has
//! no resolver: the broker has never heard of the peer before it registers.
//! So `peer_id` here is the peer's Ed25519 public key in hex, and the
//! signature is checked against the key that `peer_id` names. A peer can
//! publish only its own addresses; publishing someone else's requires their
//! key. The broker stays a phone book — it just refuses a forged entry.
//!
//! **The address is a hypothesis, not a fact.** A box's egress address may
//! differ by destination and may change over time, so a candidate is where
//! the peer believed it could be reached when it registered. A stale or
//! hostile candidate costs a failed handshake, never a connection to the
//! wrong machine.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::frame::MAX_FRAME_BYTES;
use crate::hello::PublicKey;
use crate::keys::{self, Identity, KeyError, SIGNATURE_BYTES};

/// How a candidate says it can be reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CandidateKind {
    /// A UDP path for a hole punch or QUIC.
    Udp,
    /// A TCP path, such as the relay's session listener.
    Tcp,
}

impl CandidateKind {
    /// The wire spelling of this kind, used in the signed message.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Udp => "udp",
            Self::Tcp => "tcp",
        }
    }
}

impl std::fmt::Display for CandidateKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One address a peer says it can be reached at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Whether this is a UDP or a TCP candidate.
    pub kind: CandidateKind,
    /// The address, exactly as the peer spelled it.
    pub addr: String,
    /// When the peer observed this address, in Unix milliseconds.
    pub seen_at_ms: u64,
}

impl Candidate {
    /// A candidate observed at `seen_at_ms`.
    pub fn new(kind: CandidateKind, addr: impl Into<String>, seen_at_ms: u64) -> Self {
        Self {
            kind,
            addr: addr.into(),
            seen_at_ms,
        }
    }
}

/// What a peer publishes: its addresses, and a signature over them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registration {
    /// The peer's Ed25519 public key in hex. This **is** the peer's name.
    pub peer_id: String,
    /// The addresses the peer asks to be reached at, in preference order.
    pub candidates: Vec<Candidate>,
    /// When this registration was signed, in Unix milliseconds.
    pub issued_at_ms: u64,
    /// How long the registration is useful, in milliseconds.
    pub ttl_ms: u64,
    /// The Ed25519 signature over [`Registration::signed_message`].
    pub sig: Vec<u8>,
}

impl Registration {
    /// Sign a fresh registration with `identity`.
    ///
    /// The peer id is taken from the identity: a registration cannot claim a
    /// name whose key it does not hold.
    pub fn sign(
        identity: &Identity,
        candidates: Vec<Candidate>,
        issued_at_ms: u64,
        ttl_ms: u64,
    ) -> Self {
        let peer_id = identity.public_hex();
        let mut registration = Self {
            peer_id,
            candidates,
            issued_at_ms,
            ttl_ms,
            sig: Vec::new(),
        };
        registration.sig = identity.sign(&registration.signed_message()).to_vec();
        registration
    }

    /// The exact bytes the signature covers.
    ///
    /// `{peer_id}\n{issued_at_ms}\n{ttl_ms}\n{kind:addr,kind:addr,…}` — the
    /// candidates in order, each with its kind, and no trailing separator.
    /// Moving a candidate, changing its kind, dropping one, or reordering
    /// them changes the bytes.
    pub fn signed_message(&self) -> Vec<u8> {
        let candidates = self
            .candidates
            .iter()
            .map(|candidate| format!("{}:{}", candidate.kind, candidate.addr))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{}\n{}\n{}\n{}",
            self.peer_id, self.issued_at_ms, self.ttl_ms, candidates
        )
        .into_bytes()
    }

    /// Verify this registration against a public key.
    pub fn verify(&self, public: &PublicKey) -> Result<(), RendezvousError> {
        let signature: [u8; SIGNATURE_BYTES] = self.sig.as_slice().try_into().map_err(|_| {
            RendezvousError::Malformed(format!(
                "an Ed25519 signature is {SIGNATURE_BYTES} bytes; this one is {}",
                self.sig.len()
            ))
        })?;
        keys::verify(public, &self.signed_message(), &signature).map_err(|error| match error {
            KeyError::BadSignature => RendezvousError::BadSignature,
            other => RendezvousError::Malformed(format!(
                "the registration's signature cannot be checked: {other}"
            )),
        })
    }

    /// Verify this registration against the key its `peer_id` names.
    ///
    /// This is the broker's check: a peer id that is not a public key is a
    /// malformed registration, not a signature failure.
    pub fn verify_self(&self) -> Result<(), RendezvousError> {
        let public = Identity::public_from_hex(&self.peer_id).map_err(|error| {
            RendezvousError::Malformed(format!(
                "the registration's peer id is not a mesh public key: {error}"
            ))
        })?;
        self.verify(&public)
    }

    /// Whether this registration is past its ttl at `now_ms`.
    ///
    /// Fresh while `now_ms <= issued_at_ms + ttl_ms`. A clock that runs
    /// backwards cannot make a registration stale, and a registration is
    /// fresh exactly at its expiry instant — "older than its ttl" is strict.
    pub fn is_stale(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.issued_at_ms) > self.ttl_ms
    }
}

/// The exact bytes a poll signature covers.
///
/// `poll\n{peer_id}\n{now_ms}`. The peer is the only party that can produce it, so the broker hands
/// an inbox to its owner instead of to whoever knows the key (which is public).
pub fn poll_message(peer_id: &str, now_ms: u64) -> Vec<u8> {
    format!("poll\n{peer_id}\n{now_ms}").into_bytes()
}

/// How far a poll's timestamp may differ from the broker's clock.
///
/// The signature is replayable inside this window (the URL carries it), which is the price of a GET;
/// the queue is drained once, so a replayed poll finds it empty. Two minutes is generous for clock
/// drift between two machines on the same tailnet, and short enough that a captured URL is not useful
/// for long.
pub const POLL_SKEW_MS: u64 = 2 * 60 * 1000;

/// How many candidates one lookup may contribute to a dial.
///
/// The list is written by a peer; the dialer pays a QUIC handshake per address it tries. A device
/// publishes a handful (its observed address, a local fallback, maybe a note's), so eight is generous
/// and bounds what one hostile registration can cost a punch.
pub const MAX_CANDIDATES: usize = 8;

/// What a lookup returns: the fresh candidates and the freshness stamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookupResponse {
    /// When the registration was signed, in Unix milliseconds.
    pub issued_at_ms: u64,
    /// The candidates the peer published, in its order.
    pub candidates: Vec<Candidate>,
}

/// A signed note asking one peer to meet another at a named relay session.
///
/// This is the rendezvous's second kind of value, and the only one that exists
/// because a peer may be idle: a `connect` that falls to the relay publishes
/// the pair's relay session id here, so a peer that is already in `serve` can
/// learn where to meet it without both ends calling `connect` at the same
/// instant.
///
/// **It carries no payload.** The fields are the two peer ids, the relay
/// session id and two stamps; there is no frame, no tool call, and no byte the
/// peers will exchange. The relay session id is a name, not a secret: the two
/// ends still run the TLS session inside the splice and pin each other's keys,
/// so a forged request can delay a connection but cannot impersonate a peer.
///
/// The signature covers `"request\n{requester_id}\n{target_id}\n{session_id}\n
/// {issued_at_ms}"`, with no trailing separator. `ttl_ms` is deliberately not
/// signed: it is the requester's patience, not part of who is asking, and
/// changing it changes how long the note is offered rather than the identity
/// it names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectRequest {
    /// The requester's Ed25519 public key in hex. This **is** the requester.
    pub requester_id: String,
    /// The public key in hex of the peer the requester wants to meet.
    pub target_id: String,
    /// The relay session id both ends will pair on.
    pub session_id: String,
    /// When the request was signed, in Unix milliseconds.
    pub issued_at_ms: u64,
    /// How long the request is useful, in milliseconds.
    pub ttl_ms: u64,
    /// The Ed25519 signature over [`ConnectRequest::signed_message`].
    pub sig: Vec<u8>,
}

impl ConnectRequest {
    /// Sign a fresh request with `identity`.
    ///
    /// `requester_id` is taken from the identity: a request cannot claim a
    /// name whose key it does not hold.
    pub fn sign(
        identity: &Identity,
        target_id: impl Into<String>,
        session_id: impl Into<String>,
        issued_at_ms: u64,
        ttl_ms: u64,
    ) -> Self {
        let mut request = Self {
            requester_id: identity.public_hex(),
            target_id: target_id.into(),
            session_id: session_id.into(),
            issued_at_ms,
            ttl_ms,
            sig: Vec::new(),
        };
        request.sig = identity.sign(&request.signed_message()).to_vec();
        request
    }

    /// The exact bytes the signature covers.
    ///
    /// `"request\n{requester_id}\n{target_id}\n{session_id}\n{issued_at_ms}"`.
    /// Moving the request to another target, naming another relay session, or
    /// changing its instant changes the bytes.
    pub fn signed_message(&self) -> Vec<u8> {
        format!(
            "request\n{}\n{}\n{}\n{}",
            self.requester_id, self.target_id, self.session_id, self.issued_at_ms
        )
        .into_bytes()
    }

    /// Verify this request against a public key.
    pub fn verify(&self, public: &PublicKey) -> Result<(), RendezvousError> {
        let signature: [u8; SIGNATURE_BYTES] = self.sig.as_slice().try_into().map_err(|_| {
            RendezvousError::Malformed(format!(
                "an Ed25519 signature is {SIGNATURE_BYTES} bytes; this one is {}",
                self.sig.len()
            ))
        })?;
        keys::verify(public, &self.signed_message(), &signature).map_err(|error| match error {
            KeyError::BadSignature => RendezvousError::BadSignature,
            other => RendezvousError::Malformed(format!(
                "the connect request's signature cannot be checked: {other}"
            )),
        })
    }

    /// Verify this request against the key its `requester_id` names.
    ///
    /// This is the broker's check: a requester id that is not a public key is
    /// a malformed request, not a signature failure.
    pub fn verify_self(&self) -> Result<(), RendezvousError> {
        let public = Identity::public_from_hex(&self.requester_id).map_err(|error| {
            RendezvousError::Malformed(format!(
                "the connect request's requester id is not a mesh public key: {error}"
            ))
        })?;
        self.verify(&public)
    }

    /// Whether this request is past its ttl at `now_ms`.
    ///
    /// Fresh while `now_ms <= issued_at_ms + ttl_ms`, the same strict
    /// "older than its ttl" rule a [`Registration`] uses.
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.issued_at_ms) > self.ttl_ms
    }
}

/// What a poll of a peer's connect-request inbox returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestsResponse {
    /// The unexpired requests that were waiting for the peer, in arrival order.
    pub requests: Vec<ConnectRequest>,
}

/// Why a registration or a rendezvous exchange was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RendezvousError {
    /// The broker URL is not a plain `http://host:port` origin.
    BadUrl(String),
    /// The registration or the broker's answer had the wrong shape.
    Malformed(String),
    /// The signature does not verify against the key the peer id names.
    BadSignature,
    /// The broker has no fresh registration for this peer.
    UnknownPeer(String),
    /// The broker's registration table is full; a new peer cannot register
    /// until an existing one is swept or refreshed away.
    TableFull,
    /// The broker answered with a status the client does not accept.
    Refused {
        /// The HTTP status.
        status: u16,
        /// The broker's explanation, if it sent one.
        detail: String,
    },
    /// The connection failed.
    Io(std::io::ErrorKind),
    /// The exchange did not complete in time.
    Timeout,
}

impl std::fmt::Display for RendezvousError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadUrl(message) => write!(f, "not a mesh broker URL: {message}"),
            Self::Malformed(message) => write!(f, "not a mesh rendezvous message: {message}"),
            Self::BadSignature => {
                f.write_str("the rendezvous registration's signature does not verify")
            }
            Self::UnknownPeer(peer_id) => {
                write!(f, "the rendezvous has no fresh registration for {peer_id}")
            }
            Self::TableFull => f.write_str("the rendezvous registration table is full"),
            Self::Refused { status, detail } => {
                write!(
                    f,
                    "the rendezvous refused the request with {status}: {detail}"
                )
            }
            Self::Io(kind) => write!(f, "the rendezvous connection failed: {kind:?}"),
            Self::Timeout => f.write_str("the rendezvous did not answer in time"),
        }
    }
}

impl std::error::Error for RendezvousError {}

impl From<std::io::Error> for RendezvousError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.kind())
    }
}

/// The clock a client uses, as a pure input rather than a hidden `SystemTime`.
type Clock = std::sync::Arc<dyn Fn() -> u64 + Send + Sync>;

/// When the system clock says it is, in Unix milliseconds.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

/// The rendezvous client: one identity, the candidates it wants published,
/// and the clock that stamps both the registration and the lookup.
///
/// It speaks HTTP/1.1 over one TCP connection per request — `Connection:
/// close`, a bounded answer. There is no TLS and no connection pool here on
/// purpose: phase 2's broker is a local process, and phase 2b owns the
/// relay/broker reachable over the internet.
///
/// Validate a broker URL without contacting it: the same rule every request applies.
///
/// Returns the canonical `host:port` authority (a trailing slash is accepted and stripped). This is
/// the console's startup check — a console that named an impossible broker should say so at start,
/// not on the first readiness request.
pub fn validate_broker_url(broker_url: &str) -> Result<String, RendezvousError> {
    origin(broker_url)
}

/// Ask a broker for a peer's current candidates, WITHOUT holding an identity.
///
/// A lookup is unauthenticated and signs nothing, so the console (or any readiness probe) can ask
/// "is this peer registered right now?" without a key of its own. `now_ms` is the caller's clock; the
/// broker ignores it for freshness (its own clock decides), but it remains in the wire for
/// compatibility. `timeout` bounds the whole exchange.
pub async fn lookup_candidates(
    broker_url: &str,
    peer_id: &str,
    now_ms: u64,
    timeout: std::time::Duration,
) -> Result<Vec<Candidate>, RendezvousError> {
    // The broker's path segment is a hex key; canonicalising it here keeps the request path to bytes
    // that cannot mean anything else.
    let key = Identity::public_from_hex(peer_id).map_err(|error| {
        RendezvousError::Malformed(format!("{peer_id:?} is not a mesh peer id: {error}"))
    })?;
    let peer_id = keys::hex_encode(&key);
    let path = format!("/lookup/{peer_id}?now_ms={now_ms}");

    let (status, body) = request(broker_url, "GET", &path, None, timeout).await?;
    match status {
        200 => {
            let answer: LookupResponse = serde_json::from_slice(&body)
                .map_err(|error| RendezvousError::Malformed(error.to_string()))?;
            // THE LIST COMES FROM ANOTHER PEER, through the broker. A registration may name a
            // thousand addresses (the broker's body cap allows it), and every one of them is a QUIC
            // handshake the dialer would pay for; a handful is what a real device publishes.
            let mut candidates = answer.candidates;
            if candidates.len() > MAX_CANDIDATES {
                candidates.truncate(MAX_CANDIDATES);
            }
            Ok(candidates)
        }
        404 => Err(RendezvousError::UnknownPeer(peer_id)),
        status => Err(RendezvousError::Refused {
            status,
            detail: response_detail(&body),
        }),
    }
}

/// It is [`Clone`] because a serving session hands one copy to its inbox
/// poller task while the session keeps its own.
#[derive(Clone)]
pub struct Rendezvous {
    identity: Identity,
    candidates: Vec<Candidate>,
    ttl_ms: u64,
    clock: Clock,
    /// How long one exchange — connect, write and answer — may take. The
    /// session sets this from its configuration; a standalone client gets
    /// [`REQUEST_TIMEOUT`].
    request_timeout: Duration,
}

impl Rendezvous {
    /// A client whose stamps come from the system clock.
    pub fn new(identity: Identity, candidates: Vec<Candidate>, ttl_ms: u64) -> Self {
        Self::with_clock(identity, candidates, ttl_ms, now_ms)
    }

    /// A client with an injected clock, so a test can move time.
    pub fn with_clock<F>(
        identity: Identity,
        candidates: Vec<Candidate>,
        ttl_ms: u64,
        clock: F,
    ) -> Self
    where
        F: Fn() -> u64 + Send + Sync + 'static,
    {
        Self {
            identity,
            candidates,
            ttl_ms,
            clock: Arc::new(clock),
            request_timeout: REQUEST_TIMEOUT,
        }
    }

    /// Bound each exchange by `request_timeout` instead of [`REQUEST_TIMEOUT`].
    ///
    /// This is the client's transport patience, not a path deadline: the
    /// session passes its configured lookup timeout here and turns a timeout
    /// into a typed rendezvous error rather than a `Path` tick.
    pub fn with_timeout(mut self, request_timeout: Duration) -> Self {
        self.request_timeout = request_timeout.max(Duration::from_millis(1));
        self
    }

    /// This client's peer id — the hex public key it registers under.
    pub fn peer_id(&self) -> String {
        self.identity.public_hex()
    }

    /// The registration this client would publish, signed now.
    pub fn registration(&self) -> Registration {
        Registration::sign(
            &self.identity,
            self.candidates.clone(),
            (self.clock)(),
            self.ttl_ms,
        )
    }

    /// Publish the candidates at `broker_url`.
    pub async fn register(&self, broker_url: &str) -> Result<(), RendezvousError> {
        let registration = self.registration();
        let body = serde_json::to_vec(&registration)
            .map_err(|error| RendezvousError::Malformed(error.to_string()))?;
        let (status, answer) = request(
            broker_url,
            "POST",
            "/register",
            Some(&body),
            self.request_timeout,
        )
        .await?;
        match status {
            200..=299 => Ok(()),
            // The broker's admission refusal has its own name: a caller can
            // tell "the table is full" from a malformed or refused body.
            429 => Err(RendezvousError::TableFull),
            status => Err(RendezvousError::Refused {
                status,
                detail: response_detail(&answer),
            }),
        }
    }

    /// Ask `broker_url` where `peer_id` said it was.
    pub async fn lookup(
        &self,
        broker_url: &str,
        peer_id: &str,
    ) -> Result<Vec<Candidate>, RendezvousError> {
        lookup_candidates(broker_url, peer_id, (self.clock)(), self.request_timeout).await
    }

    /// Leave a signed connect request for `target_id` at `broker_url`.
    ///
    /// The request is signed with this client's identity and stamped by its
    /// clock, so a test can move time without sleeping. It returns the request
    /// that was accepted, including the ttl the caller asked for; the broker
    /// stores exactly those fields and hands them to the target once.
    pub async fn request(
        &self,
        broker_url: &str,
        target_id: &str,
        session_id: &str,
        ttl_ms: u64,
    ) -> Result<ConnectRequest, RendezvousError> {
        let target_id = canonical_peer(target_id)?;
        let note = ConnectRequest::sign(
            &self.identity,
            target_id.clone(),
            session_id,
            (self.clock)(),
            ttl_ms,
        );
        let body = serde_json::to_vec(&note)
            .map_err(|error| RendezvousError::Malformed(error.to_string()))?;
        let path = format!("/request/{target_id}");
        let (status, answer) =
            request(broker_url, "POST", &path, Some(&body), self.request_timeout).await?;
        match status {
            // 204 is the broker's answer; any 2xx means the note was taken.
            200..=299 => Ok(note),
            404 => Err(RendezvousError::UnknownPeer(target_id)),
            status => Err(RendezvousError::Refused {
                status,
                detail: response_detail(&answer),
            }),
        }
    }

    /// Take the unexpired connect requests waiting for `peer_id`.
    ///
    /// The broker's inbox is one-shot: whatever this call returns has already
    /// been removed, and the next poll sees only requests that arrived since.
    /// A request past its ttl is dropped by the broker, never returned.
    pub async fn requests(
        &self,
        broker_url: &str,
        peer_id: &str,
    ) -> Result<Vec<ConnectRequest>, RendezvousError> {
        let peer_id = canonical_peer(peer_id)?;
        // The poll's stamp is wall time, not the registration clock a test injects: the broker
        // compares it with its own wall clock, and a test clock is a knob for ttl, not for freshness.
        let polled_at_ms = now_ms();
        // The broker verifies this against the key its registration names: knowing a peer's public
        // key is not the same as being that peer, and the inbox is the peer's own.
        let signature = self.identity.sign(&poll_message(&peer_id, polled_at_ms));
        let path = format!(
            "/requests/{peer_id}?now_ms={polled_at_ms}&sig={}",
            keys::hex_encode(&signature)
        );
        let (status, body) = request(broker_url, "GET", &path, None, self.request_timeout).await?;
        match status {
            200 => {
                let answer: RequestsResponse = serde_json::from_slice(&body)
                    .map_err(|error| RendezvousError::Malformed(error.to_string()))?;
                Ok(answer.requests)
            }
            404 => Err(RendezvousError::UnknownPeer(peer_id)),
            status => Err(RendezvousError::Refused {
                status,
                detail: response_detail(&body),
            }),
        }
    }
}

/// A peer id as the canonical lowercase hex key the broker indexes.
fn canonical_peer(peer_id: &str) -> Result<String, RendezvousError> {
    let key = Identity::public_from_hex(peer_id).map_err(|error| {
        RendezvousError::Malformed(format!("{peer_id:?} is not a mesh peer id: {error}"))
    })?;
    Ok(keys::hex_encode(&key))
}

/// How long one broker exchange may take, from connect to final byte.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Send one request and read one response.
async fn request(
    broker_url: &str,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    request_timeout: Duration,
) -> Result<(u16, Vec<u8>), RendezvousError> {
    let authority = origin(broker_url)?;
    let mut stream = tokio::time::timeout(request_timeout, TcpStream::connect(&authority))
        .await
        .map_err(|_| RendezvousError::Timeout)?
        .map_err(RendezvousError::from)?;

    let mut head =
        format!("{method} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n");
    if let Some(body) = body {
        use std::fmt::Write as _;
        head.push_str("Content-Type: application/json\r\n");
        write!(head, "Content-Length: {}\r\n", body.len())
            .expect("writing to a String cannot fail");
    }
    head.push_str("\r\n");

    tokio::time::timeout(request_timeout, async {
        stream.write_all(head.as_bytes()).await?;
        if let Some(body) = body {
            stream.write_all(body).await?;
        }
        stream.flush().await?;
        read_response(&mut stream).await
    })
    .await
    .map_err(|_| RendezvousError::Timeout)?
}

/// The `http://host:port` authority, refusing anything the phase-2 broker is not.
fn origin(broker_url: &str) -> Result<String, RendezvousError> {
    if broker_url.starts_with("https://") {
        return Err(RendezvousError::BadUrl(format!(
            "{broker_url:?} uses TLS; the local broker is plain HTTP (TLS is phase 2b)"
        )));
    }
    let Some(rest) = broker_url.strip_prefix("http://") else {
        return Err(RendezvousError::BadUrl(format!(
            "a mesh broker URL is http://host:port; {broker_url:?} is not"
        )));
    };
    let authority = rest.trim_end_matches('/');
    if authority.is_empty()
        || authority.contains('/')
        || authority.contains('?')
        || authority.contains('#')
    {
        return Err(RendezvousError::BadUrl(format!(
            "a mesh broker URL is an origin such as http://127.0.0.1:8080; {broker_url:?} is not"
        )));
    }
    Ok(authority.to_owned())
}

/// Read one complete HTTP/1.x response.
///
/// The request says `Connection: close`, so reading to EOF is bounded and
/// simple; `Content-Length` and chunked bodies are both decoded. The whole
/// answer is capped at [`MAX_FRAME_BYTES`]: a broker that says more than a
/// frame is not one this client will buffer.
async fn read_response(stream: &mut TcpStream) -> Result<(u16, Vec<u8>), RendezvousError> {
    let mut raw = Vec::new();
    stream
        .take((MAX_FRAME_BYTES + 1) as u64)
        .read_to_end(&mut raw)
        .await?;
    if raw.len() > MAX_FRAME_BYTES {
        return Err(RendezvousError::Malformed(format!(
            "the broker's answer is larger than the {MAX_FRAME_BYTES}-byte cap"
        )));
    }

    let Some(header_end) = find_bytes(&raw, b"\r\n\r\n") else {
        return Err(RendezvousError::Malformed(
            "the broker's answer has no HTTP header terminator".to_owned(),
        ));
    };
    let head = std::str::from_utf8(&raw[..header_end]).map_err(|error| {
        RendezvousError::Malformed(format!("the broker's HTTP header is not UTF-8: {error}"))
    })?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut status_parts = status_line.split_whitespace();
    let version = status_parts.next();
    if !matches!(version, Some("HTTP/1.1") | Some("HTTP/1.0")) {
        return Err(RendezvousError::Malformed(format!(
            "the broker's status line does not start with an HTTP/1.x version: {status_line:?}"
        )));
    }
    let status = status_parts
        .next()
        .and_then(|text| text.parse::<u16>().ok())
        .ok_or_else(|| {
            RendezvousError::Malformed(format!(
                "the broker's status line carries no status code: {status_line:?}"
            ))
        })?;

    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => {
                let length = value.trim().parse::<usize>().map_err(|error| {
                    RendezvousError::Malformed(format!(
                        "the broker's content-length is not a number: {error}"
                    ))
                })?;
                content_length = Some(length);
            }
            "transfer-encoding" if value.to_ascii_lowercase().contains("chunked") => {
                chunked = true;
            }
            _ => {}
        }
    }

    let body = &raw[header_end + 4..];
    let body = if chunked {
        decode_chunked(body)?
    } else if let Some(length) = content_length {
        if body.len() < length {
            return Err(RendezvousError::Malformed(format!(
                "the broker announced {length} bytes but sent {}",
                body.len()
            )));
        }
        body[..length].to_vec()
    } else {
        body.to_vec()
    };
    Ok((status, body))
}

/// Decode an HTTP/1.1 chunked body.
fn decode_chunked(body: &[u8]) -> Result<Vec<u8>, RendezvousError> {
    let mut decoded = Vec::new();
    let mut rest = body;
    loop {
        let Some(line_end) = find_bytes(rest, b"\r\n") else {
            return Err(RendezvousError::Malformed(
                "the broker's chunked body has no chunk-size line".to_owned(),
            ));
        };
        let size_text = std::str::from_utf8(&rest[..line_end]).map_err(|error| {
            RendezvousError::Malformed(format!("the broker's chunk size is not UTF-8: {error}"))
        })?;
        let size_text = size_text.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|error| {
            RendezvousError::Malformed(format!("the broker's chunk size is not hex: {error}"))
        })?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            return Ok(decoded);
        }
        let Some(payload) = rest.get(..size) else {
            return Err(RendezvousError::Malformed(
                "the broker's chunk is shorter than its chunk size".to_owned(),
            ));
        };
        let Some(trailer) = rest.get(size..size + 2) else {
            return Err(RendezvousError::Malformed(
                "the broker's chunk has no terminating CRLF".to_owned(),
            ));
        };
        if trailer != b"\r\n" {
            return Err(RendezvousError::Malformed(
                "the broker's chunk does not end in CRLF".to_owned(),
            ));
        }
        decoded.extend_from_slice(payload);
        rest = &rest[size + 2..];
    }
}

/// The first occurrence of `needle` in `haystack`.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The useful part of a broker error body, bounded.
fn response_detail(body: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) {
        if let Some(detail) = value.get("detail").and_then(serde_json::Value::as_str) {
            return detail.chars().take(256).collect();
        }
        if let Some(error) = value.get("error").and_then(serde_json::Value::as_str) {
            return error.to_owned();
        }
    }
    String::from_utf8_lossy(body)
        .trim()
        .chars()
        .take(256)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registration_with_candidates() -> Registration {
        Registration {
            peer_id: "a1b2c3".to_owned(),
            candidates: vec![
                Candidate {
                    kind: CandidateKind::Udp,
                    addr: "10.0.0.1:5000".to_owned(),
                    seen_at_ms: 900,
                },
                Candidate {
                    kind: CandidateKind::Tcp,
                    addr: "10.0.0.1:5001".to_owned(),
                    seen_at_ms: 901,
                },
            ],
            issued_at_ms: 1000,
            ttl_ms: 30_000,
            sig: Vec::new(),
        }
    }

    #[test]
    fn the_signed_message_is_exactly_the_documented_bytes() {
        assert_eq!(
            registration_with_candidates().signed_message(),
            b"a1b2c3\n1000\n30000\nudp:10.0.0.1:5000,tcp:10.0.0.1:5001".to_vec()
        );
    }

    #[test]
    fn the_signed_message_keeps_the_candidate_order() {
        let mut registration = registration_with_candidates();
        registration.candidates.swap(0, 1);

        assert_eq!(
            registration.signed_message(),
            b"a1b2c3\n1000\n30000\ntcp:10.0.0.1:5001,udp:10.0.0.1:5000".to_vec(),
            "the order is part of the signed value"
        );
    }

    #[test]
    fn a_registration_verifies_for_the_key_its_peer_id_names() {
        let identity = Identity::generate();
        let registration = Registration::sign(
            &identity,
            registration_with_candidates().candidates,
            1000,
            30_000,
        );

        assert_eq!(registration.peer_id, identity.public_hex());
        registration
            .verify(&identity.public_bytes())
            .expect("the signer's own pin verifies the registration");
        registration
            .verify_self()
            .expect("the peer id names the signing key");
    }

    #[test]
    fn a_changed_candidate_breaks_the_signature() {
        let identity = Identity::generate();
        let mut registration = Registration::sign(
            &identity,
            registration_with_candidates().candidates,
            1000,
            30_000,
        );
        registration.candidates[0].addr = "10.0.0.9:5000".to_owned();

        assert_eq!(
            registration.verify_self(),
            Err(RendezvousError::BadSignature)
        );
    }

    #[test]
    fn a_changed_kind_breaks_the_signature() {
        let identity = Identity::generate();
        let mut registration = Registration::sign(
            &identity,
            registration_with_candidates().candidates,
            1000,
            30_000,
        );
        registration.candidates[0].kind = CandidateKind::Tcp;

        assert_eq!(
            registration.verify_self(),
            Err(RendezvousError::BadSignature)
        );
    }

    #[test]
    fn a_changed_ttl_breaks_the_signature() {
        let identity = Identity::generate();
        let mut registration = Registration::sign(
            &identity,
            registration_with_candidates().candidates,
            1000,
            30_000,
        );
        registration.ttl_ms = 60_000;

        assert_eq!(
            registration.verify_self(),
            Err(RendezvousError::BadSignature)
        );
    }

    #[test]
    fn a_changed_issued_at_breaks_the_signature() {
        let identity = Identity::generate();
        let mut registration = Registration::sign(
            &identity,
            registration_with_candidates().candidates,
            1000,
            30_000,
        );
        registration.issued_at_ms = 2000;

        assert_eq!(
            registration.verify_self(),
            Err(RendezvousError::BadSignature)
        );
    }

    #[test]
    fn a_registration_signed_by_another_key_is_refused() {
        let victim = Identity::generate();
        let attacker = Identity::generate();
        let mut registration = Registration::sign(
            &attacker,
            registration_with_candidates().candidates,
            1000,
            30_000,
        );
        registration.peer_id = victim.public_hex();

        assert_eq!(
            registration.verify_self(),
            Err(RendezvousError::BadSignature)
        );
    }

    #[test]
    fn a_signature_of_the_wrong_length_is_malformed() {
        let identity = Identity::generate();
        let mut registration = Registration::sign(
            &identity,
            registration_with_candidates().candidates,
            1000,
            30_000,
        );
        registration.sig.truncate(SIGNATURE_BYTES - 1);

        assert!(matches!(
            registration.verify_self(),
            Err(RendezvousError::Malformed(_))
        ));
    }

    #[test]
    fn a_peer_id_that_is_not_a_public_key_is_malformed() {
        let identity = Identity::generate();
        let mut registration = Registration::sign(
            &identity,
            registration_with_candidates().candidates,
            1000,
            30_000,
        );
        registration.peer_id = "not-a-key".to_owned();

        assert!(matches!(
            registration.verify_self(),
            Err(RendezvousError::Malformed(_))
        ));
    }

    #[test]
    fn freshness_is_a_function_of_now_and_the_ttl() {
        let registration = registration_with_candidates();

        assert!(
            !registration.is_stale(1000),
            "a fresh registration is fresh"
        );
        assert!(
            !registration.is_stale(31_000),
            "exactly at the ttl is not older"
        );
        assert!(
            registration.is_stale(31_001),
            "one millisecond past is older"
        );
        assert!(
            !registration.is_stale(0),
            "a clock that runs backwards cannot expire a registration"
        );
    }

    #[test]
    fn a_candidate_is_wire_shaped_as_lowercase_kind_addr_and_stamp() {
        let candidate = Candidate {
            kind: CandidateKind::Tcp,
            addr: "10.0.0.1:5001".to_owned(),
            seen_at_ms: 7,
        };

        assert_eq!(
            serde_json::to_value(&candidate).expect("a candidate is JSON"),
            serde_json::json!({
                "kind": "tcp",
                "addr": "10.0.0.1:5001",
                "seen_at_ms": 7
            })
        );
    }

    #[test]
    fn a_registration_round_trips_through_json() {
        let identity = Identity::generate();
        let registration = Registration::sign(
            &identity,
            registration_with_candidates().candidates,
            1000,
            30_000,
        );
        let json = serde_json::to_vec(&registration).expect("a registration is JSON");
        let parsed: Registration =
            serde_json::from_slice(&json).expect("the registration parses back");

        assert_eq!(parsed, registration);
        parsed
            .verify_self()
            .expect("the parsed registration still verifies");
    }

    #[test]
    fn a_connect_request_signed_message_is_exactly_the_documented_bytes() {
        let request = ConnectRequest {
            requester_id: "a1b2c3".to_owned(),
            target_id: "d4e5f6".to_owned(),
            session_id: "relay-session".to_owned(),
            issued_at_ms: 1000,
            ttl_ms: 500,
            sig: Vec::new(),
        };

        assert_eq!(
            request.signed_message(),
            b"request\na1b2c3\nd4e5f6\nrelay-session\n1000".to_vec()
        );
    }

    #[test]
    fn a_connect_request_verifies_for_the_key_its_requester_id_names() {
        let identity = Identity::generate();
        let request = ConnectRequest::sign(&identity, "d4e5f6", "relay-session", 1000, 500);

        assert_eq!(request.requester_id, identity.public_hex());
        request
            .verify(&identity.public_bytes())
            .expect("the signer's own pin verifies the request");
        request
            .verify_self()
            .expect("the requester id names the signing key");
    }

    #[test]
    fn a_changed_request_target_session_or_instant_breaks_the_signature() {
        let identity = Identity::generate();
        let request = ConnectRequest::sign(&identity, "d4e5f6", "relay-session", 1000, 500);

        let mut moved = request.clone();
        moved.target_id = "d4e5f7".to_owned();
        assert_eq!(moved.verify_self(), Err(RendezvousError::BadSignature));

        let mut renamed = request.clone();
        renamed.session_id = "another-session".to_owned();
        assert_eq!(renamed.verify_self(), Err(RendezvousError::BadSignature));

        let mut restamped = request;
        restamped.issued_at_ms = 2000;
        assert_eq!(restamped.verify_self(), Err(RendezvousError::BadSignature));
    }

    #[test]
    fn a_connect_request_signed_by_another_key_is_refused() {
        let victim = Identity::generate();
        let attacker = Identity::generate();
        let mut request =
            ConnectRequest::sign(&attacker, victim.public_hex(), "relay-session", 1000, 500);
        request.requester_id = victim.public_hex();

        assert_eq!(request.verify_self(), Err(RendezvousError::BadSignature));
    }

    #[test]
    fn connect_request_freshness_is_a_function_of_now_and_the_ttl() {
        let identity = Identity::generate();
        let request = ConnectRequest::sign(&identity, "d4e5f6", "relay-session", 1000, 500);

        assert!(!request.is_expired(1000), "a fresh request is fresh");
        assert!(!request.is_expired(1500), "exactly at the ttl is not older");
        assert!(request.is_expired(1501), "one millisecond past is older");
        assert!(
            !request.is_expired(0),
            "a clock that runs backwards cannot expire a request"
        );
    }

    #[test]
    fn a_connect_request_round_trips_through_json() {
        let identity = Identity::generate();
        let request = ConnectRequest::sign(&identity, "d4e5f6", "relay-session", 1000, 500);
        let json = serde_json::to_vec(&request).expect("a request is JSON");
        let parsed: ConnectRequest =
            serde_json::from_slice(&json).expect("the request parses back");

        assert_eq!(parsed, request);
        parsed
            .verify_self()
            .expect("the parsed request still verifies");
    }

    async fn spawn_broker() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener is bindable");
        let address = listener.local_addr().expect("the bound address is known");
        tokio::spawn(async move {
            axum::serve(listener, crate::broker::Broker::new().router())
                .await
                .expect("the test broker serves");
        });
        format!("http://{address}")
    }

    fn client(
        identity: &Identity,
        candidates: Vec<Candidate>,
        now_ms: u64,
        ttl_ms: u64,
    ) -> Rendezvous {
        Rendezvous::with_clock(identity.clone(), candidates, ttl_ms, move || now_ms)
    }

    #[test]
    fn the_client_signs_with_its_own_identity() {
        let identity = Identity::generate();
        let registration = Rendezvous::new(identity.clone(), Vec::new(), 30_000).registration();

        assert_eq!(registration.peer_id, identity.public_hex());
        registration
            .verify_self()
            .expect("the client's own registration names its key");
    }

    #[tokio::test]
    async fn register_then_lookup_round_trips_the_candidates_in_order() {
        let broker = spawn_broker().await;
        let identity = Identity::generate();
        let candidates = vec![
            Candidate::new(CandidateKind::Udp, "10.0.0.1:5000", 900),
            Candidate::new(CandidateKind::Tcp, "10.0.0.1:5001", 901),
        ];
        let client = client(&identity, candidates.clone(), 1_000, 30_000);

        client
            .register(&broker)
            .await
            .expect("the broker accepts a signed registration");
        let answer = client
            .lookup(&broker, &identity.public_hex())
            .await
            .expect("the registration is fresh");

        assert_eq!(answer, candidates);
    }

    /// **THE BROKER'S CLOCK IS THE ONE THAT AGES A REGISTRATION.** This test used to assert the
    /// opposite — that the client's `now_ms` is the clock the broker saw — which was the defect: a client
    /// an hour ahead deleted every other peer's registration. A far-ahead client now sees exactly what a
    /// correct one does, because the broker does not read the number it sends.
    #[test]
    fn a_broker_url_is_validated_without_contacting_it() {
        assert_eq!(
            validate_broker_url("http://127.0.0.1:8080/").expect("a plain origin"),
            "127.0.0.1:8080"
        );
        for bad in [
            "https://broker.example",
            "http://host:1/path",
            "broker.example:1",
            "http://host:1?x=1",
        ] {
            assert!(
                matches!(validate_broker_url(bad), Err(RendezvousError::BadUrl(_))),
                "{bad:?} must be refused at startup, not at the first lookup"
            );
        }
    }

    #[tokio::test]
    async fn a_lookup_needs_no_identity() {
        let broker = spawn_broker().await;
        let identity = Identity::generate();
        let peer_id = identity.public_hex();
        let registrar = client(&identity, Vec::new(), 1_000, 30_000);
        registrar
            .register(&broker)
            .await
            .expect("the peer registers");

        // The free function answers with the candidates and signs nothing.
        let candidates = lookup_candidates(&broker, &peer_id, 1_000, Duration::from_secs(2))
            .await
            .expect("a registered peer is found");
        assert!(candidates.is_empty(), "no candidates were published");

        let unknown = Identity::generate().public_hex();
        assert_eq!(
            lookup_candidates(&broker, &unknown, 1_000, Duration::from_secs(2)).await,
            Err(RendezvousError::UnknownPeer(unknown))
        );
        assert!(
            matches!(
                lookup_candidates(&broker, "not-a-key", 1_000, Duration::from_secs(2)).await,
                Err(RendezvousError::Malformed(_))
            ),
            "a malformed peer id is refused before any request leaves"
        );
    }

    #[tokio::test]
    async fn a_lookup_with_a_client_clock_far_ahead_still_sees_the_registration() {
        let broker = spawn_broker().await;
        let identity = Identity::generate();
        let candidates = vec![Candidate::new(CandidateKind::Udp, "10.0.0.1:5000", 900)];
        let early = client(&identity, candidates.clone(), 1_000, 30_000);
        early
            .register(&broker)
            .await
            .expect("the broker accepts a signed registration");
        early
            .lookup(&broker, &identity.public_hex())
            .await
            .expect("the registration is fresh at its issue instant");

        let fast = client(&identity, candidates.clone(), 3_600_000, 30_000);
        assert_eq!(
            fast.lookup(&broker, &identity.public_hex()).await,
            Ok(candidates),
            "freshness is the broker's clock; the caller's cannot age another peer's entry"
        );
    }

    #[tokio::test]
    async fn a_connect_request_is_left_once_and_taken_once() {
        let broker = spawn_broker().await;
        let target_identity = Identity::generate();
        let target_id = target_identity.public_hex();
        let requester_identity = Identity::generate();

        let target = client(&target_identity, Vec::new(), 1_000, 30_000);
        target
            .register(&broker)
            .await
            .expect("the target registers");
        let requester = client(&requester_identity, Vec::new(), 1_000, 30_000);

        let request = requester
            .request(&broker, &target_id, "relay-session", 500)
            .await
            .expect("the broker accepts the connect request");
        assert_eq!(request.target_id, target_id);
        assert_eq!(request.requester_id, requester_identity.public_hex());

        assert_eq!(
            target.requests(&broker, &target_id).await,
            Ok(vec![request.clone()]),
            "the target takes the request it was left"
        );
        assert_eq!(
            target.requests(&broker, &target_id).await,
            Ok(Vec::new()),
            "the inbox is one-shot"
        );
    }

    /// **THE TTL RUNS FROM RECEIPT, and this is the one test that lets real time pass**, because the
    /// boundary under test IS the broker's wall clock: a note's signed stamp cannot be advanced by a
    /// caller without also changing that caller's clock, which the broker now ignores.
    #[tokio::test]
    async fn an_expired_connect_request_is_not_delivered() {
        let broker = spawn_broker().await;
        let target_identity = Identity::generate();
        let target_id = target_identity.public_hex();
        let requester_identity = Identity::generate();

        let target = client(&target_identity, Vec::new(), 1_000, 30_000);
        target
            .register(&broker)
            .await
            .expect("the target registers");
        let requester = client(&requester_identity, Vec::new(), 1_000, 30_000);
        requester
            .request(&broker, &target_id, "relay-session", 1)
            .await
            .expect("the broker accepts the request");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        assert_eq!(
            target.requests(&broker, &target_id).await,
            Ok(Vec::new()),
            "a request past its ttl measured from receipt is dropped, not delivered"
        );
    }

    #[tokio::test]
    async fn a_request_for_an_unknown_target_is_an_unknown_peer_error() {
        let broker = spawn_broker().await;
        let requester = client(&Identity::generate(), Vec::new(), 1_000, 30_000);
        let stranger = Identity::generate().public_hex();

        assert_eq!(
            requester
                .request(&broker, &stranger, "relay-session", 500)
                .await,
            Err(RendezvousError::UnknownPeer(stranger))
        );
    }

    #[tokio::test]
    async fn an_unknown_peer_is_an_unknown_peer_error() {
        let broker = spawn_broker().await;
        let identity = Identity::generate();
        let stranger = Identity::generate();
        let client = client(&identity, Vec::new(), 1_000, 30_000);

        assert_eq!(
            client.lookup(&broker, &stranger.public_hex()).await,
            Err(RendezvousError::UnknownPeer(stranger.public_hex()))
        );
    }

    #[tokio::test]
    async fn a_full_broker_is_a_typed_table_full() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let address = listener.local_addr().expect("the bound address");
        tokio::spawn(async move {
            axum::serve(listener, crate::broker::Broker::with_capacity(1).router())
                .await
                .expect("the test broker serves");
        });
        let broker = format!("http://{address}");

        let first = client(&Identity::generate(), Vec::new(), 1_000, 30_000);
        first
            .register(&broker)
            .await
            .expect("the first peer fits in the table");
        let second = client(&Identity::generate(), Vec::new(), 1_000, 30_000);
        assert_eq!(
            second.register(&broker).await,
            Err(RendezvousError::TableFull),
            "a 429 is the broker's typed admission refusal, not a generic refusal"
        );
    }

    #[tokio::test]
    async fn an_https_broker_url_is_refused_without_a_connection() {
        let client = client(&Identity::generate(), Vec::new(), 1_000, 30_000);

        let error = client
            .register("https://127.0.0.1:8443")
            .await
            .expect_err("TLS is phase 2b, not a client fallback");
        assert!(matches!(error, RendezvousError::BadUrl(_)), "{error}");
    }

    #[tokio::test]
    async fn a_broker_url_with_a_path_is_refused() {
        let client = client(&Identity::generate(), Vec::new(), 1_000, 30_000);

        let error = client
            .register("http://127.0.0.1:8080/rendezvous")
            .await
            .expect_err("the client's broker URL is an origin, not a base path");
        assert!(matches!(error, RendezvousError::BadUrl(_)), "{error}");
    }
}
