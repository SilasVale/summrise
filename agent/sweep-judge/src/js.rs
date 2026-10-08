//! JavaScript semantics, reproduced where a message depends on them.
//!
//! The judge was 580 lines of JavaScript and every one of its messages is a `template literal` over a
//! loosely-typed report. Rust has no `undefined`, no truthiness coercion and no `String(x)`, so the port
//! needs one place that answers "what would JavaScript have printed here" — this module. It is small on
//! purpose: `js_str`, `truthy`, `num`, `to_fixed` and a UTF-16 `slice`, which is all the judge uses.

use serde_json::Value;

/// `String(x)` for the values a JSON report can hold — including the two JavaScript has and JSON does not.
///
/// `None` is JavaScript's `undefined` (an absent property) and prints as `undefined`; `Value::Null` prints
/// as `null`. Getting this wrong is not a formatting detail: `${s.h1Count}` in the h1 clause prints
/// `undefined` for a report that omits the field, and a port that printed `0` or `null` there would be
/// saying something different about the page.
pub fn js_str(v: Option<&Value>) -> String {
    match v {
        None => "undefined".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => number_str(n),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| match x {
                // Array.prototype.join renders null and undefined as the empty string.
                Value::Null => String::new(),
                other => js_str(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".to_string(),
    }
}

/// `String(number)`. Ordinary doubles print the same way in Rust and JavaScript (both emit the shortest
/// round-tripping decimal); the one divergence left is `>= 1e21`, where JavaScript switches to exponent
/// notation and Rust does not — no ratio, count or percentage in a report reaches it.
fn number_str(n: &serde_json::Number) -> String {
    if let Some(f) = n.as_f64() {
        if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e21 {
            return format!("{}", f as i64);
        }
    }
    n.to_string()
}

/// JavaScript truthiness for `if (x)`.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0 && !f.is_nan()).unwrap_or(false),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// `Number(x)`: `None` is NaN, which every comparison against it makes false — exactly what the JS does
/// when `navs` or `pressed` is missing.
pub fn num(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                Some(0.0)
            } else {
                t.parse::<f64>().ok().filter(|f| !f.is_nan())
            }
        }
        Some(Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        Some(Value::Null) => Some(0.0),
        _ => None,
    }
}

/// `typeof x === "number"`.
pub fn is_num(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Number(_)))
}

/// `x === n` against a number literal — the STRICT comparison, not the coercing one.
///
/// `s.h1Count !== 1` is true for the string `"1"` and false for the number `1`, where `s.navs > 1` in the
/// clause beside it coerces. Both spellings appear within four lines of each other in the JS, so the port
/// keeps them apart rather than routing everything through `num`.
pub fn num_is(v: Option<&Value>, n: f64) -> bool {
    matches!(v, Some(Value::Number(x)) if x.as_f64() == Some(n))
}

/// `x === null || x === undefined`.
pub fn is_nullish(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null))
}

/// `v[key]`, and `None` for anything that is not an object.
pub fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    if v.is_object() {
        v.get(key)
    } else {
        None
    }
}

/// `v?.[key]` — a chained property read, which the JS writes as `s.measure.worst` and then guards with
/// `|| []`. `None` in, `None` out, so a whole chain needs one `unwrap_or` at the end rather than five.
pub fn at<'a>(v: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    v.and_then(|x| get(x, key))
}

const EMPTY: &[Value] = &[];

/// The array behind a property, or an empty slice. Used everywhere the JS writes `(x.foo || [])`.
pub fn arr(v: Option<&Value>) -> &[Value] {
    match v {
        Some(Value::Array(a)) => a,
        _ => EMPTY,
    }
}

/// `x.foo.length`, over the shapes a report actually carries. `None` (JavaScript `undefined`) would throw
/// on `.length`; treating it as zero is the one place this port is deliberately more forgiving than the
/// original, because a crash is not a verdict.
pub fn len(v: Option<&Value>) -> usize {
    match v {
        Some(Value::Array(a)) => a.len(),
        Some(Value::String(s)) => s.encode_utf16().count(),
        _ => 0,
    }
}

