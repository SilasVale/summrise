//! THE `/api/devices` ROUTE FAMILY — the glue between `worker::Fetch`/KV and the ported decisions.
//!
//! WHAT IS IN HERE AND WHAT IS NOT. The decisions live in `device_registry.rs` (what a device record is, what a
//! rename refuses, what a row shows), `session_gate.rs` (who the caller is) and `device_store.rs` (which key,
//! which lock, which TTL). This file is the ORDER those decisions run in, per route — which is itself part of the
//! port, because the order is what a caller with two problems sees: authentication before routing, routing
//! before validation, validation before the store.
//!
//! THE ROUTE TABLE IS THE PLUGIN'S, IN THE PLUGIN'S ORDER (`gateway/src/plugins/devices.ts:setup`), and the two
//! shapes that are NOT here are named rather than quietly dropped:
//!
//! ```text
//!     POST   /api/register                        (public — one-time reg key)
//!     POST   /api/devices/self-register           (public — the agent's own 64-hex token)
//!     POST   /api/install/tunnel-token            (public — reg key gated CF token)
//!     POST   /api/devices/panel-grant/redeem      (device token)
//!     GET    /api/devices/register-keys           (admin)
//!     DELETE /api/devices/register-keys/<code>    (admin)
//!     GET    /api/devices/install-cmd             (admin)
//!     POST   /api/devices/<name>/rename           (admin)
//!     POST   /api/devices/<name>/panel-grant      (admin)
//!     GET    /api/devices                         (admin)
//!     POST   /api/devices                         (admin)
//!     GET    /api/devices/<name>/mcp              (admin)
//!     DELETE /api/devices/<name>                  (admin)
//!     POST   /api/devices/register-key            (admin)
//! ```
//!
//! TWO ROUTES OF THE SAME PLUGIN ARE NOT PORTED, AND EACH HAS A REASON RATHER THAN A SENTENCE:
//!
//!   * `ANY /api/devices/<name>/proxy/<rest>` — `device-proxy.ts`, a different module with a different job (a
//!     reverse proxy that mints per-device cookies, rewrites panel HTML and can upgrade a WebSocket). It is a
//!     slice of its own; the front door keeps it on the TypeScript path, and the harness's cutover section
//!     proves that it does.
//!   * `POST|PUT /api/upload` — a 100 MiB body PASSTHROUGH to the relay. The decisions on it (which credential
//!     is accepted, the size bound, the response-header strip list) could be ported, but `workers-rs` 0.8.7
//!     cannot send a request body as a stream: `RequestInit::with_body` takes a `JsValue` and the crate's
//!     `ResponseBody` has no stream constructor for an outgoing request, so a port would have to materialize
//!     the upload in the isolate's memory. That is a regression, not a port, so the route stays where it is.
//!
//! **AND THE ONE VALUE IN THIS FILE THAT NAMES A PRODUCTION HOST IS THE INSTALL MANIFEST'S DEFAULT BASE**, which
//! `devices.ts` carries for the same reason and which `agent/tests/production_host.rs` is told about in the same
//! commit — a device that cannot be told where to download its installer is not a device this console can
//! install.

use std::sync::Mutex;
use std::time::Duration;

use futures_util::future::Either;
use serde_json::{json, Map, Value};
use worker::*;

use crate::device_registry as registry;
use crate::device_store as store;
use crate::session_gate;

/// `indexWorkerBase(env)`'s fallback — the deployment's release host, and the one production hostname this file
/// carries (see the module header).
pub const INDEX_WORKER_BASE_DEFAULT: &str = "https://agent.saisi.online";

/// `INSTALL_CMD_TTL_MS` — the install manifest is cached 5 minutes in-isolate.
const INSTALL_CMD_TTL_MS: i64 = 5 * 60 * 1000;

/// `installCmdCache` — module state, exactly like the source's: the timestamp, the version, the download URL.
type InstallCmdCache = Option<(i64, Option<String>, Option<String>)>;

/// `installCmdCache` — module state, exactly like the source's.
static INSTALL_CMD_CACHE: Mutex<InstallCmdCache> = Mutex::new(None);

const DEVICE_BASE: &str = "/api/devices";

/* ─────────────────────────── response helpers ─────────────────────────── */

/// `jsonOk(data)`.
fn json_ok(value: Value) -> Result<Response> {
    crate::json(serde_json::to_string(&value)?, 200)
}

/// `readJson(request)` — a body that does not parse is `{}`, which is what makes an empty POST behave like a
/// POST with no fields rather than a 500.
async fn read_json(request: &mut Request) -> Value {
    match request.text().await {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| json!({})),
        Err(_) => json!({}),
    }
}

/// **THE SOURCE'S `body.key` THROWS ON A LITERAL `null` BODY, AND THAT IS REPRODUCED RATHER THAN FIXED.**
/// `handleRegister` and `handleTunnelToken` spell `String(body.key || "")` without the optional chaining their
/// siblings use, so a request whose body is the four bytes `null` becomes a TypeError inside the handler and the
/// front door's catch answers 500 `Internal error`. Answering 403 there instead would be a silent behaviour
/// change on a route an installer calls; the divergence is not worth having, so the arm is `None` → 500.
fn body_key(body: &Value) -> Option<String> {
    match body {
        Value::Null => None,
        other => Some(registry::js_falsy_string(other.get("key")).to_lowercase()),
    }
}

