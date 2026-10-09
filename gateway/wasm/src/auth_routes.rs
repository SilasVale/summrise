//! THE IDENTITY SURFACE'S ROUTE TABLE — `gateway/src/plugins/auth.ts`'s handlers, and the ORDER they run in.
//!
//! WHAT IS HERE. Fourteen routes in three groups: the credential routes (`POST /api/auth/register`,
//! `/login`, `/reset-password`, `/logout`), the account route (`GET /api/me`), and the caller's own credential
//! routes (`/api/me/token/*`, `/api/me/keys`, `/api/me/keys/reveal`, `/api/me/usproxy`). Together they are the
//! surface that decides WHO MAY CALL the console and WHAT THE CALLER MAY DO — the session is issued here, ended
//! here, rotated here, and the role that gates an admin route is read here.
//!
//! WHAT IS NOT, AND EACH WITH A REASON RATHER THAN A SENTENCE:
//!
//!   * `GET|PUT /api/me/route` — the per-user `model=auto` selection. Its handler resolves through
//!     `resolveAutoModel` (`model-route.ts`), which reads the model CATALOGUE (`store/models.ts`: the static
//!     routes, the custom models, the provider models, the disabled set) and the `RouteDO` Durable Object. That
//!     is a different surface with its own corpus — the catalogue, not identity — and this slice does not port
//!     it. The route stays on the TypeScript path and the cutover section proves that it does.
//!   * `POST /api/me/keys/test` and `POST /api/me/keys/usage` — the key DIAGNOSTICS. They dial six providers
//!     (DeepSeek, OpenRouter, OpenCode Go, Command Code, GMI, AMD, NVIDIA, Qwen) and three usage endpoints,
//!     through `fetchWithTimeout` and the channel tables. The decisions in them are the providers' wire shapes,
//!     not the caller's identity; porting them here would put two unrelated corpora in one slice.
//!
//! **THE RESPONSE BYTES ARE THE SOURCE'S, INCLUDING THE ARMS THAT ARE AWKWARD:**
//!
//!   * A `null` BODY THROWS IN TWO PLACES AND LANDS IN TWO DIFFERENT ARMS. `register` evaluates
//!     `body.username` INSIDE its try, so the TypeError becomes `400 invalid_request` carrying V8's own message;
//!     `login` evaluates it OUTSIDE any try, so it becomes the front door's `500 Internal error`. Both are
//!     reproduced — see [`NULL_BODY_USERNAME`] — rather than "fixed" into a clean 400, because the console's
//!     registration form is a live caller of the first arm.
//!   * `meGet` SPREADS THE USER RECORD, so a field the record does not have is a key the response does not
//!     have (`JSON.stringify` drops `undefined`). The response is built field by field for that reason.
//!   * `mePutUsproxy`'s non-admin arm is `403 "Admin only"` with the type `"forbidden"` — NOT the
//!     `"authorization_error"` its sibling `requireAdmin` uses, and not a typo this port may normalise.
//!   * `logout`'s TTL is `Math.max(60, Math.min(86400, Math.ceil((exp - now) / 1000)))` over a value whose type
//!     is only checked for TRUTHINESS, so `exp: "0"` is a 60-second entry and `exp: {}` is a KV put the runtime
//!     rejects (inside the source's try, so the request still answers 200 with no write).
//!
//! THE CSRF GATE IS NOT HERE, AND ITS ABSENCE IS DELIBERATE. `csrfCookieViolation` runs in `index.ts` BEFORE the
//! handover, so a cookie-carrying cross-site mutation is refused by the front door and never reaches either
//! implementation. Porting it here would be a second copy of a decision that is not this surface's.

use serde_json::{json, Map, Value};
use worker::*;

use crate::auth::{clear_session_cookie_header, parse_cookie, safe_eq, session_cookie_header, SESSION_COOKIE};
use crate::device_registry::{js_falsy, js_falsy_string, js_to_string};
use crate::device_store::{kv_put, kv_text, now_ms};
use crate::ip_rate_limit::{check, Counters, IpRateLimiter};
use crate::session_gate;
use crate::user_store;

const AUTH_BASE: &str = "/api/auth";
const ME_BASE: &str = "/api/me";

/// `createIpRateLimiter({name: "auth-rate", limit: 30, windowMs: 60_000})` — the plugin's own per-IP gate, in
/// front of register / reset-password / logout (round-104's reason: each costs 2-3 KV writes, and an attacker
/// can exhaust the Free plan's daily write quota with them).
static AUTH_RATE: IpRateLimiter = IpRateLimiter {
    name: "auth-rate",
    limit: 30,
    window_ms: 60_000,
};
static AUTH_RATE_COUNTERS: std::sync::Mutex<Counters> = std::sync::Mutex::new(Vec::new());

/// `__loginGate` — `login:<ip>:<minute>` → the burst count, ten per minute per address.
static LOGIN_GATE: std::sync::Mutex<Counters> = std::sync::Mutex::new(Vec::new());
/// `__loginFails` — `login-fails:<ip>:<userId>` → consecutive failures; the fifth arms the KV lock.
static LOGIN_FAILS: std::sync::Mutex<Counters> = std::sync::Mutex::new(Vec::new());

