//! The audit event stream's TWO GROUPINGS, and the two rules they share — transliterated.
//!
//! The twelfth family to move (P2), and the one P2's own turning-point section wrote off: it lists
//! `terminalStatus` as stuck because it is called INSIDE A SYNCHRONOUS FOLD (`cards.push(finishCard(…
//! term.exitCode …))`), and "to await it you would have to make the whole fold async". **That
//! objection was an artifact of the lazy seam.** The module is loaded before the first render, so a
//! synchronous fold calls a synchronous function and the argument is simply gone.
//!
//! Three functions, and the reason they are ONE family rather than three:
//!
//!   * `terminalStatus` — the round-99/100 marker rule: `backgrounded` / `closed` / `exited:<n>`.
//!     Both groupings read it, and `derivePath`'s crate-side table is keyed on the same vocabulary.
//!   * `groupEvents` — events → command CARDS (the panel's live trail).
//!   * `groupRounds` — events → TRAJECTORY ROUNDS (the panel's path input).
//!
//! `groupRounds` is `derivePath`'s input and `derivePath` is already in this crate, so moving it
//! makes the whole session-timeline derivation one language: events in, cards and rounds and a path
//! out, with no JavaScript object in the middle.
//!
//! ── THE FOUR THINGS A TRANSLITERATION GETS WRONG HERE ──────────────────────────────────────────
//!
//! **THE OUTPUT TAIL IS Sliced IN UTF-16 CODE UNITS.** `output.slice(-MAX_OUTPUT_CHARS)` is
//! `String.prototype.slice`, which indexes UTF-16 units — so the tail is taken at a unit boundary
//! that may be MID-SURROGATE-PAIR, and the truncation mark is prepended in front of it. `runs.rs` has
//! the head-side helper (`slice_units`); this is the tail, and it is written the same way.
//!
//! **ANSI STRIPPING IS A HAND SCANNER, NOT A REGEX.** `stripAnsi` is four alternatives
//! (`CSI`, `OSC`, `DCS`, `SINGLE`) plus a sweep for stray ESC — and a port that reached for Rust's
//! `regex` would link a general engine for a 30-character problem, which is the same lesson
//! `attention.rs` records for `str::find` (10,692 raw / 5,431 gz for six substring searches). The
//! scanner below is the four rules in order, over bytes, and `runs.rs`'s `find_seq` is the precedent.
//!
//! **`Number(st.slice("exited:".length))` IS THE ENGINE'S ToNumber**, so `"exited:abc"` is `NaN` and
//! `Number.isFinite(NaN)` is false — a reason with NO exit code, not one with a zero.
//!
//! **A `command/start` WHILE THE PREVIOUS COMMAND NEVER ENDED closes it as `interrupted`** in the
//! cards and SEALS IT AS-IS in the rounds (the raw view: what the log says). The two groupings
//! disagree on purpose, and a port that made them agree would be a bug in the port.
//!
//! ── THE ONE PLACE THIS PORT IS NARROWER, AND IT IS A REFUSAL ─────────────────────────────────────
//!
//! `for (const ev of events)` accepts any ITERABLE, so `groupEvents("abc")` walks the string's three
//! characters, finds no `command/start` among them, and answers **`[]`** — which is a CLAIM: "this
//! session ran nothing". `groupRounds` on the same input seals a preamble round built from the letter
//! `a`. Both are the shape of bug AGENTS.md is about: a successful read of an unreadable thing
//! reported as a successful read of an empty one. **This port refuses a non-array**, with a message
//! that says what is wrong, and a nullish ELEMENT inside a real array likewise — `null.kind` raises
//! in the TypeScript, and a hole in the stream is a thing to fix upstream. Measured, both directions,
//! on the corpus in the commit: 117 agree, 10 throw on both sides, 2 are this narrowing.

use crate::js::{prop, text, type_of};
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;

/// `MAX_OUTPUT_CHARS` — a card's output is tail-capped at a million UTF-16 units.
const MAX_OUTPUT_CHARS: usize = 1_000_000;
/// The mark that says the cap took effect. `finishCard`'s own string, byte for byte.
const TRUNC_MARK: &str = "\n…[output truncated — older lines dropped]…\n";

