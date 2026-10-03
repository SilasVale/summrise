//! THE BODY SCAN AND REWRITE, MOVED FROM `gateway/src/body-scan.ts`.
//!
//! WHY THIS FAMILY. Its own header calls it "the 10ms CPU budget red line": the `/v1/*` hot path must
//! NEVER fully parse and re-stringify a multi-MB body (Workers Free plan, Error 1102), so everything here
//! is an O(n) raw-string scan. It is also the family where a drift CORRUPTS A REQUEST rather than throwing
//! — a `model` that is not found silently routes to the default channel, and a field appended twice is a
//! body the upstream rejects.
//!
//! THE THREE BOUNDS ARE THE CONTRACT, and each is named where it applies (the module's own B1 note):
//!   * the MODEL scan walks at most `MAX_SCAN_BYTES`;
//!   * the FIELD scan carries the same ceiling **and reports truncation**, so its caller never mistakes
//!     "did not finish looking" for "not present";
//!   * and the two answers differ: an absent field is APPENDED, a truncated one is left ALONE.
//!
//! THE INDEX ARITHMETIC IS THE OTHER CONTRACT, and it is where a port can be quietly wrong: the JavaScript
//! indexes UTF-16 CODE UNITS, and this walks `&str` bytes. For every JSON body whose structure is ASCII —
//! which is what a body IS — the two agree, and the rewritten text is identical because the boundaries
//! land on ASCII delimiters. What is NOT identical is the returned index NUMBERS when non-ASCII text
//! precedes the field, which is why the corpus compares the OBSERVABLE result (the rewritten body, the
//! model value, found/truncated) rather than the raw offsets.

/// `MAX_SCAN_BYTES` — 2 MiB. A char-by-char walk of a multi-MB body alone blows the 10 ms budget.
const MAX_SCAN_BYTES: usize = 2 * 1024 * 1024;

/// What a top-level field scan found — and **`truncated` is not decoration**: "the body was walked to the
/// end and the field is absent" and "the walk hit its ceiling and we do not know" have DIFFERENT safe
/// answers in `raw_with_top_level_field`, and conflating them was B1.
pub enum FieldScan {
    Found {
        value_start: usize,
        value_end: usize,
    },
    Absent {
        truncated: bool,
    },
}

/// Is this a JS `\s` character? The scans use `/\s/`, which is the same set `request_shape.rs` names.
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\u{000c}' | '\n' | '\r' | '\t' | '\u{000b}' | ' ' | '\u{00a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `scanTopLevelField(raw, field)` — the MORE expensive of the two walks (it tracks nested values).
fn scan_top_level_field(raw: &str, field: &str) -> FieldScan {
    let bytes = raw.as_bytes();
    let n = raw.len().min(MAX_SCAN_BYTES);
    let truncated = n < raw.len();
    let mut depth: i64 = 0;
    let mut i = 0usize;
    while i < n {
        if bytes[i] == b'"' {
            let start = i;
            i += 1;
            while i < raw.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                } else {
                    let c = bytes[i];
                    i += 1;
                    if c == b'"' {
                        break;
                    }
                }
            }
            if depth == 1 && &raw[start + 1..i.saturating_sub(1)] == field {
                while i < n && raw[i..].chars().next().map(is_js_space).unwrap_or(false) {
                    i += raw[i..].chars().next().map(char::len_utf8).unwrap_or(1);
                }
                if i >= n || bytes[i] != b':' {
                    continue;
                }
                i += 1;
                while i < n && raw[i..].chars().next().map(is_js_space).unwrap_or(false) {
                    i += raw[i..].chars().next().map(char::len_utf8).unwrap_or(1);
                }
                let value_start = i;
                let mut braces: i64 = 0;
                let mut in_string = false;
                while i < n {
                    let c = bytes[i];
                    if in_string {
                        if c == b'\\' {
                            i += 2;
                        } else {
                            if c == b'"' {
                                in_string = false;
                            }
                            i += 1;
                        }
                        continue;
                    }
                    if c == b'"' {
                        in_string = true;
                        i += 1;
                        continue;
                    }
                    if c == b'{' || c == b'[' {
                        braces += 1;
                    } else if c == b'}' || c == b']' {
                        if braces == 0 {
                            break;
                        }
                        braces -= 1;
                    } else if braces == 0 && (c == b',' || c == b'}') {
                        break;
                    }
                    i += 1;
                }
                let mut value_end = i;
                // `while (valueEnd > valueStart && /\s/.test(raw[valueEnd - 1])) valueEnd--` — a BYTE
                // walk backwards is safe here because every whitespace character in the set is ASCII or
                // starts with a byte that cannot appear inside a multi-byte sequence.
                while value_end > value_start
                    && raw[..value_end]
                        .chars()
                        .next_back()
                        .map(is_js_space)
                        .unwrap_or(false)
                {
                    value_end -= raw[..value_end]
                        .chars()
                        .next_back()
                        .map(char::len_utf8)
                        .unwrap_or(1);
                }
                return FieldScan::Found {
                    value_start,
                    value_end,
                };
            }
            continue;
        }
        if bytes[i] == b'{' {
            depth += 1;
        } else if bytes[i] == b'}' {
            depth -= 1;
        }
        i += 1;
    }
    FieldScan::Absent { truncated }
}

