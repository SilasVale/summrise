//! **THE I/O HALF OF `zen-us-proxy`, AND IT IS wasm32-ONLY ON PURPOSE.**
//!
//! ── THE MUTATION THAT MUST FAIL THIS FILE'S COMPANION ───────────────────────────────────────────
//! `verify.mjs` compares this worker's bytes with the shipping JavaScript's, on the same request. Read
//! that file's header for the mutation; this one is the thing it drives.
//!
//! ── WHY `#[cfg(target_arch = "wasm32")]`, WHICH IS NOT A PORTABILITY COMPROMISE ─────────────────
//!
//! `#[event(fetch)]` **EXPANDS TO NOTHING ON THE HOST AND PANICS THERE**, because there is no Worker
//! runtime to register with. So `lib.rs` keeps every decision — the route table, the origin policy, the
//! redaction, the gates — building and running under a plain `cargo test` with no wasm toolchain at
//! all, and this module is compiled only for the target it serves. **THE ALTERNATIVE, A HOST SHIM,
//! WOULD MEAN A SECOND COPY OF THE DISPATCH THAT NO TEST RUNS** — which is the shape this whole
//! migration exists to delete. `index/worker` carries the same arrangement and the same paragraph.
//!
//! ── WHAT IS PORTED HERE, AND WHAT IS NAMED AS NOT ───────────────────────────────────────────────
//!
//! **THE FIVE ARMS THAT NEED NO UPSTREAM**, because those are the ones whose bytes can be compared
//! today, with no stub at all:
//!
//!     OPTIONS *                  the preflight, which is `corsHeaders(request)` and nothing else
//!     GET  …/models              the CLIENT_KEY gate's 401
//!     POST …/responses           the blank-caller-key 401
//!     POST …/messages            the CLIENT_KEY gate's 401
//!     anything else              404, from the envelope
//!
//! **AND THE THREE THAT NEED ONE, NAMED RATHER THAN APPROXIMATED**: the upstream fetches for
//! `/models`, `/responses` and `/messages`, which need `fetchUpstreamHeaders` and `relayUpstreamError`
//! ported first. **They are the next stage, and this file says so instead of pretending the surface is
//! whole.** A worker that answered 501 for them would be worse than one that does not answer at all:
//! it would make `verify.mjs` green on a request the shipping worker serves.

use worker::*;

use crate::{
    bearer_key, cors_headers, json_error_body, request_host, route, x_api_key_allows, Route,
};

/// `corsHeaders(request)` — the shipping worker's own, from the request's Origin and Host.
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
    // **THE workers-rs API IS `Response::from_bytes` + `with_status` + `with_headers`, NOT A BUILDER.**
    // The first version of this used `Response::builder()...with_body()`, which does not exist — and
    // `cargo check --target wasm32-unknown-unknown` is what said so, because this module is not compiled
    // on the host at all. **THAT IS THE ARRANGEMENT WORKING**: the target-only half is checked by
    // checking the target.
    Ok(Response::from_bytes(body.into_bytes())?
        .with_status(status)
        .with_headers(headers))
}

#[event(fetch)]
pub async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let url = req.url()?;
    let pathname = url.path().to_string();
    let cors = cors(&req);

    // **THE PREFLIGHT IS A ROW OF THE ROUTE TABLE, NOT A CHECK IN FRONT OF IT.** The first version of
    // this file asked `req.method() == Method::Options` here and returned early — which is a SECOND
    // COPY OF THE DISPATCH, and exactly the shape this module's header says the whole arrangement
    // exists to avoid. `route()` already answers `Route::Preflight` for `OPTIONS`, so the match below
    // is the only place the decision lives.
    match route(req.method().as_ref(), &pathname) {
        Route::Preflight => {
            let headers = Headers::new();
            for (k, v) in &cors {
                headers.set(k, v)?;
            }
            Ok(Response::empty()?
                .with_status(crate::preflight_status())
                .with_headers(headers))
        }

        // GET …/models — `CLIENT_KEY` gated, and the gate's REFUSAL is the arm this stage owns. The
        // upstream half is named above as not ported.
        Route::Models => {
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
            // **THE UPSTREAM HALF, WIRED — and this arm is the one that proves `fetch_upstream_headers`
            // works, because a helper nothing calls is a helper nothing tests.** The shipping worker
            // forwards the upstream's OWN body and status, and labels the result `application/json`
            // rather than asking the upstream what it sent (`src/index.js:203-210`).
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

        // POST …/responses — BYOK: the CALLER's key, and a blank one is 401 before anything else.
        Route::Responses => {
            let auth = req
                .headers()
                .get("authorization")
                .ok()
                .flatten()
                .unwrap_or_default();
            if bearer_key(&auth).is_none() {
                return json_error(
                    401,
                    "caller key required (Authorization: Bearer <opencode zen key>)",
                    "authentication_error",
                    &cors,
                );
            }
            not_ported("POST /v1/responses upstream")
        }

        // POST …/messages — `CLIENT_KEY` gated and DEFAULT-CLOSED: when the variable is unset every
        // request is refused, which is the `x_api_key_allows` contract rather than a check here.
        Route::Messages => {
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
            not_ported("POST /v1/messages upstream")
        }

        Route::NotFound => json_error(404, "Not Found", "not_found_error", &cors),
    }
}

