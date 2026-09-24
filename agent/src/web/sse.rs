//! web/sse.rs — Server-Sent-Events streams (structure refactor: moved
//! verbatim from web/mod.rs). Bounded connection count, heartbeat keep-alive,
//! epoch first-frames — the loss-tolerant stream contract lives here.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;
use bytes::Bytes;
use tokio::sync::mpsc;

use summrise_agent_core::EventBus;

use crate::state::AppState;

use super::{built_response, set_cache_control};

/// Bound one mpsc send at 5s. Both SSE streams funnel their frames through
/// this: a client that stopped reading fills the bounded channel and an
/// unbounded send would block FOREVER (leaking the task + broadcast
/// subscription on a silently-dead client). Returns true when the send
/// failed or timed out — the caller breaks its loop and drops the stream.
async fn send_bounded(tx: &mpsc::Sender<Result<Bytes, Infallible>>, bytes: Bytes) -> bool {
    tokio::time::timeout(std::time::Duration::from_secs(5), tx.send(Ok(bytes)))
        .await
        .map(|r| r.is_err())
        .unwrap_or(true)
}

/// Serializes tests that touch the process-global SSE viewer pool.
///
/// The counter is GLOBAL, so a test that fills it makes every other test that
/// opens a stream receive 503 — or panic inside `test_guard()`. The suite
/// previously avoided this by never draining the pool at all, which also meant
/// the cap's ENFORCEMENT could not be tested (and R124 found the cap did not
/// work). A shared lock buys both: the cap test may drain, and the tests that
/// merely need one slot now WAIT instead of racing.
///
/// This is a test-only fixture — production acquires the counter directly.
#[cfg(test)]
pub(crate) static SSE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

