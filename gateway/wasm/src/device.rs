//! THE DEVICE-HOST RULES **AND THE ONE DIAL THAT APPLIES THEM**, MOVED FROM `gateway/src/device-fetch.ts`.
//!
//! WHY THE RULES. One decides WHAT A DEVICE HOSTNAME IS, and the other decides which hostnames must never
//! be dialled with a device credential — the stage-n SSRF audit, whose comment names the attack: a device
//! registered with hostname `169.254.169.254` (cloud metadata) or `127.0.0.1` makes the gateway dial it
//! with the device token. Both are pure, and a drift in either is a hole rather than a bug.
//!
//! **`device_host_error` DOES NOT REPEAT THE WHATWG PARSER'S WORK**, and the source says why: decimal, hex
//! and octal IPv4 (`2130706433`, `0x7f.0.0.1`, `0177.0.0.1`) are normalized to `127.0.0.1` by the URL
//! parser before this sees them. What it does NOT normalize away is the list below — including
//! IPv4-mapped IPv6, which can smuggle `127.0.0.1` past every v4 rule.
//!
//! **WHY THE DIAL IS HERE RATHER THAN IN EITHER CALLER.** It was written for the device family (landing 5
//! slice 1) as a private helper in `devices.rs` and it is needed again by the MCP surface's terminal relay —
//! same guard stack, same `redirect: "manual"`, same bounded fetch, and a different method and timeout. A
//! second copy of an SSRF stack is the one duplication in this repository that is a HOLE rather than a bug:
//! the source says it in its own header ("every device dial … flows through deviceFetch with the full SSRF
//! guard stack"), so the two callers share one function and the differences are parameters
//! ([`DialInit`]). The BROWSER bridge deliberately does NOT use this dial — its POSTs legitimately run to
//! ~320 s where `deviceFetch`'s bound is 60 s, which is why `mcp_browser.rs` applies
//! [`device_host_error`] itself instead (exactly as `mcp-browser.ts` does).

/// The default device-host suffix. **IT IS A PRODUCTION HOSTNAME**, which is why this file is declared in
/// `agent/tests/production_host.rs` beside `device-fetch.ts` — the rule that decides what a device
/// hostname is has to name one.
pub const DEFAULT_DEVICE_HOST_SUFFIX: &str = ".agent.saisi.online";

/// `hostAllowError(hostname, env)`: the allowlist, overridable per deployment.
///
/// Two tests, and the second is the one that is easy to miss: the host must END WITH the suffix **and be
/// strictly longer than it**, so the bare suffix is not a device hostname. The suffix is lowercased and so
/// is the host, because a hostname comparison is case-insensitive; an empty override falls back to the
/// default through `||`, which is a truthiness test rather than a presence test.
pub fn host_allow_error(hostname: &str, device_host_suffix: Option<&str>) -> Option<String> {
    let suffix = match device_host_suffix {
        Some(s) if !s.is_empty() => s.to_lowercase(),
        _ => DEFAULT_DEVICE_HOST_SUFFIX.to_string(),
    };
    let h = hostname.to_lowercase();
    if !h.ends_with(&suffix) || h.len() <= suffix.len() {
        return Some(format!("hostname must be under {suffix}"));
    }
    None
}

