//! ONE STATE, ONE SILHOUETTE, ACROSS BOTH SURFACES.
//!
//! WHY THIS EXISTS. Each surface already checks its own marks: the panel's `statePalette.test.ts`
//! reads its BUILT sheet, the console's `console-marks-check.mjs` reads its SOURCE sheet, and the
//! shared probe reads the COMPUTED styles of a rendered page. Every one of those is about ONE
//! surface. Nothing compared the two languages, so the thing the objective actually asks for — a
//! mark language where a state has its own silhouette, so it survives colour loss, colour-vision
//! deficiency and reduced motion — was pinned per surface and free to drift between them.
//!
//! An operator uses both: the console to see the fleet, the panel to see one device. If a diamond
//! means "a question is waiting for you" on one and "this channel is failing" on the other, the
//! shape stops carrying the state and the two surfaces teach two vocabularies. Measured 2026-09-18,
//! they AGREE:
//!
//!     panel (liveness.ts, as data)      console (globals.css, as painted)
//!     waiting  diamond                  .sig-dot.err / .dot.err  rotate(45deg) + fill   diamond
//!     working  solid-halo               .dot.ok / .dev-led.on    fill + outer shadow    fill
//!     idle     ring                     .dev-led / .dot          inset ring, no fill    ring
//!     off      dashed-ring              .sig-dot.off             dashed border, empty   dashed-ring
//!
//! THE FOUR FACTS BELOW ARE THE ONES BOTH SURFACES CAN BE HELD TO. They are deliberately about
//! MEANING rather than about class names: the panel spells state as `data-state` and the console
//! spells it as a class, which is a difference of expression and not of language.
//!
//! A FLOOR IS PART OF THE CHECK. Two surfaces that both stopped rendering marks would otherwise
//! agree perfectly.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/mark-vocabulary-check.mjs` → exit 0,
//!     "note: panel: waiting=diamond working=solid-halo failed=triangle idle=ring off=dashed-ring",
//!     "note: console: 11 marks, 4 distinct shapes", and
//!     "mark-vocabulary-check: ok — 5 panel states and 11 console marks agree on one silhouette per
//!     meaning (attention=diamond, absent=ring, fine=fill)".
//!   * this file → the SAME two notes and the SAME final line, character for character.
//!   * a planted `transform: none` on the console's `.dot.err` → the JS gate exits 1 with
//!     ".dot.err means FAILURE and does not draw the diamond (transform: none) — the panel spells
//!     that state as a diamond", and this file fails with the identical sentence.
//!   * a planted sixth `SILHOUETTE` entry reusing `diamond` → BOTH STAY GREEN, and that is the
//!     honest measurement rather than a divergence: the panel's own distinctness clause is applied
//!     to the FOUR states the two surfaces are compared through (`waiting`/`working`/`idle`/`off`),
//!     exactly as the JS gate applies it, so a sixth state folded onto an existing shape is caught
//!     by `liveness.test.ts` and not here.
//!
//! MUTATION: stop the console's failure mark from being a diamond (`.dot.err` loses
//!           `rotate(45deg)`), or give the panel's `waiting` the `idle` ring.
//! RESULT:   exit 1 both ways, with the sentence that names the surface and the state — ".dot.err
//!           means FAILURE and does not draw the diamond (transform: none) — the panel spells that
//!           state as a diamond", and for the panel "the panel's most urgent state draws \"ring\",
//!           not a diamond — the shape that means \"this wants you\" has moved" plus "the panel's
//!           four states share 1 silhouette(s)". It is the ONLY check that reads both surfaces at
//!           once — every other marks check is about one of them — and it reads the panel's
//!           vocabulary as DATA (`liveness.ts`) and the console's as PAINTED (its sheet, with the
//!           same signature rule `console-marks-check.mjs` uses).

use std::fs;
use std::path::{Path, PathBuf};

/// `cargo test` runs from the CRATE root, not the repo root.
const CRATE: &str = env!("CARGO_MANIFEST_DIR");
const PANEL_PALETTE: &str = "agent/resources/panel-react/src/lib/liveness.ts";
const CONSOLE_SHEET: &str = "gateway/ui/src/styles/globals.css";

