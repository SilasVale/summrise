//! HOVER IMPLIES PRESS, A TRANSITION MUST NOT RE-LAY-OUT, AND MOTION MUST BE OPTIONAL.
//!
//! `scripts/test/feedback-check.mjs` (301 lines), transliterated. The THIRTEENTH gate to move into
//! `agent/tests/*.rs`.
//!
//! IT READS THE BUILT SHEET (`agent/resources/panel/panel.css`), like the other CSS contracts here,
//! so it measures what SHIPS rather than what the source intends.
//!
//! FOUR RULES:
//!
//!   1. **HOVER IMPLIES PRESS.** For every selector that styles `:hover`, one with the same base must
//!      style `:active`. Exceptions are listed with a reason, because a timestamp is not pressable.
//!   2. **NO LAYOUT IN A TRANSITION.** A transition on width/height/margin/padding/inset/font-size
//!      re-lays-out the page every frame; `transform` and `opacity` are the two the compositor can do
//!      alone. The sheet transitioned `width` and used `transition: all`, which is how a hover comes to
//!      stutter on a busy device.
//!   3. **A STATED BUDGET.** No transition above 240ms: past that, an interface feels slow rather than
//!      responsive.
//!   4. **MOTION IS OPTIONAL.** The sheet must carry at least one `prefers-reduced-motion` block, or the
//!      responses it adds cannot be turned off.
//!
//! ── THREE FRONT ENDS, AND THE THIRD IS WHY THE LANDING IS HERE ──────────────────────────────────
//!
//! The console had NO press check at all until round 54 — 25 `:hover` selectors and a single `:active`
//! rule — while the panel has had this gate for hundreds of rounds. Each sheet carries its OWN floors,
//! because "a scan that read nothing" means something different in a sheet with 78 hovers and one with
//! 25. The landing is the third (round 95): its stylesheet used to be inline in `index/src/page.js`,
//! which is exactly why it went unchecked — the gate read two paths and the fourth surface was neither.
//! The landing migrated to Rust on 2026-09-28 and the sheet is now a plain file
//! (`index/landing/assets/page.css`), so the `crop` the `.mjs` carried is GONE: this reads the CSS
//! directly, which is the same sheet and one extraction fewer. **The landing's MARKUP is still scanned
//! for `class="…"`**, because the generated arms under `index/src/landing/` are `.js` modules and that
//! is the spelling they use.
//!
//! ── MIGRATION-TIME EQUIVALENCE, MEASURED (both implementations, one tree, 2026-09-29) ──────────
//!
//! The `.mjs` was restored from `main`, both were run over the same tree, and `cmp` was applied to the
//! two streams. SIX CASES, EVERY ONE BYTE-IDENTICAL — and the summaries agree on all three sheets
//! (panel 70 hovers / 6 reveals / 67 presses / 29 transitions; console 24 / 0 / 11 / 18; landing
//! 5 / 0 / 4 / 5):
//!
//!   case                                     js / rust            body bytes   what was mutated
//!   ────────────────────────────────────────────────────────────────────────────────────────────
//!   clean tree                               accept / accept      763          (none)
//!   A  a `:hover` rule with no press         exit 1 / exit 101    150          rule 1
//!   B  `.zzz-a,.zzz-b { transition: width }` exit 1 / exit 101    135          rule 2, and the
//!                                                                               MULTI-ARM message
//!   C  `transition: all`                     exit 1 / exit 101    139          rule 2's other branch
//!   D  `transition: opacity 400ms`           exit 1 / exit 101    117          rule 3
//!   E  `transition: transform 120ms`         accept / accept      763          **must NOT bite**
//!
//! **B IS THE CASE THAT FOUND A FIDELITY GAP BEFORE THE DIFFERENTIAL DID.** The messages quote the
//! rule's selector, and the first version of this file joined its `arms` back with ", " — which prints
//! `.zzz-a, .zzz-b` for a sheet that wrote `.zzz-a,.zzz-b`. The raw collapsed selector text is carried
//! separately now, and B is the case that exercises it: a single-arm rule could never have shown the
//! difference, and a clean tree has no transition failures to quote at all.
//!
//! **AND A NOTE ABOUT HOW THE BLOCK ABOVE WAS WRITTEN, BECAUSE THE FIRST VERSION OF THIS PARAGRAPH WAS
//! FALSE.** It claimed the `.mjs` carried no `MUTATION:` block — `grep -c MUTATION` = 0 — **and it does**:
//! two hits, a full block at lines 2-7 naming `.tab:active`, a layout transition and `transition: all`,
//! plus the three mutations its own rounds ran (`.lang-btn:active` on the console, `.theme-toggle:active`
//! on the landing). The zero came from `main-shape-check`, the gate checked one round earlier, and it was
//! carried over here **without being re-measured** — which is this repository's own recorded failure
//! mode, a measurement attached to the wrong subject. The block above stands as written because it names
//! the mutations THIS port ran, which is what the rule asks for; the claim about its predecessor does not,
//! and is corrected here rather than deleted, because the way it went wrong is worth more than the
//! sentence it replaced.
//!
//! MUTATION: give a hovered selector no press state (delete `.btn:active`), or transition `width`, or
//!           `transition: all`, or a duration over 240ms, or delete the `prefers-reduced-motion` block.
//! RESULT:   exit 101, one line each: "no :active for .btn:hover — a hover that answers and a press
//!           that does not is the feedback gap this checks for"; ".btn transitions width, which
//!           re-lays-out the page every frame (transform and opacity do not)"; ".btn transitions ALL —
//!           every property, including the ones that re-lay-out, is animated on every change"; ".btn
//!           transitions over 240ms (400ms) — past that it reads as slow, not responsive"; and "no
//!           prefers-reduced-motion block: the feedback this sheet adds cannot be turned off".

