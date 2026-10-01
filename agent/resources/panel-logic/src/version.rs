//! Which version to show for a device — **THE PANEL'S HALF OF A RULE THAT EXISTS TWICE**.
//!
//! THE DEVICE REPORTS TWO VERSIONS, AND THEY ARE NOT INTERCHANGEABLE:
//!
//!   * `release` — the npm release, 1.2.x, written by install/update into `.summrise-release`. THIS is
//!     the number that changes per release, and the only one a user can compare against "is my device up
//!     to date?".
//!   * `version` — the Cargo protocol version, 1.0.x, from `env!("CARGO_PKG_VERSION")`. FROZEN: it has
//!     not moved in a very long time and is not a release.
//!
//! `DesktopShell` learned this and says so beside its own copy of the rule; `ConnectCard`'s connection
//! probe kept using `version`. In the desktop shell both are on screen at once — the status strip reading
//! v1.2.354 while Settings reported v1.0.145 for the SAME device, so a user checking whether their update
//! or registration took got two answers.
//!
//! # AND IT IS STILL TWO COPIES, ONE PER PACKAGE
//!
//! The gateway decides the same thing for the console in `gateway/src/plugins/mcp.ts` (`wireVersion`),
//! and it met the same defect from the other side — round-304 there, a strip reading v1.2.354 here. They
//! cannot share a module, so each names the other and `agent/tests/device_version_rule.rs` holds both to
//! one table: it requires that each side still implements the rule under a name, that BOTH prefer
//! `release` and only then fall back to `version`, and that each side NAMES the other. `version` remains
//! the fallback for a device old enough not to send `release` at all.
//!
//! The panel's half moved here (block ②, 2026-09-30) and the gate followed it: its panel subject is this
//! file now, and the gateway's comment names this file in turn.

use wasm_bindgen::prelude::*;

use crate::js::{boxed, prop, type_of};

/// `typeof v === "string" && v` — the STRICT type test and the truthiness, which is what the TypeScript
/// does: a non-string `release` (a number, an object) is not a version, and neither is the empty string.
fn string_field(target: &JsValue, key: &str) -> Option<String> {
    let v = prop(&boxed(target), key);
    if type_of(&v) == "string" && v.is_truthy() {
        v.as_string()
    } else {
        None
    }
}

/// `releaseVersion(status)` — the npm release if the device sent one, else the frozen protocol version,
/// else nothing.
///
/// `(status ?? {})` IS THE FIRST THING THAT HAPPENS, and it is why a nullish status answers `""` rather
/// than raising. Everything else is read off a BOXED value, because `(5).release` is `undefined` in
/// JavaScript — a primitive has no properties but is not an error either.
#[wasm_bindgen]
pub fn release_version(status: JsValue) -> String {
    let s = if status.is_null() || status.is_undefined() {
        JsValue::from(js_sys::Object::new())
    } else {
        status
    };
    if let Some(release) = string_field(&s, "release") {
        return release;
    }
    if let Some(version) = string_field(&s, "version") {
        return version;
    }
    String::new()
}

/// `v1.2.354`, or `v?` when the device reported neither — never a bare "v".
#[wasm_bindgen]
pub fn release_version_label(status: JsValue) -> String {
    let v = release_version(status);
    format!("v{}", if v.is_empty() { "?" } else { &v })
}
