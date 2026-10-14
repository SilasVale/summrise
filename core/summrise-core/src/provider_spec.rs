//! `parseProviderSpec` + `parseProviderModel` — **THE ADMIN API'S ONLY BYTE-EXACT VALIDATOR**, and the largest
//! single decision that was still TypeScript in the console's tail.
//!
//! MOVED FROM `gateway/src/store/providers.ts` (2026-10-13). Every string below is a RESPONSE BODY: the admin
//! route answers `400 {error}` verbatim, so a re-worded refusal is a different wire. That is why the tests here
//! assert whole sentences rather than substrings, and why the emission is a byte comparison.
//!
//! **THE PLATFORM SEAM IS THE URL PARSE, AND THE CORE DELIBERATELY DOES NOT OWN IT.** `new URL(rawBase)` is a
//! WHATWG parser in JavaScript and the `url` crate in Rust — a dependency and a platform difference the pure core
//! must not carry. So the caller hands over the PARSED PARTS ([`ParsedBase`]) and the core decides on them,
//! exactly as `device.rs`'s dial takes its parser as a callback. **THE REFUSAL ORDER IS PRESERVED BY
//! CONSTRUCTION**: the caller parses before the call, while [`parse_provider_spec`] reads `base` only after the
//! prefix, reserved-prefix and label refusals — so a body that is wrong twice gets the refusal the shipping
//! console gives it.
//!
//! **KEY ORDER IS THE HOST'S BUILD, AND IT IS THE ONE THING THIS MODULE DOES NOT PIN.** `Object.keys` order is
//! insertion order in JavaScript and in the worker's `serde_json` (its `preserve_order` is on for the wire's
//! sake). A host that leaves it off — the core's own `cargo test`, the agent — sees the same SET of keys in
//! sorted order, which changes exactly one message (the unknown-field list, which needs TWO unknown fields to be
//! visible) and the order of the returned object's keys. The EMISSION is what the console ships, and its object
//! literal is written in the source's order, so the wire bytes are right where they are read.

use serde_json::{json, Map, Value};

use crate::catalogue::RESERVED_PREFIXES;
use crate::guard::device_host_error;
use crate::text::{js_is_integer, js_number, js_or_empty, js_string, js_truthy};

/// `MODEL_FIELDS` — the only fields a provider model may declare, in the order the refusal prints them.
pub const MODEL_FIELDS: [&str; 6] = [
    "id",
    "name",
    "input",
    "contextWindow",
    "maxTokens",
    "reasoningEffort",
];

/// `KNOWN_MODALITIES` — the `input` values this gateway understands; anything else is refused rather than
/// stored-and-ignored.
pub const KNOWN_MODALITIES: [&str; 2] = ["text", "image"];

/// The four reasoning levels the router can speak, in the order the refusal prints them.
pub const REASONING_EFFORTS: [&str; 4] = ["low", "medium", "high", "max"];

/// `SUPPORTED_PROVIDER_APIS` — dialect → the path APPENDED to `baseURL`. One table, because it backs both the
/// creation-time validation and the routing join (`routing.rs` reads this same const).
///
/// `anthropic-messages` is DELIBERATELY absent: the gateway has no OpenAI → Anthropic REQUEST translator, so
/// such a provider would serve /v1/messages and fail every OpenAI-format client upstream — precisely the
/// "accepted at creation, fails at request time" outcome a creation-time refusal exists to prevent.
pub const SUPPORTED_PROVIDER_APIS: [(&str, &str); 1] =
    [("openai-completions", "/chat/completions")];

/// `KNOWN_UNSUPPORTED_APIS` — the dialects an operator is most likely to reach for, and WHY this build does not
/// serve them. A generic "unsupported" would leave the reason to be rediscovered.
pub const KNOWN_UNSUPPORTED_APIS: [(&str, &str); 1] = [(
    "anthropic-messages",
    "this gateway has no OpenAI→Anthropic request translator, so the provider would serve /v1/messages and fail every OpenAI-format client",
)];

/// Why a provider record was refused, and with which HTTP status. `400` = the record is wrong; `409` = it
/// collides with a reserved prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderParseError {
    pub error: String,
    pub status: u16,
}

impl ProviderParseError {
    fn bad_request(error: String) -> Self {
        ProviderParseError { error, status: 400 }
    }
    fn conflict(error: String) -> Self {
        ProviderParseError { error, status: 409 }
    }
}

/// The URL's PARSED PARTS, as the host's parser produced them — the one thing this module cannot compute.
///
/// The names are the platform's: `protocol` is `"https:"` WITH its colon (the refusal prints
/// `${protocol}//`), and `origin` + `pathname` are joined and stripped of trailing slashes by the decision.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedBase {
    pub protocol: String,
    pub username: String,
    pub password: String,
    pub search: String,
    pub hash: String,
    pub hostname: String,
    pub origin: String,
    pub pathname: String,
}

/// `JSON.stringify(s)` for a STRING — the quoting the refusals carry (`got "my prefix"`).
fn quote(s: &str) -> String {
    serde_json::to_string(&Value::String(s.to_string())).unwrap_or_else(|_| "\"\"".to_string())
}

/// `String(body.baseURL ?? "").trim()` — the text the host's URL parser is handed.
///
/// It is PUBLIC because the host must read the same text to parse it: a second coercion in the host would be
/// the twin this module exists to delete.
pub fn provider_base_text(body: &Value) -> String {
    js_or_empty(body.get("baseURL")).trim().to_string()
}

/// `barePrefix(prefix)` — `String(prefix ?? "").replace(/\/+$/, "")`: the TRAILING slashes come off.
///
/// `String(... ?? "")` IS A COERCION, not a null check: a number becomes its digits and an object becomes
/// `[object Object]`. **AND `?? ""` IS NOT DECORATION**: `null` and an absent key map to the EMPTY string, where
/// `String(null)` is the text `"null"` — measured, a provider whose `prefix` was absent once advertised its
/// models as `null/foo`.
pub fn bare_prefix(prefix: &Value) -> String {
    let text = if prefix.is_null() {
        String::new()
    } else {
        js_string(prefix)
    };
    text.trim_end_matches('/').to_string()
}

/// `advertisedModelId(prefix, wire)` — **"prefix ONCE, never twice"**, in the source's own words.
///
/// The leading slashes come off the wire first, and the prefix is prepended ONLY IF the wire does not already
/// start with it, so a provider whose models are spelled `acme/foo` under the prefix `acme` is not advertised
/// as `acme/acme/foo`.
///
/// **AND THE `?? ""` IS THE SAME FIX `bare_prefix` CARRIES**: the worker's first version passed the wire through
/// `js_string`, so a null wire advertised `acme/null` where the source's `String(wire ?? "")` gives `acme/`.
/// Unreachable from the listing (an empty wire is skipped first) and wrong at the boundary, which is where the
/// decision route calls it.
pub fn advertised_model_id(prefix: &str, wire: &Value) -> String {
    let bare = bare_prefix(&Value::String(prefix.to_string()));
    let wire_text = if wire.is_null() {
        String::new()
    } else {
        js_string(wire)
    };
    let w = wire_text.trim_start_matches('/');
    if w.starts_with(&format!("{bare}/")) {
        w.to_string()
    } else {
        format!("{bare}/{w}")
    }
}

