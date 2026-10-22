//! The relay binary's contract: one line on stdout, and it is the address the
//! relay is actually serving on.
//!
//! The spawn is the point. The library tests drive `RelayServer` in process;
//! this one proves the process starts, prints a parseable address, and pairs
//! two clients on it.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};

use summrise_mesh::relay::RelayClient;
use summrise_mesh::{Frame, Identity, Kind, RelayTlsSession};

/// Kill and reap the relay even when an assertion panics.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn the_relay_prints_the_address_it_serves_on() {
    let mut relay = KillOnDrop(
        Command::new(env!("CARGO_BIN_EXE_summrise-relay"))
            .args(["--bind", "127.0.0.1:0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the relay binary starts"),
    );

    let mut line = String::new();
    let bytes = BufReader::new(relay.0.stdout.take().expect("stdout is piped"))
        .read_line(&mut line)
        .expect("the relay prints a line");
    assert!(bytes > 0, "the relay printed nothing on stdout");

    let address: SocketAddr = line
        .trim()
        .parse()
        .expect("stdout's one line is the bound socket address");
    assert!(address.ip().is_loopback(), "the default bind is loopback");
    assert_ne!(address.port(), 0, "a bound ephemeral port is not zero");

    // The printed address must be the relay's, not just a plausible one:
    // two clients pair on it and a frame crosses — through the pinned TLS
    // session, the only public way to move frames over the relay.
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    runtime.block_on(async {
        let first_identity = Identity::generate();
        let second_identity = Identity::generate();
        let first_key = first_identity.public_bytes();
        let second_key = second_identity.public_bytes();
        let first_resolver = HashMap::from([(second_identity.public_hex(), second_key)]);
        let second_resolver = HashMap::from([(first_identity.public_hex(), first_key)]);

        let (first, second) = tokio::join!(
            RelayClient::connect(address, "relay-bin"),
            RelayClient::connect(address, "relay-bin"),
        );
        let first = first.expect("the first client pairs on the printed address");
        let second = second.expect("the second client pairs on the printed address");
        let (first, second) = tokio::join!(
            RelayTlsSession::for_peer(
                &first_identity,
                &second_key,
                first.into_inner(),
                &first_resolver,
            ),
            RelayTlsSession::for_peer(
                &second_identity,
                &first_key,
                second.into_inner(),
                &second_resolver,
            ),
        );
        let mut first = first.expect("the first TLS session authenticates");
        let mut second = second.expect("the second TLS session authenticates");
        let frame = Frame {
            kind: Kind::Ping,
            id: 1,
            payload: serde_json::json!({"from": "the printed address"}),
        };
        first.send_frame(&frame).await.expect("the frame is sent");
        assert_eq!(
            second.recv_frame().await.expect("the frame arrives"),
            frame,
            "the printed address is the relay"
        );
    });
}