/// `terminalStatus(st)` — the marker rule, or nothing for a status that ends nothing.
///
/// `exited:<n>` is the one that carries a code, and the code is the DEVICE's: a non-numeric tail is
/// `NaN`, `Number.isFinite` says no, and the answer is a reason with no exit code.
#[wasm_bindgen]
pub fn terminal_status(status: &str) -> Option<Object> {
    if status == "backgrounded" || status == "closed" {
        return Some(signal(None, status));
    }
    let tail = status.strip_prefix(EXITED_PREFIX)?;
    let code = to_number(tail);
    let code = if code.is_finite() { Some(code) } else { None };
    Some(signal(code, status))
}

/// `{exitCode, reason}` — and the exit code is `null`, not `undefined`, because the TypeScript's
/// type is `number | null` and a `JSON.stringify` of `undefined` drops the key.
fn signal(exit_code: Option<f64>, reason: &str) -> Object {
    let o = Object::new();
    let _ = crate::js::put(
        &o,
        "exitCode",
        &match exit_code {
            Some(c) => JsValue::from_f64(c),
            None => JsValue::NULL,
        },
    );
    let _ = crate::js::put(&o, "reason", &JsValue::from_str(reason));
    o
}

/// `Number(v)` — THE ENGINE'S OWN COERCION, for `Number(st.slice(7))`.
///
/// **NOT `str::parse`, AND THE DIFFERENCE IS THE ANSWER TWICE OVER** — the differential found both:
///
///   * `Number("")` is **0** and `Number("  ")` is 0, so `exited:` is an exit code of ZERO and the
///     JavaScript says so. `str::parse::<f64>` rejects both and answers `NaN` → no code at all, which
///     is a DIFFERENT FACT: "exited with no code" versus "exited with code zero".
///   * `Number("0x10")` is **16** — the string-numeric grammar carries hex, binary and octal — and
///     `str::parse` rejects it. A device that writes a hex exit code gets a reason with no code here
///     and a code there.
///
/// `f64::from_str` covers what is left (`inf`, `infinity`, `nan`, exponents, signs) and is
/// case-insensitive on the words exactly as `Number` is.
fn to_number(v: &str) -> f64 {
    let t = v.trim();
    if t.is_empty() {
        // `ToNumber("")` is +0 — and it is the one coercion `str::parse` cannot express.
        return 0.0;
    }
    let (neg, digits) = match t.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, t.strip_prefix('+').unwrap_or(t).trim_start()),
    };
    let magnitude = if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).map(|n| n as f64).unwrap_or(f64::NAN)
    } else if let Some(bin) = digits.strip_prefix("0b").or_else(|| digits.strip_prefix("0B")) {
        i64::from_str_radix(bin, 2).map(|n| n as f64).unwrap_or(f64::NAN)
    } else if let Some(oct) = digits.strip_prefix("0o").or_else(|| digits.strip_prefix("0O")) {
        i64::from_str_radix(oct, 8).map(|n| n as f64).unwrap_or(f64::NAN)
    } else {
        digits.parse::<f64>().unwrap_or(f64::NAN)
    };
    if neg { -magnitude } else { magnitude }
}

/// The `exited:` prefix, from the crate's own vocabulary.
const EXITED_PREFIX: &str = "exited:";

