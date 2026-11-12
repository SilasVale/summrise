//! The device channel: the one WebSocket each device keeps to the console.
//!
//! The device dials out, so the console needs no inbound port on the device. A channel is a JSON
//! envelope exchange: the console sends `{"id","mcp":{...},"grant":{...}}`, and the device answers
//! `{"id","result":{...}}` or `{"id","error":{...}}`. The grant travels with every request so the
//! device can re-verify the operator's authorization offline rather than trusting this frame's word
//! for it. The id correlates the answer with the waiter; an id nobody is waiting for is logged and
//! dropped, because a stale or malicious answer must not reach a different request.
//!
//! The console keeps **at most one live channel per device**. A second channel replaces the first:
//! the old socket receives a Close frame naming the reason, and its unanswered requests fail as
//! disconnected. This is what makes a device reconnect after a network change without a stale
//! channel swallowing its answers.
//!
//! **THE TWO BOUNDARIES ARE AUDIT LINES.** A channel that becomes live appends `device.joined`, and
//! one that genuinely ends appends `device.left` — `kind: "device"`, `caller: "console"`, and the
//! event in `tool`. The pair is what `/fleet` subtracts to say how long a machine was away, and it
//! lives in the audit rather than in this registry so the answer survives a console restart between
//! the close and the reopen. A replacement writes only a join: the device stayed up.
//!
//! **Every queue is bounded.** One frame may wait for one socket, but no more than
//! [`CHANNEL_CAPACITY`] may; a producer that finds the queue full gets [`RequestError::Busy`] instead
//! of adding a request body the device may never read. At most [`MAX_IN_FLIGHT_PER_DEVICE`] requests
//! are in flight for one device, counting the wait for a channel. A socket that has not accepted a
//! frame within [`SEND_TIMEOUT`] is wedged: the request gets [`RequestError::SendTimeout`] and the
//! channel is closed rather than allowed to accumulate.
//!
//! A request that arrives while no channel is live (a device between connections, or one whose
//! channel was just replaced) waits up to [`RECONNECT_WAIT`] for the next channel instead of being
//! refused at once. The proxy turns that timeout into a retryable 503, so an operator's client can
//! retry rather than lose the call.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::extract::ws::{close_code, CloseFrame, Message, WebSocket};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot, Notify};

use crate::audit::{self, Audit, Caller};
use crate::grants::GrantEnvelope;

/// What authorizes one request on the wire: the legacy signed grant, or the console's permanent
/// token scope. The device accepts both (`remote::Admitted`); the console sends whichever the
/// caller's credential is.
// The Token arm lands before the /mcp handlers switch to it (both ship together); until then only
// the tests construct it.
pub(crate) enum RequestAuth<'a> {
    Grant(&'a GrantEnvelope),
    Token(&'a TokenScope),
}

impl RequestAuth<'_> {
    /// The tool patterns this request authorizes, in the order the credential named them.
    pub(crate) fn tools(&self) -> &[String] {
        match self {
            RequestAuth::Grant(grant) => &grant.grant.tools,
            RequestAuth::Token(scope) => &scope.scopes,
        }
    }
}

/// A permanent token's scope, as the console forwards it. The device checks the same facts it
/// checks on a grant: device, expiry, tool coverage.
#[derive(Serialize)]
pub(crate) struct TokenScope {
    pub(crate) token_id: String,
    pub(crate) device_id: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) allow_destructive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) expires_at_ms: Option<u64>,
}

/// How often a connected device reports on its channel.
///
/// The device's side is `device/device/src/console.rs`: it sends one host frame every 60 s, and the
/// first tick fires as the socket opens. The console keeps its own copy of that promise instead of
/// reading a value across the wire, so a device build cannot shorten the window it is judged by.
pub(crate) const DEVICE_REPORT_INTERVAL: Duration = Duration::from_secs(60);

/// How recently a frame must have arrived for a channel to count as online.
///
/// 150 s is **2.5 × the device's 60 s report**: two missed reports fit inside it, so one slow report
/// is not an outage, while a machine that has gone to sleep behind a half-open TCP connection stops
/// claiming presence after two and a half minutes. This is the whole of `online`: a socket that
/// exists but has been silent past this window is offline.
pub(crate) const PRESENCE_WINDOW: Duration =
    Duration::from_secs(DEVICE_REPORT_INTERVAL.as_secs() * 5 / 2);

/// How long a request waits for a channel to exist before it is refused as retryable.
pub(crate) const RECONNECT_WAIT: Duration = Duration::from_secs(2);

/// How many outbound frames may wait for one device's socket.
pub(crate) const CHANNEL_CAPACITY: usize = 64;

/// How many requests may be in flight for one device, counting the wait for a channel.
pub(crate) const MAX_IN_FLIGHT_PER_DEVICE: usize = 32;

/// How long one socket has to accept a frame before the channel is treated as wedged.
pub(crate) const SEND_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a device has to answer a frame the socket accepted.
///
/// The console is the operator's proxy, not the device's execution host: a device busy with a PTY or
/// a slow disk must not hang the operator's request for ever. Thirty seconds is the design's bound on
/// the proxied call; the answer is a 504 that says the device may still be working.
pub(crate) const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// What a device can answer a proxied request with.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    /// The device's `result` value.
    Result(Value),
    /// The device's `error` value.
    Error(Value),
}

/// Why a request could not be put on a channel.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RequestError {
    /// No live channel for this device; nothing was written, so a retry is safe.
    Offline,
    /// The frame was written, but the channel closed before an answer arrived. The device may have
    /// executed the request; a blind retry could execute it twice.
    Disconnected,
    /// The device's outbound queue or in-flight cap is full; nothing was written.
    Busy,
    /// The socket did not accept the frame inside [`SEND_TIMEOUT`]; the channel was closed.
    SendTimeout,
    /// The socket accepted the frame, but the device did not answer inside [`REPLY_TIMEOUT`].
    ///
    /// Without this bound the operator's request waits forever behind a device that took the frame
    /// and stopped working — the canary hit exactly that: an `/mcp` call that never returned while
    /// the device's process was wedged. The device may still be working on it, so a blind retry
    /// could execute the request twice; the answer says so rather than pretending it failed cleanly.
    ReplyTimeout,
}

