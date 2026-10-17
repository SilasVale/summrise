//! THE CALLER'S ROUTE — `model=auto`'s stored choice, and the two reads it is resolved from.
//!
//! **WHAT MOVED HERE (2026-10-17).** `GET|PUT /api/me/route` was the LAST route the console's identity
//! family kept on the TypeScript path, and it stayed behind for one reason: the choice lives in a Durable
//! Object (`RouteDO`, `route:{uid}` → the model), and this worker had no `ROUTE` binding. It has one now
//! (`wrangler.jsonc`, the same `script_name: vale-gate` shape `BREAKER` uses), so the route, its two wrappers
//! and the resolution behind `effective` are all here.
//!
//! **THE WRAPPERS ARE THE PART WORTH READING TWICE**, because their postures are what a caller depends on:
//!
//!   * `getUserRoute` NEVER THROWS. A DO that answers 401 plain text (a mismatched `DO_AUTH`) makes a bare
//!     `res.json()` throw a `SyntaxError`; a missing binding throws outright. Both fall back to the LEGACY KV
//!     key, and the migration into the DO happens **only while the DO is reachable** — copying back and
//!     deleting the KV key against an unreachable DO would drop the only surviving copy.
//!   * `setUserRoute` never throws EITHER, for the same reason: a console caller that only handles success
//!     must not see a bare fetch error. It falls back to the KV key, and that is also how the next reachable
//!     read finds something to migrate. **ONE CASE IS A THROW**: no KV binding at all AND an unreachable DO —
//!     `config_error: route store unavailable`, which is the source's own message.
//!
//! **AND THE RESOLUTION IS THE SOURCE'S**: `pickAutoRoute(choice, usable)` decides the policy (the stored
//! choice when usable, else the first usable rung of the ladder, else the default) and this module passes it
//! the two READS — the catalogue verdict first (a disabled model must stop being usable for a route already
//! pointing at it, not only for new choices) and then the channel's key matrix, with the breaker read for an
//! `og/` model only.

use serde_json::Value;
use worker::{Env, Headers, Method, RequestInit};

use crate::auto_route;

/// The DO's own name for the route store: `env.ROUTE.idFromName("global")` — ONE instance for every user,
/// keyed by uid inside the object.
const ROUTE_DO_NAME: &str = "global";

/// `route:{id}` — the legacy KV key the migration reads and retires.
fn legacy_key(id: &str) -> String {
    format!("route:{id}")
}

/// The `RouteDO` stub, or `None` when the binding is absent — the source's `env.ROUTE.get(...)` throwing.
async fn route_call(
    env: &Env,
    path: &str,
    method: Method,
    body: Option<String>,
) -> Option<(u16, String)> {
    let namespace = env.durable_object("ROUTE").ok()?;
    let stub = namespace
        .id_from_name(ROUTE_DO_NAME)
        .ok()?
        .get_stub()
        .ok()?;
    let headers = Headers::new();
    if let Ok(auth) = env.var("DO_AUTH") {
        let _ = headers.set(crate::route_do::DO_AUTH_HEADER, auth.to_string().as_str());
    }
    if body.is_some() {
        let _ = headers.set("content-type", "application/json");
    }
    let mut init = RequestInit::new();
    init.with_method(method);
    init.with_headers(headers);
    if let Some(body) = body {
        init.with_body(Some(worker::wasm_bindgen::JsValue::from_str(&body)));
    }
    let request = worker::Request::new_with_init(&format!("https://route{path}"), &init).ok()?;
    let mut response = stub.fetch_with_request(request).await.ok()?;
    let status = response.status_code();
    let text = response.text().await.unwrap_or_default();
    Some((status, text))
}

/// `getUserRoute(env, id)` — the stored choice, or `None`.
pub async fn get_user_route(env: &Env, id: &str) -> Option<String> {
    let mut do_reachable = false;
    if let Some((status, text)) = route_call(
        env,
        &format!("/route?uid={}", encode_uri_component(id)),
        Method::Get,
        None,
    )
    .await
    {
        if (200..300).contains(&status) {
            do_reachable = true;
            // `await res.json().catch(() => null)` — an unreadable body is `null`, not a throw.
            if let Ok(data) = serde_json::from_str::<Value>(&text) {
                if let Some(model) = data.get("model").filter(|m| !m.is_null()) {
                    return Some(summrise_core::text::js_string(model));
                }
            }
        }
    }
    // The legacy KV fallback, and the ONE-TIME migration — only while the DO is reachable.
    let legacy = crate::device_store::kv_text(env, &legacy_key(id)).await;
    let legacy = legacy.filter(|v| !v.is_empty())?;
    if do_reachable {
        let _ = set_user_route(env, id, Some(&legacy)).await;
        crate::device_store::kv_delete(env, &legacy_key(id)).await;
    }
    Some(legacy)
}

