//! TLS through the relay splice: the floor carries ciphertext, not payload.
//!
//! [`crate::relay`] pairs two TCP clients by session id and forwards opaque,
//! length-prefixed bodies. That is enough to carry a byte stream, not enough
//! to keep the relay from reading it: the relay still owns the socket the
//! frames cross. This module puts a TLS session **inside** the splice, so the
//! relay forwards records it cannot decrypt, and the frames travel exactly as
//! they do over a QUIC stream ([`crate::frame`]).
//!
//! **The roles are computed, not observed.** The relay says nothing about
//! which peer arrived first, and arrival order is not a fact both sides can
//! agree on. [`RelayTlsSession::for_peer`] derives the role from the two
//! public keys instead: the **lower** key is the TLS **server** and presents
//! the self-signed certificate [`crate::cert`] builds; the higher key is the
//! **client** and pins the server. Both peers have both keys (the rendezvous
//! publishes the peer's key in hex), so they compute the same answer and a
//! mismatched pair is not reachable through that constructor. A missing pin
//! is [`RelayTlsError::MissingPin`], never an accept-any verifier, and a wrong
//! pin fails the handshake — the comparison happens in the rustls verifier,
//! on the certificate rustls actually received, exactly as in
//! [`crate::endpoint`].
//!
//! **The client's identity.** TLS proves the server the client dialed; it
//! does not prove the client. So after the handshake the server sends the same
//! fresh [`Challenge`] `endpoint` sends as its first frame, and the client
//! answers with the signed [`Hello`] `endpoint` uses. With `for_peer` the
//! hello names the client by its public key in hex — the same peer id the
//! rendezvous uses — and the server resolves that id through a
//! [`PinResolver`], checks the challenge echo in constant time, and verifies
//! the signature before any other frame. A hello that does not verify closes
//! the connection with [`RelayTlsError::Hello`]; no frame is delivered. The
//! challenge is fresh per connection, so a recorded hello cannot be replayed.
//!
//! **The relay is unchanged.** It still pairs by session id and still forwards
//! length-prefixed bodies without parsing them. It never learns either peer
//! key. Because a relayed body is a length-prefixed chunk rather than a raw
//! byte, this module adapts that chunk pipe into the byte stream rustls reads
//! and writes: each TLS record becomes one body, and the relay sees only the
//! four-byte length and the ciphertext.

use std::io;
use std::sync::Arc;

use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::endpoint;
use crate::frame::{self, Frame, FrameError, LENGTH_PREFIX_BYTES, MAX_FRAME_BYTES};
use crate::hello::{Challenge, Hello, HelloError, PinResolver, PublicKey};
use crate::keys::Identity;

/// Why a relayed TLS session could not be built or used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayTlsError {
    /// No peer key was supplied. There is no accept-any verifier here.
    MissingPin,
    /// A rustls configuration could not be built from the identity.
    Config(String),
    /// The TLS handshake failed — including a pin mismatch, and including a
    /// transport error during the handshake.
    Handshake(String),
    /// A frame could not be read or written over the TLS stream.
    Frame(FrameError),
    /// The client's hello was not acceptable.
    Hello(HelloError),
    /// Both sides of `for_peer` named the same key; there is no lower key.
    SamePeer,
}

impl std::fmt::Display for RelayTlsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPin => f.write_str("a peer pin is required; refusing to accept any key"),
            Self::Config(message) => {
                write!(f, "the relay TLS configuration is unusable: {message}")
            }
            Self::Handshake(message) => write!(f, "the relay TLS handshake failed: {message}"),
            Self::Frame(error) => write!(f, "{error}"),
            Self::Hello(error) => write!(f, "{error}"),
            Self::SamePeer => {
                f.write_str("for_peer needs two different mesh identities, not one key twice")
            }
        }
    }
}

impl std::error::Error for RelayTlsError {}

impl From<FrameError> for RelayTlsError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

impl From<HelloError> for RelayTlsError {
    fn from(error: HelloError) -> Self {
        Self::Hello(error)
    }
}

/// Turn a paired relay splice into a raw byte stream for rustls.
///
/// The relay speaks length-prefixed bodies; rustls speaks bytes. One task
/// bridges the two: it reassembles each body for the TLS reader and writes
/// each TLS chunk as one body. The relay therefore still forwards an opaque
/// body and never has to know that TLS is inside it. The duplex between the
/// task and rustls is bounded, so a slow reader applies backpressure instead
/// of growing a queue.
fn relay_duplex(io: TcpStream) -> tokio::io::DuplexStream {
    let (tls_side, bridge_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (mut relay_read, mut relay_write) = io.into_split();
        let (mut from_tls, mut to_tls) = tokio::io::split(bridge_side);

        let body_in = async {
            while let Some(body) = read_body(&mut relay_read).await? {
                to_tls.write_all(&body).await?;
            }
            to_tls.shutdown().await
        };
        let body_out = async {
            let mut buffer = [0u8; 16 * 1024];
            loop {
                let read = from_tls.read(&mut buffer).await?;
                if read == 0 {
                    relay_write.shutdown().await?;
                    return Ok(());
                }
                write_body(&mut relay_write, &buffer[..read]).await?;
            }
        };

        // Whichever direction ends first ends the bridge; dropping the other
        // half closes the relay socket.
        let _ = tokio::select! {
            result = body_in => result,
            result = body_out => result,
        };
    });
    tls_side
}

