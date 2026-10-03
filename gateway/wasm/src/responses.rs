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

/// `jsonOk(data, extraHeaders)` — the success shape every JSON route returns.
///
/// `JSON.stringify` writes a whole number as `30000` and a `NaN` as `null`, so the body is built from the
/// value rather than from a formatted number; the header order is `Content-Type`, then `CORS_HEADERS`, then
/// the caller's extras, which is the spread order in the source.
pub fn json_ok(data: &serde_json::Value, extra: &[(String, String)]) -> Built {
    let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    headers.extend(cors_headers());
    for (k, v) in extra {
        headers.push((k.clone(), v.clone()));
    }
    Built {
        status: 200,
        headers,
        // **`JSON.stringify(NaN)` IS `null`** — the token estimate can be NaN (an empty sample divides by
        // zero), and the wire form of that is what the corpus recorded.
        body: stringify_like_json(data),
    }
}

/// `JSON.stringify` for the shapes a route body holds: whole numbers stay whole, and a non-finite number
/// becomes `null`.
pub fn stringify_like_json(data: &serde_json::Value) -> String {
    fn norm(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Number(n) => match n.as_f64() {
                Some(f) if !f.is_finite() => serde_json::Value::Null,
                Some(f) if f.fract() == 0.0 && f.abs() < 9e15 => serde_json::json!(f as i64),
                _ => v.clone(),
            },
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(norm).collect())
            }
            serde_json::Value::Object(map) => {
                serde_json::Value::Object(map.iter().map(|(k, v)| (k.clone(), norm(v))).collect())
            }
            other => other.clone(),
        }
    }
    serde_json::to_string(&norm(data)).unwrap_or_else(|_| "null".to_string())
}

/// **WHAT HAPPENS WHEN THE UPSTREAM IGNORES `stream: true`.**
///
/// The recorded incident is in the source's own comment: "Feeding JSON into the SSE parser produced an EMPTY
/// Anthropic message — the whole answer was silently dropped." So the branch exists, and it has three
/// answers, each of which the corpus pins:
///
///   * a plain JSON completion is translated as a ONE-SHOT SSE (`sseResponse(toSSE(toAnthropicResponse(…)))`);
///   * a 200-wrapped OpenAI ERROR envelope — `{error:{…}}`, or no `choices`, or an empty `choices` — becomes
///     a **502 naming the upstream's message**, because "a silent empty assistant message" is the failure
///     this branch was written to stop;
///   * and a JSON PARSE FAILURE is its own 502, not a fall-through to the SSE translator: the body has
///     already been consumed, so falling through would "read an empty stream and fabricate an empty
///     message".
///
/// `content_type` IS AN ARGUMENT because the caller decides with it whether this branch applies at all: a
/// `text/event-stream` answer takes the streaming path instead.
pub fn stream_ignored_response(
    upstream_json: Option<&serde_json::Value>,
    upstream_model: &str,
) -> Built {
    let Some(json) = upstream_json else {
        // The parse failed AND the body was consumed — see the third answer above.
        return json_error(502, "upstream returned invalid JSON", "api_error");
    };
    let choices = json.get("choices").and_then(|c| c.as_array());
    let usable = json.get("error").is_none() && choices.map(|c| !c.is_empty()).unwrap_or(false);
    if !usable {
        // `json.error?.message || json.message || "upstream returned an error envelope"` — the first truthy
        // of three, so a bare `{error:{}}` falls through to the generic sentence.
        let message = json
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .filter(|m| !m.is_empty())
            .or_else(|| {
                json.get("message")
                    .and_then(|m| m.as_str())
                    .filter(|m| !m.is_empty())
            })
            .unwrap_or("upstream returned an error envelope");
        return json_error(502, message, "api_error");
    }
    let one_shot = crate::translate::to_sse(&crate::translate::to_anthropic_response(
        json,
        upstream_model,
    ));
    sse_response(&one_shot)
}

/// Does the upstream's content type mean "this is JSON, not a stream"?
///
/// `ctype.includes("application/json") && !ctype.includes("text/event-stream")` — BOTH halves matter: a
/// backend that answers `application/json` on a stream request ignored the flag, and one that answers
/// `text/event-stream` did not.
pub fn upstream_ignored_stream(content_type: &str) -> bool {
    content_type.contains("application/json") && !content_type.contains("text/event-stream")
}

