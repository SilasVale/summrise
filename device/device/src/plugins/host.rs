//! The plugin supervisor (design §8.2): the registry of installed plugins, and the state machine that
//! keeps each one alive.
//!
//! A plugin is a directory with a `plugin.json`; installing it *registers the path*. The directory
//! stays the developer's, removal never deletes it, and nothing is copied. The state machine is the
//! spec's diagram made real:
//!
//! ```text
//! add -> validate -> spawn -> initialize -> tools/list -> Running
//!                                     | failure          | ping x 3 failed / crash
//!                                     v                  v
//!                                  Failed <- backoff restart (1s,2s,4s,8s,16s,30s cap)
//!                                     |
//!                                     +-- manual restart
//! remove / shutdown -> close stdin -> grace -> kill
//! ```
//!
//! Every spawn goes through [`Launcher`], a one-method boundary. Production uses [`ProcessLauncher`]
//! (`tokio::process`); the tests replace it with a scripted peer over `tokio::io::duplex`. The
//! protocol is not replaced: both sides construct the real [`Session`], so the state machine is
//! proven against the code production runs.
//!
//! **Nothing here outlives the host.** The supervisor holds a `Weak`, so dropping the last host drops
//! the state — and with it the real child, whose command is built with `kill_on_drop`. [`stop_all`]
//! is the orderly path (close stdin, grace, kill) and is called on the device's shutdown.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use summrise_core::plugins::{tool_name, ManifestError, PluginManifest, MANIFEST_FILE};

use super::logs::{self, LogSink};
use super::session::{PluginTool, Session, SessionError};

/// The registry file under the data directory.
const REGISTRY_FILE: &str = "plugins.json";

/// The registry file's schema version. A file this build does not know is not started.
const REGISTRY_VERSION: u32 = 1;

/// The backoff schedule's first delay and its cap (design §8.2: 1, 2, 4, 8, 16, 30 s).
const DEFAULT_RETRY_BASE_MS: u64 = 1_000;
/// The largest delay between restart attempts.
const DEFAULT_RETRY_CAP_MS: u64 = 30_000;

/// What a plugin's row says. The words are the page's (`Running`, `Starting`, `Stopped`, `Failed`),
/// lowercased on the wire exactly as the plan's contract states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginState {
    /// Spawning, initializing, or listing tools.
    Starting,
    /// Initialized and advertising tools.
    Running,
    /// Registered, not running (at boot before `start_all`, or after removal/stop).
    Stopped,
    /// The last start or health check failed; the supervisor may be waiting to retry.
    Failed,
}

impl PluginState {
    /// The wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            PluginState::Starting => "starting",
            PluginState::Running => "running",
            PluginState::Stopped => "stopped",
            PluginState::Failed => "failed",
        }
    }
}

/// One plugin as `plugins.list` reports it (the plan's fixed JSON shape).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginStatus {
    /// The manifest's stable id.
    pub id: String,
    /// The tool namespace.
    pub namespace: String,
    /// The human name.
    pub name: String,
    /// The plugin's version.
    pub version: String,
    /// The one-sentence description, when it has one.
    pub description: Option<String>,
    /// The state machine's current word.
    pub state: PluginState,
    /// How many tools the plugin currently contributes (zero unless running).
    pub tools: usize,
    /// The registered source directory.
    pub path: String,
    /// Why the last start or health check failed, when one did.
    pub last_error: Option<String>,
}

impl PluginStatus {
    /// The exact object `plugins.list` returns.
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "namespace": self.namespace,
            "name": self.name,
            "version": self.version,
            "description": self.description,
            "state": self.state.as_str(),
            "tools": self.tools,
            "path": self.path,
            "last_error": self.last_error,
        })
    }
}

/// One plugin tool as the catalogue advertises it: namespaced, with the destructive fact resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginToolView {
    /// `<namespace>.<tool>` — the name a client calls.
    pub name: String,
    /// A human title, when the plugin supplied one.
    pub title: Option<String>,
    /// The plugin's one-line description, when it supplied one.
    pub description: Option<String>,
    /// The plugin's input schema.
    pub input_schema: Value,
    /// MCP's `readOnlyHint`, passed through.
    pub read_only: Option<bool>,
    /// MCP's `destructiveHint`, passed through as the plugin wrote it (absent means the MCP default,
    /// which is destructive); `destructive` is the fact computed with that default.
    pub destructive_hint: Option<bool>,
    /// The destructive fact the device authorizes with.
    pub destructive: bool,
    /// MCP's `idempotentHint`, passed through.
    pub idempotent: Option<bool>,
    /// MCP's `openWorldHint`, passed through.
    pub open_world: Option<bool>,
}

/// One plugin directory's manifest, read without starting or recording anything.
///
/// The console's add sheet validates a path on blur with this shape; it is deliberately narrower than
/// [`PluginStatus`] because nothing has been installed yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginInspection {
    /// The manifest's `id`.
    pub id: String,
    /// The namespace the plugin would take.
    pub namespace: String,
    /// The human name.
    pub name: String,
    /// The plugin's version.
    pub version: String,
    /// The one-sentence description, when present.
    pub description: Option<String>,
}

impl PluginInspection {
    /// The exact object `plugins.inspect` returns.
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "namespace": self.namespace,
            "name": self.name,
            "version": self.version,
            "description": self.description,
        })
    }
}

/// Why a plugin operation failed, in the product's shape.
///
/// [`PluginError::to_text`] is the JSON `{code, message, action}` the spec says a plugin tool returns:
/// one sentence and one next action, never a bare "permission denied".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginError {
    /// The path does not resolve to a directory.
    InvalidPath {
        /// The path as the caller wrote it.
        path: String,
        /// What was wrong.
        detail: String,
    },
    /// `plugin.json` is missing, unreadable, or invalid.
    Manifest {
        /// The manifest's path.
        path: String,
        /// The rule that failed, as a sentence.
        detail: String,
    },
    /// The namespace or id is already installed.
    Duplicate {
        /// `namespace` or `id`.
        field: &'static str,
        /// The value that is taken.
        value: String,
    },
    /// The namespace is a built-in capability's name.
    ReservedNamespace {
        /// The namespace.
        namespace: String,
    },
    /// `command[0]` does not resolve.
    CommandMissing {
        /// The command as written.
        command: String,
    },
    /// A plugin tool name is not legal once namespaced.
    InvalidToolName {
        /// The plugin's namespace.
        namespace: String,
        /// The offending tool name.
        tool: String,
        /// The rule's sentence.
        detail: String,
    },
    /// A plugin tool would shadow a built-in or another plugin's tool.
    ToolCollision {
        /// The namespaced name.
        tool: String,
        /// Who already owns it.
        owner: String,
    },
    /// `plugins.inspect`: there is no directory at the path.
    InspectNotFound {
        /// The path as the caller wrote it.
        path: String,
    },
    /// `plugins.inspect`: the directory has no `plugin.json`.
    InspectNoManifest {
        /// The resolved directory.
        path: String,
    },
    /// `plugins.inspect`: the manifest is invalid.
    InspectInvalidManifest {
        /// The resolved directory.
        path: String,
        /// The rule that failed; its display is `{field}: {reason}`.
        error: ManifestError,
    },
    /// No plugin has that namespace.
    NotFound {
        /// The namespace.
        namespace: String,
    },
    /// The plugin exists but is not running.
    NotRunning {
        /// The namespace.
        namespace: String,
        /// Its current state.
        state: &'static str,
    },
    /// The running plugin does not advertise that tool.
    ToolNotFound {
        /// The namespaced name.
        tool: String,
    },
    /// The child could not be started.
    Spawn {
        /// The namespace.
        namespace: String,
        /// What the operating system said.
        detail: String,
    },
    /// A request got no answer in time. The plugin is not killed for a slow call.
    Timeout {
        /// The namespace.
        namespace: String,
        /// The method that was not answered.
        method: String,
        /// The deadline.
        timeout: Duration,
    },
    /// The plugin exited while a call was in flight.
    Exited {
        /// The namespace.
        namespace: String,
    },
    /// The plugin failed: a handshake, a tool error, or a start attempt.
    Failed {
        /// The namespace.
        namespace: String,
        /// The sentence.
        detail: String,
    },
    /// The plugin answered with a JSON-RPC error.
    Rpc {
        /// The namespace.
        namespace: String,
        /// The method.
        method: String,
        /// The plugin's code.
        code: i64,
        /// The plugin's message.
        message: String,
    },
    /// Writing to the plugin failed.
    Io {
        /// The namespace.
        namespace: String,
        /// What failed.
        detail: String,
    },
    /// The plugin answered something MCP does not allow.
    Protocol {
        /// The namespace.
        namespace: String,
        /// What was wrong.
        detail: String,
    },
}