/// `groupEvents(events)` — the live trail's CARDS.
///
/// A `command/start` while the previous command never ended closes it as `interrupted`, so a
/// mid-stream start cannot orphan a card that would otherwise read "running" forever.
#[wasm_bindgen]
pub fn group_events(events: JsValue) -> Result<Array, JsValue> {
    let list = events_array(&events, "groupEvents")?;
    let mut cards = Array::new();
    let mut start = JsValue::UNDEFINED;
    let mut outputs: Vec<String> = Vec::new();

    for ev in list.iter() {
        match prop(&ev, "kind").as_string().as_deref() {
            Some("command/start") => {
                if !start.is_undefined() {
                    let card = finish_card(
                        &start,
                        &outputs,
                        true,
                        &JsValue::NULL,
                        &JsValue::from_str("interrupted"),
                        &JsValue::NULL,
                    );
                    cards.push(&card);
                }
                start = ev;
                outputs.clear();
            }
            Some("output") => {
                // `if (start && ev.text)` — the text is TRUTHINESS-tested, so an empty output
                // frame is not collected, and it is collected only while a command is open.
                if !start.is_undefined() && prop(&ev, "text").is_truthy() {
                    if let Some(t) = prop(&ev, "text").as_string() {
                        outputs.push(t);
                    }
                }
            }
            Some("command/end") => {
                if !start.is_undefined() {
                    let card = finish_card(
                        &start,
                        &outputs,
                        true,
                        &nullish(&ev, "exit_code"),
                        &nullish(&ev, "reason"),
                        &nullish(&ev, "duration_ms"),
                    );
                    cards.push(&card);
                    start = JsValue::UNDEFINED;
                    outputs.clear();
                }
            }
            Some("status") => {
                let status = prop(&ev, "status");
                if start.is_undefined() || !status.is_truthy() {
                    continue;
                }
                let Some(status_text) = status.as_string() else { continue };
                let Some(term) = terminal_status(&status_text) else { continue };
                // A STATUS end carries no duration_ms: it is derived from the marker's ts minus the
                // start's, in ms (round-58's unit).
                let start_ts = prop(&start, "ts").as_f64().unwrap_or(f64::NAN);
                let end_ts = prop(&ev, "ts").as_f64().unwrap_or(f64::NAN);
                let card = finish_card(
                    &start,
                    &outputs,
                    true,
                    &Reflect::get(&term, &JsValue::from_str("exitCode")).unwrap_or(JsValue::NULL),
                    &Reflect::get(&term, &JsValue::from_str("reason")).unwrap_or(JsValue::NULL),
                    &JsValue::from_f64((end_ts - start_ts) * 1000.0),
                );
                cards.push(&card);
                start = JsValue::UNDEFINED;
                outputs.clear();
            }
            _ => {}
        }
    }
    // A trailing start with no end is a LIVE card — still running, or the agent died before recovery
    // appended its own `interrupted`.
    if !start.is_undefined() {
        let card = finish_card(
            &start,
            &outputs,
            false,
            &JsValue::NULL,
            &JsValue::NULL,
            &JsValue::NULL,
        );
        cards.push(&card);
    }
    Ok(cards)
}

/// `groupRounds(events)` — the trajectory's ROUNDS, which are `derivePath`'s input.
///
/// A round is ENDED by a `command/end` OR by a terminal status, and **the LAST marker in the round
/// wins** — a backgrounded command can later log `closed`. A superseded round is sealed AS-IS (the
/// raw view: what the log says), which is where this deliberately disagrees with `groupEvents`.
#[wasm_bindgen]
pub fn group_rounds(events: JsValue) -> Result<Array, JsValue> {
    let list = events_array(&events, "groupRounds")?;
    let mut rounds = Array::new();
    let mut pre: Vec<JsValue> = Vec::new();
    let mut cur: Option<Vec<JsValue>> = None;

    for ev in list.iter() {
        if prop(&ev, "kind").as_string().as_deref() == Some("command/start") {
            if let Some(open) = cur.take() {
                rounds.push(&seal(&open));
            } else if !pre.is_empty() {
                rounds.push(&seal(&pre));
                pre.clear();
            }
            cur = Some(vec![ev]);
        } else if let Some(open) = cur.as_mut() {
            open.push(ev);
        } else {
            pre.push(ev);
        }
    }
    if let Some(open) = cur {
        rounds.push(&seal(&open));
    } else if !pre.is_empty() {
        rounds.push(&seal(&pre));
    }
    Ok(rounds)
}

