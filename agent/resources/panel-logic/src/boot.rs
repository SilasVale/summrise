//! `hooks/useBootHistory.ts`'s parse — `/api/boots`'s body → the restart history, transliterated.
//!
//! The second family to move (P2), and the first to reuse the pipeline the archive move built: the
//! same crate, the same loader, the same served artifact, no new infrastructure.
//!
//! THE VOCABULARY IS THE ONE THING HERE THAT IS NOT A TRANSLITERATION, and it is worth stating
//! because it is a real (small) cost of moving a parse into Rust: `KINDS` used to live in the
//! TypeScript, and it is a copy of `BOOT_KINDS` in `lib/contract.gen.ts`, which is GENERATED from
//! `agent/src/vocabulary.rs`. It is still a copy after this move — of the same list, in a third
//! language. The right fix is one Rust home for the vocabulary that both crates depend on (or a
//! generator that emits this file too); it is named in the commit message rather than done here,
//! because it touches the contract generator and the gate that watches it.
//!
//! What the parse itself guarantees, in the panel's own words (from the TypeScript's header):
//!   * a record with no usable stamp is DROPPED, because it cannot be placed on a time axis;
//!   * an unrecognised `kind` renders as "unrecorded" — never another kind's words;
//!   * nothing throws: a body this build cannot use is an EMPTY history, and the failure flag is
//!     the caller's to keep.

use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;

/// The kinds the device writes. `lib/contract.gen.ts`'s `BOOT_KINDS`, from `vocabulary.rs`.
const KINDS: [&str; 5] = [
    "first-run",
    "clean-exit",
    "replaced",
    "machine-restart",
    "crashed",
];

fn type_of(v: &JsValue) -> String {
    v.js_typeof().as_string().unwrap_or_default()
}

/// `str(v)` — a NON-EMPTY string, and NOTE THE ABSENCE OF A TRIM: this module's rule is not
/// `archive.rs`'s `nonEmptyString`. `" "` is a usable `detail` here and an absent one there, and
/// the difference is the TypeScript's, not this port's.
fn non_empty(v: &JsValue) -> Option<String> {
    if type_of(v) != "string" {
        return None;
    }
    let s = v.as_string()?;
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// `num(v)` — a finite JS number.
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

/// A property read that never throws, for the same reason as `archive.rs`'s: `Reflect::get` refuses
/// a primitive target where JS auto-boxes. `(j ?? {})` in the TypeScript is this, spelled out —
/// and it is why `parseBootHistory("nope")` answers an empty history rather than raising.
fn prop(v: &JsValue, key: &str) -> JsValue {
    if v.is_null() || v.is_undefined() || type_of(v) != "object" {
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

/// `parseBootHistory(j)` — never throws, and never invents a value.
#[wasm_bindgen]
pub fn parse_boot_history(j: JsValue) -> Result<JsValue, JsValue> {
    let raw_boots = prop(&j, "boots");
    let list: Vec<JsValue> = if raw_boots.is_array() {
        Array::from(&raw_boots).iter().collect()
    } else {
        Vec::new()
    };
    let boots = Array::new();
    for raw in list {
        let ts_ms = finite_number(&prop(&raw, "ts_ms"));
        // DROPPED, not defaulted: a record that cannot be placed in time is not a record this card
        // can draw, and a list sorted by guesswork is worse than a shorter list.
        let Some(ts_ms) = ts_ms else { continue };
        let raw_kind = non_empty(&prop(&raw, "kind"));
        let kind = raw_kind
            .filter(|k| KINDS.contains(&k.as_str()))
            .map(|k| JsValue::from_str(&k))
            .unwrap_or(JsValue::NULL);
        let rec = Object::new();
        put(&rec, "tsMs", &JsValue::from_f64(ts_ms))?;
        put(&rec, "kind", &kind)?;
        put(
            &rec,
            "detail",
            &JsValue::from_str(&non_empty(&prop(&raw, "detail")).unwrap_or_default()),
        )?;
        put(
            &rec,
            "uptimeSecs",
            &opt_num(finite_number(&prop(&raw, "uptime_secs"))),
        )?;
        put(
            &rec,
            "gapSecs",
            &opt_num(finite_number(&prop(&raw, "gap_secs"))),
        )?;
        put(&rec, "release", &opt_str(non_empty(&prop(&raw, "release"))))?;
        boots.push(&rec);
    }
    let summary = Object::new();
    let summary_in = prop(&j, "summary");
    put(
        &summary,
        "windowSecs",
        &JsValue::from_f64(
            finite_number(&prop(&summary_in, "window_secs")).unwrap_or(86_400.0),
        ),
    )?;
    // `?? boots.length` — the count the panel can actually SEE, rather than zero.
    put(
        &summary,
        "boots",
        &JsValue::from_f64(
            finite_number(&prop(&summary_in, "boots")).unwrap_or(boots.length() as f64),
        ),
    )?;
    put(
        &summary,
        "crashes",
        &JsValue::from_f64(finite_number(&prop(&summary_in, "crashes")).unwrap_or(0.0)),
    )?;
    let out = Object::new();
    put(&out, "boots", &boots)?;
    put(&out, "summary", &summary)?;
    Ok(out.into())
}
