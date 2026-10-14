//! The JavaScript-faithful primitives. A port that "improves" on these is a port that changes behaviour.
//!
//! **WHY THEY LIVE HERE RATHER THAN IN THE WORKER, AND THE MEASUREMENT THAT PUT THEM HERE.** On 2026-10-13 the
//! worker carried EIGHT near-copies of three rules — `String(x)`, truthiness, and `Number(x)` — because each
//! module wrote the one it needed: `stream::js_text`, six in `device_registry` (`js_falsy`, `js_truthy`,
//! `js_to_string`, `js_to_string_of`, `js_falsy_string`, `js_truthy_string`), `reliability::js_number`,
//! `request_shape::js_truthy`, `decisions::js_string`, `tokens::js_string_of`, `auth_routes::js_to_string_of`.
//! They are the primitives of the WIRE — a request body, an env var, a stored record — so both hosts need them
//! and neither owns them. This is that owner.
//!
//! **AND THE COPIES DISAGREED, WHICH IS WHY CONSOLIDATING THEM WAS WORTH DOING BEFORE THE NEXT PORT.** Two of
//! them rendered a NUMBER differently: `stream::js_text` used `serde_json`'s own `Display` (so a `1.0` that
//! arrived as a float printed `"1.0"` where JavaScript prints `"1"`), while `device_registry::js_to_string` used
//! [`js_number_string`]. This module keeps the faithful one, so the divergence is FIXED rather than recorded —
//! and the corpus plus each host's suite is what says so.
//!
//! **WHAT IS *NOT* HERE**: [`crate::guard`]'s rules, and any coercion whose every caller wants something else.
//! `decisions::js_string` is the example: it maps `null` to `""` rather than to `"null"`, because its call site
//! is a source expression of the form `String(id ?? "")`. That is not this function, and folding it in would
//! change what the emission prints.

use serde_json::Value;

/// JavaScript truthiness for a JSON value: `undefined`, `null`, `false`, `0`, `NaN` and `""` are falsy, and
/// EVERY object — an empty one, an empty array — is truthy.
pub fn js_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        // `NaN != 0.0` is TRUE, so the `is_nan` test is load-bearing: a port written as `f != 0.0` calls NaN
        // truthy, which is the one answer the engine never gives. (`request_shape::js_truthy` had exactly that
        // shape; JSON has no NaN literal, so it was unreachable rather than wrong — and it is still the rule.)
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// JavaScript truthiness, the NEGATIVE form — the source writes `!x` as often as `x`, and a call site forced to
/// spell the negation reads as the opposite of what it means.
pub fn js_falsy(v: &Value) -> bool {
    !js_truthy(v)
}

/// `String(v)` for the JSON values a request body or a stored record can carry.
///
/// The exotic arms are the engine's rather than a simplification: `null` is `"null"`, an object is
/// `"[object Object]"`, and an array JOINS its elements with `,` where a null element contributes the EMPTY
/// string (`String([1,null,2])` is `"1,,2"`).
pub fn js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => js_number_string(n),
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|x| match x {
                Value::Null => String::new(),
                other => js_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `String(v)` where the key may be ABSENT — the difference `String(data.uid)` turns on, and the one
/// [`js_truthy_string`] deliberately does not have: a missing key here is `"undefined"`, which is what the
/// engine's template literal would print.
pub fn js_string_of(v: Option<&Value>) -> String {
    match v {
        Some(value) => js_string(value),
        None => "undefined".to_string(),
    }
}

/// `String(v || fallback)` — the general form of `body?.name || ""` and `String(data.role || "user")`.
///
/// Note which arm is which: a FALSY value takes the fallback (so `""`, `0`, `false` and `null` all become it),
/// and an absent key does too — while a truthy value is `String(v)`, so `true` becomes `"true"` and `{}`
/// becomes `"[object Object]"` (both then fail a name rule rather than sneaking through it).
pub fn js_truthy_string(v: Option<&Value>, fallback: &str) -> String {
    match v {
        Some(value) if js_truthy(value) => js_string(value),
        _ => fallback.to_string(),
    }
}

/// `String(v || "")` — the arm every `body?.field || ""` in the source performs before a `.trim()`.
pub fn js_falsy_string(v: Option<&Value>) -> String {
    js_truthy_string(v, "")
}

/// `String(v ?? "")` — **AND IT IS NOT `js_falsy_string`**, which is the whole reason it exists: `??` replaces
/// only `null` and an ABSENT key, while `||` also replaces every falsy value. So `String(id ?? "")` on a `0` is
/// `"0"` and `String(id || "")` is `""`, and the two land on different refusals — measured in the provider
/// parser, where an id of `0` is a valid model id under `??` and "id is required" under `||`.
pub fn js_or_empty(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(other) => js_string(other),
    }
}