/// One round, from its events — the first event opens it, and the markers close it.
fn seal(evs: &[JsValue]) -> Object {
    let first = &evs[0];
    let is_cmd = prop(first, "kind").as_string().as_deref() == Some("command/start");
    let mut ended = false;
    let mut exit_code = JsValue::NULL;
    let mut reason = JsValue::NULL;
    let mut duration_ms = JsValue::NULL;
    let mut end_ts: Option<f64> = None;

    for ev in evs {
        match prop(ev, "kind").as_string().as_deref() {
            Some("command/end") => {
                ended = true;
                exit_code = nullish(ev, "exit_code");
                reason = nullish(ev, "reason");
                // `ev.duration_ms != null ? ev.duration_ms : null` — the LOOSE test, so an absent
                // duration and a null one are the SAME case, and both become `null` here. The first
                // version kept an ABSENT field as `undefined`, which then read as "has a duration"
                // to the derivation below and every such round came back with no duration at all —
                // the differential caught it as `durationMs` missing on a round that ended with an
                // explicit `command/end`.
                duration_ms = nullish(ev, "duration_ms");
                end_ts = prop(ev, "ts").as_f64();
            }
            Some("status") => {
                let status = prop(ev, "status");
                if !status.is_truthy() {
                    continue;
                }
                let Some(status_text) = status.as_string() else { continue };
                let Some(term) = terminal_status(&status_text) else { continue };
                ended = true;
                exit_code = Reflect::get(&term, &JsValue::from_str("exitCode")).unwrap_or(JsValue::NULL);
                reason = Reflect::get(&term, &JsValue::from_str("reason")).unwrap_or(JsValue::NULL);
                end_ts = prop(ev, "ts").as_f64();
            }
            _ => {}
        }
    }
    // A STATUS-ended round carries no duration_ms: derive it from the marker's ts minus the round's
    // start, in ms. `ended && durationMs == null && endTs != null` — all three, and the first is
    // what stops a live round from being given a duration of zero.
    if ended && duration_ms.is_null() {
        if let Some(end) = end_ts {
            duration_ms = JsValue::from_f64((end - prop(first, "ts").as_f64().unwrap_or(f64::NAN)) * 1000.0);
        }
    }

    let round = Object::new();
    // `isCmd ? \`r-${first.seq}\` : "r-pre"` — the id is the ENGINE's number→text, and `r-pre` is
    // the preamble round (a session-level status before any command, which is context, not a step).
    let _ = crate::js::put(
        &round,
        "id",
        &if is_cmd {
            JsValue::from_str(&format!("r-{}", text(&prop(first, "seq"))))
        } else {
            JsValue::from_str("r-pre")
        },
    );
    let _ = crate::js::put(
        &round,
        "startSeq",
        &if is_cmd { prop(first, "seq") } else { JsValue::NULL },
    );
    let _ = crate::js::put(
        &round,
        "command",
        &if is_cmd {
            let c = prop(first, "command");
            if c.is_null() || c.is_undefined() {
                JsValue::from_str("")
            } else {
                c
            }
        } else {
            JsValue::from_str("(session)")
        },
    );
    let _ = crate::js::put(&round, "startTs", &prop(first, "ts"));
    let ev_array: Array = Array::new();
    for e in evs {
        ev_array.push(e);
    }
    let _ = crate::js::put(&round, "events", ev_array.as_ref());
    let _ = crate::js::put(&round, "ended", &JsValue::from_bool(ended));
    let _ = crate::js::put(&round, "exitCode", &exit_code);
    let _ = crate::js::put(&round, "reason", &reason);
    let _ = crate::js::put(&round, "durationMs", &duration_ms);
    round
}