impl PluginError {
    /// The machine-readable code the tool text carries.
    pub fn code(&self) -> &'static str {
        match self {
            PluginError::InvalidPath { .. } => "PLUGIN_PATH_INVALID",
            PluginError::Manifest { .. } => "PLUGIN_MANIFEST_INVALID",
            PluginError::Duplicate { .. } => "PLUGIN_ALREADY_INSTALLED",
            PluginError::ReservedNamespace { .. } => "PLUGIN_NAMESPACE_RESERVED",
            PluginError::CommandMissing { .. } => "PLUGIN_COMMAND_MISSING",
            PluginError::InvalidToolName { .. } => "PLUGIN_TOOL_NAME_INVALID",
            PluginError::ToolCollision { .. } => "PLUGIN_TOOL_COLLISION",
            PluginError::InspectNotFound { .. } => "NOT_FOUND",
            PluginError::InspectNoManifest { .. } => "NO_MANIFEST",
            PluginError::InspectInvalidManifest { .. } => "INVALID_MANIFEST",
            PluginError::NotFound { .. } => "PLUGIN_NOT_FOUND",
            PluginError::NotRunning { .. } => "PLUGIN_NOT_RUNNING",
            PluginError::ToolNotFound { .. } => "PLUGIN_TOOL_NOT_FOUND",
            PluginError::Spawn { .. } => "PLUGIN_SPAWN_FAILED",
            PluginError::Timeout { .. } => "PLUGIN_TIMEOUT",
            PluginError::Exited { .. } => "PLUGIN_EXITED",
            PluginError::Failed { .. } => "PLUGIN_FAILED",
            PluginError::Rpc { .. } => "PLUGIN_ERROR",
            PluginError::Io { .. } => "PLUGIN_IO",
            PluginError::Protocol { .. } => "PLUGIN_PROTOCOL",
        }
    }

    /// The next action the sentence names.
    pub fn action(&self) -> &'static str {
        match self {
            PluginError::InvalidPath { .. }
            | PluginError::Manifest { .. }
            | PluginError::Duplicate { .. }
            | PluginError::ReservedNamespace { .. }
            | PluginError::CommandMissing { .. }
            | PluginError::InvalidToolName { .. }
            | PluginError::ToolCollision { .. } => {
                "fix the plugin directory or its plugin.json, then plugins.add"
            }
            PluginError::InspectNotFound { .. } => {
                "check the directory path, then inspect it again"
            }
            PluginError::InspectNoManifest { .. } => "add a plugin.json, then plugins.add",
            PluginError::InspectInvalidManifest { .. } => "fix plugin.json, then plugins.add",
            PluginError::NotFound { .. } | PluginError::ToolNotFound { .. } => "plugins.list",
            PluginError::NotRunning { .. } => "plugins.restart",
            PluginError::Spawn { .. } | PluginError::Failed { .. } => {
                "plugins.restart, then plugins.logs"
            }
            PluginError::Timeout { .. } => "plugins.logs",
            PluginError::Exited { .. } => "plugins.restart",
            PluginError::Rpc { .. } | PluginError::Io { .. } | PluginError::Protocol { .. } => {
                "plugins.logs"
            }
        }
    }

    /// The JSON text a plugin tool returns as its result when it fails.
    pub fn to_text(&self) -> String {
        json!({
            "code": self.code(),
            "message": self.to_string(),
            "action": self.action(),
        })
        .to_string()
    }

    /// Turn a session error into the plugin-level error, naming the plugin.
    fn from_session(namespace: &str, error: SessionError) -> PluginError {
        match error {
            SessionError::Timeout { method, timeout } => PluginError::Timeout {
                namespace: namespace.to_owned(),
                method,
                timeout,
            },
            SessionError::Rpc {
                method,
                code,
                message,
            } => PluginError::Rpc {
                namespace: namespace.to_owned(),
                method,
                code,
                message,
            },
            SessionError::Tool { text } => PluginError::Failed {
                namespace: namespace.to_owned(),
                detail: text,
            },
            SessionError::Exited => PluginError::Exited {
                namespace: namespace.to_owned(),
            },
            SessionError::Closed { method } => PluginError::Io {
                namespace: namespace.to_owned(),
                detail: format!("{method} was not sent: the plugin connection is closed"),
            },
            SessionError::Protocol(detail) => PluginError::Protocol {
                namespace: namespace.to_owned(),
                detail,
            },
            SessionError::Io(detail) => PluginError::Io {
                namespace: namespace.to_owned(),
                detail,
            },
        }
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::InvalidPath { path, detail } => {
                write!(f, "{path} is not a plugin directory: {detail}")
            }
            PluginError::Manifest { path, detail } => write!(f, "{path}: {detail}"),
            PluginError::Duplicate { field, value } => write!(
                f,
                "another plugin already uses the {field} {value:?}; remove it first or choose a different one"
            ),
            PluginError::ReservedNamespace { namespace } => write!(
                f,
                "the namespace {namespace:?} is a built-in capability; choose another namespace in plugin.json"
            ),
            PluginError::CommandMissing { command } => write!(
                f,
                "the command {command:?} was not found; use an absolute path or a program on PATH"
            ),
            PluginError::InvalidToolName {
                namespace,
                tool,
                detail,
            } => write!(
                f,
                "plugin {namespace} advertises a tool {tool:?} that cannot be called: {detail}"
            ),
            PluginError::ToolCollision { tool, owner } => {
                write!(f, "the tool {tool:?} is already provided by {owner}")
            }
            PluginError::InspectNotFound { path } => write!(f, "no directory at {path}"),
            PluginError::InspectNoManifest { path } => write!(f, "no plugin.json in {path}"),
            PluginError::InspectInvalidManifest { error, .. } => write!(f, "{error}"),
            PluginError::NotFound { namespace } => write!(f, "no plugin named {namespace}"),
            PluginError::NotRunning { namespace, state } => {
                write!(f, "plugin {namespace} is {state}, not running")
            }
            PluginError::ToolNotFound { tool } => {
                write!(f, "the running plugin does not advertise {tool}")
            }
            PluginError::Spawn { namespace, detail } => {
                write!(f, "plugin {namespace} could not be started: {detail}")
            }
            PluginError::Timeout {
                namespace,
                method,
                timeout,
            } => write!(
                f,
                "plugin {namespace} did not answer {method} within {} s",
                timeout.as_secs_f64()
            ),
            PluginError::Exited { namespace } => {
                write!(f, "plugin {namespace} exited while the call was in flight")
            }
            PluginError::Failed { namespace, detail } => write!(f, "plugin {namespace}: {detail}"),
            PluginError::Rpc {
                namespace,
                method,
                code,
                message,
            } => write!(
                f,
                "plugin {namespace} answered {method} with JSON-RPC error {code}: {message}"
            ),
            PluginError::Io { namespace, detail } => {
                write!(f, "plugin {namespace}: {detail}")
            }
            PluginError::Protocol { namespace, detail } => {
                write!(f, "plugin {namespace} spoke the wrong protocol: {detail}")
            }
        }
    }
}

impl std::error::Error for PluginError {}

/// The timings the supervisor runs on. Production uses [`Options::default`] (the spec's numbers);
/// tests shorten them so a restart is observable without waiting a minute.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// How long spawn + initialize + tools/list may take.
    pub startup_timeout: Duration,
    /// How often a running plugin is pinged.
    pub ping_interval: Duration,
    /// How long one ping may take.
    pub ping_timeout: Duration,
    /// How long one `tools/call` may take.
    pub call_timeout: Duration,
    /// The first backoff delay.
    pub retry_base: Duration,
    /// The backoff cap.
    pub retry_cap: Duration,
    /// The window the five-start budget counts over.
    pub start_window: Duration,
    /// How many starts are allowed in the window before the plugin stays failed.
    pub max_starts: usize,
    /// How long a stopped plugin gets between stdin EOF and the kill.
    pub stop_grace: Duration,
    /// How often the supervisor looks at the state.
    pub tick: Duration,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            startup_timeout: Duration::from_secs(10),
            ping_interval: Duration::from_secs(30),
            ping_timeout: Duration::from_secs(5),
            call_timeout: Duration::from_secs(120),
            retry_base: Duration::from_millis(DEFAULT_RETRY_BASE_MS),
            retry_cap: Duration::from_millis(DEFAULT_RETRY_CAP_MS),
            start_window: Duration::from_secs(300),
            max_starts: 5,
            stop_grace: Duration::from_secs(2),
            tick: Duration::from_millis(500),
        }
    }
}

/// The child process behind a live session. The supervisor owns it; dropping it kills it.
pub trait ChildProcess: Send + Sync {
    /// Ask the operating system to terminate the child. Best effort: a process already gone is fine.
    fn kill(&self);
}

/// Everything a launcher needs to start one plugin.
pub struct LaunchSpec {
    /// The manifest's argv.
    pub command: Vec<String>,
    /// The resolved working directory.
    pub cwd: PathBuf,
    /// The manifest's environment additions.
    pub env: BTreeMap<String, String>,
    /// The plugin's log, for the session's protocol notes.
    pub log: LogSink,
}

/// One started plugin: the live protocol session and the process that owns it.
pub struct Launched {
    /// The stdio MCP session over the child's pipes (or a duplex in a test).
    pub session: Session,
    /// The process, killed on stop or when the host is dropped.
    pub child: Arc<dyn ChildProcess>,
    /// The operating system's pid, when it has one.
    pub pid: Option<u32>,
}

/// The one boundary between the supervisor and the operating system.
///
/// Production has one implementation ([`ProcessLauncher`]); a test supplies a scripted peer on
/// `tokio::io::duplex`. Because both return a real [`Session`], the state machine is exercised
/// against the same protocol code that runs on a device.
pub trait Launcher: Send + Sync {
    /// Start one plugin and return its session.
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, String>;
}

/// The real launcher: a `tokio::process` child speaking MCP on stdin/stdout.
pub struct ProcessLauncher;

impl Launcher for ProcessLauncher {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, String> {
        let mut command = tokio::process::Command::new(&spec.command[0]);
        command.args(&spec.command[1..]);
        command.current_dir(&spec.cwd);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // The last line of defence against an orphan: if every handle to the host is dropped — a
        // panic, a test, a shutdown that did not reach `stop_all` — dropping the child kills it.
        command.kill_on_drop(true);
        if !spec.env.is_empty() {
            command.envs(&spec.env);
        }
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: a plugin is not a console app.

        let mut child = command
            .spawn()
            .map_err(|error| format!("could not run {}: {error}", spec.command[0]))?;
        let pid = child.id();
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "the child was started without a stdin pipe".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "the child was started without a stdout pipe".to_owned())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "the child was started without a stderr pipe".to_owned())?;

        let session = Session::new(ChildStdio { stdin, stdout }, spec.log.clone());
        let log = spec.log.clone();
        // stderr is copied to the log; stdout is the protocol and is not logged line by line.
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt as _;
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log.line(line);
            }
        });

        Ok(Launched {
            session,
            child: Arc::new(RealChild {
                child: Mutex::new(child),
            }),
            pid,
        })
    }
}

/// The real child.
struct RealChild {
    child: Mutex<tokio::process::Child>,
}

impl ChildProcess for RealChild {
    fn kill(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.start_kill();
        }
    }
}

/// The two pipes of a child's stdio, joined into the one duplex stream a [`Session`] reads.
struct ChildStdio {
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
}

impl tokio::io::AsyncRead for ChildStdio {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stdout).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for ChildStdio {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stdin).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stdin).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stdin).poll_shutdown(cx)
    }
}

/// The installed plugins and their supervisor.
pub struct PluginHost {
    inner: Arc<Inner>,
}

