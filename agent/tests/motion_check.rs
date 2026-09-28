//! NOTHING ANIMATES FOR SOMEONE WHO ASKED FOR NO MOTION.
//!
//! `scripts/test/motion-check.mjs` (123 lines), transliterated. The eighth gate to move into
//! `agent/tests/*.rs`, and the first of them whose subject is a STYLESHEET rather than a source file
//! — which is why it carries its own small CSS scanner instead of borrowing one.
//!
//! WHY IT EXISTS (round 13 of the standing goal). The objective's clause is "the chrome neutral and
//! still", and the halves of it were measured before this was written:
//!
//!   * WHAT IS LOUD: on the Terminal with a question pending, exactly ONE element carries a
//!     saturated fill (`.approval-approve` — the operator's decision); on Settings, NONE.
//!   * WHAT MOVES AT REST: on three surfaces, the only animation running is `.xterm-cursor-blink` —
//!     xterm's own cursor, not this UI's chrome.
//!   * AND WHAT THE SHEETS DECLARE: 12 running animations in the panel, 4 in the console. The panel
//!     silences each one BY NAME inside `prefers-reduced-motion`. The console had a single block
//!     setting `--ds-dur: 0s` — **which reaches every transition written with that token and NOT ONE
//!     of its four animations**, because each declares its own literal duration. So a user who asked
//!     for reduced motion still got a shimmer, a spinner and two entrances. That is the defect this
//!     gate exists to keep fixed, and it is why the rule is about `animation:` and not about a token.
//!
//! THE RULE: every selector that runs an animation must be silenced in a `prefers-reduced-motion`
//! block — by name, or by a global `*` idiom. A clean result nothing enforces is one commit from not
//! being true.
//!
//! ── MIGRATION-TIME EQUIVALENCE, MEASURED (both implementations, one tree, 2026-09-29) ──────────
//!
//! The `.mjs` was restored from `main`, both were run over the same tree, and `cmp` was applied to
//! the two streams — the `.mjs`'s stdout/stderr against THIS file's own text. Five cases, and every
//! one byte-identical:
//!
//!   case                                js / rust            body bytes   how the sheet was mutated
//!   ────────────────────────────────────────────────────────────────────────────────────────────
//!   clean tree                          accept / accept      103          (none)
//!   A  `.mem-busy` unsilenced           exit 1 / exit 101    352          remove it from the panel's block
//!   B  two more unsilenced              exit 1 / exit 101    467          `.browser-ai-dot`, `.browser-ev-live`
//!   C  the console's spinner gone       exit 1 / exit 101    356          rename `.loading-spinner` in the console's
//!   D  a global `*` silences everything accept / accept      103          the mutation that must NOT bite
//!
//! A AND B ARE THE PANEL, C IS THE CONSOLE, AND D IS THE OTHER DIRECTION: the gate accepts either
//! idiom — a selector named in the block, or a global `*` — so a case that added `* { animation: none }`
//! while REMOVING `.mem-busy` must stay green, and it does, in both implementations. Three bite cases
//! and one that must not, across both sheets, is the whole of what this gate claims.
//!
//! ── WHAT IT DOES NOT SEE, stated rather than implied ────────────────────────────────────────────
//!
//!   * **IT READS THE BUILT PANEL SHEET AND THE CONSOLE'S SOURCES**, which is what the `.mjs` read:
//!     `agent/resources/panel/panel.css` is vite's output and `gateway/ui/src/styles/*.css` is the
//!     console's input. A rule the panel's build drops is not seen here, and a rule the console's
//!     build ADDS is not either. That asymmetry is the JavaScript's and it is kept — changing it
//!     would change the subject, which is a different commit.
//!   * **IT IS NOT A CSS PARSER.** `rules` is the same `([^{}]+)\{([^{}]*)\}` scan the `.mjs` used:
//!     it cannot see a rule nested inside another block, and it treats `@keyframes` bodies as
//!     ordinary rules — which is why the `from`/`to`/`N%` selector skip below exists.
//!   * **THE CONSOLE'S FILES ARE SORTED HERE, AND THE `.mjs` DID NOT SORT THEM.** `readdirSync`
//!     returns directory order, which is unspecified; the VERDICT is order-independent (the scan is
//!     a set membership test per rule), but the ORDER OF THE FAILURE LIST was not, so a port that
//!     kept the platform's order would report the same defects in a different sequence on a
//!     different filesystem. Sorting makes the message reproducible; it cannot make the verdict
//!     different, and the differential above is what proves that.
//!
//! MUTATION: take a selector out of a `prefers-reduced-motion` block (`.mem-busy` from the panel's)
//! RESULT:   exit 101: "panel: .mem-busy runs \"mem-busy-spin 1s linear infini\" and no
//!           prefers-reduced-motion block stops it". It reads both sheets and accepts either idiom —
//!           a selector named in the block, or a global `*`. Written after the CONSOLE was found
//!           relying on `--ds-dur: 0s`, which reaches transitions and no `animation:` at all; the
//!           panel was the model for that fix and the gate then found SEVEN of its own twelve
//!           unsilenced. Verified on the rendered panel too: with reduced motion emulated, the only
//!           animation under `no-preference` is xterm's cursor and there are ZERO under `reduce`.

