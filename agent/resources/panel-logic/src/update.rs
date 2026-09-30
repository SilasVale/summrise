//! `lib/updateDiagnosis.ts` + `components/UpdateCard.tsx`'s readers — the panel's answer to "did the
//! update take, and how old is what I am looking at?"
//!
//! ONE FAMILY, TWO FILES, and the reason they move together is that they are the two ends of the same
//! question. `diagnoseUpdate` reads `summrise-update.log` AFTER the fact — the four-way table over
//! whether a receipt and a swap start are present — while `parseUpdateStatus`/`parseAttempt`/
//! `checkedAge`/`attemptAge` read the device's LIVE answer and put an age on it. The console's half of
//! this story (`deviceUpdate.ts`'s update control) moved in the previous commit; this is the panel's.
//!
//! # The three engine behaviours that are NOT Rust's, and each one is a divergence if guessed
//!
//!   * **`Math.round` ROUNDS HALF TOWARD +INFINITY**; `f64::round` rounds half AWAY FROM ZERO. They
//!     agree on every positive number and disagree on every negative half — and a `checkedAt` in the
//!     future is exactly that case.
//!   * **`Math.max(0, NaN)` IS `NaN`**; `f64::max` returns the OTHER operand when one is NaN, so the
//!     Rust spelling of `Math.max(0, x)` answers `0` where the JavaScript answers `NaN` — which the
//!     formatter then renders as "checked NaNh ago" rather than "checked 0s ago".
//!   * **`log.trim()` and `log.split(...)` ARE METHOD CALLS**, not coercions: a number raises
//!     `log.trim is not a function` in the TypeScript, and a port that reached for `String(v)` would
//!     answer `no-log` where the JavaScript throws. Both are called THROUGH the value here, so a
//!     duck-typed object behaves as it does in JavaScript and a number raises as it does there.
//!
//! And the two regular expressions, which have no crate dependency to lean on (`panel-logic` carries
//! `wasm-bindgen` and `js-sys` and nothing else, by a measured decision): `/copy ok\s*=\s*(true|false)/i`
//! and `/task restarted/i` are hand-rolled as ASCII-case-insensitive scans with the JavaScript `\s`
//! class, and the differential's corpus carries the cases that separate a hand-rolled matcher from the
//! real one — mixed case, no spaces, tabs, a doubled `=`, a value with a suffix, and `copy ok` twice on
//! one line.

use js_sys::{Object, Reflect};
use wasm_bindgen::prelude::*;

use crate::js::{num, number_text, prop, put, text, to_number, type_of};

// ── `lib/updateDiagnosis.ts` ─────────────────────────────────────────────────────────────────────

/// `Math.round` — half toward +INFINITY, which is not `f64::round`.
fn js_round(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() || x == 0.0 {
        return x;
    }
    (x + 0.5).floor()
}

/// `Math.max` for two numbers — `NaN` if either is, where `f64::max` would answer the other one.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// The JavaScript `\s` class, which is wider than `char::is_whitespace` in both directions: it has
/// `\u{FEFF}` (not whitespace to Rust) and lacks `\u{0085}` (whitespace to Rust).
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r' | '\u{00A0}' | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

/// The index of `needle` in `hay`, ASCII-case-insensitively — what a `/…/i` regex does for the ASCII
/// alphabet, which is the alphabet both of these patterns are written in.
fn find_ci(hay: &[char], needle: &str) -> Option<usize> {
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() {
        return Some(0);
    }
    if hay.len() < n.len() {
        return None;
    }
    (0..=hay.len() - n.len())
        .find(|&i| (0..n.len()).all(|k| hay[i + k].to_ascii_lowercase() == n[k]))
}

/// `/copy ok\s*=\s*(true|false)/i` — searched anywhere in one line, first match wins.
fn match_copy_ok(line: &str) -> Option<bool> {
    let cs: Vec<char> = line.chars().collect();
    let width = "copy ok".chars().count();
    let mut from = 0;
    while from + width <= cs.len() {
        let p = find_ci(&cs[from..], "copy ok")?;
        let mut j = from + p + width;
        while j < cs.len() && is_js_space(cs[j]) {
            j += 1;
        }
        if cs.get(j) != Some(&'=') {
            from += p + 1;
            continue;
        }
        j += 1;
        while j < cs.len() && is_js_space(cs[j]) {
            j += 1;
        }
        let rest: String = cs[j..].iter().collect();
        let lower = rest.to_ascii_lowercase();
        // THE ALTERNATION IS ORDERED AND THE REGEX IS NOT ANCHORED: `true` first, and a value with a
        // suffix (`true-ish`) still matches, because the pattern has no boundary after the group.
        if lower.starts_with("true") {
            return Some(true);
        }
        if lower.starts_with("false") {
            return Some(false);
        }
        from += p + 1;
    }
    None
}