/// `parseProviderModel(raw, at)` — one entry of `models`, or the message the route answers with.
pub fn parse_provider_model(raw: &Value, at: usize) -> Result<Value, String> {
    let where_ = format!("models[{at}]");
    if let Some(id) = raw.as_str() {
        let id = id.trim();
        if id.is_empty() {
            return Err(format!("{where_}: empty model id"));
        }
        return Ok(json!({ "id": id }));
    }
    if !raw.is_object() {
        return Err(format!("{where_} must be a model id string or an object"));
    }
    let object = raw.as_object().expect("checked above");
    let unknown: Vec<&str> = object
        .keys()
        .map(String::as_str)
        .filter(|k| !MODEL_FIELDS.contains(k))
        .collect();
    if !unknown.is_empty() {
        return Err(format!(
            "{where_}: unsupported field(s) {} — this gateway stores {}",
            unknown.join(", "),
            MODEL_FIELDS.join(", ")
        ));
    }
    let id = js_or_empty(object.get("id")).trim().to_string();
    if id.is_empty() {
        return Err(format!("{where_}: id is required"));
    }
    if id.chars().any(js_is_space) {
        return Err(format!("{where_}.id must not contain whitespace"));
    }
    // **THE FIELD ORDER OF THIS OBJECT IS THE STORED BYTES**: `{ id }`, then `name`, then the two facets
    // `input` writes, then `reasoningEffort`, then the two caps — the source's own assignment order.
    let mut model = Map::new();
    model.insert("id".to_string(), Value::String(id));
    if let Some(name) = object.get("name") {
        let name = js_string(name).trim().to_string();
        if !name.is_empty() {
            model.insert("name".to_string(), Value::String(name));
        }
    }
    if let Some(input) = object.get("input") {
        let Some(items) = input.as_array() else {
            return Err(format!(
                "{where_}.input must be an array of {}",
                KNOWN_MODALITIES.join("/")
            ));
        };
        // `String(m)` per element, because a JSON array may carry a number or a null.
        let bad: Vec<String> = items
            .iter()
            .map(js_string)
            .filter(|m| !KNOWN_MODALITIES.contains(&m.as_str()))
            .collect();
        if !bad.is_empty() {
            return Err(format!(
                "{where_}.input: unsupported modalit{} {} — only {} are understood",
                if bad.len() > 1 { "ies" } else { "y" },
                bad.iter().map(|b| quote(b)).collect::<Vec<_>>().join(", "),
                KNOWN_MODALITIES.join(" and ")
            ));
        }
        // A model that declares image input must NOT have its images described by the gateway's vision model:
        // it can see them itself.
        if items.iter().map(js_string).any(|m| m == "image") {
            model.insert("vision".to_string(), Value::Bool(true));
        }
    }
    if let Some(effort) = object.get("reasoningEffort") {
        let lv = js_string(effort).trim().to_string();
        if !REASONING_EFFORTS.contains(&lv.as_str()) {
            return Err(format!(
                "{where_}.reasoningEffort must be one of {}",
                REASONING_EFFORTS.join(", ")
            ));
        }
        model.insert("reasoningEffort".to_string(), Value::String(lv));
    }
    for cap in ["contextWindow", "maxTokens"] {
        let Some(v) = object.get(cap) else {
            continue;
        };
        let n = js_number(v);
        if !js_is_integer(n) || n <= 0.0 {
            return Err(format!("{where_}.{cap} must be a positive integer"));
        }
        model.insert(cap.to_string(), js_number_value(n));
    }
    Ok(Value::Object(model))
}

/// A JavaScript number as JSON: an INTEGRAL value prints without a fraction, which `serde_json`'s `f64` does not
/// do. The `< 1e21` bound is JavaScript's own switch to the exponential form — the same bound, and the same
/// recorded divergence, as `text::js_number_string`.
fn js_number_value(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 1e21 {
        return json!(n as i64);
    }
    serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
}

