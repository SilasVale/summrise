//! THE JAVASCRIPT BOUNDARY CONDITIONS, SPELLED OUT ONCE.
//!
//! Every function in this module exists for the same reason: **JavaScript's character classes are not
//! Rust's**, and `url-policy.ts` was written against JavaScript's. A port that used Rust's nearest
//! equivalent would answer differently on inputs nobody tests and Everybody ships — this repository's
//! own rule for a ported predicate is that the boundary conditions are transliterated too (see
//! `agent/resources/panel-logic/src/lib.rs`'s "it is a MIRROR, not a reimplementation").
//!
//! THE THREE DIFFERENCES THAT ARE REAL, each measured against the ECMAScript definition rather than
//! remembered:
//!
//! | | JavaScript | Rust |
//! |---|---|---|
//! | `\s` / `trim()` | WhiteSpace ∪ LineTerminator — includes **U+FEFF**, excludes **U+0085** | `char::is_whitespace` — includes **U+0085**, excludes **U+FEFF** |
//! | `\d` | `[0-9]` only | Unicode decimal digits (so `\d` would accept `٣`) |
//! | `.` | anything but a LineTerminator (`\n`, `\r`, U+2028, U+2029) | anything but `\n` |
//!
//! The two patterns in [`crate::parse_agent_port`] are therefore built from the constants below, and
//! [`trim`] / [`trim_end`] / [`split_lines`] implement the JavaScript operations they replace rather
//! than calling the Rust ones. There is ONE spelling of JS whitespace in this crate, so the three
//! cannot start disagreeing about what a whitespace character is.

/// JavaScript's `\s`, as a COMPLETE regex class, so a pattern composes it as `{JS_WS}*` and cannot
/// mistake it for an atom sequence. `\x0B` is `\v`, which the regex crate spells `\x0B` (it has no
/// `\v` escape).
pub const JS_WS: &str = r"[\t\n\x0B\f\r \u{00A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]";

/// The complement, for the `^\S` test (`\S` is not "not-unless-told-otherwise": it is this set's
/// complement, and Rust's `\s` would make it a different complement).
pub const JS_NOT_WS: &str = r"[^\t\n\x0B\f\r \u{00A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]";

/// JavaScript's `.` — a LineTerminator is `\n`, `\r`, U+2028 (LINE SEPARATOR) or U+2029 (PARAGRAPH
/// SEPARATOR). Rust's `.` excludes only `\n`, so `# comment<U+2028>junk` would match here and not in
/// the JavaScript the port replaces.
pub const JS_DOT: &str = r"[^\n\r\u{2028}\u{2029}]";

/// `String.prototype.trimEnd()`: strips JavaScript's WhiteSpace ∪ LineTerminator from the end only.
pub fn trim_end(s: &str) -> &str {
    s.trim_end_matches(is_js_whitespace)
}

/// `String.prototype.trim()`: both ends.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// The one predicate behind both trims, written out because `char::is_whitespace` is a DIFFERENT set
/// (it answers `true` for U+0085, which JavaScript does not trim, and `false` for U+FEFF, which it
/// does).
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

/// `String(yamlText || "").split(/\r?\n/)`: split on `\n`, and drop the `\r` ONLY when it directly
/// precedes the `\n`. `str::lines()` is not this — it also treats a lone `\r` as a terminator, and it
/// drops a trailing empty line, which is a line the JavaScript loop sees.
pub fn split_lines(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three differences in the table above, pinned rather than described, because a comment
    /// claiming a difference is exactly the kind of claim this repository makes executable.
    #[test]
    fn the_js_sets_are_not_the_rust_ones() {
        // U+FEFF: JS trims it, `char::is_whitespace` does not.
        assert!(is_js_whitespace('\u{FEFF}'));
        assert!(!'\u{FEFF}'.is_whitespace());
        // U+0085 (NEL): Rust calls it whitespace, JS does not.
        assert!(!is_js_whitespace('\u{0085}'));
        assert!('\u{0085}'.is_whitespace());
        // `\S` and `\d` are spelled out, so neither can pick up a Unicode property by accident.
        assert_eq!(trim_end("port: 7740\u{FEFF}"), "port: 7740");
        assert_eq!(trim_end("port: 7740\u{0085}"), "port: 7740\u{0085}");
    }

    #[test]
    fn split_lines_drops_a_cr_only_before_a_newline() {
        // `/\r?\n/`: the `\r` goes with the `\n`, a lone `\r` stays in its line, and a trailing
        // newline yields a final empty line (which `str::lines()` would hide).
        assert_eq!(split_lines("a\r\nb"), vec!["a", "b"]);
        assert_eq!(split_lines("a\rb"), vec!["a\rb"]);
        assert_eq!(split_lines("a\n"), vec!["a", ""]);
    }
}
