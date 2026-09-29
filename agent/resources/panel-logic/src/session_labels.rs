//! `lib/sessionLabels.ts`'s `disambiguateLabels` — the ninth family, and **THE FIRST ONE THAT IS
//! CALLED DURING RENDER**.
//!
//! ── WHY THIS ONE IS THE PROOF AND NOT JUST ANOTHER FAMILY ────────────────────────────────────────
//!
//! Every family before this one moved by finding an ASYNCHRONOUS call site: a `useDeviceRead` fold,
//! an SSE listener, a `useMemo` whose input could be re-derived at the data boundary. That is what
//! the seam's "fetched at the first call" forced, and P2's own record lists what it left behind —
//! `derivePath`, `livenessOf`, `cardState`, and THIS function, which the plan says "definitely does
//! not pass" the boundary road because its input is a SUBSET chosen by the call site
//! (`TabBar` numbers `sessions`, `DesktopShell` numbers `openTabs`, and one boundary derivation
//! cannot be both).
//!
//! **That objection is dissolved rather than worked around (2026-09-29).** `index.html` now fetches
//! and compiles this module while panel.js is still downloading, and `main.tsx` awaits it before the
//! first render, so a migrated function is a PLAIN SYNCHRONOUS CALL from inside a component. The
//! measured cost of that is NEGATIVE — see the module header of
//! `panel-react/src/wasm/panelLogic.ts` for the alternating A/B: the panel's first frame arrives
//! ~12.6 ms sooner, because the module's compile no longer competes with React's first render.
//!
//! **AND THE SUBSET PROBLEM IS GONE WITH IT.** `TabBar` and `DesktopShell` each call this with
//! whatever list they are rendering, exactly as they did in TypeScript. A boundary derivation would
//! have had to number the union and would have changed one of the two strips.
//!
//! ── WHAT THE FUNCTION DOES, IN THE TYPESCRIPT'S OWN TERMS ────────────────────────────────────────
//!
//! TWO PASSES, and the first is why: "does this label collide" is a fact about the WHOLE list, which
//! a single pass cannot know. A label that appears ONCE stays bare — there is no set for it to be a
//! member of — and a label that appears more than once is numbered on EVERY member, the first
//! included, so the mark reads as an ordinal over a set rather than as a mutation of one row.
//!
//! THE COUNTER LEADS (`2·stc@192.168.1.1`, not `stc@192.168.1.1 2`) because both strips render a
//! label through `text-overflow: ellipsis`, which removes the END of the string — so a suffix is the
//! one token the renderer is guaranteed to delete first. `lib/sessionLabels.ts` carries the
//! measurements (the `·` against `#1 ` in the device's own font at the tab's own weight); this file
//! carries only the rule.
//!
//! ── THE TWO THINGS THE PORT HAD TO BE CAREFUL ABOUT ──────────────────────────────────────────────
//!
//! **The map keys are JS values, not strings.** The TypeScript keys a `Map` by `item.label`, and a
//! `Map` compares by SameValueZero — so `1` and `"1"` are two different labels there. This uses a
//! `js_sys::Map` for the same reason, and NOT a `HashMap<String, _>`: the Rust map would collapse
//! the two, and the collapse would be invisible in every test written with string labels.
//!
//! **The counter's text is the ENGINE's.** `` `${n}·…` `` is a template literal, i.e. `String(n)` for
//! the number and `String(label)` for the label. `crate::js::number_text` and `crate::js::text` ask
//! the engine for both rather than using Rust's formatter, which is this migration's standing rule
//! and not a hypothetical one: `format!("{}", 1e21)` and `String(1e21)` disagree, and a key spelled
//! two ways is two keys.
//!
//! **AND THE FIRST VERSION OF THIS FILE GOT IT WRONG, WHICH IS WHY THE SENTENCE IS HERE.** It used
//! `JsString::from(value)`, which is an UPCAST rather than a coercion: every non-string label became
//! `""` and the counter became nothing at all (`·pwsh`, `·pwsh`). The differential against the
//! deleted TypeScript caught it on 21 of its 31 cases — the numbers are in the commit message — and
//! the corpus is the reason it could: `null`, `undefined`, a number, a boolean and a missing
//! property are all in it, and none of them is a shape a typed caller can produce.

use crate::js::{number_text, prop, text};
use js_sys::{Array, Map};
use wasm_bindgen::prelude::*;

/// `disambiguateLabels(items)` — one label per item, numbered where they collide.
///
/// THE INPUT IS AN `Array` AND A NON-ARRAY IS REFUSED, which is a deliberate narrowing of the
/// TypeScript rather than an accident: its `for (const item of items)` accepts any ITERABLE, so
/// `disambiguateLabels("ab")` walks the string's characters there and answers `["1·undefined", …]`.
/// That is not a shape this panel produces (both call sites pass a `Session[]`), and reproducing it
/// would mean writing a rule for an input nobody has — so the port throws instead, loudly, in the
/// one case the TypeScript would have silently invented labels for.
#[wasm_bindgen]
pub fn disambiguate_labels(items: JsValue) -> Result<Array, JsValue> {
    let items: Array = items.dyn_into().map_err(|_| {
        JsValue::from_str("disambiguateLabels expects an array of { label } — see session_labels.rs")
    })?;

    // PASS ONE — how many times each label occurs. Keyed by the VALUE, for the reason in the header.
    let counts = Map::new();
    for item in items.iter() {
        let label = prop(&item, "label");
        let n = counts.get(&label).as_f64().unwrap_or(0.0) + 1.0;
        counts.set(&label, &JsValue::from_f64(n));
    }

    // PASS TWO — the running position within each label's own run, and the answer.
    let seen = Map::new();
    let out = Array::new();
    for item in items.iter() {
        let label = prop(&item, "label");
        let n = seen.get(&label).as_f64().unwrap_or(0.0) + 1.0;
        seen.set(&label, &JsValue::from_f64(n));
        if counts.get(&label).as_f64() == Some(1.0) {
            // A LONE LABEL IS RETURNED AS IT ARRIVED, not stringified — the TypeScript's own
            // `item.label`, and the reason this arm pushes the value rather than a copy of its text.
            out.push(&label);
        } else {
            out.push(&JsValue::from_str(&format!(
                "{}·{}",
                number_text(n),
                text(&label)
            )));
        }
    }
    Ok(out)
}
