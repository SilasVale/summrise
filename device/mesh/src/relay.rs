//! The relay floor: one TCP splice, two peers, opaque frames.
//!
//! **Why the floor exists.** A NAT that maps per destination, or a firewall
//! that drops unsolicited UDP, can make a direct path impossible. A public
//! relay both peers can dial is then the only path that works. The design
//! calls it the floor, not the default: a direct path is preferred whenever
//! the punch opens one.
//!
//! **Pairing is the whole public surface.** A [`RelayClient`] pairs two
//! streams and then gets out of the way: [`RelayClient::into_inner`] hands
//! back the raw `TcpStream`, which is the only public way to reach the splice
//! after [`RelayClient::connect`]. There is deliberately no public
//! frame-level `send`/`recv`: a frame handed to those would be plaintext the
//! relay can read. Frames move over a paired splice only inside
//! [`crate::relay_tls`], which runs pinned TLS through the stream and is the
//! session's only relay path. The crate-private frame methods exist for this
//! module's own tests.
//!
//! **The relay is a splicer, not a party.** Two clients call
//! [`RelayClient::connect`] with the same session id; the first waits, the
//! second pairs with it, and the relay copies **length-prefixed frame
//! bodies** in both directions. It reads the four-byte length, checks it
//! against [`crate::frame::MAX_FRAME_BYTES`] before allocating, and writes
//! the body unchanged. **It never calls `serde_json` on a data frame and
//! never looks at `payload`** — the only JSON it parses is the pairing
//! control message below, which names a session and carries no user data.
//! The payload is opaque bytes between the two peers, which is what lets
//! [`crate::relay_tls`] put TLS through the splice without teaching the relay
//! about it.
//!
//! **The protocol.**
//!
//! ```text
//! client ──TCP──> relay
//!   [u32 length][{"session":"..."}]      the pairing request
//!   [u32 length][{"ok":true}]            when a peer is there
//!   [u32 length][frame body] ...         opaque in both directions
//! ```
//!
//! The first client's request waits for a second. A session already in use
//! is refused immediately; a first client that waits past the pair timeout
//! is answered with `{"ok":false,"reason":"pair timeout"}`. When either side
//! of a live splice closes, the relay closes the other side, and that side's
//! [`frame::read`](crate::frame::read) reports [`FrameError::Closed`].
//!
//! **The pair timeout is an input.** It is the caller's value, not a
//! constant in this module, because the fallback decision belongs to
//! `summrise-core`'s `Path`.

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio::sync::{oneshot, Mutex};

#[cfg(test)]
use crate::frame::{self, Frame, FrameError};
use crate::frame::{LENGTH_PREFIX_BYTES, MAX_FRAME_BYTES};

/// The longest session id the relay accepts, in bytes.
pub const MAX_SESSION_ID_BYTES: usize = 128;

/// The largest pairing control message, in bytes. It is a session id and a
/// boolean, so this is generous and still bounded.
const MAX_CONTROL_BYTES: usize = 4096;

/// The reply reason for a session that is already paired.
const REASON_BUSY: &str = "session busy";

/// The reply reason for a first client whose wait expired.
const REASON_TIMEOUT: &str = "pair timeout";

/// The reply reason for a session id the relay cannot use.
const REASON_BAD_SESSION: &str = "bad session";

/// Why a relay operation failed.
#[derive(Debug)]
pub enum RelayError {
    /// The TCP transport failed.
    Io(io::ErrorKind),
    /// The pairing control message was not the expected JSON or did not
    /// arrive at all.
    MalformedHandshake(String),
    /// The session id is empty or longer than [`MAX_SESSION_ID_BYTES`].
    BadSessionId(String),
    /// Another pair already owns this session id.
    SessionBusy,
    /// The first client waited past the pair timeout.
    PairTimeout,
    /// The relay refused for a reason this client does not interpret.
    Refused(String),
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(kind) => write!(f, "the relay transport failed: {kind:?}"),
            Self::MalformedHandshake(message) => {
                write!(f, "not a relay pairing message: {message}")
            }
            Self::BadSessionId(message) => write!(f, "not a relay session id: {message}"),
            Self::SessionBusy => f.write_str("the relay session is already in use"),
            Self::PairTimeout => f.write_str("the relay pair timed out"),
            Self::Refused(reason) => write!(f, "the relay refused: {reason}"),
        }
    }
}

