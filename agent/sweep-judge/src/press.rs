//! `pressDelta(hovered, pressed)` — ported from `agent/scripts/lib/design-sweep.mjs:808`.
//!
//! **WHAT A PRESS ADDED, MEASURED AGAINST THE HOVERED STATE RATHER THAN THE RESTING ONE.** `pressPass`
//! used to snapshot the control with the pointer parked away from it, so a sheet that answered `:hover`
//! and had no `:active` rule at all still produced a difference — the hover did it — and the row read
//! `changed: true`, which the judge reports as a press that renders. Measured on the device, on the
//! landing's theme toggle, which has a `:hover` rule and NO press rule: resting → hovered changes
//! `background` and `color`; hovered → pressed changes NOTHING. A hover ALWAYS co-occurs with a press, so
//! the hover is the baseline the press has to beat.
//!
//! **PURE ON PURPOSE, AND THIS IS THE FUNCTION THE PORT TOOK FIRST.** The DOM loop around it cannot be
//! exercised off a browser, so the part that decides the verdict is a function with a test — the same
//! reason `svgRootPaints` exists in the contrast probe. That property is also why this module could leave
//! JavaScript at all while `pressPass` itself stays: `pressPass` performs the platform calls (a real
//! `mouse.move`, a real `mouse.down`) and this decides what they MEAN.
//!
//! **WHY THE FOUR KEYS, AND WHY IN THIS ORDER.** They are the four properties a press can change without
//! moving anything: `transform` (a scale or a translate), `opacity` (a dim), `background` (a fill) and
//! `filter` (a brightness). Layout properties are deliberately absent — `press_anchor.rs` pins that with
//! its `layout` case, because a control that reflows under the pointer is a different finding. The order
//! is the JavaScript's own `KEYS` array and the row's reader prints the list in it, so it is asserted as
//! a literal rather than left to the object's key order.
//!
//! MUTATION: swap two entries of `KEYS` — `"opacity"` and `"background"`.
//! RESULT:   `cargo test -p summrise-sweep-judge --test press` → exit 101, MEASURED (not predicted):
//!
//! ```text
//! test the_objects_own_key_order_does_not_decide ... FAILED
//! test the_order_is_the_js_key_order ... FAILED
//!   left: ["transform", "background", "opacity", "filter"]
//!  right: ["transform", "opacity", "background", "filter"]
//! test result: FAILED. 5 passed; 2 failed
//! ```
//!
//! TWO tests fail rather than one, and that is the honest count: both assert the literal order, the second
//! of them to prove the OBJECT's key order does not decide it. The membership tests stay green, which is what
//! makes the order the thing this gate carries — an order tested only for membership passes for any order.
//!
//! (THE FENCE ABOVE IS NOT DECORATION: this block was written as a four-space-indented block first, and
//! rustdoc read it as RUST — `cargo test -p summrise-sweep-judge` failed its doctest with `prefix OBJECT is
//! unknown` and `expected one of ! or ::, found the_objects_own_key_order_does_not_decide`. A quoted block
//! in a doc comment is a program, which is the same lesson the probes taught from the other side.)

use serde_json::Value;

/// The four properties a press can change, in the order the JavaScript names them. Public because the
/// driver's step list and the judge's sentence about a press both read the same four.
pub const KEYS: [&str; 4] = ["transform", "opacity", "background", "filter"];

/// The keys whose values differ between the hovered and pressed snapshots, in `KEYS` order.
///
/// **A MISSING SIDE IS `[]`, AND THAT IS REVIEW FOCUS 3 OF THE PLAN.** A press target that vanished
/// between discovery and press is a real case — the discovery walks the DOM, and the page can re-render
/// before the pointer arrives — and the JavaScript answers `if (!hovered || !pressed) return []`. An
/// `expect()` here would turn a page that changed under the instrument into a panic on the device, and a
/// panic is not a measurement.
///
/// **AND "MISSING" IS THE JAVASCRIPT'S OWN TEST, WHICH IS `!snapshot` RATHER THAN `=== null`.** That
/// distinction is not pedantry: the first differential run of this port disagreed with the JavaScript on
/// exactly it. `serde_json` gives `Some(&Value::Null)` for a key whose value is `null`, so a
/// `let (Some(h), Some(p)) = …` guard accepted a snapshot the JavaScript throws away — and case 2 of the
/// corpus (`{"h": {…}, "p": null}`, a control whose pressed read came back empty) answered
/// `["transform","opacity","background","filter"]` here against the JavaScript's `[]`. So the guard is
/// `falsy`, which reproduces `!x` over every JSON value there is.
///
/// **A KEY ONE SIDE DOES NOT CARRY IS A CHANGE; A KEY NEITHER CARRIES IS NOT.** That is what
/// `hovered[k] !== pressed[k]` does in JavaScript when the property is absent — `undefined !== "x"` is
/// true, `undefined !== undefined` is false — and it is preserved rather than "tidied", because a row
/// that omits a property is exactly how a partial snapshot arrives.
pub fn press_delta(hovered: Option<&Value>, pressed: Option<&Value>) -> Vec<&'static str> {
    if falsy(hovered) || falsy(pressed) {
        return Vec::new();
    }
    let (Some(hovered), Some(pressed)) = (hovered, pressed) else {
        unreachable!(
            "`falsy` accepts only `None` and values that are not, so both sides are `Some` here"
        );
    };
    KEYS.iter()
        .copied()
        .filter(|k| hovered.get(*k) != pressed.get(*k))
        .collect()
}

/// **`!x`, FOR EVERY `x` A JSON DOCUMENT CAN CARRY.** The JavaScript's guard is a truthiness test, and a
/// port that narrowed it to `null` would be a quieter rule than the one it replaced — which is the kind of
/// difference that only shows up as a row nobody can explain. `NaN` is not representable in JSON and is
/// therefore not here; a value that is not falsy in this table is truthy in JavaScript too.
fn falsy(v: Option<&Value>) -> bool {
    match v {
        None => true,
        Some(Value::Null) => true,
        Some(Value::Bool(b)) => !b,
        Some(Value::Number(n)) => n.as_f64() == Some(0.0),
        Some(Value::String(s)) => s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule in one line, so the inline module and `tests/press.rs` cannot drift about what it is:
    /// the four keys, in that order, whose values differ, and nothing when a side is missing.
    #[test]
    fn the_keys_are_the_four_the_js_names() {
        assert_eq!(KEYS, ["transform", "opacity", "background", "filter"]);
    }
}