/// **THE NON-STREAMING ANSWER — and its generic sentence is NOT the streaming branch's.**
///
/// The source has two near-twins, and the difference between them is one string:
///
/// ```text
///     the ignored-stream branch   "upstream returned an error envelope"
///     this one                    "upstream returned an invalid response"
/// ```
///
/// (THE FENCE IS NOT DECORATION: without it this block is an INDENTED CODE BLOCK, which `cargo test` tries
/// to compile as a doctest — and a table of sentences is not Rust.)
///
/// Both are reached by the same shape (`!upJson || upJson.error || choices is not a non-empty array`) and
/// both answer 502 with the upstream's own message when there is one. Conflating them would be invisible in
/// any test that only checked the status — which is why the corpus pins the BODY.
///
/// The success path is `jsonOk(toAnthropicResponse(upJson, upstreamModel))`, and both halves were ported and
/// oracle-proved long before this.
pub fn upstream_json_response(
    upstream_json: Option<&serde_json::Value>,
    upstream_model: &str,
) -> Built {
    let usable = match upstream_json {
        Some(json) => {
            let choices = json.get("choices").and_then(|c| c.as_array());
            json.get("error").is_none() && choices.map(|c| !c.is_empty()).unwrap_or(false)
        }
        None => false,
    };
    if !usable {
        let message = upstream_json
            .and_then(|json| {
                json.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .filter(|m| !m.is_empty())
                    .or_else(|| {
                        json.get("message")
                            .and_then(|m| m.as_str())
                            .filter(|m| !m.is_empty())
                    })
            })
            .unwrap_or("upstream returned an invalid response");
        return json_error(502, message, "api_error");
    }
    let json = upstream_json.expect("checked above");
    json_ok(
        &crate::translate::to_anthropic_response(json, upstream_model),
        &[],
    )
}

