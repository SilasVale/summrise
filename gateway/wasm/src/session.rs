//! THE SESSION AND HEADER DECISIONS, MOVED FROM `gateway/src/upstream.ts`.
//!
//! WHY THESE. `zen/go` requires `x-opencode-session` on every request and 400s without it — the recorded
//! breakage is the muse-spark 1.3 one — and it wants a STABLE per-conversation identifier so it can route
//! requests and reuse prompt caches. The gateway relays the client's own id when there is one and
//! synthesizes a stable per-user value otherwise, derived WITHOUT KV. Every piece of that is pure, and
//! every piece is a byte the upstream sees.
//!
//! `wireModelName` IS DELIBERATELY NOT HERE, named rather than approximated: its alias table
//! (`OG_WIRE_REMAP`) is DERIVED from `MODEL_REGISTRY`, so porting it faithfully means porting the
//! registry's wire facet with it.

/// `SESSION_SALT` — a constant application salt, not a secret.
const SESSION_SALT: &str = "summrise-og-session-v1";

/// `stripBracket(s)`: `s.replace(/\[[^\]]*\]$/, "")`.
///
/// The class cannot contain `]`, so the match is the LAST bracket group and it must END the string —
/// `"a[b]c"` is untouched, `"[a][b]"` loses only `[b]`, and `"a["` loses nothing.
pub fn strip_bracket(s: &str) -> String {
    let units: Vec<char> = s.chars().collect();
    if units.last() != Some(&']') {
        return s.to_string();
    }
    // Find the opening bracket of the group that closes at the end, with no `]` inside it.
    let mut open: Option<usize> = None;
    for i in (0..units.len()).rev() {
        if units[i] == '[' {
            open = Some(i);
            break;
        }
        if units[i] == ']' && i != units.len() - 1 {
            // A `]` before the last one means the group would contain a `]` — impossible for the class.
            return s.to_string();
        }
    }
    match open {
        Some(i) => units[..i].iter().collect(),
        None => s.to_string(),
    }
}

/// `fnvHex(s)`: FNV-1a 64-bit, FOLDED INTO TWO 32-BIT HALVES and printed as 16 hex characters.
///
/// `Math.imul(a, b) >>> 0` is a 32-bit multiply keeping the low half, unsigned — `wrapping_mul` on a
/// `u32` is the same operation, and the fold is what makes this two halves rather than one 64-bit hash.
/// Deliberately non-cryptographic: the value is a routing and cache key, not a secret.
///
/// **`charCodeAt` IS A UTF-16 CODE UNIT**, so a non-ASCII uid hashes the units and not the bytes; the
/// corpus carries one.
pub fn fnv_hex(s: &str) -> String {
    let mut h1: u32 = 0x811c_9dc5;
    let mut h2: u32 = 0x0100_0193;
    for c in s.encode_utf16() {
        h1 = (h1 ^ (c as u32)).wrapping_mul(0x0100_0193);
        h2 = (h2 ^ (c as u32)).wrapping_mul(0x85eb_ca6b);
    }
    format!("{h1:08x}{h2:08x}")
}

/// `syntheticSessionId(uid)` — the stable per-user fallback, derived without KV.
pub fn synthetic_session_id(uid: &str) -> String {
    format!("summrise-{}", fnv_hex(&format!("{SESSION_SALT}:{uid}")))
}

/// `headerValue(headers, name)`: `h.get(name)?.trim() || ""`.
///
/// `Headers.get` is CASE-INSENSITIVE, which is why this folds the name rather than comparing it, and the
/// `trim` happens BEFORE the truthiness test the callers use — a header of `"   "` is absent, not blank.
fn header_value(headers: &serde_json::Map<String, serde_json::Value>, name: &str) -> String {
    let want = name.to_ascii_lowercase();
    for (k, v) in headers {
        if k.to_ascii_lowercase() == want {
            return v.as_str().unwrap_or("").trim().to_string();
        }
    }
    String::new()
}

