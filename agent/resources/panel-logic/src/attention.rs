//! `lib/attention.ts`'s two CHANNELS — the tab title and the favicon badge, transliterated.
//!
//! The eighth family to move (P2), and the first PARTIAL one: `attentionFrom`, `stateKey`,
//! `shouldNotify` and `attentionSummary` stay in TypeScript because they are called during RENDER
//! (`useAttention`'s `useMemo`, `MonitorAlerts`, the settings card), while these two are called from
//! `useAttentionTitle`'s `useEffect` — **and an effect is a boundary, so an awaited seam costs
//! nothing there.** The plan's rule is "一处一处搬" (one place at a time), and this is the first time
//! the place is smaller than the family; the split is named here rather than left to be rediscovered.
//!
//! ── `titleFor` — THE COUNT IS FOR A TAB, NOT FOR A WINDOW ───────────────────────────────────────
//!
//! `(2) Summrise Agent` is a TAB idiom: a browser tab truncates from the right, so the count has to
//! lead. The desktop app is a native window whose title is its identity rather than a queue of pages,
//! and it already draws the same attention — with the hosts NAMED — in its status bar. So `tab:
//! false` returns the base untouched, and the operator's own report is the reason (`"(2) Summrise
//! Agent, the (2) should not be there"`, round 199).
//!
//! ── `badgeIcon` — THE BADGE IS ADDED TO THE ICON, NOT INSTEAD OF IT ─────────────────────────────
//!
//! The first version returned a simplified drawing of its own (an orange square, a red disc, a
//! count) — which looked like a different, wrong icon the moment anything needed attention. Now the
//! badge is INSERTED into the existing svg data URL, so the artwork is byte-for-byte the one the page
//! already loads and only a disc is added on top.
//!
//! AND THE HREF IS PERCENT-ENCODED, so the markers are looked for in BOTH spellings. The first
//! version searched for a literal `<svg`, never found one in the encoded href, and silently returned
//! the base — the artwork was preserved and the badge never appeared. It was found on d1 by asking
//! the panel what its icon actually was; the test's fixture had been an unencoded SVG, i.e. a guess.
//!
//! A THIRD THING, KEPT BECAUSE IT IS THE BEHAVIOUR: an icon that cannot be parsed (a file URL, an
//! empty href, a marker after the close) is left ALONE rather than replaced by something invented.
//! The `marker < 0 || close < marker` guard is what does it, and `close` really can be -1.

use js_sys::Array;
use wasm_bindgen::prelude::*;


/// A plain byte-window scan, and NOT `str::find`/`rfind`.
///
/// MEASURED, AND THIS IS WHERE THE MODULE'S COST WAS: Rust's `str::find(&str)` runs the TWO-WAY
/// substring search — a real algorithm with a critical-factorisation table — and it is linked
/// whether the haystack is a 30-byte favicon `href` or 30 KB. Six `find`/`rfind`/`contains` calls
/// cost **10,692 bytes raw / 5,431 gz**: the artifact went 73,113 → **62,421** when they became this
/// window scan (baseline without this module: 59,143). The scan is O(n·m) on inputs that are a data
/// URL, and it is what the crate already does elsewhere (`find_seq`).
///
/// THIS IS THE PLAN'S `{:.3}` LESSON ARRIVING FROM A NEW DIRECTION: that one was a FORMATTER linked
/// by a format string; this one is a SEARCH ALGORITHM linked by an idiomatic method call. Both read
/// as free, and neither shows up until the artifact is measured. **A port of a small function is not
/// a small port if it reaches for the standard library's general machinery.**
fn find_at(hay: &str, needle: &str) -> Option<usize> {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    if n.is_empty() || n.len() > h.len() {
        return None;
    }
    h.windows(n.len()).position(|w| w == n)
}

fn rfind_at(hay: &str, needle: &str) -> Option<usize> {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    if n.is_empty() || n.len() > h.len() {
        return None;
    }
    h.windows(n.len()).rposition(|w| w == n)
}

