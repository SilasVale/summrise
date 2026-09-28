//! `lib/archive.ts` — the pure half of the session archive, transliterated.
//!
//! The TypeScript this replaces carries the reasoning and it is NOT duplicated here; what is here
//! is the behaviour, plus the two places where Rust and JavaScript do not mean the same thing by
//! the same word. Both are named in `js_trim` and `is_object_like` below, and both are covered by
//! the comparison corpus rather than by argument.

use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;

/// JS `String.prototype.trim()` — which is NOT `str::trim`.
///
/// JS trims WhiteSpace + LineTerminator + **U+FEFF (the BOM)**, and Rust's `str::trim` trims
/// `char::is_whitespace`, which is the Unicode `White_Space` property: it covers U+00A0 (NBSP, and
/// the two agree there) but NOT U+FEFF. So a session id of `"\u{feff}"` is EMPTY to the panel and
/// would have been a name to a naive port — and "a row whose id is not a non-empty string is
/// DROPPED rather than rendered under a fabricated name" is a rule `archive.ts` states.
///
/// The ORIGINAL string is what the caller keeps; the trim only decides whether it counts.
///
/// AND THE TWO SETS DIFFER IN ONE MORE PLACE, which the first version of this helper missed:
/// `White_Space` includes **U+0085 (NEL)** and JS's WhiteSpace does not. So `"\u{85}"` is a NAME to
/// the panel and was an empty string here — the mirror image of the U+FEFF case, one line down.
/// Corrected where the predicate lives rather than worked around at a caller, and it is now shared
/// with `runs.rs` (`value()`), whose corpus covers both characters from the other side.
pub(crate) fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}

/// JS `typeof v`.
fn type_of(v: &JsValue) -> String {
    v.js_typeof().as_string().unwrap_or_default()
}

/// `typeof v === "object"` — and in JS that is TRUE for an array and TRUE for `null`, which is why
/// every caller here tests for `null`/`undefined` separately, exactly as `archive.ts` writes
/// `!v || typeof v !== "object"`.
fn is_object_like(v: &JsValue) -> bool {
    type_of(v) == "object"
}

/// `nonEmptyString(v)` — a string that is non-empty after JS trimming. Anything else is absence.
fn non_empty_string(v: &JsValue) -> Option<String> {
    if type_of(v) != "string" {
        return None;
    }
    let s = v.as_string()?;
    if js_trim(&s).is_empty() {
        None
    } else {
        Some(s)
    }
}

/// `finiteNumber(v)` — a JS number that is finite. `NaN` and `±Infinity` are absence.
fn finite_number(v: &JsValue) -> Option<f64> {
    if type_of(v) != "number" {
        return None;
    }
    let n = v.as_f64()?;
    if n.is_finite() {
        Some(n)
    } else {
        None
    }
}

/// A property read that never throws. `Reflect::get` REFUSES a primitive target ("called on
/// non-object") where JS's `v.k` auto-boxes, so the read is guarded — and every caller in
/// `archive.ts` guards it too, one line earlier, with the same `typeof` test.
fn prop(v: &JsValue, key: &str) -> JsValue {
    if v.is_null() || v.is_undefined() || !is_object_like(v) {
        return JsValue::UNDEFINED;
    }
    Reflect::get(v, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

fn put(obj: &Object, key: &str, value: &JsValue) -> Result<(), JsValue> {
    Reflect::set(obj.as_ref(), &JsValue::from_str(key), value).map(|_| ())
}

fn opt_num(n: Option<f64>) -> JsValue {
    match n {
        Some(x) => JsValue::from_f64(x),
        None => JsValue::NULL,
    }
}

fn opt_str(s: Option<String>) -> JsValue {
    match s {
        Some(x) => JsValue::from_str(&x),
        None => JsValue::NULL,
    }
}

/// `mapLast(state)` — one `state` object from the route, or `null` when it carries nothing usable.
fn map_last(state: &JsValue) -> Result<JsValue, JsValue> {
    if state.is_null() || state.is_undefined() || !is_object_like(state) {
        return Ok(JsValue::NULL);
    }
    let kind = non_empty_string(&prop(state, "kind")).unwrap_or_default();
    let ts = finite_number(&prop(state, "ts"));
    let status = non_empty_string(&prop(state, "status"));
    let exit_code = finite_number(&prop(state, "exit_code"));
    let reason = non_empty_string(&prop(state, "reason"));
    // A state object carrying NOTHING usable is absence, not an empty event.
    if kind.is_empty()
        && ts.is_none()
        && status.is_none()
        && exit_code.is_none()
        && reason.is_none()
    {
        return Ok(JsValue::NULL);
    }
    let last = Object::new();
    put(&last, "kind", &JsValue::from_str(&kind))?;
    put(&last, "ts", &opt_num(ts))?;
    put(&last, "status", &opt_str(status))?;
    put(&last, "exitCode", &opt_num(exit_code))?;
    put(&last, "reason", &opt_str(reason))?;
    Ok(last.into())
}

/// `archiveEntries(payload)` — `GET /api/sessions` → entries, or a THROW.
///
/// The throw is the point and it is why this function returns `Result`: `[]` from a body the panel
/// did not understand would render as "this device has recorded no sessions", which is a claim
/// about the DEVICE drawn from a response the panel failed to read. The two sentences are the
/// TypeScript's own, word for word — `useDeviceRead` carries a fold's message out to the operator
/// as the reason a read is unreadable.
#[wasm_bindgen]
pub fn archive_entries(payload: JsValue) -> Result<JsValue, JsValue> {
    if payload.is_null() || payload.is_undefined() || !is_object_like(&payload) {
        return Err(JsValue::from_str(
            "session archive: response is not an object",
        ));
    }
    let list = prop(&payload, "sessions");
    if !list.is_array() {
        return Err(JsValue::from_str(
            "session archive: response carries no sessions array",
        ));
    }
    let out = Array::new();
    for row in Array::from(&list).iter() {
        if row.is_null() || row.is_undefined() || !is_object_like(&row) {
            continue;
        }
        // unnameable — the panel cannot open what it cannot name
        let Some(sid) = non_empty_string(&prop(&row, "id")) else {
            continue;
        };
        // Both keys must be present for the identity to be usable: half an identity rendered as if
        // whole is how a placeholder becomes a fact.
        let kind = non_empty_string(&prop(&row, "kind"));
        let label = non_empty_string(&prop(&row, "label"));
        let entry = Object::new();
        put(&entry, "sid", &JsValue::from_str(&sid))?;
        put(&entry, "last", &map_last(&prop(&row, "state"))?)?;
        let identity = match (kind, label) {
            (Some(k), Some(l)) => {
                let i = Object::new();
                put(&i, "kind", &JsValue::from_str(&k))?;
                put(&i, "label", &JsValue::from_str(&l))?;
                i.into()
            }
            _ => JsValue::NULL,
        };
        put(&entry, "identity", &identity)?;
        out.push(&entry);
    }
    Ok(out.into())
}
