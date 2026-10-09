//! `gateway/src/plugins/device-proxy.ts` — THE DEVICE REVERSE PROXY, AND THE PANEL REWRITE IT CARRIES.
//!
//! `<any> /api/devices/<name>/proxy/<rest>` → reverse-proxy to the device panel. Auth: an admin session cookie,
//! OR a paired plugin token (`Authorization: Bearer`), OR the per-device `summrise_pt_<name>` cookie the
//! `?token=` 302 bootstrap mints. The plugin token grants access ONLY to the device it is paired to — no other
//! device, no admin APIs, no `/api/me`.
//!
//! **THE THREE DECISIONS THIS MODULE EXISTS TO CARRY, EACH WITH ITS REASON IN THE SOURCE:**
//!
//!   * **A 404 FOR A DEVICE THAT DOES NOT EXIST IS ONLY FOR AN ADMIN.** The source: "the 404 here was an
//!     UNAUTHENTICATED device-name oracle (probe names → 404 vs 401)". Everyone else gets the 401 envelope.
//!   * **`?token=` IS ACCEPTED ONLY FOR A TOP-LEVEL NAVIGATION** (`Sec-Fetch-Mode: navigate`, which a browser
//!     sets and `fetch()` cannot spoof) — a leaked URL in history, a sync log or a screenshot would otherwise
//!     grant terminal control for the plugin link's 30-day TTL. The navigation then gets a 302 to the SAME url
//!     with `?token=` stripped and the token pinned in a PER-DEVICE cookie: the token never reaches the
//!     omnibox, and the cookie is minted whether or not the panel boots. The cookie's key carries the device
//!     name, because one origin-wide cookie would let a later-opened device's page steal an earlier device's
//!     terminal.
//!   * **THE CATALOGUE THE OTHER DOOR CURATES IS ENFORCED HERE TOO** — `tool_policy.rs`, applied before the
//!     dial: one proxied POST to `/api/tools/terminal_sftp` used to route around `/mcp`'s curation entirely.
//!
//! **AND `rewrite_device_body` IS A SECURITY TRANSFORM, NOT A PRETTY-PRINTER.** It rewrites the panel's
//! absolute paths to the proxy mount (or the proxied panel's `/api/eventstream` and every other absolute call
//! would leave the mount), and it strips `window.__PANEL_TOKEN__` — the device's PERMANENT token, which the
//! panel serves into its own HTML. Without the strip, an extension user could read that token off a
//! console-origin page and keep direct control of the device after the plugin link expired or was revoked,
//! which is the exact scope the plugin token exists to enforce. Both rewrites are proved case for case by
//! `gateway/wasm/verify.mjs` and pinned by unit tests in this file.

use serde_json::Value;
use worker::worker_sys::ext::ResponseExt;
use worker::*;

use crate::auth::parse_cookie;
use crate::device::{device_fetch, DialBody, DialInit};
use crate::device_registry as registry;
use crate::device_store as store;
use crate::devices::decode_device_name;
use crate::session_gate;
use crate::tool_policy;
use crate::RouteFailure;

/// `DEVICE_BASE` — the `/api/devices` base path, shared by this module and the registry's routes.
pub const DEVICE_BASE: &str = "/api/devices";

/// `Max-Age=2592000` — the plugin link's 30-day TTL, in seconds.
const PLUGIN_LINK_MAX_AGE: i64 = 2_592_000;

/// `/^\/api\/devices\/[^/]+\/proxy/` — the route's own match function, and therefore this module's family
/// predicate: the front door hands the path over on THIS test and `handle` answers it, so the two cannot drift.
pub fn in_family(_method: &Method, path: &str) -> bool {
    proxy_parts(path).is_some()
}

/// The handler's own regex, `^/api/devices/([^/]+)/proxy(.*)$`: the raw (still percent-encoded) name segment
/// and the rest of the path — which is NOT required to start with a slash and is `(.*)`, so
/// `/api/devices/d1/proxyfoo` is the path `foo` on device `d1`, exactly as the source's capture group says.
fn proxy_parts(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix(DEVICE_BASE)?.strip_prefix('/')?;
    let (name, tail) = rest.split_once('/')?;
    if name.is_empty() {
        return None;
    }
    Some((name, tail.strip_prefix("proxy")?))
}

