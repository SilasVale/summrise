//! THE CHROME IS NEUTRAL AND STILL — and this is what "still" means.
//!
//! WHY THIS EXISTS (round 245 of the standing goal, migrated to Rust in batch 8 of the P1 gate
//! migration). The objective reserves colour AND motion to the state layer: "the chrome neutral and
//! still, with colour and motion reserved to that state layer". Colour has had a gate since round
//! 245; motion had only `motion-check`, which asks whether an animation is SILENCED under
//! `prefers-reduced-motion` — a DIFFERENT question. Nothing asked whether an animation has any
//! business existing, so a decorative pulse would have been silenced for the users who ask for that
//! and left running for everybody else. That is what this gate was paid for.
//!
//! THE LIST IS THE CHECK, and each entry is a PURPOSE with a reason — not a suppression file. That
//! distinction is the whole design: `state-colour-check` writes it down as "each entry is a PURPOSE
//! with a reason, which is the case the list exists to make somebody write down". The three purposes:
//!
//!   STATE       motion that describes something still happening. May run forever, because the thing
//!               it describes has not stopped — and this is the mark language's own motion channel,
//!               the one that DISAPPEARS under reduced motion, which is exactly why the shape has to
//!               carry the state on its own.
//!   ENTRANCE    a thing arriving: a drawer, a menu, a card, a toast. One-shot, at most 400ms.
//!   ATTENTION   "this changed while you were looking elsewhere". One-shot, may outlast an entrance
//!               (a flash has to be long enough to be noticed), at most 5s.
//!
//! A NEW ANIMATION FAILS UNTIL SOMEBODY WRITES DOWN WHICH IT IS. That is the point of the table
//! rather than a regex: the first version of this gate classified by SELECTOR NAME and reported five
//! defects that were not defects — `.mem-busy` and `.browser-ev-live` are state-bearing and the
//! pattern could not see it, and the two `.flash` rules are attention states that decay. Reading the
//! markup settled it: `.browser-ai-dot` and `.browser-ev-live` are rendered only while `aiActive`
//! holds.
//!
//! ── THE TWO DIRECTIONS, AND WHICH IS WHICH ──────────────────────────────────────────────────────
//!
//! This gate is TWO-WAY, and a port that walked only the sheet would be half a gate:
//!
//!   DIRECTION A — MUST BITE. An animation in the sheet with no purpose declared:
//!                 "…animates (logo-pulse 3s infinite) and NO PURPOSE IS DECLARED".
//!   DIRECTION B — MUST BITE, and this is the case that must not bite in REVERSE. A purpose whose
//!                 animation is GONE from the sheet: "PURPOSES declares .new-menu and the sheet no
//!                 longer animates it — a stale entry is a reason nobody is using". Both directions
//!                 are walked here, and both are measured in the mutation block below.
//!   THE CASE THAT MUST NOT BITE, and it is THIS gate's own rather than a generic one: a DECLARATION
//!                 WHOSE ANIMATION IS PRESENT in a form the scanner accepts — a declared STATE that
//!                 runs `infinite`, an ENTRANCE one-shot of 400ms or less, an ATTENTION one of 5s or
//!                 less. That is the whole clean tree: 11 panel + 4 console animations, every one
//!                 declared, every one accepted. Three narrower forms must not bite either, and each
//!                 is pinned in `the_scanner_reads_both_directions` below: `animation: none` (the
//!                 gate skips it by name), a COMMENT that mentions an animation (comments are
//!                 stripped first, because prose ABOUT an animation is not one — two gates in this
//!                 directory learned that the hard way), and a declared entry that is present but
//!                 whose rule sits inside an `@media` block (stated as a shared limit below).
//!
//! ── WHAT IT DOES NOT SEE, stated rather than implied ────────────────────────────────────────────
//!
//!   * THE SCAN IS NOT A CSS PARSER, and the port reproduced that rather than widening it. `@media`
//!     blocks swallow the rules inside them: `([^{}]+)\{([^}]*)\}` takes the text up to the FIRST
//!     `}`, so an `animation` inside a media query is never seen by either implementation. Both
//!     agree, and the limit is pinned by a fixture below rather than papered over.
//!   * AN `animation` DECLARATION NOT AT THE START OF A BODY AND NOT AFTER A `;` is invisible to
//!     both (`(?:^|;)\s*animation\s*:`), which is what makes the `@media` case above behave as it
//!     does.
//!   * A DURATION WITH NO `ms`/`s` UNIT reads as null on both sides, which for a capped kind is a
//!     failure that SAYS SO ("read an unreadable duration") rather than a pass.
//!   * IT DOES NOT CHECK THE ANIMATION'S NAME, only that some purpose is declared for the selector,
//!     that the run/one-shot direction matches the kind, and that a capped kind is inside its cap.
//!     `motion-check` is the gate for the reduced-motion question; this one is the declaration.
//!
//! ── MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations) ──────────────────
//!
//! RE-MEASURED ON THE LANDING TREE (2026-09-29, `8a3891e3`), because the first draft of this block
//! carried numbers that did not survive being re-run: it said the clean line was 141 bytes and the
//! three failure bodies 221/263/205. They are 120, 332, 280 and 476. A number in a header is a claim
//! like any other, so the four cases below are the re-run, each beside the command that produced it —
//! `bash` driving both implementations over one tree and `cmp`-ing the two streams:
//!
//!   case  js exit / rust exit   body bytes (js = rust, `cmp`)   how the tree was mutated
//!   ─────────────────────────────────────────────────────────────────────────────────────────────
//!   clean     0   /   0         120 stdout, identical           (none)
//!   A         1   /  101        332                             append `.rail-brand { animation: logo-pulse 3s infinite }`
//!   B         1   /  101        280                             `5021s/gs-in 160ms ease-out;/… infinite;/`
//!   C         1   /  101        476                             `s/\.new-menu/.new-menu-gone/g`
//!
//!   * CLEAN TREE. `node scripts/test/chrome-stillness-check.mjs` → exit 0, stdout 120 bytes, one
//!     line: "chrome-stillness-check: ok — 15 animation(s) across both sheets, every one declared:
//!     8 state, 5 entrance, 2 attention", stderr 0 bytes. This file → the SAME line, byte for byte
//!     (`cmp` on stdout, 120/120). The counts BEHIND it are counted from the tree and are compared
//!     to FLOORS, never to equalities: measured with the JS parse, panel 11 and console 4, against
//!     the floors 8 and 3 that this file inherits unchanged from the JS.
//!   * FULL FAILURE BODIES, byte for byte, on the three mutations the JS gate's own header names —
//!     see the block below. Both streams were captured and compared; the bodies are 332/332,
//!     280/280 and 476/476 bytes.
//!   * THE DIFFERENTIAL PROBE — the strongest evidence here, because it tests the PARSER rather than
//!     the tree. The four fixtures below were extracted to `/tmp/probe/`, the JS gate was copied with
//!     ONLY its `ROOT` and `SHEETS` array patched to read them, and both implementations were run over
//!     the same three cases. Every body is IDENTICAL byte for byte (`cmp`), re-measured on the landing
//!     tree — the numbers the first draft carried (1162/1162, 733/733) were not reproducible and are
//!     replaced by these:
//!
//!     ```text
//!     case        what it isolates                             js body   rust body   cmp
//!     ──────────────────────────────────────────────────────────────────────────────────────
//!     A clauses   every clause, both directions, floors met     2083      2083      same
//!     B stale     the stale direction alone, one panel rule     1574      1574      same
//!     C floor     the floor short-circuit (1 animation vs 8)    1867      1867      same
//!     ```
//!
//!     It covers what the tree cannot: a declared STATE that stops moving, an ENTRANCE that runs
//!     forever, an ATTENTION past 5s with a decimal duration, an unreadable duration,
//!     `animation: none`, a comment that names an animation, a two-selector rule, an undeclared stray
//!     in each sheet, the stale direction, and the floor short-circuit that makes every purpose of an
//!     under-floor sheet read as stale.
//!
//! MUTATION: add an undeclared decorative animation (`.rail-brand { animation: logo-pulse 3s infinite }`
//!           on the built panel sheet), make a declared ENTRANCE run forever (`gs-in … infinite`), or
//!           delete an animation whose reason is still declared.
//! RESULT:   exit 1 all three ways, and this file fails with the SAME THREE SENTENCES the JS prints —
//!           "…animates (logo-pulse 3s infinite) and NO PURPOSE IS DECLARED — the chrome is still, and
//!           motion belongs to the state layer. Add it to PURPOSES with one of STATE / ENTRANCE /
//!           ATTENTION and the reason it is one"; "…is declared ENTRANCE (the getting-started card
//!           arrives) and animates FOREVER (gs-in 160ms ease-out infinite) — an entrance or an
//!           acknowledgement is a ONE-SHOT"; and "PURPOSES declares .new-menu and the sheet no longer
//!           animates it — a stale entry is a reason nobody is using". THE FIX IS IN THE MESSAGE, which
//!           is the P1 criterion: the first sentence names the purpose to write down, the second names
//!           the direction the declaration got wrong, the third names the entry to delete. `motion-check`
//!           asks a different question (whether a declared animation is silenced under reduced motion),
//!           so a decorative pulse would have been silenced for the users who asked and left running for
//!           everyone else. ITS FIRST VERSION CLASSIFIED BY SELECTOR NAME and reported five defects that
//!           were not defects; reading the markup settled it, and the fix was to write the rule down
//!           instead of approximating it.

