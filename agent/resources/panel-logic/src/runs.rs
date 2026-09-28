//! `lib/runs.ts` — the device's operation timeline folded into RUNS — transliterated.
//!
//! The fourth family to move (P2), and **the first one that is on the RENDER path**, which is the
//! whole reason it is the fourth rather than the first. The three before it were free: a
//! `useDeviceRead` fold (`archiveEntries`, `parseMonitors`, `parseBootHistory`) and an SSE handler
//! (`parseMonitorChange`) are not renders, and the wasm is fetched at the first call, so a
//! migrated function may be called there and nowhere else. `groupOperation` and `operationRows`
//! are called from a `useMemo` INSIDE `RunStrip` and `ActivityPage` — a synchronous call during
//! render cannot wait for a fetch, and inlining the bytes costs +18,913 gz on the first-load
//! payload and grows with every family.
//!
//! # The sync story: the derivation moves to the boundary that OWNS the data
//!
//! `useOperationRuns` is a `useDeviceRead` reader: it fetches `/api/operation`, merges the reply
//! into one accumulated snapshot, and its `reduce` may return a promise (the module awaits it).
//! That fold is the DATA BOUNDARY — the one place where the wire becomes the panel's value and
//! where the wasm is already in hand — so the derivation happens THERE and rides out on the same
//! value the data does:
//!
//! ```text
//!   before   fetch → {events, boundaries} → render → useMemo(groupOperation) → useMemo(operationRows)
//!   after    fetch → {events, boundaries, groups, rows} → render reads `groups` / `rows`
//! ```
//!
//! **Nothing is derived twice and nothing is derived late.** The fold's two derived fields are
//! computed in the SAME state update that produces their input, so the render that first sees new
//! events is also the render that sees their grouping — the alternative (an effect that derives
//! after the render) would show one frame of new events under an old grouping, which is the
//! tearing this panel spends its comments preventing. And the render reads a FIELD, never the
//! wasm: there is no `await`, no fallback and no second implementation for the not-yet-loaded case.
//!
//! THE RULE THAT COMES WITH IT, and it is what keeps this from being an exemption: **a function
//! that reads the DERIVED value is not a second derivation.** `groupCount`, `runStateNote`,
//! `RUN_STATE_LABEL` and `ActivityPage`'s `extent` all read the grouped result — the very array
//! the views map over — so they cannot disagree with it, and they stay TypeScript where they are
//! read. What moves is everything that reads the WIRE.
//!
//! # The two conversions that could have lied
//!
//! `rowId` builds a string out of a JS NUMBER (`${tsMs}`, `${seq}`), and `disambiguate` appends a
//! count to it. Rust's `format!` is a different function from JS's number → text (`1e21` is
//! `1000000000000000000000` there and `1e+21` here; `-0` is `-0` there and `0` here), so both go
//! through the ENGINE (`Number.prototype.toString`, as `monitors.rs` established). The id is a
//! React list key: two rows that differ only in a spelling would be one row.
//!
//! And the tie-break in `groupOperation`'s sort is `(x.runId ?? "").localeCompare(y.runId ?? "")`
//! — COLLATION, not byte order, and the two disagree the moment a run id carries mixed case
//! (`"Run-1"` sorts after `"run-1"` in `en-US` and before it by code unit). So that is the
//! engine's `String.prototype.localeCompare` too, called with the same empty `locales`/`options`
//! the TypeScript's one-argument call resolves to.
//!
//! # What the corpus measured instead of argued
//!
//! Three differences are named here because they were MEASURED, not because they were assumed
//! unreachable — the corpus (4,278 shapes, 0 diffs) counts each one by a rule:
//!
//!   * **A non-array argument.** The TypeScript ITERATES (`for (const e of events)`), so a
//!     non-array throws `TypeError: … is not iterable`; this guards with `is_array()` and folds
//!     nothing. This is TOTAL where the TypeScript was not — the crate's stated discipline, and a
//!     difference all the same. Unreachable through the wrapper: it takes `OperationEvent[]`.
//!   * **A surrogate pair straddling the 64-unit slice** — see `slice_units`.
//!   * **A field holding a LONE SURROGATE** (ill-formed UTF-16, which only a hand-written body can
//!     produce — the device serializes valid UTF-8). `JsValue::as_string` decodes lossily, so
//!     `"\uD800"` arrives as U+FFFD here and as itself in the TypeScript. Same class as
//!     `monitors.rs`'s throwing getter: the boundary of what a Rust `String` can hold.

