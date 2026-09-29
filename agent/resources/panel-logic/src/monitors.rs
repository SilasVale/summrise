//! `hooks/useMonitors.ts`'s parse pair — `/api/monitors`'s body, and the `monitor-change` frame the
//! device PUSHES — transliterated.
//!
//! The third family to move (P2), and the first whose call site is an EVENT HANDLER rather than a
//! data fold: `parseMonitorChange` runs inside a `summrise-monitor-change` listener (`useMonitors`'s
//! own alert strip and `useAttention`'s OS notification), which is a boundary the migration had not
//! reached. It is free for the same reason a fold is — nothing in an event handler is on the first
//! render's path — and it is worth naming because it widens what "already asynchronous" covers:
//! **a call site is free if it is not during render**, and an SSE frame is not.
//!
//! WHAT STAYS IN TYPESCRIPT: `fmtSince`, `unstableTargets` and `downTargets` are called during
//! RENDER (`MonitorAlerts`, `MonitorsCard`, `MonitorChip`), and their boundary has not been reshaped.
//! **THE REASON THEY WERE STUCK IS GONE (2026-09-29): the module is preloaded and awaited before the
//! first render, so a render-path call is legal now — see `panel-react/src/wasm/panelLogic.ts` and
//! `session_labels.rs`, the first family to make one.** `downTargets` still cannot be precomputed at
//! the data boundary, because its second argument is a ticking clock.
//!
//! THE ONE PLACE THIS PORT COULD HAVE LIED, and it is the brief's own trap one step to the side.
//! `parseMonitorChange` builds `key` as `` `${id}:${atMs}` ``, which is a JS NUMBER → TEXT
//! conversion, and Rust's `format!` is a different function: `format!("{}", 1e21_f64)` is
//! `1000000000000000000000` where JS says `1e+21`, and `-0` is `-0` where JS says `0`. A key
//! spelled differently is a DIFFERENT key, so a repeat of one transition would stack in the strip
//! instead of replacing itself — the exact behaviour `key` exists to prevent. So the conversion is
//! the ENGINE's (`Number.prototype.toString`, through `js_sys`), never Rust's formatter: the
//! 8,879-gz `{:.3}` lesson (P0's baseline) and the correctness trap turn out to have the same fix.

use js_sys::{Array, Number, Object, Reflect};
use crate::js::{opt_num, prop, type_of};
use wasm_bindgen::prelude::*;

/// `str(v)` — a string, or the EMPTY string. NOT `boot.rs`'s `non_empty`, and the difference is a
/// real one this file's own tests pinned: `parseMonitorChange({ev, id, at_ms})` answers
/// `host: ""` here and would answer `null` there, and the two helpers live one import apart.
/// Neither trims — `" "` is a usable host to this hook, exactly as it is to the TypeScript.
fn str_of(v: &JsValue) -> String {
    if type_of(v) != "string" {
        return String::new();
    }
    v.as_string().unwrap_or_default()
}