/// Read one relayed body, or `None` at a clean body boundary.
async fn read_body<R>(reader: &mut R) -> io::Result<Option<Vec<u8>>>
where
    R: AsyncRead + Unpin,
{
    let mut prefix = [0u8; LENGTH_PREFIX_BYTES];
    let mut filled = 0;
    while filled < prefix.len() {
        let read = reader.read(&mut prefix[filled..]).await?;
        if read == 0 {
            if filled == 0 {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "a relay body prefix ended in the middle",
            ));
        }
        filled += read;
    }
    let length = u32::from_be_bytes(prefix) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a relay body announced {length} bytes; the cap is {MAX_FRAME_BYTES}"),
        ));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).await?;
    Ok(Some(body))
}

/// Write one relayed body and flush it to the relay.
async fn write_body<W>(writer: &mut W, body: &[u8]) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&(body.len() as u32).to_be_bytes()).await?;
    writer.write_all(body).await?;
    writer.flush().await
}

/// The parts of a TLS stream this module uses, as one type.
///
/// tokio-rustls returns a different concrete stream for each role; a session
/// is one type so a caller that is handed a session does not have to know
/// which side it is on. Every operation still goes through rustls, not around
/// it.
trait AsyncStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncStream for T {}

/// The TLS role [`RelayTlsSession::for_peer`] assigns from the two keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayTlsRole {
    /// The peer holding the lower public key; it presents the certificate.
    Server,
    /// The peer holding the higher public key; it pins the server.
    Client,
}

/// A TLS session running through a paired relay splice.
pub struct RelayTlsSession {
    stream: Box<dyn AsyncStream>,
    peer: PublicKey,
    hello: Hello,
    role: RelayTlsRole,
}

impl RelayTlsSession {
    /// Pair two peers deterministically: the **lower** public key is the TLS
    /// server, and the higher key pins it.
    ///
    /// The relay's pairing answer carries no arrival order, so this is the
    /// constructor the mesh uses. Both sides pass their own identity and the
    /// key the rendezvous published for the peer; the same `{my, peer}` facts
    /// produce complementary roles, and equal keys are refused.
    ///
    /// The client's hello names it by its public key in hex. `resolver` is the
    /// server's directory for that id: it must map the client's hex key to the
    /// client's key. The caller already has the peer key, so a one-entry
    /// resolver is enough.
    pub async fn for_peer(
        my: &Identity,
        peer: &PublicKey,
        stream: TcpStream,
        resolver: &dyn PinResolver,
    ) -> Result<Self, RelayTlsError> {
        let my_key = my.public_bytes();
        if my_key == *peer {
            return Err(RelayTlsError::SamePeer);
        }
        // Exact bytes, not the hex spelling: case is not part of a key.
        if my_key < *peer {
            Self::server(stream, my.clone(), resolver).await
        } else {
            Self::client(stream, my.clone(), my.public_hex(), Some(*peer)).await
        }
    }

    /// Join a paired splice as the TLS **client**, pin `pin`, read the
    /// server's challenge, and answer it with a freshly signed hello for
    /// `peer_id`.
    ///
    /// Private: [`RelayTlsSession::for_peer`] owns the role decision. Tests
    /// use this directly to exercise one side or to force a wrong role.
    ///
    /// The pin is checked before the handshake starts: `None` is
    /// [`RelayTlsError::MissingPin`], not an accept-any connection. The hello
    /// is written before this returns, so the server can verify it before the
    /// caller sends anything else.
    async fn client(
        stream: TcpStream,
        identity: Identity,
        peer_id: impl Into<String>,
        pin: Option<PublicKey>,
    ) -> Result<Self, RelayTlsError> {
        let pin = pin.ok_or(RelayTlsError::MissingPin)?;
        let config = endpoint::rustls_client_config(Some(pin))
            .map_err(|error| RelayTlsError::Config(error.to_string()))?;
        let connector = TlsConnector::from(Arc::new(config));
        // The name is not authenticated — the pin is — but rustls requires
        // one. It is the same name QUIC is given in `endpoint`.
        let server_name = ServerName::try_from(endpoint::SERVER_NAME)
            .map_err(|error| RelayTlsError::Config(error.to_string()))?;
        let mut stream = connector
            .connect(server_name, relay_duplex(stream))
            .await
            .map_err(|error| RelayTlsError::Handshake(error.to_string()))?;

        // The server speaks first. Nothing is signed until its challenge for
        // this connection has been read, so a hello cannot be lifted from
        // another connection.
        let first = frame::read(&mut stream).await?;
        let challenge = Challenge::from_frame(&first)?;
        let hello = Hello::sign(&identity, peer_id, &challenge);
        frame::write(&mut stream, &hello.to_frame()?).await?;
        Ok(Self {
            stream: Box::new(stream),
            peer: pin,
            hello,
            role: RelayTlsRole::Client,
        })
    }

