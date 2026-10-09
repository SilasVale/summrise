//! THE ADMIN SESSION GATE, MOVED FROM `gateway/src/session.ts` AND `gateway/src/auth.ts`.
//!
//! WHY THE DEVICES FAMILY NEEDS IT: eleven of its fifteen routes start with `requireAdmin(request, env)`, and a
//! port that skipped the gate would serve the console's device registry to anyone who asked. The gate is three
//! decisions in a row, and each one has a way of being silently wrong: WHICH key signs a session (the dedicated
//! `SESSION_SECRET`, with the admin password accepted once for rotation), WHAT the presented cookie must hash to
//! (HMAC-SHA256 in constant time — a token that verifies is a session), and WHO the resolved user must be (the
//! record must still exist and still be enabled).
//!
//! **THE HMAC IS A PLATFORM CALL AND THE TOKEN'S MEANING IS NOT.** `verifySessionToken` computes the expected
//! signature with `crypto.subtle` and then compares, decodes and checks the expiry; the signature is the
//! runtime's primitive (`webcrypto.rs`), and everything that turns a valid signature into a user is here, pure
//! and pinned by vectors in the crate's tests.
//!
//! **BOTH ARMS ARE HERE NOW, AND THE SECOND ONE IS THE REASON THIS FILE'S FIRST VERSION HAD A CARVE-OUT.**
//! `requireSession` tries the cookie first and then falls back to `requireAccessSession` (`access.rs`), which
//! verifies a `Cf-Access-Jwt-Assertion` RS256 JWT against the team's published certs and provisions a user from
//! the verified email. Until this slice that arm was not ported, so the front door kept every cookie-less
//! admin-gated request on the TypeScript path — and the eleven device routes rode on that condition
//! (`index.ts`'s `devicesRouteNeedsSession`). With the arm in Rust the condition is gone, and the corpus proves
//! the whole of `requireSession`: the cookie arm, both key-rotation envelopes, the refusal list, and the Access
//! arm on a real RS256 key pair.
//!
//! **THE ORDER ACROSS THE TWO ARMS IS PART OF THE CONTRACT.** The cookie arm runs to COMPLETION — including its
//! `getUser` and its `enabled` check — before the Access arm is reached, and the Access arm is reached
//! UNCONDITIONALLY: `session.ts` calls `requireAccessSession` on every cookie-less request and it is that
//! function which answers null when the deployment has not opted in (`ACCESS_AUD`/`ACCESS_TEAM_DOMAIN` unset).
//! Hoisting the opt-in test into `require_session` as a short-circuit would be equivalent today and a second
//! place to change tomorrow, so the port keeps the source's shape.

use serde_json::Value;
use worker::*;

use crate::device_registry::{js_falsy, js_to_string_of, js_truthy_string};
use crate::device_store;
use crate::user_store;
use crate::RouteFailure;

// **THE RESOLVED USER IS THE FULL RECORD RATHER THAN AN ID, AND THAT IS THE SOURCE'S SHAPE.** The cookie arm
// answers what `getUser` read and the Access arm answers what `ensureUserByEmail` returned, so a handler reads
// `user.token`, `user.relayToken` and `user.role` off that value — `GET /api/me` answers with five of its
// fields, and a `{uid, role}` struct (which is what this file had while only the devices family used it) cannot
// express that without a second read on the `/api/me` path. `require_session` and `require_admin` below answer
// `serde_json::Value`s for exactly that reason.

/// `token.indexOf(".")` then `slice(0, dot)` / `slice(dot + 1)`.
pub fn split_token(token: &str) -> Option<(&str, &str)> {
    let dot = token
        .char_indices()
        .find(|(_, c)| *c == '.')
        .map(|(i, _)| i)?;
    Some((&token[..dot], &token[dot + 1..]))
}