/// `deviceHostError(hostname)`: `Some(reason)` when the hostname must NOT be dialled with device
/// credentials.
///
/// THE `172.` ARM IS SECOND-OCTET AND NUMERIC: `172.16/12` is only 16–31, and a blanket
/// `startsWith("172.")` would also refuse the public `172.15.x.x` and `172.32+.x.x`. `Number.isInteger` is
/// what makes `"172.abc.1.1"` not private — `Number("abc")` is NaN.
///
/// THE v6 PREFIXES REQUIRE A COLON, because a hostname STRING never contains one: without that test,
/// `fc.example.com` would false-positive as a unique-local address.
pub fn device_host_error(hostname: &str) -> Option<String> {
    let host = hostname.to_lowercase();
    let is_172_private = if let Some(rest) = host.strip_prefix("172.") {
        let second = rest.split('.').next().unwrap_or("");
        match js_number_of(second) {
            Some(n) => n.fract() == 0.0 && (16.0..=31.0).contains(&n),
            None => false,
        }
    } else {
        false
    };
    let is_v6_private = host.contains(':')
        && (host.starts_with("fc") || host.starts_with("fd") || host.starts_with("fe80"));
    let blocked = host == "localhost"
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || is_172_private
        || host == "::1"
        || host == "[::1]"
        || is_v6_private
        || host == "0.0.0.0"
        || host == "[::]"
        || host == "169.254.169.254"
        || host.starts_with("::ffff:")
        || host.starts_with("[::ffff:");
    if blocked {
        Some("device hostname resolves to a private/internal address".to_string())
    } else {
        None
    }
}

/// `Number(x)` for the one place this module needs it: `Number.isInteger(Number(second))`.
///
/// `None` is `NaN`, which is what makes a non-numeric second octet not private. An EMPTY string is `0` and
/// therefore an integer — `"172..1.1"` takes the private branch, exactly as the JavaScript does.
fn js_number_of(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0.0);
    }
    t.parse::<f64>().ok()
}

/* ─────────────────────────── the dial ─────────────────────────── */

use std::time::Duration;

use worker::wasm_bindgen::{JsCast, JsValue};
use worker::{js_sys, wasm_bindgen_futures, web_sys};
use worker::*;

/// `deviceFetch`'s answer: the source's `{ status, ok, resp, error }`.
///
/// `threw` is the ONE arm the source has that is not in that object: `new URL(...)` sits OUTSIDE
/// `deviceFetch`'s `try`, so a hostname the parser refuses propagates a `TypeError` to the CALLER rather than
/// coming back as a refusal — and the MCP relay reports it as a tool failure with no code at all. Modelling it
/// as a field keeps that distinction instead of flattening it into a 400 the source never produces.
pub struct DeviceDial {
    /// The HTTP status, or the 400/502 the guard and the transport answer with.
    pub status: u16,
    /// `resp.ok` — false whenever there is no response at all.
    pub ok: bool,
    pub resp: Option<Response>,
    /// `error` — the message the caller puts in its refusal.
    pub error: Option<String>,
    /// The source THREW (see the struct comment); the message is `e.message`.
    pub threw: Option<String>,
}

impl DeviceDial {
    fn refusal(status: u16, error: &str) -> Self {
        DeviceDial {
            status,
            ok: false,
            resp: None,
            error: Some(error.to_string()),
            threw: None,
        }
    }
}

/// **WHAT A DIAL SENDS AS ITS BODY — AND THE STREAM ARM IS THE ONE THE RELAYS NEED.**
///
/// The first two arms are the source's `init.body` as the two existing callers use it: absent (a GET probe) and
/// a string (the MCP terminal relay's JSON). The third is the DEVICE PROXY's and the FILE RELAY's body — the
/// caller's own `ReadableStream`, forwarded without being buffered.
///
/// **WHY A STREAM AND NOT BYTES.** `plugins/devices.ts` says it for the upload: 100 MiB, and "the worker then
/// answers 500 with no manifest" when the old fixed timeout aborted it. Reading that body into a `Vec<u8>`
/// first would put 100 MiB inside a 128 MB isolate before a single byte left — the passthrough the source
/// deliberately wrote as `body: request.body`. `plugins/device-proxy.ts` forwards the same way for
/// non-GET/HEAD.
pub enum DialBody<'a> {
    /// `undefined` — the source's default, and what a GET/HEAD passes.
    None,
    /// A string body.
    Text(&'a str),
    /// The caller's own stream, still unread.
    Stream(web_sys::ReadableStream),
}