mod common;

use common::repo;
use std::fs;

// ── the three purposes ─────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    State,
    Entrance,
    Attention,
}

impl Kind {
    /// `${declared.kind.toUpperCase()}` — the word that appears in a failure body.
    fn upper(self) -> &'static str {
        match self {
            Kind::State => "STATE",
            Kind::Entrance => "ENTRANCE",
            Kind::Attention => "ATTENTION",
        }
    }

    /// The lowercase key, printed by the cap clause and counted by the success line.
    fn key(self) -> &'static str {
        match self {
            Kind::State => "state",
            Kind::Entrance => "entrance",
            Kind::Attention => "attention",
        }
    }

    /// `KINDS[kind].infinite` — may it run forever?
    fn infinite(self) -> bool {
        matches!(self, Kind::State)
    }

    /// `KINDS[kind].maxMs` — the cap for a one-shot, `None` for a state.
    fn max_ms(self) -> Option<f64> {
        match self {
            Kind::State => None,
            Kind::Entrance => Some(400.0),
            Kind::Attention => Some(5000.0),
        }
    }
}

struct Purpose {
    sheet: &'static str,
    selector: &'static str,
    kind: Kind,
    why: &'static str,
}

/// THE LIST IS THE CHECK. Each entry is a purpose with a reason, and the ORDER of this table is the
/// order the stale direction reports in — which is why the port keeps it identical to the JS.
const PURPOSES: [Purpose; 15] = [
    // ── the panel ──────────────────────────────────────────────────────────────────────────────
    Purpose { sheet: "panel", selector: ".cmd-dot[data-state=\"running\"]", kind: Kind::State, why: "a command is in flight — the running mark's motion channel" },
    // KEPT, AND THE ROUND-33 ATTEMPT TO DELETE THE RULE IT DECLARES IS WHY: `eventDotState` cannot
    // return `running` today, but the vocabulary's rule is that every state has a channel in every
    // renderer, so the arm is not dead weight — it is what stops a future producer from drawing an
    // invisible dot. This gate said so the moment the rule went, and four tests in
    // `statePalette.test.ts` said it louder.
    Purpose { sheet: "panel", selector: ".traj-ev-dot[data-state=\"running\"]", kind: Kind::State, why: "the same state inside the trajectory view — a CHANNEL the vocabulary requires, currently unreachable from eventDotState" },
    Purpose { sheet: "panel", selector: ".plug-dot[data-state=\"ongoing\"]", kind: Kind::State, why: "the plugin reports its work as ongoing" },
    Purpose { sheet: "panel", selector: ".browser-ai-dot", kind: Kind::State, why: "rendered only while aiActive holds — the agent is operating the browser" },
    Purpose { sheet: "panel", selector: ".mem-busy", kind: Kind::State, why: "the memory view is waiting on the device" },
    Purpose { sheet: "panel", selector: ".browser-ev-live", kind: Kind::State, why: "rendered only while aiActive holds — the action log is being appended to" },
    Purpose { sheet: "panel", selector: ".browser-ev-toggle.flash", kind: Kind::Attention, why: "new browser actions arrived while the log was closed" },
    Purpose { sheet: "panel", selector: ".desktop-status.flash", kind: Kind::Attention, why: "the device status line just changed" },
    Purpose { sheet: "panel", selector: ".gs-card", kind: Kind::Entrance, why: "the getting-started card arrives" },
    Purpose { sheet: "panel", selector: "#drawer", kind: Kind::Entrance, why: "the details drawer slides in" },
    Purpose { sheet: "panel", selector: ".new-menu", kind: Kind::Entrance, why: "the new-session menu opens" },
    // ── the console ────────────────────────────────────────────────────────────────────────────
    Purpose { sheet: "console", selector: ".user-pop", kind: Kind::Entrance, why: "the account menu opens" },
    Purpose { sheet: "console", selector: ".toast", kind: Kind::Entrance, why: "a toast lands" },
    Purpose { sheet: "console", selector: ".loading-spinner", kind: Kind::State, why: "a request is in flight" },
    Purpose { sheet: "console", selector: ".skeleton-card", kind: Kind::State, why: "content is still loading" },
];

