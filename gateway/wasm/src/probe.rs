//! `/api/summrise-probe` — the public channel probe, and the limiter that guards it.
//!
//! WHY THIS ROUTE MOVED. It is the only PUBLIC endpoint that spends real money: each probe is an upstream
//! `max_tokens: 1` call billed to a WORKER key. The limiter that guards it was already Rust
//! (`ip_rate_limit.rs`), and leaving the guard in one language and the guarded route in another means the
//! route can change without the guard knowing. **A guard and what it guards belong in the same place.**
//!
//! WHAT IS NOT PORTED AND WHY: the og branch's NATIVE arm. `OG_NATIVE_ANTHROPIC` is `new Set()` in the
//! source — **an EMPTY set, measured** — so `OG_NATIVE_ANTHROPIC.has(upstreamModel)` is `false` for every
//! input and the `x-api-key` + `OG_ZEN_ANTHROPIC` arm is unreachable. The arm is left out rather than
//! ported: **a port that carries unreachable code cannot show it is unreachable**, and the branch is where
//! the divergence would hide. The set stays the thing to watch; if it ever gains a member this file is
//! where the arm is added.

use worker::{wasm_bindgen::JsValue, Env, Headers, Method, Request, RequestInit};

use crate::device::fetch_with_timeout;
use crate::registry;
use crate::reliability;
use crate::responses::{self, Built};
use crate::routing;
use crate::session;
use crate::tooling;
use crate::RouteFailure;

/// `headers_from(pairs)` — `device_proxy.rs`'s own helper shape: a slice of pairs becomes `Headers`.
fn headers_from(pairs: &[(String, String)]) -> Headers {
    let headers = Headers::new();
    for (name, value) in pairs {
        let _ = headers.set(name, value);
    }
    headers
}

/// The source's `OG_ZEN_CHAT` — the endpoint the probe's og branch always uses (see the module header
/// for why the other one is not here).
const OG_ZEN_CHAT: &str = "https://opencode.ai/zen/go/v1/chat/completions";

/// The per-isolate counter table — the source's module-level `counters` Map, one per limiter. The
/// devices gate has its own (`device_store::PUBLIC_RATE_COUNTERS`); sharing would put the probe's keys in
/// its table and make the 4096-key eviction cover both, which the source does not do.
static COUNTERS: std::sync::Mutex<crate::ip_rate_limit::Counters> =
    std::sync::Mutex::new(Vec::new());

