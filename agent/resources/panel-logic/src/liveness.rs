//! `lib/liveness.ts`'s PREDICATES — one state per entity, transliterated.
//!
//! The tenth family to move (P2), and the LARGEST one left: seven functions, of which four compose
//! the other three. It is also the first family that could not have moved before 2026-09-29 at all —
//! every one of them is called DURING RENDER (`TabBar`, `ContextRail`, `DesktopShell`, `IconRail`
//! each call one inside a `.map()` or a component body), which the old "fetched at the first call"
//! seam made impossible. See `panel-react/src/wasm/panelLogic.ts` for the measurement that removed it.
//!
//! ── WHAT STAYS IN TYPESCRIPT, AND IT IS NOT AN OVERSIGHT ─────────────────────────────────────────
//!
//! `URGENCY`, `SILHOUETTE` and `MOVES` — the three tables that name each state's rank, shape and
//! motion. **NOTHING IN THE PRODUCT READS THEM**: the grep is three components' worth of nothing and
//! one test file, which is why they are not here. They are the SHAPE vocabulary the test pins ("no
//! two states share a silhouette", "only `working` moves", the urgency order), and moving a table
//! that only a test reads would put bytes in the artifact every operator downloads in order to
//! answer a question no operator asks. **The functions below are what the panel actually calls.**
//!
//! `WORKING_MS` ALSO STAYS, and it is passed IN rather than restated. It belongs to
//! `hooks/useDeviceActivity.ts` — the device-wide recency signal — and `sessionActive` reads the same
//! window, which is why the TypeScript imported it rather than writing 8000 twice. A constant copied
//! into this crate would be the second copy the comment in that file exists to prevent, so the
//! caller supplies it and the RULE that reads it is here.
//!
//! ── THE TWO THINGS THE DIFFERENTIAL INSISTED ON, AND BOTH WERE FIXED RATHER THAN DOCUMENTED ──────
//!
//! **`null` RAISES, HERE TOO.** `null.reachable` is a TypeError, and `prop` — the crate's `(j ?? {})`
//! guard, which is right for a PARSER — answers `undefined` for it. A predicate that returned `idle`
//! for a null session would paint a quiet dot about nothing where the TypeScript crashed loudly, so
//! every function here refuses a nullish input (`js::require_present`), and `anyCommandRunning`
//! refuses a nullish ELEMENT for the same reason (`[null].some(s => s.commandRunning)` raises).
//!
//! **`pendingCount > 0` COERCES, HERE TOO.** JavaScript's relational operators apply ToNumber, so
//! `"5" > 0` and `[5] > 0` are both TRUE — and reading the property with `as_f64()` answers `None`
//! for both, which would have made this port quietly disagree about two inputs out of four. The
//! comparison goes through `js::to_number`, which is the global `Number(v)` function.

use crate::js::{prop, require_present, to_number};
use wasm_bindgen::prelude::*;

/// `livenessOf(input)` — THE PRECEDENCE, in one place.
///
/// `reachable` is about the TRANSPORT, not the entity: a session on a dead connection cannot be
/// answered even if a question is outstanding, so it is `off` and the mark must not claim otherwise.
/// `waiting` outranks `working` because a question DECAYS if it is not seen while work continues;
/// `failed` sits between activity and quiet because it is a fact about what ALREADY HAPPENED.
///
/// EVERY TEST IS THE JAVASCRIPT'S TRUTHINESS — `!input.reachable`, `input.pending`, `input.active`,
/// `input.failed` — and not `=== true`, which is why the fields are read as `JsValue` and asked with
/// `is_truthy`. A `1`, a `"no"` and an object are all the JavaScript's answers, and a port that
/// compared to `true` would answer differently for every one of them.
#[wasm_bindgen]
pub fn liveness_of(input: JsValue) -> Result<String, JsValue> {
    require_present(&input, "livenessOf")?;
    if !prop(&input, "reachable").is_truthy() {
        return Ok("off".to_string());
    }
    if prop(&input, "pending").is_truthy() {
        return Ok("waiting".to_string());
    }
    if prop(&input, "active").is_truthy() {
        return Ok("working".to_string());
    }
    if prop(&input, "failed").is_truthy() {
        return Ok("failed".to_string());
    }
    Ok("idle".to_string())
}

/// `deviceLiveness(input)` — the device as a whole: reachable, holding questions, or busy.
///
/// NO `failed` HERE, AND THAT IS A DECISION RATHER THAN AN OMISSION: a device-level failure would
/// have to pick WHICH session's last command to blame and say nothing about which, and the rail is
/// the one mark that is always on screen — a light that is on most of the time means nothing.
#[wasm_bindgen]
pub fn device_liveness(input: JsValue) -> Result<String, JsValue> {
    require_present(&input, "deviceLiveness")?;
    let out = js_sys::Object::new();
    let _ = crate::js::put(&out, "reachable", &prop(&input, "connected"));
    let _ = crate::js::put(
        &out,
        "pending",
        &JsValue::from_bool(pending_count(&input) > 0.0),
    );
    let _ = crate::js::put(&out, "active", &prop(&input, "working"));
    liveness_of(out.into())
}