/// The dispatcher. `Ok(None)` is the plugin dispatch's `null`: no route in this module matched, and the front
/// door answers its own 404 (which is what `/api/devices//proxy/x` gets — a path under the prefix that the
/// route's regex does not match).
pub async fn handle(request: Request, env: &Env) -> Result<Option<Response>, RouteFailure> {
    let url = request.url()?;
    let Some((raw_name, rest)) = proxy_parts(url.path()) else {
        return Ok(None);
    };
    let response = handle_device_proxy(request, env, &url, raw_name, rest).await?;
    Ok(Some(response))
}

/// `handleDeviceProxy(request, env, url)`.
async fn handle_device_proxy(
    request: Request,
    env: &Env,
    url: &Url,
    raw_name: &str,
    rest: &str,
) -> Result<Response, RouteFailure> {
    // round-106/107: a malformed percent-escape in the device name (e.g. `%zz`) made `decodeURIComponent`
    // throw URIError — an unhandled 500. 400 instead.
    let Some(device_name) = decode_device_name(raw_name) else {
        return Ok(crate::json_error(400, "Invalid device name", "invalid_request")?);
    };
    let device = store::get_device(env, &device_name).await;
    let user = session_gate::require_session(&request, env).await?;
    let is_admin = user
        .as_ref()
        .and_then(|u| u.get("role"))
        .and_then(|role| role.as_str())
        == Some("admin");
    let Some(device) = device else {
        // Unveil existence only to admin sessions — see the module header.
        if is_admin {
            return Ok(crate::json_error(404, "Device not found", "not_found_error")?);
        }
        return Ok(crate::json_error(
            401,
            "Not logged in or invalid plugin token",
            "authentication_error",
        )?);
    };
    if is_admin {
        return proxy_device(&request, env, &device, if rest.is_empty() { "/" } else { rest }).await;
    }

    let auth = header_of(&request, "authorization");
    let q_token = url
        .query_pairs()
        .find(|(key, _)| key == "token")
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default();
    let is_nav = header_of(&request, "sec-fetch-mode") == "navigate";
    if !q_token.is_empty() && auth.is_empty() && !is_nav {
        return Ok(crate::json_error(401, "Invalid plugin token", "authentication_error")?);
    }
    // The cookie was written with `encodeURIComponent` — decode on read so a future non-hex token charset
    // (base64 `+/=`) still matches the plugin-link map. A malformed value is treated as ABSENT, which is what
    // the source's `catch` does.
    let cookie_token = {
        let raw = parse_cookie(&header_of(&request, "cookie"))
            .into_iter()
            .find(|(key, _)| *key == format!("summrise_pt_{device_name}"))
            .map(|(_, value)| value)
            .unwrap_or_default();
        decode_device_name(&raw).unwrap_or_default()
    };
    let bearer = match auth.strip_prefix("Bearer ") {
        Some(rest) => rest.trim().to_string(),
        None => String::new(),
    };
    let token = if !bearer.is_empty() {
        bearer
    } else if !q_token.is_empty() {
        q_token.clone()
    } else {
        cookie_token
    };
    let link = if token.is_empty() {
        None
    } else {
        store::get_plugin_by_token(env, &token).await
    };
    let linked = link
        .as_ref()
        .and_then(|link| link.get("device"))
        .and_then(|device| device.as_str())
        == Some(device_name.as_str());
    if linked {
        // round-124: a top-level navigation carrying `?token=` is the ONLY way the per-device cookie is
        // minted. The old code proxied the panel and appended Set-Cookie — the token stayed in the
        // omnibox/history until the panel JS scrubbed it, and if the panel failed to boot (device offline →
        // 502 body) it stayed forever. 302 to the SAME url with `?token=` stripped + Set-Cookie.
        if !q_token.is_empty() && is_nav {
            let cleaned = search_params_delete(url.query().unwrap_or_default(), "token");
            let location = if cleaned.is_empty() {
                url.path().to_string()
            } else {
                format!("{}?{cleaned}", url.path())
            };
            let headers = Headers::new();
            let _ = headers.set("Location", &location);
            let _ = headers.set(
                "Set-Cookie",
                &format!(
                    "summrise_pt_{device_name}={}; Path={DEVICE_BASE}/{device_name}/proxy; HttpOnly; Secure; \
                     SameSite=Lax; Max-Age={PLUGIN_LINK_MAX_AGE}",
                    crate::routing::encode_uri_component(&q_token)
                ),
            );
            // round-126: a cached 302 would drop the Set-Cookie on a re-pair (stale cookie → panel 401s
            // forever).
            let _ = headers.set("Cache-Control", "no-store");
            return Ok(Response::empty()?.with_status(302).with_headers(headers));
        }
        // Never cache a response that carried a token in the URL.
        let mut response = proxy_device(
            &request,
            env,
            &device,
            if rest.is_empty() { "/" } else { rest },
        )
        .await?;
        let _ = response.headers_mut().set("Cache-Control", "no-store");
        return Ok(response);
    }
    // A top-level navigation with a bad/expired token gets a readable page (with a re-pair hint) instead of a
    // raw JSON 401 — the panel's own recovery UI can never load if the bootstrap navigation itself 401s.
    if auth.is_empty() && is_nav {
        let mut response = Response::from_body(ResponseBody::Body(SESSION_EXPIRED_HTML.as_bytes().to_vec()))?;
        let headers = response.headers_mut();
        let _ = headers.set("content-type", "text/html; charset=utf-8");
        return Ok(response.with_status(401));
    }
    Ok(crate::json_error(
        401,
        "Not logged in or invalid plugin token",
        "authentication_error",
    )?)
}

