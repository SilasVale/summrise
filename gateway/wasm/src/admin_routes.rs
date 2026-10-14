//! THE ADMIN FAMILY — `gateway/src/plugins/admin.ts`, ported route by route.
//!
//! **THIS IS THE FIRST ROUTE-FAMILY PORT, AND IT IS WHERE THE CONSOLE'S TAIL STOPS BEING A STORE PROBLEM.** The
//! stores underneath are already Rust (`user_store`, `model_store`, `store`, `providers`), and `session_gate`
//! already owns the gate (`require_admin`, with the source's two refusal bodies). What a route adds is the part
//! that has no home yet: the PATH MATCHING, the JSON shape, and the masking rules a response must not forget.
//!
//! **THE FAMILY IS 20 ROUTES AND THIS FILE SERVES TWO OF THEM.** The rest are enumerated in the plugin's own
//! table and land in their own slices; a route that is not here yet is NOT answered by this module, so the front
//! door's fallback still owns it (`in_family` is the family, `handle` answers `404` for a path it does not
//! serve — the same split `auth_routes` uses).
//!
//! **THE FIRST SLICE IS `GET /api/admin/users`, CHOSEN BECAUSE EVERY INPUT WAS ALREADY RUST'S**: the session
//! gate, `listUsers`, `getUserKeys`, `maskKey` and `userKeysStatus`. `GET /api/admin/public` — the obvious
//! "first" route — is NOT portable yet, and the reason is worth writing down: its catalogue merges
//! `ROUTE_INFO`'s display prose (a console table, TS-only) with the CATALOGUE FILE layer (`file-config.ts`, not
//! ported). A route port that guessed at either would be a divergence with no oracle.

use serde_json::{Map, Value};
use worker::*;

use crate::device_registry::{js_falsy_string, mask_key};
use crate::device_store::{cf_token, kv_json_cached, kv_list_prefix};
use crate::model_store::{
    delete_custom_model, ids_of, known_prefixes, parse_model_spec, put_custom_model,
    put_facet_override, set_model_disabled, PINNED_FACETS,
};
use crate::user_store::{
    create_invite, get_user, get_user_keys, has_admin_password, set_admin_password,
    set_user_enabled, user_keys_status, verify_admin_password, ADMIN_ID,
};
use crate::RouteFailure;

/// The plugin's base, and the separator is part of every test (a bare `starts_with` would claim
/// `/api/administer` — the same trap `auth_routes` documents).
const ADMIN_BASE: &str = "/api/admin";

/// `in_family(method, path)` — the paths THIS module claims. A path in the family that `handle` does not serve
/// answers 404 from here, which is the front door's answer for a family prefix with no route under it.
pub fn in_family(method: &Method, path: &str) -> bool {
    if path != ADMIN_BASE && !path.starts_with(&format!("{ADMIN_BASE}/")) {
        return false;
    }
    match method {
        // The plugin's own table is exact-match for these two, and `Method::Put` needs the dynamic arm:
        // `startsWith(BASE + "/users/") && endsWith("/enabled")` — the source's own matcher, and the reason it
        // is not a prefix match is recorded there ("exact here so /api/admin/users never swallows
        // /api/admin/users/{id}/enabled").
        Method::Get => matches!(
            path,
            "/api/admin/users" | "/api/admin/cloudflare-token" | "/api/admin/password"
        ),
        // PUT is three routes with one method, and two of them are the plugin's DYNAMIC matchers: a prefix plus
        // a suffix, which is why they are written out rather than matched exactly.
        Method::Put => {
            path == "/api/admin/password"
                || (path.starts_with(&format!("{ADMIN_BASE}/users/")) && path.ends_with("/enabled"))
                || (path.starts_with(&format!("{ADMIN_BASE}/models/"))
                    && path.ends_with("/enabled"))
        }
        Method::Post => matches!(path, "/api/admin/invite" | "/api/admin/models"),
        // DELETE on the model family is the id read off the path, encoded or bare.
        Method::Delete => path.starts_with(&format!("{ADMIN_BASE}/models/")),
        _ => false,
    }
}