mod common;

use common::repo;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

/// One front end: its label, its sheet, the source tree whose MARKUP is scanned, and its floors.
struct Sheet {
    label: &'static str,
    path: &'static str,
    src: &'static str,
    min_hovers: usize,
    min_presses: usize,
    min_transitions: usize,
}

const SHEETS: [Sheet; 3] = [
    Sheet {
        label: "panel",
        path: "agent/resources/panel/panel.css",
        src: "agent/resources/panel-react/src",
        min_hovers: 30,
        min_presses: 20,
        min_transitions: 10,
    },
    Sheet {
        label: "console",
        path: "gateway/ui/src/styles/globals.css",
        src: "gateway/ui/src",
        min_hovers: 10,
        min_presses: 1,
        min_transitions: 3,
    },
    Sheet {
        label: "landing",
        path: "index/landing/assets/page.css",
        src: "index/src",
        min_hovers: 4,
        min_presses: 3,
        min_transitions: 3,
    },
];

/// NOT PRESSABLE, each with its reason. **A gate that demands a press state from a timestamp teaches
/// people to ignore the gate.**
const NOT_PRESSABLE: [(&str, &str); 11] = [
    ("*::-webkit-scrollbar-thumb", "a scrollbar thumb is dragged, not pressed"),
    (".side-row .side-time", "the row's relative timestamp — text, not a control"),
    (".log-line", "a log line is read, not clicked"),
    (".mono-line", "a log line is read, not clicked"),
    // THE CONSOLE'S CONTAINERS (round 54). Each answers a hover with a highlight and has NO onClick on
    // the element itself — measured in the console's TSX, where the only clickable class among these is
    // `.auth-tab` (three onClick handlers), and that one got a press state instead of an entry here. A
    // control added to any of these cards must remove its entry.
    (".stat-card", "a stat display, not a control — no onClick in the console's markup (round 54)"),
    (".card", "a container that highlights on hover; no onClick in the console's markup (round 54)"),
    (".key-card", "a key display card — no onClick in the console's markup (round 54)"),
    (".dev-card", "a device card — no onClick in the console's markup (round 54)"),
    (".list-row", "a list row — no onClick in the console's markup (round 54)"),
    (".lane", "a provider lane — no onClick in the console's markup (round 54)"),
    (".step", "a numbered step CARD on the landing page — the markup is a plain div with no handler and no href (round 95); the links INSIDE it are the controls, and they carry a press state"),
];

struct Rule {
    i: usize,
    /// THE RAW SELECTOR TEXT, COLLAPSED — and it is carried separately from `arms` because the
    /// messages quote it. `arms` is the split list the pairing logic needs; joining it back with ", "
    /// would print `.a, .b` for a sheet that wrote `.a,.b`, which is a difference in a gate's output
    /// for no reason. (Found by looking for it before the differential did.)
    sel: String,
    arms: Vec<String>,
    body: String,
}

