//! THE JAVASCRIPT BOUNDARY CONDITIONS `main.ts` RELIED ON, SPELLED OUT ONCE.
//!
//! Every function here exists because **JavaScript's operations are not Rust's**, and the code this crate
//! replaces was JavaScript. The rule the url-policy crate states is the rule here: the boundary conditions
//! are transliterated too, because a port that used Rust's nearest equivalent would answer differently on
//! inputs nobody tests — and this crate's inputs include a `process.env` value, a JSON body from the
//! agent, a renderer's `getBoundingClientRect()` and a `Date`.
//!
//! There is deliberately NO second spelling of JavaScript whitespace WITHIN either crate —
//! `agent/summrise-url-policy/src/js.rs` owns the set for the URL policy and this file owns it for the
//! shell policy, and **the two constants must stay identical**. They are duplicated rather than shared
//! because sharing them would mean a crate dependency, and a crate dependency is what
//! `Cargo.toml` records this crate cannot have: wasm-bindgen exports the whole crate GRAPH, so a
//! dependency would put a SECOND copy of the url-policy `thread_local!`s into the shell's module and
//! `setAgentPort` would stop meaning anything. `agent/tests/shared_literals.rs` compares the two sources
//! and refuses a disagreement, which is what makes the duplication safe to have.
//!
//! THE FOUR OPERATIONS HERE, and what the naive Rust would have answered instead:
//!
//! | operation | JavaScript | the Rust that is NOT the same |
//! |---|---|---|
//! | `String(x)` for a number | shortest round-trip; `1e21` → `"1e+21"`, `1e-7` → `"1e-7"` | `format!("{}")` never uses an exponent |
//! | `String.prototype.slice(0, n)` | counts **UTF-16 code units**, can cut a surrogate pair in half | `&s[..n]` counts BYTES and panics off a char boundary |
//! | `Math.round(x)` | half towards **+∞** (`-2.5` → `-2`), and `-0.5` → `-0` | `f64::round` is half AWAY FROM ZERO (`-2.5` → `-3`) |
//! | `String.prototype.includes` | substring search over code units | `str::contains` — the same answer on valid UTF-8, and pinned rather than assumed |

/// JavaScript's `\s` as a COMPLETE regex class — WhiteSpace ∪ LineTerminator, which **includes U+FEFF**
/// and **excludes U+0085**. It must be byte-identical to the constant of the same name in
/// `agent/summrise-url-policy/src/js.rs`; `agent/tests/shared_literals.rs` enforces that.
///
/// `\x0B` is `\v`, which the regex crate spells `\x0B` (it has no `\v` escape).
pub const JS_WS: &str = r"[\t\n\x0B\f\r \u{00A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]";

/// The one predicate behind both trims, written out because `char::is_whitespace` is a DIFFERENT set (it
/// answers `true` for U+0085, which JavaScript does not trim, and `false` for U+FEFF, which it does).
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `String(number)` — the ECMAScript `Number::toString`, for the range this crate can meet.
///
/// THE ONE DIVERGENCE IS NAMED RATHER THAN HIDDEN: outside `|n| < 1e21` JavaScript switches to
/// exponential notation (`String(1e21)` is `"1e+21"`) and Rust does not. Every number this function is
/// asked about is an uptime in seconds, a session count, or a rounded percentage — all of them far
/// inside the range, and the divergence is unreachable from any of them. A caller that one day has an
/// unbounded number must not assume this is `String()`.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    // `String(-0)` is `"0"`, and `format!("{:.0}", -0.0)` is `"-0"` — so the sign is dropped first.
    if n == 0.0 {
        return "0".to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        return format!("{n:.0}");
    }
    format!("{n}")
}

