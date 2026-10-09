//! THE CORS GATE, MOVED FROM `gateway/src/http.ts`.
//!
//! WHY THIS FAMILY. Its own comment says it: "this list is a security surface (it is what a browser is
//! allowed to read answers from)". A drift here does not throw — it WIDENS OR CLOSES A DOOR, and the
//! repository already has the measurement of what that costs: audit P2, where production reflected ANY
//! localhost origin, because the loopback rule did not also ask whether the REQUEST's own host was
//! loopback. That rule is the one subtle thing in this module and the corpus carries its case.
//!
//! `url` IS ALREADY IN THE TREE — `worker` depends on it — so parsing here costs no extra bytes, and
//! `URL`'s own semantics (lowercasing, rejecting a relative string) are the ones being mirrored.

/// The DEFAULT allowed origins, verbatim and in order.
pub const DEFAULT_ALLOWED_ORIGINS: [&str; 2] =
    ["https://ai.saisi.online", "https://api.saisi.online"];

/// `allowedOrigins(env)`: the configured list when it is given AND non-empty, else the default.
///
/// **AN EMPTY OR BLANK CONFIGURATION IS NOT AN EMPTY ALLOWLIST.** `split(",")` on `""` gives `[""]`, the
/// `filter(Boolean)` removes it, and `list.length ? … : ALLOWED_ORIGINS` then answers the DEFAULT — so a
/// misconfigured deployment falls back to the known origins rather than allowing none (or all).
pub fn allowed_origins(configured: Option<&str>) -> Vec<String> {
    let Some(raw) = configured.filter(|c| !c.is_empty()) else {
        return DEFAULT_ALLOWED_ORIGINS
            .iter()
            .map(|s| s.to_string())
            .collect();
    };
    let list: Vec<String> = raw
        .split(',')
        .map(|o| o.trim().to_string())
        .filter(|o| !o.is_empty())
        .collect();
    if list.is_empty() {
        DEFAULT_ALLOWED_ORIGINS
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        list
    }
}

/// `isLoopbackOrigin(origin)`: a parseable http/https URL whose HOSTNAME is localhost or 127.0.0.1.
///
/// The protocol test is why `ftp://localhost` is not a loopback origin, and `URL`'s lowercasing is why
/// `http://LOCALHOST` is one. `http://localhost.evil.com` is not: the hostname is the whole label.
pub fn is_loopback_origin(origin: &str) -> bool {
    let Ok(u) = url::Url::parse(origin) else {
        return false;
    };
    let scheme = u.scheme();
    if scheme != "http" && scheme != "https" {
        return false;
    }
    match u.host_str() {
        Some(h) => h == "localhost" || h == "127.0.0.1",
        None => false,
    }
}

/// `isLoopbackHost(hostname)`: an EXACT, CASE-SENSITIVE match — this is handed a hostname that `URL` has
/// already lowercased, so it is a comparison rather than a normalisation.
pub fn is_loopback_host(hostname: &str) -> bool {
    hostname == "localhost" || hostname == "127.0.0.1"
}

/// `isAllowedOrigin(origin, requestHost, env)` — **AND THE SECOND HALF OF THE LOOPBACK RULE IS THE POINT.**
///
/// A loopback origin is a `wrangler dev` affordance, not a production grant: a request to
/// `http://localhost:<port>` whose Origin is ALSO loopback is local development, while the SAME Origin
/// arriving at the deployed console host is just a foreign local page and gets NO `Access-Control-Allow-
/// Origin`. Pre-fix, production reflected any localhost origin — audit P2.
pub fn is_allowed_origin(
    origin: &str,
    request_host: Option<&str>,
    configured: Option<&str>,
) -> bool {
    if origin.is_empty() {
        return false;
    }
    if allowed_origins(configured).iter().any(|o| o == origin) {
        return true;
    }
    match request_host {
        Some(host) if !host.is_empty() => is_loopback_origin(origin) && is_loopback_host(host),
        _ => false,
    }
}

/// `corsHeadersFor(origin, requestHost, env)`: reflect-if-allowlisted plus `Vary`, else the base set with
/// NO `Access-Control-Allow-Origin`.
///
/// **`Vary: Origin` RIDES WITH THE REFLECTION AND ONLY WITH IT** — a cache that stored an answer without
/// it would serve one origin's grant to another.
pub fn cors_headers_for(
    origin: &str,
    request_host: Option<&str>,
    configured: Option<&str>,
) -> Vec<(String, String)> {
    let mut headers = vec![
        (
            "Access-Control-Allow-Methods".to_string(),
            "GET,POST,OPTIONS,DELETE,PUT".to_string(),
        ),
        ("Access-Control-Allow-Headers".to_string(), "*".to_string()),
    ];
    if is_allowed_origin(origin, request_host, configured) {
        headers.push((
            "Access-Control-Allow-Origin".to_string(),
            origin.to_string(),
        ));
        headers.push(("Vary".to_string(), "Origin".to_string()));
    }
    headers
}

