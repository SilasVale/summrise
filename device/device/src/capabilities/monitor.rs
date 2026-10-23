//! The reachability monitor capability (design §4.5).
//!
//! A monitor is one `{id, kind, host, port, path?, expect?, interval_ms}` target and a loop that
//! probes it on its own timer. `tcp` asks whether a TCP connection can be made; `http` performs a
//! GET and is up when the status is below 500 and, when `expect` is set, the body contains that
//! text — a page that answers 200 with a login stub is DOWN when the caller said what it wanted to
//! see. The last [`SAMPLE_CAP`] samples stay in memory for the summary (`up_now`, `since_ms`,
//! `up_pct`, `drops`, `last_status`); the targets themselves are persisted to
//! `<data>/monitors.json` with an atomic rename, so a watch outlives a restart. A corrupt file is
//! quarantined to `monitors.json.bad` and the event is recorded in the device's `events.jsonl`;
//! the registry starts empty rather than guessing at a damaged list.
//!
//! Every tool this capability declares is non-destructive: `monitor.add` and `monitor.remove`
//! only configure (and persist) this capability's own target list, and `monitor.list`,
//! `monitor.probe` and `monitor.status` only observe. No monitor tool starts a process, writes
//! outside the capability's data file, or changes anything a later read cannot see.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;

use crate::fsutil::create_new_sibling;

/// The most targets one device watches.
pub const MAX_TARGETS: usize = 16;

/// The shortest interval a target may be probed at, in milliseconds.
pub const MIN_INTERVAL_MS: u64 = 5_000;

/// How long a TCP connect may take before the probe is DOWN.
pub const TCP_TIMEOUT_MS: u64 = 3_000;

/// How long an HTTP probe may take (headers and the body it reads) before it is DOWN.
pub const HTTP_TIMEOUT_MS: u64 = 5_000;

/// How many probes one target retains in memory.
pub const SAMPLE_CAP: usize = 60;

/// How much of an HTTP body is searched for `expect`.
///
/// A real page's marker is in the head; the cap keeps a target streaming gigabytes from making the
/// prober the problem.
const HTTP_BODY_CAP: usize = 256 * 1024;

/// The file, under `<data>`, the target list is persisted to.
const MONITORS_FILE: &str = "monitors.json";

/// The device's audit/event log. A quarantine is recorded here, beside the call records.
const EVENTS_FILE: &str = "events.jsonl";

/// What a target probes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A bare TCP connect.
    Tcp,
    /// An HTTP GET of `path`.
    Http,
}

/// One watched target, as it is persisted and returned to a client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    /// The stable id, `host:port` for a TCP target and `host:port/path` for an HTTP one.
    pub id: String,
    /// What the probe does.
    pub kind: Kind,
    /// The host or address, without a scheme or brackets.
    pub host: String,
    /// The TCP port.
    pub port: u16,
    /// The HTTP path, always starting with `/`; `None` for a TCP target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Text the HTTP body must contain, when the caller wants a content assertion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect: Option<String>,
    /// How often the target is probed, in milliseconds.
    pub interval_ms: u64,
}

/// The arguments `monitor.add` gives the capability, before validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetSpec {
    /// What the probe does.
    pub kind: Kind,
    /// The host or address.
    pub host: String,
    /// The TCP port.
    pub port: u16,
    /// The HTTP path; ignored for a TCP target.
    pub path: Option<String>,
    /// Text the HTTP body must contain.
    pub expect: Option<String>,
    /// The probe interval, in milliseconds.
    pub interval_ms: u64,
}

impl TargetSpec {
    /// A TCP target, probed every `interval_ms`.
    ///
    /// Constructors for the two shapes the tests name; the MCP surface builds the struct field by
    /// field, because it must pass a `path` or `expect` through to the capability's refusal.
    #[cfg(test)]
    pub fn tcp(host: impl Into<String>, port: u16, interval_ms: u64) -> TargetSpec {
        TargetSpec {
            kind: Kind::Tcp,
            host: host.into(),
            port,
            path: None,
            expect: None,
            interval_ms,
        }
    }

    /// An HTTP target that GETs `path`, optionally requiring `expect` in the body.
    #[cfg(test)]
    pub fn http(
        host: impl Into<String>,
        port: u16,
        path: impl Into<String>,
        expect: Option<&str>,
        interval_ms: u64,
    ) -> TargetSpec {
        TargetSpec {
            kind: Kind::Http,
            host: host.into(),
            port,
            path: Some(path.into()),
            expect: expect.map(str::to_owned),
            interval_ms,
        }
    }
}

/// The limits a registry enforces.
///
/// They are data so a test can use a 100 ms interval and a 250 ms probe bound instead of waiting
/// out the production values, exactly as `terminal::Limits` does for the write bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most targets the registry holds.
    pub max_targets: usize,
    /// The shortest accepted probe interval.
    pub min_interval_ms: u64,
    /// The TCP connect bound.
    pub tcp_timeout_ms: u64,
    /// The HTTP probe bound.
    pub http_timeout_ms: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_targets: MAX_TARGETS,
            min_interval_ms: MIN_INTERVAL_MS,
            tcp_timeout_ms: TCP_TIMEOUT_MS,
            http_timeout_ms: HTTP_TIMEOUT_MS,
        }
    }
}

