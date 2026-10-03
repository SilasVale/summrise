//! THE PLUGIN'S RESPONSE BUILDERS, MOVED FROM `gateway/src/plugins/translate.ts` AND `gateway/src/http.ts`.
//!
//! WHY THESE. `translate.ts` is 1,696 lines and most of it reads keys, channels, KV and upstreams — but
//! the SHAPES it hands back to a client are pure: a status, a header set and a body, decided from a kind
//! string or a provider record. They are also the shapes a client reads when something is WRONG, which is
//! where a drift is least visible and most expensive.
//!
//! THEY RETURN DATA, NOT A `Response`, and that is the same discipline the panel's `badge_icon` carries:
//! the pure half decides WHAT the bytes are, the surface decides how to send them. It also makes the
//! oracle's comparison exact — status, headers and body, with no runtime in between to normalise them.

/// A response's pure half: what the surface will send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    pub status: u16,
    /// In the order the JavaScript object lists them. **THE WIRE ORDER IS THE RUNTIME'S** — the Fetch
    /// `Headers` object sorts by name and lowercases — so a comparison sorts both sides rather than
    /// pretending an order that neither implementation promises.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// `http.ts`'s `CORS_HEADERS`, verbatim — two entries, in the source's order.
pub fn cors_headers() -> Vec<(String, String)> {
    vec![
        (
            "Access-Control-Allow-Methods".to_string(),
            "GET,POST,OPTIONS,DELETE,PUT".to_string(),
        ),
        ("Access-Control-Allow-Headers".to_string(), "*".to_string()),
    ]
}

/// `jsonError(status, message, type)` — `http.ts:173`, the one place a JSON error's shape lives.
pub fn json_error(status: u16, message: &str, kind: &str) -> Built {
    let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    headers.extend(cors_headers());
    Built {
        status,
        headers,
        // `JSON.stringify({ type: "error", error: { type, message } })` — key order included.
        body: format!(
            r#"{{"type":"error","error":{{"type":{},"message":{}}}}}"#,
            serde_json::to_string(kind).expect("a string"),
            serde_json::to_string(message).expect("a string"),
        ),
    }
}

/// `sseResponse(body)` — the one place the SSE passthrough's header set lives.
pub fn sse_response(body: &str) -> Built {
    let mut headers = vec![
        (
            "Content-Type".to_string(),
            "text/event-stream; charset=utf-8".to_string(),
        ),
        ("Cache-Control".to_string(), "no-cache".to_string()),
    ];
    headers.extend(cors_headers());
    Built {
        status: 200,
        headers,
        body: body.to_string(),
    }
}

/// The nine messages `keyMissingError` owns, IN THE TABLE'S OWN ORDER — and the order is the reason this
/// is a slice rather than a map: the table is a `Record`, whose keys are insertion-ordered, and a reader
/// comparing the two implementations should see them in the same sequence.
const KEY_MISSING_MESSAGES: [(&str, &str); 9] = [
    (
        "deepseek",
        "DEEPSEEK_API_KEY not configured — add your own key in the console",
    ),
    (
        "opencode",
        "OPENCODE_GO_API_KEY not configured — add your own key in the console",
    ),
    (
        "openrouter",
        "OPENROUTER_API_KEY not configured — add your own key in the console",
    ),
    (
        "qwen",
        "QWEN_API_KEY not configured — add your own key in the console",
    ),
    (
        "nvidia",
        "NVAPI_KEY not configured — add your NVIDIA build.nvidia.com key",
    ),
    (
        "gmi",
        "GMI_API_KEY not configured — add your GMI Cloud key in the console",
    ),
    (
        "amd",
        "AMD_API_KEY not configured — add your AMD Radeon Cloud (rc-…) key in the console",
    ),
    (
        "commandgoat",
        "CMD_API_KEY not configured — add your Command Code key in the console",
    ),
    (
        "r4",
        "R4_API_KEY not configured — add your own r4.codes key in the console",
    ),
];

/// `keyMissingError(kind)`: the 502 for a route kind, or `None` when the kind needs no key.
///
/// **`KEY_MISSING_MESSAGES[kind]` IS A PROPERTY LOOKUP, NOT A MAP LOOKUP**, and the difference is a
/// prototype chain: a kind of `"constructor"` or `"toString"` finds something on `Object.prototype` and
/// the JavaScript would answer a 502 whose message is a FUNCTION SOURCE. The oracle has that case; this
/// answers `None` for it, which is what a table with no prototype does. It is recorded here rather than
/// silently matched, because "the JavaScript is wrong here" is a fact a reader needs.
pub fn key_missing_error(kind: &str) -> Option<Built> {
    KEY_MISSING_MESSAGES
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, msg)| json_error(502, msg, "config_error"))
}

