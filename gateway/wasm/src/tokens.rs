//! THE TOKEN ESTIMATION, MOVED FROM `gateway/src/body-scan.ts` — the other half of the CPU red line.
//!
//! WHY THIS HALF IS SEPARATE. `count_tokens` only needs a context-budget estimate (±20% is fine by the
//! module's own words), but the estimate must be CHEAP at any body size and must not under-count
//! Chinese-heavy prompts — the recorded incident is a body just over the old 1M walk limit that estimated
//! ~0.33 tokens/char against a real ~1.8, so the client's context accounting said it fit, no compaction
//! ran, and the upstream rejected with `request_too_large`. The fix was sampling rather than a
//! discontinuous branch, and this keeps it.
//!
//! **LENGTH MEANS UTF-16 CODE UNITS, NOT BYTES.** The JavaScript's `s.length` counts code units and
//! `charCodeAt(i) < 128` tests one of them; a port that used `str::len()` would count BYTES and a
//! Chinese-heavy body would come out roughly three times its real length — the exact error the recorded
//! incident is about, arriving from the other side. Every length below is `encode_utf16().count()`, and
//! the corpus has CJK cases that would catch it.

/// `ESTIMATE_SAMPLE` — 128K chars. Walking every char of a 1M-char body costs ~17 ms and alone blows the
/// 10 ms budget.
const ESTIMATE_SAMPLE: usize = 128 * 1024;

/// The per-image allowance: ~1600 tokens, the real vision cost. Counting image blocks as text
/// overestimates by ~580x.
const PER_IMAGE_TOKENS: i64 = 1600;

/// The UTF-16 length of a Rust string — what `s.length` is in the JavaScript.
fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The UTF-16 units, for the code-unit walks below.
fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// `countBase64Payloads(s)`: image accounting over a raw body.
///
/// Three CPU contracts are preserved and each is why this is not the obvious loop:
///   * the scan STARTS AT THE LAST `"role":"user"` (images only appear there, so a multi-MB history is not
///     re-scanned);
///   * it jumps with `indexOf` rather than a full-string regex pass;
///   * and it tests bytes with code-unit RANGES rather than per-char regex (~10x faster on an all-base64
///     body, same semantics: A-Z a-z 0-9 + / =).
///
/// Only runs of >= 512 base64 chars after a `"data":"` key count, and `removedChars` accumulates ALL of
/// them — the caller subtracts them from the text length.
pub fn count_base64_payloads(s: &str) -> (i64, i64) {
    const DATA_KEY: &str = "\"data\":\"";
    const USER_ROLE: &str = "\"role\":\"user\"";
    let u = units(s);
    let len = u.len();
    let is_b64 = |c: u16| {
        (48..=57).contains(&c)
            || (65..=90).contains(&c)
            || (97..=122).contains(&c)
            || c == 43
            || c == 47
            || c == 61
    };
    // `s.lastIndexOf(USER_ROLE)` — over code units, so a byte search would be wrong for non-ASCII bodies.
    let scan_start = last_index_of(&u, &units(USER_ROLE)).unwrap_or(0);
    let key = units(DATA_KEY);
    let mut images = 0i64;
    let mut removed_chars = 0i64;
    let mut search_from = scan_start;
    while let Some(idx) = index_of_from(&u, &key, search_from) {
        let mut j = idx + key.len();
        let start = j;
        while j < len && is_b64(u[j]) {
            j += 1;
        }
        if j - start >= 512 {
            images += 1;
            removed_chars += (j - start) as i64;
            search_from = j;
        } else {
            search_from = idx + key.len();
        }
    }
    (images, removed_chars)
}

fn index_of_from(hay: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

fn last_index_of(hay: &[u16], needle: &[u16]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len())
        .rev()
        .find(|&i| &hay[i..i + needle.len()] == needle)
}

/// `estimateTextTokens(s, textLen)`: CJK-aware density over a SAMPLED head.
///
/// `s.replace(/"data":"[A-Za-z0-9+/=]{512,}"/g, '"data":"<base64>"')` is hand-rolled here: the replacement
/// is a literal with a length-bounded class, and the reason it exists is that the sample must not be
/// dominated by base64 that the caller has already accounted for separately.
pub fn estimate_text_tokens(s: &str, text_len: i64) -> f64 {
    let u = units(s);
    let len = u.len();
    let take = ESTIMATE_SAMPLE.min(len);
    let stripped = strip_base64(&u[..take]);
    let mut ascii = 0i64;
    let mut other = 0i64;
    for c in &stripped {
        if (*c as u32) < 128 {
            ascii += 1;
        } else {
            other += 1;
        }
    }
    // **`0 / 0` IS `NaN`, AND `Math.ceil(NaN)` IS `NaN` — SO THE ANSWER IS A FLOAT, NOT A COUNT.** An
    // empty sample is reachable (an empty body reaches `estimate_text_tokens` from a caller that did not
    // take the early return), and `JSON.stringify(NaN)` is `null` on the wire. A port that answered `0`
    // would be answering a number the JavaScript never produces, so this returns the float it really is
    // and the corpus compares it as JSON does.
    let r = if stripped.is_empty() {
        f64::NAN
    } else {
        text_len as f64 / stripped.len() as f64
    };
    ((ascii as f64 * r) / 4.0 + other as f64 * r * 1.8).ceil()
}

