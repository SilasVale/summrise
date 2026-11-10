//! **A FORWARD RULE CARRIED BY A REAL sshd, THROUGH THE REAL DEVICE BINARY.**
//!
//! The unit tests drive the forward capability against an IN-PROCESS SSH server, and that fixture
//! cannot be the authority for a forwarded channel's byte order (measured 2026-11-10: with the
//! fixture dialling nothing, reading nothing and replying nothing, a client still read its own bytes
//! back, which a real sshd does not do). So the authority for the wire is here: a real `sshd`, a
//! real `summrise-device` process, and one `forward.add` through MCP.
//!
//! `#[ignore = "..."]`: this needs an sshd this test did not start — the box's own is the honest
//! stand-in, because what a forward needs from `via` is `AllowTcpForwarding` and public-key auth,
//! and any real sshd that grants both behaves the same. A conditionally-skipped test would be green
//! when nothing ran; an ignored one is red when the CI step that runs it is deleted.
//!
//! PREPARE (once, on the box that runs it) — TWO inputs, and this test invents neither:
//!   1. an sshd with `AllowTcpForwarding yes`;
//!   2. a private key that sshd ACCEPTS, at an ABSOLUTE path (the capability refuses a relative one);
//!   3. run it:
//!      `SUMMRISE_FORWARD_VIA=user@127.0.0.1:22 SUMMRISE_FORWARD_KEY=/abs/path/to/key \
//!       cargo test -p summrise-device --test forward_wire -- --ignored --nocapture`.
//!
//! The target is an HTTP service this test starts on loopback. Success is its body arriving at the
//! listen address — byte for byte, through a socket nothing else in this process owns.

#![cfg(unix)]

use std::fs::{self, File};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};

const START_TIMEOUT: Duration = Duration::from_secs(20);

/// How long the forwarded connection may take to answer, measured from `forward.add` returning.
const CARRY_TIMEOUT: Duration = Duration::from_secs(15);

const TOKEN: [u8; 32] = [0x46; 32];

static NEXT: AtomicU32 = AtomicU32::new(0);

fn token_hex() -> String {
    TOKEN.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> TempDir {
        let index = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "summrise-fwd-{}-{label}-{index}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the temp directory is creatable");
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

fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind a loopback port");
    listener.local_addr().expect("the port is known").port()
}

struct Device {
    /// Held, not read: the install and data directories live until this drops.
    _dir: TempDir,
    child: Child,
    port: u16,
}