impl PluginHost {
    /// Read the registry under `data_dir`; start nothing.
    ///
    /// Boot is [`Self::start_all`]; a host with no `plugins.json` is an empty host and a log line,
    /// not an error, because most devices have no plugins.
    pub fn load(data_dir: &Path) -> PluginHost {
        PluginHost::with_launcher(data_dir, Arc::new(ProcessLauncher), Options::default())
    }

    /// The same, with a launcher and timings a test controls.
    pub(crate) fn with_launcher(
        data_dir: &Path,
        launcher: Arc<dyn Launcher>,
        options: Options,
    ) -> PluginHost {
        let (records, note) = load_registry(data_dir);
        if let Some(note) = note {
            eprintln!("summrise-device: {note}");
        }
        let mut plugins = BTreeMap::new();
        for record in records {
            let namespace = record.manifest.namespace.clone();
            let log = LogSink::to_file(log_path(data_dir, &namespace));
            plugins.insert(namespace, Entry::new(record, log));
        }
        PluginHost {
            inner: Arc::new(Inner {
                data_dir: data_dir.to_path_buf(),
                options,
                launcher,
                state: Mutex::new(HostState { plugins }),
                wake: Arc::new(tokio::sync::Notify::new()),
                stopping: Arc::new(AtomicBool::new(false)),
                supervisor: Mutex::new(None),
            }),
        }
    }

    /// Start every registered plugin and keep watching them.
    ///
    /// Returns before the plugins are `Running`: a device must not delay its HTTP surface on a
    /// plugin that takes its 10 s to initialize. The supervisor's first tick starts them, and their
    /// state is visible as `starting` until then.
    pub async fn start_all(&self) {
        self.inner.ensure_supervisor();
        self.inner.wake.notify_one();
    }

    /// Every installed plugin, in namespace order.
    pub async fn list(&self) -> Vec<PluginStatus> {
        self.inner.state().plugins.values().map(status_of).collect()
    }

    /// One plugin, or `None`. Tests and diagnostics read a single row; the catalogue uses `list`.
    #[cfg(test)]
    pub fn status(&self, namespace: &str) -> Option<PluginStatus> {
        self.inner.state().plugins.get(namespace).map(status_of)
    }

    /// The namespaced tools of every running plugin, sorted by namespace and then by tool name.
    pub fn tools(&self) -> Vec<PluginToolView> {
        let state = self.inner.state();
        let mut views = Vec::new();
        for entry in state.plugins.values() {
            if !entry.is_running() {
                continue;
            }
            for tool in &entry.tools {
                views.push(view_of(entry, tool));
            }
        }
        views
    }

    /// The destructive flag for one namespaced plugin tool, or `None` when no running plugin
    /// advertises it. `None` is a denial, exactly as it is for a name outside the registry.
    pub fn destructive(&self, tool: &str) -> Option<bool> {
        let (namespace, _) = tool.split_once('.')?;
        let state = self.inner.state();
        let entry = state.plugins.get(namespace)?;
        if !entry.is_running() {
            return None;
        }
        entry
            .tools
            .iter()
            .find(|candidate| advertised_name(entry, candidate).as_deref() == Some(tool))
            .map(PluginTool::destructive)
    }

    /// Whether `namespace` belongs to a running plugin. The dispatcher uses this to decide whether
    /// a name it does not know is a plugin tool.
    pub fn is_running(&self, namespace: &str) -> bool {
        self.inner
            .state()
            .plugins
            .get(namespace)
            .is_some_and(Entry::is_running)
    }

    /// Read one plugin directory's manifest, starting nothing and writing nothing.
    ///
    /// This is the add sheet's check-on-blur: the operator learns the path is wrong (or the namespace
    /// is taken later, at add) before the trust sentence is agreed to. The two failure modes the
    /// caller can act on are split from "invalid manifest" so the sentence can name the missing file.
    pub async fn inspect(&self, source: &Path) -> Result<PluginInspection, PluginError> {
        let given = source.display().to_string();
        let canonical = fs::canonicalize(source).map_err(|_| PluginError::InspectNotFound {
            path: given.clone(),
        })?;
        if !canonical.is_dir() {
            return Err(PluginError::InspectNotFound { path: given });
        }
        let manifest_path = canonical.join(MANIFEST_FILE);
        let text = match fs::read_to_string(&manifest_path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(PluginError::InspectNoManifest {
                    path: canonical.display().to_string(),
                });
            }
            Err(error) => {
                return Err(PluginError::InspectInvalidManifest {
                    path: canonical.display().to_string(),
                    error: ManifestError::Malformed {
                        detail: error.to_string(),
                    },
                });
            }
        };
        let manifest = PluginManifest::parse(&text)
            .and_then(|manifest| manifest.check().map(|()| manifest))
            .map_err(|error| PluginError::InspectInvalidManifest {
                path: canonical.display().to_string(),
                error,
            })?;
        Ok(PluginInspection {
            id: manifest.id,
            namespace: manifest.namespace,
            name: manifest.name,
            version: manifest.version,
            description: manifest.description,
        })
    }

    /// Install the plugin directory at `source`: validate, start, check for collisions, persist.
    ///
    /// The record is written only after the handshake and the collision check succeed, so a manifest
    /// whose process never answers is not recorded and does not fail the same way on every boot.
    /// A plugin that speaks the protocol but fails its handshake comes back as a `failed` status
    /// (with `last_error`) and no record; a namespace or tool collision is refused with an error.
    pub async fn add(&self, source: &Path) -> Result<(PluginStatus, usize), PluginError> {
        let source_text = source.display().to_string();
        let canonical = fs::canonicalize(source).map_err(|error| PluginError::InvalidPath {
            path: source_text.clone(),
            detail: error.to_string(),
        })?;
        if !canonical.is_dir() {
            return Err(PluginError::InvalidPath {
                path: source_text,
                detail: "it is not a directory".to_owned(),
            });
        }
        let manifest_path = canonical.join(MANIFEST_FILE);
        let text = fs::read_to_string(&manifest_path).map_err(|error| PluginError::Manifest {
            path: manifest_path.display().to_string(),
            detail: error.to_string(),
        })?;
        let manifest = PluginManifest::parse(&text)
            .and_then(|manifest| manifest.check().map(|()| manifest))
            .map_err(|error| PluginError::Manifest {
                path: manifest_path.display().to_string(),
                detail: error.to_string(),
            })?;
        if summrise_core::registry::CAPABILITIES
            .iter()
            .any(|capability| capability.name == manifest.namespace)
        {
            return Err(PluginError::ReservedNamespace {
                namespace: manifest.namespace.clone(),
            });
        }
        resolve_executable(&canonical, &manifest.command[0]).map_err(|()| {
            PluginError::CommandMissing {
                command: manifest.command[0].clone(),
            }
        })?;

        let namespace = manifest.namespace.clone();
        let record = PluginRecord {
            manifest,
            path: canonical.display().to_string(),
            installed_at_ms: now_ms(),
        };
        let log = LogSink::to_file(log_path(&self.inner.data_dir, &namespace));
        {
            let mut state = self.inner.state();
            if let Some(existing) = state.plugins.get(&namespace) {
                return Err(PluginError::Duplicate {
                    field: "namespace",
                    value: existing.record.manifest.namespace.clone(),
                });
            }
            if let Some(existing) = state
                .plugins
                .values()
                .find(|entry| entry.record.manifest.id == record.manifest.id)
            {
                return Err(PluginError::Duplicate {
                    field: "id",
                    value: existing.record.manifest.id.clone(),
                });
            }
            log.line(format!(
                "summrise: adding plugin {namespace} from {}",
                record.path
            ));
            state
                .plugins
                .insert(namespace.clone(), Entry::new(record, log));
        }

        let result = self.inner.attempt_start(&namespace, false).await;
        let status = match result {
            Ok(status) => status,
            Err(error) => {
                self.inner.discard(&namespace).await;
                return Err(error);
            }
        };
        if status.state != PluginState::Running {
            self.inner.discard(&namespace).await;
            return Ok((status, 0));
        }

        // Persist after the handshake: the record promises a plugin that can be started.
        if let Err(error) = self.inner.persist() {
            self.inner.discard(&namespace).await;
            return Err(PluginError::Io {
                namespace: status.namespace.clone(),
                detail: format!("the plugin registry could not be written: {error}"),
            });
        }
        if let Some(entry) = self.inner.state().plugins.get(&namespace) {
            entry.log.line(format!(
                "summrise: plugin {namespace} installed; its source stays at {}",
                status.path
            ));
        }
        Ok((status.clone(), status.tools))
    }

    /// Forget the record and stop the process. The source directory is never touched (v1 registers a
    /// path; removal forgets it).
    pub async fn remove(&self, namespace: &str) -> Result<PluginStatus, PluginError> {
        let (live, status, log) = {
            let mut state = self.inner.state();
            if !state.plugins.contains_key(namespace) {
                return Err(PluginError::NotFound {
                    namespace: namespace.to_owned(),
                });
            }
            let others: Vec<PluginRecord> = state
                .plugins
                .values()
                .filter(|entry| entry.record.manifest.namespace != namespace)
                .map(|entry| entry.record.clone())
                .collect();
            persist(&self.inner.data_dir, &others).map_err(|error| PluginError::Io {
                namespace: namespace.to_owned(),
                detail: format!("the plugin registry could not be written: {error}"),
            })?;
            let mut entry = state
                .plugins
                .remove(namespace)
                .expect("the entry was just found under the same lock");
            let live = entry.live.take();
            entry.phase = Phase::Stopped;
            entry.tools.clear();
            let log = entry.log.clone();
            (live, status_of(&entry), log)
        };
        if let Some(live) = live {
            stop_live(live, self.inner.options.stop_grace).await;
        }
        log.line(format!(
            "summrise: plugin {namespace} removed from the device; its files were not touched"
        ));
        Ok(status)
    }

    /// Stop and start one plugin now, resetting the start budget. A manual restart is the operator's
    /// answer to "failed until restarted".
    pub async fn restart(&self, namespace: &str) -> Result<PluginStatus, PluginError> {
        let (live, log) = {
            let mut state = self.inner.state();
            let Some(entry) = state.plugins.get_mut(namespace) else {
                return Err(PluginError::NotFound {
                    namespace: namespace.to_owned(),
                });
            };
            let live = entry.live.take();
            entry.phase = Phase::Stopped;
            entry.tools.clear();
            entry.starts.clear();
            entry.failures = 0;
            entry.last_error = None;
            (live, entry.log.clone())
        };
        if let Some(live) = live {
            stop_live(live, self.inner.options.stop_grace).await;
        }
        log.line(format!("summrise: plugin {namespace} restart asked for"));
        self.inner.attempt_start(namespace, true).await
    }

    /// The last `lines` lines of a plugin's log.
    pub async fn logs(&self, namespace: &str, lines: usize) -> Result<Vec<String>, PluginError> {
        let path = {
            let state = self.inner.state();
            if !state.plugins.contains_key(namespace) {
                return Err(PluginError::NotFound {
                    namespace: namespace.to_owned(),
                });
            }
            log_path(&self.inner.data_dir, namespace)
        };
        tokio::task::spawn_blocking(move || logs::tail(&path, lines))
            .await
            .map_err(|error| PluginError::Io {
                namespace: namespace.to_owned(),
                detail: format!("the log read failed: {error}"),
            })?
            .map_err(|error| PluginError::Io {
                namespace: namespace.to_owned(),
                detail: error.to_string(),
            })
    }

    /// Call one namespaced plugin tool.
    ///
    /// One call per plugin at a time (the call lock): a plugin is a small MCP server, and two
    /// concurrent calls would interleave its answers with no protocol-level reason. A slow call is
    /// bounded by [`Options::call_timeout`] and does **not** kill the plugin; only a death or three
    /// missed pings does.
    pub async fn call(&self, tool: &str, arguments: Value) -> Result<String, PluginError> {
        let Some((namespace, local)) = tool.split_once('.') else {
            return Err(PluginError::ToolNotFound {
                tool: tool.to_owned(),
            });
        };
        let (session, call_lock) = {
            let state = self.inner.state();
            let Some(entry) = state.plugins.get(namespace) else {
                return Err(PluginError::NotFound {
                    namespace: namespace.to_owned(),
                });
            };
            if !entry.is_running() {
                return Err(PluginError::NotRunning {
                    namespace: namespace.to_owned(),
                    state: state_of(&entry.phase).as_str(),
                });
            }
            let Some(live) = entry.live.as_ref() else {
                return Err(PluginError::NotRunning {
                    namespace: namespace.to_owned(),
                    state: state_of(&entry.phase).as_str(),
                });
            };
            if !entry
                .tools
                .iter()
                .any(|candidate| advertised_name(entry, candidate).as_deref() == Some(tool))
            {
                return Err(PluginError::ToolNotFound {
                    tool: tool.to_owned(),
                });
            }
            (Arc::clone(&live.session), Arc::clone(&entry.call_lock))
        };

        let _one_at_a_time = call_lock.lock().await;
        match session
            // The plugin knows its own tool names, not the device's namespace: send `greet`, not
            // `hello.greet`. The namespaced name is what the device advertises and audits.
            .call_tool(local, arguments, self.inner.options.call_timeout)
            .await
        {
            Ok(text) => Ok(text),
            Err(error) => {
                let error = PluginError::from_session(namespace, error);
                // A death is a lifecycle fact, not just a failed call: schedule the restart now
                // rather than waiting for the next tick to notice the closed stream.
                if matches!(error, PluginError::Exited { .. } | PluginError::Io { .. }) {
                    self.inner
                        .fail_restart(namespace, "exited while a call was in flight")
                        .await;
                }
                Err(error)
            }
        }
    }

    /// Stop every plugin and end supervision. The device calls this on shutdown.
    pub async fn stop_all(&self) {
        self.inner.stopping.store(true, Ordering::Release);
        self.inner.wake.notify_waiters();
        if let Some(handle) = self
            .inner
            .supervisor
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            handle.abort();
        }
        let lives: Vec<Live> = {
            let mut state = self.inner.state();
            state
                .plugins
                .values_mut()
                .filter_map(|entry| {
                    entry.phase = Phase::Stopped;
                    entry.tools.clear();
                    entry.live.take()
                })
                .collect()
        };
        let grace = self.inner.options.stop_grace;
        let mut stopping = tokio::task::JoinSet::new();
        for live in lives {
            stopping.spawn(async move {
                stop_live(live, grace).await;
            });
        }
        while stopping.join_next().await.is_some() {}
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        // The supervisor holds a `Weak`; ending it here means the last `Arc<Inner>` can drop, and the
        // live children (kill_on_drop) with it. `stop_all` is the orderly version.
        self.inner.stopping.store(true, Ordering::Release);
        self.inner.wake.notify_waiters();
        if let Some(handle) = self
            .inner
            .supervisor
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            handle.abort();
        }
    }
}

