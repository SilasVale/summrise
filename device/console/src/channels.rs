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
}

/// The requests waiting for an answer, keyed by the envelope id.
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Answer>>>>;

/// Every live channel, keyed by device id.
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
    /// How the attempt before the current channel ended, per device. It arrives ON the next connect:
    /// a device cannot report a failure over the connection that failed, so the story is one attempt
    /// behind, and the label in the page says so.
    retries: Mutex<HashMap<String, RetryNote>>,
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
            in_flight: Mutex::new(HashMap::new()),
            peak_queued: AtomicUsize::new(0),
            versions: Mutex::new(HashMap::new()),
            names: Mutex::new(HashMap::new()),
            retries: Mutex::new(HashMap::new()),
            hosts: Mutex::new(HashMap::new()),
            changed: Notify::new(),
            send_timeout,
            reply_timeout,
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
        lock(&self.hosts).insert(device_id.to_owned(), facts);
    }

    /// The host facts the device last sent, if it has sent any.
    pub(crate) fn host(&self, device_id: &str) -> Option<serde_json::Value> {
        lock(&self.hosts).get(device_id).cloned()
    }

    /// The version the device announced, if it did.
    pub(crate) fn version(&self, device_id: &str) -> Option<String> {
        lock(&self.versions).get(device_id).cloned()
    }

    /// Whether a channel is open right now.
    pub(crate) fn online(&self, device_id: &str) -> bool {
        lock(&self.live).contains_key(device_id)
    }

    /// Serve one upgraded socket until it closes or is replaced.
    pub(crate) async fn serve(&self, device_id: String, mut socket: WebSocket) {
        let mut open = self.open(&device_id);

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
                    Some(Ok(Message::Text(text))) => {
                        if let Some(facts) = deliver(&open.pending, &text) {
                            self.announce_host(&device_id, facts);
                        }
                    }
                    Some(Err(_)) | None => break,
                    _ => {}
                },
            }
        }

        // Close first, then fail whatever this channel was waiting for. Removing the channel from
        // the registry before clearing its pending set closes the window in which a request could
        // find the dying channel and register a waiter whose sender then nobody drops.
        self.close(&device_id, open.generation);
        lock(&open.pending).clear();
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
    fn open(&self, device_id: &str) -> OpenChannel {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        let (outbound, commands) = mpsc::channel(CHANNEL_CAPACITY);
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        let mut live = lock(&self.live);
        let replaced = live.insert(
            device_id.to_owned(),
            Live {
                generation,
                outbound,
                pending: pending.clone(),
            },
        );
        drop(live);

        if let Some(replaced) = replaced {
            // Fail the old requests now; the old socket is closed by its own task. A full queue
            // means that task is already stuck in a send and will close on its own send bound.
            lock(&replaced.pending).clear();
            let _ = replaced
                .outbound
                .try_send(Outbound::Close("replaced by a new channel"));
        }

        self.changed.notify_waiters();

        OpenChannel {
            generation,
            commands,
            pending,
        }
    }

    /// Forget the channel if it is still the one this task opened.
    fn close(&self, device_id: &str, generation: u64) {
        let mut live = lock(&self.live);
        if live.get(device_id).map(|channel| channel.generation) == Some(generation) {
            live.remove(device_id);
        }
        drop(live);
        self.changed.notify_waiters();
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

/// Hand one inbound text frame to whoever is waiting for it.
///
/// Two kinds arrive on this socket: answers to the console's requests (keyed by envelope id), and the
/// device's own status reports, which have no id and are what the console shows without asking. A
/// status frame comes back to the caller to store; an answer is delivered to its waiter.
fn deliver(
    pending: &Mutex<HashMap<u64, oneshot::Sender<Answer>>>,
    text: &str,
) -> Option<serde_json::Value> {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("summrise-console: device sent a malformed frame: {error}");
            return None;
        }
    };
    if value.get("id").is_none() {
        return value.get("host").cloned();
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

        let _open = registry.open(&device_id());
        assert!(registry.online(&device_id()), "the channel is open");
    }

    #[test]
    fn a_status_frame_is_not_an_answer_and_is_handed_back() {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let stored = deliver(
            &pending,
            r#"{"host":{"machine":"desktop-9vh92bj","sessions":[{"id":2,"state":"disconnected"}]}}"#,
        )
        .expect("a status frame comes back for storage");
        assert_eq!(stored["machine"], serde_json::json!("desktop-9vh92bj"));
        assert!(
            pending.lock().expect("the pending set").is_empty(),
            "a status frame must not be mistaken for an answer"
        );
    }

    #[test]
    fn an_answer_with_no_waiter_is_dropped_without_panicking() {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        assert!(deliver(&pending, r#"{"id":7,"result":{}}"#).is_none());
    }

    #[tokio::test]
    async fn a_socket_that_never_accepts_a_frame_is_a_send_timeout() {
        let registry = ChannelRegistry::with_send_timeout(Duration::from_millis(50));
        // A channel exists, but no serving task ever accepts the frame: the socket is wedged.
        let _open = registry.open(&device_id());

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
        let open = registry.open(&device_id());
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
        let open = registry.open(&device_id());
        let mut commands = open.commands;
        let pending = open.pending.clone();
        let device = tokio::spawn(async move {
            while let Some(Outbound::Text {
                accepted, frame, ..
            }) = commands.recv().await
            {
                let _ = accepted.send(Ok(()));
                let pending = pending.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(120)).await;
                    let id = serde_json::from_str::<serde_json::Value>(&frame)
                        .ok()
                        .and_then(|value| value.get("id").cloned())
                        .unwrap_or(serde_json::Value::Null);
                    let answer =
                        format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"answered":true}}}}"#);
                    let _ = deliver(&pending, &answer);
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
        let _open = registry.open(&device_id());

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