/// `rawWithModel(raw, newModel, scanned?)` — re-scans for the value span and rebuilds only that slice.
///
/// The optional `scanned` is round-55: the handler already scanned for routing, and re-scanning a
/// multi-MB body just to swap the model doubled the CPU.
pub fn raw_with_model(
    raw: &str,
    new_model: &serde_json::Value,
    scanned: Option<(usize, usize)>,
) -> String {
    let (value_start, value_end) = match scanned {
        Some((s, e)) => (s as i64, e as i64),
        None => {
            let scan = scan_top_level_model(raw);
            (scan.value_start, scan.value_end)
        }
    };
    if value_start < 0 || value_end <= value_start {
        return raw.to_string();
    }
    format!(
        "{}{}{}",
        &raw[..value_start as usize],
        serde_json::to_string(new_model).expect("a value"),
        &raw[value_end as usize..]
    )
}

/// `rawWithTopLevelField(raw, field, value)` — force a top-level field without parsing the body.
///
/// **THE TWO FAILURE ANSWERS ARE DIFFERENT, AND THAT IS B1.** An absent field is appended; a TRUNCATED
/// scan leaves the body alone, because past the ceiling we cannot prove the field is absent and appending
/// would put a SECOND `provider`/`reasoning` key on the body — a different defect from the CPU one the
/// ceiling exists to prevent. Unchanged is the fail-safe.
pub fn raw_with_top_level_field(raw: &str, field: &str, value: &serde_json::Value) -> String {
    let scan = scan_top_level_field(raw, field);
    let encoded = serde_json::to_string(value).expect("a value");
    if let FieldScan::Found {
        value_start,
        value_end,
    } = scan
    {
        return format!("{}{}{}", &raw[..value_start], encoded, &raw[value_end..]);
    }
    if let FieldScan::Absent { truncated: true } = scan {
        return raw.to_string();
    }
    let Some(close) = raw.rfind('}') else {
        return raw.to_string();
    };
    // `raw.slice(0, close).trimEnd()` — JS `trimEnd` is the same whitespace set.
    let before = raw[..close].trim_end_matches(is_js_space);
    let separator = if before.ends_with('{') { "" } else { "," };
    format!(
        "{before}{separator}{}:{encoded}{}",
        serde_json::to_string(field).expect("a string"),
        &raw[close..]
    )
}

/// `rawWithDeepSeekProvider(raw)` — one line, one policy: DeepSeek's provider block.
pub fn raw_with_deepseek_provider(raw: &str) -> String {
    raw_with_top_level_field(
        raw,
        "provider",
        &serde_json::json!({ "order": ["deepseek"], "allow_fallbacks": false }),
    )
}

/// `rawWithOxAlphaReasoningDefault(raw)` — respect a client-sent top-level `reasoning`; only when ABSENT
/// default it to `effort: max`.
///
/// **A TRUNCATED SCAN COUNTS AS "POSSIBLY PRESENT"**: defaulting `effort=max` when the client may have
/// sent its own reasoning field is the wrong way to fail, so the truncation arm leaves the body alone.
pub fn raw_with_ox_alpha_reasoning_default(raw: &str) -> String {
    if matches!(
        scan_top_level_field(raw, "reasoning"),
        FieldScan::Found { .. }
    ) {
        return raw.to_string();
    }
    raw_with_top_level_field(raw, "reasoning", &serde_json::json!({ "effort": "max" }))
}

/// What `scanTopLevelModel` answers. The offsets are BYTE offsets (see the module header) and the corpus
/// compares the model and the rewritten body rather than the numbers.
pub struct ModelScan {
    pub model: Option<String>,
    pub value_start: i64,
    pub value_end: i64,
}