/// `str(v) || null` — the collapse `path` and `expect` are built from: a string that is not empty,
/// and `null` for everything else (a non-string, or `""`).
fn non_empty_str(v: &JsValue) -> Option<String> {
    if type_of(v) != "string" {
        return None;
    }
    let s = v.as_string()?;
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// `num(v)` — a finite JS number. `NaN` and `±Infinity` are absence, as everywhere in this crate.
fn num(v: &JsValue) -> Option<f64> {
    if type_of(v) != "number" {
        return None;
    }
    let n = v.as_f64()?;
    if n.is_finite() {
        Some(n)
    } else {
        None
    }
}

/// `v === true` — STRICT. A `Boolean` object, `1` and `"true"` are all FALSE here, which is what the
/// TypeScript's `=== true` says; a port that reached for JS truthiness would accept all three.
fn is_true(v: &JsValue) -> bool {
    type_of(v) == "boolean" && v.as_bool().unwrap_or(false)
}

/// An array-valued property as a list, or nothing at all when it is not an array. `Array.isArray`
/// is the test, not truthiness: `{targets: "nope"}` is an EMPTY list and not a one-row list.
fn rows(v: &JsValue, key: &str) -> Vec<JsValue> {
    let a = prop(v, key);
    if a.is_array() {
        Array::from(&a).iter().collect()
    } else {
        Vec::new()
    }
}

fn put(obj: &Object, key: &str, value: &JsValue) -> Result<(), JsValue> {
    Reflect::set(obj.as_ref(), &JsValue::from_str(key), value).map(|_| ())
}

fn opt_str(s: Option<String>) -> JsValue {
    match s {
        Some(x) => JsValue::from_str(&x),
        None => JsValue::NULL,
    }
}

/// `typeof v === "boolean" ? v : null` — the value ITSELF when it is a boolean, absence otherwise.
fn opt_bool(v: &JsValue) -> JsValue {
    if type_of(v) == "boolean" {
        v.clone()
    } else {
        JsValue::NULL
    }
}

/// JS `String(n)` — the ENGINE's own number → text, so this crate never links Rust's float
/// formatter. See the header: it is both the cheap answer and the correct one.
fn js_number_text(n: f64) -> String {
    Number::from(n)
        .to_string_with_radix(10)
        .map(String::from)
        .unwrap_or_default()
}

/// `parseMonitors(j)` — `GET /api/monitors`'s body → the monitor list.
///
/// Never throws, and never invents a value: a body this build cannot use is an EMPTY list, a target
/// with no id is DROPPED (a target the panel cannot name is not one it can address), and a probe or
/// a transition with no usable stamp is dropped too — it cannot be placed on the time axis, and the
/// alternative is a chart drawn from guesswork.
#[wasm_bindgen]
pub fn parse_monitors(j: JsValue) -> Result<JsValue, JsValue> {
    let targets = Array::new();
    for raw in rows(&j, "targets") {
        let id = str_of(&prop(&raw, "id"));
        // `if (!id) return []` — the whole row, not just its name.
        if id.is_empty() {
            continue;
        }
        let summary_in = prop(&raw, "summary");

        let series = Array::new();
        for p in rows(&raw, "series") {
            let Some(ts_ms) = num(&prop(&p, "ts_ms")) else {
                continue;
            };
            let probe = Object::new();
            put(&probe, "tsMs", &JsValue::from_f64(ts_ms))?;
            put(&probe, "ok", &JsValue::from_bool(is_true(&prop(&p, "ok"))))?;
            put(&probe, "ms", &opt_num(num(&prop(&p, "ms"))))?;
            series.push(&probe);
        }

        let transitions = Array::new();
        for t in rows(&raw, "transitions") {
            let Some(at_ms) = num(&prop(&t, "at_ms")) else {
                continue;
            };
            let transition = Object::new();
            put(&transition, "atMs", &JsValue::from_f64(at_ms))?;
            put(&transition, "up", &JsValue::from_bool(is_true(&prop(&t, "up"))))?;
            // `?? 0` — a transition whose length the device did not record is 0, not null: the card
            // renders it as "0s", which is what the row says.
            put(
                &transition,
                "lastedMs",
                &JsValue::from_f64(num(&prop(&t, "lasted_ms")).unwrap_or(0.0)),
            )?;
            transitions.push(&transition);
        }

        // `latency: lat ? {…} : null` — TRUTHINESS, not a null test. `latency: 0` (or `""`, or
        // `false`) is "no latency table" to the panel, while `{}` is "a table with nothing in it",
        // and those are different answers: three zeroes versus `null`. `is_truthy` is `JsValue`'s
        // own, so it is JS's rule rather than a Rust reading of it.
        let lat = prop(&summary_in, "latency");
        let latency = if lat.is_truthy() {
            let out = Object::new();
            put(&out, "min", &JsValue::from_f64(num(&prop(&lat, "min")).unwrap_or(0.0)))?;
            put(&out, "avg", &JsValue::from_f64(num(&prop(&lat, "avg")).unwrap_or(0.0)))?;
            put(&out, "max", &JsValue::from_f64(num(&prop(&lat, "max")).unwrap_or(0.0)))?;
            out.into()
        } else {
            JsValue::NULL
        };

        let summary = Object::new();
        // `?? series.length` — the count the panel can actually SEE, rather than zero.
        put(
            &summary,
            "probes",
            &JsValue::from_f64(num(&prop(&summary_in, "probes")).unwrap_or(series.length() as f64)),
        )?;
        put(
            &summary,
            "up",
            &JsValue::from_f64(num(&prop(&summary_in, "up")).unwrap_or(0.0)),
        )?;
        put(
            &summary,
            "down",
            &JsValue::from_f64(num(&prop(&summary_in, "down")).unwrap_or(0.0)),
        )?;
        // Absent rather than 0: "no probes yet" is UNKNOWN, and 0% is a claim about the link.
        put(&summary, "upPct", &opt_num(num(&prop(&summary_in, "up_pct"))))?;
        put(&summary, "upNow", &opt_bool(&prop(&summary_in, "up_now")))?;
        put(&summary, "sinceMs", &opt_num(num(&prop(&summary_in, "since_ms"))))?;
        put(&summary, "drops", &opt_num(num(&prop(&summary_in, "drops"))))?;
        put(&summary, "latency", &latency)?;
        put(
            &summary,
            "lastStatus",
            &opt_num(num(&prop(&summary_in, "last_status"))),
        )?;
        put(
            &summary,
            "lastExpectOk",
            &opt_bool(&prop(&summary_in, "last_expect_ok")),
        )?;

        let target = Object::new();
        put(&target, "id", &JsValue::from_str(&id))?;
        put(&target, "host", &JsValue::from_str(&str_of(&prop(&raw, "host"))))?;
        put(
            &target,
            "port",
            &JsValue::from_f64(num(&prop(&raw, "port")).unwrap_or(0.0)),
        )?;
        // `null` AND `undefined` both mean "no path"; anything else is `str(v) || null`, so a
        // number or a blank string collapses to `null` as well.
        put(&target, "path", &opt_str(non_empty_str(&prop(&raw, "path"))))?;
        put(&target, "expect", &opt_str(non_empty_str(&prop(&raw, "expect"))))?;
        put(&target, "transitions", &transitions)?;
        put(&target, "series", &series)?;
        put(&target, "summary", &summary)?;
        targets.push(&target);
    }
    let out = Object::new();
    put(&out, "targets", &targets)?;
    put(
        &out,
        "intervalSecs",
        &JsValue::from_f64(num(&prop(&j, "interval_secs")).unwrap_or(0.0)),
    )?;
    put(
        &out,
        "seriesMax",
        &JsValue::from_f64(num(&prop(&j, "series_max")).unwrap_or(0.0)),
    )?;
    Ok(out.into())
}

/// `parseMonitorChange(detail)` — one `monitor-change` frame → one alert, or `null`.
///
/// `null` and not a throw: this runs inside an event handler, and a frame this build cannot use must
/// not become an exception in a listener — nor a banner about something that did not happen.
#[wasm_bindgen]
pub fn parse_monitor_change(detail: JsValue) -> Result<JsValue, JsValue> {
    let ev = prop(&detail, "ev");
    // `d.ev !== "monitor-change"` — STRICT, so a `String` object and an absent key are both refused,
    // and one window event cannot be handled by the wrong listener.
    if type_of(&ev) != "string" || ev.as_string().as_deref() != Some("monitor-change") {
        return Ok(JsValue::NULL);
    }
    let id = str_of(&prop(&detail, "id"));
    // A frame with no stamp cannot be KEYED, and an unkeyed alert would show the same outage twice.
    let Some(at_ms) = num(&prop(&detail, "at_ms")) else {
        return Ok(JsValue::NULL);
    };
    if id.is_empty() {
        return Ok(JsValue::NULL);
    }
    let out = Object::new();
    put(
        &out,
        "key",
        &JsValue::from_str(&format!("{}:{}", id, js_number_text(at_ms))),
    )?;
    put(&out, "id", &JsValue::from_str(&id))?;
    put(&out, "host", &JsValue::from_str(&str_of(&prop(&detail, "host"))))?;
    put(
        &out,
        "port",
        &JsValue::from_f64(num(&prop(&detail, "port")).unwrap_or(0.0)),
    )?;
    put(&out, "up", &JsValue::from_bool(is_true(&prop(&detail, "up"))))?;
    put(
        &out,
        "lastedMs",
        &JsValue::from_f64(num(&prop(&detail, "lasted_ms")).unwrap_or(0.0)),
    )?;
    put(&out, "atMs", &JsValue::from_f64(at_ms))?;
    put(&out, "status", &opt_num(num(&prop(&detail, "status"))))?;
    Ok(out.into())
}
