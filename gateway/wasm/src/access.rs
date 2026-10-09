//! THE CLOUDFLARE ACCESS ARM OF `requireSession` — MOVED FROM `gateway/src/access.ts`.
//!
//! WHY THIS IS ITS OWN MODULE RATHER THAN A BRANCH IN `session_gate.rs`. It is a SECOND IDENTITY PROVIDER: the
//! cookie arm answers "who signed this token", and this one answers "which console user does the edge's verified
//! email belong to". That is three jobs the cookie arm does not have — an RS256 signature check against the
//! team's published certs, the claims (`aud`/`iss`/`exp`), and a PROVISIONING WRITE (a first-time email mints a
//! passwordless user and binds it). The consequence of getting it wrong is not a 401: it is a console account
//! for whoever can make `aud` and `iss` line up.
//!
//! **THE ORDER OF THE CHECKS IS THE SECURITY PROPERTY, AND IT IS THE SOURCE'S:** the token's presence and the
//! two configuration vars first (an unconfigured deployment is a no-op, not an error), then the three-part
//! shape, then the base64/JSON decode, then `kid`, then `aud`, then `iss`, then `exp` — and only THEN the JWKS
//! fetch and the signature. A token that fails any earlier check never causes a network call, and a token whose
//! signature is checked is a token whose claims already matched.
//!
//! **TWO ARMS THROW, AND THE PORT REPRODUCES BOTH RATHER THAN IMPROVING THEM**, because each is a response a
//! caller sees:
//!
//!   * `crypto.subtle.importKey("jwk", …)` is called OUTSIDE `verifyAccessJwt`'s try/catch, so a JWKS entry
//!     with `alg: "RS256"` whose JWK the runtime cannot read rejects that promise and the handler throws — the
//!     front door's catch answers 500 `Internal error`, on a request that carried no cookie at all.
//!   * `ensureUserByEmail`'s uniqueness loop ends in a bare `throw new Error("could not allocate a unique
//!     username — try again")` after ten collisions.
//!
//! `Err(RouteFailure::Threw)` below is exactly that: the source threw, and the route answers 500. `Ok(None)` is
//! the source's `null` — a refusal, and a 401.
//!
//! **THE JWKS SOURCE IS THE ONE THING THE CORPUS CANNOT DRIVE THROUGH A LIVE NETWORK, AND IT DOES NOT HAVE TO**:
//! `fetchJwks` reads `env.ACCESS_JWKS_JSON` FIRST, so a harness can hand both implementations the same published
//! keys as a value — and the fetch is exercised by the one case that leaves it unset and answers through the
//! `fetch` stub both sides already share.

use serde_json::Value;
use worker::*;

use crate::device_registry::{js_falsy_string, js_to_string};
use crate::device_store;
use crate::key_lock::{with_key_lock, KeyedLocks};
use crate::session_gate::b64url_decode;
use crate::RouteFailure;

/// `CERTS_TTL_MS` — the fetched JWKS is cached for an hour, per isolate, exactly like the source's `jwksCache`.
const CERTS_TTL_MS: i64 = 60 * 60 * 1000;

/// `jwksCache` — module state, the source's `{ keys, exp }`, and it is only ever written on a SUCCESSFUL fetch.
static JWKS_CACHE: std::sync::Mutex<Option<(Vec<Value>, i64)>> = std::sync::Mutex::new(None);

/// `withKeyLock("access-provision:" + email, …)` — the provisioning section's lock. The email is a request
/// value, so this is the KEYED table rather than a static.
static PROVISION_LOCKS: KeyedLocks = KeyedLocks::new();

/// `env[name]` as a string, with the empty string answering `None` — JavaScript truthiness, which is what every
/// `if (env.X)` in the source is.
fn env_str(env: &Env, name: &str) -> Option<String> {
    env.var(name)
        .ok()
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty())
}

/// `b64urlToJson(part)` — `atob` after the `-`/`_` translation and the padding restoration, then `JSON.parse`.
/// `None` is the source's `catch`, which covers BOTH an invalid base64 length/alphabet and a payload that does
/// not parse.
pub fn b64url_to_json(part: &str) -> Option<Value> {
    let bytes = b64url_decode(part)?;
    let text = String::from_utf8(bytes).ok()?;
    serde_json::from_str(&text).ok()
}

