//! The broker binary's contract: one line on stdout, and it is the address
//! the broker is actually serving on.
//!
//! The spawn is the point. Every other broker test drives the router in
//! process; this one proves the process starts, prints a parseable address,
//! and answers on it.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Kill and reap the broker even when an assertion panics.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn the_broker_prints_the_address_it_serves_on() {
    let mut broker = KillOnDrop(
        Command::new(env!("CARGO_BIN_EXE_summrise-broker"))
            .args(["--bind", "127.0.0.1:0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the broker binary starts"),
    );

    let mut line = String::new();
    let bytes = BufReader::new(broker.0.stdout.take().expect("stdout is piped"))
        .read_line(&mut line)
        .expect("the broker prints a line");
    assert!(bytes > 0, "the broker printed nothing on stdout");

    let address: SocketAddr = line
        .trim()
        .parse()
        .expect("stdout's one line is the bound socket address");
    assert!(address.ip().is_loopback(), "the default bind is loopback");
    assert_ne!(address.port(), 0, "a bound ephemeral port is not zero");

    // The printed address must be the broker's, not just a plausible one.
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .expect("the printed address accepts a connection");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout is settable");
    stream
        .write_all(b"GET /payload HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("the request is writable");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("the broker answers and closes");
    assert!(
        response.starts_with("HTTP/1.1 404"),
        "the printed address is the broker, which has no payload route: {response}"
    );
}
