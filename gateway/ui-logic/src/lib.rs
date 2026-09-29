//! The CONSOLE's logic, in Rust — block ③ of the migration, and the crate the plan's P0 asked for.
//!
//! WHY THIS EXISTS. `gateway/ui` and `agent/resources/panel-react` are one product with two hand-rolled
//! front ends, and the migration's goal is one language for the product. The panel's logic has been
//! moving into `agent/resources/panel-logic` since P2 began; the console had NO wasm crate at all until
//! this one, which is why the plan lists block ③ as its own piece of work rather than a tail of P2.
//!
//! THE BOUNDARY IS THE SAME ONE, and it is the P0 inventory's: `gateway/ui/src/lib/*.ts` holds the
//! console's LOGIC (31 exports across 8 files) and the `.tsx` files hold its RENDERING. React, Radix and
//! Tailwind stay exactly where they are; what moves is the part that computes rather than draws.
//!
//! ── THE FIRST MODULE IS `lane.ts`, AND IT IS THE SMALLEST HONEST ONE ─────────────────────────────
//!
//! `bare_prefix` and `lane_class` are a channel prefix's lane colour, written as a TABLE rather than a
//! ladder (round 140). They were eight `if (p === "…") return "lane-…";` lines in `Models.tsx` under a
//! docstring claiming it was "the same mapping the Routes page uses" — and there is NO second copy in
//! that console, so the sentence described a consumer that does not exist. **A mapping is DATA, and
//! written as control flow it hides two things: which prefixes have a lane at all, and that anything
//! else falls silently to `lane-def`.**
//!
//! `bare_prefix` is the same story one layer down: the rule was written out at eight places and with TWO
//! different regexes (`/\/$/` strips one trailing slash, `/\/+$/` strips all), so two of the eight already
//! disagreed about what a prefix IS. This is the wider of the two behaviours and the one a name wants.
//!
//! WHAT IT DELIBERATELY DOES NOT DO: decide which prefixes exist. That is the gateway's payload (`or/`
//! and the rest arrive from `/api/admin/public`), so an unknown prefix lands on `lane-def` on purpose —
//! the table just makes that visible.
//!
//! ── AND IT IS WIRED NOW (2026-09-29), WHICH IS WHAT THE FIRST COMMIT SAID IT WAS NOT ─────────────
//!
//! The crate landed with these two functions and nothing calling them, because `laneClass` is called
//! DURING RENDER (`Models.tsx`) and the module was fetched at the console's first migrated call — a
//! synchronous call from a component could not wait for it. `ui/src/wasm/consoleLogic.ts` is the seam
//! that removed that: `index.html` fetches and compiles this module while the bundle is still
//! downloading, and `main.tsx` awaits it before the first render. `lib/lane.ts` is a two-line wrapper
//! now, and `test/lane.test.mjs` pins the table, the wider trailing-slash rule and the coercion.
//!
//! **AND THE TEST FOUND A BUG THE COMMENT ABOVE HAD ALREADY CLAIMED WAS ABSENT.** `bare_prefix` used
//! `JsString::from(prefix)`, an UPCAST rather than a coercion: `barePrefix(5)` answered `""` where the
//! TypeScript's `String(5)` is `"5"`. The docstring said "the coercion is the ENGINE's ToString" and
//! the body did not do it — which is the shape this repository keeps paying for: a sentence that
//! describes the intent, next to code that does something else.

use wasm_bindgen::prelude::*;

/// The lane classes, as data. **THE TABLE IS THE POINT** — see the module docstring.
const LANE_CLASSES: [(&str, &str); 8] = [
    ("og", "lane-og"),
    ("ds", "lane-ds"),
    ("or", "lane-or"),
    ("qw", "lane-qw"),
    ("nv", "lane-nv"),
    ("gmi", "lane-gmi"),
    ("cm", "lane-cm"),
    ("amd", "lane-amd"),
];