/// `bytesToB64url` — standard base64 with `+`→`-` and `/`→`_`, and the `=` padding stripped.
///
/// **THE TWO SUBSTITUTIONS ARE NOT COSMETIC AND THE FIRST VERSION OF THIS FUNCTION OMITTED THEM** — it emitted
/// the standard alphabet, so every computed signature contained `+`/`/` where the token carried `-`/`_` and the
/// comparison was never equal: EVERY session would have 401'd. The test below is what caught it.
pub fn b64url_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

/// `atob` after the `-`/`_` translation and the padding restoration. Answers `None` where `atob` throws — an
/// invalid character or a length that cannot be a base64 group.
pub fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let translated: String = s
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    let pad = match translated.len() % 4 {
        0 => 0,
        n => 4 - n,
    };
    if pad > 2 {
        return None;
    }
    let mut padded = translated;
    padded.push_str(&"=".repeat(pad));
    base64_decode(&padded)
}

fn b64_value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as u32),
        b'a'..=b'z' => Some((c - b'a') as u32 + 26),
        b'0'..=b'9' => Some((c - b'0') as u32 + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// A strict-enough base64 decoder: the length must be a multiple of four, `=` only at the end (at most two), and
/// every other character must be in the alphabet — the three ways `atob` throws.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let pad = bytes.iter().rev().take_while(|c| **c == b'=').count();
    if pad > 2 {
        return None;
    }
    let body = &bytes[..bytes.len() - pad];
    let mut out = Vec::with_capacity(body.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for c in body {
        let v = b64_value(*c)?;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// `issueSessionToken(secret, uid, role)` — `b64url(payload).b64url(hmac)`, the token every login writes into
/// the cookie and every request verifies.
///
/// **THE PAYLOAD'S FIELD ORDER IS `uid, role, exp` AND IT IS THE WIRE FORMAT**: the token is the base64 of that
/// exact JSON, so two implementations that agree on the claims but not on the order produce tokens that are not
/// byte-equal and a corpus that cannot compare them.
///
/// `btoa` IN THE SOURCE IS LATIN-1, so a uid with a non-ASCII character would throw there and encode UTF-8 here.
/// Every value a ROUTE can hand this function is ASCII — a username this console validated as
/// `[A-Za-z0-9_.-]{2,32}`, or the Access arm's `usernameFromEmail`, which strips to the same alphabet.
pub fn session_payload(uid: &str, role: &str, now_ms: i64) -> String {
    let payload = serde_json::json!({
        "uid": uid,
        "role": role,
        "exp": now_ms + user_store::SESSION_TTL_MS,
    });
    b64url_encode(payload.to_string().as_bytes())
}

/// The issued token: the payload, a dot, and the platform's HMAC over that payload.
pub async fn issue_session_token(
    secret: &str,
    uid: &str,
    role: &str,
    now_ms: i64,
) -> Option<String> {
    let payload = session_payload(uid, role, now_ms);
    let signature = crate::webcrypto::hmac_sha256(secret, &payload).await?;
    Some(format!("{payload}.{}", b64url_encode(&signature)))
}

/// What a VERIFIED token says: the two claims `verifySessionToken` reads out of its payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenClaims {
    pub uid: String,
    pub role: String,
}

/// `verifySessionToken`'s last three steps: decode the payload, parse it, and read the claims — `exp` must be a
/// NUMBER and must not have passed, `uid` is `String(data.uid)`, and `role` is `String(data.role || "user")`.
pub fn session_claims(data: &Value, now_ms: i64) -> Option<TokenClaims> {
    let exp = data.get("exp")?;
    if !exp.is_number() {
        return None;
    }
    if exp.as_f64()? < now_ms as f64 {
        return None;
    }
    Some(TokenClaims {
        uid: js_to_string_of(data.get("uid")),
        role: js_truthy_string(data.get("role"), "user"),
    })
}

/// `verifySessionToken(secret, token)` — the signature first (computed, not decoded), then the payload.
pub async fn verify_session_token(secret: &str, token: &str, now_ms: i64) -> Option<TokenClaims> {
    if secret.is_empty() || token.is_empty() {
        return None;
    }
    let (payload, signature) = split_token(token)?;
    let expected = crate::webcrypto::hmac_sha256(secret, payload).await?;
    if !crate::auth::safe_eq(&b64url_encode(&expected), signature) {
        return None;
    }
    let decoded = b64url_decode(payload)?;
    let text = String::from_utf8(decoded).ok()?;
    let data: Value = serde_json::from_str(&text).ok()?;
    session_claims(&data, now_ms)
}

/// `requireCookieSession(request, env, store)` — the cookie arm, in the source's order.
///
/// **THE ORDER IS THE SECURITY PROPERTY**: no admin password means no sessions at all; the revocation blacklist
/// is consulted BEFORE the signature (a logged-out cookie dies even though it verifies); the user must exist and
/// be enabled AFTER it verifies. A port that checked the signature first would still be correct, but one that
/// checked the user before the signature would touch KV on every forged cookie.
async fn require_cookie_session(request: &Request, env: &Env) -> Option<Value> {
    let admin_password = user_store::admin_password(env).await;
    if admin_password.is_empty() {
        return None;
    }
    let header = request
        .headers()
        .get("Cookie")
        .ok()
        .flatten()
        .unwrap_or_default();
    let cookie = crate::auth::parse_cookie(&header)
        .into_iter()
        .find(|(name, _)| name == crate::auth::SESSION_COOKIE)
        .map(|(_, value)| value)?;
    if cookie.is_empty() {
        return None;
    }
    if device_store::session_revoked(env, &cookie).await {
        return None;
    }
    let configured = env
        .var("SESSION_SECRET")
        .ok()
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty());
    let primary = crate::store::session_secret(configured.as_deref(), &admin_password);
    let now = device_store::now_ms();
    let mut session = verify_session_token(&primary, &cookie, now).await;
    if session.is_none()
        && configured.is_some()
        && configured.as_deref() != Some(admin_password.as_str())
    {
        // Rotation compat: a cookie signed with the old admin-password key is accepted this once.
        session = verify_session_token(&admin_password, &cookie, now).await;
    }
    let resolved = session?;
    let user = user_store::get_user(env, &resolved.uid).await?;
    if user.get("enabled").is_none_or(js_falsy) {
        return None;
    }
    Some(user)
}

/// `requireSession(request, env)` — **THE COOKIE ARM, AND THEN THE ACCESS ARM.**
///
/// The two arms are tried in that order and the first answer wins, which is the source's contract and the reason
/// a password account keeps working on a deployment that has also switched Access on.
///
/// `Err` is the one arm that THROWS rather than refusing (`access.rs`'s key import and its uniqueness loop), and
/// it is an error rather than a `None` for exactly that reason: the shipping front door's catch answers 500
/// `Internal error` for it, and a port that answered 401 would be a different response on a real request.
pub async fn require_session(request: &Request, env: &Env) -> Result<Option<Value>, RouteFailure> {
    if let Some(user) = require_cookie_session(request, env).await {
        return Ok(Some(user));
    }
    // No valid session cookie — fall back to the edge-verified Cloudflare Access identity (option C). No-op
    // unless ACCESS_AUD/ACCESS_TEAM_DOMAIN are set, which `require_access_session` tests itself.
    crate::access::require_access_session(request, env).await
}

/// `requireAdmin(request, env)` — the resolved user, or the Response the handler returns INSTEAD.
///
/// The two refusals are the source's own bytes (`jsonError(401, "Not logged in or session expired",
/// "authentication_error")` and `jsonError(403, "Admin permission required", "authorization_error")`), and the
/// outer `Err` is `require_session`'s throw. `Result<Result<…>>` is deliberate: a caller must not be able to
/// conflate "refused" with "threw", because one is a 401/403 and the other is a 500.
pub async fn require_admin(
    request: &Request,
    env: &Env,
) -> Result<Result<Value, Response>, RouteFailure> {
    let Some(user) = require_session(request, env).await? else {
        return Ok(Err(crate::json_error(
            401,
            "Not logged in or session expired",
            "authentication_error",
        )?));
    };
    if user.get("role").and_then(Value::as_str) != Some("admin") {
        return Ok(Err(crate::json_error(
            403,
            "Admin permission required",
            "authorization_error",
        )?));
    }
    Ok(Ok(user))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_registry::js_to_string;
    use serde_json::json;

    /// **THE TOKEN SPLIT IS AT THE FIRST DOT AND THE SIGNATURE IS THE REST** — `"."` alone has an empty payload
    /// and an empty signature (which the empty-secret test then refuses), and a token with no dot is null.
    #[test]
    fn the_token_split_is_the_sources() {
        assert_eq!(split_token("a.b"), Some(("a", "b")));
        assert_eq!(split_token("a.b.c"), Some(("a", "b.c")));
        assert_eq!(split_token("."), Some(("", "")));
        assert_eq!(split_token("abc"), None);
        assert_eq!(split_token(""), None);
    }

    /// **`b64url_encode` IS `btoa(...).replace(/\+/g,"-").replace(/\//g,"_").replace(/=+$/,"")`**, and the two
    /// cases that pin it are a byte triplet that produces `+` and `/`, and every padding length.
    ///
    /// MUTATION: emit the `=` padding.
    /// RESULT:   `the_signature_encoding_has_no_padding` fails — and the consequence is not cosmetic: the
    ///           comparison against a real token's signature is then never equal, so EVERY session 401s.
    #[test]
    fn the_signature_encoding_has_no_padding() {
        assert_eq!(b64url_encode(b""), "");
        assert_eq!(b64url_encode(b"f"), "Zg");
        assert_eq!(b64url_encode(b"fo"), "Zm8");
        assert_eq!(b64url_encode(b"foo"), "Zm9v");
        assert_eq!(b64url_encode(b"foob"), "Zm9vYg");
        assert_eq!(b64url_encode(b"fooba"), "Zm9vYmE");
        assert_eq!(b64url_encode(b"foobar"), "Zm9vYmFy");
        // The classic `+`/`/` triplet, and the byte values that exercise the alphabet's edges.
        assert_eq!(b64url_encode(&[0xfb, 0xff, 0xbf]), "-_-_");
        assert_eq!(b64url_encode(&[0x00, 0x10, 0x83]), "ABCD");
    }

    /// The decoder round-trips, restores the stripped padding, and refuses what `atob` refuses.
    #[test]
    fn the_decoder_round_trips_and_refuses_garbage() {
        for bytes in [
            &b""[..],
            &b"f"[..],
            &b"fo"[..],
            &b"foob"[..],
            &b"foobar"[..],
            &[0xfb, 0xff, 0xbf][..],
        ] {
            let encoded = b64url_encode(bytes);
            assert_eq!(b64url_decode(&encoded).as_deref(), Some(bytes), "{encoded}");
        }
        assert_eq!(
            b64url_decode("eyJ1aWQiOiJhIn0"),
            Some(b"{\"uid\":\"a\"}".to_vec())
        );
        assert_eq!(b64url_decode("!!!!"), None);
        // `atob("Zg==")` is `"f"`: the caller RESTORES the padding the encoder stripped, so a 3-character
        // payload is a valid one-byte value and not an error.
        assert_eq!(b64url_decode("Zg="), Some(b"f".to_vec()));
        assert_eq!(
            b64url_decode("Z"),
            None,
            "a single character cannot be a group"
        );
        assert_eq!(b64url_decode("Zg==="), None);
    }

    /// The claims: `exp` must be a NUMBER (a string expiry is refused rather than coerced), the comparison is
    /// against the clock, `uid` is stringified — including `undefined` for a payload with no uid — and `role`
    /// falls back to `"user"` for a missing OR falsy role.
    #[test]
    fn the_claims_are_the_sources() {
        assert_eq!(
            session_claims(&json!({"uid": "u1", "role": "admin", "exp": 2000}), 1000),
            Some(TokenClaims {
                uid: "u1".into(),
                role: "admin".into()
            })
        );
        // `exp` exactly now is still valid (`data.exp < Date.now()` is strict).
        assert!(session_claims(&json!({"uid": "u1", "exp": 1000}), 1000).is_some());
        assert!(session_claims(&json!({"uid": "u1", "exp": 999}), 1000).is_none());
        assert!(session_claims(&json!({"uid": "u1", "exp": "2000"}), 1000).is_none());
        assert!(session_claims(&json!({"uid": "u1"}), 1000).is_none());
        assert_eq!(
            session_claims(&json!({"uid": "u1", "role": "", "exp": 2000}), 1000)
                .unwrap()
                .role,
            "user"
        );
        assert_eq!(
            session_claims(&json!({"uid": "u1", "role": 0, "exp": 2000}), 1000)
                .unwrap()
                .role,
            "user"
        );
        assert_eq!(
            session_claims(&json!({"exp": 2000}), 1000).unwrap().uid,
            "undefined"
        );
        assert_eq!(
            session_claims(&json!({"uid": 7, "exp": 2000}), 1000)
                .unwrap()
                .uid,
            "7"
        );
    }

    /// **THE ISSUED PAYLOAD IS THE SOURCE'S EXACT JSON**, because it is what the token IS: a different key order
    /// is a different token, and the corpus compares tokens byte for byte.
    ///
    /// MUTATION: build the payload as `{"role":…, "uid":…, "exp":…}`.
    /// RESULT:   this test fails, and so does every recorded login case — the cookie the worker sets would not
    ///           be the cookie the shipping console sets.
    #[test]
    fn the_issued_payload_is_the_sources_json() {
        let payload = session_payload("admin", "admin", 1_760_000_000_000);
        let decoded = b64url_decode(&payload).unwrap();
        assert_eq!(
            String::from_utf8(decoded).unwrap(),
            r#"{"uid":"admin","role":"admin","exp":1760086400000}"#
        );
        // ...and it verifies as a session for a clock inside its 24-hour window, with the claims it was built
        // from. (The HMAC itself is the platform's; this pins the TTL arithmetic, which is `Date.now() + TTL`.)
        let data: Value = serde_json::from_str(
            &String::from_utf8(b64url_decode(&payload).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            session_claims(&data, 1_760_000_000_000),
            Some(TokenClaims {
                uid: "admin".into(),
                role: "admin".into()
            })
        );
        assert!(
            session_claims(&data, 1_760_086_400_001).is_none(),
            "a millisecond past the 24-hour TTL"
        );
        assert!(session_claims(&data, 1_760_086_400_000).is_some(), "…to the ms");
    }

    /// The claim strings a real session cookie's payload decodes to — the two shapes `issueSessionToken`
    /// produces, byte for byte (this is the oracle the corpus was recorded from).
    #[test]
    fn a_real_payload_decodes_to_its_claims() {
        let payload = b64url_encode(br#"{"uid":"admin","role":"admin","exp":1760000000000}"#);
        let decoded = b64url_decode(&payload).unwrap();
        let text = String::from_utf8(decoded).unwrap();
        assert_eq!(
            text,
            r#"{"uid":"admin","role":"admin","exp":1760000000000}"#
        );
        assert_eq!(
            session_claims(&serde_json::from_str(&text).unwrap(), 1759999999999),
            Some(TokenClaims {
                uid: "admin".into(),
                role: "admin".into()
            })
        );
    }

    /// `js_to_string`'s object arm, which the `undefined` uid test above depends on not being reached.
    #[test]
    fn js_string_of_a_missing_value_is_the_literal_undefined() {
        assert_eq!(js_to_string_of(None), "undefined");
        assert_eq!(js_to_string_of(Some(&json!(null))), "null");
        assert_eq!(js_to_string(&json!({"a": 1})), "[object Object]");
    }
}