/// The session-expired page, byte for byte from the source.
const SESSION_EXPIRED_HTML: &str = r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Summrise — session expired</title><style>body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;background:#f5f5f7;color:#1d1d1f;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0}.card{background:#fff;border:1px solid rgba(0,0,0,.08);border-radius:14px;padding:32px 40px;max-width:400px;text-align:center;box-shadow:0 12px 32px rgba(0,0,0,.12)}h1{font-size:18px;margin:0 0 8px}p{color:#6e6e73;font-size:14px;margin:0 0 4px}.mark{display:inline-flex;align-items:center;justify-content:center;width:44px;height:44px;border-radius:10px;background:#1d1d1f;color:#fff;font-weight:700;font-size:24px;margin-bottom:14px}</style></head><body><div class="card"><span class="mark">V</span><h1>Device session expired</h1><p>This device pairing has expired or the browser was restarted.</p><p>Expired session — sign in to the console again to get a fresh device link.</p></div></body></html>"##;

/// `proxyDevice(request, env, device, restPath)` — the catalogue check, the header hygiene, the dial, and the
/// four response shapes the source distinguishes.
async fn proxy_device(
    request: &Request,
    env: &Env,
    device: &Value,
    rest_path: &str,
) -> Result<Response, RouteFailure> {
    // THE CATALOGUE THE OTHER DOOR CURATES, ENFORCED HERE TOO. 403, not 401: the console treats any 401 as a
    // dead session and ejects the operator.
    if let Some(name) = tool_segment(rest_path) {
        // `decodeURIComponent(tool[1] ?? "")` — a malformed escape THROWS here (the source has no try), which
        // is the front door's 500.
        let decoded = decode_device_name(&name).ok_or(RouteFailure::Threw)?;
        if let Err(reason) = tool_policy::proxy_may_reach_tool(&decoded) {
            return Ok(crate::json_error(
                403,
                &format!("The device proxy does not relay {decoded}: {reason}"),
                "forbidden",
            )?);
        }
    }

    let url = request.url()?;
    // The panel sits behind a tunnel that adds its own `x-forwarded-*`; don't pass the console's through.
    // `deviceFetch` injects the Bearer token and strips host/cookie; `restPath` carries the query string.
    let mut headers: Vec<(String, String)> = request.headers().entries().collect();
    for doomed in [
        "x-forwarded-proto",
        "x-forwarded-for",
        "cf-connecting-ip",
        "x-summrise-auth",
    ] {
        headers.retain(|(name, _)| !name.eq_ignore_ascii_case(doomed));
    }
    headers.push(("x-forwarded-proto".to_string(), "https".to_string()));
    // round-103: the device's /panel/ injects its Bearer token ONLY when the request carries the shared proxy
    // secret (X-Summrise-Auth) — the R102 marker header was client-spoofable end-to-end. Strip inbound FIRST:
    // a client-sent x-summrise-auth must never ride through when the record has no proxySecret.
    if let Some(secret) = device.get("proxySecret") {
        if registry::js_truthy(secret) {
            headers.push((
                "x-summrise-auth".to_string(),
                registry::js_to_string(secret),
            ));
        }
    }

    // Never forward the extension's `?token=` to the device — the plugin token is a console-side credential. A
    // leaked token must not reach device query logs.
    let query = search_params_delete(url.query().unwrap_or_default(), "token");
    let dial_path = if query.is_empty() {
        rest_path.to_string()
    } else {
        format!("{rest_path}?{query}")
    };

    // Round-55/56: the bound applies to EVERY non-POST method; POST is bounded at 60 s. `redirect: "manual"` is
    // inside the dial (it is the same guard for every caller).
    let method = request.method();
    let timeout_ms = if method == Method::Post { 60_000 } else { 15_000 };
    let body = if method == Method::Get || method == Method::Head {
        DialBody::None
    } else {
        match request.inner().body() {
            Some(stream) => DialBody::Stream(stream),
            None => DialBody::None,
        }
    };
    let init = DialInit {
        method,
        body,
        headers,
        timeout_ms,
    };
    let dial = device_fetch(
        env,
        &registry::js_to_string_of(device.get("hostname")),
        &registry::js_to_string_of(device.get("token")),
        &dial_path,
        init,
    )
    .await;
    // `new URL(…)` sits OUTSIDE the source's try, so an unparseable upstream propagates a TypeError to the
    // front door's catch rather than answering 400 here. `device_fetch` models that arm as `threw`.
    if dial.threw.is_some() {
        return Err(RouteFailure::Threw);
    }
    let Some(mut upstream) = dial.resp else {
        return Ok(crate::json_error(
            502,
            dial.error.as_deref().unwrap_or("Device unreachable"),
            "proxy_error",
        )?);
    };

    let mut out: Vec<(String, String)> = upstream.headers().entries().collect();
    // CORS: reflect-if-allowlisted (console origins + loopback) with Vary — NO wildcard. `stampCors` on a
    // header set that may already carry the upstream's own ACAO, so refusing has to REMOVE it.
    stamp_cors_like_ts(
        &mut out,
        &header_of(request, "origin"),
        url.host_str(),
        env.var("CONSOLE_ORIGINS").ok().map(|v| v.to_string()).as_deref(),
    );
    let content_type = out
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.to_lowercase())
        .unwrap_or_default();
    let status = upstream.status_code();

    // `build101Response(resp) ?? resp` — a WebSocket upgrade is rebuilt with its socket, and a repack failure
    // passes the response through untouched (workerd #3047). A 101 with NO `webSocket` is the source's
    // `new Response(resp.body || null, {status: 101, …})`, which the runtime refuses with a RangeError: that
    // arm is a THROW, and the front door's catch answers it — reproduced rather than quietly turned into a
    // passthrough the source does not have.
    if status == 101 {
        // `resp.webSocket` is read off the response the runtime handed us, exactly as `build101Response` reads
        // the property — which is why the response is taken apart rather than asked through the typed wrapper
        // (`Response::websocket` CONSUMES the wrapper and would leave nothing to pass through).
        let raw: web_sys::Response = upstream.into();
        if let Some(websocket) = raw.websocket() {
            return match Response::from_websocket(websocket.into()) {
                Ok(rebuilt) => Ok(rebuilt),
                Err(_) => Ok(Response::from(raw)),
            };
        }
        return Err(RouteFailure::Threw);
    }

    let has_body = !matches!(upstream.body(), ResponseBody::Empty);
    // Streaming (SSE / octet-stream): pass the body through untouched.
    if has_body
        && (content_type.contains("text/event-stream")
            || content_type.contains("application/octet-stream"))
    {
        let stream = upstream.stream().map_err(RouteFailure::Platform)?;
        return Ok(Response::from_stream(stream)?
            .with_status(status)
            .with_headers(headers_from(&out)));
    }
    // Text assets (HTML/JS/CSS): rewrite absolute panel paths to the proxy mount.
    if has_body && content_type.contains("text/") {
        let text = upstream.text().await?;
        let rewritten = rewrite_device_body(&text, &registry::js_to_string_of(device.get("name")));
        set_content_length_if_present(&mut out, rewritten.len());
        return Ok(
            Response::from_body(ResponseBody::Body(rewritten.into_bytes()))?
                .with_status(status)
                .with_headers(headers_from(&out)),
        );
    }
    // JSON / binary: pass through — EXCEPT strip the `proxy_secret` (round-104: a plugin-token holder proxying
    // `/api/status` could read the secret and escalate to the permanent device token, defeating unpair/revoke
    // scope).
    if has_body && content_type.contains("application/json") {
        let text = upstream.text().await?;
        if let Ok(Value::Object(mut object)) = serde_json::from_str::<Value>(&text) {
            if object.contains_key("proxy_secret") {
                // **`shift_remove`, NOT `remove`.** A `serde_json::Map` under `preserve_order` is an
                // `IndexMap`, and its `remove` is `swap_remove`: it moves the LAST entry into the hole, so a
                // blob that was read and re-serialised would come back with its keys reordered. The
                // TypeScript's `delete` keeps the order, and the order is the wire bytes here.
                object.shift_remove("proxy_secret");
                let out_text = crate::responses::stringify_like_json(&Value::Object(object));
                set_content_length_if_present(&mut out, out_text.len());
                return Ok(
                    Response::from_body(ResponseBody::Body(out_text.into_bytes()))?
                        .with_status(status)
                        .with_headers(headers_from(&out)),
                );
            }
        }
        return Ok(
            Response::from_body(ResponseBody::Body(text.into_bytes()))?
                .with_status(status)
                .with_headers(headers_from(&out)),
        );
    }
    if has_body {
        let stream = upstream.stream().map_err(RouteFailure::Platform)?;
        return Ok(Response::from_stream(stream)?
            .with_status(status)
            .with_headers(headers_from(&out)));
    }
    Ok(Response::empty()?
        .with_status(status)
        .with_headers(headers_from(&out)))
}