/// THE BARE PREFIX (`or/` and `or` are one channel).
///
/// `String(prefix ?? "")` — the JavaScript coerces ANY value (a number, `null`, `undefined`) rather
/// than only accepting a string, so the input stays a `JsValue` and the coercion is the ENGINE's
/// `ToString`, not Rust's formatter. That is this migration's standing rule: ask the engine for a
/// value's text, never `format!` — a `key` spelled `1e+21` on one side and
/// `1000000000000000000000` on the other is a different key.
///
/// **AND THIS FUNCTION CLAIMED THAT RULE WHILE BREAKING IT (found 2026-09-29, by the test that wired
/// it).** The body was `JsString::from(prefix)`, which is an UPCAST and not a coercion: it reinterprets
/// the value as a string without converting it, so `as_string()` answers `None` for a number, a `null`
/// and an `undefined`, and every one of them became `""`. The TypeScript's `String(5)` is `"5"`. The
/// console's `test/lane.test.mjs` pins exactly that case, which is why it was found the moment the
/// function was reachable rather than the day a caller sent a number.
///
/// The `?? ""` half is `is_null() || is_undefined()`: `String(null)` is `"null"`, and the TypeScript
/// asked for the empty string.
#[wasm_bindgen]
pub fn bare_prefix(prefix: JsValue) -> String {
    let s = if prefix.is_null() || prefix.is_undefined() {
        String::new()
    } else {
        js_text(&prefix)
    };
    // `/\/+$/` — ALL trailing slashes, which is the wider of the two behaviours the console had.
    s.trim_end_matches('/').to_string()
}

/// JS `String(v)` — the ENGINE's coercion, which is what a template literal's interpolation performs.
fn js_text(v: &JsValue) -> String {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("String"))
        .ok()
        .and_then(|ctor| ctor.dyn_into::<js_sys::Function>().ok())
        .and_then(|f| f.call1(&JsValue::UNDEFINED, v).ok())
        .and_then(|s| s.as_string())
        .unwrap_or_default()
}

