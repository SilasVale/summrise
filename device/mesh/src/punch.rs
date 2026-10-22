//! The punch: opening a path through a NAT with no inbound port.
//!
//! **The problem.** A peer behind a NAT is reachable only if the NAT already
//! holds a mapping for the sender's outbound flow, and only from the source
//! the NAT saw. A peer that has never sent anything to the other is
//! unreachable.
//!
//! **The primitive.** Given a local UDP socket and the address a peer
//! published, send a probe from that socket to that address, repeat on a
//! schedule, and call the path open when a datagram arrives **from that exact
//! address**. The outgoing probe creates the mapping in both directions: our
//! NAT now knows how to send to the peer, and the peer's NAT sees us from the
//! address it might answer.
//!
//! **The source address is the identity and the payload is not.** This module
//! does not parse a probe or a reply: a datagram from any other address is
//! ignored, and a datagram from the peer's address is evidence regardless of
//! what it contains. Trust is not decided here — the pinned QUIC handshake is
//! what proves the peer. A probe that lands on the wrong machine costs a
//! failed handshake, never a connection.
//!
//! **The deadline and the retry interval are inputs.** The scheduling
//! decision (when to give up, how long to wait) belongs to `summrise-core`'s
//! `Path`, which takes `now_ms` and never reads a clock. This module only
//! carries that decision out: [`Puncher::poll`] is the non-blocking step and
//! [`Puncher::recv`] is the same step with a socket wait in front of it.
//! Neither reads a clock; the caller supplies the elapsed milliseconds.

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;

/// The bytes of a punch probe.
///
/// The payload is deliberately meaningless to the receiver: the address is
/// the evidence, and the peer's pinned identity is the authentication. A
/// fixed marker is still useful in a packet capture.
pub const PROBE: &[u8] = b"summrise mesh punch v1";

/// What one step of the punch found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PunchOutcome {
    /// A datagram from the peer's address has arrived; the path is open.
    Open,
    /// The deadline passed without a datagram from the peer.
    TimedOut,
    /// Still punching; call again with a fresh `now_ms`.
    Probing,
}

/// One punch, towards one peer address, on one socket.
#[derive(Debug)]
pub struct Puncher {
    /// The Tokio socket, used for the asynchronous receive.
    socket: UdpSocket,
    /// A nonblocking duplicate of the same socket, used for synchronous
    /// sends. Tokio's `try_send_to` refuses with `WouldBlock` until the
    /// runtime has polled the socket's readiness once — which a synchronous
    /// `poll` never does — so the send goes through the standard library,
    /// which performs the syscall.
    sender: std::net::UdpSocket,
    /// A second duplicate, used to drain queued datagrams synchronously for
    /// the same reason.
    receiver: std::net::UdpSocket,
    peer: SocketAddr,
    /// The instant on the caller's `now_ms` scale at which the punch gives
    /// up; the same clock as [`Puncher::poll`], not a wall-clock instant.
    deadline_ms: u64,
    /// The gap between probes; at least one millisecond.
    retry_ms: u64,
    /// The next probe on the caller's `now_ms` scale.
    next_probe_ms: u64,
    /// Whether a datagram from `peer` has already been seen.
    open: bool,
}

impl Puncher {
    /// Punch towards `peer` from `socket`, giving up at `deadline_ms` and
    /// probing every `retry_ms`.
    ///
    /// `deadline_ms` is an instant on the same clock as [`Puncher::poll`]'s
    /// `now_ms`, so a caller can pass `summrise-core`'s
    /// `PathAction::StartPunch { deadline_ms }` unchanged. The first probe is
    /// due on the first poll. A `retry_ms` of zero is treated as one
    /// millisecond so a caller cannot turn the loop into a busy spin.
    ///
    /// # Errors
    ///
    /// The socket must be convertible to a standard socket and back; both
    /// conversions can fail at the OS boundary.
    pub fn new(
        socket: UdpSocket,
        peer: SocketAddr,
        deadline_ms: u64,
        retry_ms: u64,
    ) -> io::Result<Self> {
        let standard = socket.into_std()?;
        let sender = standard.try_clone()?;
        let receiver = standard.try_clone()?;
        Ok(Self {
            socket: UdpSocket::from_std(standard)?,
            sender,
            receiver,
            peer,
            deadline_ms,
            retry_ms: retry_ms.max(1),
            next_probe_ms: 0,
            open: false,
        })
    }