/// The answer a waiter receives: the device's reply, or the reason it will never arrive.
type Answer = Result<Reply, RequestError>;

/// A frame queued for the socket.
enum Outbound {
    Text {
        id: u64,
        frame: String,
        /// Completed once the socket accepts the frame, with the transport error if it does not.
        accepted: oneshot::Sender<Result<(), RequestError>>,
    },
    Close(&'static str),
}

/// One live channel, as the registry sees it.
struct Live {
    generation: u64,
    outbound: mpsc::Sender<Outbound>,
    pending: Pending,
    /// When a frame last arrived on THIS socket, or `None` until one does.
    ///
    /// The clock belongs to the channel, not the device: a replacement starts empty, because the
    /// old socket's last frame is not evidence that the new one is alive.
    last_frame_ms: Option<u64>,
}

/// The requests waiting for an answer, keyed by the envelope id.
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Answer>>>>;

/// Milliseconds since the Unix epoch, for a registry with no console clock of its own.
fn system_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// How the attempt before a device's current one ended, as that device reported it on connecting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RetryNote {
    /// How many attempts had failed before the one that carried this note.
    pub attempts: u8,
    /// The failure in a sentence, or empty when the device sent none.
    pub reason: String,
}

pub(crate) struct ChannelRegistry {
    next_request: AtomicU64,
    next_generation: AtomicU64,
    live: Mutex<HashMap<String, Live>>,
    /// The moment the console last saw any frame from each device, since it started.
    ///
    /// This outlives a channel: closing the socket must not erase when the device was last heard.
    /// It is a record, not a presence claim — [`online`](Self::online) asks the live channel, so a
    /// reopened channel that has sent nothing still reads offline even while this moment is recent.
    last_frames: Mutex<HashMap<String, u64>>,
    /// Requests in flight per device, including the wait for a channel.
    in_flight: Mutex<HashMap<String, usize>>,
    /// The deepest the outbound queues have ever been, for the operator's health route.
    peak_queued: AtomicUsize,
    /// The version each device announced when it opened its channel, for the operator's device list.
    versions: Mutex<HashMap<String, String>>,
    /// The machine name each device announced, for an operator list that reads like a list of machines.
    names: Mutex<HashMap<String, String>>,
    /// The last host facts each device sent over its channel: the machine, its sessions, and where
    /// the per-user helper landed. Kept after a channel closes, because "last known" is what an
    /// operator needs to read a device that is down.
    hosts: Mutex<HashMap<String, serde_json::Value>>,
    /// The last update story each device sent, as the device wrote it: the version it runs, its last
    /// start's verdict and the updater's last answer. A separate map from `hosts` because it is a
    /// separate section of the frame, and the console carries it through unchanged; a device that
    /// has never sent one has no entry, which is what lets the page draw nothing new for it.
    updates: Mutex<HashMap<String, serde_json::Value>>,
    /// How the attempt before the current channel ended, per device. It arrives ON the next connect:
    /// a device cannot report a failure over the connection that failed, so the story is one attempt
    /// behind, and the label in the page says so.
    retries: Mutex<HashMap<String, RetryNote>>,
    /// Where a channel's two boundaries are written, when the console gave the registry a sink.
    ///
    /// **THE OUTAGE IS NOT KEPT HERE.** The registry used to hold the pair in a map, so a console
    /// restarted between a close and the reopen forgot the start and the fleet could not say how long
    /// the machine was gone. The two moments are audit lines ([`Audit`]) now, and the registry's only
    /// job is to write them at the moment each happens.
    audit: Option<Audit>,
    /// The console's clock, so a boundary line carries the same time as the audit's other lines.
    now: std::sync::Arc<dyn Fn() -> u64 + Send + Sync>,
    /// The capability list each device announced. The console offers a terminal stream only to a
    /// device that names `terminal.stream`; an older build would fail to parse the frame and say
    /// nothing, which is a hung stream rather than a fallback.
    caps: Mutex<HashMap<String, Vec<String>>>,
    /// The live terminal subscriptions, by the frame id that opened each one. A `term` frame on the
    /// channel is delivered here and never mistaken for an answer.
    streams: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    /// Wakes a request waiting for a channel to open.
    changed: Notify,
    send_timeout: Duration,
    reply_timeout: Duration,
}

impl ChannelRegistry {
    pub(crate) fn new() -> Self {
        Self::with_send_timeout(SEND_TIMEOUT)
    }

    /// The registry with an injected send bound, so a test can observe a wedged socket without
    /// waiting five seconds.
    pub(crate) fn with_send_timeout(send_timeout: Duration) -> Self {
        Self::with_timeouts(send_timeout, REPLY_TIMEOUT)
    }

    /// The registry with both bounds injected: the socket's accept and the device's answer.
    pub(crate) fn with_timeouts(send_timeout: Duration, reply_timeout: Duration) -> Self {
        Self {
            next_request: AtomicU64::new(0),
            next_generation: AtomicU64::new(0),
            live: Mutex::new(HashMap::new()),
            last_frames: Mutex::new(HashMap::new()),
            in_flight: Mutex::new(HashMap::new()),
            peak_queued: AtomicUsize::new(0),
            versions: Mutex::new(HashMap::new()),
            names: Mutex::new(HashMap::new()),
            retries: Mutex::new(HashMap::new()),
            hosts: Mutex::new(HashMap::new()),
            updates: Mutex::new(HashMap::new()),
            caps: Mutex::new(HashMap::new()),
            streams: Mutex::new(HashMap::new()),
            audit: None,
            now: std::sync::Arc::new(system_now_ms),
            changed: Notify::new(),
            send_timeout,
            reply_timeout,
        }
    }

    /// The registry that records each channel's two boundaries in `audit`, on the console's clock.
    ///
    /// Without one (every unit test that only drives requests) a boundary is not written anywhere:
    /// the registry still works, it just leaves no record a restart can read.
    pub(crate) fn recording(
        audit: Audit,
        now: std::sync::Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        Self {
            audit: Some(audit),
            now,
            ..Self::new()
        }
    }

    /// The deepest any outbound queue has been since the console started.
    pub(crate) fn peak_queued_frames(&self) -> usize {
        self.peak_queued.load(Ordering::Relaxed)
    }