/// `titleFor(items, base, tab)` — the tab title, or the base when there is nothing to say.
///
/// `items.length` IS A NON-NEGATIVE INTEGER BELOW 2^32, which is the one place this file may use
/// Rust's formatter for a number: `String(n)` and `{}` agree on every such value (they diverge at
/// 1e21 and on `-0`, neither of which an array length can be). The crate's rule — ask the ENGINE,
/// never Rust's formatter — is about values a device can send, and this one cannot be sent at all.
#[wasm_bindgen]
pub fn title_for(items: JsValue, base: String, tab: bool) -> String {
    let arr = Array::from(&items);
    let len = arr.length();
    if len == 0 {
        return base;
    }
    // THE COUNT IS FOR A TAB, NOT FOR A WINDOW — see the header.
    if !tab {
        return base;
    }
    // `items.some((i) => i.kind === "approval")` — STRICT equality with the string, and a missing
    // `kind` is not it.
    let urgent = arr.iter().any(|i| {
        crate::js::prop(&i, "kind")
            .as_string()
            .map(|k| k == "approval")
            .unwrap_or(false)
    });
    format!("({len}){} {base}", if urgent { " ⚠" } else { "" })
}

/// `badgeIcon(count, urgent, baseHref)` — the favicon for this much attention, as a data URL.
///
/// `count` arrives as a JS number rather than a `usize` because the TypeScript's own tests are the
/// subject: `count <= 0` and `count > 9` are the two tests, and a `-1` or a `2.5` reaches them.
#[wasm_bindgen]
pub fn badge_icon(count: f64, urgent: bool, base_href: JsValue) -> String {
    // `baseHref ?? ""` — null AND undefined both become the empty string.
    let base = match base_href.as_string() {
        Some(s) => s,
        None => String::new(),
    };
    if count <= 0.0 || base.is_empty() {
        return base;
    }

    // THE REAL HREF IS PERCENT-ENCODED — both spellings are looked for.
    let encoded = find_at(&base, "%3Csvg").is_some() || find_at(&base, "%3csvg").is_some();
    let close = if encoded {
        let a = rfind_at(&base, "%3C/svg%3E");
        let b = rfind_at(&base, "%3c/svg%3e");
        match (a, b) {
            (Some(x), Some(y)) => x.max(y),
            (Some(x), None) | (None, Some(x)) => x,
            (None, None) => usize::MAX, // "not found", handled by the guard below
        }
    } else {
        rfind_at(&base, "</svg>").unwrap_or(usize::MAX)
    };
    let marker = if encoded {
        let a = find_at(&base, "%3Csvg");
        let b = find_at(&base, "%3csvg");
        match (a, b) {
            (Some(x), Some(y)) => x.min(y),
            (Some(x), None) | (None, Some(x)) => x,
            (None, None) => usize::MAX,
        }
    } else {
        find_at(&base, "<svg").unwrap_or(usize::MAX)
    };
    // `marker < 0 || close < marker` — with `usize::MAX` standing in for -1, "not found" is the
    // larger value, so the same two comparisons refuse the same inputs.
    if marker == usize::MAX || close == usize::MAX || close < marker {
        return base;
    }

    let fill = if urgent { "%23d9480f" } else { "%23e03131" };
    // `count > 9 ? "" : String(count)` — a small integer, so Rust's formatter is the same text.
    let label = if count > 9.0 {
        String::new()
    } else {
        format!("{}", count as i64)
    };
    let (lt, gt) = if encoded { ("%3C", "%3E") } else { ("<", ">") };
    let badge = format!(
        "{lt}circle cx='37' cy='11' r='11' fill='{fill}'/{gt}{}",
        if label.is_empty() {
            String::new()
        } else {
            format!(
                "{lt}text x='37' y='43' font-family='system-ui,sans-serif' font-size='20' font-weight='700' fill='white' text-anchor='middle'{gt}{label}{lt}/text{gt}"
            )
        }
    );
    // Inserted just before the closing tag: on top of the artwork, with the artwork intact.
    //
    // BUILT BY WALKING THE CHARACTERS rather than by `&base[..close]`. The slice is 134 bytes
    // LARGER, which is why the walk is here — and that number is written down because the first
    // version of this comment claimed the slice cost 13,113 bytes. It did not: the real cost was
    // `str::find` (see `find_at` below), and a measurement that gets attached to the wrong line is
    // worse than no measurement, because the next reader optimises the wrong thing.
    let mut out = String::with_capacity(base.len() + badge.len());
    for (i, c) in base.char_indices() {
        if i == close {
            out.push_str(&badge);
        }
        out.push(c);
    }
    out
}