    /// Join a paired splice as the TLS **server**, issue a fresh challenge,
    /// and accept only a hello that echoes it and verifies against `resolver`.
    ///
    /// Private: [`RelayTlsSession::for_peer`] owns the role decision.
    ///
    /// The challenge is written before the first frame is read. An unknown
    /// peer, a wrong or missing echo, a bad signature and a non-hello first
    /// frame all close the TLS connection with a close-notify and return
    /// [`RelayTlsError::Hello`]; no application frame is ever delivered.
    async fn server(
        stream: TcpStream,
        identity: Identity,
        resolver: &dyn PinResolver,
    ) -> Result<Self, RelayTlsError> {
        let config = endpoint::rustls_server_config(&identity)
            .map_err(|error| RelayTlsError::Config(error.to_string()))?;
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let mut stream = acceptor
            .accept(relay_duplex(stream))
            .await
            .map_err(|error| RelayTlsError::Handshake(error.to_string()))?;

        // The challenge is the whole replay defence: it is fresh for this
        // connection and signed by the answer below.
        let challenge = Challenge::generate();
        frame::write(&mut stream, &challenge.to_frame()?).await?;

        // The first frame is the whole application-layer authentication. It is
        // read before anything else, and the connection is closed if it is
        // not a valid hello for a peer the resolver pins and this challenge.
        let first = frame::read(&mut stream).await?;
        let hello = match Hello::from_frame(&first) {
            Ok(hello) => hello,
            Err(error) => return Err(refuse_hello(stream, error).await),
        };
        let Some(peer) = resolver.resolve(&hello.peer_id_hex) else {
            let peer_id = hello.peer_id_hex.clone();
            return Err(refuse_hello(stream, HelloError::UnknownPeer(peer_id)).await);
        };
        if let Err(error) = hello.verify(&peer, &challenge) {
            return Err(refuse_hello(stream, error).await);
        }

        Ok(Self {
            stream: Box::new(stream),
            peer,
            hello,
            role: RelayTlsRole::Server,
        })
    }

    /// The role this session holds.
    pub fn role(&self) -> RelayTlsRole {
        self.role
    }

    /// The peer's pinned public key.
    pub fn peer(&self) -> PublicKey {
        self.peer
    }

    /// The hello that authenticated the client.
    pub fn hello(&self) -> &Hello {
        &self.hello
    }

    /// Send one frame over the TLS stream.
    pub async fn send_frame(&mut self, frame: &Frame) -> Result<(), RelayTlsError> {
        frame::write(&mut self.stream, frame)
            .await
            .map_err(Into::into)
    }

    /// Read one frame from the TLS stream.
    pub async fn recv_frame(&mut self) -> Result<Frame, RelayTlsError> {
        frame::read(&mut self.stream).await.map_err(Into::into)
    }
}