    /// Record the version a device announced on its channel URL.
    ///
    /// The version is metadata the device volunteers; a device that sends none simply has none, which
    /// the operator's list shows as unknown rather than guessing from the install.
    pub(crate) fn announce_version(&self, device_id: &str, version: &str) {
        lock(&self.versions).insert(device_id.to_owned(), version.to_owned());
    }

    /// Record how the attempt before this channel ended.
    pub(crate) fn announce_retry(&self, device_id: &str, note: RetryNote) {
        lock(&self.retries).insert(device_id.to_owned(), note);
    }

    /// How the attempt before the last one ended, if the device has said.
    pub(crate) fn retry(&self, device_id: &str) -> Option<RetryNote> {
        lock(&self.retries).get(device_id).cloned()
    }

    /// Record the name the device announced on its channel URL, when the registry has none.
    pub(crate) fn announce_name(&self, device_id: &str, name: &str) {
        lock(&self.names).insert(device_id.to_owned(), name.to_owned());
    }

    /// The name the device announced, if it did.
    pub(crate) fn name(&self, device_id: &str) -> Option<String> {
        lock(&self.names).get(device_id).cloned()
    }

    /// Record the host facts a device sent on its channel.
    pub(crate) fn announce_host(&self, device_id: &str, facts: serde_json::Value) {
        let caps: Vec<String> = facts
            .get("caps")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        self.announce_caps(device_id, caps);
        lock(&self.hosts).insert(device_id.to_owned(), facts);
    }

    /// The host facts the device last sent, if it has sent any.
    pub(crate) fn host(&self, device_id: &str) -> Option<serde_json::Value> {
        lock(&self.hosts).get(device_id).cloned()
    }

    /// Record the update story a device sent in its host frame.
    ///
    /// The section is opaque here: the console does not rename its fields, fill in a missing one or
    /// decide what an outcome means. It holds the device's own words for the page, and the newest
    /// frame replaces the previous story whole. A frame without the section is not stored as an
    /// empty one — a device that sends nothing must read as "never said", not "said nothing".
    pub(crate) fn announce_update(&self, device_id: &str, update: serde_json::Value) {
        if update.is_object() {
            lock(&self.updates).insert(device_id.to_owned(), update);
        }
    }

    /// The update story the device last sent, if it has sent one.
    pub(crate) fn update(&self, device_id: &str) -> Option<serde_json::Value> {
        lock(&self.updates).get(device_id).cloned()
    }

    /// The version the device announced, if it did.
    pub(crate) fn version(&self, device_id: &str) -> Option<String> {
        lock(&self.versions).get(device_id).cloned()
    }

    /// Record the capabilities a device announced in its host frame.
    pub(crate) fn announce_caps(&self, device_id: &str, caps: Vec<String>) {
        lock(&self.caps).insert(device_id.to_owned(), caps);
    }

    /// Whether the device announced a capability by name.
    pub(crate) fn has_cap(&self, device_id: &str, cap: &str) -> bool {
        lock(&self.caps)
            .get(device_id)
            .is_some_and(|caps| caps.iter().any(|name| name == cap))
    }

    /// Whether a frame arrived on the live channel within [`PRESENCE_WINDOW`].
    ///
    /// **A SOCKET IS A CONNECTION, NOT PRESENCE.** The console used to answer this by asking whether
    /// a channel existed, so a half-open TCP connection to a sleeping machine kept claiming it was
    /// there (measured on DESKTOP-14RJCR8, offline on the tailnet for 7+ minutes while `/fleet`
    /// served `"online": true`). The device reports every [`DEVICE_REPORT_INTERVAL`], so a channel
    /// that has sent nothing for 2.5 reports is offline even though its socket still exists.
    pub(crate) fn online(&self, device_id: &str) -> bool {
        let now = (self.now)();
        let window_ms = PRESENCE_WINDOW.as_millis() as u64;
        lock(&self.live).get(device_id).is_some_and(|channel| {
            channel
                .last_frame_ms
                .is_some_and(|at| now.saturating_sub(at) <= window_ms)
        })
    }

    /// When the console last saw a frame from this device, or `None` when it has seen none since it
    /// started.
    ///
    /// This is the moment `/fleet` serves as `last_frame_ms`, and the page's "seen 7m ago" reads it.
    /// A device the console never heard has no moment, so the key is omitted rather than defaulted
    /// to zero or to the time the socket opened.
    pub(crate) fn last_frame(&self, device_id: &str) -> Option<u64> {
        lock(&self.last_frames).get(device_id).copied()
    }

    /// Note that a frame arrived on the channel `generation` opened.
    ///
    /// Any frame counts — a host report, an answer, a ping — because presence is transport-level:
    /// the device spoke, whatever it said. The generation is the guard: a replaced socket can still
    /// deliver a frame while it closes, and that frame must not prove the replacement alive.
    fn record_frame(&self, device_id: &str, generation: u64) {
        let at = (self.now)();
        let mut live = lock(&self.live);
        let Some(channel) = live.get_mut(device_id) else {
            return;
        };
        if channel.generation != generation {
            return;
        }
        channel.last_frame_ms = Some(at);
        lock(&self.last_frames).insert(device_id.to_owned(), at);
    }