/// The `init` half of a dial. The callers differ in exactly these four things — the device family probes with a
/// GET and a 15 s bound, the MCP terminal relay posts JSON with a 60 s one, the proxy forwards the caller's
/// method, headers and stream — and nothing else.
pub struct DialInit<'a> {
    pub method: Method,
    pub body: DialBody<'a>,
    /// The CALLER'S OWN headers, before this dial's hygiene. Empty for the two JSON callers; the proxy's
    /// inbound set for the proxy, which is the only way the device sees the panel's `Range`, `Accept`, etc.
    pub headers: Vec<(String, String)>,
    pub timeout_ms: u64,
}

impl<'a> DialInit<'a> {
    /// `deviceFetch(env, device, path)` — the source's default `init`: GET, no body, 15 s.
    pub fn get() -> Self {
        DialInit {
            method: Method::Get,
            body: DialBody::None,
            headers: Vec::new(),
            timeout_ms: 15_000,
        }
    }

    /// `deviceFetch(env, device, path, {method: "POST", headers: {"content-type": "application/json"}, body})`
    /// — the MCP terminal relay's call, bounded at 60 s (the source: "POST is bounded at 60 s … the old
    /// unbounded POST was a slow-loris vector on blackholed tunnels").
    pub fn post_json(body: &'a str) -> Self {
        DialInit {
            method: Method::Post,
            body: DialBody::Text(body),
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            timeout_ms: 60_000,
        }
    }
}

/// `deviceFetch(env, device, restPath, init)` — the guard stack, then the bounded fetch.
///
/// The order is the source's: the authority-prefix rejection, the URL build (which may THROW), the parsed
/// hostname equality, the private-IP blocklist, the suffix allowlist, the header hygiene
/// (`host`/`cookie` deleted, `Authorization` set), and `redirect: "manual"`.
pub async fn device_fetch(
    env: &Env,
    hostname: &str,
    token: &str,
    rest_path: &str,
    init: DialInit<'_>,
) -> DeviceDial {
    // round-120/121: a `restPath` carrying userinfo or a scheme can re-root the URL, and the token goes with it.
    // The check is narrowed to the AUTHORITY-relevant prefix (up to the first `/`, `?` or `#`) because an
    // at-sign in a query string is legitimate.
    let authority = rest_path.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') || starts_like_a_scheme(authority) {
        return DeviceDial::refusal(400, "invalid proxy path");
    }
    let path = if rest_path.starts_with('/') {
        rest_path.to_string()
    } else {
        format!("/{rest_path}")
    };
    let url = format!("https://{hostname}{path}");
    let parsed = match Url::parse(&url) {
        Ok(url) => url,
        // THE SOURCE THROWS HERE (`new URL` is outside its `try`), so this is not a refusal.
        Err(_) => {
            return DeviceDial {
                status: 0,
                ok: false,
                resp: None,
                error: None,
                threw: Some("Invalid URL".to_string()),
            }
        }
    };
    // Belt-and-suspenders: whatever the parser produced, the host MUST be the device's own hostname.
    if parsed.host_str().unwrap_or_default().to_lowercase() != hostname.to_lowercase() {
        return DeviceDial::refusal(400, "invalid proxy path");
    }
    if let Some(reason) = device_host_error(parsed.host_str().unwrap_or_default()) {
        return DeviceDial::refusal(400, &reason);
    }
    let suffix = env
        .var("DEVICE_HOST_SUFFIX")
        .ok()
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty());
    if let Some(reason) = host_allow_error(hostname, suffix.as_deref()) {
        return DeviceDial::refusal(400, &reason);
    }

    // `const headers = new Headers(init.headers || {}); headers.delete("host"); headers.delete("cookie");
    //  headers.set("Authorization", `Bearer ${device.token}`)` — in that order, and the ORDER is the source's
    // too: a caller that sends its own `Authorization` (or `host`, or `cookie`) has it replaced or removed, so
    // no path can smuggle a device's credential past this dial.
    let headers = Headers::new();
    for (name, value) in &init.headers {
        let _ = headers.set(name, value);
    }
    let _ = headers.delete("host");
    let _ = headers.delete("cookie");
    let _ = headers.set("Authorization", &format!("Bearer {token}"));

    // **TWO CONSTRUCTION PATHS, AND THE SECOND ONE EXISTS FOR ONE REASON.** `Request::new_with_init` is the
    // proven one (it is what every other route in this worker dials with); it cannot carry a `ReadableStream`
    // body, because `web_sys::RequestInit` has no `duplex` and the fetch standard requires it for a stream. So
    // the stream arm builds the same request through the GLOBAL `Request` constructor with `duplex: "half"`,
    // which is a platform call rather than a decision — and it is what makes the 100 MiB upload a passthrough
    // here instead of a 100 MiB allocation.
    let DialInit {
        method,
        body,
        timeout_ms,
        ..
    } = init;
    let request = match body {
        DialBody::Stream(ref stream) => match streaming_request(&url, &method, &headers, stream) {
            Ok(request) => request,
            Err(message) => {
                return DeviceDial::refusal(502, &format!("Device unreachable: {message}"))
            }
        },
        other => {
            let mut request_init = RequestInit::new();
            request_init.with_method(method);
            request_init.with_headers(headers);
            request_init.with_redirect(RequestRedirect::Manual);
            if let DialBody::Text(text) = other {
                request_init.with_body(Some(JsValue::from_str(text)));
            }
            match Request::new_with_init(&url, &request_init) {
                Ok(request) => request,
                Err(error) => {
                    return DeviceDial::refusal(
                        502,
                        &format!("Device unreachable: {}", thrown_message(&error)),
                    )
                }
            }
        }
    };
    match fetch_request_with_timeout(request, timeout_ms).await {
        Ok(resp) => {
            let status = resp.status_code();
            DeviceDial {
                status,
                ok: (200..300).contains(&status),
                resp: Some(resp),
                error: None,
                threw: None,
            }
        }
        // `catch (e) { return {status: 502, ok: false, error: \`Device unreachable: ${e.message}\`} }` — and the
        // AbortError arm above it, which `fetchWithTimeout` has already renamed to `timeout after <ms>ms`.
        Err(failure) => DeviceDial::refusal(502, &format!("Device unreachable: {failure}")),
    }
}

