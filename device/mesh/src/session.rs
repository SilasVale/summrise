//! The mesh session: identity, rendezvous and a path, tied together.
//!
//! A [`MeshSession`] is the object a device holds while it is part of the mesh.
//! [`MeshSession::join`] binds the QUIC endpoint, publishes the address it can be
//! reached at, and keeps that registration fresh. [`MeshSession::connect`] then
//! drives one [`summrise_core::mesh::Path`] from the rendezvous to a
//! [`Channel`] — direct when the pinned QUIC handshake works, through the relay
//! when it does not. [`MeshSession::serve`] is the passive half: an accept loop
//! for a device that is idle, so a peer can open a direct channel without this
//! side calling `connect` at the same instant, and a relay-inbox poll so a peer
//! whose direct path is dead can still meet it on the relay floor.
//!
//! **The decisions are `core`'s.** The punch deadline, the fallback reason, the
//! retry budget, the backoff and the 60 s stable-window reset all come from
//! `Path`; this module feeds it observations and the instant of each one, and
//! carries out the actions. It adds no rule of its own.
//!
//! **Time.** `Path` reads no clock. The caller seeds a connect with `now_ms`,
//! and the session advances that value with the monotonic time it actually
//! spent waiting, only so it can compare against the deadlines `Path` handed
//! back (`StartPunch { deadline_ms }`) and tick them when they are due. Two
//! transport bounds are not path decisions: [`RELAY_PATIENCE`] and
//! [`MeshConfig::lookup_timeout_ms`]. The relay is the floor, so a relay that
//! never answers cannot hold a connect forever; the broker exchange is bounded
//! because a broker that accepts TCP and stalls is not a rendezvous. Reaching
//! the lookup bound is a typed [`RendezvousError::Timeout`], never a fabricated
//! `Path` tick: the path deadline has not passed, and only the path may say
//! that it has.
//!
//! **The registration is kept fresh by a task.** `join` publishes the bound
//! address and then re-signs the registration every
//! [`REGISTRATION_REFRESH_MS`], well inside [`REGISTRATION_TTL_MS`], until the
//! session is dropped. A session that cannot reach the broker keeps trying: a
//! lapsed registration makes the peer unreachable (the broker answers 404), it
//! does not break a path that is already up.
//!
//! **The direct roles are computed, not raced.** As in [`crate::relay_tls`],
//! the **lower** public key is the TLS **server**: it accepts the QUIC
//! connection, issues the fresh challenge, and verifies the dialer's signed
//! hello. The **higher** key is the TLS **client**: it dials each UDP candidate
//! the rendezvous returned, under the deadline `Path` set. Both peers compute
//! the same answer from the same two keys, so there is exactly one connection
//! per pair and no "both dialled" duplicate to reconcile. The relay's TLS roles
//! are the same rule.
//!
//! **The address is a hint.** Candidates are tried in the order the peer
//! published them; a wrong or dead one costs a failed handshake, and the
//! pinned certificate decides whether the dial reached the right machine. The
//! punch deadline is split across the candidates: an equal slice each, never
//! less than [`MIN_CANDIDATE_SLICE_MS`], so one blackholed address cannot
//! consume the time the live candidate after it needs. Only when every slice
//! has been spent does the session report the failure to `Path`. A budget
//! shorter than one second per candidate makes the attempt run past the
//! path's deadline, deliberately: the alternative is abandoning a candidate
//! before a QUIC handshake could plausibly finish.
//!
//! **A stranger cannot hold the accept loop.** [`MeshSession::serve`] takes one
//! connection at a time from the endpoint and authenticates it in its own task,
//! under one deadline that covers the QUIC handshake, the challenge, the hello
//! and its verification. A connection that stalls, or fails verification, is
//! closed and the loop moves on; the real peer's connection is never queued
//! behind it. The authenticated [`Channel`] is handed to the caller's async
//! closure, which runs in that connection's task.
//!
//! **The relay inbox is a doorbell, not a mailbox.** A `connect` that falls to
//! the relay first leaves a signed [`ConnectRequest`] at the broker, naming the
//! pair's relay session; a `serve` polls that inbox every
//! [`REQUEST_POLL_INTERVAL`] and meets each request on the relay under its
//! `session_id`, authenticating the peer with the same
//! [`RelayTlsSession::for_peer`] the dialer uses. The request holds three names
//! and two stamps and no payload, and the broker hands it out once. A request
//! that expires before a serving peer collects it is
//! [`SessionError::RequestExpired`] in the dialer, never a wait on the relay's
//! longer pair timeout; a request a served session refuses is dropped without
//! ending either loop.
//!
//! **The candidates are the bound address and, when a reflector answers, the
//! reflected one.** The STUN exchange runs on the very socket quinn will own,
//! before the endpoint is built: a NAT mapping belongs to that socket's
//! 5-tuple, so a reflection asked on a separate socket would name a port QUIC
//! is not listening on. The mapping persists while the socket is open. An
//! unspecified bound address (`0.0.0.0` or `[::]`) is never a candidate: no
//! peer can dial it. With a wildcard bind the reflection is the only
//! candidate; if the reflector is silent, the join asks the kernel for the
//! local address it would use to reach the reflector and publishes that with
//! the socket's own port. Without a reflector, a wildcard bind publishes
//! nothing and says so: the relay is the floor. A reflector that is dead,
//! silent, or speaks something other than STUN is one log line, because a
//! missing reflection costs a direct path, not the session.
//!
//! The 60 s stable-window rule is proven where the clock can be moved:
//! `summrise_core::mesh::tests::a_path_up_for_the_stable_window_starts_the_retries_over`.
//! A session cannot fake time; it takes every instant from its caller.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use summrise_core::mesh::{Path, PathAction, PathEvent, PathState};
use tokio::task::JoinHandle;

use crate::endpoint::{EndpointError, MeshEndpoint, PeerSession};
use crate::frame::{self, Frame, FrameError};
use crate::hello::{PinResolver, PublicKey};
use crate::keys::{self, Identity};
use crate::relay::{RelayClient, RelayError};
use crate::relay_tls::{RelayTlsError, RelayTlsSession};
use crate::rendezvous::{
    self, Candidate, CandidateKind, ConnectRequest, Rendezvous, RendezvousError,
};

/// The broker the session registers with.
pub use axum::http::Uri;

/// A peer is named by its Ed25519 public key in lowercase hex — the same name
/// the rendezvous publishes and the signed hello carries.
pub type PeerId = str;

/// How long a usable path must stay up before a close resets the retry count.
///
/// This is `Path`'s stable-window rule (the design's "a path that worked for a
/// minute is retry 1, not retry 7"), handed to it at construction rather than
/// re-derived here.
pub const STABLE_WINDOW_MS: u64 = 60_000;

/// How long a published registration is fresh, by default.
pub const REGISTRATION_TTL_MS: u64 = 30_000;

/// How often the refresh task re-signs the registration, by default.
pub const REGISTRATION_REFRESH_MS: u64 = 10_000;

/// How long one relay pairing or relay TLS handshake may take.
///
/// The relay is the floor, so there is no fallback to order when it does not
/// answer. This is a transport patience, not a path decision: `Path` has
/// already decided [`PathState::Relayed`] before the relay is dialled.
pub const RELAY_PATIENCE: Duration = Duration::from_secs(10);

/// How often a serving session polls the broker for its connect requests.
///
/// The poll is what lets an idle session meet a relay fallback without ever
/// calling `connect`. A dialer's request must stay fresh for at least one poll
/// plus the TLS handshake after it, which is why [`REQUEST_TTL_MS`] is a
/// multiple of this interval.
pub const REQUEST_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// How long a relay connect request stays in the broker's inbox, by default.
///
/// It is the dialer's patience for the serving peer to collect the request and
/// join the relay. Ten seconds covers several [`REQUEST_POLL_INTERVAL`] polls
/// and the TLS handshake, and is the same order as [`RELAY_PATIENCE`]: a
/// request that outlived the relay wait would be a hang dressed as a retry.
pub const REQUEST_TTL_MS: u64 = 10_000;

/// How long one broker exchange may take, in milliseconds, by default.
///
/// It is the same order as [`RELAY_PATIENCE`] and short enough that a broker
/// that accepted TCP but never answers cannot spend a whole punch timeout.
/// A session takes its value from [`MeshConfig::lookup_timeout_ms`].
pub const LOOKUP_TIMEOUT_MS: u64 = 10_000;

/// How long the join waits for the STUN reflector before publishing the bound
/// address only.
///
/// A reflector that has not answered in two seconds is not going to make the
/// join useful; the relay floor is still there. This is a startup patience,
/// not a path decision.
pub const STUN_TIMEOUT: Duration = Duration::from_secs(2);

/// The time every punch candidate is given, at least.
///
/// A punch deadline is split equally across the candidates still to try. The
/// floor keeps a short budget from writing off a blackholed address in a few
/// milliseconds: every candidate gets enough time for a QUIC handshake, and
/// the failure reaches `Path` only after all of them have had their slice.
pub const MIN_CANDIDATE_SLICE_MS: u64 = 1_000;

/// What a session needs to join a mesh.
#[derive(Clone, Debug)]
pub struct MeshConfig {
    /// The rendezvous broker's origin, such as `http://127.0.0.1:8080`.
    pub broker: Uri,
    /// The relay floor to use when no direct path can be made. `None` means
    /// "direct only": a connect that needs the relay fails with
    /// [`SessionError::RelayUnavailable`] instead of hanging.
    pub relay: Option<SocketAddr>,
    /// The local UDP address the QUIC endpoint binds. Port `0` asks the OS for
    /// one; a specified bound address is one of the candidates the session
    /// publishes.
    pub bind: SocketAddr,
    /// The STUN reflector to ask, on the same socket QUIC will own, what this
    /// session's public address looks like from there.
    ///
    /// `None` publishes the bound address only when it is specified; a
    /// wildcard bind with no reflector publishes nothing, because `0.0.0.0`
    /// is not an address a peer can dial and the relay is the floor. A
    /// reflector that is silent, dead, or speaks something other than STUN is
    /// one log line: the join continues with the best local address it has,
    /// because a missing reflection costs a direct path, not the session.
    pub stun_server: Option<SocketAddr>,
    /// The one timeout `Path` uses for both the rendezvous wait and the punch.
    pub punch_timeout_ms: u64,
    /// How long one broker exchange may take, in milliseconds.
    ///
    /// This bounds the rendezvous client's HTTP exchange, including the
    /// connect, the write and the whole answer. A lookup is additionally
    /// bounded by the rendezvous deadline `Path` armed; this value is the
    /// transport patience, not a second path clock. Reaching it is a typed
    /// [`RendezvousError::Timeout`], never a `Path` tick: the path deadline
    /// has not been reached, so the session must not claim it has.
    pub lookup_timeout_ms: u64,
}

