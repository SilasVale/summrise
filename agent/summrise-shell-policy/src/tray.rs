//! THE TRAY — the `/api/status` parse with its keep-last rule, and every string the tooltip and the
//! status row are built from.
//!
//! The tray is the ONE surface that cannot re-render: a tooltip is "a string written once per poll" and
//! Electron gives it no way to recompute, so its wording is a decision with a documented incident behind
//! it (round-275, quoted in `main.ts`): it used to print "Agent stopped" as present tense from ONE
//! un-timestamped probe, recomputed every 30 s — "half a minute stale while reading as now". The
//! replacement names the OBSERVATION and the TIME, and falls back to "reachability NOT VERIFIED · no
//! reply since launch" when nothing has ever answered. That is the sentence this file computes.

use crate::js;
use serde_json::{json, Value};

/// What the last `/api/status` answer said, carried across polls.
///
/// EVERY FIELD IS KEPT WHEN THE NEXT ANSWER OMITS IT, which is the TypeScript's shape: the five `let`s in
/// `refreshTray` are copied from the previous poll and only overwritten by a field the answer actually
/// carries. A poll that fails therefore leaves the last known version on screen rather than blanking it.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusFacts {
    /// `j.release || j.version` — the npm RELEASE version first (the number that changes per release,
    /// written by update/setup), with the Cargo protocol `version` as the fallback.
    pub version: String,
    /// The uptime ALREADY FORMATTED by [`fmt_uptime`], because that is what the tooltip renders and
    /// reformatting it per poll would be the same answer computed twice.
    pub uptime: String,
    pub sessions: f64,
    pub cpu: Option<f64>,
    pub mem: Option<f64>,
}

impl Default for StatusFacts {
    fn default() -> Self {
        Self {
            version: String::new(),
            uptime: String::new(),
            sessions: 0.0,
            cpu: None,
            mem: None,
        }
    }
}

/// Is this JSON value TRUTHY the way JavaScript means it?
///
/// It exists for exactly one expression, `j.release || j.version`, and the distinction it preserves is
/// the one a truthiness test always carries: `0`, `""`, `null` and `false` fall through to the fallback,
/// while `[]` and `{}` — which a Rust `!is_null()` would also refuse — do NOT.
fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(inner) => *inner,
        Value::Number(number) => number.as_f64().map(|n| n != 0.0).unwrap_or(true),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `String(value)` for a JSON value — what a template literal does to whatever it is handed.
