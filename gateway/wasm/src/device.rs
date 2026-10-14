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

//! The worker's own half of the device surface (`DeviceDial`, `device_fetch`, the emitters) stays here; the GUARD
//! RULES moved to `summrise-core` and are re-exported so every call site and gate keeps its path.

pub use summrise_core::guard::{
    authority_prefix_ok, device_host_error, dial_path, host_allow_error, hostname_mismatch,
    starts_like_a_scheme, DEFAULT_DEVICE_HOST_SUFFIX, PROXY_PATH_REFUSAL,
};

/* ─────────────────────────── the dial ─────────────────────────── */

use std::time::Duration;

use worker::wasm_bindgen::{JsCast, JsValue};
use worker::*;
use worker::{js_sys, wasm_bindgen_futures, web_sys};

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
    if !authority_prefix_ok(rest_path) {
        return DeviceDial::refusal(400, PROXY_PATH_REFUSAL);
    }
    let url = format!("https://{hostname}{}", dial_path(rest_path));
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
    if hostname_mismatch(parsed.host_str().unwrap_or_default(), hostname) {
        return DeviceDial::refusal(400, PROXY_PATH_REFUSAL);
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
    // **THE DIAL GOES TO THE PARSED URL, NOT THE STRING THAT WAS PARSED** — the WHATWG parser lowercases the host
    // (and `Url::parse` does too), and the TypeScript source dials `upstream.toString()` for exactly that reason:
    // `device-fetch.test.mjs` pins `dFresh` arriving as `dfresh`.
    let url = parsed.to_string();

    // `const headers = new Headers(init.headers || {}); headers.delete("host"); headers.delete("cookie");
    //  headers.set("Authorization", `Bearer ${device.token}`)` — in that order, and the ORDER is the source's
    // too: a caller that sends its own `Authorization` (or `host`, or `cookie`) has it replaced or removed, so
    // no path can smuggle a device's credential past this dial.
    let headers = Headers::new();
    for (name, value) in &init.headers {
        let _ = headers.set(name, value);
    }
    for op in dial_header_ops(token) {
        match op {
            DialHeaderOp::Delete(name) => {
                let _ = headers.delete(name);
            }
            DialHeaderOp::Set(name, value) => {
                let _ = headers.set(name, &value);
            }
        }
    }

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
            Err(message) => return DeviceDial::refusal(502, &device_unreachable(&message)),
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
                    return DeviceDial::refusal(502, &device_unreachable(&thrown_message(&error)))
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
    js_request(url, method, headers, Some(body.as_ref()), Some("manual"))
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

/// One header operation the dial applies, IN ORDER, after copying the caller's own headers.
#[derive(Debug, PartialEq, Eq)]
pub enum DialHeaderOp {
    Delete(&'static str),
    Set(&'static str, String),
}

/// The header hygiene: `host` and `cookie` are DELETED and `Authorization` is SET — in that order, so a caller
/// sending any of the three has it removed or replaced, and no path can smuggle a device's credential past this
/// dial.
pub fn dial_header_ops(token: &str) -> Vec<DialHeaderOp> {
    vec![
        DialHeaderOp::Delete("host"),
        DialHeaderOp::Delete("cookie"),
        DialHeaderOp::Set("Authorization", format!("Bearer {token}")),
    ]
}

/// The bound, by method (round-55/56): a blackholed tunnel used to hang until the platform's subrequest ceiling, and
/// POST — a slow-loris vector there — is bounded at 60 s.
pub fn dial_timeout_ms(is_post: bool) -> u64 {
    if is_post {
        60_000
    } else {
        15_000
    }
}

/// The transport refusal's message.
pub fn device_unreachable(message: &str) -> String {
    format!("Device unreachable: {message}")
}

/// Which of the three forms a proxied response is rebuilt into.
#[derive(Debug, PartialEq, Eq)]
pub enum UpgradeShape {
    None,
    Socket,
    Body,
}

/// `build101Response`'s shape: only a 101 is rebuilt at all, with its socket when it has one and as the
/// body-plus-headers form when it does not (a 101 the runtime refuses, which the caller reproduces as a throw).
pub fn upgrade_shape(status: u16, has_socket: bool) -> UpgradeShape {
    if status != 101 {
        UpgradeShape::None
    } else if has_socket {
        UpgradeShape::Socket
    } else {
        UpgradeShape::Body
    }
}

/// The emitted module's header — **THE SOURCE FILE'S OWN COMMENTS, CARRIED VERBATIM, PLUS THE GENERATED NOTICE.**
/// The SSRF rationale and the round-120/121 history are the record of why these rules are what they are.
const TS_HEADER: &str = r#"/**
 * device-dial-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/device.rs` (`render_typescript`), which is now the single source. The freshness test
 * in that module is a byte comparison against this file, so a hand edit fails there with the text and one command to
 * fix it:
 *
 *     SUMMRISE_REFRESH_DEVICE_DIAL=1 cargo test --manifest-path gateway/wasm/Cargo.toml device::emission
 *
 * `gateway/src/device-fetch.ts` re-exports this module and keeps the HOST: the env reads, the URL parse, the
 * `fetchWithTimeout` call and the `Response` rebuilds. **The RULES left the file, and the two that were already
 * shared with the Rust worker — the SSRF blocklist and the suffix allowlist — had a copy on each side until now.**
 *
 * The comments below are the source file's own, carried verbatim.
 */

/// The default device-host suffix. **IT IS A PRODUCTION HOSTNAME**, which is why `agent/tests/production_host.rs`
/// declares it beside `device-fetch.ts`.
export const DEFAULT_DEVICE_HOST_SUFFIX = ".agent.saisi.online";

/**
 * `hostAllowError(hostname, env)`'s RULE, with the suffix already resolved — the Rust takes the VALUE, so reading
 * `DEVICE_HOST_SUFFIX` off the environment stays in the caller.
 *
 * Two tests, and the second is the one that is easy to miss: the host must END WITH the suffix **and be strictly
 * longer than it**, so the bare suffix is not a device hostname. The suffix is lowercased and so is the host, because
 * a hostname comparison is case-insensitive; an empty override falls back to the default through `||`, which is a
 * truthiness test rather than a presence test.
 *
 * (The length test counts UTF-16 units here and BYTES in the Rust. Reachable only for a non-ASCII hostname, which the
 * URL parser's own rules punish long before this — recorded rather than quietly aligned, because a port improves
 * nothing.)
 */
export function hostAllowErrorFor(
  hostname: string,
  suffix: string | null | undefined,
): string | null {
  const s = (suffix || DEFAULT_DEVICE_HOST_SUFFIX).toLowerCase();
  const h = String(hostname).toLowerCase();
  if (!h.endsWith(s) || h.length <= s.length) {
    return `hostname must be under ${s}`;
  }
  return null;
}

/**
 * `deviceHostError(hostname)`: an error string when the hostname must NOT be dialled with device credentials.
 *
 * stage-n SSRF audit (HIGH): reject private/link-local/internal IPs — an attacker who registers a device with
 * hostname "169.254.169.254" (cloud metadata) or "127.0.0.1" makes the gateway dial it with the device token.
 *
 * Decimal/hex/octal IPv4 need no extra rules — the WHATWG parser already normalizes them (2130706433/0x7f.0.0.1/
 * 0177.0.0.1 all parse to 127.0.0.1 and hit the 127. block). What it does NOT normalize away: 0.0.0.0, the cloud
 * metadata IP, and IPv4-mapped IPv6 (::ffff:/96 can smuggle 127.0.0.1 past every v4 rule — bracketed or bare).
 * 172.16/12 is ONLY second-octet 16-31 (numeric) — a blanket startsWith("172.") also blocked public 172.15.x.x /
 * 172.32+.x.x. The fc/fd/fe80 unique-local/link-local prefixes must be actual address forms (contain ':': hostnames
 * never do) — otherwise a hostname STRING like 'fc.example.com' false-positives.
 */
export function deviceHostError(hostname: string): string | null {
  const hostLower = String(hostname).toLowerCase();
  let is172Private = false;
  if (hostLower.startsWith("172.")) {
    const second = Number(hostLower.split(".")[1]);
    is172Private = Number.isInteger(second) && second >= 16 && second <= 31;
  }
  const isV6Private =
    hostLower.includes(":") &&
    (hostLower.startsWith("fc") || hostLower.startsWith("fd") || hostLower.startsWith("fe80"));
  if (
    hostLower === "localhost" ||
    hostLower.startsWith("127.") ||
    hostLower.startsWith("10.") ||
    hostLower.startsWith("192.168.") ||
    is172Private ||
    hostLower === "::1" ||
    hostLower === "[::1]" ||
    isV6Private ||
    hostLower === "0.0.0.0" ||
    hostLower === "[::]" ||
    hostLower === "169.254.169.254" ||
    hostLower.startsWith("::ffff:") ||
    hostLower.startsWith("[::ffff:")
  ) {
    return "device hostname resolves to a private/internal address";
  }
  return null;
}

/** The refusal both authority-shaped bad paths answer with (round-120/121). */
export const PROXY_PATH_REFUSAL = "invalid proxy path";

/** The AUTHORITY-RELEVANT prefix of `restPath` — up to the first `/`, `?` or `#` — may carry neither userinfo nor a
 *  scheme. **NARROWED ON PURPOSE** (round-121): an at-sign in a QUERY string is legitimate (`?user=a@b.com`), and the
 *  hostname-equality gate already blocks userinfo smuggling. */
export function authorityPrefixOk(restPath: string): boolean {
  const authority = restPath.split(/[/?#]/, 1)[0]!;
  return !authority.includes("@") && !/^[a-z][a-z0-9+.-]*:/i.test(authority);
}

/** The path half of the dial URL: `/` is prepended unless it is already there. */
export function dialPath(restPath: string): string {
  return restPath.startsWith("/") ? restPath : `/${restPath}`;
}

/** **BELT-AND-SUSPENDERS**: whatever the parser produced, the host MUST be the device's own — case-insensitively,
 *  because WHATWG lowercases special-scheme hosts while registration stores hostnames verbatim. */
export function hostnameMismatch(parsedHost: string, hostname: string): boolean {
  return parsedHost.toLowerCase() !== String(hostname).toLowerCase();
}

/** A guard refusal: the shape every path in the stack answers with. */
export type DialRefusal = { status: 400; ok: false; resp: undefined; error: string };

/** What the guards decided: either a refusal to return as-is, or the URL to dial. */
export type DialPlan = { refusal: DialRefusal } | { url: string };

/** THE GUARD STACK, IN ORDER — and the order IS the security argument, which is why it is here rather than in the
 *  caller: the authority-prefix sanitization (round-120/121) comes BEFORE the URL is built, then the parsed-hostname
 *  equality, then the private-IP blocklist, then the suffix allowlist.
 *
 *  `parse` is the platform's parser and **MAY THROW — the source's `new URL` sits outside its `try`, so an
 *  unparseable authority escapes `deviceFetch` instead of becoming a refusal**, and this function does not catch
 *  it. */
export function dialPlan(
  hostname: string,
  restPath: string,
  suffix: string | null | undefined,
  parse: (url: string) => URL,
): DialPlan {
  if (!authorityPrefixOk(restPath)) {
    return { refusal: { status: 400, ok: false, resp: undefined, error: PROXY_PATH_REFUSAL } };
  }
  const parsed = parse(`https://${hostname}${dialPath(restPath)}`);
  if (hostnameMismatch(parsed.hostname, hostname)) {
    return { refusal: { status: 400, ok: false, resp: undefined, error: PROXY_PATH_REFUSAL } };
  }
  const hostErr = deviceHostError(parsed.hostname);
  if (hostErr) return { refusal: { status: 400, ok: false, resp: undefined, error: hostErr } };
  const suffixErr = hostAllowErrorFor(hostname, suffix);
  if (suffixErr) return { refusal: { status: 400, ok: false, resp: undefined, error: suffixErr } };
  // **THE DIAL GOES TO THE PARSED URL, NOT THE STRING THAT WAS PARSED** — the WHATWG parser lowercases the host, and
  // `device-fetch.test.mjs` pins exactly that (`dFresh` arrives as `dfresh`). The Rust dials `Url::to_string()` for
  // the same reason.
  return { url: parsed.toString() };
}

/** One header operation the dial applies, IN ORDER, after copying the caller's own headers. */
export type DialHeaderOp =
  { op: "delete"; name: string } | { op: "set"; name: string; value: string };

/** The header hygiene: `host` and `cookie` are DELETED and `Authorization` is SET — in that order, so a caller
 *  sending any of the three has it removed or replaced. */
export function dialHeaderOps(token: string): DialHeaderOp[] {
  return [
    { op: "delete", name: "host" },
    { op: "delete", name: "cookie" },
    { op: "set", name: "Authorization", value: `Bearer ${token}` },
  ];
}

/** The bound, by method (round-55/56): reads are bounded, and POST — a slow-loris vector on a blackholed tunnel — is
 *  bounded at 60 s where everything else gets 15 s. */
export function dialTimeoutMs(isPost: boolean): number {
  return isPost ? 60000 : 15000;
}

/** The transport refusal's message. */
export function deviceUnreachable(message: string): string {
  return `Device unreachable: ${message}`;
}

/** Which of the three forms a proxied response is rebuilt into. */
export type UpgradeShape = "none" | "socket" | "body";

/** `build101Response`'s shape: only a 101 is rebuilt at all, with its socket when it has one and as the
 *  body-plus-headers form when it does not. */
export function upgradeShape(status: number, hasSocket: boolean): UpgradeShape {
  if (status !== 101) return "none";
  return hasSocket ? "socket" : "body";
}
"#;

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_HEADER.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/device-dial-logic.ts")
}

#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK — AND IT IS THE REASON THE EMISSION CANNOT DRIFT.** The same shape the six sibling
    /// emitters use.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_DEVICE_DIAL").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write device-dial-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_DEVICE_DIAL=1 cargo test device::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/device-dial-logic.ts is not what device.rs emits — \
             run SUMMRISE_REFRESH_DEVICE_DIAL=1 cargo test device::emission"
        );
    }
}

#[cfg(test)]
mod dial_rules {
    use super::*;

    /// **THE GUARD STACK'S RULES, EACH PINNED WHERE IT NOW LIVES.** The order they are applied in is
    /// `device_fetch`'s, and every one of these has a row in `device-fetch.test.mjs` too.
    ///
    /// MUTATION: drop the `dial_path` call and interpolate `restPath` directly.
    /// RESULT:   `the_authority_and_the_path` fails on the bare-path row — and the dial would ask for
    ///           `https://hostpath` instead of `https://host/path`.
    #[test]
    fn the_authority_and_the_path() {
        assert!(authority_prefix_ok("/api/x"));
        assert!(
            authority_prefix_ok("/x?user=a@b.com"),
            "an at-sign in a QUERY is legitimate"
        );
        assert!(
            !authority_prefix_ok("@evil.example/x"),
            "userinfo re-roots the URL"
        );
        assert!(!authority_prefix_ok("https://evil.example/x"));
        assert!(
            !authority_prefix_ok("HTTPS://evil.example"),
            "the scheme test is case-insensitive"
        );
        assert_eq!(dial_path("/x"), "/x");
        assert_eq!(dial_path("x"), "/x");
        assert!(
            !hostname_mismatch("Host.Example", "host.example"),
            "case-insensitive"
        );
        assert!(hostname_mismatch("evil.example", "host.example"));
    }

    /// **THE HEADER HYGIENE, IN ORDER** — a caller's own `host`/`cookie`/`Authorization` cannot survive it.
    #[test]
    fn the_header_ops_are_in_order() {
        assert_eq!(
            dial_header_ops("sekret"),
            vec![
                DialHeaderOp::Delete("host"),
                DialHeaderOp::Delete("cookie"),
                DialHeaderOp::Set("Authorization", "Bearer sekret".to_string()),
            ]
        );
        assert_eq!(dial_timeout_ms(true), 60_000);
        assert_eq!(dial_timeout_ms(false), 15_000);
        assert_eq!(device_unreachable("boom"), "Device unreachable: boom");
    }

    /// **THE 101 SHAPES.** The middle arm is the one the runtime refuses, which is why the caller reproduces it as a
    /// throw rather than a quiet passthrough.
    #[test]
    fn the_upgrade_shapes() {
        assert_eq!(upgrade_shape(200, false), UpgradeShape::None);
        assert_eq!(upgrade_shape(101, true), UpgradeShape::Socket);
        assert_eq!(upgrade_shape(101, false), UpgradeShape::Body);
    }
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