impl std::error::Error for RelayError {}

impl From<io::Error> for RelayError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

/// The pairing request a client sends first.
#[derive(Debug, Serialize, Deserialize)]
struct PairRequest {
    session: String,
}

/// The relay's answer to a pairing request.
#[derive(Debug, Serialize, Deserialize)]
struct PairReply {
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

/// One relay process: a listener and the sessions it is pairing.
pub struct RelayServer {
    listener: TcpListener,
    pair_timeout: Duration,
    local: SocketAddr,
    state: Arc<Mutex<RelayState>>,
}

/// The waiting and live sessions, guarded as one table so a pairing is
/// claimed atomically.
#[derive(Default)]
struct RelayState {
    waiting: HashMap<String, Waiting>,
    active: HashSet<String>,
}

/// A first client's slot, and the channel a second client uses to hand its
/// socket to that first task.
struct Waiting {
    reply: oneshot::Sender<TcpStream>,
    token: u64,
    /// When the slot was taken. The sweep uses it to drop slots whose waiter
    /// task is gone without having removed itself.
    created_at: Instant,
}

impl RelayState {
    /// Drop waiting slots older than the pair timeout.
    ///
    /// A slot is normally released by its pairing, its timeout, or the
    /// waiter's disconnect; this is the belt for the case where none of those
    /// ran (for example a task dropped at the wrong instant). Dropping the
    /// `Waiting` drops its sender, so a task still holding the receiver wakes
    /// with an error instead of waiting forever.
    fn sweep_waiting(&mut self, pair_timeout: Duration) {
        self.waiting
            .retain(|_, waiting| waiting.created_at.elapsed() <= pair_timeout);
    }
}

/// A token that lets a timed-out first client tell "my slot is still mine"
/// from "a second client claimed it while I was timing out".
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

/// What this client's pairing request turned into.
enum Claim {
    /// No one was waiting; this client waits for a second.
    Wait {
        reply: oneshot::Receiver<TcpStream>,
        token: u64,
    },
    /// A first client was waiting; this client's socket travels to it.
    Pair { waiting: Waiting },
    /// Another pair owns the session.
    Busy,
}

impl RelayServer {
    /// Bind a relay and set its pair timeout.
    ///
    /// The timeout bounds both the wait for a second client and the wait for
    /// the first client's pairing message: an accepted socket that says
    /// nothing cannot hold a task forever.
    pub async fn bind(
        address: impl ToSocketAddrs,
        pair_timeout: Duration,
    ) -> Result<Self, RelayError> {
        let listener = TcpListener::bind(address).await?;
        let local = listener.local_addr()?;
        Ok(Self {
            listener,
            pair_timeout,
            local,
            state: Arc::new(Mutex::new(RelayState::default())),
        })
    }

