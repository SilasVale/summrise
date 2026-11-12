//! A scripted plugin launcher for the host and MCP tests.
//!
//! The tests never spawn a process. Each launch gets one end of a `tokio::io::duplex` and a peer
//! task that speaks real MCP JSON-RPC on the other; the [`Session`] under test is the production one.
//! The switches are the interesting failures: a handshake that errors, pings that are ignored, a
//! peer that dies during a call, a slow call that never answers.

use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::host::{ChildProcess, LaunchSpec, Launched, Launcher, Options, PluginHost};
use crate::session::Session;

/// A launcher whose children are duplex peers.
pub struct ScriptedLauncher {
    /// How many times a plugin was started.
    pub launches: AtomicUsize,
    /// How many times a child was killed.
    pub(crate) kills: Arc<AtomicUsize>,
    /// Whether `initialize` answers with an error.
    pub(crate) fail_initialize: AtomicBool,
    /// Whether `ping` is ignored (a plugin that stopped answering health checks).
    pub(crate) ignore_pings: Arc<AtomicBool>,
    /// Whether the next `tools/call` makes the peer close instead of answering.
    pub(crate) drop_on_call: Arc<AtomicBool>,
    /// Whether `tools/call` is ignored (a slow call).
    pub(crate) ignore_calls: Arc<AtomicBool>,
    /// Break the peer's loop, closing its stdout the way a crashed process would.
    pub(crate) die: Arc<tokio::sync::Notify>,
    /// Every `tools/call` params object the peers received.
    pub(crate) calls: Arc<Mutex<Vec<Value>>>,
    /// The tools each launch advertises.
    pub(crate) tools: Vec<Value>,
}

impl ScriptedLauncher {
    /// A launcher that advertises `tools` on every launch.
    pub fn new(tools: Vec<Value>) -> Arc<ScriptedLauncher> {
        Arc::new(ScriptedLauncher {
            launches: AtomicUsize::new(0),
            kills: Arc::new(AtomicUsize::new(0)),
            fail_initialize: AtomicBool::new(false),
            ignore_pings: Arc::new(AtomicBool::new(false)),
            drop_on_call: Arc::new(AtomicBool::new(false)),
            ignore_calls: Arc::new(AtomicBool::new(false)),
            die: Arc::new(tokio::sync::Notify::new()),
            calls: Arc::new(Mutex::new(Vec::new())),
            tools,
        })
    }

    /// The common fixture: one read-only `greet` tool.
    pub fn hello() -> Arc<ScriptedLauncher> {
        ScriptedLauncher::new(vec![json!({
            "name": "greet",
            "title": "Greet someone",
            "description": "Say hello to a name.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": { "type": "string" } },
                "required": ["name"]
            },
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        })])
    }
}

impl Launcher for ScriptedLauncher {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, String> {
        self.launches.fetch_add(1, Ordering::SeqCst);
        let (client, peer) = tokio::io::duplex(64 * 1024);
        let session = Session::new(client, spec.log.clone());

        let tools = self.tools.clone();
        let fail_initialize = self.fail_initialize.load(Ordering::SeqCst);
        let ignore_pings = Arc::clone(&self.ignore_pings);
        let drop_on_call = Arc::clone(&self.drop_on_call);
        let ignore_calls = Arc::clone(&self.ignore_calls);
        let die = Arc::clone(&self.die);
        let calls = Arc::clone(&self.calls);

        tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(peer);
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            loop {
                line.clear();
                let read = tokio::select! {
                    _ = die.notified() => break,
                    read = reader.read_line(&mut line) => read,
                };
                match read {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
                    continue;
                };
                let Some(method) = message["method"].as_str() else {
                    continue;
                };
                // Notifications (no id) need no answer.
                let Some(id) = message.get("id").cloned() else {
                    continue;
                };
                let result =
                    |result: Value| json!({ "jsonrpc": "2.0", "id": id, "result": result });
                let answer = match method {
                    "initialize" => {
                        if fail_initialize {
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "error": { "code": -32603, "message": "initialize refused" }
                            })
                        } else {
                            result(json!({
                                "protocolVersion": "2025-06-18",
                                "capabilities": { "tools": {} },
                                "serverInfo": { "name": "scripted", "version": "0.1.0" }
                            }))
                        }
                    }
                    "ping" => {
                        if ignore_pings.load(Ordering::SeqCst) {
                            continue;
                        }
                        result(json!({}))
                    }
                    "tools/list" => result(json!({ "tools": tools })),
                    "tools/call" => {
                        if drop_on_call.swap(false, Ordering::SeqCst) {
                            // Dropping both halves closes the stream: the device sees its plugin die.
                            break;
                        }
                        if ignore_calls.load(Ordering::SeqCst) {
                            continue;
                        }
                        let params = message["params"].clone();
                        let tool = params["name"].as_str().unwrap_or("tool").to_owned();
                        let name = params["arguments"]["name"]
                            .as_str()
                            .unwrap_or("someone")
                            .to_owned();
                        calls.lock().unwrap().push(params);
                        result(json!({
                            "content": [{
                                "type": "text",
                                "text": format!("{tool} says hi {name}")
                            }]
                        }))
                    }
                    _ => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": "method not found" }
                    }),
                };
                if write
                    .write_all(format!("{answer}\n").as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        Ok(Launched {
            session,
            child: Arc::new(ScriptedChild {
                kills: Arc::clone(&self.kills),
            }),
            pid: None,
        })
    }
}

/// A child that only counts its kills; there is no process behind it.
struct ScriptedChild {
    kills: Arc<AtomicUsize>,
}

impl ChildProcess for ScriptedChild {
    fn kill(&self) {
        self.kills.fetch_add(1, Ordering::SeqCst);
    }
}

/// The timings the tests run on: the same state machine, compressed.
pub(crate) fn options() -> Options {
    Options {
        startup_timeout: Duration::from_secs(2),
        ping_interval: Duration::from_millis(25),
        ping_timeout: Duration::from_millis(25),
        call_timeout: Duration::from_secs(2),
        retry_base: Duration::from_millis(20),
        retry_cap: Duration::from_millis(60),
        start_window: Duration::from_secs(60),
        max_starts: 5,
        stop_grace: Duration::from_millis(5),
        tick: Duration::from_millis(5),
    }
}

/// A host over a fresh data directory, with a scripted launcher.
pub fn host(temp: impl AsRef<Path>, launcher: Arc<ScriptedLauncher>) -> PluginHost {
    let data = temp.as_ref().join("data");
    std::fs::create_dir_all(&data).expect("the data directory is creatable");
    PluginHost::with_launcher(&data, launcher, options())
}

/// A plugin directory whose manifest names `namespace` and a command that exists (this test binary).
pub fn plugin_dir(temp: impl AsRef<Path>, namespace: &str) -> PathBuf {
    let directory = temp.as_ref().join("plugins").join(namespace);
    std::fs::create_dir_all(&directory).expect("the plugin directory is creatable");
    let command = std::env::current_exe().expect("the test binary has a path");
    let manifest = json!({
        "id": format!("com.example.{namespace}"),
        "namespace": namespace,
        "name": format!("{namespace} plugin"),
        "version": "0.1.0",
        "description": "A scripted plugin for the host tests.",
        "command": [command.display().to_string()],
    });
    std::fs::write(directory.join("plugin.json"), manifest.to_string())
        .expect("the manifest is written");
    directory
}

/// Wait until `check` is true, or the deadline passes. The condition is re-checked at the deadline so
/// a call that settles exactly then is not a flake.
pub async fn wait_until<F, Fut>(deadline: Duration, mut check: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let started = Instant::now();
    loop {
        if check().await {
            return true;
        }
        if started.elapsed() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// A unique temporary directory for this crate's tests, removed on drop.
///
/// The host is its own crate now, so it cannot borrow the app's `crate::test_support::TempDir`; this
/// is the same discipline locally: a name that includes the pid and a process-wide counter cannot
/// collide between test binaries, and `Drop` removes what the test created.
#[cfg(test)]
pub(crate) struct TempDir {
    path: PathBuf,
}

#[cfg(test)]
impl TempDir {
    pub(crate) fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "summrise-plugin-host-test-{}-{unique}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the test directory is creatable");
        Self { path }
    }

    pub(crate) fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

#[cfg(test)]
impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
