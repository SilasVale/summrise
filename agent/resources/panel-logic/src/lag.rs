//! `lib/lagMarkers.ts` — WHICH LAG MARKERS ARE STILL WORTH KEEPING.
//!
//! `useSSE` keeps `lagBackfill` — a marker per session meaning "the next frame must be gap-backfilled"
//! — sets it WHOLESALE on reconnect (every registered session gets one) and clears it one entry at a
//! time as that session's frames arrive. A session that died in between therefore kept its marker for
//! the life of the page (found 2026-09-24 by the panel exploration). The sweep already computes the set
//! of sessions that could still frame, which is what the prune needs — and the KEEP SET is the subtle
//! part, so it is a pure function with a test rather than one line inside a transport loop.
//!
//! **THE KEEP SET IS `registered`, NOT `live`.** A closed tombstone that the pane still holds can be
//! REVIVED (round-245), and a revived session must find its marker intact — pruning to live sessions
//! alone would drop markers for sessions the panel is still capable of showing.
//!
//! # WHY THIS RETURNS A LIST RATHER THAN DELETING
//!
//! The TypeScript takes the map and deletes from it, returning how many it dropped. The DECISION is
//! what moves here; the mutation stays with the caller, which is the object that owns the map. The
//! wrapper deletes exactly these keys and returns their count, so the observable contract — the same
//! deletions, the same number — is unchanged.

use js_sys::Array;
use wasm_bindgen::prelude::*;

use crate::js::to_js_string;

/// The keys whose markers should be dropped: every key that is NOT in `reachable`.
///
/// `new Set(reachable)` is the ITERABLE protocol, so an array, a string, a Set and a Map all work and a
/// non-iterable raises — which the corpus carries.
#[wasm_bindgen]
pub fn lag_markers_to_drop(keys: JsValue, reachable: JsValue) -> Result<Array, JsValue> {
    let mut keep: Vec<String> = Vec::new();
    if reachable.is_array() {
        for item in Array::from(&reachable).iter() {
            keep.push(to_js_string(&item).as_string().unwrap_or_default());
        }
    } else if let Some(text) = reachable.as_string() {
        // A STRING IS ITERABLE BY CODE POINT, so `new Set("ab")` is `{"a","b"}`.
        for c in text.chars() {
            keep.push(c.to_string());
        }
    } else if !reachable.is_null() && !reachable.is_undefined() {
        let Some(iter) = js_sys::try_iter(&reachable)
            .map_err(|_| JsValue::from_str("reachable: next() threw"))?
        else {
            return Err(JsValue::from_str(
                "reachable is not iterable: the TypeScript's new Set raises",
            ));
        };
        for item in iter {
            let item = item.map_err(|_| JsValue::from_str("reachable: next() threw"))?;
            keep.push(to_js_string(&item).as_string().unwrap_or_default());
        }
    }
    let Some(keys) = js_sys::try_iter(&keys)
        .map_err(|_| JsValue::from_str("keys: next() threw"))?
    else {
        return Err(JsValue::from_str(
            "keys is not iterable: the TypeScript spreads the map's keys",
        ));
    };
    let out = Array::new();
    for key in keys {
        let key = key.map_err(|_| JsValue::from_str("keys: next() threw"))?;
        let sid = to_js_string(&key).as_string().unwrap_or_default();
        if !keep.contains(&sid) {
            out.push(&key);
        }
    }
    Ok(out)
}