/// Everything the host shares with its supervisor tasks.
struct Inner {
    data_dir: PathBuf,
    options: Options,
    launcher: Arc<dyn Launcher>,
    state: Mutex<HostState>,
    wake: Arc<tokio::sync::Notify>,
    stopping: Arc<AtomicBool>,
    supervisor: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, HostState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Spawn the one supervisor task, if it is not running.
    fn ensure_supervisor(self: &Arc<Self>) {
        let mut slot = self
            .supervisor
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.is_some() {
            return;
        }
        let weak = Arc::downgrade(self);
        let wake = Arc::clone(&self.wake);
        let stopping = Arc::clone(&self.stopping);
        let tick = self.options.tick;
        *slot = Some(tokio::spawn(async move {
            loop {
                if stopping.load(Ordering::Acquire) {
                    return;
                }
                tokio::select! {
                    _ = wake.notified() => {}
                    _ = tokio::time::sleep(tick) => {}
                }
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if stopping.load(Ordering::Acquire) {
                    return;
                }
                inner.tick().await;
                // Do not hold the host across the wait: dropping it is what ends the loop.
                drop(inner);
            }
        }));
    }

    /// One supervision pass: notice a dead session, ping what is due, start what is due.
    async fn tick(self: &Arc<Self>) {
        if self.stopping.load(Ordering::Acquire) {
            return;
        }
        let now = Instant::now();
        let mut crashed = Vec::new();
        let mut pings = Vec::new();
        let mut starts = Vec::new();
        {
            let mut state = self.state();
            for (namespace, entry) in state.plugins.iter_mut() {
                let action = match &entry.phase {
                    Phase::Running { next_ping } => {
                        let alive = entry
                            .live
                            .as_ref()
                            .is_some_and(|live| !live.session.is_closed());
                        if !alive {
                            Some(TickAction::Crashed)
                        } else if now >= *next_ping {
                            let session = entry
                                .live
                                .as_ref()
                                .map(|live| Arc::clone(&live.session))
                                .expect("alive implies a live session");
                            Some(TickAction::Ping(session))
                        } else {
                            None
                        }
                    }
                    // A start attempt that never finished (the task died) is a failure, not a state
                    // to sit in until the process restarts.
                    Phase::Starting { since }
                        if now.saturating_duration_since(*since)
                            > self.options.startup_timeout + self.options.startup_timeout =>
                    {
                        Some(TickAction::Crashed)
                    }
                    Phase::Failed {
                        retry_at: Some(at),
                        permanent: false,
                    } if now >= *at => {
                        starts.push(namespace.clone());
                        None
                    }
                    Phase::Stopped => {
                        starts.push(namespace.clone());
                        None
                    }
                    _ => None,
                };
                match action {
                    Some(TickAction::Crashed) => crashed.push(namespace.clone()),
                    Some(TickAction::Ping(session)) => {
                        entry.phase = Phase::Running {
                            next_ping: now + self.options.ping_interval,
                        };
                        pings.push((namespace.clone(), session));
                    }
                    None => {}
                }
            }
        }

        for namespace in crashed {
            self.fail_restart(&namespace, "the plugin process exited")
                .await;
        }
        for (namespace, session) in pings {
            let inner = Arc::clone(self);
            tokio::spawn(async move {
                let result = session.ping(inner.options.ping_timeout).await;
                inner.ping_result(&namespace, result).await;
            });
        }
        for namespace in starts {
            let inner = Arc::clone(self);
            tokio::spawn(async move {
                let _ = inner.attempt_start(&namespace, false).await;
            });
        }
    }

    /// Apply one ping's outcome: a success resets the miss count; three misses restart the plugin.
    async fn ping_result(self: &Arc<Self>, namespace: &str, result: Result<(), SessionError>) {
        match result {
            Ok(()) => {
                let mut state = self.state();
                if let Some(entry) = state.plugins.get_mut(namespace) {
                    if matches!(entry.phase, Phase::Running { .. }) {
                        entry.phase = Phase::Running {
                            next_ping: Instant::now() + self.options.ping_interval,
                        };
                        entry.misses = 0;
                    }
                }
            }
            Err(error) => {
                let restart = {
                    let mut state = self.state();
                    let Some(entry) = state.plugins.get_mut(namespace) else {
                        return;
                    };
                    if !matches!(entry.phase, Phase::Running { .. }) {
                        return;
                    }
                    entry.misses = entry.misses.saturating_add(1);
                    let misses = entry.misses;
                    entry
                        .log
                        .line(format!("summrise: ping {misses}/3 failed: {error}"));
                    misses >= 3
                };
                if restart {
                    self.fail_restart(namespace, "stopped answering pings")
                        .await;
                }
            }
        }
    }

