//! THE PANEL'S SPACING IS MOSTLY AD-HOC, AND IT MAY NOT GET WORSE.
//!
//! WHY THIS EXISTS (round 220). The objective names five axes — spacing, hierarchy, contrast,
//! typography, states — and four of them have rendered checks. Spacing had geometry, overflow,
//! reflow and the 2.5.8 spacing clause; nothing looked at the SCALE itself. Measured for the
//! first time:
//!
//!     token uses     151   (5 of the 6 scale steps: --sp-0-5/1/2/3/4/5 = 2/4/8/12/16/24px)
//!     literal px     539
//!     off-scale      305 uses across 20 distinct values — 6px x62, 10px x61, 14px x32, 5px x31 …
//!
//! So 44% of the panel's spacing is off-scale and the scale carries 22%. REWRITING 305
//! DECLARATIONS IS NOT THIS GATE'S BUSINESS: spacing is the one axis where a mechanical change
//! is VISIBLE, and a late sweep of 305 values would be a redesign wearing a refactor's clothes.
//! What is worth having is the measurement held still: the counts may improve and may not get
//! worse, the same ratchet the unstyled scan's floor and the coverage floor use. A new ad-hoc
//! `13px` fails this, and so does deleting a token use to make room for one.
//!
//! ONE STEP IS FREE: --sp-5 (24px) is defined and used nowhere, in any of the UIs (round 221).
//! KEPT, not pruned, and the distinction matters: an unused DECLARATION is dead weight, but an
//! unused STEP IN A SCALE is a slot the system offers. Deleting it would leave a scale with a
//! hole in it, which is worse than a token that is waiting.
//!
//! Deliberately counts EVERY literal, including the legitimate ones (0 is skipped; 1px
//! hairlines and negative optical adjustments are counted and ratcheted like the rest). An
//! allow-list would need a reason per entry and would drift; a number that may only improve
//! does not.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/spacing-scale-check.mjs` → exit 0, "spacing scale: ok — panel N token
//!     / M off-scale · console N token / M off-scale".
//!   * this file → ok, with the SAME four numbers.
//!   * a planted `gap: 13px` in the panel's sheet → both fail with "off-scale spacing rose
//!     from M to M+1".
//!
//! MUTATION: add a spacing declaration with an off-scale literal (a planted `13px`), or an
//!           ON-scale one where a token exists (a planted `8px`).
//! RESULT:   fails either way: "off-scale spacing rose from M to M+1", or "on-scale literals
//!           rose from 0 to 1 — a value that HAS a token was written out by hand, which is how
//!           234 of them accumulated unnoticed". A token use replaced by a literal fails the
//!           third count.
//!
//! RE-READ THE BASELINES WHENEVER THE COUNTS IMPROVE (round 171). The baselines were taken in
//! rounds 220-222; by round 171's re-measurement the counts had FALLEN to 299 off-scale and
//! RISEN to 397 token uses, and a one-way ratchet compares against the OLD number — so it
//! silently tolerated SIX new off-scale literals before it would fire. A ratchet that is not
//! tightened is slack, and slack is the vacuity this suite exists to catch in others. When a
//! round legitimately improves them, move them in the same commit.

use std::fs;
use std::path::{Path, PathBuf};

/// `cargo test` runs from the CRATE root, not the repo root.
const CRATE: &str = env!("CARGO_MANIFEST_DIR");

fn repo() -> PathBuf {
    Path::new(CRATE)
        .parent()
        .expect("the crate lives one level below the repo root")
        .to_path_buf()
}

const SCALE: [f64; 7] = [0.0, 2.0, 4.0, 8.0, 12.0, 16.0, 24.0];

const PROPS: [&str; 13] = [
    "gap",
    "row-gap",
    "column-gap",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "padding",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "margin",
];

struct Ui {
    name: &'static str,
    dir: &'static str,
    /// Each may only move in the good direction.
    token_uses: usize,
    off_scale_uses: usize,
    on_scale_literals: usize,
}

/// THREE UIs UNTIL ROUND 243, when the extension was removed (it shipped nowhere and its
/// feature was off by default); its baseline went with it rather than being kept at zero for a
/// directory that no longer exists.
const UIS: [Ui; 2] = [
    Ui {
        name: "panel",
        dir: "agent/resources/panel-react/src/styles",
        token_uses: 397,
        off_scale_uses: 299,
        // ZERO, and this number was INVISIBLE for 230 rounds. The check counted token uses and
        // OFF-scale literals, so a value already ON the scale — `padding: 8px` where
        // `var(--sp-2)` exists — fell into neither and could accumulate forever. It did: 234 of
        // them, in the panel alone.
        on_scale_literals: 0,
    },
    Ui {
        name: "console",
        dir: "gateway/ui/src/styles",
        // 86 token uses as of round 223: the console adopted the panel's scale, and every one
        // of those 86 was already a literal with that exact value, so NO PIXEL MOVED.
        token_uses: 87,
        // 136, NOT the 194 an ad-hoc scan reported: that scan swept gateway/public/style.css as
        // well, which round 209 established is DEAD (nothing links it). A baseline has to come
        // from the instrument that will enforce it.
        off_scale_uses: 132,
        on_scale_literals: 0,
    },
];

struct Counts {
    token_uses: usize,
    off_scale_uses: usize,
    on_scale_literals: usize,
    offenders: Vec<String>,
}