/// `parseProviderSpec(body, opts)` — the spec to store, or the refusal the route answers with.
///
/// `base` is `None` when the host's parser THREW (the source's `catch`), and `raw_base` is the coerced text it
/// was handed, because the refusal quotes it.
pub fn parse_provider_spec(
    body: &Value,
    base: Option<&ParsedBase>,
    keep_key_from: Option<&Value>,
) -> Result<Value, ProviderParseError> {
    if !body.is_object() {
        return Err(ProviderParseError::bad_request(
            "body must be a JSON object".to_string(),
        ));
    }
    let prefix = js_or_empty(body.get("prefix")).trim().to_string();
    if !is_channel_prefix(&prefix) {
        return Err(ProviderParseError::bad_request(format!(
            "prefix must be a lowercase channel prefix ending in \"/\" (e.g. \"my/\") — got {}",
            quote(&prefix)
        )));
    }
    let bare = bare_prefix(&Value::String(prefix.clone()));
    if RESERVED_PREFIXES.contains(&bare.as_str()) {
        return Err(ProviderParseError::conflict(format!(
            "prefix {} is reserved by a built-in channel (reserved: {})",
            quote(&prefix),
            RESERVED_PREFIXES.join(", ")
        )));
    }
    let label = {
        let trimmed = js_or_empty(body.get("label")).trim().to_string();
        if trimmed.is_empty() {
            bare.clone()
        } else {
            trimmed
        }
    };
    // `label.length` IS THE UTF-16 LENGTH, and the control-character class is the source's own
    // `[\u0000-\u001f\u007f]`.
    if label.encode_utf16().count() > 80 || label.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}') {
        return Err(ProviderParseError::bad_request(
            "label must be at most 80 characters, with no control characters".to_string(),
        ));
    }
    let raw_base = provider_base_text(body);
    let Some(url) = base else {
        return Err(ProviderParseError::bad_request(format!(
            "baseURL is not a URL — got {}",
            quote(&raw_base)
        )));
    };
    if url.protocol != "https:" {
        return Err(ProviderParseError::bad_request(format!(
            "baseURL must be https (got {}//) — a provider key must never travel in clear",
            url.protocol
        )));
    }
    if !url.username.is_empty() || !url.password.is_empty() {
        return Err(ProviderParseError::bad_request(
            "baseURL must not embed credentials".to_string(),
        ));
    }
    if !url.search.is_empty() || !url.hash.is_empty() {
        return Err(ProviderParseError::bad_request(
            "baseURL must not carry a query or a fragment".to_string(),
        ));
    }
    // The SSRF check, REUSED: loopback/private/link-local/metadata hosts are refused here for the same reason a
    // device hostname is — the gateway would dial them with a credential attached.
    if let Some(reason) = device_host_error(&url.hostname) {
        return Err(ProviderParseError::bad_request(format!(
            "baseURL host rejected: {reason}"
        )));
    }
    // Treated as a PREFIX, not resolved against: a deployment path such as https://host/openai/v1 must survive.
    let base_url = format!("{}{}", url.origin, url.pathname)
        .trim_end_matches('/')
        .to_string();
    let api = js_or_empty(body.get("api")).trim().to_string();
    let served: Vec<&str> = SUPPORTED_PROVIDER_APIS
        .iter()
        .map(|(name, _)| *name)
        .collect();
    if !served.contains(&api.as_str()) {
        let why = KNOWN_UNSUPPORTED_APIS
            .iter()
            .find(|(name, _)| *name == api)
            .map(|(_, why)| *why);
        return Err(ProviderParseError::bad_request(format!(
            "unsupported protocol {} — this gateway serves {}{}",
            quote(&api),
            served.join(", "),
            match why {
                Some(why) => format!(" ({api}: {why})"),
                None => String::new(),
            }
        )));
    }
    let api_key_env = js_or_empty(body.get("apiKeyEnv")).trim().to_string();
    let api_key = js_or_empty(body.get("apiKey")).trim().to_string();
    if !api_key_env.is_empty() && !api_key.is_empty() {
        return Err(ProviderParseError::bad_request(
            "set apiKeyEnv (a deployed Worker secret) or apiKey (inline), not both — two key sources cannot \
             both be the one this provider sends"
                .to_string(),
        ));
    }
    // AN UPDATE MAY OMIT THE KEY; A CREATE MAY NOT. The caller passes what the STORED record already has, and
    // only for an existing prefix; a create has nothing to keep and is refused exactly as before.
    let keep = if api_key_env.is_empty() && api_key.is_empty() {
        keep_key_from
    } else {
        None
    };
    if api_key_env.is_empty() && api_key.is_empty() && keep.is_none() {
        return Err(ProviderParseError::bad_request(
            "a key is required: apiKeyEnv (the NAME of a Worker secret, DSH's settings.yaml spelling) or \
             apiKey (an inline value stored in KV)"
                .to_string(),
        ));
    }
    if !api_key_env.is_empty() && !is_env_name(&api_key_env) {
        return Err(ProviderParseError::bad_request(format!(
            "apiKeyEnv must be a Worker env/secret NAME (letters, digits, \"_\" — e.g. SUMMRISE_API_KEY), not \
             a value — got {}",
            quote(&api_key_env)
        )));
    }
    // `<8` also keeps the credential out of `redactSecrets`' "too generic to replace safely" bucket, so an
    // upstream echo of it is still redacted.
    if !api_key.is_empty()
        && (api_key.encode_utf16().count() < 8 || api_key.chars().any(js_is_space))
    {
        return Err(ProviderParseError::bad_request(
            "apiKey must be at least 8 characters with no whitespace".to_string(),
        ));
    }
    let Some(models_raw) = body.get("models").and_then(Value::as_array) else {
        return Err(ProviderParseError::bad_request(
            "models must be a non-empty array".to_string(),
        ));
    };
    if models_raw.is_empty() {
        return Err(ProviderParseError::bad_request(
            "models must be a non-empty array".to_string(),
        ));
    }
    if models_raw.len() > 200 {
        return Err(ProviderParseError::bad_request(format!(
            "models: at most 200 entries (got {})",
            models_raw.len()
        )));
    }
    let mut models: Vec<Value> = Vec::new();
    for (i, raw) in models_raw.iter().enumerate() {
        let mut model = parse_provider_model(raw, i).map_err(ProviderParseError::bad_request)?;
        // Normalize to the WIRE form: the full-id spelling (prefix included) is accepted, stored once, and
        // never advertised twice.
        let id = model
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let wire = match id.strip_prefix(&format!("{bare}/")) {
            Some(rest) => rest.to_string(),
            None => id,
        };
        if wire.is_empty() {
            return Err(ProviderParseError::bad_request(format!(
                "models[{i}]: empty model id after the prefix"
            )));
        }
        if models
            .iter()
            .any(|m| m.get("id").and_then(Value::as_str) == Some(wire.as_str()))
        {
            return Err(ProviderParseError::bad_request(format!(
                "models[{i}]: duplicate model id {}",
                advertised_model_id(&bare, &Value::String(wire.clone()))
            )));
        }
        if let Some(object) = model.as_object_mut() {
            object.insert("id".to_string(), Value::String(wire));
        }
        models.push(model);
    }
    let mut spec = Map::new();
    spec.insert("prefix".to_string(), Value::String(prefix));
    spec.insert("label".to_string(), Value::String(label));
    spec.insert("baseURL".to_string(), Value::String(base_url));
    spec.insert("api".to_string(), Value::String(api));
    spec.insert("models".to_string(), Value::Array(models));
    if !api_key_env.is_empty() {
        spec.insert("apiKeyEnv".to_string(), Value::String(api_key_env));
    } else if !api_key.is_empty() {
        spec.insert("apiKey".to_string(), Value::String(api_key));
    } else if let Some(keep) = keep {
        // The stored key, carried forward because the body omitted it — never a synthesis: an update that DOES
        // send one replaces it, which is how key rotation must keep working.
        if js_truthy(keep.get("apiKeyEnv").unwrap_or(&Value::Null)) {
            spec.insert(
                "apiKeyEnv".to_string(),
                keep.get("apiKeyEnv").cloned().unwrap_or(Value::Null),
            );
        } else if js_truthy(keep.get("apiKey").unwrap_or(&Value::Null)) {
            spec.insert(
                "apiKey".to_string(),
                keep.get("apiKey").cloned().unwrap_or(Value::Null),
            );
        }
    }
    Ok(Value::Object(spec))
}