/// `/^\/api\/tools\/([^/?#]+)/` — the tool name a proxied path addresses, if any.
fn tool_segment(rest_path: &str) -> Option<String> {
    let rest = rest_path.strip_prefix("/api/tools/")?;
    let end = rest
        .find(['/', '?', '#'])
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    Some(rest[..end].to_string())
}

/// A request header, or `""` — the source's `String(headers.get(x) || "")`.
fn header_of(request: &Request, name: &str) -> String {
    request
        .headers()
        .get(name)
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// `stampCors(request, headers, env)` — the ORDER of the two branches is the source's, and the difference from
/// `cors.rs::stamp_cors` is deliberate and measured: the `else` branch deletes the ACAO **and leaves `Vary`
/// alone**, where the shared helper removes both. The proxy is handed the UPSTREAM's headers, so a `Vary` the
/// device sent is the device's to keep.
fn stamp_cors_like_ts(
    headers: &mut Vec<(String, String)>,
    origin: &str,
    request_host: Option<&str>,
    configured: Option<&str>,
) {
    if crate::cors::is_allowed_origin(origin, request_host, configured) {
        set_header(headers, "Access-Control-Allow-Origin", origin.to_string());
        set_header(headers, "Vary", "Origin".to_string());
    } else {
        headers.retain(|(name, _)| !name.eq_ignore_ascii_case("access-control-allow-origin"));
    }
}

/// `Headers.set` on a pair list: REPLACE the first occurrence and drop the rest, which is what a `Headers`
/// object does when the source writes `outHeaders.set(...)`.
fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    let mut seen = false;
    headers.retain(|(key, _)| {
        if key.eq_ignore_ascii_case(name) {
            if seen {
                return false;
            }
            seen = true;
        }
        true
    });
    if seen {
        if let Some(slot) = headers
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
        {
            slot.1 = value;
            return;
        }
    }
    headers.push((name.to_string(), value));
}

