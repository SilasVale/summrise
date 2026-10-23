//! The app's self-watchdog (fix design F5 / I4).
//!
//! A process that is alive but no longer serving is invisible to the supervisor, which only sees the
//! child exit. This thread probes the app's own `/api/health` every 30 s and, after two consecutive
//! failures, exits nonzero. **The supervisor is what sees that exit**: it owns the child and restarts
//! it within its restart budget. The SCM's failure actions belong to the supervisor, not to this
//! process — they run only if the supervisor itself gives up and exits nonzero.
//!
//! The counter is a pure function ([`record_probe`]) so the two-strike rule is unit-tested without a
//! clock; the loop around it is deliberately trivial and untested.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// How often the app probes itself.
pub const PROBE_INTERVAL: Duration = Duration::from_secs(30);

/// The connect/read/write timeout of one probe.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// How many consecutive failed probes make the process exit.
pub const MAX_CONSECUTIVE_FAILURES: u8 = 2;

/// What the counter says to do after one probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeVerdict {
    /// Keep running; the process answered, or has not failed enough times yet.
    Continue,
    /// The process has failed the two-strike rule; exit so the supervisor sees the child exit.
    Exit,
}

/// The pure half of the watchdog: fold one probe result into the consecutive-failure count.
///
/// A healthy answer resets the count, because the rule is *consecutive* failures: a process that
/// answered once is not the hung process the previous failures described.
pub fn record_probe(consecutive_failures: u8, healthy: bool) -> (u8, ProbeVerdict) {
    let consecutive = if healthy {
        0
    } else {
        consecutive_failures.saturating_add(1)
    };
    let verdict = if consecutive >= MAX_CONSECUTIVE_FAILURES {
        ProbeVerdict::Exit
    } else {
        ProbeVerdict::Continue
    };
    (consecutive, verdict)
}

/// Start the watchdog thread. A thread that cannot start is loud but not fatal: the app still
/// serves, and the missing recovery is the supervisor's concern rather than a reason to refuse
/// traffic.
pub fn spawn(port: u16) {
    let builder = std::thread::Builder::new().name("summrise-device-health-watchdog".to_owned());
    if let Err(error) = builder.spawn(move || watch(port)) {
        eprintln!(
            "summrise-device: cannot start the health watchdog ({error}); a hung app will not be \
             restarted by the supervisor"
        );
    }
}

/// The timer loop: probe every [`PROBE_INTERVAL`], and exit after [`MAX_CONSECUTIVE_FAILURES`].
fn watch(port: u16) {
    let mut consecutive_failures = 0;
    loop {
        std::thread::sleep(PROBE_INTERVAL);
        let healthy = probe(port);
        let (next, verdict) = record_probe(consecutive_failures, healthy);
        consecutive_failures = next;
        if verdict == ProbeVerdict::Exit {
            eprintln!(
                "summrise-device: /api/health failed {consecutive_failures} consecutive probes; \
                 exiting so the supervisor restarts the device"
            );
            std::process::exit(1);
        }
    }
}

/// One probe, judged as healthy only when it is a 200 whose document says `status == "ok"`.
fn probe(port: u16) -> bool {
    probe_once(port).unwrap_or(false)
}

fn probe_once(port: u16) -> io::Result<bool> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, PROBE_TIMEOUT)?;
    stream.set_read_timeout(Some(PROBE_TIMEOUT))?;
    stream.set_write_timeout(Some(PROBE_TIMEOUT))?;
    stream
        .write_all(b"GET /api/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")?;

    let mut raw = Vec::new();
    stream.take(64 * 1024).read_to_end(&mut raw)?;
    Ok(health_answer(&raw))
}

/// Whether a raw HTTP response is the health answer the watchdog accepts.
fn health_answer(raw: &[u8]) -> bool {
    let text = String::from_utf8_lossy(raw);
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return false;
    };
    let status_ok = head.lines().next().is_some_and(|status_line| {
        let mut parts = status_line.split_whitespace();
        parts.next() == Some("HTTP/1.1") && parts.next() == Some("200")
    });
    if !status_ok {
        return false;
    }

    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|document| {
            document
                .get("status")
                .and_then(|status| status.as_str())
                .map(|status| status == "ok")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_failure_is_not_enough_and_two_are() {
        assert_eq!(record_probe(0, false), (1, ProbeVerdict::Continue));
        assert_eq!(record_probe(1, false), (2, ProbeVerdict::Exit));
        assert_eq!(record_probe(2, false), (3, ProbeVerdict::Exit));
    }

    #[test]
    fn a_healthy_probe_resets_the_consecutive_count() {
        assert_eq!(record_probe(1, true), (0, ProbeVerdict::Continue));
        assert_eq!(record_probe(9, true), (0, ProbeVerdict::Continue));
        assert_eq!(
            record_probe(0, true),
            (0, ProbeVerdict::Continue),
            "a healthy start stays healthy"
        );
    }

    #[test]
    fn a_failure_after_a_success_starts_a_new_strike_sequence() {
        let (after_success, _) = record_probe(1, true);
        assert_eq!(
            record_probe(after_success, false),
            (1, ProbeVerdict::Continue),
            "the count is consecutive, not lifetime"
        );
    }

    #[test]
    fn the_watchdog_reads_only_a_200_ok_document_as_healthy() {
        let response = |status: &str, body: &str| {
            format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len())
        };

        assert!(health_answer(
            response("200 OK", r#"{"status":"ok","version":"2.0.3","pid":7}"#).as_bytes()
        ));
        assert!(!health_answer(
            response("500 Internal Server Error", r#"{"status":"ok"}"#).as_bytes()
        ));
        assert!(!health_answer(
            response("200 OK", r#"{"status":"down"}"#).as_bytes()
        ));
        assert!(!health_answer(response("200 OK", "not json").as_bytes()));
        assert!(!health_answer(b"not an HTTP response"));
        assert!(!health_answer(b""));
    }
}