/// **THE `/v1/models` LISTING — the pure half, with the three store reads as arguments.**
///
/// The arm reads the advertised set, the console-added models and the facet overrides from KV, then builds a
/// document. This takes those three as values, which is the same seam every other port here uses.
///
/// THREE THINGS IN IT ARE BYTES RATHER THAN SHAPE, and the corpus pins all three:
///
///   * **`created` IS `1785000000 + i`** — a constant plus the entry's POSITION, so a model's `created`
///     changes when the list around it does. It looks like a timestamp and is an index;
///   * the disabled ids are removed from the REGISTRY's order, and the extras are APPENDED after it;
///   * and the three optional fields are `m.name || ov?.name` — JS TRUTHINESS, so an empty string falls
///     through to the override, and a `0` context window would too.
///
/// The override is a DECORATION and not an override, in the source's own words: "the registry supplies the
/// routing facets and never a display one, so this decorates rather than overrides — and a custom record's
/// own values stay authoritative where both could speak."
pub fn models_listing(
    registry: &[crate::registry::ModelSpec],
    live_ids: &[String],
    extra: &[serde_json::Value],
    overrides: &serde_json::Map<String, serde_json::Value>,
) -> Built {
    let enabled: Vec<&crate::registry::ModelSpec> = registry
        .iter()
        .filter(|m| live_ids.iter().any(|id| id == m.id))
        .collect();
    let mut data: Vec<serde_json::Value> = Vec::new();
    for (i, m) in enabled.iter().enumerate() {
        let ov = overrides.get(m.id);
        let field = |from_registry: Option<&str>, from_override: &str| -> Option<String> {
            match from_registry.filter(|v| !v.is_empty()) {
                Some(v) => Some(v.to_string()),
                None => ov
                    .and_then(|o| o.get(from_override))
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.is_empty())
                    .map(str::to_string),
            }
        };
        let mut entry = serde_json::Map::new();
        entry.insert("id".into(), serde_json::json!(m.id));
        entry.insert("object".into(), serde_json::json!("model"));
        entry.insert(
            "created".into(),
            serde_json::json!(1_785_000_000i64 + i as i64),
        );
        entry.insert("owned_by".into(), serde_json::json!(m.owned_by));
        if let Some(name) = field(None, "name") {
            entry.insert("name".into(), serde_json::json!(name));
        }
        if let Some(window) = m.context_window {
            entry.insert("context_window".into(), serde_json::json!(window));
        } else if let Some(window) = ov
            .and_then(|o| o.get("contextWindow"))
            .and_then(|v| v.as_i64())
        {
            entry.insert("context_window".into(), serde_json::json!(window));
        }
        if let Some(max) = m.max_tokens {
            entry.insert("max_tokens".into(), serde_json::json!(max));
        } else if let Some(max) = ov.and_then(|o| o.get("maxTokens")).and_then(|v| v.as_i64()) {
            entry.insert("max_tokens".into(), serde_json::json!(max));
        }
        data.push(serde_json::Value::Object(entry));
    }
    // **THE EXTRAS GO THROUGH THE SAME MAP AS THE REGISTRY, WHICH I GOT WRONG FIRST.** The source is
    // `[...MODELS.filter(...), ...extra].map((m, i) => ({…}))` — one `map` over BOTH — so a console-added
    // model gets `object`, `created` (continuing the index) and the same optional-field rules. My first
    // version appended the extra record VERBATIM, and the differential caught it on its first run: the
    // captured listing has `{"id":"og/console-added","object":"model","created":1785000024,…}` while mine
    // had `{"id":"og/console-added","owned_by":"opencode"}`.
    //
    // AND THE EXTRAS ARE SNAKE_CASED ALREADY (`owned_by`, not the registry's `ownedBy`) — a different record
    // type that happens to describe the same thing, which is why they are `Value`s here.
    for (i, e) in extra.iter().enumerate() {
        let ov = e
            .get("id")
            .and_then(|v| v.as_str())
            .and_then(|id| overrides.get(id));
        let mut entry = serde_json::Map::new();
        if let Some(id) = e.get("id") {
            entry.insert("id".into(), id.clone());
        }
        entry.insert("object".into(), serde_json::json!("model"));
        entry.insert(
            "created".into(),
            serde_json::json!(1_785_000_000i64 + (enabled.len() + i) as i64),
        );
        if let Some(owner) = e.get("owned_by") {
            entry.insert("owned_by".into(), owner.clone());
        }
        let text = |from_record: Option<&str>, from_override: &str| -> Option<String> {
            match from_record.filter(|v| !v.is_empty()) {
                Some(v) => Some(v.to_string()),
                None => ov
                    .and_then(|o| o.get(from_override))
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.is_empty())
                    .map(str::to_string),
            }
        };
        if let Some(name) = text(e.get("name").and_then(|v| v.as_str()), "name") {
            entry.insert("name".into(), serde_json::json!(name));
        }
        if let Some(window) = e
            .get("context_window")
            .and_then(|v| v.as_i64())
            .or_else(|| {
                ov.and_then(|o| o.get("contextWindow"))
                    .and_then(|v| v.as_i64())
            })
        {
            entry.insert("context_window".into(), serde_json::json!(window));
        }
        if let Some(max) = e
            .get("max_tokens")
            .and_then(|v| v.as_i64())
            .or_else(|| ov.and_then(|o| o.get("maxTokens")).and_then(|v| v.as_i64()))
        {
            entry.insert("max_tokens".into(), serde_json::json!(max));
        }
        data.push(serde_json::Value::Object(entry));
    }
    json_ok(&serde_json::json!({ "object": "list", "data": data }), &[])
}

/// `errorTypeForStatus(status)` — ONE line in the source, and it decides which of the two Anthropic error
/// types a status wears. `429` is the only status that is not a generic `api_error`.
pub fn error_type_for_status(status: u16) -> &'static str {
    if status == 429 {
        "rate_limit_error"
    } else {
        "api_error"
    }
}

/// `jsonError(status, message, type, extraHeaders)` — the four-argument form.
///
/// **THE FOURTH ARGUMENT IS HEADERS, NOT BODY FIELDS.** `jsonError` spreads it into the header object after
/// `CORS_HEADERS`, which is how a `retry-after` reaches the client — a fact worth stating because the call
/// sites read like they are adding to the error document.
pub fn json_error_with(
    status: u16,
    message: &str,
    kind: &str,
    extra_headers: &[(&str, &str)],
) -> Built {
    let mut built = json_error(status, message, kind);
    for (k, v) in extra_headers {
        built.headers.push(((*k).to_string(), (*v).to_string()));
    }
    built
}