/// The spacing declarations in one stylesheet, as (value, source-file) pairs.
///
/// The JS gate matches `(?:^|[\s;{])(gap|…)\s*:\s*([^;}]+)`; this walks the same shapes by
/// hand because the crate carries no `regex`.
fn declarations(css: &str, file: &str) -> Vec<(String, String)> {
    let s: Vec<char> = css.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let boundary = i == 0 || matches!(s[i - 1], ' ' | '\t' | '\n' | '\r' | ';' | '{');
        if !boundary {
            i += 1;
            continue;
        }
        let mut matched = None;
        for p in PROPS {
            let pc: Vec<char> = p.chars().collect();
            if s.len() < i + pc.len() || s[i..i + pc.len()] != pc[..] {
                continue;
            }
            let mut j = i + pc.len();
            while j < s.len() && s[j].is_whitespace() {
                j += 1;
            }
            if s.get(j) != Some(&':') {
                continue;
            }
            j += 1;
            while j < s.len() && s[j].is_whitespace() {
                j += 1;
            }
            let start = j;
            while j < s.len() && s[j] != ';' && s[j] != '}' {
                j += 1;
            }
            matched = Some((s[start..j].iter().collect::<String>(), j));
            break;
        }
        match matched {
            Some((value, j)) => {
                out.push((value, file.to_string()));
                i = j;
            }
            None => i += 1,
        }
    }
    out
}

/// `var(--x)` occurrences in a declaration's value.
fn token_count(value: &str) -> usize {
    let s: Vec<char> = value.chars().collect();
    let needle: Vec<char> = "var(--".chars().collect();
    let mut n = 0;
    let mut i = 0;
    while i + needle.len() <= s.len() {
        if s[i..i + needle.len()] == needle[..] {
            n += 1;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    n
}

/// `-?\d+(?:\.\d+)?px` — the literal lengths in a declaration's value.
fn px_values(value: &str) -> Vec<f64> {
    let s: Vec<char> = value.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let start = i;
        if s[i] == '-' {
            i += 1;
        }
        let digits = i;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
        if i == digits {
            i = start + 1;
            continue;
        }
        if s.get(i) == Some(&'.') {
            let mut k = i + 1;
            while k < s.len() && s[k].is_ascii_digit() {
                k += 1;
            }
            if k > i + 1 {
                i = k;
            }
        }
        if s.get(i) == Some(&'p') && s.get(i + 1) == Some(&'x') {
            let text: String = s[start..i].iter().collect();
            if let Ok(n) = text.parse::<f64>() {
                out.push(n);
            }
            i += 2;
        } else {
            i = start + 1;
        }
    }
    out
}

fn sheets(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "css"))
        .collect();
    v.sort();
    v
}

fn count(ui: &Ui, root: &Path) -> Counts {
    let mut c = Counts {
        token_uses: 0,
        off_scale_uses: 0,
        on_scale_literals: 0,
        offenders: Vec::new(),
    };
    for file in sheets(&root.join(ui.dir)) {
        let css = fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        for (value, _) in declarations(&css, &name) {
            c.token_uses += token_count(&value);
            for n in px_values(&value) {
                if n == 0.0 {
                    continue;
                }
                if SCALE.contains(&n) {
                    c.on_scale_literals += 1;
                    continue;
                }
                c.off_scale_uses += 1;
                if c.offenders.len() < 3 {
                    let head: String = value.trim().chars().take(28).collect();
                    c.offenders.push(format!("{name}: {head}"));
                }
            }
        }
    }
    c
}

#[test]
fn the_spacing_scale_may_only_improve() {
    let root = repo();
    let mut failures = Vec::new();
    let mut lines = Vec::new();
    for ui in &UIS {
        let c = count(ui, &root);
        if c.off_scale_uses > ui.off_scale_uses {
            failures.push(format!(
                "{}: off-scale spacing rose from {} to {} — {}",
                ui.name,
                ui.off_scale_uses,
                c.off_scale_uses,
                c.offenders.join(" | ")
            ));
        }
        if c.token_uses < ui.token_uses {
            failures.push(format!(
                "{}: token uses fell from {} to {}",
                ui.name, ui.token_uses, c.token_uses
            ));
        }
        if c.on_scale_literals > ui.on_scale_literals {
            failures.push(format!(
                "{}: on-scale literals rose from {} to {} — a value that HAS a token was written out by hand, \
                 which is how 234 of them accumulated unnoticed",
                ui.name, ui.on_scale_literals, c.on_scale_literals
            ));
        }
        lines.push(format!(
            "{} {} token / {} off-scale",
            ui.name, c.token_uses, c.off_scale_uses
        ));
    }
    assert!(
        failures.is_empty(),
        "spacing scale: FAILED\n  {}\n\nUse a `--sp-*` token, or tighten the baseline in this file in the SAME \
         commit that legitimately improves the count.",
        failures.join("\n  ")
    );
    println!("spacing scale: ok — {}", lines.join(" · "));
}

#[test]
fn the_scanner_finds_a_declaration_and_its_literals() {
    let d = declarations(".a { gap: var(--sp-2); padding: 8px 13px; }", "x.css");
    assert_eq!(d.len(), 2, "{d:?}");
    assert_eq!(token_count(&d[0].0), 1);
    assert_eq!(px_values(&d[1].0), vec![8.0, 13.0]);
    // A negative optical adjustment is counted like the rest, deliberately.
    assert_eq!(px_values("-2px"), vec![-2.0]);
    // `.5px` yields 5: the pattern needs a digit immediately before `px`, and the `.` is
    // simply not part of the number it finds. Matched against the JS regex rather than
    // assumed — both report [5].
    assert_eq!(px_values(".5px"), vec![5.0]);
    // `padding-top` is its own property, not `padding` followed by `-top`.
    assert_eq!(declarations(".a { padding-top: 4px; }", "x.css").len(), 1);
}
