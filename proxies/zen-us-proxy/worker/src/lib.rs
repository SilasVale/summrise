//! `zen-us-proxy`'s PORTABLE HALF — the origin policy and the redaction, with no I/O anywhere in it.
//!
//! ## What is here and what is not
//!
//! `proxies/zen-us-proxy/src/index.js` is 339 lines, and the parts that touch a network or a request are
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