/// Why a session could not be joined or a peer could not be reached.
#[derive(Debug)]
pub enum SessionError {
    /// The peer id is not an Ed25519 public key in hex.
    BadPeerId(String),
    /// The peer id names this session's own key.
    SamePeer,
    /// The broker has no fresh registration for the peer — the lookup was a
    /// 404. Nothing is dialled and no path is spent.
    UnknownPeer(String),
    /// The broker exchange failed for a reason other than "unknown peer".
    Rendezvous(RendezvousError),
    /// The local QUIC endpoint could not be built or bound.
    Endpoint(EndpointError),
    /// The path chose the relay, but this session was not configured with one.
    RelayUnavailable,
    /// The relay did not pair this session with the peer in time.
    RelayTimeout,
    /// The connect request left at the broker expired before a serving peer
    /// collected it and met this session on the relay.
    ///
    /// This is the typed outcome of a relay fallback nobody answered: the
    /// dialer waits no longer than the request it published, rather than the
    /// relay's longer pair timeout.
    RequestExpired,
    /// Pairing through the relay failed.
    Relay(RelayError),
    /// The TLS session inside the relay splice failed.
    RelayTls(RelayTlsError),
    /// The path has no attempts left (six consecutive closes).
    Exhausted,
    /// A frame could not be read or written on the channel.
    Frame(FrameError),
    /// `Path` produced no action when the driver expected one. This is a
    /// defect in the session, not a condition a peer can cause.
    Stalled(PathState),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadPeerId(peer) => {
                write!(f, "{peer:?} is not a mesh peer id (a hex Ed25519 key)")
            }
            Self::SamePeer => f.write_str("a session cannot connect to its own key"),
            Self::UnknownPeer(peer) => {
                write!(f, "the rendezvous has no fresh registration for {peer}")
            }
            Self::Rendezvous(error) => write!(f, "{error}"),
            Self::Endpoint(error) => write!(f, "{error}"),
            Self::RelayUnavailable => {
                f.write_str("the path needs the relay, but the session has none configured")
            }
            Self::RelayTimeout => f.write_str("the relay did not answer in time"),
            Self::RequestExpired => {
                f.write_str("the relay connect request expired before the peer met it")
            }
            Self::Relay(error) => write!(f, "the relay pairing failed: {error}"),
            Self::RelayTls(error) => write!(f, "{error}"),
            Self::Exhausted => f.write_str("the mesh path exhausted its retry budget"),
            Self::Frame(error) => write!(f, "{error}"),
            Self::Stalled(state) => {
                write!(
                    f,
                    "the mesh path stalled in {state:?} with no action to take"
                )
            }
        }
    }
}

impl std::error::Error for SessionError {}

impl From<RendezvousError> for SessionError {
    fn from(error: RendezvousError) -> Self {
        Self::Rendezvous(error)
    }
}

impl From<EndpointError> for SessionError {
    fn from(error: EndpointError) -> Self {
        Self::Endpoint(error)
    }
}

impl From<RelayError> for SessionError {
    fn from(error: RelayError) -> Self {
        Self::Relay(error)
    }
}

impl From<RelayTlsError> for SessionError {
    fn from(error: RelayTlsError) -> Self {
        Self::RelayTls(error)
    }
}

impl From<FrameError> for SessionError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

/// A joined mesh session.
///
/// One session owns one path at a time: the peer most recently passed to
/// [`MeshSession::connect`]. Connecting to a different peer starts a fresh
/// path; connecting to the same peer again reports the previous channel as
/// closed, so the retry count and backoff are `Path`'s.
pub struct MeshSession {
    identity: Identity,
    path: Path,
    endpoint: MeshEndpoint,
    /// The client that signs and publishes this session's registration.
    registration: Rendezvous,
    broker: String,
    relay: Option<SocketAddr>,
    punch_timeout_ms: u64,
    lookup_timeout_ms: u64,
    /// What the broker currently holds for this session, served locally too.
    candidates: Vec<Candidate>,
    /// The peer `path` describes, if any.
    path_peer: Option<PublicKey>,
    /// Aborted when the session is dropped.
    refresh: JoinHandle<()>,
}

impl fmt::Debug for MeshSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MeshSession")
            .field("identity", &self.identity.public_hex())
            .field("path", &self.path.state())
            .field("broker", &self.broker)
            .field("relay", &self.relay)
            .field("candidates", &self.candidates)
            .finish_non_exhaustive()
    }
}

impl Drop for MeshSession {
    fn drop(&mut self) {
        self.refresh.abort();
    }
}

impl MeshSession {
    /// Join the mesh: bind the endpoint, publish the candidates, and keep the
    /// registration fresh in the background.
    ///
    /// The join fails if the endpoint cannot bind or the broker refuses the
    /// first registration. A failed welcome is not retried behind the caller's
    /// back: without a registration this session cannot be found.
    ///
    /// The published candidates are the bound UDP address when it is
    /// specified, plus the address the configured STUN reflector saw when it
    /// differs. An unspecified bind is never published: with a reflector its
    /// reflection is the candidate, and a silent reflector falls back to the
    /// local address the kernel would use to reach it. Without a reflector, a
    /// wildcard bind publishes nothing. The reflector is asked on the same
    /// socket quinn takes over, and one that does not answer is logged and
    /// skipped, never fatal. See the module docs.
    pub async fn join(identity: Identity, config: MeshConfig) -> Result<Self, SessionError> {
        Self::join_with_ttl(
            identity,
            config,
            REGISTRATION_TTL_MS,
            REGISTRATION_REFRESH_MS,
        )
        .await
    }

    /// Join with an explicit registration ttl and refresh period.
    ///
    /// [`MeshSession::join`] uses [`REGISTRATION_TTL_MS`] and
    /// [`REGISTRATION_REFRESH_MS`]. A deployment whose NAT mapping is short
    /// lived can publish a shorter ttl; the refresh task re-signs every
    /// `refresh_ms`, so `refresh_ms` must be shorter than `ttl_ms` or the
    /// registration lapses between refreshes.
    pub async fn join_with_ttl(
        identity: Identity,
        config: MeshConfig,
        ttl_ms: u64,
        refresh_ms: u64,
    ) -> Result<Self, SessionError> {
        // A server endpoint also dials: quinn's `connect_with` takes the client
        // configuration per connection, so one UDP socket both accepts the
        // direct handshake and opens the outbound one the role chooses.
        //
        // The socket is bound here, not inside `MeshEndpoint::server`, because
        // the STUN exchange must run on the very socket quinn will own: a NAT
        // mapping belongs to a 5-tuple, and a reflection asked on any other
        // socket names a port QUIC is not listening on.
        let socket = std::net::UdpSocket::bind(config.bind).map_err(EndpointError::from)?;
        // The exchange below is blocking; a freshly bound socket is already in
        // blocking mode, but say so instead of relying on a default.
        socket.set_nonblocking(false).map_err(EndpointError::from)?;
        let bound = socket.local_addr().map_err(EndpointError::from)?;
        // An unspecified address is not a candidate: no peer can dial
        // `0.0.0.0` or `[::]`. A specified bind is the first candidate.
        let unspecified = bound.ip().is_unspecified();
        let mut candidates = Vec::new();
        if !unspecified {
            candidates.push(Candidate::new(
                CandidateKind::Udp,
                bound.to_string(),
                rendezvous::now_ms(),
            ));
        }
        if let Some(server) = config.stun_server {
            let reflected = match crate::stun::query_blocking(&socket, server, STUN_TIMEOUT) {
                Ok(reflected) => Some(reflected),
                // A dead or silent reflector costs the direct path, not the
                // session. One line, then the best local candidate we have.
                Err(error) => {
                    eprintln!(
                        "summrise-mesh: the STUN server {server} did not answer ({error}); \
                         continuing without a reflection"
                    );
                    None
                }
            };
            match reflected {
                Some(reflected) if !reflected.ip().is_unspecified() && reflected != bound => {
                    candidates.push(Candidate::new(
                        CandidateKind::Udp,
                        reflected.to_string(),
                        rendezvous::now_ms(),
                    ));
                }
                Some(reflected) if reflected.ip().is_unspecified() => {
                    eprintln!(
                        "summrise-mesh: the STUN server {server} reported an unspecified \
                         address; ignoring it"
                    );
                    if unspecified {
                        push_local_fallback(&mut candidates, bound, server);
                    }
                }
                // A reflection equal to a specified bound address says nothing
                // new; an unspecified bind never equals a usable reflection.
                Some(_) => {}
                None if unspecified => push_local_fallback(&mut candidates, bound, server),
                None => {}
            }
        } else if unspecified {
            eprintln!(
                "summrise-mesh: the bind address {bound} is unspecified and no STUN server is \
                 configured; publishing no candidates (the relay is the floor)"
            );
        }
        let endpoint = MeshEndpoint::server_with_socket(identity.clone(), socket)?;
        let broker = config.broker.to_string();
        let registration = Rendezvous::new(identity.clone(), candidates.clone(), ttl_ms)
            .with_timeout(Duration::from_millis(config.lookup_timeout_ms.max(1)));
        registration.register(&broker).await?;

        // The refresh is a task, not a hook on every call: a session that is
        // idle must stay findable. It re-signs (a fresh `issued_at_ms`), so the
        // broker's ttl is measured from the last refresh, and a failed refresh
        // is retried on the next tick instead of ending the session.
        let refresh_identity = identity.clone();
        let refresh_candidates = candidates.clone();
        let refresh_broker = broker.clone();
        let period = Duration::from_millis(refresh_ms.max(1));
        let refresh = tokio::spawn(async move {
            let registration = Rendezvous::new(refresh_identity, refresh_candidates, ttl_ms)
                .with_timeout(Duration::from_millis(config.lookup_timeout_ms.max(1)));
            loop {
                tokio::time::sleep(period).await;
                let _ = registration.register(&refresh_broker).await;
            }
        });

        Ok(Self {
            identity,
            path: Path::new(config.punch_timeout_ms, STABLE_WINDOW_MS),
            endpoint,
            registration,
            broker,
            relay: config.relay,
            punch_timeout_ms: config.punch_timeout_ms,
            lookup_timeout_ms: config.lookup_timeout_ms,
            candidates,
            path_peer: None,
            refresh,
        })
    }

