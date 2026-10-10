//! THE BROWSER-SESSION WINDOWS AND THE TWO EMBEDDED VIEWS — their doors, their plans and their records.
//!
//! Two tables live in `main.ts` and neither is a decision: `browserSessions` (id → `BrowserWindow`) and
//! `browserTargets` (id → the target it was opened on) are the HOST's state, because only the host can
//! observe `isDestroyed()` and `getURL()`. What moves here is every question asked OF them — which
//! window a reopen should focus, which one the cap evicts, whether a slot is big enough to show a native
//! view over, and what a crashed view should be reloaded with.

use serde_json::{json, Value};

/// The blank page — the one target every refusing door collapses to.
pub const ABOUT_BLANK: &str = "about:blank";

/// The cap on concurrent browser-session windows, so an AI loop opening sessions repeatedly cannot pile
/// windows up on the desktop.
pub const MAX_BROWSER_WINDOWS: usize = 8;

/// The smallest slot a native view is placed over, in CSS pixels. Below this the view is HIDDEN rather
/// than placed, which is the SPA's way of saying "the page is not showing".
///
/// IT IS ONE CONSTANT FOR TWO VIEWS AND IT WAS TWO LITERALS BEFORE THIS PORT — `embeddedViewPlace` and
/// `dshPlace` each spelled `bounds.width < 50 || bounds.height < 50`. Two copies of one threshold is
/// how the browser view and the DSH view would come to disagree about what "showing" means.
pub const MIN_SLOT_PX: f64 = 50.0;

/// One row of the host's session table, as the plan needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserSession {
    pub id: String,
    /// The INITIAL target, stored at open — not `webContents.getURL()`, which LAGS during navigation
    /// (it answers `about:blank` while loading), so a same-URL reopen right after the first open would
    /// otherwise miss the reuse and stack a duplicate.
    pub target: String,
    /// A destroyed window is skipped for REUSE but still COUNTS against the cap, exactly as
    /// `browserSessions.size` did — the map entry is only removed by the `closed` handler.
    pub destroyed: bool,
}

/// What `browserOpen` should do with a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenPlan {
    /// Focus this existing window; the cap and the id are not consulted.
    Reuse(String),
    /// Open a new window, evicting this id first (the cap was reached), or evicting nothing.
    New { evict: Option<String> },
}

/// `browserOpen`'s two branches, in the order the TypeScript wrote them.
///
/// THE FIRST MATCH WINS AND THE ORDER IS INSERTION ORDER: `Map` iteration is insertion-ordered, so
/// "oldest" is `keys().next()` — the first row of `existing`, not the least recently used. A port that
/// reached for an LRU would evict a different window than the shell does.
pub fn plan_browser_open(existing: &[BrowserSession], target: &str, cap: usize) -> OpenPlan {
    for session in existing {
        if !session.destroyed && session.target == target {
            return OpenPlan::Reuse(session.id.clone());
        }
    }
    OpenPlan::New {
        evict: if existing.len() >= cap {
            existing.first().map(|session| session.id.clone())
        } else {
            None
        },
    }
}

