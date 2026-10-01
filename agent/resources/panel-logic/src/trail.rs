//! `lib/trailRead.ts` — WHAT AN EMPTY TRAIL IS ALLOWED TO SAY, in one wording for three views.
//!
//! A view that draws its own "nothing here yet" line makes a CLAIM about the device: that the session
//! ran nothing. That claim is only available when a read actually SUCCEEDED. `useCommandEvents` has
//! reported `readState` (`"reading" | "ok" | "unreadable"`) for longer than the Archive has used it —
//! but the Archive was its ONLY consumer, so the live trajectory and path views printed "No commands in
//! this session yet." unconditionally. Two reachable windows, one of them on EVERY session switch:
//! `useCommandEvents` resets `events` to `[]` synchronously while the new read is in flight, so the
//! operator is told the session is empty for the whole round trip.
//!
//! This is round 27's defect one field over: `firstSeq` was hoisted through `App` for exactly this
//! reason and `readState` was left behind in the same object literal. The Archive's header states the
//! rule — "a session whose trail cannot be read SAYS SO, and never renders as an empty history" — and it
//! was honoured in one place out of three.
//!
//! The wording lives HERE rather than in each view because three copies of one sentence is how this
//! repo's views come to disagree about what they are saying. The Archive's phrasing is the original and
//! is preserved verbatim.

use js_sys::Object;
use wasm_bindgen::prelude::*;

use crate::js::put;

/// The sentence a view shows while a read is in flight, or when it failed — or `null` when the caller's
/// own empty state is TRUE and may be shown.
///
/// THE COMPARISONS ARE STRICT, so anything that is not exactly `"ok"` or `"unreadable"` takes the
/// in-flight branch — which is the safe direction: a state this build has never heard of is not a
/// licence to claim the session ran nothing.
#[wasm_bindgen]
pub fn trail_read_notice(read: JsValue) -> Result<JsValue, JsValue> {
    match read.as_string().as_deref() {
        Some("ok") => Ok(JsValue::NULL),
        Some("unreadable") => {
            let out = Object::new();
            put(&out, "failed", &JsValue::TRUE)?;
            // NOT "the session is empty": the read failed and nothing established anything about the
            // session. The file may be gone, or the device may be unreachable — both are different from
            // "it ran nothing".
            put(
                &out,
                "text",
                &JsValue::from_str(
                    "This session's audit trail could not be read from the device, so nothing is shown. \
                     This is not an empty history: the file may be gone, or the device may be unreachable.",
                ),
            )?;
            Ok(out.into())
        }
        _ => {
            let out = Object::new();
            put(&out, "failed", &JsValue::FALSE)?;
            put(
                &out,
                "text",
                &JsValue::from_str("Reading this session's audit trail…"),
            )?;
            Ok(out.into())
        }
    }
}