    /// The address the relay is bound to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// Accept and serve pairings until the process ends.
    ///
    /// Each client gets its own task; one slow client cannot stop another
    /// from pairing. Accept errors are transient (a full descriptor table
    /// clears), so the loop continues rather than taking the relay down.
    pub async fn run(self) {
        loop {
            match self.listener.accept().await {
                Ok((stream, _from)) => {
                    let state = self.state.clone();
                    let pair_timeout = self.pair_timeout;
                    tokio::spawn(async move {
                        let _ = handle_client(stream, state, pair_timeout).await;
                    });
                }
                Err(_) => continue,
            }
        }
    }
}

/// One end of a paired relay splice, before any TLS or framing.
///
/// This type is pairing-only. Use [`RelayClient::connect`], take the stream
/// with [`RelayClient::into_inner`], and run
/// [`RelayTlsSession::for_peer`](crate::relay_tls::RelayTlsSession::for_peer)
/// on it; that is the supported way to move frames through the relay, and the
/// only one that keeps the payload unreadable to the relay.
pub struct RelayClient {
    stream: TcpStream,
}

impl RelayClient {
    /// Ask the relay to pair this connection under `session_id`.
    ///
    /// The call returns once the relay says `ok`; the first caller waits
    /// until a second arrives (or the relay's pair timeout expires).
    pub async fn connect(relay: SocketAddr, session_id: &str) -> Result<Self, RelayError> {
        validate_session_id(session_id)?;
        let mut stream = TcpStream::connect(relay).await?;
        // The relay is a latency floor, not a bulk pipe; do not let Nagle
        // hold a small frame behind the next one.
        let _ = stream.set_nodelay(true);
        write_control(
            &mut stream,
            &PairRequest {
                session: session_id.to_owned(),
            },
        )
        .await?;
        let reply: PairReply = read_control(&mut stream).await?;
        if reply.ok {
            return Ok(Self { stream });
        }
        match reply.reason.as_deref() {
            Some(REASON_BUSY) => Err(RelayError::SessionBusy),
            Some(REASON_TIMEOUT) => Err(RelayError::PairTimeout),
            Some(REASON_BAD_SESSION) => Err(RelayError::BadSessionId(
                "the relay rejected the session id".to_owned(),
            )),
            Some(other) => Err(RelayError::Refused(other.to_owned())),
            None => Err(RelayError::Refused(
                "the relay refused without a reason".to_owned(),
            )),
        }
    }

    /// Send one frame through the splice, in the clear.
    ///
    /// Crate-private on purpose: this is the plaintext path, and the relay
    /// can read it. Production code must use
    /// [`RelayTlsSession::for_peer`](crate::relay_tls::RelayTlsSession::for_peer)
    /// over [`RelayClient::into_inner`]. Tests in this module use this to
    /// prove the splice forwards opaque bodies.
    #[cfg(test)]
    pub(crate) async fn send(&mut self, frame: &Frame) -> Result<(), FrameError> {
        frame::write(&mut self.stream, frame).await
    }

    /// Read one frame from the splice, in the clear.
    ///
    /// [`FrameError::Closed`] means the peer (or the relay) closed the
    /// splice at a frame boundary. Crate-private for the same reason as
    /// [`RelayClient::send`].
    #[cfg(test)]
    pub(crate) async fn recv(&mut self) -> Result<Frame, FrameError> {
        frame::read(&mut self.stream).await
    }

    /// Close this end of the splice; the peer sees `Closed`.
    #[cfg(test)]
    pub(crate) async fn close(&mut self) -> io::Result<()> {
        self.stream.shutdown().await
    }

