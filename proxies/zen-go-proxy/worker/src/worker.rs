//! **THE I/O HALF OF `zen-go-proxy`, AND IT IS wasm32-ONLY ON PURPOSE.**
//!
//! `#[event(fetch)]` expands to nothing on the host and panics there, so `lib.rs` keeps every decision —
//! the route table, the body-size gate, the token estimate, the translation, the SSE encoder — building
//! and running under a plain `cargo test` with no wasm toolchain at all. **A host shim would mean a
//! second copy of the dispatch that no test runs**, which is the shape this migration exists to delete.
//!
//! ── THE GATE IS FIRST, BEFORE THE ROUTE TABLE ───────────────────────────────────────────────────
//!
//! `zen-go`'s JavaScript checks `CLIENT_KEY` **before it looks at the path**, so a request to a path that
//! does not exist is refused with 401 rather than 404 — one gate over every route. `zen-us` checks per
//! arm. **That is a structural difference, so it lives here rather than in the table**, and the corpus in
//! `lib.rs` pins the table alone.
//!
//! ── AND THE `/v1/messages` ARM HAS A MODEL-DEPENDENT BRANCH ──────────────────────────────────────
//!
//! `deepseek-flash` and `deepseek-v4-flash` are **Anthropic-native** on `zen/go/v1/messages`, so the raw
//! request is forwarded and the response is a true Anthropic SSE stream. Every other model goes to
//! `chat/completions` with the request TRANSLATED to OpenAI, and the answer translated back — which is
//! what the four functions in `lib.rs` are for.

use worker::*;

use crate::{
    cors_headers, estimate_tokens, json_error_body, json_too_large, request_host, route,
    to_anthropic_response, to_openai_request, to_sse, x_api_key_allows, Route, HEADER_TIMEOUT_MS,
};

/// `NATIVE` — the models zen/go serves Anthropic-natively on `/v1/messages` (`src/index.js:207`).
/// `deepseek-flash` is the version-less lane, and `deepseek-v4-flash` is the retired V4 slug that zen
/// still accepts as an alias.
const NATIVE: [&str; 2] = ["deepseek-flash", "deepseek-v4-flash"];

/// `corsHeaders(request)` — from the request's Origin and Host.
fn cors(request: &Request) -> Vec<(String, String)> {
    let origin = request
        .headers()
        .get("origin")
        .ok()
        .flatten()
        .unwrap_or_default();
    let host = request
        .url()
        .ok()
        .map(|u| request_host(u.as_str()))
        .unwrap_or_default();
    cors_headers(&origin, &host)
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

/// `jsonError(status, message, type, cors)` — the envelope, with the CORS headers merged in.
fn json_error(
    status: u16,
    message: &str,
    kind: &str,
    cors: &[(String, String)],
) -> Result<Response> {
    let (status, body) = json_error_body(status, message, kind);
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    for (k, v) in cors {
        headers.set(k, v)?;
    }
    Ok(Response::from_bytes(body.into_bytes())?
        .with_status(status)
        .with_headers(headers))
}

/// `jsonOk(data, cors)` — status 200 and `application/json`, with no `Cache-Control` (`src/index.js:394`).
fn json_ok(body: String, cors: &[(String, String)]) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    for (k, v) in cors {
        headers.set(k, v)?;
    }
    Ok(Response::from_bytes(body.into_bytes())?
        .with_status(200)
        .with_headers(headers))
}

/// `relayUpstreamError(upstream, cors, secrets)` — **THE ONE PLACE A PROVIDER'S OWN WORDS REACH THE
/// CLIENT, WHICH IS WHY THE CREDENTIAL DIES HERE.** `upstream_message` decides WHICH words (ported, with a
/// twelve-case corpus in `zen-us`'s crate) and `redact_secrets` is what the shipping worker calls next.
async fn relay_upstream_error(
    up: &mut Response,
    status: u16,
    cors: &[(String, String)],
    secrets: &[&str],
) -> Result<Response> {
    let body = up.text().await.unwrap_or_default();
    let message = summrise_zen_us::upstream_message(status, &body);
    let redacted = summrise_zen_us::redact_secrets(&message, secrets);
    json_error(status, &redacted, "api_error", cors)
}

/// `new Response(body, { headers: { "Content-Type": …, "Cache-Control": "no-cache", …cors } })` over a
/// STRING body — the translated-and-encoded path, where the bytes are ours rather than the upstream's.
fn text_response(body: String, ctype: &str, cors: &[(String, String)]) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Content-Type", ctype)?;
    headers.set("Cache-Control", "no-cache")?;
    for (k, v) in cors {
        headers.set(k, v)?;
    }
    Ok(Response::from_bytes(body.into_bytes())?
        .with_status(200)
        .with_headers(headers))
}