/// `providerKeyMissingError(provider)`: the 502 for a CUSTOM provider, which names the provider and the
/// exact thing an operator has to set.
///
/// `provider?.prefix || "custom provider"` and `provider?.apiKeyEnv ? … : …` are JS TRUTHINESS on an
/// optional record: an absent provider, an empty prefix and an empty env name all take the fallback.
pub fn provider_key_missing_error(provider: Option<&serde_json::Value>) -> Built {
    let field = |name: &str| -> String {
        provider
            .and_then(|p| p.get(name))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let prefix = field("prefix");
    let name = if prefix.is_empty() {
        "custom provider".to_string()
    } else {
        prefix
    };
    let env = field("apiKeyEnv");
    let hint = if env.is_empty() {
        "the provider record carries no key — re-register it with apiKey or apiKeyEnv".to_string()
    } else {
        format!(
            "{env} is not set in this deployment — add the Worker secret, or re-register the provider with an inline apiKey"
        )
    };
    json_error(
        502,
        &format!("{name}: no provider key — {hint}"),
        "config_error",
    )
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `built` cases were produced by the SHIPPING TypeScript
    //! (`gateway/wasm/oracle-translate.mjs`), and this replays them. Header ORDER is compared as a
    //! SORTED map: the Fetch `Headers` object sorts by name and lowercases, so neither implementation
    //! promises the order its source lists.
    use super::*;

    fn corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/translate-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    fn sorted(headers: &[(String, String)]) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = headers
            .iter()
            .map(|(k, v)| (k.to_lowercase(), v.clone()))
            .collect();
        out.sort();
        out
    }

    fn check(case: &serde_json::Value, built: Option<Built>) {
        let func = case["fn"].as_str().unwrap_or("?");
        let name = case["name"].as_str().unwrap_or("?");
        let expected = &case["expected"];
        // **A RECORDED DIVERGENCE IS ASSERTED, NOT IGNORED.** `KEY_MISSING_MESSAGES[kind]` is a property
        // lookup in JavaScript, so a kind of `constructor` or `toString` finds something on
        // `Object.prototype` and the shipping worker answers a 502 whose message is a FUNCTION'S SOURCE.
        // A Rust table has no prototype. This asserts the difference is exactly that — the port answers
        // null — rather than letting the case pass unnoticed or reproducing an accident as behaviour.
        if let Some(kind) = case.get("divergence").and_then(|v| v.as_str()) {
            assert_eq!(kind, "prototype", "an unknown divergence marker");
            assert!(
                built.is_none(),
                "{func} / {name}: this must answer null where the JavaScript finds a prototype property"
            );
            return;
        }
        if expected.get("null").is_some() {
            assert!(
                built.is_none(),
                "{func} / {name}: the TypeScript answers null"
            );
            return;
        }
        let built = built.unwrap_or_else(|| panic!("{func} / {name}: this answered null"));
        assert_eq!(
            built.status,
            expected["status"].as_u64().unwrap_or(0) as u16,
            "{func} / {name}: status"
        );
        assert_eq!(
            built.body,
            expected["body"].as_str().unwrap_or(""),
            "{func} / {name}: body"
        );
        let want: Vec<(String, String)> = expected["headers"]
            .as_object()
            .expect("headers")
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
            .collect();
        assert_eq!(sorted(&built.headers), want, "{func} / {name}: headers");
    }

    #[test]
    fn every_response_builder_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let input = &case["input"];
            match func {
                "sse_response" => {
                    check(case, Some(sse_response(input.as_str().unwrap_or(""))));
                    checked += 1;
                }
                "key_missing_error" => {
                    check(case, key_missing_error(input.as_str().unwrap_or("")));
                    checked += 1;
                }
                "provider_key_missing_error" => {
                    let provider = if input.is_null() { None } else { Some(input) };
                    check(case, Some(provider_key_missing_error(provider)));
                    checked += 1;
                }
                _ => continue,
            }
        }
        assert!(
            checked >= 14,
            "the response-builder corpus shrank to {checked} cases"
        );
    }
}