/// `finishCard(start, outputs, ended, exitCode, reason, durationMs)` — one card.
///
/// The output is the JOINED text with the ANSI stripped and the tail capped, and both of those are
/// UTF-16 operations on the JavaScript side (see the module header).
fn finish_card(
    start: &JsValue,
    outputs: &[String],
    ended: bool,
    exit_code: &JsValue,
    reason: &JsValue,
    duration_ms: &JsValue,
) -> Object {
    let joined = outputs.concat();
    let mut output = strip_ansi_str(&joined);
    if output.encode_utf16().count() > MAX_OUTPUT_CHARS {
        output = format!("{TRUNC_MARK}{}", tail_units(&output, MAX_OUTPUT_CHARS));
    }
    let card = Object::new();
    let _ = crate::js::put(
        &card,
        "id",
        &JsValue::from_str(&format!("c-{}", text(&prop(start, "seq")))),
    );
    let _ = crate::js::put(&card, "seq", &prop(start, "seq"));
    // `start.command ?? ""` — an absent or nullish command is the EMPTY STRING, so the key is always
    // there. A card whose `command` key vanished is a card the panel cannot render a title for.
    let command = prop(start, "command");
    let _ = crate::js::put(
        &card,
        "command",
        &if command.is_null() || command.is_undefined() {
            JsValue::from_str("")
        } else {
            command
        },
    );
    let _ = crate::js::put(&card, "output", &JsValue::from_str(&output));
    let _ = crate::js::put(&card, "startedAt", &prop(start, "ts"));
    let _ = crate::js::put(&card, "ended", &JsValue::from_bool(ended));
    let _ = crate::js::put(&card, "exitCode", &exit_code);
    let _ = crate::js::put(&card, "reason", &reason);
    let _ = crate::js::put(&card, "durationMs", &duration_ms);
    card
}

/// `ev.x ?? null` — the nullish coalesce every field of this family reads its optional values with,
/// and the reason it is not `|| null`: an empty string and a zero SURVIVE here.
fn nullish(v: &JsValue, key: &str) -> JsValue {
    let got = prop(v, key);
    if got.is_null() || got.is_undefined() {
        JsValue::NULL
    } else {
        got
    }
}

/// The events, as a list — and a non-array is a THROW, because `for (const ev of events)` raises on
/// one and a grouping that answered `[]` would say "this session ran nothing".
fn events_array(events: &JsValue, who: &str) -> Result<Array, JsValue> {
    if events.is_array() {
        let list = Array::from(events);
        for (i, ev) in list.iter().enumerate() {
            if ev.is_null() || ev.is_undefined() {
                return Err(js_sys::Error::new(&format!(
                    "{who}: event {i} is null — every event is read for its `kind`, and the \
                     TypeScript raises on a nullish element. A hole in the stream is a thing to fix \
                     upstream, not to answer with an empty timeline."
                ))
                .into());
            }
        }
        return Ok(list);
    }
    Err(js_sys::Error::new(&format!(
        "{who}: the events are a `{}` and both groupings iterate them — the TypeScript raises here. \\
         An empty answer would say the session recorded nothing.",
        type_of(events)
    ))
    .into())
}

/// `output.slice(-n)` — the LAST `n` UTF-16 units, cut at a unit boundary.
fn tail_units(s: &str, max_units: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    if units.len() <= max_units {
        return s.to_string();
    }
    // `String::from_utf16_lossy` on a tail that starts mid-surrogate answers U+FFFD, which is
    // EXACTLY what the JavaScript's own `slice` hands the DOM: the low half of a pair with no high
    // half is not a character, and the browser's renderer substitutes rather than drops.
    String::from_utf16_lossy(&units[units.len() - max_units..])
}

/// `stripAnsi(s)` — the four escape rules, then a sweep for any ESC that survived.
///
/// THE SCAN, IN THE ORDER THE REGEX ALTERNATION READS:
///
///   1. `CSI`     ESC `[` [0-9;?]* [ -/]* [@-~]   — SGR colours, cursor moves, `\x1b[2J`
///   2. `OSC`     ESC `]` [^BEL ESC]* (BEL | ESC `\` | end-of-input) — titles, `]133;D;`
///   3. `DCS`     ESC `[P^_] … ESC `\`  — device-control strings
///   4. `SINGLE`  ESC [ `=` `>` 7 8 6 M N O c ]  — the one-byte escapes
///
/// and then every remaining ESC is dropped, so a control byte can never reach the DOM as text.
#[wasm_bindgen]
pub fn strip_ansi(input: JsValue) -> String {
    match input.as_string() {
        Some(s) => strip_ansi_str(&s),
        // `(s ?? "")` — an absent or nullish text is the empty string, which the JavaScript's own
        // `??` answers before the replace ever runs.
        None => String::new(),
    }
}

