//! console-logic — the console page's pure decisions, in Rust, compiled to wasm.
//!
//! The page is React for rendering; the decisions are Rust. This crate is the seam: the functions
//! here are MIRRORS of the TypeScript they replace, including JS's UTF-16 `slice` and `Math.round`
//! semantics, because a mirror that answers differently at a boundary is not a mirror.
//!
//! The first family is formatting: ids, ages and the sentence a reconnected link gets. The rule for
//! adding more is the same — a function that has no DOM, no fetch and no clock of its own.

use wasm_bindgen::prelude::*;

/// The first twelve UTF-16 code units of an id, with an ellipsis only when something was cut.
///
/// JS `id.slice(0, 12)` counts UTF-16 units, not Unicode scalars. A device id is 64 hex characters,
/// so the distinction never matters in practice; it is still implemented exactly, because a mirror
/// that only matches on the easy input is a guess.
#[wasm_bindgen]
pub fn short_id(id: &str) -> String {
    let units: Vec<u16> = id.encode_utf16().collect();
    if units.len() > 12 {
        let mut out = String::from_utf16_lossy(&units[..12]);
        out.push('…');
        out
    } else {
        id.to_string()
    }
}

/// How long ago a device was last seen, in the fleet's one vocabulary.
///
/// `ms` is NaN for "never" (the page maps its null to NaN at the seam), because a wasm-bindgen
/// `Option<f64>` would answer `None` for a legitimate 0 as well.
#[wasm_bindgen]
pub fn age(ms: f64, now_ms: f64) -> String {
    if !ms.is_finite() {
        return "never".to_string();
    }
    let seconds = ((now_ms - ms) / 1000.0).round().max(0.0);
    if seconds < 60.0 {
        return format!("{}s ago", seconds as i64);
    }
    let minutes = (seconds / 60.0).round();
    if minutes < 60.0 {
        return format!("{}m ago", minutes as i64);
    }
    format!("{}h ago", (minutes / 60.0).round() as i64)
}

/// The one sentence both surfaces use for a link that came back.
#[wasm_bindgen]
pub fn retry_sentence(attempts: u32, reason: &str) -> String {
    let noun = if attempts == 1 {
        "failed attempt"
    } else {
        "failed attempts"
    };
    let base = format!("reconnected after {attempts} {noun}");
    if reason.is_empty() {
        base
    } else {
        format!("{base} — {reason}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_id_matches_js_slice_semantics() {
        assert_eq!(short_id("0123456789abcdef"), "0123456789ab…");
        assert_eq!(short_id("0123456789ab"), "0123456789ab");
        assert_eq!(short_id(""), "");
        // A non-BMP scalar is two UTF-16 units. The cut is only represented exactly when it does not
        // split a surrogate pair; a device id is hex, so a split is impossible in the product, and a
        // Rust String cannot hold the lone surrogate JS would produce if it did.
        assert_eq!(short_id("0123456789😀"), "0123456789😀");
        assert_eq!(short_id("0123456789😀xyz"), "0123456789😀…");
    }

    #[test]
    fn age_uses_the_same_rounding_as_math_round() {
        assert_eq!(age(f64::NAN, 1_000_000.0), "never");
        assert_eq!(age(985_000.0, 1_000_000.0), "15s ago");
        assert_eq!(age(910_000.0, 1_000_000.0), "2m ago");
        assert_eq!(age(100_000.0, 10_900_000.0), "3h ago");
        // A clock that moved backwards is not a negative age.
        assert_eq!(age(1_000_000.0, 0.0), "0s ago");
    }

    #[test]
    fn retry_sentence_keeps_the_reason_verbatim() {
        assert_eq!(retry_sentence(1, ""), "reconnected after 1 failed attempt");
        assert_eq!(
            retry_sentence(2, "connection reset by peer"),
            "reconnected after 2 failed attempts — connection reset by peer"
        );
        assert_eq!(
            retry_sentence(5, "timed out waiting for the channel"),
            "reconnected after 5 failed attempts — timed out waiting for the channel"
        );
    }
}
