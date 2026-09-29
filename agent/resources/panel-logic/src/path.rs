//! `lib/path.ts` — a session's work as one scannable PATH, and the ONE derivation of how a command
//! ended. Transliterated, both of them.
//!
//! The eleventh family to move (P2), and the largest one: ten exports over 304 lines, of which five
//! are pure functions and one is a table. It is called DURING RENDER (`PathView.tsx`'s `useMemo`, and
//! `attentionSteps` beside it), which under the old lazy seam would have needed its derivation moved
//! to the data boundary first — the panel-logic README named this family as the one that proved the
//! boundary road was the wrong answer. It is also where `cardState` and `stateFromEnd` live, which the
//! command card, the details panel and the trajectory's per-event dot all read, so the two move
//! together or not at all.
//!
//! ── THE SIX THINGS THAT ARE NOT OBVIOUS ─────────────────────────────────────────────────────────
//!
//! **`outputChars` COUNTS UTF-16 CODE UNITS, NOT CHARACTERS.** `e.text?.length ?? 0` is
//! `String.prototype.length`, and a CJK output line contributes 1 per ASCII char and 1 PER UNIT —
//! so a Rust `chars().count()` would undercount every non-BMP character and, for anything with an
//! emoji, misplace the bar. The crate reaches for `encode_utf16().count()` for the same reason
//! `runs.rs` reaches for `find_seq` instead of `str::find`.
//!
//! **THE ENDINGS ARE A TABLE KEYED BY THE GENERATED VOCABULARY, and the vocabulary now has a home
//! here too.** `END_STATE` and `END_LABEL` were `Record<EndReason, …>` over `contract.gen.ts`'s
//! `END_REASONS`, a GENERATED list from `agent/src/vocabulary.rs`. `vocabulary.rs` in this crate now
//! carries the same list and `contract_vocabulary.rs` pins it against the source of truth, so a
//! reason the device adds is a GATE failure here rather than a state that silently renders as
//! `muted`. A reason this build cannot name is `muted` with its own words — never a guess.
//!
//! **`durationMs == null` IS A LOOSE TEST**, so an absent duration and a null one count the same:
//! `durationMs` is `number | null` and a step with neither is "untimed".
//!
//! **THE OWNERSHIP FOLD IS "MOST RECENT EVENT AT OR BEFORE `ts`", not "any event in the window"**: a
//! step belongs to whoever held the keyboard when it STARTED, so a handoff mid-command does not
//! retroactively reassign a command the agent already issued.
//!
//! **THE SORT IS NOT STABLE-CLAIMING, and it is `Array.prototype.sort`'s:** `attentionSteps` orders
//! by `(rank, index)` with both keys unique per step, so the comparator is a total order and the
//! result does not depend on the engine's stability.
//!
//! ── AND THE OUTPUT IS A NEW OBJECT GRAPH, WHICH IS THE POINT ────────────────────────────────────
//!
//! `derivePath` returns `{steps, summary, indexOf}` — plain JSON-shaped objects the view reads. The
//! port builds the same shape, field for field, and the panel's own `PathView.test.tsx` and
//! `TrajectoryView.test.tsx` run UNCHANGED against it, because the signatures did not change.

use crate::js::{prop, type_of};
use crate::vocabulary::{end_reason, EXITED_PREFIX};
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;

/// A session-level status before any command (e.g. "opened") forms the preamble round; it is
/// context, not a step along the path.
const PREAMBLE_ID: &str = "r-pre";

/// The state a step lands in, keyed by the reason the device wrote — order is the TypeScript's
/// because the gate reads these entries by name, not by position, but the pairings are the contract.
const END_STATE: [(&str, &str); 6] = [
    ("marker", "muted"),
    ("idle", "muted"),
    ("timeout", "muted"),
    ("interrupted", "warn"),
    ("backgrounded", "bg"),
    ("closed", "muted"),
];

/// The words, keyed the same way. `backgrounded` is its own STATE rather than `warn` because folding
/// it into "ended badly" made the operator-facing line read "3 interrupted" for three commands that
/// were still legitimately RUNNING — and lit the session's "bad" marker for work nothing had gone
/// wrong with.
const END_LABEL: [(&str, &str); 6] = [
    ("marker", "Ended"),
    ("idle", "Idle"),
    ("timeout", "Timed out"),
    ("interrupted", "Interrupted"),
    ("backgrounded", "Backgrounded"),
    ("closed", "Closed"),
];

