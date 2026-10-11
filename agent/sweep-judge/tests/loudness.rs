//! `loudnessOf(colour)` — `agent/scripts/lib/design-sweep.mjs:1644`, and the two review-focus cases the
//! JavaScript has no test for.
//!
//! **WHAT IT DECIDES.** The graphics axis asks whether a filled element is SHOUTING: a saturated, mid-light
//! colour reads as a signal, and the same colour at the extremes of lightness reads as a surface. The rule
//! the probe evaluates in the page is three lines of HSL, and the page-side probe still evaluates it —
//! `marksProbe` binds `__loudnessOf` through `withHelpers` — so this is the RUST TWIN of the rule, landed
//! beside the JavaScript rather than in place of it, exactly as `press_delta` is.
//!
//! **BOTH SIDES OF EVERY THRESHOLD, BECAUSE A THRESHOLD TESTED ONLY FROM INSIDE PASSES FOR ANY THRESHOLD.**
//! The plan names this as the reason the task has its own test file. The three colours below were found by
//! searching the 0-255 cube for the integer triples that straddle each boundary under the JavaScript's OWN
//! arithmetic (`r/255`, `Math.max`, the HSL saturation), and each one's computed `(sat, l)` is written beside
//! it so a reader can see which side it lands on rather than trusting the assertion:
//!
//! ```text
//! sat >= 0.35   {r:173,g:82,b:82}  sat 0.35686274  LOUD     | {r:172,g:83,b:83}  sat 0.34901960  not loud
//! l   >= 0.20   {r:102,g:0,b:0}    l   0.2         LOUD     | {r:101,g:1,b:0}    l   0.19803921  not loud
//! l   <= 0.90   {r:255,g:204,b:255} l  0.9         LOUD     | {r:255,g:205,b:255} l  0.90196078  not loud
//! ```
//!
//! The two `l` cases sit exactly ON their boundary (`>=` and `<=` are inclusive), which is the half a
//! from-inside test never reaches: a mutation to `0.2000001` or to `0.8999999` breaks them and leaves every
//! other case green.
//!
//! MUTATION: change `SAT_FLOOR` from `0.35` to `0.34` in `src/loudness.rs`.
//! RESULT:   `cargo test -p summrise-sweep-judge --test loudness` → exit 101, MEASURED:
//!
//! ```text
//! the_saturation_threshold_has_both_sides ... FAILED
//! assertion `left == right` failed: {r:172,g:83,b:83} sat 0.34901961 is BELOW the floor
//!   left: true
//!  right: false
//! test result: FAILED. 8 passed; 1 failed
//! ```
//!
//! which is exactly the one case the moved bound should break, and the one that would have gone on passing
//! had the test only asserted a saturated colour.

use serde_json::json;
use summrise_sweep_judge::loudness_of;

/// An opaque `rgb()` triple in the shape the probe's `parse()` produces: three 0-255 channels.
fn rgb(r: f64, g: f64, b: f64) -> serde_json::Value {
    json!({ "r": r, "g": g, "b": b, "a": 1.0 })
}

#[test]
fn a_saturated_mid_light_colour_is_loud() {
    let l = loudness_of(&rgb(255.0, 80.0, 0.0));
    assert!(l.loud, "{l:?}");
    assert!(l.sat >= 0.35 && (0.2..=0.9).contains(&l.l), "{l:?}");
}

#[test]
fn the_saturation_threshold_has_both_sides() {
    // l == 0.5 by construction (the channels sum to 255), so `sat` is the changed variable.
    let inside = loudness_of(&rgb(173.0, 82.0, 82.0));
    assert!(
        inside.loud,
        "sat {} is ABOVE the 0.35 floor: {inside:?}",
        inside.sat
    );
    let outside = loudness_of(&rgb(172.0, 83.0, 83.0));
    assert!(
        !outside.loud,
        "sat {} is BELOW the 0.35 floor, and the floor is the only thing that changed: {outside:?}",
        outside.sat
    );
    assert!(
        (0.2..=0.9).contains(&outside.l),
        "the l of this pair is mid-range on purpose, so the saturation is what decides it: {outside:?}"
    );
}