/// One probe's result: when, whether it answered, and how long it took when it did.
///
/// `latency_ms` is `None` for a failed probe: a latency for a connection that never happened would
/// be a fabricated measurement. `status` is the HTTP status behind the verdict when a response
/// arrived; it is not part of the sample's wire shape, but the summary's `last_status` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Sample {
    /// When the probe finished, in Unix milliseconds.
    pub ts: u64,
    /// Whether the target answered as the target's rule requires.
    pub ok: bool,
    /// The round trip in milliseconds; `None` when no connection happened.
    pub latency_ms: Option<u64>,
    /// The HTTP status, when this was an HTTP probe and a response arrived.
    #[serde(skip_serializing)]
    pub(crate) status: Option<u16>,
}

/// One target's summary over the samples it holds.
///
/// Every field is `None` before the first probe: no measurement has been made, and a zero would
/// read as "measured, stable" rather than "not asked yet".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// Whether the newest probe answered.
    pub up_now: Option<bool>,
    /// When the current run of identical states began, in Unix milliseconds.
    pub since_ms: Option<u64>,
    /// The share of the retained probes that were up, as a rounded percentage.
    pub up_pct: Option<u64>,
    /// The up→down transitions inside the retained window.
    pub drops: Option<u64>,
    /// The newest probe's HTTP status, when it was an HTTP probe with a response.
    pub last_status: Option<u16>,
}

/// Why a monitor operation was refused or failed.
#[derive(Debug)]
pub enum MonitorError {
    /// The registry already holds [`Limits::max_targets`] watches.
    TooManyTargets {
        /// The cap.
        max: usize,
    },
    /// The host is empty or whitespace.
    EmptyHost,
    /// The host is not a name or address: it carries a scheme, a path, whitespace or a non-IPv6
    /// colon.
    InvalidHost,
    /// The port is zero; ports are `1..=65535`.
    InvalidPort {
        /// The port the caller gave.
        port: u16,
    },
    /// The interval is below [`Limits::min_interval_ms`].
    IntervalTooShort {
        /// The interval the caller gave.
        interval_ms: u64,
        /// The minimum this registry enforces.
        min: u64,
    },
    /// A TCP target was given a path or an expectation; those are HTTP-only.
    TcpTakesNoPath,
    /// An HTTP path contains whitespace.
    InvalidPath,
    /// The id names no watched target.
    UnknownTarget {
        /// The id the caller sent.
        id: String,
    },
    /// The filesystem refused a read or a write.
    Io(io::Error),
}

impl fmt::Display for MonitorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MonitorError::TooManyTargets { max } => {
                write!(f, "the device watches at most {max} targets; remove one first")
            }
            MonitorError::EmptyHost => write!(f, "a target's host must not be empty"),
            MonitorError::InvalidHost => write!(
                f,
                "a host is a name or address, not a URL: it may not carry a scheme, a path, whitespace or a non-IPv6 colon"
            ),
            MonitorError::InvalidPort { port } => {
                write!(f, "port {port} is not usable; ports are 1..=65535")
            }
            MonitorError::IntervalTooShort { interval_ms, min } => write!(
                f,
                "the probe interval is {interval_ms} ms; the minimum is {min} ms"
            ),
            MonitorError::TcpTakesNoPath => {
                write!(f, "a tcp target probes a port; path and expect are http-only")
            }
            MonitorError::InvalidPath => {
                write!(f, "an http path may not contain whitespace")
            }
            MonitorError::UnknownTarget { id } => {
                write!(f, "target {id} is not watched")
            }
            MonitorError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for MonitorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MonitorError::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// The device's one monitor registry: targets, their samples, and one probe task per target.
pub struct MonitorRegistry {
    /// `<data>/monitors.json`.
    path: PathBuf,
    limits: Limits,
    /// Built once; `None` only when the TLS/HTTP stack refused to initialize, in which case an
    /// HTTP probe is DOWN with no status rather than a startup panic.
    client: Option<reqwest::Client>,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    targets: Vec<TargetState>,
    tasks: HashMap<String, JoinHandle<()>>,
    next_generation: u64,
}

struct TargetState {
    target: Target,
    /// Bumped whenever the target is reconfigured; a probe from an older generation is discarded.
    generation: u64,
    samples: VecDeque<Sample>,
}

/// The persisted document: one array, so a future version can add siblings without moving `targets`.
#[derive(Serialize, Deserialize)]
struct Persisted {
    targets: Vec<Target>,
}

impl MonitorRegistry {
    /// A registry reading and writing `<data>/monitors.json`, with the production limits.
    pub fn new(data: &Path) -> MonitorRegistry {
        MonitorRegistry::with_limits(data, Limits::default())
    }

    /// A registry with the caller's limits; a test shortens the interval and probe bounds.
    pub fn with_limits(data: &Path, limits: Limits) -> MonitorRegistry {
        let path = data.join(MONITORS_FILE);
        let events = data.join(EVENTS_FILE);
        let targets = match load(&path, limits) {
            LoadOutcome::Loaded(targets) => targets,
            LoadOutcome::Corrupt(reason) => {
                let quarantined = quarantine(&path);
                record_event(
                    &events,
                    json!({
                        "ts_ms": now_ms(),
                        "event": "monitors_quarantined",
                        "path": path.display().to_string(),
                        "quarantined": quarantined
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        "error": reason,
                    }),
                );
                Vec::new()
            }
            LoadOutcome::Unreadable(error) => {
                record_event(
                    &events,
                    json!({
                        "ts_ms": now_ms(),
                        "event": "monitors_load_failed",
                        "path": path.display().to_string(),
                        "error": error.to_string(),
                    }),
                );
                Vec::new()
            }
        };
        let targets = targets
            .into_iter()
            .map(|target| TargetState {
                target,
                generation: 0,
                samples: VecDeque::new(),
            })
            .collect();
        MonitorRegistry {
            path,
            limits,
            client: http_client(limits),
            inner: Mutex::new(Inner {
                targets,
                tasks: HashMap::new(),
                next_generation: 1,
            }),
        }
    }