/// **V8'S OWN TypeError MESSAGE, CARRIED AS A LITERAL — AND THIS IS A REPRODUCTION, NOT AN IMPROVEMENT.**
///
/// `authRegister` evaluates `body.username` INSIDE its `try`, so a request whose body is the four bytes `null`
/// raises a TypeError and the handler's catch answers `jsonError(400, e.message, "invalid_request")`. The port
/// cannot raise a JavaScript TypeError, so the message is carried verbatim; answering a cleaner
/// "username is required" would be a DIFFERENT BODY on a route the console's own registration form calls.
///
/// The message is engine phrasing, and both implementations run under V8 in production (the console worker and
/// this worker are both workerd). A future V8 rewording would make the shipping implementation answer something
/// this constant does not — which is why the constant is named here, and why the harness's message assertion is
/// a recorded byte comparison rather than a claim about the string.
const NULL_BODY_USERNAME: &str = "Cannot read properties of null (reading 'username')";

/// **A HANDLER'S ANSWER: A RESPONSE, OR WHY IT HAS NONE.** The failure type is the crate's own
/// (`lib.rs::RouteFailure`) because `session_gate` and this module must speak about the same two arms —
/// [`crate::Routecrate::RouteFailure::Threw`] is the source raising, which the front door's catch answers `500
/// Internal error`, and [`crate::RouteFailure::Platform`] is a `workers-rs` failure while building a response.
type Handled = Result<Response, crate::RouteFailure>;

/* ─────────────────────────── the family ─────────────────────────── */

/// **WHICH PATHS THIS WORKER OWNS, MIRRORED EXACTLY IN `index.ts`'S `isAuthFamily`.** It is the same kind of
/// predicate `devices::in_family` is, and the same rule applies: a path the front door hands over is a path this
/// worker must answer, so the two spellings are one list.
///
/// `/api/auth/<anything>` is the family (the four routes above AND the front door's own 404 for the shapes the
/// plugin does not register). `/api/me` is the family EXCEPT the three routes this slice does not port — which
/// is why the shape is a prefix with three exclusions rather than a path list: `/api/me/anything/else` must
/// answer this worker's 404 rather than fall through to an implementation the front door would have sent here.
pub fn in_family(_method: &Method, path: &str) -> bool {
    // `/api/auth` is the plugin's BASE and every route is `BASE + "/…"`, so the separator is part of the test:
    // a bare `starts_with("/api/auth")` would also claim `/api/authentication`, a path the front door answers
    // with its own 404 — the same bytes today, and a different decision the day a route is named that.
    if path == AUTH_BASE || path.starts_with(&format!("{AUTH_BASE}/")) {
        return true;
    }
    if path == ME_BASE {
        return true;
    }
    if !path.starts_with(&format!("{ME_BASE}/")) {
        return false;
    }
    !matches!(
        path,
        "/api/me/route" | "/api/me/keys/test" | "/api/me/keys/usage"
    )
}

/// The dispatcher: an exact `(method, path)` match for every route the plugin registers, in the plugin's order,
/// and `None` for a family path it does not own (the caller answers the front door's 404).
pub async fn handle(mut request: Request, env: &Env) -> Handled {
    let method = request.method();
    let path = request.url().map(|u| u.path().to_string()).unwrap_or_default();
    let secure = request
        .url()
        .map(|u| u.scheme() == "https")
        .unwrap_or(false);

    match (method, path.as_str()) {
        (Method::Post, "/api/auth/register") => auth_register(&mut request, env, secure).await,
        (Method::Post, "/api/auth/login") => auth_login(&mut request, env, secure).await,
        (Method::Post, "/api/auth/reset-password") => auth_reset_password(&mut request, env).await,
        (Method::Post, "/api/auth/logout") => auth_logout(&mut request, env, secure).await,
        (Method::Get, "/api/me") => me_get(&request, env).await,
        (Method::Post, "/api/me/token/regenerate") => me_regenerate_token(&request, env).await,
        (Method::Post, "/api/me/token/relay") => me_rotate_relay_token(&request, env).await,
        (Method::Delete, "/api/me/token/relay") => me_revoke_relay_token(&request, env).await,
        (Method::Post, "/api/me/token/relay/reveal") => me_reveal_relay_token(&request, env).await,
        (Method::Get, "/api/me/usproxy") => me_get_usproxy(&request, env).await,
        (Method::Put, "/api/me/usproxy") => me_put_usproxy(&mut request, env).await,
        (Method::Put, "/api/me/keys") => me_put_keys(&mut request, env).await,
        (Method::Delete, "/api/me/keys") => me_delete_keys(&request, env).await,
        (Method::Post, "/api/me/keys/reveal") => me_reveal_key(&mut request, env).await,
        _ => Ok(crate::json_error(404, "Not Found", "not_found_error")?),
    }
}

/* ─────────────────────────── the pieces every handler shares ─────────────────────────── */