    /// Reach `peer` and return a channel over the path `Path` chose.
    ///
    /// `now_ms` is the caller's instant, the same clock `Path` takes. The
    /// session returns:
    ///
    /// - [`SessionError::UnknownPeer`] when the broker's lookup is a 404
    ///   (never a hang), before any path is spent;
    /// - [`SessionError::RelayUnavailable`] when direct failed and no relay is
    ///   configured;
    /// - [`SessionError::RequestExpired`] when the relay was needed but no
    ///   serving peer collected the connect request within its ttl;
    /// - [`SessionError::Exhausted`] when `Path` has no attempts left.
    ///
    /// A second connect to the same peer reports the channel it supersedes as
    /// closed, so `Path` counts the retry and waits its backoff. Because
    /// `now_ms` is the caller's clock, a caller that comes back 61 s later
    /// makes that close look stable and the retry count starts over — the same
    /// rule `core` proves with a fake clock.
    ///
    /// A connect future that is dropped while its punch is still running does
    /// not wedge the peer: the next connect to the same peer supersedes the
    /// abandoned attempt by rebuilding its path, and charges no retry for a
    /// call the caller withdrew. A relay fallback that never opened is counted
    /// as a [`PathEvent::RelayClosed`] before the error is returned, so a dead
    /// relay consumes the retry budget instead of looking like a path that was
    /// up for a minute.
    pub async fn connect(&mut self, peer: &PeerId, now_ms: u64) -> Result<Channel, SessionError> {
        let peer_key = Identity::public_from_hex(peer)
            .map_err(|_| SessionError::BadPeerId(peer.to_owned()))?;
        if peer_key == self.identity.public_bytes() {
            return Err(SessionError::SamePeer);
        }
        let peer_id = keys::hex_encode(&peer_key);
        let clock = Clock::new(now_ms);

        // A path belongs to one peer. A different peer starts from Idle; the
        // same peer resumes its path, and the channel this call supersedes is
        // reported closed so `Path` owns the retry count and the backoff.
        let same_peer = self.path_peer == Some(peer_key);
        if !same_peer {
            self.path = Path::new(self.punch_timeout_ms, STABLE_WINDOW_MS);
            self.path_peer = Some(peer_key);
        }
        if self.path.state() == PathState::Closed {
            return Err(SessionError::Exhausted);
        }

        // The instant `Path` armed its rendezvous deadline, turned into the
        // deadline itself. `Path` exposes no deadline accessor, but its
        // `begin_rendezvous` is exactly `at + punch_timeout_ms`; recording the
        // `at` passed to `Start` lets the lookup use the path's own deadline
        // instead of re-deriving one from a later clock read.
        let mut rendezvous_deadline_ms: Option<u64> = None;
        let mut actions: VecDeque<PathAction> = VecDeque::new();
        if same_peer {
            // The entry handling is total over the states a previous connect
            // can leave behind. A `Direct` or `Relayed` path is reported
            // closed, so `Path` counts the retry and owns the backoff. A
            // `Punching` path is a *cancelled* attempt: the previous connect
            // future was dropped while its punch was still running, so it
            // never reached a decision. Rebuilding the path is how the next
            // connect supersedes it — `PunchFailed` would also leave Punching,
            // but it would order a relay the caller never asked for and spend
            // one of six retries on a call that was withdrawn. The abandoned
            // attempt's deadline is discarded with it; a fresh rendezvous
            // timer starts below.
            match self.path.state() {
                PathState::Direct => {
                    let produced = self.path.apply(PathEvent::DirectClosed, clock.now());
                    actions.extend(produced);
                }
                PathState::Relayed => {
                    let produced = self.path.apply(PathEvent::RelayClosed, clock.now());
                    actions.extend(produced);
                }
                PathState::Punching => {
                    self.path = Path::new(self.punch_timeout_ms, STABLE_WINDOW_MS);
                }
                // Idle, Rendezvous (a retry waiting for Start) and Closed (a
                // GiveUp with no actions left) are already in the state the
                // logic below handles.
                PathState::Idle | PathState::Rendezvous | PathState::Closed => {}
            }
        }
        // No close means the first connect (Idle), a retry after a failed
        // lookup (Rendezvous), or a superseded punch (rebuilt above); Start
        // arms the rendezvous. A close that answered Retry waits the backoff
        // before that Start.
        if actions.is_empty() {
            let started_at = clock.now();
            let produced = self.path.apply(PathEvent::Start, started_at);
            if produced.contains(&PathAction::RequestRendezvous) {
                rendezvous_deadline_ms = Some(started_at.saturating_add(self.punch_timeout_ms));
            }
            actions.extend(produced);
        }

        let mut candidates: Vec<SocketAddr> = Vec::new();
        let mut direct: Option<PeerSession> = None;

        while let Some(action) = actions.pop_front() {
            match action {
                PathAction::RequestRendezvous => {
                    let now = clock.now();
                    // The deadline `Path` armed when its `Start` produced this
                    // action. A lookup is bounded by it; it never outlives the
                    // fallback it would trigger.
                    let deadline_ms = rendezvous_deadline_ms
                        .take()
                        .unwrap_or_else(|| now.saturating_add(self.punch_timeout_ms));
                    match self.lookup(&peer_id, deadline_ms, now).await? {
                        Lookup::Candidates(found) => {
                            candidates = found;
                            let produced = self
                                .path
                                .apply(PathEvent::Candidates { fresh: true }, clock.now());
                            actions.extend(produced);
                        }
                        Lookup::TimedOut => {
                            // The lookup ran out the path's own deadline, so
                            // this tick names an instant that really happened.
                            let ticked_at = clock.now();
                            let produced = self
                                .path
                                .apply(PathEvent::Tick { now_ms: ticked_at }, ticked_at);
                            actions.extend(produced);
                        }
                    }
                }
                PathAction::StartPunch { deadline_ms } => {
                    let now = clock.now();
                    if candidates.is_empty() {
                        let produced = self.path.apply(PathEvent::PunchFailed, now);
                        actions.extend(produced);
                        continue;
                    }
                    match self
                        .try_direct(&peer_key, &peer_id, &candidates, deadline_ms, &clock)
                        .await
                    {
                        DirectAttempt::Open(session) => {
                            direct = Some(session);
                            let produced = self.path.apply(PathEvent::PunchOpen, clock.now());
                            actions.extend(produced);
                        }
                        DirectAttempt::Failed => {
                            let produced = self.path.apply(PathEvent::PunchFailed, clock.now());
                            actions.extend(produced);
                        }
                        DirectAttempt::TimedOut => {
                            let produced = self.path.apply(
                                PathEvent::Tick {
                                    now_ms: deadline_ms,
                                },
                                deadline_ms,
                            );
                            actions.extend(produced);
                        }
                    }
                }
                PathAction::UseDirect => {
                    let session = direct
                        .take()
                        .ok_or(SessionError::Stalled(self.path.state()))?;
                    return Ok(Channel {
                        peer: peer_key,
                        peer_id,
                        transport: Transport::Direct(session),
                    });
                }
                PathAction::UseRelay { .. } => {
                    let result = self.relay_channel(&peer_key, &peer_id).await;
                    match result {
                        Ok(channel) => return Ok(channel),
                        Err(error) => {
                            // `Path` moved to Relayed when it *ordered* the
                            // fallback, but the relay never opened: no path
                            // carried a frame. Count it as a close now, while
                            // `connected_at_ms` is still the ordering instant,
                            // so a caller that retries a minute later cannot
                            // make a dead relay look like a stable path and
                            // reset the retry budget. The produced Retry (or
                            // GiveUp) is dropped: this connect is already
                            // returning the concrete relay error, and the next
                            // connect reads the state this left.
                            let _ = self.path.apply(PathEvent::RelayClosed, clock.now());
                            return Err(error);
                        }
                    }
                }
                PathAction::Retry { after_ms, .. } => {
                    // The wait belongs to the caller in `Path`'s contract; here
                    // the caller is this session, and it then sends Start at
                    // the instant it actually resumes.
                    tokio::time::sleep(Duration::from_millis(after_ms)).await;
                    let started_at = clock.now();
                    let produced = self.path.apply(PathEvent::Start, started_at);
                    if produced.contains(&PathAction::RequestRendezvous) {
                        rendezvous_deadline_ms =
                            Some(started_at.saturating_add(self.punch_timeout_ms));
                    }
                    actions.extend(produced);
                }
                PathAction::GiveUp { .. } => return Err(SessionError::Exhausted),
            }
        }

        Err(SessionError::Stalled(self.path.state()))
    }

    /// The state of the path to the peer most recently passed to
    /// [`MeshSession::connect`].
    ///
    /// [`PathState::Idle`] before the first connect.
    pub fn path(&self) -> PathState {
        self.path.state()
    }

    /// Serve incoming connections until the endpoint closes.
    ///
    /// This is the passive half of the session. [`MeshSession::connect`] needs
    /// both peers to be looking at the same moment; `serve` lets an idle
    /// session accept the connection a dialing peer opens, and meet a dialing
    /// peer at the relay when the direct path is dead.
    ///
    /// The direct accept loop takes one incoming connection at a time and
    /// authenticates it in its own task, so a stranger that completes the QUIC
    /// handshake and then sends nothing cannot hold the loop: it is closed at
    /// `handshake_timeout` and the next connection is authenticated
    /// independently. `handshake_timeout` covers the whole authentication: the
    /// QUIC handshake, the challenge this side issues, the hello it reads back,
    /// and the challenge and signature checks. A connection that fails any of
    /// them is closed and does not end the loop.
    ///
    /// If the session has a relay, a second task polls the broker's
    /// connect-request inbox every [`REQUEST_POLL_INTERVAL`]. Each request that
    /// names this peer is paired on the relay under its `session_id` and
    /// authenticated by [`RelayTlsSession::for_peer`] in its own task, so one
    /// slow pairing does not stop the accept loop or the next poll. A request
    /// this session refuses — its own key, a request that no longer verifies,
    /// a relay that refuses the session — is dropped without ending either
    /// loop.
    ///
    /// On success `handler` is called with the peer's canonical hex id and the
    /// authenticated [`Channel`], and its future is driven to completion in
    /// that connection's task. The channel is direct when the peer dialled,
    /// relayed when the peer arrived through the inbox; the handler does not
    /// branch on which. The loop keeps running as long as this future is
    /// polled; the caller stops serving by aborting the task, and an endpoint
    /// closed underneath the loop ends it with `Ok(())`. Handlers already
    /// running are their own tasks and are not cancelled with it.
    pub async fn serve<H, Fut>(
        &self,
        handshake_timeout: Duration,
        handler: H,
    ) -> Result<(), SessionError>
    where
        H: Fn(String, Channel) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let handler = Arc::new(handler);
        // The relay inbox is drained in its own task: a broker poll must not
        // hold the direct accept loop. The guard aborts the poller when this
        // future ends or is dropped, so an aborted `serve` does not leave a
        // task polling the broker forever.
        let _relay_poller = self.spawn_relay_poller(Arc::clone(&handler));
        loop {
            let incoming = match self.endpoint.accept_incoming().await {
                Ok(incoming) => incoming,
                // The endpoint is closed: dropped, or closed by its owner.
                Err(EndpointError::Closed) => return Ok(()),
                Err(error) => return Err(SessionError::Endpoint(error)),
            };
            let endpoint = self.endpoint.clone();
            let handler = Arc::clone(&handler);
            tokio::spawn(async move {
                let session = match endpoint
                    .accept_authenticated_within(incoming, &PeerKeyResolver, handshake_timeout)
                    .await
                {
                    Ok(session) => session,
                    // A stalled connection, an unknown key and a bad hello
                    // are all closed by the endpoint. One refusal is not the
                    // end of the accept loop.
                    Err(_) => return,
                };
                let peer = session.peer();
                let peer_id = keys::hex_encode(&peer);
                let channel = Channel {
                    peer,
                    peer_id: peer_id.clone(),
                    transport: Transport::Direct(session),
                };
                handler(peer_id, channel).await;
            });
        }
    }
}

/// The session's pin resolver: a mesh peer id **is** its public key.
///
/// There is no separate directory for a passive session: a hello that names a
/// key is verified against that same key. A peer id that is not an Ed25519
/// public key in hex resolves to nothing and is refused.
struct PeerKeyResolver;

impl PinResolver for PeerKeyResolver {
    fn resolve(&self, peer_id: &str) -> Option<PublicKey> {
        Identity::public_from_hex(peer_id).ok()
    }
}

/// A live frame channel to one peer, direct or relayed.
pub struct Channel {
    peer: PublicKey,
    peer_id: String,
    transport: Transport,
}

enum Transport {
    Direct(PeerSession),
    Relayed(RelayTlsSession),
}

impl fmt::Debug for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Channel")
            .field("peer", &keys::hex_encode(&self.peer))
            .field(
                "transport",
                &match &self.transport {
                    Transport::Direct(_) => "direct",
                    Transport::Relayed(_) => "relayed",
                },
            )
            .finish_non_exhaustive()
    }
}

impl Channel {
    /// The peer's pinned public key.
    pub fn peer(&self) -> PublicKey {
        self.peer
    }

    /// The peer's rendezvous name: its public key in hex.
    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }

    /// Whether this channel runs peer to peer (not through the relay).
    pub fn is_direct(&self) -> bool {
        matches!(&self.transport, Transport::Direct(_))
    }

    /// Send one frame.
    pub async fn send_frame(&mut self, frame: &Frame) -> Result<(), SessionError> {
        match &mut self.transport {
            Transport::Direct(session) => frame::write(session.send(), frame)
                .await
                .map_err(SessionError::from),
            Transport::Relayed(session) => {
                session.send_frame(frame).await.map_err(SessionError::from)
            }
        }
    }

    /// Read one frame.
    pub async fn recv_frame(&mut self) -> Result<Frame, SessionError> {
        match &mut self.transport {
            Transport::Direct(session) => frame::read(session.recv())
                .await
                .map_err(SessionError::from),
            Transport::Relayed(session) => session.recv_frame().await.map_err(SessionError::from),
        }
    }
}

/// The session's view of the caller's clock: the supplied instant, advanced by
/// the monotonic time the session actually spent waiting.
///
/// It is only ever compared to the deadlines `Path` handed back. It never turns
/// elapsed time into a decision of its own.
struct Clock {
    base_ms: u64,
    started: Instant,
}

impl Clock {
    fn new(base_ms: u64) -> Self {
        Self {
            base_ms,
            started: Instant::now(),
        }
    }

    fn now(&self) -> u64 {
        self.base_ms
            .saturating_add(u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX))
    }
}

/// What the rendezvous lookup produced before the path's deadline.
enum Lookup {
    Candidates(Vec<SocketAddr>),
    TimedOut,
}

/// What one direct attempt produced.
enum DirectAttempt {
    Open(PeerSession),
    Failed,
    TimedOut,
}

