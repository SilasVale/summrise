//! THE PRESS RULE, AS THE BROWSER RENDERS IT — `pressDelta` from `agent/scripts/lib/design-sweep.mjs:808`.
//!
//! **WHAT THIS GATE IS FOR.** `pressDelta` decides whether a control's PRESSED state differs from its
//! HOVERED one — that is the whole of "the press renders". It sat in JavaScript because `pressPass`, the
//! function that drives a real mouse over a real control, calls it; the pass keeps the `page.mouse.down()`
//! and this file owns the rule the row is built from. The port is checked against the shipping JavaScript
//! on the same inputs (the differential in the commit that landed it), and `agent/tests/press_anchor.rs`
//! keeps reading the JS twin as the oracle for the emitted artifacts.
//!
//! **THE TWO CASES THE JAVASCRIPT LEAVES IMPLICIT ARE THE INTERESTING ONES**, and both are Review Focus 3
//! of the plan: a press target that VANISHED between discovery and press. `pressDelta(null, pressed)`
//! returns `[]` — and a Rust `unwrap()` on a missing side would panic where the JS answers. So a `None`
//! on either side is a test, not an oversight, and so is a KEY that one side does not carry, because
//! `hovered[k] !== pressed[k]` with both `undefined` is `false` in JavaScript and must stay `false` here.
//!
//! MUTATION: swap two entries of `KEYS` in `src/press.rs` (`"opacity"` and `"background"`).
//! RESULT:   `cargo test -p summrise-sweep-judge --test press` → exit 101 —
//!             the_order_is_the_js_key_order ... FAILED
//!             assertion `left == right` failed
//!               left: ["transform", "background", "opacity", "filter"]
//!              right: ["transform", "opacity", "background", "filter"]
//!           and `only_the_differing_keys_are_reported` stays green, which is the point: the ORDER is a
//!           decision the mutation breaks and the membership is not. Restoring `KEYS` returns 7 passed.

use serde_json::json;
use summrise_sweep_judge::press_delta;

/// The hovering snapshot: what the pointer already did to the control. `pressPass` takes this AFTER
/// `page.mouse.move`, which is why the rule is a hover→press comparison and not a rest→press one.
fn hovered() -> serde_json::Value {
    json!({
        "transform": "none",
        "opacity": "1",
        "background": "rgb(20, 20, 24)",
        "filter": "none",
    })
}

#[test]
fn no_press_reports_no_change() {
    assert!(press_delta(None, None).is_empty());
}

#[test]
fn a_missing_side_reports_no_change() {
    // Review Focus 3: the target vanished between discovery and press. Both directions.
    let h = hovered();
    assert!(press_delta(Some(&h), None).is_empty());
    assert!(press_delta(None, Some(&h)).is_empty());
}

/// **THE GUARD IS THE JAVASCRIPT'S `!snapshot`, NOT A NULL CHECK — AND THE FIRST DIFFERENTIAL RUN OF THIS
/// PORT DISAGREED WITH THE JAVASCRIPT ON EXACTLY THIS.** `serde_json` hands back `Some(&Value::Null)` for a
/// key whose value is `null`, so `if let (Some(h), Some(p))` accepted a snapshot the JavaScript throws
/// away: corpus case 2 (a control whose pressed read came back `null`) answered all four keys here against
/// the JavaScript's `[]`. These are the cases that separate the two rules, and each is falsy in JS.
#[test]
fn an_explicit_null_is_missing_like_the_javascript_says() {
    let h = hovered();
    let null = serde_json::Value::Null;
    assert!(press_delta(Some(&h), Some(&null)).is_empty());
    assert!(press_delta(Some(&null), Some(&h)).is_empty());
    assert!(press_delta(Some(&null), Some(&null)).is_empty());
}

/// The rest of `!x`: `false`, `0` and `""`. `[]` and `{}` are TRUTHY in JavaScript and are the other half
/// of the same rule — an empty snapshot does not short-circuit, and it carries none of the four keys, so
/// every key the other side has differs.
#[test]
fn the_other_falsy_snapshots_are_missing_too_but_an_empty_one_is_not() {
    let h = hovered();
    for falsy in [json!(false), json!(0), json!("")] {
        assert!(
            press_delta(Some(&h), Some(&falsy)).is_empty(),
            "`{falsy}` is falsy in JavaScript, so the guard fires"
        );
        assert!(
            press_delta(Some(&falsy), Some(&h)).is_empty(),
            "`{falsy}` is falsy in JavaScript, so the guard fires"
        );
    }
    assert_eq!(
        press_delta(Some(&json!([])), Some(&h)),
        vec!["transform", "opacity", "background", "filter"]
    );
}

#[test]
fn only_the_differing_keys_are_reported() {
    let h = hovered();
    let mut pressed = h.clone();
    pressed["transform"] = json!("matrix(1, 0, 0, 1, 0, 1)");
    assert_eq!(press_delta(Some(&h), Some(&pressed)), vec!["transform"]);
}

#[test]
fn the_order_is_the_js_key_order() {
    let h = hovered();
    let pressed = json!({
        "transform": "matrix(1, 0, 0, 1, 0, 1)",
        "opacity": "0.72",
        "background": "rgb(30, 30, 36)",
        "filter": "brightness(0.9)",
    });
    assert_eq!(
        press_delta(Some(&h), Some(&pressed)),
        vec!["transform", "opacity", "background", "filter"]
    );
}

/// The keys of the OBJECT must not decide the order — the JS `KEYS` array does. A row written with
/// `filter` first still reports `transform` first.
#[test]
fn the_objects_own_key_order_does_not_decide() {
    let h = json!({ "filter": "none", "background": "a", "opacity": "1", "transform": "none" });
    let pressed = json!({ "filter": "f", "background": "b", "opacity": "o", "transform": "t" });
    assert_eq!(
        press_delta(Some(&h), Some(&pressed)),
        vec!["transform", "opacity", "background", "filter"]
    );
}

/// A key one side does not carry is `undefined` on that side, so `undefined !== "x"` is TRUE in
/// JavaScript and the key IS reported. A key BOTH sides omit is `undefined !== undefined`, which is
/// FALSE — and that is the case a `.get(k).unwrap_or(&null)` would get wrong in the other direction.
#[test]
fn a_key_only_one_side_carries_is_a_change() {
    let h = json!({});
    let pressed = json!({ "transform": "matrix(1, 0, 0, 1, 0, 1)" });
    assert_eq!(press_delta(Some(&h), Some(&pressed)), vec!["transform"]);
    let both_empty = json!({});
    assert!(press_delta(Some(&h), Some(&both_empty)).is_empty());
}

/// A property the rule does not watch is not a change, however different it is — this is
/// `press_anchor.rs`'s `layout` case (`width`/`height`), carried to the Rust side.
#[test]
fn a_property_outside_the_four_keys_is_not_reported() {
    let h = json!({ "width": "12px", "height": "14px", "transform": "none" });
    let pressed = json!({ "width": "99px", "height": "42px", "transform": "none" });
    assert!(press_delta(Some(&h), Some(&pressed)).is_empty());
}
