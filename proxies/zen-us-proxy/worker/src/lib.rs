//! `zen-us-proxy`'s PORTABLE HALF — the origin policy and the redaction, with no I/O anywhere in it.
//!
//! ## What is here and what is not
//!
//! `proxies/zen-us-proxy/src/index.js` was 339 lines — **deleted 2026-10-08; this crate and `worker.rs`
//! are its port** — and the parts that touch a network or a request were
//! not portable: the upstream fetch, the streaming relay, the Durable-Object session header and the
//! constant-time key comparison (SHA-256, which is not available in this crate's host build). **What IS
//! portable is the policy** — which origins may be reflected, and what is scrubbed out of an error
//! before a client ever sees it — and policy is exactly the part a transliteration can get subtly
//! wrong without any test noticing.
//!
//! ## THE THREE FACTS, each with the trap it carries
//!
//! * **`redact_secrets` SORTS BY LENGTH, DESCENDING, AND THE ORDER IS THE WHOLE POINT.** A secret that
//!   is a substring of another is masked by the longer one first; sort ascending and the shorter masks
//!   the tail of the longer, and what comes out is `*******cret` instead of `***`. It also DEDUPES
//!   first (a repeated secret would be harmless but the list is the order the sort sees), it IGNORES
//!   anything shorter than 8 characters (so a 7-char secret survives — deliberately, since masking a
//!   1-char string would shred the text), and it IGNORES NON-STRINGS, which is a real filter and not a
//!   type assertion: a numeric `12345678` has a length and must still be left alone.
//! * **`is_loopback_origin` PARSES, `is_loopback_host` COMPARES.** The first answers "is this an
//!   http(s) URL on the loopback host", the second answers "is this hostname loopback". They are
//!   different questions, and the CORS rule needs BOTH: a loopback `Origin` is reflected only when
//!   the request's own host is also loopback, so the deployed proxy never reflects a foreign page's
//!   `http://localhost`.
//! * **AND `is_loopback_host` IS NOT THE SAME FACT AS THE LANDING'S LOOPBACK TEST.** That one is
//!   `Some("localhost") | Some("127.0.0.1") | Some("[::1]")` — round 449 added `[::1]` deliberately.
//!   This one has two hostnames and no `[::1]`. **The two copies have drifted, and this port keeps
//!   each worker's own answer** because unifying them is a behaviour change to a CORS decision, not a
//!   refactor, and nothing here is evidence for which is intended. It is recorded instead, which is
//!   the honest state: two workers, two loopback rules, one of them unexplained.

/// `VERIFY_PATH` — the native Anthropic pass-through's path suffix, matched with `endsWith`.
pub const VERIFY_PATH: &str = "/v1/messages";

/// `RESPONSES_PATH` — the OpenAI Responses API BYOK pass-through's path suffix, same matching.
pub const RESPONSES_PATH: &str = "/v1/responses";

/// The two production origins this worker reflects. A `Set` in the JavaScript, so membership is
/// EXACT — no prefix, no suffix, no case folding. (The origin header is compared as the browser sent
/// it, and a browser lowercases the host and scheme, so this set is already in the form one arrives.)
pub const ALLOWED_ORIGINS: [&str; 2] = ["https://ai.saisi.online", "https://api.saisi.online"];

/// `isLoopbackOrigin(origin)` — is this an `http:`/`https:` URL whose hostname is loopback?
///
/// **AN UNPARSEABLE VALUE IS FALSE, NOT AN ERROR**: the JavaScript wraps `new URL(origin)` in a
/// `try` and returns `false` on a throw, and a CORS decision that throws would take down the request
/// rather than refuse the reflection.
pub fn is_loopback_origin(origin: &str) -> bool {
    let Ok(u) = url::Url::parse(origin) else {
        return false;
    };
    let scheme_ok = matches!(u.scheme(), "http" | "https");
    // **THE URL CRATE HAS NO `hostname` FIELD** — it is `host_str()`, and it is a sub-slice of
    // `u.host()`, which for an IPv6 host carries the brackets. The two callers below are the reason
    // that matters: `is_loopback_host` compares bare hostnames, and `[::1]` would never match
    // `::1` here. This worker's list has no IPv6 entry, so brackets are not a live case — but the
    // function must not be written as if they were.
    let Some(host) = u.host_str() else {
        return false;
    };
    scheme_ok && is_loopback_host(host)
}