/// `fetchWithTimeout(url, init, ms)` — the race, and the ONE difference from the source is stated rather than
/// hidden: `workers-rs` 0.8.7's `RequestInit` has no `signal`, so this races the fetch against a `Delay`
/// instead of aborting it (the same shape `v1.rs` and `devices.rs` already use). The client sees the same
/// answer at the same moment; the upstream request itself is ended by the platform.
///
/// **THE `AbortError` ARM IS THE SOURCE'S OWN, AND IT IS NOT DECORATION**: `fetchWithTimeout` renames an
/// aborted fetch to `timeout after <ms>ms`, and a runtime that aborts for its own reasons (a cancelled
/// subrequest) arrives here with exactly that name — so a port that only raced its own `Delay` would report
/// `Device unreachable: AbortError: …` where the source reports a timeout, and the caller's code mapping
/// (`/timeout/i`) would put it in the wrong bucket.
pub async fn fetch_with_timeout(
    url: &str,
    init: &RequestInit,
    timeout_ms: u64,
) -> std::result::Result<Response, String> {
    let request = Request::new_with_init(url, init).map_err(|e| thrown_message(&e))?;
    fetch_request_with_timeout(request, timeout_ms).await
}

/// **THE RACE, OVER AN ALREADY-BUILT REQUEST** — split out so the STREAM arm can reach it: `fetch_with_timeout`
/// builds through `web_sys::RequestInit`, and a `ReadableStream` body cannot be built that way (see
/// [`streaming_request`]). The behaviour is identical; the split is plumbing.
pub async fn fetch_request_with_timeout(
    request: Request,
    timeout_ms: u64,
) -> std::result::Result<Response, String> {
    let fetcher = Fetch::Request(request);
    let fetch = fetcher.send();
    let delay = Delay::from(Duration::from_millis(timeout_ms));
    futures_util::pin_mut!(fetch);
    futures_util::pin_mut!(delay);
    match futures_util::future::select(fetch, delay).await {
        futures_util::future::Either::Left((result, _)) => result.map_err(|e| {
            if is_abort_error(&e) {
                format!("timeout after {timeout_ms}ms")
            } else {
                thrown_message(&e)
            }
        }),
        futures_util::future::Either::Right((_, _)) => Err(format!("timeout after {timeout_ms}ms")),
    }
}