/// `scanTopLevelModel(raw)` — one walk, strings and escapes skipped, depth tracked.
///
/// THE COMMA ARM IS LOAD-BEARING: without it, any field BEFORE `model` (system/tools, which Claude Code
/// sends first) leaves the pending-key flag false and `model` is never matched — and the request silently
/// routes to the default channel. The corpus has that case.
pub fn scan_top_level_model(raw: &str) -> ModelScan {
    let bytes = raw.as_bytes();
    let n = raw.len().min(MAX_SCAN_BYTES);
    let mut i = 0usize;
    let mut depth: i64 = 0;
    let mut in_str = false;
    let mut key_start: i64 = -1;
    let mut key_end: i64 = -1;
    let mut pending_key = false;
    while i < n {
        let c = bytes[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
                if key_start >= 0 {
                    key_end = i as i64;
                }
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            if pending_key {
                key_start = i as i64 + 1;
                key_end = -1;
                pending_key = false;
            }
            in_str = true;
            i += 1;
            continue;
        }
        if c == b'{' || c == b'[' {
            depth += 1;
            if depth == 1 {
                pending_key = true;
            }
            i += 1;
            continue;
        }
        if c == b'}' || c == b']' {
            depth -= 1;
            i += 1;
            continue;
        }
        if c == b',' {
            if depth == 1 {
                pending_key = true;
            }
            key_start = -1;
            key_end = -1;
            i += 1;
            continue;
        }
        if c == b':' {
            if depth == 1
                && key_start >= 0
                && key_end > key_start
                && &raw[key_start as usize..key_end as usize] == "model"
            {
                let mut j = i + 1;
                while j < n && matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r') {
                    j += 1;
                }
                if j < n && bytes[j] == b'"' {
                    let vs = j;
                    let mut k = j + 1;
                    let mut val = String::new();
                    while k < n {
                        if bytes[k] == b'\\' {
                            // `val += raw[k] + (raw[k + 1] || "")` — the ESCAPE IS KEPT AS WRITTEN.
                            val.push(bytes[k] as char);
                            if k + 1 < raw.len() {
                                val.push(raw[k + 1..].chars().next().unwrap_or(' '));
                            }
                            k += 2;
                            continue;
                        }
                        if bytes[k] == b'"' {
                            break;
                        }
                        val.push(raw[k..].chars().next().unwrap_or(' '));
                        k += raw[k..].chars().next().map(char::len_utf8).unwrap_or(1);
                    }
                    return ModelScan {
                        model: Some(val),
                        value_start: vs as i64,
                        value_end: k as i64 + 1,
                    };
                }
                return ModelScan {
                    model: None,
                    value_start: -1,
                    value_end: -1,
                };
            }
            key_start = -1;
            key_end = -1;
            i += 1;
            continue;
        }
        i += 1;
    }
    ModelScan {
        model: None,
        value_start: -1,
        value_end: -1,
    }
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `body_scan` cases were produced by the SHIPPING TypeScript
    //! (`gateway/wasm/oracle-translate.mjs`), and this replays them. The corpus compares the OBSERVABLE
    //! result — the rewritten text, the model value, found/truncated — not the raw offsets, for the
    //! encoding reason in this module's header.
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
    fn every_body_scan_case_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            match func {
                "scan_top_level_model" => {
                    let scan = scan_top_level_model(input.as_str().unwrap_or(""));
                    let got = serde_json::json!({
                        "model": scan.model,
                        "found": scan.value_start >= 0,
                    });
                    assert_eq!(&got, want, "{func} / {name}");
                }
                "raw_with_model" => {
                    let got = raw_with_model(
                        input["raw"].as_str().unwrap_or(""),
                        &serde_json::Value::String(
                            input["model"].as_str().unwrap_or("").to_string(),
                        ),
                        None,
                    );
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                "raw_with_top_level_field" => {
                    let got = raw_with_top_level_field(
                        input.as_str().unwrap_or(""),
                        "reasoning",
                        &serde_json::json!({ "effort": "max" }),
                    );
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                "raw_with_deepseek_provider" => {
                    let got = raw_with_deepseek_provider(input.as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                "raw_with_ox_alpha_reasoning_default" => {
                    let got = raw_with_ox_alpha_reasoning_default(input.as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                _ => continue,
            }
            checked += 1;
        }
        assert!(
            checked >= 20,
            "the body-scan corpus shrank to {checked} cases"
        );
    }
}