    /// Serve one upgraded socket until it closes or is replaced.
    pub(crate) async fn serve(&self, device_id: String, mut socket: WebSocket) {
        let mut open = self.open(&device_id).await;

        // Subscribe to the device's own journal before serving requests: every session event the
        // device publishes arrives as a `{"evt":...}` frame and is relayed to the page's stream.
        // From 0 replays what the journal still retains; the console does not need a cursor yet.
        let events_id = self.next_request.fetch_add(1, Ordering::SeqCst);
        let subscribe = json!({ "id": events_id, "events": { "from_seq": 0 } }).to_string();
        if socket.send(Message::Text(subscribe)).await.is_err() {
            self.close(&device_id, open.generation, &open.pending).await;
            return;
        }

        loop {
            tokio::select! {
                command = open.commands.recv() => match command {
                    Some(Outbound::Text { id, frame, accepted }) => {
                        // A request that already gave up must not be written to the device late.
                        if !lock(&open.pending).contains_key(&id) {
                            let _ = accepted.send(Err(RequestError::SendTimeout));
                            continue;
                        }
                        match tokio::time::timeout(self.send_timeout, socket.send(Message::Text(frame))).await {
                            Ok(Ok(())) => {
                                let _ = accepted.send(Ok(()));
                            }
                            Ok(Err(_)) => {
                                // Nothing reached the wire: safe to retry on the next channel.
                                let _ = accepted.send(Err(RequestError::Offline));
                                break;
                            }
                            Err(_) => {
                                // A socket that accepted no bytes for the whole bound is wedged.
                                let _ = accepted.send(Err(RequestError::SendTimeout));
                                break;
                            }
                        }
                    }
                    Some(Outbound::Close(reason)) => {
                        let frame = CloseFrame {
                            code: close_code::AGAIN,
                            reason: reason.into(),
                        };
                        let _ = socket.send(Message::Close(Some(frame))).await;
                        break;
                    }
                    None => break,
                },
                incoming = socket.recv() => match incoming {
                    Some(Ok(message)) => {
                        // Every inbound frame is evidence the device is there: the host report, an
                        // answer, a ping. Record it before the content is parsed, so a malformed
                        // frame still proves the socket is alive.
                        self.record_frame(&device_id, open.generation);
                        if let Message::Text(text) = message {
                            if let Some(frame) = deliver(&open.pending, &self.streams, &text) {
                                if let Some(facts) = frame.host {
                                    self.announce_host(&device_id, facts);
                                }
                                if let Some(update) = frame.update {
                                    self.announce_update(&device_id, update);
                                }
                                if let Some(event) = frame.events {
                                    if let Some(audit) = &self.audit {
                                        audit.publish(json!({
                                            "device_event": { "device": device_id, "event": event }
                                        }));
                                    }
                                }
                            }
                        }
                    }
                    Some(Err(_)) | None => break,
                },
            }
        }

        // Close first, then fail whatever this channel was waiting for. Removing the channel from
        // the registry before clearing its pending set closes the window in which a request could
        // find the dying channel and register a waiter whose sender then nobody drops. The close
        // also writes the `device.left` line the fleet needs, after the pending set is cleared so no
        // waiter waits on the audit's disk.
        self.close(&device_id, open.generation, &open.pending).await;
    }

    /// Forward `mcp` and the grant that authorizes it, then wait for the device's own answer.
    ///
    /// The request waits up to [`RECONNECT_WAIT`] for a live channel. Once it is written, it is bound
    /// to the channel that carried it: a replacement does not inherit the pending request, which
    /// therefore fails as [`RequestError::Disconnected`] and is not retried by the console.
    pub(crate) async fn request(
        &self,
        device_id: &str,
        mcp: Value,
        auth: &RequestAuth<'_>,
    ) -> Result<Reply, RequestError> {
        self.request_within(device_id, mcp, auth, self.reply_timeout)
            .await
    }

    /// Forward `mcp` and wait `reply` for the device's answer.
    ///
    /// [`request`](Self::request) uses [`REPLY_TIMEOUT`], which is right for a tool that answers in
    /// milliseconds. `runs.exec` is the exception: it blocks on the device for the command's own
    /// timeout, so the proxy passes the run's wait here — otherwise the console would answer 504 while
    /// the command kept running and its history row was still written.
    pub(crate) async fn request_within(
        &self,
        device_id: &str,
        mcp: Value,
        auth: &RequestAuth<'_>,
        reply: Duration,
    ) -> Result<Reply, RequestError> {
        let _in_flight = InFlight::begin(self, device_id)?;
        let id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let (outbound, pending, receiver) = self
            .wait_for_channel(device_id, id)
            .await
            .ok_or(RequestError::Offline)?;

        let _guard = PendingGuard {
            pending: pending.clone(),
            id,
        };

        let (accepted, accepted_rx) = oneshot::channel();
        let frame = match auth {
            RequestAuth::Grant(grant) => json!({ "id": id, "mcp": mcp, "grant": grant }),
            RequestAuth::Token(scope) => json!({ "id": id, "mcp": mcp, "token": scope }),
        }
        .to_string();
        let queued = outbound.max_capacity().saturating_sub(outbound.capacity());
        match outbound.try_send(Outbound::Text {
            id,
            frame,
            accepted,
        }) {
            Ok(()) => {
                self.peak_queued.fetch_max(queued + 1, Ordering::Relaxed);
            }
            Err(TrySendError::Full(_)) => {
                self.peak_queued.fetch_max(queued, Ordering::Relaxed);
                return Err(RequestError::Busy);
            }
            Err(TrySendError::Closed(_)) => return Err(RequestError::Offline),
        }

        match tokio::time::timeout(self.send_timeout, accepted_rx).await {
            // No frame was accepted inside the bound; the serve loop closes the channel itself.
            Err(_) => return Err(RequestError::SendTimeout),
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => return Err(error),
            // The serving task ended before it could accept the frame.
            Ok(Err(_)) => return Err(RequestError::Disconnected),
        }

        // The answer has its own bound: a device that accepted the frame and then stopped working
        // must not hold the operator's request open forever.
        match tokio::time::timeout(reply, receiver).await {
            Err(_) => Err(RequestError::ReplyTimeout),
            Ok(Ok(answer)) => answer,
            // The sender was dropped: the channel closed without answering.
            Ok(Err(_)) => Err(RequestError::Disconnected),
        }
    }