/// `lines.filter((l) => l.includes(needle))` — a CASE-SENSITIVE substring test.
fn lines_with(lines: &[String], needle: &str) -> Vec<String> {
    lines
        .iter()
        .filter(|l| l.contains(needle))
        .cloned()
        .collect()
}

/// `copyOkFrom` — the LAST `copy ok=` line's verdict, or nothing when the script never got there.
fn copy_ok_from(lines: &[String]) -> Option<bool> {
    lines.iter().rev().find_map(|l| match_copy_ok(l))
}

/// `restartedFrom` — `/task restarted/i` searched from the END, answering `true` or nothing.
fn restarted_from(lines: &[String]) -> Option<bool> {
    lines
        .iter()
        .rev()
        .any(|l| find_ci(&l.chars().collect::<Vec<char>>(), "task restarted").is_some())
        .then_some(true)
}

/// The `no-log` verdict, which is a WHOLE OBJECT rather than a branch of the others: a device that has
/// never been updated has nothing to have arrived, and collapsing it into `never-arrived` would tell an
/// operator their update was lost when none was attempted.
fn no_log() -> Object {
    let o = Object::new();
    let _ = put(&o, "verdict", &JsValue::from_str("no-log"));
    let _ = put(
        &o,
        "summary",
        &JsValue::from_str(
            "This device has no update log, so no update has been attempted here (or the log was removed).",
        ),
    );
    let _ = put(&o, "receipt", &JsValue::NULL);
    let _ = put(&o, "copyOk", &JsValue::NULL);
    let _ = put(&o, "restarted", &JsValue::NULL);
    o
}

/// The shared result shape — `{verdict, summary, receipt, copyOk, restarted}`, in the TypeScript's key
/// order.
fn diagnosis(verdict: &str, summary: String, receipt: Option<&String>, copy_ok: Option<bool>, restarted: Option<bool>) -> Object {
    let o = Object::new();
    let _ = put(&o, "verdict", &JsValue::from_str(verdict));
    let _ = put(&o, "summary", &JsValue::from_str(&summary));
    let _ = put(
        &o,
        "receipt",
        &match receipt {
            Some(r) => JsValue::from_str(r),
            None => JsValue::NULL,
        },
    );
    let _ = put(&o, "copyOk", &bool_or_null(copy_ok));
    let _ = put(&o, "restarted", &bool_or_null(restarted));
    o
}

fn bool_or_null(v: Option<bool>) -> JsValue {
    match v {
        Some(b) => JsValue::from_bool(b),
        None => JsValue::NULL,
    }
}

/// `diagnoseUpdate(log)` — the four-way verdict over `summrise-update.log`.
///
/// PRESENCE IS DECIDED PER KIND, not by position: a receipt and a start can be interleaved over
/// several updates, and the question is only whether each kind appears at all. Ordering by line index
/// would answer a different question ("was the LAST thing a receipt?") and would report `cli-only` for
/// a device whose most recent update succeeded.
#[wasm_bindgen]
pub fn diagnose_update(log: JsValue) -> Result<Object, JsValue> {
    // `log == null` — the loose test, so `null` and `undefined` are both the absent log.
    if log.is_null() || log.is_undefined() {
        return Ok(no_log());
    }
    // `log.trim() === ""`. THE METHOD IS CALLED ON THE VALUE, so a number raises here exactly as it
    // does in the TypeScript ("log.trim is not a function") rather than being coerced into `no-log`.
    let (trim_fn, target) = call_method(&log, "trim")?;
    let trimmed = trim_fn.call0(&target)?;
    if trimmed.as_string().is_some_and(|s| s.is_empty()) {
        return Ok(no_log());
    }
    // `log.split(/\r?\n/)` — the REGEX, through the value's own method, for the same reason.
    let re = js_sys::RegExp::new("\\r?\\n", "");
    let (split_fn, target) = call_method(&log, "split")?;
    let parts = split_fn.call1(&target, re.as_ref())?;
    let lines: Vec<String> = js_sys::Array::from(&parts).iter().map(|v| text(&v)).collect();

    let receipts = lines_with(&lines, "update requested");
    let starts = lines_with(&lines, "update start");
    let receipt = receipts.last();
    let copy_ok = copy_ok_from(&lines);
    let restarted = restarted_from(&lines);

    if receipt.is_some() && !starts.is_empty() {
        // Say what the SCRIPT reported, not just that it ran: a launched swap that failed to copy
        // leaves the device on the old binary, and "launched" alone would read as success.
        let detail = if copy_ok == Some(false) {
            " The copy did NOT succeed, so the device is still running the previous build."
        } else if restarted == Some(true) {
            " The copy succeeded and the agent was restarted."
        } else {
            " The swap started but the log does not say it finished — check the lines below."
        };
        return Ok(diagnosis(
            "cli-swap-launched",
            format!("The CLI reached this device and the swap launched.{detail}"),
            receipt,
            copy_ok,
            restarted,
        ));
    }
    if let Some(r) = receipt {
        return Ok(diagnosis(
            "cli-only",
            "The CLI reached this device but the swap never launched — nothing was replaced. Re-running the update is safe.".to_string(),
            Some(r),
            copy_ok,
            restarted,
        ));
    }
    if !starts.is_empty() {
        return Ok(diagnosis(
            "rust-swap",
            "A swap was launched by the agent itself (the console/auto channel), which writes no receipt. The lines below are its record.".to_string(),
            None,
            copy_ok,
            restarted,
        ));
    }
    Ok(diagnosis(
        "never-arrived",
        "No update was ever requested through either channel. If you just asked for one, the command did not reach this device — the connection drop is NOT proof it started.".to_string(),
        None,
        copy_ok,
        restarted,
    ))
}

