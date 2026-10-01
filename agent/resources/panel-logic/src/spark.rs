//! `lib/spark.ts` — the SHAPE of a series: sparkline geometry, its summary, and the one rule that
//! decides whether a sustained load is worth interrupting an operator for.
//!
//! WHY A SHAPE AT ALL. Every instrument this panel had was instantaneous: a dial, two percentages, a
//! number of seconds. "CPU 87%" is a reading; "CPU has been above 90% for twelve minutes" is a fact
//! somebody can act on, and the difference between them is the series — which the device keeps (see
//! `metrics.rs`) and this module draws and reads.
//!
//! TWO RULES, BOTH PURE AND BOTH HERE:
//!   * `spark_segments` turns readings into path data for an SVG polyline, BREAKING the line where a
//!     reading is missing rather than bridging it. A chart that connects across a gap asserts values
//!     nobody measured — the same reason this panel renders an em dash for an unknown reading instead
//!     of a zero.
//!   * `load_notice` decides whether the recent series says something an operator should be told
//!     without asking. It says nothing until it has enough evidence, and nothing when the load is
//!     merely high ONCE — a spike is not a condition.
//!
//! # THE TWO NUMBERS THE CALLER STILL OWNS
//!
//! `LOAD_WINDOW_MS` and `LOAD_MIN_SAMPLES` are ARGUMENTS rather than constants in here, which is the
//! arrangement `liveness.rs` records for `WORKING_MS`: they are the SURFACE'S numbers — how far back
//! an operator's "just now" reaches, and how much evidence this panel will wait for — and the panel's
//! own test imports them. `CRIT_PCT` and `SUSTAINED_FRACTION` are not: they are the rule's own
//! thresholds, they are not exported anywhere, and the module that decides "sustained" is this one.
//!
//! # THE THREE ENGINE BEHAVIOURS A PORT HAS TO GET RIGHT
//!
//!   * **`typeof NaN === "number"`, SO A NaN READING IS A READING.** `seriesValues` filters by `typeof`,
//!     so `[1, NaN, 2]` has THREE known values and `Math.min(...)` answers NaN — and `toFixed(2)` of
//!     NaN is the string `"NaN"`, which is what the path data then carries. A port that treated NaN
//!     as absent would draw a different chart and report a different average.
//!   * **`Math.min`/`Math.max` PROPAGATE NaN** where `f64::min`/`f64::max` return the OTHER operand.
//!   * **`toFixed(2)` TAKES THE LARGER RESULT ON A TIE** where Rust's `{:.2}` takes the EVEN one —
//!     and the difference is REACHABLE here: `0.125` is exactly representable and is a tie, and a
//!     series scaled into a one-pixel box produces it.

use js_sys::{Array, Object};
use wasm_bindgen::prelude::*;

use crate::js::{js_max, js_round, number_text, prop, put, to_number, type_of};

/// Percentages at or above this are "pegged" for the purpose of a notice. This is the dial's own
/// `crit` band, restated here because this rule is about DURATION: a single reading in the band is a
/// spike, and the band is only interesting when it persists.
const CRIT_PCT: f64 = 90.0;

/// How much of the window must be in the band. Two thirds: a device that is genuinely pegged stays
/// there, while one that is bursting (builds, browser tabs) does not.
const SUSTAINED_FRACTION: f64 = 2.0 / 3.0;

/// `Math.min` over a list — `NaN` if ANY element is, which is the half `f64::min` does not have.
fn js_min(values: &[f64]) -> f64 {
    let mut out = f64::INFINITY;
    for v in values {
        if v.is_nan() {
            return f64::NAN;
        }
        out = out.min(*v);
    }
    out
}

/// `Math.max`, with the same NaN rule.
fn js_max_list(values: &[f64]) -> f64 {
    let mut out = f64::NEG_INFINITY;
    for v in values {
        if v.is_nan() {
            return f64::NAN;
        }
        out = out.max(*v);
    }
    out
}