/// **`upstreamFetchFailedResponse` — the upstream never answered.**
///
/// Two decisions, both pinned by the source's own comments: the status is the FETCH's own when it looks like
/// an HTTP status (`>= 400 && <= 599`, so a `0` or a `999` from a broken inspection becomes a 502 rather
/// than a status no client can read), and the message names the status and the kind so an operator can tell
/// which channel died.
///
/// The `opencode` branch's `recordChannelFailure(env)` is a Durable Object call — the I/O edge — and is NOT
/// here; the caller makes it.
pub fn upstream_fetch_failed(kind: &str, inspect_status: Option<u16>, detail: &str) -> Built {
    let fail_status = match inspect_status {
        Some(s) if (400..=599).contains(&s) => s,
        _ => 502,
    };
    json_error(
        fail_status,
        &format!("upstream {fail_status} ({kind}): {detail}"),
        error_type_for_status(fail_status),
    )
}

/// **THE LABEL THAT RIDES EVERY UPSTREAM-ERROR MESSAGE FROM THE TRANSLATE ARM — and it is NOT the kind.**
///
/// The source names it `translateLabel` and spells the mapping out:
///
/// ```text
///     custom        -> the provider's prefix, or "custom"
///     commandgoat   -> "cm"
///     everything else -> "og"
/// ```
///
/// So an `opencode` route labels its errors `og:`, and a `commandgoat` route labels them `cm:` — which is
/// what the CAPTURED messages say. A port that passed `route.kind` through would produce `opencode:` and
/// `commandgoat:` instead, and the corpus would catch it on the first case.
pub fn translate_label(kind: &str, provider_prefix: Option<&str>) -> String {
    if kind == "custom" {
        return provider_prefix
            .filter(|p| !p.is_empty())
            .unwrap_or("custom")
            .to_string();
    }
    if kind == "commandgoat" {
        return "cm".to_string();
    }
    "og".to_string()
}