    /// Open a terminal subscription on a device's channel.
    ///
    /// The frame carries a `terminal.read` as its request: that is what the device authorizes, what
    /// it executes once (the ack is the first read), and what its audit records. What follows is
    /// pushed — `{"term": {...}}` frames with no request id — until the caller drops the receiver
    /// and [`ChannelRegistry::unsubscribe`] stops it.
    pub(crate) async fn subscribe(
        &self,
        device_id: &str,
        session_id: &str,
        from: u64,
        mcp: Value,
        auth: &RequestAuth<'_>,
    ) -> Result<(u64, mpsc::Receiver<Value>, Reply), RequestError> {
        let _in_flight = InFlight::begin(self, device_id)?;
        let id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let (outbound, pending, receiver) = self
            .wait_for_channel(device_id, id)
            .await
            .ok_or(RequestError::Offline)?;
        // The sink exists before the frame is sent: a device may push before the ack is read.
        let (sink, stream) = mpsc::channel::<Value>(CHANNEL_CAPACITY);
        lock(&self.streams).insert(id, sink);
        let guard = StreamSubscription {
            streams: &self.streams,
            id,
            armed: true,
        };
        let _pending = PendingGuard {
            pending: pending.clone(),
            id,
        };
        let (accepted, accepted_rx) = oneshot::channel();
        let frame = match auth {
            RequestAuth::Grant(grant) => json!({
                "id": id,
                "mcp": mcp,
                "subscribe": { "session_id": session_id, "from": from },
                "grant": grant,
            }),
            RequestAuth::Token(scope) => json!({
                "id": id,
                "mcp": mcp,
                "subscribe": { "session_id": session_id, "from": from },
                "token": scope,
            }),
        }
        .to_string();
        match outbound.try_send(Outbound::Text {
            id,
            frame,
            accepted,
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Err(RequestError::Busy),
            Err(TrySendError::Closed(_)) => return Err(RequestError::Offline),
        }
        match tokio::time::timeout(self.send_timeout, accepted_rx).await {
            Err(_) => return Err(RequestError::SendTimeout),
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => return Err(error),
            Ok(Err(_)) => return Err(RequestError::Disconnected),
        }
        match tokio::time::timeout(self.reply_timeout, receiver).await {
            Err(_) => Err(RequestError::ReplyTimeout),
            Ok(Ok(answer)) => answer.map(|ack| {
                guard.disarm();
                (id, stream, ack)
            }),
            Ok(Err(_)) => Err(RequestError::Disconnected),
        }
    }

    /// Stop a terminal subscription: drop its sink and tell the device to stop following.
    ///
    /// It is deliberately not awaited: a closed tab must not wait on a device, and the frames that
    /// race the unsubscribe find no sink and are dropped.
    pub(crate) fn unsubscribe(&self, device_id: &str, sub: u64) {
        lock(&self.streams).remove(&sub);
        let outbound = lock(&self.live)
            .get(device_id)
            .map(|live| live.outbound.clone());
        let Some(outbound) = outbound else {
            return;
        };
        let id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let frame = json!({ "id": id, "unsubscribe": { "sub": sub } }).to_string();
        let (accepted, _dropped) = oneshot::channel();
        let _ = outbound.try_send(Outbound::Text {
            id,
            frame,
            accepted,
        });
    }

    /// Close a device's live channel, because the operator revoked it.
    ///
    /// The socket is asked to close and then removed from the registry, which fails every request
    /// waiting on it. A device that reconnects is refused by the registry's token check: removing the
    /// registration is what makes the revocation stick, this is what makes it immediate.
    pub(crate) fn disconnect(&self, device_id: &str) {
        let removed = lock(&self.live).remove(device_id);
        if let Some(live) = removed {
            // The serve loop treats a full queue as "already wedged" and closes on its own bound.
            let _ = live
                .outbound
                .try_send(Outbound::Close("the operator revoked this device"));
            for (_, sender) in lock(&live.pending).drain() {
                let _ = sender.send(Err(RequestError::Offline));
            }
        }
        lock(&self.versions).remove(device_id);
        lock(&self.names).remove(device_id);
        // A removed device's last-seen moment belonged to a registration that no longer exists; a
        // later registration under the same key must not inherit it.
        lock(&self.last_frames).remove(device_id);
    }

    /// Wait for a live channel and claim a waiter slot on it, or `None` when the bound expires.
    ///
    /// The sender is inserted while the live map is held, so a channel cannot be replaced between
    /// "found" and "waiting on it": either the request is registered before the replacement clears
    /// the pending set, or it finds the replacement.
    async fn wait_for_channel(
        &self,
        device_id: &str,
        id: u64,
    ) -> Option<(mpsc::Sender<Outbound>, Pending, oneshot::Receiver<Answer>)> {
        let deadline = tokio::time::Instant::now() + RECONNECT_WAIT;

        loop {
            // Interest is registered before the map is read: a channel that opens between the read
            // and the await must still wake this waiter. `notify_waiters` does not store a permit.
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            {
                let live = lock(&self.live);
                if let Some(channel) = live.get(device_id) {
                    let (sender, receiver) = oneshot::channel();
                    lock(&channel.pending).insert(id, sender);
                    return Some((channel.outbound.clone(), channel.pending.clone(), receiver));
                }
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            if tokio::time::timeout(remaining, notified).await.is_err() {
                return None;
            }
        }
    }

    /// Register a socket as the device's live channel, replacing and closing any predecessor.
    ///
    /// **THE JOIN IS A LINE, NOT A MEMORY.** The moment this channel becomes the live one is what
    /// the fleet subtracts the previous `device.left` from, so it is recorded where a restart can
    /// read it. A replacement writes a second `device.joined` with no `device.left` between: the
    /// device was up the whole time, and the derivation reads that as no absence.
    async fn open(&self, device_id: &str) -> OpenChannel {
        let joined_at = (self.now)();
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        let (outbound, commands) = mpsc::channel(CHANNEL_CAPACITY);
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        let replaced = {
            let mut live = lock(&self.live);
            live.insert(
                device_id.to_owned(),
                Live {
                    generation,
                    outbound,
                    pending: pending.clone(),
                    last_frame_ms: None,
                },
            )
        };

        if let Some(replaced) = replaced {
            // Fail the old requests now; the old socket is closed by its own task. A full queue
            // means that task is already stuck in a send and will close on its own send bound.
            lock(&replaced.pending).clear();
            let _ = replaced
                .outbound
                .try_send(Outbound::Close("replaced by a new channel"));
        }

        self.changed.notify_waiters();
        self.record_boundary(device_id, audit::DEVICE_JOINED, joined_at)
            .await;

        OpenChannel {
            generation,
            commands,
            pending,
        }
    }

    /// Forget the channel if it is still the one this task opened, and record that it left.
    ///
    /// A replaced channel never reaches the record: its generation is not the live one any more, and
    /// the replacement already announced the join that says the device stayed up.
    async fn close(&self, device_id: &str, generation: u64, pending: &Pending) {
        let left_at = (self.now)();
        let closed = {
            let mut live = lock(&self.live);
            let closed = live.get(device_id).map(|channel| channel.generation) == Some(generation);
            if closed {
                live.remove(device_id);
            }
            closed
        };
        self.changed.notify_waiters();
        // Remove the channel before clearing its waiters, so a request cannot find the dying channel
        // and register a waiter nobody will drop. The audit write comes after the waiters are told.
        lock(pending).clear();
        if closed {
            self.record_boundary(device_id, audit::DEVICE_LEFT, left_at)
                .await;
        }
    }

    /// Append one channel boundary to the audit, when the console gave the registry a sink.
    ///
    /// `kind: "device"` (not `mcp`), `caller: "console"` and the event in `tool`: the page's
    /// client-call rule sees the console and never counts it, and the timeline drops the marker.
    async fn record_boundary(&self, device_id: &str, tool: &'static str, ts_ms: u64) {
        let Some(audit) = &self.audit else {
            return;
        };
        audit
            .append(&audit::Entry {
                ts_ms,
                kind: "device",
                device_id: Some(device_id),
                status: 200,
                detail: None,
                tool: Some(tool),
                token_id: None,
                caller: Some(Caller::Console),
            })
            .await;
    }
}

/// Counts one device's in-flight requests and releases the slot when dropped.
struct InFlight<'a> {
    registry: &'a ChannelRegistry,
    device_id: String,
}

impl<'a> InFlight<'a> {
    fn begin(registry: &'a ChannelRegistry, device_id: &str) -> Result<Self, RequestError> {
        let mut counts = lock(&registry.in_flight);
        let count = counts.entry(device_id.to_owned()).or_insert(0);
        if *count >= MAX_IN_FLIGHT_PER_DEVICE {
            return Err(RequestError::Busy);
        }
        *count += 1;
        Ok(Self {
            registry,
            device_id: device_id.to_owned(),
        })
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let mut counts = lock(&self.registry.in_flight);
        let empty = match counts.get_mut(&self.device_id) {
            Some(count) => {
                *count -= 1;
                *count == 0
            }
            None => false,
        };
        if empty {
            counts.remove(&self.device_id);
        }
    }
}

/// The receiver half a serving task keeps while its socket is live.
struct OpenChannel {
    generation: u64,
    commands: mpsc::Receiver<Outbound>,
    pending: Pending,
}

/// Removes a timed-out (or disconnected) request from the pending map. Without it, every timeout
/// would leave a sender behind until the channel closed.
struct PendingGuard {
    pending: Pending,
    id: u64,
}

/// Removes a subscription's sink when the subscription is not handed to its caller.
struct StreamSubscription<'a> {
    streams: &'a Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    id: u64,
    armed: bool,
}

