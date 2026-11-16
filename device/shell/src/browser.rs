//! The browser-session door: one `POST` to the loopback control API the Electron host owns.
//!
//! **THE 9444 LISTENER IS NOT OURS.** It belongs to the running `SummriseDesktop`, which creates the
//! real browser window through CDP. This module is the shell's client of that existing route — it
//! opens no listener, adds no route and sends no token, because the API has none: its guard is the
//! loopback bind plus the foreign-origin veto, and a native request that sends no `Origin` is exactly
//! what that veto admits.
//!
//! The request is hand-written HTTP/1.1 over loopback: one fixed route, one fixed query, a
//! `Content-Length: 0` and `Connection: close`. There is no TLS, no redirect and no retry — a door
//! that is not answering is a sentence naming the door, not a storm of attempts. The answer is
//! bounded to 64 KiB, because a control reply is a small JSON document and the shell should not read
//! an unbounded body from a process it does not own.
//!
//! **THE LIVE ANSWER IS CHUNK-FRAMED.** Node's `http.ServerResponse` writes this route's JSON without
//! a `Content-Length`, so the wire carries `Transfer-Encoding: chunked`; [`parse_response`] decodes it
//! as well as a plain body, which is what the stub tests below use for the status contract.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// The existing loopback control API, as the Electron host binds it.
pub const CONTROL: &str = "127.0.0.1:9444";

/// The route the Browser row asks for: the console's Product Browser session at `about:blank`.
const PATH: &str = "/api/browser-session/open?url=about%3Ablank";

/// A control answer is a small JSON document; stop reading after this much rather than trust it.
const READ_LIMIT: u64 = 64 * 1024;

/// The exact bytes this shell sends. No `Origin`: the API's foreign-origin veto is what a browser
/// page would trip, and a native client that omits the header is the caller the route expects.
pub fn request() -> String {
    format!(
        "POST {PATH} HTTP/1.1\r\nHost: {CONTROL}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

/// Ask the door for one browser session.
pub fn open() -> Result<(), String> {
    open_at(CONTROL)
}

/// Ask the door at `addr` for one browser session.
///
/// One connect, one write, one bounded read. A refused or absent listener is an error that names the
/// address; the caller decides whether the console's prefilled view is the fallback.
pub fn open_at(addr: &str) -> Result<(), String> {
    let mut addresses = addr
        .to_socket_addrs()
        .map_err(|error| format!("the browser-session door at {addr}: {error}"))?;
    let address = addresses
        .next()
        .ok_or_else(|| format!("the browser-session door at {addr} resolved to no address"))?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|error| format!("the browser-session door at {addr} is not answering: {error}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    stream.write_all(request().as_bytes()).map_err(|error| {
        format!("the browser-session door at {addr} refused the request: {error}")
    })?;
    // Read at most the cap. A short read ended at EOF; only a read that filled the cap needs the
    // one-byte probe that distinguishes a legitimate answer ending exactly at 64 KiB from a body
    // that was cut off. An over-long answer is a refusal, never a document parsed from its prefix.
    let mut raw = Vec::new();
    Read::by_ref(&mut stream)
        .take(READ_LIMIT)
        .read_to_end(&mut raw)
        .map_err(|error| format!("the browser-session door at {addr} did not answer: {error}"))?;
    if raw.len() as u64 == READ_LIMIT {
        let mut extra = [0u8; 1];
        match stream.read(&mut extra) {
            Ok(0) => {}
            Ok(_) => {
                return Err(format!(
                    "the browser-session door at {addr} answered more than {READ_LIMIT} bytes"
                ))
            }
            Err(error) => {
                return Err(format!(
                    "the browser-session door at {addr} did not finish its answer: {error}"
                ))
            }
        }
    }
    parse_response(&raw).map_err(|error| format!("{addr}: {error}"))
}

/// Read one control answer: a `200` whose JSON document says `{"ok":true}`.
///
/// Every other answer is an `Err` that names the status and the body, so a veto (403), a route the
/// host does not have (404) or a refusal (`{"ok":false,"error":…}`) is a sentence the log can carry
/// rather than silence. A body with extra fields is accepted: the route answers the opened session's
/// id, URL and CDP address beside `ok`.
pub fn parse_response(raw: &[u8]) -> Result<(), String> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| {
            format!(
                "the browser-session door answered without an HTTP body: {:?}",
                String::from_utf8_lossy(raw)
            )
        })?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let status_line = head.lines().next().unwrap_or_default();
    let status = status_line.split_whitespace().nth(1).unwrap_or_default();
    let body = decode_body(&head, &raw[split + 4..]).map_err(|reason| {
        format!("the browser-session door answered {status_line} with a body that does not decode: {reason}")
    })?;
    let body = String::from_utf8_lossy(&body);
    if status != "200" {
        return Err(format!(
            "the browser-session door answered {status_line}: {body}"
        ));
    }
    let document: serde_json::Value = serde_json::from_str(body.trim()).map_err(|error| {
        format!(
            "the browser-session door answered {status_line} with something that is not JSON: {error}: {body}"
        )
    })?;
    if document.get("ok") == Some(&serde_json::Value::Bool(true)) {
        Ok(())
    } else {
        Err(format!(
            "the browser-session door answered {status_line} without ok:true: {body}"
        ))
    }
}

