//! A real MCP client against the real device binary (design §4.5).
//!
//! Task 2's unit tests drive the router with a hand-written JSON-RPC client because they have to
//! see the wire. This file proves the other half: `rmcp`'s own streamable-HTTP client — configured
//! the way a product client is configured, with the install token as its bearer — completes the
//! handshake, reads the registry through `tools/list`, moves a file and a memory entry through
//! `tools/call`, is refused outside the files sandbox, and is refused by the token gate when it
//! presents no token.
//!
//! It starts `summrise-device` itself with a temp install and data directory, so the proof covers
//! the process's configuration, token file, router, capability sandbox and audit path as one
//! running product rather than a router assembled inside the test.
//!
//! The Windows lane (`launch/tests/windows_lifecycle.rs`) drives the same surface against the
//! installed binary; this file is the Linux half of that pair.

#![cfg(unix)]

use std::fs::{self, File};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use rmcp::model::{CallToolRequestParams, CallToolResult, ProtocolVersion};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};

/// How long the device may take to answer `/api/health` after it is spawned.
const START_TIMEOUT: Duration = Duration::from_secs(20);

/// The install token the fixture mints: 32 bytes as 64 hex characters, the shape the installer
/// writes. This is fixture entropy, not a secret — it guards one loopback port for one test.
const TOKEN: [u8; 32] = [0x4d; 32];

static NEXT: AtomicU32 = AtomicU32::new(0);