struct Sheet {
    label: &'static str,
    path: &'static str,
    /// A FLOOR, NOT AN EQUALITY, because this number is counted from a tree that grows. Measured
    /// 2026-09-28: panel 11, console 4 — three and one above their floors. The floors are inherited
    /// from the JS unchanged and there is no second, stricter copy of them anywhere in this file.
    min_animations: usize,
}

const SHEETS: [Sheet; 2] = [
    Sheet {
        label: "panel",
        path: "agent/resources/panel/panel.css",
        min_animations: 8,
    },
    Sheet {
        label: "console",
        path: "gateway/ui/src/styles/globals.css",
        min_animations: 3,
    },
];

// ── the parser ─────────────────────────────────────────────────────────────────────────────────

/// JavaScript's `\s`: the Unicode White_Space property PLUS U+FEFF, which `char::is_whitespace`
/// does not have.
fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// JavaScript's `\w`, for the `\b` assertions.
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `String.prototype.trim()`, on the JS whitespace set.
fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// `/\/\*[\s\S]*?\*\//g` — block comments, non-greedy to the FIRST `*/`. An UNTERMINATED `/*` is
/// left as ordinary text, exactly as `String.replace` leaves it: the regex requires the close, so
/// there is no match to remove.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let Some(open) = rest.find("/*") else {
            out.push_str(rest);
            return out;
        };
        let after = &rest[open + 2..];
        match after.find("*/") {
            Some(close) => {
                out.push_str(&rest[..open]);
                rest = &after[close + 2..];
            }
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
}

