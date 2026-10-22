//! The mesh's device-to-device primitives.
//!
//! Phase 2 builds the transport in layers, and each one is a module here:
//!
//! - [`keys`] — the Ed25519 identity. A peer is 32 bytes of public key.
//! - [`cert`] — the self-signed certificate whose SubjectPublicKeyInfo carries
//!   that key, because TLS speaks certificates and not raw keys.
//! - [`endpoint`] — the QUIC endpoint: the client pins the server's
//!   certificate, and the server issues a fresh challenge that the dialer's
//!   signed hello must echo. A missing pin is a typed error, never accept-all.
//! - [`frame`] — the wire unit: length-prefixed JSON `{kind, id, payload}`,
//!   capped at one mebibyte before a body buffer is allocated.
//! - [`rendezvous`] — the signed address book, plus the signed connect request
//!   that asks an idle peer to meet at a relay session. A peer publishes where
//!   it can be reached, and a client asks where another peer said it was.
//! - [`broker`] — the local HTTP/JSON server for that address book. It stores
//!   candidates, timestamps and one-shot connect notes, never a frame.
//! - [`stun`] — the public-address question: a binding request and its
//!   `XOR-MAPPED-ADDRESS` answer, IPv4 and IPv6.
//! - [`punch`] — the outbound probe that opens a NAT mapping, and the rule
//!   that only the peer's own address opens the path.
//! - [`relay`] — the floor: a TCP relay that pairs by session id and forwards
//!   length-prefixed frames as opaque bytes.
//! - [`relay_tls`] — the TLS session **inside** that splice: the relay
//!   forwards ciphertext it cannot read, and the peers keep the same pinned
//!   identity and signed hello the QUIC path uses.
//! - [`session`] — the object that ties them together: join the mesh, keep the
//!   registration fresh, and drive `core`'s path from the rendezvous to a
//!   direct or relayed [`session::Channel`].
//!
//! The property everything rests on: **the identity is the truth and the
//! address is a hint.** A stale address costs a failed handshake, never a
//! connection to the wrong machine.

#![forbid(unsafe_code)]

pub mod broker;
pub mod cert;
pub mod endpoint;
pub mod frame;
pub mod hello;
pub mod keys;
pub mod punch;
pub mod relay;
pub mod relay_tls;
pub mod rendezvous;
pub mod session;
pub mod stun;

pub use broker::Broker;
pub use cert::CertError;
pub use endpoint::{EndpointError, MeshEndpoint, PROTOCOL_VERSION};
pub use frame::{Frame, FrameError, Kind};
pub use hello::{Challenge, Hello, HelloError, PinResolver, CHALLENGE_BYTES};
pub use keys::{Identity, KeyError};
pub use punch::{PunchOutcome, Puncher};
pub use relay::{RelayClient, RelayError, RelayServer};
pub use relay_tls::{RelayTlsError, RelayTlsRole, RelayTlsSession};
pub use rendezvous::{
    Candidate, CandidateKind, ConnectRequest, LookupResponse, Registration, Rendezvous,
    RendezvousError, RequestsResponse,
};
pub use session::{
    Channel, MeshConfig, MeshSession, PeerId, SessionError, Uri, LOOKUP_TIMEOUT_MS,
    MIN_CANDIDATE_SLICE_MS, REGISTRATION_REFRESH_MS, REGISTRATION_TTL_MS, RELAY_PATIENCE,
    REQUEST_POLL_INTERVAL, REQUEST_TTL_MS, STABLE_WINDOW_MS, STUN_TIMEOUT,
};
pub use stun::{
    binding_request, binding_request_with, parse_binding_response, query, query_blocking,
    transaction_id, StunError, TransactionId, MAGIC_COOKIE, TRANSACTION_ID_BYTES,
};
