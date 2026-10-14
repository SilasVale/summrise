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
use crate::device_store::{kv_json_cached, kv_list_prefix};
use crate::user_store::{get_user_keys, user_keys_status};
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
        Method::Get => matches!(path, "/api/admin/users"),
        _ => false,
    }
}

/// `handle(request, env)` — the family's routes, in the plugin's own order.
pub async fn handle(request: Request, env: &Env) -> Result<Response, RouteFailure> {
    let url = request.url()?;
    match (request.method(), url.path()) {
        (Method::Get, "/api/admin/users") => admin_list_users(&request, env).await,
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