/// `n.toFixed(2)` — **WITHOUT LINKING RUST'S FLOAT FORMATTER, WHICH IS THE WHOLE POINT.**
///
/// MEASURED, THIS FAMILY, SAME FLAGS: `format!("{:.2}", f64)` cost **+12,013 gz** of wasm — the
/// largest single-family jump this crate has ever taken, for five exports. It is the trade P0
/// recorded for the landing page's `{:.3}` (8,879 gz, 26% of that payload) arriving again, and the
/// fix is the same one: compute the DIGITS here and let `format!` see integers only.
///
/// THE ARITHMETIC IS EXACT, which is also how the tie rule is satisfied without a special case. The
/// value is `m * 2^e` with `m` an integer; `toFixed(2)` is `round-half-up(100 * x)`, and that is:
///
///   * `e >= 0`  — `100 * m * 2^e`, an integer, so there is nothing to round;
///   * `e < 0`   — `floor((200m + 2^-e) / 2^(-e+1))`, which is the definition of round-half-up.
///
/// Every step fits `u128`: `x < 1e21` here (above that the spec switches to `ToString`), so `m` is at
/// most 53 bits and `100 * x` at most ~1e23. A tie is taken by the LARGER result, which is what the
/// `+ 2^-e` does — and the sign is taken off first, so `(-0.125).toFixed(2)` is `"-0.13"`.
///
/// FOUR ENGINE RULES, each found by the differential rather than by reading:
///   * `NaN`, `Infinity` and `-Infinity` are their own STRINGS, not Rust's `"NaN"`/`"inf"`.
///   * **AT OR ABOVE `1e21` THE SPEC SWITCHES TO `ToString`**, so `(1e21).toFixed(2)` is `"1e+21"`
///     and not twenty-two digits. `number_text` is the engine's own conversion.
///   * `-0` PRINTS WITHOUT A SIGN (`"-0.00"` is not a thing): the spec's sign test is `x < 0`, and
///     `-0 < 0` is false.
///   * ON A TIE IT TAKES THE LARGER MAGNITUDE, where Rust's `{:.2}` takes the even one — and that is
///     REACHABLE here, because `0.125` is exactly representable and a series scaled into a one-pixel
///     box produces it.
fn to_fixed_2(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if n.abs() >= 1e21 {
        return number_text(n);
    }
    let sign = if n < 0.0 { "-" } else { "" };
    let x = n.abs();
    // `x = m * 2^e`, exactly: the mantissa with its implicit bit, and the exponent the bias removed.
    let bits = x.to_bits();
    let raw_exp = ((bits >> 52) & 0x7ff) as i32;
    let mantissa = bits & 0xf_ffff_ffff_ffff;
    let (m, e) = if raw_exp == 0 {
        (mantissa as u128, -1074)
    } else {
        ((mantissa | (1u64 << 52)) as u128, raw_exp - 1075)
    };
    let cents: u128 = if e >= 0 {
        (100 * m) << e
    } else {
        let d = (-e) as u32;
        // Below 2^-64 the answer is zero — `200m < 2^64 <= 2^d` — and shifting by that much is not
        // something a `u128` can do at all.
        if d > 64 {
            0
        } else {
            (200 * m + (1u128 << d)) >> (d + 1)
        }
    };
    format!("{sign}{}.{:02}", cents / 100, cents % 100)
}

/// `seriesValues(samples)` — `typeof v === "number"`, so a NaN IS KEPT (see the module header).
fn series_values(values: &[JsValue]) -> Vec<f64> {
    values
        .iter()
        .filter(|v| type_of(v) == "number")
        .filter_map(|v| v.as_f64())
        .collect()
}

/// The elements of an array-valued argument, or the TypeError the JavaScript would raise on a value
/// that has no `filter`/`forEach` — a caller handing this a number is a caller the TypeScript throws
/// at, and a port that quietly answered `[]` would turn a crash into an empty chart.
fn array_arg(v: &JsValue, what: &str) -> Result<Vec<JsValue>, JsValue> {
    if !v.is_array() {
        return Err(JsValue::from_str(&format!(
            "{what}: expected an array, which is what the TypeScript's `.filter` requires"
        )));
    }
    Ok(Array::from(v).iter().collect())
}

/// Path data for an SVG polyline, one entry per RUN of known values.
///
/// `values` is oldest-first (the device's order). `width`/`height` are the drawing box; the path is
/// scaled to fill it, with the series' own min/max as the vertical range — a sparkline's job is the
/// shape, and a fixed 0-100 axis would flatten every real series into a straight line. A run of ONE
/// known value draws a dot-sized segment rather than nothing, so a single reading is still visible.
///
/// Returns an EMPTY list when there is nothing to draw (all values absent): an empty chart must be an
/// ABSENT chart, never a flat line at zero.
#[wasm_bindgen]
pub fn spark_segments(values: JsValue, width: JsValue, height: JsValue) -> Result<Array, JsValue> {
    let list = array_arg(&values, "sparkSegments")?;
    let known = series_values(&list);
    let out = Array::new();
    if known.is_empty() {
        return Ok(out);
    }
    let width = to_number(&width);
    let height = to_number(&height);
    let min = js_min(&known);
    let max = js_max_list(&known);
    let span = max - min;
    // `values.length` — the SLOT COUNT, including the absent readings: a gap consumes its slot rather
    // than being closed up, so the time axis stays true.
    let n = list.len();
    let x = |i: usize| -> f64 {
        if n <= 1 {
            width / 2.0
        } else {
            (i as f64 / (n - 1) as f64) * width
        }
    };
    // A flat series draws through the middle rather than along the top or the floor: with min === max
    // every point is the same value, and pinning it to an edge would read as "at the limit".
    let y = |v: f64| -> f64 {
        if span == 0.0 {
            height / 2.0
        } else {
            height - ((v - min) / span) * height
        }
    };

    // Coordinates WITHOUT their command letter: the letter depends on the point's position in its run,
    // which is only known when the run ends.
    let mut run: Vec<String> = Vec::new();
    let flush = |run: &mut Vec<String>| {
        if run.len() == 1 {
            // A lone reading: a zero-length segment, which SVG renders as a dot with a round linecap.
            // Dropping it would hide a real measurement.
            out.push(&JsValue::from_str(&format!("M{} L{}", run[0], run[0])));
        } else if run.len() > 1 {
            let mut path = String::new();
            for (i, p) in run.iter().enumerate() {
                if i > 0 {
                    path.push(' ');
                }
                path.push_str(if i == 0 { "M" } else { "L" });
                path.push_str(p);
            }
            out.push(&JsValue::from_str(&path));
        }
        run.clear();
    };
    for (i, v) in list.iter().enumerate() {
        if type_of(v) == "number" {
            let value = v.as_f64().unwrap_or(f64::NAN);
            run.push(format!(
                "{},{}",
                to_fixed_2(x(i)),
                to_fixed_2(y(value))
            ));
        } else {
            flush(&mut run); // the gap BREAKS the line — see the module header
        }
    }
    flush(&mut run);
    Ok(out)
}