/// The scanner, over a `&str` — the wasm export is the boundary, this is the work.
fn strip_ansi_str(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < b.len() {
        if b[i] != 0x1b {
            // SAFETY-free: copy the whole UTF-8 char at once, so a multi-byte sequence is never
            // split by the byte scan below.
            let start = i;
            i += utf8_len(b[i]);
            out.push_str(&s[start..i]);
            continue;
        }
        match escape_end(b, i) {
            Some(next) => i = next,
            None => i += 1, // an ESC that starts no rule is dropped by the sweep
        }
    }
    out
}

/// The tail of a DCS: anything (LAZY, so the FIRST ST after it wins), then ESC `\`.
///
/// An unterminated one does not match — the regex's `.*?` cannot reach an ST that is not there — so
/// the rule FAILS and the stray ESC is dropped by the sweep instead of swallowing the rest of the
/// text.
fn dcs(b: &[u8], from: usize) -> Option<usize> {
    let mut j = from;
    while j + 1 < b.len() {
        if b[j] == 0x1b && b[j + 1] == b'\\' {
            return Some(j + 2);
        }
        j += 1;
    }
    None
}

/// The byte AFTER the escape sequence starting at `start`, or `None` when it starts none.
fn escape_end(b: &[u8], start: usize) -> Option<usize> {
    let esc = b[start];
    debug_assert_eq!(esc, 0x1b);
    let next = *b.get(start + 1)?;
    match next {
        // 1. CSI, and 3. DCS — THE TWO SHARE THE `[`, so a regex alternation BACKTRACKS between them
        //    and this arm has to as well: try CSI, and only if it does not match try DCS.
        b'[' => {
            // CSI: ESC `[` [0-9;?]* [ -/]* [@-~]
            let mut i = start + 2;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b';' || b[i] == b'?') {
                i += 1;
            }
            while i < b.len() && (0x20..=0x2f).contains(&b[i]) {
                i += 1;
            }
            if let Some(c) = b.get(i) {
                if (0x40..=0x7e).contains(c) {
                    return Some(i + 1);
                }
            }
            dcs(b, start + 2)
        }
        // 3. DCS WHOSE INTRODUCER IS THE CLASS MEMBER ITSELF — `ESC P` is the ordinary DCS
        //    introducer, and the class is `[P^_]`, so `ESC Ptmux; ESC \` is the sequence the
        //    TypeScript removes. Reading the class as "ESC [ then one of P^_" put this arm behind
        //    the CSI one and it never ran: the differential caught `\x1bPtmux;\x1b\\` coming back
        //    with its payload intact.
        b'P' | b'^' | b'_' => dcs(b, start + 2),
        // 2. OSC — runs to BEL, to ST (ESC `\`), or to the END OF INPUT (streams end mid-sequence,
        //    and an unterminated `]133;D;` otherwise leaks as text)
        b']' => {
            let mut i = start + 2;
            while i < b.len() && b[i] != 0x07 && b[i] != 0x1b {
                i += 1;
            }
            match b.get(i) {
                Some(0x07) => Some(i + 1),
                // ESC `\` — the ST terminator
                Some(0x1b) if b.get(i + 1) == Some(&b'\\') => Some(i + 2),
                // end of input
                Some(0x1b) => Some(i),
                None => Some(b.len()),
                _ => None,
            }
        }
        // 4. SINGLE
        b'=' | b'>' | b'7' | b'8' | b'6' | b'M' | b'N' | b'O' | b'c' => Some(start + 2),
        _ => None,
    }
}

/// The UTF-8 length of the character starting with `b` — 1 for ASCII, 2/3/4 by the lead byte, and
/// one for anything malformed, so the scan cannot run past the end of a slice.
fn utf8_len(b: u8) -> usize {
    match b {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}