/// A property read that ANSWERS `undefined` for a nullish target — the console's optional-chaining
/// shape, and the one rule every function below needs: a null or undefined OBJECT has no properties,
/// which is why `deviceTally` can read `statuses[name]` and then `st.agent_up` without a guard at
/// each step.
fn prop(target: &JsValue, key: &str) -> JsValue {
    if target.is_null() || target.is_undefined() {
        return JsValue::UNDEFINED;
    }
    js_sys::Reflect::get(target, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

/// The lane class for a channel prefix, with its trailing slash ignored.
#[wasm_bindgen]
pub fn lane_class(prefix: JsValue) -> String {
    let bare = bare_prefix(prefix);
    LANE_CLASSES
        .iter()
        .find(|(k, _)| *k == bare)
        .map(|(_, v)| (*v).to_string())
        .unwrap_or_else(|| "lane-def".to_string())
}

// ── `lib/channelState.ts` — IS THIS PROVIDER CHANNEL HEALTHY? ──────────────────────────────────────
//
// The second console module, and the first one that CALLS BACK INTO JAVASCRIPT: `channelLabel` takes
// the console's own translator `t`, because the word for a state belongs to the console's dictionary
// and not to a table in a crate. The port keeps that seam where the TypeScript put it — Rust decides
// WHICH of the three answers applies, and the dictionary resolves it — by passing `t` as a
// `js_sys::Function` rather than moving a dictionary into wasm that no operator can read.
//
// WHY IT COULD MOVE AT ALL: all three are called DURING RENDER (`Overview.tsx`), and the module is
// fetched-and-compiled before the first render by the seam this crate's first commit did not have.
// See `ui/src/wasm/consoleLogic.ts`.

/// The signal for a channel: `ok` is the provider's own answer, anything else is a failure with a
/// reason the row shows.
///
/// **TRUTHINESS, NOT `=== true`** — the TypeScript is `ok ? "ok" : "err"`, so a `1` is `ok` there and
/// `false` here would be a divergence on the same input.
#[wasm_bindgen]
pub fn channel_signal(ok: JsValue) -> String {
    if ok.is_truthy() { "ok".to_string() } else { "err".to_string() }
}

/// The words for that signal, given the provider's reason when it has one.
///
/// `c.ok ? t("overview.healthOk") : c.reason || t("overview.healthDown")` — the `reason` wins over the
/// generic line, because a provider that says WHY is more useful than a label that says WHAT. The
/// `||` is a TRUTHINESS test on the reason as well: an empty string is no reason, and the TypeScript
/// falls through to the generic line for it.
///
/// **AND IT RETURNS THE REASON AS IT ARRIVED, WHICH IS NOT A STRING AND IS THE TYPESCRIPT'S OWN
/// BEHAVIOUR.** `c.reason || t(…)` returns the VALUE: the first version of this port stringified it
/// through the engine and answered `"[object Object]"` where the JavaScript answers `{}`. The return
/// type has always said `string` while the body could hand back anything truthy, and the differential
/// is what turned that into a fact — so the port is faithful and the cast stays in the wrapper.
#[wasm_bindgen]
pub fn channel_label(channel: JsValue, t: &js_sys::Function) -> Result<JsValue, JsValue> {
    if prop_truthy(&channel, "ok") {
        return call_translator(t, "overview.healthOk");
    }
    let reason = js_sys::Reflect::get(&channel, &JsValue::from_str("reason")).unwrap_or(JsValue::UNDEFINED);
    if reason.is_truthy() {
        return Ok(reason);
    }
    call_translator(t, "overview.healthDown")
}

/// A DIAL'S TONE, from "how many of N are well" — the same question the channels tile and the devices
/// tile both ask, and they used to answer it differently.
///
/// `known` is separate from `total` because "we have not asked yet" is not "none are healthy" — the
/// same distinction `deviceState.ts` records for a probe has not answered.
///
/// **BOTH COUNTS ARRIVE AS VALUES, NOT NUMBERS, AND THAT IS THE SECOND DIVERGENCE THE DIFFERENTIAL
/// FOUND.** `ok === total` is JavaScript's STRICT equality and `ok > 0` is a RELATIONAL comparison,
/// and they disagree on everything that is not a number: for `ok = true, total = 1` the first is
/// `false` and the second is `true`, so the TypeScript answers `warn` — and a port that took two
/// `f64` parameters turned that into `1 === 1` and answered `ok`. Reading the two through the
/// operators the TypeScript actually wrote is the whole fix; see `strictly_equals` and `to_number`.
#[wasm_bindgen]
pub fn health_tone(known: JsValue, ok: JsValue, total: JsValue) -> String {
    if !known.is_truthy() || strictly_equals(&total, &JsValue::from_f64(0.0)) {
        return "off".to_string();
    }
    if strictly_equals(&ok, &total) {
        "ok".to_string()
    } else if to_number(&ok) > 0.0 {
        "warn".to_string()
    } else {
        "off".to_string()
    }
}

/// JavaScript's `a === b` for the two shapes this module compares — a NUMBER against a number.
///
/// It is strict in the way that matters: two non-numbers are never equal to each other (so `true === 1`
/// is false, and `"1" === 1` is false), and `NaN === NaN` is false because `f64` says so.
fn strictly_equals(a: &JsValue, b: &JsValue) -> bool {
    matches!((a.as_f64(), b.as_f64()), (Some(x), Some(y))
        if a.js_typeof().as_string().as_deref() == Some("number")
            && b.js_typeof().as_string().as_deref() == Some("number")
            && x == y)
}

/// JavaScript's `Number(v)` — the engine's own coercion, which is what a relational operator applies.
/// `js_sys::Number::from(n)` would only do it for a value that is already a number.
fn to_number(v: &JsValue) -> f64 {
    match v.js_typeof().as_string().as_deref() {
        Some("number") => v.as_f64().unwrap_or(f64::NAN),
        _ => js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("Number"))
            .ok()
            .and_then(|ctor| ctor.dyn_into::<js_sys::Function>().ok())
            .and_then(|f| f.call1(&JsValue::UNDEFINED, v).ok())
            .and_then(|n| n.as_f64())
            .unwrap_or(f64::NAN),
    }
}

/// A property read, in the shape `channelLabel` uses it: a missing property is `undefined`, and a
/// non-object target is what the TypeScript's `(c ?? {})` guard answers for — except that this one
/// does NOT guard, because `channelLabel` is called with the record itself and the TypeScript raises
/// on `null.reason`; `Reflect::get` on a primitive throws the same way, so the two agree.
fn prop_truthy(v: &JsValue, key: &str) -> bool {
    js_sys::Reflect::get(v, &JsValue::from_str(key))
        .map(|value| value.is_truthy())
        .unwrap_or(false)
}

/// `t(key)` — the console's own dictionary call, and its result is whatever the dictionary returns,
/// which is a string. A translator that throws propagates, exactly as the TypeScript's would.
fn call_translator(t: &js_sys::Function, key: &str) -> Result<JsValue, JsValue> {
    t.call1(&JsValue::UNDEFINED, &JsValue::from_str(key))
}

// ── `lib/deviceState.ts` — IS THIS DEVICE'S AGENT ANSWERING? ──────────────────────────────────────
//
// The console's second grouping module, and the one that fixes the truth/answering the two device
// facts. Like `channelState`, `agentSignal`/`tunnelSignal` take the console's `t` and the crate decides
// WHICH of the three states applies — the words stay in the console's dictionary.
//
// THE TRI-STATE IS THE POINT. An ABSENT status is `off` ("not checked"), a `false` is `err`
// ("offline"), and a `true` is `ok` — three outcomes, not two. Collapsing them is what made a row
// paint a red dot for a device nobody had asked about, and this module is where that collapse is
// written down once.

/// `tunnelKnownDown(status)` — THE TRI-STATE RULE, IN ONE PLACE.
///
/// `status?.tunnel_up === false` — STRICT equality against `false`, so an absent flag and an absent
/// status are both `false` here: "not known down" is not "known up". That distinction is the whole
/// point of the module; the differential carries `tunnel_up` missing, `true`, `false` and `null`.
#[wasm_bindgen]
pub fn tunnel_known_down(status: JsValue) -> bool {
    // `=== false` — a boolean, and false. `!v` would also answer true for `0` and `""`, and
    // `tunnel_up` is a wire boolean; a numeric 0 is not the same claim as a checked-and-down flag.
    let v = prop(&status, "tunnel_up");
    v.js_typeof().as_string().as_deref() == Some("boolean") && !v.is_truthy()
}

/// `deviceIsUp(status)` — `!!status?.agent_up`, i.e. TRUTHINESS on the optional chain. A caller that
/// needs "offline" rather than "not checked" reads the signal below instead.
#[wasm_bindgen]
pub fn device_is_up(status: JsValue) -> bool {
    optional(&status, "agent_up").is_truthy()
}

/// `agentSignal(status, t)` — the agent's row: what it is called, which of the three states, and the
/// word for it.
///
/// `signalOf(status?.agent_up, …)`: an ABSENT status makes the field `undefined`, which is the
/// `off` / "not checked" arm. The field's TYPE is `boolean | undefined`, so the JS test is
/// `value === undefined` — distinct from `false`, which is `err`. `js_sys::JsValue` has no
/// "is undefined" in the loose sense, so the test is `is_undefined()` here.
#[wasm_bindgen]
pub fn agent_signal(status: JsValue, t: &js_sys::Function) -> Result<js_sys::Object, JsValue> {
    let value = optional(&status, "agent_up");
    let row = signal_of(&value, t, "devices.online", "devices.offline")?;
    // The spread `{ label, ...signalOf }` puts `label` FIRST, and this module's own comment records
    // that the KEY ORDER is the wire format — so the row is rebuilt in the TypeScript's order.
    let out = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &out,
        &JsValue::from_str("label"),
        &t.call1(&JsValue::UNDEFINED, &JsValue::from_str("devices.statusAgent"))?,
    );
    for k in ["signal", "ok", "err", "state"] {
        let v = js_sys::Reflect::get(&row, &JsValue::from_str(k)).unwrap_or(JsValue::UNDEFINED);
        let _ = js_sys::Reflect::set(&out, &JsValue::from_str(k), &v);
    }
    Ok(out)
}

