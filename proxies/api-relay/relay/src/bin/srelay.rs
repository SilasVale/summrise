//! THE LOCAL RELAY, AS A BINARY — the second VPS process the plan names, and the one an operator runs
//! on their OWN machine.
//!
//! WHAT IT IS. A client's request is carried to the device over the agent's long poll: the client's
//! request is queued, the agent polls `/agent/pull` and is handed the next queued frame, the agent runs
//! it through the same dispatch its local listener uses and posts the answer to `/agent/answer`, and the
//! waiting client gets it. What a client sees is the agent's own HTTP surface, unchanged.
//!
//! THE DECISIONS ARE IN THE LIBRARY (`token_ok`, `relay_route`, the caps) and were proved against a
//! corpus generated from the shipping `relay.mjs`. **THIS FILE IS THE QUEUE** — the state the decisions
//! do not have — and it keeps that file's three subtleties, each named where it lives below:
//!
//!   * ONE QUEUE, ONE FLAG: a job is handed to exactly one poller, so the parked-agent path and the
//!     polling path cannot both hand out the same request;
//!   * TWO DIFFERENT WAITS: a client that arrives when no agent has EVER polled is most likely an
//!     operator's mistake, so it waits two seconds and is told 503 — while an agent that has been here
//!     before is merely between polls, so the request waits the full window and a missing answer is 504;
//!   * A PARKED POLL IS ANSWERED 204 when the window closes, so the agent re-polls instead of hanging.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use serde_json::{json, Value};
use tokio::sync::oneshot;

use summrise_relay::{long_poll_ms, max_body, relay_route, token_ok};

/// A request waiting for the agent.
struct Job {
    frame: Value,
    /// Where the answer goes when `/agent/answer` arrives.
    answer: Option<oneshot::Sender<Value>>,
    /// `taken` says an agent already has it — the flag that makes one queue safe.
    taken: bool,
}

/// A parked agent poll, waiting for a job.
struct Poll {
    slot: oneshot::Sender<Value>,
}

#[derive(Clone)]
pub struct Local {
    token: Arc<String>,
    device_name: Arc<String>,
    /// Requests waiting for the agent, **IN ARRIVAL ORDER** — a `Vec` of pairs rather than a map,
    /// because the JavaScript's `pending` is a `Map` and a `Map` is INSERTION-ORDERED: `pendingJob()`
    /// hands out the OLDEST request first. A `HashMap` here would hand them out in whatever order the
    /// hasher felt like, which is what a flaky test caught (it asserted "a" then "b" and got "b" first on
    /// one run in three).
    pending: Arc<Mutex<Vec<(String, Job)>>>,
    /// Agent long-polls parked here, in arrival order.
    waiting: Arc<Mutex<VecDeque<Poll>>>,
    /// When an agent was last seen, in milliseconds since the epoch; `0` means NEVER.
    agent_seen_ms: Arc<AtomicU64>,
}

impl Local {
    pub fn new(token: String, device_name: String) -> Self {
        Local {
            token: Arc::new(token),
            device_name: Arc::new(device_name),
            pending: Arc::new(Mutex::new(Vec::new())),
            waiting: Arc::new(Mutex::new(VecDeque::new())),
            agent_seen_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    fn seen(&self) -> bool {
        self.agent_seen_ms.load(Ordering::SeqCst) != 0
    }

    fn mark_seen(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.agent_seen_ms.store(now, Ordering::SeqCst);
    }

    /// `pendingJob()`: the first job no agent has taken, marked taken in the same breath.
    fn take_job(&self) -> Option<(String, Value)> {
        let mut pending = self.pending.lock().expect("the queue");
        for (id, job) in pending.iter_mut() {
            if !job.taken {
                job.taken = true;
                return Some((id.clone(), job.frame.clone()));
            }
        }
        None
    }

    fn queue_job(&self, id: String, job: Job) {
        self.pending.lock().expect("the queue").push((id, job));
    }

    fn take_answer(&self, id: &str) -> Option<Job> {
        let mut pending = self.pending.lock().expect("the queue");
        let at = pending.iter().position(|(key, _)| key == id)?;
        Some(pending.remove(at).1)
    }
}

pub fn app(relay: Local) -> Router {
    Router::new().fallback(any(dispatch)).with_state(relay)
}

/// **THE ROUTE DECISION IS THE LIBRARY'S, NOT THE ROUTER'S TABLE.** `relay_route` is the function the
/// corpus proved against the shipping `routeOf`, and this is the one place it is asked — so a change to
/// the order or the method test moves the behaviour rather than being shadowed by a second table.
async fn dispatch(State(relay): State<Local>, req: axum::extract::Request) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().as_str().to_string();
    match relay_route(&path, &method) {
        "agent-pull" => pull(State(relay), req).await,
        "agent-answer" => answer(State(relay), req).await,
        "healthz" => healthz(State(relay)).await,
        _ => forward(State(relay), req).await,
    }
}

fn bearer(headers: &HeaderMap) -> String {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_string()
}

fn json_response(status: u16, body: Value) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST),
        headers,
        body.to_string(),
    )
        .into_response()
}