/// `/([^{}]+)\{([^}]*)\}/g` — every rule, as (prelude, body), in source order.
///
/// The two `[^{}]`/`[^}]` classes are why an `@media` block hides its rules: the prelude is the text
/// up to the first `{` and the body is the text up to the FIRST `}`, so the inner rule lands inside
/// the body and is never a prelude of its own.
fn rules(sheet: &str) -> Vec<(String, String)> {
    let cs: Vec<char> = sheet.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '{' || cs[i] == '}' {
            i += 1;
            continue;
        }
        // The maximal run of non-brace characters, which is what `[^{}]+` is greedy for.
        let start = i;
        let mut j = i;
        while j < cs.len() && cs[j] != '{' && cs[j] != '}' {
            j += 1;
        }
        if j < cs.len() && cs[j] == '{' {
            let mut k = j + 1;
            while k < cs.len() && cs[k] != '}' {
                k += 1;
            }
            if k < cs.len() {
                out.push((cs[start..j].iter().collect(), cs[j + 1..k].iter().collect()));
                i = k + 1;
                continue;
            }
        }
        // No match can start anywhere inside this run: every suffix of it is followed by the same
        // brace (or the end), so the engine's one-character advance cannot change the outcome.
        i = j;
    }
    out
}

/// `/(?:^|;)\s*animation\s*:\s*([^;]+)/` — the animation value in a body, or `None`.
///
/// The `(?:^|;)` is load-bearing: without a `;` or the start of the body in front of it, the word
/// `animation` is prose inside a nested block (an `@media` body) and neither implementation reads it.
fn animation_value(body: &str) -> Option<String> {
    let cs: Vec<char> = body.chars().collect();
    let skip_ws = |mut i: usize| {
        while i < cs.len() && is_js_space(cs[i]) {
            i += 1;
        }
        i
    };
    let matches_at = |mut i: usize| -> Option<String> {
        i = skip_ws(i);
        for ch in "animation".chars() {
            if i >= cs.len() || cs[i] != ch {
                return None;
            }
            i += 1;
        }
        i = skip_ws(i);
        if i >= cs.len() || cs[i] != ':' {
            return None;
        }
        i = skip_ws(i + 1);
        let start = i;
        while i < cs.len() && cs[i] != ';' {
            i += 1;
        }
        // `[^;]+` needs at least one character.
        if i == start {
            return None;
        }
        Some(cs[start..i].iter().collect())
    };
    if let Some(v) = matches_at(0) {
        return Some(v);
    }
    for (i, c) in cs.iter().enumerate() {
        if *c == ';' {
            if let Some(v) = matches_at(i + 1) {
                return Some(v);
            }
        }
    }
    None
}