/// `stampCors(request, headers, env)` — set the CORS pair on an EXISTING header set, or REMOVE the origin.
///
/// **THE `else` BRANCH DELETES, AND THAT IS NOT SYMMETRIC WITH `corsHeadersFor`.** That one starts from a
/// fresh set that never had an ACAO, so "not allowed" means "do not add one". This one is handed the
/// UPSTREAM's headers, which may carry an ACAO of their own — so refusing has to take it away. A port that
/// merely skipped the `if` would forward whatever the upstream said about origins.
///
/// **AND IT TAKES THE ORIGIN ALONE — `Vary` IS NOT ITS TO DELETE.** The source's `else` is one line,
/// `headers.delete("Access-Control-Allow-Origin")` (`gateway/src/http.ts:124`); `Vary` is touched only in the
/// ALLOWED branch, where the source's `headers.set("Vary", "Origin")` REPLACES whatever the upstream sent. This
/// function removed both in every branch, which is a real difference from the console: measured on the file
/// relay's own answer, a `vary: accept-encoding` sent to a refused origin came back DROPPED here and KEPT by the
/// shipping implementation. It has no caller outside its own unit tests today — the two live stamps are
/// `lib::with_cors` and `device_proxy::stamp_cors_like_ts` — which is exactly why it went unnoticed: a helper
/// nothing calls cannot fail a corpus case, and the test beside it asserted the wrong behaviour as if it were
/// the source's. Both are corrected together.
pub fn stamp_cors(
    headers: &mut Vec<(String, String)>,
    origin: &str,
    request_host: Option<&str>,
    configured: Option<&str>,
) {
    headers.retain(|(k, _)| !k.eq_ignore_ascii_case("access-control-allow-origin"));
    if is_allowed_origin(origin, request_host, configured) {
        headers.retain(|(k, _)| !k.eq_ignore_ascii_case("vary"));
        headers.push((
            "Access-Control-Allow-Origin".to_string(),
            origin.to_string(),
        ));
        headers.push(("Vary".to_string(), "Origin".to_string()));
    }
}