    /// Start a probe task for every loaded target that does not have one. Idempotent.
    pub fn spawn(self: &Arc<Self>) {
        if tokio::runtime::Handle::try_current().is_err() {
            // A caller outside a runtime (a synchronous unit test, a startup probe) has no task to
            // give the target to; a later call inside the runtime starts them.
            return;
        }
        let pending: Vec<(String, u64, u64)> = {
            let inner = lock(&self.inner);
            inner
                .targets
                .iter()
                .filter(|state| !inner.tasks.contains_key(&state.target.id))
                .map(|state| {
                    (
                        state.target.id.clone(),
                        state.generation,
                        state.target.interval_ms,
                    )
                })
                .collect()
        };
        for (id, generation, interval_ms) in pending {
            let handle = self.spawn_loop(id.clone(), generation, interval_ms);
            lock(&self.inner).tasks.insert(id, handle);
        }
    }

    /// Add a target, persist the list, and start its probe loop.
    ///
    /// Re-adding the same id updates that target's rule and interval and keeps its samples; the
    /// id is the address (or the URL), and the expectation is an attribute of it.
    pub fn add(self: &Arc<Self>, spec: TargetSpec) -> Result<Target, MonitorError> {
        let target = target_from_spec(spec, self.limits)?;
        let (old_handle, generation) = {
            let mut inner = lock(&self.inner);
            let existing = inner
                .targets
                .iter()
                .position(|state| state.target.id == target.id);
            if existing.is_none() && inner.targets.len() >= self.limits.max_targets {
                return Err(MonitorError::TooManyTargets {
                    max: self.limits.max_targets,
                });
            }

            let mut persisted: Vec<Target> = inner
                .targets
                .iter()
                .map(|state| state.target.clone())
                .collect();
            match existing {
                Some(index) => persisted[index] = target.clone(),
                None => persisted.push(target.clone()),
            }
            // The file is the commit point: a refused or failed write leaves both the registry and
            // the persisted list exactly as they were.
            save(&self.path, &persisted).map_err(MonitorError::Io)?;

            let generation = inner.next_generation;
            inner.next_generation = inner.next_generation.saturating_add(1);
            match existing {
                Some(index) => {
                    let state = &mut inner.targets[index];
                    state.target = target.clone();
                    state.generation = generation;
                }
                None => inner.targets.push(TargetState {
                    target: target.clone(),
                    generation,
                    samples: VecDeque::new(),
                }),
            }
            (inner.tasks.remove(&target.id), generation)
        };
        if let Some(handle) = old_handle {
            // Aborting does not wait for an in-flight probe; the generation check in `record`
            // discards its sample if it lands after this update.
            handle.abort();
        }
        if tokio::runtime::Handle::try_current().is_ok() {
            let handle = self.spawn_loop(target.id.clone(), generation, target.interval_ms);
            lock(&self.inner).tasks.insert(target.id.clone(), handle);
        }
        Ok(target)
    }

    /// Remove a target and stop its loop. `false` means it was not being watched.
    pub fn remove(&self, id: &str) -> Result<bool, MonitorError> {
        let handle = {
            let mut inner = lock(&self.inner);
            let Some(index) = inner.targets.iter().position(|state| state.target.id == id) else {
                return Ok(false);
            };
            let mut persisted: Vec<Target> = inner
                .targets
                .iter()
                .map(|state| state.target.clone())
                .collect();
            persisted.remove(index);
            save(&self.path, &persisted).map_err(MonitorError::Io)?;
            inner.targets.remove(index);
            inner.tasks.remove(id)
        };
        if let Some(handle) = handle {
            handle.abort();
        }
        Ok(true)
    }

    /// The watched targets, in the order they were added.
    pub fn list(&self) -> Vec<Target> {
        lock(&self.inner)
            .targets
            .iter()
            .map(|state| state.target.clone())
            .collect()
    }

    /// One target's summary, or `None` when it is not watched.
    pub fn summary(&self, id: &str) -> Option<Summary> {
        let inner = lock(&self.inner);
        inner
            .targets
            .iter()
            .find(|state| state.target.id == id)
            .map(|state| summarize(&state.samples))
    }

    /// Probe one target right now, record the sample and return it.
    pub async fn probe(&self, id: &str) -> Result<Sample, MonitorError> {
        let (target, generation) = {
            let inner = lock(&self.inner);
            inner
                .targets
                .iter()
                .find(|state| state.target.id == id)
                .map(|state| (state.target.clone(), state.generation))
                .ok_or_else(|| MonitorError::UnknownTarget { id: id.to_owned() })?
        };
        let sample = self.probe_target(&target).await;
        self.record(id, generation, sample);
        Ok(sample)
    }

    /// The target as it was when this loop's generation was created, if it still is.
    fn target_for(&self, id: &str, generation: u64) -> Option<Target> {
        let inner = lock(&self.inner);
        inner
            .targets
            .iter()
            .find(|state| state.target.id == id && state.generation == generation)
            .map(|state| state.target.clone())
    }