/// `isLoopbackHost(hostname)` — the bare comparison, and the whole of what "loopback" means here.
///
/// **LOWERCASING IS NOT DONE, AND THAT IS FAITHFUL**: the JavaScript compares `u.hostname`, which the
/// WHATWG parser has already lowercased, so the comparison is case-insensitive *through the parser*
/// and not here. A caller passing a raw mixed-case string is a caller the JavaScript could not be.
pub fn is_loopback_host(hostname: &str) -> bool {
    hostname == "localhost" || hostname == "127.0.0.1"
}

/// `requestHost(request)` — the host of the request's own URL, or `""` when it cannot be parsed.
///
/// The JavaScript takes a `Request`; the decision only ever uses the URL, so this takes the URL
/// string and the caller reads it off the request. **AN EMPTY STRING IS A REAL ANSWER HERE**, and it
/// is the safe one: an unparseable request URL must not become "loopback".
pub fn request_host(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(u) => u.host_str().unwrap_or_default().to_string(),
        Err(_) => String::new(),
    }
}

/// `corsHeaders(request)` — the header map, as a decision over `(origin, request host)`.
///
/// **THE REFLECTION IS CONDITIONAL AND THE CONDITION HAS THREE PARTS**: the origin is in the
/// allowlist, OR the origin is loopback AND the request's own host is loopback. A deployed proxy
/// therefore never reflects a foreign page's `http://localhost` — which is the whole point of the
/// second half, and the reason this is worth a function of its own.
///
/// `Vary: Origin` is set EXACTLY WHEN the origin is reflected: setting it unconditionally would tell
/// every shared cache that this response varies by origin while the header is absent for the
/// (common) refusal case, and a cache that believed it could serve one origin's response to another.
pub fn cors_headers(origin: &str, request_host: &str) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = vec![
        (
            "access-control-allow-methods",
            "GET,POST,OPTIONS".to_string(),
        ),
        ("access-control-allow-headers", "*".to_string()),
    ];
    if ALLOWED_ORIGINS.contains(&origin)
        || (is_loopback_origin(origin) && is_loopback_host(request_host))
    {
        out.push(("access-control-allow-origin", origin.to_string()));
        out.push(("vary", "Origin".to_string()));
    }
    out
}

/// `redactSecrets(text, secrets)` — replace every secret with `***`, LONGEST FIRST.
///
/// The rules, in the order they matter:
///  * a secret shorter than 8 characters is ignored, and a NON-STRING is ignored even when it has a
///    length (a number `12345678` passes `.length >= 8` in JavaScript and must still be left alone —
///    it is not a string, and `split` on it would not do what a reader expects);
///  * duplicates are removed, so the list the sort sees has one entry per secret;
///  * **the sort is by length DESCENDING and it is stable**, so two secrets of the same length are
///    masked in the order they were given. Ascending would mask the tail of a longer secret and
///    leave `*******ret` where the caller wanted `***`;
///  * the replacement is non-overlapping and applied once per secret, which is what `split`/`join`
///    means and is NOT what a repeated `replace` loop would mean.
pub fn redact_secrets(text: &str, secrets: &[&str]) -> String {
    // The dedup and the length filter, in ONE pass — `distinct` is a set, so the first occurrence
    // wins, and the first occurrence is also the position that survives the stable sort.
    let mut distinct: Vec<&str> = Vec::new();
    for s in secrets {
        if s.chars().count() >= 8 && !distinct.contains(s) {
            distinct.push(s);
        }
    }
    // DESCENDING BY LENGTH, STABLE WITHIN A LENGTH. `sort_by` in Rust is stable, so the explicit tie
    // break is unnecessary — and adding one would CHANGE the order for equal lengths, which is the
    // one case the JavaScript's stable `Array.prototype.sort` fixes by insertion.
    // `sort_by_key` is what clippy suggests here and it is WRONG: it would sort ASCENDING, and the
    // whole rule is descending. `Reverse(len)` is the shape that says "descending" out loud, so the
    // next reader does not have to re-derive the direction from a `cmp` chain.
    distinct.sort_by_key(|s| std::cmp::Reverse(s.chars().count()));
    let mut out = text.to_string();
    for s in distinct {
        out = out.split(s).collect::<Vec<_>>().join("***");
    }
    out
}

/// `jsonError(status, message, type)` — the body, as a `(status, JSON)` pair.
///
/// **THE BODY IS `{"type":"error","error":{"type":…,"message":…}}` AND THAT NESTING IS THE CONTRACT**
///: the messages flow on this worker reads it back. The status is a parameter and the cors headers
/// ride alongside, so the caller owns them.
pub fn json_error_body(status: u16, message: &str, kind: &str) -> (u16, String) {
    (
        status,
        format!(
            r#"{{"type":"error","error":{{"type":{},"message":{}}}}}"#,
            json_string(kind),
            json_string(message)
        ),
    )
}