#[test]
fn the_lightness_floor_has_both_sides() {
    // Both are fully saturated, so only `l >= 0.2` can tell them apart — and the first sits exactly ON it.
    let on_the_boundary = loudness_of(&rgb(102.0, 0.0, 0.0));
    assert!(
        on_the_boundary.loud,
        "l == 0.2 exactly, and `>=` is inclusive: {on_the_boundary:?}"
    );
    let below = loudness_of(&rgb(101.0, 1.0, 0.0));
    assert!(
        !below.loud,
        "l {} is below 0.2 and the colour is fully saturated: {below:?}",
        below.l
    );
}

#[test]
fn the_lightness_ceiling_has_both_sides() {
    let on_the_boundary = loudness_of(&rgb(255.0, 204.0, 255.0));
    assert!(
        on_the_boundary.loud,
        "l == 0.9 exactly, and `<=` is inclusive: {on_the_boundary:?}"
    );
    let above = loudness_of(&rgb(255.0, 205.0, 255.0));
    assert!(
        !above.loud,
        "l {} is above 0.9 and the colour is fully saturated: {above:?}",
        above.l
    );
}

/// The greys: `d == 0` is the JavaScript's own branch and gives `sat = 0` rather than `0/0`.
#[test]
fn a_grey_has_no_saturation_and_is_not_loud() {
    let mid = loudness_of(&rgb(128.0, 128.0, 128.0));
    assert_eq!(mid.sat, 0.0);
    assert!(!mid.loud, "{mid:?}");
    let black = loudness_of(&rgb(0.0, 0.0, 0.0));
    assert_eq!(black.sat, 0.0);
    assert_eq!(black.l, 0.0);
    assert!(!black.loud, "l == 0 is below the 0.2 floor: {black:?}");
    let white = loudness_of(&rgb(255.0, 255.0, 255.0));
    assert_eq!(white.sat, 0.0);
    assert_eq!(white.l, 1.0);
    assert!(!white.loud, "l == 1 is above the 0.9 ceiling: {white:?}");
}

/// **REVIEW FOCUS 2.** A colour record with no `r` is what the probe's `parse()` produces when the channel
/// list is short, and `undefined / 255` is `NaN` in JavaScript. What the plan asks for is that the `false`
/// be a MEASUREMENT rather than an accident: the fields must be non-finite AND `loud` must be false, so a
/// caller that reads `loud` without reading `sat` cannot mistake "not measured" for "measured, quiet".
///
/// `Number.isFinite(sat) && Number.isFinite(l) && …` is what makes that true in the JavaScript, and the
/// guard is transcribed rather than relied on: with `&&` the comparison would also be false for `NaN`, but
/// only by luck of IEEE comparison rules, and the next person to reorder the clauses should not have to
/// re-derive that.
#[test]
fn a_colour_with_no_r_is_not_loud_and_says_it_measured_nothing() {
    let l = loudness_of(&json!({ "g": 0.0, "b": 0.0, "a": 1.0 }));
    assert!(!l.loud, "a missing channel is not a quiet colour: {l:?}");
    assert!(!l.sat.is_finite(), "sat must be NaN, not a default: {l:?}");
    assert!(!l.l.is_finite(), "l must be NaN, not a default: {l:?}");
}

/// **THE SAME QUESTION FOR A `null` CHANNEL, AND THE ANSWER IS NOT THE OBVIOUS ONE.** `undefined / 255` is
/// `NaN`, but `null / 255` is **`0`** — JavaScript's `Number(null)` is zero, so a channel that crossed JSON
/// as `null` reads as BLACK on that channel rather than as unmeasurable, and the fields come back FINITE.
/// This is the case a "sensible" port gets wrong in the safe-looking direction (NaN is "clearly" what a
/// missing number means), and it is the reason the coercion is a named function here rather than
/// `as_f64().unwrap_or(NAN)`.
#[test]
fn a_null_channel_reads_as_zero_like_the_javascript() {
    let l = loudness_of(&json!({ "r": null, "g": 0.0, "b": 0.0 }));
    assert_eq!(l.sat, 0.0, "{l:?}");
    assert_eq!(l.l, 0.0, "{l:?}");
    assert!(
        l.sat.is_finite() && l.l.is_finite(),
        "`Number(null)` is 0, not NaN: {l:?}"
    );
    assert!(!l.loud, "black is below the lightness floor: {l:?}");
}