/// `if (outHeaders.has("content-length")) outHeaders.set("content-length", String(byteLength))` — the source
/// updates the declared length ONLY when the upstream declared one, because the runtime reframes a response
/// that has none.
fn set_content_length_if_present(headers: &mut Vec<(String, String)>, bytes: usize) {
    if headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-length"))
    {
        set_header(headers, "content-length", bytes.to_string());
    }
}

/// `new Headers(pairs)` — every pair set in order, so a later duplicate wins, exactly as the source's
/// `outHeaders.set(...)` calls do.
///
/// A value the runtime refuses is DROPPED rather than fatal: every pair here came off a `Response` the runtime
/// itself built, so this cannot fire, and a header that cannot be set is not a reason to fail a panel request.
fn headers_from(pairs: &[(String, String)]) -> Headers {
    let headers = Headers::new();
    for (name, value) in pairs {
        let _ = headers.set(name, value);
    }
    headers
}

/* ─────────────────────────── the panel rewrite ─────────────────────────── */

/// Absolute paths a summrise-agent panel serves from its own root. When proxied through the console they must
/// carry the proxy mount so absolute references keep resolving through the proxy.
///
/// SOLID Round-54 pruned this to the paths that actually occur: the pre-panel-react SPA's entries served zero
/// matches in any currently served asset. Rule for future entries, from the source: must match served content
/// or a live endpoint family — speculative entries cost a regex compile + full scan per proxied text response.
pub const PANEL_ROOT_PATHS: [&str; 2] = ["/api/", "/mcp"];