/// A JSON string literal, by way of `serde_json` — **and not by hand**, because the two failure modes
/// of a hand-rolled escaper are an unescaped quote (a broken body) and a missed control character (a
/// body that is not JSON at all), and the second is invisible until a client parses it.
fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

// ───────────────────────────── the entry point's pure half ────────────────────────────────────────
//
// Everything above is a rule about a REQUEST. What follows is a rule about WHICH rule applies — the
// dispatch, the two gates, and the header pick — and it is here for the same reason the manifest and
// the CDN route table were: **the decision is portable, the I/O is not, and a decision is what a
// transliteration gets wrong.**

/// Which of this worker's four answers a request is for.
///
/// **THE ORDER IS THE ROUTING**, and three of the four rules are not what a reader would guess:
///
///  * `OPTIONS` IS ITS OWN ANSWER AND IT COMES FIRST**, before any path is looked at, so a preflight
///    never reaches a gate and never needs a credential.
///  * **THE TWO GATED ROUTES ARE `endsWith`, NOT EQUALS.** `/v1/messages` matches a path that ENDS
///    with it, so a deployment that mounts the worker under a prefix keeps working. The `/v1/models`
///    arm is looser still — it is `endsWith("/models")` on its own.
///  * **EVERYTHING ELSE IS A 404, AND THE 404 IS REACHED BEFORE THE KEY IS READ.** The worker is
///    default-closed twice over: an unknown path is refused, and a known path with no
///    `CLIENT_KEY` configured is refused rather than falling through to the paid upstream key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// `OPTIONS *` — the CORS preflight, answered before anything else.
    Preflight,
    /// `GET …/models` — the model list, gated on `x-api-key`.
    Models,
    /// `POST …/v1/responses` — the BYOK pass-through, gated on the CALLER's own `Bearer` key.
    Responses,
    /// `POST …/v1/messages` — the native Anthropic pass-through, gated on `x-api-key`.
    Messages,
    /// Anything else.
    NotFound,
}

/// `route(method, pathname)` — the whole table, in the worker's own order.
pub fn route(method: &str, pathname: &str) -> Route {
    if method == "OPTIONS" {
        return Route::Preflight;
    }
    if method == "GET" && pathname.ends_with("/models") {
        return Route::Models;
    }
    if method == "POST" && pathname.ends_with(RESPONSES_PATH) {
        return Route::Responses;
    }
    // The LAST arm is the `/v1/messages` one, and it is written as a NEGATION in the JavaScript —
    // `if (!(POST && endsWith(VERIFY_PATH))) return 404` — which means a request that is NOT a
    // messages POST is a 404 and a request that IS one continues. The same decision, phrased as the
    // table's final arm, because a positive arm reads as a list and a negation reads as a trap.
    if !(method == "POST" && pathname.ends_with(VERIFY_PATH)) {
        return Route::NotFound;
    }
    Route::Messages
}

impl Route {
    /// The path this route answers, for the tests that read the table rather than the branches.
    pub fn path(self) -> &'static str {
        match self {
            Route::Preflight => "OPTIONS *",
            Route::Models => "GET …/models",
            Route::Responses => "POST …/v1/responses",
            Route::Messages => "POST …/v1/messages",
            Route::NotFound => "(everything else)",
        }
    }
}

/// `route()` as the STATUS it answers with when the body is empty, which is only the preflight.
///
/// A preflight is `new Response(null, { headers })` — **status 200 with NO body**, and not 204: a
/// browser accepts both, and the deployed worker sends 200 because that is what `new Response(null)`
/// does by default. The port states the number instead of leaving it to a constructor's default,
/// because a default is a thing a reader cannot see.
pub fn preflight_status() -> u16 {
    200
}

/// The four caller headers this worker forwards as `x-opencode-session`, **in preference order**.
///
/// The first one that is present AND non-blank wins, and the value is TRIMMED before it is sent — so a
/// header of three spaces is treated as absent rather than forwarded as a session id of nothing.
pub const SESSION_SOURCE_HEADERS: [&str; 4] = [
    "x-opencode-session",
    "x-client-request-id",
    "session_id",
    "x-session-id",
];

