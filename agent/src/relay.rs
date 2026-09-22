//! THE OUTBOUND RELAY CLIENT — the agent side of `proxies/summrise-relay/`.
//!
//! WHY THIS EXISTS. A NAT'd agent cannot be reached, so every comparable product inverts the direction: the agent dials out and
//! the server never dials in (Portainer Edge Agent, Microsoft dev tunnels — "No inbound connections are required" — Nabu Casa's
//! SniTun, GitLab and GitHub runners). `docs/research/remote-mcp-access.md` is the survey behind that claim.
//!
//! THE ONE RULE THIS FILE KEEPS: a relayed request is pushed through the SAME `axum::Router` the local listener serves
//! (`crate::mcp::compose`). Not a second set of routes, not a second gate, not a second fallback — because a remote caller and
//! a local one must not be able to tell the difference, and a second composition is exactly how they would come to differ.
//!
//! THE FRAMES are the relay's (`relay.mjs`): a job is `{id, method, path, headers, bodyB64}` and an answer is
//! `{id, status, headers, bodyB64}`. Base64 because a panel response can be binary (fonts, screenshots) and must survive the
//! trip byte for byte.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::Router;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt as _;

/// A panel or MCP response is capped at 8 MiB by the relay; refuse to build anything larger on this side.
const MAX_BODY: usize = 8 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct Job {
    pub id: String,
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default, rename = "bodyB64")]
    pub body_b64: String,
}

#[derive(Debug, Serialize)]
pub struct Answer {
    pub id: String,
    pub status: u16,
    pub headers: HashMap<String, String>,
    #[serde(rename = "bodyB64")]
    pub body_b64: String,
}

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// Turn one relayed frame into a request against `app` and back into a frame. Pure enough to test without a relay.
pub async fn serve_job(app: &Router, job: Job) -> Answer {
    let body = b64().decode(job.body_b64.as_bytes()).unwrap_or_default();
    if body.len() > MAX_BODY {
        return Answer {
            id: job.id,
            status: 413,
            headers: HashMap::new(),
            body_b64: String::new(),
        };
    }
    let mut builder = axum::http::Request::builder()
        .method(job.method.as_str())
        .uri(job.path.as_str());
    for (k, v) in &job.headers {
        // A header the relay carried but that is not a legal name/value here is DROPPED, not fatal: the relay forwards what a
        // real client sent, and one malformed header must not lose the whole request.
        if let (Ok(name), Ok(value)) = (
            axum::http::HeaderName::from_bytes(k.as_bytes()),
            axum::http::HeaderValue::from_str(v),
        ) {
            builder = builder.header(name, value);
        }
    }
    let req = match builder.body(Body::from(body)) {
        Ok(r) => r,
        Err(e) => {
            return Answer {
                id: job.id,
                status: 400,
                headers: HashMap::new(),
                body_b64: b64().encode(format!("relay: could not rebuild the request: {e}")),
            }
        }
    };
    let id = job.id;
    match app.clone().oneshot(req).await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let mut headers = HashMap::new();
            for (k, v) in resp.headers() {
                if let Ok(v) = v.to_str() {
                    headers.insert(k.as_str().to_string(), v.to_string());
                }
            }
            let bytes = axum::body::to_bytes(resp.into_body(), MAX_BODY)
                .await
                .unwrap_or_default();
            Answer {
                id,
                status,
                headers,
                body_b64: b64().encode(bytes),
            }
        }
        Err(e) => Answer {
            id,
            status: 500,
            headers: HashMap::new(),
            body_b64: b64().encode(format!("relay: the app refused the request: {e}")),
        },
    }
}