/// `/^none\b/` on the trimmed value.
fn is_none(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("none") else {
        return false;
    };
    rest.chars().next().is_none_or(|c| !is_word(c))
}

/// `/\binfinite\b/` anywhere in the value.
fn is_infinite(value: &str) -> bool {
    let cs: Vec<char> = value.chars().collect();
    let needle: Vec<char> = "infinite".chars().collect();
    let mut i = 0;
    while i + needle.len() <= cs.len() {
        if cs[i..i + needle.len()] == needle[..]
            && (i == 0 || !is_word(cs[i - 1]))
            && (i + needle.len() == cs.len() || !is_word(cs[i + needle.len()]))
        {
            return true;
        }
        i += 1;
    }
    false
}

/// `/(\d+(?:\.\d+)?)(ms|s)\b/` — the FIRST duration in the value, in ms, or `None`.
///
/// The `ms` alternative is tried before `s`, and the optional decimal part is tried before its
/// absence, which is the order the JS engine backtracks in.
fn parse_ms(value: &str) -> Option<f64> {
    let cs: Vec<char> = value.chars().collect();
    let unit_at = |i: usize| -> Option<f64> {
        let rest: String = cs[i..].iter().collect();
        for (unit, scale) in [("ms", 1.0), ("s", 1000.0)] {
            if let Some(tail) = rest.strip_prefix(unit) {
                let boundary = tail.chars().next().is_none_or(|c| !is_word(c));
                if boundary {
                    return Some(scale);
                }
            }
        }
        None
    };
    let digits_end = |mut i: usize| {
        while i < cs.len() && cs[i].is_ascii_digit() {
            i += 1;
        }
        i
    };
    for start in 0..cs.len() {
        if !cs[start].is_ascii_digit() {
            continue;
        }
        let whole_end = digits_end(start);
        // (a) with the decimal part, then (b) without it — greedy first, as the regex does.
        if whole_end < cs.len()
            && cs[whole_end] == '.'
            && whole_end + 1 < cs.len()
            && cs[whole_end + 1].is_ascii_digit()
        {
            let frac_end = digits_end(whole_end + 1);
            if let Some(scale) = unit_at(frac_end) {
                let n: String = cs[start..frac_end].iter().collect();
                return Some(n.parse::<f64>().expect("digits and one dot") * scale);
            }
        }
        if let Some(scale) = unit_at(whole_end) {
            let n: String = cs[start..whole_end].iter().collect();
            return Some(n.parse::<f64>().expect("digits") * scale);
        }
    }
    None
}

/// `${ms}` in a template literal: `Number.prototype.toString`, which for every value this parser
/// can produce is the same shortest round-trip form Rust's `Display` writes.
fn js_num(v: f64) -> String {
    format!("{v}")
}

struct Found {
    selector: String,
    value: String,
    infinite: bool,
}

/// The animations of one sheet, in source order — the JS's `found` array.
fn find_animations(sheet: &str) -> Vec<Found> {
    let mut found = Vec::new();
    for (prelude, body) in rules(sheet) {
        let Some(value) = animation_value(&body) else {
            continue;
        };
        let value = js_trim(&value).to_string();
        if is_none(&value) {
            continue;
        }
        let infinite = is_infinite(&value);
        for selector in prelude.split(',').map(js_trim).filter(|s| !s.is_empty()) {
            found.push(Found {
                selector: selector.to_string(),
                value: value.clone(),
                infinite,
            });
        }
    }
    found
}

// ── the gate ───────────────────────────────────────────────────────────────────────────────────