fn token_hex() -> String {
    TOKEN.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A uniquely named directory under the system temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> TempDir {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "summrise-mcp-client-{}-{nanos}-{}-{label}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the fixture directory is creatable");
        TempDir(path)
    }

    fn join(&self, path: impl AsRef<Path>) -> PathBuf {
        self.0.join(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The real `summrise-device` process the test drives.
struct Device {
    /// Kept alive so every path the config names exists for the whole test.
    dir: TempDir,
    /// Killed when the test ends, including on a panic.
    child: Child,
    port: u16,
}

impl Device {
    fn start(label: &str) -> Device {
        let dir = TempDir::new(label);
        let install = dir.join("install");
        let data = dir.join("data");
        // A fresh install: no workspace and no directories under it. The device creates the root at
        // startup; the client creates `notes` through `files.mkdir`.
        for path in [&install, &data] {
            fs::create_dir_all(path).expect("the fixture directory is creatable");
        }
        // A real file outside the sandbox: the traversal below must be refused by the capability,
        // not merely fail because the target does not exist.
        fs::write(data.join("outside.txt"), "SECRET OUTSIDE THE FILES ROOT")
            .expect("the outside file is written");
        fs::write(data.join("token"), format!("{}\n", token_hex()))
            .expect("the install token is minted");

        let logs = dir.join("logs");
        fs::create_dir_all(&logs).expect("the log directory is creatable");
        let stdout = File::create(logs.join("device.out")).expect("the stdout log is created");
        let stderr = File::create(logs.join("device.err")).expect("the stderr log is created");

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

        Device { dir, child, port }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn stderr_text(&self) -> String {
        fs::read_to_string(self.dir.join("logs").join("device.err")).unwrap_or_default()
    }

    /// Poll health until the app serves. A failed start carries the device's stderr, which is where
    /// a bad argument or an unparsable token file says why.
    async fn wait_healthy(&self, http: &reqwest::Client) {
        let deadline = Instant::now() + START_TIMEOUT;
        let mut last = String::new();
        while Instant::now() < deadline {
            match http.get(self.url("/api/health")).send().await {
                Ok(response) if response.status().is_success() => match response.text().await {
                    Ok(body) => match serde_json::from_str::<Value>(&body) {
                        Ok(health) if health["status"] == "ok" => return,
                        Ok(_) => last = format!("health is not ok yet: {body}"),
                        Err(error) => last = format!("health is not JSON ({error}): {body}"),
                    },
                    Err(error) => last = format!("the health body is unreadable: {error}"),
                },
                Ok(response) => last = format!("/api/health answered {}", response.status()),
                Err(error) => last = format!("/api/health is not up yet: {error}"),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!(
            "the device did not answer /api/health within {}s ({last})\n--- device stderr ---\n{}",
            START_TIMEOUT.as_secs(),
            self.stderr_text()
        );
    }

    /// Connect rmcp's own streamable-HTTP client with the install token as its bearer.
    async fn connect(&self) -> RunningService<RoleClient, ()> {
        let config = StreamableHttpClientTransportConfig::with_uri(self.url("/mcp"))
            .auth_header(token_hex());
        ().serve(StreamableHttpClientTransport::from_config(config))
            .await
            .unwrap_or_else(|error| {
                panic!("rmcp's client cannot complete the MCP handshake: {error}")
            })
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind a loopback port");
    listener.local_addr().expect("the port is known").port()
}

/// One `tools/call`, with the arguments the tool's schema names.
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
                    .unwrap_or_else(|| panic!("{tool}'s arguments are an object: {arguments}"))
                    .clone(),
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("tools/call {tool} is answered: {error}"))
}

/// The single text block a successful tool result carries.
fn text_of(result: &CallToolResult) -> &str {
    result
        .content
        .iter()
        .find_map(|block| block.as_text())
        .map(|text| text.text.as_str())
        .unwrap_or_else(|| panic!("a text result: {result:?}"))
}

#[tokio::test]
async fn a_real_mcp_client_proves_the_registry_surface() {
    let device = Device::start("mcp-client");
    let http = reqwest::Client::new();
    device.wait_healthy(&http).await;

    // 1. The token gate answers before MCP sees a request, so a client without the install token
    //    never reaches the handshake. This is one HTTP request, not a second MCP client: the status
    //    code is exactly the fact under test.
    let refused = http
        .post(device.url("/mcp"))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream",
        )
        .body(
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "intruder", "version": "0" }
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("the MCP endpoint answers");
    assert_eq!(
        refused.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "an unauthenticated client must be refused by the token gate"
    );
    let body: Value =
        serde_json::from_str(&refused.text().await.expect("the 401 body is readable"))
            .expect("the 401 body is JSON");
    assert_eq!(body, json!({ "error": "unauthorized" }));

    // 2. The real client negotiates `initialize` and keeps the server's answer.
    let client = device.connect().await;
    let info = client
        .peer_info()
        .expect("the client kept the server's initialize result");
    assert_eq!(
        info.protocol_version,
        ProtocolVersion::LATEST,
        "both ends are rmcp 2 and agree on the latest protocol version"
    );
    assert_eq!(info.server_info.name, "summrise-device");
    assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    assert!(
        info.capabilities.tools.is_some(),
        "the server advertises the tools capability: {:?}",
        info.capabilities
    );

    // 3. `tools/list` is exactly the registry: the names, their order, and each description are
    //    read from the same table the authorization decision reads.
    let tools = client
        .list_all_tools()
        .await
        .expect("tools/list answers through the real client");
    let registry: Vec<&str> = summrise_core::registry::CAPABILITIES
        .iter()
        .flat_map(|capability| capability.tools)
        .map(|tool| tool.name)
        .collect();
    let advertised: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert_eq!(
        tools.len(),
        registry.len(),
        "the registry's tools, and no others"
    );
    assert_eq!(
        advertised, registry,
        "tools/list is a reading of the registry"
    );
    for tool in &tools {
        let declared = summrise_core::registry::tool(&tool.name)
            .unwrap_or_else(|| panic!("{} is in the registry", tool.name));
        assert_eq!(
            tool.description.as_deref(),
            Some(declared.description),
            "{}'s description is the registry's sentence",
            tool.name
        );
        assert_eq!(
            tool.input_schema["type"], "object",
            "{}'s input schema is an object",
            tool.name
        );
        assert!(
            tool.input_schema["properties"].is_object(),
            "{}'s input schema has no properties object: {:?}",
            tool.name,
            tool.input_schema
        );
        // A tool that takes arguments names them; a no-argument tool (terminal.list) may have an
        // empty `properties` object, which is still an object and still refuses unknown fields.
        if tool
            .input_schema
            .get("required")
            .and_then(|required| required.as_array())
            .is_some_and(|required| !required.is_empty())
        {
            assert!(
                tool.input_schema["properties"]
                    .as_object()
                    .is_some_and(|properties| !properties.is_empty()),
                "{}'s required arguments are named in its properties: {:?}",
                tool.name,
                tool.input_schema
            );
        }
    }
    let write_schema = tools
        .iter()
        .find(|tool| tool.name == "files.write")
        .expect("files.write is advertised")
        .input_schema
        .clone();
    assert_eq!(
        write_schema["required"],
        json!(["path", "content"]),
        "a write needs both the destination and its bytes"
    );

    // 4. A fresh device's root exists (created at startup), and `files.mkdir` is how a client
    //    creates the first directory: no fixture made `notes` for it.
    let workspace = device.dir.join("data").join("workspace");
    assert!(
        workspace.is_dir(),
        "the device created the files root at startup: {}",
        workspace.display()
    );
    assert!(!workspace.join("notes").exists());
    let root = call(&client, "files.list", json!({ "path": "" })).await;
    assert_eq!(
        root.is_error,
        Some(false),
        "the fresh root lists as an empty directory rather than NotFound: {root:?}"
    );
    assert_eq!(text_of(&root), "[]");
    let made = call(&client, "files.mkdir", json!({ "path": "notes" })).await;
    assert_eq!(made.is_error, Some(false), "{made:?}");
    assert!(workspace.join("notes").is_dir());

    // 4b. `files.write` then `files.read` returns the same bytes, including multi-byte text.
    let content = "the rmcp client wrote these bytes — π = 3.14159\n";
    let written = call(
        &client,
        "files.write",
        json!({ "path": "notes/client.txt", "content": content }),
    )
    .await;
    assert_eq!(written.is_error, Some(false), "{written:?}");
    let read = call(&client, "files.read", json!({ "path": "notes/client.txt" })).await;
    assert_eq!(read.is_error, Some(false), "{read:?}");
    assert_eq!(
        text_of(&read),
        content,
        "the read is the bytes just written"
    );

    // 5. `memory.append` then `memory.search` finds the entry through the same client.
    let appended = call(
        &client,
        "memory.append",
        json!({ "text": "the rmcp client remembers this", "tags": ["interop"] }),
    )
    .await;
    assert_eq!(appended.is_error, Some(false), "{appended:?}");
    let found = call(&client, "memory.search", json!({ "query": "remembers" })).await;
    assert_eq!(found.is_error, Some(false), "{found:?}");
    let entries: Value = serde_json::from_str(text_of(&found)).expect("search answers JSON");
    assert_eq!(entries[0]["text"], "the rmcp client remembers this");
    assert_eq!(entries[0]["tags"], json!(["interop"]));

    // 6. A path outside the files root is a tool-level refusal, not bytes. The traversal target
    //    exists and holds a secret, so a capability that resolved it would be visible here.
    let absolute = device
        .dir
        .join("data")
        .join("outside.txt")
        .to_str()
        .expect("the fixture path is UTF-8")
        .to_owned();
    for path in ["../outside.txt".to_owned(), absolute] {
        let refused = call(&client, "files.read", json!({ "path": path })).await;
        assert_eq!(
            refused.is_error,
            Some(true),
            "{path:?} must be refused by the sandbox: {refused:?}"
        );
        let reason = text_of(&refused);
        assert!(
            reason.contains("files.read"),
            "the refusal names the tool: {reason}"
        );
        assert!(
            !reason.contains("SECRET OUTSIDE THE FILES ROOT"),
            "a refused read must not return the file: {reason}"
        );
    }

    // 7. The audit records both halves of every call: a begin line before dispatch and an end line
    //    after it. Eight calls above, so eight of each; a refused read ends with `ok: false`.
    let audit = fs::read_to_string(device.dir.join("data").join("events.jsonl"))
        .expect("the audit file exists after eight tool calls");
    let lines: Vec<Value> = audit
        .lines()
        .map(|line| serde_json::from_str(line).expect("an audit line is JSON"))
        .collect();
    assert_eq!(
        lines.iter().filter(|line| line["event"] == "begin").count(),
        8,
        "one begin line per tool call: {lines:?}"
    );
    let ends: Vec<&Value> = lines.iter().filter(|line| line["event"] == "end").collect();
    assert_eq!(ends.len(), 8, "one end line per tool call: {lines:?}");
    assert_eq!(
        lines
            .iter()
            .filter(|line| line["event"] == "begin")
            .map(|line| line["id"].clone())
            .collect::<Vec<Value>>(),
        ends.iter()
            .map(|line| line["id"].clone())
            .collect::<Vec<Value>>(),
        "begins and ends pair by id in call order"
    );

    client.cancel().await.expect("the client closes cleanly");
}
