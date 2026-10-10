//! §4b `panel` — the AI writes a unique marker into a terminal session, and the SPA's VISIBLE xterm must show
//! it. This is display verification rather than API verification: the session can be perfect and the operator
//! can still be looking at nothing.
//!
//! **THE POLL SERIES IS RE-JUDGED HERE, NOT TRUSTED.** The driver polls the visible xterm and stops when it
//! has any text at all; whether the text contains the MARKER is this function's decision, and it replays the
//! series the driver recorded. That is why the series is in the rows rather than summarized into a boolean:
//! a `found: true` computed by the driver would be the verdict, computed in the wrong language.

use crate::rows::{text_of, View};
use crate::verdict::{Check, Verdict};

pub fn judge(rows: &View, out: &mut Verdict) {
    let marker = rows
        .got("mint.panel_marker")
        .ok()
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    // ── the AI's write landed in the session ─────────────────────────────────────────────────────────
    let Some(ex) = rows.got("panel.exec").ok() else {
        return;
    };
    let ai_write = text_of(ex).contains(&marker);
    out.push(Check::pass_or_fail(
        "panel ai write",
        ai_write,
        format!("state={}", crate::rows::js_field(ex, "state")),
    ));

    // ── and the SPA is showing that session ──────────────────────────────────────────────────────────
    //
    // NO DESKTOP TARGET IS ITS OWN VERDICT: without the SPA there is no visible xterm to read, and the suite
    // says so on the check rather than skipping it (the panel target is a precondition of the section, not an
    // arm of the product).
    let Some(list) = rows.got("panel.cdp.targets").ok() else {
        return;
    };
    let has_spa = list
        .as_array()
        .map(|a| {
            a.iter()
                .any(|t| crate::rows::js_field(t, "url").contains("/desktop/"))
        })
        .unwrap_or(false);
    if !has_spa {
        out.push(Check::fail(
            "panel xterm shows ai output",
            "no desktop SPA target",
        ));
        return;
    }

    // The JS update rule, replayed: a series entry that is `NO_VISIBLE` leaves the previous answer standing;
    // an entry carrying the marker family ends the loop with its own answer; any other text replaces it.
    let mut found = false;
    for raw in rows.series_values("panel.xterm") {
        let text = match raw {
            serde_json::Value::String(s) => s.clone(),
            other => text_of(other),
        };
        if text.is_empty() {
            continue;
        }
        if text != "NO_VISIBLE" && text.contains("PANEL-VIS-") {
            found = text.contains(&marker);
            break;
        }
        if text != "NO_VISIBLE" {
            found = text.contains(&marker);
        }
    }
    out.push(Check::pass_or_fail(
        "panel xterm shows ai output",
        found,
        format!("marker={}", crate::text::truncate(&marker, 22)),
    ));
}