/// The plan as JSON: `{"reuse": id|null, "evict": id|null}`.
pub fn plan_browser_open_json(existing_json: &str, target: &str, cap: f64) -> String {
    let rows: Vec<BrowserSession> = serde_json::from_str::<Value>(existing_json)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|row| BrowserSession {
            id: row
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            target: row
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            destroyed: row
                .get("destroyed")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect();
    // A fractional or negative cap cannot make `len() >= cap` mean anything useful; the shell passes a
    // whole number, and a value that is not one falls back to the cap this crate owns.
    let cap = valid_cap(cap);
    match plan_browser_open(&rows, target, cap) {
        OpenPlan::Reuse(id) => json!({ "reuse": id, "evict": Value::Null }).to_string(),
        OpenPlan::New { evict } => json!({ "reuse": Value::Null, "evict": evict }).to_string(),
    }
}

fn valid_cap(cap: f64) -> usize {
    // A RANGE rather than two comparisons, and the NaN question is answered by `is_finite` first:
    // `(0.0..1e6).contains(&NaN)` is already `false` (`NaN >= 0.0` is), so the finite test is about the
    // `as usize` cast below rather than about the bounds.
    if cap.is_finite() && cap.fract() == 0.0 && (0.0..1e6).contains(&cap) {
        cap as usize
    } else {
        MAX_BROWSER_WINDOWS
    }
}

/// `browser-${Date.now()}` — the id every session window is keyed by.
pub fn browser_id(now_ms: f64) -> String {
    format!("browser-{}", crate::js::number_to_string(now_ms))
}

/// `http://127.0.0.1:9333` — the CDP endpoint every browser-session answer carries.
///
/// 9333 is a product contract and this function only FORMATS it: nothing here opens the port. The switch
/// that opens it (`app.commandLine.appendSwitch("remote-debugging-port", …)`) is an effect and stays in
/// `main.ts`, unguarded and unmoved.
pub fn cdp_endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// `!bounds || bounds.width < 50 || bounds.height < 50` — is this slot too small to show a view over?
///
/// `None` is the TypeScript's `null`/`undefined`/falsy bounds; a MISSING width or height is `NaN` here
/// and answers `false`, which is what `undefined < 50` answers in JavaScript too (any comparison with
/// NaN is false) — so an object without the fields places rather than hides, on both sides.
pub fn slot_too_small(width: f64, height: f64) -> bool {
    width < MIN_SLOT_PX || height < MIN_SLOT_PX
}

/// The embedded view's `window.open` decision: the decided target, or `None` meaning DROP IT.
///
/// THE ARGUMENT IS THE DECIDED TARGET — `sanitizeBrowserUrl`'s answer, evaluated by the host, because the
/// load door is the url-policy crate's and this crate cannot call it (see `Cargo.toml`). What is decided
/// HERE is the second half, and it is the half that was wrong:
///
/// `main.ts` records the user's report ("浏览器超链接跳转不了"): `target=_blank` links are the norm on real
/// sites and were DENIED outright, so clicking them did nothing — but the view is a single-tab browser, so
/// the fix navigates the SAME view. And a target the ONE load door REFUSES (`javascript:`, `file:`,
/// `chrome:`) must not blank the page being read, so it is dropped rather than collapsed to
/// `about:blank`. Two different answers to the same refusal, which is why this is a function and not an
/// `if` at the call site: a caller that compared against `about:blank` itself would navigate the view to
/// the blank page and lose what the operator was reading.
pub fn embedded_popup_target(target: &str) -> Option<String> {
    if target == ABOUT_BLANK {
        None
    } else {
        Some(target.to_string())
    }
}

/// The DSH view's `window.open` decision — the same shape over the DSH's own door.
///
/// `admitted` is `isDshUrl(raw)`, evaluated by the host; "the door allows one origin, so in practice an
/// external link is simply dropped rather than silently navigating this view away from the harness."
pub fn dsh_popup_target(raw: &str, admitted: bool) -> Option<String> {
    let target = dsh_target(raw, admitted);
    if target == ABOUT_BLANK {
        None
    } else {
        Some(target)
    }
}

/// `dshTarget` — the DSH view's ONE load door: its own origin, or nothing.
///
/// `admitted` is `isDshUrl(raw)`, the url-policy crate's predicate over its own port list, evaluated by
/// the host — this crate must not hold a second copy of that list (see `Cargo.toml`), and re-deriving the
/// comparison here would be a second oracle for it. What is decided HERE is the SERIALIZATION: the
/// admitted target is run through the WHATWG serializer, which is `new URL(raw).toString()`. A target the
/// door admitted cannot fail to parse, so the `Err` arm is unreachable and answers the refusal rather
/// than a panic.
pub fn dsh_target(raw: &str, admitted: bool) -> String {
    if !admitted {
        return ABOUT_BLANK.to_string();
    }
    match url::Url::parse(raw) {
        Ok(parsed) => parsed.to_string(),
        Err(_) => ABOUT_BLANK.to_string(),
    }
}

/// `dshBase() + "/"` — the address `dshOpen` and `dshRecover` both load.
///
/// It is a decision and not a string concatenation to be left at the call site: it was spelled TWICE in
/// `main.ts` (open and recover), and the recovery path is the one that must not drift, because a crash
/// recovery that lands somewhere else is a view the operator did not ask for. The BASE arrives from the
/// host, which is the only thing that can read the configured DSH port.
pub fn dsh_home(dsh_base: &str) -> String {
    format!("{dsh_base}/")
}

/// `Math.min(3, Math.max(0.5, Number(factor) || 1))` — the embedded view's zoom clamp.
///
/// The `|| 1` is a FALSY test on the COERCED value, so `NaN` and `0` (and `-0`) all become 1 before the
/// clamp — which is why a zoom of 0 is 100% rather than 50%. `Number(factor)` itself happens at the wasm
/// boundary, where JavaScript can do it.
pub fn zoom_factor(number: f64) -> f64 {
    let factor = if number == 0.0 || number.is_nan() {
        1.0
    } else {
        number
    };
    // `clamp` rather than `max(0.5).min(3.0)`, and the ORDER IS WHY IT IS SAFE: `f64::clamp` answers NaN
    // for a NaN input, where `max`/`min` swallow it (each ignores a NaN operand) — and the `|| 1` above is
    // what removes the NaN before this line, which is the same falsy test the TypeScript applied. The test
    // `zoom_is_clamped_and_a_zero_means_one_hundred_percent` pins the NaN case from the outside.
    factor.clamp(0.5, 3.0)
}

/// `embeddedUrl || "about:blank"` — what a crashed view is reloaded with.
///
/// THE FALLBACK IS NOT A HOME PAGE AND THAT IS THE DECISION. `main.ts` records that the view used to
/// load `https://www.bing.com` eagerly at startup, i.e. "a request to a third party on EVERY launch
/// before anyone asked for a page", and recovery must not reintroduce it: "recovering a crashed view
/// must not make a request to a search engine to do it."
pub fn embedded_recover_url(last: Option<&str>) -> String {
    match last {
        Some(url) if !url.is_empty() => url.to_string(),
        _ => ABOUT_BLANK.to_string(),
    }
}

/// `wc ? wc.getURL() || embeddedUrl : embeddedUrl` — the URL a view's state record reports.
///
/// `live` is `None` when there is no live `webContents` at all; `Some("")` is a live one that has not
/// navigated yet. Both answer the fallback, which is why the two cases share one function.
pub fn shown_url(live: Option<&str>, fallback: &str) -> String {
    match live {
        Some(url) if !url.is_empty() => url.to_string(),
        _ => fallback.to_string(),
    }
}

/// `embeddedVisible && !!view && !view.webContents.isDestroyed()` — the `visible` field of a state
/// record. The host computes the two booleans; the conjunction is the decision.
pub fn view_visible(want_visible: bool, live_contents: bool) -> bool {
    want_visible && live_contents
}

/// `delta < 0` — which way `embedded-browser:back`/`:fwd` go.
///
/// The channel pair is the shell's own and the argument is typed `-1 | 1` at the call site, so this
/// looks like a formality. It is here because the SIGN is the only thing that distinguishes two IPC
/// channels from one, and a port that guessed `!== -1` would answer `goForward` for a delta of `-2`.
pub fn go_backwards(delta: f64) -> bool {
    delta < 0.0
}

/// THE HARNESS DOORS — one port per host's forward, from the agent's OWN table.
///
/// `GET /api/workspace/harnesses` names a `local_port` per host, and the shell admits what the answer
/// names: never a port of its own choosing, and never a host that is not in the table. A read that fails
/// leaves the list as it was — "a shell that widened its own door list on a failed read is exactly how
/// the view ends up pointed somewhere it was not told about."
///
/// The ARGUMENT IS THE COERCED NUMBERS, one per row, in order: `Number(row?.local_port)` is JavaScript's
/// conversion and happens at the wasm boundary, while WHICH numbers are admitted is decided here by the
/// same range test every other port in the shell goes through. `None` is a row whose `local_port` was
/// missing or not a number.
pub fn harness_doors(numbers: &[Option<f64>]) -> Vec<u16> {
    numbers
        .iter()
        .filter_map(|number| number.and_then(crate::boot::valid_port))
        .collect()
}

/// The single-row form, for the wasm wrapper's per-row loop: is this one admissible?
pub fn harness_door(number: Option<f64>) -> Option<u16> {
    number.and_then(crate::boot::valid_port)
}
