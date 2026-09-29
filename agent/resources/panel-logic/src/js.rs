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
//! **THREE OF THE FOUR COPIES ARE FOLDED IN AS OF 2026-09-29, AND THE REST ARE NOT — both halves
//! measured rather than asserted.** `type_of`, `prop` and `opt_num` were byte-identical in all four
//! modules (`archive.rs`'s `prop` spelled the same test through an `is_object_like` helper, which is
//! the same predicate), so each module now imports them from here and twelve definitions became
//! three. **THE WASM GOT SMALLER FOR IT: 59,283 → 59,143 bytes raw, 24,741 → 24,713 gz** — a
//! deduplication that is also a size win, which is not the usual direction for this crate.
//!
//! WHAT IS STILL DUPLICATED, and each for a reason a mechanical sweep would have destroyed:
//!
//!   * `put` — three modules take an `&Object` and `runs.rs` takes a `&JsValue`. Unifying means
//!     `obj.as_ref()` at ~40 call sites in three files, which is churn rather than deduplication.
//!   * `opt_str` — `Option<String>` in three, `Option<&str>` in `runs.rs`, for the same reason.
//!   * `finite_number` / `finite_num` / `num` — three names, and `monitors.rs`'s carries a comment
//!     about `typeof` versus `Number.isFinite` that its family's tests pin. `evicted.rs` reads the
//!     same field with `typeof … === "number"` and NO finiteness test, on purpose, and says so.
//!   * `non_empty_string` / `non_empty` / `non_empty_str` — these DIFFER IN BEHAVIOUR: `boot.rs`'s
//!     does not trim and `" "` is a usable value there, `archive.rs`'s does. Each module's header
//!     says so, and folding them would silently pick one family's rule for another's field.
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

/// JS `String(n)` — the ENGINE's own number → text, so this crate never links Rust's float formatter.
///
/// WHY IT MATTERS, measured: `format!("{}", 1e21_f64)` writes `1000000000000000000000` where
/// `String(1e21)` writes `1e+21`, and a key spelled two ways is two keys. The brief's own warning is
/// the same class of cost from the other direction — one `format!("{:.3}")` on an `f64` pulled the
/// whole float formatter into the landing page's wasm and cost 8,879 gz.
///
/// IT LIVES HERE NOW (2026-09-29). `monitors.rs` and `runs.rs` each grew their own copy of this
/// function, which is the duplication this module exists to remove — `js.rs`'s header already lists
/// what is still duplicated and why, and this was not on that list.
pub fn number_text(n: f64) -> String {
    js_sys::Number::from(n)
        .to_string_with_radix(10)
        .map(String::from)
        .unwrap_or_default()
}

/// JS `String(v)` for a value whose type is not known — **the ENGINE's coercion, not a cast**.
///
/// THIS IS NOT `JsString::from(v)`, and the difference is a bug this crate shipped for one build.
/// `JsString::from(JsValue)` is an UPCAST: it reinterprets the value as a string without coercing
/// it, so a number, `null` and `undefined` all answer `None` from `as_string()` and every one of
/// them became `""`. The TypeScript's template literal coerces all three — `` `${null}` `` is
/// `"null"` — and the differential for `session_labels.rs` caught it on 21 of its 31 cases.
///
/// The mechanism is the global `String` function called as a function, which IS the abstract
/// operation a template literal's interpolation performs.
pub fn text(v: &JsValue) -> String {
    Reflect::get(&js_sys::global(), &JsValue::from_str("String"))
        .ok()
        .and_then(|ctor| ctor.dyn_into::<js_sys::Function>().ok())
        .and_then(|f| f.call1(&JsValue::UNDEFINED, v).ok())
        .and_then(|s| s.as_string())
        .unwrap_or_default()
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

/// JS `Number(v)` — the ENGINE's ToNumber, for a comparison the JavaScript writes with `>`.
///
/// WHY THIS EXISTS AND NOT `as_f64()`: JavaScript's relational operators COERCE. `"5" > 0` is true,
/// `[5] > 0` is true, `null > 0` is false, `undefined > 0` is false — and `as_f64()` answers `None`
/// for every one of the four, so a port that read the property as a number would be right about two
/// of them and wrong about two, which is the shape of a divergence nobody notices. The differential
/// for `liveness.rs` caught exactly this on `deviceLiveness({pendingCount: "5"})`.
pub fn to_number(v: &JsValue) -> f64 {
    Reflect::get(&js_sys::global(), &JsValue::from_str("Number"))
        .ok()
        .and_then(|ctor| ctor.dyn_into::<js_sys::Function>().ok())
        .and_then(|f| f.call1(&JsValue::UNDEFINED, v).ok())
        .and_then(|n| n.as_f64())
        .unwrap_or(f64::NAN)
}

/// THE INPUT GUARD THE PREDICATES SHARE: `null` and `undefined` are what JavaScript RAISES on.
///
/// `null.reachable` is a TypeError, and so is `undefined.reachable` — while `42 .reachable`,
/// `"x".reachable` and `[].reachable` are all `undefined`, because JavaScript AUTO-BOXES a primitive
/// and only the two nullish values have nothing to box. `prop` in this module tolerates all of them
/// (it is the `(j ?? {})` guard, which is right for a PARSER), and that tolerance is wrong here: a
/// predicate that answered `idle` for a null session would paint a quiet dot about nothing where the
/// TypeScript crashed loudly. A mark that is silently wrong is worse than a component that is
/// visibly broken.
pub fn require_present(v: &JsValue, what: &str) -> Result<(), JsValue> {
    if v.is_null() || v.is_undefined() {
        return Err(JsValue::from_str(&format!(
            "{what}: the input is null or undefined, which the TypeScript raises on"
        )));
    }
    Ok(())
}