/// `cf-connecting-ip`, or the literal `"unknown"` — `request?.headers?.get?.(…) || "unknown"`.
fn caller_ip(request: &Request) -> String {
    request
        .headers()
        .get("cf-connecting-ip")
        .ok()
        .flatten()
        .filter(|ip| !ip.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// The 429 — `jsonError(429, "probe rate limit exceeded", "rate_limit_error")`.
fn rate_limited() -> Built {
    responses::json_error(429, "probe rate limit exceeded", "rate_limit_error")
}

/// `index.ts`'s `POST /api/summrise-probe`. POST only: a GET probe would cost an upstream call with no
/// body to name a channel.
pub fn in_family(method: &Method, path: &str) -> bool {
    *method == Method::Post && path == "/api/summrise-probe"
}

/// The dispatcher. `Ok(None)` is "not this family".
pub async fn handle(request: Request, env: &Env) -> Result<Option<worker::Response>, RouteFailure> {
    let url = request.url().map_err(|_| RouteFailure::Threw)?;
    if !in_family(&request.method(), url.path()) {
        return Ok(None);
    }
    Ok(Some(probe(request, env).await?))
}

/// `summriseProbe(env, model)`, in the source's order.
async fn probe(mut request: Request, env: &Env) -> Result<worker::Response, RouteFailure> {
    // **THE LIMITER FIRST, AND IT DOES NOT READ THE BODY — THAT IS THE ORDER IN THE SOURCE.** A caller
    // over budget is refused without their body being parsed, so a rate-limited probe costs one KV read
    // and no upstream call.
    // **THE LOCK IS NEVER HELD ACROSS AN AWAIT — CLIPPY REFUSED IT AND WAS RIGHT.** `v1.rs`'s own limiter does
    // it this way: lock inside a block, drop it at the block's end. So each of the seed's three steps takes
    // the lock separately and releases it before the KV call.
    let ip = caller_ip(&request);
    let now = crate::device_store::now_ms();
    let key = crate::ip_rate_limit::counter_key(crate::ip_rate_limit::PROBE_RATE, &ip, now);
    let seed = {
        let counters = COUNTERS.lock().unwrap_or_else(|e| e.into_inner());
        crate::ip_rate_limit::needs_seed(&counters, &key)
    };
    let cur = if seed {
        crate::device_store::kv_text(env, &key)
            .await
            .and_then(|raw| raw.parse::<u32>().ok())
            .unwrap_or(0)
    } else {
        0
    };
    let refused = {
        let mut counters = COUNTERS.lock().unwrap_or_else(|e| e.into_inner());
        if seed {
            counters.push((key.clone(), cur));
        }
        crate::ip_rate_limit::check(crate::ip_rate_limit::PROBE_RATE, &mut counters, &ip, now)
    };
    if seed {
        crate::device_store::kv_put(
            env,
            &key,
            &(cur + 1).to_string(),
            Some(crate::ip_rate_limit::seed_ttl_secs(
                crate::ip_rate_limit::PROBE_RATE.window_ms,
            )),
        )
        .await;
    }
    if refused {
        return reply(rate_limited());
    }
    let model = body_model(&mut request).await;
    let prefix = model.split('/').next().unwrap_or("").to_string();

    if let Some(hint) = routing::retired_model_hint(&model) {
        // `jsonError(400, …, "invalid_request")` — the PROBE's kind. `retired_model_error` sends
        // `"invalid_request_error"`, which is the /v1 front door's convention (whose TypeScript is
        // deleted); **the probe's TypeScript still exists, so the probe keeps its own spelling.**
        return reply(responses::json_error(
            400,
            &format!(
                "Model {model} was retired on 2026-09-10 — the DeepSeek V4 Flash line is superseded by \
                 V4.1 Flash. Use {hint} instead."
            ),
            "invalid_request",
        ));
    }
    let known_channel = crate::HEALTH_CHANNELS.iter().any(|(id, _)| *id == prefix);
    let known_model = registry::model_spec(&model).is_some();
    if !known_channel || !known_model {
        return reply(responses::json_error(
            400,
            &format!("Unknown channel model: {model}"),
            "invalid_request",
        ));
    }

    if prefix == "og" {
        if crate::read_breaker(env).await.unwrap_or(false) {
            return reply(probe_threw(&prefix, "circuit open"));
        }
        let key = env
            .var("OPENCODE_GO_API_KEY")
            .map(|v| v.to_string())
            .unwrap_or_default();
        if key.is_empty() {
            return reply(probe_threw(&prefix, "OPENCODE_GO_API_KEY not configured"));
        }
        let upstream_model =
            registry::wire_model_name(&prefix, &routing::strip_bracket(&model[prefix.len() + 1..]));
        // The source passes `undefined` for the incoming headers. The Rust takes a map, and the probe has
        // none, so it gets an EMPTY one — `client_session_id` reads the `x-opencode-session` header off it
        // and falls back to the anonymous digest, which is what the source's `undefined` produces too.
        let session = session::opencode_session_header(&serde_json::Map::new(), "probe");
        let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        headers.push(("Authorization".to_string(), format!("Bearer {key}")));
        headers.extend(session);
        let result = upstream(OG_ZEN_CHAT, &upstream_model, &headers, env).await;
        return reply(probe_result(&prefix, result));
    }

    let route = routing::pick_route(&prefix, Some(&env_value(env)), None, "");
    let key = env
        .var(tooling::probe_env_key_name(&prefix))
        .map(|v| v.to_string())
        .unwrap_or_default();
    if key.is_empty() {
        return reply(probe_threw(
            &prefix,
            &format!("{prefix}: key not configured"),
        ));
    }
    let stripped = if route.strip_prefix {
        &model[prefix.len() + 1..]
    } else {
        &model[..]
    };
    let upstream_model = registry::wire_model_name(&prefix, &routing::strip_bracket(stripped));
    let headers = session::passthrough_headers(Some(&key), None, &[]);
    let result = upstream(&route.upstream, &upstream_model, &headers, env).await;
    reply(probe_result(&prefix, result))
}

/// **THE CATCH ARM — THREE FIELDS AND NO STATUS.** The source's
/// `jsonOk({ ok: false, channel, detail: e.message })`. It is NOT the four-field shape with
/// `status: 0`: a caller parsing `status` from a failed probe would read `undefined`, and the source's
/// own bytes are three keys. **The shapes are different on purpose and a port that unifies them
/// changes the response.**
fn probe_threw(channel: &str, message: &str) -> Built {
    responses::json_ok(
        &serde_json::json!({
            "ok": false,
            "channel": channel,
            "detail": message,
        }),
        &[],
    )
}

/// `probeResultJson(prefix, res)` — **four** fields, all derived from the response, and the derivation
/// is the whole of it: `ok` is the response's `ok`, `detail` is empty when it is and `upstream <status>`
/// when it is not. **A port that hardcodes `ok` or `detail` answers a probe that never ran.**
fn probe_answered(channel: &str, status: u16) -> Built {
    let ok = (200..300).contains(&status);
    responses::json_ok(
        &serde_json::json!({
            "ok": ok,
            "channel": channel,
            "status": status,
            "detail": if ok { String::new() } else { format!("upstream {status}") },
        }),
        &[],
    )
}

/// The two arms side by side, which is the only place the answer's shape is decided.
fn probe_result(prefix: &str, result: std::result::Result<u16, String>) -> Built {
    match result {
        Err(message) => probe_threw(prefix, &message),
        Ok(status) => probe_answered(prefix, status),
    }
}

/// The POST itself — the source's `fetchWithTimeout(upstream, {…}, upstreamTimeoutMs(env))`, with the
/// catch arm answering the error's own message.
async fn upstream(
    url: &str,
    upstream_model: &str,
    headers: &[(String, String)],
    env: &Env,
) -> std::result::Result<u16, String> {
    let body = serde_json::json!({
        "model": upstream_model,
        "messages": [{ "role": "user", "content": "ping" }],
        "max_tokens": 1,
        "stream": false,
    })
    .to_string();
    let mut init = RequestInit::new();
    init.with_method(worker::Method::Post);
    init.with_headers(headers_from(headers));
    init.with_body(Some(JsValue::from_str(&body)));
    let timeout = reliability::upstream_timeout_ms(Some(&env_value(env))) as u64;
    fetch_with_timeout(url, &init, timeout)
        .await
        .map(|response| response.status_code())
}

/// A `Built` becomes the `Response` the runtime sends — `lib.rs`'s `json`, which is the general
/// conversion (v1 has its own private copy of the same shape).
fn reply(built: Built) -> Result<worker::Response, RouteFailure> {
    crate::json(built.body, built.status).map_err(|_| RouteFailure::Threw)
}

/// `readJson(request)` — a body that does not parse is `{}`, so `body.model` is absent.
async fn body_model(request: &mut Request) -> String {
    let text = request.text().await.unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(str::to_string))
        .unwrap_or_default()
}

/// **THE ENV AS A `serde_json::Value`, BUILT FROM THE VARS — MY FIRST VERSION RETURNED `None`, AND THAT
/// WAS A DIVERGENCE: `pick_route` and `upstream_timeout_ms` would have read NO config at all, so every probe
/// would have taken the default route and the default timeout.** Caught by clippy's "unused variable" on the
/// parameter, not by a test — the same instrument that has caught most of today's defects.
///
/// **`US_PROXY` IS READ AS A VAR HERE, NOT AS THE KV SETTING.** The probe's `pickRoute` reads the worker's own
/// egress switch (`upstream.ts`); the KV-setting precedence is the /v1 front door's own proved decision and
/// importing it here would be a behaviour change.
fn env_value(env: &Env) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for name in crate::v1::ENV_KEYS {
        if let Ok(value) = env.var(name) {
            let text = value.to_string();
            if !text.is_empty() {
                map.insert(name.to_string(), serde_json::json!(text));
            }
        }
    }
    if let Ok(value) = env.var("US_PROXY") {
        let text = value.to_string();
        if !text.is_empty() {
            map.insert("US_PROXY".to_string(), serde_json::json!(text));
        }
    }
    serde_json::Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **POST, AND THE EXACT PATH.** A GET probe would spend an upstream call with no body to name a
    /// channel, and a prefix would swallow a neighbour (`/api/summrise-probe-2`).
    ///
    /// MUTATION: drop the method guard, or change `==` to `starts_with`.
    /// RESULT:   this test fails on the method arm or the neighbour, and nothing else notices.
    #[test]
    fn the_family_is_post_and_the_exact_path() {
        assert!(in_family(&Method::Post, "/api/summrise-probe"));
        assert!(!in_family(&Method::Get, "/api/summrise-probe"));
        assert!(!in_family(&Method::Post, "/api/summrise-probe-2"));
        assert!(!in_family(&Method::Post, "/api/summrise-probe/x"));
    }

    /// **THE ANSWERED ARM IS FOUR FIELDS, AND EVERY ONE IS DERIVED FROM THE STATUS.**
    /// `ok` is `200..300`, `detail` is empty exactly when `ok` is, and `status` is the upstream's own.
    ///
    /// MUTATION: hardcode `"ok": true`, or make `detail` always `""`.
    /// RESULT:   this test fails on the 4xx case — a probe whose upstream refused would answer
    ///           `ok: true` with an empty detail, which is the worst answer this route can give.
    #[test]
    fn the_answered_arm_derives_every_field_from_the_status() {
        let ok =
            serde_json::from_str::<serde_json::Value>(&probe_answered("ds", 200).body).unwrap();
        assert_eq!(ok["ok"], serde_json::json!(true));
        assert_eq!(ok["channel"], serde_json::json!("ds"));
        assert_eq!(ok["status"], serde_json::json!(200));
        assert_eq!(ok["detail"], serde_json::json!(""));

        let refused =
            serde_json::from_str::<serde_json::Value>(&probe_answered("ds", 429).body).unwrap();
        assert_eq!(refused["ok"], serde_json::json!(false));
        assert_eq!(refused["status"], serde_json::json!(429));
        assert_eq!(refused["detail"], serde_json::json!("upstream 429"));
    }

    /// **THE THREW ARM IS THREE FIELDS, AND THAT IS NOT AN OMISSION.**
    ///
    /// MUTATION: add a `"status"` field to the threw arm, or unify the two shapes.
    /// RESULT:   this test fails: the source's answer for a transport error has three keys, and a
    ///           caller reading `status` there must see `undefined`, not `0`.
    #[test]
    fn the_threw_arm_is_three_fields_and_no_status() {
        let built = probe_threw("ds", "boom");
        let body = serde_json::from_str::<serde_json::Value>(&built.body).unwrap();
        let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 3, "three keys, in this order: {keys:?}");
        assert!(body.get("status").is_none(), "no status key");
        assert_eq!(body["detail"], serde_json::json!("boom"));
        assert_eq!(body["ok"], serde_json::json!(false));
    }
}
