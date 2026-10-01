//! `lib/bootNotice.ts` — what the panel says about the PREVIOUS run of the agent.
//!
//! WHY A RULE AT ALL. The device has known how the last run ended since round 254: `REPLACED by a
//! restart` (an update swap or a task restart) or `CRASHED or was killed`. That sentence lived in
//! `logs\startup.log` on the machine, and a field engineer reading a log is not the operator watching
//! a panel. `/api/status` ships it now, and this module decides what it is worth saying out loud —
//! because the two cases deserve OPPOSITE treatment: one is the normal consequence of updating, and
//! one is a fault. Rendering them the same way is how an operator learns to ignore both.
//!
//! TWO TONES, TWO LIFETIMES:
//!
//!   * `crashed` — a fault, and it stays true for the whole life of the current run (the agent is
//!     running BECAUSE something restarted it after a bad death). The chip stays until the next clean
//!     boot clears it. It is not an alarm to dismiss; it is a fact about the run you are in.
//!   * `replaced` — normal, and worth saying only while it is NEWS: right after `summrise update` the
//!     operator wants the panel to confirm the swap, and after that the uptime already answers it.
//!
//! `first-run`, `clean-exit` and `machine-restart` say NOTHING: a device that was rebooted did not
//! fault, and a warning that fires after every routine reboot is one nobody reads by the time a real
//! crash arrives.
//!
//! # THE ONE NUMBER THE CALLER STILL OWNS
//!
//! `REPLACED_NOTICE_SECS` is an ARGUMENT, not a constant in here: it is the panel's own patience with
//! a normal restart, it is exported from `lib/bootNotice.ts`, and the module's test imports it. The
//! arrangement `liveness.rs` records for `WORKING_MS`.
//!
//! # AND THE VOCABULARY IS NOT COPIED A THIRD TIME
//!
//! `isKnownKind` asks whether `BOOT_KINDS` — generated from the agent's own enum — contains the wire's
//! string, and this module reads that list from `vocabulary.rs`, where the crate keeps its one copy.
//! A kind a NEWER device invents is answered "no" and renders as SILENCE, never as another kind's
//! words.

use js_sys::Object;
use wasm_bindgen::prelude::*;

use crate::js::{prop, put, text, type_of};
use crate::vocabulary::BOOT_KINDS;

/// The kinds, in words. ONE vocabulary for every surface that names a verdict — the chip's hover, the
/// history card's rows — so a kind cannot be described two ways in one panel. `null` (an unrecognised
/// kind) says so rather than borrowing another kind's wording.
#[wasm_bindgen]
pub fn boot_kind_label(kind: JsValue) -> String {
    // `switch (kind)` is a STRICT comparison, so only a string takes a case and everything else —
    // a number, an object, `null` — lands on the default.
    match kind.as_string().as_deref() {
        Some("first-run") => "first start",
        Some("clean-exit") => "previous run exited cleanly",
        Some("replaced") => "replaced by a restart",
        Some("machine-restart") => "the machine restarted",
        Some("crashed") => "previous run crashed",
        _ => "unrecorded",
    }
    .to_string()
}

/// True for the one kind that means the agent died on its own. Used by the history card to weigh a row
/// and by the summary line to count; the chip has its own rule.
#[wasm_bindgen]
pub fn is_crash(kind: JsValue) -> bool {
    kind.as_string().as_deref() == Some("crashed")
}

/// IS THIS KIND ONE THE DEVICE CAN REPORT? `!!kind && BOOT_KINDS.includes(kind)` — truthiness first
/// (so `null`, `undefined` and the empty string are all "no"), then membership, which for a non-string
/// is false for the same reason `includes` says so.
fn is_known_kind(kind: &JsValue) -> bool {
    if !kind.is_truthy() {
        return false;
    }
    match kind.as_string() {
        Some(s) => BOOT_KINDS.contains(&s.as_str()),
        None => false,
    }
}

/// The chip to render, or `null` for "nothing worth saying".
///
/// `uptime_secs` is required for the `replaced` case and may be null: without a trustworthy "how long
/// ago", a normal restart cannot be told from a stale one, and the rule then says nothing rather than
/// guessing.
///
/// `recent_crashes` is the device's own 24 h count from `/api/boots`. It never changes WHETHER the
/// chip appears — it is the same verdict either way — and it only ever adds a sentence to the hover:
/// "this happened once" and "this keeps happening" are different situations, and an operator staring
/// at the chip is exactly who needs to know which.
#[wasm_bindgen]
pub fn boot_notice(
    last_boot: JsValue,
    uptime_secs: JsValue,
    recent_crashes: JsValue,
    replaced_notice_secs: JsValue,
) -> JsValue {
    if !last_boot.is_truthy() {
        return JsValue::NULL;
    }
    let kind = prop(&last_boot, "kind");
    // A KIND THIS BUILD DOES NOT KNOW IS SILENCE, and `is_known_kind` is what says so: the list is
    // generated from the agent's own enum, so a verdict a NEWER device invents cannot be described
    // with an older one's words. This is the same outcome the default branch reaches, stated where
    // the decision belongs.
    if !is_known_kind(&kind) {
        return JsValue::NULL;
    }
    let detail = prop(&last_boot, "detail");
    match kind.as_string().as_deref() {
        Some("crashed") => {
            let base = format!(
                "{}\n\nThe agent is running now — this is how the run before it ended.",
                text(&detail)
            );
            // `typeof recentCrashes === "number" && recentCrashes > 1` — STRICT on the type, so a
            // numeric string is not a count, and the comparison then coerces.
            let repeated = match type_of(&recent_crashes).as_str() {
                "number" if crate::js::to_number(&recent_crashes) > 1.0 => format!(
                    "\n\n{} crashes in the last 24 hours — the Restarts card in Settings lists them.",
                    text(&recent_crashes)
                ),
                _ => String::new(),
            };
            let o = Object::new();
            let _ = put(&o, "tone", &JsValue::from_str("warn"));
            let _ = put(&o, "text", &JsValue::from_str("last run crashed"));
            let _ = put(&o, "title", &JsValue::from_str(&format!("{base}{repeated}")));
            o.into()
        }
        Some("replaced") => {
            // `typeof uptimeSecs !== "number" || uptimeSecs >= REPLACED_NOTICE_SECS` — `typeof` is
            // strict (a numeric STRING answers null) and the comparison coerces, so a NaN uptime
            // passes the guard and the chip IS shown, which is what the TypeScript does.
            if type_of(&uptime_secs) != "number"
                || crate::js::to_number(&uptime_secs) >= crate::js::to_number(&replaced_notice_secs)
            {
                return JsValue::NULL;
            }
            let o = Object::new();
            let _ = put(&o, "tone", &JsValue::from_str("info"));
            let _ = put(&o, "text", &JsValue::from_str("just restarted"));
            // THE RAW VALUE, not its text: the TypeScript's `title: lastBoot.detail` is an assignment,
            // while the crash branch's template COERCES — and the two are different objects when the
            // device sends something that is not a string.
            let _ = put(&o, "title", &detail);
            o.into()
        }
        _ => JsValue::NULL,
    }
}