/// The body bytes, decoding `Transfer-Encoding: chunked` when the head says so.
///
/// Node's server writes this route without a `Content-Length`, so the live answer is chunk-framed.
/// A plain body (what the loopback stubs and older/test servers send) is returned unchanged.
fn decode_body(head: &str, body: &[u8]) -> Result<Vec<u8>, String> {
    let chunked = head.lines().any(|line| {
        let line = line.to_ascii_lowercase();
        line.starts_with("transfer-encoding:") && line.contains("chunked")
    });
    if !chunked {
        return Ok(body.to_vec());
    }
    let mut decoded = Vec::new();
    let mut rest = body;
    loop {
        let Some((line, after)) = split_crlf(rest) else {
            return Err("a chunk size line has no CRLF".to_owned());
        };
        let size_text = line.split(|byte| *byte == b';').next().unwrap_or(line);
        let size_text = std::str::from_utf8(size_text)
            .map_err(|error| format!("a chunk size is not text: {error}"))?
            .trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|error| format!("{size_text:?} is not a chunk size: {error}"))?;
        if size == 0 {
            // The last chunk's trailers (if any) are not part of the document.
            return Ok(decoded);
        }
        // A chunk length is untrusted input. The peer can name `usize::MAX`, so the frame's end is
        // computed with a checked add: an overflow must be a refusal, not a debug panic or a wrap
        // that slices outside the buffer in release.
        let framed = size
            .checked_add(2)
            .ok_or_else(|| format!("a {size}-byte chunk cannot be framed on this platform"))?;
        if after.len() < framed || &after[size..framed] != b"\r\n" {
            return Err(format!(
                "a {size}-byte chunk is truncated or is not followed by CRLF"
            ));
        }
        decoded.extend_from_slice(&after[..size]);
        rest = &after[framed..];
    }
}

