//! `lib/gettingStarted.ts`'s `shouldShowGuide` — the one line of LOGIC in a file of vocabulary.
//!
//! The guide opens once per VERSION: the stored value is the version the operator has already seen, so
//! anything that is not the current one opens it again. The version and the storage key stay in
//! TypeScript — they are the surface's own vocabulary, and the key is what `localStorage` is read with,
//! which is the boundary this crate does not cross.

use wasm_bindgen::prelude::*;

/// `stored !== GETTING_STARTED_VERSION` — a STRICT comparison, so `null` (nothing stored, a fresh
/// install) opens the guide, and so does a stored value from any other version.
#[wasm_bindgen]
pub fn should_show_guide(stored: JsValue, version: JsValue) -> bool {
    !strictly_equals(&stored, &version)
}

/// `a === b` for the two shapes this needs: two strings, or anything else that is not the same value.
fn strictly_equals(a: &JsValue, b: &JsValue) -> bool {
    match (a.as_string(), b.as_string()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}