/// `Number(x)` — the engine's COERCION, not a parse.
///
/// `NaN` is the answer for anything that is not a number, and the callers' `Number.isFinite` test is what turns
/// it into their default. An ARRAY coerces through its string form, which is why the length arms are here:
/// `[]` is `""` is `0`, `[5]` is `"5"` is `5`, and two elements are `"1,2"` is `NaN`.
pub fn js_number(v: &Value) -> f64 {
    match v {
        Value::Null => 0.0,
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => js_number_of(s),
        Value::Array(items) => match items.len() {
            0 => 0.0,
            1 => js_number(&items[0]),
            _ => f64::NAN,
        },
        Value::Object(_) => f64::NAN,
    }
}

/// `Number(str)`: trim, then the empty string is `0`, the three prefixed bases are integers, `Infinity` has its
/// three spellings, and anything else must be a COMPLETE decimal literal.
///
/// **`NaN` IS THE ANSWER FOR "NOT A NUMBER"**, rather than an `Option`: every caller asks a question whose
/// answer can be NaN (`Number.isInteger`, `Number.isFinite`, a comparison), and an `Option` would make each of
/// them spell the same unwrap.
///
/// **AND THE PREFIXED BASES ARE WHY THIS IS NOT `str::parse::<f64>()`.** `Number("0x10")` is 16 in JavaScript.
/// The first version of this function — written in `guard.rs`, where a `172.` second octet was the only caller
/// — answered `None` for it and recorded the difference as an ACCEPTED DIVERGENCE. **It was not a divergence,
/// it was a missing arm**, and it made the hostname rule read `172.0x10.0.1` as public. The arms are moved from
/// `reliability.rs`, which had them right.
pub fn js_number_of(s: &str) -> f64 {
    let t =
        s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000c}' | '\u{000b}'));
    if t.is_empty() {
        return 0.0;
    }
    let lower = t.to_ascii_lowercase();
    let prefixed = |prefix: &str, radix: u32| -> Option<f64> {
        lower
            .strip_prefix(prefix)
            .filter(|rest| !rest.is_empty())
            .and_then(|rest| i64::from_str_radix(rest, radix).ok())
            .map(|n| n as f64)
    };
    if let Some(v) = prefixed("0x", 16) {
        return v;
    }
    if let Some(v) = prefixed("0o", 8) {
        return v;
    }
    if let Some(v) = prefixed("0b", 2) {
        return v;
    }
    match lower.as_str() {
        "infinity" | "+infinity" => f64::INFINITY,
        "-infinity" => f64::NEG_INFINITY,
        _ => lower.parse::<f64>().unwrap_or(f64::NAN),
    }
}

/// `Number.isInteger(n)` — `false` for `NaN` and for the infinities, which is what a caller's `!isInteger` arm
/// is refusing.
pub fn js_is_integer(n: f64) -> bool {
    n.is_finite() && n.fract() == 0.0
}