    /// Take the underlying stream, after the pairing handshake.
    ///
    /// The stream is raw bytes at a frame boundary. Run
    /// [`RelayTlsSession::for_peer`](crate::relay_tls::RelayTlsSession::for_peer)
    /// on it to move frames; writing frames directly would hand them to the
    /// relay in the clear.
    pub fn into_inner(self) -> TcpStream {
        self.stream
    }
}

/// Read one client's request, pair it, and (for the first client) splice.
async fn handle_client(
    mut stream: TcpStream,
    state: Arc<Mutex<RelayState>>,
    pair_timeout: Duration,
) -> Result<(), RelayError> {
    let _ = stream.set_nodelay(true);
    let request: PairRequest =
        match tokio::time::timeout(pair_timeout, read_control(&mut stream)).await {
            Ok(result) => result?,
            Err(_) => {
                write_reply(&mut stream, false, Some(REASON_TIMEOUT)).await?;
                return Ok(());
            }
        };
    if validate_session_id(&request.session).is_err() {
        write_reply(&mut stream, false, Some(REASON_BAD_SESSION)).await?;
        return Ok(());
    }
    let session = request.session;

    let (sender, receiver) = oneshot::channel();
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    let claim = {
        let mut table = state.lock().await;
        // A waiting slot whose task is gone must not pair a later dialer with
        // a socket nobody owns. The waiter's own task also removes it on
        // disconnect; this sweep covers a slot that outlived its owner.
        table.sweep_waiting(pair_timeout);
        if table.active.contains(&session) {
            Claim::Busy
        } else if let Some(waiting) = table.waiting.remove(&session) {
            table.active.insert(session.clone());
            Claim::Pair { waiting }
        } else {
            table.waiting.insert(
                session.clone(),
                Waiting {
                    reply: sender,
                    token,
                    created_at: Instant::now(),
                },
            );
            Claim::Wait {
                reply: receiver,
                token,
            }
        }
    };

    match claim {
        Claim::Busy => {
            write_reply(&mut stream, false, Some(REASON_BUSY)).await?;
            Ok(())
        }
        Claim::Pair { waiting } => {
            if waiting.reply.is_closed() {
                // The first client timed out and dropped its receiver between
                // our claim and our send. Undo the claim so the session is not
                // left permanently busy.
                state.lock().await.active.remove(&session);
                write_reply(&mut stream, false, Some(REASON_TIMEOUT)).await?;
                return Ok(());
            }
            match waiting.reply.send(stream) {
                // The first task owns both ends now and answers both clients.
                Ok(()) => Ok(()),
                Err(mut stream) => {
                    state.lock().await.active.remove(&session);
                    write_reply(&mut stream, false, Some(REASON_BUSY)).await?;
                    Ok(())
                }
            }
        }
        Claim::Wait { mut reply, token } => {
            // The wait ends three ways: a partner claims the slot, the pair
            // timeout fires, or the waiting client goes away. The third is
            // the one the socket cannot tell us by itself, so watch both the
            // pairing channel and the client's read side. `peek` never
            // consumes a byte; a waiter that sends data before the ack is a
            // client that does not speak the protocol, and its slot is
            // released too.
            enum Waited {
                Paired(Result<TcpStream, oneshot::error::RecvError>),
                Gone,
            }
            let wait = async {
                let mut probe = [0u8; 1];
                tokio::select! {
                    paired = &mut reply => Waited::Paired(paired),
                    _closed = stream.peek(&mut probe) => Waited::Gone,
                }
            };
            match tokio::time::timeout(pair_timeout, wait).await {
                Ok(Waited::Paired(Ok(partner))) => {
                    pair_and_splice(stream, partner, state, session).await
                }
                Ok(Waited::Paired(Err(_))) => {
                    // The sender disappeared; nobody paired with us.
                    write_reply(&mut stream, false, Some(REASON_TIMEOUT)).await?;
                    Ok(())
                }
                Ok(Waited::Gone) => {
                    // Remove the slot if it is still this task's. If a partner
                    // claimed it at the same instant, take its socket so the
                    // session is not left active with no splice, then release
                    // the session: the waiter's own stream is gone, so there
                    // is nothing to splice.
                    let still_ours = remove_if_ours(&state, &session, token).await;
                    if still_ours {
                        let _ = write_reply(&mut stream, false, Some(REASON_TIMEOUT)).await;
                    } else {
                        let _ = reply.await;
                        state.lock().await.active.remove(&session);
                    }
                    Ok(())
                }
                Err(_) => {
                    // The timer fired, but a second client may have claimed the
                    // slot at the same instant. Remove the slot only if it is
                    // still this task's; if it is gone, the partner's socket is
                    // on its way and pairing wins over the timeout.
                    let still_ours = remove_if_ours(&state, &session, token).await;
                    if still_ours {
                        write_reply(&mut stream, false, Some(REASON_TIMEOUT)).await?;
                        return Ok(());
                    }
                    let partner = reply.await.map_err(|_| {
                        RelayError::Refused(
                            "the pairing handshake was dropped before it arrived".to_owned(),
                        )
                    })?;
                    pair_and_splice(stream, partner, state, session).await
                }
            }
        }
    }
}

/// Remove `session`'s waiting slot only if it still belongs to `token`.
///
/// The token distinguishes "my slot" from "a partner claimed it and my task
/// lost the race", which is the difference between answering a timeout and
/// splicing to a live partner.
async fn remove_if_ours(state: &Arc<Mutex<RelayState>>, session: &str, token: u64) -> bool {
    let mut table = state.lock().await;
    let ours = table
        .waiting
        .get(session)
        .is_some_and(|waiting| waiting.token == token);
    if ours {
        table.waiting.remove(session);
    }
    ours
}

/// Acknowledge both clients, splice until one side closes, then release the
/// session id.
async fn pair_and_splice(
    mut first: TcpStream,
    second: TcpStream,
    state: Arc<Mutex<RelayState>>,
    session: String,
) -> Result<(), RelayError> {
    let result = async {
        write_reply(&mut first, true, None).await?;
        let mut second = second;
        write_reply(&mut second, true, None).await?;
        splice(first, second).await
    }
    .await;
    // A session id is reusable once its splice ends; holding it forever would
    // turn a transient pairing into a permanent refusal.
    state.lock().await.active.remove(&session);
    result
}

/// Copy frame bodies in both directions until either direction ends.
///
/// Whichever direction finishes first cancels the other; dropping both
/// write halves closes the peer's read side, so the survivor observes
/// [`FrameError::Closed`] rather than hanging.
async fn splice(first: TcpStream, second: TcpStream) -> Result<(), RelayError> {
    let (mut first_reader, mut first_writer) = first.into_split();
    let (mut second_reader, mut second_writer) = second.into_split();
    let first_to_second = copy_frames(&mut first_reader, &mut second_writer);
    let second_to_first = copy_frames(&mut second_reader, &mut first_writer);
    tokio::select! {
        result = first_to_second => result.map_err(RelayError::from),
        result = second_to_first => result.map_err(RelayError::from),
    }
}

/// Forward length-prefixed bodies without interpreting them.
///
/// This is the function that makes the relay opaque: it owns a four-byte
/// prefix and a byte vector, and it never asks what kind of frame the body
/// is. The cap is checked before the vector is allocated.
async fn copy_frames<R, W>(reader: &mut R, writer: &mut W) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    while let Some(body) = read_frame_body(reader, MAX_FRAME_BYTES).await? {
        write_frame_body(writer, &body).await?;
    }
    Ok(())
}