use crate::archive::js_trim;
use js_sys::{Array, JsString, Number, Object, Reflect};
use crate::js::{opt_num, prop, type_of};
use wasm_bindgen::prelude::*;

fn put(obj: &JsValue, key: &str, value: &JsValue) -> Result<(), JsValue> {
    Reflect::set(obj, &JsValue::from_str(key), value).map(|_| ())
}

fn opt_str(s: Option<&str>) -> JsValue {
    match s {
        Some(x) => JsValue::from_str(x),
        None => JsValue::NULL,
    }
}

/// JS `String(n)` — the ENGINE's own number → text, so this crate never links Rust's float
/// formatter (see the header; `monitors.rs` carries the same helper, and the crate keeps its
/// helpers per module because each module is one TypeScript file's transliteration).
fn js_number_text(n: f64) -> String {
    Number::from(n)
        .to_string_with_radix(10)
        .map(String::from)
        .unwrap_or_default()
}

/// `value(v)` — a string that holds something, or `null`. The blank test is the JS `trim()`
/// (shared with `archive.rs`, which measured what `str::trim` gets wrong), and the ORIGINAL string
/// is what is kept: the trim only decides whether it counts.
fn value(v: &JsValue) -> Option<String> {
    if type_of(v) != "string" {
        return None;
    }
    let s = v.as_string()?;
    if js_trim(&s).is_empty() {
        None
    } else {
        Some(s)
    }
}

/// `stamp(v)` / `num(v)` — a finite JS number, or absence. The two TypeScript functions are
/// byte-identical apart from their doc comments, so they are one function here. Deliberately not
/// truthiness: `0` is a real exit code and a real duration.
fn finite_num(v: &JsValue) -> Option<f64> {
    if type_of(v) != "number" {
        return None;
    }
    let n = v.as_f64()?;
    if n.is_finite() {
        Some(n)
    } else {
        None
    }
}

/// `strings(v)` — a list of non-blank strings, in the order given. `Array.isArray` is the test, so
/// `{considered: "nope"}` contributes nothing rather than one item.
fn strings(v: &JsValue) -> Vec<String> {
    if !v.is_array() {
        return Vec::new();
    }
    Array::from(v)
        .iter()
        .filter_map(|item| value(&item))
        .collect()
}

/// A JS string array out of a Rust list.
fn str_array(items: &[String]) -> Array {
    let out = Array::new();
    for s in items {
        out.push(&JsValue::from_str(s));
    }
    out
}

/// JS `String.prototype.slice(0, n)` — in UTF-16 CODE UNITS, which is what `.slice` counts.
///
/// THE ONE PLACE THIS IS NOT EXACT, and it is measured rather than hidden: JS's `slice` may cut a
/// surrogate pair in half and keep the lone half, and a lone surrogate cannot exist in a Rust
/// `String` (which is UTF-8). So the boundary is rounded DOWN to a whole character, and the id of a
/// row whose command is longer than 64 units with an emoji straddling unit 64 differs from the
/// TypeScript's by that half-character — measured on `"a"×63 + 🚀 + "b"`, where the TypeScript's id
/// ends `…aaa\ud83d` and this one ends `…aaa`.
///
/// WHAT IT CANNOT CHANGE, which is why this is named rather than bought back at any price: the id
/// is a React list key and is never rendered (`ActivityRow.id`), and the de-duplication rule that
/// reads it is a PREFIX test on both sides — two commands that agree on their first 63 units are
/// one id in JavaScript and one id here, so the same repeats get the same `#2`. The corpus's 18
/// `sliceSplit` shapes are the whole of the difference.
fn slice_units(s: &str, max_units: usize) -> String {
    let mut used = 0usize;
    let mut out = String::new();
    for c in s.chars() {
        let n = c.len_utf16();
        if used + n > max_units {
            break;
        }
        used += n;
        out.push(c);
    }
    out
}

