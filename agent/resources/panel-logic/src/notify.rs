//! `lib/notify.ts`'s TWO PURE DECISIONS — the permission state and what the operator is told about it.
//!
//! WHAT MAKES THIS WORTH ITS OWN MODULE is not `new Notification(...)`; it is everything around it:
//!
//!   * **PERMISSION IS A STATE, NOT A BOOLEAN.** The browser has four answers — unsupported, `default`
//!     (never asked), `granted`, `denied` — and `denied` is PERMANENT for the origin: the page cannot
//!     re-ask, so the only honest thing to do is SAY SO and point at the channel that still works (the
//!     tab title). A toggle that silently does nothing is how a feature gets distrusted.
//!   * **THE REQUEST NEEDS A GESTURE**, which is why `Notification.requestPermission()` is called from
//!     the toggle and never from an effect — a rule the CALLER keeps, not this module.
//!
//! # WHAT IS NOT HERE
//!
//! `DeviceNotifier` — the dedupe, the rate limit and the `tag` — is a STATEFUL object, and it is still in
//! `lib/notify.ts`. Its rules are real and its own: at most one notification every `MIN_GAP_MS`, at most
//! `BURST` in a minute, a key retired when the condition clears so "down, up, down again" notifies
//! twice, and the suppressed count reported rather than silently lost. Moving it needs a handle the
//! panel holds across calls, which is a shape this crate does not export yet; it is named here rather
//! than half-done.

use wasm_bindgen::prelude::*;

/// `readPermission(ctor, permission)` — the browser's answer, read defensively.
///
/// A page in an insecure context has no `Notification` at all, and that is a STATE, not an error: the
/// caller passes the constructor it found (or nothing) and the permission string it read (or nothing),
/// because reading `Notification.permission` is the BOUNDARY this module deliberately does not cross.
///
/// `permission ?? …` IS NULLISH COALESCING, not truthiness: an EMPTY STRING is a value the caller read
/// and is passed through to the strict comparison below, where it answers `default`.
#[wasm_bindgen]
pub fn read_permission(ctor: JsValue, permission: JsValue) -> String {
    if !ctor.is_truthy() {
        return "unsupported".to_string();
    }
    // **THE COMPARISON IS ON THE RAW VALUE, NOT ON ITS TEXT.** `p === "granted"` is false for
    // `["granted"]` and for `5`, so those answer `default` — and the first version of this port
    // stringified first (`String(["granted"])` is `"granted"`) and answered `granted`, which the
    // differential caught on four cases. `as_string()` is the strict reading: `None` for anything that
    // is not a string.
    let p = if permission.is_null() || permission.is_undefined() {
        None
    } else {
        permission.as_string()
    };
    match p.as_deref() {
        Some("granted") => "granted",
        Some("denied") => "denied",
        _ => "default",
    }
    .to_string()
}

/// What the operator should be told about the current state, in the panel's own voice — the settings
/// card renders this verbatim, and it is the ONLY place the difference between "you never turned it
/// on", "the browser said no" and "this browser cannot" is explained.
#[wasm_bindgen]
pub fn permission_hint(state: JsValue) -> String {
    match state.as_string().as_deref() {
        Some("granted") => {
            "Desktop notifications are on. A watched host going down reaches you even when this window \
             is in the background."
        }
        Some("denied") => {
            "This browser is blocking notifications for this page, and it will not ask again — allow \
             them in the site settings (the padlock in the address bar), or rely on the tab title, which \
             always works."
        }
        Some("unsupported") => {
            "This browser (or this page's context) cannot show desktop notifications, so the tab title \
             carries the count instead — it always works."
        }
        _ => {
            "Notifications are off. Turning them on asks your browser once; if it says no, the tab \
             title still carries the count."
        }
    }
    .to_string()
}