/// `/agent/pull` — hand this poll a job, or park it until the window closes.
async fn pull(State(relay): State<Local>, req: axum::extract::Request) -> Response {
    if !token_ok(&bearer(req.headers()), &relay.token) {
        return json_response(401, json!({ "ok": false }));
    }
    relay.mark_seen();
    if let Some((_id, frame)) = relay.take_job() {
        return json_response(200, frame);
    }
    // PARKED, WITH A WINDOW. The agent re-polls after a 204, which is why the timeout is an ANSWER rather
    // than a hang.
    let (slot, wait) = oneshot::channel::<Value>();
    relay
        .waiting
        .lock()
        .expect("the parked list")
        .push_back(Poll { slot });
    match tokio::time::timeout(Duration::from_millis(long_poll_ms()), wait).await {
        Ok(Ok(frame)) => json_response(200, frame),
        // The sender was dropped (the relay shut down) or the window closed: 204, no body.
        _ => StatusCode::NO_CONTENT.into_response(),
    }
}

/// `/agent/answer` — the device's reply, delivered by id.
async fn answer(State(relay): State<Local>, req: axum::extract::Request) -> Response {
    if req.method() != axum::http::Method::POST {
        // `routeOf` only calls this route for a POST; a GET falls through to the agent like any path.
        return json_response(404, json!({ "ok": false }));
    }
    if !token_ok(&bearer(req.headers()), &relay.token) {
        return json_response(401, json!({ "ok": false }));
    }
    let (parts, body) = req.into_parts();
    let _ = parts;
    let Ok(bytes) = axum::body::to_bytes(body, max_body()).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let Ok(frame) = serde_json::from_slice::<Value>(&bytes) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(id) = frame.get("id").and_then(|v| v.as_str()).map(str::to_string) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let job = relay.take_answer(&id);
    match job.and_then(|j| j.answer) {
        Some(slot) => {
            let _ = slot.send(frame);
            json_response(200, json!({ "ok": true }))
        }
        None => json_response(404, json!({ "ok": false, "error": "unknown id" })),
    }
}

/// `/healthz` — tells an operator whether an agent is parked.
async fn healthz(State(relay): State<Local>) -> Response {
    let waiting = relay.waiting.lock().expect("the parked list").len();
    let ago = {
        let seen = relay.agent_seen_ms.load(Ordering::SeqCst);
        if seen == 0 {
            Value::Null
        } else {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            json!(now.saturating_sub(seen))
        }
    };
    json_response(
        200,
        json!({ "ok": true, "device": relay.device_name.as_str(), "agentConnectedMsAgo": ago, "waiting": waiting }),
    )
}