/// Read one length-prefixed body, or `None` at a clean frame boundary.
///
/// The length is capped **before** the body buffer exists, so a peer cannot
/// announce four gigabytes and make the relay allocate it.
async fn read_frame_body<R>(reader: &mut R, max: usize) -> io::Result<Option<Vec<u8>>>
where
    R: AsyncRead + Unpin,
{
    let mut prefix = [0u8; LENGTH_PREFIX_BYTES];
    let mut filled = 0;
    while filled < prefix.len() {
        match reader.read(&mut prefix[filled..]).await {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "a frame prefix ended in the middle",
                ))
            }
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    let length = u32::from_be_bytes(prefix) as usize;
    if length > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a frame announced {length} bytes; the cap is {max}"),
        ));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).await?;
    Ok(Some(body))
}

/// Write one length-prefixed body unchanged.
async fn write_frame_body<W>(writer: &mut W, body: &[u8]) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&(body.len() as u32).to_be_bytes()).await?;
    writer.write_all(body).await?;
    writer.flush().await
}

/// Read one pairing control message.
async fn read_control<T>(reader: &mut TcpStream) -> Result<T, RelayError>
where
    T: serde::de::DeserializeOwned,
{
    let body = read_frame_body(reader, MAX_CONTROL_BYTES)
        .await?
        .ok_or_else(|| {
            RelayError::MalformedHandshake("the control stream ended before a message".to_owned())
        })?;
    serde_json::from_slice(&body).map_err(|error| RelayError::MalformedHandshake(error.to_string()))
}