impl MeshSession {
    /// Ask the broker where the peer said it was, bounded by the deadline
    /// `Path` armed for the rendezvous.
    async fn lookup(
        &self,
        peer_id: &str,
        deadline_ms: u64,
        now: u64,
    ) -> Result<Lookup, SessionError> {
        let remaining = deadline_ms.saturating_sub(now);
        if remaining == 0 {
            // The path's rendezvous deadline is already due.
            return Ok(Lookup::TimedOut);
        }
        // Two bounds, and the smaller wins. The path's remaining time is the
        // fallback clock; the configured exchange timeout is the transport's
        // patience. Only the former is reachable as a `TimedOut` tick — a
        // transport timeout must not claim a path deadline that has not
        // passed, so it stays a typed error.
        let lookup_ms = remaining.min(self.lookup_timeout_ms.max(1));
        let at_path_deadline = lookup_ms == remaining;
        let answer = tokio::time::timeout(
            Duration::from_millis(lookup_ms),
            self.registration.lookup(&self.broker, peer_id),
        )
        .await;
        match answer {
            Err(_) if at_path_deadline => Ok(Lookup::TimedOut),
            Err(_) => Err(SessionError::Rendezvous(RendezvousError::Timeout)),
            Ok(Err(RendezvousError::UnknownPeer(peer))) => Err(SessionError::UnknownPeer(peer)),
            Ok(Err(RendezvousError::Timeout)) if at_path_deadline => Ok(Lookup::TimedOut),
            Ok(Err(RendezvousError::Timeout)) => {
                Err(SessionError::Rendezvous(RendezvousError::Timeout))
            }
            Ok(Err(error)) => Err(SessionError::Rendezvous(error)),
            Ok(Ok(candidates)) => Ok(Lookup::Candidates(
                candidates
                    .into_iter()
                    .filter(|candidate| candidate.kind == CandidateKind::Udp)
                    .filter_map(|candidate| candidate.addr.parse().ok())
                    .collect(),
            )),
        }
    }

    /// Try the direct path, under the deadline `Path` set.
    ///
    /// The role is a function of the two keys, not of who called first: the
    /// higher key is the QUIC/TLS client and dials the candidates; the lower
    /// key is the server and accepts, verifying the dialer's hello.
    async fn try_direct(
        &self,
        peer_key: &PublicKey,
        peer_id: &str,
        candidates: &[SocketAddr],
        deadline_ms: u64,
        clock: &Clock,
    ) -> DirectAttempt {
        if self.identity.public_bytes() > *peer_key {
            self.dial(peer_key, candidates, deadline_ms, clock).await
        } else {
            self.accept_direct(peer_key, peer_id, deadline_ms, clock)
                .await
        }
    }

    /// Dial each candidate in the peer's published order.
    ///
    /// The punch deadline is split across the candidates still to try: an
    /// equal share each, never less than [`MIN_CANDIDATE_SLICE_MS`]. A
    /// blackholed address spends its own slice and no more, so the live
    /// candidate after it still gets a chance to answer. Every candidate is
    /// tried even when the floor has carried the attempt past the path's
    /// deadline; only after the last slice does the session report a timeout.
    /// A fast refusal of every candidate is `Failed`.
    async fn dial(
        &self,
        peer_key: &PublicKey,
        candidates: &[SocketAddr],
        deadline_ms: u64,
        clock: &Clock,
    ) -> DirectAttempt {
        let my_id = self.identity.public_hex();
        let total = candidates.len();
        if deadline_ms.saturating_sub(clock.now()) == 0 {
            return DirectAttempt::TimedOut;
        }
        for (index, candidate) in candidates.iter().enumerate() {
            let remaining_ms = deadline_ms.saturating_sub(clock.now());
            let candidates_left = total - index;
            let slice_ms = candidate_slice_ms(remaining_ms, candidates_left);
            let attempt = tokio::time::timeout(
                Duration::from_millis(slice_ms),
                self.endpoint
                    .connect_authenticated(*candidate, Some(*peer_key), &my_id),
            )
            .await;
            match attempt {
                Ok(Ok(session)) => return DirectAttempt::Open(session),
                // This candidate's slice expired. Move to the next one; the
                // whole punch has not timed out until every slice is spent.
                Err(_) => continue,
                Ok(Err(_)) => continue,
            }
        }
        if clock.now() >= deadline_ms {
            DirectAttempt::TimedOut
        } else {
            DirectAttempt::Failed
        }
    }

    /// Wait for the dialer's authenticated QUIC session, under the same
    /// deadline. A refused stranger does not end the wait for the real peer.
    async fn accept_direct(
        &self,
        peer_key: &PublicKey,
        peer_id: &str,
        deadline_ms: u64,
        clock: &Clock,
    ) -> DirectAttempt {
        let resolver = HashMap::from([(peer_id.to_owned(), *peer_key)]);
        loop {
            let remaining = deadline_ms.saturating_sub(clock.now());
            if remaining == 0 {
                return DirectAttempt::TimedOut;
            }
            let attempt = tokio::time::timeout(
                Duration::from_millis(remaining),
                self.endpoint.accept_authenticated(&resolver),
            )
            .await;
            match attempt {
                Err(_) => return DirectAttempt::TimedOut,
                Ok(Ok(session)) => {
                    if session.peer() == *peer_key && session.hello().peer_id_hex == peer_id {
                        return DirectAttempt::Open(session);
                    }
                }
                // A closed endpoint cannot accept again; yielding to the
                // path turns it into the fallback instead of a busy loop.
                Ok(Err(EndpointError::Closed)) => return DirectAttempt::Failed,
                Ok(Err(_)) => continue,
            }
        }
    }

    /// Pair through the relay and run TLS inside the splice.
    ///
    /// The relay is the floor: there is no fallback below it. Before dialling,
    /// this leaves a signed connect request at the broker naming the pair's
    /// relay session, so a serving peer that is not in `connect` can learn
    /// where to meet. The request's ttl is this side's patience for that
    /// meeting: if it passes with no serving peer, the result is
    /// [`SessionError::RequestExpired`], not a wait on the relay's own, longer
    /// pair timeout. The TLS roles are computed by
    /// [`RelayTlsSession::for_peer`] from the two keys, the same rule the
    /// direct attempt uses.
    async fn relay_channel(
        &self,
        peer_key: &PublicKey,
        peer_id: &str,
    ) -> Result<Channel, SessionError> {
        let relay = self.relay.ok_or(SessionError::RelayUnavailable)?;
        let session_id = relay_session_id(&self.identity.public_bytes(), peer_key);

        // Publish the meeting before joining the relay: the serving peer
        // learns the session id only from this request, so the note must exist
        // before the dialer starts waiting for a counterpart.
        let request = match self
            .registration
            .request(&self.broker, peer_id, &session_id, REQUEST_TTL_MS)
            .await
        {
            Ok(request) => request,
            Err(RendezvousError::UnknownPeer(peer)) => {
                return Err(SessionError::UnknownPeer(peer));
            }
            Err(error) => return Err(SessionError::Rendezvous(error)),
        };
        let patience = Duration::from_millis(request.ttl_ms).min(RELAY_PATIENCE);

        let stream = tokio::time::timeout(patience, RelayClient::connect(relay, &session_id))
            .await
            .map_err(|_| SessionError::RequestExpired)?
            .map_err(SessionError::from)?
            .into_inner();

        // The server side of `for_peer` resolves the client's hex key; both
        // sessions already know that key, so one entry is the whole directory.
        let resolver = HashMap::from([(peer_id.to_owned(), *peer_key)]);
        let session = tokio::time::timeout(
            RELAY_PATIENCE,
            RelayTlsSession::for_peer(&self.identity, peer_key, stream, &resolver),
        )
        .await
        .map_err(|_| SessionError::RelayTimeout)?
        .map_err(SessionError::from)?;

        Ok(Channel {
            peer: *peer_key,
            peer_id: peer_id.to_owned(),
            transport: Transport::Relayed(session),
        })
    }

    /// Start the relay inbox poller for [`MeshSession::serve`].
    ///
    /// The task owns a copy of the rendezvous client, the identity and the
    /// handler, so it does not borrow the session. `None` when the session has
    /// no relay: a request would have nowhere to meet. The returned guard
    /// aborts the task when it is dropped.
    fn spawn_relay_poller<H, Fut>(&self, handler: Arc<H>) -> Option<AbortOnDrop>
    where
        H: Fn(String, Channel) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let relay = self.relay?;
        let registration = self.registration.clone();
        let broker = self.broker.clone();
        let identity = self.identity.clone();
        let my_id = identity.public_hex();
        let my_key = identity.public_bytes();
        let task = tokio::spawn(async move {
            let mut ticks = tokio::time::interval(REQUEST_POLL_INTERVAL);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticks.tick().await;
                let requests = match registration.requests(&broker, &my_id).await {
                    Ok(requests) => requests,
                    // A broker that is unreachable, or that has forgotten this
                    // session, is retried on the next tick: an idle device must
                    // keep accepting direct connections even when the inbox
                    // cannot be read.
                    Err(_) => continue,
                };
                for request in requests {
                    let Some(requester) = servable_requester(&request, &my_id, &my_key) else {
                        // A request for another target, one that no longer
                        // verifies, or our own key: refuse it, keep polling.
                        continue;
                    };
                    let handler = Arc::clone(&handler);
                    let identity = identity.clone();
                    tokio::spawn(async move {
                        if let Some(channel) =
                            meet_at_relay(&identity, relay, requester, request).await
                        {
                            let peer_id = channel.peer_id().to_owned();
                            handler(peer_id, channel).await;
                        }
                    });
                }
            }
        });
        Some(AbortOnDrop(task))
    }
}

/// Aborts a spawned task when dropped.
///
/// A `serve` future can be aborted by its caller, and dropping it must stop the
/// relay poller that it started: the poller holds the handler and would
/// otherwise keep asking the broker for work for the life of the runtime.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The requester key a connect request names, when this session can serve it.
///
/// `None` is a refusal: the request names another target, it names this
/// session's own key, or its signature does not verify. A refused request is
/// dropped; it never ends the poll or the accept loop.
fn servable_requester(
    request: &ConnectRequest,
    my_id: &str,
    my_key: &PublicKey,
) -> Option<PublicKey> {
    if request.target_id != my_id {
        return None;
    }
    let requester = Identity::public_from_hex(&request.requester_id).ok()?;
    if requester == *my_key {
        return None;
    }
    request.verify_self().ok()?;
    Some(requester)
}

/// Meet one connect request at the relay and authenticate the requester.
///
/// The request's `session_id` is the relay's pairing name; the TLS roles and
/// the pinned key come from [`RelayTlsSession::for_peer`], exactly as on the
/// dialing side. Every failure — a relay that does not pair in time, a bad
/// session id, a TLS or hello refusal — is `None`: the request is spent and the
/// serving loop keeps going.
async fn meet_at_relay(
    identity: &Identity,
    relay: SocketAddr,
    requester: PublicKey,
    request: ConnectRequest,
) -> Option<Channel> {
    let ConnectRequest {
        requester_id,
        session_id,
        ..
    } = request;
    let resolver = HashMap::from([(requester_id.clone(), requester)]);
    let stream = tokio::time::timeout(RELAY_PATIENCE, RelayClient::connect(relay, &session_id))
        .await
        .ok()?
        .ok()?
        .into_inner();
    let session = tokio::time::timeout(
        RELAY_PATIENCE,
        RelayTlsSession::for_peer(identity, &requester, stream, &resolver),
    )
    .await
    .ok()?
    .ok()?;
    Some(Channel {
        peer: requester,
        peer_id: requester_id,
        transport: Transport::Relayed(session),
    })
}

/// The relay session id for a pair: the two public keys in hex, lower key first.
///
/// The relay caps a session id at 128 bytes and two 64-character keys
/// concatenated are exactly that, so the pair names the session without a
/// truncation or a collision-prone hash. Both peers compute the same id from
/// the same two keys, whichever argument order they pass.
fn relay_session_id(one: &PublicKey, other: &PublicKey) -> String {
    let (lower, higher) = if one <= other {
        (one, other)
    } else {
        (other, one)
    };
    format!("{}{}", keys::hex_encode(lower), keys::hex_encode(higher))
}