/// `handle(request, env)` — the family's routes, in the plugin's own order.
pub async fn handle(mut request: Request, env: &Env) -> Result<Response, RouteFailure> {
    let url = request.url()?;
    match (request.method(), url.path()) {
        (Method::Get, "/api/admin/users") => admin_list_users(&request, env).await,
        (Method::Get, "/api/admin/cloudflare-token") => admin_get_cf_token(&request, env).await,
        (Method::Get, "/api/admin/password") => admin_get_password(&request, env).await,
        (Method::Put, "/api/admin/password") => admin_put_password(&mut request, env).await,
        (Method::Post, "/api/admin/invite") => admin_invite(&request, env).await,
        (Method::Post, "/api/admin/models") => admin_add_model(&mut request, env).await,
        (Method::Delete, path) if path.starts_with(&format!("{ADMIN_BASE}/models/")) => {
            admin_delete_model(
                &request,
                env,
                &path[format!("{ADMIN_BASE}/models/").len()..],
            )
            .await
        }
        (Method::Put, path)
            if path.starts_with(&format!("{ADMIN_BASE}/models/")) && path.ends_with("/enabled") =>
        {
            let id = &path[format!("{ADMIN_BASE}/models/").len()..path.len() - "/enabled".len()];
            admin_enable_model(&request, env, id).await
        }
        (Method::Put, path)
            if path.starts_with(&format!("{ADMIN_BASE}/users/")) && path.ends_with("/enabled") =>
        {
            admin_set_user_enabled(&mut request, env, path).await
        }
        // A path under the family with no route here: the front door's own 404, in the source's bytes.
        _ => Ok(crate::json_error(404, "Not Found", "not_found_error")?),
    }
}