/// `([^{}]+)\{([^{}]*)\}` — the whole selector list, NOT its last line.
///
/// The idiom `.split("\n").pop()` is used elsewhere to drop a banner line above a rule, and it silently
/// discards every selector but the last when the sheet groups a comma list across lines — which is
/// exactly how the press layer is written, so the first run of the `.mjs` saw 6 `:active` rules instead
/// of ~90 selectors and reported the whole layer as missing.
fn rules(css: &str) -> Vec<Rule> {
    let c: Vec<char> = css.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut n = 0;
    while i < c.len() {
        if c[i] == '{' {
            let mut s = i;
            while s > 0 && c[s - 1] != '}' && c[s - 1] != '{' {
                s -= 1;
            }
            let sel: String = c[s..i].iter().collect();
            let mut e = i + 1;
            while e < c.len() && c[e] != '}' && c[e] != '{' {
                e += 1;
            }
            if e < c.len() && c[e] == '}' {
                let body: String = c[i + 1..e].iter().collect();
                out.push(Rule {
                    i: n,
                    sel: sel.split_whitespace().collect::<Vec<_>>().join(" "),
                    arms: sel.split(',').map(|x| x.trim().to_string()).collect(),
                    body,
                });
                n += 1;
                i = e + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn strip_comments(css: &str) -> String {
    let s: Vec<char> = css.chars().collect();
    let mut out = String::with_capacity(css.len());
    let mut i = 0;
    while i < s.len() {
        if i + 1 < s.len() && s[i] == '/' && s[i + 1] == '*' {
            match (i + 2..s.len().saturating_sub(1)).find(|&k| s[k] == '*' && s[k + 1] == '/') {
                Some(k) => {
                    i = k + 2;
                    continue;
                }
                None => break,
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// A selector without its state, so `.btn:hover` and `.btn:active` are recognised as the same thing.
///
/// `:not(...)` IS STRIPPED TOO, and leaving it in is how the first run of the `.mjs` reported `.btn` as
/// having no press state when it has had one since round 148 (`transform: translateY(1px)`): the base of
/// `.btn:active:not(:disabled)` came out as `.btn:not()`, which matches nothing. **A gate that misreads
/// a rule as absent is worse than no gate — it sends you to add what is already there.**
fn base(s: &str) -> String {
    let mut out = String::new();
    let ch: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < ch.len() {
        let rest: String = ch[i..].iter().collect();
        let matched = [":hover", ":active", ":focus-visible", ":focus", ":disabled"]
            .iter()
            .find(|m| rest.starts_with(**m));
        if let Some(m) = matched {
            i += m.len();
            continue;
        }
        if rest.starts_with(":not(") {
            match rest.find(')') {
                Some(k) => {
                    i += k + 1;
                    continue;
                }
                None => break,
            }
        }
        out.push(ch[i]);
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `/:hover\s+\S/` — `.tab:hover .tab-export` is not a pressable thing; it REVEALS a descendant, and
/// the state belongs to the tab, not to the export button.
fn is_reveal(s: &str) -> bool {
    let b: Vec<char> = s.chars().collect();
    let h = [':', 'h', 'o', 'v', 'e', 'r'];
    for i in 0..b.len().saturating_sub(h.len()) {
        if b[i..i + h.len()] == h {
            let mut k = i + h.len();
            let start = k;
            while k < b.len() && b[k].is_whitespace() {
                k += 1;
            }
            if k > start && k < b.len() {
                return true;
            }
        }
    }
    false
}

/// `[ids, classes, elements]`.
fn specificity(s: &str) -> (i32, i32, i32) {
    let ch: Vec<char> = s.chars().collect();
    let (mut ids, mut classes) = (0, 0);
    let mut i = 0;
    // Count `#name`, `.name`, `[...]`, and `:pseudo` (single colon, not `::`).
    while i < ch.len() {
        if ch[i] == '#' {
            let mut k = i + 1;
            while k < ch.len() && (ch[k].is_alphanumeric() || ch[k] == '_' || ch[k] == '-') {
                k += 1;
            }
            if k > i + 1 {
                ids += 1;
                i = k;
                continue;
            }
        }
        if ch[i] == '.' {
            let mut k = i + 1;
            while k < ch.len() && (ch[k].is_alphanumeric() || ch[k] == '_' || ch[k] == '-') {
                k += 1;
            }
            if k > i + 1 {
                classes += 1;
                i = k;
                continue;
            }
        }
        if ch[i] == '[' {
            let mut k = i + 1;
            while k < ch.len() && ch[k] != ']' {
                k += 1;
            }
            if k < ch.len() {
                classes += 1;
                i = k + 1;
                continue;
            }
        }
        if ch[i] == ':' && (i + 1 >= ch.len() || ch[i + 1] != ':') {
            let mut k = i + 1;
            while k < ch.len() && (ch[k].is_alphanumeric() || ch[k] == '_' || ch[k] == '-') {
                k += 1;
            }
            if k > i + 1 {
                classes += 1;
                if k < ch.len() && ch[k] == '(' {
                    while k < ch.len() && ch[k] != ')' {
                        k += 1;
                    }
                    k += 1;
                }
                i = k;
                continue;
            }
        }
        i += 1;
    }
    // Elements: `[a-zA-Z][\w-]*` over the string with `#x`, `.x`, `[...]` and `:pseudo` removed.
    let mut stripped = String::new();
    let mut i = 0;
    while i < ch.len() {
        if ch[i] == '#' || ch[i] == '.' {
            let mut k = i + 1;
            while k < ch.len() && (ch[k].is_alphanumeric() || ch[k] == '_' || ch[k] == '-') {
                k += 1;
            }
            stripped.push(' ');
            i = k;
            continue;
        }
        if ch[i] == '[' {
            let mut k = i + 1;
            while k < ch.len() && ch[k] != ']' {
                k += 1;
            }
            stripped.push(' ');
            i = k + 1;
            continue;
        }
        if ch[i] == ':' {
            let mut k = i;
            while k < ch.len() && ch[k] == ':' {
                k += 1;
            }
            while k < ch.len() && (ch[k].is_alphanumeric() || ch[k] == '_' || ch[k] == '-') {
                k += 1;
            }
            if k < ch.len() && ch[k] == '(' {
                while k < ch.len() && ch[k] != ')' {
                    k += 1;
                }
                k += 1;
            }
            stripped.push(' ');
            i = k;
            continue;
        }
        stripped.push(ch[i]);
        i += 1;
    }
    let sc: Vec<char> = stripped.chars().collect();
    let mut elements = 0;
    let mut i = 0;
    while i < sc.len() {
        if sc[i].is_ascii_alphabetic() {
            let mut k = i + 1;
            while k < sc.len() && (sc[k].is_alphanumeric() || sc[k] == '_' || sc[k] == '-') {
                k += 1;
            }
            elements += 1;
            i = k;
            continue;
        }
        i += 1;
    }
    (ids, classes, elements)
}

/// `a[0] !== b[0] ? a[0] > b[0] : a[1] !== b[1] ? a[1] > b[1] : a[2] >= b[2]`
fn at_least(a: (i32, i32, i32), b: (i32, i32, i32)) -> bool {
    if a.0 != b.0 {
        a.0 > b.0
    } else if a.1 != b.1 {
        a.1 > b.1
    } else {
        a.2 >= b.2
    }
}

/// `(?:^|[;{\s])([a-z-]+)\s*:\s*([^;}]+)` — a block's declarations, in order.
fn decls_of(body: &str) -> BTreeMap<String, String> {
    let ch: Vec<char> = body.chars().collect();
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < ch.len() {
        let boundary = i == 0 || matches!(ch[i - 1], ';' | '{') || ch[i - 1].is_whitespace();
        if boundary && (ch[i].is_ascii_lowercase() || ch[i] == '-') {
            let start = i;
            let mut k = i;
            while k < ch.len() && (ch[k].is_ascii_lowercase() || ch[k] == '-') {
                k += 1;
            }
            let name: String = ch[start..k].iter().collect();
            let mut j = k;
            while j < ch.len() && ch[j].is_whitespace() {
                j += 1;
            }
            if j < ch.len() && ch[j] == ':' {
                j += 1;
                while j < ch.len() && ch[j].is_whitespace() {
                    j += 1;
                }
                let vstart = j;
                while j < ch.len() && ch[j] != ';' && ch[j] != '}' {
                    j += 1;
                }
                let value: String = ch[vstart..j].iter().collect();
                out.insert(name, value.trim().to_string());
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `arm.split(/\s+|[>+~]/).filter(Boolean).pop()` — the SUBJECT of a rule, not an ancestor of it.
///
/// `.cmd-btn.cmd-toggle.open svg` and `.traj-round.open .traj-chev` name `.cmd-btn` and `.traj-round`
/// while styling a descendant, and the first run of the `.mjs` called those shadows: two findings the
/// rendered measurement would have had to disprove one at a time.
fn subject_of(arm: &str) -> String {
    // COLLECTED, THEN TAKEN FROM THE END, and the reason is that clippy fires BOTH ways on the direct
    // form: `double_ended_iterator_last` on `.filter(..).next_back()` ("consider using `.last()`") and
    // `iter_last` on `.filter(..).last()` ("this will needlessly iterate the entire iterator"). A slice
    // has one `last()` and neither lint applies to it, so the expression is written where the meaning
    // is unambiguous rather than where the lints disagree.
    let parts: Vec<&str> = arm
        .split(|c: char| c.is_whitespace() || matches!(c, '>' | '+' | '~'))
        .filter(|x| !x.is_empty())
        .collect();
    parts.last().copied().unwrap_or("").to_string()
}

/// `namesKey` — **A WHOLE CLASS TOKEN, NOT A SUBSTRING.** `arm.includes(".btn")` is true of
/// `.btn-ghost:active:not(:disabled)`, and the first run of the `.mjs` reported `.btn`'s press as
/// shadowed for that reason alone, while the rendered measurement had already watched it move. The key
/// must also be the SUBJECT of the rule — the element the press belongs to.
fn names_key(arm: &str, key: &str) -> bool {
    let hay = if key.contains(' ') {
        arm.to_string()
    } else {
        subject_of(arm)
    };
    let hb = hay.as_bytes();
    let kb = key.as_bytes();
    let mut from = 0;
    while from + kb.len() <= hb.len() {
        if &hb[from..from + kb.len()] == kb {
            let end = from + kb.len();
            let ok = end >= hb.len()
                || !(hb[end].is_ascii_alphanumeric() || hb[end] == b'_' || hb[end] == b'-');
            if ok {
                return true;
            }
        }
        from += 1;
    }
    false
}

/// One press, grouped by WHAT IS PRESSED: `.tab:active` and `.tab:active:not(.closed)` are one control.
///
/// The map is the group's declarations (last wins), and the list is every arm that contributed to it —
/// which is what the specificity bar is taken from.
type Press = (BTreeMap<String, String>, Vec<(usize, String, String)>);

/// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN — and the floor is per sheet.
struct Outcome {
    fatal: Option<String>,
    failures: Vec<String>,
    hovers: usize,
    presses: usize,
    transitions: usize,
    reveals: usize,
}

const LAYOUT_PROPS: [&str; 11] = [
    "width",
    "height",
    "margin",
    "padding",
    "top",
    "right",
    "bottom",
    "left",
    "inset",
    "font-size",
    "line-height",
];

/// DOCUMENTED EXCEPTIONS, each with the reason it cannot be compositor-only. Named by selector AND
/// property so a second one cannot ride along on a vague match.
const LAYOUT_OK: [(&str, &str, &str); 4] = [
    (".side-section-body", "max-height", "an accordion that opens by height has no transform equivalent — its content must actually take space"),
    (".prov-body", "max-height", "same accordion idiom"),
    (".facets-body", "max-height", "same accordion idiom"),
    (".browser-embedded-slot", "width", "NOT a paint: the slot sizes an Electron WebContentsView, and the ResizeObserver reports these bounds to the main process. A transform would MOVE that native view instead of shrinking it, so the width is the function, not the animation"),
];

const MAX_TRANSITION_MS: f64 = 240.0;

fn check_sheet(sheet: &Sheet) -> Outcome {
    let path = repo().join(sheet.path);
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let css = strip_comments(&raw);
    let rules = rules(&css);

    let mut hovers: BTreeMap<String, String> = BTreeMap::new();
    let mut actives: BTreeSet<String> = BTreeSet::new();
    let mut reveals = 0usize;
    for r in &rules {
        for arm in &r.arms {
            if arm.contains(":hover") {
                if is_reveal(arm) {
                    reveals += 1;
                    continue;
                }
                hovers.insert(base(arm), arm.clone());
            }
            if arm.contains(":active") {
                actives.insert(base(arm));
            }
        }
    }

    // A PRESS STATE ON A BASE COVERS ITS VARIANTS, which is what CSS does: `.btn:active` matches an
    // element that carries both `btn` and `primary`, so `.btn.primary:hover` is already answered by
    // `.btn:active`. Requiring the two bases to be identical reported every variant as a gap — 64 of
    // them — and would have sent me to write rules that change nothing.
    let has_press = |b: &str| {
        actives
            .iter()
            .any(|a| b == a || b.starts_with(&format!("{a}.")) || b.starts_with(&format!("{a}:")))
    };

    let mut failures: Vec<String> = Vec::new();

    // ── 1a. A PRESS WHOSE EVERY PROPERTY IS SHADOWED IS A PRESS THAT CANNOT BE SEEN (round 52) ────
    //
    // Round 51 measured presses AS RENDERED for the first time and found the ACTIVE TAB dead:
    // `.tab.active` and `.tab:active` are both (0,2,0), the sheet put the state rule later, and the
    // press rule's only property was a background — so the one control an operator presses to re-focus
    // it did nothing at all ON SCREEN. The other checks cannot see that: they prove a rule EXISTS.
    //
    // THE RULE IS "EVERY PROPERTY", not "any": a press that also moves is a press you can see, so
    // `.tab:active`'s background being shadowed is harmless now that a transform survives. Only when
    // NOTHING of the press reaches the screen is it the defect.
    let mut presses: BTreeMap<String, Press> = BTreeMap::new();
    for r in &rules {
        for arm in &r.arms {
            if !arm.contains(":active") {
                continue;
            }
            let key = base(arm);
            if key.is_empty() {
                continue;
            }
            let e = presses
                .entry(key)
                .or_insert_with(|| (BTreeMap::new(), Vec::new()));
            for (p, v) in decls_of(&r.body) {
                e.0.insert(p, v);
            }
            e.1.push((r.i, arm.clone(), r.body.clone()));
        }
    }

    if presses.len() < sheet.min_presses {
        return Outcome {
            fatal: Some(format!(
                "{}: found {} press rule(s) where this sheet should have at least {}; the shadow scan is reading the wrong thing",
                sheet.label, presses.len(), sheet.min_presses
            )),
            failures,
            hovers: hovers.len(),
            presses: presses.len(),
            transitions: 0,
            reveals,
        };
    }

    for (key, (decls, order)) in &presses {
        let mut surviving: Vec<String> = Vec::new();
        for (prop, value) in decls {
            // SHADOWED WHEN A LATER RULE FOR THE SAME ELEMENTS SETS THE SAME PROPERTY AT >=
            // SPECIFICITY. **THE STRONGEST ARM OF THE PRESS, not the weakest**: a press written as
            // `.btn:active:not(:disabled)` is (0,3,0) while `.btn:active` alone is (0,2,0) — and
            // comparing a later rule against the WEAKER one reported `.btn`'s press as shadowed when
            // the rendered measurement had already shown it moving. The grouped arms are one control,
            // so the bar a later rule must clear is the highest one in the group.
            let bar = order
                .iter()
                .map(|(_, arm, _)| specificity(arm))
                .reduce(|a, b| if at_least(a, b) { a } else { b })
                .unwrap_or((0, 0, 0));
            let last_i = order.iter().map(|(i, _, _)| *i).max().unwrap_or(0);
            let shadow = rules.iter().find(|r| {
                if r.i <= last_i {
                    return false;
                }
                if !r
                    .arms
                    .iter()
                    .any(|arm| names_key(arm, key) && at_least(specificity(arm), bar))
                {
                    return false;
                }
                match decls_of(&r.body).get(prop) {
                    Some(later) => later != value,
                    None => false,
                }
            });
            if shadow.is_none() {
                surviving.push(prop.clone());
            }
        }
        if surviving.is_empty() && !decls.is_empty() {
            failures.push(format!(
                "{key}:active sets {} — and EVERY one is shadowed by a later rule, so the press cannot be seen. \
                 Round 51's active tab was this: .tab.active put a background over .tab:active's only property. \
                 Give the press a transform, which no background rule can override",
                decls.keys().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
    }

    // ── 1b. A VARIANT IS ALSO COVERED WHEN THE MARKUP PUTS BOTH CLASSES ON ONE ELEMENT ───────────
    //
    // And that is MEASURED here, not assumed. The panel writes `.btn.primary` (one selector, dot form)
    // so the prefix rule above already sees it; the console writes `className="btn btn-primary"` (TWO
    // classes, dash form), where `.btn:active` matches the element but no prefix rule can know. Reading
    // the markup settles it per variant — and it settles it the right way: `.btn-dashed` is used ALONE
    // in this console, so the same reasoning does NOT cover it, and that one was a real gap this check
    // reported rather than an exemption it granted.
    let mut class_lists: Vec<Vec<String>> = Vec::new();
    walk_markup(&repo().join(sheet.src), &mut class_lists);
    let paired = |a: &str, b: &str| {
        class_lists
            .iter()
            .any(|l| l.iter().any(|x| x == a) && l.iter().any(|x| x == b))
    };
    let covered_variant = |b: &str| {
        if !b.starts_with('.') {
            return false;
        }
        let name: String = b[1..].split(['.', ':']).next().unwrap_or("").to_string();
        actives.iter().any(|a| {
            if !a.starts_with('.') {
                return false;
            }
            let an: String = a[1..].split(['.', ':']).next().unwrap_or("").to_string();
            b.starts_with(&format!("{a}-")) && paired(&an, &name)
        })
    };

    let missing: Vec<String> = hovers
        .iter()
        .filter(|(k, _)| {
            !has_press(k)
                && !covered_variant(k)
                && !NOT_PRESSABLE.iter().any(|(sel, _)| sel == &k.as_str())
        })
        .map(|(_, v)| v.clone())
        .collect();
    for sel in missing {
        failures.push(format!(
            "no :active for {sel} — a hover that answers and a press that does not is the feedback gap this checks for"
        ));
    }

    // ── 2/3. transitions: what they touch and how long they take ─────────────────────────────────
    let mut transitions = 0usize;
    for r in &rules {
        for decl in transition_decls(&r.body) {
            transitions += 1;
            for part in decl.split(',').map(|p| p.trim().to_string()) {
                let prop = part.split_whitespace().next().unwrap_or("").to_string();
                if prop == "all" {
                    failures.push(format!(
                        "{} transitions ALL — every property, including the ones that re-lay-out, is animated on every change",
                        r.sel
                    ));
                } else if LAYOUT_PROPS.contains(&prop.as_str())
                    && !LAYOUT_OK
                        .iter()
                        .any(|(sel, p, _)| r.arms.iter().any(|a| a.contains(sel)) && *p == prop)
                {
                    failures.push(format!(
                        "{} transitions {prop}, which re-lays-out the page every frame (transform and opacity do not)",
                        r.sel
                    ));
                }
                if let Some((text, value)) = ms_in(&part) {
                    if value > MAX_TRANSITION_MS {
                        failures.push(format!(
                            "{} transitions over {MAX_TRANSITION_MS:.0}ms ({text}) — past that it reads as slow, not responsive",
                            r.sel
                        ));
                    }
                }
            }
        }
    }

    // ── 4. motion is optional ────────────────────────────────────────────────────────────────────
    if !css.contains("prefers-reduced-motion") {
        failures.push(
            "no prefers-reduced-motion block: the feedback this sheet adds cannot be turned off"
                .to_string(),
        );
    }

    // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN. The panel has had dozens of hovers for hundreds of
    // rounds; the console has 25. A count near zero means this file is reading the wrong thing, not
    // that a sheet got simple.
    if hovers.len() < sheet.min_hovers || transitions < sheet.min_transitions {
        return Outcome {
            fatal: Some(format!(
                "{}: read {} hover selectors and {} transitions from {}, so this proves nothing",
                sheet.label,
                hovers.len(),
                transitions,
                sheet.path
            )),
            failures,
            hovers: hovers.len(),
            presses: presses.len(),
            transitions,
            reveals,
        };
    }

    Outcome {
        fatal: None,
        failures,
        hovers: hovers.len(),
        presses: presses.len(),
        transitions,
        reveals,
    }
}

/// `transition:\s*([^;]+);` — every such declaration in a body.
fn transition_decls(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = body[from..].find("transition:") {
        let start = from + at + "transition:".len();
        let rest = &body[start..];
        let skipped = rest.len() - rest.trim_start().len();
        let value_start = start + skipped;
        match body[value_start..].find(';') {
            Some(end) => {
                out.push(body[value_start..value_start + end].to_string());
                from = value_start + end + 1;
            }
            None => break,
        }
    }
    out
}

/// `/([\d.]+)m?s/` — the first duration in a transition part, in ms.
fn ms_in(part: &str) -> Option<(String, f64)> {
    let ch: Vec<char> = part.chars().collect();
    let mut i = 0;
    while i < ch.len() {
        if ch[i].is_ascii_digit() || ch[i] == '.' {
            let start = i;
            let mut k = i;
            while k < ch.len() && (ch[k].is_ascii_digit() || ch[k] == '.') {
                k += 1;
            }
            // `m?s` — the `m` is optional, so `400s` would match too; the `.mjs` has the same shape.
            let text = if k + 1 < ch.len() && ch[k] == 'm' && ch[k + 1] == 's' {
                ch[start..k + 2].iter().collect::<String>()
            } else if k < ch.len() && ch[k] == 's' {
                ch[start..k + 1].iter().collect::<String>()
            } else {
                i = k.max(i + 1);
                continue;
            };
            let num: String = ch[start..k].iter().collect();
            let value = num.parse::<f64>().ok()?;
            let value = if text.ends_with("ms") {
                value
            } else {
                value * 1000.0
            };
            return Some((text, value));
        }
        i += 1;
    }
    None
}

/// BOTH SPELLINGS, AND BOTH EXTENSIONS. The panel and the console write `className="…"` in `.tsx`; the
/// landing builds HTML strings in a `.js` module and writes `class="…"`. Visiting only `.tsx` made
/// every landing variant invisible to the exemption below — which is SILENT, because a missing
/// exemption only means a rule this check would have granted is reported as a gap instead (round 95).
fn walk_markup(dir: &std::path::Path, out: &mut Vec<Vec<String>>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            walk_markup(&p, out);
            continue;
        }
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let ok = name.ends_with(".tsx")
            || name.ends_with(".ts")
            || name.ends_with(".js")
            || name.ends_with(".jsx");
        if !ok {
            continue;
        }
        let Ok(text) = fs::read_to_string(&p) else {
            continue;
        };
        // `(?:className=\{?|class=)[`"']([^`"']+)[`"']`
        for (needle, braced) in [("className=", true), ("class=", false)] {
            let mut from = 0;
            while let Some(at) = text[from..].find(needle) {
                let mut i = from + at + needle.len();
                if braced && text[i..].starts_with('{') {
                    i += 1;
                }
                let rest = &text[i..];
                let mut chars = rest.chars();
                if let Some(q) = chars.next() {
                    if q == '"' || q == '\'' || q == '`' {
                        let body: String = rest[1..].chars().take_while(|c| *c != q).collect();
                        if !body.is_empty() {
                            out.push(body.split_whitespace().map(|x| x.to_string()).collect());
                        }
                        from = i + 1 + body.len() + 1;
                        continue;
                    }
                }
                from = i;
                if from <= at {
                    break;
                }
            }
        }
    }
}

#[test]
fn hover_implies_press_and_a_transition_never_re_lays_out() {
    let mut all_failures: Vec<String> = Vec::new();
    let mut summaries: Vec<String> = Vec::new();

    for sheet in &SHEETS {
        let r = check_sheet(sheet);
        if let Some(fatal) = r.fatal {
            panic!("feedback-check: FAILED — {fatal}");
        }
        for f in &r.failures {
            all_failures.push(format!("{}: {f}", sheet.label));
        }
        summaries.push(format!(
            "{}: {} pressable hover selectors, all with a press state ({} reveal rules skipped); {} presses with at least one property that survives the cascade; {} transitions, none touching layout, all within {:.0}ms; reduced-motion honoured",
            sheet.label, r.hovers, r.reveals, r.presses, r.transitions, MAX_TRANSITION_MS
        ));
    }

    if !all_failures.is_empty() {
        let msg = format!(
            "feedback-check: FAILED\n{}",
            all_failures
                .iter()
                .map(|f| format!("  {f}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        panic!("{msg}");
    }
    for s in summaries {
        println!("feedback-check: ok — {s}");
    }
}