/// How long one candidate gets out of the punch budget still remaining.
///
/// Equal shares, with a [`MIN_CANDIDATE_SLICE_MS`] floor. The floor is not
/// capped by the remaining budget: when the path's punch timeout is shorter
/// than one second per candidate, the attempt runs past it rather than giving
/// a candidate less time than a QUIC handshake may need. The caller reports
/// the deadline to `Path` only after the last slice.
fn candidate_slice_ms(remaining_ms: u64, candidates_left: usize) -> u64 {
    let left = u64::try_from(candidates_left).unwrap_or(u64::MAX).max(1);
    (remaining_ms / left).max(MIN_CANDIDATE_SLICE_MS)
}

/// Append the local address the kernel would use to reach `server`, carrying
/// the QUIC socket's own port.
///
/// This is the fallback for a wildcard bind whose reflector did not answer:
/// the bound address is `0.0.0.0` and cannot be dialled, but the route to the
/// reflector still names the interface a peer on the same network can use. A
/// failure to discover one is one line, and the relay remains the floor.
fn push_local_fallback(candidates: &mut Vec<Candidate>, bound: SocketAddr, server: SocketAddr) {
    match local_candidate(bound, server) {
        Some(local) => candidates.push(Candidate::new(
            CandidateKind::Udp,
            local.to_string(),
            rendezvous::now_ms(),
        )),
        None => eprintln!(
            "summrise-mesh: could not discover a local address towards the STUN server {server}; \
             publishing no candidates (the relay is the floor)"
        ),
    }
}