/* ─────────────────────────── the route shapes ─────────────────────────── */

/// `^/api/devices/([^/]+)<suffix>$` — one non-empty segment, no slash inside it.
fn device_segment<'a>(path: &'a str, suffix: &str) -> Option<&'a str> {
    let rest = path.strip_prefix(DEVICE_BASE)?.strip_prefix('/')?;
    let name = rest.strip_suffix(suffix)?;
    if name.is_empty() || name.contains('/') {
        return None;
    }
    Some(name)
}

/// `decodeDeviceName(seg)` — `decodeURIComponent`, with `null` on a malformed or non-UTF-8 escape (the source's
/// `URIError` that used to be an unhandled 500).
pub fn decode_device_name(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            let byte = u8::from_str_radix(hex, 16).ok()?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `/^[a-z][a-z0-9+.-]*:/i` — a leading scheme in the authority prefix, which is the other way a `restPath` can
/// re-root the URL the device token is sent to.
fn starts_like_a_scheme(authority: &str) -> bool {
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

/// Does this request belong to the family at all? Used by the front door's cutover AND by `handle`, so a path
/// that is handed over is a path that is served — the two cannot drift.
pub fn in_family(method: &Method, path: &str) -> bool {
    if let Some(rest) = path.strip_prefix(DEVICE_BASE) {
        // `/api/devices/<name>/proxy…` is `device-proxy.ts` — a different module, and the ONE shape of this
        // prefix that stays on the TypeScript path.
        if let Some(rest) = rest.strip_prefix('/') {
            if rest.split('/').nth(1) == Some("proxy") {
                return false;
            }
        }
        return true;
    }
    *method == Method::Post && (path == "/api/register" || path == "/api/install/tunnel-token")
}

/// The dispatcher. `None` is `dispatch()`'s null: no route in this plugin matched, and the front door answers
/// its own 404.
pub async fn handle(mut request: Request, env: &Env) -> Result<Option<Response>> {
    let url = request.url()?;
    let path = url.path().to_string();
    let method = request.method();

    // ── the three public routes, each behind the per-IP gate the plugin wraps them in ──────────────
    if method == Method::Post && path == "/api/register" {
        if public_rate_limited(&request) {
            return Ok(Some(crate::json_error(
                429,
                "rate limit exceeded",
                "rate_limit_error",
            )?));
        }
        return Ok(Some(handle_register(&mut request, env).await?));
    }
    if method == Method::Post && path == "/api/devices/self-register" {
        if public_rate_limited(&request) {
            return Ok(Some(crate::json_error(
                429,
                "rate limit exceeded",
                "rate_limit_error",
            )?));
        }
        return Ok(Some(handle_self_register(&mut request, env).await?));
    }
    if method == Method::Post && path == "/api/install/tunnel-token" {
        if public_rate_limited(&request) {
            return Ok(Some(crate::json_error(
                429,
                "rate limit exceeded",
                "rate_limit_error",
            )?));
        }
        return Ok(Some(handle_tunnel_token(&mut request, env).await?));
    }

    // ── the device-token route (no session) ────────────────────────────────────────────────────────
    if method == Method::Post && path == "/api/devices/panel-grant/redeem" {
        return Ok(Some(handle_panel_grant_redeem(&mut request, env).await?));
    }

    // ── the admin routes, in the plugin's registration order ───────────────────────────────────────
    if method == Method::Get && path == "/api/devices/register-keys" {
        if let Some(refusal) = admin_check(&request, env).await? {
            return Ok(Some(refusal));
        }
        return Ok(Some(json_ok(
            json!({ "keys": store::list_reg_keys(env).await }),
        )?));
    }
    if method == Method::Delete {
        // `^/api/devices/register-keys/[^/]+$` — registered BEFORE the generic single-segment delete, and it
        // cannot overlap it (the code segment has a slash in front of it).
        if let Some(rest) = path.strip_prefix("/api/devices/register-keys/") {
            if !rest.is_empty() && !rest.contains('/') {
                if let Some(refusal) = admin_check(&request, env).await? {
                    return Ok(Some(refusal));
                }
                // `if (!code) 400` — the regex already guarantees a non-empty segment, and `decodeDeviceName`
                // answers null for a malformed escape, which is the same arm.
                let Some(code) = decode_device_name(rest) else {
                    return Ok(Some(crate::json_error(
                        400,
                        "Invalid key",
                        "invalid_request",
                    )?));
                };
                store::delete_reg_key(env, &code).await;
                return Ok(Some(json_ok(json!({ "ok": true }))?));
            }
        }
    }
    if method == Method::Get && path == "/api/devices/install-cmd" {
        if let Some(refusal) = admin_check(&request, env).await? {
            return Ok(Some(refusal));
        }
        return Ok(Some(handle_install_cmd(env).await?));
    }
    if method == Method::Post {
        if let Some(name) = device_segment(&path, "/rename") {
            if let Some(refusal) = admin_check(&request, env).await? {
                return Ok(Some(refusal));
            }
            return Ok(Some(handle_rename(&mut request, env, name).await?));
        }
        if let Some(name) = device_segment(&path, "/panel-grant") {
            if let Some(refusal) = admin_check(&request, env).await? {
                return Ok(Some(refusal));
            }
            return Ok(Some(handle_panel_grant(env, name).await?));
        }
    }
    if path == DEVICE_BASE && (method == Method::Get || method == Method::Post) {
        if let Some(refusal) = admin_check(&request, env).await? {
            return Ok(Some(refusal));
        }
        if method == Method::Get {
            return Ok(Some(handle_devices_list(env).await?));
        }
        return Ok(Some(handle_devices_add(&mut request, env).await?));
    }
    if method == Method::Get {
        if let Some(name) = device_segment(&path, "/mcp") {
            if let Some(refusal) = admin_check(&request, env).await? {
                return Ok(Some(refusal));
            }
            return Ok(Some(handle_device_mcp(env, name).await?));
        }
    }
    if method == Method::Delete {
        if let Some(name) = device_segment(&path, "").filter(|rest| !rest.contains('/')) {
            if let Some(refusal) = admin_check(&request, env).await? {
                return Ok(Some(refusal));
            }
            return Ok(Some(handle_device_delete(env, name).await?));
        }
    }
    if method == Method::Post && path == "/api/devices/register-key" {
        if let Some(refusal) = admin_check(&request, env).await? {
            return Ok(Some(refusal));
        }
        let key = store::create_reg_key(env).await;
        return Ok(Some(json_ok(json!({ "ok": true, "key": key }))?));
    }
    Ok(None)
}

/// `requireAdmin(request, env)` — `None` when the caller is an admin, `Some(response)` when the handler must
/// return the refusal directly (401 not logged in, 403 not an admin).
async fn admin_check(request: &Request, env: &Env) -> Result<Option<Response>> {
    match session_gate::require_session(request, env).await {
        None => Ok(Some(crate::json_error(
            401,
            "Not logged in or session expired",
            "authentication_error",
        )?)),
        Some(user) if user.role != "admin" => Ok(Some(crate::json_error(
            403,
            "Admin permission required",
            "authorization_error",
        )?)),
        Some(_) => Ok(None),
    }
}

/// `gate(handler)` — the plugin's per-IP limiter, in front of the three public routes.
fn public_rate_limited(request: &Request) -> bool {
    let ip = request
        .headers()
        .get("cf-connecting-ip")
        .ok()
        .flatten()
        .filter(|ip| !ip.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let now = store::now_ms();
    let mut counters = store::PUBLIC_RATE_COUNTERS
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::ip_rate_limit::check(crate::ip_rate_limit::PUBLIC_RATE, &mut counters, &ip, now)
}

/* ─────────────────────────── the device dial ─────────────────────────── */

/// The answer `deviceFetch` gives its callers.
struct DeviceDial {
    /// `deviceFetch`'s `{ status, ok, resp, error }`, of which this slice's two dial sites read `resp` alone.
    /// The status is kept because it IS the contract — the reverse-proxy slice reads it, and a port that
    /// dropped it would have to re-derive it from the response.
    #[allow(dead_code)]
    status: u16,
    resp: Option<Response>,
}

/// `deviceFetch(env, device, "/api/status")` — the dial the registration paths make to read the device's
/// `proxy_secret`, with the SSRF guard stack the source documents: the authority-prefix rejection, the parsed
/// hostname equality, the private-IP blocklist, the suffix allowlist, and `redirect: "manual"`.
async fn device_fetch(env: &Env, hostname: &str, token: &str, rest_path: &str) -> DeviceDial {
    // round-120/121: a `restPath` carrying userinfo or a scheme can re-root the URL, and the token goes with it.
    // The check is narrowed to the AUTHORITY-relevant prefix (up to the first `/`, `?` or `#`) because an
    // at-sign in a query string is legitimate.
    let authority = rest_path.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') || starts_like_a_scheme(authority) {
        return DeviceDial {
            status: 400,
            resp: None,
        };
    }
    let path = if rest_path.starts_with('/') {
        rest_path.to_string()
    } else {
        format!("/{rest_path}")
    };
    let url = format!("https://{hostname}{path}");
    let parsed = match Url::parse(&url) {
        Ok(url) => url,
        Err(_) => {
            return DeviceDial {
                status: 400,
                resp: None,
            }
        }
    };
    if parsed.host_str().unwrap_or_default().to_lowercase() != hostname.to_lowercase() {
        return DeviceDial {
            status: 400,
            resp: None,
        };
    }
    if crate::device::device_host_error(parsed.host_str().unwrap_or_default()).is_some() {
        return DeviceDial {
            status: 400,
            resp: None,
        };
    }
    let suffix = env
        .var("DEVICE_HOST_SUFFIX")
        .ok()
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty());
    if crate::device::host_allow_error(hostname, suffix.as_deref()).is_some() {
        return DeviceDial {
            status: 400,
            resp: None,
        };
    }

    let headers = Headers::new();
    let _ = headers.set("Authorization", &format!("Bearer {token}"));
    let mut init = RequestInit::new();
    init.with_method(Method::Get);
    init.with_headers(headers);
    init.with_redirect(RequestRedirect::Manual);
    match fetch_with_timeout(&url, &init, 15_000).await {
        Ok(resp) => DeviceDial {
            status: resp.status_code(),
            resp: Some(resp),
        },
        Err(_) => DeviceDial {
            status: 502,
            resp: None,
        },
    }
}

/// `fetchWithTimeout(url, init, ms)` — the same shape `v1.rs` uses and for the same recorded reason: 0.8.7's
/// `RequestInit` has no `signal`, so this RACES the fetch against a `Delay`.
async fn fetch_with_timeout(url: &str, init: &RequestInit, timeout_ms: u64) -> Result<Response> {
    let request = Request::new_with_init(url, init)?;
    let fetcher = Fetch::Request(request);
    let fetch = fetcher.send();
    let delay = Delay::from(Duration::from_millis(timeout_ms));
    futures_util::pin_mut!(fetch);
    futures_util::pin_mut!(delay);
    match futures_util::future::select(fetch, delay).await {
        Either::Left((result, _)) => result,
        Either::Right((_, _)) => Err(Error::RustError("timeout".into())),
    }
}

/// `deviceFetch(env, device, "/api/status")` → the `proxy_secret` a NEW record should carry, when the device
/// answers with one at least 32 characters long. Every failure arm — unreachable, non-JSON, too short — is the
/// source's `catch`, and the caller carries on without a secret.
async fn probe_proxy_secret(env: &Env, hostname: &str, token: &str) -> Option<String> {
    let dial = device_fetch(env, hostname, token, "/api/status").await;
    let mut resp = dial.resp?;
    let text = resp.text().await.ok()?;
    let body: Value = serde_json::from_str(&text).ok()?;
    let secret = body.get("proxy_secret").and_then(Value::as_str)?;
    if secret.chars().count() >= 32 {
        Some(secret.to_string())
    } else {
        None
    }
}

/* ─────────────────────────── the handlers ─────────────────────────── */

/// `POST /api/register`.
async fn handle_register(request: &mut Request, env: &Env) -> Result<Response> {
    let body = read_json(request).await;
    let Some(key) = body_key(&body) else {
        return crate::json_error(500, "Internal error", "api_error");
    };
    // round-115: reject garbage keys BEFORE the claim lock — invalid keys are zero-write.
    if key.is_empty()
        || (!store::has_reg_key(env, &key).await && !store::has_reg_grant(env, &key).await)
    {
        return crate::json_error(
            403,
            "Invalid or used registration key",
            "authorization_error",
        );
    }
    // round-103: single-flight claim — a bare check-then-act let one reg key register TWO devices.
    if store::reg_claim(env, "regclaim2:", &key).await {
        return crate::json_error(
            403,
            "Registration key already in use",
            "authorization_error",
        );
    }
    store::take_reg_claim(env, "regclaim2:", &key).await;
    let response = register_claimed(env, &body, &key).await;
    // The `finally` arm, which runs on every path including the refusals.
    store::release_reg_claim(env, "regclaim2:", &key).await;
    response
}

/// The body of `handleRegister`'s `try`.
async fn register_claimed(env: &Env, body: &Value, key: &str) -> Result<Response> {
    let key_ok = store::has_reg_key(env, key).await || store::has_reg_grant(env, key).await;
    if !key_ok {
        return crate::json_error(
            403,
            "Invalid or used registration key",
            "authorization_error",
        );
    }
    let device = match registry::validate_device(body) {
        Ok(device) => device,
        Err(message) => return crate::json_error(400, &message, "invalid_request"),
    };
    if let Some(message) = host_allow_error(env, &device.hostname) {
        return crate::json_error(400, &message, "invalid_request");
    }
    // round-68: a one-time-key holder could upsert an EXISTING device name and redirect the console to itself.
    if store::get_device(env, &device.name).await.is_some() {
        return already_registered_conflict(&device.name);
    }
    let mut record = Map::new();
    record.insert("name".to_string(), Value::String(device.name.clone()));
    record.insert(
        "hostname".to_string(),
        Value::String(device.hostname.clone()),
    );
    record.insert("token".to_string(), Value::String(device.token.clone()));
    if let Some(secret) = probe_proxy_secret(env, &device.hostname, &device.token).await {
        record.insert("proxySecret".to_string(), Value::String(secret));
    }
    record.insert(
        "registeredAt".to_string(),
        Value::Number(store::now_ms().into()),
    );
    // round-122: the existence check runs INSIDE the lock, so two concurrent same-name registrations cannot
    // both pass it.
    if store::insert_device(env, Value::Object(record))
        .await
        .is_none()
    {
        return already_registered_conflict(&device.name);
    }
    store::delete_reg_key(env, key).await; // one-time — consumed only after success
    store::delete_reg_grant(env, key).await;
    json_ok(json!({ "ok": true, "device": { "name": device.name, "hostname": device.hostname } }))
}

/// `POST /api/devices/self-register` — the npm-installed agent registering with its OWN device token.
async fn handle_self_register(request: &mut Request, env: &Env) -> Result<Response> {
    let body = read_json(request).await;
    let device = match registry::validate_device(&body) {
        Ok(device) => device,
        Err(message) => return crate::json_error(400, &message, "invalid_request"),
    };
    if !registry::is_hex_of_len(&device.token, 64) {
        return crate::json_error(403, "Invalid device token", "authorization_error");
    }
    if let Some(message) = host_allow_error(env, &device.hostname) {
        return crate::json_error(400, &message, "invalid_request");
    }
    // **THE SECURITY FIX THIS ROUTE RECORDS**: the old proof dialled the CALLER-SUPPLIED hostname, so
    // `{name:"d1", hostname:"evil.com", token:<random>}` had the ATTACKER's server answer with a proxy secret,
    // "proving" ownership and taking over the real record. For an existing record, identity can only be proven
    // against the STORED hostname and only by returning the STORED proxy secret.
    if let Some(existing) = store::get_device(env, &device.name).await {
        let stored_hostname = registry::js_falsy_string(existing.get("hostname"));
        let stored_token = registry::js_falsy_string(existing.get("token"));
        if device.hostname.to_lowercase() != stored_hostname.to_lowercase() {
            return crate::json_error(
                409,
                &format!(
                    "Device '{}' hostname is fixed — change it from the console (admin)",
                    device.name
                ),
                "conflict",
            );
        }
        // The stored secret, if there is one: a truthy value, kept as it is stored (the source assigns the
        // VALUE, not a stringified copy).
        let stored_secret = existing
            .get("proxySecret")
            .filter(|v| !registry::js_falsy(v))
            .cloned();
        let same_token = crate::auth::safe_eq(&stored_token, &device.token);
        let secret_to_keep = if same_token {
            // `if (!device.proxySecret && existing.proxySecret) device.proxySecret = existing.proxySecret` —
            // and `device` here is the validated body, which never carries a secret, so this is always taken
            // when one is stored.
            stored_secret.clone()
        } else {
            // Token rotation needs proof from the STORED tunnel: its `/api/status` must answer with the
            // proxy_secret already on record.
            let proved = match &stored_secret {
                Some(secret) => {
                    let dial =
                        device_fetch(env, &stored_hostname, &stored_token, "/api/status").await;
                    match dial.resp {
                        Some(mut resp) => resp
                            .text()
                            .await
                            .ok()
                            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                            .and_then(|body| {
                                body.get("proxy_secret")
                                    .and_then(Value::as_str)
                                    .map(str::to_string)
                            })
                            .is_some_and(|answer| {
                                answer == registry::js_falsy_string(Some(secret))
                            }),
                        None => false,
                    }
                }
                None => false,
            };
            if !proved {
                return crate::json_error(
                    409,
                    &format!(
                        "Device '{}' already registered with a different token — use the console (admin)",
                        device.name
                    ),
                    "conflict",
                );
            }
            stored_secret.clone()
        };
        // Idempotent refresh: keep the original registration date AND hostname.
        let registered_at = existing
            .get("registeredAt")
            .cloned()
            .unwrap_or_else(|| Value::Number(store::now_ms().into()));
        let mut record = Map::new();
        record.insert("name".to_string(), Value::String(device.name.clone()));
        record.insert(
            "hostname".to_string(),
            Value::String(stored_hostname.clone()),
        );
        record.insert("token".to_string(), Value::String(device.token.clone()));
        if let Some(secret) = secret_to_keep {
            record.insert("proxySecret".to_string(), secret);
        }
        record.insert("registeredAt".to_string(), registered_at);
        store::upsert_device(env, Value::Object(record)).await;
        return json_ok(json!({
            "ok": true,
            "device": { "name": device.name, "hostname": stored_hostname }
        }));
    }
    // New device: still constrained to the agent-host suffix (SSRF) before it can be proxied.
    if let Some(message) = host_allow_error(env, &device.hostname) {
        return crate::json_error(400, &message, "invalid_request");
    }
    let mut record = Map::new();
    record.insert("name".to_string(), Value::String(device.name.clone()));
    record.insert(
        "hostname".to_string(),
        Value::String(device.hostname.clone()),
    );
    record.insert("token".to_string(), Value::String(device.token.clone()));
    if let Some(secret) = probe_proxy_secret(env, &device.hostname, &device.token).await {
        record.insert("proxySecret".to_string(), Value::String(secret));
    }
    record.insert(
        "registeredAt".to_string(),
        Value::Number(store::now_ms().into()),
    );
    if store::insert_device(env, Value::Object(record))
        .await
        .is_none()
    {
        return crate::json_error(
            409,
            &format!("Device '{}' already registered", device.name),
            "conflict",
        );
    }
    json_ok(json!({
        "ok": true,
        "device": { "name": device.name, "hostname": device.hostname }
    }))
}

/// `POST /api/install/tunnel-token` — the account-level CF API token, spent once per registration key.
async fn handle_tunnel_token(request: &mut Request, env: &Env) -> Result<Response> {
    let body = read_json(request).await;
    let Some(key) = body_key(&body) else {
        return crate::json_error(500, "Internal error", "api_error");
    };
    if key.is_empty() || !store::has_reg_key(env, &key).await {
        return crate::json_error(
            403,
            "Invalid or used registration key",
            "authorization_error",
        );
    }
    // Single-flight: claim FIRST, then consume — hasRegKey→consume was check-then-act on eventually-consistent
    // KV, so two concurrent requests could both harvest the account-level CF token.
    if store::reg_claim(env, "regclaim:", &key).await {
        return crate::json_error(
            403,
            "Registration key already in use",
            "authorization_error",
        );
    }
    store::take_reg_claim(env, "regclaim:", &key).await;
    if !store::has_reg_key(env, &key).await {
        store::release_reg_claim(env, "regclaim:", &key).await;
        return crate::json_error(
            403,
            "Invalid or used registration key",
            "authorization_error",
        );
    }
    store::consume_reg_key(env, &key).await;
    let api_token = store::cf_token(env).await;
    json_ok(json!({ "ok": true, "apiToken": api_token }))
}

/// `GET /api/devices`.
async fn handle_devices_list(env: &Env) -> Result<Response> {
    let devices = store::list_devices(env).await;
    let rows: Vec<Value> = devices.iter().map(registry::device_row).collect();
    json_ok(json!({ "devices": rows }))
}

/// `POST /api/devices` — the admin add/update.
async fn handle_devices_add(request: &mut Request, env: &Env) -> Result<Response> {
    let body = read_json(request).await;
    let device = match registry::validate_device(&body) {
        Ok(device) => device,
        Err(message) => return crate::json_error(400, &message, "invalid_request"),
    };
    // THE SAME GATE THE OTHER WRITE PATHS APPLY: `validateDevice` checks the SHAPE of a hostname, not an
    // allowlist, and without this a console session could point the proxy (and the device's permanent token) at
    // any host it liked.
    if let Some(message) = host_allow_error(env, &device.hostname) {
        return crate::json_error(400, &message, "invalid_request");
    }
    let existing = store::get_device(env, &device.name).await;
    let registered_at = existing
        .as_ref()
        .and_then(|d| d.get("registeredAt"))
        .cloned()
        .unwrap_or_else(|| Value::Number(store::now_ms().into()));
    let mut record = Map::new();
    record.insert("name".to_string(), Value::String(device.name.clone()));
    record.insert(
        "hostname".to_string(),
        Value::String(device.hostname.clone()),
    );
    record.insert("token".to_string(), Value::String(device.token.clone()));
    record.insert("registeredAt".to_string(), registered_at);
    let saved = store::upsert_device(env, Value::Object(record)).await;
    let saved_token = registry::js_falsy_string(saved.get("token"));
    json_ok(json!({
        "ok": true,
        "device": {
            "name": registry::js_falsy_string(saved.get("name")),
            "hostname": registry::js_falsy_string(saved.get("hostname")),
            "token": registry::mask_key(&saved_token)
        }
    }))
}

/// `GET /api/devices/<name>/mcp`.
async fn handle_device_mcp(env: &Env, segment: &str) -> Result<Response> {
    let Some(name) = decode_device_name(segment) else {
        return crate::json_error(400, "Invalid device name", "invalid_request");
    };
    let Some(device) = store::get_device(env, &name).await else {
        return crate::json_error(404, "Device not found", "not_found_error");
    };
    json_ok(json!({
        "name": registry::js_falsy_string(device.get("name")),
        "hostname": registry::js_falsy_string(device.get("hostname")),
        "mcp": mcp_object(&device)
    }))
}

/// `mcpConfig(d)` as the `{url, json}` object a response embeds.
fn mcp_object(device: &Value) -> Value {
    let (url, json) = registry::mcp_config(
        device.get("hostname").unwrap_or(&Value::Null),
        device.get("token").unwrap_or(&Value::Null),
    );
    json!({ "url": url, "json": json })
}

/// `DELETE /api/devices/<name>`.
async fn handle_device_delete(env: &Env, segment: &str) -> Result<Response> {
    let Some(name) = decode_device_name(segment) else {
        return crate::json_error(400, "Invalid device name", "invalid_request");
    };
    // round-115: deleteDevice alone left the device's plugin links alive, so a same-name re-registration
    // resurrected the old pairing's browser control.
    store::remove_plugin_links_for_device(env, &name).await;
    store::delete_device(env, &name).await;
    json_ok(json!({ "ok": true }))
}

/// `POST /api/devices/<name>/rename`.
async fn handle_rename(request: &mut Request, env: &Env, segment: &str) -> Result<Response> {
    let Some(old_name) = decode_device_name(segment) else {
        return crate::json_error(400, "Invalid device name", "invalid_request");
    };
    let body = read_json(request).await;
    let new_name = registry::js_falsy_string(body.get("name"))
        .trim()
        .to_string();
    let hostname = registry::js_falsy_string(body.get("hostname"))
        .trim()
        .to_string();
    if let Some(message) = registry::rename_shape_error(&new_name, &hostname) {
        return crate::json_error(400, &message, "invalid_request");
    }
    // SHAPE IS NOT AN ALLOWLIST: rename PRESERVES the token and proxy secret, so a hostile hostname here hands
    // the next proxy call credentials the device already had.
    if !hostname.is_empty() {
        if let Some(message) = host_allow_error(env, &hostname) {
            return crate::json_error(400, &message, "invalid_request");
        }
    }
    let updated = store::rename_device(
        env,
        &old_name,
        &new_name,
        if hostname.is_empty() {
            None
        } else {
            Some(hostname.as_str())
        },
    )
    .await;
    let updated = match updated {
        registry::RenameOutcome::NotFound => {
            return crate::json_error(404, "Device not found", "not_found_error")
        }
        registry::RenameOutcome::NameTaken => {
            return crate::json_error(
                409,
                &format!("Device '{new_name}' already registered"),
                "conflict",
            )
        }
        registry::RenameOutcome::Renamed(updated) => updated,
    };
    store::migrate_plugin_links(env, &old_name, &new_name).await;
    json_ok(json!({
        "ok": true,
        "device": {
            "name": registry::js_falsy_string(updated.get("name")),
            "hostname": registry::js_falsy_string(updated.get("hostname")),
            "token": registry::mask_key(&registry::js_falsy_string(updated.get("token")))
        }
    }))
}

/// `POST /api/devices/<name>/panel-grant` — mint a 120 s single-use grant.
async fn handle_panel_grant(env: &Env, segment: &str) -> Result<Response> {
    let Some(name) = decode_device_name(segment) else {
        return crate::json_error(400, "Invalid device name", "invalid_request");
    };
    let Some(device) = store::get_device(env, &name).await else {
        return crate::json_error(404, "Device not found", "not_found_error");
    };
    let hostname = registry::js_falsy_string(device.get("hostname"));
    let code = store::create_panel_grant(env, &registry::js_falsy_string(device.get("name"))).await;
    json_ok(json!({ "ok": true, "url": format!("https://{hostname}/panel/?grant={code}") }))
}

/// `POST /api/devices/panel-grant/redeem` — the AGENT consuming a grant with its own Bearer token.
async fn handle_panel_grant_redeem(request: &mut Request, env: &Env) -> Result<Response> {
    let auth = request
        .headers()
        .get("authorization")
        .ok()
        .flatten()
        .unwrap_or_default();
    let token = auth
        .strip_prefix("Bearer ")
        .map(|t| t.trim().to_string())
        .unwrap_or_default();
    if token.is_empty() {
        return crate::json_error(401, "Missing device token", "authentication_error");
    }
    // The device registry is small; the scan compares with `safeEq` and never short-circuits.
    let caller = store::list_devices(env).await.into_iter().find(|d| {
        d.get("token")
            .and_then(Value::as_str)
            .is_some_and(|t| t.chars().count() >= 32 && crate::auth::safe_eq(t, &token))
    });
    let Some(caller) = caller else {
        return crate::json_error(401, "Invalid device token", "authentication_error");
    };
    let body = read_json(request).await;
    let code = registry::js_falsy_string(body.get("grant"))
        .trim()
        .to_string();
    let Some(grant) = store::get_panel_grant(env, &code).await else {
        return crate::json_error(404, "Grant not found or expired", "not_found_error");
    };
    if grant.device != registry::js_falsy_string(caller.get("name")) {
        return crate::json_error(
            403,
            "Grant does not match this device",
            "authorization_error",
        );
    }
    // The delete runs FIRST so a crash between delete and respond fails closed (no grant left to retry).
    store::delete_panel_grant(env, &code).await;
    json_ok(json!({ "ok": true }))
}

/// `GET /api/devices/install-cmd` — the CURRENT npm install version, cached 5 minutes in-isolate.
async fn handle_install_cmd(env: &Env) -> Result<Response> {
    let now = store::now_ms();
    let cached = INSTALL_CMD_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let entry = match cached {
        Some((at, version, download)) if now - at <= INSTALL_CMD_TTL_MS => {
            Some((version, download))
        }
        _ => None,
    };
    let (version, download) = match entry {
        Some(entry) => entry,
        None => {
            let (version, download) = fetch_install_manifest(env).await;
            if let Ok(mut cache) = INSTALL_CMD_CACHE.lock() {
                *cache = Some((now, version.clone(), download.clone()));
            }
            (version, download)
        }
    };
    json_ok(registry::install_doc(
        version.as_deref(),
        download.as_deref(),
    ))
}

/// `installSource(env)` → the manifest, with the source's three failure arms (unreachable, not ok, not a JSON
/// object with a string `version`) all leaving `version` null so the page falls back.
async fn fetch_install_manifest(env: &Env) -> (Option<String>, Option<String>) {
    let base = env
        .var("INDEX_WORKER_URL")
        .ok()
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| INDEX_WORKER_BASE_DEFAULT.to_string());
    let source = format!("{base}/api/version");
    let mut init = RequestInit::new();
    init.with_method(Method::Get);
    let Ok(mut response) = fetch_with_timeout(&source, &init, 8_000).await else {
        return (None, None);
    };
    // `res.ok` — a 4xx/5xx is not a manifest, and the page falls back rather than showing an error.
    if !(200..300).contains(&response.status_code()) {
        return (None, None);
    }
    let Ok(text) = response.text().await else {
        return (None, None);
    };
    let Ok(body) = serde_json::from_str::<Value>(&text) else {
        return (None, None);
    };
    match body.get("version").and_then(Value::as_str) {
        Some(version) => (
            Some(version.to_string()),
            body.get("download")
                .and_then(Value::as_str)
                .map(str::to_string),
        ),
        None => (None, None),
    }
}