/// `rewriteDeviceBody(text, name)` — the two rewrites, hand-rolled because both are regular expressions with
/// features the Rust regex crate does not have (a negative lookahead in the first, and the `\s` set of
/// ECMAScript in the second), and because a hand-rolled scanner over an ASCII delimiter set is easier to prove
/// than a translation of two flag combinations.
pub fn rewrite_device_body(text: &str, name: &str) -> String {
    let prefix = format!("{DEVICE_BASE}/{name}/proxy");
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let ch = text[i..].chars().next().unwrap_or_default();
        let width = ch.len_utf8();
        if matches!(ch, '"' | '\'' | '`' | '}') {
            let rest = &text[i + width..];
            if let Some(path) = PANEL_ROOT_PATHS.iter().find(|path| rest.starts_with(**path)) {
                // The lookahead: a path that is ALREADY under some device's proxy mount is left alone, which is
                // what stops a second pass from doubling the prefix.
                if !is_already_proxied(rest) {
                    out.push(ch);
                    out.push_str(&prefix);
                    out.push_str(path);
                    i += width + path.len();
                    continue;
                }
            }
        }
        out.push_str(&text[i..i + width]);
        i += width;
    }
    strip_panel_token(&out)
}

/// The first pattern's `(?!${DEVICE_BASE}/[^/"']+/proxy/)`: `/api/devices/` + one or more characters that are
/// not `/`, `"` or `'` + `/proxy/`.
fn is_already_proxied(rest: &str) -> bool {
    let Some(after) = rest.strip_prefix(DEVICE_BASE) else {
        return false;
    };
    let Some(after) = after.strip_prefix('/') else {
        return false;
    };
    let mut taken = 0;
    for ch in after.chars() {
        if matches!(ch, '/' | '"' | '\'') {
            break;
        }
        taken += ch.len_utf8();
    }
    if taken == 0 {
        return false;
    }
    after[taken..].starts_with("/proxy/")
}