/// `^[a-z0-9][a-z0-9-]{0,30}\/$` — one lowercase alphanumeric, then up to 30 of those or `-`, then a slash.
fn is_channel_prefix(prefix: &str) -> bool {
    let Some(body) = prefix.strip_suffix('/') else {
        return false;
    };
    if body.is_empty() || body.chars().count() > 31 {
        return false;
    }
    let mut chars = body.chars();
    let first = chars.next().expect("non-empty");
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// JavaScript's `\s` — **THE ENGINE'S CLASS, NOT RUST'S `char::is_whitespace`**, which differs in exactly one
/// code point that a copy can carry: `\u{feff}` (the BOM) is whitespace to `\s` and NOT White_Space to Rust, so
/// `"a\u{feff}b"` is refused as an id here and accepted there.
fn js_is_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `^[A-Za-z_][A-Za-z0-9_]*$` — a Worker env/secret NAME, never a value.
fn is_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn https(host: &str) -> ParsedBase {
        ParsedBase {
            protocol: "https:".to_string(),
            username: String::new(),
            password: String::new(),
            search: String::new(),
            hash: String::new(),
            hostname: host.to_string(),
            origin: format!("https://{host}"),
            pathname: String::new(),
        }
    }

    /// The DSH `settings.yaml` provider, as the admin API receives it.
    fn summrise_like() -> Value {
        json!({
            "prefix": "my/",
            "label": "My Provider",
            "baseURL": "https://api.example.com",
            "api": "openai-completions",
            "apiKeyEnv": "MY_PROVIDER_KEY",
            "models": [
                { "id": "llama-3", "contextWindow": 1000000, "maxTokens": 128000, "input": ["text", "image"] },
                "mistral-small"
            ]
        })
    }

    fn parse(body: &Value) -> Result<Value, ProviderParseError> {
        let raw = provider_base_text(body);
        let base = if raw.is_empty() {
            None
        } else {
            Some(https("api.example.com"))
        };
        parse_provider_spec(body, base.as_ref(), None)
    }

    /// **THE HAPPY PATH, AND THE WHOLE RECORD'S BYTES.** The model normalization, the vision facet and the
    /// integers are each asserted, because every one of them is a value a client reads back.
    #[test]
    fn the_dsh_provider_parses() {
        let spec = parse(&summrise_like()).expect("a valid provider");
        assert_eq!(spec["prefix"], json!("my/"));
        assert_eq!(spec["label"], json!("My Provider"));
        assert_eq!(spec["baseURL"], json!("https://api.example.com"));
        assert_eq!(spec["api"], json!("openai-completions"));
        assert_eq!(spec["apiKeyEnv"], json!("MY_PROVIDER_KEY"));
        assert_eq!(spec["models"][0]["id"], json!("llama-3"));
        assert_eq!(spec["models"][0]["contextWindow"], json!(1000000));
        assert_eq!(spec["models"][0]["maxTokens"], json!(128000));
        assert_eq!(spec["models"][0]["vision"], json!(true));
        assert_eq!(spec["models"][1], json!({ "id": "mistral-small" }));
        // The spec's key ORDER is the response's bytes where the host preserves order — the emission is what
        // pins it for the console, and this asserts the SET rather than the order (see the module header).
        let keys: Vec<&String> = spec.as_object().expect("object").keys().collect();
        assert_eq!(keys.len(), 6);
        assert!(keys.contains(&&"models".to_string()));
    }

    /// **THE RESERVED PREFIX IS A 409 AND ITS MESSAGE IS THE LIST.** The order is the wire's.
    #[test]
    fn a_reserved_prefix_is_refused_with_the_list() {
        for reserved in RESERVED_PREFIXES {
            let mut body = summrise_like();
            body["prefix"] = json!(format!("{reserved}/"));
            let err = parse(&body).expect_err("reserved");
            assert_eq!(err.status, 409, "{reserved}/ must be a conflict");
            assert!(err.error.contains("reserved"), "{reserved}/: {}", err.error);
        }
        let mut body = summrise_like();
        body["prefix"] = json!("og/");
        let err = parse(&body).expect_err("reserved");
        assert_eq!(
            err.error,
            "prefix \"og/\" is reserved by a built-in channel (reserved: og, or, nv, gmi, qw, cm, r4, none, \
             ds, amd)"
        );
    }

    /// **THE SSRF AND SCHEME REFUSALS, EACH WITH THE SENTENCE THE ROUTE ANSWERS.** The hosts below are the ones
    /// the guard stack exists for; the point of reusing `device_host_error` is that this list cannot drift.
    #[test]
    fn the_base_url_is_https_credential_free_and_public() {
        let cases: [(&str, &str); 4] = [
            (
                "http://api.example.com",
                "baseURL must be https (got http://) — a provider key must never travel in clear",
            ),
            (
                "https://user:pw@api.example.com",
                "baseURL must not embed credentials",
            ),
            (
                "https://api.example.com/v1?key=1",
                "baseURL must not carry a query or a fragment",
            ),
            (
                "https://api.example.com/v1#f",
                "baseURL must not carry a query or a fragment",
            ),
        ];
        for (raw, want) in cases {
            let mut body = summrise_like();
            body["baseURL"] = json!(raw);
            // The host's parser would produce these parts; the decision reads them positionally.
            let base = if raw.starts_with("https://user:pw@") {
                ParsedBase {
                    username: "user".into(),
                    password: "pw".into(),
                    ..https("api.example.com")
                }
            } else if raw.contains('?') {
                ParsedBase {
                    search: "?key=1".into(),
                    ..https("api.example.com")
                }
            } else if raw.contains('#') {
                ParsedBase {
                    hash: "#f".into(),
                    ..https("api.example.com")
                }
            } else {
                ParsedBase {
                    protocol: "http:".into(),
                    ..https("api.example.com")
                }
            };
            let err = parse_provider_spec(&body, Some(&base), None).expect_err(raw);
            assert_eq!(err.error, want, "{raw}");
            assert_eq!(err.status, 400);
        }
        // A private host is refused by the guard, with the guard's own sentence inside the refusal.
        let mut body = summrise_like();
        body["baseURL"] = json!("https://169.254.169.254");
        let err = parse_provider_spec(&body, Some(&https("169.254.169.254")), None)
            .expect_err("metadata");
        assert_eq!(
            err.error,
            "baseURL host rejected: device hostname resolves to a private/internal address"
        );
        // A host the parser refused outright quotes the text it was handed.
        let mut body = summrise_like();
        body["baseURL"] = json!("not a url");
        let err = parse_provider_spec(&body, None, None).expect_err("no url");
        assert_eq!(err.error, "baseURL is not a URL — got \"not a url\"");
    }

    /// **THE PROTOCOL TABLE, AND THE NAMED REASON.** `anthropic-messages` is the dialect an operator reaches for
    /// first, so the refusal says WHY rather than "unsupported".
    #[test]
    fn an_unsupported_protocol_names_what_is_served_and_why() {
        let mut body = summrise_like();
        body["api"] = json!("anthropic-messages");
        let err = parse(&body).expect_err("unsupported");
        assert_eq!(
            err.error,
            "unsupported protocol \"anthropic-messages\" — this gateway serves openai-completions \
             (anthropic-messages: this gateway has no OpenAI→Anthropic request translator, so the provider \
             would serve /v1/messages and fail every OpenAI-format client)"
        );
        // A dialect with no written reason gets the shorter sentence.
        let mut body = summrise_like();
        body["api"] = json!("gemini");
        let err = parse(&body).expect_err("unsupported");
        assert_eq!(
            err.error,
            "unsupported protocol \"gemini\" — this gateway serves openai-completions"
        );
    }

    /// **THE KEY RULES, INCLUDING THE ONE THAT KEEPS A SECRET OUT OF A PRINTED FIELD.**
    ///
    /// MUTATION: accept a key VALUE in `apiKeyEnv` (drop the NAME pattern).
    /// RESULT:   the last assertion fails — `sk-live-abcdef123456` would be stored in a field the console
    ///           prints, which is the mistake the check exists for.
    #[test]
    fn exactly_one_key_source_and_it_must_be_a_name_or_a_long_enough_value() {
        let mut none = summrise_like();
        none.as_object_mut().expect("object").remove("apiKeyEnv");
        let err = parse(&none).expect_err("no key");
        assert_eq!(
            err.error,
            "a key is required: apiKeyEnv (the NAME of a Worker secret, DSH's settings.yaml spelling) or \
             apiKey (an inline value stored in KV)"
        );

        let mut both = summrise_like();
        both["apiKey"] = json!("sk-inline-key-1234");
        let err = parse(&both).expect_err("two sources");
        assert_eq!(
            err.error,
            "set apiKeyEnv (a deployed Worker secret) or apiKey (inline), not both — two key sources cannot \
             both be the one this provider sends"
        );

        let mut value_as_name = summrise_like();
        value_as_name["apiKeyEnv"] = json!("sk-live-abcdef123456");
        let err = parse(&value_as_name).expect_err("a value is not a name");
        assert_eq!(
            err.error,
            "apiKeyEnv must be a Worker env/secret NAME (letters, digits, \"_\" — e.g. SUMMRISE_API_KEY), not \
             a value — got \"sk-live-abcdef123456\""
        );

        let mut short = summrise_like();
        short.as_object_mut().expect("object").remove("apiKeyEnv");
        short["apiKey"] = json!("sk-1234");
        let err = parse(&short).expect_err("too short");
        assert_eq!(
            err.error,
            "apiKey must be at least 8 characters with no whitespace"
        );

        // An inline key is carried, and the env NAME is not synthesized beside it.
        let mut inline = summrise_like();
        inline.as_object_mut().expect("object").remove("apiKeyEnv");
        inline["apiKey"] = json!("sk-inline-key-1234");
        let spec = parse(&inline).expect("inline");
        assert_eq!(spec["apiKey"], json!("sk-inline-key-1234"));
        assert!(spec.get("apiKeyEnv").is_none());
    }

    /// **AN UPDATE MAY OMIT THE KEY; A CREATE MAY NOT** — `keepKeyFrom` is passed only for an existing prefix,
    /// and it is a CARRY-FORWARD rather than a synthesis.
    #[test]
    fn an_update_carries_the_stored_key_forward() {
        let mut body = summrise_like();
        body.as_object_mut().expect("object").remove("apiKeyEnv");
        let stored = json!({ "apiKey": "sk-stored-key-1234" });
        let spec = parse_provider_spec(&body, Some(&https("api.example.com")), Some(&stored))
            .expect("an update keeps the key");
        assert_eq!(spec["apiKey"], json!("sk-stored-key-1234"));
        // A body that SENDS one replaces it — that is how rotation keeps working.
        let mut with_key = body.clone();
        with_key["apiKey"] = json!("sk-rotated-key-99");
        let spec = parse_provider_spec(&with_key, Some(&https("api.example.com")), Some(&stored))
            .expect("rotation");
        assert_eq!(spec["apiKey"], json!("sk-rotated-key-99"));
        // An env-referenced record carries the NAME.
        let spec = parse_provider_spec(
            &body,
            Some(&https("api.example.com")),
            Some(&json!({ "apiKeyEnv": "STORED_KEY" })),
        )
        .expect("an update keeps the name");
        assert_eq!(spec["apiKeyEnv"], json!("STORED_KEY"));
    }

    /// **THE MODELS, INCLUDING THE TWO ARMS A CARELESS PORT COLLAPSES**: the full-id spelling is normalized to
    /// the wire name, and a duplicate is refused by its ADVERTISED id.
    #[test]
    fn the_models_are_normalized_and_deduplicated() {
        let mut body = summrise_like();
        body["models"] = json!([{ "id": "my/llama-3" }, "mistral-small"]);
        let spec = parse(&body).expect("valid");
        assert_eq!(spec["models"][0]["id"], json!("llama-3"));
        assert_eq!(spec["models"][1]["id"], json!("mistral-small"));

        let cases: [(Value, &str); 8] = [
            (json!([]), "models must be a non-empty array"),
            (json!([{ "id": "" }]), "models[0]: id is required"),
            (json!([{ "id": "a b" }]), "models[0].id must not contain whitespace"),
            (
                json!([{ "id": "x", "temperature": 1 }]),
                "models[0]: unsupported field(s) temperature — this gateway stores id, name, input, \
                 contextWindow, maxTokens, reasoningEffort",
            ),
            (
                json!([{ "id": "x", "input": ["audio"] }]),
                "models[0].input: unsupported modality \"audio\" — only text and image are understood",
            ),
            (
                json!([{ "id": "x", "input": ["audio", "video"] }]),
                "models[0].input: unsupported modalities \"audio\", \"video\" — only text and image are \
                 understood",
            ),
            (json!([{ "id": "x", "contextWindow": 0 }]), "models[0].contextWindow must be a positive integer"),
            (json!([{ "id": "dup" }, { "id": "my/dup" }]), "models[1]: duplicate model id my/dup"),
        ];
        for (models, want) in cases {
            let mut body = summrise_like();
            body["models"] = models.clone();
            let err = parse(&body).expect_err(&format!("{models}"));
            assert_eq!(err.error, want, "{models}");
            assert_eq!(err.status, 400);
        }
        // A model that is neither a string nor an object, and the array's own bound.
        let mut body = summrise_like();
        body["models"] = json!([42]);
        let err = parse(&body).expect_err("a number is not a model");
        assert_eq!(
            err.error,
            "models[0] must be a model id string or an object"
        );
        let mut body = summrise_like();
        body["models"] = json!(vec!["m"; 201]);
        let err = parse(&body).expect_err("too many");
        assert_eq!(err.error, "models: at most 200 entries (got 201)");
    }

    /// **THE MODEL'S FACETS, ONE ROW PER FIELD**, because each is a different coercion: a name that trims to
    /// nothing is DROPPED, `input` without `image` sets no vision, and `reasoningEffort` is refused BY NAME.
    #[test]
    fn a_model_field_by_field() {
        let one = |m: Value| {
            let mut body = summrise_like();
            body["models"] = json!([m]);
            parse(&body).map(|s| s["models"][0].clone())
        };
        assert_eq!(
            one(json!({ "id": "x", "name": "  " })).expect("ok"),
            json!({ "id": "x" })
        );
        assert_eq!(
            one(json!({ "id": "x", "name": " Nice " })).expect("ok")["name"],
            json!("Nice")
        );
        assert!(one(json!({ "id": "x", "input": ["text"] }))
            .expect("ok")
            .get("vision")
            .is_none());
        assert_eq!(
            one(json!({ "id": "x", "input": "text" }))
                .expect_err("not an array")
                .error,
            "models[0].input must be an array of text/image"
        );
        assert_eq!(
            one(json!({ "id": "x", "reasoningEffort": "MAX" }))
                .expect_err("not a level")
                .error,
            "models[0].reasoningEffort must be one of low, medium, high, max"
        );
        assert_eq!(
            one(json!({ "id": "x", "reasoningEffort": "max" })).expect("ok")["reasoningEffort"],
            json!("max")
        );
        // `Number(v)` is a COERCION: the string "5" is five, and 1.5 is not an integer.
        assert_eq!(
            one(json!({ "id": "x", "maxTokens": "5" })).expect("ok")["maxTokens"],
            json!(5)
        );
        assert_eq!(
            one(json!({ "id": "x", "maxTokens": 1.5 }))
                .expect_err("fractional")
                .error,
            "models[0].maxTokens must be a positive integer"
        );
        // A body that is not an object at all, and the prefix's own shape.
        let err = parse(&json!([1, 2])).expect_err("an array body");
        assert_eq!(err.error, "body must be a JSON object");
        // **A LEADING DIGIT IS LEGAL** (`^[a-z0-9]`), which is why `1x/` is asserted as VALID below rather
        // than listed as bad — the first version of this test listed it and the parser was right.
        let mut digit = summrise_like();
        digit["prefix"] = json!("1x/");
        assert_eq!(
            parse(&digit).expect("a leading digit is legal")["prefix"],
            json!("1x/")
        );
        for bad in ["my", "My/", "my//", "/", "x".repeat(31).as_str()] {
            let mut body = summrise_like();
            body["prefix"] = json!(bad);
            let err = parse(&body).expect_err(bad);
            assert_eq!(
                err.error,
                format!(
                    "prefix must be a lowercase channel prefix ending in \"/\" (e.g. \"my/\") — got {}",
                    quote(bad)
                ),
                "{bad}"
            );
        }
        // The label defaults to the bare prefix, and its two refusals are the length and a control character.
        let mut body = summrise_like();
        body.as_object_mut().expect("object").remove("label");
        assert_eq!(parse(&body).expect("default label")["label"], json!("my"));
        body["label"] = json!("x".repeat(81));
        assert_eq!(
            parse(&body).expect_err("too long").error,
            "label must be at most 80 characters, with no control characters"
        );
        body["label"] = json!("ok\u{7f}");
        assert_eq!(
            parse(&body).expect_err("a control character").error,
            "label must be at most 80 characters, with no control characters"
        );
    }

    /// **`barePrefix` AND `advertisedModelId`, WHICH THE EMISSION SHIPS BESIDE THE PARSER.** The `?? ""` arm is
    /// the one a port loses: `String(null)` is the text `"null"`, and a provider with no prefix once advertised
    /// `null/foo`.
    #[test]
    fn the_prefix_helpers_keep_the_null_arm() {
        assert_eq!(bare_prefix(&json!("my/")), "my");
        assert_eq!(bare_prefix(&json!("my///")), "my");
        assert_eq!(
            bare_prefix(&Value::Null),
            "",
            "String(null ?? \"\") is the empty string"
        );
        assert_eq!(bare_prefix(&json!(5)), "5", "a coerced number");
        assert_eq!(bare_prefix(&json!({})), "[object Object]");
        assert_eq!(
            advertised_model_id("acme", &json!("acme/chat")),
            "acme/chat"
        );
        assert_eq!(advertised_model_id("acme", &json!("/chat")), "acme/chat");
        assert_eq!(advertised_model_id("acme/", &json!("chat")), "acme/chat");
        assert_eq!(
            advertised_model_id("acme", &Value::Null),
            "acme/",
            "String(null ?? \"\") is empty, NOT the text null"
        );
    }
}

