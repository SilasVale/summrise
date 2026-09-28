//! `lib/evicted.ts`'s parse — one `session-evicted` frame → one notice, or `null`, transliterated.
//!
//! The sixth family to move (P2), and the second whose call site is an EVENT HANDLER: `parseEvicted`
//! runs inside a `summrise-session-evicted` listener (`useEvictedNotice`), which is free for the
//! reason `parseMonitorChange`'s move established — nothing in an event handler is on the first
//! render's path, so a seam that must be awaited costs nothing there.
//!
//! WHAT STAYS IN TYPESCRIPT: `evictedText` (called while `EvictedNotice` RENDERS), the
//! `EvictionNotice` interface, and the `humanIdle` re-export. The hook itself computes nothing.
//!
//! ── THE ONE BOUNDARY THAT IS NOT `num()`, AND IT IS THE REASON THIS FILE IS NOT A COPY-PASTE ────
//!
//! `idleMs` is read as `typeof r.idle_ms === "number" ? r.idle_ms : 0` — a TYPE test, NOT
//! `Number.isFinite`. So a frame carrying `idle_ms: NaN` (or `Infinity`, or `-Infinity`) keeps that
//! value here where this crate's own [`crate::js::num`] would answer `None` and write `0`. The
//! difference is deliberate and it is the TypeScript's: `num` exists for the parsers that plot a
//! value on an axis, where a non-finite number is not a reading, and this one formats a duration
//! into a sentence. **A port that reached for the crate's helper because it was there would have
//! changed the behaviour of a frame nobody sends** — which is exactly the kind of divergence that
//! survives every test and appears once, in production, on the day somebody sends it.
//!
//! The same reading applies to `limit`, and to the four string fields, which fall back to `""` on
//! anything that is not a string — including `null`, a number and an object.

use crate::js::{prop, put, rows, type_of};
use js_sys::{Array, Object};
use wasm_bindgen::prelude::*;

/// `typeof v === "number" ? v : 0` — the TYPE test, spelled out rather than reached for.
fn type_number_or_zero(v: &JsValue) -> f64 {
    if type_of(v) == "number" {
        v.as_f64().unwrap_or(0.0)
    } else {
        0.0
    }
}

/// `typeof v === "string" ? v : ""`.
fn type_string_or_empty(v: &JsValue) -> String {
    if type_of(v) == "string" {
        v.as_string().unwrap_or_default()
    } else {
        String::new()
    }
}

/// `parseEvicted(detail)` — the notice, or `null` for anything this build cannot describe.
///
/// `null` and not a throw: this runs inside an event handler, and a frame this build cannot use must
/// be a frame it says nothing about rather than a listener that raises. The three refusals are the
/// TypeScript's own and each is a strict test:
///
///   * `d.ev !== "session-evicted"` — the frame's own name, so a listener registered for one event
///     cannot turn a different one into a notice about something that did not happen;
///   * a `cause` that is neither `"idle"` nor `"cap"` — the two rules the device enforces, and a
///     third one invented by a newer device is not a line this build knows how to write;
///   * NO SESSIONS LEFT after the rows are filtered — a notice that says nothing was taken is worse
///     than no notice, and it is what an empty `sessions` array would produce.
#[wasm_bindgen]
pub fn parse_evicted(detail: JsValue) -> Result<JsValue, JsValue> {
    // `d.ev !== "session-evicted"` — a MISSING `ev` is not this frame, and neither is a non-string.
    if prop(&detail, "ev").as_string().as_deref() != Some("session-evicted") {
        return Ok(JsValue::NULL);
    }

    let cause = prop(&detail, "cause");
    let cause_str = cause.as_string().unwrap_or_default();
    if cause_str != "idle" && cause_str != "cap" {
        return Ok(JsValue::NULL);
    }

    let sessions = Array::new();
    for raw in rows(&detail, "sessions") {
        // `typeof r.id !== "string" || !r.id` — the SECOND half is truthiness, so an EMPTY id is
        // skipped as well as a missing one. A session with no name cannot be named in the line.
        let id = type_string_or_empty(&prop(&raw, "id"));
        if id.is_empty() {
            continue;
        }
        let session = Object::new();
        put(&session, "id", &JsValue::from_str(&id))?;
        put(
            &session,
            "label",
            &JsValue::from_str(&type_string_or_empty(&prop(&raw, "label"))),
        )?;
        put(
            &session,
            "kind",
            &JsValue::from_str(&type_string_or_empty(&prop(&raw, "kind"))),
        )?;
        put(
            &session,
            "idleMs",
            &JsValue::from_f64(type_number_or_zero(&prop(&raw, "idle_ms"))),
        )?;
        put(
            &session,
            "reason",
            &JsValue::from_str(&type_string_or_empty(&prop(&raw, "reason"))),
        )?;
        sessions.push(&session);
    }
    if sessions.length() == 0 {
        return Ok(JsValue::NULL);
    }

    let out = Object::new();
    put(&out, "cause", &JsValue::from_str(&cause_str))?;
    put(
        &out,
        "limit",
        &JsValue::from_f64(type_number_or_zero(&prop(&detail, "limit"))),
    )?;
    put(&out, "sessions", &sessions)?;
    Ok(out.into())
}