/// `new Request(url, {method, headers, body, redirect, duplex})` — built through the GLOBAL constructor because
/// `web_sys::RequestInit` has no `duplex` setter, and the fetch standard refuses a stream body without it.
/// Without this, a passthrough would have to buffer the whole body first.
///
/// **RUST NAMES THE CALL AND JAVASCRIPT PERFORMS IT, WHICH IS THE BOUNDARY THIS FILE ALREADY CROSSES**: the same
/// constructor is what `Request::new_with_init` calls underneath, one binding layer down. Nothing here decides
/// anything — the method, the headers, the redirect mode and the body all come from the caller.
///
/// `redirect` is `Some("manual")` for every device dial and for the upload's host fallback, and `None` for a
/// service binding (which cannot redirect at all, and where the source passes no redirect).
pub fn js_request(
    url: &str,
    method: &Method,
    headers: &Headers,
    body: Option<&JsValue>,
    redirect: Option<&str>,
) -> std::result::Result<Request, String> {
    let init = js_sys::Object::new();
    let set = |key: &str, value: &JsValue| -> std::result::Result<(), String> {
        js_sys::Reflect::set(&init, &JsValue::from_str(key), value)
            .map(|_| ())
            .map_err(|e| js_message(&e))
    };
    set("method", &JsValue::from_str(method.as_ref()))?;
    set("headers", headers.as_ref())?;
    if let Some(body) = body {
        set("body", body)?;
        // Required by the fetch standard (and by undici, which the harness runs on) for a stream body.
        set("duplex", &JsValue::from_str("half"))?;
    }
    if let Some(redirect) = redirect {
        set("redirect", &JsValue::from_str(redirect))?;
    }
    let constructor = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("Request"))
        .map_err(|e| js_message(&e))?;
    let constructor: js_sys::Function = constructor
        .dyn_into()
        .map_err(|_| "Request is not a constructor".to_string())?;
    // **`Reflect::construct`, NOT `Function::call2`.** `Request` is a CLASS, and a class constructor cannot be
    // invoked as a plain function — the first version of this asked for `Request(url, init)` and the runtime
    // answered `Class constructor _Request cannot be invoked without 'new'` (measured, 2026-10-09, on this
    // section's first run). `Reflect.construct` is the `new` the platform needs.
    let args = js_sys::Array::of2(&JsValue::from_str(url), &init);
    let built = js_sys::Reflect::construct(&constructor, &args).map_err(|e| js_message(&e))?;
    let web_request: web_sys::Request = built
        .dyn_into()
        .map_err(|_| "the Request constructor returned something else".to_string())?;
    Ok(Request::from(web_request))
}

/// The dial's stream arm: the same constructor, with the caller's `ReadableStream` as the body.
fn streaming_request(
    url: &str,
    method: &Method,
    headers: &Headers,
    body: &web_sys::ReadableStream,
) -> std::result::Result<Request, String> {
    js_request(
        url,
        method,
        headers,
        Some(body.as_ref()),
        Some("manual"),
    )
}

