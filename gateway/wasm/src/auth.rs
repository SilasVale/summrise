//! THE AUTH DECISIONS THAT ARE PURE, MOVED FROM `gateway/src/auth.ts`.
//!
//! WHY THESE. Every one of them is a SECURITY SHAPE, and none of them throws when it drifts: a compare
//! that stops being timing-safe, a cookie parse that reads the wrong side of the `=`, a CSRF gate that
//! starts refusing the browser it was written for — or stops refusing a drive-by. The crypto (HMAC issue
//! and verify) stays in TypeScript: it needs `crypto.subtle`, which is I/O, not a decision.
//!
//! THE TWO HARD PARTS ARE NAMED WHERE THEY LIVE: `b64_url_decode`'s `atob` returns a string of ONE CODE
//! UNIT PER BYTE (not a UTF-8 decode), and `csrf_cookie_violation` treats an ABSENT `Sec-Fetch-Site` as
//! ALLOWED, which is deliberate and documented in the source.

/// `SESSION_COOKIE` — the name the CSRF gate keys on.
pub const SESSION_COOKIE: &str = "ag_session";

/// `MUTATING_METHODS` — the methods the CSRF gate applies to at all.
pub const MUTATING_METHODS: [&str; 4] = ["POST", "PUT", "PATCH", "DELETE"];

/// `safeEq(a, b)` — `timingSafeEqual`: equal LENGTH first, then an XOR fold over the code units.
///
/// **THE LENGTH CHECK IS A SHORT-CIRCUIT AND THE JAVASCRIPT HAS IT TOO**, so a port that folded without
/// it would be answering `false` more slowly rather than more safely. The fold accumulates and compares
/// once, which is what makes the comparison not depend on WHERE the first difference is.
pub fn safe_eq(a: &str, b: &str) -> bool {
    let au: Vec<u16> = a.encode_utf16().collect();
    let bu: Vec<u16> = b.encode_utf16().collect();
    if au.len() != bu.len() {
        return false;
    }
    let mut diff: u32 = 0;
    for i in 0..au.len() {
        diff |= (au[i] ^ bu[i]) as u32;
    }
    diff == 0
}

/// `parseCookie(str)` — split on `;`, keep the pairs with an `=`, take the FIRST `=`, trim BOTH sides.
///
/// Three details that a "split on = and trim" port gets wrong: a pair with no `=` is SKIPPED (not stored
/// as an empty value), the split is at the FIRST `=` so a value may contain more of them, and a duplicate
/// name keeps the LAST one — a plain object assignment, which is also what a `BTreeMap` would NOT do.
pub fn parse_cookie(s: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for pair in s.split(';') {
        let Some(i) = pair.find('=') else {
            continue;
        };
        let name = pair[..i].trim().to_string();
        let value = pair[i + 1..].trim().to_string();
        match out.iter_mut().find(|(k, _)| *k == name) {
            Some((_, v)) => *v = value,
            None => out.push((name, value)),
        }
    }
    out
}

/// `csrfCookieViolation(method, cookie, secFetchSite)` — the pure decision, with the request's three
/// inputs as arguments.
///
/// **AN ABSENT `Sec-Fetch-Site` IS ALLOWED, AND THAT IS THE DOCUMENTED RULE.** Browsers attach that header
/// to every request and a cross-site page cannot forge it (it is a forbidden header), so a MISSING one
/// means a non-browser client: raw HTTP has no ambient cookie to ride, and the test suites and CLI session
/// probes depend on this arm. The gate applies only to MUTATING methods, only when a cookie is present,
/// and only to the TWO credential families — the session cookie and the device proxy's per-device
/// `summrise_pt_<name>` cookies.
pub fn csrf_cookie_violation(method: &str, cookie: &str, sec_fetch_site: Option<&str>) -> bool {
    let method = method.to_uppercase();
    if !MUTATING_METHODS.contains(&method.as_str()) {
        return false;
    }
    let has_session = cookie.contains(&format!("{SESSION_COOKIE}="));
    let has_device_pair = parse_cookie(cookie)
        .iter()
        .any(|(name, _)| name.starts_with("summrise_pt_"));
    if !has_session && !has_device_pair {
        return false; // the bearer path carries no cookie
    }
    let Some(site) = sec_fetch_site else {
        return false;
    };
    let v = site.to_lowercase();
    v != "same-origin" && v != "none"
}