/// Close a connection whose hello failed, with a TLS close-notify the peer can
/// see, and return the typed refusal.
async fn refuse_hello<W>(mut stream: W, error: HelloError) -> RelayTlsError
where
    W: AsyncWrite + Unpin,
{
    let _ = stream.shutdown().await;
    RelayTlsError::Hello(error)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use serde_json::json;
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::{sleep, timeout};

    use super::*;
    use crate::frame::{self, Frame, Kind};
    use crate::keys::hex_encode;
    use crate::relay::{RelayClient, RelayServer};

    const PATIENCE: Duration = Duration::from_secs(5);

    fn loopback() -> std::net::SocketAddr {
        "127.0.0.1:0".parse().expect("a loopback address")
    }

    /// Start a real relay on an ephemeral loopback port.
    async fn start_relay() -> std::net::SocketAddr {
        let server = RelayServer::bind(loopback(), PATIENCE)
            .await
            .expect("a relay");
        let address = server.local_addr();
        tokio::spawn(async move { server.run().await });
        address
    }

    /// Ask the relay to pair, and hand back the opaque stream the TLS session
    /// runs through. It returns only once the relay has paired this end.
    async fn pair_through_relay(address: std::net::SocketAddr, session: &str) -> TcpStream {
        RelayClient::connect(address, session)
            .await
            .expect("the relay pairs both ends")
            .into_inner()
    }

    /// A resolver keyed the way the rendezvous names peers: by public key hex.
    fn key_resolver(identities: &[&Identity]) -> HashMap<String, PublicKey> {
        identities
            .iter()
            .map(|identity| (identity.public_hex(), identity.public_bytes()))
            .collect()
    }

    /// Run both sides of `for_peer` concurrently on one relay session.
    async fn paired_for_peer(
        address: std::net::SocketAddr,
        session: &str,
        a: &Identity,
        b: &Identity,
        resolver: &HashMap<String, PublicKey>,
    ) -> (
        Result<RelayTlsSession, RelayTlsError>,
        Result<RelayTlsSession, RelayTlsError>,
    ) {
        let a_key = a.public_bytes();
        let b_key = b.public_bytes();
        tokio::join!(
            async {
                let stream = pair_through_relay(address, session).await;
                RelayTlsSession::for_peer(a, &b_key, stream, resolver).await
            },
            async {
                let stream = pair_through_relay(address, session).await;
                RelayTlsSession::for_peer(b, &a_key, stream, resolver).await
            },
        )
    }

    /// Drive the client side of the challenge exchange by hand, so a test can
    /// answer with bytes the protocol would never produce. Returns the TLS
    /// stream and the challenge the server issued on this connection.
    async fn raw_tls_client<F>(
        stream: TcpStream,
        pin: PublicKey,
        answer: F,
    ) -> Result<(Box<dyn AsyncStream>, Challenge), RelayTlsError>
    where
        F: FnOnce(&Challenge) -> Vec<u8>,
    {
        let config = endpoint::rustls_client_config(Some(pin))
            .map_err(|error| RelayTlsError::Config(error.to_string()))?;
        let connector = TlsConnector::from(Arc::new(config));
        let server_name = ServerName::try_from(endpoint::SERVER_NAME)
            .map_err(|error| RelayTlsError::Config(error.to_string()))?;
        let mut stream = connector
            .connect(server_name, relay_duplex(stream))
            .await
            .map_err(|error| RelayTlsError::Handshake(error.to_string()))?;

        let first = frame::read(&mut stream).await?;
        let challenge = Challenge::from_frame(&first)?;
        stream
            .write_all(&answer(&challenge))
            .await
            .map_err(|error| RelayTlsError::Frame(FrameError::Io(error.kind())))?;
        stream
            .flush()
            .await
            .map_err(|error| RelayTlsError::Frame(FrameError::Io(error.kind())))?;
        Ok((Box::new(stream), challenge))
    }

    type Record = Arc<Mutex<Vec<u8>>>;

    /// Copy bytes and record the first 4096 that pass in this direction.
    async fn copy_recorded<R, W>(reader: &mut R, writer: &mut W, record: Record) -> io::Result<()>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut buffer = [0u8; 1024];
        loop {
            let read = reader.read(&mut buffer).await?;
            if read == 0 {
                return Ok(());
            }
            writer.write_all(&buffer[..read]).await?;
            let mut record = record.lock().expect("the record lock");
            let room = 4096usize.saturating_sub(record.len());
            record.extend_from_slice(&buffer[..read.min(room)]);
        }
    }

    /// A TCP proxy in front of the real relay that records what it forwards.
    ///
    /// It forwards **bytes**, not frames, so the relay's own parser is not
    /// involved: whatever is recorded is exactly what crossed the splice.
    async fn recording_relay(
        upstream: std::net::SocketAddr,
    ) -> (std::net::SocketAddr, Record, Record) {
        let listener = TcpListener::bind(loopback())
            .await
            .expect("a recording relay");
        let address = listener.local_addr().expect("its port");
        let to_relay: Record = Arc::new(Mutex::new(Vec::new()));
        let from_relay: Record = Arc::new(Mutex::new(Vec::new()));
        let upload_record = to_relay.clone();
        let download_record = from_relay.clone();
        tokio::spawn(async move {
            loop {
                let Ok((client, _)) = listener.accept().await else {
                    return;
                };
                let Ok(upstream) = TcpStream::connect(upstream).await else {
                    continue;
                };
                let upload_record = upload_record.clone();
                let download_record = download_record.clone();
                tokio::spawn(async move {
                    let (mut client_read, mut client_write) = client.into_split();
                    let (mut upstream_read, mut upstream_write) = upstream.into_split();
                    let upload =
                        copy_recorded(&mut client_read, &mut upstream_write, upload_record);
                    let download =
                        copy_recorded(&mut upstream_read, &mut client_write, download_record);
                    tokio::select! {
                        _ = upload => {}
                        _ = download => {}
                    }
                });
            }
        });
        (address, to_relay, from_relay)
    }

    #[tokio::test]
    async fn two_peers_pair_through_the_relay_handshake_and_echo_a_binary_frame() {
        let address = start_relay().await;
        let a = Identity::generate();
        let b = Identity::generate();
        let a_key = a.public_bytes();
        let b_key = b.public_bytes();
        let resolver = key_resolver(&[&a, &b]);

        // Both peers call for_peer with the same facts. The relay pairs the
        // connections; the TLS role comes from the lower key, not arrival.
        let (a_result, b_result) = timeout(
            PATIENCE,
            paired_for_peer(address, "echo", &a, &b, &resolver),
        )
        .await
        .expect("the handshake timed out");
        let a_session = a_result.expect("A connects");
        let b_session = b_result.expect("B connects");

        let a_is_server = a_key < b_key;
        assert_eq!(
            a_session.role(),
            if a_is_server {
                RelayTlsRole::Server
            } else {
                RelayTlsRole::Client
            }
        );
        assert_ne!(
            a_session.role(),
            b_session.role(),
            "the two peers must hold complementary roles"
        );

        let (mut client, mut server) = if a_is_server {
            (b_session, a_session)
        } else {
            (a_session, b_session)
        };
        let (client_key, server_key) = if a_is_server {
            (b_key, a_key)
        } else {
            (a_key, b_key)
        };
        assert_eq!(client.peer(), server_key);
        assert_eq!(server.peer(), client_key);
        // The hello names the client by its public key, as the rendezvous does.
        assert_eq!(server.hello().peer_id_hex, hex_encode(&client_key));

        let bytes: Vec<u8> = (0u8..=255).collect();
        let frame = Frame {
            kind: Kind::Stream,
            id: 11,
            payload: json!(bytes),
        };
        client
            .send_frame(&frame)
            .await
            .expect("the binary frame through TLS");
        let received = timeout(PATIENCE, server.recv_frame())
            .await
            .expect("the frame timed out")
            .expect("the binary frame arrives");
        assert_eq!(
            received, frame,
            "the payload crosses the relay byte for byte"
        );

        server
            .send_frame(&received)
            .await
            .expect("the echo through TLS");
        let echoed = timeout(PATIENCE, client.recv_frame())
            .await
            .expect("the echo timed out")
            .expect("the echo arrives");
        assert_eq!(echoed, frame);
    }

    #[tokio::test]
    async fn for_peer_assigns_complementary_roles_from_the_keys() {
        let address = start_relay().await;
        let a = Identity::generate();
        let b = Identity::generate();
        let a_key = a.public_bytes();
        let b_key = b.public_bytes();
        let resolver = key_resolver(&[&a, &b]);

        let (a_result, b_result) = timeout(
            PATIENCE,
            paired_for_peer(address, "roles", &a, &b, &resolver),
        )
        .await
        .expect("the handshake timed out");
        let a_session = a_result.expect("A connects");
        let b_session = b_result.expect("B connects");

        // A's `for_peer` sees B as the peer and B's sees A: the argument
        // order is swapped, so the roles must swap too. Two servers or two
        // clients would mean the decision leaked in from outside the keys.
        if a_key < b_key {
            assert_eq!(a_session.role(), RelayTlsRole::Server);
            assert_eq!(b_session.role(), RelayTlsRole::Client);
        } else {
            assert_eq!(a_session.role(), RelayTlsRole::Client);
            assert_eq!(b_session.role(), RelayTlsRole::Server);
        }
        assert_ne!(a_session.role(), b_session.role());
    }

    #[tokio::test]
    async fn forcing_the_same_role_fails_without_delivering_a_frame() {
        let address = start_relay().await;
        let a = Identity::generate();
        let b = Identity::generate();
        let (a_key, b_key) = (a.public_bytes(), b.public_bytes());

        // The private constructors are the test hatch: `for_peer` can never
        // choose Client for both ends, but forcing that choice must still fail
        // before either side yields a session (and therefore a frame).
        let (a_result, b_result) = timeout(Duration::from_secs(2), async {
            tokio::join!(
                async {
                    let stream = pair_through_relay(address, "wrong-role").await;
                    RelayTlsSession::client(stream, a.clone(), a.public_hex(), Some(b_key)).await
                },
                async {
                    let stream = pair_through_relay(address, "wrong-role").await;
                    RelayTlsSession::client(stream, b.clone(), b.public_hex(), Some(a_key)).await
                },
            )
        })
        .await
        .expect("two TLS clients must fail, not hang");

        let a_error = a_result.err().expect("A must not get a session");
        let b_error = b_result.err().expect("B must not get a session");
        assert!(
            matches!(a_error, RelayTlsError::Handshake(_)),
            "the wrong role must fail A's handshake: {a_error:?}"
        );
        assert!(
            matches!(b_error, RelayTlsError::Handshake(_)),
            "the wrong role must fail B's handshake: {b_error:?}"
        );
    }

    #[tokio::test]
    async fn a_peer_cannot_pair_with_itself() {
        let identity = Identity::generate();
        let key = identity.public_bytes();
        let resolver = key_resolver(&[&identity]);
        // A connected socket whose peer never speaks: the equal-key refusal
        // must happen before the TLS stack is asked to start.
        let listener = TcpListener::bind(loopback()).await.expect("a listener");
        let stream = TcpStream::connect(listener.local_addr().expect("its port"))
            .await
            .expect("a connection");

        let result = RelayTlsSession::for_peer(&identity, &key, stream, &resolver).await;

        assert_eq!(result.err(), Some(RelayTlsError::SamePeer));
    }

    #[tokio::test]
    async fn a_missing_pin_is_a_typed_error_before_any_handshake() {
        // A connected socket whose peer never speaks: the missing pin must be
        // refused before the TLS stack is even asked to start.
        let listener = TcpListener::bind(loopback()).await.expect("a listener");
        let address = listener.local_addr().expect("its port");
        let stream = TcpStream::connect(address).await.expect("a connection");

        let result = RelayTlsSession::client(stream, Identity::generate(), "client-1", None).await;

        assert_eq!(result.err(), Some(RelayTlsError::MissingPin));
    }

    #[tokio::test]
    async fn a_wrong_pin_fails_the_handshake_and_no_session_is_delivered() {
        let address = start_relay().await;
        let server_identity = Identity::generate();
        let wrong = Identity::generate().public_bytes();
        let client_key = Identity::generate().public_bytes();
        let resolver = HashMap::from([("client-1".to_string(), client_key)]);

        let client_task = tokio::spawn(async move {
            let stream = pair_through_relay(address, "wrong-pin").await;
            RelayTlsSession::client(stream, Identity::generate(), "client-1", Some(wrong)).await
        });
        sleep(Duration::from_millis(50)).await;
        let stream = pair_through_relay(address, "wrong-pin").await;
        let server_result = timeout(
            PATIENCE,
            RelayTlsSession::server(stream, server_identity, &resolver),
        )
        .await
        .expect("the server side must not hang");

        assert!(
            server_result.is_err(),
            "a wrong pin must not produce a server session: {:?}",
            server_result.err()
        );
        let client_result = timeout(PATIENCE, client_task)
            .await
            .expect("the client task timed out")
            .expect("the client task did not panic");
        assert!(
            matches!(client_result, Err(RelayTlsError::Handshake(_))),
            "the pin mismatch must fail the handshake, not fall back: {:?}",
            client_result.err()
        );
    }

    #[tokio::test]
    async fn a_hello_signed_by_another_key_is_refused_before_any_frame() {
        let address = start_relay().await;
        let server_identity = Identity::generate();
        let server_key = server_identity.public_bytes();
        let allowed = Identity::generate();
        let attacker = Identity::generate();
        let resolver = HashMap::from([("client-1".to_string(), allowed.public_bytes())]);

        let (client_result, server_result) = timeout(PATIENCE, async {
            tokio::join!(
                async {
                    let stream = pair_through_relay(address, "bad-hello").await;
                    raw_tls_client(stream, server_key, |challenge| {
                        let hello = Hello::sign(&attacker, "client-1", challenge);
                        frame::encode(&hello.to_frame().expect("a hello frame"))
                            .expect("the hello's wire bytes")
                    })
                    .await
                },
                async {
                    let stream = pair_through_relay(address, "bad-hello").await;
                    RelayTlsSession::server(stream, server_identity, &resolver).await
                },
            )
        })
        .await
        .expect("the handshake timed out");

        assert_eq!(
            server_result.err(),
            Some(RelayTlsError::Hello(HelloError::BadSignature)),
            "the hello must be refused for the key the resolver pinned"
        );

        let (mut client, _challenge) =
            client_result.expect("the TLS handshake itself succeeds; only the hello is refused");
        let next = timeout(Duration::from_secs(2), frame::read(&mut client))
            .await
            .expect("a refused client must not hang");
        assert!(
            next.is_err(),
            "no frame may be delivered after a refused hello: {next:?}"
        );
    }

    #[tokio::test]
    async fn a_hello_with_a_wrong_challenge_is_refused() {
        let address = start_relay().await;
        let server_identity = Identity::generate();
        let server_key = server_identity.public_bytes();
        let client_identity = Identity::generate();
        let resolver = HashMap::from([("client-1".to_string(), client_identity.public_bytes())]);
        let wrong = Challenge::generate();

        let (client_result, server_result) = timeout(PATIENCE, async {
            tokio::join!(
                async {
                    let stream = pair_through_relay(address, "wrong-challenge").await;
                    raw_tls_client(stream, server_key, |_challenge| {
                        let hello = Hello::sign(&client_identity, "client-1", &wrong);
                        frame::encode(&hello.to_frame().expect("a hello frame"))
                            .expect("the hello's wire bytes")
                    })
                    .await
                },
                async {
                    let stream = pair_through_relay(address, "wrong-challenge").await;
                    RelayTlsSession::server(stream, server_identity, &resolver).await
                },
            )
        })
        .await
        .expect("the handshake timed out");

        let _client = client_result.expect("the TLS handshake succeeds");
        assert_eq!(
            server_result
                .err()
                .expect("a hello for another challenge must be refused"),
            RelayTlsError::Hello(HelloError::ChallengeMismatch)
        );
    }

    #[tokio::test]
    async fn a_hello_with_a_missing_challenge_field_is_refused() {
        let address = start_relay().await;
        let server_identity = Identity::generate();
        let server_key = server_identity.public_bytes();
        let client_identity = Identity::generate();
        let resolver = HashMap::from([("client-1".to_string(), client_identity.public_bytes())]);

        let (client_result, server_result) = timeout(PATIENCE, async {
            tokio::join!(
                async {
                    let stream = pair_through_relay(address, "missing-challenge").await;
                    raw_tls_client(stream, server_key, |challenge| {
                        let hello = Hello::sign(&client_identity, "client-1", challenge);
                        let mut value =
                            serde_json::to_value(&hello).expect("a hello is serializable");
                        value
                            .as_object_mut()
                            .expect("a hello is an object")
                            .remove("challenge");
                        let frame = Frame {
                            kind: Kind::Hello,
                            id: 0,
                            payload: value,
                        };
                        frame::encode(&frame).expect("the malformed hello's wire bytes")
                    })
                    .await
                },
                async {
                    let stream = pair_through_relay(address, "missing-challenge").await;
                    RelayTlsSession::server(stream, server_identity, &resolver).await
                },
            )
        })
        .await
        .expect("the handshake timed out");

        let _client = client_result.expect("the TLS handshake succeeds");
        assert!(
            matches!(
                server_result
                    .err()
                    .expect("a hello with no challenge field must be refused"),
                RelayTlsError::Hello(HelloError::Malformed(_))
            ),
            "the missing field is a malformed hello"
        );
    }

    #[tokio::test]
    async fn a_replayed_hello_is_refused_on_a_new_relay_session() {
        let address = start_relay().await;
        let server_identity = Identity::generate();
        let server_key = server_identity.public_bytes();
        let client_identity = Identity::generate();
        let resolver = HashMap::from([("client-1".to_string(), client_identity.public_bytes())]);

        // Session A: a real handshake, with the exact hello bytes recorded.
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_a = captured.clone();
        let (client_a, server_a) = timeout(PATIENCE, async {
            tokio::join!(
                async {
                    let stream = pair_through_relay(address, "replay-a").await;
                    raw_tls_client(stream, server_key, |challenge| {
                        let hello = Hello::sign(&client_identity, "client-1", challenge);
                        let encoded = frame::encode(&hello.to_frame().expect("a hello frame"))
                            .expect("the hello's wire bytes");
                        *captured_a.lock().expect("the capture lock") = encoded.clone();
                        encoded
                    })
                    .await
                },
                async {
                    let stream = pair_through_relay(address, "replay-a").await;
                    RelayTlsSession::server(stream, server_identity.clone(), &resolver).await
                },
            )
        })
        .await
        .expect("A's handshake timed out");
        let (_client_a, challenge_a) = client_a.expect("A's TLS handshake");
        let server_a = server_a.expect("A's valid hello is accepted");
        assert_eq!(server_a.hello().challenge, challenge_a.bytes);

        let replayed = captured.lock().expect("the capture lock").clone();
        assert!(!replayed.is_empty(), "A's hello must have been recorded");

        // Session B: a fresh challenge, then A's bytes verbatim.
        let (client_b, server_b) = timeout(PATIENCE, async {
            tokio::join!(
                async {
                    let stream = pair_through_relay(address, "replay-b").await;
                    raw_tls_client(stream, server_key, |_challenge| replayed.clone()).await
                },
                async {
                    let stream = pair_through_relay(address, "replay-b").await;
                    RelayTlsSession::server(stream, server_identity.clone(), &resolver).await
                },
            )
        })
        .await
        .expect("B's handshake timed out");
        let (mut client_b, challenge_b) = client_b.expect("B's TLS handshake succeeds");
        assert_ne!(
            challenge_a.bytes, challenge_b.bytes,
            "every connection must get its own challenge"
        );
        assert_eq!(
            server_b
                .err()
                .expect("a recorded hello must not authenticate a new connection"),
            RelayTlsError::Hello(HelloError::ChallengeMismatch),
            "the replayed hello answers the old challenge, not this one"
        );

        let next = timeout(Duration::from_secs(2), frame::read(&mut client_b))
            .await
            .expect("a refused client must not hang");
        assert!(
            next.is_err(),
            "no frame may be delivered after a replayed hello: {next:?}"
        );
    }

    #[tokio::test]
    async fn two_connections_from_one_peer_get_independent_challenges() {
        let address = start_relay().await;
        let server_identity = Identity::generate();
        let server_key = server_identity.public_bytes();
        let client_identity = Identity::generate();
        let resolver = HashMap::from([("client-1".to_string(), client_identity.public_bytes())]);

        let (pair_one, pair_two) = timeout(PATIENCE, async {
            tokio::join!(
                async {
                    tokio::join!(
                        async {
                            let stream = pair_through_relay(address, "concurrent-a").await;
                            RelayTlsSession::client(
                                stream,
                                client_identity.clone(),
                                "client-1",
                                Some(server_key),
                            )
                            .await
                        },
                        async {
                            let stream = pair_through_relay(address, "concurrent-a").await;
                            RelayTlsSession::server(stream, server_identity.clone(), &resolver)
                                .await
                        },
                    )
                },
                async {
                    tokio::join!(
                        async {
                            let stream = pair_through_relay(address, "concurrent-b").await;
                            RelayTlsSession::client(
                                stream,
                                client_identity.clone(),
                                "client-1",
                                Some(server_key),
                            )
                            .await
                        },
                        async {
                            let stream = pair_through_relay(address, "concurrent-b").await;
                            RelayTlsSession::server(stream, server_identity.clone(), &resolver)
                                .await
                        },
                    )
                },
            )
        })
        .await
        .expect("the concurrent handshakes timed out");

        let (client_one, server_one) = pair_one;
        let (client_two, server_two) = pair_two;
        let mut client_one = client_one.expect("the first client session");
        let mut client_two = client_two.expect("the second client session");
        let mut server_one = server_one.expect("the first server session is authenticated");
        let mut server_two = server_two.expect("the second server session is authenticated");

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

        // Both TLS sessions are live: one frame in each direction per pair.
        for (client, server, id) in [
            (&mut client_one, &mut server_one, 1u64),
            (&mut client_two, &mut server_two, 2u64),
        ] {
            let ping = Frame {
                kind: Kind::Ping,
                id,
                payload: json!({}),
            };
            client
                .send_frame(&ping)
                .await
                .expect("the ping crosses the relay");
            let received = server
                .recv_frame()
                .await
                .expect("the ping arrives over TLS");
            assert_eq!(received, ping);
            server
                .send_frame(&received)
                .await
                .expect("the echo crosses the relay");
            let echoed = client
                .recv_frame()
                .await
                .expect("the echo arrives over TLS");
            assert_eq!(echoed, ping);
        }
    }

    #[tokio::test]
    async fn the_relay_forwards_ciphertext_not_the_payload() {
        let relay_address = start_relay().await;
        let (address, to_relay, from_relay) = recording_relay(relay_address).await;
        let server_identity = Identity::generate();
        let server_key = server_identity.public_bytes();
        let client_identity = Identity::generate();
        let resolver = HashMap::from([("client-1".to_string(), client_identity.public_bytes())]);

        // Thirty-two printable random bytes. JSON carries them verbatim, so a
        // relay that saw the frame body would record them; TLS must not.
        let marker: String = Identity::generate().public_hex().chars().take(32).collect();
        assert_eq!(marker.len(), 32);
        let plaintext_marker = marker.clone().into_bytes();
        let frame = Frame {
            kind: Kind::Res,
            id: 3,
            payload: json!({ "marker": marker }),
        };
        let plaintext = frame::encode(&frame).expect("the frame's plaintext bytes");
        assert!(
            plaintext
                .windows(plaintext_marker.len())
                .any(|window| window == plaintext_marker),
            "the marker must appear in the plaintext frame, or the absence test proves nothing"
        );

        let sent = frame.clone();
        let client_task = tokio::spawn(async move {
            let stream = pair_through_relay(address, "ciphertext").await;
            let mut client =
                RelayTlsSession::client(stream, client_identity, "client-1", Some(server_key))
                    .await
                    .expect("the client handshake");
            client
                .send_frame(&sent)
                .await
                .expect("the frame through TLS");
            let echo = client.recv_frame().await.expect("the echo");
            (client, echo)
        });
        sleep(Duration::from_millis(50)).await;
        let stream = pair_through_relay(address, "ciphertext").await;
        let mut server = timeout(
            PATIENCE,
            RelayTlsSession::server(stream, server_identity, &resolver),
        )
        .await
        .expect("the server handshake timed out")
        .expect("the server accepts the client");
        let received = timeout(PATIENCE, server.recv_frame())
            .await
            .expect("the frame timed out")
            .expect("the frame arrives");
        assert_eq!(received, frame);
        server
            .send_frame(&received)
            .await
            .expect("the echo through TLS");
        let (_client, echo) = timeout(PATIENCE, client_task)
            .await
            .expect("the client task timed out")
            .expect("the client task did not panic");
        assert_eq!(echo, frame);

        let forwarded_up = to_relay.lock().expect("the upload record").clone();
        let forwarded_down = from_relay.lock().expect("the download record").clone();
        assert!(
            !forwarded_up.is_empty() && !forwarded_down.is_empty(),
            "the recording relay must have forwarded bytes in both directions"
        );
        assert!(
            !forwarded_up
                .windows(plaintext_marker.len())
                .any(|window| window == plaintext_marker),
            "the relay read the payload on the way to the server"
        );
        assert!(
            !forwarded_down
                .windows(plaintext_marker.len())
                .any(|window| window == plaintext_marker),
            "the relay read the payload on the way back"
        );
    }
}