/// Poll, answer, repeat — forever, with a backoff that only grows on TRANSPORT failure.
pub fn spawn(
    url: String,
    token: String,
    app: Router,
    state: SharedRelayState,
    ct: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let base = url.trim_end_matches('/').to_string();
        // A LOOPBACK RELAY MUST NOT GO THROUGH A PROXY (round 209, measured on the device). This agent sits behind a
        // corporate proxy and reqwest honours the proxy environment by default, so `http://127.0.0.1:18990/agent/pull` was
        // handed to the proxy — which cannot reach anybody's loopback — and the device reported eleven consecutive failures
        // with "error sending request" while the relay was answering /healthz perfectly on that same host. A relay on a THIRD
        // machine still uses the proxy, which is correct and unchanged.
        let loopback_relay = base.contains("//127.0.0.1")
            || base.contains("//localhost")
            || base.contains("//[::1]");
        let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(45));
        if loopback_relay {
            builder = builder.no_proxy();
            tracing::info!("relay: {base} is loopback — bypassing any configured proxy");
        }
        let client = match builder.build() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("relay: no HTTP client: {e}");
                return;
            }
        };
        let mut backoff_ms: u64 = 500;
        tracing::info!("relay: dialling out to {base}");
        loop {
            if ct.is_cancelled() {
                return;
            }
            match pull(&client, &base, &token).await {
                Ok(Some(job)) => {
                    backoff_ms = 500; // an answer proves the path is healthy
                    if let Ok(mut st) = state.lock() {
                        st.last_ok_ms = Some(now_ms());
                        st.consecutive_failures = 0;
                        st.last_error = None;
                    }
                    let out = serve_job(&app, job).await;
                    if let Err(e) = post_answer(&client, &base, &token, &out).await {
                        tracing::warn!("relay: could not deliver an answer: {e}");
                    }
                }
                Ok(None) => {
                    // A 204 IS EVIDENCE OF HEALTH (round 210, measured on the device): the relay answered. The first version
                    // recorded health only when a JOB arrived, so a working but IDLE relay read as connected:false with a
                    // stale failure count — the agent was parked on its long poll, the relay reported `waiting: 1`, and
                    // /api/status told the operator "relay unreachable". The interface must say what is true: answering a
                    // poll is contact, and it happens at least once per poll window.
                    backoff_ms = 500;
                    if let Ok(mut st) = state.lock() {
                        st.last_ok_ms = Some(now_ms());
                        st.consecutive_failures = 0;
                        st.last_error = None;
                    }
                }
                Err(e) => {
                    tracing::warn!("relay: {e} — retrying in {backoff_ms}ms");
                    if let Ok(mut st) = state.lock() {
                        st.consecutive_failures = st.consecutive_failures.saturating_add(1);
                        st.last_error = Some(e.clone());
                    }
                    tokio::select! {
                        _ = ct.cancelled() => return,
                        _ = tokio::time::sleep(Duration::from_millis(backoff_ms)) => {}
                    }
                    backoff_ms = (backoff_ms * 2).min(30_000);
                }
            }
        }
    })
}

async fn pull(client: &reqwest::Client, base: &str, token: &str) -> Result<Option<Job>, String> {
    let resp = client
        .get(format!("{base}/agent/pull"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    match resp.status().as_u16() {
        200 => resp
            .json::<Job>()
            .await
            .map(Some)
            .map_err(|e| e.to_string()),
        204 => Ok(None),
        401 => Err("the relay refused this token".to_string()),
        other => Err(format!("the relay answered {other}")),
    }
}

async fn post_answer(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    answer: &Answer,
) -> Result<(), String> {
    client
        .post(format!("{base}/agent/answer"))
        .bearer_auth(token)
        .json(answer)
        .send()
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| {
            if r.status().is_success() {
                Ok(())
            } else {
                Err(format!("answer rejected: {}", r.status()))
            }
        })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The relay's own report, for `/api/status`: an operator has to be able to see whether their agent is connected to the relay
/// they configured, which is the same "the interface carries the state" rule everything else here follows.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RelayState {
    pub configured: bool,
    pub last_ok_ms: Option<u64>,
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
}

pub type SharedRelayState = Arc<std::sync::Mutex<RelayState>>;

pub fn shared() -> SharedRelayState {
    Arc::new(std::sync::Mutex::new(RelayState::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;

    fn app() -> Router {
        Router::new()
            .route("/mcp", get(|| async { "mcp saw it" }))
            .route(
                "/api/echo",
                axum::routing::post(|body: String| async move { body }),
            )
    }

    fn job(method: &str, path: &str, body: &str) -> Job {
        Job {
            id: "j1".into(),
            method: method.into(),
            path: path.into(),
            headers: HashMap::new(),
            body_b64: b64().encode(body),
        }
    }

    #[tokio::test]
    async fn a_job_runs_through_the_real_app_and_comes_back() {
        let out = serve_job(&app(), job("GET", "/mcp", "")).await;
        assert_eq!(out.status, 200);
        assert_eq!(
            String::from_utf8(b64().decode(out.body_b64).unwrap()).unwrap(),
            "mcp saw it"
        );
    }

    #[tokio::test]
    async fn a_body_survives_the_trip_byte_for_byte() {
        let payload = "x".repeat(40_000) + "≠é";
        let out = serve_job(&app(), job("POST", "/api/echo", &payload)).await;
        assert_eq!(out.status, 200);
        assert_eq!(
            String::from_utf8(b64().decode(out.body_b64).unwrap()).unwrap(),
            payload
        );
    }

    #[tokio::test]
    async fn an_unknown_route_is_the_apps_own_404_not_a_second_routing_table() {
        let out = serve_job(&app(), job("GET", "/nothing-here", "")).await;
        assert_eq!(
            out.status, 404,
            "the app decides, exactly as it does for a local caller"
        );
    }

    #[tokio::test]
    async fn a_malformed_header_is_dropped_rather_than_losing_the_request() {
        let mut j = job("GET", "/mcp", "");
        j.headers.insert("bad header".into(), "x".into());
        j.headers.insert("x-good".into(), "y".into());
        let out = serve_job(&app(), j).await;
        assert_eq!(out.status, 200, "one bad header must not lose the request");
    }
}