/// `stateFromEnd(ended, exitCode, reason)` — THE ONE DERIVATION OF A COMMAND'S STATE.
///
/// `reason` is the STATUS string the trail carries (`backgrounded`, `closed`, `interrupted`,
/// `exited:3`), which is why feeding it through unchanged is what makes the views agree.
#[wasm_bindgen]
pub fn state_from_end(
    ended: JsValue,
    exit_code: JsValue,
    reason: JsValue,
) -> Result<Object, JsValue> {
    if !ended.is_truthy() {
        return Ok(state_row("running", "Running", "running"));
    }
    // `exitCode !== null` is a loose test in the TypeScript, so an ABSENT exit code is a present one
    // here (`undefined !== null` is true) — and both reach the reason table below.
    if !exit_code.is_null() {
        let code = exit_code.as_f64();
        return Ok(if code == Some(0.0) {
            state_row("ok", "Success (exit 0)", "0")
        } else {
            // `${exitCode}` is the ENGINE's number → text: a non-numeric value here is not a number
            // at all, and `1e21` is where Rust's formatter and JavaScript's disagree.
            let text = crate::js::text(&exit_code);
            state_row(
                "fail",
                &format!("Failed (exit {text})"),
                &format!("exit {text}"),
            )
        });
    }
    // `reason && (END_REASONS as readonly string[]).includes(reason)` — the membership test is
    // reached only for a TRUTHY reason, and a truthy reason that is not a string (a number, a
    // boolean, an object) makes `includes` answer false rather than raising: `Array.includes` takes
    // any value.
    if let Some(raw) = truthy_string(&reason) {
        if let Some(named) = end_reason(&raw) {
            return Ok(state_row(lookup(&END_STATE, named), lookup(&END_LABEL, named), named));
        }
    }
    // `reason?.startsWith(EXITED_PREFIX)` — AND THIS ONE RAISES on a value that has no `startsWith`,
    // WHICH IS EVERY NON-NULLISH NON-STRING. Optional chaining short-circuits on `null` and
    // `undefined` and on NOTHING else, so `stateFromEnd(true, null, 0)` throws here exactly as it
    // throws in the TypeScript: `(0).startsWith` is not a function. The first version of this port
    // guarded the call with a TRUTHINESS test and answered "Ended" for `0` and `false` — the two
    // inputs where the TypeScript refuses to answer at all, which is the differential's second
    // finding and the more interesting of the two: a falsy value is not a missing value.
    if !reason.is_null() && !reason.is_undefined() && type_of(&reason) != "string" {
        return Err(throw(&format!(
            "stateFromEnd: the reason is a `{}`, and the `exited:` arm reads it with `startsWith` — so \
             the TypeScript raises `reason?.startsWith is not a function` here. A device that sends a \
             non-string reason is the thing to fix; this crate refuses to invent a word for it.",
            type_of(&reason)
        )));
    }
    // `known` is the reason itself when it carries the vocabulary's prefix, and `null` otherwise.
    let known = reason
        .as_string()
        .filter(|r| r.starts_with(EXITED_PREFIX));
    // `label: known || reason || "Ended"` and `compact: known || reason || "ended"` — TWO `||` CHAINS
    // WITH TWO DIFFERENT DEFAULTS, and they differ only in the letter. A falsy reason makes `known`
    // null and the chain lands on the default; that is why collapsing the two into one string here
    // answered `Ended` where the compact form says `ended`.
    let or_reason = |default: &str| -> String {
        known
            .clone()
            .filter(|k| !k.is_empty())
            .or_else(|| reason.as_string().filter(|r| !r.is_empty()))
            .unwrap_or_else(|| default.to_string())
    };
    Ok(state_row("muted", &or_reason("Ended"), &or_reason("ended")))
}

/// A real JavaScript `Error` carrying the FIX, because a caught failure that is a bare string shows
/// up in a console as `undefined` and says nothing. `wasm_bindgen` throws whatever `Err` holds, so
/// this is what a `catch` in the panel will actually print.
fn throw(message: &str) -> JsValue {
    js_sys::Error::new(message).into()
}

/// A truthy reason, AS A STRING: `reason && typeof reason === "string" ? reason : null`.
fn truthy_string(v: &JsValue) -> Option<String> {
    if !v.is_truthy() {
        return None;
    }
    v.as_string()
}

/// `cardState(card)` — the same derivation, over the command card's own fields. It is a separate
/// export because two callers hold a card and not the three arguments, and folding it into one means
/// every call site builds an object to pass three values.
#[wasm_bindgen]
pub fn card_state(card: JsValue) -> Result<Object, JsValue> {
    state_from_end(
        prop(&card, "ended"),
        prop(&card, "exitCode"),
        prop(&card, "reason"),
    )
}