    /// Record a sample only against the generation it was taken for.
    fn record(&self, id: &str, generation: u64, sample: Sample) {
        let mut inner = lock(&self.inner);
        if let Some(state) = inner
            .targets
            .iter_mut()
            .find(|state| state.target.id == id && state.generation == generation)
        {
            push_sample(&mut state.samples, sample);
        }
    }

    async fn scheduled_probe(&self, id: &str, generation: u64) -> bool {
        let Some(target) = self.target_for(id, generation) else {
            return false;
        };
        let sample = self.probe_target(&target).await;
        self.record(id, generation, sample);
        true
    }

    /// One task per target, on that target's own interval.
    ///
    /// The first tick is immediate (`tokio::time::interval`'s contract), so a fresh watch has a
    /// reading at once; a slow probe that overruns the interval delays the next tick instead of
    /// piling probes up behind it.
    fn spawn_loop(
        self: &Arc<Self>,
        id: String,
        generation: u64,
        interval_ms: u64,
    ) -> JoinHandle<()> {
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(interval_ms));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let Some(registry) = weak.upgrade() else {
                    return;
                };
                if !registry.scheduled_probe(&id, generation).await {
                    return;
                }
            }
        })
    }

    async fn probe_target(&self, target: &Target) -> Sample {
        match target.kind {
            Kind::Tcp => self.probe_tcp(target).await,
            Kind::Http => self.probe_http(target).await,
        }
    }

    async fn probe_tcp(&self, target: &Target) -> Sample {
        let started = Instant::now();
        let connected = tokio::time::timeout(
            Duration::from_millis(self.limits.tcp_timeout_ms),
            TcpStream::connect((target.host.as_str(), target.port)),
        )
        .await;
        let ok = matches!(connected, Ok(Ok(_)));
        Sample {
            ts: now_ms(),
            ok,
            // A latency for a connection that never happened would be a fabricated measurement.
            latency_ms: if ok { Some(elapsed_ms(started)) } else { None },
            status: None,
        }
    }

    async fn probe_http(&self, target: &Target) -> Sample {
        let Some(client) = self.client.as_ref() else {
            return Sample {
                ts: now_ms(),
                ok: false,
                latency_ms: None,
                status: None,
            };
        };
        let started = Instant::now();
        let url = http_url(target);
        let request = async {
            let mut response = client.get(&url).send().await.ok()?;
            let status = response.status().as_u16();
            let expect_ok = match target.expect.as_deref() {
                None => None,
                Some(needle) => {
                    let mut body = Vec::new();
                    while body.len() < HTTP_BODY_CAP {
                        match response.chunk().await {
                            Ok(Some(chunk)) => {
                                let room = HTTP_BODY_CAP - body.len();
                                body.extend_from_slice(&chunk[..chunk.len().min(room)]);
                            }
                            Ok(None) => break,
                            // A read that fails mid-body is judged on the bytes that did arrive;
                            // the marker was either observed in them or it was not.
                            Err(_) => break,
                        }
                    }
                    Some(String::from_utf8_lossy(&body).contains(needle))
                }
            };
            Some((status, expect_ok))
        };

        // The client has its own timeout; this one also bounds the body read and the outer wait,
        // so a target that answers headers and then stalls cannot hold the task.
        match tokio::time::timeout(Duration::from_millis(self.limits.http_timeout_ms), request)
            .await
        {
            Ok(Some((status, expect_ok))) => Sample {
                ts: now_ms(),
                ok: status < 500 && expect_ok.unwrap_or(true),
                latency_ms: Some(elapsed_ms(started)),
                status: Some(status),
            },
            _ => Sample {
                ts: now_ms(),
                ok: false,
                latency_ms: None,
                status: None,
            },
        }
    }
}

impl Drop for MonitorRegistry {
    fn drop(&mut self) {
        let inner = self
            .inner
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (_, handle) in inner.tasks.drain() {
            handle.abort();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Build the HTTP probe's one client. `None` is a broken TLS/HTTP stack, not a per-target fact;
/// the error is reported once and every HTTP probe is then DOWN with no fabricated status.
fn http_client(limits: Limits) -> Option<reqwest::Client> {
    match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_millis(limits.http_timeout_ms))
        .build()
    {
        Ok(client) => Some(client),
        Err(error) => {
            eprintln!(
                "summrise-device: the monitor's HTTP client could not be built ({error}); \
                 http targets will probe as down"
            );
            None
        }
    }
}

fn http_url(target: &Target) -> String {
    let host = if target.host.contains(':') {
        format!("[{}]", target.host)
    } else {
        target.host.clone()
    };
    let path = target.path.as_deref().unwrap_or("/");
    format!("http://{host}:{}{path}", target.port)
}

/// Shape a validated target from the caller's arguments and the registry's limits.
fn target_from_spec(spec: TargetSpec, limits: Limits) -> Result<Target, MonitorError> {
    let host = normalize_host(&spec.host)?;
    if spec.port == 0 {
        return Err(MonitorError::InvalidPort { port: spec.port });
    }
    if spec.interval_ms < limits.min_interval_ms {
        return Err(MonitorError::IntervalTooShort {
            interval_ms: spec.interval_ms,
            min: limits.min_interval_ms,
        });
    }
    let expect = spec
        .expect
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned);
    let (path, expect) = match spec.kind {
        Kind::Http => (Some(normalize_path(spec.path.as_deref())?), expect),
        Kind::Tcp => {
            let has_path = spec
                .path
                .as_deref()
                .is_some_and(|path| !path.trim().is_empty());
            if has_path || expect.is_some() {
                return Err(MonitorError::TcpTakesNoPath);
            }
            (None, None)
        }
    };
    Ok(Target {
        id: target_id(&host, spec.port, path.as_deref()),
        kind: spec.kind,
        host,
        port: spec.port,
        path,
        expect,
        interval_ms: spec.interval_ms,
    })
}

fn normalize_host(text: &str) -> Result<String, MonitorError> {
    let text = text.trim();
    let host = text
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(text);
    if host.is_empty() {
        return Err(MonitorError::EmptyHost);
    }
    if host.chars().any(char::is_whitespace)
        || host.contains(['/', '?', '#', '@', '[', ']'])
        || (host.contains(':') && host.parse::<std::net::Ipv6Addr>().is_err())
    {
        return Err(MonitorError::InvalidHost);
    }
    Ok(host.to_owned())
}

fn normalize_path(path: Option<&str>) -> Result<String, MonitorError> {
    let path = path.unwrap_or("").trim();
    let path = if path.is_empty() {
        "/".to_owned()
    } else if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    };
    if path.chars().any(char::is_whitespace) {
        return Err(MonitorError::InvalidPath);
    }
    Ok(path)
}