/// The SUCCESS path of `relayUpstreamResult`: the upstream's status and body, its headers with the CORS pair
/// stamped, and nothing else touched.
///
/// **`new Response(upstream.body, …)` FORWARDS THE BODY UNTOUCHED** — a passthrough answer is the upstream's
/// bytes, not a reshaping of them — and the only header surgery is `stampCors`. The `x-generation-id`
/// header the source also reads goes to the LOG context rather than to the client, so it is not here.
pub fn forwarded_upstream_response(
    status: u16,
    upstream_headers: &[(String, String)],
    origin: &str,
    request_host: Option<&str>,
    configured: Option<&str>,
    body: &str,
) -> crate::responses::Built {
    let mut headers = upstream_headers.to_vec();
    stamp_cors(&mut headers, origin, request_host, configured);
    crate::responses::Built {
        status,
        headers,
        body: body.to_string(),
    }
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `cors` cases were produced by the SHIPPING TypeScript
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

    /// **A PLAIN OBJECT KEEPS ITS SPELLING; A `Headers` OBJECT DOES NOT.** `corsHeadersFor` returns a
    /// plain `Record<string, string>`, so the oracle recorded `Access-Control-Allow-Methods` as the source
    /// spells it — the opposite of `passthroughHeaders`, whose `Headers` object folds every name. Both
    /// facts are the runtime's, and a test that folded here would be comparing against a shape the
    /// JavaScript never produces.
    fn as_object(pairs: Vec<(String, String)>) -> serde_json::Map<String, serde_json::Value> {
        pairs
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect()
    }

    #[test]
    fn stamping_removes_an_origin_it_does_not_allow() {
        // **THE ASYMMETRY WITH `corsHeadersFor`, WHICH IS THE WHOLE REASON THIS FUNCTION EXISTS.** That one
        // builds a fresh set, so "not allowed" means "add nothing". This one is handed the UPSTREAM's
        // headers, which may carry an ACAO of their own — so refusing has to take it away, and a port that
        // merely skipped the `if` would forward whatever the upstream claimed about origins.
        let upstream = vec![
            ("content-type".to_string(), "application/json".to_string()),
            (
                "access-control-allow-origin".to_string(),
                "https://evil.example".to_string(),
            ),
            ("vary".to_string(), "Accept-Encoding".to_string()),
        ];
        let mut headers = upstream.clone();
        stamp_cors(
            &mut headers,
            "https://evil.example",
            Some("api.saisi.online"),
            None,
        );
        assert!(
            !headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("access-control-allow-origin")),
            "the upstream's origin must be REMOVED: {headers:?}"
        );
        // **AND ITS `Vary` STAYS — THIS ASSERTION USED TO SAY THE OPPOSITE, AND THAT IS WHY THE LIVE CODE READ
        // AS CORRECT.** The source's `else` is ONE line, `headers.delete("Access-Control-Allow-Origin")`
        // (`gateway/src/http.ts:124`); `Vary` is replaced only where the source replaces it, in the ALLOWED
        // branch below (`headers.set("Vary", "Origin")`). A test asserting that refusing takes `Vary` with it
        // made this function the authority for an asymmetry it does not have — and the function it "justified"
        // is the one the door actually uses. Measured on the file relay's own answer: a device or a relay that
        // sent `vary: accept-encoding` to a refused origin came back with it DROPPED here and KEPT by the
        // shipping console. `verify.mjs` carries the pairing now; this pins it one level down.
        assert_eq!(
            headers
                .iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("vary"))
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>(),
            vec!["Accept-Encoding"],
            "an upstream Vary is not ours to delete when we refuse the origin: {headers:?}"
        );
        assert!(
            headers.iter().any(|(k, _)| k == "content-type"),
            "the rest rides"
        );

        // An ALLOWED origin is stamped, and the upstream's value is replaced rather than duplicated.
        let mut headers = upstream.clone();
        stamp_cors(
            &mut headers,
            "https://ai.saisi.online",
            Some("api.saisi.online"),
            None,
        );
        let acao: Vec<&String> = headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("access-control-allow-origin"))
            .map(|(_, v)| v)
            .collect();
        assert_eq!(acao, vec!["https://ai.saisi.online"], "{headers:?}");
        // **AND HERE `Vary` IS REPLACED, WHICH IS THE OTHER HALF OF THE ASYMMETRY.** `Vary: Origin` is what the
        // ALLOWED branch writes, so the upstream's `Accept-Encoding` does not survive it — the two branches
        // differ in exactly this, and pinning only one of them is how the pair came to be misread.
        let vary: Vec<&String> = headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("vary"))
            .map(|(_, v)| v)
            .collect();
        assert_eq!(
            vary,
            vec!["Origin"],
            "the allowed branch REPLACES the upstream's Vary: {headers:?}"
        );
    }

    #[test]
    fn the_forwarded_response_is_the_upstreams_bytes_plus_the_cors_pair() {
        let upstream = vec![
            ("content-type".to_string(), "application/json".to_string()),
            ("x-generation-id".to_string(), "gen-1".to_string()),
        ];
        let got = forwarded_upstream_response(
            200,
            &upstream,
            "https://ai.saisi.online",
            Some("api.saisi.online"),
            None,
            "{\"ok\":true}",
        );
        assert_eq!(got.status, 200);
        assert_eq!(got.body, "{\"ok\":true}", "the body is forwarded untouched");
        // **THE KEY IS COMPARED CASE-INSENSITIVELY, AND THE VALUE KEEPS THE SOURCE'S SPELLING.** `stampCors`
        // writes `Access-Control-Allow-Origin` on a `Headers` object, which folds it; this port carries a
        // `Vec` of pairs, so the spelling is the source's — and a test that compared the folded form would be
        // asserting something the runtime does rather than something the function does.
        assert!(
            got.headers.iter().any(
                |(k, v)| k.eq_ignore_ascii_case("access-control-allow-origin")
                    && v == "https://ai.saisi.online"
            ),
            "headers were {:?}",
            got.headers
        );
        // The generation id is the LOG context's, not the client's — the source reads it into `ctx`.
        assert!(got.headers.iter().any(|(k, _)| k == "x-generation-id"));
    }

    #[test]
    fn every_cors_decision_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            let configured = input["configured"].as_str();
            let host = input["requestHost"].as_str();
            match func {
                "allowed_origins" => {
                    let got: Vec<serde_json::Value> = allowed_origins(configured)
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect();
                    assert_eq!(&serde_json::Value::Array(got), want, "{func} / {name}");
                }
                "is_loopback_origin" => {
                    let got = is_loopback_origin(input["origin"].as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::Bool(got), want, "{func} / {name}");
                }
                "is_loopback_host" => {
                    let got = is_loopback_host(input["host"].as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::Bool(got), want, "{func} / {name}");
                }
                "is_allowed_origin" => {
                    let got =
                        is_allowed_origin(input["origin"].as_str().unwrap_or(""), host, configured);
                    assert_eq!(&serde_json::Value::Bool(got), want, "{func} / {name}");
                }
                "cors_headers_for" => {
                    let got =
                        cors_headers_for(input["origin"].as_str().unwrap_or(""), host, configured);
                    assert_eq!(
                        &serde_json::Value::Object(as_object(got)),
                        want,
                        "{func} / {name}"
                    );
                }
                _ => continue,
            }
            checked += 1;
        }
        assert!(checked >= 25, "the cors corpus shrank to {checked} cases");
    }
}