/// `hostAllowError(hostname, env)` — the suffix allowlist, read from the same var the source reads.
fn host_allow_error(env: &Env, hostname: &str) -> Option<String> {
    let suffix = env
        .var("DEVICE_HOST_SUFFIX")
        .ok()
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty());
    crate::device::host_allow_error(hostname, suffix.as_deref())
}

/// `alreadyRegisteredConflict(name)`.
fn already_registered_conflict(name: &str) -> Result<Response> {
    crate::json_error(
        409,
        &format!("Device '{name}' already registered — use the console (admin) to update it"),
        "conflict",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The route table's SHAPE, which is what the front door's cutover and this dispatcher must agree on: every
    /// ported route is in the family, the reverse proxy is not, and a path nobody serves is still the family (so
    /// that the 404 comes from the implementation that owns the prefix).
    #[test]
    fn the_family_and_its_one_exclusion() {
        for (method, path) in [
            (Method::Post, "/api/register"),
            (Method::Post, "/api/devices/self-register"),
            (Method::Post, "/api/install/tunnel-token"),
            (Method::Post, "/api/devices/panel-grant/redeem"),
            (Method::Get, "/api/devices"),
            (Method::Post, "/api/devices"),
            (Method::Get, "/api/devices/register-keys"),
            (Method::Delete, "/api/devices/register-keys/abc"),
            (Method::Get, "/api/devices/install-cmd"),
            (Method::Post, "/api/devices/register-key"),
            (Method::Get, "/api/devices/d1/mcp"),
            (Method::Delete, "/api/devices/d1"),
            (Method::Post, "/api/devices/d1/rename"),
            (Method::Post, "/api/devices/d1/panel-grant"),
            (Method::Get, "/api/devices/nope/nope/nope"),
        ] {
            assert!(
                in_family(&method, path),
                "{method:?} {path} is in the family"
            );
        }
        assert!(!in_family(&Method::Get, "/api/devices/d1/proxy/panel/"));
        assert!(!in_family(&Method::Post, "/api/devices/d1/proxy/mcp"));
        assert!(!in_family(&Method::Post, "/api/upload"));
        assert!(!in_family(&Method::Get, "/api/health"));
        assert!(!in_family(&Method::Get, "/api/install/tunnel-token"));
    }

    /// `decodeDeviceName` — the escapes it must accept, and the three shapes that made the source's
    /// `decodeURIComponent` throw an unhandled 500.
    #[test]
    fn the_name_decoder_matches_decode_uri_component() {
        assert_eq!(decode_device_name("d1"), Some("d1".to_string()));
        assert_eq!(decode_device_name("d%31"), Some("d1".to_string()));
        assert_eq!(decode_device_name("a%2Fb"), Some("a/b".to_string()));
        assert_eq!(decode_device_name("%E4%B8%AD"), Some("中".to_string()));
        assert_eq!(
            decode_device_name("a+b"),
            Some("a+b".to_string()),
            "`+` is not a space here"
        );
        assert_eq!(decode_device_name("%zz"), None);
        assert_eq!(decode_device_name("%2"), None);
        assert_eq!(
            decode_device_name("%FF"),
            None,
            "invalid UTF-8 is a URIError"
        );
    }

    /// `device_segment` is the three regexes' common shape, including what `[^/]+` refuses.
    #[test]
    fn the_segment_matchers_refuse_a_slash() {
        assert_eq!(device_segment("/api/devices/d1/mcp", "/mcp"), Some("d1"));
        assert_eq!(device_segment("/api/devices/d1/a/mcp", "/mcp"), None);
        assert_eq!(device_segment("/api/devices//mcp", "/mcp"), None);
        assert_eq!(device_segment("/api/devicesx/d1/mcp", "/mcp"), None);
        assert_eq!(device_segment("/api/devices/d1", ""), Some("d1"));
        assert_eq!(device_segment("/api/devices/d1/x", ""), None);
    }
}