mod common;

use common::repo;
use std::fs;

/// Everything inside `@media (prefers-reduced-motion…) { … }`, with balanced braces.
///
/// The `.mjs`'s `/@media[^{]*prefers-reduced-motion[^{]*\{/g`: `@media`, then any run of characters
/// that are not `{`, then the feature name, then more non-`{`, then the opening brace. `[^{]` is
/// what stops it from matching an `@media` whose CONDITION is a block — and it is why a nested
/// `@media` inside the block is invisible to the brace balance, exactly as it is to the regex.
fn reduced_motion_blocks(css: &str) -> Vec<String> {
    let s: Vec<char> = css.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        // `@media`
        if !(i + 6 <= s.len() && s[i..i + 6].iter().collect::<String>() == "@media") {
            i += 1;
            continue;
        }
        // `[^{]*prefers-reduced-motion[^{]*\{` — everything up to the first `{` must contain it.
        let mut j = i + 6;
        while j < s.len() && s[j] != '{' {
            j += 1;
        }
        if j >= s.len() {
            break;
        }
        let between: String = s[i + 6..j].iter().collect();
        if !between.contains("prefers-reduced-motion") {
            i += 1;
            continue;
        }
        // The block: from after the brace, balanced.
        let start = j + 1;
        let mut depth = 1;
        let mut k = start;
        while k < s.len() && depth > 0 {
            if s[k] == '{' {
                depth += 1;
            } else if s[k] == '}' {
                depth -= 1;
            }
            k += 1;
        }
        out.push(s[start..k.saturating_sub(1)].iter().collect());
        i = k;
    }
    out
}

