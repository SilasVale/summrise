//! `POST|PUT /api/upload` — THE FILE RELAY'S UPLOAD LEG, AND THE 100 MiB BODY IT CARRIES.
//!
//! **WHY THIS IS NOT IN `devices.rs` WITH ITS SIBLINGS.** It is not under `/api/devices` at all: it is the
//! console's half of the file relay (ADR-0010 D6), and the landing that ported the device registry excluded it
//! in so many words — "a 100 MiB body passthrough to the relay, which `workers-rs` 0.8.7 cannot forward as a
//! stream" (`index.ts`'s cutover comment). This module is that sentence answered: the body IS forwarded as a
//! stream, through the global `Request` constructor's `duplex: "half"` — see `device::js_request`.
//!
//! **THE CREDENTIAL IS ONE OF FOUR, AND THE SOURCE'S HEADER SAYS WHY.** An admin session cookie; the device's
//! own Bearer token (possession of the token IS the device identity, the same rule as self-register); a paired
//! plugin-link token; or the admin API token `/mcp` already requires (`findUserByToken`, role admin) — which is
//! what makes the relay symmetric, so an AI client on another machine can stage a file for a device to pull.
//! Relay-role tokens (ADR-0007) are excluded by the same role check that admits the admin's.
//!
//! **AND THE SIZE BOUND IS THE GATEWAY'S, WHILE THE AUTHORITATIVE ONE IS THE RELAY'S.** The declared
//! `Content-Length` is refused above 100 MiB + a 64 KiB framing margin; a chunked body has no declared length
//! and is bounded by the platform's own request ceiling, exactly as the source says.

use serde_json::Value;
use worker::*;

use crate::device::{js_request, service_fetch, fetch_request_with_timeout};
use crate::device_store as store;
use crate::session_gate;
use crate::user_store;
use crate::RouteFailure;

/// `UPLOAD_MAX_BYTES` — 100 MiB, the number both other layers on this path use (the relay caps the file at
/// 100 MiB and the agent's own upload does too). The old 25 MB here silently undercut both.
const UPLOAD_MAX_BYTES: f64 = 100.0 * 1024.0 * 1024.0;

/// `UPLOAD_CL_MARGIN` — multipart framing adds a little on top of the file bytes, and the margin matches the
/// relay's, which does the authoritative size check.
const UPLOAD_CL_MARGIN: f64 = 64.0 * 1024.0;

/// The upstream response headers that must never be re-served at the console origin: a `Set-Cookie` from the
/// relay would plant a foreign cookie there, and the rest are hop-by-hop framing headers the runtime owns.
const UPLOAD_STRIP_RESPONSE_HEADERS: [&str; 6] = [
    "set-cookie",
    "set-cookie2",
    "connection",
    "transfer-encoding",
    "keep-alive",
    "upgrade",
];

/// `(m === "POST" || m === "PUT") && p === "/api/upload"` — the route's match function, and therefore this
/// module's family predicate.
pub fn in_family(method: &Method, path: &str) -> bool {
    (*method == Method::Post || *method == Method::Put) && path == "/api/upload"
}

/// `indexWorkerBase(env)`'s fallback — the deployment's release host.
const INDEX_WORKER_BASE_DEFAULT: &str = "https://agent.saisi.online";

/// The dispatcher. `Ok(None)` is the plugin dispatch's `null`.
pub async fn handle(request: Request, env: &Env) -> Result<Option<Response>, RouteFailure> {
    let url = request.url()?;
    if !in_family(&request.method(), url.path()) {
        return Ok(None);
    }
    Ok(Some(handle_file_upload(request, env, &url).await?))
}

/// `handleFileUpload(request, env, url)`.
async fn handle_file_upload(
    request: Request,
    env: &Env,
    url: &Url,
) -> Result<Response, RouteFailure> {
    let user = session_gate::require_session(&request, env).await?;
    let auth = request
        .headers()
        .get("authorization")
        .ok()
        .flatten()
        .unwrap_or_default();
    let is_admin = user
        .as_ref()
        .and_then(|u| u.get("role"))
        .and_then(|role| role.as_str())
        == Some("admin");
    if is_admin {
        return proxy_upload_to_worker(request, env, url).await;
    }
    let token = match auth.strip_prefix("Bearer ") {
        Some(rest) => rest.trim().to_string(),
        None => String::new(),
    };
    if token.is_empty() {
        return Ok(crate::json_error(
            401,
            "Not logged in or missing device token",
            "authentication_error",
        )?);
    }
    // The admin API token (the /mcp credential): an AI client on another machine staging a file for a device to
    // pull. `findUserByToken`'s role downgrade is the same call /mcp makes.
    let api_user = user_store::find_user_by_token(env, &token).await;
    let api_admin = api_user
        .as_ref()
        .and_then(|u| u.get("role"))
        .and_then(|role| role.as_str())
        == Some("admin");
    if api_admin {
        return proxy_upload_to_worker(request, env, url).await;
    }
    // Device token: accept a paired plugin-link token OR the device's own config token. Scan the small device
    // registry — `typeof d.token === "string" && d.token.length >= 32 && safeEq(d.token, token)`.
    let mut ok = store::get_plugin_by_token(env, &token).await.is_some();
    if !ok {
        let devices: Vec<Value> = store::list_devices(env).await;
        ok = devices.iter().any(|device| {
            match device.get("token").and_then(|t| t.as_str()) {
                // `.length` is UTF-16 CODE UNITS, not bytes, which is what the source's test counts.
                Some(stored) if stored.encode_utf16().count() >= 32 => {
                    crate::auth::safe_eq(stored, &token)
                }
                _ => false,
            }
        });
    }
    if !ok {
        return Ok(crate::json_error(
            401,
            "Invalid device token",
            "authentication_error",
        )?);
    }
    proxy_upload_to_worker(request, env, url).await
}