/// The emitted module's header, and then the module.
///
/// **THE TEXT BELOW IS THE SOURCE'S OWN CODE, WITH ONE SUBSTITUTION**: the `try { url = new URL(rawBase) }`
/// block became a `base` ARGUMENT, because the parse is the host's (see the module header). Everything else —
/// every sentence, every coercion, every field assignment, in the source's order — is what
/// `gateway/src/store/providers.ts` shipped.
const TS_MODULE: &str = r#"/**
 * providers-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `core/summrise-core/src/provider_spec.rs` (`render_typescript`). The freshness test in that module
 * is a byte comparison against this file, so a hand edit fails there with the text and one command to fix it:
 *
 *     SUMMRISE_REFRESH_PROVIDERS=1 cargo test --manifest-path core/summrise-core/Cargo.toml provider_spec
 *
 * `gateway/src/store/providers.ts` re-exports this module and keeps the HOST: the KV reads and writes, the
 * cache, the key resolution, and the `new URL` call. **THE DECISIONS ARE HERE** — the vocabulary, the two
 * prefix helpers, and the parser whose every refusal is a response byte.
 *
 * `parseProviderSpec` TAKES THE PARSED URL, because the core cannot own a WHATWG parser: the host calls
 * `providerBaseText(body)`, parses it, and hands the parts over. **THE REFUSAL ORDER IS UNCHANGED BY THAT**:
 * the host parses before the call and the decision reads `base` only after the prefix, reserved-prefix and
 * label refusals — so a body that is wrong twice gets the refusal the shipping console gives it.
 *
 * **AND IT IMPORTS THE SSRF GUARD RATHER THAN RESTATING IT**: `deviceHostError` is itself an emission (from
 * `gateway/wasm/src/device.rs`), and a second copy of a private-address list is the one duplication in this
 * repository that is a HOLE rather than a bug.
 */