/// `sessionHeader(request)` — which caller's header becomes `x-opencode-session`, if any.
///
/// **THIS IS AN ORDER, AND THE ORDER IS THE FEATURE**: the loop returns the FIRST non-blank value, so
/// a request carrying all four forwards the first and drops the rest. The test that matters is one
/// where two are present and the later one is the more specific — the first still wins.
pub fn session_header(present: &[(&str, &str)]) -> Option<(&'static str, String)> {
    for name in SESSION_SOURCE_HEADERS {
        // The lookup is by HEADER NAME, case-insensitively, because HTTP header names are
        // case-insensitive and a JavaScript `Headers.get` is too — so a caller sending
        // `X-OpenCode-Session` is found by the first entry.
        let value = present
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim())
            .unwrap_or_default();
        if !value.is_empty() {
            return Some(("x-opencode-session", value.to_string()));
        }
    }
    None
}

/// The `x-api-key` gate, as a DECISION over the three things it reads.
///
/// **DEFAULT-CLOSED, AND THAT IS THE WHOLE RULE.** `!env.CLIENT_KEY || !(await safeEq(...))` refuses
/// when the secret is UNSET as well as when it does not match — so a worker deployed without its secret
/// refuses everything rather than falling through to the paid `OPENCODE_GO_API_KEY`. The refusal is
/// also the same for both cases, so a caller cannot tell "no key configured" from "wrong key", which is
/// the point: a 401 that distinguishes them tells a prober which half of the pair it got right.
///
/// The comparison itself is [`safe_eq`], and it is CONSTANT-TIME for the reason the comment there
/// gives — so this function is a decision about a boolean, not about a string.
pub fn x_api_key_allows(configured: Option<&str>, presented: &str) -> bool {
    match configured {
        None => false,
        Some(expected) => {
            // AN EMPTY CONFIGURED KEY IS NOT A KEY. `!env.CLIENT_KEY` is a truthiness test, so a secret
            // that was set to "" refuses everything — and this is a case a `Some("")` would wave
            // through if the emptiness were checked with `== ""` and not with truthiness.
            if expected.is_empty() {
                return false;
            }
            safe_eq(presented, expected)
        }
    }
}

/// The `/v1/responses` gate: the CALLER's own key, out of a `Bearer` header.
///
/// **THE PREFIX IS EXACT AND THE SLICE IS FIXED.** `auth.startsWith("Bearer ")` then
/// `auth.slice(7).trim()` — a header of `bearer x` (lower-case) is NOT a Bearer header, because
/// `startsWith` is case-sensitive, so it is refused; and the slice is seven characters, so
/// `Bearer  x` (two spaces) leaves one leading space that the trim removes.
///
/// **THERE IS NO `CLIENT_KEY` ON THIS ROUTE AND THAT IS DELIBERATE**: the caller carries its own zen
/// key, this worker never substitutes its paid one, so a blank key is a 401 and nothing else.
pub fn bearer_key_allows(authorization: &str) -> bool {
    let auth = authorization.trim();
    match auth.strip_prefix("Bearer ") {
        // `slice(7).trim()` — the rest, trimmed. A header of exactly `Bearer ` leaves an empty
        // string, which is a refusal.
        Some(rest) => !rest.trim().is_empty(),
        None => false,
    }
}

/// The key `/v1/responses` forwards, which is the SAME parse with the value rather than the verdict.
pub fn bearer_key(authorization: &str) -> Option<String> {
    let auth = authorization.trim();
    auth.strip_prefix("Bearer ")
        .map(|rest| rest.trim().to_string())
        .filter(|k| !k.is_empty())
}

/// `safeEq(a, b)` — SHA-256 both sides, then fold the XOR across every byte WITHOUT short-circuiting.
///
/// **TWO RULES, AND BOTH ARE TIMING RULES.** No length early-exit (the digests are always 32 bytes,
/// so there is no length to leak), and the fold accumulates `diff |= a[i] ^ b[i]` over all 32 bytes
/// rather than returning at the first difference. A port that wrote `if a[i] != b[i] { return false }`
/// would be a correct function with a leak, and the leak is the point of the function.
///
/// Two digests of different inputs are compared for EQUALITY, so the answer is the same as `==`; the
/// constant time is the property, and `==` does not have it.
pub fn safe_eq(a: &str, b: &str) -> bool {
    use sha2::{Digest, Sha256};
    let da = Sha256::digest(a.as_bytes());
    let db = Sha256::digest(b.as_bytes());
    let mut diff: u8 = 0;
    for i in 0..32 {
        diff |= da[i] ^ db[i];
    }
    diff == 0
}