/// **`fetchUpstreamHeaders` — THE HEADER BUDGET, WITH THE SIGNAL.** `worker::RequestInit` has no signal
/// field (`worker-0.8.7/src/request_init.rs:12`, and its `From` impl sets five things and no signal at
/// `:66`), so the `web_sys::RequestInit` is built here and `set_signal` is called on it. **The `select` is
/// the `finally { clearTimeout(timer) }`**: when the fetch wins the `AbortController` is simply DROPPED —
/// dropping one does not abort — so the body stream the caller forwards stays untimed, which is the whole
/// point of a HEADER budget.
async fn fetch_upstream_headers(
    url: &str,
    method: &str,
    headers: &[(String, String)],
    body: Option<String>,
) -> Result<Response> {
    let ctrl = worker::AbortController::default();
    let signal = ctrl.signal();

    let init = web_sys::RequestInit::new();
    init.set_method(method);
    let h = web_sys::Headers::new().map_err(Error::from)?;
    for (k, v) in headers {
        h.set(k, v).map_err(Error::from)?;
    }
    init.set_headers(&h);
    let signal_ref: &web_sys::AbortSignal = &signal;
    init.set_signal(Some(signal_ref));
    if let Some(b) = body.as_ref() {
        init.set_body(&wasm_bindgen::JsValue::from_str(b));
    }

    let inner = web_sys::Request::new_with_str_and_init(url, &init).map_err(Error::from)?;
    let req: Request = inner.into();

    let fetcher = Fetch::Request(req);
    let fetch = fetcher.send();
    let budget = Delay::from(std::time::Duration::from_millis(HEADER_TIMEOUT_MS));
    futures_util::pin_mut!(fetch, budget);
    match futures_util::future::select(fetch, budget).await {
        futures_util::future::Either::Left((result, _)) => result,
        futures_util::future::Either::Right((_, _)) => {
            ctrl.abort();
            Err(Error::RustError(format!(
                "upstream headers did not arrive within {HEADER_TIMEOUT_MS} ms"
            )))
        }
    }
}

/// The body as a `serde_json::Value`, or `None` when it does not parse. **`await request.json()` throws in
/// the JavaScript and the caller's `catch` answers 500**, which is what `None` becomes here.
async fn json_body(req: &mut Request) -> Option<serde_json::Value> {
    let text = req.text().await.ok()?;
    serde_json::from_str(&text).ok()
}