/// The local address a route to `server` would use, with `bound`'s port.
///
/// The QUIC socket itself is not connected: connecting it would make quinn's
/// destination-addressed sends fail on some platforms. A throwaway UDP socket
/// on the same address family performs the route lookup; its source IP is
/// combined with the bound port of the socket quinn will own.
fn local_candidate(bound: SocketAddr, server: SocketAddr) -> Option<SocketAddr> {
    // A route lookup across families says nothing about the socket's family.
    if bound.is_ipv4() != server.is_ipv4() {
        return None;
    }
    let wildcard: SocketAddr = if server.is_ipv4() {
        "0.0.0.0:0".parse().expect("a static wildcard address")
    } else {
        "[::]:0".parse().expect("a static wildcard address")
    };
    let probe = std::net::UdpSocket::bind(wildcard).ok()?;
    probe.connect(server).ok()?;
    let local = probe.local_addr().ok()?;
    if local.ip().is_unspecified() {
        return None;
    }
    Some(SocketAddr::new(local.ip(), bound.port()))
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, UdpSocket};
    use tokio::sync::mpsc;
    use tokio::task::JoinHandle;
    use tokio::time::timeout;

    use super::*;
    use crate::broker::Broker;
    use crate::frame::{Frame, Kind};
    use crate::hello::{Challenge, Hello};
    use crate::relay::RelayServer;
    use crate::rendezvous::{now_ms, Candidate, CandidateKind};

    const PATIENCE: Duration = Duration::from_secs(10);
    const DIRECT_TIMEOUT_MS: u64 = 2_000;
    /// Long enough for a direct attempt to fail, short enough to keep the
    /// fallback test fast.
    const FALLBACK_TIMEOUT_MS: u64 = 300;
    /// The handshake deadline a served session uses in these tests. It is the
    /// production-shaped value, not a tiny test one, because the point of the
    /// stalled-connection test is that the real peer gets through well inside
    /// the window a device would actually use.
    const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

    fn loopback() -> SocketAddr {
        "127.0.0.1:0".parse().expect("a loopback address")
    }

    /// Start a real broker on an ephemeral loopback port.
    async fn start_broker() -> String {
        let listener = TcpListener::bind(loopback())
            .await
            .expect("a loopback listener");
        let address = listener.local_addr().expect("the bound address");
        tokio::spawn(async move {
            axum::serve(listener, Broker::new().router())
                .await
                .expect("the test broker serves");
        });
        format!("http://{address}")
    }

    /// A broker that answers `POST /register` (so a session can join) and
    /// never answers anything else. It holds the connection open, so a lookup
    /// times out at its transport bound instead of being refused.
    async fn start_stalling_broker() -> String {
        let listener = TcpListener::bind(loopback())
            .await
            .expect("a loopback listener");
        let address = listener.local_addr().expect("the bound address");
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    continue;
                };
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 4096];
                    let header_end = loop {
                        let read = match stream.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => read,
                        };
                        request.extend_from_slice(&buffer[..read]);
                        if let Some(end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            break end + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
                    let content_length = head
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    while request.len() < header_end + content_length {
                        let read = match stream.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => read,
                        };
                        request.extend_from_slice(&buffer[..read]);
                    }
                    if request.starts_with(b"POST /register ") {
                        let _ = stream
                            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                            .await;
                    } else {
                        // Never answer. Holding the stream open is what makes
                        // the client's own bound the only way out.
                        tokio::time::sleep(Duration::from_secs(60)).await;
                    }
                });
            }
        });
        format!("http://{address}")
    }

    /// Start a real relay on an ephemeral loopback port.
    async fn start_relay() -> SocketAddr {
        let server = RelayServer::bind(loopback(), Duration::from_secs(5))
            .await
            .expect("a relay");
        let address = server.local_addr();
        tokio::spawn(async move { server.run().await });
        address
    }

    fn config(broker: &str, relay: Option<SocketAddr>, punch_timeout_ms: u64) -> MeshConfig {
        MeshConfig {
            broker: broker.parse().expect("a broker URI"),
            relay,
            bind: loopback(),
            stun_server: None,
            punch_timeout_ms,
            lookup_timeout_ms: LOOKUP_TIMEOUT_MS,
        }
    }

    /// A fake STUN reflector: it answers one binding request with the source
    /// address it saw, which is what a real reflector does.
    ///
    /// It is a thread, not a tokio task, because `join` does the exchange on a
    /// blocking socket; a task on the test's current-thread runtime would not
    /// get to run until the exchange had already timed out. The thread is
    /// detached and gives up after ten seconds, so a join that never asks (the
    /// red half of this feature's TDD) cannot wedge the test process.
    fn start_stun_reflector() -> SocketAddr {
        start_stun_reflector_with(None)
    }

    /// A reflector that reports `mapped` instead of the source it saw, for the
    /// case where the bound address and the reflection differ.
    fn start_stun_reflector_with(mapped: Option<SocketAddr>) -> SocketAddr {
        let reflector = std::net::UdpSocket::bind(loopback()).expect("a reflector");
        let address = reflector.local_addr().expect("the reflector's address");
        std::thread::spawn(move || {
            reflector
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("a read timeout on the reflector");
            let mut buffer = [0u8; 1500];
            let Ok((len, peer)) = reflector.recv_from(&mut buffer) else {
                return;
            };
            assert_eq!(len, 20, "a binding request carries no attributes");
            let transaction_id: crate::stun::TransactionId =
                buffer[8..20].try_into().expect("a transaction id");
            let response = binding_response_for(&transaction_id, mapped.unwrap_or(peer));
            let _ = reflector.send_to(&response, peer);
        });
        address
    }

    /// Build a binding success response for `mapped`, as a reflector would.
    fn binding_response_for(
        transaction_id: &crate::stun::TransactionId,
        mapped: SocketAddr,
    ) -> Vec<u8> {
        let SocketAddr::V4(address) = mapped else {
            panic!("the test reflector only reflects IPv4");
        };
        let mut value = vec![0u8, 0x01];
        value.extend_from_slice(
            &(address.port() ^ (crate::stun::MAGIC_COOKIE >> 16) as u16).to_be_bytes(),
        );
        value.extend_from_slice(
            &(u32::from(*address.ip()) ^ crate::stun::MAGIC_COOKIE).to_be_bytes(),
        );

        let mut response = Vec::with_capacity(20 + 4 + value.len());
        response.extend_from_slice(&0x0101u16.to_be_bytes()); // binding success
        response.extend_from_slice(&((4 + value.len()) as u16).to_be_bytes());
        response.extend_from_slice(&crate::stun::MAGIC_COOKIE.to_be_bytes());
        response.extend_from_slice(transaction_id);
        response.extend_from_slice(&0x0020u16.to_be_bytes()); // XOR-MAPPED-ADDRESS
        response.extend_from_slice(&(value.len() as u16).to_be_bytes());
        response.extend_from_slice(&value);
        response
    }

    #[tokio::test]
    async fn join_publishes_only_the_reflected_candidate_when_the_bind_is_unspecified() {
        let broker = start_broker().await;
        let reflector = start_stun_reflector();
        let identity = Identity::generate();
        let peer = identity.public_hex();
        let config = MeshConfig {
            broker: broker.parse().expect("a broker URI"),
            relay: None,
            bind: "0.0.0.0:0".parse().expect("a wildcard bind"),
            stun_server: Some(reflector),
            punch_timeout_ms: DIRECT_TIMEOUT_MS,
            lookup_timeout_ms: LOOKUP_TIMEOUT_MS,
        };

        let session = MeshSession::join(identity, config)
            .await
            .expect("the join succeeds");

        let bound = session.endpoint.local_addr().expect("the bound address");
        assert!(
            bound.ip().is_unspecified(),
            "the fixture must bind the wildcard, got {bound}"
        );
        let reflected = SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            bound.port(),
        );
        assert_eq!(
            session.candidates.len(),
            1,
            "an unspecified bound address is never a candidate: {:?}",
            session.candidates
        );
        assert_eq!(
            session.candidates[0].addr,
            reflected.to_string(),
            "the reflection is the socket's own address on loopback"
        );
        assert_eq!(session.candidates[0].kind, CandidateKind::Udp);

        let published = Rendezvous::new(Identity::generate(), Vec::new(), REGISTRATION_TTL_MS)
            .lookup(&broker, &peer)
            .await
            .expect("the registration is fresh");
        assert_eq!(
            published
                .iter()
                .map(|candidate| candidate.addr.clone())
                .collect::<Vec<_>>(),
            vec![reflected.to_string()],
            "the broker holds what the session published"
        );
    }

    #[tokio::test]
    async fn join_publishes_the_bound_address_and_a_different_reflection() {
        let broker = start_broker().await;
        let reflected: SocketAddr = "192.0.2.9:54321".parse().expect("a reflected address");
        let reflector = start_stun_reflector_with(Some(reflected));
        let config = MeshConfig {
            broker: broker.parse().expect("a broker URI"),
            relay: None,
            bind: loopback(),
            stun_server: Some(reflector),
            punch_timeout_ms: DIRECT_TIMEOUT_MS,
            lookup_timeout_ms: LOOKUP_TIMEOUT_MS,
        };

        let session = MeshSession::join(Identity::generate(), config)
            .await
            .expect("the join succeeds");

        let bound = session.endpoint.local_addr().expect("the bound address");
        assert!(!bound.ip().is_unspecified());
        assert_eq!(
            session.candidates.len(),
            2,
            "a specified bound address and a different reflection are both candidates: {:?}",
            session.candidates
        );
        assert_eq!(session.candidates[0].addr, bound.to_string());
        assert_eq!(session.candidates[1].addr, reflected.to_string());
    }

    #[tokio::test]
    async fn join_without_a_stun_server_publishes_only_the_bound_address() {
        let broker = start_broker().await;
        let session = MeshSession::join(
            Identity::generate(),
            config(&broker, None, DIRECT_TIMEOUT_MS),
        )
        .await
        .expect("the join succeeds");

        let bound = session.endpoint.local_addr().expect("the bound address");
        assert_eq!(session.candidates.len(), 1);
        assert_eq!(session.candidates[0].addr, bound.to_string());
    }

    #[tokio::test]
    async fn a_silent_stun_server_does_not_fail_the_join() {
        let broker = start_broker().await;
        let silent = std::net::UdpSocket::bind(loopback()).expect("a silent reflector");
        let server = silent.local_addr().expect("its address");
        let config = MeshConfig {
            broker: broker.parse().expect("a broker URI"),
            relay: None,
            bind: "0.0.0.0:0".parse().expect("a wildcard bind"),
            stun_server: Some(server),
            punch_timeout_ms: DIRECT_TIMEOUT_MS,
            lookup_timeout_ms: LOOKUP_TIMEOUT_MS,
        };

        let started = Instant::now();
        let session = MeshSession::join(Identity::generate(), config)
            .await
            .expect("a silent STUN server must not fail the join");
        let elapsed = started.elapsed();

        assert!(
            elapsed >= Duration::from_millis(1_900),
            "the join must wait out the reflector's two-second deadline, took {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(2_500),
            "the STUN wait must stay near two seconds, took {elapsed:?}"
        );
        assert_eq!(
            session.candidates.len(),
            1,
            "a silent reflector falls back to the local address the kernel would use to reach it: {:?}",
            session.candidates
        );
        let bound = session.endpoint.local_addr().expect("the bound address");
        assert!(bound.ip().is_unspecified());
        let expected = SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            bound.port(),
        );
        assert_eq!(
            session.candidates[0].addr,
            expected.to_string(),
            "the fallback is the route's local IP with the QUIC socket's own port"
        );
    }

    #[tokio::test]
    async fn an_unspecified_bind_without_stun_publishes_no_candidates() {
        let broker = start_broker().await;
        let identity = Identity::generate();
        let peer = identity.public_hex();
        let config = MeshConfig {
            broker: broker.parse().expect("a broker URI"),
            relay: None,
            bind: "0.0.0.0:0".parse().expect("a wildcard bind"),
            stun_server: None,
            punch_timeout_ms: DIRECT_TIMEOUT_MS,
            lookup_timeout_ms: LOOKUP_TIMEOUT_MS,
        };

        let session = MeshSession::join(identity, config)
            .await
            .expect("the join succeeds");

        assert!(
            session.candidates.is_empty(),
            "0.0.0.0 is not an address a peer can dial: {:?}",
            session.candidates
        );
        let published = Rendezvous::new(Identity::generate(), Vec::new(), REGISTRATION_TTL_MS)
            .lookup(&broker, &peer)
            .await
            .expect("the registration is fresh");
        assert!(
            published.is_empty(),
            "nothing usable is published, so the peer's punch fails and the relay is the floor"
        );
    }

    /// Two identities with the direct roles decided: the higher key dials, the
    /// lower key accepts.
    fn dialer_and_listener() -> (Identity, Identity) {
        loop {
            let first = Identity::generate();
            let second = Identity::generate();
            if first.public_bytes() > second.public_bytes() {
                return (first, second);
            }
        }
    }

    fn frame(kind: Kind, id: u64) -> Frame {
        Frame {
            kind,
            id,
            payload: json!({"echo": "mesh", "binary": [0, 1, 2, 255]}),
        }
    }

    /// A peer whose key is higher than the listener's, so it is the QUIC dialer
    /// in a `serve` + `connect` pair.
    fn dialer_for(listener: &PublicKey) -> Identity {
        loop {
            let identity = Identity::generate();
            if identity.public_bytes() > *listener {
                return identity;
            }
        }
    }

    /// Serve every frame back as a response until the peer disconnects.
    async fn echo_frames(mut channel: Channel) {
        while let Ok(frame) = channel.recv_frame().await {
            let response = Frame {
                kind: Kind::Res,
                id: frame.id,
                payload: frame.payload,
            };
            if channel.send_frame(&response).await.is_err() {
                return;
            }
        }
    }

    /// Run `serve` in its own task. The session is shared because `serve`
    /// borrows it while the test drives `connect` on the other session.
    fn spawn_serve<H, Fut>(
        session: &Arc<MeshSession>,
        handler: H,
    ) -> JoinHandle<Result<(), SessionError>>
    where
        H: Fn(String, Channel) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let session = Arc::clone(session);
        tokio::spawn(async move { session.serve(HANDSHAKE_TIMEOUT, handler).await })
    }

    /// Send one request over `channel` and check the echoed response.
    async fn echo_once(channel: &mut Channel, id: u64) {
        let request = frame(Kind::Req, id);
        channel
            .send_frame(&request)
            .await
            .expect("the request goes out");
        let response = timeout(PATIENCE, channel.recv_frame())
            .await
            .expect("the echo timed out")
            .expect("the echo arrives");
        assert_eq!(response.kind, Kind::Res);
        assert_eq!(response.id, id);
        assert_eq!(response.payload, request.payload);
    }

    #[test]
    fn the_punch_budget_is_split_equally_across_candidates_with_a_floor() {
        assert_eq!(candidate_slice_ms(4_000, 2), 2_000);
        assert_eq!(candidate_slice_ms(3_000, 2), 1_500);
        assert_eq!(
            candidate_slice_ms(2_000, 3),
            MIN_CANDIDATE_SLICE_MS,
            "no candidate gets less than the floor, even from a budget too small to divide"
        );
        assert_eq!(
            candidate_slice_ms(500, 1),
            MIN_CANDIDATE_SLICE_MS,
            "a short punch budget still gives a single candidate a real handshake window"
        );
    }

    /// One request, one response, both directions, over the pair of channels.
    async fn echo(dialer: &mut Channel, listener: &mut Channel) {
        let request = frame(Kind::Req, 7);
        dialer
            .send_frame(&request)
            .await
            .expect("the request goes out");
        let received = timeout(PATIENCE, listener.recv_frame())
            .await
            .expect("the request timed out")
            .expect("the request arrives");
        assert_eq!(received, request);

        let response = Frame {
            kind: Kind::Res,
            id: received.id,
            payload: received.payload.clone(),
        };
        listener
            .send_frame(&response)
            .await
            .expect("the response goes out");
        let echoed = timeout(PATIENCE, dialer.recv_frame())
            .await
            .expect("the response timed out")
            .expect("the response arrives");
        assert_eq!(echoed, response);

        // The channel is kind-agnostic: a stream chunk and a ping travel the
        // same path as a request and its response, in both directions.
        let stream = frame(Kind::Stream, 8);
        dialer
            .send_frame(&stream)
            .await
            .expect("the stream chunk goes out");
        let received = timeout(PATIENCE, listener.recv_frame())
            .await
            .expect("the stream chunk timed out")
            .expect("the stream chunk arrives");
        assert_eq!(received, stream);

        let ping = frame(Kind::Ping, 9);
        listener.send_frame(&ping).await.expect("the ping goes out");
        let received = timeout(PATIENCE, dialer.recv_frame())
            .await
            .expect("the ping timed out")
            .expect("the ping arrives");
        assert_eq!(received, ping);
    }

    #[tokio::test]
    async fn a_served_session_accepts_a_connect_and_echoes() {
        let broker = start_broker().await;
        let (dialer_identity, listener_identity) = dialer_and_listener();
        let dialer_peer = dialer_identity.public_hex();
        let listener_peer = listener_identity.public_hex();

        let listener = Arc::new(
            MeshSession::join(
                listener_identity.clone(),
                config(&broker, None, DIRECT_TIMEOUT_MS),
            )
            .await
            .expect("the listener joins"),
        );
        let (seen_tx, mut seen_rx) = mpsc::unbounded_channel::<String>();
        let serve = spawn_serve(&listener, move |peer_id, channel| {
            let seen_tx = seen_tx.clone();
            async move {
                let _ = seen_tx.send(peer_id);
                echo_frames(channel).await;
            }
        });

        let mut dialer = MeshSession::join(
            dialer_identity.clone(),
            config(&broker, None, DIRECT_TIMEOUT_MS),
        )
        .await
        .expect("the dialer joins");

        let mut channel = timeout(PATIENCE, dialer.connect(&listener_peer, now_ms()))
            .await
            .expect("the connect timed out")
            .expect("the served session accepts the dialer");

        assert!(channel.is_direct());
        assert_eq!(channel.peer(), listener_identity.public_bytes());
        assert_eq!(dialer.path(), PathState::Direct);

        let seen = timeout(PATIENCE, seen_rx.recv())
            .await
            .expect("the handler was never called")
            .expect("the handler reports the peer");
        assert_eq!(
            seen, dialer_peer,
            "the handler gets the dialer's canonical hex id"
        );

        echo_once(&mut channel, 7).await;
        serve.abort();
    }

    #[tokio::test]
    async fn a_stalled_connection_is_closed_at_the_deadline_and_the_next_connect_succeeds() {
        let broker = start_broker().await;
        let (dialer_identity, listener_identity) = dialer_and_listener();
        let listener_peer = listener_identity.public_hex();
        let listener_key = listener_identity.public_bytes();

        let listener = Arc::new(
            MeshSession::join(
                listener_identity.clone(),
                config(&broker, None, DIRECT_TIMEOUT_MS),
            )
            .await
            .expect("the listener joins"),
        );
        let serve = spawn_serve(&listener, |_peer_id, channel| echo_frames(channel));
        let listener_addr = listener
            .endpoint
            .local_addr()
            .expect("the listener's bound address");

        // A stranger completes the QUIC/TLS handshake and reads this
        // connection's challenge, then sends no hello. It holds both stream
        // halves open, so only the listener's deadline can end it.
        let stranger =
            MeshEndpoint::client(Identity::generate(), loopback()).expect("a stranger endpoint");
        let connection = timeout(
            PATIENCE,
            stranger.connect(listener_addr, Some(listener_key)),
        )
        .await
        .expect("the transport handshake timed out")
        .expect("the transport handshake completes");
        let (_send, mut recv) = timeout(HANDSHAKE_TIMEOUT, connection.accept_bi())
            .await
            .expect("the listener never opened its session stream")
            .expect("the listener opens the session stream");
        let _challenge = timeout(HANDSHAKE_TIMEOUT, frame::read(&mut recv))
            .await
            .expect("the listener's challenge timed out")
            .expect("the listener's challenge arrives");

        // The real dialer arrives while the stranger is still stalled. If the
        // accept loop waited on the stranger, this would not finish until the
        // deadline; it must finish well inside it.
        let mut dialer = MeshSession::join(
            dialer_identity.clone(),
            config(&broker, None, DIRECT_TIMEOUT_MS),
        )
        .await
        .expect("the dialer joins");
        let started = Instant::now();
        let mut channel = timeout(
            Duration::from_secs(2),
            dialer.connect(&listener_peer, now_ms()),
        )
        .await
        .expect("a stalled stranger must not block the next connection")
        .expect("the real peer gets a direct channel");
        assert!(channel.is_direct());
        assert!(
            started.elapsed() < HANDSHAKE_TIMEOUT,
            "the legitimate handshake took {:?}, which is not inside the listener's deadline",
            started.elapsed()
        );

        // The stranger is closed at that deadline rather than left hanging.
        let reason = timeout(
            HANDSHAKE_TIMEOUT + Duration::from_secs(2),
            connection.closed(),
        )
        .await
        .expect("the stalled connection was never closed");
        assert!(
            matches!(reason, quinn::ConnectionError::ApplicationClosed(_)),
            "the listener must close a stalled handshake with an application close, got {reason:?}"
        );

        echo_once(&mut channel, 21).await;
        serve.abort();
    }

    #[tokio::test]
    async fn a_refused_hello_does_not_stop_serve() {
        let broker = start_broker().await;
        let (dialer_identity, listener_identity) = dialer_and_listener();
        let listener_peer = listener_identity.public_hex();
        let listener_key = listener_identity.public_bytes();
        let dialer_peer = dialer_identity.public_hex();

        let listener = Arc::new(
            MeshSession::join(
                listener_identity.clone(),
                config(&broker, None, DIRECT_TIMEOUT_MS),
            )
            .await
            .expect("the listener joins"),
        );
        let serve = spawn_serve(&listener, |_peer_id, channel| echo_frames(channel));
        let listener_addr = listener
            .endpoint
            .local_addr()
            .expect("the listener's bound address");

        // The raw client names the real dialer but signs with another key, so
        // the pin resolves and the signature does not verify.
        let forger = MeshEndpoint::client(Identity::generate(), loopback())
            .expect("a forged-hello endpoint");
        let connection = timeout(PATIENCE, forger.connect(listener_addr, Some(listener_key)))
            .await
            .expect("the transport handshake timed out")
            .expect("the transport handshake completes");
        let (mut send, mut recv) = timeout(HANDSHAKE_TIMEOUT, connection.accept_bi())
            .await
            .expect("the listener never opened its session stream")
            .expect("the listener opens the session stream");
        let challenge_frame = timeout(HANDSHAKE_TIMEOUT, frame::read(&mut recv))
            .await
            .expect("the challenge timed out")
            .expect("the challenge arrives");
        let challenge = Challenge::from_frame(&challenge_frame).expect("a challenge frame");
        let forged = Hello::sign(&Identity::generate(), dialer_peer.as_str(), &challenge);
        frame::write(&mut send, &forged.to_frame().expect("a hello frame"))
            .await
            .expect("the forged hello goes out");

        let reason = timeout(HANDSHAKE_TIMEOUT, connection.closed())
            .await
            .expect("the refused connection was never closed");
        assert!(
            matches!(reason, quinn::ConnectionError::ApplicationClosed(_)),
            "a bad hello must be closed, got {reason:?}"
        );

        // The refusal happened in one connection's task; the loop still serves.
        let mut dialer = MeshSession::join(
            dialer_identity.clone(),
            config(&broker, None, DIRECT_TIMEOUT_MS),
        )
        .await
        .expect("the dialer joins");
        let mut channel = timeout(PATIENCE, dialer.connect(&listener_peer, now_ms()))
            .await
            .expect("the connect after a refused hello timed out")
            .expect("the real peer is served after a refused hello");
        assert!(channel.is_direct());
        echo_once(&mut channel, 22).await;

        serve.abort();
    }

    #[tokio::test]
    async fn a_blackholed_first_candidate_still_opens_directly_via_the_second() {
        let broker = start_broker().await;
        let (dialer_identity, listener_identity) = dialer_and_listener();
        let listener_peer = listener_identity.public_hex();
        let listener_key = listener_identity.public_bytes();

        let listener = Arc::new(
            MeshSession::join(
                listener_identity.clone(),
                config(&broker, None, DIRECT_TIMEOUT_MS),
            )
            .await
            .expect("the listener joins"),
        );
        let serve = spawn_serve(&listener, |_peer_id, channel| echo_frames(channel));

        let live = listener
            .endpoint
            .local_addr()
            .expect("the listener's bound address");
        // A bound UDP socket that reads nothing: QUIC's handshake packets
        // reach the kernel and go no further, so this candidate is a black
        // hole rather than a port that refuses.
        let blackhole = UdpSocket::bind(loopback())
            .await
            .expect("a bound, silent UDP socket");
        let blackhole_addr = blackhole.local_addr().expect("the silent address");
        Rendezvous::new(
            listener_identity.clone(),
            vec![
                Candidate::new(CandidateKind::Udp, blackhole_addr.to_string(), now_ms()),
                Candidate::new(CandidateKind::Udp, live.to_string(), now_ms()),
            ],
            REGISTRATION_TTL_MS,
        )
        .register(&broker)
        .await
        .expect("the two candidates are registered");

        let mut dialer = MeshSession::join(dialer_identity.clone(), config(&broker, None, 3_000))
            .await
            .expect("the dialer joins");

        let started = Instant::now();
        let mut channel = timeout(PATIENCE, dialer.connect(&listener_peer, now_ms()))
            .await
            .expect("the two-candidate connect timed out")
            .expect("the live second candidate opens a direct channel");

        assert!(channel.is_direct());
        assert_eq!(channel.peer(), listener_key);
        assert_eq!(dialer.path(), PathState::Direct);
        assert!(
            started.elapsed() >= Duration::from_millis(MIN_CANDIDATE_SLICE_MS),
            "the blackholed first candidate must get its own slice before the \
             second is tried, not be abandoned immediately"
        );

        echo_once(&mut channel, 33).await;
        serve.abort();
        drop(blackhole);
    }

    #[tokio::test]
    async fn a_connect_dropped_at_the_punch_does_not_wedge_the_next_one() {
        let broker = start_broker().await;
        let (dialer_identity, listener_identity) = dialer_and_listener();
        let listener_peer = listener_identity.public_hex();

        let listener = Arc::new(
            MeshSession::join(
                listener_identity.clone(),
                config(&broker, None, DIRECT_TIMEOUT_MS),
            )
            .await
            .expect("the listener joins"),
        );
        let serve = spawn_serve(&listener, |_peer_id, channel| echo_frames(channel));
        let live = listener
            .endpoint
            .local_addr()
            .expect("the listener's bound address");

        // A bound UDP socket that reads nothing: the first connect enters the
        // punch and stays there until its future is dropped.
        let blackhole = UdpSocket::bind(loopback())
            .await
            .expect("a bound, silent UDP socket");
        Rendezvous::new(
            listener_identity.clone(),
            vec![Candidate::new(
                CandidateKind::Udp,
                blackhole
                    .local_addr()
                    .expect("the silent address")
                    .to_string(),
                now_ms(),
            )],
            REGISTRATION_TTL_MS,
        )
        .register(&broker)
        .await
        .expect("the black hole is registered");

        let mut dialer =
            MeshSession::join(dialer_identity, config(&broker, None, DIRECT_TIMEOUT_MS))
                .await
                .expect("the dialer joins");

        // `timeout` drops the connect future inside the punch — the same thing
        // a caller's own deadline does.
        let cancelled = timeout(
            Duration::from_millis(200),
            dialer.connect(&listener_peer, now_ms()),
        )
        .await;
        assert!(
            cancelled.is_err(),
            "the first connect must still be punching when it is dropped"
        );
        assert_eq!(
            dialer.path(),
            PathState::Punching,
            "the dropped attempt must have reached the punch"
        );

        // The peer is now reachable at its live address. A path that treats a
        // cancelled punch as terminal answers `Stalled(Punching)` here instead.
        Rendezvous::new(
            listener_identity,
            vec![Candidate::new(
                CandidateKind::Udp,
                live.to_string(),
                now_ms(),
            )],
            REGISTRATION_TTL_MS,
        )
        .register(&broker)
        .await
        .expect("the live candidate is registered");

        let mut channel = timeout(PATIENCE, dialer.connect(&listener_peer, now_ms()))
            .await
            .expect("the reconnect after a cancelled punch timed out")
            .expect("a cancelled punch must not wedge the path");
        assert!(channel.is_direct());
        assert_eq!(dialer.path(), PathState::Direct);

        echo_once(&mut channel, 81).await;
        serve.abort();
        drop(blackhole);
    }

    #[tokio::test]
    async fn serve_keeps_serving_a_second_peer_after_the_first_disconnects() {
        let broker = start_broker().await;
        let listener_identity = Identity::generate();
        let listener_peer = listener_identity.public_hex();
        let listener_key = listener_identity.public_bytes();

        let listener = Arc::new(
            MeshSession::join(
                listener_identity.clone(),
                config(&broker, None, DIRECT_TIMEOUT_MS),
            )
            .await
            .expect("the listener joins"),
        );
        let (seen_tx, mut seen_rx) = mpsc::unbounded_channel::<String>();
        let serve = spawn_serve(&listener, move |peer_id, channel| {
            let seen_tx = seen_tx.clone();
            async move {
                let _ = seen_tx.send(peer_id);
                echo_frames(channel).await;
            }
        });

        let first_identity = dialer_for(&listener_key);
        let first_peer = first_identity.public_hex();
        let mut first_session = MeshSession::join(
            first_identity.clone(),
            config(&broker, None, DIRECT_TIMEOUT_MS),
        )
        .await
        .expect("the first dialer joins");
        let mut first = timeout(PATIENCE, first_session.connect(&listener_peer, now_ms()))
            .await
            .expect("the first connect timed out")
            .expect("the first peer gets a channel");
        echo_once(&mut first, 41).await;
        assert_eq!(
            timeout(PATIENCE, seen_rx.recv())
                .await
                .expect("the first handler was never called")
                .as_deref(),
            Some(first_peer.as_str())
        );
        drop(first);
        tokio::time::sleep(Duration::from_millis(50)).await;

        let second_identity = dialer_for(&listener_key);
        let second_peer = second_identity.public_hex();
        let mut second_session = MeshSession::join(
            second_identity.clone(),
            config(&broker, None, DIRECT_TIMEOUT_MS),
        )
        .await
        .expect("the second dialer joins");
        let mut second = timeout(PATIENCE, second_session.connect(&listener_peer, now_ms()))
            .await
            .expect("the second connect timed out")
            .expect("the second peer gets a channel after the first disconnects");
        assert!(second.is_direct());
        echo_once(&mut second, 42).await;
        assert_eq!(
            timeout(PATIENCE, seen_rx.recv())
                .await
                .expect("the second handler was never called")
                .as_deref(),
            Some(second_peer.as_str())
        );

        serve.abort();
    }

    #[tokio::test]
    async fn a_dead_candidate_falls_back_to_the_relay_and_echoes() {
        let broker = start_broker().await;
        let relay = start_relay().await;
        let (dialer_identity, listener_identity) = dialer_and_listener();
        let dialer_peer = dialer_identity.public_hex();
        let listener_peer = listener_identity.public_hex();

        let mut dialer = MeshSession::join(
            dialer_identity.clone(),
            config(&broker, Some(relay), FALLBACK_TIMEOUT_MS),
        )
        .await
        .expect("the dialer joins");
        let mut listener = MeshSession::join(
            listener_identity.clone(),
            config(&broker, Some(relay), FALLBACK_TIMEOUT_MS),
        )
        .await
        .expect("the listener joins");

        // The listener still answers on its bound address, but its published
        // candidate is a UDP socket that reads nothing: the dialer's QUIC
        // handshake can never complete, so the punch must time out.
        let dead = UdpSocket::bind(loopback()).await.expect("a dead UDP port");
        Rendezvous::new(
            listener_identity.clone(),
            vec![Candidate::new(
                CandidateKind::Udp,
                dead.local_addr().expect("the dead address").to_string(),
                now_ms(),
            )],
            REGISTRATION_TTL_MS,
        )
        .register(&broker)
        .await
        .expect("the dead candidate is registered");

        let (dialer_channel, listener_channel) = timeout(PATIENCE, async {
            tokio::join!(
                dialer.connect(&listener_peer, now_ms()),
                listener.connect(&dialer_peer, now_ms()),
            )
        })
        .await
        .expect("the relay connect timed out");

        let mut dialer_channel = dialer_channel.expect("the dialer falls back to the relay");
        let mut listener_channel = listener_channel.expect("the listener falls back to the relay");

        assert_eq!(dialer.path(), PathState::Relayed);
        assert_eq!(listener.path(), PathState::Relayed);
        assert!(!dialer_channel.is_direct());
        assert!(!listener_channel.is_direct());

        echo(&mut dialer_channel, &mut listener_channel).await;
    }

    #[tokio::test]
    async fn a_refusing_relay_is_a_typed_error_and_counts_as_a_close() {
        let broker = start_broker().await;
        let (dialer_identity, listener_identity) = dialer_and_listener();
        let listener_peer = listener_identity.public_hex();

        // No candidates: the punch fails immediately, so the fallback is
        // ordered without spending a candidate slice. This test is about the
        // relay's accounting, not the punch's timing.
        Rendezvous::new(listener_identity, Vec::new(), REGISTRATION_TTL_MS)
            .register(&broker)
            .await
            .expect("the peer registers with no direct candidates");

        // Bind a port and drop it: connecting there is refused, not blackholed.
        let refusing = TcpListener::bind(loopback())
            .await
            .expect("a loopback listener");
        let refusing_address = refusing.local_addr().expect("the bound address");
        drop(refusing);

        let mut dialer = MeshSession::join(
            dialer_identity,
            config(&broker, Some(refusing_address), DIRECT_TIMEOUT_MS),
        )
        .await
        .expect("the dialer joins");

        // Each attempt is a fallback that never opened a channel. Jumping the
        // caller's clock past the stable window between attempts is what makes
        // the accounting visible: a relay marked connected before it opened
        // would look stable and reset the retry budget to 1 every time.
        let mut now = now_ms();
        for attempt in 1..=6u8 {
            let error = timeout(PATIENCE, dialer.connect(&listener_peer, now))
                .await
                .unwrap_or_else(|_| panic!("dead-relay connect {attempt} hung"))
                .expect_err("a refusing relay cannot produce a channel");
            assert!(
                matches!(
                    error,
                    SessionError::Relay(RelayError::Io(std::io::ErrorKind::ConnectionRefused))
                ),
                "attempt {attempt}: the refusal must stay typed, got {error:?}"
            );
            assert!(
                matches!(dialer.path(), PathState::Rendezvous | PathState::Closed),
                "attempt {attempt}: a relay that never opened must not remain the path, got {:?}",
                dialer.path()
            );
            now = now.saturating_add(STABLE_WINDOW_MS + 1_000);
        }
        assert_eq!(
            dialer.path(),
            PathState::Closed,
            "six dead-relay attempts must exhaust the budget, not reset it"
        );

        let exhausted = timeout(PATIENCE, dialer.connect(&listener_peer, now))
            .await
            .expect("the exhausted connect hung")
            .expect_err("an exhausted path cannot produce a channel");
        assert!(
            matches!(exhausted, SessionError::Exhausted),
            "expected Exhausted, got {exhausted:?}"
        );
    }

    #[tokio::test]
    async fn a_served_session_meets_a_relayed_connect_without_connecting() {
        let broker = start_broker().await;
        let relay = start_relay().await;
        let listener_identity = Identity::generate();
        let listener_peer = listener_identity.public_hex();

        let listener = Arc::new(
            MeshSession::join(
                listener_identity.clone(),
                config(&broker, Some(relay), FALLBACK_TIMEOUT_MS),
            )
            .await
            .expect("the listener joins"),
        );
        let (seen_tx, mut seen_rx) = mpsc::unbounded_channel::<String>();
        let serve = spawn_serve(&listener, move |peer_id, channel| {
            let seen_tx = seen_tx.clone();
            async move {
                let _ = seen_tx.send(peer_id);
                echo_frames(channel).await;
            }
        });

        // The listener still answers on its bound address, but its published
        // candidate is a UDP socket that reads nothing. The dialer's direct
        // punch must fail and fall back to the relay.
        let dead = UdpSocket::bind(loopback()).await.expect("a dead UDP port");
        let dead_addr = dead.local_addr().expect("the dead address");
        Rendezvous::new(
            listener_identity.clone(),
            vec![Candidate::new(
                CandidateKind::Udp,
                dead_addr.to_string(),
                now_ms(),
            )],
            REGISTRATION_TTL_MS,
        )
        .register(&broker)
        .await
        .expect("the dead candidate is registered");

        let dialer_identity = dialer_for(&listener_identity.public_bytes());
        let dialer_peer = dialer_identity.public_hex();
        let mut dialer = MeshSession::join(
            dialer_identity,
            config(&broker, Some(relay), FALLBACK_TIMEOUT_MS),
        )
        .await
        .expect("the dialer joins");

        // The listener never calls `connect`: `serve` must meet the request the
        // dialer left at the broker.
        let mut channel = timeout(PATIENCE, dialer.connect(&listener_peer, now_ms()))
            .await
            .expect("the served relay connect timed out")
            .expect("the served session meets the dialer on the relay");

        assert!(!channel.is_direct(), "there is no live direct candidate");
        assert_eq!(dialer.path(), PathState::Relayed);
        assert_eq!(channel.peer(), listener_identity.public_bytes());

        let seen = timeout(PATIENCE, seen_rx.recv())
            .await
            .expect("the relay handler was never called")
            .expect("the handler reports the dialer");
        assert_eq!(seen, dialer_peer);

        echo_once(&mut channel, 51).await;
        serve.abort();
        drop(dead);
    }

    #[tokio::test]
    async fn a_served_session_serves_two_relayed_connects_in_a_row() {
        let broker = start_broker().await;
        let relay = start_relay().await;
        let listener_identity = Identity::generate();
        let listener_peer = listener_identity.public_hex();
        let listener_key = listener_identity.public_bytes();

        let listener = Arc::new(
            MeshSession::join(
                listener_identity.clone(),
                config(&broker, Some(relay), FALLBACK_TIMEOUT_MS),
            )
            .await
            .expect("the listener joins"),
        );
        let (seen_tx, mut seen_rx) = mpsc::unbounded_channel::<String>();
        let serve = spawn_serve(&listener, move |peer_id, channel| {
            let seen_tx = seen_tx.clone();
            async move {
                let _ = seen_tx.send(peer_id);
                echo_frames(channel).await;
            }
        });

        let dead = UdpSocket::bind(loopback()).await.expect("a dead UDP port");
        let dead_addr = dead.local_addr().expect("the dead address");
        Rendezvous::new(
            listener_identity,
            vec![Candidate::new(
                CandidateKind::Udp,
                dead_addr.to_string(),
                now_ms(),
            )],
            REGISTRATION_TTL_MS,
        )
        .register(&broker)
        .await
        .expect("the dead candidate is registered");

        for id in [61, 62] {
            let dialer_identity = dialer_for(&listener_key);
            let dialer_peer = dialer_identity.public_hex();
            let mut dialer = MeshSession::join(
                dialer_identity,
                config(&broker, Some(relay), FALLBACK_TIMEOUT_MS),
            )
            .await
            .expect("the dialer joins");

            let mut channel = timeout(PATIENCE, dialer.connect(&listener_peer, now_ms()))
                .await
                .unwrap_or_else(|_| panic!("the relay connect {id} timed out"))
                .unwrap_or_else(|error| panic!("the relay connect {id} failed: {error}"));
            assert!(!channel.is_direct());
            echo_once(&mut channel, id).await;
            assert_eq!(
                timeout(PATIENCE, seen_rx.recv())
                    .await
                    .expect("the relay handler was never called")
                    .as_deref(),
                Some(dialer_peer.as_str()),
                "the serving loop must keep polling after the first relayed peer"
            );
        }

        serve.abort();
        drop(dead);
    }

    #[tokio::test]
    async fn a_refused_relay_request_does_not_stop_the_serve_loop() {
        let broker = start_broker().await;
        let relay = start_relay().await;
        let listener_identity = Identity::generate();
        let listener_peer = listener_identity.public_hex();
        let listener_key = listener_identity.public_bytes();

        let listener = Arc::new(
            MeshSession::join(
                listener_identity.clone(),
                config(&broker, Some(relay), FALLBACK_TIMEOUT_MS),
            )
            .await
            .expect("the listener joins"),
        );
        let (seen_tx, mut seen_rx) = mpsc::unbounded_channel::<String>();
        let serve = spawn_serve(&listener, move |peer_id, channel| {
            let seen_tx = seen_tx.clone();
            async move {
                let _ = seen_tx.send(peer_id);
                echo_frames(channel).await;
            }
        });

        // A request that names this session to itself: `for_peer` can never
        // pair a key with itself, so `serve` must refuse it and keep polling.
        let self_request =
            Rendezvous::new(listener_identity.clone(), Vec::new(), REGISTRATION_TTL_MS)
                .request(&broker, &listener_peer, "self-session", REQUEST_TTL_MS)
                .await
                .expect("the broker accepts a self-addressed request");
        assert_eq!(self_request.requester_id, listener_peer);

        let dead = UdpSocket::bind(loopback()).await.expect("a dead UDP port");
        let dead_addr = dead.local_addr().expect("the dead address");
        Rendezvous::new(
            listener_identity,
            vec![Candidate::new(
                CandidateKind::Udp,
                dead_addr.to_string(),
                now_ms(),
            )],
            REGISTRATION_TTL_MS,
        )
        .register(&broker)
        .await
        .expect("the dead candidate is registered");

        let dialer_identity = dialer_for(&listener_key);
        let dialer_peer = dialer_identity.public_hex();
        let mut dialer = MeshSession::join(
            dialer_identity,
            config(&broker, Some(relay), FALLBACK_TIMEOUT_MS),
        )
        .await
        .expect("the dialer joins");
        let mut channel = timeout(PATIENCE, dialer.connect(&listener_peer, now_ms()))
            .await
            .expect("the connect after a refused request timed out")
            .expect("the real peer is served after a refused request");
        assert!(!channel.is_direct());

        echo_once(&mut channel, 71).await;
        assert_eq!(
            timeout(PATIENCE, seen_rx.recv())
                .await
                .expect("the relay handler was never called")
                .as_deref(),
            Some(dialer_peer.as_str())
        );

        serve.abort();
        drop(dead);
    }

    #[tokio::test]
    async fn a_broker_that_never_answers_is_a_typed_timeout_at_the_configured_bound() {
        let broker = start_stalling_broker().await;
        let peer = Identity::generate().public_hex();
        let lookup_timeout_ms = 150;

        let mut dialer = MeshSession::join(
            Identity::generate(),
            MeshConfig {
                broker: broker.parse().expect("a broker URI"),
                relay: None,
                bind: loopback(),
                stun_server: None,
                punch_timeout_ms: 5_000,
                lookup_timeout_ms,
            },
        )
        .await
        .expect("the stalling broker answers a registration");

        let started = Instant::now();
        let error = timeout(PATIENCE, dialer.connect(&peer, now_ms()))
            .await
            .expect("a broker that never answers must not hang a connect")
            .expect_err("a broker that never answers has no candidates");
        let elapsed = started.elapsed();

        assert!(
            matches!(error, SessionError::Rendezvous(RendezvousError::Timeout)),
            "expected a typed rendezvous timeout, got {error:?}"
        );
        assert!(
            elapsed >= Duration::from_millis(lookup_timeout_ms),
            "the lookup timed out before its configured bound: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "the lookup must use its configured {lookup_timeout_ms} ms bound, \
             not the 5 s path deadline: {elapsed:?}"
        );
        assert_eq!(
            dialer.path(),
            PathState::Rendezvous,
            "a transport timeout must not tick the path past a deadline it never reached"
        );
    }

    #[tokio::test]
    async fn an_expired_registration_is_a_typed_error_not_a_hang() {
        let broker = start_broker().await;
        let (dialer_identity, expired_identity) = dialer_and_listener();
        let expired = expired_identity.public_hex();

        let mut dialer =
            MeshSession::join(dialer_identity, config(&broker, None, DIRECT_TIMEOUT_MS))
                .await
                .expect("the dialer joins");
        // A one-millisecond ttl and a minute between refreshes: by the time the
        // dialer looks, the broker's answer is a 404.
        let _expired = MeshSession::join_with_ttl(
            expired_identity,
            config(&broker, None, DIRECT_TIMEOUT_MS),
            1,
            60_000,
        )
        .await
        .expect("the expired session joins");
        tokio::time::sleep(Duration::from_millis(20)).await;

        let error = timeout(PATIENCE, dialer.connect(&expired, now_ms()))
            .await
            .expect("connect must not hang when the lookup is a 404")
            .expect_err("an expired registration is not a peer");

        match error {
            SessionError::UnknownPeer(peer) => assert_eq!(peer, expired),
            other => panic!("expected UnknownPeer, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_joined_session_keeps_its_registration_fresh() {
        let broker = start_broker().await;
        let listener_identity = Identity::generate();
        let listener_peer = listener_identity.public_hex();

        let _listener = MeshSession::join_with_ttl(
            listener_identity,
            config(&broker, None, DIRECT_TIMEOUT_MS),
            200,
            50,
        )
        .await
        .expect("the listener joins");

        // Past the 200 ms ttl by 150 ms. Without the refresh task the broker
        // would answer 404; with it, the last re-signing is at most one
        // refresh period old.
        tokio::time::sleep(Duration::from_millis(350)).await;

        let lookup = Rendezvous::new(Identity::generate(), Vec::new(), REGISTRATION_TTL_MS)
            .lookup(&broker, &listener_peer)
            .await;
        assert!(
            lookup.is_ok(),
            "the refresh task must keep the registration findable: {lookup:?}"
        );
    }
}
