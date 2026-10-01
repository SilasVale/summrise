//! `lib/recipe.ts` — turn a walked path into a RECIPE.
//!
//! This is beat 6 of the design's core loop: "HARVEST — the path becomes a recipe; next run reuses
//! it". The module's own header carries why the DEVICE's memory store is the right place (it is
//! already shared with every AI client on the device, it sanitises credential-shaped values on write,
//! and it needs no new endpoint) and what a recipe is NOT (an executable script — re-running still
//! goes through the AI or the operator).
//!
//! THE INPUT IS ALREADY A RUST SHAPE: `SessionPath`/`PathStep` are produced by `path.rs`'s
//! `derive_path` and `summarize_path`, so this module reads a structure the crate built.
//!
//! # THE TWO STRINGS THAT STAY IN TYPESCRIPT
//!
//! `RECIPE_MARKER` and `RECIPE_TAG` are ARGUMENTS here, not constants: they are the recipe format's
//! VOCABULARY — what a client greps the shared store for — and the panel's own test imports them from
//! `lib/recipe.ts`. It is the same split `liveness.rs` records for `WORKING_MS` and the console's
//! crate records for its translator: the surface owns the words, this module renders them.
//!
//! # THREE ENGINE BEHAVIOURS THE PORT HAS TO KEEP
//!
//!   * **`=== 1` IS STRICT**, so `"1"` does not take the singular: `${s.steps} step${s.steps === 1 ?
//!     "" : "s"}` says "1 steps" for a string, and the differential carries the case.
//!   * **`if (s.counts.ok)` IS TRUTHINESS**, so a count of `0` contributes nothing and a count of
//!     `"0"` contributes `"0 succeeded"` — both are in the corpus.
//!   * **`input.name.trim()` IS A METHOD CALL**: a missing `name` raises (`undefined.trim` is a
//!     TypeError) rather than being coerced into "untitled", and `slice` counts UTF-16 UNITS, which
//!     is why both go through the engine here rather than through `str::trim` and `chars()`.

use js_sys::{Array, Object};
use wasm_bindgen::prelude::*;

use crate::js::{prop, put, type_of};