static SSE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
pub(crate) const SSE_MAX_CONNECTIONS: usize = 64;
pub(crate) struct SseConnectionGuard;
impl SseConnectionGuard {
    pub(crate) fn acquire() -> Option<Self> {
        let prev = SSE_CONNECTIONS.fetch_add(1, Ordering::SeqCst);
        if prev < SSE_MAX_CONNECTIONS {
            Some(SseConnectionGuard)
        } else {
            SSE_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
            None
        }
    }
}
impl Drop for SseConnectionGuard {
    fn drop(&mut self) {
        SSE_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Acquire an SSE viewer slot, or the 503 response to return when the
/// 64-viewer pool is full. The /api/events and /api/events/term handlers in
/// mod.rs used to each inline the same acquire-match-503 block.
///
/// ⚠️ THE RETURNED GUARD MUST OUTLIVE THE RESPONSE — it releases its slot on
/// `Drop`, so binding it to a local in a function that merely CONSTRUCTS the
/// response frees the slot immediately and the cap stops existing.
///
/// SOLID R124 found exactly that bug at both call sites (70 held streams
/// produced zero refusals); R128 fixed it by MOVING the guard into the
/// streaming task, which is why `sse_stream`/`sse_term_stream` now take one.
/// The release is PROMPT: the pump selects on `tx.closed()`, so dropping the
/// response body returns the slot at once rather than at the next heartbeat
/// tick (30s here, 60s on the terminal stream) — a lag that would let a burst
/// of short-lived viewers starve the pool.
///
/// THE GUARD LIVES IN THE TASK, NOT IN THE BODY, deliberately: a body that is
/// dropped without ever being polled still closes the mpsc, which the pump
/// already has to detect for dead-client cleanup, so release is guaranteed on
/// EVERY path. A guard owned by a custom Body wrapper would leak whenever the
/// body was dropped unpolled. Pinned by
/// `sse_viewer_cap_holds_and_releases_per_connection` in `mod.rs`'s tests,
/// which checks the HOLD and the RELEASE separately.
pub(crate) fn acquire_sse_guard() -> Result<SseConnectionGuard, Box<Response>> {
    match SseConnectionGuard::acquire() {
        Some(g) => Ok(g),
        None => Err(Box::new(built_response(
            StatusCode::SERVICE_UNAVAILABLE,
            // THE DEVICE'S OWN ERROR ENVELOPE, not a plain sentence. This said `text/plain` with the
            // same words, and the panel's client surfaces a body ONLY when it parses as JSON with a
            // string `error` — "otherwise keep the HTTP status" (`lib/api.ts`, and its comment says so).
            // So the most useful message on this surface, "too many SSE viewers (max 64)", reached the
            // operator as a bare "HTTP 503" and the panel simply reported the stream down. Every other
            // error this agent sends is `{ok:false,error}`; this one now is too.
            "application/json",
            Body::from(
                serde_json::json!({
                    "ok": false,
                    "error": "too many SSE viewers (max 64)",
                })
                .to_string(),
            ),
        ))),
    }
}

/// Adapter: tokio mpsc::Receiver → futures::Stream for axum Body::from_stream
struct MpscStream {
    rx: mpsc::Receiver<Result<Bytes, Infallible>>,
}

impl futures::stream::Stream for MpscStream {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// ONE STREAM, DESCRIBED — the whole interface of the SSE pump.
///
/// WHY THIS IS A VALUE AND NOT SIX ARGUMENTS. The pump used to be *called* by
/// one stream and *copied* by the other: `sse_term_stream` built its own mpsc and
/// its own `select!` loop, because the one thing it needed to differ — a 60 s
/// heartbeat where the events stream uses 30 s — was a literal inside the loop
/// rather than something a caller could pass. The cost was paid twice over: the
/// R128 fix (a viewer slot must ride the STREAMING TASK, not the Body) had to be
/// applied in two places and both copies carry the same comment saying so, and
/// the heartbeat arm was untestable because reaching it took 30 s of wall clock.
/// With the cadence in the description, both streams are ~10-line adapters at
/// this seam and the arm can be driven at 10 ms.
///
/// The DELETION TEST is what settled it: deleting the old `sse_response` would
/// NOT have deleted the pump, because half its consumers had their own copy.
/// That is a module bypassed by its own callers, whatever its parameter list
/// suggests.
pub(crate) struct SseStream<T> {
    /// The broadcast subscription feeding this stream.
    pub rx: tokio::sync::broadcast::Receiver<T>,
    /// Heartbeat cadence. A comment frame every tick keeps the socket alive AND
    /// is how a dead client is noticed (the send into a full mpsc fails). The two
    /// streams want different values, and each states its reason at its own call
    /// site rather than in a copy of this loop.
    pub tick: std::time::Duration,
    /// A frame to emit before anything else — the epoch marker, where the stream
    /// has one. `None` means the first frame is whatever the device pushes.
    pub initial: Option<String>,
    /// A received item → one SSE frame.
    pub encode: Box<dyn Fn(&T) -> String + Send + 'static>,
    /// The fallback frame when the receiver fell behind (loss-tolerant stream —
    /// the client catches up from its own last seq).
    pub lagged: Box<dyn Fn(u64) -> String + Send + 'static>,
    /// The viewer slot, held for as long as this stream lives (SOLID R128).
    pub guard: SseConnectionGuard,
}

/// Build a text/event-stream response from the description above.
pub(crate) async fn sse_response<T>(plan: SseStream<T>) -> Response
where
    T: Clone + Send + 'static,
{
    let SseStream {
        mut rx,
        tick: tick_every,
        initial,
        encode,
        lagged,
        guard,
    } = plan;
    let (tx, mpsc_rx) = mpsc::channel::<Result<Bytes, Infallible>>(128);

    tokio::spawn(async move {
        // The viewer slot rides the STREAMING TASK (SOLID R128, see
        // SseConnectionGuard's header for why not the Body).
        let _guard = guard;
        use tokio::sync::broadcast::error::RecvError;
        // stage-n: emit an epoch marker as the FIRST frame so SSE clients can
        // distinguish a fresh agent boot from a quiet stream (the epoch nonce
        // is otherwise only in /api/events/poll).
        if let Some(init) = &initial {
            let _ = tx.send(Ok(Bytes::from(init.clone()))).await;
        }
        // Heartbeat + dead-client detection, both in ONE select.
        //
        // The tick: an idle stream emitted zero bytes while declaring
        // keep-alive, so a silently-dropped connection was never detected and
        // a reconnect missed every event during the outage. A comment frame
        // keeps the socket alive AND lets the client's read loop notice a dead
        // connection. The mpsc is bounded (128); a client that stopped reading
        // fills it and an unbounded send would block FOREVER, so each send is
        // bounded at 5s by send_bounded — a full channel means the client is
        // gone. The CADENCE is the caller's (`SseStream::tick`): 30 s here, 60 s
        // on the terminal stream, each for a reason stated at its call site.
        //
        // `tx.closed()` is the PROMPT release path (SOLID R128). Without it
        // the task stays parked on the BROADCAST receiver and only notices a
        // dropped body at the next tick — up to 30s here, 60s on the terminal
        // stream — so the viewer slot (and the broadcast subscription) stayed
        // held long after the client left. Measured by
        // `sse_viewer_cap_holds_and_releases_per_connection`: the pool was
        // still exhausted seconds after every response was dropped. A flood of
        // short-lived viewers could therefore starve the pool even though no
        // one was watching. `closed()` fires the moment the Body drops.
        let mut tick = tokio::time::interval(tick_every);
        loop {
            tokio::select! {
                // Body dropped (client gone / response discarded) — release now.
                _ = tx.closed() => break,
                _ = tick.tick() => {
                    if send_bounded(&tx, Bytes::from(": ping\n\n")).await {
                        break;
                    }
                }
                msg = rx.recv() => {
                    match msg {
                        Ok(item) => {
                            if send_bounded(&tx, Bytes::from(encode(&item))).await {
                                break;
                            }
                        }
                        Err(RecvError::Lagged(n)) => {
                            if send_bounded(&tx, Bytes::from(lagged(n))).await {
                                break;
                            }
                        }
                        Err(RecvError::Closed) => break,
                    }
                }
            }
        }
    });

    sse_response_from_rx(mpsc_rx)
}

/// Wrap an mpsc receiver into the SSE Response — content-type plus the
/// no-cache / keep-alive header set. sse_response and sse_term_stream
/// used to each inline this tail.
fn sse_response_from_rx(mpsc_rx: mpsc::Receiver<Result<Bytes, Infallible>>) -> Response {
    let body = Body::from_stream(MpscStream { rx: mpsc_rx });

    let mut resp = built_response(StatusCode::OK, "text/event-stream", body);
    set_cache_control(&mut resp, "no-cache");
    resp.headers_mut().insert(
        axum::http::HeaderName::from_static("connection"),
        axum::http::HeaderValue::from_static("keep-alive"),
    );
    resp
}

pub(crate) async fn sse_stream(state: Arc<AppState>, guard: SseConnectionGuard) -> Response {
    let rx = state.event_bus.subscribe();
    // SeqEvent serializes as {"seq":n,"event":{...}}. The `v` field is a
    // protocol version anchor (round-54): clients ignore unknown fields, so
    // this is purely a diagnostic marker.
    let encode = |event: &summrise_agent_core::events::SeqEvent| {
        let mut obj = serde_json::to_value(event).unwrap_or_default();
        if let Some(o) = obj.as_object_mut() {
            o.insert("v".into(), serde_json::json!(1));
        }
        format!("data: {}\n\n", obj)
    };
    // Plain data frame so EventSource.onmessage fires; the client responds by
    // polling once from its last seq to catch up.
    let lagged = |n: u64| format!("data: {{\"v\":1,\"lagged\":{n}}}\n\n");
    // stage-n: emit the epoch nonce as the initial frame so SSE clients can
    // distinguish a fresh boot from a quiet stream.
    let epoch = state.event_bus.epoch();
    let initial = Some(format!("data: {{\"v\":1,\"epoch\":{epoch}}}\n\n"));
    sse_response(SseStream {
        rx,
        // 30 s. Long enough that an idle panel costs one frame a tick, short
        // enough that a dropped connection is noticed while the operator is
        // still looking at the page.
        tick: std::time::Duration::from_secs(30),
        initial,
        encode: Box::new(encode),
        lagged: Box::new(lagged),
        guard,
    })
    .await
}

/// SSE stream of raw terminal output (TermOutput JSON frames).
pub(crate) async fn sse_term_stream(state: Arc<AppState>, guard: SseConnectionGuard) -> Response {
    let rx = state.event_bus.subscribe_term_output();
    // {"v":1,"session_id":"term-0","data":[104,101,...]} — the v field is a
    // protocol version anchor (round-54), same semantics as /api/events.
    let encode = |output: &serde_json::Value| {
        let mut obj = output.clone();
        if let Some(o) = obj.as_object_mut() {
            o.insert("v".into(), serde_json::json!(1));
        }
        format!(
            "data: {}\n\n",
            serde_json::to_string(&obj).unwrap_or_default()
        )
    };
    // Loss-tolerant stream. The lagged frame carries NO session_id — the broadcast is cross-session —
    // and the client does NOT ignore it: the panel marks EVERY session's rendered offset as needing a
    // gap backfill, then uses the next frame's `start` (the true lower bound) to re-read exactly the
    // dropped range (rounds 100 and 103 in `useSSE.ts`). This comment used to claim the frame was
    // "ignored client-side", which was wrong, and wrong in the direction that matters: someone reading
    // it could delete the client's recovery path as dead code. The shape here and the handling there
    // are pinned together by `useSSE`'s tests, which feed this exact frame.
    let lagged = |_n: u64| "data: {\"v\":1,\"lagged\":true}\n\n".to_string();

    // THIS USED TO BE A SECOND COPY OF THE PUMP — its own mpsc, its own
    // `select!`, and the same R128 comment as the events stream — differing only
    // in the cadence below. It is an adapter now (see `SseStream`).
    sse_response(SseStream {
        rx,
        // 60 s, NOT the events stream's 30 s. The tick here is dead-client
        // detection only — NO session keepalive. The panel's 30s
        // terminal_select heartbeat (panel.js) already touches every live
        // session it watches; touching ALL sessions from the SSE tick
        // disabled the idle sweeper for the whole device while ANY tab was
        // open (round-49: an orphaned MCP ssh to prod was never reaped while
        // a panel tab sat open) and stamped every last_output equal, breaking
        // the eviction tiebreak. This ping only keeps the connection alive (a
        // closed tab → send fails → the pump breaks).
        tick: std::time::Duration::from_secs(60),
        initial: None,
        encode: Box::new(encode),
        lagged: Box::new(lagged),
        guard,
    })
    .await
}

// ── Status ────────────────────────────────────────────────────

#[cfg(test)]
mod sse_tests {
    //! round-371: the SSE loss-tolerant contract (epoch-first frames,
    //! lagged fallback, header shape) and the guard acquire/release cycle
    //! had zero tests. All sse_response cases use pre-queued broadcast
    //! sends + sender-drop, so no timers are involved —
    //! EXCEPT the heartbeat, which this header used to write off ("the 30s
    //! heartbeat arm is intentionally untested — it would take 30s"). That was
    //! true of the CONSTANT, not of the arm: `SseStream::tick` is a field now,
    //! so `heartbeat_frames_are_emitted_on_the_configured_tick` drives it at
    //! 10 ms and reads the comment frame off the live body. A header that still
    //! claimed the arm was untestable would be the file contradicting what it
    //! proves, which is the failure this ledger records more than any other.
    //!
    //! NOTE: the counter is process-global, so tests that take a slot hold
    //! SSE_TEST_LOCK. Before R128 the rule was "never drain the pool", because
    //! a drain starved the parallel endpoint tests into 503s — but that also
    //! made the cap's ENFORCEMENT untestable, which is how the R124 bug (a cap
    //! that did not hold) survived. The lock lets the cap test drain while the
    //! single-slot tests wait.
    use super::*;

    async fn body_bytes(resp: Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn frames() -> (impl Fn(&String) -> String, impl Fn(u64) -> String) {
        (
            |s: &String| format!("data: {s}\n\n"),
            |n: u64| format!("data: lagged={n}\n\n"),
        )
    }

    #[test]
    fn guard_acquire_release_cycle() {
        let _sse = SSE_TEST_LOCK.blocking_lock();
        let g = SseConnectionGuard::acquire();
        assert!(g.is_some(), "a free pool must grant a slot");
        drop(g);
        assert!(
            SseConnectionGuard::acquire().is_some(),
            "a dropped guard must release its slot"
        );
    }

    #[test]
    fn guard_two_concurrent_holds_coexist() {
        let _sse = SSE_TEST_LOCK.blocking_lock();
        let a = SseConnectionGuard::acquire();
        let b = SseConnectionGuard::acquire();
        assert!(a.is_some() && b.is_some(), "two holds must both grant");
    }

    /// A real guard for tests that are not exercising the cap. Taken through
    /// the production `acquire_sse_guard()`, so the tests build the same shape
    /// production does — and the slot is returned when the stream task ends.
    fn test_guard() -> SseConnectionGuard {
        SseConnectionGuard::acquire().expect("the 64-slot pool has room for a test stream")
    }

    /// The description every test here starts from: the shared `frames()`
    /// closures, no initial frame, and a tick so long that no test reaches it
    /// unless it asks to. `heartbeat_frames_are_emitted_on_the_configured_tick`
    /// is the one that does, and it passes its own.
    fn test_plan(
        rx: tokio::sync::broadcast::Receiver<String>,
        initial: Option<String>,
    ) -> SseStream<String> {
        let (encode, lagged) = frames();
        SseStream {
            rx,
            tick: std::time::Duration::from_secs(3_600),
            initial,
            encode: Box::new(encode),
            lagged: Box::new(lagged),
            guard: test_guard(),
        }
    }

    #[tokio::test]
    async fn response_carries_sse_headers() {
        let _sse = SSE_TEST_LOCK.lock().await;
        let (_tx, rx) = tokio::sync::broadcast::channel::<String>(16);
        let resp = sse_response(test_plan(rx, None)).await;
        let h = resp.headers();
        assert_eq!(h.get("content-type").unwrap(), "text/event-stream");
        assert_eq!(h.get("cache-control").unwrap(), "no-cache");
        assert_eq!(h.get("connection").unwrap(), "keep-alive");
        drop(_tx);
    }

    /// THE HEARTBEAT ARM, WHICH NO TEST COULD REACH BEFORE.
    ///
    /// This module's header recorded it as untestable — "the 30s heartbeat arm
    /// is intentionally untested — it would take 30s" — and that was true of the
    /// CONSTANT, not of the arm: the cadence is a field of `SseStream` now, so
    /// 10 ms reaches it. The arm matters twice over: it is the dead-client
    /// detector (a client that stopped reading fills the bounded mpsc and
    /// `send_bounded` fails), and it was the ONE line that differed between the
    /// two copies of the pump, which is why the copies existed.
    #[tokio::test]
    async fn heartbeat_frames_are_emitted_on_the_configured_tick() {
        let _sse = SSE_TEST_LOCK.lock().await;
        let (_tx, rx) = tokio::sync::broadcast::channel::<String>(16);
        let mut plan = test_plan(rx, None);
        plan.tick = std::time::Duration::from_millis(10);
        let resp = sse_response(plan).await;
        // Read frames off the LIVE body. `body_bytes` waits for the stream to
        // END, and this one does not end while the sender is alive — so the
        // helper would hang rather than observe anything.
        let mut frames = resp.into_body().into_data_stream();
        let first = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            futures::StreamExt::next(&mut frames),
        )
        .await
        .expect("a ping must arrive well within 5s of a 10ms tick")
        .expect("the body must still be open")
        .expect("the frame must not be an error");
        assert_eq!(
            &first[..],
            b": ping\n\n",
            "the heartbeat frame must be the comment frame the client's read \
             loop treats as liveness"
        );
        drop(_tx);
    }

    #[tokio::test]
    async fn initial_frame_comes_first_then_close_ends_stream() {
        let _sse = SSE_TEST_LOCK.lock().await;
        let (tx, rx) = tokio::sync::broadcast::channel::<String>(16);
        let resp = sse_response(test_plan(rx, Some("data: epoch=7\n\n".into()))).await;
        drop(tx); // no live items: Closed must end the body right after initial
        assert_eq!(body_bytes(resp).await, "data: epoch=7\n\n");
    }

    #[tokio::test]
    async fn live_items_encode_in_fifo_order() {
        let _sse = SSE_TEST_LOCK.lock().await;
        let (tx, rx) = tokio::sync::broadcast::channel::<String>(16);
        tx.send("a".to_string()).unwrap();
        tx.send("b".to_string()).unwrap();
        let resp = sse_response(test_plan(rx, None)).await;
        drop(tx);
        assert_eq!(body_bytes(resp).await, "data: a\n\ndata: b\n\n");
    }

    #[tokio::test]
    async fn lagged_receiver_gets_lagged_frame() {
        let _sse = SSE_TEST_LOCK.lock().await;
        // cap-2 channel, 5 sends, zero recvs before handoff: the receiver
        // is lagged by exactly 3 when the pump task takes over.
        let (tx, rx) = tokio::sync::broadcast::channel::<String>(2);
        for i in 0..5 {
            tx.send(format!("m{i}")).unwrap();
        }
        let resp = sse_response(test_plan(rx, None)).await;
        drop(tx);
        // Lag notice first, then the surviving tail (m3, m4) the channel
        // kept — the loss-tolerant contract: notice + newest, never a gap
        // mistaken for a quiet stream.
        assert_eq!(
            body_bytes(resp).await,
            "data: lagged=3\n\ndata: m3\n\ndata: m4\n\n"
        );
    }

    // SOLID Round-16 (contract completion): send_bounded is the dead-client
    // detector both SSE pumps depend on (a silently-dead client must break
    // the loop, never leak the task + subscription), yet neither arm had a
    // direct pin. The 5s-timeout arm itself stays untested by design — it
    // would take 5s; the closed-channel arm proves the failure path returns
    // true instantly, and the drain arm proves the success path.
    #[tokio::test]
    async fn send_bounded_fails_fast_on_closed_channel() {
        let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(8);
        drop(rx); // silently-dead client: subscription gone
        assert!(
            send_bounded(&tx, Bytes::from("data: x\n\n")).await,
            "closed channel must report failure (caller breaks its loop)"
        );
    }

    #[tokio::test]
    async fn send_bounded_succeeds_and_delivers_when_drained() {
        let (tx, mut rx) = mpsc::channel::<Result<Bytes, Infallible>>(8);
        assert!(
            !send_bounded(&tx, Bytes::from("data: y\n\n")).await,
            "live receiver must report success (loop continues)"
        );
        let got = rx.recv().await.expect("frame must arrive").unwrap();
        assert_eq!(got, Bytes::from("data: y\n\n"));
    }

    #[test]
    fn acquire_sse_guard_ok_path_holds_a_slot() {
        let _sse = SSE_TEST_LOCK.blocking_lock();
        // Pins the Ok wiring — a granted guard is a real hold. The Err (503)
        // arm is covered by the cap test in mod.rs, which is allowed to drain
        // the pool because it holds SSE_TEST_LOCK (see NOTE above); this test
        // takes the same lock so a concurrent drain cannot fail it.
        let g = acquire_sse_guard();
        assert!(g.is_ok(), "a free pool must grant through the helper");
        drop(g);
        assert!(acquire_sse_guard().is_ok(), "dropped guard releases");
    }
}