/// `proxyUploadToWorker(request, env, url)` — the size bound, the minimal header set, the binding, and the
/// host fallback.
async fn proxy_upload_to_worker(
    request: Request,
    env: &Env,
    url: &Url,
) -> Result<Response, RouteFailure> {
    let index_worker_url = env
        .var("INDEX_WORKER_URL")
        .ok()
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| INDEX_WORKER_BASE_DEFAULT.to_string());
    // Forward the QUERY, not just the path: the raw-stream PUT carries the filename in `?name=`, and dropping
    // it silently renamed every upload "file" on the download side.
    let search = match url.query() {
        Some(query) => format!("?{query}"),
        None => String::new(),
    };
    let upload_url = format!("{index_worker_url}/api/upload{search}");

    // Size bound: an unbounded passthrough turns the gateway into a free large-file relay (subrequest memory +
    // egress). Bodies without a declared length (chunked) are still bounded by the platform's own request-body
    // ceiling, and the relay does the authoritative check.
    let declared = js_number(
        &request
            .headers()
            .get("content-length")
            .ok()
            .flatten()
            .unwrap_or_default(),
    );
    if declared.is_finite() && declared > UPLOAD_MAX_BYTES + UPLOAD_CL_MARGIN {
        return Ok(crate::json_error(
            413,
            &format!("Upload too large (max {}MB)", (UPLOAD_MAX_BYTES / (1024.0 * 1024.0)).floor()),
            "invalid_request",
        )?);
    }

    // Rebuild the request with the UPLOAD_KEY header for the relay. Forward a MINIMAL header set: Authorization
    // is the only credential the relay needs, and Content-Type must survive verbatim (the multipart boundary in
    // it is how the worker's `formData()` splits parts). Everything else — notably the client's Cookie header
    // and the inbound Content-Length — stays on this side: the relay is a separate origin and must never see
    // console cookies.
    let headers = Headers::new();
    let _ = headers.set(
        "Authorization",
        &format!(
            "Bearer {}",
            env.var("UPLOAD_KEY").ok().map(|v| v.to_string()).unwrap_or_default()
        ),
    );
    if let Ok(Some(content_type)) = request.headers().get("content-type") {
        let _ = headers.set("Content-Type", &content_type);
    }
    // Raw-stream upload metadata (PUT): the filename and the file's own type ride in these two headers; both
    // are non-credential and must reach the worker that stores the Content-Disposition.
    for name in ["x-filename", "x-content-type"] {
        if let Ok(Some(value)) = request.headers().get(name) {
            let _ = headers.set(name, &value);
        }
    }

    let method = request.method();
    let body = if method == Method::Get || method == Method::Head {
        None
    } else {
        request.inner().body()
    };
    // THE RELAY, BY SERVICE BINDING (ADR 0010 D6) — and the URL path below is kept as the fallback, unchanged,
    // so this code is correct both before and after the relay exists.
    if let Ok(relay) = env.service("RELAY") {
        let relay_url = format!("https://summrise-relay.internal/api/upload{search}");
        // A service binding cannot redirect, so the source passes no `redirect` here — the `manual` defence
        // below is for the HOST fallback, whose target carries the gateway→relay shared secret.
        let request = match js_request(
            &relay_url,
            &method,
            &headers,
            body.as_ref().map(|stream| stream.as_ref()),
            None,
        ) {
            Ok(request) => request,
            Err(message) => return Ok(crate::json_error(502, &message, "proxy_error")?),
        };
        let response = match service_fetch(&relay, &request).await {
            Ok(response) => response,
            Err(message) => return Ok(crate::json_error(502, &message, "proxy_error")?),
        };
        // **THE RELAY ARM DOES NOT STRIP ANYTHING, AND THAT IS THE SOURCE'S BEHAVIOUR REPRODUCED.** The host
        // fallback below strips six hop-by-hop/cookie headers with a comment that says why ("a Set-Cookie from
        // the index worker would plant a foreign cookie on the console origin"), but the binding arm returns
        // `await relay.fetch(...)` DIRECTLY — so a `Set-Cookie` or a `Connection` from the relay DOES reach the
        // client on that path. Measured, not read: the corpus's two relay cases differ on exactly those two
        // headers when this side strips and the TypeScript does not. It is a defect in the source (the comment
        // states an invariant the other arm does not keep) and it is REPORTED rather than fixed here — the
        // criterion for this whole migration is that the same request produces the same bytes from both
        // implementations, and a "fix" on this side would be an unmeasured divergence on a live route.
        return Ok(response);
    }

    // DEFENCE IN DEPTH, and NOT the same severity as the device dial: `uploadUrl`'s host comes from
    // `env.INDEX_WORKER_URL` (operator-set), not from a caller. But the request carries
    // `Authorization: Bearer ${UPLOAD_KEY}`, and with the default `redirect: "follow"` Cloudflare forwards that
    // header to a cross-host Location. `manual` turns a redirected upload into a VISIBLE 3xx instead of an
    // invisible key leak.
    let request = match js_request(
        &upload_url,
        &method,
        &headers,
        body.as_ref().map(|stream| stream.as_ref()),
        Some("manual"),
    ) {
        Ok(request) => request,
        Err(message) => return Ok(crate::json_error(502, &message, "proxy_error")?),
    };
    // A 100 MB stream over a slow uplink is minutes, not 60 s: the old fixed timeout aborted exactly the large
    // transfers this route is about (the worker then answers 500 with no manifest).
    let response = match fetch_request_with_timeout(request, 600_000).await {
        Ok(response) => response,
        Err(message) => return Ok(crate::json_error(502, &message, "proxy_error")?),
    };
    Ok(strip_hop_by_hop(response)?)
}