/// `list.join(sep)`, with every element through `String()`.
pub fn join(v: Option<&Value>, sep: &str) -> String {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| match x {
                Value::Null => String::new(),
                other => js_str(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(sep),
        Some(other) => js_str(Some(other)),
        None => String::new(),
    }
}

/// `String(x).slice(a, b)` in UTF-16 code units — the unit the JS counts, so a 24-character truncation of
/// a string containing an astral character cuts where the browser's `slice` would.
pub fn slice_utf16(s: &str, start: usize, end: usize) -> String {
    if end <= start {
        return String::new();
    }
    let units: Vec<u16> = s.encode_utf16().collect();
    if start >= units.len() {
        return String::new();
    }
    let stop = end.min(units.len());
    String::from_utf16_lossy(&units[start..stop])
}

/// `s.slice(0, n)` — the truncation every finding uses to quote a control's text.
pub fn slice0(s: &str, n: usize) -> String {
    slice_utf16(s, 0, n)
}

/// `x.toFixed(digits)`.
///
/// Rust's `{:.N}` rounds half to EVEN on the exact binary value, and JavaScript's `toFixed` rounds half
/// UP — `(0.25).toFixed(1)` is `"0.3"` there and `"0.2"` here if you let the formatter decide. The blind
/// row percentage is computed as `blind / all * 100` and printed at one decimal, so a report with one
/// blind row in four hundred would print a different sentence under the two rules. This does the decimal
/// rounding by hand, on twenty extra digits of the exact value, which is what the specification asks for.
pub fn to_fixed(x: f64, digits: usize) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let negative = x < 0.0;
    let exact = format!("{:.*}", digits + 20, x.abs());
    let (int_part, frac_part) = match exact.split_once('.') {
        Some((i, f)) => (i.to_string(), f.to_string()),
        None => (exact.clone(), String::new()),
    };
    let keep: String = frac_part.chars().take(digits).collect();
    let rest: Vec<u8> = frac_part.bytes().skip(digits).collect();
    // 0.rest >= 0.5 exactly when the first dropped digit is 5 or more.
    let round_up = rest.first().map(|c| *c >= b'5').unwrap_or(false);
    let mut all: Vec<u8> = format!("{int_part}{keep}").into_bytes();
    if round_up {
        let mut i = all.len();
        loop {
            if i == 0 {
                all.insert(0, b'1');
                break;
            }
            i -= 1;
            if all[i] == b'9' {
                all[i] = b'0';
            } else {
                all[i] += 1;
                break;
            }
        }
    }
    let all = String::from_utf8(all).unwrap_or_default();
    let mut out = if digits == 0 {
        all
    } else if all.len() > digits {
        let split = all.len() - digits;
        format!("{}.{}", &all[..split], &all[split..])
    } else {
        format!("0.{}{}", "0".repeat(digits - all.len()), all)
    };
    if negative {
        out.insert(0, '-');
    }
    out
}

/// `String.prototype.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")` — the escape the mark-coverage note builds its
/// per-family regex with.
pub fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '.' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn js_str_renders_undefined_and_null_apart() {
        assert_eq!(js_str(None), "undefined");
        assert_eq!(js_str(Some(&Value::Null)), "null");
        assert_eq!(js_str(Some(&json!("x"))), "x");
        assert_eq!(js_str(Some(&json!(true))), "true");
        assert_eq!(js_str(Some(&json!(24))), "24");
        assert_eq!(js_str(Some(&json!(1.5))), "1.5");
    }

    #[test]
    fn js_str_joins_arrays_like_template_interpolation() {
        assert_eq!(js_str(Some(&json!(["a", "b"]))), "a,b");
        assert_eq!(js_str(Some(&json!([1, null, "c"]))), "1,,c");
    }

    #[test]
    fn truthiness_matches_javascript() {
        assert!(!truthy(None));
        assert!(!truthy(Some(&Value::Null)));
        assert!(!truthy(Some(&json!(0))));
        assert!(!truthy(Some(&json!(""))));
        assert!(!truthy(Some(&json!(false))));
        assert!(truthy(Some(&json!(1))));
        assert!(truthy(Some(&json!("0"))));
        assert!(truthy(Some(&json!([]))));
    }

    #[test]
    fn number_coercion_matches_javascript() {
        assert_eq!(num(Some(&json!(2))), Some(2.0));
        assert_eq!(num(Some(&json!("700"))), Some(700.0));
        assert_eq!(num(Some(&json!(""))), Some(0.0));
        assert_eq!(num(Some(&json!(true))), Some(1.0));
        assert_eq!(num(Some(&Value::Null)), Some(0.0));
        assert_eq!(num(None), None);
        // NaN makes every comparison false, which is what `s.navs > 1` needs for an absent field.
        assert!(!num(None).map(|n| n > 1.0).unwrap_or(false));
    }

    #[test]
    fn to_fixed_rounds_half_up_like_javascript() {
        assert_eq!(to_fixed(0.25, 1), "0.3");
        // 0.35 is 0.34999... in binary, so JavaScript prints "0.3" here and the intuition is wrong.
        assert_eq!(to_fixed(0.35, 1), "0.3");
        assert_eq!(to_fixed(0.10000000000000009, 2), "0.10");
        assert_eq!(to_fixed(-0.019999999999999907, 2), "-0.02");
        assert_eq!(to_fixed(100.0, 1), "100.0");
        assert_eq!(to_fixed(10.000000000000002, 0), "10");
        assert_eq!(to_fixed(0.0, 1), "0.0");
        assert_eq!(to_fixed(12.5, 0), "13");
        // 9.95 is 9.9499999... in binary, so JavaScript prints "9.9" and so must this.
        assert_eq!(to_fixed(9.95, 1), "9.9");
        assert_eq!(to_fixed(0.05, 1), "0.1");
    }

    #[test]
    fn slice_is_counted_in_utf16_units() {
        assert_eq!(slice0("hello", 24), "hello");
        assert_eq!(slice0("hello", 3), "hel");
        // An astral character is TWO units to JavaScript, so a 3-unit slice keeps the emoji whole.
        assert_eq!(slice0("a\u{1F600}b", 3), "a\u{1F600}");
    }

    #[test]
    fn regex_escape_matches_the_javascript_class() {
        assert_eq!(regex_escape("span.approval-grant"), "span\\.approval-grant");
        assert_eq!(regex_escape("a+b(c)[d]"), "a\\+b\\(c\\)\\[d\\]");
        assert_eq!(regex_escape("plain-name"), "plain-name");
    }
}
