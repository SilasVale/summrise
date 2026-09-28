//! `hooks/useVitalsSeries.ts`'s parse — `/api/vitals/history`'s body, transliterated.
//!
//! The fifth family to move (P2), and the SECOND whose call site is a `useDeviceRead` fold
//! (`parseMonitors` was the first). It is free for the reason the plan gives: the wasm is fetched at
//! the first call rather than at page load, so a migrated function may be called anywhere that is
//! already asynchronous — a fold may return a promise, and nothing in it is on the first render's
//! path.
//!
//! WHAT STAYS IN TYPESCRIPT, and it is the same reason as `lib/archive.ts`'s and `lib/monitors`':
//! `EMPTY_SERIES` is a CONSTANT a render reads, and `useVitalsSeries` is the hook itself. Neither
//! computes anything. What moved is the one function in the file that parses.
//!
//! THE TWO RULES THE PORT HAD TO KEEP, both of which are visible in the TypeScript's own comments:
//!
//!   * **A body this build cannot use is an EMPTY series — never a throw.** `parseVitalsSeries(42)`,
//!     `(null)`, `("text")` and `({samples: "nope"})` all answer `{samples: [], intervalSecs: 0,
//!     spanSecs: 0}`. The `(j ?? {})` in the TypeScript is [`prop`]'s guard, and `Array.isArray` is
//!     [`rows`]'s test — a port that reached for truthiness would turn `"nope"` into a one-row list
//!     and `0` into an empty one.
//!   * **A sample with no usable stamp is DROPPED**, not defaulted. It cannot be placed on the time
//!     axis the chip's window and the chart's x-axis both use, and a sample at `tsMs: 0` would draw a
//!     point at the epoch. The three VALUES beside it may be absent — `cpu`/`mem`/`memTotalMb` are
//!     `number | null` and a chart skips a null rather than plotting a zero — which is why the stamp
//!     is the only field that decides whether the row exists.

use crate::js::{num, opt_num, prop, put, rows};
use js_sys::{Array, Object};
use wasm_bindgen::prelude::*;

/// `parseVitalsSeries(j)` — the series, or the empty one.
///
/// THE OUTPUT KEYS ARE THE TYPESCRIPT'S (`tsMs`, `intervalSecs`, `spanSecs`, `memTotalMb`) and not
/// the wire's (`ts_ms`, `interval_secs`, `span_secs`, `mem_total_mb`). The wire names are read; the
/// camelCase names are what the panel's readers destructure, and a port that returned the wire's
/// spelling would type-check against `VitalsSeries` only if the interface were changed too — which
/// is a change to the panel, not a migration of it.
#[wasm_bindgen]
pub fn parse_vitals_series(j: JsValue) -> Result<JsValue, JsValue> {
    let samples = Array::new();
    for raw in rows(&j, "samples") {
        // `if (tsMs === null) return []` — the whole row, before any of its values are read.
        let Some(ts_ms) = num(&prop(&raw, "ts_ms")) else {
            continue;
        };
        let sample = Object::new();
        put(&sample, "tsMs", &JsValue::from_f64(ts_ms))?;
        put(&sample, "cpu", &opt_num(num(&prop(&raw, "cpu_pct"))))?;
        put(&sample, "mem", &opt_num(num(&prop(&raw, "mem_pct"))))?;
        put(&sample, "memTotalMb", &opt_num(num(&prop(&raw, "mem_total_mb"))))?;
        samples.push(&sample);
    }

    let out = Object::new();
    put(&out, "samples", &samples)?;
    // `?? 0` on both: a reply that did not state a cadence or a span is 0, which the chip renders as
    // "0s" — the honest sentence, because the device really did not say.
    put(
        &out,
        "intervalSecs",
        &JsValue::from_f64(num(&prop(&j, "interval_secs")).unwrap_or(0.0)),
    )?;
    put(
        &out,
        "spanSecs",
        &JsValue::from_f64(num(&prop(&j, "span_secs")).unwrap_or(0.0)),
    )?;
    Ok(out.into())
}