import { deviceHostError } from "../device-fetch.ts";

/** The fields a provider model may declare, in the order the refusal prints them. */
export const MODEL_FIELDS: string[] = [
  "id",
  "name",
  "input",
  "contextWindow",
  "maxTokens",
  "reasoningEffort",
];

/** The `input` values this gateway understands; anything else is refused rather than stored-and-ignored. */
export const KNOWN_MODALITIES: string[] = ["text", "image"];

/** The four reasoning levels the router can speak, in the order the refusal prints them. */
export const REASONING_EFFORTS: string[] = ["low", "medium", "high", "max"];

/** Dialect → the path APPENDED to `baseURL`. One row today; a second is a row here plus its translator. */
export const SUPPORTED_PROVIDER_APIS: Record<string, string> = {
  "openai-completions": "/chat/completions",
};

/** The dialects an operator reaches for first, and WHY this build does not serve them. */
export const KNOWN_UNSUPPORTED_APIS: Record<string, string> = {
  "anthropic-messages":
    "this gateway has no OpenAI→Anthropic request translator, so the provider would serve /v1/messages and fail every OpenAI-format client",
};

/** Every prefix a CUSTOM provider may not claim, **in the order the 409 prints them**. */
export const RESERVED_PREFIXES: string[] = [
  "og",
  "or",
  "nv",
  "gmi",
  "qw",
  "cm",
  "r4",
  "none",
  "ds",
  "amd",
];

/** One model of a custom provider. */
export interface ProviderModel {
  /** The WIRE name — what the upstream is asked for. */
  id: string;
  /** Display name, advertised on /v1/models. */
  name?: string;
  /** DSH `input: [text, image]`: the model sees images itself, so gateway-side vision preprocessing must stay
   *  OUT of the way. */
  vision?: boolean;
  /** Declared limits, advertised on /v1/models as context_window/max_tokens. */
  contextWindow?: number;
  maxTokens?: number;
  /** Default reasoning effort when the client sends none. */
  reasoningEffort?: "low" | "medium" | "high" | "max";
}

