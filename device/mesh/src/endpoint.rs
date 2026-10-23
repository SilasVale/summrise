//! The mesh endpoint: a QUIC endpoint whose handshake proves the peer's key.
//!
//! There are two different questions, answered at two different layers:
//!
//! - **Is this the peer I dialed?** The client pins the server's certificate.
//!   The comparison happens in the verifier, on the certificate rustls
//!   actually received — the one place where a wrong address becomes a failed
//!   handshake instead of a connection to the wrong machine.
//! - **Who dialed the server?** TLS does not answer that: one server endpoint
//!   serves many peers, and a per-connection client pin cannot live in the
//!   server's TLS configuration. The server sends a fresh
//!   [`Challenge`](crate::hello::Challenge) as its first frame; the dialer
//!   answers with a signed [`Hello`](crate::hello::Hello) that echoes it. The
//!   server resolves its `peer_id_hex` through a [`PinResolver`], checks the
//!   echo in constant time, and verifies the signature **before any other
//!   frame**. The challenge is fresh per connection, so a recorded hello is
//!   useless on a later one.
//!
//! A missing pin is [`EndpointError::MissingPin`], never an accept-all
//! verifier.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as TlsError, SignatureScheme};

use crate::cert;
use crate::frame::{self, FrameError};
use crate::hello::{Challenge, Hello, HelloError, PinResolver, PublicKey};
use crate::keys::{hex_encode, pkcs8_ed25519, Identity, PUBLIC_KEY_BYTES};

/// The mesh wire version.
///
/// Version 2 is the server challenge: the accepting side sends a
/// [`Challenge`] first and the dialer signs it, replacing version 1's signed
/// nonce. A change to the frame or hello wire bumps this and [`ALPN`].
pub const PROTOCOL_VERSION: u32 = 2;

/// The ALPN the mesh speaks. A peer that does not offer it is not a mesh peer.
/// The name carries [`PROTOCOL_VERSION`].
pub const ALPN: &[u8] = b"summrise/mesh/2";

/// The name given to the TLS stack. It is not authenticated — the pin is — but
/// QUIC's TLS layer insists on one.
pub const SERVER_NAME: &str = "summrise-mesh";

/// The QUIC error code a server uses when it refuses a hello.
pub const HELLO_REFUSED: u32 = 1;

/// Why an endpoint could not be built or a peer could not be reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointError {
    /// No peer key was supplied. A verifier without a pin does not exist.
    MissingPin,
    /// The certificate could not be built or did not carry an Ed25519 key.
    Certificate(String),
    /// The TLS configuration was refused.
    Config(String),
    /// Binding or connecting failed at the socket layer.
    Io(std::io::ErrorKind),
    /// The handshake was refused — including a pin mismatch.
    Handshake(String),
    /// Opening or accepting a QUIC stream failed.
    Stream(String),
    /// The connection did not authenticate within its handshake deadline.
    HandshakeTimeout,
    /// The endpoint has shut down and will accept no more connections.
    Closed,
    /// A frame could not be read or written.
    Frame(FrameError),
    /// The first client frame was not an acceptable hello.
    Hello(HelloError),
}

impl std::fmt::Display for EndpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPin => f.write_str("a peer pin is required; refusing to accept any key"),
            Self::Certificate(message) => write!(f, "the mesh certificate is unusable: {message}"),
            Self::Config(message) => write!(f, "the mesh TLS configuration is unusable: {message}"),
            Self::Io(kind) => write!(f, "the mesh socket failed: {kind:?}"),
            Self::Handshake(message) => write!(f, "the mesh handshake failed: {message}"),
            Self::Stream(message) => write!(f, "the mesh stream failed: {message}"),
            Self::HandshakeTimeout => f.write_str("the mesh handshake deadline expired"),
            Self::Closed => f.write_str("the mesh endpoint is closed"),
            Self::Frame(error) => write!(f, "{error}"),
            Self::Hello(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for EndpointError {}

impl From<std::io::Error> for EndpointError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.kind())
    }
}

impl From<FrameError> for EndpointError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

impl From<HelloError> for EndpointError {
    fn from(error: HelloError) -> Self {
        Self::Hello(error)
    }
}

/// The crypto provider: `ring`, explicitly, so the build never falls through
/// to a platform verifier or a second crypto stack.
fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The certificate chain and private key the server presents.
fn server_certificate_and_key(
    identity: &Identity,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), EndpointError> {
    let certificate = cert::self_signed(identity)
        .map_err(|error| EndpointError::Certificate(error.to_string()))?;
    let key = PrivateKeyDer::try_from(pkcs8_ed25519(&identity.private_bytes()))
        .map_err(|error| EndpointError::Config(format!("the identity's key: {error}")))?;
    Ok((vec![certificate], key))
}

