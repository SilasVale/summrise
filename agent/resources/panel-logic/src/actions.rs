//! `lib/browserAction.ts` — WHAT A RECORD IN THE ACTION FEED CAN ACTUALLY BE.
//!
//! The panel collapsed `exit_code === null` into a badge reading "running" — and NO RECORD IN THIS FEED
//! CAN BE RUNNING. Every writer appends only after the action is over, and a `null` code therefore means
//! "it finished and there was no exit code", which happens in exactly two ways that the device
//! distinguishes with `timed_out`. So a browser action that could not even START was shown as one still
//! in progress — the opposite of what happened — while `stderr_tail`, the sentence saying "spawn
//! failed", was declared on the type, fetched from the route, and rendered NOWHERE.
//!
//! TWO DISTINCTIONS THIS KEEPS:
//!
//!   * **`undefined` IS NOT `null`.** An absent field means the record does not carry one (an older
//!     agent); `null` means the device looked and found none. The old `=== null` test sent an absent
//!     value down the exit-code branch and rendered the literal string "exit undefined".
//!   * **Success is `exit_code === 0`**, strictly. Anything else is not success, and "no code" is its
//!     own answer rather than a shade of failure.

use js_sys::Object;
use wasm_bindgen::prelude::*;

use crate::js::{boxed, call_method, number_text, prop, put, to_number, type_of};

/// `(a.stderr_tail || "").trim() || (a.stdout_tail || "").trim() || null`
///
/// THE TRIM IS A METHOD CALL, so a non-string tail RAISES — which is what the TypeScript does
/// (`(5).trim` is a TypeError) and what the corpus carries.
fn tail_detail(action: &JsValue) -> Result<JsValue, JsValue> {
    for key in ["stderr_tail", "stdout_tail"] {
        let raw = prop(&boxed(action), key);
        let value = if raw.is_truthy() { raw } else { JsValue::from_str("") };
        let trimmed = call_method(&value, "trim", &[])?;
        if trimmed.is_truthy() {
            return Ok(trimmed);
        }
    }
    Ok(JsValue::NULL)
}

/// `actionVerdict(a)` — the badge's word and the device's own sentence, or nothing.
#[wasm_bindgen]
pub fn action_verdict(action: JsValue) -> Result<JsValue, JsValue> {
    if action.is_null() || action.is_undefined() {
        // `a.exit_code` on a nullish `a` is the TypeError the TypeScript raises; `prop` is the guard
        // and this function has no guard.
        return Err(JsValue::from_str(
            "Cannot read properties of null or undefined (reading 'exit_code')",
        ));
    }
    let code = prop(&action, "exit_code");
    let detail = tail_detail(&action)?;
    let (state, label, detail) = if code.as_f64() == Some(0.0) && type_of(&code) == "number" {
        ("ok", "ok".to_string(), JsValue::NULL)
    } else if type_of(&code) == "number" {
        ("fail", format!("exit {}", number_text(to_number(&code))), detail)
    } else if prop(&action, "timed_out").as_bool() == Some(true) {
        ("timeout", "timeout".to_string(), detail)
    } else if code.is_null() {
        ("nocode", "did not start".to_string(), detail)
    } else {
        ("unknown", "no exit code recorded".to_string(), detail)
    };
    let out = Object::new();
    put(&out, "state", &JsValue::from_str(state))?;
    put(&out, "label", &JsValue::from_str(&label))?;
    put(&out, "detail", &detail)?;
    Ok(out.into())
}
