//! `loudnessOf(colour)` — ported from `agent/scripts/lib/design-sweep.mjs:1644`.
//!
//! **WHAT IT DECIDES.** The graphics axis asks whether a filled element is SHOUTING. Saturation 0.35 with a
//! lightness between 0.2 and 0.9 is what an operator reads as "something shouting", and the bands are
//! MEASURED rather than chosen — the rule is three lines of HSL and the page-side probe has always
//! evaluated it. `marksProbe` binds it through `withHelpers` as `__loudnessOf`, so what lands here is the
//! RUST TWIN of a rule a browser still runs, not a replacement for it — the same arrangement `press.rs` has
//! with `pressPass`.
//!
//! **AND THE ARITHMETIC IS THE JAVASCRIPT'S, INCLUDING THE PARTS A PORT WOULD "TIDY".** Three of them are
//! load-bearing and each has a test:
//!
//!   * **`Math.max`/`Math.min` ARE NaN-PROPAGATING; `f64::max` IS NOT.** `Math.max(NaN, 0.4)` is `NaN`,
//!     while Rust's `f64::max` returns the other operand — so `js_max`/`js_min` exist, and a channel that is
//!     not a number makes the whole lightness `NaN` rather than half a colour.
//!   * **`Number.isFinite` IS AN EXPLICIT GUARD, NOT A CONSEQUENCE OF `>=`.** `NaN >= 0.35` is false by
//!     IEEE rules, which happens to give the right answer — and would stop giving it the moment somebody
//!     reordered the clauses. The guard is transcribed so `loud: false` is a MEASUREMENT (Review Focus 2 of
//!     the plan) rather than an accident of comparison.
//!   * **`d === 0` IS THE GREY BRANCH.** It is `false` for `NaN`, which is how `NaN` reaches the division
//!     instead of becoming a quiet `0`.
//!
//! **`Number(x)`, NOT `x`.** `undefined / 255` is `NaN` but `null / 255` is **`0`** — JavaScript coerces —
//! so a channel that arrived as `null` reads as black on that channel and the fields come back FINITE. The
//! `to_number` function below is that coercion, written out for the shapes a JSON colour can carry, because
//! `as_f64().unwrap_or(NAN)` is the "sensible" answer and it is the wrong one.
//!
//! MUTATION: `SAT_FLOOR` from `0.35` to `0.34`.
//! RESULT:   `cargo test -p summrise-sweep-judge --test loudness` → exit 101, MEASURED:
//!
//! ```text
//! --test loudness   the_saturation_threshold_has_both_sides ... FAILED
//!                     sat 0.3490196078431373 is BELOW the 0.35 floor, and the floor is the only thing
//!                     that changed: Loudness { sat: 0.3490196078431373, l: 0.5, loud: true }
//!                   test result: FAILED. 9 passed; 1 failed
//! --lib             loudness::tests::the_bands_are_the_javascripts ... FAILED
//!                   test result: FAILED. 49 passed; 1 failed
//! ```
//!
//! **TWO GATES BREAK AND THEY ARE TWO DIFFERENT KINDS OF GATE**: the inline one reads the CONSTANT and says
//! the bands changed, and the integration one reads the RULE and says which colour moved — a mutation that
//! edits a threshold in the source would otherwise be invisible to a suite that only ever tested colours
//! comfortably inside the band. Neither absorbs the other.

use serde_json::Value;

/// The three bands, named so a mutation is a one-token edit and the test's message can quote it.
pub const SAT_FLOOR: f64 = 0.35;
pub const L_FLOOR: f64 = 0.2;
pub const L_CEIL: f64 = 0.9;

/// What the graphics axis needs to know about one colour, in the JavaScript's own field names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loudness {
    /// HSL saturation, `d / (1 - |2l - 1|)` — `0` for a grey and `NaN` for a colour with no readable channel.
    pub sat: f64,
    /// HSL lightness, `(max + min) / 2` over the three channels scaled to 0..1.
    pub l: f64,
    /// Whether this colour is a signal rather than a surface. **`false` COVERS TWO DIFFERENT THINGS** — a
    /// colour that was measured and is quiet, and a colour that could not be measured at all — and a caller
    /// tells them apart by asking whether `sat`/`l` are finite. The probe does exactly that, which is why
    /// the pair is published rather than collapsed into the boolean.
    pub loud: bool,
}