/// Split off one CRLF-terminated line, returning the line and what follows it.
fn split_crlf(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let split = bytes.windows(2).position(|window| window == b"\r\n")?;
    Some((&bytes[..split], &bytes[split + 2..]))
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    /// Serve one canned response on a free loopback port.
    fn serve(response: impl AsRef<[u8]> + Send + 'static) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let port = listener.local_addr().expect("the port").port();
        let response = response.as_ref().to_vec();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("one connection");
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(&response);
        });
        format!("127.0.0.1:{port}")
    }

    /// The same, but the test keeps the request bytes the door received.
    fn serve_and_capture(
        response: impl AsRef<[u8]> + Send + 'static,
    ) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let port = listener.local_addr().expect("the port").port();
        let response = response.as_ref().to_vec();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("one connection");
            let mut request = [0u8; 4096];
            let read = stream.read(&mut request).expect("the request bytes");
            let _ = sender.send(String::from_utf8_lossy(&request[..read]).into_owned());
            let _ = stream.write_all(&response);
        });
        (format!("127.0.0.1:{port}"), receiver)
    }

    /// **THE WIRE BYTES ARE THE CONTRACT.** The route, the method, the encoded query, the host, the
    /// zero length and the close are pinned exactly; a request that grew an `Origin` would be vetoed
    /// by the API the way the console's HTTPS page is, and one that changed a byte of the query would
    /// miss the route. The comparison is the whole request, not a `contains`, so an added header
    /// fails here.
    #[test]
    fn the_request_is_the_existing_route_with_no_origin() {
        let expected = "POST /api/browser-session/open?url=about%3Ablank HTTP/1.1\r\n\
                        Host: 127.0.0.1:9444\r\n\
                        Content-Length: 0\r\n\
                        Connection: close\r\n\
                        \r\n";
        assert_eq!(request(), expected, "the exact request bytes");
        assert!(
            !request().to_ascii_lowercase().contains("origin"),
            "the foreign-origin veto admits an absent Origin, not a populated one"
        );
        assert!(request().starts_with("POST "), "the route is POST-only");
    }

    #[test]
    fn a_two_hundred_with_ok_is_accepted() {
        let raw = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n\
                    {\"ok\":true,\"id\":\"b1\",\"url\":\"about:blank\",\"cdp\":\"http://127.0.0.1:9333\"}";
        assert_eq!(
            parse_response(raw),
            Ok(()),
            "extra fields are the route's own"
        );
    }

    /// The live answer is chunk-framed because Node writes it without a `Content-Length`; a plain-
    /// body parser would pass the stubs and fail the one machine the door exists for.
    #[test]
    fn the_live_chunk_framed_answer_is_decoded() {
        let raw = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                    Transfer-Encoding: chunked\r\n\r\n\
                    37\r\n{\"ok\":true,\"sessions\":[],\"cdp\":\"http://127.0.0.1:9333\"}\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw), Ok(()), "chunked 200 ok:true");
    }

    /// A peer can name a chunk length that cannot be framed on this platform. The frame end is
    /// untrusted arithmetic: it must be a refusal, never a debug panic and never a release wrap
    /// that slices outside the body.
    #[test]
    fn an_unframeable_chunk_size_is_a_refusal_not_a_panic() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                    ffffffffffffffff\r\n{\"ok\":true}";
        let error = parse_response(raw).expect_err("an impossible chunk length is not success");
        assert!(
            error.contains("cannot be framed") || error.contains("not a chunk size"),
            "{error}"
        );
    }

    #[test]
    fn a_forbidden_answer_names_its_status_and_body() {
        let raw = b"HTTP/1.1 403 Forbidden\r\ncontent-type: application/json\r\n\r\n\
                    {\"ok\":false,\"error\":\"foreign origin\"}";
        let error = parse_response(raw).expect_err("a veto is not success");
        assert!(error.contains("403"), "{error}");
        assert!(error.contains("foreign origin"), "{error}");
    }

    #[test]
    fn a_two_hundred_with_ok_false_is_a_refusal_not_a_success() {
        let raw =
            b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"ok\":false,\"error\":\"no display\"}";
        let error = parse_response(raw).expect_err("ok:false is not success");
        assert!(error.contains("200"), "{error}");
        assert!(error.contains("\"ok\":false"), "{error}");
        assert!(error.contains("no display"), "{error}");
    }

    #[test]
    fn a_two_hundred_that_is_not_json_is_an_error_naming_the_body() {
        let raw = b"HTTP/1.1 200 OK\r\ncontent-type: text/html\r\n\r\n<html>a login page</html>";
        let error = parse_response(raw).expect_err("HTML is not the control document");
        assert!(error.contains("200"), "{error}");
        assert!(error.contains("a login page"), "{error}");
    }

    #[test]
    fn a_bound_stub_answers_open_at() {
        let (target, request_seen) = serve_and_capture(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"ok\":true}",
        );
        assert_eq!(
            open_at(&target),
            Ok(()),
            "a 200 ok:true is the door opening"
        );
        assert_eq!(
            request_seen.recv().expect("the stub saw one request"),
            request(),
            "open_at sends exactly the pinned request"
        );
    }

    /// The live door is chunk-framed, so `open_at` must succeed against that framing too, not only
    /// against the plain bodies the other stubs use.
    #[test]
    fn open_at_reads_the_live_chunk_framed_answer() {
        let target = serve(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
             Transfer-Encoding: chunked\r\n\r\n\
             b\r\n{\"ok\":true}\r\n0\r\n\r\n",
        );
        assert_eq!(open_at(&target), Ok(()), "a chunked 200 ok:true");
    }

    /// **A REFUSAL IS ONE ATTEMPT.** The Electron host may be absent, and a retry loop on the tray's
    /// event loop would be a storm against a closed door. The stub serves a 403 once and closes; the
    /// listener is then checked after the call returned, so a retry — synchronous or deferred —
    /// would be a queued connection.
    #[test]
    fn a_refusal_is_one_attempt_not_a_retry() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let port = listener.local_addr().expect("the port").port();
        let server = listener.try_clone().expect("a second listener handle");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = server.accept().expect("the first attempt");
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(
                b"HTTP/1.1 403 Forbidden\r\ncontent-type: application/json\r\n\r\n{\"ok\":false}",
            );
            // Dropping the stream closes the connection; a client retry occupies the backlog.
        });
        let target = format!("127.0.0.1:{port}");
        let error = open_at(&target).expect_err("403 is a refusal");
        assert!(error.contains("403"), "{error}");
        handle.join().expect("the stub thread");
        // The refusal is already read; a retry had its chance while the first answer was parsed.
        std::thread::sleep(Duration::from_millis(50));
        listener.set_nonblocking(true).expect("nonblocking");
        match listener.accept() {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            other => panic!("a refusal was retried: {other:?}"),
        }
    }

    #[test]
    fn a_refusing_stub_is_an_error_from_open_at() {
        let target = serve(
            "HTTP/1.1 403 Forbidden\r\ncontent-type: application/json\r\n\r\n\
             {\"ok\":false,\"error\":\"foreign origin\"}",
        );
        let error = open_at(&target).expect_err("403 is a refusal");
        assert!(error.contains("403"), "{error}");
        assert!(
            error.contains(&target),
            "the sentence names the door: {error}"
        );
    }

    /// A host that does not have the route answers 404; that is a refusal naming its status and the
    /// body, not a silence and not a success.
    #[test]
    fn a_missing_route_names_its_status() {
        let target = serve(
            "HTTP/1.1 404 Not Found\r\ncontent-type: application/json\r\n\r\n\
             {\"ok\":false,\"error\":\"not found\"}",
        );
        let error = open_at(&target).expect_err("a missing route is not success");
        assert!(error.contains("404"), "{error}");
        assert!(error.contains("not found"), "{error}");
    }

    /// **THE READ CAP IS A CONTRACT.** A control answer is small; an answer over 64 KiB is refused
    /// rather than parsed from the prefix that happened to fit, so a hostile or broken listener
    /// cannot feed the shell an unbounded body.
    #[test]
    fn a_response_over_the_read_cap_is_refused() {
        let mut response = String::from(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"ok\":true,\"padding\":\"",
        );
        response.push_str(&"a".repeat(70 * 1024));
        response.push_str("\"}");
        let target = serve(response);
        let error = open_at(&target).expect_err("an answer over the cap is not success");
        assert!(error.contains("more than"), "{error}");
        assert!(
            error.contains(&target),
            "the sentence names the door: {error}"
        );
    }

    #[test]
    fn a_closed_port_names_the_address() {
        // Bind and drop, so the port is almost certainly closed.
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("a listener");
            listener.local_addr().expect("the port").port()
        };
        let target = format!("127.0.0.1:{port}");
        let error = open_at(&target).expect_err("no listener is a refusal");
        assert!(
            error.contains(&target),
            "the sentence names the address: {error}"
        );
        assert!(
            error.contains("not answering"),
            "the sentence says the door did not answer: {error}"
        );
    }
}
