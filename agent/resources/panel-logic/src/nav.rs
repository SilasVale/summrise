//! `lib/embeddedNav.ts` — the address-bar merge rule for embedded-browser nav pushes.
//!
//! FOCUS-TRAP FIX: the pane drops nav pushes while the address bar is focused (so typing is never
//! clobbered). But the focus flag is cleared only by DOM blur — clicking into the NATIVE view (a
//! separate OS window over the SPA) never fires blur, so the flag sticks forever and every later push
//! is dropped: the bar freezes until the pane remounts.
//!
//! THE RULE: follow the push unless the user has UNSENT edits — focused AND the value differs both from
//! what it was at focus time and from the last pushed URL.

use wasm_bindgen::prelude::*;

use crate::js::{boxed, prop, to_js_string};

/// `shouldAcceptNavPush(m)`.
///
/// Each `===` is STRICT and on the RAW value, so a numeric `inputValue` equal to a numeric
/// `valueAtFocus` answers true — and the comparisons are in the source's order, because each one is a
/// separate reason to follow the push.
#[wasm_bindgen]
pub fn should_accept_nav_push(m: JsValue) -> bool {
    let field = |key: &str| prop(&boxed(&m), key);
    if !field("editing").is_truthy() {
        return true;
    }
    if same(&field("inputValue"), &field("valueAtFocus")) {
        return true;
    }
    if same(&field("inputValue"), &field("lastPushedUrl")) {
        return true;
    }
    false
}

/// `a === b`, read as text where both are strings and by identity otherwise — the two shapes this rule
/// compares (`inputValue` is a string in every caller, and a test may pass anything).
fn same(a: &JsValue, b: &JsValue) -> bool {
    let (x, y) = (to_js_string(a), to_js_string(b));
    match (x.as_string(), y.as_string()) {
        (Some(p), Some(q)) => p == q,
        _ => a == b,
    }
}
