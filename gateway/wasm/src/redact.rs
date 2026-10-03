//! THE TWO REDACTION FUNCTIONS, MOVED FROM `gateway/src/plugins/translate.ts`.
//!
//! WHY THESE TWO FIRST, out of a 1,696-line plugin. The plugin's route is not portable in one piece —
//! it reads keys, channels, KV and upstreams — but P3's unit is the ROUTE and the plan's method is to
//! take the PURE LOGIC inside it first, prove it against the shipping TypeScript, and leave the I/O at
//! the edge. These two are pure `String -> String`, they are the plugin's own safety net (a key that
//! reaches a log or a client is the failure they exist to prevent), and eleven call sites inside the
//! plugin depend on them.
//!
//! BOTH ARE TRANSLITERATED RATHER THAN REINVENTED, and the two places where that matters are called out
//! where they live: the regex in `scrub_keys` is hand-rolled because `\b` and a GREEDY character class
//! are the semantics, not an approximation of them; and `redact_secrets` replaces SEQUENTIALLY after a
//! STABLE sort, so a short secret nested inside a long one is already gone by the time its turn comes.

/// `scrubKeys(msg)`: `String(msg || "").replace(/\b(?:sk|rc|sc|or|xox[baprs])-[A-Za-z0-9_-]{8,}/g, "***")`.
///
/// `String(msg || "")` IS NOT DECORATION: a falsy input answers the empty string, and a truthy
/// non-string is COERCED (the same rule the panel's `to_js_string` carries). A `&str` cannot express
/// either, so the JavaScript value comes in as `&Value`.
pub fn scrub_keys(msg: &serde_json::Value) -> String {
    let text = match msg {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        // `String([...])` joins with commas and `String({})` is "[object Object]" — the engine's own
        // coercion, which is what the JavaScript actually does here.
        serde_json::Value::Array(items) => items
            .iter()
            .map(coerce_element)
            .collect::<Vec<_>>()
            .join(","),
        serde_json::Value::Object(_) => "[object Object]".to_string(),
    };
    scrub_into(&text)
}

fn coerce_element(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Array(_) => "[object Object]".to_string(),
        serde_json::Value::Object(_) => "[object Object]".to_string(),
    }
}

/// The regex itself, as a scanner. `\b` is a WORD BOUNDARY (the preceding character must not be a word
/// character), the prefix alternation is tried in the source's own order, and `[A-Za-z0-9_-]{8,}` is
/// GREEDY — which is why `sk-or-v1-abcdefgh` is consumed whole rather than at its first `sk-`.
fn scrub_into(text: &str) -> String {
    const PREFIXES: [&str; 5] = ["sk", "rc", "sc", "or", "xoxb"];
    const XOX: [&str; 5] = ["xoxb", "xoxa", "xoxp", "xoxr", "xoxs"];
    let bytes = text.as_bytes();
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let is_class = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'-';
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < bytes.len() {
        // A boundary: the start of the string, or a non-word character before it.
        let boundary = i == 0 || !is_word(bytes[i - 1]);
        let mut matched = 0usize;
        if boundary {
            for prefix in PREFIXES {
                let is_xox = prefix.starts_with("xox");
                if is_xox {
                    // `xox[baprs]` — the fifth character decides.
                    if bytes.len() >= i + 4
                        && &text[i..i + 3] == "xox"
                        && XOX.contains(&&text[i..i + 4])
                    {
                        matched = 4;
                    }
                } else if text[i..].starts_with(prefix) {
                    matched = prefix.len();
                }
                if matched > 0 {
                    break;
                }
            }
        }
        if matched > 0 && bytes.len() > i + matched && bytes[i + matched] == b'-' {
            let start = i + matched + 1;
            let mut end = start;
            while end < bytes.len() && is_class(bytes[end]) {
                end += 1;
            }
            if end - start >= 8 {
                out.push_str("***");
                i = end;
                continue;
            }
        }
        // Not a match: copy one CHARACTER (not one byte) and carry on.
        let ch = text[i..].chars().next().expect("a character");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `redactSecrets(text, secrets)`: keep the truthy secrets of length >= 8, DEDUPE them (`new Set`,
/// which is insertion-ordered), sort by length DESCENDING with a STABLE sort, then replace each one
/// everywhere — IN THAT ORDER, so a short secret that sat inside a long one is already `***` and cannot
/// match again.
pub fn redact_secrets(text: &str, secrets: &[serde_json::Value]) -> String {
    let mut distinct: Vec<String> = Vec::new();
    for s in secrets {
        // **`!!s && s.length >= 8` IS TWO TESTS, AND THE SECOND ONE IS NOT "COERCE THEN MEASURE".** A
        // NUMBER has no `.length` — it is `undefined`, and `undefined >= 8` is FALSE — so a number is
        // FILTERED OUT rather than stringified and redacted. The corpus has that case ("a non-string
        // entry": the TypeScript answers `12345678 here` untouched) and this port got it wrong until the
        // corpus said so. Only strings reach here in the typed signature; the runtime rule is mirrored
        // anyway, because the runtime is what the deployed worker does.
        let value = match s {
            serde_json::Value::String(v) => v.clone(),
            _ => continue,
        };
        if value.is_empty() || value.chars().count() < 8 {
            continue;
        }
        if !distinct.contains(&value) {
            distinct.push(value);
        }
    }
    // `distinct.sort((a, b) => b.length - a.length)` — and JavaScript's sort is STABLE, so equal
    // lengths keep the order they were first seen in. `sort_by` here is stable for the same reason.
    distinct.sort_by_key(|s| std::cmp::Reverse(s.chars().count()));
    let mut out = text.to_string();
    for secret in distinct {
        // `out.split(s).join("***")` — a literal, global replacement.
        out = out.split(secret.as_str()).collect::<Vec<_>>().join("***");
    }
    out
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `redact` cases were produced by the SHIPPING TypeScript
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
    fn every_redaction_case_matches_the_shipping_typescript() {
        let doc = corpus();
        let cases = doc["cases"].as_array().expect("cases");
        let mut checked = 0;
        for case in cases {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            // DISJOINT FROM THE OTHER CORPUS TEST, for the reason written there.
            if !matches!(func, "scrub_keys" | "redact_secrets") {
                continue;
            }
            let expected = case["expected"]["text"]
                .as_str()
                .unwrap_or_else(|| panic!("{func} / {name}: the corpus has no expectation"));
            let got = match func {
                "scrub_keys" => scrub_keys(&case["input"]),
                "redact_secrets" => redact_secrets(
                    case["input"]["text"].as_str().unwrap_or(""),
                    case["input"]["secrets"].as_array().expect("secrets"),
                ),
                other => panic!("unknown redaction function in the corpus: {other}"),
            };
            assert_eq!(got, expected, "{func} / {name}");
            checked += 1;
        }
        assert!(
            checked >= 10,
            "the redaction corpus shrank to {checked} cases"
        );
    }
}