    /// The address this puncher is trying to open.
    pub fn peer(&self) -> SocketAddr {
        self.peer
    }

    /// The local address of the punching socket.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// The non-blocking step: send a probe if one is due, drain anything that
    /// has arrived, and report the state at `now_ms`.
    ///
    /// A datagram from any address other than [`Puncher::peer`] is discarded
    /// without changing the outcome. A datagram that arrives after the
    /// deadline but before this call still opens the path: the drain runs
    /// before the deadline is checked, so a probe that crossed the deadline
    /// in flight is not thrown away.
    pub fn poll(&mut self, now_ms: u64) -> io::Result<PunchOutcome> {
        if self.open {
            return Ok(PunchOutcome::Open);
        }
        if now_ms < self.deadline_ms && now_ms >= self.next_probe_ms {
            self.send(PROBE)?;
            self.next_probe_ms = now_ms.saturating_add(self.retry_ms);
        }
        self.drain()?;
        if self.open {
            // The peer's probe arrived before ours was sent (or ours crossed
            // it in flight). Answer it, so a one-sided punch still opens in
            // both directions.
            self.send(PROBE)?;
            return Ok(PunchOutcome::Open);
        }
        if now_ms >= self.deadline_ms {
            return Ok(PunchOutcome::TimedOut);
        }
        Ok(PunchOutcome::Probing)
    }

    /// [`Puncher::poll`], but wait until a datagram arrives or the next probe
    /// or the deadline is due.
    ///
    /// `now_ms` advances by the scheduled waits, never by reading a clock:
    /// the caller's timeline is the only timeline. A late wake-up makes the
    /// punch take slightly longer than the budget, but it can never make a
    /// dead path look open.
    pub async fn recv(&mut self, now_ms: u64) -> io::Result<PunchOutcome> {
        let mut now = now_ms;
        let mut buffer = [0u8; 2048];
        loop {
            match self.poll(now)? {
                PunchOutcome::Probing => {}
                done => return Ok(done),
            }
            let wake_ms = self.next_probe_ms.min(self.deadline_ms);
            let wait_ms = wake_ms.saturating_sub(now);
            tokio::select! {
                result = self.socket.recv_from(&mut buffer) => {
                    let (_len, from) = result?;
                    if from == self.peer {
                        self.open = true;
                        // Answer the peer's probe, so its side opens too.
                        self.send(PROBE)?;
                        return Ok(PunchOutcome::Open);
                    }
                    // The identity of the path is the address. Anything else
                    // is somebody else's datagram; keep waiting.
                }
                _ = tokio::time::sleep(Duration::from_millis(wait_ms)) => {
                    now = wake_ms;
                }
            }
        }
    }