/// **`Fetcher::fetch(request)` FOR A REQUEST THIS MODULE BUILT.** `workers-rs` 0.8.7's typed
/// `Fetcher::fetch_request` needs `worker::Error: From<Infallible>` for the reflexive conversion, which is not
/// in the crate — so this calls the binding's own `fetch` method, which is the same call `fetch_request` makes
/// one layer down (`self.0.fetch(req.inner())`). The body is whatever the caller built, stream included.
pub async fn service_fetch(
    fetcher: &Fetcher,
    request: &Request,
) -> std::result::Result<Response, String> {
    let value: &JsValue = fetcher.as_ref();
    let fetch = js_sys::Reflect::get(value, &JsValue::from_str("fetch"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .ok_or_else(|| "the service binding has no fetch method".to_string())?;
    let promise = fetch
        .call1(value, request.inner().as_ref())
        .map_err(|e| js_message(&e))?;
    let promise: js_sys::Promise = promise
        .dyn_into()
        .map_err(|_| "fetch did not return a promise".to_string())?;
    let response = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|e| js_message(&e))?;
    let web_response: web_sys::Response = response
        .dyn_into()
        .map_err(|_| "the service binding returned something that is not a Response".to_string())?;
    Ok(Response::from(web_response))
}

/// **`e.message` FOR A JAVASCRIPT THROW THAT NEVER BECAME A `worker::Error`.** The source's `catch (e)` puts
/// `e.message` in the refusal, which for a `TypeError` out of the `Request` constructor is the useful half —
/// `RequestInit: duplex option is required when sending a body`, not `TypeError: RequestInit: …`.
fn js_message(value: &JsValue) -> String {
    js_sys::Reflect::get(value, &JsValue::from_str("message"))
        .ok()
        .and_then(|m| m.as_string())
        .unwrap_or_else(|| "undefined".to_string())
}

/// `e.name === "AbortError"` — the runtime's own abort, which the source renames to a timeout.
fn is_abort_error(error: &Error) -> bool {
    matches!(
        error,
        Error::UnknownJsError { name: Some(name), .. } if name == "AbortError"
    )
}

/// **`e.message`, NOT `String(e)`.** `worker::Error`'s `Display` prints `TypeError: Invalid URL` for a
/// `UnknownJsError`, where the JavaScript's `e.message` is `Invalid URL` alone — and that string is what the
/// refusal's body carries. A BARE STRING throw has no `.message` at all, which is `undefined` in JavaScript
/// and is reproduced as the word rather than as an empty string.
fn thrown_message(error: &Error) -> String {
    match error {
        Error::UnknownJsError { message, .. } => message.clone(),
        Error::JsError(_) => "undefined".to_string(),
        other => other.to_string(),
    }
}

/// `authorityPrefix`'s scheme test: `^[a-z][a-z0-9+.-]*:`. Moved here with the dial, because both the dial and
/// the device family's own path check read it.
pub fn starts_like_a_scheme(authority: &str) -> bool {
    let Some((head, _)) = authority.split_once(':') else {
        return false;
    };
    let mut chars = head.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `device` cases were produced by the SHIPPING TypeScript
    //! (`gateway/wasm/oracle-translate.mjs`), and this replays them.
    use super::*;

    fn corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/translate-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    #[test]
    fn every_device_host_rule_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            let got: Option<String> = match func {
                "host_allow_error" => host_allow_error(
                    input["hostname"].as_str().unwrap_or(""),
                    input["suffix"].as_str(),
                ),
                "device_host_error" => device_host_error(input["hostname"].as_str().unwrap_or("")),
                _ => continue,
            };
            let as_json = match got {
                Some(s) => serde_json::Value::String(s),
                None => serde_json::Value::Null,
            };
            assert_eq!(&as_json, want, "{func} / {name}");
            checked += 1;
        }
        assert!(checked >= 30, "the device corpus shrank to {checked} cases");
    }
}
