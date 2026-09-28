//! The `js_sys` helpers every family in this crate needs, in ONE place.
//!
//! WHY THIS MODULE EXISTS, and it is this crate's own rule arriving late. `archive.rs`, `boot.rs`,
//! `runs.rs` and `monitors.rs` each define their own `type_of`, `prop`, `put` and `opt_num` — four
//! copies of four functions, which is the defect the JavaScript side extracted
//! `scripts/test/lib/decomment.mjs` to remove ("which made three copies of one rule — the exact
//! defect this objective spends its rounds removing, committed by the gates that enforce it"). The
//! Rust port reproduced it in a new language, one module at a time, and nobody noticed because each
//! copy is defensible on its own.
//!
//! **THE FOUR OLDER COPIES ARE STILL THERE, AND THAT IS RECORDED RATHER THAN QUIETLY FIXED.** They
//! are byte-identical in behaviour but not in signature (`runs.rs`'s `put` takes a `&JsValue` where
//! the others take an `&Object`) and each carries a comment that is about ITS family's boundary
//! conditions — `monitors.rs`'s `num` and `boot.rs`'s `non_empty` differ on purpose and say so.
//! Folding them in is a change to four families at once, so it wants its own commit, its own
//! differential over the four families' tests, and its own size measurement. What this module does
//! is stop the count going to five.
//!
//! THE SEMANTICS ARE `monitors.rs`'s, copied verbatim, because that module has the most complete set
//! and because the differences between the copies are the thing that must not drift.

use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;

/// JS `typeof v`. `JsValue::js_typeof` answers the same string, but it answers `"undefined"` for a
/// missing `JsValue` and the callers here compare against `"object"`/`"number"`/`"string"`, so the
/// empty-string fallback is the behaviour that is wanted when the value is not there at all.
pub fn type_of(v: &JsValue) -> String {
    v.js_typeof().as_string().unwrap_or_default()
}

/// `typeof v === "number" && Number.isFinite(v)` — a finite number, or nothing.
///
/// STRICT ON BOTH HALVES, and each half has a test behind it: `"12"` is not a number to the
/// TypeScript (`typeof`), and `NaN`/`Infinity` ARE numbers to it but fail `Number.isFinite`, so a
/// device that answered `cpu_pct: null` (or `"high"`, or `1e999`) reads as ABSENT rather than as a
/// value a chart would plot.
pub fn num(v: &JsValue) -> Option<f64> {
    if type_of(v) != "number" {
        return None;
    }
    // `f64::is_finite` and NOT a `Number::is_finite` — `js_sys::Number` has no such method (the JS
    // `Number.isFinite` is a STATIC, and `Number.prototype` has nothing of the kind). The first
    // version of this file reached for it and the build said so; the f64 test is what `monitors.rs`
    // has always done and it is the same predicate.
    let n = v.as_f64()?;
    if n.is_finite() {
        Some(n)
    } else {
        None
    }
}

/// A property read that never throws: `Reflect::get` REFUSES a primitive target where JS auto-boxes.
/// `(j ?? {})`, `(r.summary ?? {})` and `(p ?? {})` are all this one guard spelled out — and it is
/// why a parser here answers an empty shape rather than raising when it is handed `42` or `"text"`.
pub fn prop(v: &JsValue, key: &str) -> JsValue {
    if v.is_null() || v.is_undefined() || type_of(v) != "object" {
        return JsValue::UNDEFINED;
    }
    Reflect::get(v, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

/// An array-valued property as a list, or nothing at all when it is not an array. `Array.isArray` is
/// the test, not truthiness: `{targets: "nope"}` is an EMPTY list and not a one-row list.
pub fn rows(v: &JsValue, key: &str) -> Vec<JsValue> {
    let a = prop(v, key);
    if a.is_array() {
        Array::from(&a).iter().collect()
    } else {
        Vec::new()
    }
}

pub fn put(obj: &Object, key: &str, value: &JsValue) -> Result<(), JsValue> {
    Reflect::set(obj.as_ref(), &JsValue::from_str(key), value).map(|_| ())
}

/// A `number | null` field, which is what the TypeScript's `number | null` becomes across the FFI.
/// `null` and not `undefined`: the panel's readers test `=== null` in places, and the two are
/// different values to a `JSON.stringify` a caller might do.
pub fn opt_num(n: Option<f64>) -> JsValue {
    match n {
        Some(x) => JsValue::from_f64(x),
        None => JsValue::NULL,
    }
}