/// `new Response(resp.body, {status, headers})` with every hop-by-hop and cookie header removed — the response
/// is streamed through, never re-served verbatim.
fn strip_hop_by_hop(mut upstream: Response) -> Result<Response> {
    let status = upstream.status_code();
    let mut kept: Vec<(String, String)> = Vec::new();
    for (name, value) in upstream.headers().entries() {
        if !UPLOAD_STRIP_RESPONSE_HEADERS.contains(&name.to_lowercase().as_str()) {
            kept.push((name, value));
        }
    }
    let headers = Headers::new();
    for (name, value) in &kept {
        // `outHeaders.append(...)`, not `set`: a duplicated upstream header stays duplicated.
        let _ = headers.append(name, value);
    }
    let has_body = !matches!(upstream.body(), ResponseBody::Empty);
    let base = if has_body {
        Response::from_stream(upstream.stream()?)?
    } else {
        Response::empty()?
    };
    Ok(base.with_status(status).with_headers(headers))
}

/// `Number(value)` — and it is written out because `f64::from_str` is not it. The differences that matter for a
/// declared length: `Number("")` is `0` (not a parse failure), and the radix prefixes and the `Infinity`
/// spelling are numbers to JavaScript. `NaN` (which is not finite, and so passes the bound) is the answer for
/// everything else, which is what `!Number.isFinite(declared)` in the source tests.
fn js_number(text: &str) -> f64 {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0.0;
    }
    let lower = trimmed.to_ascii_lowercase();
    let (sign, digits) = match lower.strip_prefix('-') {
        Some(rest) => (-1.0, rest.to_string()),
        None => (1.0, lower.trim_start_matches('+').to_string()),
    };
    if digits == "infinity" {
        return sign * f64::INFINITY;
    }
    for (prefix, radix) in [("0x", 16u32), ("0o", 8), ("0b", 2)] {
        if let Some(rest) = digits.strip_prefix(prefix) {
            return match i64::from_str_radix(rest, radix) {
                Ok(value) => sign * value as f64,
                Err(_) => f64::NAN,
            };
        }
    }
    trimmed.parse::<f64>().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_shape_is_the_sources() {
        assert!(in_family(&Method::Post, "/api/upload"));
        assert!(in_family(&Method::Put, "/api/upload"));
        assert!(!in_family(&Method::Get, "/api/upload"));
        assert!(!in_family(&Method::Post, "/api/upload/"));
        assert!(!in_family(&Method::Post, "/api/devices"));
    }

    /// `Number(x)` for the values a `Content-Length` header can carry, plus the two that decide the branch.
    #[test]
    fn the_declared_length_is_read_the_way_javascript_reads_it() {
        assert_eq!(js_number(""), 0.0);
        assert_eq!(js_number("0"), 0.0);
        // the bound is `> MAX + MARGIN`, so the byte AT it is allowed and the next one is refused
        let bound = UPLOAD_MAX_BYTES + UPLOAD_CL_MARGIN;
        assert!(
            js_number("104923136") <= bound,
            "104923136 is the bound itself, and the comparison is strict"
        );
        assert!(js_number("104923137") > bound, "one byte over the bound is refused");
        assert!(js_number("104857601").is_finite());
        assert!(js_number("abc").is_nan(), "a header that is not a number is not a bound");
        assert_eq!(js_number(" 12 "), 12.0);
        assert_eq!(js_number("0x10"), 16.0);
        assert_eq!(js_number("1e3"), 1000.0);
        assert!(js_number("Infinity").is_infinite());
    }
}