/// `jsonOk(data, extraHeaders)`.
fn json_ok_with(value: Value, extra: &[(&str, String)]) -> Result<Response> {
    let mut response = crate::json(serde_json::to_string(&value)?, 200)?;
    for (name, value) in extra {
        response.headers_mut().set(name, value)?;
    }
    Ok(response)
}

/// `jsonOk(data)`.
fn json_ok(value: Value) -> Result<Response> {
    json_ok_with(value, &[])
}

/// `jsonError(status, message, type, extraHeaders)`.
fn json_error_with(status: u16, message: &str, kind: &str, extra: &[(&str, String)]) -> Result<Response> {
    let mut response = crate::json_error(status, message, kind)?;
    for (name, value) in extra {
        response.headers_mut().set(name, value)?;
    }
    Ok(response)
}

/// `readJson(request)` — **THE PARSE FAILURE ARM INCLUDES THE EMPTY BODY**, and the `null` body is NOT a parse
/// failure: it is `JSON.parse("null")`, which is the value that makes `body.username` throw.
async fn read_json(request: &mut Request) -> Value {
    match request.text().await {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| json!({})),
        Err(_) => json!({}),
    }
}

/// `body.<name>` — and the ONE body that makes it throw. `null.username` is a TypeError; every other JSON value
/// (a number, a string, an array) answers `undefined` and is handled downstream by the truthiness tests.
fn body_field<'a>(body: &'a Value, name: &str) -> Result<Option<&'a Value>, crate::RouteFailure> {
    if body.is_null() {
        return Err(crate::RouteFailure::Threw);
    }
    Ok(body.get(name))
}