/// `css.replace(/\/\*[\s\S]*?\*\//g, "")` — comments out, NON-GREEDY, so two comments on one line
/// are two removals rather than one span from the first `/*` to the last `*/`.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let s: Vec<char> = css.chars().collect();
    let mut i = 0;
    while i < s.len() {
        if i + 1 < s.len() && s[i] == '/' && s[i + 1] == '*' {
            match (i + 2..s.len().saturating_sub(1)).find(|&k| s[k] == '*' && s[k + 1] == '/') {
                Some(k) => {
                    i = k + 2;
                    continue;
                }
                // An unclosed comment: the regex cannot match, so the rest is left as it is.
                None => break,
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// One rule: its selector list, whitespace-collapsed, and its body.
struct Rule {
    sel: String,
    body: String,
}

/// `([^{}]+)\{([^{}]*)\}` over the comment-stripped text — every rule the scan can see, in source
/// order.
///
/// The two character classes are the whole shape: a selector may not contain a brace and a body may
/// not contain one either, so a rule nested inside another block is invisible here and a `@keyframes`
/// body is read as a rule whose "selector" is `from`/`to`/`40%`.
fn rules(css: &str) -> Vec<Rule> {
    let clean = strip_comments(css);
    let s: Vec<char> = clean.chars().collect();
    let mut out = Vec::new();
    let mut seg_start = 0;
    let mut i = 0;
    while i < s.len() {
        if s[i] == '{' {
            let sel: String = s[seg_start..i].iter().collect();
            let mut j = i + 1;
            while j < s.len() && s[j] != '}' && s[j] != '{' {
                j += 1;
            }
            if j < s.len() && s[j] == '}' {
                let body: String = s[i + 1..j].iter().collect();
                // `sel.trim().replace(/\s+/g, " ")` — and `split_whitespace` is that for the ASCII
                // whitespace a selector is written with.
                out.push(Rule {
                    sel: sel.split_whitespace().collect::<Vec<_>>().join(" "),
                    body,
                });
                i = j + 1;
                seg_start = i;
                continue;
            }
            // A `{` before the next `}`: the regex cannot match starting here either.
            seg_start = i + 1;
            i += 1;
            continue;
        }
        if s[i] == '}' {
            seg_start = i + 1;
        }
        i += 1;
    }
    out
}

/// `sel.replace(/::?[a-z-]+(\([^)]*\))?/g, "").replace(/\[[^\]]*\]/g, "").trim()` — the selector
/// without its state qualifiers or its attribute tests.
fn base(sel: &str) -> String {
    let s: Vec<char> = sel.chars().collect();
    let mut stripped = String::with_capacity(sel.len());
    let mut i = 0;
    while i < s.len() {
        // `::?` — one colon, or two.
        if s[i] == ':' {
            let mut k = i + 1;
            if k < s.len() && s[k] == ':' {
                k += 1;
            }
            // `[a-z-]+`
            let name_start = k;
            while k < s.len() && (s[k].is_ascii_lowercase() || s[k] == '-') {
                k += 1;
            }
            if k > name_start {
                // `(\([^)]*\))?` — greedy, and it must CLOSE, or the group does not match and only
                // the name is removed.
                if k < s.len() && s[k] == '(' {
                    if let Some(close) = (k + 1..s.len()).find(|&m| s[m] == ')') {
                        k = close + 1;
                    }
                }
                i = k;
                continue;
            }
        }
        stripped.push(s[i]);
        i += 1;
    }
    // `\[[^\]]*\]` — the attribute tests.
    let t: Vec<char> = stripped.chars().collect();
    let mut out = String::with_capacity(stripped.len());
    let mut i = 0;
    while i < t.len() {
        if t[i] == '[' {
            if let Some(close) = (i + 1..t.len()).find(|&m| t[m] == ']') {
                i = close + 1;
                continue;
            }
        }
        out.push(t[i]);
        i += 1;
    }
    out.trim().to_string()
}

/// `(?:^|[;{\s])animation\s*:\s*([^;}]+)` — the FIRST animation declaration in a body, trimmed.
///
/// The leading class is what makes `animation-name:` NOT match: the colon must follow `animation`
/// (with only whitespace between), and there is a `-name` there instead.
fn animation_value(body: &str) -> Option<String> {
    let s: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < s.len() {
        // The boundary: start of the body, or one of `;`, `{`, or whitespace.
        let boundary = i == 0 || matches!(s[i - 1], ';' | '{') || s[i - 1].is_whitespace();
        if !boundary {
            i += 1;
            continue;
        }
        if !(i + 9 <= s.len() && s[i..i + 9].iter().collect::<String>() == "animation") {
            i += 1;
            continue;
        }
        let mut k = i + 9;
        while k < s.len() && s[k].is_whitespace() {
            k += 1;
        }
        if k >= s.len() || s[k] != ':' {
            i += 1;
            continue;
        }
        k += 1;
        while k < s.len() && s[k].is_whitespace() {
            k += 1;
        }
        let start = k;
        while k < s.len() && s[k] != ';' && s[k] != '}' {
            k += 1;
        }
        if k == start {
            // `[^;}]+` needs at least one character.
            i += 1;
            continue;
        }
        return Some(s[start..k].iter().collect::<String>().trim().to_string());
    }
    None
}

/// `/(?:^|[;{\s])animation(?:-name)?\s*:\s*(none|0)/` — does this body turn an animation OFF?
///
/// A TEST, not an anchored match: the value only has to START with `none` or `0`, so `0s` and
/// `none !important` both count, and `none` is what the panel writes.
fn silences(body: &str) -> bool {
    let s: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < s.len() {
        let boundary = i == 0 || matches!(s[i - 1], ';' | '{') || s[i - 1].is_whitespace();
        if !boundary {
            i += 1;
            continue;
        }
        if !(i + 9 <= s.len() && s[i..i + 9].iter().collect::<String>() == "animation") {
            i += 1;
            continue;
        }
        let mut k = i + 9;
        // `(?:-name)?`
        if k + 5 <= s.len() && s[k..k + 5].iter().collect::<String>() == "-name" {
            k += 5;
        }
        while k < s.len() && s[k].is_whitespace() {
            k += 1;
        }
        if k >= s.len() || s[k] != ':' {
            i += 1;
            continue;
        }
        k += 1;
        while k < s.len() && s[k].is_whitespace() {
            k += 1;
        }
        if s[k..].starts_with(&['n', 'o', 'n', 'e']) || s.get(k) == Some(&'0') {
            return true;
        }
        i += 1;
    }
    false
}

/// `/^(from|to|\d+%)$/` — a `@keyframes` step is not a selector that needs silencing.
fn is_keyframe_step(sel: &str) -> bool {
    if sel == "from" || sel == "to" {
        return true;
    }
    let s: Vec<char> = sel.chars().collect();
    if s.len() < 2 || *s.last().unwrap() != '%' {
        return false;
    }
    s[..s.len() - 1].iter().all(|c| c.is_ascii_digit())
}

/// The console's stylesheet directory, concatenated the way the `.mjs` did it — with `\n` between
/// files, and SORTED (see the header: the `.mjs` used `readdirSync` order, which is unspecified).
fn console_css() -> String {
    let dir = repo().join("gateway/ui/src/styles");
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("css"))
        .collect();
    files.sort();
    files
        .iter()
        .map(|p| {
            fs::read_to_string(p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every reason a sheet animates for someone who asked for no motion, in the order the `.mjs`
/// reported them, and the number of animations it counted on the way.
fn motion_failures(name: &str, css: &str) -> (Vec<String>, usize) {
    let blocks = reduced_motion_blocks(css);

    let mut silenced: Vec<String> = Vec::new();
    let mut global_stop = false;
    for block in &blocks {
        for r in rules(block) {
            if !silences(&r.body) {
                continue;
            }
            for one in r.sel.split(',') {
                let s = one.trim();
                // THE GLOBAL IDIOM, and `startsWith("*")` covers `*::before` and `*, *::after`.
                if s == "*" || s.starts_with('*') || s.contains("::before") {
                    global_stop = true;
                }
                silenced.push(s.to_string());
                silenced.push(base(s));
            }
        }
    }

    let mut failures = Vec::new();
    let mut animated = 0;
    for r in rules(css) {
        let Some(value) = animation_value(&r.body) else {
            continue;
        };
        // `animation: none` IS the off switch — not an animation that needs silencing.
        if value == "none" || value.starts_with("none") {
            continue;
        }
        animated += 1;
        for one in r.sel.split(',') {
            let s = one.trim();
            if is_keyframe_step(s) {
                continue;
            }
            if global_stop || silenced.iter().any(|x| x == s || *x == base(s)) {
                continue;
            }
            // `value.slice(0, 30)` — the first 30 CHARACTERS (the `.mjs` sliced UTF-16 units; every
            // sheet here is ASCII, and the difference is stated rather than assumed).
            let shown: String = value.chars().take(30).collect();
            failures.push(format!(
                "{name}: {s} runs \"{shown}\" and no prefers-reduced-motion block stops it"
            ));
        }
    }
    (failures, animated)
}

#[test]
fn nothing_animates_for_someone_who_asked_for_no_motion() {
    let panel = fs::read_to_string(repo().join("agent/resources/panel/panel.css"))
        .expect("the panel's built sheet");
    let console = console_css();

    let (mut failures, mut animated) = motion_failures("panel", &panel);
    let (console_failures, console_animated) = motion_failures("console", &console);
    failures.extend(console_failures);
    animated += console_animated;

    // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN. Both sheets ran 16 animations when this was
    // written; the floor is 10, and it is the `.mjs`'s own.
    assert!(
        animated >= 10,
        "motion-check: FAILED — found {animated} running animations, which is too few to be reading both sheets"
    );

    if !failures.is_empty() {
        let report = failures
            .iter()
            .map(|f| format!("  {f}"))
            .collect::<Vec<_>>()
            .join("\n");
        panic!(
            "motion-check: FAILED — motion that cannot be turned off:\n{report}\n\n  A `--dur: 0s` token does NOT \
             reach an `animation:` that declares its own duration. Name the\n  selector in a \
             `prefers-reduced-motion` block, and keep the STATE readable without the motion."
        );
    }

    println!(
        "motion-check: ok — {animated} animations across both sheets, every one silenced under prefers-reduced-motion"
    );
}