/// `tunnelSignal(status, t)` — the same three states for the tunnel, because the gateway may not
/// have probed it either.
#[wasm_bindgen]
pub fn tunnel_signal(status: JsValue, t: &js_sys::Function) -> Result<js_sys::Object, JsValue> {
    let value = optional(&status, "tunnel_up");
    let row = signal_of(&value, t, "devices.tunnelUp", "devices.tunnelDown")?;
    let out = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &out,
        &JsValue::from_str("label"),
        &t.call1(&JsValue::UNDEFINED, &JsValue::from_str("devices.statusTunnel"))?,
    );
    for k in ["signal", "ok", "err", "state"] {
        let v = js_sys::Reflect::get(&row, &JsValue::from_str(k)).unwrap_or(JsValue::UNDEFINED);
        let _ = js_sys::Reflect::set(&out, &JsValue::from_str(k), &v);
    }
    Ok(out)
}

/// `signalOf(value, t, yes, no)` — THE THREE OUTCOMES.
///
/// An `undefined` value is the `off` / not-checked arm and is BOTH flags false; a truthy value is `ok`
/// and a falsy (present) one is `err`. `value === undefined` is the JS's test, and it is why the
/// absent case is separated from the false case all the way down.
fn signal_of(
    value: &JsValue,
    t: &js_sys::Function,
    yes: &str,
    no: &str,
) -> Result<js_sys::Object, JsValue> {
    let o = js_sys::Object::new();
    if value.is_undefined() {
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str("signal"), &JsValue::from_str("off"));
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str("ok"), &JsValue::FALSE);
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str("err"), &JsValue::FALSE);
        let _ = js_sys::Reflect::set(
            &o,
            &JsValue::from_str("state"),
            &t.call1(&JsValue::UNDEFINED, &JsValue::from_str("devices.notChecked"))?,
        );
    } else if value.is_truthy() {
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str("signal"), &JsValue::from_str("ok"));
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str("ok"), &JsValue::TRUE);
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str("err"), &JsValue::FALSE);
        let _ = js_sys::Reflect::set(
            &o,
            &JsValue::from_str("state"),
            &t.call1(&JsValue::UNDEFINED, &JsValue::from_str(yes))?,
        );
    } else {
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str("signal"), &JsValue::from_str("err"));
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str("ok"), &JsValue::FALSE);
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str("err"), &JsValue::TRUE);
        let _ = js_sys::Reflect::set(
            &o,
            &JsValue::from_str("state"),
            &t.call1(&JsValue::UNDEFINED, &JsValue::from_str(no))?,
        );
    }
    Ok(o)
}