/// `input.pendingCount > 0`, with the JavaScript's own coercion — `to_number` is `Number(v)`, which
/// is what the relational operator applies. `"5"` and `[5]` are counts here because they are counts
/// THERE, and the differential is what insisted on it.
fn pending_count(input: &JsValue) -> f64 {
    to_number(&prop(input, "pendingCount"))
}

/// `sessionWaiting(session)` — CAN THIS SESSION STILL ANSWER, the ONE predicate the mark, the tab's
/// title and its aria-label all read.
///
/// A CLOSED session's row keeps its data — the tombstone is the same record — so a question that
/// expired with the session it belonged to survives in `pendingApproval`. Without the `closed` half,
/// the desktop tab's title says "waiting for your approval" about a tab that cannot be answered at
/// all, which is the disagreement between the two densities this model exists to stop.
#[wasm_bindgen]
pub fn session_waiting(session: JsValue) -> Result<bool, JsValue> {
    require_present(&session, "sessionWaiting")?;
    Ok(!prop(&session, "closed").is_truthy() && prop(&session, "pendingApproval").is_truthy())
}

/// `sessionActive(session, workingMs)` — is the SESSION working, not "is the device busy".
///
/// THE DEVICE'S ANSWER FIRST: `command_running` is the manager's own busy flag, so it is true for the
/// WHOLE life of a command, including the silent minutes that output recency cannot see. Recency is
/// the second signal, for work that is not a command through this path — and it is `typeof idleMs ===
/// "number"`, a TYPE test, so a session whose `idle_ms` arrived as a string is not a reading.
#[wasm_bindgen]
pub fn session_active(session: JsValue, working_ms: f64) -> Result<bool, JsValue> {
    require_present(&session, "sessionActive")?;
    if prop(&session, "commandRunning").is_truthy() {
        return Ok(true);
    }
    let idle = prop(&session, "idleMs");
    Ok(crate::js::type_of(&idle) == "number" && idle.as_f64().unwrap_or(f64::NAN) < working_ms)
}

/// `anyCommandRunning(sessions)` — is ANY session holding a command in flight, for the DEVICE mark.
///
/// `!!sessions?.some(…)`: an absent or null list is `false`, and anything that is NOT a list is a
/// throw in the JavaScript (`{}.some` is not a function) — so it is an error here too rather than a
/// silent `false`, which would turn a caller's mistake into "nothing is running".
#[wasm_bindgen]
pub fn any_command_running(sessions: JsValue) -> Result<bool, JsValue> {
    if sessions.is_null() || sessions.is_undefined() {
        return Ok(false);
    }
    let list: js_sys::Array = sessions.dyn_into().map_err(|_| {
        JsValue::from_str("anyCommandRunning expects an array of sessions — see liveness.rs")
    })?;
    // THE ELEMENTS ARE GUARDED TOO, because `[null].some(s => s.commandRunning)` raises in the
    // JavaScript — `null` has nothing to auto-box — while `[3]` is `undefined` and simply falsy.
    let mut any = false;
    for s in list.iter() {
        require_present(&s, "anyCommandRunning")?;
        if prop(&s, "commandRunning").is_truthy() {
            any = true;
        }
    }
    Ok(any)
}

/// `sessionLiveness(session, workingMs)` — ONE DERIVATION FOR EVERY SURFACE.
///
/// Nothing about a session's own mark needs the device: connectivity is the RAIL's fact, so a
/// disconnected device does not make every session `off` — which is what a CLOSED session means.
#[wasm_bindgen]
pub fn session_liveness(session: JsValue, working_ms: f64) -> Result<String, JsValue> {
    require_present(&session, "sessionLiveness")?;
    let out = js_sys::Object::new();
    let _ = crate::js::put(&out, "reachable", &JsValue::from_bool(!session_closed(&session)));
    let _ = crate::js::put(
        &out,
        "pending",
        &JsValue::from_bool(session_waiting(session.clone())?),
    );
    let _ = crate::js::put(
        &out,
        "active",
        &JsValue::from_bool(session_active(session.clone(), working_ms)?),
    );
    let _ = crate::js::put(
        &out,
        "failed",
        &JsValue::from_bool(session_failed(session.clone())?),
    );
    liveness_of(out.into())
}

/// `!!session.closed` — spelled once because two of the four inputs read it.
fn session_closed(session: &JsValue) -> bool {
    prop(session, "closed").is_truthy()
}

/// `sessionFailed(session)` — DID THIS SESSION'S LAST COMMAND FAIL, the device's own exit code.
///
/// ABSENT IS NOT FAILURE and not success: `lastExitCode` is `null` when the device observed no code
/// at all, which is a third state a mark must not turn into either answer. NON-ZERO IS A FAILURE,
/// with no judgement about which codes deserve it — the command cards have called every non-zero exit
/// "Failed (exit N)" since they existed.
#[wasm_bindgen]
pub fn session_failed(session: JsValue) -> Result<bool, JsValue> {
    require_present(&session, "sessionFailed")?;
    let code = prop(&session, "lastExitCode");
    Ok(crate::js::type_of(&code) == "number" && code.as_f64().unwrap_or(0.0) != 0.0)
}