    /// One start attempt: spawn, handshake, catalogue, collision check, then Running.
    ///
    /// `manual` is the operator's restart: it ignores the start budget and the pending backoff, so a
    /// plugin that failed out stays startable. Failures after the spawn return `Ok(failed status)`
    /// (the caller can show the sentence); a collision is `Err`, because there is nothing the plugin
    /// can be allowed to advertise.
    async fn attempt_start(
        self: &Arc<Self>,
        namespace: &str,
        manual: bool,
    ) -> Result<PluginStatus, PluginError> {
        let now = Instant::now();
        let (record, log, previous) = {
            let mut state = self.state();
            let Some(entry) = state.plugins.get_mut(namespace) else {
                return Err(PluginError::NotFound {
                    namespace: namespace.to_owned(),
                });
            };
            if self.stopping.load(Ordering::Acquire) {
                return Ok(status_of(entry));
            }
            if manual {
                entry.starts.clear();
                entry.failures = 0;
                entry.last_error = None;
            }
            match &entry.phase {
                Phase::Running { .. } | Phase::Starting { .. } => return Ok(status_of(entry)),
                Phase::Failed {
                    retry_at,
                    permanent,
                } => {
                    if !manual && (*permanent || retry_at.is_some_and(|retry_at| now < retry_at)) {
                        return Ok(status_of(entry));
                    }
                }
                Phase::Stopped => {}
            }
            entry.starts.retain(|started| {
                now.saturating_duration_since(*started) <= self.options.start_window
            });
            if entry.starts.len() >= self.options.max_starts {
                entry.phase = Phase::Failed {
                    retry_at: None,
                    permanent: true,
                };
                entry.last_error = Some(format!(
                    "failed {} times in {} s; fix the failure, then restart it",
                    entry.starts.len(),
                    self.options.start_window.as_secs()
                ));
                entry.log.line(format!(
                    "summrise: plugin {namespace} failed too often; not restarting until a manual restart"
                ));
                return Ok(status_of(entry));
            }
            entry.starts.push_back(now);
            entry.phase = Phase::Starting { since: now };
            entry.tools.clear();
            entry.last_error = None;
            entry.misses = 0;
            let previous = entry.live.take();
            (entry.record.clone(), entry.log.clone(), previous)
        };
        if let Some(previous) = previous {
            stop_live(previous, self.options.stop_grace).await;
        }

        let spec = LaunchSpec {
            command: record.manifest.command.clone(),
            cwd: resolve_cwd(&record.path, record.manifest.cwd.as_deref()),
            env: record.manifest.env.clone(),
            log: log.clone(),
        };
        let launched = match self.launcher.launch(&spec) {
            Ok(launched) => launched,
            Err(detail) => {
                self.record_failure(
                    namespace,
                    PluginError::Spawn {
                        namespace: namespace.to_owned(),
                        detail,
                    },
                )
                .await;
                return self.status_or_not_found(namespace);
            }
        };
        let session = Arc::new(launched.session);

        let version = match session.initialize(self.options.startup_timeout).await {
            Ok(version) => version,
            Err(error) => {
                launched.child.kill();
                self.record_failure(
                    namespace,
                    PluginError::Failed {
                        namespace: namespace.to_owned(),
                        detail: format!("the handshake failed: {error}"),
                    },
                )
                .await;
                return self.status_or_not_found(namespace);
            }
        };
        log.line(format!(
            "summrise: plugin {namespace} negotiated MCP protocol {version}"
        ));
        let tools = match session.list_tools(self.options.startup_timeout).await {
            Ok(tools) => tools,
            Err(error) => {
                launched.child.kill();
                self.record_failure(
                    namespace,
                    PluginError::Failed {
                        namespace: namespace.to_owned(),
                        detail: format!("tools/list failed: {error}"),
                    },
                )
                .await;
                return self.status_or_not_found(namespace);
            }
        };

        // Namespaced names are the device's contract: an illegal one fails the plugin with the
        // offending name, and a duplicate inside one plugin is the same kind of malformed catalogue.
        let mut names = BTreeSet::new();
        for tool in &tools {
            let full = match tool_name(namespace, &tool.name) {
                Ok(full) => full,
                Err(error) => {
                    launched.child.kill();
                    self.record_failure(
                        namespace,
                        PluginError::InvalidToolName {
                            namespace: namespace.to_owned(),
                            tool: tool.name.clone(),
                            detail: error.to_string(),
                        },
                    )
                    .await;
                    return self.status_or_not_found(namespace);
                }
            };
            if !names.insert(full.clone()) {
                launched.child.kill();
                self.record_failure(
                    namespace,
                    PluginError::InvalidToolName {
                        namespace: namespace.to_owned(),
                        tool: tool.name.clone(),
                        detail: format!("the tool {full} is advertised twice"),
                    },
                )
                .await;
                return self.status_or_not_found(namespace);
            }
        }

        // A built-in name can only collide when the namespace itself does; the reserved-namespace
        // rule catches that at add time. This is the second lock on the same door, because a
        // hand-edited registry must fail rather than shadow `files.read`.
        if let Some(owner) = names.iter().find_map(|name| {
            summrise_core::registry::tool(name).map(|_| "the built-in capabilities")
        }) {
            launched.child.kill();
            let tool = names
                .iter()
                .find(|name| summrise_core::registry::tool(name).is_some())
                .cloned()
                .unwrap_or_default();
            let error = PluginError::ToolCollision {
                tool,
                owner: owner.to_owned(),
            };
            self.record_failure(namespace, error.clone()).await;
            return Err(error);
        }

        let tools: Vec<PluginTool> = {
            let mut tools = tools;
            tools.sort_by(|left, right| left.name.cmp(&right.name));
            tools
        };

        // Commit under one borrow, then await nothing while the guard is alive: a `std::sync` guard
        // held across an await would make every `tokio::spawn` of `attempt_start` non-Send.
        let mut post = PostStart::Running;
        let mut live_session = None;
        {
            let mut state = self.state();
            if !state.plugins.contains_key(namespace) {
                post = PostStart::Gone;
            } else {
                // Another plugin's record may have started while this handshake ran. Namespaces are
                // unique, but a tool carrying the other namespace's prefix is not impossible (`a`
                // exposing `b.c` produces `a.b.c`), so check rather than assume.
                let collision = {
                    let mut taken: BTreeMap<String, String> = BTreeMap::new();
                    for (other_namespace, other) in state.plugins.iter() {
                        if other_namespace == namespace || !other.is_running() {
                            continue;
                        }
                        for candidate in &other.tools {
                            if let Some(full) = advertised_name(other, candidate) {
                                taken.insert(full, other_namespace.clone());
                            }
                        }
                    }
                    names
                        .iter()
                        .find_map(|full| taken.get(full).map(|owner| (full.clone(), owner.clone())))
                };
                if let Some((tool, other_namespace)) = collision {
                    post = PostStart::Collision(PluginError::ToolCollision {
                        tool,
                        owner: format!("plugin {other_namespace}"),
                    });
                } else {
                    let entry = state
                        .plugins
                        .get_mut(namespace)
                        .expect("the entry was checked under the same lock");
                    if self.stopping.load(Ordering::Acquire) {
                        entry.phase = Phase::Stopped;
                        post = PostStart::Stopped;
                    } else {
                        let pid = launched.pid;
                        live_session = Some(Arc::clone(&session));
                        entry.tools = tools;
                        entry.live = Some(Live {
                            session: Arc::clone(&session),
                            child: Arc::clone(&launched.child),
                            pid,
                        });
                        entry.phase = Phase::Running {
                            next_ping: Instant::now() + self.options.ping_interval,
                        };
                        entry.failures = 0;
                        entry.misses = 0;
                        entry.last_error = None;
                        entry.log.line(format!(
                            "summrise: plugin {namespace} started (pid {pid:?}) with {} tool(s)",
                            entry.tools.len()
                        ));
                    }
                }
            }
        }
        let _ = live_session;

        match post {
            PostStart::Running | PostStart::Stopped => {
                if matches!(post, PostStart::Stopped) {
                    launched.child.kill();
                }
                self.status_or_not_found(namespace)
            }
            PostStart::Collision(error) => {
                launched.child.kill();
                self.record_failure(namespace, error.clone()).await;
                Err(error)
            }
            PostStart::Gone => {
                launched.child.kill();
                Err(PluginError::NotFound {
                    namespace: namespace.to_owned(),
                })
            }
        }
    }

    /// Put a plugin into `Failed` with a backoff (or permanently, when the budget is spent).
    async fn record_failure(&self, namespace: &str, error: PluginError) {
        let live = {
            let mut state = self.state();
            let Some(entry) = state.plugins.get_mut(namespace) else {
                return;
            };
            let live = entry.live.take();
            entry.tools.clear();
            entry.failures = entry.failures.saturating_add(1);
            entry.last_error = Some(error.to_string());
            entry
                .log
                .line(format!("summrise: plugin {namespace} failed: {error}"));

            let now = Instant::now();
            entry.starts.retain(|started| {
                now.saturating_duration_since(*started) <= self.options.start_window
            });
            let permanent = entry.starts.len() >= self.options.max_starts;
            let delay = self.backoff(entry.failures);
            entry.phase = Phase::Failed {
                retry_at: if permanent { None } else { Some(now + delay) },
                permanent,
            };
            entry.log.line(if permanent {
                format!(
                    "summrise: plugin {namespace} failed {} times in {} s; not restarting until a manual restart",
                    entry.starts.len(),
                    self.options.start_window.as_secs()
                )
            } else {
                format!(
                    "summrise: plugin {namespace} restarting in {} ms",
                    delay.as_millis()
                )
            });
            live
        };
        if let Some(live) = live {
            stop_live(live, Duration::ZERO).await;
        }
    }

    /// A running plugin died or stopped answering: take it out of `Running` now, schedule the retry.
    async fn fail_restart(&self, namespace: &str, reason: &str) {
        let live = {
            let mut state = self.state();
            let Some(entry) = state.plugins.get_mut(namespace) else {
                return;
            };
            if !matches!(entry.phase, Phase::Running { .. }) {
                return;
            }
            let live = entry.live.take();
            entry.tools.clear();
            entry.failures = entry.failures.saturating_add(1);
            entry.misses = 0;
            entry.last_error = Some(reason.to_owned());
            entry
                .log
                .line(format!("summrise: plugin {namespace} {reason}"));

            let now = Instant::now();
            entry.starts.retain(|started| {
                now.saturating_duration_since(*started) <= self.options.start_window
            });
            let permanent = entry.starts.len() >= self.options.max_starts;
            let delay = self.backoff(entry.failures);
            entry.phase = Phase::Failed {
                retry_at: if permanent { None } else { Some(now + delay) },
                permanent,
            };
            entry.log.line(if permanent {
                format!(
                    "summrise: plugin {namespace} failed {} times in {} s; not restarting until a manual restart",
                    entry.starts.len(),
                    self.options.start_window.as_secs()
                )
            } else {
                format!(
                    "summrise: plugin {namespace} restarting in {} ms",
                    delay.as_millis()
                )
            });
            live
        };
        if let Some(live) = live {
            stop_live(live, Duration::ZERO).await;
        }
    }