/// `b64urlDecodeStr(s)` — restore the padding, translate the URL alphabet, and `atob`.
///
/// **`atob` ANSWERS A STRING OF ONE CODE UNIT PER BYTE**, which is NOT a UTF-8 decode: a payload whose
/// bytes are not valid UTF-8 still decodes, and every code unit is 0..=255. So this returns those code
/// points as a `String` — the same characters JavaScript would hold — rather than running the bytes
/// through `from_utf8`, which would refuse payloads the shipping worker accepts.
///
/// `None` IS THE REFUSAL, because that is all the caller asks: `verifySessionToken` wraps the decode in a
/// `try/catch` and answers 401 either way.
///
/// IT REFUSES WHAT `atob` REFUSES — AND **`atob` DOES *NOT* IGNORE WHITESPACE**, which is a thing I
/// assumed from the specification's forgiving-base64 and the oracle refused: `atob("aGVs bG8=")` THROWS
/// (measured by `oracle-translate.mjs`, case "whitespace is ignored"). So nothing is stripped here; the
/// only translation is the URL alphabet, which is the function's own (`-` -> `+`, `_` -> `/`).
pub fn b64_url_decode(s: &str) -> Option<String> {
    let mut padded: String = s
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    // `const pad = s.length % 4 === 0 ? "" : "=".repeat(4 - (s.length % 4))`
    let rem = padded.len() % 4;
    if rem != 0 {
        padded.push_str(&"=".repeat(4 - rem));
    }
    let bytes = padded.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = String::with_capacity(bytes.len() / 4 * 3);
    let groups = bytes.len() / 4;
    for gi in 0..groups {
        let group = &bytes[gi * 4..gi * 4 + 4];
        let last = gi == groups - 1;
        let mut acc: u32 = 0;
        let mut pad = 0usize;
        for (j, &c) in group.iter().enumerate() {
            let v = match c {
                b'A'..=b'Z' => (c - b'A') as u32,
                b'a'..=b'z' => (c - b'a' + 26) as u32,
                b'0'..=b'9' => (c - b'0' + 52) as u32,
                b'+' => 62,
                b'/' => 63,
                b'=' => {
                    // Padding is legal ONLY in the last group and only at its END — `atob` refuses
                    // `A=AA` and refuses padding in any earlier group.
                    if !last || j < 2 {
                        return None;
                    }
                    pad += 1;
                    0
                }
                _ => return None,
            };
            acc = (acc << 6) | v;
        }
        if pad == 1 && group[3] != b'=' {
            return None;
        }
        if pad == 2 && (group[2] != b'=' || group[3] != b'=') {
            return None;
        }
        let triple = [
            ((acc >> 16) & 0xff) as u8,
            ((acc >> 8) & 0xff) as u8,
            (acc & 0xff) as u8,
        ];
        for b in &triple[..3 - pad] {
            out.push(*b as char);
        }
    }
    Some(out)
}

/// `sessionCookieHeader(token, maxAgeSec, secure)` — the exact string, `Secure` only when asked.
pub fn session_cookie_header(token: &str, max_age_sec: i64, secure: bool) -> String {
    format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_sec}{}",
        if secure { "; Secure" } else { "" }
    )
}

/// `clearSessionCookieHeader(secure)` — the same shape with an empty value and `Max-Age=0`.
pub fn clear_session_cookie_header(secure: bool) -> String {
    format!(
        "{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
        if secure { "; Secure" } else { "" }
    )
}