/// `derivePath(rounds, controlEvents)` — the steps, their summary, and the round id → index map.
///
/// `controlEvents` DEFAULTS to `[]` in the TypeScript, and a `JsValue::UNDEFINED` in is that same
/// empty list — so a caller that omits it gets a path with every step owned by the agent, which is
/// what "no handoff" means.
#[wasm_bindgen]
pub fn derive_path(rounds: JsValue, control_events: JsValue) -> Result<Object, JsValue> {
    let rounds: Array = rounds.dyn_into().map_err(|_| {
        throw("derivePath: the rounds are not an array — it is called with useTrajectory's output, and \
               a non-array here means a caller built one.")
    })?;
    let timeline = ownership_timeline(&control_events);

    let steps = Array::new();
    let index_of = Object::new();
    let mut n = 0usize;

    for round in rounds.iter() {
        let id = prop(&round, "id");
        let id_text = id.as_string().unwrap_or_default();
        if id_text == PREAMBLE_ID || prop(&round, "startSeq").is_null() {
            continue;
        }
        let start = start_event(&round)?;
        let st = state_from_end(
            prop(&round, "ended"),
            prop(&round, "exitCode"),
            prop(&round, "reason"),
        )?;

        crate::js::put(
            &index_of,
            &id_text,
            &JsValue::from_f64(n as f64),
        )?;
        n += 1;

        let step = Object::new();
        crate::js::put(&step, "id", &id)?;
        crate::js::put(&step, "index", &JsValue::from_f64(n as f64))?;
        crate::js::put(&step, "command", &prop(&round, "command"))?;
        crate::js::put(&step, "owner", &JsValue::from_str(owner_at(&timeline, ts(&round, "startTs"))))?;
        crate::js::put(&step, "state", &state_of(&st))?;
        crate::js::put(&step, "stateLabel", &Reflect::get(&st, &JsValue::from_str("compact")).unwrap_or(JsValue::UNDEFINED))?;
        crate::js::put(&step, "startedAt", &prop(&round, "startTs"))?;
        crate::js::put(&step, "durationMs", &prop(&round, "durationMs"))?;
        crate::js::put(&step, "exitCode", &prop(&round, "exitCode"))?;
        crate::js::put(&step, "reason", &prop(&round, "reason"))?;
        crate::js::put(&step, "outputChars", &JsValue::from_f64(output_chars(&round)?))?;
        crate::js::put(&step, "intent", &opt_text(&start, "intent"))?;
        crate::js::put(&step, "considered", &considered(&start))?;
        crate::js::put(&step, "planStep", &plan_step(&start))?;
        crate::js::put(&step, "runId", &run_id(&start))?;
        steps.push(&step);
    }

    let out = Object::new();
    crate::js::put(&out, "steps", &steps)?;
    crate::js::put(&out, "summary", &summarize_path(steps.clone().into()))?;
    crate::js::put(&out, "indexOf", &index_of)?;
    Ok(out)
}

