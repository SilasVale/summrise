//! `hooks/useAgentVitals.ts`'s `parseLastBoot` — the device's last boot, transliterated.
//!
//! The seventh family to move (P2), and the THIRD at a `useDeviceRead` fold (`parseMonitors` and
//! `parseVitalsSeries` were the first two). `parseLastBoot` runs inside `reduceVitals`, which the
//! seam declares `T | Promise<T>` — so the migration is free for the reason the rule states: **a
//! call site is free if it is not during render**.
//!
//! WHAT STAYS IN TYPESCRIPT, and each for its own reason rather than by habit: `reduceVitals` is the
//! fold itself (a hook's business), `fmtUptime` is called while `DeviceHealthCard` and the status
//! line RENDER, `AgentVitals`/`LastBoot`/`BootKind` are types, and `EMPTY_VITALS` is a constant.
//! `parseLastBoot` was the only function in the file that parsed.
//!
//! ── THE THREE REFUSALS, EACH A DIFFERENT KIND OF TEST ───────────────────────────────────────────
//!
//!   * **A TRIMMED-EMPTY SENTENCE IS NOT A SENTENCE.** `last_boot` must be a string, and it is
//!     TRIMMED before the emptiness test — so `""` and `"   "` both answer `null`, while `" "` in
//!     `boot.rs`'s `detail` is a usable value. The two modules read the same field family and
//!     disagree on purpose; each is the TypeScript's own rule and neither was tidied into the other.
//!   * **A KIND WITH NO SENTENCE IS NOT A BOOT.** `{last_boot_kind: "crashed"}` alone answers
//!     `null` — the test is on `detail`, which is read FIRST and decides everything.
//!   * **AN UNRECOGNISED KIND IS `null`, NOT A NEAREST MATCH.** The membership test is
//!     [`crate::vocabulary::boot_kind`], strict equality against the five the device writes; a
//!     `last_boot_kind: "melted"` keeps its sentence and gets `kind: null`, which is the panel's way
//!     of saying "unrecorded" rather than of putting another kind's words under this one's name.
//!
//! AND THE FIELD IS NOT TRIMMED ON THE KIND SIDE: `last_boot_kind` is read as-is, so `"crashed "`
//! is unrecognised. That is the TypeScript's `typeof j.last_boot_kind === "string" ? … : ""` with no
//! `.trim()`, and it is written down here because the asymmetry one line above is exactly the kind
//! of thing a port "fixes" without noticing.

use crate::js::{prop, put, type_of};
use crate::vocabulary::boot_kind;
use js_sys::Object;
use wasm_bindgen::prelude::*;

/// `parseLastBoot(j)` — the last boot, or `null` when the body does not describe one.
#[wasm_bindgen]
pub fn parse_last_boot(j: JsValue) -> Result<JsValue, JsValue> {
    // `typeof j.last_boot === "string" ? j.last_boot.trim() : ""`
    let detail = match prop(&j, "last_boot") {
        v if type_of(&v) == "string" => v.as_string().unwrap_or_default().trim().to_string(),
        _ => String::new(),
    };
    // `if (!detail) return null` — the trimmed sentence decides, before the kind is looked at.
    if detail.is_empty() {
        return Ok(JsValue::NULL);
    }

    // NO TRIM HERE, deliberately — see the header.
    let raw = match prop(&j, "last_boot_kind") {
        v if type_of(&v) == "string" => v.as_string().unwrap_or_default(),
        _ => String::new(),
    };

    let out = Object::new();
    put(
        &out,
        "kind",
        &match boot_kind(&raw) {
            Some(k) => JsValue::from_str(k),
            None => JsValue::NULL,
        },
    )?;
    put(&out, "detail", &JsValue::from_str(&detail))?;
    Ok(out.into())
}
