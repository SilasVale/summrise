//! The panel's SESSION-IDLE family — one module, three sources, because they answer one question:
//! which sessions nobody is using, and which per-session views are still worth keeping.
//!
//!   * `lib/duration.ts`'s `human_idle` — HOW LONG A SILENCE LASTED, in the panel's own vocabulary.
//!     It has ONE owner because two files each kept a private copy and both had drifted from the shape
//!     the rest of the panel uses: both emitted `1h04m` where `useAttention`/`useMonitors` write
//!     `1h 04m`, and they disagreed with EACH OTHER below a minute (`45s` against `0m` — not a style
//!     difference, a false statement about a 45-second silence, reached by branch order).
//!   * `lib/idleSessions.ts`'s `idle_sessions` and `idle_offer_text` — the rule behind OFFERING to
//!     close a session before the device's sweeper takes it. The threshold is not arbitrary: the
//!     device's idle TTL is 15 minutes and the measured distribution is bimodal (seconds of silence for
//!     sessions in use, hours for sessions forgotten), so an hour sits in the empty middle. And
//!     silence is NOT idleness: a long command prints nothing for an hour, which is why the
//!     `commandRunning` guard is a condition and not belt and braces — an offer to close is an ACTION,
//!     and a wrong action is taken where a wrong mark is only read.
//!   * `lib/sessionViews.ts`'s `prune_session_views` — which per-session view choices are still worth
//!     keeping, with the IDENTITY RETURN as part of the contract (see its own note below).
//!
//! # THE NUMBER THAT STAYS
//!
//! `IDLE_OFFER_MS` is an ARGUMENT: it is the panel's own threshold, `lib/idleSessions.ts` exports it,
//! and it is also the JavaScript DEFAULT of `idleSessions(sessions, thresholdMs = …)` — which the
//! wrapper decides, because a JavaScript default parameter fires only for `undefined`.

use js_sys::{Array, Object};
use wasm_bindgen::prelude::*;

use crate::js::{call_method, js_add, js_max, number_text, read, to_js_string, to_number, type_of};

/// HOW LONG A SILENCE LASTED — `45s`, `4m`, `1h 04m`.
///
/// THE SHAPE IS THE PANEL'S DOMINANT ONE, with the seconds branch kept: under a minute, seconds are the
/// useful unit and `0m` is a lie about a 45-second silence.
#[wasm_bindgen]
pub fn human_idle(ms: JsValue) -> String {
    // `Math.max(0, Math.floor(ms / 1000))` — `ms` COERCES (a numeric string is a duration), `Math.floor`
    // is `f64::floor`, and `Math.max` propagates NaN where `f64::max` would answer the other operand.
    // `Math.floor(ms / 1000)` — THE DIVISION IS PART OF IT, and the first version of this port left it
    // out: the differential reported `16h 23.333333333333332m` for a 59-second silence, which is a
    // number that never went through the seconds branch at all.
    let s = js_max(0.0, (to_number(&ms) / 1000.0).floor());
    if s < 60.0 {
        return format!("{}s", number_text(s));
    }
    if s < 3600.0 {
        return format!("{}m", number_text((s / 60.0).floor()));
    }
    // `String(Math.floor((s % 3600) / 60)).padStart(2, "0")` — the minutes are PADDED, which is the
    // half the drifted copies got wrong (`1h04m`).
    format!(
        "{}h {}m",
        number_text((s / 3600.0).floor()),
        pad_start_2(&number_text(((s % 3600.0) / 60.0).floor()))
    )
}

/// `String(v).padStart(2, "0")` — a zero in front of a ONE-UNIT string. `"-5"` is already two units and
/// is left alone, which is what `padStart` does with it.
fn pad_start_2(s: &str) -> String {
    if s.encode_utf16().count() < 2 {
        format!("0{s}")
    } else {
        s.to_string()
    }
}

/// The sessions nobody is using: live, silent for longer than the threshold, and NOT holding a command.
///
/// CLOSED SESSIONS ARE NOT CANDIDATES: they have no shell to release (the device closed them when they
/// exited), and counting them would make the offer's number wrong. `savedOnly` is the same kind of
/// exclusion.
#[wasm_bindgen]
pub fn idle_sessions(sessions: JsValue, threshold_ms: JsValue) -> Result<Array, JsValue> {
    if !sessions.is_array() {
        // `sessions.filter` on a non-array is a TypeError in the TypeScript.
        return Err(JsValue::from_str(
            "idleSessions expects an array: the TypeScript's filter raises",
        ));
    }
    let threshold = to_number(&threshold_ms);
    let out = Array::new();
    for s in Array::from(&sessions).iter() {
        // A NULLISH SESSION RAISES (`null.closed` is a TypeError), and the reads are in the
        // TypeScript's order because its `&&` chain short-circuits exactly like this `||` does.
        if read(&s, "closed")?.is_truthy()
            || read(&s, "savedOnly")?.is_truthy()
            || read(&s, "commandRunning")?.is_truthy()
        {
            continue;
        }
        // `s.idleMs > thresholdMs` — a COERCING comparison, and the SAME session object is pushed
        // through, so a caller can compare identity with what it passed in.
        if to_number(&read(&s, "idleMs")?) > threshold {
            out.push(&s);
        }
    }
    Ok(out)
}