/** A custom provider record (one `providers:custom` entry). */
export interface ProviderSpec {
  /** Routing prefix WITH its trailing slash — the ROUTE_INFO spelling ("my/"). */
  prefix: string;
  /** Console/route-card name. Defaults to the prefix without its slash. */
  label: string;
  /** Upstream root; the dialect's path is APPENDED to it. */
  baseURL: string;
  /** Wire dialect — a key of SUPPORTED_PROVIDER_APIS. */
  api: string;
  /** NAME of a Worker env binding/secret holding the key. */
  apiKeyEnv?: string;
  /** Inline key. NEVER returned by the admin API. */
  apiKey?: string;
  models: ProviderModel[];
}

/** Why a provider record was refused, and with which HTTP status. 400 = wrong; 409 = a reserved prefix. */
export interface ProviderParseError {
  error: string;
  status: number;
}

/** The URL's parsed parts, as the host's parser produced them. `protocol` carries its colon. */
export interface ParsedBase {
  protocol: string;
  username: string;
  password: string;
  search: string;
  hash: string;
  hostname: string;
  origin: string;
  pathname: string;
}

/** `barePrefix(prefix)` — the TRAILING slashes come off. `?? ""` is a COERCION, not a null check: a number
 *  becomes its digits and an object becomes `[object Object]`. */
export function barePrefix(prefix: unknown): string {
  const text = prefix === null || prefix === undefined ? "" : String(prefix);
  return text.replace(/\/+$/, "");
}

/** The advertised id of a wire model under a prefix — prefix ONCE, never twice. */
export function advertisedModelId(prefix: string, wire: unknown): string {
  const bare = barePrefix(prefix);
  const w = (wire === null || wire === undefined ? "" : String(wire)).replace(/^\/+/, "");
  return w.startsWith(bare + "/") ? w : `${bare}/${w}`;
}

/** `String(body.baseURL ?? "").trim()` — the text the host's URL parser is handed. One owner, because a second
 *  coercion in the host would be the twin this module exists to delete. */
export function providerBaseText(body: any): string {
  return String(body?.baseURL ?? "").trim();
}

/** One entry of `models`, or the message the route answers with. */
export function parseProviderModel(
  raw: any,
  at: number,
): { model?: ProviderModel; error?: string } {
  const where = `models[${at}]`;
  if (typeof raw === "string") {
    const id = raw.trim();
    return id ? { model: { id } } : { error: `${where}: empty model id` };
  }
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    return { error: `${where} must be a model id string or an object` };
  }
  const unknown = Object.keys(raw).filter((k) => !MODEL_FIELDS.includes(k));
  if (unknown.length) {
    return {
      error: `${where}: unsupported field(s) ${unknown.join(", ")} — this gateway stores ${MODEL_FIELDS.join(", ")}`,
    };
  }
  const id = String(raw.id ?? "").trim();
  if (!id) return { error: `${where}: id is required` };
  if (/\s/.test(id)) return { error: `${where}.id must not contain whitespace` };
  const model: ProviderModel = { id };
  if (raw.name !== undefined) {
    const name = String(raw.name).trim();
    if (name) model.name = name;
  }
  if (raw.input !== undefined) {
    if (!Array.isArray(raw.input)) {
      return { error: `${where}.input must be an array of ${KNOWN_MODALITIES.join("/")}` };
    }
    const bad = raw.input.filter((m: any) => !KNOWN_MODALITIES.includes(String(m)));
    if (bad.length) {
      return {
        error: `${where}.input: unsupported modalit${bad.length > 1 ? "ies" : "y"} ${bad
          .map((b: any) => JSON.stringify(String(b)))
          .join(", ")} — only ${KNOWN_MODALITIES.join(" and ")} are understood`,
      };
    }
    // A model that declares image input must NOT have its images described by the gateway's vision model
    // (preprocessImages) — it can see them itself.
    if (raw.input.map(String).includes("image")) model.vision = true;
  }
  // The reasoning DEFAULT, on the same terms as a console-added record: a level the router cannot speak is
  // refused BY NAME here rather than dropped, because a typo that silently means "no default" is a behaviour
  // change nobody can see.
  if (raw.reasoningEffort !== undefined) {
    const lv = String(raw.reasoningEffort).trim();
    if (!REASONING_EFFORTS.includes(lv))
      return { error: `${where}.reasoningEffort must be one of ${REASONING_EFFORTS.join(", ")}` };
    model.reasoningEffort = lv as "low" | "medium" | "high" | "max";
  }
  for (const cap of ["contextWindow", "maxTokens"] as const) {
    const v = raw[cap];
    if (v === undefined) continue;
    const n = Number(v);
    if (!Number.isInteger(n) || n <= 0) {
      return { error: `${where}.${cap} must be a positive integer` };
    }
    model[cap] = n;
  }
  return { model };
}