/// `host:port`, plus the path for an HTTP target; lowercased and stripped of characters that would
/// make an id ambiguous, so adding the same watch twice is the same watch.
fn target_id(host: &str, port: u16, path: Option<&str>) -> String {
    let mut host: String = host
        .trim()
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | ':') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if host.is_empty() {
        host = "target".to_owned();
    }
    if host.contains(':') && !host.starts_with('[') {
        host = format!("[{host}]");
    }
    match path.filter(|path| !path.is_empty()) {
        Some(path) => {
            let path = if path.starts_with('/') {
                path.to_owned()
            } else {
                format!("/{path}")
            };
            format!("{host}:{port}{path}")
        }
        None => format!("{host}:{port}"),
    }
}

fn push_sample(samples: &mut VecDeque<Sample>, sample: Sample) {
    samples.push_back(sample);
    while samples.len() > SAMPLE_CAP {
        samples.pop_front();
    }
}

fn summarize(samples: &VecDeque<Sample>) -> Summary {
    let Some(newest) = samples.back() else {
        return Summary::default();
    };
    let total = samples.len();
    let up = samples.iter().filter(|sample| sample.ok).count();
    let up_pct = ((up as f64 / total as f64) * 100.0).round() as u64;

    // `since_ms` is the stamp of the OLDEST probe in the current run of identical states: walking
    // back from the newest until the state differs is when the state began.
    let mut since_ms = newest.ts;
    for sample in samples.iter().rev() {
        if sample.ok != newest.ok {
            break;
        }
        since_ms = sample.ts;
    }

    let mut drops = 0;
    let mut previous: Option<bool> = None;
    for sample in samples {
        if previous == Some(true) && !sample.ok {
            drops += 1;
        }
        previous = Some(sample.ok);
    }

    Summary {
        up_now: Some(newest.ok),
        since_ms: Some(since_ms),
        up_pct: Some(up_pct),
        drops: Some(drops),
        last_status: newest.status,
    }
}

enum LoadOutcome {
    Loaded(Vec<Target>),
    /// The file exists and is not a target list this build understands.
    Corrupt(String),
    /// The file could not be read at all.
    Unreadable(io::Error),
}

fn load(path: &Path, limits: Limits) -> LoadOutcome {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Persisted>(&bytes) {
            Ok(document) => match normalize_loaded(document.targets, limits) {
                Ok(targets) => LoadOutcome::Loaded(targets),
                Err(reason) => LoadOutcome::Corrupt(reason),
            },
            Err(error) => LoadOutcome::Corrupt(format!("not a target list: {error}")),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => LoadOutcome::Loaded(Vec::new()),
        Err(error) => LoadOutcome::Unreadable(error),
    }
}

/// Validate every persisted target with the same rules `monitor.add` uses.
///
/// One bad row quarantines the file instead of silently dropping that row: a target list that
/// disagrees with itself is not something the registry can repair by guessing which half is true.
fn normalize_loaded(targets: Vec<Target>, limits: Limits) -> Result<Vec<Target>, String> {
    if targets.len() > limits.max_targets {
        return Err(format!(
            "the file holds {} targets; the cap is {}",
            targets.len(),
            limits.max_targets
        ));
    }
    let mut seen = std::collections::HashSet::new();
    let mut normalized = Vec::with_capacity(targets.len());
    for stored in targets {
        let id = stored.id.clone();
        let spec = TargetSpec {
            kind: stored.kind,
            host: stored.host,
            port: stored.port,
            path: stored.path,
            expect: stored.expect,
            interval_ms: stored.interval_ms,
        };
        let target =
            target_from_spec(spec, limits).map_err(|error| format!("target {id:?}: {error}"))?;
        if target.id != id {
            return Err(format!(
                "target {id:?} names {:?}; the id and the address disagree",
                target.id
            ));
        }
        if !seen.insert(target.id.clone()) {
            return Err(format!("target {id:?} appears more than once"));
        }
        normalized.push(target);
    }
    Ok(normalized)
}