/// `HEADER_TIMEOUT_MS` — the budget the shipping worker gives the UPSTREAM's response HEADERS, and nothing
/// else. `src/index.js:64`: `const HEADER_TIMEOUT_MS = 30000;`. It lives here rather than in `worker.rs`
/// because it is a number the JavaScript states and the port has to agree with, and because `worker.rs` is
/// wasm32-only — a constant nothing can test on the host is a constant that drifts.
pub const HEADER_TIMEOUT_MS: u64 = 30000;

/// **`relayUpstreamError`'s DECISION, WHICH IS THE PART OF IT THAT IS NOT I/O.**
///
/// The shipping worker does:
///
/// ```text
///     let message = `Upstream ${upstream.status}`;
///     try { const err = await upstream.json(); message = err.error?.message || err.message || message; } catch {}
///     return jsonError(upstream.status, redactSecrets(message, secrets), "api_error", cors);
/// ```
///
/// **AND ITS OWN COMMENT SAYS WHY IT MATTERS**: "this is the ONE place a provider's own words reach the
/// client on this worker, so it is where the credential must die". The redaction is the caller's (it is
/// already ported above); what is decided here is WHICH words, and the `||` chain has three traps that a
/// translation can walk into:
///
///   * **`err.error?.message` IS OPTIONAL CHAINING, NOT A CAST.** When `error` is a STRING — which
///     providers do — `.message` is `undefined` and the chain falls through to `err.message`.
///   * **`||` IS TRUTHINESS, NOT NULLISHNESS.** An EMPTY message falls through; so does `0`, so does
///     `false`. Only a truthy one wins.
///   * **A BODY THAT IS NOT JSON AT ALL** leaves the default, because `upstream.json()` throws and the
///     `catch {}` is silent.
///
/// The body is passed as TEXT rather than as a parsed value because that is what the fetch hands back —
/// and because a caller that has already consumed the stream cannot re-read it.
pub fn upstream_message(status: u16, body: &str) -> String {
    let fallback = format!("Upstream {status}");
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) else {
        return fallback;
    };
    // `err.error?.message` — an object's `message`, and nothing else. A string `error` has no `.message`.
    if let Some(m) = parsed
        .get("error")
        .and_then(|e| e.get("message"))
        .filter(|m| js_truthy(m))
        .and_then(|m| m.as_str())
    {
        return m.to_string();
    }
    if let Some(m) = parsed.get("message").filter(|m| js_truthy(m)) {
        // `||` picks the VALUE, and the value reaches `redactSecrets`, whose first act is
        // `String(text == null ? "" : text)` — so a non-string that is truthy arrives as its JS text.
        if let Some(s) = m.as_str() {
            return s.to_string();
        }
        return js_text(m);
    }
    fallback
}

/// `if (x)` in JavaScript: `false`, `0`, `""`, `null`, `undefined` and `NaN` are falsy, and everything
/// else — **including an empty object or array** — is truthy.
fn js_truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => true,
    }
}

/// `String(value)` for the values `JSON.parse` can produce. `serde_json`'s own `to_string` is NOT this:
/// it quotes a string (`"x"` where JavaScript gives `x`) and writes `null` for a null.
fn js_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

/// **THE I/O HALF, wasm32-ONLY, AND `index/worker` CARRIES THE SAME PARAGRAPH.** `#[event(fetch)]`
/// expands to nothing on the host and PANICS there, because there is no Worker runtime to register with.
/// So every decision above builds and tests under a plain `cargo test`, and the entrypoint compiles only
/// for the target it serves. **A host shim would be a second copy of the dispatch that no test runs.**
#[cfg(all(target_arch = "wasm32", feature = "worker"))]
pub mod worker;

#[cfg(test)]
mod upstream_message_tests {
    //! **THE CORPUS IS THE SHIPPING EXPRESSION'S OWN OUTPUT**, produced by running
    //! `err.error?.message || err.message || \`Upstream 502\`` followed by
    //! `String(text == null ? "" : text)` in Node over these twelve bodies. **The two lines are the
    //! oracle because they ARE the shipping code** — the extraction is not reimplemented here, it is run.
    use super::*;