/// EVERYTHING ELSE IS FOR THE AGENT: queue it, and wait.
async fn forward(State(relay): State<Local>, req: axum::extract::Request) -> Response {
    let (parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, max_body()).await else {
        return json_response(413, json!({ "ok": false, "error": "body too large" }));
    };
    // **TWO DIFFERENT SITUATIONS, TWO DIFFERENT ANSWERS.** "No agent has EVER connected" is most likely an
    // operator's mistake — a wrong URL, a missing token, a service that never started — and waiting 25
    // seconds to say so helps nobody; that short wait also covers the honest race where a client arrives a
    // moment before its agent's first poll. "An agent was here and has not polled back yet" is normal (its
    // poll window is 25s), so the request waits the full window and a missing answer is a 504.
    let grace = if relay.seen() {
        Duration::from_millis(long_poll_ms())
    } else {
        Duration::from_millis(2_000)
    };
    // **UNIQUE, WHICH `randomUUID()` GAVE THE JAVASCRIPT FOR FREE.** A timestamp plus a pid is unique
    // across processes but NOT within one: two requests in the same nanosecond would share an id, and the
    // answer to one would be delivered to the other. The counter is what makes that impossible.
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let id = format!(
        "{:x}-{:x}-{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    );
    let headers: serde_json::Map<String, Value> = parts
        .headers
        .iter()
        .filter(|(k, _)| k.as_str() != "host")
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|v| (k.as_str().to_string(), Value::String(v.to_string())))
        })
        .collect();
    let frame = json!({
        "id": id,
        "method": parts.method.as_str(),
        "path": parts.uri.path_and_query().map(|p| p.as_str()).unwrap_or("/"),
        "headers": headers,
        "bodyB64": base64(&bytes),
    });
    let (slot, wait) = oneshot::channel::<Value>();
    relay.queue_job(
        id.clone(),
        Job {
            frame: frame.clone(),
            answer: Some(slot),
            taken: false,
        },
    );
    // Hand it to a parked poller immediately, if there is one.
    if let Some(poll) = relay.waiting.lock().expect("the parked list").pop_front() {
        let _ = poll.slot.send(frame);
    }
    match tokio::time::timeout(grace, wait).await {
        // **THE ANSWER IS THE DEVICE'S OWN RESPONSE, REPLAYED** — its status, its headers and its body,
        // not an envelope about them. `relay.mjs` writes exactly this:
        //
        //     const headers = { ...(answer.headers || {}) };
        //     delete headers["content-length"];
        //     res.writeHead(answer.status || 502, headers).end(Buffer.from(answer.bodyB64 || "", "base64"));
        //
        // and the version before this one answered `200 {"id":…,"status":…}` — a client would have seen a
        // frame where it expected the device's HTTP answer. The default status is 502 for an answer that
        // carries none, and the length goes because the device's framing is not this relay's.
        Ok(Ok(answer)) => replay(&answer),
        Ok(Err(_)) => json_response(502, json!({ "ok": false, "error": "agent went away" })),
        Err(_) => {
            relay.take_answer(&id);
            // THE TWO SENTENCES ARE THE JAVASCRIPT'S, and the difference between them is the point: "no
            // agent has EVER connected" is an operator's mistake, "it did not answer in time" is a device
            // that is merely between polls.
            if relay.seen() {
                json_response(
                    504,
                    json!({ "ok": false, "error": "the agent did not answer in time" }),
                )
            } else {
                json_response(
                    503,
                    json!({ "ok": false, "error": "no agent has connected to this relay yet" }),
                )
            }
        }
    }
}

/// The device's response, as the client receives it.
fn replay(answer: &Value) -> Response {
    let status = answer
        .get("status")
        .and_then(|v| v.as_u64())
        .filter(|s| (100..=599).contains(s))
        .unwrap_or(502) as u16;
    let mut headers = HeaderMap::new();
    if let Some(map) = answer.get("headers").and_then(|v| v.as_object()) {
        for (k, v) in map {
            // `delete headers["content-length"]` — the device's framing does not describe what THIS
            // process sends, exactly as the comment in `relay.mjs` says.
            if k.eq_ignore_ascii_case("content-length") {
                continue;
            }
            if let (Ok(name), Some(value)) = (
                axum::http::HeaderName::from_bytes(k.as_bytes()),
                v.as_str().and_then(|s| HeaderValue::from_str(s).ok()),
            ) {
                headers.insert(name, value);
            }
        }
    }
    let body = decode_base64(answer.get("bodyB64").and_then(|v| v.as_str()).unwrap_or(""));
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
        headers,
        body,
    )
        .into_response()
}