/// The `"data":"<512+ base64>"` runs replaced by `"data":"<base64>"`, over UTF-16 units.
fn strip_base64(u: &[u16]) -> Vec<u16> {
    const NEEDLE: &str = "\"data\":\"";
    const MARKER: &str = "\"data\":\"<base64>\"";
    let key: Vec<u16> = NEEDLE.encode_utf16().collect();
    let marker: Vec<u16> = MARKER.encode_utf16().collect();
    let is_b64 = |c: u16| {
        (48..=57).contains(&c)
            || (65..=90).contains(&c)
            || (97..=122).contains(&c)
            || c == 43
            || c == 47
            || c == 61
    };
    let mut out: Vec<u16> = Vec::with_capacity(u.len());
    let mut i = 0usize;
    while i < u.len() {
        if i + key.len() <= u.len() && u[i..i + key.len()] == key[..] {
            let mut j = i + key.len();
            let start = j;
            while j < u.len() && is_b64(u[j]) {
                j += 1;
            }
            // `{512,}` — GREEDY, and the run must be long enough to be a payload.
            if j - start >= 512 {
                out.extend_from_slice(&marker);
                i = j;
                continue;
            }
        }
        out.push(u[i]);
        i += 1;
    }
    out
}

/// `estimateTokens(jsonStr)` — the thin composer, keeping the exact historical arithmetic: subtract ALL
/// scanned base64, then charge 1600 per image on top.
pub fn estimate_tokens(json_str: &serde_json::Value) -> f64 {
    let s = js_string_of(json_str);
    let (images, removed_chars) = count_base64_payloads(&s);
    let text_len = js_len(&s) as i64 - removed_chars;
    if text_len <= 0 {
        return (images * PER_IMAGE_TOKENS) as f64;
    }
    estimate_text_tokens(&s, text_len) + (images * PER_IMAGE_TOKENS) as f64
}

/// `String(x)` for the shapes a caller can pass — the engine's own coercion, because the parameter is
/// typed `any` and the deployed worker really can be handed a number here.
fn js_string_of(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(|i| match i {
                serde_json::Value::Null => String::new(),
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Number(n) => n.to_string(),
                _ => "[object Object]".to_string(),
            })
            .collect::<Vec<_>>()
            .join(","),
        serde_json::Value::Object(_) => "[object Object]".to_string(),
    }
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `tokens` cases were produced by the SHIPPING TypeScript
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

    /// **A WHOLE NUMBER IS NOT A FLOAT ON THE WIRE, AND `NaN` IS `null`.** `JSON.stringify(100)` is `100`
    /// while `serde_json::json!(100.0)` is `100.0`, and `JSON.stringify(NaN)` is `null` — so comparing the
    /// two Values would fail on two differences the JavaScript does not have. Both sides are compared as
    /// what a JS number IS, with the NaN encoding the oracle recorded.
    fn same_number(got: f64, want: &serde_json::Value) -> bool {
        if want.is_null() {
            return got.is_nan();
        }
        match want.as_f64() {
            Some(w) => got == w,
            None => false,
        }
    }

    #[test]
    fn every_token_estimate_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            match func {
                "count_base64_payloads" => {
                    let (images, removed) = count_base64_payloads(input.as_str().unwrap_or(""));
                    let got = serde_json::json!({ "images": images, "removedChars": removed });
                    assert_eq!(&got, want, "{func} / {name}");
                }
                "estimate_text_tokens" => {
                    let got = estimate_text_tokens(
                        input["s"].as_str().unwrap_or(""),
                        input["textLen"].as_i64().unwrap_or(0),
                    );
                    assert!(
                        same_number(got, want),
                        "{func} / {name}: {got} against {want}"
                    );
                }
                "estimate_tokens" => {
                    let got = estimate_tokens(&input["value"]);
                    assert!(
                        same_number(got, want),
                        "{func} / {name}: {got} against {want}"
                    );
                }
                _ => continue,
            }
            checked += 1;
        }
        assert!(checked >= 15, "the token corpus shrank to {checked} cases");
    }
}