/// A stable identity for one row, so the list does not re-key (and re-mount) on every poll. Built
/// from the record's own fields; never rendered.
fn row_id(
    source: &str,
    session: Option<&str>,
    seq: Option<f64>,
    ts_ms: f64,
    kind: Option<&str>,
    e: &JsValue,
) -> String {
    let where_ = if source == "browser" {
        "b".to_string()
    } else {
        format!(
            "t:{}:{}",
            session.unwrap_or(""),
            seq.map(js_number_text).unwrap_or_default()
        )
    };
    let what = value(&prop(e, "command"))
        .or_else(|| value(&prop(e, "script")))
        .or_else(|| value(&prop(e, "text")))
        .unwrap_or_default();
    format!(
        "{}:{}:{}:{}",
        where_,
        js_number_text(ts_ms),
        kind.unwrap_or(""),
        slice_units(&what, 64)
    )
}

/// One event as a row, and the id it was born with. Every field is the device's own value or
/// `null`; nothing here is inferred from a neighbouring record. THE KEY ORDER IS THE TYPESCRIPT'S —
/// the corpus compares it.
fn row_from_event(e: &JsValue, ts_ms: f64) -> Result<(JsValue, String), JsValue> {
    let source_raw = prop(e, "source");
    let source = if type_of(&source_raw) == "string"
        && source_raw.as_string().as_deref() == Some("browser")
    {
        "browser"
    } else {
        "terminal"
    };
    let kind = value(&prop(e, "kind"));
    let run_id = value(&prop(e, "run_id"));
    let session = if source == "browser" {
        None
    } else {
        value(&prop(e, "session"))
    };
    let seq = if source == "browser" {
        None
    } else {
        finite_num(&prop(e, "seq"))
    };
    let id = row_id(source, session.as_deref(), seq, ts_ms, kind.as_deref(), e);

    let row = Object::new();
    put(&row, "id", &JsValue::from_str(&id))?;
    put(&row, "source", &JsValue::from_str(source))?;
    put(&row, "tsMs", &JsValue::from_f64(ts_ms))?;
    put(&row, "kind", &opt_str(kind.as_deref()))?;
    put(&row, "session", &opt_str(session.as_deref()))?;
    put(&row, "seq", &opt_num(seq))?;
    put(
        &row,
        "command",
        &opt_str(value(&prop(e, "command")).as_deref()),
    )?;
    put(
        &row,
        "script",
        &opt_str(value(&prop(e, "script")).as_deref()),
    )?;
    put(&row, "text", &opt_str(value(&prop(e, "text")).as_deref()))?;
    put(
        &row,
        "status",
        &opt_str(value(&prop(e, "status")).as_deref()),
    )?;
    put(
        &row,
        "exitCode",
        &opt_num(finite_num(&prop(e, "exit_code"))),
    )?;
    put(
        &row,
        "durationMs",
        &opt_num(finite_num(&prop(e, "duration_ms"))),
    )?;
    put(
        &row,
        "intent",
        &opt_str(value(&prop(e, "intent")).as_deref()),
    )?;
    put(
        &row,
        "considered",
        &str_array(&strings(&prop(e, "considered"))),
    )?;
    put(
        &row,
        "planStep",
        &opt_num(finite_num(&prop(e, "plan_step"))),
    )?;
    put(
        &row,
        "screenshots",
        &str_array(&strings(&prop(e, "screenshots"))),
    )?;
    // Exactly what the device wrote: `true` only for a written `true`.
    let timed_out = prop(e, "timed_out");
    put(
        &row,
        "timedOut",
        &JsValue::from_bool(type_of(&timed_out) == "boolean" && timed_out.as_bool() == Some(true)),
    )?;
    put(&row, "runId", &opt_str(run_id.as_deref()))?;
    Ok((row.into(), id))
}

/// `#2`, `#3`, … for a row whose identity is already taken by another row in the same group. Two
/// genuinely identical records exist (a client may run the same script twice inside one
/// millisecond, and the browser feed has no sequence), and React throws away a list with duplicate
/// keys — so a repeat gets a positional suffix instead of a collision.
fn disambiguate(seen: &mut Vec<(String, u32)>, id: String) -> String {
    let n = match seen.iter_mut().find(|(k, _)| *k == id) {
        Some((_, n)) => {
            *n += 1;
            *n
        }
        None => {
            seen.push((id.clone(), 1));
            1
        }
    };
    if n == 1 {
        id
    } else {
        format!("{}#{}", id, js_number_text(f64::from(n)))
    }
}