    /// The backoff for the `failures`-th consecutive failure: base, 2x, 4x... capped.
    fn backoff(&self, failures: u32) -> Duration {
        let exponent = failures.saturating_sub(1).min(16);
        self.options
            .retry_base
            .saturating_mul(1u32 << exponent)
            .min(self.options.retry_cap)
    }

    fn status_or_not_found(&self, namespace: &str) -> Result<PluginStatus, PluginError> {
        self.state()
            .plugins
            .get(namespace)
            .map(status_of)
            .ok_or_else(|| PluginError::NotFound {
                namespace: namespace.to_owned(),
            })
    }

    /// Remove a record the caller has decided not to keep, stopping anything it started.
    async fn discard(&self, namespace: &str) {
        let live = {
            let mut state = self.state();
            state.plugins.remove(namespace).and_then(|mut entry| {
                entry.tools.clear();
                entry.live.take()
            })
        };
        if let Some(live) = live {
            stop_live(live, self.options.stop_grace).await;
        }
    }

    /// Write the current records to `<data>/plugins.json`, sorted by namespace.
    fn persist(&self) -> io::Result<()> {
        let records: Vec<PluginRecord> = self
            .state()
            .plugins
            .values()
            .map(|entry| entry.record.clone())
            .collect();
        persist(&self.data_dir, &records)
    }
}

/// How one handshake ended, once the state lock is no longer held.
enum PostStart {
    Running,
    Stopped,
    Collision(PluginError),
    Gone,
}

/// What the supervisor decided to do with one entry this tick.
enum TickAction {
    /// The session is closed; fail and schedule a restart.
    Crashed,
    /// The ping is due.
    Ping(Arc<Session>),
}

/// The registry's in-memory state.
struct HostState {
    plugins: BTreeMap<String, Entry>,
}

/// One installed plugin.
struct Entry {
    record: PluginRecord,
    phase: Phase,
    tools: Vec<PluginTool>,
    live: Option<Live>,
    last_error: Option<String>,
    /// The start attempts inside the window, oldest first.
    starts: VecDeque<Instant>,
    /// Consecutive failures, driving the backoff.
    failures: u32,
    /// Consecutive missed pings.
    misses: u32,
    /// One call at a time per plugin.
    call_lock: Arc<tokio::sync::Mutex<()>>,
    log: LogSink,
}

impl Entry {
    fn new(record: PluginRecord, log: LogSink) -> Entry {
        Entry {
            record,
            phase: Phase::Stopped,
            tools: Vec::new(),
            live: None,
            last_error: None,
            starts: VecDeque::new(),
            failures: 0,
            misses: 0,
            call_lock: Arc::new(tokio::sync::Mutex::new(())),
            log,
        }
    }

    fn is_running(&self) -> bool {
        matches!(self.phase, Phase::Running { .. }) && self.live.is_some()
    }
}

/// The state machine's phase.
enum Phase {
    Stopped,
    Starting {
        since: Instant,
    },
    Running {
        next_ping: Instant,
    },
    Failed {
        retry_at: Option<Instant>,
        permanent: bool,
    },
}

/// The live half of a running plugin.
struct Live {
    session: Arc<Session>,
    child: Arc<dyn ChildProcess>,
    #[allow(dead_code)]
    pid: Option<u32>,
}

/// One registry row: the manifest as installed, where it lives, and when it was installed.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct PluginRecord {
    manifest: PluginManifest,
    path: String,
    installed_at_ms: u64,
}

/// `<data>/plugins.json`.
#[derive(Debug, Serialize, Deserialize)]
struct RegistryFile {
    version: u32,
    plugins: Vec<PluginRecord>,
}

fn status_of(entry: &Entry) -> PluginStatus {
    let phase = &entry.phase;
    PluginStatus {
        id: entry.record.manifest.id.clone(),
        namespace: entry.record.manifest.namespace.clone(),
        name: entry.record.manifest.name.clone(),
        version: entry.record.manifest.version.clone(),
        description: entry.record.manifest.description.clone(),
        state: state_of(phase),
        tools: if matches!(phase, Phase::Running { .. }) {
            entry.tools.len()
        } else {
            0
        },
        path: entry.record.path.clone(),
        last_error: entry.last_error.clone(),
    }
}

fn state_of(phase: &Phase) -> PluginState {
    match phase {
        Phase::Stopped => PluginState::Stopped,
        Phase::Starting { .. } => PluginState::Starting,
        Phase::Running { .. } => PluginState::Running,
        Phase::Failed { .. } => PluginState::Failed,
    }
}

fn view_of(entry: &Entry, tool: &PluginTool) -> PluginToolView {
    PluginToolView {
        name: advertised_name(entry, tool).unwrap_or_else(|| {
            // Every stored tool passed `tool_name` before it was stored; this branch exists so the
            // catalogue can never fail to build, and would be a bug rather than a user error.
            format!("{}.{}", entry.record.manifest.namespace, tool.name)
        }),
        title: tool.title.clone(),
        description: tool.description.clone(),
        input_schema: tool.input_schema.clone(),
        read_only: tool.read_only,
        destructive_hint: tool.destructive,
        destructive: tool.destructive(),
        idempotent: tool.idempotent,
        open_world: tool.open_world,
    }
}

/// The advertised name of one stored tool, or `None` if the stored name is somehow illegal.
fn advertised_name(entry: &Entry, tool: &PluginTool) -> Option<String> {
    tool_name(&entry.record.manifest.namespace, &tool.name).ok()
}

/// Close stdin, wait the grace, then kill whatever is left.
async fn stop_live(live: Live, grace: Duration) {
    live.session.shutdown(grace).await;
    live.child.kill();
}

/// Where a namespace's log lives: `<data>/logs/plugin-<namespace>.log`.
fn log_path(data_dir: &Path, namespace: &str) -> PathBuf {
    data_dir
        .join("logs")
        .join(format!("plugin-{namespace}.log"))
}

/// Where the registry lives: `<data>/plugins.json`.
fn registry_path(data_dir: &Path) -> PathBuf {
    data_dir.join(REGISTRY_FILE)
}

/// Read and validate the registry. Missing is normal; a bad file is a log line and an empty host.
fn load_registry(data_dir: &Path) -> (Vec<PluginRecord>, Option<String>) {
    let path = registry_path(data_dir);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return (
                Vec::new(),
                Some("no plugins registered; plugins.json is absent".to_owned()),
            );
        }
        Err(error) => {
            return (
                Vec::new(),
                Some(format!(
                    "the plugin registry {} is unreadable: {error}; no plugins were loaded",
                    path.display()
                )),
            );
        }
    };
    let file: RegistryFile = match serde_json::from_str(&text) {
        Ok(file) => file,
        Err(error) => {
            return (
                Vec::new(),
                Some(format!(
                    "the plugin registry {} is not valid JSON: {error}; no plugins were loaded",
                    path.display()
                )),
            );
        }
    };
    if file.version != REGISTRY_VERSION {
        return (
            Vec::new(),
            Some(format!(
                "the plugin registry {} is version {}; this build reads version {}; no plugins were loaded",
                path.display(),
                file.version,
                REGISTRY_VERSION
            )),
        );
    }

    let mut records = Vec::new();
    let mut namespaces = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut note = None;
    for record in file.plugins {
        if let Err(error) = record.manifest.check() {
            note = Some(format!(
                "a recorded plugin was refused on load: {error}; it was skipped"
            ));
            continue;
        }
        if !namespaces.insert(record.manifest.namespace.clone())
            || !ids.insert(record.manifest.id.clone())
        {
            note = Some(format!(
                "the plugin registry names {} twice; the duplicate was skipped",
                record.manifest.namespace
            ));
            continue;
        }
        records.push(record);
    }
    (records, note)
}