/// The whole check over `(label, sheet text, floor)` triples, in order. `Err` is the FAILURE BODY
/// exactly as the JS writes it to stderr — every line prefixed, then the summary line — so a diff of
/// the two is a diff of the gate's conclusion and not of its wording.
fn check(sheets: &[(&str, &str, usize)]) -> Result<String, String> {
    let mut failures: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    for (label, raw, min_animations) in sheets {
        // Comments are stripped first: prose ABOUT an animation is not one.
        let sheet = strip_comments(raw);
        let found = find_animations(&sheet);
        if found.len() < *min_animations {
            failures.push(format!(
                "{label}: read {} animation(s), expected at least {min_animations} — a floor that is not met proves nothing about stillness",
                found.len()
            ));
            // THE FLOOR SHORT-CIRCUITS THE SHEET, and the stale direction below then reports EVERY
            // purpose of it: `seen` was never populated. That is the JS's behaviour, kept rather
            // than tidied, and pinned by a fixture in the probe test.
            continue;
        }
        for a in &found {
            seen.push(format!("{label}|{}", a.selector));
            let declared = PURPOSES
                .iter()
                .find(|p| p.sheet == *label && p.selector == a.selector);
            let Some(declared) = declared else {
                failures.push(format!(
                    "{label}: {} animates ({}) and NO PURPOSE IS DECLARED — the chrome is still, and motion belongs to the state layer. Add it to PURPOSES with one of STATE / ENTRANCE / ATTENTION and the reason it is one",
                    a.selector, a.value
                ));
                continue;
            };
            let kind = declared.kind;
            let ms = parse_ms(&a.value);
            if kind.infinite() && !a.infinite {
                failures.push(format!(
                    "{label}: {} is declared {} ({}) but {} does not run while the state holds — a state that stops moving is not describing anything",
                    a.selector, kind.upper(), declared.why, a.value
                ));
            }
            if !kind.infinite() && a.infinite {
                failures.push(format!(
                    "{label}: {} is declared {} ({}) and animates FOREVER ({}) — an entrance or an acknowledgement is a ONE-SHOT",
                    a.selector, kind.upper(), declared.why, a.value
                ));
            }
            if let Some(cap) = kind.max_ms() {
                if ms.is_none_or(|ms| ms > cap) {
                    let read = match ms {
                        Some(ms) => format!("{}ms", js_num(ms)),
                        None => "an unreadable duration".to_string(),
                    };
                    failures.push(format!(
                        "{label}: {} is declared {} and runs {} — {} is capped at {}ms (read {read})",
                        a.selector, kind.upper(), a.value, kind.key(), js_num(cap)
                    ));
                }
            }
        }
    }

    for p in &PURPOSES {
        if !seen.contains(&format!("{}|{}", p.sheet, p.selector)) {
            failures.push(format!(
                "{}: PURPOSES declares {} and the sheet no longer animates it — a stale entry is a reason nobody is using",
                p.sheet, p.selector
            ));
        }
    }

    if !failures.is_empty() {
        let mut body: String = failures
            .iter()
            .map(|f| format!("chrome-stillness-check: {f}\n"))
            .collect();
        body.push_str(&format!(
            "chrome-stillness-check: FAILED — {} animation(s) the state layer does not own\n",
            failures.len()
        ));
        return Err(body);
    }

    let by_kind = |k: Kind| PURPOSES.iter().filter(|p| p.kind == k).count();
    Ok(format!(
        "chrome-stillness-check: ok — {} animation(s) across both sheets, every one declared: {} state, {} entrance, {} attention\n",
        PURPOSES.len(),
        by_kind(Kind::State),
        by_kind(Kind::Entrance),
        by_kind(Kind::Attention)
    ))
}

