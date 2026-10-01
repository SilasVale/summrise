//! `lib/monitorMark.ts` — THE MONITOR MARK'S STATE, DERIVED ONCE.
//!
//! WHY IT EXISTS (round 126 of the standing goal). The spine's rule is one source of truth per fact,
//! and this family was computing the same fact in two components with THREE SPELLINGS: `MonitorAlerts`
//! wrote `a.up ? "is-up" : "is-down"`, `MonitorChip` wrote `is-flapping` for a flapping target and a
//! BARE `monitor-mark` for a down one — so the sheet had to know that "no modifier" means down in one
//! place and that `is-down` means it in another. Round 126 found that by tracing the fact, not by
//! reading the sheet.
//!
//! WHAT IT DOES NOT DO: change a single class on screen. The output is exactly what both components
//! already rendered — the point is that the STATES are named once, and a second rendering of the family
//! cannot invent a fourth spelling.
//!
//! # AND THIS FILE IS THE ONLY PLACE THE LITERALS MAY APPEAR
//!
//! `agent/tests/one_derivation.rs` fails any module that spells `is-up`, `is-down` or `is-flapping` for
//! a mark. With the derivation here, that rule is enforced on BOTH sides of the boundary: the gate scans
//! the panel's TypeScript (where no file may spell them now, the wrapper included) AND this crate's Rust
//! sources, where `marks.rs` is the declared home.

use js_sys::Array;
use wasm_bindgen::prelude::*;

use crate::js::{call_method, to_js_string};

/// THE MODIFIER ALONE — the same state, for the elements that carry the family's vocabulary without
/// being the mark itself.
///
/// `MonitorAlerts` renders `<div className={"monitor-alert " + …}>` around a `<span className=
/// "monitor-mark …">`, and both describe ONE fact: this alert is up or down. Round 126 gave the mark
/// the derivation and left the container spelling the words by hand — the gate this module is guarded
/// by found it on its next run, which is what it is for.
///
/// THE COMPARISON IS STRICT, and anything that is not `"up"` or `"down"` is `is-flapping` — which is
/// what the TypeScript's chained ternary does with a value that is neither.
#[wasm_bindgen]
pub fn monitor_modifier(state: JsValue) -> String {
    match state.as_string().as_deref() {
        Some("up") => "is-up",
        Some("down") => "is-down",
        _ => "is-flapping",
    }
    .to_string()
}

/// The class list for a monitor mark. `flapping` wins over `up`: a target that is up now but has been
/// dropping is the thing the chip exists to say.
///
/// `["monitor-mark", modifier, extra].filter(Boolean).join(" ")` — TRUTHINESS drops a falsy `extra`
/// (an empty string, `null`, `0`), and the join goes through the ENGINE because an `extra` can carry
/// anything a wire string can, a lone surrogate included.
#[wasm_bindgen]
pub fn monitor_mark_class(state: JsValue, extra: JsValue) -> Result<JsValue, JsValue> {
    let parts = Array::new();
    parts.push(&JsValue::from_str("monitor-mark"));
    parts.push(&JsValue::from_str(&monitor_modifier(state)));
    // `extra = ""` in the TypeScript is a DEFAULT PARAMETER, which fires only for `undefined` — the
    // wrapper decides that case, and `null` or `0` arrive here to be filtered out by truthiness.
    if extra.is_truthy() {
        parts.push(&to_js_string(&extra));
    }
    call_method(&parts, "join", &[JsValue::from_str(" ")])
}