/// Write the registry atomically: a random sibling, fsync, rename. Sorted by namespace.
fn persist(data_dir: &Path, records: &[PluginRecord]) -> io::Result<()> {
    let mut sorted: Vec<PluginRecord> = records.to_vec();
    sorted.sort_by(|left, right| {
        left.manifest
            .namespace
            .cmp(&right.manifest.namespace)
            .then_with(|| left.manifest.id.cmp(&right.manifest.id))
    });
    let file = RegistryFile {
        version: REGISTRY_VERSION,
        plugins: sorted,
    };
    let text = serde_json::to_string_pretty(&file).map_err(io::Error::other)?;
    let path = registry_path(data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let (temporary, mut handle) = crate::fsutil::create_new_sibling(&path)?;
    let write = (|| -> io::Result<()> {
        handle.write_all(text.as_bytes())?;
        handle.write_all(b"\n")?;
        handle.sync_all()
    })();
    if let Err(error) = write {
        drop(handle);
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    drop(handle);
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

/// The working directory a launch uses: the plugin's own directory unless the manifest says
/// otherwise (validation has already proven the value is relative and cannot climb out).
fn resolve_cwd(plugin_dir: &str, cwd: Option<&str>) -> PathBuf {
    let root = Path::new(plugin_dir);
    match cwd {
        Some(cwd) if !cwd.is_empty() => root.join(cwd),
        _ => root.to_path_buf(),
    }
}

/// Whether `command` names something runnable: absolute, relative to the plugin directory, or on
/// `PATH`. The launcher still lets the operating system resolve it; this is the install-time check
/// that refuses a typo before a record exists.
fn resolve_executable(plugin_dir: &Path, command: &str) -> Result<(), ()> {
    let candidate = Path::new(command);
    if candidate.is_absolute() {
        return if candidate.is_file() { Ok(()) } else { Err(()) };
    }
    if command.contains('/') || command.contains('\\') {
        return if plugin_dir.join(candidate).is_file() {
            Ok(())
        } else {
            Err(())
        };
    }
    let Some(path) = std::env::var_os("PATH") else {
        return Err(());
    };
    for directory in std::env::split_paths(&path) {
        if directory.join(candidate).is_file() {
            return Ok(());
        }
        #[cfg(windows)]
        for extension in windows_extensions() {
            if directory.join(format!("{command}{extension}")).is_file() {
                return Ok(());
            }
        }
    }
    Err(())
}

#[cfg(windows)]
fn windows_extensions() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned())
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use serde_json::Value;

    use super::*;
    use crate::plugins::test_support::{
        host, options as test_options, plugin_dir, wait_until, ScriptedLauncher,
    };
    use crate::test_support::TempDir;

    #[test]
    fn a_missing_directory_asks_for_the_path_not_another_plugins_add() {
        let error = PluginError::InspectNotFound {
            path: "C:\\no-such-plugin".to_owned(),
        };
        assert_eq!(error.code(), "NOT_FOUND");
        assert!(
            error.action().contains("check the directory path"),
            "the next action names the fix: {}",
            error.action()
        );
        let text: Value = serde_json::from_str(&error.to_text()).expect("JSON");
        assert!(text["message"]
            .as_str()
            .unwrap()
            .contains("no directory at"));
    }

    /// The longest a test waits for a state change. The supervisor's timings are milliseconds, so
    /// this is only a guard against a real hang, not a budget the machine is expected to use.
    const WAIT: Duration = Duration::from_secs(10);

    async fn is_state(host: &PluginHost, namespace: &str, wanted: PluginState) -> bool {
        host.status(namespace)
            .is_some_and(|status| status.state == wanted)
    }

    /// The registry row a plugin directory would have been given: its manifest, its canonical path.
    fn record_for(source: &Path) -> Value {
        let text =
            fs::read_to_string(source.join(MANIFEST_FILE)).expect("the manifest is readable");
        let manifest: Value = serde_json::from_str(&text).expect("the manifest is JSON");
        json!({
            "manifest": manifest,
            "path": source.display().to_string(),
            "installed_at_ms": 0
        })
    }

    /// Write the registry file by hand, before the host reads it: the hand-edited / older-build case.
    fn write_registry(dir: &TempDir, records: Vec<Value>) {
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data directory is creatable");
        fs::write(
            data.join("plugins.json"),
            json!({ "version": 1, "plugins": records }).to_string(),
        )
        .expect("the registry is written");
    }

    fn host_with(dir: &TempDir, launcher: Arc<ScriptedLauncher>, options: Options) -> PluginHost {
        PluginHost::with_launcher(&dir.join("data"), launcher, options)
    }

    #[tokio::test]
    async fn add_starts_the_plugin_advertises_its_tools_and_routes_a_call() {
        let dir = TempDir::new("plugin-add");
        let launcher = ScriptedLauncher::hello();
        let host = host(&dir, Arc::clone(&launcher));
        let source = plugin_dir(&dir, "hello");

        let (status, tools) = host.add(&source).await.expect("the plugin starts");
        assert_eq!(status.state, PluginState::Running);
        assert_eq!(status.namespace, "hello");
        assert_eq!(status.name, "hello plugin");
        assert_eq!(status.version, "0.1.0");
        assert_eq!(status.tools, 1);
        assert_eq!(tools, 1);
        assert_eq!(status.last_error, None);
        assert_eq!(
            status.path,
            fs::canonicalize(&source).unwrap().display().to_string()
        );

        let listed = host.list().await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].state, PluginState::Running);
        assert_eq!(listed[0].tools, 1);

        let views = host.tools();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].name, "hello.greet");
        assert_eq!(views[0].title.as_deref(), Some("Greet someone"));
        assert!(!views[0].destructive, "the plugin said it is read-only");
        assert_eq!(host.destructive("hello.greet"), Some(false));
        assert_eq!(host.destructive("hello.unknown"), None);
        assert_eq!(host.destructive("files.read"), None);
        assert!(host.is_running("hello"));
        assert!(!host.is_running("hello2"));

        // The plugin is sent its own tool name, not the namespaced one the device advertises.
        let text = host
            .call("hello.greet", json!({ "name": "Bob" }))
            .await
            .expect("the call reaches the scripted plugin");
        assert_eq!(text, "greet says hi Bob");
        assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);

        let registry = fs::read_to_string(dir.join("data").join("plugins.json"))
            .expect("the registry is written");
        let registry: Value = serde_json::from_str(&registry).expect("the registry is JSON");
        assert_eq!(registry["version"], 1);
        assert_eq!(registry["plugins"][0]["manifest"]["namespace"], "hello");
        assert_eq!(registry["plugins"][0]["path"], status.path);
    }

    #[tokio::test]
    async fn an_initialize_failure_returns_a_failed_status_and_writes_no_record() {
        let dir = TempDir::new("plugin-init-failure");
        let launcher = ScriptedLauncher::hello();
        launcher.fail_initialize.store(true, Ordering::SeqCst);
        let host = host(&dir, Arc::clone(&launcher));
        let source = plugin_dir(&dir, "hello");

        let (status, tools) = host
            .add(&source)
            .await
            .expect("a plugin that will not start is a status, not a failed install");
        assert_eq!(status.state, PluginState::Failed);
        assert_eq!(tools, 0);
        assert!(
            status
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("handshake")),
            "the sentence names the handshake: {:?}",
            status.last_error
        );
        assert!(
            host.list().await.is_empty(),
            "a failed start is not recorded"
        );
        assert!(
            !dir.join("data").join("plugins.json").exists(),
            "nothing is promised for the next boot"
        );
        assert!(
            launcher.kills.load(Ordering::SeqCst) >= 1,
            "the child that failed its handshake is not left running"
        );
    }

    #[tokio::test]
    async fn a_namespace_that_is_a_built_in_capability_is_refused_and_nothing_is_spawned() {
        let dir = TempDir::new("plugin-reserved-namespace");
        let launcher = ScriptedLauncher::hello();
        let host = host(&dir, Arc::clone(&launcher));
        let source = plugin_dir(&dir, "files");

        let error = host
            .add(&source)
            .await
            .expect_err("a plugin cannot take the files capability's namespace");
        assert!(
            matches!(&error, PluginError::ReservedNamespace { namespace } if namespace == "files"),
            "{error:?}"
        );
        assert_eq!(error.code(), "PLUGIN_NAMESPACE_RESERVED");
        assert!(error.to_string().contains("files"), "{error}");
        assert_eq!(
            launcher.launches.load(Ordering::SeqCst),
            0,
            "the reserved namespace is refused before a process is started"
        );
        assert!(host.list().await.is_empty());
    }

    #[tokio::test]
    async fn a_recorded_plugin_that_collides_with_a_built_in_fails_instead_of_starting() {
        let dir = TempDir::new("plugin-builtin-collision");
        let source = plugin_dir(&dir, "files");
        write_registry(&dir, vec![record_for(&source)]);
        let launcher = ScriptedLauncher::new(vec![json!({
            "name": "read",
            "inputSchema": { "type": "object" }
        })]);
        let host = host_with(&dir, Arc::clone(&launcher), test_options());

        host.start_all().await;

        assert!(
            wait_until(WAIT, || is_state(&host, "files", PluginState::Failed)).await,
            "a hand-edited record cannot shadow files.read: {:?}",
            host.status("files")
        );
        let status = host.status("files").expect("the record is still listed");
        assert!(
            status
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("built-in capabilities")),
            "{:?}",
            status.last_error
        );
        assert!(
            host.tools().is_empty(),
            "a failed plugin advertises nothing"
        );
        assert!(launcher.kills.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn a_dead_plugin_leaves_the_catalogue_restarts_and_comes_back() {
        let dir = TempDir::new("plugin-crash-restart");
        let launcher = ScriptedLauncher::hello();
        let host = host(&dir, Arc::clone(&launcher));
        let source = plugin_dir(&dir, "hello");
        host.add(&source).await.expect("the plugin starts");
        host.start_all().await;
        assert_eq!(host.tools().len(), 1);

        // The peer stops, exactly as a crashed child's stdout would close.
        launcher.die.notify_one();

        assert!(
            wait_until(WAIT, || async {
                !is_state(&host, "hello", PluginState::Running).await
                    || launcher.launches.load(Ordering::SeqCst) >= 2
            })
            .await,
            "the death is noticed: {:?}",
            host.status("hello")
        );
        assert!(
            wait_until(WAIT, || async {
                is_state(&host, "hello", PluginState::Running).await
                    && launcher.launches.load(Ordering::SeqCst) >= 2
                    && host.tools().len() == 1
            })
            .await,
            "the plugin comes back and its tools return: {:?}",
            host.status("hello")
        );

        let log = host
            .logs("hello", 200)
            .await
            .expect("the log is readable")
            .join("\n");
        assert!(log.contains("exited"), "the log has the death: {log}");
        assert!(log.contains("started"), "the log has the restart: {log}");
    }

    #[tokio::test]
    async fn three_missed_pings_restart_the_plugin() {
        let dir = TempDir::new("plugin-health");
        let launcher = ScriptedLauncher::hello();
        let host = host(&dir, Arc::clone(&launcher));
        let source = plugin_dir(&dir, "hello");
        host.add(&source).await.expect("the plugin starts");
        host.start_all().await;

        launcher.ignore_pings.store(true, Ordering::SeqCst);
        assert!(
            wait_until(WAIT, || async {
                launcher.launches.load(Ordering::SeqCst) >= 2
            })
            .await,
            "three missed pings restart the plugin: {:?}",
            host.status("hello")
        );
        launcher.ignore_pings.store(false, Ordering::SeqCst);
        assert!(
            wait_until(WAIT, || async {
                is_state(&host, "hello", PluginState::Running).await
            })
            .await,
            "the restarted plugin is healthy: {:?}",
            host.status("hello")
        );

        let log = host.logs("hello", 200).await.expect("the log is readable");
        let log = log.join("\n");
        assert!(log.contains("ping 1/3 failed"), "{log}");
        assert!(log.contains("stopped answering pings"), "{log}");
    }

    #[tokio::test]
    async fn an_in_flight_call_fails_with_exited_when_the_peer_dies() {
        let dir = TempDir::new("plugin-call-death");
        let launcher = ScriptedLauncher::hello();
        let host = host(&dir, Arc::clone(&launcher));
        let source = plugin_dir(&dir, "hello");
        host.add(&source).await.expect("the plugin starts");
        host.start_all().await;

        launcher.drop_on_call.store(true, Ordering::SeqCst);
        let started = std::time::Instant::now();
        let error = host
            .call("hello.greet", json!({ "name": "Bob" }))
            .await
            .expect_err("the peer closed instead of answering");
        assert!(
            matches!(&error, PluginError::Exited { namespace } if namespace == "hello"),
            "{error:?}"
        );
        assert_eq!(error.code(), "PLUGIN_EXITED");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a dead peer fails the call now, not at the 2 s deadline: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_slow_call_times_out_without_killing_the_plugin() {
        let dir = TempDir::new("plugin-call-timeout");
        let launcher = ScriptedLauncher::hello();
        let mut options = test_options();
        options.call_timeout = Duration::from_millis(100);
        let host = host_with(&dir, Arc::clone(&launcher), options);
        let source = plugin_dir(&dir, "hello");
        host.add(&source).await.expect("the plugin starts");

        launcher.ignore_calls.store(true, Ordering::SeqCst);
        let error = host
            .call("hello.greet", json!({ "name": "Bob" }))
            .await
            .expect_err("the plugin never answered");
        assert!(
            matches!(
                &error,
                PluginError::Timeout {
                    namespace,
                    method,
                    ..
                } if namespace == "hello" && method == "tools/call"
            ),
            "{error:?}"
        );
        assert_eq!(error.code(), "PLUGIN_TIMEOUT");
        assert!(
            error.to_string().contains("hello"),
            "the sentence names it: {error}"
        );
        assert_eq!(
            host.status("hello").map(|status| status.state),
            Some(PluginState::Running),
            "a slow call does not take the plugin down"
        );
        assert_eq!(
            launcher.kills.load(Ordering::SeqCst),
            0,
            "a slow call does not kill the plugin"
        );

        launcher.ignore_calls.store(false, Ordering::SeqCst);
        let text = host
            .call("hello.greet", json!({ "name": "Bob" }))
            .await
            .expect("the next call answers");
        assert_eq!(text, "greet says hi Bob");
    }

    #[tokio::test]
    async fn remove_forgets_the_record_stops_the_process_and_never_deletes_the_source() {
        let dir = TempDir::new("plugin-remove");
        let launcher = ScriptedLauncher::hello();
        let host = host(&dir, Arc::clone(&launcher));
        let source = plugin_dir(&dir, "hello");
        host.add(&source).await.expect("the plugin starts");

        let status = host.remove("hello").await.expect("the plugin is removed");
        assert_eq!(status.state, PluginState::Stopped);
        assert!(host.list().await.is_empty());
        assert!(host.tools().is_empty());
        assert!(
            source.join(MANIFEST_FILE).is_file(),
            "removing forgets the record; the operator's files stay"
        );
        assert!(launcher.kills.load(Ordering::SeqCst) >= 1);

        let registry: Value = serde_json::from_str(
            &fs::read_to_string(dir.join("data").join("plugins.json")).expect("written"),
        )
        .expect("JSON");
        assert_eq!(
            registry["plugins"],
            json!([]),
            "the record is gone from the registry"
        );

        let error = host.remove("hello").await.expect_err("already removed");
        assert!(matches!(error, PluginError::NotFound { .. }), "{error:?}");
    }

    #[tokio::test]
    async fn the_start_budget_fails_permanently_until_a_manual_restart_resets_it() {
        let dir = TempDir::new("plugin-start-budget");
        let source = plugin_dir(&dir, "hello");
        write_registry(&dir, vec![record_for(&source)]);
        let launcher = ScriptedLauncher::hello();
        launcher.fail_initialize.store(true, Ordering::SeqCst);
        let mut options = test_options();
        options.max_starts = 2;
        options.retry_base = Duration::from_millis(10);
        options.retry_cap = Duration::from_millis(20);
        let host = host_with(&dir, Arc::clone(&launcher), options);

        host.start_all().await;
        assert!(
            wait_until(WAIT, || async {
                launcher.launches.load(Ordering::SeqCst) >= 2
                    && is_state(&host, "hello", PluginState::Failed).await
            })
            .await,
            "two failed starts exhaust the budget: {:?}",
            host.status("hello")
        );
        // Stay quiet for longer than the retry delay: the budget says stop, not retry forever.
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            launcher.launches.load(Ordering::SeqCst),
            2,
            "a permanent failure does not keep spawning: {:?}",
            host.status("hello")
        );

        launcher.fail_initialize.store(false, Ordering::SeqCst);
        let status = host
            .restart("hello")
            .await
            .expect("a manual restart ignores the spent budget");
        assert_eq!(status.state, PluginState::Running);
        assert_eq!(launcher.launches.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn add_refuses_a_duplicate_a_missing_manifest_and_an_unresolvable_command() {
        let dir = TempDir::new("plugin-add-refusals");
        let launcher = ScriptedLauncher::hello();
        let host = host(&dir, Arc::clone(&launcher));
        let source = plugin_dir(&dir, "hello");
        host.add(&source).await.expect("the first add starts");

        let error = host.add(&source).await.expect_err("the namespace is taken");
        assert!(
            matches!(&error, PluginError::Duplicate { field, .. } if *field == "namespace"),
            "{error:?}"
        );

        let missing = dir.join("plugins").join("empty");
        fs::create_dir_all(&missing).expect("the directory is creatable");
        let error = host
            .add(&missing)
            .await
            .expect_err("there is no plugin.json");
        assert!(matches!(error, PluginError::Manifest { .. }), "{error:?}");

        let invalid = dir.join("plugins").join("invalid");
        fs::create_dir_all(&invalid).expect("the directory is creatable");
        let command = std::env::current_exe().unwrap().display().to_string();
        fs::write(
            invalid.join(MANIFEST_FILE),
            json!({
                "id": "com.example.invalid",
                "namespace": "Bad",
                "name": "Bad",
                "version": "0.1.0",
                "command": [command],
            })
            .to_string(),
        )
        .expect("the manifest is written");
        let error = host
            .add(&invalid)
            .await
            .expect_err("the namespace rule fails");
        assert!(matches!(error, PluginError::Manifest { .. }), "{error:?}");
        assert!(error.to_string().contains("namespace"), "{error}");

        let bad_command = dir.join("plugins").join("bad-command");
        fs::create_dir_all(&bad_command).expect("the directory is creatable");
        fs::write(
            bad_command.join(MANIFEST_FILE),
            json!({
                "id": "com.example.bad-command",
                "namespace": "bad-command",
                "name": "Bad command",
                "version": "0.1.0",
                "command": ["definitely-not-a-real-command-for-this-test"],
            })
            .to_string(),
        )
        .expect("the manifest is written");
        let error = host
            .add(&bad_command)
            .await
            .expect_err("a command that does not resolve is refused");
        assert!(
            matches!(&error, PluginError::CommandMissing { command } if command == "definitely-not-a-real-command-for-this-test"),
            "{error:?}"
        );

        assert_eq!(
            launcher.launches.load(Ordering::SeqCst),
            1,
            "only the valid plugin was started"
        );
        assert_eq!(host.list().await.len(), 1);
    }

    #[tokio::test]
    async fn the_registry_is_written_atomically_and_sorted_by_namespace() {
        let dir = TempDir::new("plugin-registry");
        let launcher = ScriptedLauncher::hello();
        let host = host(&dir, Arc::clone(&launcher));
        host.add(&plugin_dir(&dir, "zulu"))
            .await
            .expect("zulu starts");
        host.add(&plugin_dir(&dir, "alpha"))
            .await
            .expect("alpha starts");

        let data = dir.join("data");
        let registry: Value = serde_json::from_str(
            &fs::read_to_string(data.join("plugins.json")).expect("the registry is written"),
        )
        .expect("the registry is JSON");
        assert_eq!(registry["version"], 1);
        let namespaces: Vec<&str> = registry["plugins"]
            .as_array()
            .expect("plugins is an array")
            .iter()
            .map(|record| {
                record["manifest"]["namespace"]
                    .as_str()
                    .expect("a namespace")
            })
            .collect();
        assert_eq!(namespaces, ["alpha", "zulu"], "sorted by namespace");

        let leftovers: Vec<String> = fs::read_dir(&data)
            .expect("the data directory is readable")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "plugins.json" && name != "logs")
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temporary registry file is left behind: {leftovers:?}"
        );
    }

    #[tokio::test]
    async fn load_lists_recorded_plugins_as_stopped_and_starts_them_with_start_all() {
        let dir = TempDir::new("plugin-load");
        let source = plugin_dir(&dir, "hello");
        write_registry(&dir, vec![record_for(&source)]);
        let launcher = ScriptedLauncher::hello();
        let host = host_with(&dir, Arc::clone(&launcher), test_options());

        let listed = host.list().await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].state, PluginState::Stopped);
        assert_eq!(listed[0].tools, 0);
        assert_eq!(
            launcher.launches.load(Ordering::SeqCst),
            0,
            "loading the registry starts nothing"
        );
        assert!(host
            .logs("hello", 10)
            .await
            .expect("a missing log is an empty read")
            .is_empty());

        host.start_all().await;
        assert!(
            wait_until(WAIT, || is_state(&host, "hello", PluginState::Running)).await,
            "start_all starts the recorded plugins: {:?}",
            host.status("hello")
        );
    }

    #[tokio::test]
    async fn an_empty_data_directory_is_an_empty_host() {
        let dir = TempDir::new("plugin-empty");
        let launcher = ScriptedLauncher::hello();
        let host = host(&dir, Arc::clone(&launcher));

        assert!(host.list().await.is_empty());
        assert!(host.tools().is_empty());
        assert_eq!(host.destructive("hello.greet"), None);
        let error = host.logs("hello", 10).await.expect_err("no such plugin");
        assert!(matches!(error, PluginError::NotFound { .. }), "{error:?}");
    }

    #[tokio::test]
    async fn stop_all_stops_every_plugin_and_leaves_no_live_child() {
        let dir = TempDir::new("plugin-stop-all");
        let launcher = ScriptedLauncher::hello();
        let host = host(&dir, Arc::clone(&launcher));
        host.add(&plugin_dir(&dir, "alpha"))
            .await
            .expect("alpha starts");
        host.add(&plugin_dir(&dir, "beta"))
            .await
            .expect("beta starts");
        assert_eq!(host.tools().len(), 2);

        host.stop_all().await;

        for namespace in ["alpha", "beta"] {
            assert_eq!(
                host.status(namespace).map(|status| status.state),
                Some(PluginState::Stopped),
                "{namespace} is stopped"
            );
        }
        assert!(host.tools().is_empty());
        assert_eq!(
            launcher.kills.load(Ordering::SeqCst),
            2,
            "every child was killed"
        );

        host.stop_all().await; // idempotent
    }
}