    const CORPUS: [(&str, &str); 12] = [
        (r#"{"error":{"message":"bad key"}}"#, "bad key"),
        (r#"{"message":"rate limited"}"#, "rate limited"),
        // `error` is a STRING: `.message` on it is `undefined`, and there is no `err.message` to fall to.
        (r#"{"error":"a string"}"#, "Upstream 502"),
        // AN EMPTY MESSAGE IS FALSY, so `||` walks past it.
        (r#"{"error":{"message":""}}"#, "Upstream 502"),
        (r#"{"error":{"message":0}}"#, "Upstream 502"),
        // A TRUTHY NON-STRING WINS, and `String()` turns it into its JavaScript text.
        (r#"{"message":123}"#, "123"),
        (r#"{"error":{"message":null},"message":"second"}"#, "second"),
        ("not json", "Upstream 502"),
        ("{}", "Upstream 502"),
        (r#"{"error":{}}"#, "Upstream 502"),
        (r#"{"error":{"message":false},"message":"after"}"#, "after"),
        (r#"{"message":null}"#, "Upstream 502"),
    ];

    #[test]
    fn the_message_matches_the_shipping_expression() {
        let mut distinct = std::collections::HashSet::new();
        for (body, want) in CORPUS {
            let got = upstream_message(502, body);
            assert_eq!(got, want, "body: {body}");
            distinct.insert(got);
        }
        // **A FLOOR AGAINST A VACUOUS CORPUS**: a function that always answered the fallback would pass
        // seven of these twelve. The shipping expression produces at least four distinct answers here.
        assert!(
            distinct.len() >= 4,
            "only {} distinct messages — the corpus stopped distinguishing anything",
            distinct.len()
        );
    }

    #[test]
    fn the_status_is_in_the_fallback_and_nowhere_else() {
        assert_eq!(upstream_message(429, "{}"), "Upstream 429");
        assert_eq!(upstream_message(500, r#"{"message":"x"}"#), "x");
    }
}

#[cfg(test)]
// THE NAMES SHOUT WHERE THE BEHAVIOUR IS THE OPPOSITE OF WHAT A READER EXPECTS — the two loopback
// rules, and the length sort. The capitals are the note; see the other crates' tests for the same.
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_secret_is_masked_FIRST_and_the_shorter_one_never_leaks_its_tail() {
        // "supersecretvalue" CONTAINS "secretval" — masking the short one first would leave
        // `super***ue` where the caller wanted the whole thing gone.
        let out = redact_secrets(
            "token=supersecretvalue;",
            &["secretval", "supersecretvalue"],
        );
        assert_eq!(out, "token=***;", "longest first: {out}");
        // AND THE OTHER ORDER, to show the sort is doing the work and not the input order.
        assert_eq!(
            out,
            redact_secrets(
                "token=supersecretvalue;",
                &["supersecretvalue", "secretval"]
            )
        );
    }

    #[test]
    fn a_secret_shorter_than_EIGHT_characters_is_IGNORED() {
        // `short` is five characters: masking a one-character secret would shred the text, and the
        // threshold is the caller's decision rather than this function's. "seven77" is seven, and
        // "1234567a" is eight — so the boundary is exercised on both sides.
        assert_eq!(
            redact_secrets("a-short and seven77 here", &["short"]),
            "a-short and seven77 here"
        );
        assert_eq!(
            redact_secrets("pin 1234567a here", &["1234567a"]),
            "pin *** here"
        );
        // **AND THE NON-STRING RULE IS NOT HERE BECAUSE THIS API CANNOT EXPRESS IT.** The
        // JavaScript's filter also refuses a number `12345678`, which has a length and is not a
        // string; `&[&str]` makes that unrepresentable, so a Rust test could only "prove" it by
        // passing the digits as a string — which is a DIFFERENT input and would assert the opposite.
        // That case is proven where the input can actually be a number: `redact-diff.mjs`.
    }

    #[test]
    fn the_replacement_is_NON_OVERLAPPING_and_applied_once_per_secret() {
        // Two occurrences in one string: `split`/`join` replaces both and does not rescan its own
        // output, which is why `***` cannot grow into itself.
        // "sekret12" is EIGHT characters — "secret" is six and is ignored by the threshold, which the
        // first version of this assertion got wrong by using a six-character secret and expecting it
        // to be masked. That is the third time in two rounds that the JavaScript was the authority
        // and the Rust test was the thing that was wrong.
        // The text BETWEEN the two occurrences survives: `***XX***`, and not `*****` — the first
        // expectation dropped the `XX` and the assertion was checking a string the function can
        // never produce. A test written from memory rather than from a run is a test that lies.
        assert_eq!(
            redact_secrets("sekret12XXsekret12", &["sekret12"]),
            "***XX***"
        );
        // A secret that is a substring of ANOTHER is only fully masked when the longer one is first,
        // which is the sort's job and is asserted in the test above.
        // The secret has to BE IN THE TEXT and BE AT LEAST EIGHT CHARACTERS, and the first version of
        // this line satisfied one and not the other — "abcd1234" is eight characters and occurs
        // nowhere in "aaaa1234aaaa", so the honest answer was the input unchanged.
        assert_eq!(redact_secrets("xx1234AAAAxx", &["1234AAAA"]), "xx***xx");
        // And the eight-character boundary again, from the other side: seven is ignored, eight is not.
        assert_eq!(redact_secrets("1234567", &["1234567"]), "1234567");
        assert_eq!(redact_secrets("12345678", &["12345678"]), "***");
        // Seven is unchanged and eight is masked, in one string, so the boundary is visible side
        // by side: "1234567" is seven characters and stays; "12345678" is eight and does not.
        assert_eq!(
            redact_secrets("1234567 and 12345678", &["1234567", "12345678"]),
            "1234567 and ***"
        );
    }

    #[test]
    fn the_loopback_rule_is_THIS_worker_s_and_not_the_landing_s() {
        // **`[::1]` IS NOT LOOPBACK HERE**, and that is the drift from the landing's rule recorded in
        // the module header. A test that "fixes" this to match the other worker is a behaviour change
        // to a CORS decision, and this file is the evidence for what the code does today.
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(!is_loopback_host("[::1]"));
        assert!(!is_loopback_host("::1"));
        assert!(!is_loopback_host("localhost.evil.com"));
        assert!(!is_loopback_host(""));
    }

    #[test]
    fn the_ROUTE_TABLE_is_the_workers_own_order_and_a_path_that_ALMOST_matches_is_a_404() {
        assert_eq!(
            route("OPTIONS", "/anything/at/all"),
            Route::Preflight,
            "a preflight is asked before any path is"
        );
        assert_eq!(route("GET", "/v1/models"), Route::Models);
        assert_eq!(route("POST", "/v1/responses"), Route::Responses);
        assert_eq!(route("POST", "/v1/messages"), Route::Messages);
        // **`endsWith`, NOT EQUALS** — a deployment that mounts the worker under a prefix keeps
        // working, which is why every arm is a suffix test and not a path equality.
        assert_eq!(route("POST", "/gw/proxy/v1/messages"), Route::Messages);
        assert_eq!(route("GET", "/some/other/models"), Route::Models);
        // AND THE NEAR MISSES ARE 404s, NOT THE PAGE OR A GATE.
        for (method, path) in [
            ("GET", "/v1/messages"), // the right path, the wrong method
            ("POST", "/v1/models"),  // ditto
            ("PUT", "/v1/messages"),
            ("DELETE", "/v1/responses"),
            ("POST", "/v1/response"), // one character short
            ("POST", "/v1/messages2"),
            ("GET", "/"),
            ("POST", "/v1/"),
            ("get", "/v1/models"), // **METHODS ARE CASE-SENSITIVE** here, and that is the
                                   // JavaScript's `===`; a lower-case `get` is a 404.
        ] {
            assert_eq!(route(method, path), Route::NotFound, "{method} {path}");
        }
    }

    #[test]
    fn the_X_API_KEY_gate_is_DEFAULT_CLOSED_and_an_EMPTY_secret_refuses_EVERYthing() {
        // **NO CONFIGURED SECRET IS A REFUSAL, NOT AN OPEN DOOR.** `!env.CLIENT_KEY || !(await safeEq)`
        // means a worker deployed without its secret answers 401 to everything rather than falling
        // through to the paid upstream key — and the same 401 for both failures, so a caller cannot
        // tell "not configured" from "wrong key".
        assert!(!x_api_key_allows(None, "anything"));
        // AN EMPTY CONFIGURED KEY IS NOT A KEY: `!env.CLIENT_KEY` is truthiness, so `Some("")` must
        // refuse too, and a `== ""` check would have waved it through.
        assert!(!x_api_key_allows(Some(""), "anything"));
        assert!(x_api_key_allows(Some("k-12345678"), "k-12345678"));
        assert!(!x_api_key_allows(Some("k-12345678"), "k-12345679"));
        assert!(!x_api_key_allows(Some("k-12345678"), ""));
    }

    #[test]
    fn the_BEARER_prefix_is_CASE_SENSITIVE_and_the_slice_is_seven_characters() {
        // **`startsWith("Bearer ")` IS CASE-SENSITIVE**, so a lower-case `bearer` is not a Bearer
        // header and is refused — a port that lowercased the comparison would accept one.
        assert!(bearer_key_allows("Bearer sk-123"));
        assert!(!bearer_key_allows("bearer sk-123"));
        assert!(!bearer_key_allows("BEARER sk-123"));
        // `slice(7).trim()` — two spaces leave one, and the trim takes it.
        assert_eq!(bearer_key("Bearer  sk-123").as_deref(), Some("sk-123"));
        assert_eq!(bearer_key("  Bearer sk-123  ").as_deref(), Some("sk-123"));
        // A header of exactly `Bearer ` leaves nothing, which is a refusal.
        assert!(!bearer_key_allows("Bearer "));
        assert!(!bearer_key_allows("Bearer"));
        assert!(!bearer_key_allows(""));
        assert!(!bearer_key_allows("Basic sk-123"));
    }

    #[test]
    fn the_SESSION_header_is_an_ORDER_and_the_FIRST_non_blank_wins() {
        // **THE FIRST PRESENT WINS, NOT THE MOST SPECIFIC** — the order is the feature, and a request
        // carrying all four forwards the first and drops the rest.
        let all: &[(&str, &str)] = &[
            ("x-opencode-session", "one"),
            ("x-client-request-id", "two"),
            ("session_id", "three"),
            ("x-session-id", "four"),
        ];
        assert_eq!(
            session_header(all),
            Some(("x-opencode-session", "one".into()))
        );
        // A BLANK FIRST HEADER IS TREATED AS ABSENT, so the second wins — the value is TRIMMED before
        // the emptiness test, which is why three spaces do not forward as a session id of nothing.
        let blank_first: &[(&str, &str)] = &[("x-opencode-session", "   "), ("session_id", "real")];
        assert_eq!(
            session_header(blank_first),
            Some(("x-opencode-session", "real".into())),
            "the forwarded VALUE is the caller's own, under the first name that had one"
        );
        // **HEADER NAMES ARE CASE-INSENSITIVE**, so `X-OpenCode-Session` is found by the first entry.
        let shouty: &[(&str, &str)] = &[("X-OpenCode-Session", "s")];
        assert_eq!(
            session_header(shouty),
            Some(("x-opencode-session", "s".into()))
        );
        // AND NONE OF THEM PRESENT IS NO HEADER AT ALL, which is what makes the spread add nothing.
        assert_eq!(session_header(&[]), None);
        assert_eq!(session_header(&[("x-request-id", "x")]), None);
    }

    #[test]
    fn safe_eq_is_a_COMPARISON_of_DIGESTS_and_the_answer_agrees_with_equality() {
        assert!(safe_eq("same", "same"));
        assert!(!safe_eq("a", "b"));
        assert!(!safe_eq("", "x"));
        assert!(safe_eq("", ""));
        // A DIFFERENT LENGTH IS STILL COMPARED BY DIGEST, so a prefix of the key is not a match — the
        // no-early-exit rule means there is no length to leak, and this is the consequence.
        assert!(!safe_eq("k-12345678", "k-12345678-extra"));
        // A UNICODE KEY COMPARES BY ITS BYTES, which is what `TextEncoder` does in the JavaScript.
        assert!(safe_eq("键-12345678", "键-12345678"));
        assert!(!safe_eq("键-12345678", "键-12345679"));
    }

    #[test]
    fn a_loopback_ORIGIN_is_reflected_only_when_the_REQUEST_HOST_is_loopback_too() {
        let reflected = |origin, host| {
            cors_headers(origin, host)
                .iter()
                .any(|(k, v)| *k == "access-control-allow-origin" && v == origin)
        };
        assert!(reflected("https://ai.saisi.online", "zen-us.saisi.online"));
        // The deployed proxy, asked by a foreign page's loopback origin: NOT reflected.
        assert!(!reflected("http://localhost:5173", "zen-us.saisi.online"));
        // The same origin against a loopback request host: reflected, which is the local-dev affordance.
        // **THE HOST IS A HOSTNAME AND NOT A `host:port` PAIR**, because `requestHost` is
        // `new URL(request.url).hostname` — the first version of this test passed "127.0.0.1:8787" and
        // the reflection did not happen, which is a test that was wrong about JavaScript and not a bug.
        assert!(reflected("http://localhost:5173", "127.0.0.1"));
        assert!(!reflected("http://localhost:5173", "127.0.0.1:8787"));
        // A scheme that is not http(s) is not a loopback origin however local the host is.
        assert!(!reflected("file:///tmp/x", "127.0.0.1:8787"));
        assert!(!reflected("garbage", "127.0.0.1:8787"));
    }

    #[test]
    fn vary_is_set_EXACTLY_WHEN_the_origin_is_reflected() {
        let has_vary = |origin, host| cors_headers(origin, host).iter().any(|(k, _)| *k == "vary");
        assert!(has_vary("https://api.saisi.online", "zen-us.saisi.online"));
        assert!(!has_vary("https://evil.example", "zen-us.saisi.online"));
    }
}