/// `v[name]` as a callable, or the TypeError the JavaScript would raise for a value that has no such
/// method. The message is not the engine's, but the BEHAVIOUR is: a caller that handed this a number
/// gets a rejection rather than a quiet `no-log`.
///
/// **THE VALUE IS BOXED FIRST, WHICH IS WHAT JAVASCRIPT DOES AND WHAT `Reflect::get` DOES NOT.** A
/// property read goes through ToObject, so `"a\nb".trim` finds `String.prototype.trim` — while
/// `Reflect::get` on a primitive target raises. The first version of this function read the method
/// straight off the value, and the differential reported EVERY string log as `trim is not a function`,
/// which is the failure mode this file's header predicts for a guessed engine behaviour.
fn call_method(v: &JsValue, name: &str) -> Result<(js_sys::Function, JsValue), JsValue> {
    let target = boxed(v);
    let f = Reflect::get(&target, &JsValue::from_str(name))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .ok_or_else(|| JsValue::from_str(&format!("{name} is not a function")))?;
    Ok((f, target))
}

/// `Object(v)` — ToObject. Identity for an object or a function, a WRAPPER for a primitive, which is
/// the receiver JavaScript's property access and method call both use.
fn boxed(v: &JsValue) -> JsValue {
    let t = type_of(v);
    if t == "object" || t == "function" {
        return v.clone();
    }
    Reflect::get(&js_sys::global(), &JsValue::from_str("Object"))
        .ok()
        .and_then(|ctor| ctor.dyn_into::<js_sys::Function>().ok())
        .and_then(|f| f.call1(&JsValue::UNDEFINED, v).ok())
        .unwrap_or(JsValue::UNDEFINED)
}

// ── `components/UpdateCard.tsx` — the readers ─────────────────────────────────────────────────────

/// `str(v)` — `typeof v === "string" && v ? v : null`: a NON-EMPTY string, or nothing.
fn str_of(v: &JsValue) -> JsValue {
    match v.as_string() {
        Some(s) if !s.is_empty() => JsValue::from_str(&s),
        _ => JsValue::NULL,
    }
}

/// `b.update_available === true` — STRICT, so the string `"true"` and the number `1` are not a claim
/// that an update exists.
fn is_true(v: &JsValue) -> bool {
    v.as_bool() == Some(true)
}