/// Write one pairing control message.
async fn write_control<T>(writer: &mut TcpStream, value: &T) -> Result<(), RelayError>
where
    T: Serialize,
{
    let body = serde_json::to_vec(value)
        .map_err(|error| RelayError::MalformedHandshake(error.to_string()))?;
    write_frame_body(writer, &body).await?;
    Ok(())
}

/// Answer a pairing request.
async fn write_reply(
    writer: &mut TcpStream,
    ok: bool,
    reason: Option<&str>,
) -> Result<(), RelayError> {
    write_control(
        writer,
        &PairReply {
            ok,
            reason: reason.map(str::to_owned),
        },
    )
    .await
}

/// Refuse a session id the relay cannot key a table on.
fn validate_session_id(session: &str) -> Result<(), RelayError> {
    if session.is_empty() {
        return Err(RelayError::BadSessionId(
            "a session id must not be empty".to_owned(),
        ));
    }
    if session.len() > MAX_SESSION_ID_BYTES {
        return Err(RelayError::BadSessionId(format!(
            "a session id is at most {MAX_SESSION_ID_BYTES} bytes; this one is {}",
            session.len()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Start a relay on an ephemeral loopback port and return its address.
    async fn started(pair_timeout: Duration) -> SocketAddr {
        let server = RelayServer::bind("127.0.0.1:0", pair_timeout)
            .await
            .expect("a relay");
        let address = server.local_addr();
        tokio::spawn(server.run());
        address
    }

    /// Connect two clients with the same session id, concurrently.
    async fn paired(address: SocketAddr) -> (RelayClient, RelayClient) {
        let (first, second) = tokio::join!(
            RelayClient::connect(address, "echo"),
            RelayClient::connect(address, "echo"),
        );
        (
            first.expect("the first client pairs"),
            second.expect("the second client pairs"),
        )
    }

    #[tokio::test]
    async fn two_clients_pair_and_echo_a_frame() {
        let address = started(Duration::from_secs(5)).await;
        let (mut first, mut second) = paired(address).await;

        let frame = Frame {
            kind: frame::Kind::Req,
            id: 7,
            payload: json!({"echo": "hello"}),
        };
        first.send(&frame).await.expect("a frame to the relay");
        assert_eq!(second.recv().await.expect("the frame arrives"), frame);

        second.send(&frame).await.expect("a frame to the relay");
        assert_eq!(first.recv().await.expect("the echo arrives"), frame);
    }

    #[tokio::test]
    async fn a_waiter_that_disconnects_does_not_poison_the_next_pair() {
        let address = started(Duration::from_secs(5)).await;

        // A client asks for a session and then goes away before anyone pairs.
        let mut ghost = TcpStream::connect(address).await.expect("a connection");
        write_control(
            &mut ghost,
            &PairRequest {
                session: "ghost".to_owned(),
            },
        )
        .await
        .expect("the ghost asks for a pairing");
        drop(ghost);
        // Let the relay observe the EOF and release the slot. Without that,
        // the first real client claims the ghost's slot and is handed to a
        // task whose socket is already gone.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let (first, second) = tokio::join!(
            RelayClient::connect(address, "ghost"),
            RelayClient::connect(address, "ghost"),
        );
        let mut first = first.expect("the first real client pairs");
        let mut second = second.expect("the second real client pairs");

        let frame = Frame {
            kind: frame::Kind::Ping,
            id: 17,
            payload: json!({"after": "the ghost"}),
        };
        first.send(&frame).await.expect("the frame is sent");
        assert_eq!(
            second.recv().await.expect("the frame arrives"),
            frame,
            "two real clients must pair with each other, not with a dead slot"
        );
    }

    #[test]
    fn the_waiting_table_sweeps_slots_older_than_the_pair_timeout() {
        let mut state = RelayState::default();
        let (old_sender, _old_receiver) = oneshot::channel();
        state.waiting.insert(
            "old".to_owned(),
            Waiting {
                reply: old_sender,
                token: 1,
                created_at: Instant::now() - Duration::from_secs(2),
            },
        );
        let (current_sender, _current_receiver) = oneshot::channel();
        state.waiting.insert(
            "current".to_owned(),
            Waiting {
                reply: current_sender,
                token: 2,
                created_at: Instant::now(),
            },
        );

        state.sweep_waiting(Duration::from_secs(1));

        assert!(
            !state.waiting.contains_key("old"),
            "an expired waiting slot must be removed"
        );
        assert!(
            state.waiting.contains_key("current"),
            "a current waiter must survive the sweep"
        );
    }

    #[tokio::test]
    async fn a_third_client_with_the_same_session_is_refused() {
        let address = started(Duration::from_secs(5)).await;
        let (_first, _second) = paired(address).await;

        let third = RelayClient::connect(address, "echo").await;
        assert!(
            matches!(third, Err(RelayError::SessionBusy)),
            "a third client must not take a live session: {:?}",
            third.err()
        );
    }

    #[tokio::test]
    async fn when_one_side_closes_the_other_sees_closed() {
        let address = started(Duration::from_secs(5)).await;
        let (mut first, mut second) = paired(address).await;

        first.close().await.expect("closing the first side");
        let closed = tokio::time::timeout(Duration::from_secs(2), second.recv())
            .await
            .expect("the relay must close the other side");
        assert_eq!(closed, Err(FrameError::Closed));
    }

    #[tokio::test]
    async fn a_lone_client_times_out_at_the_pair_timeout() {
        let address = started(Duration::from_millis(50)).await;
        let result = RelayClient::connect(address, "alone").await;
        assert!(
            matches!(result, Err(RelayError::PairTimeout)),
            "a lone client must time out: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn a_binary_payload_round_trips_byte_for_byte() {
        let address = started(Duration::from_secs(5)).await;
        let (mut first, mut second) = paired(address).await;
        let bytes: Vec<u8> = (0u8..=255).collect();
        let frame = Frame {
            kind: frame::Kind::Stream,
            id: 11,
            payload: json!(bytes),
        };
        first.send(&frame).await.expect("the binary frame");
        let received = second.recv().await.expect("the binary frame arrives");
        assert_eq!(received, frame);
    }

    #[tokio::test]
    async fn the_relay_forwards_bytes_it_cannot_parse() {
        // The strongest form of "no payload parsing": the body is not JSON,
        // not UTF-8, and not a `Frame`. A relay that deserialized payloads
        // could not pass it through byte for byte.
        let address = started(Duration::from_secs(5)).await;
        let mut first = TcpStream::connect(address).await.expect("a connection");
        let mut second = TcpStream::connect(address).await.expect("a connection");

        // Send both requests before reading either reply: whichever arrives
        // first waits, and the other pairs with it.
        write_control(
            &mut first,
            &PairRequest {
                session: "raw".to_owned(),
            },
        )
        .await
        .expect("the first request");
        write_control(
            &mut second,
            &PairRequest {
                session: "raw".to_owned(),
            },
        )
        .await
        .expect("the second request");
        let first_reply: PairReply = read_control(&mut first).await.expect("the first reply");
        let second_reply: PairReply = read_control(&mut second).await.expect("the second reply");
        assert!(first_reply.ok && second_reply.ok);

        let body = vec![0xff, 0x00, 0xfe, b'{', b'"', 0x80, 0x01];
        write_frame_body(&mut first, &body)
            .await
            .expect("an arbitrary body");
        let forwarded = read_frame_body(&mut second, MAX_FRAME_BYTES)
            .await
            .expect("the relay's copy")
            .expect("a body");
        assert_eq!(forwarded, body);
    }
}