/// `/window\.__PANEL_TOKEN__\s*=\s*(?:"[^"]*"|'[^']*')/g` → `window.__PANEL_TOKEN__=""`.
///
/// Strip the agent's injected device token from proxied HTML. Direct same-origin HTML never passes through
/// this function, and the admin session-cookie flow authenticates BEFORE any token parsing — so stripping
/// costs nothing functionally, and it prevents an extension user from reading the PERMANENT device token off a
/// console-origin page (DOM/devtools/XSS) and keeping direct control of `/api/tools/*` and `/mcp` after the
/// plugin-link TTL or unpair.
fn strip_panel_token(text: &str) -> String {
    const NEEDLE: &str = "window.__PANEL_TOKEN__";
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let Some(offset) = text[i..].find(NEEDLE) else {
            out.push_str(&text[i..]);
            break;
        };
        let start = i + offset;
        out.push_str(&text[i..start]);
        let after_name = skip_js_whitespace(text, start + NEEDLE.len());
        if text[after_name..].starts_with('=') {
            let quote_at = skip_js_whitespace(text, after_name + 1);
            let quote = text[quote_at..].chars().next();
            if let Some(quote @ ('"' | '\'')) = quote {
                if let Some(close) = text[quote_at + 1..].find(quote) {
                    out.push_str("window.__PANEL_TOKEN__=\"\"");
                    i = quote_at + 1 + close + 1;
                    continue;
                }
            }
        }
        // No match at this position: the regex engine would advance one character, and the needle has no 'w'
        // after its first, so no later match can begin inside it.
        out.push_str(NEEDLE);
        i = start + NEEDLE.len();
    }
    out
}