/** The parser. Pure — the admin handler only has to map the error onto a status. */
export function parseProviderSpec(
  body: any,
  base: ParsedBase | null,
  opts: { keepKeyFrom?: { apiKey?: string; apiKeyEnv?: string } } = {},
): { spec?: ProviderSpec } & Partial<ProviderParseError> {
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    return { error: "body must be a JSON object", status: 400 };
  }
  // Lowercase only: routing derives the prefix by splitting the CLIENT's model id on "/", so a mixed-case
  // prefix could never match a lowercase one.
  const prefix = String(body.prefix ?? "").trim();
  if (!/^[a-z0-9][a-z0-9-]{0,30}\/$/.test(prefix)) {
    return {
      error: `prefix must be a lowercase channel prefix ending in "/" (e.g. "my/") — got ${JSON.stringify(prefix)}`,
      status: 400,
    };
  }
  const bare = barePrefix(prefix);
  if (RESERVED_PREFIXES.includes(bare)) {
    return {
      error: `prefix ${JSON.stringify(prefix)} is reserved by a built-in channel (reserved: ${RESERVED_PREFIXES.join(", ")})`,
      status: 409,
    };
  }
  const label = String(body.label ?? "").trim() || bare;
  // eslint-disable-next-line no-control-regex
  if (label.length > 80 || /[\u0000-\u001f\u007f]/.test(label)) {
    return {
      error: "label must be at most 80 characters, with no control characters",
      status: 400,
    };
  }
  const rawBase = providerBaseText(body);
  if (!base) {
    return { error: `baseURL is not a URL — got ${JSON.stringify(rawBase)}`, status: 400 };
  }
  const url = base;
  if (url.protocol !== "https:") {
    return {
      error: `baseURL must be https (got ${url.protocol}//) — a provider key must never travel in clear`,
      status: 400,
    };
  }
  if (url.username || url.password) {
    return { error: "baseURL must not embed credentials", status: 400 };
  }
  if (url.search || url.hash) {
    return { error: "baseURL must not carry a query or a fragment", status: 400 };
  }
  // The SSRF check, REUSED (device-fetch.ts) — loopback/private/link-local/metadata hosts are refused here for
  // the same reason a device hostname is: the gateway would dial them with a credential attached.
  const hostErr = deviceHostError(url.hostname);
  if (hostErr) return { error: `baseURL host rejected: ${hostErr}`, status: 400 };
  // Treated as a PREFIX, not resolved against (see providerRoute): a deployment path such as
  // https://host/openai/v1 must survive.
  const baseURL = (url.origin + url.pathname).replace(/\/+$/, "");
  const api = String(body.api ?? "").trim();
  if (!Object.prototype.hasOwnProperty.call(SUPPORTED_PROVIDER_APIS, api)) {
    const why = KNOWN_UNSUPPORTED_APIS[api];
    return {
      error:
        `unsupported protocol ${JSON.stringify(api)} — this gateway serves ` +
        `${Object.keys(SUPPORTED_PROVIDER_APIS).join(", ")}${why ? ` (${api}: ${why})` : ""}`,
      status: 400,
    };
  }
  const apiKeyEnv = String(body.apiKeyEnv ?? "").trim();
  const apiKey = String(body.apiKey ?? "").trim();
  if (apiKeyEnv && apiKey) {
    return {
      error:
        "set apiKeyEnv (a deployed Worker secret) or apiKey (inline), not both — two key sources cannot both be the one this provider sends",
      status: 400,
    };
  }
  // AN UPDATE MAY OMIT THE KEY; A CREATE MAY NOT: `POST /api/admin/providers` is the edit path for a record the
  // console owns, and the admin view never returns the key, only `keyMasked`. The caller passes what the STORED
  // record already has, and only for an existing prefix.
  const keep = !apiKeyEnv && !apiKey ? opts.keepKeyFrom : undefined;
  if (!apiKeyEnv && !apiKey && !keep) {
    return {
      error:
        "a key is required: apiKeyEnv (the NAME of a Worker secret, DSH's settings.yaml spelling) or apiKey (an inline value stored in KV)",
      status: 400,
    };
  }
  if (apiKeyEnv && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(apiKeyEnv)) {
    return {
      error: `apiKeyEnv must be a Worker env/secret NAME (letters, digits, "_" — e.g. SUMMRISE_API_KEY), not a value — got ${JSON.stringify(apiKeyEnv)}`,
      status: 400,
    };
  }
  if (apiKey && (apiKey.length < 8 || /\s/.test(apiKey))) {
    // <8 also keeps the credential out of redactSecrets' "too generic to replace safely" bucket, so an
    // upstream echo of it is still redacted.
    return { error: "apiKey must be at least 8 characters with no whitespace", status: 400 };
  }
  if (!Array.isArray(body.models) || body.models.length === 0) {
    return { error: "models must be a non-empty array", status: 400 };
  }
  if (body.models.length > 200) {
    return { error: `models: at most 200 entries (got ${body.models.length})`, status: 400 };
  }
  const models: ProviderModel[] = [];
  for (let i = 0; i < body.models.length; i++) {
    const { model, error } = parseProviderModel(body.models[i], i);
    if (error || !model) return { error: error || `models[${i}] is invalid`, status: 400 };
    // Normalize to the WIRE form: the full-id spelling (prefix included) is accepted, stored once, and never
    // advertised twice.
    const wire = model.id.startsWith(bare + "/") ? model.id.slice(bare.length + 1) : model.id;
    if (!wire) return { error: `models[${i}]: empty model id after the prefix`, status: 400 };
    if (models.some((m) => m.id === wire)) {
      return {
        error: `models[${i}]: duplicate model id ${advertisedModelId(bare, wire)}`,
        status: 400,
      };
    }
    models.push({ ...model, id: wire });
  }
  const spec: ProviderSpec = { prefix, label, baseURL, api, models };
  if (apiKeyEnv) spec.apiKeyEnv = apiKeyEnv;
  else if (apiKey) spec.apiKey = apiKey;
  // The stored key, carried forward because the body omitted it — never a synthesis: an update that DOES send
  // one replaces it, which is how key rotation must keep working.
  else if (keep?.apiKeyEnv) spec.apiKeyEnv = keep.apiKeyEnv;
  else if (keep?.apiKey) spec.apiKey = keep.apiKey;
  return { spec };
}
"#;

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_MODULE.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../gateway/src/store/providers-logic.ts")
}

#[cfg(test)]
mod emission_tests {
    use super::*;

    /// **THE FRESHNESS CHECK — AND IT IS THE REASON THE EMISSION CANNOT DRIFT.** The same shape `mcp_tools.rs`
    /// and `store/settings-logic.ts` use: the committed file IS `render_typescript()`'s bytes.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_PROVIDERS").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write providers-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_PROVIDERS=1 cargo test provider_spec",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/store/providers-logic.ts is not what provider_spec.rs emits — \
             run SUMMRISE_REFRESH_PROVIDERS=1 cargo test provider_spec"
        );
    }

    /// **THE EMISSION MUST CARRY EVERY SENTENCE THE ROUTE ANSWERS WITH**, because a missing one is a 500 rather
    /// than a refusal. This is a floor on the TEXT, not a substitute for the byte comparison above: it fails
    /// when a rule is dropped from the renderer rather than when a byte moves.
    ///
    /// MUTATION: delete the `apiKeyEnv must be a Worker env/secret NAME` arm from the emitted text.
    /// RESULT:   this test fails naming the sentence — and the console would answer `undefined` to an operator
    ///           who pasted a key VALUE into the NAME field.
    #[test]
    fn the_emission_carries_every_refusal() {
        let ts = render_typescript();
        for sentence in [
            "body must be a JSON object",
            "prefix must be a lowercase channel prefix ending in",
            "is reserved by a built-in channel (reserved: ",
            "label must be at most 80 characters, with no control characters",
            "baseURL is not a URL",
            "baseURL must be https (got ",
            "baseURL must not embed credentials",
            "baseURL must not carry a query or a fragment",
            "baseURL host rejected: ",
            "unsupported protocol ",
            "set apiKeyEnv (a deployed Worker secret) or apiKey (inline), not both",
            "a key is required: apiKeyEnv (the NAME of a Worker secret",
            "apiKeyEnv must be a Worker env/secret NAME",
            "apiKey must be at least 8 characters with no whitespace",
            "models must be a non-empty array",
            "models: at most 200 entries (got ",
            "empty model id after the prefix",
            "duplicate model id ",
            "empty model id",
            "must be a model id string or an object",
            "unsupported field(s) ",
            "id is required",
            ".id must not contain whitespace",
            ".input must be an array of ",
            ".input: unsupported modalit",
            ".reasoningEffort must be one of ",
            // The cap arm is INTERPOLATED (`${where}.${cap}`), so the floor on it is the template itself:
            // asserting the two expanded sentences would be asserting the test's own expansion.
            "${where}.${cap} must be a positive integer",
        ] {
            assert!(ts.contains(sentence), "the emission lost: {sentence}");
        }
        // AND THE VOCABULARY, WHICH IS THE OTHER HALF OF THE WIRE.
        assert!(ts.contains("\"openai-completions\": \"/chat/completions\""));
        assert!(ts.contains("\"anthropic-messages\":"));
        assert!(ts.contains("\"og\",\n  \"or\",\n  \"nv\",\n  \"gmi\",\n  \"qw\",\n  \"cm\",\n  \"r4\",\n  \"none\",\n  \"ds\",\n  \"amd\","));
    }
}