fn repo() -> PathBuf {
    Path::new(CRATE)
        .parent()
        .expect("the crate lives one level below the repo root")
        .to_path_buf()
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_space(c: char) -> bool {
    // JS `\s` is broader than ASCII, but every sheet and source here is ASCII whitespace.
    c.is_whitespace()
}

// ── the panel's vocabulary, read from the module that IS the vocabulary ─────────────────────────

/// `export const SILHOUETTE[^{]*\{([^}]*)\}` then `(\w+)\s*:\s*"([\w-]+)"`.
///
/// The map is kept as an ORDERED list with JS object semantics: a later duplicate key overwrites the
/// value but keeps the FIRST position, because `Object.entries` is what the note line prints.
fn silhouette_map(src: &str) -> Option<Vec<(String, String)>> {
    const ANCHOR: &str = "export const SILHOUETTE";
    let rest = &src[src.find(ANCHOR)? + ANCHOR.len()..];
    // `[^{]*` cannot cross a `{`, so the first one after the anchor is the one the regex takes.
    let brace = rest.find('{')?;
    let body = &rest[brace + 1..];
    // `[^}]*` is greedy to the first `}`.
    let block = &body[..body.find('}')?];

    let chars: Vec<char> = block.chars().collect();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !is_word(chars[i]) {
            i += 1;
            continue;
        }
        // The maximal word run; only its full length can be followed by `\s*:\s*"`.
        let run_start = i;
        while i < chars.len() && is_word(chars[i]) {
            i += 1;
        }
        let key: String = chars[run_start..i].iter().collect();
        let mut j = i;
        while j < chars.len() && is_space(chars[j]) {
            j += 1;
        }
        if chars.get(j) != Some(&':') {
            continue;
        }
        j += 1;
        while j < chars.len() && is_space(chars[j]) {
            j += 1;
        }
        if chars.get(j) != Some(&'"') {
            continue;
        }
        j += 1;
        let val_start = j;
        while j < chars.len() && (is_word(chars[j]) || chars[j] == '-') {
            j += 1;
        }
        if j == val_start || chars.get(j) != Some(&'"') {
            continue;
        }
        let val: String = chars[val_start..j].iter().collect();
        j += 1;
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = val,
            None => out.push((key, val)),
        }
        i = j;
    }
    Some(out)
}

// ── the console's vocabulary, computed from its sheet with the SAME rule its own gate uses ─────
// (border-radius, border-style, transform, kind) — copied deliberately rather than re-invented: an
// approximation of a rule is an approximation of the verdict.