/// `listUsers(env)` — **THE SORT IS A DECISION AND IT IS THE SOURCE'S**: `(a.createdAt || 0) - (b.createdAt ||
/// 0)`, so a record with no `createdAt` sorts as the epoch rather than being dropped, and a STABLE sort keeps
/// KV's own order for equal stamps.
///
/// `env.KEYS` unbound answers the empty list rather than throwing — the source's own first line.
pub async fn list_users(env: &Env) -> Vec<Value> {
    if env.kv("KEYS").is_err() {
        return Vec::new();
    }
    let mut out: Vec<Value> = Vec::new();
    for (name, _expiration) in kv_list_prefix(env, "user:").await {
        if let Some(user) = kv_json_cached(env, &name).await {
            out.push(user);
        }
    }
    out.sort_by(|a, b| {
        // `(a.createdAt || 0) - (b.createdAt || 0)`: a missing field, a `null` and a non-number all fall to 0
        // through the source's own coercion, and the comparison is on numbers.
        let key = |v: &Value| {
            let raw = v.get("createdAt").unwrap_or(&Value::Null);
            // `|| 0` FIRST (a falsy stamp is the epoch), then the subtraction's numeric coercion.
            let raw = if summrise_core::text::js_truthy(raw) {
                raw
            } else {
                &Value::Null
            };
            summrise_core::text::js_number(raw)
        };
        key(a)
            .partial_cmp(&key(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// `adminGetCfToken(request, env)` — **THE TOKEN IS NEVER RETURNED, ONLY ITS EXISTENCE AND A MASK.** The
/// account-level Cloudflare API token is a credential for the whole zone; the console needs to know whether one
/// is set and which one it is, and neither of those requires the value.
async fn admin_get_cf_token(request: &Request, env: &Env) -> Result<Response, RouteFailure> {
    if let Err(refusal) = crate::session_gate::require_admin(request, env).await? {
        return Ok(refusal);
    }
    let token = cf_token(env).await;
    let body = serde_json::to_string(&serde_json::json!({
        "configured": !token.is_empty(),
        "masked": if token.is_empty() { String::new() } else { mask_key(&token) },
    }))
    .map_err(|_| RouteFailure::Threw)?;
    Ok(crate::json(body, 200)?)
}

/// `isBuiltIn(id)` — over THIS worker's registry, which is the same table the console's `channels.ts` mirrors.
fn is_built_in(id: &str) -> bool {
    crate::registry::model_spec(id).is_some()
}

/// `decodeModelId(id)` — `decodeURIComponent(id)`, the same helper the device family uses. **THE SOURCE CALLS
/// IT WITHOUT A `try`**, so a malformed escape there is an unhandled `URIError` and the front door's 500; this
/// is the same answer, reached deliberately.
fn decode_model_id(raw: &str) -> Result<String, RouteFailure> {
    crate::devices::decode_device_name(raw).ok_or(RouteFailure::Threw)
}

/// `adminAddModel(request, env)` — **ONE ROUTE, TWO ANSWERS, AND THE REGISTRY DECIDES.**
///
/// A BUILT-IN's routing facets are pinned in the channel registry and its display facets are not in the registry
/// at all — so an operator may store the LATTER and never the FORMER, and a record carrying `wire` or `usEgress`
/// is refused BY NAME rather than quietly ignored ("an operator who set one must not believe it took"). What
/// lands is a FACET OVERRIDE. Everything else is a console-owned model, and what lands is a custom record.
///
/// The two arms answer different shapes — `override` + every override id, versus the whole `model` record plus
/// every custom id — because the console re-renders from them.
async fn admin_add_model(request: &mut Request, env: &Env) -> Result<Response, RouteFailure> {
    if let Err(refusal) = crate::session_gate::require_admin(request, env).await? {
        return Ok(refusal);
    }
    let body = read_json(request).await;
    let spec = match parse_model_spec(&body, &known_prefixes()) {
        Ok(spec) => spec,
        Err(error) => return Ok(crate::json_error(400, &error, "invalid_request")?),
    };
    let id = spec
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let answer = if is_built_in(&id) {
        let pinned: Vec<&str> = PINNED_FACETS
            .iter()
            .copied()
            .filter(|f| spec.get(*f).is_some())
            .collect();
        if !pinned.is_empty() {
            return Ok(crate::json_error(
                400,
                &format!(
                    "a built-in model's {} is pinned in the channel registry — only name, contextWindow, \
                     maxTokens and reasoningEffort can be stored for it",
                    pinned.join(", ")
                ),
                "invalid_request",
            )?);
        }
        // Only the four display facets, and only when they are TRUTHY — the source's spread.
        let mut override_spec = serde_json::Map::new();
        override_spec.insert("id".into(), Value::String(id.clone()));
        for field in ["name", "contextWindow", "maxTokens", "reasoningEffort"] {
            if let Some(v) = spec.get(field) {
                if crate::device_registry::js_truthy(v) {
                    override_spec.insert(field.to_string(), v.clone());
                }
            }
        }
        let next = put_facet_override(env, &Value::Object(override_spec)).await;
        serde_json::json!({ "ok": true, "override": id, "overrides": ids_of(&next) })
    } else {
        let next = put_custom_model(env, &spec).await;
        serde_json::json!({ "ok": true, "model": spec, "custom": ids_of(&next) })
    };
    let text = serde_json::to_string(&answer).map_err(|_| RouteFailure::Threw)?;
    Ok(crate::json(text, 200)?)
}

/// `adminDeleteModel(request, env, id)` — **DELETE MEANS TWO DIFFERENT THINGS AND THE REGISTRY DECIDES WHICH.**
/// A console-owned model is REMOVED; a BUILT-IN is DISABLED, because its routing facets are pinned in the
/// registry and deleting it would be deleting a compile-time fact. The 404 is the source's: an id this
/// deployment does not own and the registry does not know.
async fn admin_delete_model(
    request: &Request,
    env: &Env,
    raw_id: &str,
) -> Result<Response, RouteFailure> {
    if let Err(refusal) = crate::session_gate::require_admin(request, env).await? {
        return Ok(refusal);
    }
    let id = decode_model_id(raw_id)?;
    let body = if is_built_in(&id) {
        let next = set_model_disabled(env, &id, true).await;
        serde_json::json!({ "ok": true, "disabled": id, "disabledModels": next })
    } else {
        if !delete_custom_model(env, &id).await {
            return Ok(crate::json_error(
                404,
                &format!("No custom model {id}"),
                "not_found_error",
            )?);
        }
        serde_json::json!({ "ok": true, "removed": id })
    };
    let text = serde_json::to_string(&body).map_err(|_| RouteFailure::Threw)?;
    Ok(crate::json(text, 200)?)
}

/// `adminEnableModel(request, env, id)` — the inverse, and it only ever means one thing: the id leaves the
/// disabled list (a built-in that was never disabled is simply not in it).
async fn admin_enable_model(
    request: &Request,
    env: &Env,
    raw_id: &str,
) -> Result<Response, RouteFailure> {
    if let Err(refusal) = crate::session_gate::require_admin(request, env).await? {
        return Ok(refusal);
    }
    let id = decode_model_id(raw_id)?;
    let next = set_model_disabled(env, &id, false).await;
    let text = serde_json::to_string(
        &serde_json::json!({ "ok": true, "enabled": id, "disabledModels": next }),
    )
    .map_err(|_| RouteFailure::Threw)?;
    Ok(crate::json(text, 200)?)
}

/// `adminGetPassword(request, env)` — **THE PASSWORD ITSELF IS NEVER RETURNED, ONLY WHETHER ONE IS SET.** The
/// route used to hand the stored value back verbatim (plaintext, before the hashing landing), which made any
/// console session able to impersonate the admin indefinitely.
async fn admin_get_password(request: &Request, env: &Env) -> Result<Response, RouteFailure> {
    if let Err(refusal) = crate::session_gate::require_admin(request, env).await? {
        return Ok(refusal);
    }
    let body = serde_json::to_string(&serde_json::json!({ "set": has_admin_password(env).await }))
        .map_err(|_| RouteFailure::Threw)?;
    Ok(crate::json(body, 200)?)
}

/// `adminPutPassword(request, env)` — **TWO ROUTES IN ONE, AND THE FIRST ARM IS THE BOOTSTRAP.**
///
/// round-119: bootstrap is circular. With NO password set, no session can ever exist (`requireSession` answers
/// null on an empty `getAdminPassword`, and login 500s "not configured"), so a fresh deployment with no
/// `ADMIN_PASSWORD` secret had NO way into the console. The rule that resolves it: a session-less FIRST set is
/// allowed, gated by the ADMIN GATEWAY TOKEN from KV — the same credential the reset-password path uses — and
/// it answers `initial: true` so the console can tell the two apart. **Once a password exists, the session and
/// the CURRENT password are both required** (a hijacked session must not be able to rotate the password and lock
/// the real admin out), and the answer is `changed: true` without `initial`.
///
/// The order inside the bootstrap arm is the source's: LENGTH first (a 400 before any store read), then the key.
async fn admin_put_password(request: &mut Request, env: &Env) -> Result<Response, RouteFailure> {
    if !has_admin_password(env).await {
        let body = read_json(request).await;
        let password = js_or_empty(body.get("password"));
        if js_length(&password) < 8 {
            return Ok(crate::json_error(
                400,
                "Admin password must be at least 8 chars",
                "invalid_request",
            )?);
        }
        // **THE ADMIN KEY IS THE BOOTSTRAP'S CREDENTIAL, COMPARED IN CONSTANT TIME** — an unauthenticated caller
        // on the internet must not be able to set the console's password on a fresh deployment, and a
        // length-leaking comparison on a token is the other half of that.
        let admin = get_user(env, ADMIN_ID).await;
        let admin_token = admin
            .as_ref()
            .map(|u| js_falsy_string(u.get("token")))
            .unwrap_or_default();
        let admin_key = js_or_empty(body.get("adminKey")).trim().to_string();
        if admin_token.is_empty() || !crate::auth::safe_eq(&admin_key, &admin_token) {
            return Ok(crate::json_error(
                403,
                "Invalid admin key — cannot set initial password",
                "authentication_error",
            )?);
        }
        if set_admin_password(env, &password).await.is_err() {
            return Err(RouteFailure::Threw);
        }
        let body = serde_json::to_string(
            &serde_json::json!({"ok": true, "changed": true, "initial": true}),
        )
        .map_err(|_| RouteFailure::Threw)?;
        return Ok(crate::json(body, 200)?);
    }
    if let Err(refusal) = crate::session_gate::require_admin(request, env).await? {
        return Ok(refusal);
    }
    let body = read_json(request).await;
    let password = js_or_empty(body.get("password"));
    if js_length(&password) < 8 {
        return Ok(crate::json_error(
            400,
            "Admin password must be at least 8 chars",
            "invalid_request",
        )?);
    }
    let current = js_or_empty(body.get("currentPassword"));
    if !verify_admin_password(env, &current).await {
        return Ok(crate::json_error(
            403,
            "Current password is incorrect",
            "authentication_error",
        )?);
    }
    if set_admin_password(env, &password).await.is_err() {
        return Err(RouteFailure::Threw);
    }
    let body = serde_json::to_string(&serde_json::json!({ "ok": true, "changed": true }))
        .map_err(|_| RouteFailure::Threw)?;
    Ok(crate::json(body, 200)?)
}

/// `adminInvite(request, env)` — a code, and nothing else. The TTL lives at the binding ([`create_invite`]).
async fn admin_invite(request: &Request, env: &Env) -> Result<Response, RouteFailure> {
    if let Err(refusal) = crate::session_gate::require_admin(request, env).await? {
        return Ok(refusal);
    }
    let code = create_invite(env).await;
    let body = serde_json::to_string(&serde_json::json!({ "ok": true, "code": code }))
        .map_err(|_| RouteFailure::Threw)?;
    Ok(crate::json(body, 200)?)
}

/// `String(v || "")` — the coercion `String(body?.password || "")` performs, named once.
fn js_or_empty(v: Option<&serde_json::Value>) -> String {
    summrise_core::text::js_or_empty(v)
}

/// `v.length` — UTF-16 code units, and `chars()` is NOT it (see the core's [`js_length`]).
fn js_length(text: &str) -> usize {
    summrise_core::text::js_length(text)
}

/// `adminSetUserEnabled(request, env, url)` — **THE WRITE ROUTE, AND ITS THREE REFUSALS ARE EACH A ROUND**.
///
/// * **round-107**: a malformed percent-escape in the id threw `URIError` and answered 500. The decode is the
///   same helper the device family uses, which is the same `decodeURIComponent` — including its `+`-is-not-a-
///   space arm and its refusal of a truncated or non-UTF-8 escape.
/// * **the admin account cannot be disabled** — a 400 with `invalid_request`, before any store call. Disabling
///   it would lock the operator out of the console they are standing in.
/// * **an unknown id is the store's own throw** (`User not found`), which becomes the front door's 500 — the
///   source has no catch for it, and inventing a 404 here would be a decision the source never made.
///
/// `!!body.enabled` IS THE COERCION: a missing field, a `"false"` STRING and a `0` all mean DISABLE, which is
/// what `!!` does — and the source's `body?.enabled` is why a `null` body is not a throw here (unlike
/// `login`'s `body.username`).
async fn admin_set_user_enabled(
    request: &mut Request,
    env: &Env,
    path: &str,
) -> Result<Response, RouteFailure> {
    if let Err(refusal) = crate::session_gate::require_admin(request, env).await? {
        return Ok(refusal);
    }
    let raw = &path[format!("{ADMIN_BASE}/users/").len()..path.len() - "/enabled".len()];
    let Some(id) = crate::devices::decode_device_name(raw) else {
        return Ok(crate::json_error(
            400,
            "Invalid user id",
            "invalid_request",
        )?);
    };
    let body = read_json(request).await;
    if id == ADMIN_ID {
        return Ok(crate::json_error(
            400,
            "Cannot disable the admin account",
            "invalid_request",
        )?);
    }
    let enabled = crate::device_registry::js_truthy(body.get("enabled").unwrap_or(&Value::Null));
    match set_user_enabled(env, &id, enabled).await {
        Ok(user) => {
            let body = serde_json::to_string(&serde_json::json!({
                "ok": true,
                "id": id,
                "enabled": crate::device_registry::js_truthy(
                    user.get("enabled").unwrap_or(&Value::Null)
                ),
            }))
            .map_err(|_| RouteFailure::Threw)?;
            Ok(crate::json(body, 200)?)
        }
        // `setUserEnabled` throwing is the front door's 500, not a route answer.
        Err(_) => Err(RouteFailure::Threw),
    }
}

/// `readJson(request)` — the parse failure arm is `{}` (the source's `catch`), and a body that parses to
/// `null` stays `null` (the optional-chaining call sites depend on it).
async fn read_json(request: &mut Request) -> Value {
    match request.text().await {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    }
}

/// `adminListUsers(request, env)` — the gate, then every user with its credentials MASKED.
///
/// **`token` AND `relayToken` GO THROUGH `maskKey`, AND THAT IS THE ROUTE'S WHOLE POINT**: a console session
/// holder must not be able to harvest every user's credential from a list (there is no reveal endpoint by
/// design; rotation lives in `/api/me`).
///
/// FIELD ORDER IS THE RESPONSE BYTES: `id, username, role, enabled, createdAt, token, relayToken, keys`.
async fn admin_list_users(request: &Request, env: &Env) -> Result<Response, RouteFailure> {
    // The gate: `require_admin` answers the source's own 401/403 bodies, and the resolved user is what the
    // write routes below will check against (`PUT .../enabled` refuses the admin account by ID).
    if let Err(refusal) = crate::session_gate::require_admin(request, env).await? {
        return Ok(refusal);
    }

    let users = list_users(env).await;
    let mut out: Vec<Value> = Vec::new();
    for u in &users {
        let mut row = Map::new();
        // The five fields are COPIED from the record, so an absent one is absent — `u.enabled` on a record
        // without the field is `undefined`, and `JSON.stringify` drops it.
        for name in ["id", "username", "role", "enabled", "createdAt"] {
            if let Some(v) = u.get(name) {
                row.insert(name.to_string(), v.clone());
            }
        }
        row.insert(
            "token".to_string(),
            Value::String(mask_key(&js_falsy_string(u.get("token")))),
        );
        row.insert(
            "relayToken".to_string(),
            Value::String(mask_key(&js_falsy_string(u.get("relayToken")))),
        );
        let id = u.get("id").and_then(Value::as_str).unwrap_or_default();
        let ukeys = get_user_keys(env, id).await;
        row.insert(
            "keys".to_string(),
            user_keys_status(&ukeys, env).unwrap_or(Value::Null),
        );
        out.push(Value::Object(row));
    }
    let body = serde_json::to_string(&serde_json::json!({ "users": out }))
        .map_err(|_| RouteFailure::Threw)?;
    Ok(crate::json(body, 200)?)
}