/// `clientSessionId(incoming)` — the FIRST non-blank of the four identifiers a client may already carry.
///
/// The order is the contract: native opencode clients, then DSH's pi-ai adapter's per-conversation uuid,
/// then OpenAI/OpenRouter-style ids. Relaying the client's own id keeps ONE cache namespace per real
/// conversation upstream.
pub fn client_session_id(headers: &serde_json::Map<String, serde_json::Value>) -> String {
    for name in [
        "x-opencode-session",
        "x-client-request-id",
        "session_id",
        "x-session-id",
    ] {
        let value = header_value(headers, name);
        if !value.is_empty() {
            return value;
        }
    }
    String::new()
}

/// `opencodeSessionHeader(incoming, uid)` — the composer, and it ALWAYS sets the header.
///
/// The client's id wins verbatim; otherwise the synthetic per-user value. Only attached to requests
/// actually destined for `zen/go`, so the other wires stay untouched.
pub fn opencode_session_header(
    headers: &serde_json::Map<String, serde_json::Value>,
    uid: &str,
) -> Vec<(String, String)> {
    let id = client_session_id(headers);
    let value = if id.is_empty() {
        synthetic_session_id(uid)
    } else {
        id
    };
    vec![("x-opencode-session".to_string(), value)]
}

/// `passthroughHeaders(bearerKey, { apiKeyHeader, extra })` — the header set every passthrough target
/// gets.
///
/// THE CLIENT'S AUTH HEADER IS NEVER FORWARDED: this user's own key is used instead. `apiKeyHeader` is
/// `false` by default, and the truthiness of it is what chooses between the native-Anthropic `x-api-key`
/// form (`zen/go/v1/messages`) and `Bearer` for everything else. `extra`'s values are truthiness-filtered,
/// so an empty string adds no header.
pub fn passthrough_headers(
    bearer_key: Option<&str>,
    api_key_header: Option<&str>,
    extra: &[(String, String)],
) -> Vec<(String, String)> {
    let mut out = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("anthropic-version".to_string(), "2023-06-01".to_string()),
    ];
    if let Some(key) = bearer_key.filter(|k| !k.is_empty()) {
        match api_key_header.filter(|h| !h.is_empty()) {
            Some(name) => out.push((name.to_string(), key.to_string())),
            None => out.push(("Authorization".to_string(), format!("Bearer {key}"))),
        }
    }
    for (name, value) in extra {
        if !value.is_empty() {
            out.push((name.clone(), value.clone()));
        }
    }
    out
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `session` cases were produced by the SHIPPING TypeScript
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

    fn as_map(v: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    /// **THE FETCH `Headers` OBJECT LOWERCASES ITS NAMES**, so the oracle recorded
    /// `content-type`/`authorization` while this module returns the capitalisation its source lists. Both
    /// sides are folded before comparing: the NAME's case is not a byte the upstream sees, and the values
    /// are what matter.
    fn folded(pairs: Vec<(String, String)>) -> serde_json::Map<String, serde_json::Value> {
        pairs
            .into_iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), serde_json::Value::String(v)))
            .collect()
    }

    #[test]
    fn every_session_and_header_case_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            match func {
                "strip_bracket" => {
                    let got = strip_bracket(input.as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                "fnv_hex" => {
                    let got = fnv_hex(input.as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                "synthetic_session_id" => {
                    let got = synthetic_session_id(input.as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                "client_session_id" => {
                    let got = client_session_id(&as_map(input));
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                "opencode_session_header" => {
                    let got = opencode_session_header(
                        &as_map(&input["headers"]),
                        input["uid"].as_str().unwrap_or(""),
                    );
                    let obj: serde_json::Map<String, serde_json::Value> = got
                        .into_iter()
                        .map(|(k, v)| (k, serde_json::Value::String(v)))
                        .collect();
                    assert_eq!(&serde_json::Value::Object(obj), want, "{func} / {name}");
                }
                "passthrough_headers" => {
                    let extra: Vec<(String, String)> = input["extra"]
                        .as_object()
                        .map(|o| {
                            o.iter()
                                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    let got = passthrough_headers(
                        input["bearerKey"].as_str(),
                        input["apiKeyHeader"].as_str(),
                        &extra,
                    );
                    assert_eq!(
                        &serde_json::Value::Object(folded(got)),
                        want,
                        "{func} / {name}"
                    );
                }
                _ => continue,
            }
            checked += 1;
        }
        assert!(
            checked >= 20,
            "the session corpus shrank to {checked} cases"
        );
    }
}
