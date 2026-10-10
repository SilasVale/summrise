//! THE THREE `String(...)` SHAPES THE JS SUITE FORMATTED WITH, PORTED SO THE DETAIL STRINGS DO NOT DRIFT.
//!
//! Every `detail` in this suite is a sentence a person reads to find out what the device actually said, and
//! they were written as `String(rs).slice(0, 20)`, `JSON.stringify(bad).slice(0, 60)` and `.trim().slice(0, 60)`.
//! Rust has no `String(anything)` and no `JSON.stringify`, so porting those inline would have produced nine
//! approximations of one rule — the defect `agent/tests/common/mod.rs` was extracted to prevent on the gate
//! side. They live here, once, named after the JavaScript they replace.

use serde_json::Value;

/// `String(v)` — and the cases that are not obvious are the ones that matter: `String(null)` is `"null"` and
/// not an empty string, `String({})` is `"[object Object]"`, and `String(['a','b'])` is `"a,b"`.
pub fn js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `JSON.stringify(v)` in its compact form. Differences from JavaScript that this data cannot reach are not
/// papered over: a float `1.0` prints `1.0` here and `1` there, and the agent's answers carry integers.
pub fn js_json(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".to_string())
}

/// `s.slice(0, n)` — by CHARACTER, because JavaScript's own units here are UTF-16 code units and every
/// string this suite truncates is ASCII with a `?` substituted for anything else.
pub fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `JSON.stringify(v).slice(0, n)` — the detail shape most checks use.
pub fn json_trunc(v: &Value, n: usize) -> String {
    truncate(&js_json(v), n)
}

/// `String(v).slice(0, n)` — the detail shape a check uses when the answer is a bare word (`OK`).
pub fn str_trunc(v: &Value, n: usize) -> String {
    truncate(&js_string(v), n)
}

/// `String(v || '').trim().slice(0, n)` — a command's stdout, cut to what a log line can hold.
pub fn text_trunc(v: &Value, n: usize) -> String {
    truncate(js_string(v).trim(), n)
}

/// **THE LOG IS GBK ON A DEVICE CONSOLE, WHICH IS WHY THE JS SUITE SANITIZED ITS OWN LINES.** A UTF-8 em-dash
/// printed into a `cp936` transcript mojibakes to `鈥?`, and OS error strings arrive in Chinese
/// ("系统找不到指定的文件") only to be re-decoded as GBK by a PowerShell `>` redirect. So every line this judge
/// prints goes through the same printable-ASCII substitution the suite's `check()` used — the difference being
/// that the substitution is now in ONE place rather than in two copies that could drift.
pub fn ascii(s: &str) -> String {
    s.chars()
        .map(|c| {
            if ('\u{20}'..='\u{7e}').contains(&c) {
                c
            } else {
                '?'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn js_string_matches_the_cases_the_details_rely_on() {
        assert_eq!(js_string(&json!("OK")), "OK");
        assert_eq!(js_string(&json!(null)), "null");
        assert_eq!(js_string(&json!({})), "[object Object]");
        assert_eq!(js_string(&json!(["a", "b"])), "a,b");
        assert_eq!(js_string(&json!(120)), "120");
    }

    #[test]
    fn truncation_is_by_character_and_never_splits_one() {
        assert_eq!(truncate("héllo", 2), "hé");
        assert_eq!(str_trunc(&json!("abcdef"), 3), "abc");
    }

    #[test]
    fn a_non_ascii_detail_is_substituted_rather_than_mojibaked() {
        // Five Han characters in, five `?` out — one per CHARACTER, which is what the JS `/[^\x20-\x7E]/g`
        // substitution does and what a byte-wise port would get wrong.
        assert_eq!(ascii("no such file 系统找不到"), "no such file ?????");
    }
}