/// ONE LINE for the offer: how many, and the longest silence among them.
#[wasm_bindgen]
pub fn idle_offer_text(candidates: JsValue) -> Result<JsValue, JsValue> {
    if !candidates.is_array() {
        return Err(JsValue::from_str(
            "idleOfferText expects an array: the TypeScript's map raises",
        ));
    }
    let list = Array::from(&candidates);
    let n = list.length() as f64;
    if n == 0.0 {
        return Ok(JsValue::from_str(""));
    }
    // `Math.max(...candidates.map((s) => s.idleMs))` — the SPREAD of the mapped values, which coerces
    // each one and propagates NaN. (The spread's own argument-count limit is a JavaScript engine fact
    // this port does not reproduce; a session list that large is not one this panel can hold.)
    let mut longest = f64::NEG_INFINITY;
    let names = Array::new();
    for s in list.iter() {
        let idle = to_number(&read(&s, "idleMs")?);
        longest = if idle.is_nan() { f64::NAN } else { longest.max(idle) };
        // `s.label || s.sid` — TRUTHINESS, so an empty label falls through to the id.
        let label = read(&s, "label")?;
        names.push(&if label.is_truthy() {
            label
        } else {
            read(&s, "sid")?
        });
    }
    // `names.slice(0, 3).join(", ")` — through the engine, because a label can carry anything a wire
    // string can, and `Array.prototype.join` is the rule for the holes.
    let head = call_method(&names, "slice", &[JsValue::from_f64(0.0), JsValue::from_f64(3.0)])?;
    let joined = call_method(&head, "join", &[JsValue::from_str(", ")])?;
    let more = if n > 3.0 {
        format!(" +{} more", number_text(n - 3.0))
    } else {
        String::new()
    };
    // The sentence is built through the engine for the same reason the recipe's body is: a label is a
    // wire string and a lone surrogate cannot exist in a Rust `String`.
    let mut text = js_add(
        &JsValue::from_str(&format!(
            "{} session{} idle for up to ",
            number_text(n),
            if n == 1.0 { "" } else { "s" }
        )),
        &JsValue::from_str(&human_idle(JsValue::from_f64(longest))),
    );
    text = js_add(&text, &JsValue::from_str(" — "));
    text = js_add(&text, &js_add(&joined, &JsValue::from_str(&more)));
    Ok(text)
}

/// Which per-session views are still worth keeping.
///
/// THE IDENTITY RETURN IS PART OF THE CONTRACT, not an optimisation. The caller's effect depends on the
/// session list, which changes on every poll; returning a fresh object whenever nothing was pruned
/// would re-render `App` once per poll for ever. **Unchanged in, SAME OBJECT out.**
#[wasm_bindgen]
pub fn prune_session_views(views: JsValue, live_sids: JsValue) -> Result<JsValue, JsValue> {
    // `new Set(liveSids)` — the ITERABLE protocol, so an array, a string, a Set and a Map all work and
    // a non-iterable raises, which is what the TypeScript does.
    let mut live: Vec<String> = Vec::new();
    // **`new Set(null)` IS AN EMPTY SET, NOT A TypeError** — the spec skips a nullish iterable — while
    // `new Set(5)` DOES raise. The differential found this: the first version threw for both, and
    // `pruneSessionViews({"a":1}, null)` answers `{}` in the TypeScript.
    if live_sids.is_array() {
        for item in Array::from(&live_sids).iter() {
            live.push(to_js_string(&item).as_string().unwrap_or_default());
        }
    } else if let Some(text) = live_sids.as_string() {
        // A STRING IS ITERABLE, and its iterator yields CODE POINTS — so `new Set("🚀")` is ONE element,
        // not two. (`try_iter` in this binding does not walk a string at all, which is how the
        // differential found this: `pruneSessionViews(views, "ab")` threw here and answered `{}`
        // there.)
        for c in text.chars() {
            live.push(c.to_string());
        }
    } else if !live_sids.is_null() && !live_sids.is_undefined() {
        // `try_iter` answers `Ok(None)` for a value with no iterator and `Err` when `next()` throws.
        let Some(iter) = js_sys::try_iter(&live_sids)
            .map_err(|_| JsValue::from_str("liveSids: next() threw"))?
        else {
            return Err(JsValue::from_str(
                "liveSids is not iterable: the TypeScript's new Set raises",
            ));
        };
        for item in iter {
            let item = item.map_err(|_| JsValue::from_str("liveSids: next() threw"))?;
            live.push(to_js_string(&item).as_string().unwrap_or_default());
        }
    }
    // `Object.keys(views)` — a NULLISH views raises; a primitive answers no keys, and the identity
    // return below then hands the primitive back untouched (which is what the TypeScript does).
    if views.is_null() || views.is_undefined() {
        return Err(JsValue::from_str(
            "Object.keys(views): the TypeScript raises on a nullish views",
        ));
    }
    if type_of(&views) != "object" {
        return Ok(views);
    }
    let keys = Object::keys(&views.clone().dyn_into::<Object>().map_err(|_| {
        JsValue::from_str("views is not an object the TypeScript can enumerate")
    })?);
    let mut kept: Vec<String> = Vec::new();
    for k in keys.iter() {
        let sid = to_js_string(&k).as_string().unwrap_or_default();
        if live.contains(&sid) {
            kept.push(sid);
        }
    }
    let total = keys.length() as usize;
    if kept.len() == total {
        return Ok(views);
    }
    let next = Object::new();
    for sid in kept {
        let v = js_sys::Reflect::get(&views, &JsValue::from_str(&sid)).unwrap_or(JsValue::UNDEFINED);
        let _ = js_sys::Reflect::set(&next, &JsValue::from_str(&sid), &v);
    }
    Ok(next.into())
}