/// The accumulator behind one group. A Rust struct rather than a JS object because it is never
/// observed — only the groups and rows it produces are.
struct Acc {
    id: Option<String>,
    begin_ts: Option<f64>,
    end_ts: Option<f64>,
    label: Option<String>,
    goal: Option<String>,
    outcome: Option<String>,
    /// Extent of the EVENTS carrying this id, which is what an open run's span is derived from.
    first_ms: Option<f64>,
    last_ms: Option<f64>,
    terminal: f64,
    browser: f64,
    /// The rows, with the stamp they are ordered by and the order they ARRIVED in — `pushRow`
    /// sorts by `ts_ms` alone, and the arrival index is the tie-break that makes that a total
    /// order (see the sort below).
    rows: Vec<(f64, usize, JsValue)>,
}

impl Acc {
    fn new(id: Option<String>) -> Self {
        Acc {
            id,
            begin_ts: None,
            end_ts: None,
            label: None,
            goal: None,
            outcome: None,
            first_ms: None,
            last_ms: None,
            terminal: 0.0,
            browser: 0.0,
            rows: Vec::new(),
        }
    }

    /// The group this accumulator becomes, in the TypeScript's own key order.
    fn group(&self, state: &str) -> Result<JsValue, JsValue> {
        let start_ms = self
            .begin_ts
            .or(self.first_ms)
            .or(self.end_ts)
            .unwrap_or(0.0);
        // A CLOSED run's span ends where the client said it ended; an OPEN run's ends at its newest
        // event. Neither ever reads the wall clock.
        let end_ms = match self.end_ts {
            Some(e) => e,
            None => match self.last_ms {
                Some(l) => l.max(start_ms),
                None => start_ms,
            },
        };
        let rows = Array::new();
        for (_, _, r) in &self.rows {
            rows.push(r);
        }
        let g = Object::new();
        put(&g, "runId", &opt_str(self.id.as_deref()))?;
        put(&g, "state", &JsValue::from_str(state))?;
        put(&g, "label", &opt_str(self.label.as_deref()))?;
        put(&g, "goal", &opt_str(self.goal.as_deref()))?;
        put(&g, "outcome", &opt_str(self.outcome.as_deref()))?;
        put(&g, "startMs", &JsValue::from_f64(start_ms))?;
        put(&g, "endMs", &JsValue::from_f64(end_ms))?;
        put(&g, "terminal", &JsValue::from_f64(self.terminal))?;
        put(&g, "browser", &JsValue::from_f64(self.browser))?;
        put(&g, "rows", &rows)?;
        Ok(g.into())
    }
}