/// `String(v)` AS A JS STRING — NOT through a Rust `String`, because `slice` can cut a surrogate pair
/// in half and a lone surrogate cannot exist in UTF-8. The engine's coercion keeps it.
fn to_js_string(v: &JsValue) -> JsValue {
    if type_of(v) == "string" {
        return v.clone();
    }
    if v.is_null() {
        return JsValue::from_str("null");
    }
    if v.is_undefined() {
        return JsValue::from_str("undefined");
    }
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("String"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .and_then(|f| f.call1(&JsValue::UNDEFINED, v).ok())
        .unwrap_or_else(|| JsValue::from_str(""))
}

/// `a + b` THROUGH THE ENGINE — `String.prototype.concat`, so a lone surrogate survives the join.
/// `runs.rs` names the same boundary for a React list key (`slice_units`, rounded down to a whole
/// character); here the string is RENDERED, so the exact answer is worth one engine call.
fn js_add(a: &JsValue, b: &JsValue) -> JsValue {
    let a = to_js_string(a);
    let b = to_js_string(b);
    // THE RECEIVER IS BOXED, and this is the third time this repository has paid for the same rule:
    // `Reflect::get` RAISES on a primitive where JavaScript's property access boxes it. Without the
    // box the concat silently answered its left operand, and the differential reported
    // `"Recipe: "` for every title.
    let receiver = boxed(&a);
    js_sys::Reflect::get(&receiver, &JsValue::from_str("concat"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .and_then(|f| f.call1(&receiver, &b).ok())
        .unwrap_or(a)
}

/// `v.k` WHERE A NULLISH `v` IS A TypeError — which is not what `js::prop` does, and the difference
/// is the whole reason this helper exists: `prop` is the `(j ?? {})` GUARD, right for a parser that
/// answers an empty shape for a body it cannot use, and this module's TypeScript has no such guard.
/// `path.summary.steps` on a path with no summary raises (`Cannot read properties of undefined`), and
/// a port that answered `undefined steps` would write a recipe the JavaScript refuses to write. A
/// PRIMITIVE is fine: `(5).summary` is `undefined`, because JavaScript boxes it.
fn read(v: &JsValue, key: &str) -> Result<JsValue, JsValue> {
    if v.is_null() || v.is_undefined() {
        return Err(JsValue::from_str(&format!(
            "Cannot read properties of {} (reading '{key}')",
            if v.is_null() { "null" } else { "undefined" }
        )));
    }
    Ok(prop(v, key))
}

/// `v[name]()` — a method called ON THE VALUE, which is what the TypeScript does. A primitive is
/// BOXED first (`Reflect::get` raises on one where JavaScript's property access does not), and a
/// value with no such method raises, which is the behaviour a caller depends on.
fn call_method(v: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let target = boxed(v);
    let f = js_sys::Reflect::get(&target, &JsValue::from_str(name))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .ok_or_else(|| JsValue::from_str(&format!("{name} is not a function")))?;
    let list = Array::new();
    for a in args {
        list.push(a);
    }
    f.apply(&target, &list)
}

/// `Object(v)` — ToObject: identity for an object, a wrapper for a primitive.
fn boxed(v: &JsValue) -> JsValue {
    let t = type_of(v);
    if t == "object" || t == "function" {
        return v.clone();
    }
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("Object"))
        .ok()
        .and_then(|ctor| ctor.dyn_into::<js_sys::Function>().ok())
        .and_then(|f| f.call1(&JsValue::UNDEFINED, v).ok())
        .unwrap_or(JsValue::UNDEFINED)
}

/// `undefined` as a VALUE, for the places the TypeScript reads a missing property and prints it:
/// `${undefined}` is the string `"undefined"`, which is not the same sentence as `${null}`.
fn undefined() -> JsValue {
    JsValue::UNDEFINED
}

/// `v.length` WHERE A NULLISH `v` RAISES — `st.considered.length` on a step with no `considered` is
/// a TypeError in the TypeScript, and `undefined > 0` would quietly skip the line instead.
fn length_of_nullish_raises(v: &JsValue, what: &str) -> Result<f64, JsValue> {
    if v.is_null() || v.is_undefined() {
        return Err(JsValue::from_str(&format!(
            "Cannot read properties of {} (reading 'length' of {what})",
            if v.is_null() { "null" } else { "undefined" }
        )));
    }
    Ok(length_of(v).unwrap_or(0.0))
}

/// `v.length` — a string's UTF-16 length, an array's element count, and `undefined` for anything
/// else, which is what makes `undefined > 48` false rather than a raise.
fn length_of(v: &JsValue) -> Option<f64> {
    if v.is_array() {
        return Some(Array::from(v).length() as f64);
    }
    if type_of(v) == "string" {
        return v.as_string().map(|s| s.encode_utf16().count() as f64);
    }
    None
}

/// `steps[i]` — an array's element, a string's UTF-16 UNIT (JavaScript indexes a string by code
/// unit, so an astral character is two entries), and `undefined` otherwise.
fn index_of(v: &JsValue, i: usize) -> Result<JsValue, JsValue> {
    if v.is_null() || v.is_undefined() {
        return Err(JsValue::from_str(&format!(
            "Cannot read properties of {} (reading '{i}')",
            if v.is_null() { "null" } else { "undefined" }
        )));
    }
    if v.is_array() {
        return Ok(Array::from(v).get(i as u32));
    }
    if type_of(v) == "string" {
        if let Some(s) = v.as_string() {
            let units: Vec<u16> = s.encode_utf16().collect();
            if let Some(u) = units.get(i) {
                return Ok(JsValue::from_str(&String::from_utf16_lossy(&[*u])));
            }
        }
    }
    Ok(undefined())
}

/// Title shown in the save form, derived from the path so the operator usually only has to confirm
/// it. Uses the FIRST command (what the run was about) and the step count.
#[wasm_bindgen]
pub fn suggested_title(path: JsValue) -> Result<JsValue, JsValue> {
    let steps = read(&path, "steps")?;
    // `path.steps[0]` — indexing a NULLISH value raises too, which is why `index_of` answers a
    // `Result` rather than `undefined`.
    let at = index_of(&steps, 0)?;
    let first = if at.is_undefined() {
        undefined()
    } else {
        let cmd = prop(&at, "command");
        if cmd.is_undefined() {
            undefined()
        } else {
            cmd
        }
    };
    // `first ?? "empty path"` — NULLISH, so an empty command stays empty and only an absent one
    // becomes the placeholder.
    let first = if first.is_null() || first.is_undefined() {
        JsValue::from_str("empty path")
    } else {
        first
    };
    // THE TITLE IS BUILT IN JS, not in a Rust `String`: `slice(0, 45)` counts UTF-16 UNITS and can
    // cut a surrogate pair in half, and the differential carries an emoji command exactly long enough
    // to do it (`\ud83d` alone in the TypeScript's answer, which no Rust `String` can hold).
    let short = match length_of(&first) {
        Some(n) if n > 48.0 => js_add(
            &call_method(&first, "slice", &[JsValue::from_f64(0.0), JsValue::from_f64(45.0)])?,
            &JsValue::from_str("…"),
        ),
        _ => to_js_string(&first),
    };
    let count = match length_of(&steps) {
        Some(n) => JsValue::from_str(&crate::js::number_text(n)),
        None => JsValue::from_str("undefined"),
    };
    let title = js_add(
        &js_add(
            &js_add(&js_add(&JsValue::from_str("Recipe: "), &short), &JsValue::from_str(" (")),
            &count,
        ),
        &JsValue::from_str(" steps)"),
    );
    Ok(title)
}

/// Render the recipe body.
///
/// The shape is deliberate: a machine-readable marker line, the outcome summary (so a recipe that
/// half-failed is honest about it rather than presenting itself as a known-good procedure), then the
/// commands one per line in order.
#[wasm_bindgen]
pub fn build_recipe(
    path: JsValue,
    input: JsValue,
    marker: JsValue,
    tag: JsValue,
) -> Result<Object, JsValue> {
    // `input.name.trim() || "untitled"` — the METHOD CALL raises on a missing name, which is what the
    // TypeScript does; the `||` then takes the placeholder for an EMPTY one.
    let raw_name = read(&input, "name")?;
    let trimmed = call_method(&raw_name, "trim", &[])?;
    // `|| "untitled"` — TRUTHINESS on the trimmed string, and it stays a JS string: a name can carry
    // a lone surrogate for the same reason a command can.
    let name_js = if trimmed.is_truthy() {
        to_js_string(&trimmed)
    } else {
        JsValue::from_str("untitled")
    };

    let summary = read(&path, "summary")?;
    let steps_count = read(&summary, "steps")?;
    let counts = read(&summary, "counts")?;

    // ── THE CONTENT IS BUILT IN JS, for the same reason the title is: a command can carry a LONE
    // SURROGATE (JSON does not require well-formed UTF-16), and a Rust `String` cannot hold one — it
    // would be replaced by U+FFFD, which the differential reports as a different recipe. Everything
    // that comes from the wire stays a `JsValue` until the engine joins it.
    let mut outcome = to_js_string(&steps_count);
    outcome = js_add(
        &outcome,
        &JsValue::from_str(
            // `s.steps === 1` is STRICT: `"1"` is not `1`, and the differential carries that case.
            if type_of(&steps_count) == "number" && steps_count.as_f64() == Some(1.0) {
                " step"
            } else {
                " steps"
            },
        ),
    );
    // `if (s.counts.ok)` — TRUTHINESS, so `0` contributes nothing and `"0"` contributes `"0 succeeded"`.
    for (key, suffix) in [
        ("ok", "succeeded"),
        ("fail", "FAILED"),
        ("warn", "interrupted"),
        ("running", "still running"),
        // `bg` IS NOT `running` AND IS NOT A VERDICT. It was absent from this list, so a path whose
        // commands were all handed off to run in the background produced a recipe whose outcome line
        // read complete.
        ("bg", "backgrounded"),
        ("muted", "with no verdict"),
    ] {
        let v = read(&counts, key)?;
        if v.is_truthy() {
            outcome = js_add(
                &js_add(&outcome, &JsValue::from_str(", ")),
                &js_add(&to_js_string(&v), &JsValue::from_str(&format!(" {suffix}"))),
            );
        }
    }

    let session_label = read(&input, "sessionLabel")?;
    let session_kind = read(&input, "sessionKind")?;
    let where_ = if session_label.is_truthy() {
        js_add(
            &JsValue::from_str(" on "),
            &js_add(
                &if session_kind.is_truthy() {
                    js_add(&to_js_string(&session_kind), &JsValue::from_str(" "))
                } else {
                    JsValue::from_str("")
                },
                &to_js_string(&session_label),
            ),
        )
    } else if session_kind.is_truthy() {
        js_add(
            &JsValue::from_str(" on a "),
            &js_add(&to_js_string(&session_kind), &JsValue::from_str(" session")),
        )
    } else {
        JsValue::from_str("")
    };

    let mut lines: Vec<JsValue> = vec![
        to_js_string(&marker),
        js_add(&JsValue::from_str("# "), &name_js),
        js_add(
            &js_add(
                &js_add(&JsValue::from_str("# Walked"), &where_),
                &JsValue::from_str(". Outcome: "),
            ),
            &js_add(&outcome, &JsValue::from_str(".")),
        ),
    ];
    let fail = read(&counts, "fail")?;
    let warn = read(&counts, "warn")?;
    if fail.as_f64().unwrap_or(f64::NAN) > 0.0 || warn.as_f64().unwrap_or(f64::NAN) > 0.0 {
        lines.push(JsValue::from_str(
            "# NOTE: this run did not complete cleanly — review the marked steps before reusing it.",
        ));
    }
    let goal = read(&input, "goal")?;
    if goal.is_truthy() {
        lines.push(js_add(
            &JsValue::from_str("# Goal: "),
            &to_js_string(&goal),
        ));
    }
    lines.push(JsValue::from_str("#"));
    lines.push(JsValue::from_str("# Commands, in order:"));

    let steps = read(&path, "steps")?;
    if !steps.is_array() {
        // `path.steps.forEach` on a non-array is a TypeError in the TypeScript.
        return Err(JsValue::from_str(
            "path.steps is not an array: the TypeScript's forEach raises",
        ));
    }
    for (i, st) in Array::from(&steps).iter().enumerate() {
        lines.push(js_add(
            &JsValue::from_str(&format!("{}. ", i + 1)),
            &to_js_string(&read(&st, "command")?),
        ));
        // The reasoning is carried as an indented comment. It is the part a reader cannot
        // reconstruct: why THIS command, and what else was on the table.
        let intent = read(&st, "intent")?;
        if intent.is_truthy() {
            lines.push(js_add(
                &JsValue::from_str("   #    why: "),
                &to_js_string(&intent),
            ));
        }
        let considered = read(&st, "considered")?;
        if length_of_nullish_raises(&considered, "considered")? > 0.0 {
            // `st.considered.join(" | ")` — A METHOD CALL, so a `considered` that is a non-empty
            // STRING (it has a length and no `join`) raises exactly as the TypeScript does. The
            // differential found this: the first version answered an empty join where the JavaScript
            // answered a TypeError.
            let joined = call_method(&considered, "join", &[JsValue::from_str(" | ")])?;
            lines.push(js_add(
                &JsValue::from_str("   #    instead of: "),
                &to_js_string(&joined),
            ));
        }
    }
    // `lines.join("\n")`, through the engine so nothing is lost on the way out.
    let mut content = JsValue::from_str("");
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            content = js_add(&content, &JsValue::from_str("\n"));
        }
        content = js_add(&content, line);
    }

    let o = Object::new();
    // `name.startsWith("Recipe:")` — a prefix test on a JS string, so the comparison never round-trips
    // through UTF-8.
    let starts = js_sys::Reflect::get(&boxed(&name_js), &JsValue::from_str("startsWith"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .and_then(|f| f.call1(&boxed(&name_js), &JsValue::from_str("Recipe:")).ok())
        .map(|v| v.is_truthy())
        .unwrap_or(false);
    let title = if starts {
        name_js.clone()
    } else {
        js_add(&JsValue::from_str("Recipe: "), &name_js)
    };
    let _ = put(&o, "title", &title);
    let _ = put(&o, "content", &content);
    let tags = Array::new();
    tags.push(&tag);
    let _ = put(&o, "tags", &tags);
    Ok(o)
}

/// Steps that make a recipe questionable — surfaced in the save form so the operator is not silently
/// saving a broken procedure as a good one.
#[wasm_bindgen]
pub fn recipe_warnings(steps: JsValue) -> Result<Array, JsValue> {
    if !steps.is_array() {
        return Err(JsValue::from_str(
            "recipeWarnings expects an array of steps: the TypeScript's filter raises",
        ));
    }
    let list = Array::from(&steps);
    let count = |state: &str| -> usize {
        list.iter()
            .filter(|s| prop(s, "state").as_string().as_deref() == Some(state))
            .count()
    };
    let out = Array::new();
    let failed = count("fail");
    let cut = count("warn");
    let live = count("running");
    if failed > 0 {
        out.push(&JsValue::from_str(&format!(
            "{failed} step{} failed",
            if failed == 1 { "" } else { "s" }
        )));
    }
    if cut > 0 {
        out.push(&JsValue::from_str(&format!(
            "{cut} step{} was interrupted",
            if cut == 1 { "" } else { "s" }
        )));
    }
    if live > 0 {
        out.push(&JsValue::from_str("the run has not finished"));
    }
    Ok(out)
}