/// ECMAScript's `\s`, which is not Rust's `char::is_whitespace`: it includes U+FEFF and the Unicode space
/// separators, and `\v` is a single character there.
fn is_js_space(ch: char) -> bool {
    matches!(
        ch,
        '\u{0009}'
            | '\u{000a}'
            | '\u{000b}'
            | '\u{000c}'
            | '\u{000d}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

fn skip_js_whitespace(text: &str, mut at: usize) -> usize {
    while at < text.len() {
        let ch = text[at..].chars().next().unwrap_or_default();
        if !is_js_space(ch) {
            break;
        }
        at += ch.len_utf8();
    }
    at
}

/* ─────────────────────────── the query string ─────────────────────────── */

/// `new URLSearchParams(url.search)` → `.delete(name)` → `.toString()`.
///
/// **THE RE-SERIALISATION IS NOT THE ORIGINAL STRING, AND THAT IS THE POINT.** The `application/x-www-form-
/// urlencoded` serializer is not `encodeURIComponent`: it keeps only ASCII alphanumerics and `*-._`, it writes
/// a space as `+`, and it percent-encodes `~ ! ' ( )`. So `?name=固件.bin&token=x` forwards as
/// `name=%E5%9B%BA%E4%BB%B6.bin`, which is what the device — and the corpus — sees.
pub fn search_params_delete(search: &str, name: &str) -> String {
    let trimmed = search.strip_prefix('?').unwrap_or(search);
    let mut pairs: Vec<(String, String)> = Vec::new();
    for piece in trimmed.split('&') {
        // An EMPTY sequence produces no pair, but `=` alone produces one with an empty name and value.
        if piece.is_empty() {
            continue;
        }
        let (key, value) = match piece.split_once('=') {
            Some((key, value)) => (key, value),
            None => (piece, ""),
        };
        pairs.push((form_decode(key), form_decode(value)));
    }
    pairs.retain(|(key, _)| key != name);
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", form_encode(key), form_encode(value)))
        .collect::<Vec<String>>()
        .join("&")
}

/// The urlencoded parser's decoder: `+` is a space, a `%` that is not followed by two hex digits is literal,
/// and invalid UTF-8 becomes U+FFFD.
fn form_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or_default();
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The urlencoded serializer's encoder.
fn form_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'*' | b'-' | b'.' | b'_' => out.push(*byte as char),
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' => out.push(*byte as char),
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every expectation in this module was produced by running the SHIPPING JavaScript, and the command is in
    /// the test's name so the next reader can re-derive it rather than trust it:
    /// `node -e 'console.log(new URLSearchParams("…").toString())'`.
    #[test]
    fn the_query_rewrite_is_the_platforms_own() {
        // `new URLSearchParams("?a=1&token=x&b=2").delete("token").toString()` → `a=1&b=2`
        assert_eq!(search_params_delete("?a=1&token=x&b=2", "token"), "a=1&b=2");
        // a name with no value survives as `key=`
        assert_eq!(search_params_delete("?a", "token"), "a=");
        // `=` alone is a pair with an empty name
        assert_eq!(search_params_delete("?=", "token"), "=");
        // an empty sequence is skipped, not kept as an empty pair
        assert_eq!(search_params_delete("?a=1&", "token"), "a=1");
        // `+` is a space; the serializer writes it back as `+`
        assert_eq!(search_params_delete("?a=b+c", "token"), "a=b+c");
        // `~ ! ' ( )` are percent-encoded by THIS serializer and not by `encodeURIComponent`
        assert_eq!(
            search_params_delete("?a=~!%27()", "token"),
            "a=%7E%21%27%28%29"
        );
        // a non-ASCII filename survives as its own UTF-8 percent-encoding
        assert_eq!(
            search_params_delete("?name=%E5%9B%BA%E4%BB%B6.bin&token=x", "token"),
            "name=%E5%9B%BA%E4%BB%B6.bin"
        );
        // a malformed escape is literal going IN and re-encoded coming OUT: the parser keeps `%zz` as three
        // characters, and the serializer writes the `%` back as `%25` (`node -e 'const p = new
        // URLSearchParams("?a=%zz"); p.delete("token"); console.log(p.toString())'` → `a=%25zz`)
        assert_eq!(search_params_delete("?a=%zz&token=x", "token"), "a=%25zz");
        // EVERY occurrence of the name goes, which `delete` does and a `find`/`skip` would not
        assert_eq!(
            search_params_delete("?token=a&b=1&token=c", "token"),
            "b=1"
        );
    }

    /// `rewriteDeviceBody` — the four shapes the source's own comments name, plus the lookahead.
    #[test]
    fn the_panel_rewrite_is_the_sources_own() {
        assert_eq!(
            rewrite_device_body(r#"fetch("/api/status")"#, "d1"),
            r#"fetch("/api/devices/d1/proxy/api/status")"#
        );
        // the `}` case: a template literal's interpolation close, which the source added because the panel
        // builds `https://${hostname}/api/events/term` and the SSE stream 404'd without it
        assert_eq!(
            rewrite_device_body("`https://${h}/api/events/term`", "d1"),
            "`https://${h}/api/devices/d1/proxy/api/events/term`"
        );
        assert_eq!(rewrite_device_body("'/mcp'", "d1"), "'/api/devices/d1/proxy/mcp'");
        // ALREADY PROXIED — the negative lookahead, which is what stops a double prefix
        assert_eq!(
            rewrite_device_body(r#""/api/devices/d1/proxy/api/status""#, "d1"),
            r#""/api/devices/d1/proxy/api/status""#
        );
        // …and the suppression is the LOOKAHEAD's, not this device's: `[^/"']+` accepts any name, so another
        // device's mount leaves the string alone too (measured against the shipping regex, 2026-10-09)
        assert_eq!(
            rewrite_device_body(r#""/api/devices/d2/proxy/api/status""#, "d1"),
            r#""/api/devices/d2/proxy/api/status""#
        );
        // a `}` or a quote NOT followed by a root path is untouched
        assert_eq!(rewrite_device_body("}x", "d1"), "}x");
        // the injected token is stripped, both quote styles, `\s` included
        assert_eq!(
            rewrite_device_body(r#"window.__PANEL_TOKEN__ = "abc.def""#, "d1"),
            "window.__PANEL_TOKEN__=\"\""
        );
        assert_eq!(
            rewrite_device_body("window.__PANEL_TOKEN__='xyz'", "d1"),
            "window.__PANEL_TOKEN__=\"\""
        );
        // a token line with no closing quote is left alone — the regex needs the pair
        assert_eq!(
            rewrite_device_body(r#"window.__PANEL_TOKEN__ = "abc"#, "d1"),
            r#"window.__PANEL_TOKEN__ = "abc"#
        );
        // a multi-byte character before a path is copied whole
        assert_eq!(
            rewrite_device_body("—\"/api/x\"", "d1"),
            "—\"/api/devices/d1/proxy/api/x\""
        );
    }
}