/// **`upstreamBodyErrorResponse` — the upstream answered, and the answer was an error.**
///
/// The source's own header calls out what this does that a hand-rolled envelope does not: it unwraps AMD's
/// `{"detail":{…}}` FastAPI shape, scrubs any leaked key in TWO layers (`redactSecrets` for the exact
/// credentials this request carried, then `scrubKeys` for the prefix heuristic), keeps the upstream's OWN
/// `error.type` when it is a known Anthropic type — "Claude Code keys retry/auth flows off it" — and carries
/// `Retry-After` when the upstream sent one.
///
/// `json` IS THE PARSED BODY (`upstream.json()`, `None` when it was not JSON at all) and `retry_after` is the
/// upstream's header, because both are I/O. Every branch of the decision is here.
pub fn upstream_body_error(
    status: u16,
    json: Option<&serde_json::Value>,
    retry_after: Option<&str>,
    label: &str,
    secrets: &[serde_json::Value],
) -> Built {
    let prefix = if label.is_empty() {
        String::new()
    } else {
        format!("{label}: ")
    };
    let mut message = format!("{prefix}Upstream {status}");
    let mut kind = error_type_for_status(status);
    let mut extra: Vec<(&str, &str)> = Vec::new();
    if let Some(raw) = json {
        // `rawErr?.detail && typeof rawErr.detail === "object" ? rawErr.detail : rawErr`
        let err = match raw.get("detail") {
            Some(d) if d.is_object() => d,
            _ => raw,
        };
        // `err.error?.message || err.message || JSON.stringify(err).slice(0, 200)` — JS truthiness, so an
        // EMPTY string falls through to the next, and the last resort is a TRUNCATED stringify.
        let raw_msg = err
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .filter(|m| !m.is_empty())
            .or_else(|| {
                err.get("message")
                    .and_then(|m| m.as_str())
                    .filter(|m| !m.is_empty())
            })
            .map(str::to_string)
            .unwrap_or_else(|| {
                let text = crate::responses::stringify_like_json(err);
                text.chars().take(200).collect()
            });
        let scrubbed = crate::redact::scrub_keys(&serde_json::Value::String(
            crate::redact::redact_secrets(&raw_msg, secrets),
        ));
        if !scrubbed.is_empty() {
            message = format!("{prefix}{scrubbed}");
        }
        let up_type = err
            .get("error")
            .and_then(|e| e.get("type"))
            .and_then(|t| t.as_str())
            .or_else(|| err.get("type").and_then(|t| t.as_str()));
        const KNOWN: [&str; 6] = [
            "rate_limit_error",
            "overloaded_error",
            "authentication_error",
            "invalid_request_error",
            "permission_error",
            "not_found_error",
        ];
        // (`request_too_large` and `api_error` are in the source's list too; the array is seven long and the
        // two above are the ones a status maps to already.)
        if let Some(t) = up_type {
            if KNOWN.contains(&t) || t == "request_too_large" || t == "api_error" {
                kind = match t {
                    "rate_limit_error" => "rate_limit_error",
                    "overloaded_error" => "overloaded_error",
                    "authentication_error" => "authentication_error",
                    "invalid_request_error" => "invalid_request_error",
                    "permission_error" => "permission_error",
                    "not_found_error" => "not_found_error",
                    "request_too_large" => "request_too_large",
                    _ => "api_error",
                };
            }
        }
    }
    if let Some(ra) = retry_after {
        extra.push(("retry-after", ra));
    }
    json_error_with(status, &message, kind, &extra)
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

    fn stream_corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/stream-ignored-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the stream-ignored corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    /// **THE RESPONSE SIDE'S DIFFERENTIAL**: `fixtures/stream-ignored-corpus.json` records what the shipping
    /// route ANSWERS when the upstream ignores `stream: true` and returns JSON — captured by stubbing
    /// `fetch` (`gateway/wasm/route-oracle.mjs`).
    fn non_stream_corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/non-stream-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the non-stream corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    /// **THE NON-STREAMING TWIN, AND ITS SENTENCE IS THE POINT.** This branch and the ignored-stream one
    /// answer the same SHAPE with different generic text ("upstream returned an invalid response" against
    /// "…an error envelope"), so a test that compared statuses would call them equivalent. The corpus pins
    /// the body, and this asserts it.
    fn models_corpus() -> serde_json::Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/models-corpus.json");
        let text = std::fs::read_to_string(path).expect("the models corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    /// **`GET /v1/models`, REPLAYED FROM THE SHIPPING ROUTE.** The fixture records the three REAL inputs the
    /// arm reads (`advertisedIds`, `extraModelEntries`, `facetOverrides`) rather than re-deriving them, so
    /// this drives the listing and not a re-implementation of the store.
    #[test]
    fn the_models_listing_matches_the_shipping_route() {
        let doc = models_corpus();
        let mut checked = 0;
        let mut lengths = Vec::new();
        for case in doc["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().unwrap_or("?");
            let live: Vec<String> = case["liveIds"]
                .as_array()
                .expect("liveIds")
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            let extra: Vec<serde_json::Value> = case["extraEntries"]
                .as_array()
                .expect("extraEntries")
                .clone();
            let overrides: serde_json::Map<String, serde_json::Value> = case["facet"]
                .as_array()
                .expect("facet")
                .iter()
                .filter_map(|o| {
                    o.get("id")
                        .and_then(|i| i.as_str())
                        .map(|i| (i.to_string(), o.clone()))
                })
                .collect();
            let got = models_listing(&crate::registry::MODEL_REGISTRY, &live, &extra, &overrides);
            let want = &case["expected"];
            assert_eq!(
                got.status,
                want["status"].as_u64().unwrap_or(0) as u16,
                "{name}: status"
            );
            assert_eq!(
                got.body,
                want["body"].as_str().unwrap_or(""),
                "{name}: body"
            );
            let headers: serde_json::Map<String, serde_json::Value> = got
                .headers
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), serde_json::Value::String(v)))
                .collect();
            assert_eq!(
                &serde_json::Value::Object(headers),
                &want["headers"],
                "{name}: headers"
            );
            let parsed: serde_json::Value = serde_json::from_str(&got.body).expect("a JSON body");
            lengths.push(parsed["data"].as_array().map(|d| d.len()).unwrap_or(0));
            checked += 1;
        }
        assert!(checked >= 5, "the models corpus shrank to {checked} cases");
        // **THE LENGTHS ARE THE POINT OF THREE OF THESE CASES.** 24 is the registry; disabling one drops it
        // to 23 and disabling two to 22; a console-added model APPENDS and makes 25. A corpus that only ever
        // listed 24 would prove the happy path and nothing about the merge.
        assert_eq!(
            lengths,
            vec![24, 23, 22, 25, 24],
            "the listing lengths moved"
        );
    }

    fn failure_corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/upstream-failure-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the failure corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    /// **THE UPSTREAM-FAILURE DIFFERENTIAL.** Nine cases captured from the shipping route with a stubbed
    /// `fetch` answering a 5xx: the unwrap of AMD's `{"detail":{…}}`, the two-layer key scrub, the KNOWN
    /// upstream type, `Retry-After` as a header, the truncated stringify, and the truthiness chain.
    #[test]
    fn the_upstream_failure_path_matches_the_shipping_route() {
        let doc = failure_corpus();
        let mut checked = 0;
        let mut kinds = Vec::new();
        for case in doc["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().unwrap_or("?");
            let json = &case["json"];
            let parsed = if json.is_null() { None } else { Some(json) };
            // The label is COMPUTED, not read: that is `translate_label`'s rule and this drives it.
            let label = translate_label(case["kind"].as_str().unwrap_or(""), None);
            let got = upstream_body_error(
                case["status"].as_u64().unwrap_or(0) as u16,
                parsed,
                case["retryAfter"].as_str(),
                &label,
                &[],
            );
            let want = &case["expected"];
            assert_eq!(
                got.status,
                want["status"].as_u64().unwrap_or(0) as u16,
                "{name}: status"
            );
            assert_eq!(
                got.body,
                want["body"].as_str().unwrap_or(""),
                "{name}: body"
            );
            // The captured headers are a real `Response`'s, so both sides fold.
            let headers: serde_json::Map<String, serde_json::Value> = got
                .headers
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), serde_json::Value::String(v)))
                .collect();
            for (k, v) in want["headers"].as_object().expect("headers") {
                assert_eq!(
                    headers.get(k),
                    Some(v),
                    "{name}: header {k} (got {headers:?})"
                );
            }
            let parsed_body: serde_json::Value =
                serde_json::from_str(&got.body).expect("an error body is JSON");
            kinds.push(
                parsed_body["error"]["type"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            );
            checked += 1;
        }
        assert!(checked >= 9, "the failure corpus shrank to {checked} cases");
        // **THE TYPES ARE THE POINT OF FOUR OF THESE CASES.** `rate_limit_error` comes from the STATUS, the
        // other four from the upstream's own `error.type` — kept when known, replaced when not.
        assert!(
            kinds.contains(&"rate_limit_error".to_string()),
            "no 429 case"
        );
        assert!(
            kinds.contains(&"invalid_request_error".to_string()),
            "no kept type"
        );
        assert!(
            kinds.contains(&"permission_error".to_string()),
            "no unwrapped type"
        );
        assert!(kinds.contains(&"api_error".to_string()), "no fallback type");
    }

    #[test]
    fn the_label_is_not_the_kind() {
        // **THE RULE THE CAPTURE TAUGHT ME.** An `opencode` route labels its errors `og:`, and a
        // `commandgoat` route labels them `cm:` — neither is the kind, and my first version of this test
        // hardcoded `"opencode"` and produced a message the shipping route never sends.
        assert_eq!(translate_label("opencode", None), "og");
        assert_eq!(translate_label("commandgoat", None), "cm");
        assert_eq!(translate_label("deepseek", None), "og");
        assert_eq!(translate_label("custom", Some("acme")), "acme");
        assert_eq!(translate_label("custom", None), "custom");
        // An EMPTY prefix is `|| "custom"`, not an empty label.
        assert_eq!(translate_label("custom", Some("")), "custom");
    }

    #[test]
    fn the_non_stream_branch_matches_the_shipping_route() {
        let doc = non_stream_corpus();
        let mut checked = 0;
        let mut statuses = Vec::new();
        for case in doc["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().unwrap_or("?");
            let json = &case["upstreamJson"];
            let upstream_json = if json.is_null() { None } else { Some(json) };
            let got =
                upstream_json_response(upstream_json, case["upstreamModel"].as_str().unwrap_or(""));
            let want = &case["expected"];
            assert_eq!(
                got.status,
                want["status"].as_u64().unwrap_or(0) as u16,
                "{name}: status"
            );
            assert_eq!(
                got.body,
                want["body"].as_str().unwrap_or(""),
                "{name}: body"
            );
            let headers: serde_json::Map<String, serde_json::Value> = got
                .headers
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), serde_json::Value::String(v)))
                .collect();
            assert_eq!(
                &serde_json::Value::Object(headers),
                &want["headers"],
                "{name}: headers"
            );
            statuses.push(got.status);
            checked += 1;
        }
        assert!(
            checked >= 6,
            "the non-stream corpus shrank to {checked} cases"
        );
        assert!(
            statuses.contains(&200),
            "no case exercised the success path"
        );
        assert!(statuses.contains(&502), "no case exercised a refusal");
        // **AND THE TWO SENTENCES ARE DISTINCT.** If a refactor made these branches share one generic string,
        // the corpus would still pass — unless something asserts the difference, which is this.
        let invalid = upstream_json_response(None, "deepseek-flash");
        assert!(
            invalid
                .body
                .contains("upstream returned an invalid response"),
            "{}",
            invalid.body
        );
        assert!(
            !invalid.body.contains("error envelope"),
            "this branch must NOT use the streaming branch's sentence: {}",
            invalid.body
        );
    }

    #[test]
    fn the_ignored_stream_branch_matches_the_shipping_route() {
        let doc = stream_corpus();
        let mut checked = 0;
        let mut seen_statuses = Vec::new();
        for case in doc["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().unwrap_or("?");
            let json = &case["upstreamJson"];
            let upstream_json = if json.is_null() { None } else { Some(json) };
            let got = stream_ignored_response(
                upstream_json,
                case["upstreamModel"].as_str().unwrap_or(""),
            );
            let want = &case["expected"];
            assert_eq!(
                got.status,
                want["status"].as_u64().unwrap_or(0) as u16,
                "{name}: status"
            );
            assert_eq!(
                got.body,
                want["body"].as_str().unwrap_or(""),
                "{name}: body"
            );
            // **FOLDED, BECAUSE THE ROUTE ANSWERS WITH A REAL `Response`.** The capture read its `Headers`
            // object, which lowercases every name — the same rule `json_ok`'s arm carries and the opposite
            // of `corsHeadersFor`'s, whose oracle recorded a plain `Record`. The rule is about HOW THE
            // ORACLE READ IT, not about the function.
            let headers: serde_json::Map<String, serde_json::Value> = got
                .headers
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), serde_json::Value::String(v)))
                .collect();
            assert_eq!(
                &serde_json::Value::Object(headers),
                &want["headers"],
                "{name}: headers"
            );
            seen_statuses.push(got.status);
            checked += 1;
        }
        assert!(
            checked >= 6,
            "the stream-ignored corpus shrank to {checked} cases"
        );
        // **BOTH ANSWERS ARE IN THE CORPUS, AND THE FLOOR ABOVE WOULD PASS WITHOUT THIS.** A corpus that
        // only ever produced 502s would prove the error path and nothing else; one that only produced 200s
        // would prove the happy path. The one-shot SSE arm is the reason this branch exists.
        assert!(
            seen_statuses.contains(&200),
            "no case exercised the one-shot SSE"
        );
        assert!(seen_statuses.contains(&502), "no case exercised a refusal");
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
                "json_ok" => {
                    // This arm reads the case directly: the loop binds `case`, and every other arm goes
                    // through `check`'s narrower parameters.
                    let name = case["name"].as_str().unwrap_or("?");
                    let input = &case["input"];
                    let want = &case["expected"];
                    let extra: Vec<(String, String)> = input["extra"]
                        .as_object()
                        .map(|o| {
                            o.iter()
                                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    let got = json_ok(&input["data"], &extra);
                    assert_eq!(got.status, 200, "json_ok / {name}: status");
                    assert_eq!(
                        got.body,
                        want["body"].as_str().unwrap_or(""),
                        "json_ok / {name}: body"
                    );
                    // **FOLDED HERE, UNFOLDED FOR `corsHeadersFor`, AND THE DIFFERENCE IS HOW THE ORACLE
                    // READ IT.** `jsonOk` returns a `Response`, so the oracle read its `Headers` object —
                    // which lowercases every name. `corsHeadersFor` returns a plain `Record`, so its oracle
                    // recorded the source's own spelling. Neither function's output changed; the recording
                    // did.
                    let headers: serde_json::Map<String, serde_json::Value> = got
                        .headers
                        .into_iter()
                        .map(|(k, v)| (k.to_ascii_lowercase(), serde_json::Value::String(v)))
                        .collect();
                    assert_eq!(
                        &serde_json::Value::Object(headers),
                        &want["headers"],
                        "json_ok / {name}: headers"
                    );
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
