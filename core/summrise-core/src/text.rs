//! The JavaScript-faithful primitives. A port that "improves" on these is a port that changes behaviour.

/// `Number(x)` for the one place this module needs it: `Number.isInteger(Number(second))`.
///
/// `None` is `NaN`, which is what makes a non-numeric second octet not private. An EMPTY string is `0` and
/// therefore an integer — `"172..1.1"` takes the private branch, exactly as the JavaScript does.
pub fn js_number_of(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0.0);
    }
    t.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_javascript_coercions_hold() {
        assert_eq!(js_number_of("16"), Some(16.0));
        assert_eq!(js_number_of(" 31 "), Some(31.0));
        assert_eq!(js_number_of("abc"), None, "NaN");
        assert_eq!(js_number_of("16abc"), None, "Number() is all-or-nothing");
        assert_eq!(js_number_of(""), Some(0.0), "an empty string is 0");
        // **A RECORDED DIVERGENCE, NOT A CLAIM OF PARITY**: JavaScript's `Number("0x10")` is 16, and Rust's
        // `parse::<f64>()` refuses it. It is unobservable where this is used — a `172.` second octet is never
        // hex — so the port keeps the source's behaviour and this row keeps the difference visible.
        assert_eq!(
            js_number_of("0x10"),
            None,
            "hex is NOT a Rust float literal"
        );
    }
}
