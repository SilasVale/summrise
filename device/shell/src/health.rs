//! The device's own health, read over loopback.
//!
//! This is the same `/api/health` the panel reads and the launcher probes: the app is up, the version
//! it is running, and the channel it follows. It needs no credential because it is the loopback
//! surface the device publishes to any local process — the tray is not asking the device anything the
//! panel would not answer.
//!
//! The request is hand-written because it is HTTP/1.1 over loopback with one fixed path and a
//! `Connection: close`: there is no TLS, no redirect, no chunking to honour. A full HTTP client would
//! be a dependency the shell does not otherwise need.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde::Deserialize;

/// The facts the tray shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Health {
    /// `ok` while the app is serving.
    pub status: String,
    /// The release the app is running.
    pub version: Option<String>,
    /// The channel that release came from.
    pub channel: Option<String>,
}

/// What one probe found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// The panel answered with a health document.
    Up(Health),
    /// The panel did not answer, or answered something that is not a health document.
    Down(String),
}

/// Probe `target` (`host:port`) once.
pub fn probe(target: &str) -> State {
    match ask(target) {
        Ok(body) => match serde_json::from_str::<Wire>(&body) {
            Ok(wire) => State::Up(Health {
                status: wire.status,
                version: wire.version,
                channel: wire.channel,
            }),
            Err(error) => State::Down(format!("the panel answered something else: {error}")),
        },
        Err(reason) => State::Down(reason),
    }
}

/// The document `/api/health` serves. Everything but `status` is optional so a newer or older device
/// still produces a state rather than a parse failure.
#[derive(Deserialize)]
struct Wire {
    status: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    channel: Option<String>,
}

fn ask(target: &str) -> Result<String, String> {
    let mut addresses = target
        .to_socket_addrs()
        .map_err(|error| format!("{target}: {error}"))?;
    let address = addresses
        .next()
        .ok_or("the panel name resolved to no address")?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|error| format!("{target}: {error}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    stream
        .write_all(b"GET /api/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .map_err(|error| format!("{target}: {error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("{target}: {error}"))?;
    body_of(&response)
}

/// The body of a response this shell accepts: a `200` whose head ends where the body starts.
fn body_of(response: &[u8]) -> Result<String, String> {
    let text = String::from_utf8_lossy(response);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or("the panel's answer has no body")?;
    let status = head.lines().next().unwrap_or_default();
    if !status.contains(" 200 ") {
        return Err(format!("the panel answered {status}"));
    }
    Ok(body.to_owned())
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    /// Serve one canned response, then tell the probe where it is.
    fn serve(response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let port = listener.local_addr().expect("the port").port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("one connection");
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(response.as_bytes());
        });
        format!("127.0.0.1:{port}")
    }

    #[test]
    fn a_health_document_becomes_a_state() {
        let target = serve(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n\
             {\"status\":\"ok\",\"version\":\"0.3.29\",\"channel\":\"canary\",\"pid\":30580}",
        );
        assert_eq!(
            probe(&target),
            State::Up(Health {
                status: "ok".to_owned(),
                version: Some("0.3.29".to_owned()),
                channel: Some("canary".to_owned()),
            })
        );
    }

    #[test]
    fn a_refusal_is_a_down_state_naming_the_status() {
        let target = serve("HTTP/1.1 503 Service Unavailable\r\n\r\n{\"error\":\"no\"}");
        match probe(&target) {
            State::Down(reason) => assert!(reason.contains("503"), "{reason}"),
            other => panic!("a refusal must not read as up: {other:?}"),
        }
    }

    #[test]
    fn something_that_is_not_json_is_down_rather_than_a_guess() {
        let target = serve("HTTP/1.1 200 OK\r\n\r\n<html>a login page</html>");
        match probe(&target) {
            State::Down(reason) => assert!(reason.contains("something else"), "{reason}"),
            other => panic!("html must not read as up: {other:?}"),
        }
    }

    #[test]
    fn an_address_with_no_listener_is_down() {
        // Bind and drop, so the port is almost certainly closed.
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("a listener");
            listener.local_addr().expect("the port").port()
        };
        assert!(matches!(
            probe(&format!("127.0.0.1:{port}")),
            State::Down(_)
        ));
    }
}