/// min / avg / max over the known readings, or `null` when there are none. Never invents a zero for
/// an empty series.
#[wasm_bindgen]
pub fn series_stats(values: JsValue) -> Result<JsValue, JsValue> {
    let list = array_arg(&values, "seriesStats")?;
    let known = series_values(&list);
    if known.is_empty() {
        return Ok(JsValue::NULL);
    }
    let sum: f64 = known.iter().sum();
    let o = Object::new();
    let _ = put(&o, "min", &JsValue::from_f64(js_min(&known)));
    let _ = put(&o, "avg", &JsValue::from_f64(sum / known.len() as f64));
    let _ = put(&o, "max", &JsValue::from_f64(js_max_list(&known)));
    let _ = put(&o, "n", &JsValue::from_f64(known.len() as f64));
    Ok(o.into())
}

/// Does the recent series say something worth interrupting an operator for?
///
/// `samples` is the device's series, oldest first; `nowMs` the caller's clock. The rule reads only
/// the last `window_ms`, needs `min_samples` of evidence, and fires only when at least two thirds of
/// those readings sit in the dial's `crit` band.
///
/// SILENCE IS THE DEFAULT, and it is the honest one: a device this panel cannot see a series for (a
/// host that reports no vitals, an agent that just started) produces no notice, because "I have not
/// looked" and "nothing is wrong" are different facts and a chip that conflates them is worse than no
/// chip.
#[wasm_bindgen]
pub fn load_notice(
    samples: JsValue,
    now_ms: JsValue,
    window_ms: JsValue,
    min_samples: JsValue,
) -> Result<JsValue, JsValue> {
    let list = array_arg(&samples, "loadNotice")?;
    let now = to_number(&now_ms);
    let window = to_number(&window_ms);
    let floor = to_number(&min_samples);

    // `nowMs - s.tsMs <= LOAD_WINDOW_MS` — a COERCING comparison, so a sample with no stamp at all is
    // `NaN` and falls out of the window rather than throwing.
    let recent: Vec<JsValue> = list
        .into_iter()
        .filter(|s| now - to_number(&prop(s, "tsMs")) <= window)
        .collect();
    if (recent.len() as f64) < floor {
        return Ok(JsValue::NULL);
    }

    // The window's OWN span, read off its first and last stamp: the device's cadence is part of its
    // contract (`interval_secs`), but a notice that spells a duration must spell the one the readings
    // actually cover. Never less than a minute — "pegged 0m" is not a fact.
    let first = to_number(&prop(&recent[0], "tsMs"));
    let last = to_number(&prop(&recent[recent.len() - 1], "tsMs"));
    let span_minutes = js_max(1.0, js_round((last - first) / 60_000.0));

    // CPU first: when both are pegged, the CPU is the one an operator can usually act on.
    for metric in ["cpu", "mem"] {
        let values: Vec<f64> = recent
            .iter()
            .map(|s| prop(s, metric))
            .filter(|v| type_of(v) == "number")
            .filter_map(|v| v.as_f64())
            .collect();
        // Not enough of THIS series to judge: the other one may still speak.
        if (values.len() as f64) < floor {
            continue;
        }
        let high = values.iter().filter(|v| **v >= CRIT_PCT).count();
        if (high as f64) / (values.len() as f64) < SUSTAINED_FRACTION {
            continue;
        }
        let label = if metric == "cpu" { "CPU" } else { "memory" };
        let o = Object::new();
        let _ = put(&o, "tone", &JsValue::from_str("crit"));
        let _ = put(&o, "metric", &JsValue::from_str(metric));
        let _ = put(
            &o,
            "text",
            &JsValue::from_str(&format!("{label} pegged {}m", number_text(span_minutes))),
        );
        let _ = put(
            &o,
            "title",
            &JsValue::from_str(&format!(
                "{label} has been at or above {}% in {} of the last {} readings ({} minutes) — the \
                 device is under sustained load, which is what makes everything on it feel slow.",
                number_text(CRIT_PCT),
                number_text(high as f64),
                number_text(values.len() as f64),
                number_text(span_minutes),
            )),
        );
        return Ok(o.into());
    }
    Ok(JsValue::NULL)
}