/// The other coercions a JSON colour can carry, each spelled out so the rule is readable in one place: a
/// numeric string is a number, an empty or blank string is zero, a boolean is 0/1, and a string that is not
/// a number is `NaN` — which is `Number("abc")`, and which makes the row unmeasurable rather than quiet.
#[test]
fn the_numbers_javascript_would_make_of_a_channel() {
    let feathers = |v: serde_json::Value| loudness_of(&json!({ "r": v, "g": 0.0, "b": 0.0 }));
    assert_eq!(feathers(json!("12")).l, 12.0 / 510.0, "`\"12\"` is 12");
    assert_eq!(feathers(json!("")).l, 0.0, "`Number(\"\")` is 0");
    assert_eq!(feathers(json!("  ")).l, 0.0, "`Number(\"  \")` is 0");
    assert_eq!(feathers(json!(true)).l, 1.0 / 510.0, "`Number(true)` is 1");
    assert_eq!(feathers(json!(false)).l, 0.0, "`Number(false)` is 0");
    let not_a_number = feathers(json!("abc"));
    assert!(!not_a_number.loud, "{not_a_number:?}");
    assert!(
        !not_a_number.sat.is_finite() && !not_a_number.l.is_finite(),
        "{not_a_number:?}"
    );
}

/// **A CHANNEL BIG ENOUGH TO BREAK THE HSL FORMULA, PINNED BECAUSE THE ANSWER IS NOT WHAT IT LOOKS LIKE.**
/// `l` is a huge FINITE number (1e308/510), and `1 - |2l - 1|` is then hugely NEGATIVE — so `sat` comes back
/// as **`-1.0`**, a saturation below zero, which the `sat >= 0.35` floor refuses. The JavaScript agrees
/// exactly (`{"sat":-1,"l":1.9607843137254903e+305,"loud":false}`, read off the shipping function before
/// this test was written), which is the point: a port that "fixed" the negative saturation would be a
/// second rule.
///
/// It is also the honest note on the `Number.isFinite` guard: **no input can make `l` infinite through this
/// path** (255 is not zero, so `r/255` is finite for every finite `r`, and serde_json carries no infinity),
/// and a `NaN` already fails every comparison. So the guard is belt-and-braces — transcribed because it is
/// the JavaScript's own clause, not because a case reaches it.
#[test]
fn a_channel_that_breaks_the_hsl_formula_is_refused_by_the_floor() {
    let l = loudness_of(&json!({ "r": 1e308, "g": 0.0, "b": 0.0 }));
    assert!(!l.loud, "{l:?}");
    assert!(
        l.l.is_finite() && l.l > 0.9,
        "the lightness is huge but finite: {l:?}"
    );
    assert_eq!(
        l.sat, -1.0,
        "the denominator goes negative, and the JS says -1 too: {l:?}"
    );
}

/// A snapshot that is not an object at all: the JavaScript would throw on `null.r`, and a rows document is
/// not the place to throw. `NaN` is the answer that makes the row unmeasurable, which is what the reader
/// upstream already knows how to report.
#[test]
fn a_colour_that_is_not_an_object_is_unmeasurable_rather_than_a_panic() {
    for colour in [json!(null), json!(0), json!("rgb(1,2,3)")] {
        let l = loudness_of(&colour);
        assert!(!l.loud, "{colour}: {l:?}");
        assert!(!l.sat.is_finite() && !l.l.is_finite(), "{colour}: {l:?}");
    }
}
