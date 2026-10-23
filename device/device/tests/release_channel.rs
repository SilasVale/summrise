//! The release channel is a fact the launcher passes, not a constant the app assumes.
//!
//! The launcher reads the signed sidecar it booted and passes `--release-channel` on the child's
//! argv. This test drives the real binary twice on one data directory: a canary start must record
//! `canary` in `state.json` and `/api/health`, and a restart with a stable argv must flip both back.
//! A standalone start (no release flags) is the unit suite's `stable` case.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// A directory removed when the test ends, unique across parallel test binaries.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> TempDir {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "summrise-device-channel-{}-{}-{label}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the fixture directory is creatable");
        TempDir(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A real device process that is killed and reaped when the test ends, including on a panic.
struct Running {
    child: Child,
    stderr: PathBuf,
}

impl Running {
    fn start(dir: &TempDir, channel: &str, port: u16) -> Running {
        let stderr = dir.join(&format!("{channel}.err"));
        let log = File::create(&stderr).expect("the log is created");
        let child = Command::new(env!("CARGO_BIN_EXE_summrise-device"))
            .arg("--install")
            .arg(dir.join("install"))
            .arg("--data")
            .arg(dir.join("data"))
            .arg("--port")
            .arg(port.to_string())
            .arg("--release-version")
            .arg("9.9.9")
            .arg("--release-sha")
            .arg("b1946ac92492d2347c6235b4d2611184")
            .arg("--release-channel")
            .arg(channel)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .expect("the device binary starts");
        Running { child, stderr }
    }

    fn stderr_text(&self) -> String {
        fs::read_to_string(&self.stderr).unwrap_or_default()
    }

    /// Poll `/api/health` until the app answers, then return the document.
    fn wait_healthy(&mut self, port: u16) -> serde_json::Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(status) = self
                .child
                .try_wait()
                .expect("the child's status is readable")
            {
                panic!(
                    "the device exited ({status}) before it served\n--- stderr ---\n{}",
                    self.stderr_text()
                );
            }
            if let Some(body) = http_get(port, "/api/health") {
                if let Ok(health) = serde_json::from_str::<serde_json::Value>(&body) {
                    if health["status"] == "ok" {
                        return health;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "the device did not answer /api/health within 10s\n--- stderr ---\n{}",
            self.stderr_text()
        );
    }

    fn stop(&mut self) {
        if self
            .child
            .try_wait()
            .expect("the child's status is readable")
            .is_none()
        {
            self.child.kill().expect("the app is killable");
        }
        self.child.wait().expect("the app is reaped");
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if self
            .child
            .try_wait()
            .map(|status| status.is_none())
            .unwrap_or(true)
        {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// A loopback port nothing is listening on, chosen the way the launcher's probe chooses one.
fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind a loopback port");
    listener.local_addr().expect("a bound address").port()
}

/// One plain-HTTP GET; `None` when nothing answers yet.
fn http_get(port: u16, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("a read timeout");
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    let response = String::from_utf8_lossy(&raw);
    let (_, body) = response.split_once("\r\n\r\n")?;
    Some(body.to_owned())
}

fn recorded_channel(dir: &TempDir) -> String {
    let text = fs::read_to_string(dir.join("data").join("state.json"))
        .expect("the state document is readable");
    let state: serde_json::Value = serde_json::from_str(&text).expect("the state document is JSON");
    state["install"]["channel"]
        .as_str()
        .unwrap_or_else(|| panic!("the document names a channel: {state}"))
        .to_owned()
}

#[test]
fn a_canary_start_records_canary_and_a_stable_restart_flips_it_back() {
    let dir = TempDir::new("flip");

    let port = free_port();
    let mut canary = Running::start(&dir, "canary", port);
    let health = canary.wait_healthy(port);
    assert_eq!(
        health["channel"], "canary",
        "health reports the channel the launcher passed: {health}"
    );
    assert_eq!(
        recorded_channel(&dir),
        "canary",
        "state.json records the channel the launcher passed, not a constant"
    );
    canary.stop();

    let port = free_port();
    let mut stable = Running::start(&dir, "stable", port);
    let health = stable.wait_healthy(port);
    assert_eq!(
        health["channel"], "stable",
        "a restart under a stable sidecar flips health back: {health}"
    );
    assert_eq!(
        recorded_channel(&dir),
        "stable",
        "the channel is a release fact and follows the sidecar this start booted"
    );
    stable.stop();
}