/// `loudnessOf(colour)`, the whole rule.
pub fn loudness_of(colour: &Value) -> Loudness {
    let channel = |k: &str| to_number(colour.get(k)) / 255.0;
    let (r, g, b) = (channel("r"), channel("g"), channel("b"));
    let mx = js_max(r, g, b);
    let mn = js_min(r, g, b);
    let l = (mx + mn) / 2.0;
    let d = mx - mn;
    let sat = if d == 0.0 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs())
    };
    Loudness {
        sat,
        l,
        // `Number.isFinite(sat) && Number.isFinite(l) && sat >= 0.35 && l >= 0.2 && l <= 0.9`, clause for
        // clause — the lightness pair written as a range because clippy requires it of the two comparisons
        // and the two spellings are the same rule for every `f64`, `NaN` included.
        loud: sat.is_finite()
            && l.is_finite()
            && sat >= SAT_FLOOR
            && (L_FLOOR..=L_CEIL).contains(&l),
    }
}

/// `Math.max` over three JS numbers: **any `NaN` makes the result `NaN`**, where `f64::max` would return a
/// non-`NaN` operand. The sign of a zero follows `Math.max` (`-0` loses to `+0`), which `f64::max` also does.
fn js_max(a: f64, b: f64, c: f64) -> f64 {
    if a.is_nan() || b.is_nan() || c.is_nan() {
        f64::NAN
    } else {
        a.max(b).max(c)
    }
}

/// `Math.min`, with the same `NaN` propagation.
fn js_min(a: f64, b: f64, c: f64) -> f64 {
    if a.is_nan() || b.is_nan() || c.is_nan() {
        f64::NAN
    } else {
        a.min(b).min(c)
    }
}

/// **`Number(x)` FOR THE SHAPES A JSON COLOUR CAN CARRY.** The probe divides `colour.r` by 255 directly, so
/// whatever is in the field is coerced by JavaScript's own rules first: `Number(undefined)` and
/// `Number("abc")` are `NaN`, `Number(null)` is **`0`**, `Number("")` is `0`, `Number(true)` is `1`. An
/// array coerces through its single element (`[]` → 0, `[5]` → 5) and anything longer is `NaN`; an object
/// is `NaN`. Only the first of those is reachable from the probe, and the rest are here so the port is the
/// SAME RULE rather than a narrower one that happens to agree on today's inputs.
fn to_number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                0.0
            } else {
                t.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        Some(Value::Array(a)) => match a.len() {
            0 => 0.0,
            1 => to_number(a.first()),
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The thresholds are the JavaScript's, spelled once, so a mutation to a band is a visible diff rather
    /// than a number buried in a boolean expression.
    #[test]
    fn the_bands_are_the_javascripts() {
        assert_eq!((SAT_FLOOR, L_FLOOR, L_CEIL), (0.35, 0.2, 0.9));
    }

    /// `Math.max(NaN, x)` is `NaN` in JavaScript and `x` in Rust. This is the divergence `js_max` exists
    /// for, and it is asserted directly because no colour in the corpus reaches it by accident.
    #[test]
    fn a_nan_channel_makes_the_whole_lightness_nan() {
        assert!(js_max(f64::NAN, 0.5, 0.5).is_nan());
        assert!(js_min(f64::NAN, 0.5, 0.5).is_nan());
        assert_eq!(
            0.5f64.max(f64::NAN),
            0.5,
            "this is the Rust default being refused"
        );
        let l = loudness_of(&json!({ "r": null, "g": 1.0, "b": 1.0 }));
        assert!(l.l.is_finite() && l.l > 0.0, "`null` is 0, not NaN: {l:?}");
    }
}