    /// Send one probe, treating a full socket buffer as sent (the retry
    /// schedule will try again).
    fn send(&self, probe: &[u8]) -> io::Result<()> {
        match self.sender.send_to(probe, self.peer) {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Read every datagram already queued, opening only for the peer's
    /// address.
    fn drain(&mut self) -> io::Result<()> {
        let mut buffer = [0u8; 2048];
        loop {
            match self.receiver.recv_from(&mut buffer) {
                Ok((_len, from)) => {
                    if from == self.peer {
                        self.open = true;
                        return Ok(());
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn bound() -> (UdpSocket, SocketAddr) {
        let socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("a loopback UDP socket");
        let address = socket.local_addr().expect("its address");
        (socket, address)
    }

    #[tokio::test]
    async fn two_sockets_punch_and_each_sees_the_other() {
        let (a_socket, a_address) = bound().await;
        let (b_socket, b_address) = bound().await;
        let mut a = Puncher::new(a_socket, b_address, 1_000, 20).expect("a puncher");
        let mut b = Puncher::new(b_socket, a_address, 1_000, 20).expect("a puncher");

        let (a_outcome, b_outcome) = tokio::time::timeout(Duration::from_secs(2), async move {
            tokio::join!(a.recv(0), b.recv(0))
        })
        .await
        .expect("each side should see the other");

        assert_eq!(a_outcome.expect("no socket error"), PunchOutcome::Open);
        assert_eq!(b_outcome.expect("no socket error"), PunchOutcome::Open);
    }

    #[tokio::test]
    async fn a_dead_address_times_out_at_the_deadline() {
        let (socket, _) = bound().await;
        // A socket that is bound but never sends: packets sent to it are
        // accepted into its receive buffer, so no ICMP refusal races the
        // test. It is a peer that simply never answers.
        let (_dead, dead_address) = bound().await;
        let mut puncher = Puncher::new(socket, dead_address, 300, 50).expect("a puncher");

        assert_eq!(puncher.poll(0).expect("a send"), PunchOutcome::Probing);
        assert_eq!(puncher.poll(299).expect("a send"), PunchOutcome::Probing);
        assert_eq!(
            puncher.poll(300).expect("no send at the deadline"),
            PunchOutcome::TimedOut
        );
        assert_eq!(
            puncher.poll(10_000).expect("still timed out"),
            PunchOutcome::TimedOut
        );
    }

    #[tokio::test]
    async fn probes_repeat_on_the_schedule() {
        let (socket, _) = bound().await;
        let (watcher, watcher_address) = bound().await;
        let mut puncher = Puncher::new(socket, watcher_address, 1_000, 100).expect("a puncher");
        let mut buffer = [0u8; 64];

        assert_eq!(
            puncher.poll(0).expect("the first probe"),
            PunchOutcome::Probing
        );
        let (len, from) = watcher.recv_from(&mut buffer).await.expect("a probe");
        assert_eq!(&buffer[..len], PROBE);
        assert_eq!(from, puncher.local_addr().expect("the puncher's address"));

        // Nothing is due yet.
        assert_eq!(
            puncher.poll(99).expect("no probe yet"),
            PunchOutcome::Probing
        );
        let early =
            tokio::time::timeout(Duration::from_millis(50), watcher.recv_from(&mut buffer)).await;
        assert!(early.is_err(), "no probe may arrive before the retry");

        // At the retry, another probe goes out.
        assert_eq!(puncher.poll(100).expect("the retry"), PunchOutcome::Probing);
        let (len, _) = tokio::time::timeout(Duration::from_secs(1), watcher.recv_from(&mut buffer))
            .await
            .expect("the retry should arrive")
            .expect("a datagram");
        assert_eq!(&buffer[..len], PROBE);
    }

    #[tokio::test]
    async fn a_reply_from_a_different_address_is_ignored() {
        let (socket, socket_address) = bound().await;
        let (peer, peer_address) = bound().await;
        let (stranger, stranger_address) = bound().await;
        let mut puncher = Puncher::new(socket, peer_address, 1_000, 50).expect("a puncher");

        let mut task = tokio::spawn(async move { puncher.recv(0).await });

        // A datagram from somebody else must not open the path, even though
        // it is a well-formed probe to the right socket.
        stranger
            .send_to(PROBE, socket_address)
            .await
            .expect("the stranger's datagram");
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut task)
                .await
                .is_err(),
            "a datagram from {stranger_address} must not open the path"
        );

        // The peer's own datagram does. Its bytes are not inspected.
        peer.send_to(b"a probe from the right address", socket_address)
            .await
            .expect("the peer's datagram");
        let outcome = tokio::time::timeout(Duration::from_secs(1), &mut task)
            .await
            .expect("the right source should open the path")
            .expect("the task should finish")
            .expect("no socket error");
        assert_eq!(outcome, PunchOutcome::Open);
    }
}