/// `summarizePath(steps)` — how much work, how much of it failed, and how long it took.
///
/// `commandMs` is a FLOOR, not a total: a backgrounded or still-running step has no duration, so
/// `untimed` counts them and the view says "at least" instead of implying a total it cannot know.
#[wasm_bindgen]
pub fn summarize_path(steps: JsValue) -> Object {
    let list: Array = Array::from(&steps);
    // A `Map`, not a fixed array, and the reason is the JavaScript's own arithmetic: `counts[s.state]
    // += 1` on a state the record does not declare CREATES the key and stores `undefined + 1`, which
    // is `NaN`. The differential found this on a hand-made step carrying `state: "unknown"`. Dropping
    // the state (what the first version did) is not the same answer — one is a count that says "not
    // available", the other is no count at all, and a summary that silently omits a state is the
    // shape the crate exists to prevent.
    let mut counts: Vec<(String, f64)> = ["running", "ok", "fail", "warn", "bg", "muted"]
        .iter()
        .map(|s| (s.to_string(), 0.0))
        .collect();
    let mut human_steps = 0f64;
    let mut command_ms = 0f64;
    let mut untimed = 0f64;
    let mut first_start = f64::INFINITY;
    let mut last_end = f64::NEG_INFINITY;

    for step in list.iter() {
        bump(&mut counts, prop(&step, "state").as_string().unwrap_or_default());
        if prop(&step, "owner").as_string().as_deref() == Some("human") {
            human_steps += 1.0;
        }
        let duration = prop(&step, "durationMs");
        // `s.durationMs == null` — LOOSE, so absent and null are the same case, and a duration of
        // ZERO is a duration.
        if duration.is_null() || duration.is_undefined() {
            untimed += 1.0;
        } else if let Some(ms) = duration.as_f64() {
            command_ms += ms;
            last_end = last_end.max(ts(&step, "startedAt") * 1000.0 + ms);
        }
        first_start = first_start.min(ts(&step, "startedAt") * 1000.0);
    }

    let counts_obj = Object::new();
    for (state, n) in &counts {
        let _ = crate::js::put(&counts_obj, state, &JsValue::from_f64(*n));
    }

    let out = Object::new();
    let _ = crate::js::put(&out, "steps", &JsValue::from_f64(list.length() as f64));
    let _ = crate::js::put(&out, "counts", &counts_obj);
    let _ = crate::js::put(&out, "commandMs", &JsValue::from_f64(command_ms));
    let _ = crate::js::put(&out, "untimed", &JsValue::from_f64(untimed));
    let _ = crate::js::put(&out, "humanSteps", &JsValue::from_f64(human_steps));
    let _ = crate::js::put(
        &out,
        "spanMs",
        &if last_end > f64::NEG_INFINITY && first_start < f64::INFINITY {
            JsValue::from_f64(last_end - first_start)
        } else {
            JsValue::NULL
        },
    );
    let running = counts
        .iter()
        .find(|(k, _)| k == "running")
        .map(|(_, n)| *n)
        .unwrap_or(0.0);
    let _ = crate::js::put(&out, "live", &JsValue::from_bool(running > 0.0));
    out
}

/// `attentionSteps(steps)` — worth a second look, worst first.
///
/// `bg` ranks with `running`: both are "not finished", and neither is a problem to draw the eye. The
/// order is `(rank, index)`, and both keys are unique per step, so the comparator is a total order
/// and the sort does not depend on the engine's stability.
#[wasm_bindgen]
pub fn attention_steps(steps: JsValue) -> Array {
    const RANK: [(&str, f64); 6] = [
        ("fail", 0.0),
        ("warn", 1.0),
        ("running", 2.0),
        ("bg", 2.0),
        ("muted", 3.0),
        ("ok", 4.0),
    ];
    let list = Array::from(&steps);
    let mut kept: Vec<(f64, f64, JsValue)> = Vec::new();
    for step in list.iter() {
        let state = prop(&step, "state").as_string().unwrap_or_default();
        if state != "fail" && state != "warn" && state != "running" {
            continue;
        }
        let rank = RANK.iter().find(|(k, _)| *k == state).map(|(_, r)| *r).unwrap_or(9.0);
        let index = prop(&step, "index").as_f64().unwrap_or(f64::NAN);
        kept.push((rank, index, step));
    }
    kept.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    let out = Array::new();
    for (_, _, step) in kept {
        out.push(&step);
    }
    out
}

/// The state row `{state, label, compact}`, in that FIELD ORDER — the object spread order is what a
/// `JSON.stringify` reads, and this crate's parsers are compared on values, not on key order.
fn state_row(state: &str, label: &str, compact: &str) -> Object {
    let o = Object::new();
    let _ = crate::js::put(&o, "state", &JsValue::from_str(state));
    let _ = crate::js::put(&o, "label", &JsValue::from_str(label));
    let _ = crate::js::put(&o, "compact", &JsValue::from_str(compact));
    o
}

fn lookup(table: &[(&'static str, &'static str)], key: &str) -> &'static str {
    table
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or("muted")
}

/// The `state` field of a `stateFromEnd` row, read back off the object the same functions built —
/// a second derivation of it would be the copy this module exists to delete.
fn state_of(row: &Object) -> JsValue {
    Reflect::get(row, &JsValue::from_str("state")).unwrap_or(JsValue::UNDEFINED)
}

/// A numeric field of a round/step, read as the engine's own number.
fn ts(v: &JsValue, key: &str) -> f64 {
    prop(v, key).as_f64().unwrap_or(f64::NAN)
}

/// `r.events.find((e) => e.kind === "command/start")` — the reasoning rides the round's own
/// command/start event, so a step and its reason arrive together or not at all.
fn start_event(round: &JsValue) -> Result<JsValue, JsValue> {
    let events = prop(round, "events");
    if !events.is_array() {
        return Err(throw(&format!(
            "derivePath: a round's `events` is a `{}`, and the path reads it with `find`/`reduce` — so \
             the TypeScript raises here. A round without an event list is not a round.",
            type_of(&events)
        )));
    }
    Ok(Array::from(&events)
        .iter()
        .find(|e| prop(e, "kind").as_string().as_deref() == Some("command/start"))
        .unwrap_or(JsValue::UNDEFINED))
}

