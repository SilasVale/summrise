//! `lib/terminalAdopt.ts` — the adopt-paging decision and the per-frame write slicing.
//!
//! WHY THE PAGING RULE EXISTS. A single `terminal_read` is capped at 1 MiB server-side (`read_spill`),
//! so a chatty AI session's head used to be silently dropped when a human opened its tab late. The
//! response carries absolute cursors — `start`, `end`, and `evicted` when the server reset the buffer —
//! and this module decides whether ANOTHER read is needed to catch up. Pure and timer-free: it inspects
//! one response and nothing else.
//!
//! WHY THE SLICING EXISTS (terminal backpressure). One `term.write` of a full 1 MiB page — or a burst
//! of SSE frames — blocks the main thread while xterm parses and lays out the text. `TerminalPane`
//! queues these slices and writes at most one budget per animation frame, so a chatty session cannot
//! freeze the UI in a single call.
//!
//! # THE TWO NUMBERS STAY IN TYPESCRIPT
//!
//! `MAX_ADOPT_PAGES` and `WRITE_SLICE_CHARS` are ARGUMENTS: the first is the wedged-server bound and the
//! second is the per-frame budget, both are exported from `lib/terminalAdopt.ts`, both are read by
//! `TerminalPane`, and the module's test imports them. The arrangement `liveness.rs` records for
//! `WORKING_MS`.
//!
//! # AND THE SLICES ARE BUILT BY THE ENGINE
//!
//! `text.slice(i, i + n)` counts UTF-16 UNITS, so a slice can end between the two halves of a surrogate
//! pair and the slice IS then a lone surrogate — which a Rust `String` cannot hold. Every slice is cut
//! by the engine, which is the same reason `maskToken` and the recipe body are built there.

use js_sys::Array;
use wasm_bindgen::prelude::*;

use crate::js::{call_method, js_max, prop, to_number};

/// True when the response proves more history exists past what we rendered.
#[wasm_bindgen]
pub fn adopt_needs_another_page(resp: JsValue, rendered: JsValue, advanced: JsValue) -> bool {
    // `if (!resp) return false` — TRUTHINESS, so `null`, `undefined`, `0` and `""` are all "nothing to
    // say" rather than a cursor read off nothing.
    if !resp.is_truthy() {
        return false;
    }
    // `if (resp.evicted) return false` — truthiness again: the caller handles a buffer reset, and a
    // server that sends `evicted: 1` means it just as much as one that sends `true`.
    if prop(&resp, "evicted").is_truthy() {
        return false;
    }
    // `if (!advanced) return false` — nothing new was written (the SSE stream already caught up).
    if !advanced.is_truthy() {
        return false;
    }
    // `Number(resp.end)` — the ENGINE's ToNumber, so a numeric STRING is a cursor (`"500"` is 500),
    // `null` is 0, and a missing field is NaN.
    let end = to_number(&prop(&resp, "end"));
    if !end.is_finite() {
        return false; // no cursor → a legacy server, stop
    }
    end > to_number(&rendered)
}

/// Page bound: never chain more than this many reads (wedged-server guard).
#[wasm_bindgen]
pub fn adopt_page_exceeded(page: JsValue, max_pages: JsValue) -> bool {
    // `page > MAX_ADOPT_PAGES` — a COERCING comparison on both sides, which is what the TypeScript's
    // `>` does; a numeric string counts as a page number.
    to_number(&page) > to_number(&max_pages)
}

/// Split text into per-frame write slices (pure, unit-tested).
#[wasm_bindgen]
pub fn split_write_slices(text: JsValue, size: JsValue) -> Result<Array, JsValue> {
    let out = Array::new();
    if !text.is_truthy() {
        return Ok(out);
    }
    // `Math.max(1, Math.floor(size))` — `Math.max` PROPAGATES NaN where `f64::max` answers the other
    // operand, and a NaN step ends the loop below rather than raising. `Math.floor` is `f64::floor`,
    // `-0` and the infinities included.
    let n = js_max(1.0, to_number(&size).floor());
    // `text.length` — UTF-16 UNITS for a string, an element count for an array, and `undefined`
    // otherwise, which makes the loop not run at all (`i < undefined` is false).
    let len = if let Some(s) = text.as_string() {
        s.encode_utf16().count() as f64
    } else if text.is_array() {
        Array::from(&text).length() as f64
    } else {
        f64::NAN
    };
    let mut i = 0.0f64;
    while i < len {
        // `text.slice(i, i + n)` — through the engine, because the cut is in UTF-16 units and the
        // result can be half a character.
        out.push(&call_method(
            &text,
            "slice",
            &[JsValue::from_f64(i), JsValue::from_f64(i + n)],
        )?);
        i += n;
    }
    Ok(out)
}
