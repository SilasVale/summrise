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