/// The output character count of a round, in UTF-16 code units — see the module header.
fn output_chars(round: &JsValue) -> Result<f64, JsValue> {
    let events = prop(round, "events");
    if !events.is_array() {
        return Err(throw("derivePath: a round's `events` is not an array — see the note in `start_event`."));
    }
    Ok(Array::from(&events).iter().fold(0f64, |n, e| {
        if prop(&e, "kind").as_string().as_deref() != Some("output") {
            return n;
        }
        let text = prop(&e, "text");
        if text.is_null() || text.is_undefined() {
            n
        } else {
            n + text.as_string().map(|s| s.encode_utf16().count()).unwrap_or(0) as f64
        }
    }))
}

/// `start?.intent ?? null` — a string field, `null` when absent.
fn opt_text(start: &JsValue, key: &str) -> JsValue {
    let v = prop(start, key);
    if v.is_null() || v.is_undefined() {
        JsValue::NULL
    } else {
        v
    }
}

/// `Array.isArray(start?.considered) ? start.considered : []` — anything else is an empty list, and a
/// fresh one per step (the TypeScript's `[]` literal is inside the map).
fn considered(start: &JsValue) -> JsValue {
    let v = prop(start, "considered");
    if v.is_array() {
        v
    } else {
        Array::new().into()
    }
}

/// `typeof start?.plan_step === "number" && start.plan_step > 0 ? start.plan_step : null` — a TYPE
/// test, so a `"3"` from the wire is not a plan step, and zero is not one either.
fn plan_step(start: &JsValue) -> JsValue {
    let v = prop(start, "plan_step");
    if type_of(&v) == "number" && v.as_f64().unwrap_or(0.0) > 0.0 {
        v
    } else {
        JsValue::NULL
    }
}

/// `typeof start?.run_id === "string" && start.run_id.trim() !== "" ? start.run_id : null` — blank is
/// ABSENT, not a run named "", the same rule the device applies when it writes the field.
fn run_id(start: &JsValue) -> JsValue {
    let v = prop(start, "run_id");
    if v.is_null() || v.is_undefined() || type_of(&v) != "string" {
        return JsValue::NULL;
    }
    match v.as_string() {
        Some(s) if !trim_js(&s).is_empty() => v,
        _ => JsValue::NULL,
    }
}

/// `String.prototype.trim` — which removes the WHOLE Unicode whitespace set, and the crate's own
/// `str::trim` is close enough that this is the one place the note belongs: the fields here are
/// wire strings from the agent, and both agree on every character the agent writes.
fn trim_js(s: &str) -> String {
    s.trim().to_string()
}

/// `events.filter(e => e.kind === "control" && (e.status === "human" || e.status === "ai"))`, mapped
/// and sorted by `ts` — the fold is "most recent event at or before ts", so a handoff mid-command
/// does not retroactively reassign a command the agent already issued. `"ai"` before any handoff,
/// because a session with no control event was the agent's throughout.
fn ownership_timeline(events: &JsValue) -> Vec<(f64, &'static str)> {
    if !events.is_array() {
        return Vec::new();
    }
    let mut out: Vec<(f64, &'static str)> = Array::from(events)
        .iter()
        .filter_map(|e| {
            if prop(&e, "kind").as_string().as_deref() != Some("control") {
                return None;
            }
            let holder = match prop(&e, "status").as_string().as_deref() {
                Some("human") => "human",
                Some("ai") => "ai",
                _ => return None,
            };
            Some((prop(&e, "ts").as_f64().unwrap_or(f64::NAN), holder))
        })
        .collect();
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// The holder in effect at `ts`; `"ai"` before any handoff.
fn owner_at(timeline: &[(f64, &'static str)], ts: f64) -> &'static str {
    let mut holder = "ai";
    for (at, who) in timeline {
        if *at > ts {
            break;
        }
        holder = who;
    }
    holder
}

/// `counts[state] += 1` — an unknown state GAINS a key whose value is `NaN`, because that is what
/// `undefined + 1` is. See the note at the fold.
fn bump(counts: &mut Vec<(String, f64)>, state: String) {
    if let Some(slot) = counts.iter_mut().find(|(k, _)| *k == state) {
        slot.1 += 1.0;
    } else {
        counts.push((state, f64::NAN));
    }
}