/// `String(number)`, and it exists because `serde_json`'s own `Display` is NOT the engine's.
///
/// Two arms the engine has and this does not, recorded rather than hidden:
///
/// * a float that arrived as `1.0` prints `"1.0"` through `serde_json` and `"1"` here — this is the faithful
///   one, which is why `stream::js_text`'s old copy was the copy that was wrong;
/// * `String(1e21)` is `"1e+21"` in JavaScript and a 22-digit integer through every Rust float formatter.
///   Rust has no shortest-roundtrip decimal formatter in `core`, so this stays the one divergence: it is pinned
///   by a test, and no wire value in this product is near it (a token count, a timestamp, a model id).
fn js_number_string(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    let f = n.as_f64().unwrap_or(f64::NAN);
    if f.fract() == 0.0 && f.abs() < 1e21 {
        return format!("{}", f as i64);
    }
    format!("{f}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// **THE THREE RULES, PINNED WHERE BOTH HOSTS READ THEM.** The rows are the case families the eight copies
    /// disagreed about: a float that arrived as `1.0`, an array containing null, an empty array, an empty
    /// object, `NaN`, and the number no Rust formatter spells the way the engine does.
    #[test]
    fn the_javascript_coercions_hold() {
        // `String(x)`
        assert_eq!(js_string(&json!("abc")), "abc");
        assert_eq!(
            js_string(&json!(null)),
            "null",
            "String(null) is not an empty string"
        );
        assert_eq!(js_string(&json!(true)), "true");
        assert_eq!(
            js_string(&json!(1.0)),
            "1",
            "serde_json's Display would answer \"1.0\""
        );
        assert_eq!(js_string(&json!(-7)), "-7");
        assert_eq!(js_string(&json!([])), "");
        assert_eq!(
            js_string(&json!([1, null, 2])),
            "1,,2",
            "a null element is the empty string"
        );
        assert_eq!(js_string(&json!(["a", ["b", "c"]])), "a,b,c");
        assert_eq!(js_string(&json!({})), "[object Object]");

        // `String(x)` with an ABSENT key versus a present null — the difference the two functions exist for,
        // and the one a single implementation would lose.
        assert_eq!(js_string_of(None), "undefined");
        assert_eq!(js_string_of(Some(&json!(null))), "null");
        assert_eq!(
            js_truthy_string(None, ""),
            "",
            "an absent key takes the fallback"
        );
        assert_eq!(
            js_truthy_string(Some(&json!(0)), "user"),
            "user",
            "falsy takes the fallback"
        );
        assert_eq!(js_truthy_string(Some(&json!("admin")), "user"), "admin");

        // truthiness — every object is truthy.
        for falsy in [json!(null), json!(false), json!(0), json!("")] {
            assert!(js_falsy(&falsy), "{falsy} must be falsy");
        }
        for truthy in [json!([]), json!({}), json!("0"), json!(-1)] {
            assert!(js_truthy(&truthy), "{truthy} must be truthy");
        }

        // `Number(x)`
        assert_eq!(js_number(&json!("16")), 16.0);
        assert_eq!(js_number(&json!(" 31 ")), 31.0);
        assert!(js_number(&json!("abc")).is_nan());
        assert!(
            js_number(&json!("16abc")).is_nan(),
            "Number() is all-or-nothing"
        );
        assert_eq!(js_number(&json!("")), 0.0, "an empty string is 0");
        assert_eq!(js_number(&json!(null)), 0.0);
        assert_eq!(js_number(&json!(true)), 1.0);
        assert_eq!(js_number(&json!([])), 0.0);
        assert_eq!(
            js_number(&json!([5])),
            5.0,
            "a one-element array coerces through its element"
        );
        assert!(
            js_number(&json!([1, 2])).is_nan(),
            "two elements are \"1,2\""
        );
        assert!(js_number(&json!({})).is_nan());

        // **THE ARM THAT WAS MISSING RATHER THAN A DIVERGENCE.** `Number("0x10")` is 16, and the hostname rule
        // that read it as not-a-number was reading `172.0x10.0.1` as public.
        assert_eq!(
            js_number_of("0x10"),
            16.0,
            "hex IS a JavaScript number literal"
        );
        assert_eq!(js_number_of("0b101"), 5.0);
        assert_eq!(js_number_of("0o17"), 15.0);
        assert_eq!(js_number_of("Infinity"), f64::INFINITY);
        assert_eq!(js_number_of("-infinity"), f64::NEG_INFINITY);
        assert!(
            js_number_of("0x").is_nan(),
            "the prefix alone is not a number"
        );
        assert!(
            js_number_of("nan").is_nan(),
            "Number(\"nan\") is NaN in JavaScript too"
        );

        // `Number.isInteger`
        assert!(js_is_integer(16.0));
        assert!(js_is_integer(0.0));
        assert!(!js_is_integer(16.5));
        assert!(!js_is_integer(f64::NAN));
        assert!(!js_is_integer(f64::INFINITY));

        // THE ONE RECORDED DIVERGENCE, PINNED SO IT CANNOT WIDEN SILENTLY.
        assert_eq!(js_string(&json!(1e21)), "1000000000000000000000");
        assert_eq!(js_string(&json!(1e-7)), "0.0000001");
    }
}