/// ONE STABLE SORT PER GROUP, BY `ts_ms` ALONE — never by `ts`, which the two feeds stamp in
/// different units. The tie-break is arrival order, so two records in the same millisecond do not
/// swap between polls.
///
/// `sort_unstable_by` IS THE SAME ANSWER HERE, and the reason is worth stating: the comparator
/// carries the arrival index as its LAST key, so it is a TOTAL order and an unstable sort has no
/// ties left to break differently. The stable sort's drift-sort machinery is ~1 KB gz of the served
/// artifact, measured, and this is what buys it back.
fn sort_rows(a: &mut Acc) {
    a.rows.sort_unstable_by(|x, y| {
        x.0.partial_cmp(&y.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(x.1.cmp(&y.1))
    });
}

/// `groupOperation(events, boundaries)` — the timeline's events and its run boundaries folded into
/// one group per run, plus the unattributed bucket.
///
/// Order-independent: both inputs may arrive in any order (the hook accumulates them across polls),
/// so extents are computed with min/max rather than by position, and a `run/begin` that arrives
/// after its own events still registers the run.
///
/// `pushRow` sorts a group's rows by `ts_ms` after every push; a single stable sort of the whole
/// list at the end is the same order (each push appends, and a stable sort of an already-sorted
/// list preserves what it had), so that is what happens here.
#[wasm_bindgen]
pub fn group_operation(events: JsValue, boundaries: JsValue) -> Result<JsValue, JsValue> {
    // Insertion order (what JS's `Map` iteration gives, and what the stable sort's ties fall back
    // to) plus key lookup.
    let mut order: Vec<Acc> = Vec::new();
    let mut by_id: Vec<(String, usize)> = Vec::new();
    let mut unattributed = Acc::new(None);
    // Row ids are de-duplicated per group, so the counter is per group — keyed by the group's own
    // id, with the bucket's sentinel spelled exactly as the TypeScript spells it.
    let mut seen_ids: Vec<(String, Vec<(String, u32)>)> = Vec::new();

    if events.is_array() {
        for e in Array::from(&events).iter() {
            // No usable stamp ⇒ the row cannot be placed on the axis, counted into a span, or
            // ordered. Dropped, exactly as the device drops its unstamped half.
            let Some(ts) = finite_num(&prop(&e, "ts_ms")) else {
                continue;
            };
            let id = value(&prop(&e, "run_id"));
            let idx = match &id {
                Some(k) => Some(match by_id.iter().find(|(key, _)| key == k) {
                    Some((_, i)) => *i,
                    None => {
                        order.push(Acc::new(Some(k.clone())));
                        let i = order.len() - 1;
                        by_id.push((k.clone(), i));
                        i
                    }
                }),
                None => None,
            };
            let a = match idx {
                Some(i) => &mut order[i],
                None => &mut unattributed,
            };
            let src = prop(&e, "source");
            if type_of(&src) == "string" && src.as_string().as_deref() == Some("browser") {
                a.browser += 1.0;
            } else {
                a.terminal += 1.0;
            }
            let (row, id) = row_from_event(&e, ts)?;
            let key = a.id.clone().unwrap_or_else(|| "\u{0}unattributed".into());
            let at = match seen_ids.iter().position(|(k, _)| *k == key) {
                Some(i) => i,
                None => {
                    seen_ids.push((key, Vec::new()));
                    seen_ids.len() - 1
                }
            };
            let seen = &mut seen_ids[at].1;
            put(&row, "id", &JsValue::from_str(&disambiguate(seen, id)))?;
            a.rows.push((ts, a.rows.len(), row));
            a.first_ms = Some(match a.first_ms {
                Some(f) => f.min(ts),
                None => ts,
            });
            a.last_ms = Some(match a.last_ms {
                Some(l) => l.max(ts),
                None => ts,
            });
        }
    }

    if boundaries.is_array() {
        for b in Array::from(&boundaries).iter() {
            let Some(ts) = finite_num(&prop(&b, "ts_ms")) else {
                continue;
            };
            let Some(id) = value(&prop(&b, "run_id")) else {
                continue;
            };
            // `accFor` — a boundary for an id nothing has been seen for CREATES the group, even
            // when its kind is neither a begin nor an end.
            let i = match by_id.iter().find(|(k, _)| *k == id) {
                Some((_, i)) => *i,
                None => {
                    order.push(Acc::new(Some(id.clone())));
                    let i = order.len() - 1;
                    by_id.push((id.clone(), i));
                    i
                }
            };
            let a = &mut order[i];
            let kind = value(&prop(&b, "kind"));
            if kind.as_deref() == Some("run/begin") {
                // The EARLIEST begin is the run's identity: a duplicate begin for one id is a
                // client error, and honouring the later one would move the run's start forward
                // while its events stayed put.
                if a.begin_ts.is_none() || ts < a.begin_ts.unwrap_or(f64::INFINITY) {
                    a.begin_ts = Some(ts);
                    a.label = value(&prop(&b, "label"));
                    a.goal = value(&prop(&b, "goal"));
                }
            } else if kind.as_deref() == Some("run/end") {
                // The LATEST end closes it: a repeated `run_end` must not reopen a span that has
                // already been reported as finished.
                if a.end_ts.is_none() || ts > a.end_ts.unwrap_or(f64::NEG_INFINITY) {
                    a.end_ts = Some(ts);
                    a.outcome = value(&prop(&b, "outcome"));
                }
            }
        }
    }

    // One stable sort per group, by `ts_ms` alone — never by `ts`, which the two feeds stamp in
    // different units. The tie-break is arrival order, so two records in the same millisecond do
    // not swap between polls.
    let mut sorted: Vec<(f64, String, usize, JsValue)> = Vec::new();
    for (arrival, a) in order.iter_mut().enumerate() {
        // ONE STABLE SORT PER GROUP, BY `ts_ms` ALONE — never by `ts`, which the two feeds stamp in
        // different units. The tie-break is arrival order, so two records in the same millisecond do
        // not swap between polls.
        //
        // `sort_unstable_by` IS THE SAME ANSWER HERE, and the reason is worth stating: the
        // comparator carries the arrival index as its LAST key, so it is a TOTAL order and an
        // unstable sort has no ties left to break differently. The stable sort's drift-sort
        // machinery is ~1 KB gz of the served artifact, measured, and this is what buys it back.
        sort_rows(a);
        // `closed` first: a `run/end` is the one record that says the run is over, even when its
        // `run/begin` is missing (the device's runs log is capped, so an old begin can be trimmed
        // out from under a recent end).
        let state = if a.end_ts.is_some() {
            "closed"
        } else if a.begin_ts.is_some() {
            "open"
        } else {
            "unregistered"
        };
        let start_ms = a.begin_ts.or(a.first_ms).or(a.end_ts).unwrap_or(0.0);
        sorted.push((
            start_ms,
            a.id.clone().unwrap_or_default(),
            arrival,
            a.group(state)?,
        ));
    }

    // The same total-order trick as the rows: `startMs`, then the collation, then the insertion
    // order the JavaScript's stable sort falls back to — which is `byId`'s own iteration order.
    sorted.sort_unstable_by(|x, y| {
        let d = x.0 - y.0;
        // `x.startMs - y.startMs || localeCompare(...)`: a zero difference (and a NaN one, which a
        // finite stamp cannot produce) falls through to the collation.
        if d != 0.0 && !d.is_nan() {
            return if d < 0.0 {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        // THE ENGINE'S COLLATION, not Rust's byte order — see the header. `localeCompare(other)`
        // and `localeCompare(other, [], {})` are the same call: an empty locale list means the
        // default locale and an empty options object means the default options.
        let this = JsString::from(x.1.as_str());
        this.locale_compare(&y.1, &Array::new(), &Object::new())
            .cmp(&0)
            .then(x.2.cmp(&y.2))
    });

    let runs = Array::new();
    for (_, _, _, g) in &sorted {
        runs.push(g);
    }

    // THE BUCKET IS SORTED TOO — it is a group like any other, and the first version of this port
    // forgot it here while `pushRow` had sorted it in the TypeScript. The panel's own suite caught
    // it on the first run (`orders rows by ts_ms ALONE, whatever order they arrived in`), which is
    // what that test is for.
    sort_rows(&mut unattributed);
    let residue = unattributed.terminal + unattributed.browser;
    let out = Object::new();
    put(&out, "runs", &runs)?;
    if residue > 0.0 {
        put(&out, "unattributed", &unattributed.group("unattributed")?)?;
    } else {
        put(&out, "unattributed", &JsValue::NULL)?;
    }
    Ok(out.into())
}

/// `operationRows(...)` — every row of the timeline, in the same groups `groupOperation` builds,
/// in the order the groups are rendered: "grouped by run, oldest group first, the unattributed
/// bucket last and separate".
///
/// It takes the GROUPS rather than the raw events, because the grouping has exactly one
/// implementation above: the rows a reader sees cannot disagree with the counts a strip shows.
/// The TypeScript wrapper keeps its `(events, boundaries)` signature for its own callers and makes
/// the same two calls this does.
#[wasm_bindgen]
pub fn operation_rows(groups: JsValue) -> Result<JsValue, JsValue> {
    let out = Array::new();
    let runs = prop(&groups, "runs");
    if runs.is_array() {
        for g in Array::from(&runs).iter() {
            let row = Object::new();
            put(&row, "group", &g)?;
            put(&row, "rows", &prop(&g, "rows"))?;
            out.push(&row);
        }
    }
    // `if (groups.unattributed)` — TRUTHINESS, and `is_truthy` is `JsValue`'s own, so it is JS's
    // rule rather than a Rust reading of it.
    let bucket = prop(&groups, "unattributed");
    if bucket.is_truthy() {
        let row = Object::new();
        put(&row, "group", &bucket)?;
        put(&row, "rows", &prop(&bucket, "rows"))?;
        out.push(&row);
    }
    Ok(out.into())
}
