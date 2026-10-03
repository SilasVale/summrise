//! THE `/v1` FRONT DOOR — the glue between `worker::Fetch`/KV and the ported decisions.
//!
//! WHY THIS FILE EXISTS AND WHY IT IS THIS SHORT. Every decision on this path was ported and proved
//! separately: the route match, auth, the per-arm key gates, the model chain, the two-phase dispatch and the
//! response shaping. What is left is the I/O — four KV reads, one upstream call — and this file is that, so
//! that the parts that CAN be proved in-process stay free of `worker::`.
//!
//! THE KEYS IT READS, each matching `store/`'s own:
//!
//! ```text
//!     token:<token>          -> the uid
//!     user:<uid>             -> the user record (whose `enabled` the auth gate checks)
//!     ukeys:<uid>            -> the user's BYOK keys
//!     settings:<name>        -> a global setting, with the Worker var of the same name as the fallback
//! ```
//!
//! AND ONE THING IT DOES NOT DO YET, NAMED RATHER THAN HIDDEN: the true-streaming path. `v1_finish` answers
//! `ArmOutcome::Stream` for a real SSE upstream, and this file returns a **501** for it — the ported encoder
//! exists (`stream.rs`), but wiring a live `ReadableStream` through it is its own step with its own proof.

use worker::*;

use crate::request_shape::{v1_finish, v1_plan, ArmOutcome, UpstreamAnswer, V1Plan, V1Request};
use crate::responses::Built;

/// A `Built` becomes the `Response` the runtime sends.
pub fn to_response(built: Built) -> Result<Response> {
    let headers = Headers::new();
    for (k, v) in &built.headers {
        headers.set(k, v)?;
    }
    Ok(Response::ok(built.body)?
        .with_status(built.status)
        .with_headers(headers))
}

/// One KV string, or `None`.
async fn kv_text(env: &Env, key: &str) -> Option<String> {
    let kv = env.kv("KEYS").ok()?;
    kv.get(key).text().await.ok().flatten()
}

/// `GET /v1/models`'s advertised set is not wired here — the four `/v1` arms the handler serves are.
pub fn is_v1_route(method: &Method, path: &str) -> bool {
    if method != &Method::Post {
        return false;
    }
    path.ends_with("/v1/messages")
        || path.ends_with("/v1/messages/count_tokens")
        || path.ends_with("/v1/chat/completions")
        || path.ends_with("/v1/responses")
}

/// The front door for `/v1/*`.
pub async fn handle(mut req: Request, env: Env) -> Result<Response> {
    let url = req.url()?;
    let method = req.method();
    let path = url.path().to_string();
    if !is_v1_route(&method, &path) {
        return Response::error("Not Found", 404);
    }
    let raw = req.text().await?;
    let parsed: Option<serde_json::Value> = serde_json::from_str(&raw).ok();
    let model = parsed
        .as_ref()
        .and_then(|b| b.get("model"))
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();

    // THE RETIRED GATE, BEFORE ANY ROUTING — it refuses rather than redirects.
    if let Some(hint) = crate::routing::retired_model_hint(&model) {
        return to_response(crate::routing::retired_model_error(&model, hint));
    }

    // AUTH: the token names a uid, the uid names a user, and the user's `enabled` is the gate's business.
    let token = req.headers().get("x-api-key")?.unwrap_or_default();
    let user = match kv_text(&env, &format!("token:{token}")).await {
        Some(uid) => kv_text(&env, &format!("user:{uid}"))
            .await
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()),
        None => None,
    };
    let byok = match user
        .as_ref()
        .and_then(|u| u.get("id"))
        .and_then(|i| i.as_str())
    {
        Some(uid) => kv_text(&env, &format!("ukeys:{uid}"))
            .await
            .and_then(|t| {
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&t).ok()
            })
            .unwrap_or_default(),
        None => serde_json::Map::new(),
    };

    // THE SETTINGS the chain reads: `settings:US_PROXY` with the Worker var as its fallback.
    let us_proxy_setting = crate::store::global_setting_enabled(
        crate::store::get_global_setting(
            kv_text(&env, "settings:US_PROXY").await.as_deref(),
            env.var("US_PROXY").ok().map(|v| v.to_string()).as_deref(),
        )
        .as_deref(),
    );
    let env_json = serde_json::json!({
        "US_PROXY": if us_proxy_setting { "1" } else { "0" },
    });

    // THE CHAIN: the model -> the route, then the two-phase dispatch.
    let resolved = crate::routing::resolve_model(&model, us_proxy_setting, &path, Some(&env_json));
    let request = V1Request {
        method: method.to_string(),
        path: path.clone(),
        user,
        kind: resolved.route.kind.to_string(),
        byok,
        env: env_json,
        raw_text: serde_json::json!(raw),
    };
    let og_session: Vec<(String, String)> = Vec::new();
    let plan = v1_plan(
        &request,
        &resolved.route,
        &resolved.upstream_model,
        parsed.as_ref(),
        &og_session,
        None,
    );
    let V1Plan::Messages(call) = plan else {
        let V1Plan::Respond(built) = plan else {
            unreachable!()
        };
        return to_response(built);
    };

    // PHASE TWO'S I/O: one upstream call.
    let mut init = RequestInit::new();
    init.with_method(Method::Post);
    // `Headers::set` takes `&self` in workers-rs, so this is not `mut`.
    let headers = Headers::new();
    for (k, v) in &call.request.headers {
        headers.set(k, v)?;
    }
    init.with_headers(headers);
    init.with_body(Some(call.request.body.clone().into()));
    let upstream = Fetch::Request(Request::new_with_init(&call.request.url, &init)?)
        .send()
        .await;
    let answer = match upstream {
        Ok(mut res) => {
            let status = res.status_code();
            let content_type = res.headers().get("content-type")?.unwrap_or_default();
            let retry_after = res.headers().get("retry-after")?;
            let text = res.text().await.unwrap_or_default();
            let json = serde_json::from_str(&text).ok();
            Some(UpstreamAnswer {
                status,
                content_type,
                json,
                retry_after,
                text,
            })
        }
        Err(_) => None,
    };
    match v1_finish(&call, answer.as_ref()) {
        ArmOutcome::Response(built) => to_response(built),
        // Named, not faked: the live SSE path is its own step (see this file's header).
        ArmOutcome::Stream => to_response(crate::responses::json_error(
            501,
            "streaming is not wired yet",
            "api_error",
        )),
    }
}