/// `Math.round`.
///
/// The spec is `floor(x + 0.5)` with four special cases, and the two that Rust's `f64::round` gets wrong
/// are both on the negative side: `Math.round(-2.5)` is `-2` where `f64::round` answers `-3`, and
/// `Math.round(-0.5)` is `-0` where `f64::round` answers `-1`. The third case is the one a naive
/// `(x + 0.5).floor()` gets wrong in the OTHER direction: `Math.round(0.49999999999999994)` is `0`,
/// because the addition rounds that double up to exactly `1.0`.
pub fn round(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    if (-0.5..0.0).contains(&x) {
        // JavaScript answers `-0`; the sign is unobservable through `number_to_string`, and `0.0` is
        // what this crate can return without a signed-zero literal that clippy reads as a mistake.
        return 0.0;
    }
    let shifted = (x + 0.5).floor();
    // ...and the case above, in the positive direction: a value below the half whose sum with 0.5
    // rounds up to 1.0 must still answer 0.
    if shifted == 1.0 && x < 0.5 {
        return 0.0;
    }
    shifted
}

/// `String.prototype.slice(0, units)` — the first `units` UTF-16 CODE UNITS.
///
/// IT IS NOT A BYTE SLICE AND IT IS NOT `chars().take(n)`: the shell truncates a URL for a log line
/// (`url.slice(0, 80)`) and a `User-Agent` header for a warning (`ua.slice(0, 60)`), and both are counted
/// the way JavaScript counts them — so a URL carrying non-ASCII is truncated at the same place on both
/// sides of this port.
///
/// THE ONE ANSWER THAT CANNOT BE TRANSLITERATED: if the cut lands INSIDE a surrogate pair, JavaScript
/// returns a string ending in a LONE SURROGATE, and a Rust `String` cannot hold one. This returns the
/// prefix WITHOUT the straddling character — one code unit shorter — and that is the whole of the
/// difference. It is pinned by a test rather than left to be discovered.
pub fn truncate_utf16(s: &str, units: usize) -> &str {
    let mut used = 0usize;
    for (byte_at, ch) in s.char_indices() {
        let width = ch.len_utf16();
        if used + width > units {
            return &s[..byte_at];
        }
        used += width;
    }
    s
}

/// `String.prototype.includes` — substring search.
///
/// A named function rather than an inline `str::contains` because the shell asks this question about a
/// `User-Agent` header to decide whether CDP port 9333 belongs to THIS app, and the answer is a
/// contract (`agent/tests/mcp_autoselect_integration.rs` binds 9333; playwright's `connectOverCDP`
/// drives the operator's visible page). One name makes it greppable from both sides.
pub fn includes(haystack: &str, needle: &str) -> bool {
    haystack.contains(needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four rows of the table at the top of this file, pinned rather than described.
    #[test]
    fn the_js_operations_are_not_the_rust_ones() {
        // `String(number)`.
        assert_eq!(number_to_string(0.0), "0");
        assert_eq!(number_to_string(-0.0), "0");
        assert_eq!(number_to_string(90.0), "90");
        assert_eq!(number_to_string(90.5), "90.5");
        assert_eq!(number_to_string(1759123456789.0), "1759123456789");
        assert_eq!(number_to_string(f64::NAN), "NaN");

        // `Math.round` — the negative half is where `f64::round` answers differently.
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(-0.5), 0.0);
        assert_eq!(round(0.49999999999999994), 0.0);
        assert_eq!(round(0.5), 1.0);
        assert!(round(f64::NAN).is_nan());

        // `slice` counts UTF-16 units, not bytes and not chars.
        assert_eq!(truncate_utf16("abcdef", 3), "abc");
        assert_eq!(truncate_utf16("abc", 10), "abc");
        // U+00E9 is ONE unit and TWO bytes: a byte slice of 2 would answer "é", and this answers "é".
        assert_eq!(truncate_utf16("é", 1), "é");
        // U+1F600 is TWO units and four bytes.
        assert_eq!(truncate_utf16("😀x", 2), "😀");
        // THE NAMED DIVERGENCE: a cut inside the pair drops the character rather than emitting half of
        // it, because a Rust string cannot hold a lone surrogate. `"😀x".slice(0,1)` in JavaScript is a
        // one-unit string holding an unpaired high surrogate; this is the empty string.
        assert_eq!(truncate_utf16("😀x", 1), "");

        // `includes`.
        assert!(includes(
            "Mozilla/5.0 summrise-desktop-electron/1",
            "summrise-desktop-electron"
        ));
        assert!(!includes(
            "Mozilla/5.0 Chrome/141",
            "summrise-desktop-electron"
        ));
    }
}