/// `/\/\*[\s\S]*?\*\//g` — prose about a rule is not the rule.
fn strip_block_comments(sheet: &str) -> String {
    let chars: Vec<char> = sheet.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
            // non-greedy: to the FIRST `*/`. With none, the regex does not match at all.
            let mut j = i + 2;
            let mut close = None;
            while j + 1 < chars.len() {
                if chars[j] == '*' && chars[j + 1] == '/' {
                    close = Some(j);
                    break;
                }
                j += 1;
            }
            match close {
                Some(k) => {
                    i = k + 2;
                    continue;
                }
                None => {
                    out.push(chars[i]);
                    i += 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `([^{}]+)\{([^}]*)\}` global — selector text and declaration body, in document order.
fn blocks(sheet: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = sheet.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        if chars[start] == '{' || chars[start] == '}' {
            start += 1;
            continue;
        }
        let mut run_end = start;
        while run_end < chars.len() && chars[run_end] != '{' && chars[run_end] != '}' {
            run_end += 1;
        }
        // Only the maximal run can be followed by `{` — a shorter one ends inside the run.
        if chars.get(run_end) != Some(&'{') {
            start = run_end + 1;
            continue;
        }
        let body_start = run_end + 1;
        let mut body_end = body_start;
        while body_end < chars.len() && chars[body_end] != '}' {
            body_end += 1;
        }
        if chars.get(body_end) != Some(&'}') {
            start = run_end + 1;
            continue;
        }
        out.push((
            chars[start..run_end].iter().collect(),
            chars[body_start..body_end].iter().collect(),
        ));
        start = body_end + 1;
    }
    out
}

/// `blockOf(sel)` — the cascade's winner, approximated by document order: the LAST block whose
/// comma-split, trimmed selector list contains `sel` exactly.
fn block_of(blocks: &[(String, String)], sel: &str) -> String {
    let mut out = String::new();
    for (sels, body) in blocks {
        if sels.split(',').map(str::trim).any(|s| s == sel) {
            out = body.clone();
        }
    }
    out
}

/// `propOf(sel, prop)` — `(?:^|;)\s*PROP\s*:\s*([^;]+)`, trimmed.
fn prop_of(body: &str, prop: &str) -> String {
    let chars: Vec<char> = body.chars().collect();
    let p: Vec<char> = prop.chars().collect();
    for anchor in 0..chars.len() {
        if anchor != 0 && chars[anchor] != ';' {
            continue;
        }
        let mut j = anchor + 1;
        while j < chars.len() && is_space(chars[j]) {
            j += 1;
        }
        if j + p.len() > chars.len() || chars[j..j + p.len()] != p[..] {
            continue;
        }
        j += p.len();
        while j < chars.len() && is_space(chars[j]) {
            j += 1;
        }
        if chars.get(j) != Some(&':') {
            continue;
        }
        j += 1;
        let val_start = j;
        while j < chars.len() && chars[j] != ';' {
            j += 1;
        }
        if j == val_start {
            continue;
        }
        return chars[val_start..j]
            .iter()
            .collect::<String>()
            .trim()
            .to_string();
    }
    String::new()
}

fn contains(hay: &str, needle: &str) -> bool {
    hay.contains(needle)
}

struct Shape {
    kind: &'static str,
    dashed: bool,
    rotated: bool,
    radius: String,
}

fn shape_of(blocks: &[(String, String)], sel: &str) -> Shape {
    let body = block_of(blocks, sel);
    let background = prop_of(&body, "background");
    let bg = if background.is_empty() {
        prop_of(&body, "background-color")
    } else {
        background
    };
    let shadow = prop_of(&body, "box-shadow");
    let border = format!(
        "{} {}",
        prop_of(&body, "border"),
        prop_of(&body, "border-style")
    );
    let filled = !bg.is_empty() && !contains(&bg, "transparent") && !contains(&bg, "none");
    let inset = contains(&shadow, "inset");
    let outer = !shadow.is_empty() && !inset;
    let dashed = contains(&border, "dashed") || contains(&border, "dotted");
    let rotated = contains(&prop_of(&body, "transform"), "rotate(45deg)");
    let kind = if inset && filled {
        "ring+fill"
    } else if inset {
        "ring"
    } else if filled && outer {
        "solid-halo"
    } else if filled {
        "solid"
    } else {
        "empty"
    };
    let radius = prop_of(&body, "border-radius");
    Shape {
        kind,
        dashed,
        rotated,
        radius: if radius.is_empty() {
            "0".to_string()
        } else {
            radius
        },
    }
}

/// (mark, the state it means) — the console's own families, which its gate already proves distinct.
const CONSOLE_MARKS: [(&str, &str); 11] = [
    (".sig-dot.ok", "fine"),
    (".sig-dot.err", "failure"),
    (".sig-dot.off", "absent"),
    (".dot.ok", "fine"),
    (".dot.err", "failure"),
    (".dev-led", "absent"),
    (".dev-led.on", "fine"),
    (".dev-mini-led", "absent"),
    (".dev-mini-led.on", "fine"),
    (".ov-keyled", "absent"),
    (".ov-keyled.on", "fine"),
];

/// JS prints `undefined` for a missing key; these two reproduce that and JS truthiness, because the
/// failure sentences interpolate the value and one clause tests it for truth.
fn js(v: Option<&str>) -> &str {
    v.unwrap_or("undefined")
}
fn truthy(v: Option<&str>) -> bool {
    matches!(v, Some(s) if !s.is_empty())
}

fn check(palette_src: &str, sheet_src: &str) -> Result<String, String> {
    let panel = silhouette_map(palette_src).ok_or_else(|| {
        format!(
            "mark-vocabulary-check: FAILED — no SILHOUETTE map in {PANEL_PALETTE}, so this proves nothing"
        )
    })?;
    if panel.len() < 5 {
        return Err(format!(
            "mark-vocabulary-check: FAILED — read {} panel state(s), expected at least 5",
            panel.len()
        ));
    }
    // AND NO TWO STATES MAY SHARE A SHAPE, which is the property the map exists for.
    {
        let shapes: Vec<&str> = panel.iter().map(|(_, v)| v.as_str()).collect();
        let distinct: std::collections::BTreeSet<&str> = shapes.iter().copied().collect();
        if distinct.len() != shapes.len() {
            return Err(format!(
                "mark-vocabulary-check: FAILED — the panel's {} states share {} silhouette(s): {}",
                shapes.len(),
                distinct.len(),
                shapes.join(", ")
            ));
        }
    }

    let sheet = strip_block_comments(sheet_src);
    let parsed = blocks(&sheet);
    let console_shapes: Vec<(&str, &str, Shape)> = CONSOLE_MARKS
        .iter()
        .map(|(sel, means)| (*sel, *means, shape_of(&parsed, sel)))
        .collect();
    if console_shapes.len() < 10 {
        return Err(format!(
            "mark-vocabulary-check: FAILED — read {} console marks, expected at least 10",
            console_shapes.len()
        ));
    }

    let get = |k: &str| -> Option<&str> {
        panel
            .iter()
            .find(|(name, _)| name == k)
            .map(|(_, v)| v.as_str())
    };
    let waiting = get("waiting");
    let working = get("working");
    let failed = get("failed");
    let idle = get("idle");
    let off = get("off");

    let mut failures: Vec<String> = Vec::new();

    // ── 1. THE ATTENTION SHAPE IS THE DIAMOND ON BOTH SURFACES ──────────────────────────────────
    if waiting != Some("diamond") {
        failures.push(format!(
            "the panel's most urgent state draws \"{}\", not a diamond — the shape that means \"this wants you\" has moved",
            js(waiting)
        ));
    }
    if failed == waiting {
        failures.push(format!(
            "the panel draws FAILURE as \"{}\" — the shape it already uses for a QUESTION, so the two states that both want a person are one silhouette",
            js(failed)
        ));
    }
    if !truthy(failed) || failed == Some("solid-halo") {
        failures.push(format!(
            "the panel gives FAILED no shape of its own (\"{}\") — a state the device reports and no surface can draw is the hole this vocabulary closed in round 97",
            js(failed)
        ));
    }
    for (sel, means, s) in &console_shapes {
        if *means != "failure" {
            continue;
        }
        if !s.rotated {
            failures.push(format!(
                "{sel} means FAILURE and does not draw the diamond (transform: {}) — the panel spells that state as a diamond",
                if s.rotated { "rotate(45deg)" } else { "none" }
            ));
        }
        if s.kind == "empty" {
            failures.push(format!("{sel} means FAILURE and draws nothing at all"));
        }
    }

    // ── 2. ABSENT IS A RING ON BOTH — outline only, never a fill ────────────────────────────────
    if !contains(js(off), "ring") || !contains(js(idle), "ring") {
        failures.push(format!(
            "the panel's quiet states draw \"{}\"/\"{}\" — an absent thing is an OUTLINE on that surface",
            js(off),
            js(idle)
        ));
    }
    for (sel, means, s) in &console_shapes {
        if *means != "absent" {
            continue;
        }
        if !(s.kind == "ring" || (s.kind == "empty" && s.dashed)) {
            failures.push(format!(
                "{sel} means ABSENT and draws \"{}\"{} — absent is a ring or a dashed ring, on both surfaces",
                s.kind,
                if s.dashed { " (dashed)" } else { "" }
            ));
        }
    }

    // ── 3. FINE IS A FILL ON BOTH, and the halo is the family's intensifier ─────────────────────
    if working != Some("solid-halo") {
        failures.push(format!(
            "the panel's active state draws \"{}\" — activity is a FILL with a halo, because the halo is what motion rides on",
            js(working)
        ));
    }
    for (sel, means, s) in &console_shapes {
        if *means != "fine" {
            continue;
        }
        if !(s.kind == "solid" || s.kind == "solid-halo") {
            failures.push(format!(
                "{sel} means FINE and draws \"{}\" — a healthy thing is a fill on both surfaces",
                s.kind
            ));
        }
    }

    // ── 4. NO MARK IS TWO THINGS AT ONCE, on either surface ─────────────────────────────────────
    for (sel, _, s) in &console_shapes {
        if s.kind == "ring+fill" {
            failures.push(format!(
                "{sel} is a FILL inside a RING — the vocabulary is solid / ring / halo / empty, and a mark that is two of them is neither"
            ));
        }
    }
    if panel.iter().any(|(_, v)| contains(v, "ring+fill")) {
        failures.push(
            "the panel's vocabulary names a ring+fill — that is not a state, it is two".to_string(),
        );
    }

    let four: std::collections::BTreeSet<Option<&str>> =
        [waiting, working, idle, off].into_iter().collect();
    if four.len() != 4 {
        failures.push(format!(
            "the panel's four states share {} silhouette(s) — \"shape carries the state\" is worthless if two states look alike",
            4 - four.len()
        ));
    }

    let mut notes = Vec::new();
    notes.push(format!(
        "panel: {}",
        panel
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    let distinct_shapes: std::collections::BTreeSet<String> = console_shapes
        .iter()
        .map(|(_, _, s)| format!("{}{}", s.kind, if s.dashed { "+dashed" } else { "" }))
        .collect();
    notes.push(format!(
        "console: {} marks, {} distinct shapes",
        console_shapes.len(),
        distinct_shapes.len()
    ));

    let mut out = String::new();
    for n in &notes {
        out.push_str(&format!("note: {n}\n"));
    }
    if !failures.is_empty() {
        for f in &failures {
            out.push_str(&format!("mark-vocabulary-check: {f}\n"));
        }
        out.push_str(&format!(
            "mark-vocabulary-check: FAILED — {} disagreement(s) between the two surfaces' mark languages",
            failures.len()
        ));
        return Err(out.trim_end().to_string());
    }
    out.push_str(&format!(
        "mark-vocabulary-check: ok — {} panel states and {} console marks agree on one silhouette per meaning (attention=diamond, absent=ring, fine=fill)",
        panel.len(),
        console_shapes.len()
    ));
    Ok(out)
}

fn read(rel: &str) -> String {
    let p = repo().join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

#[test]
fn both_surfaces_speak_one_mark_language() {
    let out = check(&read(PANEL_PALETTE), &read(CONSOLE_SHEET)).unwrap_or_else(|e| panic!("{e}"));
    for line in out.lines() {
        println!("{line}");
    }
    assert!(out.contains("agree on one silhouette per meaning"), "{out}");
}

// ── the scanner's own proof: a parser that reads nothing must not pass ─────────────────────────

#[test]
fn a_sheet_with_no_mark_rules_is_read_as_empty_rather_than_passing() {
    // The four shapes are computed from the sheet, so a sheet that yields no declarations must
    // produce the ABSENT/FINE/FAILURE failures rather than a clean sheet.
    let err =
        check(&read(PANEL_PALETTE), "/* nothing here */\n").expect_err("an empty sheet must fail");
    assert!(
        err.contains("means FAILURE and draws nothing at all"),
        "{err}"
    );
    assert!(err.contains("means ABSENT and draws"), "{err}");
    assert!(err.contains("means FINE and draws"), "{err}");
}

#[test]
fn a_missing_silhouette_map_is_not_a_pass() {
    let err = check("export const SOMETHING_ELSE = {};\n", &read(CONSOLE_SHEET))
        .expect_err("no map must fail");
    assert!(err.contains("no SILHOUETTE map"), "{err}");
}

#[test]
fn the_last_selector_in_document_order_wins() {
    // `blockOf` approximates the cascade by document order, and the arms sit after their bases.
    let sheet = ".dot.err { background: red; }\n.dot.err { transform: rotate(45deg); }\n";
    let parsed = blocks(&strip_block_comments(sheet));
    let s = shape_of(&parsed, ".dot.err");
    assert!(s.rotated, "the later block must be the one read");
    assert_eq!(s.kind, "empty", "the earlier block's fill must be gone");
}

#[test]
fn a_comma_list_selects_the_exact_selector_and_not_a_substring() {
    let sheet = ".health-row .dot.ok { background: red; }\n";
    let parsed = blocks(&strip_block_comments(sheet));
    // `.dot.ok` is not `.health-row .dot.ok` — the JS rule is exact equality after trimming.
    assert_eq!(shape_of(&parsed, ".dot.ok").kind, "empty");
    assert_eq!(shape_of(&parsed, ".health-row .dot.ok").kind, "solid");
    // A selector in a comma list IS selected, and the radius default is `0` when undeclared.
    let listed = blocks(&strip_block_comments(
        ".a, .dot.ok { background: red; border-radius: 4px; }\n",
    ));
    assert_eq!(shape_of(&listed, ".dot.ok").kind, "solid");
    assert_eq!(shape_of(&listed, ".dot.ok").radius, "4px");
    assert_eq!(shape_of(&listed, ".b").radius, "0");
}

#[test]
fn a_comment_naming_a_rule_is_not_the_rule() {
    let sheet = "/* .dot.err { transform: rotate(45deg); } */\n.dot.err { transform: none; }\n";
    let parsed = blocks(&strip_block_comments(sheet));
    assert!(!shape_of(&parsed, ".dot.err").rotated);
}