/// Build the rustls client configuration: pin the server, present no client
/// certificate.
///
/// `None` is a typed error. An "accept any certificate" verifier is not
/// constructible in this module.
pub fn rustls_client_config(
    pinned: Option<PublicKey>,
) -> Result<rustls::ClientConfig, EndpointError> {
    let verifier = Arc::new(PinnedServerVerifier::new(pinned)?);
    let mut config = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| EndpointError::Config(error.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    config.alpn_protocols = vec![ALPN.to_vec()];
    Ok(config)
}

/// Build the rustls server configuration: present our certificate only.
///
/// The client's identity is established by the signed hello at the
/// application layer, because one server endpoint serves many peers and a
/// client pin cannot be a constant in the server's TLS configuration.
pub fn rustls_server_config(identity: &Identity) -> Result<rustls::ServerConfig, EndpointError> {
    let (chain, key) = server_certificate_and_key(identity)?;
    let mut config = rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| EndpointError::Config(error.to_string()))?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|error| EndpointError::Config(error.to_string()))?;
    config.alpn_protocols = vec![ALPN.to_vec()];
    Ok(config)
}

/// The quinn client configuration, derived from [`rustls_client_config`].
pub fn client_config(pinned: Option<PublicKey>) -> Result<quinn::ClientConfig, EndpointError> {
    let crypto = rustls_client_config(pinned)?;
    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
        .map_err(|error| EndpointError::Config(format!("QUIC refuses the TLS config: {error}")))?;
    Ok(quinn::ClientConfig::new(Arc::new(quic)))
}

/// The quinn server configuration, derived from [`rustls_server_config`].
pub fn server_config(identity: &Identity) -> Result<quinn::ServerConfig, EndpointError> {
    let crypto = rustls_server_config(identity)?;
    let quic = quinn::crypto::rustls::QuicServerConfig::try_from(crypto)
        .map_err(|error| EndpointError::Config(format!("QUIC refuses the TLS config: {error}")))?;
    Ok(quinn::ServerConfig::with_crypto(Arc::new(quic)))
}

/// A pinned server-certificate verifier.
pub struct PinnedServerVerifier {
    pinned: [u8; PUBLIC_KEY_BYTES],
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl PinnedServerVerifier {
    /// Pin exactly one public key.
    pub fn new(pinned: Option<PublicKey>) -> Result<Self, EndpointError> {
        let pinned = pinned.ok_or(EndpointError::MissingPin)?;
        Ok(Self {
            pinned,
            provider: provider(),
        })
    }

    /// The key this verifier accepts.
    pub fn pinned(&self) -> PublicKey {
        self.pinned
    }
}

impl std::fmt::Debug for PinnedServerVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinnedServerVerifier")
            .field("pinned", &hex_encode(&self.pinned))
            .finish_non_exhaustive()
    }
}

impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        // THE PIN IS ENFORCED HERE, on the certificate rustls actually
        // received. A check anywhere else would be decoration.
        if peer_key_matches(end_entity, &self.pinned) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(TlsError::General(
                "the mesh peer's certificate does not carry the pinned public key".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Does the certificate carry exactly `pinned`?
///
/// The comparison does not stop at the first differing byte; a pin's prefix is
/// not something timing should reveal.
pub fn peer_key_matches(cert: &CertificateDer<'_>, pinned: &PublicKey) -> bool {
    let Ok(public) = cert::public_key(cert) else {
        return false;
    };
    let mut difference = 0u8;
    for (received, expected) in public.iter().zip(pinned.iter()) {
        difference |= received ^ expected;
    }
    difference == 0
}

/// Close a connection that failed the hello check, with a reason the peer can
/// see, and return the typed refusal.
fn refuse_hello(connection: &quinn::Connection, error: HelloError) -> EndpointError {
    connection.close(HELLO_REFUSED.into(), b"mesh hello refused");
    EndpointError::Hello(error)
}

/// A QUIC endpoint carrying one identity.
///
/// It is [`Clone`]: every clone is another handle to the same socket and the
/// same server configuration, which is what lets an accept loop hand one
/// connection at a time to a task without moving the endpoint.
#[derive(Clone)]
pub struct MeshEndpoint {
    endpoint: quinn::Endpoint,
    identity: Identity,
}

impl MeshEndpoint {
    /// Bind a client-side endpoint. It dials; it has no server configuration.
    pub fn client(identity: Identity, bind: SocketAddr) -> Result<Self, EndpointError> {
        let endpoint = quinn::Endpoint::client(bind)?;
        Ok(Self { endpoint, identity })
    }

    /// Bind a server-side endpoint. It presents its certificate and learns
    /// each client's identity from the signed hello.
    pub fn server(identity: Identity, bind: SocketAddr) -> Result<Self, EndpointError> {
        let socket = std::net::UdpSocket::bind(bind)?;
        Self::server_with_socket(identity, socket)
    }

    /// Build a server-side endpoint on a socket the caller already bound and
    /// used — the seam a STUN exchange needs.
    ///
    /// The socket is handed to quinn unchanged. A NAT mapping belongs to the
    /// socket's 5-tuple, so a reflection asked on any other socket would name
    /// a port QUIC does not own; the caller asks on this one, then passes it
    /// here. quinn's runtime switches the socket to non-blocking mode when it
    /// takes ownership.
    pub fn server_with_socket(
        identity: Identity,
        socket: std::net::UdpSocket,
    ) -> Result<Self, EndpointError> {
        let config = server_config(&identity)?;
        let runtime = quinn::default_runtime().ok_or_else(|| {
            EndpointError::Config("no async runtime is running for the QUIC endpoint".to_owned())
        })?;
        let endpoint = quinn::Endpoint::new(
            quinn::EndpointConfig::default(),
            Some(config),
            socket,
            runtime,
        )?;
        Ok(Self { endpoint, identity })
    }

    /// The socket the endpoint actually bound (the port is real after `:0`).
    pub fn local_addr(&self) -> Result<SocketAddr, EndpointError> {
        self.endpoint.local_addr().map_err(Into::into)
    }

    /// The identity this endpoint presents.
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Connect to `peer`, accepting only a certificate carrying `pin`.
    ///
    /// This is the **transport** connection. A server that calls
    /// [`MeshEndpoint::accept_authenticated`] will refuse it until its first
    /// frame is a valid hello; use [`MeshEndpoint::connect_authenticated`]
    /// unless a test needs to send a deliberately bad first frame.
    pub async fn connect(
        &self,
        peer: SocketAddr,
        pin: Option<PublicKey>,
    ) -> Result<quinn::Connection, EndpointError> {
        let config = client_config(pin)?;
        let connecting = self
            .endpoint
            .connect_with(config, peer, SERVER_NAME)
            .map_err(|error| EndpointError::Handshake(error.to_string()))?;
        connecting
            .await
            .map_err(|error| EndpointError::Handshake(error.to_string()))
    }

    /// Accept one transport connection.
    ///
    /// **Hold the returned `Connection`.** Dropping the last handle closes
    /// the connection with an application close (code 0) — quinn's rule, not
    /// this wrapper's.
    pub async fn accept(&self) -> Result<quinn::Connection, EndpointError> {
        let incoming = self.endpoint.accept().await.ok_or(EndpointError::Closed)?;
        incoming
            .await
            .map_err(|error| EndpointError::Handshake(error.to_string()))
    }

    /// Connect, accept the server's session stream, read its challenge, and
    /// send a freshly signed hello that answers it.
    ///
    /// The hello is written before this returns, so the server can verify it
    /// before the caller sends anything else.
    pub async fn connect_authenticated(
        &self,
        peer: SocketAddr,
        pin: Option<PublicKey>,
        peer_id: &str,
    ) -> Result<PeerSession, EndpointError> {
        let Some(peer_key) = pin else {
            return Err(EndpointError::MissingPin);
        };
        let connection = self.connect(peer, Some(peer_key)).await?;

        // The server opens the stream and speaks first on it. Nothing is
        // signed until this connection's challenge has been read, so a hello
        // can never be lifted from another connection.
        let (mut send, mut recv) = connection
            .accept_bi()
            .await
            .map_err(|error| EndpointError::Stream(error.to_string()))?;
        let first = frame::read(&mut recv).await?;
        let challenge = Challenge::from_frame(&first)?;
        let hello = Hello::sign(&self.identity, peer_id, &challenge);
        frame::write(&mut send, &hello.to_frame()?).await?;
        Ok(PeerSession {
            connection,
            peer: peer_key,
            hello,
            send,
            recv,
        })
    }

    /// Accept one incoming connection **without completing its handshake**.
    ///
    /// This is the primitive an accept loop uses when it must not wait for a
    /// stranger: the loop takes one [`quinn::Incoming`] per iteration and
    /// hands it to [`MeshEndpoint::accept_authenticated_within`] in its own
    /// task, so a connection that never answers cannot block the next.
    pub async fn accept_incoming(&self) -> Result<quinn::Incoming, EndpointError> {
        self.endpoint.accept().await.ok_or(EndpointError::Closed)
    }

    /// Accept a connection, issue a fresh challenge, and accept only a hello
    /// that echoes it and verifies against the pin the resolver returns.
    ///
    /// The challenge is written before the first frame is read, so the dialer
    /// has this connection's fresh value and nothing it recorded earlier can
    /// answer for it. A non-hello first frame, an unknown peer, a wrong or
    /// missing echo and a bad signature all close the connection and return
    /// [`EndpointError::Hello`]. The check happens before any other frame is
    /// read.
    ///
    /// The returned stream **is** the authenticated session. A second stream
    /// opened on the same connection is outside the hello's coverage; serve
    /// only the stream returned here.
    ///
    /// There is no deadline here: a caller that accepts strangers should use
    /// [`MeshEndpoint::accept_authenticated_within`] (or
    /// [`MeshEndpoint::accept_incoming`] plus that method), because this call
    /// waits for the hello on the one connection it was given.
    pub async fn accept_authenticated(
        &self,
        resolver: &dyn PinResolver,
    ) -> Result<PeerSession, EndpointError> {
        let connection = self.accept().await?;
        Self::authenticate_connection(connection, resolver).await
    }

    /// Authenticate one already-accepted connection under a deadline that
    /// covers the QUIC handshake, the challenge, the hello and its
    /// verification.
    ///
    /// On expiry the connection is closed explicitly with
    /// [`HELLO_REFUSED`] and [`EndpointError::HandshakeTimeout`] is returned.
    /// The caller can keep the accept loop moving: this future owns one
    /// connection and does not touch any other.
    pub async fn accept_authenticated_within(
        &self,
        incoming: quinn::Incoming,
        resolver: &dyn PinResolver,
        handshake_timeout: Duration,
    ) -> Result<PeerSession, EndpointError> {
        let deadline = tokio::time::Instant::now() + handshake_timeout;

        // `Incoming`'s own Drop refuses the connection, so a stranger that
        // never completes the QUIC handshake is closed too.
        let connection = match tokio::time::timeout_at(deadline, incoming).await {
            Ok(Ok(connection)) => connection,
            Ok(Err(error)) => return Err(EndpointError::Handshake(error.to_string())),
            Err(_) => return Err(EndpointError::HandshakeTimeout),
        };

        // Keep a handle for the close below: the future that fails the
        // deadline is dropped, and dropping a future is not a promise about
        // when its connection closes.
        let close = connection.clone();
        match tokio::time::timeout_at(
            deadline,
            Self::authenticate_connection(connection, resolver),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                close.close(HELLO_REFUSED.into(), b"mesh handshake timed out");
                Err(EndpointError::HandshakeTimeout)
            }
        }
    }

    /// The challenge/hello exchange on one accepted connection.
    async fn authenticate_connection(
        connection: quinn::Connection,
        resolver: &dyn PinResolver,
    ) -> Result<PeerSession, EndpointError> {
        // The server opens the bidirectional stream and speaks first on it.
        // The challenge is the whole replay defence: it is fresh for this
        // connection and signed by the answer below.
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|error| EndpointError::Stream(error.to_string()))?;
        let challenge = Challenge::generate();
        frame::write(&mut send, &challenge.to_frame()?).await?;

        // The first frame from the client is the application-layer
        // authentication. A non-hello, an unknown peer, a wrong echo, or a bad
        // signature closes the connection before anything else is read.
        let first = frame::read(&mut recv).await?;
        let hello = match Hello::from_frame(&first) {
            Ok(hello) => hello,
            Err(error) => return Err(refuse_hello(&connection, error)),
        };
        let Some(peer) = resolver.resolve(&hello.peer_id_hex) else {
            return Err(refuse_hello(
                &connection,
                HelloError::UnknownPeer(hello.peer_id_hex.clone()),
            ));
        };
        if let Err(error) = hello.verify(&peer, &challenge) {
            return Err(refuse_hello(&connection, error));
        }

        Ok(PeerSession {
            connection,
            peer,
            hello,
            send,
            recv,
        })
    }

    /// Close the endpoint and every connection on it.
    pub fn close(&self, code: u32, reason: &[u8]) {
        self.endpoint.close(code.into(), reason);
    }
}