#[event(fetch)]
pub async fn fetch(mut req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let url = req.url()?;
    let pathname = url.path().to_string();
    let cors = cors(&req);

    // THE PREFLIGHT IS FIRST, and it is answered before the gate — the JavaScript returns it before it
    // reads `CLIENT_KEY` at all, so a browser's preflight never needs a key.
    if req.method() == Method::Options {
        let headers = Headers::new();
        for (k, v) in &cors {
            headers.set(k, v)?;
        }
        return Ok(Response::empty()?
            .with_status(crate::preflight_status())
            .with_headers(headers));
    }

    // **THE GATE, OVER EVERY PATH — INCLUDING THE ONES THAT DO NOT EXIST.** A request to an unknown path
    // with no key is a 401 here and a 404 in `zen-us`.
    let presented = req
        .headers()
        .get("x-api-key")
        .ok()
        .flatten()
        .unwrap_or_default();
    let configured = env.var("CLIENT_KEY").ok().map(|v| v.to_string());
    if !x_api_key_allows(configured.as_deref(), &presented) {
        return json_error(
            401,
            "Missing or invalid x-api-key",
            "authentication_error",
            &cors,
        );
    }

    match route(req.method().as_ref(), &pathname) {
        Route::Preflight => unreachable!("handled above"),

        // GET …/models — the passthrough, labelled `application/json` rather than asking the upstream.
        Route::Models => {
            let key = env.var("OPENCODE_GO_API_KEY")?.to_string();
            let mut up = fetch_upstream_headers(
                "https://opencode.ai/zen/go/v1/models",
                "GET",
                &[("x-api-key".to_string(), key)],
                None,
            )
            .await?;
            let status = up.status_code();
            let stream = up.stream()?;
            let headers = Headers::new();
            headers.set("Content-Type", "application/json")?;
            for (k, v) in &cors {
                headers.set(k, v)?;
            }
            Ok(Response::from_stream(stream)?
                .with_status(status)
                .with_headers(headers))
        }

        // POST …/count_tokens — the estimate, and the whole endpoint is `estimate_tokens`.
        Route::CountTokens => {
            let declared = req.headers().get("content-length").ok().flatten();
            if json_too_large(declared.as_deref()) {
                return json_error(
                    413,
                    "Request body too large",
                    "invalid_request_error",
                    &cors,
                );
            }
            let Some(body) = json_body(&mut req).await else {
                return json_error(500, "Internal error", "api_error", &cors);
            };
            let tokens =
                estimate_tokens(body.get("system"), body.get("tools"), body.get("messages"));
            json_ok(format!("{{\"input_tokens\":{tokens}}}"), &cors)
        }

        // POST …/messages — native, or translated.
        Route::Messages => {
            let declared = req.headers().get("content-length").ok().flatten();
            if json_too_large(declared.as_deref()) {
                return json_error(
                    413,
                    "Request body too large",
                    "invalid_request_error",
                    &cors,
                );
            }
            let Some(anthropic_req) = json_body(&mut req).await else {
                return json_error(500, "Internal error", "api_error", &cors);
            };
            let model = anthropic_req
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("deepseek-flash")
                .to_string();
            let native = NATIVE.contains(&model.as_str());
            let worker_key = env.var("OPENCODE_GO_API_KEY")?.to_string();
            let streaming = anthropic_req
                .get("stream")
                .map(|s| s.as_bool().unwrap_or(false))
                .unwrap_or(false);

            let (upstream_url, headers, payload) = if native {
                (
                    "https://opencode.ai/zen/go/v1/messages".to_string(),
                    vec![
                        ("x-api-key".to_string(), worker_key.clone()),
                        ("Content-Type".to_string(), "application/json".to_string()),
                        ("anthropic-version".to_string(), "2023-06-01".to_string()),
                    ],
                    anthropic_req.clone(),
                )
            } else {
                (
                    "https://opencode.ai/zen/go/v1/chat/completions".to_string(),
                    vec![
                        ("Authorization".to_string(), format!("Bearer {worker_key}")),
                        ("Content-Type".to_string(), "application/json".to_string()),
                    ],
                    to_openai_request(&anthropic_req, &model),
                )
            };
            let body_text = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
            let mut up =
                fetch_upstream_headers(&upstream_url, "POST", &headers, Some(body_text)).await?;
            let status = up.status_code();

            if !(200..300).contains(&status) {
                if status >= 500 {
                    // A 5xx gets GENERIC client text and the detail stays server-side.
                    let body = up.text().await.unwrap_or_default();
                    let detail = summrise_zen_us::upstream_message(status, &body);
                    worker::console_error!(
                        "[zen-go] upstream 5xx: {}",
                        summrise_zen_us::redact_secrets(&detail, &[worker_key.as_str()])
                    );
                    return json_error(status, "Upstream unavailable", "api_error", &cors);
                }
                // **THREE SECRETS, AND THE THIRD IS THE CALLER'S OWN BEARER TOKEN.** The JavaScript
                // strips the `Bearer ` prefix before redacting (`src/index.js:246-249`).
                let caller_api_key = req
                    .headers()
                    .get("x-api-key")
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let auth = req
                    .headers()
                    .get("authorization")
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let caller_bearer = auth
                    .trim()
                    .strip_prefix("Bearer ")
                    .or_else(|| auth.trim().strip_prefix("bearer "))
                    .unwrap_or("")
                    .trim()
                    .to_string();
                return relay_upstream_error(
                    &mut up,
                    status,
                    &cors,
                    &[
                        worker_key.as_str(),
                        caller_api_key.as_str(),
                        caller_bearer.as_str(),
                    ],
                )
                .await;
            }

            if native {
                // Native passthrough: the upstream body straight through, SSE if the caller asked for a
                // stream and JSON otherwise — and the JSON arm carries NO `Cache-Control`.
                let stream = up.stream()?;
                let headers = Headers::new();
                if streaming {
                    headers.set("Content-Type", "text/event-stream; charset=utf-8")?;
                    headers.set("Cache-Control", "no-cache")?;
                } else {
                    headers.set("Content-Type", "application/json")?;
                }
                for (k, v) in &cors {
                    headers.set(k, v)?;
                }
                return Ok(Response::from_stream(stream)?
                    .with_status(status)
                    .with_headers(headers));
            }

            // Translated: read the OpenAI answer, translate it back, and encode it as SSE when streaming.
            let text = up.text().await.unwrap_or_default();
            let openai: serde_json::Value =
                serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
            let anthropic_res = to_anthropic_response(&openai, &model);
            if streaming {
                return text_response(
                    to_sse(&anthropic_res),
                    "text/event-stream; charset=utf-8",
                    &cors,
                );
            }
            json_ok(
                serde_json::to_string(&anthropic_res).unwrap_or_default(),
                &cors,
            )
        }

        Route::NotFound => json_error(404, "Not Found", "not_found_error", &cors),
    }
}