/// Write the target list atomically: a random sibling, fsync, then the rename that commits it.
fn save(path: &Path, targets: &[Target]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(&Persisted {
        targets: targets.to_vec(),
    })
    .map_err(io::Error::other)?;
    let (temporary, mut file) = create_new_sibling(path)?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

/// Move a corrupt file aside. `None` means the rename failed; the registry still starts empty.
fn quarantine(path: &Path) -> Option<PathBuf> {
    let mut name = path.as_os_str().to_owned();
    name.push(".bad");
    let quarantined = PathBuf::from(name);
    // On Windows a rename over an existing file fails; replacing the previous evidence is the
    // honest choice, because this file is the evidence for the CURRENT start.
    let _ = fs::remove_file(&quarantined);
    match fs::rename(path, &quarantined) {
        Ok(()) => Some(quarantined),
        Err(error) => {
            eprintln!(
                "summrise-device: could not quarantine {} ({error}); the registry starts empty",
                path.display()
            );
            None
        }
    }
}

/// Append one event to the device's event log.
///
/// A quarantine happens before the MCP service starts, so a synchronous append is enough here; a
/// failed write is reported and never stops the device from starting.
fn record_event(path: &Path, event: serde_json::Value) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut line = event.to_string();
    line.push('\n');
    match fs::OpenOptions::new().create(true).append(true).open(path) {
        Ok(mut file) => {
            if let Err(error) = file.write_all(line.as_bytes()) {
                eprintln!(
                    "summrise-device: could not record a monitor event in {}: {error}",
                    path.display()
                );
            }
        }
        Err(error) => eprintln!(
            "summrise-device: could not record a monitor event in {}: {error}",
            path.display()
        ),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use axum::{routing::get, Router};
    use serde_json::Value;
    use tokio::net::TcpListener;

    use super::*;
    use crate::test_support::TempDir;

    fn limits() -> Limits {
        Limits {
            max_targets: MAX_TARGETS,
            min_interval_ms: 100,
            tcp_timeout_ms: 500,
            http_timeout_ms: 250,
        }
    }

    fn registry(dir: &TempDir) -> Arc<MonitorRegistry> {
        Arc::new(MonitorRegistry::with_limits(&dir.join("data"), limits()))
    }

    fn tcp(port: u16) -> TargetSpec {
        TargetSpec::tcp("127.0.0.1", port, 100)
    }

    fn http(port: u16, path: &str, expect: Option<&str>) -> TargetSpec {
        TargetSpec::http("127.0.0.1", port, path, expect, 100)
    }

    async fn tcp_listener() -> (TcpListener, u16) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let port = listener.local_addr().expect("the bound address").port();
        (listener, port)
    }

    async fn closed_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let port = listener.local_addr().expect("the bound address").port();
        drop(listener);
        port
    }

    /// A real HTTP server whose pages all answer 200 with `body`.
    async fn http_server(body: &'static str) -> (u16, JoinHandle<()>) {
        let app = Router::new()
            .route("/", get(move || async move { body }))
            .route("/health", get(move || async move { body }));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let port = listener.local_addr().expect("the bound address").port();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (port, server)
    }

    /// A server that accepts connections and never answers, so the HTTP probe must time out.
    async fn silent_server() -> (u16, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let port = listener.local_addr().expect("the bound address").port();
        let server = tokio::spawn(async move {
            let mut sockets = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                sockets.push(socket);
            }
        });
        (port, server)
    }

    #[tokio::test]
    async fn a_tcp_listener_is_up_and_a_closed_port_is_down() {
        let dir = TempDir::new("monitor-tcp");
        let registry = registry(&dir);
        registry.spawn();

        let (listener, port) = tcp_listener().await;
        let up = registry.add(tcp(port)).expect("a valid tcp target");
        let sample = registry.probe(&up.id).await.expect("the target is watched");
        assert!(sample.ok, "a listening socket accepts the connect");
        let latency = sample
            .latency_ms
            .expect("a connection that happened has a latency");
        assert!(
            latency < limits().tcp_timeout_ms,
            "a local connect is under the probe bound: {latency} ms"
        );
        assert!(sample.ts > 0);
        drop(listener);

        let port = closed_port().await;
        let down = registry
            .add(tcp(port))
            .expect("a valid tcp target on a closed port");
        let sample = registry
            .probe(&down.id)
            .await
            .expect("the target is watched");
        assert!(!sample.ok, "a refused connect is down");
        assert_eq!(
            sample.latency_ms, None,
            "a failed connect has no latency to report"
        );
    }

    #[tokio::test]
    async fn an_http_200_without_the_expected_text_is_down_only_when_expect_is_set() {
        let dir = TempDir::new("monitor-http-expect");
        let registry = registry(&dir);
        registry.spawn();
        let (port, server) = http_server("login page").await;

        let expected = registry
            .add(http(port, "/", Some("OpenWrt")))
            .expect("a valid http target");
        let sample = registry
            .probe(&expected.id)
            .await
            .expect("the target is watched");
        assert!(
            !sample.ok,
            "a 200 whose body does not contain the expected text is down"
        );
        let summary = registry
            .summary(&expected.id)
            .expect("the target has a summary");
        assert_eq!(
            summary.last_status,
            Some(200),
            "the status is recorded so the verdict can be explained"
        );
        assert_eq!(summary.up_now, Some(false));

        let plain = registry
            .add(http(port, "/health", None))
            .expect("a second path is a second target");
        let sample = registry
            .probe(&plain.id)
            .await
            .expect("the target is watched");
        assert!(
            sample.ok,
            "a 200 is up when the caller expected no particular text"
        );
        assert_eq!(
            registry
                .summary(&plain.id)
                .expect("the target has a summary")
                .last_status,
            Some(200)
        );
        server.abort();
    }

    #[tokio::test]
    async fn a_probe_returns_a_sample_within_its_timeout() {
        let dir = TempDir::new("monitor-timeout");
        let registry = registry(&dir);
        let (port, server) = silent_server().await;
        let target = registry
            .add(http(port, "/", None))
            .expect("a valid http target");

        let started = Instant::now();
        let sample = registry
            .probe(&target.id)
            .await
            .expect("a slow target still produces a sample");
        let elapsed = started.elapsed();

        assert!(!sample.ok, "a server that never answers is down");
        assert_eq!(sample.latency_ms, None);
        assert!(
            elapsed < Duration::from_secs(2),
            "the {} ms probe bound ends the wait: {elapsed:?}",
            limits().http_timeout_ms
        );
        assert!(sample.ts > 0);
        server.abort();
    }

    #[tokio::test]
    async fn list_and_status_have_the_documented_shape() {
        let dir = TempDir::new("monitor-shape");
        let registry = registry(&dir);
        let (listener, port) = tcp_listener().await;
        let target = registry.add(tcp(port)).expect("a valid target");
        let sample = registry
            .probe(&target.id)
            .await
            .expect("the target is watched");

        assert_eq!(
            registry.list(),
            vec![target.clone()],
            "list is the watched targets, in insertion order"
        );
        let summary = registry.summary(&target.id).expect("a summary");
        assert_eq!(summary.up_now, Some(true));
        assert_eq!(summary.up_pct, Some(100));
        assert_eq!(summary.drops, Some(0));
        assert_eq!(summary.since_ms, Some(sample.ts));
        assert_eq!(summary.last_status, None, "a tcp probe has no status");

        let status = serde_json::to_value(summary).expect("the summary serializes");
        let keys: Vec<&String> = status
            .as_object()
            .expect("the summary is an object")
            .keys()
            .collect();
        for key in ["up_now", "since_ms", "up_pct", "drops", "last_status"] {
            assert!(keys.iter().any(|present| *present == key), "{status}");
        }
        assert_eq!(keys.len(), 5, "the summary has exactly the documented keys");

        let wire = serde_json::to_value(&target).expect("the target serializes");
        assert_eq!(wire["id"], target.id);
        assert_eq!(wire["kind"], "tcp");
        assert_eq!(wire["host"], "127.0.0.1");
        assert_eq!(wire["port"], port);
        assert_eq!(wire["interval_ms"], 100);
        assert!(
            wire.get("path").is_none(),
            "an absent path is omitted from the persisted/wire form"
        );

        assert_eq!(registry.summary("unknown:1"), None);
        assert!(matches!(
            registry.probe("unknown:1").await,
            Err(MonitorError::UnknownTarget { .. })
        ));
        drop(listener);
    }

    #[tokio::test]
    async fn a_target_survives_a_registry_restart() {
        let dir = TempDir::new("monitor-restart");
        let data = dir.join("data");
        let target = {
            let registry = Arc::new(MonitorRegistry::with_limits(&data, limits()));
            registry.spawn();
            let (_listener, port) = tcp_listener().await;
            registry.add(tcp(port)).expect("a valid target")
        };

        let registry = Arc::new(MonitorRegistry::with_limits(&data, limits()));
        registry.spawn();
        assert_eq!(
            registry.list(),
            vec![target.clone()],
            "the target list is reloaded from monitors.json"
        );
        assert_eq!(
            registry.summary(&target.id),
            Some(Summary::default()),
            "samples are in-memory only; a restart starts a fresh series"
        );
    }

    #[tokio::test]
    async fn a_seventeenth_target_is_refused() {
        let dir = TempDir::new("monitor-cap");
        let registry = registry(&dir);
        for port in 1..=MAX_TARGETS as u16 {
            registry
                .add(tcp(port))
                .expect("targets up to the cap are accepted");
        }

        let error = registry
            .add(tcp(MAX_TARGETS as u16 + 1))
            .expect_err("a seventeenth target does not fit");
        assert!(
            matches!(error, MonitorError::TooManyTargets { max: MAX_TARGETS }),
            "{error:?}"
        );
        assert_eq!(registry.list().len(), MAX_TARGETS);
    }

    #[tokio::test]
    async fn a_corrupt_monitors_file_is_quarantined_and_the_registry_starts_empty() {
        let dir = TempDir::new("monitor-corrupt");
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data directory is creatable");
        let corrupt = "{ this is not the target list";
        fs::write(data.join("monitors.json"), corrupt).expect("the corrupt fixture is written");

        let registry = Arc::new(MonitorRegistry::new(&data));

        assert!(
            registry.list().is_empty(),
            "a corrupt file starts an empty registry instead of a guessed list"
        );
        assert!(
            !data.join("monitors.json").exists(),
            "the corrupt file is moved out of the way"
        );
        assert_eq!(
            fs::read_to_string(data.join("monitors.json.bad"))
                .expect("the quarantine file is readable"),
            corrupt,
            "the evidence is kept, byte for byte"
        );

        let events = fs::read_to_string(data.join("events.jsonl")).expect("the event is recorded");
        let event: Value = serde_json::from_str(
            events
                .lines()
                .next()
                .expect("the quarantine writes one event line"),
        )
        .expect("the event is JSON");
        assert_eq!(event["event"], "monitors_quarantined");
        assert!(
            event["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty()),
            "the event carries the reason: {event}"
        );

        let target = registry
            .add(TargetSpec::tcp("127.0.0.1", 9, MIN_INTERVAL_MS))
            .expect("the registry is usable after a quarantine");
        let persisted: Value = serde_json::from_str(
            &fs::read_to_string(data.join("monitors.json")).expect("a fresh file is written"),
        )
        .expect("the fresh file is JSON");
        assert_eq!(persisted["targets"][0]["id"], target.id);
    }

    #[test]
    fn the_caps_refuse_an_empty_host_a_short_interval_and_a_tcp_path() {
        let dir = TempDir::new("monitor-validation");
        let registry = Arc::new(MonitorRegistry::new(&dir.join("data")));

        let empty = registry
            .add(TargetSpec::tcp("   ", 80, MIN_INTERVAL_MS))
            .expect_err("an empty host names nothing");
        assert!(matches!(empty, MonitorError::EmptyHost), "{empty:?}");

        let short = registry
            .add(TargetSpec::tcp("127.0.0.1", 80, MIN_INTERVAL_MS - 1))
            .expect_err("the production minimum is five seconds");
        assert!(
            matches!(
                short,
                MonitorError::IntervalTooShort {
                    interval_ms: 4_999,
                    min: MIN_INTERVAL_MS
                }
            ),
            "{short:?}"
        );

        let zero = registry
            .add(TargetSpec::tcp("127.0.0.1", 0, MIN_INTERVAL_MS))
            .expect_err("port zero names no service");
        assert!(
            matches!(zero, MonitorError::InvalidPort { port: 0 }),
            "{zero:?}"
        );

        let mut with_path = TargetSpec::tcp("127.0.0.1", 80, MIN_INTERVAL_MS);
        with_path.path = Some("/".to_owned());
        let path = registry
            .add(with_path)
            .expect_err("a tcp probe does not request a path");
        assert!(matches!(path, MonitorError::TcpTakesNoPath), "{path:?}");
    }

    #[tokio::test]
    async fn the_scheduled_loop_probes_without_a_call() {
        let dir = TempDir::new("monitor-loop");
        let registry = registry(&dir);
        registry.spawn();
        let (_listener, port) = tcp_listener().await;
        let target = registry.add(tcp(port)).expect("a valid target");

        let deadline = Instant::now() + Duration::from_secs(5);
        while registry
            .summary(&target.id)
            .and_then(|summary| summary.up_now)
            != Some(true)
        {
            assert!(
                Instant::now() < deadline,
                "the 100 ms loop probes on its own"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            registry
                .summary(&target.id)
                .expect("a summary")
                .since_ms
                .is_some(),
            "the first probe starts the current state's clock"
        );

        assert!(
            matches!(registry.remove(&target.id), Ok(true)),
            "the removal reports that a watch existed"
        );
        assert_eq!(registry.summary(&target.id), None);
        assert!(
            matches!(registry.remove(&target.id), Ok(false)),
            "removing a target twice is an honest no-op"
        );
    }

    #[test]
    fn the_series_keeps_the_newest_sixty_samples() {
        let mut samples = VecDeque::new();
        for index in 0..SAMPLE_CAP as u64 + 5 {
            push_sample(
                &mut samples,
                Sample {
                    ts: index + 1,
                    ok: true,
                    latency_ms: Some(1),
                    status: None,
                },
            );
        }

        assert_eq!(samples.len(), SAMPLE_CAP);
        assert_eq!(samples.front().expect("a sample").ts, 6);
        assert_eq!(samples.back().expect("a sample").ts, 65);
    }

    #[test]
    fn the_summary_counts_drops_and_the_current_run_start() {
        let mut samples = VecDeque::new();
        for (ts, ok, status) in [
            (1_000, true, None),
            (2_000, true, None),
            (3_000, false, Some(503)),
            (4_000, true, None),
            (5_000, false, Some(200)),
        ] {
            push_sample(
                &mut samples,
                Sample {
                    ts,
                    ok,
                    latency_ms: ok.then_some(5),
                    status,
                },
            );
        }

        let summary = summarize(&samples);
        assert_eq!(summary.up_now, Some(false));
        assert_eq!(
            summary.since_ms,
            Some(5_000),
            "the current down run began at the newest flip"
        );
        assert_eq!(summary.up_pct, Some(60), "three of five probes answered");
        assert_eq!(summary.drops, Some(2), "up→down twice: at 3 s and at 5 s");
        assert_eq!(summary.last_status, Some(200));
    }

    #[test]
    fn a_target_id_names_the_address_and_its_path() {
        let tcp = target_from_spec(
            TargetSpec::tcp("LOCALHOST", 80, MIN_INTERVAL_MS),
            Limits::default(),
        )
        .expect("a valid tcp target");
        assert_eq!(tcp.id, "localhost:80");

        let http = target_from_spec(
            TargetSpec::http("127.0.0.1", 80, "health", None, MIN_INTERVAL_MS),
            Limits::default(),
        )
        .expect("a valid http target");
        assert_eq!(http.id, "127.0.0.1:80/health");
        assert_eq!(http.path.as_deref(), Some("/health"));
    }
}