/// An authenticated peer session: the connection, the resolved peer key, the
/// client's hello, and the bidirectional stream both sides already have.
#[derive(Debug)]
pub struct PeerSession {
    connection: quinn::Connection,
    peer: PublicKey,
    hello: Hello,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

impl PeerSession {
    /// The peer's pinned public key.
    pub fn peer(&self) -> PublicKey {
        self.peer
    }

    /// The hello that authenticated this session.
    pub fn hello(&self) -> &Hello {
        &self.hello
    }

    /// The underlying QUIC connection.
    pub fn connection(&self) -> &quinn::Connection {
        &self.connection
    }

    /// The send half of the session stream.
    pub fn send(&mut self) -> &mut quinn::SendStream {
        &mut self.send
    }

    /// The receive half of the session stream.
    pub fn recv(&mut self) -> &mut quinn::RecvStream {
        &mut self.recv
    }

    /// Consume the session into its parts, keeping the connection alive.
    pub fn into_parts(
        self,
    ) -> (
        quinn::Connection,
        PublicKey,
        Hello,
        quinn::SendStream,
        quinn::RecvStream,
    ) {
        (self.connection, self.peer, self.hello, self.send, self.recv)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use serde_json::json;
    use tokio::io::AsyncWriteExt;
    use tokio::time::timeout;

    use super::*;
    use crate::frame::{self, Frame, Kind};

    fn loopback() -> SocketAddr {
        "127.0.0.1:0".parse().expect("a loopback address")
    }

    fn test_timeout() -> Duration {
        Duration::from_secs(5)
    }

    fn resolver(entries: &[(&str, PublicKey)]) -> HashMap<String, PublicKey> {
        entries
            .iter()
            .map(|(peer_id, key)| ((*peer_id).to_string(), *key))
            .collect()
    }

    /// Open a transport connection and read the server's challenge **without
    /// answering it**. The stream is where a hello would go, so a test can
    /// send any bytes it likes.
    async fn raw_client(
        client: &MeshEndpoint,
        address: SocketAddr,
        server_key: PublicKey,
    ) -> (
        quinn::Connection,
        quinn::SendStream,
        quinn::RecvStream,
        Challenge,
    ) {
        let connection = timeout(test_timeout(), client.connect(address, Some(server_key)))
            .await
            .expect("the handshake timed out")
            .expect("the right pin connects");
        let (send, mut recv) = connection
            .accept_bi()
            .await
            .expect("the server opens the session stream");
        let first = timeout(test_timeout(), frame::read(&mut recv))
            .await
            .expect("the challenge timed out")
            .expect("the challenge arrives");
        let challenge =
            Challenge::from_frame(&first).expect("the server's first frame is a challenge");
        (connection, send, recv, challenge)
    }

    #[test]
    fn the_pin_accepts_the_right_certificate_and_rejects_a_wrong_one() {
        let identity = Identity::generate();
        let certificate = cert::self_signed(&identity).expect("a certificate");
        let other = Identity::generate();

        assert!(peer_key_matches(&certificate, &identity.public_bytes()));
        assert!(!peer_key_matches(&certificate, &other.public_bytes()));
    }

    #[test]
    fn a_certificate_that_is_not_ed25519_is_rejected() {
        let garbage = CertificateDer::from(vec![0u8; 48]);
        assert!(!peer_key_matches(&garbage, &[0u8; PUBLIC_KEY_BYTES]));
    }

    #[test]
    fn a_missing_pin_is_never_an_accept_all_verifier() {
        assert_eq!(
            PinnedServerVerifier::new(None).unwrap_err(),
            EndpointError::MissingPin
        );
        assert_eq!(client_config(None).unwrap_err(), EndpointError::MissingPin);
    }

    #[test]
    fn the_alpn_names_the_current_wire_version() {
        assert_eq!(
            ALPN,
            format!("summrise/mesh/{PROTOCOL_VERSION}").as_bytes(),
            "the ALPN must name the version the hello speaks"
        );
    }

    #[tokio::test]
    async fn a_prebound_socket_is_the_endpoint_s_socket() {
        let identity = Identity::generate();
        let socket = std::net::UdpSocket::bind(loopback()).expect("a prebound socket");
        let address = socket.local_addr().expect("its address");

        let endpoint =
            MeshEndpoint::server_with_socket(identity, socket).expect("an endpoint on that socket");

        assert_eq!(
            endpoint.local_addr().expect("the endpoint's address"),
            address,
            "the endpoint must own the very socket it was handed, not bind another"
        );
    }

    #[tokio::test]
    async fn a_known_peer_with_a_valid_hello_is_accepted_and_can_exchange_a_frame() {
        let server_identity = Identity::generate();
        let client_identity = Identity::generate();
        let server_key = server_identity.public_bytes();
        let client_key = client_identity.public_bytes();
        let resolver = resolver(&[("client-1", client_key)]);

        let server = MeshEndpoint::server(server_identity, loopback()).expect("a server endpoint");
        let client = MeshEndpoint::client(client_identity, loopback()).expect("a client endpoint");
        let address = server.local_addr().expect("a bound port");
        let server_task = tokio::spawn(async move { server.accept_authenticated(&resolver).await });

        let mut client_session = timeout(
            test_timeout(),
            client.connect_authenticated(address, Some(server_key), "client-1"),
        )
        .await
        .expect("the pinned handshake timed out")
        .expect("the right pin connects");
        let mut server_session = timeout(test_timeout(), server_task)
            .await
            .expect("the server timed out")
            .expect("the server did not panic")
            .expect("a known peer with a valid hello is accepted");

        assert_eq!(server_session.peer(), client_key);
        assert_eq!(server_session.hello().peer_id_hex, "client-1");
        assert_eq!(
            server_session.hello().challenge,
            client_session.hello().challenge,
            "the hello must echo the challenge this connection issued"
        );

        let ping = Frame {
            kind: Kind::Ping,
            id: 42,
            payload: json!({"nonce": 7}),
        };
        frame::write(client_session.send(), &ping)
            .await
            .expect("write the ping");
        let received = frame::read(server_session.recv())
            .await
            .expect("the server reads the ping");
        assert_eq!(received, ping);

        let response = Frame {
            kind: Kind::Res,
            id: received.id,
            payload: received.payload.clone(),
        };
        frame::write(server_session.send(), &response)
            .await
            .expect("write the response");
        let echoed = frame::read(client_session.recv())
            .await
            .expect("the client reads the response");

        assert_eq!(echoed.kind, Kind::Res);
        assert_eq!(echoed.id, 42);
        assert_eq!(echoed.payload, ping.payload, "the response echoes the ping");
    }

    #[tokio::test]
    async fn an_unknown_peer_is_refused() {
        let server_identity = Identity::generate();
        let client_identity = Identity::generate();
        let server_key = server_identity.public_bytes();
        let resolver = resolver(&[("client-1", client_identity.public_bytes())]);

        let server = MeshEndpoint::server(server_identity, loopback()).expect("a server endpoint");
        let client =
            MeshEndpoint::client(Identity::generate(), loopback()).expect("a client endpoint");
        let address = server.local_addr().expect("a bound port");
        let server_task = tokio::spawn(async move { server.accept_authenticated(&resolver).await });

        let (_connection, mut send, _recv, challenge) =
            raw_client(&client, address, server_key).await;
        let hello = Hello::sign(&Identity::generate(), "stranger", &challenge);
        frame::write(&mut send, &hello.to_frame().expect("a hello frame"))
            .await
            .expect("write the hello");

        let error = timeout(test_timeout(), server_task)
            .await
            .expect("the server timed out")
            .expect("the server did not panic")
            .expect_err("an unknown peer must be refused");
        assert_eq!(
            error,
            EndpointError::Hello(HelloError::UnknownPeer("stranger".to_string()))
        );
    }

    #[tokio::test]
    async fn a_hello_signed_by_another_key_is_refused() {
        let server_identity = Identity::generate();
        let allowed = Identity::generate();
        let attacker = Identity::generate();
        let server_key = server_identity.public_bytes();
        let resolver = resolver(&[("client-1", allowed.public_bytes())]);

        let server = MeshEndpoint::server(server_identity, loopback()).expect("a server endpoint");
        let client =
            MeshEndpoint::client(Identity::generate(), loopback()).expect("a client endpoint");
        let address = server.local_addr().expect("a bound port");
        let server_task = tokio::spawn(async move { server.accept_authenticated(&resolver).await });

        let (_connection, mut send, _recv, challenge) =
            raw_client(&client, address, server_key).await;
        let hello = Hello::sign(&attacker, "client-1", &challenge);
        frame::write(&mut send, &hello.to_frame().expect("a hello frame"))
            .await
            .expect("write the hello");

        let error = timeout(test_timeout(), server_task)
            .await
            .expect("the server timed out")
            .expect("the server did not panic")
            .expect_err("a hello signed by the wrong key must be refused");
        assert_eq!(error, EndpointError::Hello(HelloError::BadSignature));
    }

    #[tokio::test]
    async fn a_hello_that_is_not_the_first_frame_is_refused() {
        let server_identity = Identity::generate();
        let client_identity = Identity::generate();
        let server_key = server_identity.public_bytes();
        let resolver = resolver(&[("client-1", client_identity.public_bytes())]);

        let server = MeshEndpoint::server(server_identity, loopback()).expect("a server endpoint");
        let client =
            MeshEndpoint::client(client_identity.clone(), loopback()).expect("a client endpoint");
        let address = server.local_addr().expect("a bound port");
        let server_task = tokio::spawn(async move { server.accept_authenticated(&resolver).await });

        let (_connection, mut send, _recv, challenge) =
            raw_client(&client, address, server_key).await;
        let first = Frame {
            kind: Kind::Ping,
            id: 1,
            payload: json!({}),
        };
        frame::write(&mut send, &first)
            .await
            .expect("write a non-hello first frame");
        // The server must refuse on the first frame; a hello that follows it
        // must not be read. The write may race the close, which is fine.
        let hello = Hello::sign(&client_identity, "client-1", &challenge);
        let _ = frame::write(&mut send, &hello.to_frame().expect("a hello frame")).await;

        let error = timeout(test_timeout(), server_task)
            .await
            .expect("the server timed out")
            .expect("the server did not panic")
            .expect_err("a hello that is not first must be refused");
        assert_eq!(error, EndpointError::Hello(HelloError::NotFirstFrame));
    }

    #[tokio::test]
    async fn a_hello_with_a_wrong_challenge_is_refused() {
        let server_identity = Identity::generate();
        let client_identity = Identity::generate();
        let server_key = server_identity.public_bytes();
        let resolver = resolver(&[("client-1", client_identity.public_bytes())]);

        let server = MeshEndpoint::server(server_identity, loopback()).expect("a server endpoint");
        let client =
            MeshEndpoint::client(client_identity.clone(), loopback()).expect("a client endpoint");
        let address = server.local_addr().expect("a bound port");

        // A hello validly signed for a challenge this connection never
        // issued: the echo check is what must refuse it.
        let wrong = Challenge::generate();
        let (_client_result, server_result) = timeout(test_timeout(), async {
            tokio::join!(
                async {
                    let (connection, mut send, _recv, _challenge) =
                        raw_client(&client, address, server_key).await;
                    let hello = Hello::sign(&client_identity, "client-1", &wrong);
                    frame::write(&mut send, &hello.to_frame().expect("a hello frame"))
                        .await
                        .expect("write the hello");
                    // Hold the connection open until the server has refused;
                    // dropping it first would close the connection and turn
                    // the typed refusal into a stream error.
                    connection
                },
                server.accept_authenticated(&resolver),
            )
        })
        .await
        .expect("the handshake timed out");

        assert_eq!(
            server_result.expect_err("a hello for another challenge must be refused"),
            EndpointError::Hello(HelloError::ChallengeMismatch)
        );
    }

    #[tokio::test]
    async fn a_hello_with_a_missing_challenge_field_is_refused() {
        let server_identity = Identity::generate();
        let client_identity = Identity::generate();
        let server_key = server_identity.public_bytes();
        let resolver = resolver(&[("client-1", client_identity.public_bytes())]);

        let server = MeshEndpoint::server(server_identity, loopback()).expect("a server endpoint");
        let client =
            MeshEndpoint::client(client_identity.clone(), loopback()).expect("a client endpoint");
        let address = server.local_addr().expect("a bound port");

        let (_client_result, server_result) = timeout(test_timeout(), async {
            tokio::join!(
                async {
                    let (connection, mut send, _recv, challenge) =
                        raw_client(&client, address, server_key).await;
                    let hello = Hello::sign(&client_identity, "client-1", &challenge);
                    let mut value = serde_json::to_value(&hello).expect("a hello is serializable");
                    value
                        .as_object_mut()
                        .expect("a hello is an object")
                        .remove("challenge");
                    let frame = Frame {
                        kind: Kind::Hello,
                        id: 0,
                        payload: value,
                    };
                    frame::write(&mut send, &frame)
                        .await
                        .expect("write the malformed hello");
                    // Hold the connection open until the server has refused.
                    connection
                },
                server.accept_authenticated(&resolver),
            )
        })
        .await
        .expect("the handshake timed out");

        assert!(
            matches!(
                server_result.expect_err("a hello with no challenge field must be refused"),
                EndpointError::Hello(HelloError::Malformed(_))
            ),
            "the missing field is a malformed hello"
        );
    }

    #[tokio::test]
    async fn a_replayed_hello_is_refused_on_a_new_connection() {
        let server_identity = Identity::generate();
        let client_identity = Identity::generate();
        let server_key = server_identity.public_bytes();
        let resolver = resolver(&[("client-1", client_identity.public_bytes())]);

        let server = MeshEndpoint::server(server_identity, loopback()).expect("a server endpoint");
        let client =
            MeshEndpoint::client(client_identity.clone(), loopback()).expect("a client endpoint");
        let address = server.local_addr().expect("a bound port");

        // Connection A: a real handshake, with the exact hello bytes recorded.
        let (client_a, server_a) = timeout(test_timeout(), async {
            tokio::join!(
                async {
                    let (connection, mut send, _recv, challenge) =
                        raw_client(&client, address, server_key).await;
                    let hello = Hello::sign(&client_identity, "client-1", &challenge);
                    let encoded = frame::encode(&hello.to_frame().expect("a hello frame"))
                        .expect("the hello's wire bytes");
                    frame::write(&mut send, &hello.to_frame().expect("a hello frame"))
                        .await
                        .expect("write the hello");
                    (connection, challenge, encoded)
                },
                server.accept_authenticated(&resolver),
            )
        })
        .await
        .expect("A's handshake timed out");
        let (_connection_a, challenge_a, replayed) = client_a;
        let server_a = server_a.expect("A's valid hello is accepted");
        assert_eq!(server_a.hello().challenge, challenge_a.bytes);

        // Connection B: a fresh challenge, then A's bytes verbatim.
        let (client_b, server_b) = timeout(test_timeout(), async {
            tokio::join!(
                async {
                    let (connection, mut send, _recv, challenge) =
                        raw_client(&client, address, server_key).await;
                    let _ = send.write_all(&replayed).await;
                    let _ = send.flush().await;
                    (connection, challenge)
                },
                server.accept_authenticated(&resolver),
            )
        })
        .await
        .expect("B's handshake timed out");
        let (_connection_b, challenge_b) = client_b;

        assert_ne!(
            challenge_a.bytes, challenge_b.bytes,
            "every connection must get its own challenge"
        );
        assert_eq!(
            server_b.expect_err("a recorded hello must not authenticate a new connection"),
            EndpointError::Hello(HelloError::ChallengeMismatch),
            "the replayed hello answers the old challenge, not this one"
        );
        let closed = timeout(test_timeout(), _connection_b.closed()).await;
        assert!(
            closed.is_ok(),
            "the server must close a connection whose hello it refused"
        );
    }

    #[tokio::test]
    async fn two_connections_from_one_peer_get_independent_challenges() {
        let server_identity = Identity::generate();
        let client_identity = Identity::generate();
        let server_key = server_identity.public_bytes();
        let client_key = client_identity.public_bytes();
        let resolver = resolver(&[("client-1", client_key)]);

        let server = MeshEndpoint::server(server_identity, loopback()).expect("a server endpoint");
        let client = MeshEndpoint::client(client_identity, loopback()).expect("a client endpoint");
        let address = server.local_addr().expect("a bound port");

        let (client_one, client_two, server_one, server_two) = timeout(test_timeout(), async {
            tokio::join!(
                client.connect_authenticated(address, Some(server_key), "client-1"),
                client.connect_authenticated(address, Some(server_key), "client-1"),
                server.accept_authenticated(&resolver),
                server.accept_authenticated(&resolver),
            )
        })
        .await
        .expect("the concurrent handshakes timed out");

        let mut client_one = client_one.expect("the first client connection is accepted");
        let mut client_two = client_two.expect("the second client connection is accepted");
        let mut server_one = server_one.expect("the first server session is authenticated");
        let mut server_two = server_two.expect("the second server session is authenticated");

        assert_eq!(server_one.peer(), client_key);
        assert_eq!(server_two.peer(), client_key);
        assert_eq!(server_one.hello().peer_id_hex, "client-1");
        assert_eq!(server_two.hello().peer_id_hex, "client-1");

        let server_challenges = [server_one.hello().challenge, server_two.hello().challenge];
        let client_challenges = [client_one.hello().challenge, client_two.hello().challenge];
        assert_ne!(
            server_challenges[0], server_challenges[1],
            "two connections must not share one challenge"
        );
        assert!(
            server_challenges
                .iter()
                .all(|challenge| client_challenges.contains(challenge)),
            "each connection's hello must echo that connection's challenge"
        );

        // Both streams are live and independent: whatever the pairing, each
        // server session receives one of the two pings.
        let first = Frame {
            kind: Kind::Ping,
            id: 1,
            payload: json!({}),
        };
        let second = Frame {
            kind: Kind::Ping,
            id: 2,
            payload: json!({}),
        };
        frame::write(client_one.send(), &first)
            .await
            .expect("write the first ping");
        frame::write(client_two.send(), &second)
            .await
            .expect("write the second ping");
        let mut ids = [
            frame::read(server_one.recv())
                .await
                .expect("the first server session reads a ping")
                .id,
            frame::read(server_two.recv())
                .await
                .expect("the second server session reads a ping")
                .id,
        ];
        ids.sort_unstable();
        assert_eq!(ids, [1, 2], "both authenticated streams must carry frames");
    }

    #[tokio::test]
    async fn the_wrong_pin_fails_the_handshake() {
        let server_identity = Identity::generate();
        let client_identity = Identity::generate();
        let server = MeshEndpoint::server(server_identity, loopback()).expect("a server endpoint");
        let client = MeshEndpoint::client(client_identity, loopback()).expect("a client endpoint");
        let address = server.local_addr().expect("a bound port");
        let wrong = Identity::generate().public_bytes();
        let server_task = tokio::spawn(async move { server.accept().await });

        let error = timeout(test_timeout(), client.connect(address, Some(wrong)))
            .await
            .expect("the wrong-pin handshake timed out instead of failing")
            .expect_err("a wrong pin must not connect");

        assert!(
            matches!(error, EndpointError::Handshake(_)),
            "the failure must be a handshake refusal, got {error:?}"
        );
        let _ = timeout(test_timeout(), server_task).await;
    }

    #[tokio::test]
    async fn a_missing_pin_is_a_typed_error_before_any_handshake() {
        let client =
            MeshEndpoint::client(Identity::generate(), loopback()).expect("a client endpoint");
        // A port nothing listens on: a missing pin must be caught before the
        // socket is even asked to connect.
        let error = client
            .connect("127.0.0.1:1".parse().expect("an address"), None)
            .await
            .expect_err("no pin must be refused");
        assert_eq!(error, EndpointError::MissingPin);
    }
}