/// `parseUpdateStatus(j)` — read `/api/update`. Never throws; a body this build cannot use is the
/// empty state, which renders as "unknown" rather than as "current".
#[wasm_bindgen]
pub fn parse_update_status(j: JsValue) -> Object {
    // `(j ?? {})` — the nullish guard, and `prop` tolerates the primitives JavaScript auto-boxes.
    let b = if j.is_null() || j.is_undefined() {
        JsValue::UNDEFINED
    } else {
        j
    };
    let o = Object::new();
    let current = str_of(&prop(&b, "current"));
    let _ = put(
        &o,
        "current",
        &if current.is_null() {
            JsValue::from_str("")
        } else {
            current
        },
    );
    let _ = put(&o, "channel", &str_of(&prop(&b, "channel")));
    let _ = put(&o, "latest", &str_of(&prop(&b, "latest")));
    let _ = put(
        &o,
        "updateAvailable",
        &JsValue::from_bool(is_true(&prop(&b, "update_available"))),
    );
    let _ = put(&o, "pinnedTo", &str_of(&prop(&b, "pinned_to")));
    let _ = put(&o, "busy", &JsValue::from_bool(is_true(&prop(&b, "busy"))));
    let _ = put(&o, "error", &str_of(&prop(&b, "error")));
    // A NUMBER, NOT A STRING, and `0` is not a time anyone can weigh (it reads as 1970): the wire sends
    // epoch ms and a non-positive or non-finite value is null — "absent, never zero".
    let checked = prop(&b, "checked_at");
    let _ = put(
        &o,
        "checkedAt",
        &match num(&checked) {
            Some(n) if n > 0.0 => JsValue::from_f64(n),
            _ => JsValue::NULL,
        },
    );
    let _ = put(&o, "lastAttempt", &parse_attempt(prop(&b, "last_attempt")));
    o
}

/// `parseAttempt(v)` — the launch record, or null. A record without a POSITIVE time, a `from` and a
/// `to` is not one: half a record would render as "updated from to at Invalid Date".
#[wasm_bindgen]
pub fn parse_attempt(v: JsValue) -> JsValue {
    // `!v || typeof v !== "object"` — truthiness FIRST, so `0`, `""`, `false` and `NaN` are all null
    // without a property read; then `typeof`, which is what excludes a string or a number that happens
    // to be truthy. An ARRAY passes both, and reads `at_ms` as absent, exactly as the JavaScript does.
    if !v.is_truthy() || type_of(&v) != "object" {
        return JsValue::NULL;
    }
    let at = prop(&v, "at_ms");
    let Some(at_ms) = num(&at).filter(|n| *n > 0.0) else {
        return JsValue::NULL;
    };
    let from = prop(&v, "from").as_string().unwrap_or_default();
    let to = prop(&v, "to").as_string().unwrap_or_default();
    if from.is_empty() && to.is_empty() {
        return JsValue::NULL;
    }
    let o = Object::new();
    let _ = put(&o, "atMs", &JsValue::from_f64(at_ms));
    let _ = put(&o, "from", &JsValue::from_str(&from));
    let _ = put(&o, "to", &JsValue::from_str(&to));
    o.into()
}

/// `checkedAge(checkedAt, nowMs)` — "checked 12s ago", or null when there is no time to weigh.
///
/// ZERO IS NOT A TIME, and the formatter is the last place before the screen: `epoch 0` renders as
/// "checked 497204h ago", which is a claim about a device that simply has not answered.
#[wasm_bindgen]
pub fn checked_age(checked_at: JsValue, now_ms: JsValue) -> JsValue {
    // `checkedAt === null || !Number.isFinite(checkedAt) || checkedAt <= 0` — note the STRICT `null`
    // test and then `Number.isFinite`, which refuses a numeric STRING as well as NaN and ±Infinity.
    if checked_at.is_null() {
        return JsValue::NULL;
    }
    let Some(at) = num(&checked_at).filter(|n| *n > 0.0) else {
        return JsValue::NULL;
    };
    // `nowMs - checkedAt` COERCES, so `to_number` is the engine's ToNumber rather than a cast.
    let secs = js_max(0.0, js_round((to_number(&now_ms) - at) / 1000.0));
    if secs < 90.0 {
        return JsValue::from_str(&format!("checked {}s ago", number_text(secs)));
    }
    let mins = js_round(secs / 60.0);
    if mins < 90.0 {
        return JsValue::from_str(&format!("checked {}m ago", number_text(mins)));
    }
    JsValue::from_str(&format!("checked {}h ago", number_text(js_round(mins / 60.0))))
}

/// `attemptAge(atMs, nowMs)` — the age of an ACT, where `checkedAge` is the age of a READING. Same
/// units, no verb: the sentence around it already says what happened.
#[wasm_bindgen]
pub fn attempt_age(at_ms: JsValue, now_ms: JsValue) -> String {
    let age = checked_age(at_ms, now_ms);
    match age.as_string() {
        // `age.replace(/^checked /, "")` — ANCHORED, so only a leading occurrence is removed.
        Some(s) => match s.strip_prefix("checked ") {
            Some(rest) => rest.to_string(),
            None => s,
        },
        None => "at an unknown time".to_string(),
    }
}