impl<'a> StreamSubscription<'a> {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for StreamSubscription<'_> {
    fn drop(&mut self) {
        if self.armed {
            lock(self.streams).remove(&self.id);
        }
    }
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        lock(&self.pending).remove(&self.id);
    }
}

/// The fields of a device answer. `untagged` makes `{"id","result"}` and `{"id","error"}` the two
/// accepted shapes; a frame with neither is malformed and logged.
#[derive(Deserialize)]
struct DeviceFrame {
    id: u64,
    #[serde(flatten)]
    body: DeviceBody,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DeviceBody {
    Result { result: Value },
    Error { error: Value },
}

/// The two sections an unsolicited status frame may carry: the machine facts and the device's own
/// update story. Either may be absent; a frame with neither is simply not stored.
struct Unsolicited {
    host: Option<serde_json::Value>,
    update: Option<serde_json::Value>,
    /// A pushed event from the device's own journal, relayed to the page's stream.
    events: Option<serde_json::Value>,
}

/// Hand one inbound text frame to whoever is waiting for it.
///
/// Two kinds arrive on this socket: answers to the console's requests (keyed by envelope id), and the
/// device's own status reports, which have no id and are what the console shows without asking. A
/// status frame comes back to the caller to store; an answer is delivered to its waiter.
fn deliver(
    pending: &Mutex<HashMap<u64, oneshot::Sender<Answer>>>,
    streams: &Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    text: &str,
) -> Option<Unsolicited> {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("summrise-console: device sent a malformed frame: {error}");
            return None;
        }
    };
    // A pushed terminal frame is neither an answer nor a host report: it belongs to the
    // subscription it names, and a subscriber that is gone drops it (the sender is removed, so the
    // next frame is not even tried).
    if let Some(term) = value.get("term") {
        if let Some(sub) = term.get("sub").and_then(Value::as_u64) {
            let mut sinks = lock(streams);
            match sinks.get(&sub) {
                Some(sink) if sink.try_send(term.clone()).is_ok() => {}
                Some(_) => {
                    sinks.remove(&sub);
                }
                None => {}
            }
        }
        return None;
    }
    if let Some(event) = value.get("evt") {
        return Some(Unsolicited {
            host: None,
            update: None,
            events: Some(event.clone()),
        });
    }
    if value.get("id").is_none() {
        return Some(Unsolicited {
            host: value.get("host").cloned(),
            update: value.get("update").cloned(),
            events: None,
        });
    }
    let frame: DeviceFrame = match serde_json::from_value(value) {
        Ok(frame) => frame,
        Err(error) => {
            eprintln!("summrise-console: device sent a malformed frame: {error}");
            return None;
        }
    };

    let reply = match frame.body {
        DeviceBody::Result { result } => Reply::Result(result),
        DeviceBody::Error { error } => Reply::Error(error),
    };

    match lock(pending).remove(&frame.id) {
        Some(sender) => {
            let _ = sender.send(Ok(reply));
        }
        None => {
            eprintln!(
                "summrise-console: device answered unknown request id {}",
                frame.id
            );
        }
    }
    None
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grants::Grant;

    fn grant() -> GrantEnvelope {
        GrantEnvelope {
            grant: Grant {
                device_id: "ab".repeat(32),
                tools: vec!["files.read".to_owned()],
                expires_at_ms: u64::MAX,
                allow_destructive: false,
                nonce: String::new(),
            },
            signature: "00".repeat(64),
        }
    }

    fn device_id() -> String {
        "ab".repeat(32)
    }

    /// A registry whose clock the test moves by hand, so presence is a number rather than a sleep.
    fn registry_with_clock(start_ms: u64) -> (ChannelRegistry, Arc<AtomicU64>) {
        let clock = Arc::new(AtomicU64::new(start_ms));
        let reader = clock.clone();
        let mut registry = ChannelRegistry::new();
        registry.now = Arc::new(move || reader.load(Ordering::Relaxed));
        (registry, clock)
    }

    /// The unit-level truth the fleet row reads: a socket alone is not an observation.
    #[tokio::test]
    async fn a_socket_alone_is_not_presence() {
        let (registry, _clock) = registry_with_clock(1_000_000);
        let id = device_id();
        let _open = registry.open(&id).await;

        assert!(!registry.online(&id), "an open channel is not a frame");
        assert_eq!(
            registry.last_frame(&id),
            None,
            "the console has seen nothing, so it has no moment to serve"
        );
    }

    /// A frame ten seconds ago is inside the window, and the moment it arrived is what `/fleet`
    /// would serve.
    #[tokio::test]
    async fn a_frame_ten_seconds_ago_is_online() {
        let (registry, clock) = registry_with_clock(1_000_000);
        let id = device_id();
        let open = registry.open(&id).await;
        registry.record_frame(&id, open.generation);

        clock.fetch_add(10_000, Ordering::Relaxed);
        assert!(
            registry.online(&id),
            "10 s is inside the {PRESENCE_WINDOW:?} window"
        );
        assert_eq!(registry.last_frame(&id), Some(1_000_000));
    }

    /// Past the window the channel is offline, and the moment is left where it was rather than
    /// moved to now: the console saw the frame then, not later.
    #[tokio::test]
    async fn a_channel_silent_for_two_hundred_seconds_is_offline_with_the_same_moment() {
        let (registry, clock) = registry_with_clock(1_000_000);
        let id = device_id();
        let open = registry.open(&id).await;
        registry.record_frame(&id, open.generation);

        clock.fetch_add(200_000, Ordering::Relaxed);
        assert!(!registry.online(&id), "200 s is past the window");
        assert_eq!(
            registry.last_frame(&id),
            Some(1_000_000),
            "the last moment is unchanged"
        );
    }

    /// A replacement starts with an empty presence clock: the old socket's frame is not evidence
    /// that the new one is alive. The console's last-seen moment survives the close, because it
    /// happened.
    #[tokio::test]
    async fn a_reopened_channel_has_no_presence_until_a_frame() {
        let (registry, _clock) = registry_with_clock(1_000_000);
        let id = device_id();
        let first = registry.open(&id).await;
        registry.record_frame(&id, first.generation);
        assert!(registry.online(&id));

        let second = registry.open(&id).await;
        assert!(
            !registry.online(&id),
            "the new channel has received no frame yet"
        );
        assert_eq!(
            registry.last_frame(&id),
            Some(1_000_000),
            "the moment the console saw is a fact about the past"
        );
        // A frame from the replaced socket must not count for the replacement either.
        registry.record_frame(&id, first.generation);
        assert!(!registry.online(&id), "a stale generation is not presence");

        registry.record_frame(&id, second.generation);
        assert!(registry.online(&id), "the new channel's own frame is");
    }

    /// The window is derived from the device's report cadence rather than chosen independently.
    #[test]
    fn the_presence_window_is_two_and_a_half_host_reports() {
        assert_eq!(PRESENCE_WINDOW, DEVICE_REPORT_INTERVAL * 5 / 2);
        assert_eq!(PRESENCE_WINDOW, Duration::from_secs(150));
    }

    /// What the operator's device list reads: a channel that is open now, and what the device said
    /// about itself on the way in.
    #[tokio::test]
    async fn the_registry_reports_online_and_what_the_device_announced() {
        let registry = ChannelRegistry::new();
        assert!(!registry.online(&device_id()), "no channel yet");
        assert_eq!(registry.version(&device_id()), None);
        assert_eq!(registry.name(&device_id()), None);

        registry.announce_version(&device_id(), "0.3.11");
        registry.announce_name(&device_id(), "desktop-9vh92bj");
        assert_eq!(registry.version(&device_id()).as_deref(), Some("0.3.11"));
        assert_eq!(
            registry.name(&device_id()).as_deref(),
            Some("desktop-9vh92bj")
        );

        let open = registry.open(&device_id()).await;
        assert!(
            !registry.online(&device_id()),
            "a channel that has sent no frame is not presence"
        );
        registry.record_frame(&device_id(), open.generation);
        assert!(
            registry.online(&device_id()),
            "the frame is what makes it online"
        );
    }

    #[test]
    fn a_status_frame_is_not_an_answer_and_is_handed_back() {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let streams: Mutex<HashMap<u64, mpsc::Sender<Value>>> = Mutex::new(HashMap::new());
        let stored = deliver(
            &pending,
            &streams,
            r#"{"host":{"machine":"desktop-9vh92bj","sessions":[{"id":2,"state":"disconnected"}]}}"#,
        )
        .expect("a status frame comes back for storage");
        assert_eq!(
            stored.host.as_ref().map(|host| &host["machine"]),
            Some(&serde_json::json!("desktop-9vh92bj"))
        );
        assert!(
            stored.update.is_none(),
            "this frame carried no update story"
        );
        assert!(
            pending.lock().expect("the pending set").is_empty(),
            "a status frame must not be mistaken for an answer"
        );
    }

    /// A pushed journal event is carried back to the stream, never mistaken for an answer.
    #[test]
    fn a_pushed_event_comes_back_for_the_stream() {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let streams: Mutex<HashMap<u64, mpsc::Sender<Value>>> = Mutex::new(HashMap::new());
        let frame = deliver(
            &pending,
            &streams,
            r#"{"evt":{"sub":3,"event":{"v":1,"seq":5,"type":"session.opened"}}}"#,
        )
        .expect("a pushed event comes back for the stream");
        let event = frame.events.expect("the event section is carried");
        assert_eq!(event["sub"], 3);
        assert_eq!(event["event"]["seq"], 5);
    }

    /// The update section has its own slot on the stored frame. It is not folded into the host facts
    /// and it is not composed here: a device that sends both keeps them apart on the way through.
    #[test]
    fn a_status_frames_update_section_comes_back_beside_its_host_facts() {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let streams: Mutex<HashMap<u64, mpsc::Sender<Value>>> = Mutex::new(HashMap::new());
        let stored = deliver(
            &pending,
            &streams,
            concat!(
                r#"{"host":{"machine":"workshop"},"#,
                r#""update":{"current":"0.3.275","verdict":{"outcome":"ok","detail":"started 0.3.275"}}}"#,
            ),
        )
        .expect("a status frame comes back for storage");
        assert_eq!(
            stored.host.as_ref().map(|host| &host["machine"]),
            Some(&serde_json::json!("workshop"))
        );
        assert_eq!(
            stored.update.as_ref().map(|update| &update["current"]),
            Some(&serde_json::json!("0.3.275"))
        );
    }

    #[test]
    fn an_answer_with_no_waiter_is_dropped_without_panicking() {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let streams: Mutex<HashMap<u64, mpsc::Sender<Value>>> = Mutex::new(HashMap::new());
        assert!(deliver(&pending, &streams, r#"{"id":7,"result":{}}"#).is_none());
    }

    #[tokio::test]
    async fn a_socket_that_never_accepts_a_frame_is_a_send_timeout() {
        let registry = ChannelRegistry::with_send_timeout(Duration::from_millis(50));
        // A channel exists, but no serving task ever accepts the frame: the socket is wedged.
        let _open = registry.open(&device_id()).await;

        let result = registry
            .request(
                &device_id(),
                json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
                &RequestAuth::Grant(&grant()),
            )
            .await;

        assert_eq!(result, Err(RequestError::SendTimeout));
        assert_eq!(
            registry.peak_queued_frames(),
            1,
            "one frame waited for the wedged socket: the queue is bounded, not unbounded"
        );
    }

    /// The canary's wedged device: the frame is accepted, the answer never arrives, and the
    /// operator's request waited for ever. The bound turns that into a 504 that says the device may
    /// still be working on it (a blind retry could execute twice).
    #[tokio::test]
    async fn a_device_that_accepts_and_never_answers_is_a_reply_timeout() {
        let registry = Arc::new(ChannelRegistry::with_timeouts(
            Duration::from_millis(50),
            Duration::from_millis(50),
        ));
        let open = registry.open(&device_id()).await;
        let commands = open.commands;
        // A device that accepts every frame and answers none.
        let acceptor = tokio::spawn(async move {
            let mut commands = commands;
            while let Some(Outbound::Text { accepted, .. }) = commands.recv().await {
                let _ = accepted.send(Ok(()));
            }
        });

        let result = registry
            .request(
                &device_id(),
                json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
                &RequestAuth::Grant(&grant()),
            )
            .await;

        assert_eq!(result, Err(RequestError::ReplyTimeout));
        acceptor.abort();
    }

    /// A run's channel wait follows the run, not the general answer bound. Without this, a command
    /// that outlives [`REPLY_TIMEOUT`] would be answered 504 by the channel while the device was still
    /// running it — the exact case the Runs page's timeout field made visible.
    #[tokio::test]
    async fn a_run_waits_past_the_general_reply_bound() {
        // The general bound is 20 ms; this device answers after 120 ms.
        let registry = Arc::new(ChannelRegistry::with_timeouts(
            Duration::from_millis(50),
            Duration::from_millis(20),
        ));
        let open = registry.open(&device_id()).await;
        let mut commands = open.commands;
        let pending = open.pending.clone();
        let streams: Arc<Mutex<HashMap<u64, mpsc::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let device = tokio::spawn(async move {
            while let Some(Outbound::Text {
                accepted, frame, ..
            }) = commands.recv().await
            {
                let _ = accepted.send(Ok(()));
                let pending = pending.clone();
                let streams = Arc::clone(&streams);
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(120)).await;
                    let id = serde_json::from_str::<serde_json::Value>(&frame)
                        .ok()
                        .and_then(|value| value.get("id").cloned())
                        .unwrap_or(serde_json::Value::Null);
                    let answer =
                        format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"answered":true}}}}"#);
                    let _ = deliver(&pending, &streams, &answer);
                });
            }
        });

        // The general bound gives up first.
        let general = registry
            .request(
                &device_id(),
                json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
                &RequestAuth::Grant(&grant()),
            )
            .await;
        assert_eq!(general, Err(RequestError::ReplyTimeout));

        // A run's own wait lets the same device answer.
        let run = registry
            .request_within(
                &device_id(),
                json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call" }),
                &RequestAuth::Grant(&grant()),
                Duration::from_millis(500),
            )
            .await;
        assert!(
            matches!(run, Ok(Reply::Result(ref value)) if value["answered"] == json!(true)),
            "the run's wait reads the late answer: {run:?}"
        );
        device.abort();
    }

    #[tokio::test]
    async fn the_in_flight_cap_refuses_rather_than_grows() {
        let registry = Arc::new(ChannelRegistry::with_send_timeout(Duration::from_millis(
            100,
        )));
        let _open = registry.open(&device_id()).await;

        let mut calls = Vec::new();
        for _ in 0..MAX_IN_FLIGHT_PER_DEVICE {
            let registry = registry.clone();
            calls.push(tokio::spawn(async move {
                registry
                    .request(
                        &device_id(),
                        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
                        &RequestAuth::Grant(&grant()),
                    )
                    .await
            }));
        }
        // Wait until every spawned request has claimed its slot; a fixed sleep would make the
        // assertion depend on the scheduler.
        for _ in 0..1_000 {
            let in_flight = *lock(&registry.in_flight).get(&device_id()).unwrap_or(&0);
            if in_flight == MAX_IN_FLIGHT_PER_DEVICE {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(
            *lock(&registry.in_flight).get(&device_id()).unwrap_or(&0),
            MAX_IN_FLIGHT_PER_DEVICE,
            "all slots are held before the overflow is attempted"
        );

        let overflow = registry
            .request(
                &device_id(),
                json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
                &RequestAuth::Grant(&grant()),
            )
            .await;
        assert_eq!(
            overflow,
            Err(RequestError::Busy),
            "the request past the cap is refused, never queued"
        );

        for call in calls {
            call.abort();
        }
    }
}