/// `cf-connecting-ip`, or the literal `"unknown"` — `request.headers?.get?.(…) || "unknown"`.
fn caller_ip(request: &Request) -> String {
    request
        .headers()
        .get("cf-connecting-ip")
        .ok()
        .flatten()
        .filter(|ip| !ip.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// `authRateLimited(request)`.
fn auth_rate_limited(request: &Request) -> bool {
    let ip = caller_ip(request);
    let now = now_ms();
    let mut counters = AUTH_RATE_COUNTERS
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    check(AUTH_RATE, &mut counters, &ip, now)
}

/// `const gate = (__loginGate.get(gk) || 0) + 1; __loginGate.set(gk, gate); … if (gate > 10)` — the counter is
/// bumped on EVERY attempt (unlike the rate limiter above, which does not consume a refused request).
fn bump_login_gate(key: &str) -> u32 {
    let mut counters = LOGIN_GATE.lock().unwrap_or_else(|e| e.into_inner());
    counter_bump(&mut counters, key)
}

/// `const fails = (__loginFails.get(fk) || 0) + 1`.
fn bump_login_fails(key: &str) -> u32 {
    let mut counters = LOGIN_FAILS.lock().unwrap_or_else(|e| e.into_inner());
    counter_bump(&mut counters, key)
}

/// `__loginFails.delete(fk)`.
fn clear_login_fails(key: &str) {
    let mut counters = LOGIN_FAILS.lock().unwrap_or_else(|e| e.into_inner());
    counters.retain(|(k, _)| k != key);
}

/// The two in-memory tables' shared arithmetic, including the `size > 4096` eviction of the OLDEST key.
fn counter_bump(counters: &mut Counters, key: &str) -> u32 {
    let next = match counters.iter_mut().find(|(k, _)| k == key) {
        Some(entry) => {
            entry.1 += 1;
            entry.1
        }
        None => {
            counters.push((key.to_string(), 1));
            1
        }
    };
    if counters.len() > 4096 {
        counters.remove(0);
    }
    next
}

/// `issueSessionSecret(env)` — the fail-closed issuance key.
fn session_secret_var(env: &Env) -> Option<String> {
    env.var("SESSION_SECRET")
        .ok()
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty())
}

/// `requireSession(request, env)` with the 401 arm every `/api/me` route spells identically.
async fn session_or_401(request: &Request, env: &Env) -> Result<Result<Value, Response>, crate::RouteFailure> {
    match session_gate::require_session(request, env).await {
        Ok(Some(user)) => Ok(Ok(user)),
        Ok(None) => Ok(Err(crate::json_error(
            401,
            "Not logged in or session expired",
            "authentication_error",
        )?)),
        Err(_) => Err(crate::RouteFailure::Threw),
    }
}

/// **`sessionAndKeyName(request, env, allowed)`** — the prologue `mePutKeys`, `meRevealKey`, `meTestKeys` and
/// `meKeyUsage` share: resolve the session, read the body, validate `name`. It answers the BODY as well,
/// because the request stream is consumed by the read.
async fn session_and_key_name(
    request: &mut Request,
    env: &Env,
    allowed: &[&str],
) -> Result<Result<(Value, String, Value), Response>, crate::RouteFailure> {
    let user = match session_or_401(request, env).await? {
        Ok(user) => user,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let body = read_json(request).await;
    let name = body.get("name");
    let allowed_hit = name
        .and_then(Value::as_str)
        .is_some_and(|n| allowed.contains(&n));
    if !allowed_hit {
        return Ok(Err(crate::json_error(
            400,
            &format!("Unknown key name: {}", js_to_string_of(name)),
            "invalid_request",
        )?));
    }
    Ok(Ok((
        user,
        name.and_then(Value::as_str).unwrap_or_default().to_string(),
        body,
    )))
}

/// `String(x)` for the two places a request value is interpolated into a message — with `undefined` for a
/// missing field, which is what the source's template literal produces.
fn js_to_string_of(v: Option<&Value>) -> String {
    match v {
        Some(value) => js_to_string(value),
        None => "undefined".to_string(),
    }
}

/// The user's id as the store addresses it: **`String(user.id)`, RAW** — the source passes `user.id` without the
/// `|| ""` its message sites use, so a record with no id addresses `ukeys:undefined` / `user:undefined`. A
/// `|| ""` here would be a different KV key, which is a different read and a different write.
fn user_id(user: &Value) -> String {
    js_to_string_of(user.get("id"))
}

/* ─────────────────────────── register / login ─────────────────────────── */

/// `authRegister` — the four shape refusals, the invite, the record, and the FIRST session cookie.
async fn auth_register(request: &mut Request, env: &Env, secure: bool) -> Handled {
    if auth_rate_limited(request) {
        return Ok(crate::json_error(
            429,
            "rate limit exceeded",
            "rate_limit_error",
        )?);
    }
    if user_store::admin_password(env).await.is_empty() {
        return Ok(crate::json_error(
            500,
            "Admin password not configured",
            "config_error",
        )?);
    }
    let body = read_json(request).await;
    // **THE SOURCE'S TRY BEGINS AT `body.username` AND THE CATCH ANSWERS `400 e.message`** — so every failure
    // below this line is a 400 with the message the store (or V8) produced.
    let created = match body_field(&body, "username") {
        Err(_) => {
            return Ok(crate::json_error(
                400,
                NULL_BODY_USERNAME,
                "invalid_request",
            )?)
        }
        Ok(username) => {
            let password = body_field(&body, "password").ok().flatten();
            let invite = body_field(&body, "inviteCode").ok().flatten();
            match user_store::create_user(env, username, password, invite, "user").await {
                Ok(created) => created,
                Err(message) => {
                    return Ok(crate::json_error(400, &message, "invalid_request")?)
                }
            }
        }
    };
    // Fail-closed issuance: without SESSION_SECRET there is no safe HMAC key (the admin-password fallback would
    // be offline-brute-forceable from any invited user's own cookie).
    let Some(secret) = crate::store::issue_session_secret(session_secret_var(env).as_deref()) else {
        return Ok(crate::json_error(
            500,
            "Session signing not configured",
            "config_error",
        )?);
    };
    let id = js_falsy_string(created.get("id"));
    let role = js_falsy_string(created.get("role"));
    let Some(token) = session_gate::issue_session_token(&secret, &id, &role, now_ms()).await else {
        return Err(crate::RouteFailure::Threw);
    };
    Ok(json_ok_with(
        json!({
            "ok": true,
            "username": created.get("username").cloned().unwrap_or(Value::Null),
            "role": created.get("role").cloned().unwrap_or(Value::Null),
            "token": created.get("token").cloned().unwrap_or(Value::Null),
        }),
        &[("Set-Cookie", session_cookie_header(&token, 86400, secure))],
    )?)
}

/// `authLogin` — the burst gate, the per-caller lock, the password check, and the failure counter.
async fn auth_login(request: &mut Request, env: &Env, secure: bool) -> Handled {
    if !user_store::has_admin_password(env).await {
        return Ok(crate::json_error(
            500,
            "Admin password not configured",
            "config_error",
        )?);
    }
    let body = read_json(request).await;
    let ip = caller_ip(request);
    // The per-minute burst gate, BEFORE the KV lock path: a brute-force loop otherwise burns 2-3 KV WRITES per
    // failed login.
    let gate_key = format!("login:{ip}:{}", now_ms().div_euclid(60_000));
    if bump_login_gate(&gate_key) > 10 {
        return Ok(crate::json_error(
            429,
            "Too many attempts — try again in a moment",
            "rate_limit_error",
        )?);
    }
    // **THIS IS THE `login` NULL-BODY ARM: OUTSIDE ANY TRY, SO THE FRONT DOOR'S CATCH ANSWERS 500.** The lock key
    // is trimmed and lowercased because `findUserByUsername` trims — an untrimmed key let `"admin "` variants
    // bypass the lock while still reaching the real password check.
    let username = body_field(&body, "username").map_err(|_| crate::RouteFailure::Threw)?;
    // `String(body.username || "")` — a MISSING username is the empty string, not an early return: the source
    // reads `login-lock:<ip>:` and asks `getUser("")` before it answers its 401, and an early return here would
    // skip a lock the shipping worker honours.
    let lock_key = format!(
        "login-lock:{ip}:{}",
        js_falsy_string(username).trim().to_lowercase()
    );
    if kv_text(env, &lock_key).await.is_some() {
        return Ok(crate::json_error(
            429,
            "Too many attempts — try again in ~30s",
            "rate_limit_error",
        )?);
    }
    let password = js_falsy_string(body.get("password"));
    let Some(user) = user_store::find_user_by_username(env, username).await else {
        // Auth-core audit LOW: burn the SAME PBKDF2 work as a real attempt — the instant return was a
        // username-enumeration timing oracle.
        user_store::verify_password(&password, &"0".repeat(32), &"0".repeat(32)).await;
        return Ok(crate::json_error(
            401,
            "Incorrect username or password",
            "authentication_error",
        )?);
    };
    if user.get("enabled").is_none_or(js_falsy) {
        user_store::verify_password(&password, &"0".repeat(32), &"0".repeat(32)).await;
        return Ok(crate::json_error(
            401,
            "Incorrect username or password",
            "authentication_error",
        )?);
    }
    let ok = if user.get("id").and_then(Value::as_str) == Some(user_store::ADMIN_ID) {
        // The admin account logs in with the ADMIN password (stored HASHED — compare, never read the plaintext).
        user_store::verify_admin_password(env, &password).await
    } else {
        let salt = js_falsy_string(user.get("salt"));
        let hash = js_falsy_string(user.get("passwordHash"));
        !salt.is_empty()
            && !hash.is_empty()
            && user_store::verify_password(&password, &salt, &hash).await
    };
    if !ok {
        // Only ARMING the 30 s lock touches KV: the counter itself is in memory, because the old KV counter
        // burned one WRITE per failed attempt — the same quota-exhaustion path the gates above exist for.
        let fails_key = format!("login-fails:{ip}:{}", user_id(&user));
        if bump_login_fails(&fails_key) >= 5 {
            kv_put(env, &lock_key, "1", Some(60)).await;
            clear_login_fails(&fails_key);
        }
        return Ok(crate::json_error(
            401,
            "Incorrect username or password",
            "authentication_error",
        )?);
    }
    clear_login_fails(&format!("login-fails:{ip}:{}", user_id(&user)));
    let Some(secret) = crate::store::issue_session_secret(session_secret_var(env).as_deref()) else {
        return Ok(crate::json_error(
            500,
            "Session signing not configured",
            "config_error",
        )?);
    };
    let Some(token) = session_gate::issue_session_token(
        &secret,
        &user_id(&user),
        &js_falsy_string(user.get("role")),
        now_ms(),
    )
    .await
    else {
        return Err(crate::RouteFailure::Threw);
    };
    Ok(json_ok_with(
        json!({
            "ok": true,
            "username": user.get("username").cloned().unwrap_or(Value::Null),
            "role": user.get("role").cloned().unwrap_or(Value::Null),
        }),
        &[("Set-Cookie", session_cookie_header(&token, 86400, secure))],
    )?)
}

/// `authResetPassword` — recovery by possession of the admin GATEWAY token, because the password is stored
/// hashed and there is nothing to look up.
async fn auth_reset_password(request: &mut Request, env: &Env) -> Handled {
    if auth_rate_limited(request) {
        return Ok(crate::json_error(
            429,
            "rate limit exceeded",
            "rate_limit_error",
        )?);
    }
    let Some(admin) = user_store::get_user(env, user_store::ADMIN_ID).await else {
        return Ok(crate::json_error(500, "Admin not seeded", "config_error")?);
    };
    let body = read_json(request).await;
    // `body?.adminKey` — the optional chaining is the source's, so a `null` body is `undefined` here and NOT a
    // throw: this route answers a clean 400 where `login` answers 500.
    let admin_key = js_falsy_string(body.get("adminKey")).trim().to_string();
    let new_password = js_falsy_string(body.get("newPassword"));
    if admin_key.is_empty() || new_password.is_empty() {
        return Ok(crate::json_error(
            400,
            "adminKey and newPassword are required",
            "invalid_request",
        )?);
    }
    if new_password.encode_utf16().count() < 8 {
        return Ok(crate::json_error(
            400,
            "New password must be at least 8 chars",
            "invalid_request",
        )?);
    }
    let token = js_falsy_string(admin.get("token"));
    if token.is_empty() || !safe_eq(&admin_key, &token) {
        return Ok(crate::json_error(
            403,
            "Invalid admin key",
            "authentication_error",
        )?);
    }
    if user_store::set_admin_password(env, &new_password).await.is_err() {
        return Err(crate::RouteFailure::Threw);
    }
    Ok(json_ok(json!({"ok": true}))?)
}

/// `authLogout` — the server-side blacklist, and the cookie cleared on EVERY arm including the 429.
async fn auth_logout(request: &mut Request, env: &Env, secure: bool) -> Handled {
    if auth_rate_limited(request) {
        // round-111: a 429 must STILL clear the client cookie — skipping the whole handler left the browser
        // cookie alive, which is the endpoint's entire purpose.
        return Ok(json_error_with(
            429,
            "rate limit exceeded",
            "rate_limit_error",
            &[("set-cookie", clear_session_cookie_header(secure))],
        )?);
    }
    let header = request
        .headers()
        .get("Cookie")
        .ok()
        .flatten()
        .unwrap_or_default();
    let cookie = parse_cookie(&header)
        .into_iter()
        .find(|(name, _)| name == SESSION_COOKIE)
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty());
    if let Some(cookie) = cookie {
        if env.kv("KEYS").is_ok() {
            // The source's try/catch: a malformed cookie is ignored, and so is a KV put the runtime refuses.
            // The payload is segment [0] — segment [1] is the binary HMAC signature, and parsing it as JSON
            // always throws, which silently disabled this whole blacklist once.
            let payload = cookie
                .split('.')
                .next()
                .and_then(session_gate::b64url_decode)
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .and_then(|text| serde_json::from_str::<Value>(&text).ok());
            if let Some(exp) = payload.as_ref().and_then(|p| p.get("exp")) {
                if !js_falsy(exp) {
                    // exp is MILLISECONDS (`issueSessionToken` uses `Date.now() + SESSION_TTL_MS`), the floor is
                    // KV's own 60 s minimum, and the ceiling is the 24 h maximum session life.
                    if let Some(ttl) = logout_ttl(exp, now_ms()) {
                        kv_put(env, &format!("sess-revoked:{cookie}"), "1", Some(ttl)).await;
                    }
                }
            }
        }
    }
    Ok(json_ok_with(
        json!({"ok": true}),
        &[("Set-Cookie", clear_session_cookie_header(secure))],
    )?)
}

/// `Math.max(60, Math.min(86400, Math.ceil((exp - now) / 1000)))`, over a value whose type the source never
/// checks: a truthy non-number coerces through the subtraction, and a value that coerces to `NaN` produces a TTL
/// the runtime rejects — the put throws INSIDE the source's try, so the request still answers 200 and nothing is
/// written. `None` is that arm.
fn logout_ttl(exp: &Value, now: i64) -> Option<u64> {
    let exp_ms = match exp {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        // `true - now` is 1 - now in JavaScript; `false` is falsy and never reaches the arithmetic (the caller
        // tests truthiness first), so its TTL is the KV-rejected one.
        Value::Bool(true) => 1.0,
        _ => return None,
    };
    if !exp_ms.is_finite() {
        return None;
    }
    let ttl = ((exp_ms - now as f64) / 1000.0).ceil();
    if !ttl.is_finite() {
        return None;
    }
    Some(ttl.clamp(60.0, 86400.0) as u64)
}

/* ─────────────────────────── /api/me ─────────────────────────── */

/// `meGet` — the account, its masked key status, and whether a relay token exists.
async fn me_get(request: &Request, env: &Env) -> Handled {
    let user = match session_or_401(request, env).await? {
        Ok(user) => user,
        Err(refusal) => return Ok(refusal),
    };
    let ukeys = user_store::get_user_keys(env, &user_id(&user)).await;
    // `userKeysStatus` throws on a truthy non-string key value (`maskKey` has no `.slice`), and that throw is a
    // 500 — not one bad row.
    let Some(keys) = user_store::user_keys_status(&ukeys, env) else {
        return Err(crate::RouteFailure::Threw);
    };
    // **THE RESPONSE IS THE RECORD'S OWN FIELDS, AND A FIELD THE RECORD LACKS IS A KEY THE RESPONSE LACKS** —
    // `JSON.stringify` drops `undefined`, so a hand-built object would answer `"token": null` where the source
    // answers nothing at all.
    let mut out = Map::new();
    for field in ["id", "username", "role", "enabled", "token"] {
        if let Some(value) = user.get(field) {
            out.insert(field.to_string(), value.clone());
        }
    }
    out.insert(
        "relayTokenSet".to_string(),
        Value::Bool(user.get("relayToken").is_some_and(|v| !js_falsy(v))),
    );
    out.insert("keys".to_string(), keys);
    Ok(json_ok(Value::Object(out))?)
}

/// `meRegenerateToken`.
async fn me_regenerate_token(request: &Request, env: &Env) -> Handled {
    let user = match session_or_401(request, env).await? {
        Ok(user) => user,
        Err(refusal) => return Ok(refusal),
    };
    match user_store::regenerate_token(env, &user_id(&user)).await {
        Ok(token) => Ok(json_ok(json!({"ok": true, "token": token}))?),
        // `regenerateToken` throws "User not found" when the record vanished between the gate and the write —
        // uncaught, so the front door's catch answers 500.
        Err(_) => Err(crate::RouteFailure::Threw),
    }
}

/// `meRotateRelayToken` — the F3 scoped credential, returned in the clear exactly once.
async fn me_rotate_relay_token(request: &Request, env: &Env) -> Handled {
    let user = match session_or_401(request, env).await? {
        Ok(user) => user,
        Err(refusal) => return Ok(refusal),
    };
    match user_store::rotate_relay_token(env, &user_id(&user)).await {
        Ok(token) => Ok(json_ok(json!({"ok": true, "token": token}))?),
        Err(_) => Err(crate::RouteFailure::Threw),
    }
}

/// `meRevokeRelayToken` — `revoked` is `true` only when one existed.
async fn me_revoke_relay_token(request: &Request, env: &Env) -> Handled {
    let user = match session_or_401(request, env).await? {
        Ok(user) => user,
        Err(refusal) => return Ok(refusal),
    };
    match user_store::revoke_relay_token(env, &user_id(&user)).await {
        Ok(revoked) => Ok(json_ok(json!({"ok": true, "revoked": revoked}))?),
        Err(_) => Err(crate::RouteFailure::Threw),
    }
}

/// `meRevealRelayToken` — a session holder can already rotate it, so reading it adds no privilege.
async fn me_reveal_relay_token(request: &Request, env: &Env) -> Handled {
    let user = match session_or_401(request, env).await? {
        Ok(user) => user,
        Err(refusal) => return Ok(refusal),
    };
    let token = match user.get("relayToken") {
        Some(value) if !js_falsy(value) => value.clone(),
        _ => {
            return Ok(crate::json_error(
                404,
                "Relay token not configured",
                "not_found_error",
            )?)
        }
    };
    Ok(json_ok(json!({"ok": true, "value": token}))?)
}

/// `meGetUsproxy` — the global switch the request path reads on every call.
async fn me_get_usproxy(request: &Request, env: &Env) -> Handled {
    // The session is resolved and NOT otherwise used: the switch is global, and the gate is the whole point of
    // the route on the read side.
    if let Err(refusal) = session_or_401(request, env).await? {
        return Ok(refusal);
    }
    let enabled = user_store::global_setting(env, "US_PROXY").await.is_some();
    Ok(json_ok(json!({"enabled": enabled}))?)
}

/// `mePutUsproxy` — ADMIN ONLY, and its refusal is `403 "Admin only"` / `"forbidden"`: a different message AND a
/// different type from `requireAdmin`'s, because the handler writes its own.
async fn me_put_usproxy(request: &mut Request, env: &Env) -> Handled {
    let user = match session_or_401(request, env).await? {
        Ok(user) => user,
        Err(refusal) => return Ok(refusal),
    };
    if user.get("role").and_then(Value::as_str) != Some("admin") {
        return Ok(crate::json_error(403, "Admin only", "forbidden")?);
    }
    let body = read_json(request).await;
    // An explicit OFF is persisted as "0" so it shadows an env/US_PROXY Worker var — deleting let the env
    // fallback bounce the toggle straight back ON.
    let enabled = body.get("enabled").is_some_and(|v| !js_falsy(v));
    user_store::set_global_setting(env, "US_PROXY", Some(&Value::String(
        if enabled { "1" } else { "0" }.to_string(),
    )))
    .await;
    let value = user_store::global_setting(env, "US_PROXY").await;
    Ok(json_ok(json!({
        "ok": true,
        "enabled": crate::store::global_setting_enabled(value.as_deref()),
    }))?)
}

/* ─────────────────────────── the caller's own keys ─────────────────────────── */

/// `mePutKeys` — a save, answered with the MASK rather than the value ("masks survive screenshots and DOM
/// scrapes").
async fn me_put_keys(request: &mut Request, env: &Env) -> Handled {
    let allowed = user_store::user_key_names();
    let (user, name, body) = match session_and_key_name(request, env, &allowed).await? {
        Ok(triple) => triple,
        Err(refusal) => return Ok(refusal),
    };
    let value = body.get("value");
    // `typeof value !== "string" || !value.trim()` — a NUMBER is refused even when it is truthy.
    let Some(text) = value.and_then(Value::as_str) else {
        return Ok(crate::json_error(
            400,
            "value must not be empty",
            "invalid_request",
        )?);
    };
    if text.trim().is_empty() {
        return Ok(crate::json_error(
            400,
            "value must not be empty",
            "invalid_request",
        )?);
    }
    let trimmed = text.trim().to_string();
    user_store::set_user_key(env, &user_id(&user), &name, &trimmed).await;
    Ok(json_ok(json!({
        "ok": true,
        "name": name,
        "masked": crate::device_registry::mask_key(&trimmed),
    }))?)
}

/// `meDeleteKeys` — the name is a QUERY PARAMETER here, not a body field, and the refusal is the same 400.
async fn me_delete_keys(request: &Request, env: &Env) -> Handled {
    let user = match session_or_401(request, env).await? {
        Ok(user) => user,
        Err(refusal) => return Ok(refusal),
    };
    let mut name = String::new();
    if let Ok(url) = request.url() {
        for (key, value) in url.query_pairs() {
            if key.as_ref() == "name" {
                name = value.to_string();
                break;
            }
        }
    }
    if !user_store::user_key_names().contains(&name.as_str()) {
        return Ok(crate::json_error(
            400,
            &format!("Unknown key name: {name}"),
            "invalid_request",
        )?);
    }
    user_store::delete_user_key(env, &user_id(&user), &name).await;
    Ok(json_ok(json!({"ok": true, "name": name}))?)
}

/// `meRevealKey` — the explicit-intent counterpart to the masked list: one key per click, the caller's OWN.
async fn me_reveal_key(request: &mut Request, env: &Env) -> Handled {
    let allowed = user_store::user_key_names();
    let (user, name, _body) = match session_and_key_name(request, env, &allowed).await? {
        Ok(triple) => triple,
        Err(refusal) => return Ok(refusal),
    };
    let ukeys = user_store::get_user_keys(env, &user_id(&user)).await;
    let key = match ukeys.get(&name) {
        Some(value) if !js_falsy(value) => value.clone(),
        _ => return Ok(crate::json_error(404, "Key not configured", "not_found_error")?),
    };
    Ok(json_ok(json!({"ok": true, "name": name, "value": key}))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE FAMILY PREDICATE, WHICH IS THE CUTOVER'S HALF AND IS MIRRORED IN `index.ts`.** A path that is
    /// handed over must be a path this worker answers — including the ones it answers with the front door's 404.
    ///
    /// MUTATION: drop the three exclusions (make `/api/me/` a bare prefix).
    /// RESULT:   `the_family_is_the_fourteen_routes_plus_the_404s` fails on `/api/me/route`, and the console's
    ///           model-route selection would be answered by an implementation that does not have the catalogue.
    #[test]
    fn the_family_is_the_fourteen_routes_plus_the_404s() {
        let get = Method::Get;
        for path in [
            "/api/auth/register",
            "/api/auth/login",
            "/api/auth/reset-password",
            "/api/auth/logout",
            "/api/auth",
            "/api/auth/anything/else",
            "/api/me",
            "/api/me/token/regenerate",
            "/api/me/token/relay",
            "/api/me/token/relay/reveal",
            "/api/me/usproxy",
            "/api/me/keys",
            "/api/me/keys/reveal",
            "/api/me/anything/else",
        ] {
            assert!(in_family(&get, path), "{path} belongs to this worker");
        }
        for path in [
            "/api/me/route",
            "/api/me/keys/test",
            "/api/me/keys/usage",
            "/api/meX",
            "/api/authentication",
            "/api/health",
            "/v1/messages",
        ] {
            assert!(!in_family(&get, path), "{path} stays where it is");
        }
    }

    /// The logout TTL, over the four value shapes the source's arithmetic accepts and the two it turns into a
    /// rejected KV write. **THE FLOOR AND THE CEILING ARE BOTH REAL**: the 60 s floor is KV's minimum
    /// `expirationTtl` (a smaller one made the blacklist write fail silently), and the ceiling is the maximum
    /// session life.
    ///
    /// MUTATION: `exp_ms - now` → `exp_ms` (treating `exp` as seconds).
    /// RESULT:   this test fails on the first case — every logout would write a ~2-day entry (the source's own
    ///           round-122 bug, where "the old *1000 treated it as seconds, overstating the remaining life
    ///           ~1000x").
    #[test]
    fn the_logout_ttl_is_the_sources() {
        let now = 1_760_000_000_000;
        assert_eq!(logout_ttl(&json!(now + 7_200_000), now), Some(7200));
        assert_eq!(logout_ttl(&json!(now - 1), now), Some(60), "the KV floor");
        assert_eq!(logout_ttl(&json!(0), now), Some(60));
        assert_eq!(
            logout_ttl(&json!(now + 10 * 86_400_000), now),
            Some(86400),
            "the ceiling"
        );
        assert_eq!(logout_ttl(&json!("1760007200000"), now), Some(7200));
        assert_eq!(logout_ttl(&json!("nonsense"), now), None, "NaN: no write");
        assert_eq!(logout_ttl(&json!({"a": 1}), now), None, "NaN: no write");
    }

    /// The two in-memory login tables: the value is bumped on every attempt and the oldest key is evicted at the
    /// bound (the source's `if (__loginGate.size > 4096) delete the first key`).
    #[test]
    fn the_login_tables_count_and_evict() {
        let mut counts: Counters = Vec::new();
        assert_eq!(counter_bump(&mut counts, "login:1.2.3.4:0"), 1);
        assert_eq!(counter_bump(&mut counts, "login:1.2.3.4:0"), 2);
        assert_eq!(counter_bump(&mut counts, "login:5.6.7.8:0"), 1);
        assert_eq!(counts.len(), 2);
        // A bounded table, exactly: the 4097th distinct key evicts the OLDEST, which is the first entry of the
        // insertion-ordered table (`counters.keys().next().value`).
        for i in 0..4094 {
            counter_bump(&mut counts, &format!("k{i}"));
        }
        assert_eq!(counts.len(), 4096);
        assert_eq!(counts[0].0, "login:1.2.3.4:0");
        counter_bump(&mut counts, "fresh");
        assert_eq!(counts.len(), 4096);
        assert_eq!(counts[0].0, "login:5.6.7.8:0", "the oldest went");
        assert_eq!(counts[4095].0, "fresh", "and the newest stayed");
    }

    /// `body_field`: only the literal `null` throws — a number, a string and an array all answer `undefined` for
    /// a property read, which is what makes `login`'s arm a 500 and `reset-password`'s a clean 400.
    #[test]
    fn the_null_body_is_the_only_throwing_one() {
        assert!(body_field(&Value::Null, "username").is_err());
        for body in [json!(5), json!("x"), json!([1, 2]), json!({})] {
            assert!(body_field(&body, "username").unwrap().is_none());
        }
        assert_eq!(
            body_field(&json!({"username": "bob"}), "username").unwrap(),
            Some(&json!("bob"))
        );
    }
}