/// `token.split(".")` with the source's `parts.length !== 3` refusal — THREE parts exactly, and `parts[2]` is
/// the signature. (This is `access.ts`'s split and NOT `auth.ts`'s first-dot split: a session token has one dot,
/// an Access JWT has two.)
pub fn split_jwt(token: &str) -> Option<(&str, &str, &str)> {
    let mut parts = token.split('.');
    let header = parts.next()?;
    let payload = parts.next()?;
    let signature = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some((header, payload, signature))
}

/// `Array.isArray(payload.aud) ? payload.aud : typeof payload.aud === "string" ? [payload.aud] : []` — the two
/// shapes Cloudflare emits (RFC 7519 §4.1.3). A non-string entry is carried as the empty string, which is what
/// `includes` does with it: it can only ever match an empty `ACCESS_AUD`, and an empty `ACCESS_AUD` is refused
/// one line earlier by the truthiness check.
pub fn aud_list(payload: &Value) -> Vec<String> {
    match payload.get("aud") {
        Some(Value::Array(items)) => items.iter().map(js_to_string).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// **THE FOUR CLAIMS CHECKS, IN THE SOURCE'S ORDER, EACH ONE A REFUSAL THAT NEVER REACHES THE NETWORK.**
///
/// `header` and `payload` are the decoded JWT halves, `aud` and `team_domain` are the two configuration values
/// (already known to be non-empty — the caller checks that), and `now_ms` is `Date.now()`.
///
/// MUTATION: drop the `iss` check.
/// RESULT:   `the_issuer_is_pinned` fails — a token minted by ANOTHER Cloudflare team under the same `aud`
///           verifies, which is the audit finding ("aud + team JWKS sig mostly covers it, but a token signed by
///           ANOTHER CF team's app under the same aud was possible") this check was added for.
pub fn claims_ok(header: &Value, payload: &Value, aud: &str, team_domain: &str, now_ms: i64) -> bool {
    // `if (!header.kid) return null` — the JWKS lookup is by kid, so a token without one can never be verified.
    if !matches!(header.get("kid"), Some(v) if v.is_string() && !v.as_str().unwrap_or("").is_empty()) {
        return false;
    }
    if !aud_list(payload).iter().any(|a| a == aud) {
        return false;
    }
    if payload.get("iss").and_then(Value::as_str) != Some(&format!("https://{team_domain}")) {
        return false;
    }
    // `typeof payload.exp !== "number" || payload.exp * 1000 < Date.now()` — a string expiry is refused rather
    // than coerced, and the comparison is strict (`exp * 1000 === now` is still valid).
    matches!(
        payload.get("exp").and_then(Value::as_f64),
        Some(exp) if exp * 1000.0 >= now_ms as f64
    )
}

/// `String(payload.email || "").toLowerCase().trim()`, then the `@` test. `None` where the source answers null.
pub fn email_of(payload: &Value) -> Option<String> {
    let email = js_falsy_string(payload.get("email"))
        .to_lowercase()
        .trim()
        .to_string();
    email.contains('@').then_some(email)
}

/// `usernameFromEmail(email)`: the local part, everything outside `[A-Za-z0-9_.-]` dropped, 28 characters at
/// most — and `"user"` when what is left is not a 2-to-32 character name.
///
/// THE ORDER MATTERS AND IS THE SOURCE'S: the strip happens BEFORE the slice (so 28 characters of the STRIPPED
/// local part) and the test happens AFTER it (so a name that is legal before stripping and illegal after becomes
/// `"user"` rather than being kept).
pub fn username_from_email(email: &str) -> String {
    let local = email.split('@').next().unwrap_or("");
    let stripped: String = local
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        .collect();
    let base: String = stripped.chars().take(28).collect();
    if is_valid_username(&base) {
        base
    } else {
        "user".to_string()
    }
}

/// `/^[A-Za-z0-9_.-]{2,32}$/` — the same anchored shape `createUser` enforces.
///
/// THE LENGTH IS THE UTF-16 LENGTH, which is what `{2,32}` counts: a name with an astral character in it fails
/// the character class anyway, but the bound is written as the source writes it rather than as bytes.
pub fn is_valid_username(name: &str) -> bool {
    (2..=32).contains(&name.encode_utf16().count())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// `fetchJwks(env)`: `ACCESS_JWKS_JSON` first, then the hour-long cache, then the team's published certs.
///
/// `None` is the source's throw — the caller (`verify_access_jwt`) has this call inside its try/catch, so EVERY
/// failure here is the same refusal rather than a different one: a non-2xx certs answer, a body that does not
/// parse, and a `keys` that is not an array all answer "no keys", and the arm declines the request.
async fn fetch_jwks(env: &Env) -> Option<Vec<Value>> {
    if let Some(configured) = env_str(env, "ACCESS_JWKS_JSON") {
        let parsed: Value = serde_json::from_str(&configured).ok()?;
        return parsed.get("keys")?.as_array().cloned();
    }
    let now = device_store::now_ms();
    if let Ok(cache) = JWKS_CACHE.lock() {
        if let Some((keys, exp)) = cache.as_ref() {
            if now < *exp {
                return Some(keys.clone());
            }
        }
    }
    let team_domain = env_str(env, "ACCESS_TEAM_DOMAIN")?;
    let url = format!("https://{team_domain}/cdn-cgi/access/certs")
        .parse::<Url>()
        .ok()?;
    let mut response = Fetch::Url(url).send().await.ok()?;
    if response.status_code() >= 400 {
        return None;
    }
    let body: Value = response.json().await.ok()?;
    let keys = body.get("keys")?.as_array()?.clone();
    if let Ok(mut cache) = JWKS_CACHE.lock() {
        *cache = Some((keys.clone(), now + CERTS_TTL_MS));
    }
    Some(keys)
}

/// `verifyAccessJwt(request, env)` — the verified email, `Ok(None)` for the source's `null`, and `Err` for the
/// one arm that throws (the key import).
pub async fn verify_access_jwt(request: &Request, env: &Env) -> Result<Option<String>, RouteFailure> {
    let token = request
        .headers()
        .get("cf-access-jwt-assertion")
        .ok()
        .flatten()
        .unwrap_or_default();
    let (Some(aud), Some(team_domain)) = (
        env_str(env, "ACCESS_AUD"),
        env_str(env, "ACCESS_TEAM_DOMAIN"),
    ) else {
        return Ok(None);
    };
    if token.is_empty() {
        return Ok(None);
    }
    let Some((raw_header, raw_payload, raw_signature)) = split_jwt(&token) else {
        return Ok(None);
    };
    let (Some(header), Some(payload)) = (b64url_to_json(raw_header), b64url_to_json(raw_payload)) else {
        return Ok(None);
    };
    if !claims_ok(&header, &payload, &aud, &team_domain, device_store::now_ms()) {
        return Ok(None);
    }
    let kid = header.get("kid").and_then(Value::as_str).unwrap_or_default();
    let Some(keys) = fetch_jwks(env).await else {
        return Ok(None);
    };
    let Some(key_jwk) = keys
        .iter()
        .find(|k| k.get("kid").and_then(Value::as_str) == Some(kid))
    else {
        return Ok(None);
    };
    if key_jwk.get("alg").and_then(Value::as_str) != Some("RS256") {
        return Ok(None);
    }
    // **THE IMPORT IS OUTSIDE THE TRY, SO A BAD KEY IS A THROW RATHER THAN A REFUSAL** — see the module header.
    let Some(key) = crate::webcrypto::import_rs256_jwk(&key_jwk.to_string()).await else {
        return Err(RouteFailure::Threw);
    };
    // ...and from here the source is inside its try again: a signature that `atob` cannot decode and a
    // `subtle.verify` that rejects are both "invalid token", not an error.
    let Some(signature) = b64url_decode(raw_signature) else {
        return Ok(None);
    };
    let data = format!("{raw_header}.{raw_payload}");
    match crate::webcrypto::verify_rs256(&key, &signature, data.as_bytes()).await {
        Some(true) => Ok(email_of(&payload)),
        _ => Ok(None),
    }
}

/// `ensureUserByEmail(env, email)` — bind, reuse or provision. `Ok(None)` is the source's `null` (no KV, or a
/// SUSPENDED account), and `Err` is the uniqueness loop giving up.
pub async fn ensure_user_by_email(env: &Env, email: &str) -> Result<Option<Value>, RouteFailure> {
    if env.kv("KEYS").is_err() {
        return Ok(None);
    }
    let email = email.to_lowercase().trim().to_string();
    // Owner shortcut: the configured admin email drives the SEEDED admin account.
    let admin_email = env_str(env, "ACCESS_ADMIN_EMAIL")
        .unwrap_or_default()
        .to_lowercase();
    if !admin_email.is_empty() && email == admin_email {
        if let Some(raw) = device_store::kv_text(env, "user:admin").await {
            if let Ok(u) = serde_json::from_str::<Value>(&raw) {
                if u.get("enabled") != Some(&Value::Bool(false)) {
                    return Ok(Some(u));
                }
            }
        }
        return Ok(None);
    }

    if let Some(bound) = device_store::kv_text(env, &format!("access-email:{email}")).await {
        if let Some(raw) = device_store::kv_text(env, &format!("user:{bound}")).await {
            if let Ok(user) = serde_json::from_str::<Value>(&raw) {
                // **A DISABLED BOUND ACCOUNT STAYS LOGGED OUT** (the source's round-395): returning `None`
                // WITHOUT provisioning is the point — minting a fresh suffixed account for a suspended user
                // would defeat the suspension, and the old code's re-check returned the record enabled-blind.
                if user.get("enabled") == Some(&Value::Bool(false)) {
                    return Ok(None);
                }
                return Ok(Some(user));
            }
        }
    }

    with_key_lock(&PROVISION_LOCKS, &format!("access-provision:{email}"), async {
        if let Some(again) = device_store::kv_text(env, &format!("access-email:{email}")).await {
            if let Some(raw) = device_store::kv_text(env, &format!("user:{again}")).await {
                if let Ok(user) = serde_json::from_str::<Value>(&raw) {
                    if user.get("enabled") == Some(&Value::Bool(false)) {
                        return Ok(None);
                    }
                    return Ok(Some(user));
                }
            }
        }

        let base = username_from_email(&email);
        let mut name = base.clone();
        // The uniqueness loop: a SINGLE suffix attempt could still collide and overwrite the colliding account's
        // record (the old code suffixed once, then put unconditionally). Ten attempts, then give up loudly.
        if device_store::kv_text(env, &format!("user:{name}"))
            .await
            .is_some()
        {
            let mut unique = false;
            for _ in 0..10 {
                let candidate = format!("{base}-{}", crate::webcrypto::random_hex(2));
                if device_store::kv_text(env, &format!("user:{candidate}"))
                    .await
                    .is_none()
                {
                    name = candidate;
                    unique = true;
                    break;
                }
            }
            if !unique {
                return Err(RouteFailure::Threw);
            }
        }

        let token = crate::webcrypto::random_hex(24);
        // FIELD ORDER IS THE STORED BYTES: `id, username, role, enabled, createdAt, token, accessEmail`, which
        // is the order the source's object literal writes and therefore the order this record's JSON has.
        let user = serde_json::json!({
            "id": name,
            "username": name,
            "role": "user",
            "enabled": true,
            "createdAt": device_store::now_ms(),
            "token": token,
            "accessEmail": email,
        });
        device_store::kv_put(env, &format!("user:{name}"), &user.to_string(), None).await;
        device_store::kv_put(env, &format!("token:{token}"), &name, None).await;
        device_store::kv_put(env, &format!("access-email:{email}"), &name, None).await;
        Ok(Some(user))
    })
    .await
}

/// `requireAccessSession(request, env)` — the fallback `requireSession` reaches for when the cookie arm declines.
pub async fn require_access_session(request: &Request, env: &Env) -> Result<Option<Value>, RouteFailure> {
    if env_str(env, "ACCESS_AUD").is_none() || env_str(env, "ACCESS_TEAM_DOMAIN").is_none() {
        return Ok(None);
    }
    let Some(email) = verify_access_jwt(request, env).await? else {
        return Ok(None);
    };
    ensure_user_by_email(env, &email).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TEAM: &str = "team.cloudflareaccess.test";
    const AUD: &str = "aud-1234";

    fn good_header() -> Value {
        json!({"alg": "RS256", "kid": "kid-1", "typ": "JWT"})
    }
    fn good_payload() -> Value {
        json!({"aud": [AUD], "iss": format!("https://{TEAM}"), "exp": 2_000_000, "email": "A@B.test"})
    }

    /// **THE CLAIMS, ONE AT A TIME — AND EVERY REFUSAL IS A REFUSAL THAT NEVER DIALS.** The `aud` arm is the
    /// one with two legal shapes (a string and an array), and the `iss` arm is the audit's own L5 pin.
    ///
    /// MUTATION: accept `payload.aud` only as a string.
    /// RESULT:   `the_claims_are_the_sources` fails on the array case — which is the shape Cloudflare currently
    ///           emits, so EVERY Access login would be refused.
    #[test]
    fn the_claims_are_the_sources() {
        let now = 1_000_000_000;
        assert!(claims_ok(&good_header(), &good_payload(), AUD, TEAM, now));
        // aud as a bare string is the other legal shape.
        let mut string_aud = good_payload();
        string_aud["aud"] = json!(AUD);
        assert!(claims_ok(&good_header(), &string_aud, AUD, TEAM, now));
        // ...and a token for ANOTHER audience, another team, or a past expiry is refused.
        let mut wrong_aud = good_payload();
        wrong_aud["aud"] = json!(["someone-else"]);
        assert!(!claims_ok(&good_header(), &wrong_aud, AUD, TEAM, now));
        let mut wrong_iss = good_payload();
        wrong_iss["iss"] = json!("https://other.cloudflareaccess.test");
        assert!(!claims_ok(&good_header(), &wrong_iss, AUD, TEAM, now));
        let mut expired = good_payload();
        expired["exp"] = json!(999);
        assert!(!claims_ok(&good_header(), &expired, AUD, TEAM, now));
        // A string expiry is refused rather than coerced, and `exp > now` is still valid.
        let mut string_exp = good_payload();
        string_exp["exp"] = json!("2000000");
        assert!(!claims_ok(&good_header(), &string_exp, AUD, TEAM, now));
        let mut future = good_payload();
        future["exp"] = json!(now / 1000);
        assert!(claims_ok(&good_header(), &future, AUD, TEAM, now));
        // kid must be present AND a non-empty string.
        for header in [json!({}), json!({"kid": ""}), json!({"kid": 7})] {
            assert!(!claims_ok(&header, &good_payload(), AUD, TEAM, now));
        }
        // An aud of a non-string type cannot match a real audience.
        let mut numeric_aud = good_payload();
        numeric_aud["aud"] = json!([1]);
        assert!(!claims_ok(&good_header(), &numeric_aud, AUD, TEAM, now));
    }

    /// `emailOf`: `String(payload.email || "")`, lowercased and trimmed, and the `@` test.
    #[test]
    fn the_email_is_lowercased_and_must_look_like_one() {
        assert_eq!(email_of(&json!({"email": "  A@B.test "})).unwrap(), "a@b.test");
        assert_eq!(email_of(&json!({"email": "x@y"})).unwrap(), "x@y");
        assert_eq!(email_of(&json!({"email": "no-at-sign"})), None);
        assert_eq!(email_of(&json!({"email": ""})), None);
        assert_eq!(email_of(&json!({})), None);
        assert_eq!(email_of(&json!({"email": 0})), None);
    }

    /// **`usernameFromEmail`, AND THE ORDER OF THE STRIP AND THE SLICE IS THE WHOLE TEST.** 28 characters of the
    /// STRIPPED local part; a name that is illegal after stripping falls back to `"user"`.
    ///
    /// MUTATION: `take(32)` instead of `take(28)`.
    /// RESULT:   `the_username_is_derived_the_sources_way` fails on the long local part — and the longer name is
    ///           one `createUser` would also accept, so the collision loop's behaviour changes with it.
    #[test]
    fn the_username_is_derived_the_sources_way() {
        assert_eq!(username_from_email("alice@example.test"), "alice");
        assert_eq!(username_from_email("a+b/c@example.test"), "abc");
        assert_eq!(username_from_email("a".repeat(40).as_str()), "a".repeat(28));
        assert_eq!(username_from_email("x@example.test"), "user");
        assert_eq!(username_from_email("@@"), "user");
        assert_eq!(username_from_email("ab@x"), "ab");
        assert_eq!(username_from_email("a.b-c_d@x"), "a.b-c_d");
        // A leading dot survives the strip and the test (both allow `.`), which is why the test is not a
        // "starts with a letter" rule.
        assert_eq!(username_from_email(".ab@x"), ".ab");
        assert!(is_valid_username("ab"));
        assert!(!is_valid_username("a"));
        assert!(!is_valid_username("a b"));
    }

    /// The JWT split is `split(".")` with EXACTLY three parts — a session token's single dot is refused here.
    #[test]
    fn the_jwt_split_is_exactly_three_parts() {
        assert_eq!(split_jwt("a.b.c"), Some(("a", "b", "c")));
        assert_eq!(split_jwt("a.b"), None);
        assert_eq!(split_jwt("a.b.c.d"), None);
        assert_eq!(split_jwt(""), None);
    }

    /// `b64urlToJson` is the padding restoration plus `JSON.parse`, and it refuses what either of them refuses.
    #[test]
    fn the_jwt_half_decodes_or_refuses() {
        assert_eq!(b64url_to_json("eyJraWQiOiJrIn0"), Some(json!({"kid": "k"})));
        assert_eq!(b64url_to_json("!!!!"), None, "atob refuses the alphabet");
        assert_eq!(b64url_to_json("Zg"), None, "…and so does JSON.parse");
        assert_eq!(b64url_to_json("eyJhIjoxfQ"), Some(json!({"a": 1})));
    }
}
