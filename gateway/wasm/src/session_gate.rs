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

/// **THE SESSION'S SEQUENCE, EMITTED — AND THE ORDER IS THE SECURITY PROPERTY.**
///
/// `session.ts` is 151 lines and every one of its checks is this module's, but the file is a HOST for the same
/// reason `store/admin.ts` is: what it holds is a SEQUENCE of platform calls, and the sequence's ORDER is the
/// decision ("an unconfigured deployment answers `null` before it parses a cookie"; "a revoked cookie dies before
/// the signature is verified"; "a rotation is accepted ONCE and only when the two keys really differ").
///
/// So the emission is a function that takes the platform as ARGUMENTS — the `seedAdminSequence` treatment — and
/// what stays in TypeScript is the cookie parse, the Cloudflare Access fallback, the two refusal responses and the
/// store seam.
const TS_MODULE: &str = r#"/**
 * session-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/session_gate.rs` (`render_typescript`). The freshness test in that module is a
 * byte comparison against this file:
 *
 *     SUMMRISE_REFRESH_SESSION=1 cargo test --manifest-path gateway/wasm/Cargo.toml session_gate::emission
 *
 * `gateway/src/session.ts` re-exports this module and keeps the HOST: the cookie parse, the Cloudflare Access
 * fallback, the store seam (`SessionUserStore`), the isolate cache the revocation reads go through, and the two
 * refusal responses.
 */

/** The session's lifetime — 24 hours, written into the signed payload as `exp` at ISSUANCE. */
export const SESSION_TTL_MS = 24 * 3600 * 1000;

/** The cookie the whole console keys on. */
export const SESSION_COOKIE = "ag_session";

/** The revocation negative cache: 60 s, bounded at 512 entries like the source's `Map`. */
export const NOT_REVOKED_TTL_MS = 60000;
export const NOT_REVOKED_MAX = 512;

/** The two refusals, spelled exactly — they are response bodies. */
export const NOT_LOGGED_IN_MESSAGE = "Not logged in or session expired";
export const ADMIN_REQUIRED_MESSAGE = "Admin permission required";

/** The line the console logs when it refuses to ISSUE a session. **FAIL CLOSED**: without a dedicated
 *  high-entropy `SESSION_SECRET` the HMAC key would fall back to the admin password, letting any invited user
 *  offline-brute-force it from their own signed cookie — HMAC-SHA256 is not memory-hard. */
export const ISSUE_SECRET_REFUSED_LOG =
  "[session] SESSION_SECRET not configured — refusing to issue new session (fail-closed)";

/** The VERIFY path's key choice (rotation compat): the dedicated `SESSION_SECRET`, or the admin password so a
 *  first-time `SESSION_SECRET` rollout does not log everyone out. **ISSUANCE MUST NOT USE THIS** — it uses
 *  `issueSessionSecret`, which fails closed. */
export function sessionSecret(env: any, adminPassword: string): string {
  return env?.SESSION_SECRET || adminPassword;
}

/** The ISSUANCE key, or `null` when unconfigured — the caller must refuse to issue a session (HTTP 500
 *  `config_error`) rather than silently signing with the admin password. */
export function issueSessionSecret(env: any): string | null {
  return env?.SESSION_SECRET ? env.SESSION_SECRET : null;
}

/** The admin gate's role test, spelled once. */
export function isAdmin(user: unknown): boolean {
  return (user as { role?: unknown } | null | undefined)?.role === "admin";
}

/** What the revocation cache holds for one cookie. */
export interface RevokedEntry {
  revoked: boolean;
  exp: number;
}

/** The isolate cache's surface — the HOST owns the `Map`, because it is platform state. */
export interface RevokedCache {
  get(key: string): RevokedEntry | undefined;
  set(key: string, entry: RevokedEntry): void;
  size(): number;
  clear(): void;
}

/** A verified session's claims. */
export interface SessionClaims {
  uid: string;
  role: string;
}

/** Everything `cookieSession` needs from the platform, named once.
 *
 *  **GENERIC IN THE USER RECORD, BECAUSE THE SEQUENCE RETURNS THE USER, NOT THE CLAIMS.** The source returns
 *  `user` — the record its caller then reads fields off — and an earlier revision of this emission returned the
 *  verified CLAIMS instead: `role` survived (so the admin gate still worked) while every other field was
 *  missing, which the console's own suite caught as 28 failures across session-gated routes. The claims are the
 *  uid lookup's input and nothing else. */
export interface CookieSessionDeps<T> {
  /** The deployment's admin password — an EMPTY one means no session can exist. */
  adminPassword: string;
  /** The env-like object the session flow reads — its ONE field is `SESSION_SECRET`, and it is an object
   *  rather than a string because the SOURCE's `sessionSecret(env, adminPassword)` takes one. */
  env: { SESSION_SECRET?: string | null };
  /** The request's session cookie, already parsed out (the host owns header parsing). */
  cookie: string | undefined;
  /** Whether the KV binding exists at all: the revocation read is skipped without one. */
  hasKeys: boolean;
  /** `Date.now()`, injected so the cache's expiry is testable. */
  nowMs(): number;
  /** The revocation read: `(await KEYS.get(`sess-revoked:${cookie}`)) === "1"`. */
  readRevoked(cookie: string): Promise<boolean>;
  /** The negative cache, bounded and cleared by the host's `Map`. */
  cache: RevokedCache;
  /** `verifySessionToken(secret, token)`. */
  verify(secret: string, token: string): Promise<SessionClaims | null>;
  /** The user record, or null when it is gone. */
  loadUser(uid: string): Promise<T | null>;
}

/**
 * **THE SESSION SEQUENCE, IN ORDER — AND THE ORDER IS THE BEHAVIOUR:**
 *
 *   1. an EMPTY admin password answers `null` before anything else is read;
 *   2. a missing cookie answers `null`;
 *   3. a REVOKED cookie dies BEFORE the signature is verified (a copied cookie is dead even when it verifies),
 *      through a 60 s negative cache that is CLEARED rather than pruned at its bound;
 *   4. the signature is tried with `sessionSecret(...)`, then — ONCE, and only when the two keys really differ —
 *      with the admin password, for the rotation window;
 *   5. the user must exist AND be enabled.
 *
 * Every arm returns `null` rather than distinguishing its reason: the caller answers 401 either way, and the
 * distinction is not one a client may learn.
 */
export async function cookieSession<T extends { enabled?: boolean }>(
  deps: CookieSessionDeps<T>,
): Promise<T | null> {
  const adminPassword = deps.adminPassword;
  if (!adminPassword) return null;
  const cookie = deps.cookie;
  if (!cookie) return null;
  if (deps.hasKeys) {
    const hit = deps.cache.get(cookie);
    if (hit && hit.exp > deps.nowMs()) {
      if (hit.revoked) return null;
    } else {
      const revoked = await deps.readRevoked(cookie);
      if (deps.cache.size() >= NOT_REVOKED_MAX) deps.cache.clear();
      deps.cache.set(cookie, { revoked, exp: deps.nowMs() + NOT_REVOKED_TTL_MS });
      if (revoked) return null;
    }
  }
  // **ROTATION COMPAT, ONCE**: `SESSION_SECRET` is set now, but this cookie may have been signed with the old
  // admin-password key — and the two keys must really differ for the second attempt to be a rotation rather
  // than the same verify twice.
  const primary = sessionSecret(deps.env, adminPassword);
  let session = await deps.verify(primary, cookie);
  if (
    !session &&
    deps.env.SESSION_SECRET &&
    adminPassword &&
    deps.env.SESSION_SECRET !== adminPassword
  ) {
    session = await deps.verify(adminPassword, cookie);
  }
  if (!session) return null;
  // **THE USER, NOT THE CLAIMS** — `role` is the only claim the caller reads, and its presence is what made a
  // claims-returning version look correct while breaking twenty-eight routes.
  const user = await deps.loadUser(session.uid);
  if (!user || !user.enabled) return null;
  return user;
}
"#;

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_MODULE.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/session-logic.ts")
}

#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK** — the committed file IS `render_typescript()`'s bytes.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_SESSION").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write session-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_SESSION=1 cargo test session_gate::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/session-logic.ts is not what session_gate.rs emits — \
             run SUMMRISE_REFRESH_SESSION=1 cargo test session_gate::emission"
        );
    }

    /// **THE SEQUENCE'S FIVE STEPS ARE PINNED BY NAME, IN ORDER** — a comment reordered is a comment, and this
    /// function's arms ARE the security property. The mutation that matters is moving the revocation check BELOW
    /// the verify: a copied cookie would then be checked against the signature first, which is the one thing the
    /// blacklist exists to prevent.
    #[test]
    fn the_emission_carries_the_sequence() {
        let ts = render_typescript();
        let order = [
            "if (!adminPassword) return null;",
            "if (!cookie) return null;",
            "const revoked = await deps.readRevoked(cookie);",
            "const primary = sessionSecret(",
            "session = await deps.verify(adminPassword, cookie);",
            "if (!session) return null;",
            "if (!user || !user.enabled) return null;",
            "return user;",
        ];
        let mut at = 0usize;
        for step in order {
            let found = ts[at..]
                .find(step)
                .unwrap_or_else(|| panic!("the emission lost the step: {step}"));
            at += found + step.len();
        }
        for item in [
            "export const SESSION_TTL_MS = 24 * 3600 * 1000;",
            "export const SESSION_COOKIE = \"ag_session\";",
            "export const NOT_REVOKED_TTL_MS = 60000;",
            "export const NOT_REVOKED_MAX = 512;",
            "export const NOT_LOGGED_IN_MESSAGE = \"Not logged in or session expired\";",
            "export const ADMIN_REQUIRED_MESSAGE = \"Admin permission required\";",
            "export function issueSessionSecret(",
            "export function isAdmin(",
        ] {
            assert!(ts.contains(item), "the emission lost: {item}");
        }
    }
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
        let data: Value =
            serde_json::from_str(&String::from_utf8(b64url_decode(&payload).unwrap()).unwrap())
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
        assert!(
            session_claims(&data, 1_760_086_400_000).is_some(),
            "…to the ms"
        );
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