/// **THE AUTH PRIMITIVES, EMITTED — `auth-logic.ts`, GENERATED from this module.**
///
/// `auth.ts` is 209 lines, and everything in it that DECIDES is here already: the timing-safe compare's
/// length rule, the cookie parser's four details, the CSRF gate's order, the two cookie headers' spellings, the
/// base64url alphabet, and the token payload's claims check. What the TypeScript keeps is the HOST half:
/// `crypto.subtle` (PBKDF2 and HMAC), `btoa`/`atob`/`TextEncoder`, and `randomHex`.
///
/// **THE SIGNATURES ARE THE SOURCE'S**, `Request` and all: the emitted module calls `.method`,
/// `.headers.get(...)` and nothing else, so a request-shaped signature keeps every call site identical rather
/// than moving an extraction into each of them.
const TS_MODULE: &str = r#"/**
 * auth-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/auth.rs` (`render_typescript`). The freshness test in that module is a byte
 * comparison against this file:
 *
 *     SUMMRISE_REFRESH_AUTH=1 cargo test --manifest-path gateway/wasm/Cargo.toml auth::emission
 *
 * `gateway/src/auth.ts` re-exports this module and keeps the HOST: `crypto.subtle`'s PBKDF2 and HMAC, the
 * base64 platform calls, and `randomHex`.
 */

/** The cookie the whole console keys on. `session-logic.ts` carries it too, from `session_gate.rs` — ONE
 *  spelling, two emissions of it, and the tests that import either get the same string. */
export const SESSION_COOKIE = "ag_session";

/** Session lifetime. **SAFELY CHANGEABLE**, unlike the password iteration count: the value is not read at
 *  VERIFY time — it is written into the signed payload as `exp` at issuance, and verification trusts that
 *  embedded expiry. */
export const SESSION_TTL_MS = 24 * 3600 * 1000;

/** The methods the CSRF gate looks at. */
export const MUTATING_METHODS = new Set(["POST", "PUT", "PATCH", "DELETE"]);

/** The timing-safe compare: **LENGTH FIRST**, then a constant-time XOR fold. A length difference is not a
 *  secret; the contents are. */
export function safeEq(a: string, b: string): boolean {
  if (typeof a !== "string" || typeof b !== "string" || a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

/**
 * `parseCookie(str)` — split on `;`, keep the pairs with an `=`, take the FIRST `=`, trim BOTH sides.
 *
 * **FOUR DETAILS A "SPLIT ON `=` AND TRIM" PORT GETS WRONG**: a pair with no `=` is SKIPPED rather than
 * stored as an empty value; the split is at the FIRST `=` so a value may contain more of them; both sides are
 * trimmed; and a duplicate name keeps the LAST one — a plain object assignment.
 */
export function parseCookie(str: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const pair of str.split(";")) {
    const i = pair.indexOf("=");
    if (i < 0) continue;
    const name = pair.slice(0, i).trim();
    out[name] = pair.slice(i + 1).trim();
  }
  return out;
}

/**
 * `csrfCookieViolation(request)` — **THE GATE IS ABOUT COOKIES, NOT ABOUT WRITES**: a mutating request is only
 * a CSRF candidate when the browser will ATTACH a credential by itself, which is why the bearer path is
 * exempt. A missing `sec-fetch-site` is exempt too (a non-browser client), and the two accepted values are
 * `same-origin` and `none` — anything else, including an absent-looking `""`, is a violation.
 */
export function csrfCookieViolation(request: Request): boolean {
  const method = request.method.toUpperCase();
  if (!MUTATING_METHODS.has(method)) return false;
  const cookie = request.headers.get("cookie") || "";
  const hasSession = cookie.includes(`${SESSION_COOKIE}=`);
  const hasDevicePair = Object.keys(parseCookie(cookie)).some((n) => n.startsWith("summrise_pt_"));
  if (!hasSession && !hasDevicePair) return false; // bearer path
  const site = request.headers.get("sec-fetch-site");
  if (site == null) return false;
  const v = site.toLowerCase();
  return v !== "same-origin" && v !== "none";
}

/** `b64urlDecodeStr(s)` — the URL-safe alphabet back to standard, PADDED to a multiple of four, then the
 *  platform's decode. */
export function b64urlDecodeStr(s: string): string {
  const pad = s.length % 4 === 0 ? "" : "=".repeat(4 - (s.length % 4));
  return atob(s.replace(/-/g, "+").replace(/_/g, "/") + pad);
}

/** The session cookie as the console sets it. `Secure` is the caller's — a loopback deployment has no TLS. */
export function sessionCookieHeader(token: string, maxAgeSec: number, secure: boolean): string {
  return `${SESSION_COOKIE}=${token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=${maxAgeSec}${secure ? "; Secure" : ""}`;
}

/** The same shape with an empty value and `Max-Age=0`. */
export function clearSessionCookieHeader(secure: boolean): string {
  return `${SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0${secure ? "; Secure" : ""}`;
}

/** `splitToken(token)` — at the FIRST dot, and `null` when there is none. */
export function splitToken(token: string): [string, string] | null {
  const dot = token.indexOf(".");
  if (dot < 0) return null;
  return [token.slice(0, dot), token.slice(dot + 1)];
}

/**
 * `verifySessionToken`'s last three steps — the claims, and the two rules that make them safe to trust:
 * **`exp` MUST BE A NUMBER** (a string `exp` is not compared as text) and must not have passed; `uid` is
 * `String(data.uid)`, and `role` falls back to `"user"` through a TRUTHINESS test, so `""` and `0` fall back
 * too.
 */
export function sessionClaims(data: any, nowMs: number): { uid: string; role: string } | null {
  if (typeof data?.exp !== "number") return null;
  if (data.exp < nowMs) return null;
  return { uid: String(data.uid), role: String(data.role || "user") };
}
"#;

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_MODULE.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/auth-logic.ts")
}

#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK** — the committed file IS `render_typescript()`'s bytes.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_AUTH").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write auth-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_AUTH=1 cargo test auth::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/auth-logic.ts is not what auth.rs emits — \
             run SUMMRISE_REFRESH_AUTH=1 cargo test auth::emission"
        );
    }

    /// **THE FOUR DETAILS OF THE COOKIE PARSER, AND THE GATE'S ORDER, ARE PINNED BY NAME.** Each of them is a
    /// line a "cleanup" would drop: the `i < 0` skip, the `slice(0, i)`/`slice(i + 1)` split, the two `trim()`
    /// calls, and the `site == null` exemption that keeps the bearer path working.
    #[test]
    fn the_emission_carries_the_details() {
        let ts = render_typescript();
        for item in [
            "export function safeEq(a: string, b: string): boolean {",
            "if (i < 0) continue;",
            "const name = pair.slice(0, i).trim();",
            "out[name] = pair.slice(i + 1).trim();",
            "if (!MUTATING_METHODS.has(method)) return false;",
            "if (!hasSession && !hasDevicePair) return false; // bearer path",
            "const site = request.headers.get(\"sec-fetch-site\");",
            "if (site == null) return false;",
            "return v !== \"same-origin\" && v !== \"none\";",
            "export function b64urlDecodeStr(s: string): string {",
            "export function sessionCookieHeader(",
            "export function clearSessionCookieHeader(secure: boolean): string {",
            "export function splitToken(token: string): [string, string] | null {",
            "if (typeof data?.exp !== \"number\") return null;",
            "return { uid: String(data.uid), role: String(data.role || \"user\") };",
        ] {
            assert!(ts.contains(item), "the emission lost: {item}");
        }
    }
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `auth` cases were produced by the SHIPPING TypeScript
    //! (`gateway/wasm/oracle-translate.mjs`), and this replays them.
    use super::*;

    fn corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/translate-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    #[test]
    fn every_auth_decision_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"];
            match func {
                "safe_eq" => {
                    let got = safe_eq(
                        input["a"].as_str().unwrap_or(""),
                        input["b"].as_str().unwrap_or(""),
                    );
                    assert_eq!(
                        &serde_json::Value::Bool(got),
                        &want["value"],
                        "{func} / {name}"
                    );
                }
                "parse_cookie" => {
                    let got: serde_json::Map<String, serde_json::Value> =
                        parse_cookie(input.as_str().unwrap_or(""))
                            .into_iter()
                            .map(|(k, v)| (k, serde_json::Value::String(v)))
                            .collect();
                    assert_eq!(
                        &serde_json::Value::Object(got),
                        &want["value"],
                        "{func} / {name}"
                    );
                }
                "csrf_cookie_violation" => {
                    let got = csrf_cookie_violation(
                        input["method"].as_str().unwrap_or(""),
                        input["cookie"].as_str().unwrap_or(""),
                        input["secFetchSite"].as_str(),
                    );
                    assert_eq!(
                        &serde_json::Value::Bool(got),
                        &want["value"],
                        "{func} / {name}"
                    );
                }
                "b64_url_decode" => {
                    let got = b64_url_decode(input.as_str().unwrap_or(""));
                    match (&got, want.get("threw").is_some()) {
                        (Some(text), false) => assert_eq!(
                            &serde_json::Value::String(text.clone()),
                            &want["value"],
                            "{func} / {name}"
                        ),
                        (None, true) => {}
                        (Some(text), true) => {
                            panic!("{func} / {name}: atob THROWS and this answered {text:?}")
                        }
                        (None, false) => {
                            panic!("{func} / {name}: this refused where atob answered")
                        }
                    }
                }
                "session_cookie_header" => {
                    let got = session_cookie_header(
                        input["token"].as_str().unwrap_or(""),
                        input["maxAgeSec"].as_i64().unwrap_or(0),
                        input["secure"].as_bool().unwrap_or(false),
                    );
                    assert_eq!(
                        &serde_json::Value::String(got),
                        &want["value"],
                        "{func} / {name}"
                    );
                }
                "clear_session_cookie_header" => {
                    let got =
                        clear_session_cookie_header(input["secure"].as_bool().unwrap_or(false));
                    assert_eq!(
                        &serde_json::Value::String(got),
                        &want["value"],
                        "{func} / {name}"
                    );
                }
                _ => continue,
            }
            checked += 1;
        }
        assert!(checked >= 25, "the auth corpus shrank to {checked} cases");
    }
}