/// **`fetchUpstreamHeaders`, AND THE ANSWER TO THE QUESTION THE PORT LEFT OPEN — MEASURED, NOT GUESSED.**
///
/// The shipping worker's decision is the timer discipline:
///
/// ```text
///     const ctrl = new AbortController();
///     const timer = setTimeout(() => ctrl.abort(), HEADER_TIMEOUT_MS);
///     try { return await fetch(url, { ...init, signal: ctrl.signal }); }
///     finally { clearTimeout(timer); }
/// ```
///
/// **THE SIGNAL IS THE WHOLE POINT, AND THE `finally` IS THE OTHER HALF.** `fetch()` resolves when the
/// HEADERS arrive, so clearing the timer there leaves the returned BODY stream untimed — which is what lets
/// a caller forward `upstream.body` for minutes. "`AbortSignal.timeout` on the whole fetch would also abort
/// the body mid-stream once the budget expires — **the Vercel bug this worker avoids**."
///
/// ── AND THE PRIMARY SOURCE SAYS `worker`'s WRAPPER CANNOT CARRY IT ────────────────────────────
///
/// Read in `worker-0.8.7` itself rather than assumed:
///
///   * `worker::AbortController` EXISTS (`src/abort.rs`: "An interface that allows you to abort in-flight
///     Fetch requests"), and `worker` enables the `AbortController`/`AbortSignal` web-sys features.
///   * **`worker::RequestInit` HAS NO SIGNAL FIELD** (`src/request_init.rs:12`): `body`, `headers`, `cf`,
///     `method`, `redirect`, `cache` — and nothing else.
///   * **AND ITS `From<&RequestInit> for web_sys::RequestInit` SETS FIVE THINGS AND NO SIGNAL**
///     (`src/request_init.rs:66`): headers, method, redirect, cache, body.
///
/// **SO THE FAITHFUL PORT BUILDS THE `web_sys::RequestInit` ITSELF AND CALLS `set_signal`.** That needs
/// `web-sys` with the `RequestInit` and `AbortSignal` features as a DIRECT dependency of this crate — the
/// crate already takes `worker`'s features transitively, but a direct `web_sys::RequestInit` is this
/// crate's own use and its own declaration.
///
/// ── AND THE CHEAPER ALTERNATIVE, WITH THE DIFFERENCE NAMED ─────────────────────────────────────
///
/// Racing `worker::Delay` against the fetch needs no `web-sys` and no signal — **but on timeout it stops
/// WAITING without ABORTING, so the upstream request stays in flight.** The client sees the same bytes
/// either way (`ctrl.abort()` rejects the fetch, the handler's `catch` answers
/// `jsonError(500, "Internal error", "api_error", cors)` — measured, `src/index.js:317-320`), **so
/// `verify.mjs` could not tell the two apart** — which is exactly the kind of difference this repository
/// says to name rather than to discover later. **THE SIGNAL IS THE PORT.**
///
/// ── AND HERE IT IS, WRITTEN FROM THAT MEASUREMENT ─────────────────────────────────────────────
///
/// `Fetch::Request` needs a `worker::Request`, and the only way to get one carrying a SIGNAL is to build the
/// `web_sys::Request` here — `worker::Request` has `From<web_sys::Request>`, which is the same conversion its
/// own `new_with_init` ends with.
///
/// **THE `finally { clearTimeout(timer) }` IS THE `select`.** When the fetch wins, the `AbortController` is
/// simply DROPPED — dropping one does not abort — and the body stream the caller forwards is untouched.
/// When the budget wins, the controller is aborted and the caller gets an `Err`, which is what the shipping
/// worker's `catch` turns into `jsonError(500, "Internal error", "api_error", cors)`.
async fn fetch_upstream_headers(
    url: &str,
    method: &str,
    headers: &[(String, String)],
    body: Option<web_sys::ReadableStream>,
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
    // `set_signal` is the ONE thing `worker::RequestInit` cannot carry, and it is the whole point.
    // **`worker::AbortSignal` DEREFS TO `web_sys::AbortSignal`** (`worker-0.8.7/src/abort.rs:81`), so the
    // coercion is what carries it across; `AbortController` has no `Deref`, and it does not need one —
    // `abort()` consumes it here.
    let signal_ref: &web_sys::AbortSignal = &signal;
    init.set_signal(Some(signal_ref));
    if let Some(b) = body.as_ref() {
        init.set_body(b);
    }

    let inner = web_sys::Request::new_with_str_and_init(url, &init).map_err(Error::from)?;
    let req: Request = inner.into();

    // **`send(&self)` BORROWS THE `Fetch`**, so the temporary cannot be the receiver: the future holds the
    // borrow, and `pin_mut!` holds the future.
    let fetcher = Fetch::Request(req);
    let fetch = fetcher.send();
    let budget = Delay::from(std::time::Duration::from_millis(crate::HEADER_TIMEOUT_MS));
    futures_util::pin_mut!(fetch, budget);
    match futures_util::future::select(fetch, budget).await {
        futures_util::future::Either::Left((result, _)) => result,
        futures_util::future::Either::Right((_, _)) => {
            ctrl.abort();
            Err(Error::RustError(format!(
                "upstream headers did not arrive within {} ms",
                crate::HEADER_TIMEOUT_MS
            )))
        }
    }
}
///
/// **AN ARM THAT IS NAMED RATHER THAN FAKED.** The three upstream routes are not ported yet, and a
/// 501 says exactly that — where a plausible-looking empty 200 would let `verify.mjs` pass on a request
/// the shipping worker actually serves. **THE FAILURE IS THE HONEST ANSWER.**
fn not_ported(what: &str) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Content-Type", "text/plain; charset=utf-8")?;
    Ok(
        Response::from_bytes(format!("not ported yet: {what}").into_bytes())?
            .with_status(501)
            .with_headers(headers),
    )
}