/// `setUserRoute(env, id, model)` — the engine of both the console's write and the migration above.
///
/// The `Result` is the source's ONE throw: an absent KV binding with an unreachable DO. Every other failure is
/// swallowed and answered by the KV fallback, which is what keeps a console caller from seeing a bare fetch
/// error.
pub async fn set_user_route(
    env: &Env,
    id: &str,
    model: Option<&str>,
) -> std::result::Result<(), String> {
    let empty = model.is_none_or(str::is_empty);
    let call = if empty {
        route_call(
            env,
            &format!("/route?uid={}", encode_uri_component(id)),
            Method::Delete,
            None,
        )
        .await
    } else {
        let body = serde_json::json!({ "uid": id, "model": model.unwrap_or_default() }).to_string();
        route_call(env, "/route", Method::Put, Some(body)).await
    };
    if let Some((status, _)) = call {
        if (200..300).contains(&status) {
            return Ok(());
        }
    }
    // **THE SOURCE'S ONE THROW**: an absent KV binding with the DO unreachable. `kv_text` answers `None` for an
    // absent binding too, so the binding is asked for directly here rather than inferred from a read.
    if env.kv("KEYS").is_err() {
        return Err(
            "config_error: route store unavailable (RouteDO unreachable, no KV)".to_string(),
        );
    }
    if empty {
        crate::device_store::kv_delete(env, &legacy_key(id)).await;
    } else {
        // No TTL: the legacy key is a MIGRATION SOURCE, and the source's own `put` carries no options.
        crate::device_store::kv_put(env, &legacy_key(id), model.unwrap_or_default(), None).await;
    }
    Ok(())
}

/// `isAdvertised(env, model)` — the catalogue's own answer, from the reader the admin routes use, so a probe
/// and a route can never disagree about what this deployment advertises.
pub async fn is_advertised(env: &Env, model: &str) -> bool {
    crate::admin_routes::advertised_ids_for(env)
        .await
        .iter()
        .any(|id| id == model)
}

/// `isModelUsable(env, model, uid)` — the catalogue first, then the channel's key matrix, then (for `og/` only)
/// the breaker.
pub async fn is_model_usable(env: &Env, model: &str, uid: &str) -> bool {
    if !is_advertised(env, model).await {
        return false;
    }
    let ukeys = crate::user_store::get_user_keys(env, uid).await;
    let prefix = model.split('/').next().unwrap_or_default();
    let rule = crate::byok::rule_for_prefix(prefix);
    let has_user = rule.is_some_and(|(user_key, _)| {
        ukeys
            .get(user_key)
            .is_some_and(|v| !crate::device_registry::js_falsy(v))
    });
    let has_env = rule
        .and_then(|(_, env_key)| env_key)
        .is_some_and(|env_key| env.var(env_key).is_ok());
    let degraded = if model.starts_with("og/") {
        crate::read_breaker(env).await.unwrap_or(false)
    } else {
        false
    };
    auto_route::channel_usable(model, rule, has_user, has_env, degraded)
}

/// `resolveAutoModel(env, uid)` — the model a `model=auto` request actually resolves to.
///
/// **THE VERDICTS ARE GATHERED FOR THE LADDER, NOT FOR ONE MODEL**: `pickAutoRoute` walks
/// `AUTO_FALLBACK_LADDER` in order, so every rung is asked once, in order, and the first usable one wins.
pub async fn resolve_auto_model(env: &Env, uid: &str) -> String {
    let chosen = get_user_route(env, uid).await;
    let chosen_usable = match chosen.as_deref() {
        Some(model) if !model.is_empty() => is_model_usable(env, model, uid).await,
        _ => false,
    };
    let mut ladder = Vec::with_capacity(auto_route::AUTO_FALLBACK_LADDER.len());
    for model in auto_route::AUTO_FALLBACK_LADDER.iter() {
        ladder.push(is_model_usable(env, model, uid).await);
    }
    auto_route::pick_auto_route(chosen.as_deref(), chosen_usable, &ladder)
}

/// `encodeURIComponent(id)` for a uid.
///
/// **AN ID THAT IS NOT `[A-Za-z0-9-_.!~*'()]` IS PERCENT-ENCODED**, which is what the source's
/// `encodeURIComponent` does — a uid is a hex string in practice, and the rule is carried because a query
/// string built by hand is where an id with a `&` in it would become a second parameter.
fn encode_uri_component(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => (b as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! The pure half: the query-string encoding. **THE THREE WRAPPERS AROUND THE DO ARE THE HARNESS'S
    //! BUSINESS** (`verify.mjs` drives the built worker with a `ROUTE` binding stub, the same way the device
    //! family's KV and the breaker are stubbed), and the resolution's policy is `auto_route.rs`'s own tests.
    use super::*;

    #[test]
    fn a_uid_is_percent_encoded_the_way_the_source_encodes_it() {
        assert_eq!(encode_uri_component("u-1"), "u-1");
        assert_eq!(encode_uri_component("a b"), "a%20b");
        assert_eq!(encode_uri_component("a&b=c"), "a%26b%3Dc");
        assert_eq!(encode_uri_component("ü"), "%C3%BC");
        // The legacy key is the raw id, NOT the encoded one — the source interpolates `id` there.
        assert_eq!(legacy_key("a b"), "route:a b");
    }
}