///
/// The array arm is JavaScript's `Array.prototype.join(",")`, recursively, and the object arm is the
/// `"[object Object]"` that `Object.prototype.toString` answers. Neither can occur in a real
/// `/api/status` answer; they are here because a version field that answered `[1,2]` would render as
/// `v1,2` in the tooltip and the point of this crate is that the shell and the source agree about what
/// it renders.
fn js_to_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(inner) => inner.to_string(),
        Value::Number(number) => match number.as_f64() {
            Some(n) => js::number_to_string(n),
            None => number.to_string(),
        },
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(js_to_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `"3m 24s"` / `"1h 5m"` / `"2d 3h"` — the tray's compact uptime.
///
/// The boundaries are the interesting part and they are pinned: 59 s is `59s`, 60 s is `1m 0s`, 3599 s is
/// `59m 59s`, 3600 s is `1h 0m`, 86399 s is `23h 59m`, 86400 s is `1d 0h`.
pub fn fmt_uptime(secs: f64) -> String {
    if secs < 60.0 {
        return format!("{}s", js::number_to_string(secs));
    }
    if secs < 3600.0 {
        return format!(
            "{}m {}s",
            js::number_to_string((secs / 60.0).floor()),
            js::number_to_string(secs % 60.0)
        );
    }
    if secs < 86400.0 {
        return format!(
            "{}h {}m",
            js::number_to_string((secs / 3600.0).floor()),
            js::number_to_string(((secs % 3600.0) / 60.0).floor())
        );
    }
    format!(
        "{}d {}h",
        js::number_to_string((secs / 86400.0).floor()),
        js::number_to_string(((secs % 86400.0) / 3600.0).floor())
    )
}

/// `new Date(at).toTimeString().slice(0, 8)` — `"14:02:31"` in LOCAL time.
///
/// **THE LOCAL TIME ZONE IS A HOST FACT AND IT CROSSES THE BOUNDARY AS AN ARGUMENT.** wasm has no clock
/// and no zone database, so `main.ts` passes `new Date(at).getTimezoneOffset()` for the SAME instant it
/// is formatting — which is the correct offset even across a DST boundary, because the offset is read
/// per-date rather than once per process. The arithmetic (subtract the offset, take the time of day) is
/// the decision and it is here.
///
/// WHY A CLOCK AND NOT "answered 12s ago": a relative age "would freeze at the value it had when written
/// and read as 'now' for the next 30 s — the very failure this line removes. A timestamp does not decay."
pub fn fmt_clock(at_ms: f64, tz_offset_min: f64) -> String {
    // `getTimezoneOffset()` is minutes to ADD to local to get UTC, so local = utc - offset.
    let local_ms = at_ms - tz_offset_min * 60_000.0;
    let secs = (local_ms / 1000.0).floor();
    let second = secs.rem_euclid(60.0);
    let minute = (secs / 60.0).floor().rem_euclid(60.0);
    let hour = (secs / 3600.0).floor().rem_euclid(24.0);
    format!(
        "{:02}:{:02}:{:02}",
        hour as i64, minute as i64, second as i64
    )
}

/// Apply one `/api/status` body to the previous facts.
///
/// A body that does not parse, or that is not an object, KEEPS THE PREVIOUS FACTS ENTIRELY — which is
/// the TypeScript's `await r.json()` throwing into a `catch { /* keep last */ }`. The four `typeof`
/// guards are `Value::as_f64`, which is exactly as narrow: a JSON string, bool or null is not a number
/// on either side.
pub fn apply_status(status_text: &str, previous: &StatusFacts) -> StatusFacts {
    let body: Value = match serde_json::from_str(status_text) {
        Ok(value) => value,
        Err(_) => return previous.clone(),
    };
    if !body.is_object() {
        return previous.clone();
    }
    let mut facts = previous.clone();
    // `const v = j.release || j.version; if (v) version = v;`
    if let Some(field) = ["release", "version"]
        .iter()
        .filter_map(|key| body.get(*key))
        .find(|value| js_truthy(value))
    {
        facts.version = js_to_string(field);
    }
    if let Some(secs) = body.get("uptime_secs").and_then(Value::as_f64) {
        facts.uptime = fmt_uptime(secs);
    }
    if let Some(sessions) = body.get("live_sessions").and_then(Value::as_f64) {
        facts.sessions = sessions;
    }
    if let Some(cpu) = body.get("cpu_pct").and_then(Value::as_f64) {
        facts.cpu = Some(cpu);
    }
    if let Some(mem) = body.get("mem_pct").and_then(Value::as_f64) {
        facts.mem = Some(mem);
    }
    facts
}

/// The tooltip's one line: `"Summrise — <this>"`.
///
/// The three shapes are the round-275 fix, and each names what was observed:
///
///   * `answered 14:02:31 · v1.2.510, up 3m 24s, 2 sessions, CPU 4% · MEM 12%` — a reply arrived, at a
///     time, with whatever of the answer's fields were present. A field the answer omitted is ABSENT
///     rather than zero: `version || "?"`, `uptime ? ", up …" : ""`, `sessions ? … : ""`.
///   * `reachability NOT VERIFIED · no reply since launch` — nothing has ever answered. This is the
///     sentence the whole file exists for: it says what was CHECKED rather than asserting a state.
///   * `not answering · last reply 14:02:31` — it answered once and has not since, and the WHEN is the
///     last reply rather than the failed probe.
///
/// **TWO OFFSETS, NOT ONE.** The two clock faces can be hours apart — a device that answered before a DST
/// change and is being polled after it — and the TypeScript read `new Date(at).getTimezoneOffset()` for
/// EACH instant it formatted. One offset for both would put the wrong hour on one of the two lines for
/// exactly one poll, which is the "a timestamp does not decay" promise this line was rewritten to keep.
pub fn tray_health(
    running: bool,
    facts: &StatusFacts,
    observed_at: f64,
    observed_tz_min: f64,
    last_answered: Option<(f64, f64)>,
) -> String {
    if !running {
        return match last_answered {
            None => "reachability NOT VERIFIED \u{00b7} no reply since launch".to_string(),
            Some((at, at_tz)) => {
                format!("not answering \u{00b7} last reply {}", fmt_clock(at, at_tz))
            }
        };
    }
    // `running && mem !== null` — the vitals are shown only when a memory figure arrived, because half a
    // vitals line would read as a CPU figure with no denominator.
    let vitals = match facts.mem {
        None => String::new(),
        Some(mem) => format!(
            ", CPU {}% \u{00b7} MEM {}%",
            match facts.cpu {
                Some(cpu) => js::number_to_string(js::round(cpu)),
                None => "?".to_string(),
            },
            js::number_to_string(js::round(mem))
        ),
    };
    let version = if facts.version.is_empty() {
        "?"
    } else {
        facts.version.as_str()
    };
    let uptime = if facts.uptime.is_empty() {
        String::new()
    } else {
        format!(", up {}", facts.uptime)
    };
    let sessions = if facts.sessions != 0.0 && !facts.sessions.is_nan() {
        format!(
            ", {} session{}",
            js::number_to_string(facts.sessions),
            if facts.sessions == 1.0 { "" } else { "s" }
        )
    } else {
        String::new()
    };
    format!(
        "answered {} \u{00b7} v{version}{uptime}{sessions}{vitals}",
        fmt_clock(observed_at, observed_tz_min)
    )
}

/// `!running && !agentWatchActive && win && !win.isDestroyed() && isBaseOrigin(win.webContents.getURL())`
/// — should a failed tray poll re-enter the load loop?
///
/// THE ORIGIN HALF IS THE POINT: the wait page is a `data:` URL and the tripwire's blank page is
/// `about:blank`, and neither should be swapped for the wait page again. Only a main window still ON the
/// SPA's own origin is re-entered — and `agent_watch_active` is the loop guard, because the tray polls
/// every 30 s and "repeated tray polls cannot stack retries".
///
/// `base_origin` is `isBaseOrigin(...)`, evaluated by the host over the url-policy crate's own port — this
/// crate must not hold a second copy of that port (see `Cargo.toml`). The host also keeps the
/// SHORT-CIRCUIT the TypeScript had: it evaluates the predicate only when the window is live, so a
/// destroyed window never reaches a `getURL()` that would throw.
pub fn tray_should_watch(
    running: bool,
    watch_active: bool,
    window_live: bool,
    base_origin: bool,
) -> bool {
    !running && !watch_active && window_live && base_origin
}

/// The facts as JSON, the shape `main.ts` carries between polls.
pub fn facts_to_json(facts: &StatusFacts) -> Value {
    json!({
        "version": facts.version,
        "uptime": facts.uptime,
        "sessions": facts.sessions,
        "cpu": facts.cpu,
        "mem": facts.mem,
    })
}

/// The facts back out of JSON. A missing or mistyped field is the DEFAULT, never an error: this is the
/// shell's own state coming back to it, not an answer from anywhere.
pub fn facts_from_json(value: &Value) -> StatusFacts {
    StatusFacts {
        version: value
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        uptime: value
            .get("uptime")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        sessions: value.get("sessions").and_then(Value::as_f64).unwrap_or(0.0),
        cpu: value.get("cpu").and_then(Value::as_f64),
        mem: value.get("mem").and_then(Value::as_f64),
    }
}

/// ONE POLL'S WHOLE DECISION, at the JSON boundary `main.ts` uses.
///
/// The request carries what the host observed — whether the probe answered, the `/api/status` body as
/// TEXT (empty when the fetch failed or the response was not ok), the previous facts, the time of this
/// observation, the time of the last reply, and the local offset for the same instant. The answer
/// carries the new facts, the tooltip line, and the new `lastAnsweredAt`.
///
/// THE PARSE IS HERE RATHER THAN AT THE CALL SITE because it is where the keep-last rule lives: `r.text()`
/// instead of `r.json()` moves the throw into this function, and this function is the one that knows a
/// failed read means "keep what we had" rather than "everything is zero".
pub fn refresh_tray_health(request_json: &str) -> String {
    let request: Value = serde_json::from_str(request_json).unwrap_or(Value::Null);
    let previous = facts_from_json(&request["prev"]);
    let running = request
        .get("running")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let observed_at = request
        .get("observedAt")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let last_answered_at = request.get("lastAnsweredAt").and_then(Value::as_f64);
    let tz_offset_min = request
        .get("tzOffsetMin")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    // The offset for the LAST REPLY's own instant, read by the host from the same instant it names. It
    // falls back to the observation's offset, which is what a caller that has only one offset gets.
    let last_tz_offset_min = request
        .get("lastTzOffsetMin")
        .and_then(Value::as_f64)
        .unwrap_or(tz_offset_min);
    let status_text = request
        .get("statusText")
        .and_then(Value::as_str)
        .unwrap_or("");

    let facts = if running && !status_text.is_empty() {
        apply_status(status_text, &previous)
    } else {
        previous
    };
    let health = tray_health(
        running,
        &facts,
        observed_at,
        tz_offset_min,
        last_answered_at.map(|at| (at, last_tz_offset_min)),
    );
    // `if (running) trayLastAnsweredAt = observedAt;` — the WHEN of the observation, recorded only when
    // the observation was a reply.
    let next_answered_at = if running {
        Some(observed_at)
    } else {
        last_answered_at
    };
    json!({
        "facts": facts_to_json(&facts),
        "health": health,
        "lastAnsweredAt": next_answered_at,
    })
    .to_string()
}