fn read(rel: &str) -> String {
    let p = repo().join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

fn check_tree() -> Result<String, String> {
    let sheets: Vec<(&str, String, usize)> = SHEETS
        .iter()
        .map(|s| (s.label, read(s.path), s.min_animations))
        .collect();
    let refs: Vec<(&str, &str, usize)> = sheets
        .iter()
        .map(|(l, t, m)| (*l, t.as_str(), *m))
        .collect();
    check(&refs)
}

#[test]
fn the_chrome_is_still() {
    let msg = check_tree().unwrap_or_else(|e| panic!("{e}"));
    print!("{msg}");
    // AN EQUALITY, AND IT IS A FIXTURE'S NUMBER RATHER THAN THE TREE'S: 15 is `PURPOSES.len()`, this
    // file's own table, and the three counts beside it are read off the same table. Nothing here is
    // counted from a tree that grows — the tree's two counts are compared to the FLOORS in `SHEETS`,
    // which is where a number that moves belongs.
    let expected = format!(
        "chrome-stillness-check: ok — {} animation(s) across both sheets, every one declared: {} state, {} entrance, {} attention\n",
        PURPOSES.len(),
        PURPOSES.iter().filter(|p| p.kind == Kind::State).count(),
        PURPOSES.iter().filter(|p| p.kind == Kind::Entrance).count(),
        PURPOSES.iter().filter(|p| p.kind == Kind::Attention).count(),
    );
    assert_eq!(msg, expected);
}

// ── the scanner's own proof: both directions, on adversarial fixtures ───────────────────────────

/// A sheet that exercises every clause, and the three forms that must NOT bite. Extracted to disk
/// verbatim for the JS half of the differential probe; the two implementations read the same bytes.
const PROBE_PANEL: &str = r####"/* a comment that MENTIONS an animation: .ghost { animation: ghost-pulse 3s infinite } — prose is not a rule */
.cmd-dot[data-state="running"] { animation: cmd-pulse 1.6s var(--ds-ease-in-out) infinite; }
.traj-ev-dot[data-state="running"] { animation: cmd-pulse 1.6s linear; }
.plug-dot[data-state="ongoing"] { animation: plug-pulse 1.6s infinite; }
.browser-ai-dot { animation: browser-ai-pulse 1.2s infinite; }
.mem-busy { animation: mem-busy-spin 1s linear infinite; }
.browser-ev-live { animation: browser-ev-pulse 1.6s ease-out infinite; }
.browser-ev-toggle.flash { animation: browser-ev-flash 5.4s ease-out; }
.desktop-status.flash { animation: desktop-status-flash 6s ease-out; }
.gs-card { animation: gs-in 160ms ease-out infinite; }
#drawer { animation: drawer-in 0.18s ease; }
.new-menu { animation: new-menu-in 0.12s ease; }
.rail-brand { animation: logo-pulse 3s infinite; }
.a, .b { animation: both-pulse 2s infinite; }
.skip-me { animation: none; }
@media (min-width: 600px) { .media-hidden { animation: media-pulse 1s infinite; } }
"####;

const PROBE_CONSOLE: &str = r####".user-pop { animation: pop-in 0.14s var(--ds-ease); }
.toast { animation: toast-in ease-out; }
.loading-spinner { animation: spin 0.8s linear infinite; }
.skeleton-card { animation: dev-shimmer 1.2s infinite; }
.zz-console-stray { animation: glow 2s infinite; }
"####;

/// The four console purposes, every one present and accepted — so a probe case can isolate ONE
/// sheet's behaviour without the other sheet's purposes reading as stale.
const PROBE_CONSOLE_OK: &str = r####".user-pop { animation: pop-in 0.14s var(--ds-ease); }
.toast { animation: toast-in 0.2s var(--ds-ease); }
.loading-spinner { animation: spin 0.8s linear infinite; }
.skeleton-card { animation: dev-shimmer 1.2s infinite; }
"####;

/// One panel purpose present, so the stale direction has a subject that is NOT stale.
const PROBE_PANEL_ONE: &str = r####".cmd-dot[data-state="running"] { animation: cmd-pulse 1.6s infinite; }
"####;

#[test]
fn the_scanner_reads_both_directions() {
    // DIRECTION A and the clauses, with the floors cleared so no floor message can mask them.
    let err = check(&[("panel", PROBE_PANEL, 8), ("console", PROBE_CONSOLE, 3)])
        .expect_err("the probe sheets carry animations nobody declared");
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 10, "{err}");
    assert_eq!(
        lines[9],
        "chrome-stillness-check: FAILED — 9 animation(s) the state layer does not own"
    );
    // The clauses, in the order the JS emits them: the sheet's rules in source order, and within one
    // rule the three tests in the order STATE/ENTRANCE/ATTENTION then the cap.
    let want = [
        "chrome-stillness-check: panel: .traj-ev-dot[data-state=\"running\"] is declared STATE (the same state inside the trajectory view — a CHANNEL the vocabulary requires, currently unreachable from eventDotState) but cmd-pulse 1.6s linear does not run while the state holds — a state that stops moving is not describing anything",
        "chrome-stillness-check: panel: .browser-ev-toggle.flash is declared ATTENTION and runs browser-ev-flash 5.4s ease-out — attention is capped at 5000ms (read 5400ms)",
        "chrome-stillness-check: panel: .desktop-status.flash is declared ATTENTION and runs desktop-status-flash 6s ease-out — attention is capped at 5000ms (read 6000ms)",
        "chrome-stillness-check: panel: .gs-card is declared ENTRANCE (the getting-started card arrives) and animates FOREVER (gs-in 160ms ease-out infinite) — an entrance or an acknowledgement is a ONE-SHOT",
        "chrome-stillness-check: panel: .rail-brand animates (logo-pulse 3s infinite) and NO PURPOSE IS DECLARED — the chrome is still, and motion belongs to the state layer. Add it to PURPOSES with one of STATE / ENTRANCE / ATTENTION and the reason it is one",
        "chrome-stillness-check: panel: .a animates (both-pulse 2s infinite) and NO PURPOSE IS DECLARED — the chrome is still, and motion belongs to the state layer. Add it to PURPOSES with one of STATE / ENTRANCE / ATTENTION and the reason it is one",
        "chrome-stillness-check: panel: .b animates (both-pulse 2s infinite) and NO PURPOSE IS DECLARED — the chrome is still, and motion belongs to the state layer. Add it to PURPOSES with one of STATE / ENTRANCE / ATTENTION and the reason it is one",
        "chrome-stillness-check: console: .toast is declared ENTRANCE and runs toast-in ease-out — entrance is capped at 400ms (read an unreadable duration)",
        "chrome-stillness-check: console: .zz-console-stray animates (glow 2s infinite) and NO PURPOSE IS DECLARED — the chrome is still, and motion belongs to the state layer. Add it to PURPOSES with one of STATE / ENTRANCE / ATTENTION and the reason it is one",
    ];
    for (got, want) in lines[..9].iter().zip(want) {
        assert_eq!(*got, want, "{err}");
    }
    // THE CASE THAT MUST NOT BITE, measured rather than asserted in prose: a DECLARATION WHOSE
    // ANIMATION IS PRESENT and accepted (a state that runs forever, an entrance inside its cap), the
    // skip-by-name, the comment, and the rule an `@media` block swallows.
    for absent in [
        ".cmd-dot[data-state=\"running\"]",
        ".plug-dot[data-state=\"ongoing\"]",
        "#drawer",
        ".new-menu",
        ".user-pop",
        ".loading-spinner",
        ".skip-me",
        ".ghost",
        ".media-hidden",
    ] {
        assert!(!err.contains(absent), "{absent} must not bite:\n{err}");
    }

    // DIRECTION B — the stale direction, alone: one declared animation present, every other panel
    // purpose absent from the sheet, reported in the ORDER of the table. The console sheet is whole,
    // because the JS always walks both and a missing sheet would make ITS purposes stale too.
    let err = check(&[
        ("panel", PROBE_PANEL_ONE, 1),
        ("console", PROBE_CONSOLE_OK, 3),
    ])
    .expect_err("ten panel purposes have no animation in this sheet");
    assert_eq!(err.lines().count(), 11, "{err}");
    assert!(err.contains(
        "chrome-stillness-check: panel: PURPOSES declares .plug-dot[data-state=\"ongoing\"] and the sheet no longer animates it — a stale entry is a reason nobody is using"
    ), "{err}");
    assert!(!err.contains("PURPOSES declares .cmd-dot"), "{err}");
    let stale: Vec<&str> = err
        .lines()
        .filter(|l| l.contains("no longer animates it"))
        .collect();
    assert_eq!(stale.len(), 10, "{err}");
    assert!(stale[0].contains(".traj-ev-dot"), "{}", stale[0]);
    assert!(stale[9].contains(".new-menu"), "{}", stale[9]);

    // AND THE FLOOR'S SHORT-CIRCUIT: an under-floor sheet reports its floor AND every one of its
    // purposes as stale, because `seen` was never populated. Kept, not tidied.
    let err = check(&[
        ("panel", PROBE_PANEL_ONE, 8),
        ("console", PROBE_CONSOLE_OK, 3),
    ])
    .expect_err("one animation is not a floor of eight");
    assert!(err.contains("panel: read 1 animation(s), expected at least 8 — a floor that is not met proves nothing about stillness"), "{err}");
    assert_eq!(err.lines().count(), 13, "{err}");
    assert_eq!(
        err.lines()
            .filter(|l| l.contains("no longer animates it"))
            .count(),
        11,
        "{err}"
    );
}
