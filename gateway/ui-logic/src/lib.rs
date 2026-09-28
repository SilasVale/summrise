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
/// number-or-value's text, never `format!` — a `key` spelled `1e+21` on one side and
/// `1000000000000000000000` on the other is a different key.
#[wasm_bindgen]
pub fn bare_prefix(prefix: JsValue) -> String {
    let s: String = js_sys::JsString::from(prefix).as_string().unwrap_or_default();
    // `/\/+$/` — ALL trailing slashes, which is the wider of the two behaviours the console had.
    s.trim_end_matches('/').to_string()
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