/// `status?.agent_up` — OPTIONAL CHAINING, so a null or undefined STATUS is `undefined` here and the
/// `off` arm below fires, rather than reading a property off nullish.
fn optional(status: &JsValue, key: &str) -> JsValue {
    if status.is_null() || status.is_undefined() {
        return JsValue::UNDEFINED;
    }
    js_sys::Reflect::get(status, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

/// `deviceTally(devices, statuses)` — THE COUNTS, IN ONE PLACE.
///
/// Two surfaces used to compute "N devices online" from the same fact with their own
/// `filter(…).length`, which is how the per-device mark disagreed with the count. A count cannot show
/// ambiguity per device, so BOTH numbers are returned: `online` is what is known, `unchecked` is what
/// is not yet known. `devices ?? []` — a nullish list is an EMPTY list, not a throw.
#[wasm_bindgen]
pub fn device_tally(devices: JsValue, statuses: JsValue) -> js_sys::Object {
    let list: Vec<JsValue> = if devices.is_null() || devices.is_undefined() {
        Vec::new()
    } else if devices.is_array() {
        js_sys::Array::from(&devices).iter().collect()
    } else {
        Vec::new()
    };
    let mut online = 0f64;
    let mut tunnels = 0f64;
    let mut unchecked = 0f64;
    for d in &list {
        let name = js_sys::Reflect::get(d, &JsValue::from_str("name")).unwrap_or(JsValue::UNDEFINED);
        let st = js_sys::Reflect::get(&statuses, &JsValue::from_str(&js_text(&name))).unwrap_or(JsValue::UNDEFINED);
        // `statuses[d.name]` is `undefined` for a name the map does not carry, and THAT is the
        // unchecked test (`st === undefined`), not a missing `agent_up` inside a present entry.
        if st.is_undefined() {
            unchecked += 1.0;
        }
        if optional(&st, "agent_up").is_truthy() {
            online += 1.0;
        }
        if optional(&st, "tunnel_up").is_truthy() {
            tunnels += 1.0;
        }
    }
    let o = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&o, &JsValue::from_str("online"), &JsValue::from_f64(online));
    let _ = js_sys::Reflect::set(&o, &JsValue::from_str("tunnels"), &JsValue::from_f64(tunnels));
    let _ = js_sys::Reflect::set(&o, &JsValue::from_str("unchecked"), &JsValue::from_f64(unchecked));
    let _ = js_sys::Reflect::set(&o, &JsValue::from_str("total"), &JsValue::from_f64(list.len() as f64));
    o
}

// `CONSOLE_POLL_MS` IS NOT HERE, AND THE REASON IS THE SAME ONE THAT KEPT `WORKING_MS` IN THE PANEL.
// It is the CONSOLE'S OWN POLLING CADENCE, read by two views' intervals; no function in this crate
// consults it — so moving it would move a number the logic never looks at, and leave the module-level
// `export const CONSOLE_POLL_MS = logic().console_poll_ms()` evaluating BEFORE any caller can await
// `consoleLogic()`, which is what the first version did and what `test/device-state.test.mjs` caught
// by throwing `console logic was called before it loaded` on import. A surface's own number stays with
// the surface; the panel reached the same call on `liveness.rs`'s `WORKING_MS`.