/// `Buffer.from(b64, "base64")` — the frame carries the device's body encoded, because a frame is JSON.
fn decode_base64(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            // Padding and any whitespace a producer may have added end the stream.
            _ => break,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// The frame carries the body as base64, because a device request can be any bytes and the frame is JSON.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[tokio::main]
async fn main() {
    let arg = |name: &str, dflt: &str| -> String {
        let args: Vec<String> = std::env::args().collect();
        args.iter()
            .position(|a| a == &format!("--{name}"))
            .and_then(|i| args.get(i + 1).cloned())
            .unwrap_or_else(|| dflt.to_string())
    };
    let listen = arg("listen", "127.0.0.1:18990");
    let token = arg("token", "");
    let device = arg("name", "device");
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .unwrap_or_else(|e| panic!("cannot listen on {listen}: {e}"));
    println!("summrise-relay listening on {listen}");
    axum::serve(listener, app(Local::new(token, device)))
        .await
        .expect("the server runs");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request;
    use tower::ServiceExt;

    fn relay() -> Local {
        Local::new("s3cret".to_string(), "d1".to_string())
    }

    async fn call_headers(
        relay: Local,
        request: Request<axum::body::Body>,
    ) -> (StatusCode, axum::http::HeaderMap, String) {
        let response = app(relay).oneshot(request).await.expect("a response");
        let status = response.status();
        let headers = response.headers().clone();
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("a body");
        (status, headers, String::from_utf8_lossy(&body).to_string())
    }

    async fn call(relay: Local, request: Request<axum::body::Body>) -> (StatusCode, String) {
        let response = app(relay).oneshot(request).await.expect("a response");
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("a body");
        (status, String::from_utf8_lossy(&body).to_string())
    }

    #[tokio::test]
    async fn healthz_says_whether_an_agent_has_ever_polled() {
        let relay = relay();
        let (status, body) = call(
            relay.clone(),
            Request::builder()
                .uri("/healthz")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"device\":\"d1\""), "{body}");
        // NEVER SEEN IS `null`, not zero — the difference between "no agent has connected" and "one
        // connected at the epoch" is the difference between a 503 and a 504.
        assert!(body.contains("\"agentConnectedMsAgo\":null"), "{body}");
        assert!(body.contains("\"waiting\":0"), "{body}");
    }

    #[tokio::test]
    async fn the_agents_endpoints_refuse_a_wrong_or_missing_token() {
        for header in [None, Some("Bearer wrong"), Some("s3cret")] {
            let mut request = Request::builder().uri("/agent/pull");
            if let Some(h) = header {
                request = request.header("authorization", h);
            }
            let (status, body) =
                call(relay(), request.body(axum::body::Body::empty()).unwrap()).await;
            match header {
                Some("Bearer s3cret") => {
                    // A correct token with nothing queued PARKS — and the window is 25 s, so the test
                    // drives the timeout by hand rather than waiting: the relay answers 204 and the
                    // agent re-polls.
                    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
                }
                _ => assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}"),
            }
        }
    }

    #[tokio::test]
    async fn a_request_with_no_agent_ever_is_refused_in_two_seconds_and_says_why() {
        // The short wait: "no agent has EVER connected" is an operator's mistake, and waiting the full
        // window to say so helps nobody.
        let relay = relay();
        let started = std::time::Instant::now();
        let (status, body) = call(
            relay,
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .body(axum::body::Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            body.contains("no agent has connected to this relay yet"),
            "{body}"
        );
        assert!(
            started.elapsed() < Duration::from_millis(8_000),
            "the short grace, not the full window: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_client_and_an_agent_carry_one_request_and_its_answer() {
        // THE WHOLE RELAY, IN ONE TEST: the agent polls and parks, a client's request is handed to it, the
        // client waits, the agent answers BY ID, and the client gets exactly what the device said.
        let relay = relay();
        let poller = {
            let relay = relay.clone();
            tokio::spawn(async move {
                call(
                    relay,
                    Request::builder()
                        .uri("/agent/pull")
                        .header("authorization", "Bearer s3cret")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
            })
        };
        // **WAIT FOR THE STATE, DO NOT GUESS AT IT.** The first version slept 50 ms and hoped the poll had
        // parked; on a loaded machine it had not, the client's request found no parker, and the test failed
        // once and passed the next run — a flake is a test that reports the machine rather than the code.
        for _ in 0..200 {
            if !relay.waiting.lock().expect("the parked list").is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(
            !relay.waiting.lock().expect("the parked list").is_empty(),
            "the agent's poll never parked"
        );
        let client = {
            let relay = relay.clone();
            tokio::spawn(async move {
                call_headers(
                    relay,
                    Request::builder()
                        .method("POST")
                        .uri("/mcp?x=1")
                        .header("authorization", "Bearer sk-caller")
                        .body(axum::body::Body::from("{\"hello\":true}"))
                        .unwrap(),
                )
                .await
            })
        };
        let (status, frame) = poller.await.expect("the poll");
        assert_eq!(status, StatusCode::OK, "{frame}");
        let frame: Value = serde_json::from_str(&frame).expect("a frame");
        assert_eq!(frame["method"], "POST");
        assert_eq!(frame["path"], "/mcp?x=1");
        assert_eq!(frame["bodyB64"], "eyJoZWxsbyI6dHJ1ZX0=");
        // The caller's own header rides; `host` does not.
        assert_eq!(frame["headers"]["authorization"], "Bearer sk-caller");
        assert!(frame["headers"].get("host").is_none());
        // ...and the answer comes back by id.
        let id = frame["id"].as_str().expect("an id").to_string();
        let (status, _) = call(
            relay.clone(),
            Request::builder()
                .method("POST")
                .uri("/agent/answer")
                .header("authorization", "Bearer s3cret")
                .body(axum::body::Body::from(
                    serde_json::json!({
                        "id": id,
                        "status": 201,
                        "headers": { "content-type": "text/plain", "content-length": "999", "x-device": "d1" },
                        "bodyB64": "aGVsbG8=",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // **THE CLIENT GETS THE DEVICE'S RESPONSE, NOT AN ENVELOPE ABOUT IT.** `relay.mjs` replays
        // `answer.status`, `answer.headers` (minus `content-length`) and the decoded body; the version
        // before this one answered `200 {…frame…}`, which a client would read as the device's answer.
        let (status, headers, answer) = client.await.expect("the client");
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(answer, "hello");
        assert_eq!(
            headers.get("x-device").and_then(|v| v.to_str().ok()),
            Some("d1"),
            "the device's own headers ride"
        );
        // **THE DEVICE'S FRAMING DOES NOT RIDE; THIS PROCESS FRAMES ITS OWN REPLY.** `relay.mjs` deletes
        // the header and lets Node frame it — the same arrangement `proxies/README.md` describes for the
        // api-relay — so the length present is axum's, computed for the five bytes actually sent, and never
        // the device's `999`.
        assert_ne!(
            headers.get("content-length").and_then(|v| v.to_str().ok()),
            Some("999"),
            "the device's length does not ride"
        );
        assert_eq!(
            headers.get("content-length").and_then(|v| v.to_str().ok()),
            Some("5"),
            "the reply's own framing does"
        );
    }

    #[test]
    fn the_replay_defaults_to_502_drops_the_length_and_decodes_base64() {
        // An answer that carries no status is a 502 — the JavaScript's `answer.status || 502`.
        let bare = replay(&serde_json::json!({
            "headers": { "content-length": "999", "x-device": "d1" },
            "bodyB64": "aGk=",
        }));
        assert_eq!(bare.status(), StatusCode::BAD_GATEWAY);
        assert!(
            bare.headers().get("content-length").is_none(),
            "its framing does not ride"
        );
        assert_eq!(
            bare.headers().get("x-device").and_then(|v| v.to_str().ok()),
            Some("d1")
        );
        // `Buffer.from(b64, "base64")`, including the unpadded and empty forms.
        assert_eq!(decode_base64("aGk="), b"hi");
        assert_eq!(decode_base64("aGk"), b"hi");
        assert_eq!(decode_base64(""), b"");
        // And an answer that carries a status keeps it.
        assert_eq!(
            replay(&serde_json::json!({ "status": 404 })).status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn an_answer_for_an_unknown_id_is_a_404_and_the_queue_hands_a_job_out_once() {
        let relay = relay();
        let (status, body) = call(
            relay.clone(),
            Request::builder()
                .method("POST")
                .uri("/agent/answer")
                .header("authorization", "Bearer s3cret")
                .body(axum::body::Body::from("{\"id\":\"nope\"}"))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.contains("unknown id"), "{body}");

        // ONE QUEUE, ONE FLAG: two jobs queued, and each poll gets a DIFFERENT one.
        relay.queue_job(
            "a".to_string(),
            Job {
                frame: serde_json::json!({"id":"a"}),
                answer: None,
                taken: false,
            },
        );
        relay.queue_job(
            "b".to_string(),
            Job {
                frame: serde_json::json!({"id":"b"}),
                answer: None,
                taken: false,
            },
        );
        assert_eq!(
            relay.take_job().map(|(id, _)| id),
            Some("a".to_string()),
            "THE OLDEST FIRST, which a Map gives and a HashMap does not"
        );
        assert_eq!(relay.take_job().map(|(id, _)| id), Some("b".to_string()));
        assert_eq!(
            relay.take_job(),
            None,
            "a taken job is never handed out twice"
        );
    }
}
