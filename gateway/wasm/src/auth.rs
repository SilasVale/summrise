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