impl Device {
    fn start(label: &str) -> Device {
        let dir = TempDir::new(label);
        let install = dir.join("install");
        let data = dir.join("data");
        fs::create_dir_all(&install).expect("the install directory is creatable");
        fs::create_dir_all(&data).expect("the data directory is creatable");
        fs::write(data.join("token"), format!("{}\n", token_hex()))
            .expect("the install token is minted");
        let logs = dir.join("logs");
        fs::create_dir_all(&logs).expect("the log directory is creatable");
        let stdout = File::create(logs.join("device.out")).expect("the stdout log");
        let stderr = File::create(logs.join("device.err")).expect("the stderr log");
        let port = free_port();
        let child = Command::new(env!("CARGO_BIN_EXE_summrise-device"))
            .arg("--install")
            .arg(&install)
            .arg("--data")
            .arg(&data)
            .arg("--port")
            .arg(port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .unwrap_or_else(|error| panic!("cannot start the device binary: {error}"));
        Device {
            _dir: dir,
            child,
            port,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    async fn wait_healthy(&self, http: &reqwest::Client) {
        let deadline = Instant::now() + START_TIMEOUT;
        let mut last = String::new();
        while Instant::now() < deadline {
            match http.get(self.url("/api/health")).send().await {
                Ok(response) if response.status().is_success() => return,
                Ok(response) => last = format!("/api/health answered {}", response.status()),
                Err(error) => last = format!("/api/health is not up yet: {error}"),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the device did not become healthy within {START_TIMEOUT:?} ({last})");
    }

    async fn connect(&self) -> RunningService<RoleClient, ()> {
        let config = StreamableHttpClientTransportConfig::with_uri(self.url("/mcp"))
            .auth_header(token_hex());
        ().serve(StreamableHttpClientTransport::from_config(config))
            .await
            .unwrap_or_else(|error| panic!("the MCP handshake fails: {error}"))
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn call(
    client: &RunningService<RoleClient, ()>,
    tool: &'static str,
    arguments: Value,
) -> CallToolResult {
    client
        .call_tool(
            CallToolRequestParams::new(tool).with_arguments(
                arguments
                    .as_object()
                    .unwrap_or_else(|| panic!("{tool}'s arguments are an object"))
                    .clone(),
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("tools/call {tool} is answered: {error}"))
}

fn text_of(result: &CallToolResult) -> &str {
    result
        .content
        .iter()
        .find_map(|block| block.as_text())
        .map(|text| text.text.as_str())
        .unwrap_or_else(|| panic!("a text result: {result:?}"))
}

/// Read an HTTP/1.1 answer's body off a socket, however long the far side takes to think.
fn read_body(mut stream: TcpStream) -> String {
    use std::io::{Read as _, Write as _};
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: forward\r\nConnection: close\r\n\r\n")
        .expect("the request is written");
    let mut body = String::new();
    stream
        .read_to_string(&mut body)
        .expect("the answer is read");
    body.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned()
}

#[tokio::test]
#[ignore = "manual: needs a real sshd with AllowTcpForwarding and an accepted key; see the header"]
async fn a_forward_rule_carries_a_real_http_answer_through_a_real_sshd() {
    let via = std::env::var("SUMMRISE_FORWARD_VIA")
        .expect("SUMMRISE_FORWARD_VIA=user@host:port names the sshd this test forwards through");
    let key = std::env::var("SUMMRISE_FORWARD_KEY").expect(
        "SUMMRISE_FORWARD_KEY=/abs/path names a private key the sshd at SUMMRISE_FORWARD_VIA accepts \
         (a key this test invented would be refused by any sshd, because nothing told it about it)",
    );

    // THE TARGET: a service on loopback that answers with the marker.
    let target = TcpListener::bind(("127.0.0.1", 0)).expect("the target port");
    let target_port = target.local_addr().expect("the target address").port();
    let marker = "FORWARDED-THROUGH-A-REAL-SSHD";
    let marker_for_task = marker.to_owned();
    std::thread::spawn(move || {
        for stream in target.incoming() {
            let Ok(mut stream) = stream else { continue };
            use std::io::{Read as _, Write as _};
            let mut buf = [0u8; 512];
            let _ = stream.read(&mut buf);
            let body = marker_for_task.clone();
            let answer = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(answer.as_bytes());
        }
    });

    // THE LISTEN ADDRESS the rule binds — this test's own, so nothing else is disturbed.
    let listen_port = free_port();
    let listen = format!("127.0.0.1:{listen_port}");

    let device = Device::start("forward-wire");
    let http = reqwest::Client::new();
    device.wait_healthy(&http).await;
    let client = device.connect().await;

    let added = call(
        &client,
        "forward.add",
        json!({
            "name": "wire",
            "listen": listen,
            "target": format!("127.0.0.1:{target_port}"),
            "via": via,
            "auth": { "kind": "key", "path": key },
        }),
    )
    .await;
    assert!(
        added.is_error != Some(true),
        "forward.add is accepted: {}",
        text_of(&added)
    );

    // THE ROW MUST BE LISTENING, and its own error says why when it is not — polling the tool rather
    // than the log is what an operator does, so the test reads what they read.
    let deadline = Instant::now() + CARRY_TIMEOUT;
    loop {
        let listed = call(&client, "forward.list", json!({})).await;
        let rows: Value = serde_json::from_str(text_of(&listed)).expect("rows are JSON");
        let row = rows
            .as_array()
            .and_then(|rows| rows.first())
            .unwrap_or_else(|| panic!("the rule is listed: {listed:?}"));
        if row["listening"] == true {
            break;
        }
        assert!(Instant::now() < deadline, "the rule never listened: {row}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // THE PROOF: a fresh connection to the LISTEN address is carried over the SSH connection to the
    // target, and the target's own words come back through it.
    let stream =
        TcpStream::connect(("127.0.0.1", listen_port)).expect("a connection to the rule's address");
    let body = tokio::task::spawn_blocking(move || read_body(stream))
        .await
        .expect("the block does not panic");
    assert_eq!(body, marker, "the far side's answer travelled the rule");

    let removed = call(&client, "forward.remove", json!({ "name": "wire" })).await;
    assert_eq!(text_of(&removed), "{\"removed\":true}");
}

/// The binary path must exist before the test starts, or `spawn` would be the first failure and the
/// message would be about the binary rather than about the forward.
#[test]
fn the_device_binary_is_present() {
    assert!(Path::new(env!("CARGO_BIN_EXE_summrise-device")).exists());
}
