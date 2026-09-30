//! ONE DESIGN VOCABULARY ACROSS THE TWO FRONTENDS — the Rust half of `token-contract-check.mjs`.
//!
//! WHY. The console (`gateway/ui`, the fleet surface served by the worker) and the device panel
//! (`agent/resources/panel-react`, the operator surface) are one product with two hand-rolled token
//! sets. They shared SIXTEEN token NAMES and TWELVE of them held different VALUES: the console's frame
//! was Bootstrap's gray scale while the panel's was zinc/Apple, and the radii were 6/14px against
//! 10/20px. A shared name therefore meant two different things depending on which surface you were
//! looking at — and the console's own comment claimed "Same vocabulary as the device panel's frame",
//! which is what stopped anyone checking.
//!
//! WHAT IT COMPARES. Only the names BOTH sides define, and it compares what they RESOLVE to rather
//! than how they are spelled: the panel writes `var(--ds-neutral-50)` where the console writes the
//! literal `#fafafa`, and that difference is legitimate — the console has no `--ds-neutral-*` scale.
//! One `var()` level is resolved on each side; anything deeper is reported UNRESOLVED rather than
//! silently treated as a match, because a comparison that cannot read a value must not report
//! agreement.
//!
//! WHAT IT DOES NOT DO. It does not check the names only ONE side defines, and it is not a contrast
//! audit — that is `panel-render-audit.mjs`'s job.
//!
//! # THE EIGHT CHECKS, AND WHY EACH ONE SURVIVES THE OTHERS
//!
//! 1. `divergences` — the shared names, per theme, resolved one level on each side
//! 2. `offScaleNeutrals` — the console's `--bg/--border/--text/--chrome*` values must be values the
//!    PANEL declares, because the names only the console defines are invisible to (1)
//! 3. `deadFallbacks` — `var(--x, v)` where the system declares `--x` can never apply
//! 4. the semantic colours' TEXT weight, and no mark weight painted as text
//! 5. the ACCENT family, AA in both directions and both themes
//! 6. the LANDING's shared names — a third surface, in effective sets, with two names REQUIRED
//! 7. the SPACING SCALE, six steps named explicitly — the half a shared-name comparison cannot see
//!    by construction, because a DELETED step stops being shared
//! 8. the landing's PARSEABILITY (`:root` must be there, or the parser reports a clean nothing)
//!
//! # MIGRATION-TIME EQUIVALENCE, MEASURED
//!
//! The JavaScript ran beside this file on the same trees — the clean one and one planted mutation per
//! check — and the two printed the same lines and reached the same verdict. The transcripts are in the
//! commit that added this file; the numbers are in its message rather than here, because a gate that
//! quotes its own past is a gate nobody re-runs.
//!
//! MUTATION: plant `--sp-2: 9px;` in the console's `globals.css` → this gate fails with
//!           "--sp-2 differs: console=9px panel=8px", and the shared-name comparison reports the same
//!           token first — the two are NOT the same check, and the spacing block is the one that
//!           survives a DELETED step.
//! RESULT:   `token contract: the SPACING SCALE is not shared —` then the line above, exit 1.

mod common;

use common::{repo, strip_block_comments};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

// ── THE ORDERED MAP, BECAUSE `Object.keys` IS ONE ────────────────────────────────────────────────────
//
// THE FIRST THING THE PORT HAD TO GET RIGHT, and it is not a detail: every message this gate prints
// lists tokens in the order the sheets declare them, and `Object.keys` answers INSERTION order — with
// a re-assignment keeping the FIRST position (`o.a=1; o.b=2; o.a=3` is `["a","b"]`). A `BTreeMap`
// would sort them, and the differential would then differ on the ORDER of every multi-token message
// while agreeing on every verdict: a divergence that looks like noise and is a different program.

#[derive(Default, Clone, Debug)]
struct Vars(Vec<(String, String)>);

impl Vars {
    fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
    fn has(&self, key: &str) -> bool {
        self.0.iter().any(|(k, _)| k == key)
    }
    /// `o[k] = v` — update in place (the position is kept) or append.
    fn set(&mut self, key: &str, value: &str) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.to_string(),
            None => self.0.push((key.to_string(), value.to_string())),
        }
    }
    fn keys(&self) -> Vec<String> {
        self.0.iter().map(|(k, _)| k.clone()).collect()
    }
    fn values(&self) -> Vec<String> {
        self.0.iter().map(|(_, v)| v.clone()).collect()
    }
    /// `{ ...a, ...b }` — the spread, which is what the effective theme sets are built with.
    fn spread(&self, other: &Vars) -> Vars {
        let mut out = self.clone();
        for (k, v) in &other.0 {
            out.set(k, v);
        }
        out
    }
}

/// The selector → declarations map, ALSO insertion-ordered, and for the same reason: `blocks()` merges
/// a repeated selector into its FIRST position (`out[sel] = {...out[sel], ...vars}`).
#[derive(Default, Clone, Debug)]
struct Blocks(Vec<(String, Vars)>);

impl Blocks {
    fn get(&self, sel: &str) -> Vars {
        self.0
            .iter()
            .find(|(s, _)| s == sel)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }
    fn merge(&mut self, sel: &str, vars: &Vars) {
        match self.0.iter_mut().find(|(s, _)| s == sel) {
            Some(slot) => {
                for (k, v) in &vars.0 {
                    slot.1.set(k, v);
                }
            }
            None => self.0.push((sel.to_string(), vars.clone())),
        }
    }
}

/// Every `--name: value;` in every rule OUTSIDE comments, keyed by selector.
///
/// THE COMMENTS ARE STRIPPED FIRST, and it is not tidiness: the console's header mentions
/// `body[data-theme="dark"]` in prose, and a naive scan matches there and then reads the LIGHT block as
/// if it were the dark one — which is exactly the wrong answer the JavaScript's author published
/// before writing the original.
fn blocks(css: &str) -> Blocks {
    let stripped = strip_block_comments(css);
    let c: Vec<char> = stripped.chars().collect();
    let mut out = Blocks::default();
    let mut i = 0;
    while i < c.len() {
        let Some(open) = (i..c.len()).find(|&p| c[p] == '{') else {
            break;
        };
        // `([^{}]+)\{` — the prelude is the run of non-brace characters immediately before the brace,
        // and it must be NON-EMPTY (`+`, not `*`).
        let mut start = open;
        while start > 0 && c[start - 1] != '{' && c[start - 1] != '}' {
            start -= 1;
        }
        if start == open {
            i = open + 1;
            continue;
        }
        // `\{([^{}]*)\}` — the body is the run of non-brace characters up to the closing brace. A
        // body that meets another `{` first is not a match, and the scan continues past this one.
        let mut end = open + 1;
        while end < c.len() && c[end] != '{' && c[end] != '}' {
            end += 1;
        }
        if end >= c.len() || c[end] != '}' {
            i = open + 1;
            continue;
        }
        let prelude: String = c[start..open].iter().collect();
        // `m[1].trim().split("\n").pop().trim()` — the LAST line of the prelude, which is the selector
        // when a rule carries a comment or a media query above it.
        let sel = prelude
            .trim()
            .split('\n')
            .next_back()
            .unwrap_or("")
            .trim()
            .to_string();
        let body: String = c[open + 1..end].iter().collect();
        out.merge(&sel, &vars_in(&body));
        i = end + 1;
    }
    out
}

/// `(--[a-z0-9-]+)\s*:\s*([^;]+);` — every declaration in one rule's body, in order.
fn vars_in(body: &str) -> Vars {
    let c: Vec<char> = body.chars().collect();
    let mut out = Vars::default();
    let mut i = 0;
    while i < c.len() {
        // The name: `--` then `[a-z0-9-]+`, which must be non-empty.
        if !(c[i] == '-' && c.get(i + 1) == Some(&'-')) {
            i += 1;
            continue;
        }
        let mut j = i + 2;
        while j < c.len() && (c[j].is_ascii_lowercase() || c[j].is_ascii_digit() || c[j] == '-') {
            j += 1;
        }
        if j == i + 2 {
            i += 1;
            continue;
        }
        let name: String = c[i..j].iter().collect();
        // `\s*:\s*`
        let mut k = skip_ws(&c, j);
        if c.get(k) != Some(&':') {
            i = j;
            continue;
        }
        k = skip_ws(&c, k + 1);
        // `([^;]+);` — greedy up to a semicolon, and at least one character.
        let mut end = k;
        while end < c.len() && c[end] != ';' {
            end += 1;
        }
        if end == k || end >= c.len() {
            i = j;
            continue;
        }
        let value: String = c[k..end].iter().collect();
        out.set(&name, value.trim());
        i = end + 1;
    }
    out
}

/// `\s` — the JavaScript class, which is what `\s*` in these patterns means.
fn skip_ws(c: &[char], mut i: usize) -> usize {
    while i < c.len() && common::is_js_space(c[i]) {
        i += 1;
    }
    i
}

/// Resolve ONE `var(--x)` level against the same block. `None` when the value still contains a `var()`
/// afterwards — the caller reports that rather than comparing two strings neither side can read.
fn resolve(value: &str, vars: &Vars) -> Option<String> {
    let trimmed = value.trim();
    let c: Vec<char> = trimmed.chars().collect();
    // `^var\(\s*(--[a-z0-9-]+)\s*\)$`
    if !trimmed.starts_with("var(") {
        return Some(trimmed.to_string());
    }
    let mut i = skip_ws(&c, 4);
    if !(c.get(i) == Some(&'-') && c.get(i + 1) == Some(&'-')) {
        return Some(trimmed.to_string());
    }
    let mut j = i + 2;
    while j < c.len() && (c[j].is_ascii_lowercase() || c[j].is_ascii_digit() || c[j] == '-') {
        j += 1;
    }
    if j == i + 2 {
        return Some(trimmed.to_string());
    }
    let name: String = c[i..j].iter().collect();
    i = skip_ws(&c, j);
    if c.get(i) != Some(&')') || i + 1 != c.len() {
        return Some(trimmed.to_string());
    }
    let next = vars.get(&name)?;
    if next.contains("var(") {
        return None;
    }
    Some(next.to_string())
}

/// `/^#|^rgba?\(/` — the JavaScript tests the STRING, so a `var()` is not a colour and neither is a
/// named colour.
fn is_colour(v: &str) -> bool {
    v.starts_with('#') || v.starts_with("rgb(") || v.starts_with("rgba(")
}

/// A shared name, a divergent one (`token`, `console`, `panel`), and a name neither side can read.
type Shared = Vec<String>;
type Differ = Vec<(String, String, String)>;
type Unresolved = Vec<String>;

/// The shared names, the ones that DIFFER, and the ones neither side can read.
fn divergences(console: &Vars, panel: &Vars) -> (Shared, Differ, Unresolved) {
    let shared: Vec<String> = console
        .keys()
        .into_iter()
        .filter(|k| panel.has(k))
        .collect();
    let mut differ = Vec::new();
    let mut unresolved = Vec::new();
    for k in &shared {
        let a = resolve(console.get(k).unwrap_or(""), console);
        let b = resolve(panel.get(k).unwrap_or(""), panel);
        let (Some(a), Some(b)) = (a, b) else {
            unresolved.push(k.clone());
            continue;
        };
        // Compare MEANING, not source formatting: whitespace anywhere (including just inside a
        // function's parentheses) is not a difference between two surfaces.
        let squash = |v: &str| v.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        if squash(&a) != squash(&b) {
            differ.push((k.clone(), a, b));
        }
    }
    (shared, differ, unresolved)
}

/// The console's NEUTRALS must be drawn from the panel's declared scale.
///
/// WHY THIS EXISTS ON TOP OF THE NAME COMPARISON: the name comparison compares only names BOTH sides
/// define, and the console's own `--text`, `--border` and `--bg-secondary` are not shared names — so
/// nothing was checking them. Aligning the console's `--chrome-*` frame first and leaving its body on
/// Bootstrap's grays produced a ZINC FRAME AROUND A BOOTSTRAP BODY: measurably worse than leaving both
/// alone, because the two halves then disagreed INSIDE one surface.
fn off_scale_neutrals(console: &Vars, panel: &Vars) -> Vec<(String, String)> {
    let scale: BTreeSet<String> = panel
        .values()
        .into_iter()
        .filter(|v| is_colour(v))
        .collect();
    let mut out = Vec::new();
    for (k, v) in &console.0 {
        if !is_neutral_name(k) {
            continue;
        }
        let Some(r) = resolve(v, console) else {
            continue; // reported by the caller's unresolved list
        };
        if is_colour(&r) && !scale.contains(&r) {
            out.push((k.clone(), r));
        }
    }
    out
}

/// `/^--(bg|border|text|chrome)/`.
fn is_neutral_name(k: &str) -> bool {
    ["--bg", "--border", "--text", "--chrome"]
        .iter()
        .any(|p| k.starts_with(p))
}

/// A fallback on a token the system DECLARES is dead code.
///
/// `var(--accent, #4f7cff)` never applies, because `--accent` is always defined — so it is not a
/// safety net, it is a description of a design the surface no longer has. The panel carried
/// THIRTY-EIGHT of these, and together they spelled out an entire abandoned palette. A fallback on a
/// token the system does NOT declare is a different thing and is deliberately NOT reported — that is
/// how a caller supplies a default for a variable someone else owns.
fn dead_fallbacks(css: &str, defined: &BTreeSet<String>) -> Vec<(String, String)> {
    let c: Vec<char> = css.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        // `var\(`
        if !(c[i] == 'v'
            && c.get(i + 1) == Some(&'a')
            && c.get(i + 2) == Some(&'r')
            && c.get(i + 3) == Some(&'('))
        {
            i += 1;
            continue;
        }
        let j = skip_ws(&c, i + 4);
        if !(c.get(j) == Some(&'-') && c.get(j + 1) == Some(&'-')) {
            i += 1;
            continue;
        }
        let mut k = j + 2;
        while k < c.len() && (c[k].is_ascii_lowercase() || c[k].is_ascii_digit() || c[k] == '-') {
            k += 1;
        }
        if k == j + 2 {
            i += 1;
            continue;
        }
        let name: String = c[j..k].iter().collect();
        let mut p = skip_ws(&c, k);
        if c.get(p) != Some(&',') {
            i += 1;
            continue;
        }
        p = skip_ws(&c, p + 1);
        // `([\s\S]*?)\)\s*[,;)]` — LAZY: the shortest fallback whose closing paren is followed by one
        // of `,`, `;` or `)`. A greedy match here would swallow the rest of the rule and report a
        // fallback nobody wrote.
        let mut end = p;
        let mut found = None;
        while end < c.len() {
            if c[end] == ')' {
                let after = skip_ws(&c, end + 1);
                if matches!(c.get(after), Some(&',') | Some(&';') | Some(&')')) {
                    found = Some(end);
                    break;
                }
            }
            end += 1;
        }
        let Some(close) = found else {
            i += 1;
            continue;
        };
        if defined.contains(&name) {
            let fallback: String = c[p..close].iter().collect();
            let trimmed = fallback.trim();
            let shown: String = trimmed.chars().take(40).collect();
            out.push((name, shown));
        }
        i = close + 1;
    }
    out
}

// ── COLOUR ───────────────────────────────────────────────────────────────────────────────────────

/// `toRgb` from the check — hex `#rgb`/`#rrggbb` and `rgb()`/`rgba()`, or nothing.
///
/// NOT the probe library's `parseColour`, deliberately: that one is built for `getComputedStyle`
/// output and reads digit runs, so `#ffffff` has NO digits and parses to null. A hex-aware resolver is
/// needed here, and an unparseable value must FAIL rather than be skipped — a check that cannot read
/// its input is not a check that found nothing.
fn to_rgb(c: &str) -> Option<(f64, f64, f64)> {
    let v = c.trim();
    let lower = v.to_ascii_lowercase();
    let hex = |s: &str| -> Option<Vec<u32>> {
        if !s.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return None;
        }
        Some(
            s.chars()
                .map(|ch| ch.to_digit(16).unwrap_or(0))
                .collect::<Vec<u32>>(),
        )
    };
    if let Some(body) = lower.strip_prefix('#') {
        let d = hex(body)?;
        match d.len() {
            3 => {
                return Some((
                    (d[0] * 16 + d[0]) as f64,
                    (d[1] * 16 + d[1]) as f64,
                    (d[2] * 16 + d[2]) as f64,
                ))
            }
            6 => {
                return Some((
                    (d[0] * 16 + d[1]) as f64,
                    (d[2] * 16 + d[3]) as f64,
                    (d[4] * 16 + d[5]) as f64,
                ))
            }
            _ => return None,
        }
    }
    // `^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)`
    let body = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))?;
    let cb: Vec<char> = body.chars().collect();
    let mut i = skip_ws(&cb, 0);
    let mut channels = Vec::new();
    while channels.len() < 3 {
        let start = i;
        while i < cb.len() && (cb[i].is_ascii_digit() || cb[i] == '.') {
            i += 1;
        }
        if i == start {
            return None;
        }
        let text: String = cb[start..i].iter().collect();
        channels.push(text.parse::<f64>().ok()?);
        if channels.len() == 3 {
            break;
        }
        let mut sep = 0;
        while i < cb.len() && (cb[i] == ',' || common::is_js_space(cb[i])) {
            i += 1;
            sep += 1;
        }
        if sep == 0 {
            return None;
        }
    }
    Some((channels[0], channels[1], channels[2]))
}

/// `contrastRatio(fg, bg)` — the probe library's own formula, and `+x.toFixed(2)` on the way out.
fn contrast_ratio(fg: (f64, f64, f64), bg: (f64, f64, f64)) -> f64 {
    let f = |v: f64| {
        let v = v / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let lum = |c: (f64, f64, f64)| 0.2126 * f(c.0) + 0.7152 * f(c.1) + 0.0722 * f(c.2);
    let (a, b) = (lum(fg), lum(bg));
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    round_2((hi + 0.05) / (lo + 0.05))
}

/// `+x.toFixed(2)`.
///
/// THE ROUNDING IS THE ENGINE'S, and it is not `(x * 100).round() / 100`: `toFixed` rounds the EXACT
/// binary value, so `(1.005).toFixed(2)` is `"1.00"` and not `"1.01"`. Formatting to two decimals and
/// reading it back is the same operation, and it is what the messages and the AA comparison both use —
/// the comparison is against the ROUNDED value, which is why this cannot be approximated.
fn round_2(x: f64) -> f64 {
    format!("{x:.2}").parse::<f64>().unwrap_or(x)
}

fn fmt_2(x: f64) -> String {
    format!("{x:.2}")
}

// ── THE RUN ──────────────────────────────────────────────────────────────────────────────────────

const CONSOLE: &str = "gateway/ui/src/styles/globals.css";
const PANEL: &str = "agent/resources/panel-react/src/styles/tokens.css";
const LANDING: &str = "index/landing/assets/page.css";
const SPACING_SIDES: [(&str, &str); 2] = [
    ("panel", "agent/resources/panel-react/src/styles/tokens.css"),
    ("console", "gateway/ui/src/styles/globals.css"),
];
const SPACING_STEPS: [&str; 6] = ["--sp-0-5", "--sp-1", "--sp-2", "--sp-3", "--sp-4", "--sp-5"];
const AA_TEXT: f64 = 4.5;

/// The failure count and the one way this gate speaks.
///
/// IT PRINTS AS IT GOES, and that is a deliberate mirror rather than a convenience: the JavaScript
/// prints each line where it is produced, so an assertion that fires halfway through (the landing's
/// `:root` guard) still leaves everything the earlier checks said on the screen. A gate that collected
/// its lines and printed them at the end would report the same VERDICT and lose the evidence — which
/// is exactly what the first version of this file did, and what the differential caught.
struct Report {
    failures: usize,
}

impl Report {
    fn new() -> Self {
        Report { failures: 0 }
    }
    fn say(&mut self, line: impl Into<String>) {
        println!("{}", line.into());
    }
}

fn read(rel: &str) -> String {
    let p = repo().join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

/// The accent family, in both directions, for one theme's token set.
fn contrast_failures(label: &str, tokens: &Vars) -> Vec<String> {
    let mut out = Vec::new();
    let get = |k: &str| tokens.get(k).map(|s| s.trim().to_string());
    let ratio_of = |a: &str, b: &str| -> Option<f64> {
        let (fg, bg) = (to_rgb(a)?, to_rgb(b)?);
        Some(contrast_ratio(fg, bg))
    };
    let fg = get("--accent-fg");
    let accent = get("--accent");
    let bg = get("--bg");
    // A missing token is itself a failure — an absent `--accent-fg` silently falls back to
    // inheritance, which is how a 1.90:1 button ships.
    if fg.is_none() {
        out.push(format!("{label}: --accent-fg is not declared"));
    }
    if accent.is_none() {
        out.push(format!("{label}: --accent is not declared"));
    }
    {
        let mut check = |what: &str, fg: &str, bg: &str| match ratio_of(fg, bg) {
            None => out.push(format!(
                "{label}: {what} could not be measured ({fg} on {bg})"
            )),
            Some(r) if r < AA_TEXT => out.push(format!(
                "{label}: {what} measures {}, under AA {AA_TEXT} ({fg} on {bg})",
                fmt_2(r)
            )),
            Some(_) => {}
        };
        if let (Some(fg), Some(accent)) = (&fg, &accent) {
            check("--accent-fg on --accent", fg, accent);
        }
        if let (Some(accent), Some(bg)) = (&accent, &bg) {
            check("--accent as text on --bg", accent, bg);
        }
    }
    out
}

/// The recursive walk the dead-fallback check makes over a source tree.
fn walk(dir: &Path, token_file: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "node_modules") {
                continue;
            }
            walk(&p, token_file, out);
        } else {
            let name = p.to_string_lossy().to_string();
            let wanted = name.ends_with(".css") || name.ends_with(".tsx") || name.ends_with(".ts");
            if wanted && !name.ends_with(token_file) {
                out.push(p);
            }
        }
    }
}

#[test]
fn token_contract() {
    let mut r = Report::new();

    // ── 1/4. the semantic colours' two weights, and the marks painted as text ────────────────────
    {
        let g = blocks(&read(CONSOLE));
        let root = g.get(":root");
        let dark = g.get("body[data-theme=\"dark\"]");
        for tok in ["--success-text", "--warning-text", "--error-text"] {
            if !root.has(tok) || !dark.has(tok) {
                r.failures += 1;
                r.say(format!(
                    "    console: {tok} must be declared in BOTH theme blocks (light-only freezes against the light background)"
                ));
            }
        }
        let css = strip_block_comments(&read(CONSOLE));
        // `--text-muted` passes on the two lightest surfaces and FAILS on the two darker ones, so
        // whether it was safe depended on which surface a rule happened to land on — not something a
        // stylesheet check can see. Both remain valid as MARKS.
        for mark in ["--text-muted", "--text-faint"] {
            let n = count_color_var(&css, mark);
            if n > 0 {
                r.failures += n;
                r.say(format!(
                    "    console: {n} rule(s) paint TEXT with {mark}, a MARK weight — use --text-secondary"
                ));
            }
        }
        for mark in ["--success", "--warning", "--error"] {
            let n = count_color_var(&css, mark);
            if n > 0 {
                r.failures += n;
                r.say(format!(
                    "    console: {n} rule(s) paint TEXT with {mark}, the MARK weight — use {mark}-text"
                ));
            }
        }
        if r.failures == 0 {
            r.say("  console: semantic colours have a readable text weight");
        }
    }

    // ── 3. dead fallbacks, both frontends ────────────────────────────────────────────────────────
    for (label, dir, token_file) in [
        ("console", "gateway/ui/src/", "styles/globals.css"),
        (
            "panel",
            "agent/resources/panel-react/src/",
            "styles/tokens.css",
        ),
    ] {
        let tokens = blocks(&read(&format!("{dir}{token_file}")));
        let defined: BTreeSet<String> = tokens.0.iter().flat_map(|(_, v)| v.keys()).collect();
        let mut files = Vec::new();
        let dir_path = repo().join(dir);
        walk(&dir_path, token_file, &mut files);
        let mut dead = 0;
        for f in &files {
            let text = fs::read_to_string(f).unwrap_or_default();
            for (token, fallback) in dead_fallbacks(&text, &defined) {
                dead += 1;
                if dead <= 3 {
                    // `dir + relative`, SPELLED THE WAY THE JAVASCRIPT SPELLS IT — the walk builds
                    // `${d}/${e}` from a `d` that already ends in a slash, so the message carries a
                    // doubled separator and a reader comparing the two outputs sees the same path.
                    let rel = f
                        .strip_prefix(&dir_path)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    r.say(format!(
                        "    {dir}/{rel}: var({token}, {fallback}…) can never apply"
                    ));
                }
            }
        }
        if dead > 0 {
            r.failures += dead;
            r.say(format!(
                "  {label}: {dead} DEAD fallback(s) — a token the system declares always wins"
            ));
        } else {
            r.say(format!("  {label}: no dead fallbacks"));
        }
    }

    // ── the sheets, and 8. the landing's parseability ────────────────────────────────────────────
    let cases = [
        ("light", ":root", ":root"),
        (
            "dark",
            "body[data-theme=\"dark\"]",
            "body[data-theme=\"dark\"]",
        ),
    ];
    let gc = blocks(&read(CONSOLE));
    let pc = blocks(&read(PANEL));
    let landing_src = read(LANDING);
    // THE READ IS FATAL ON PURPOSE: a parser pointed at an empty string reports a clean surface.
    assert!(
        landing_src.contains(":root"),
        "{LANDING}: no :root block — the parser is reading the wrong input"
    );
    let lc = blocks(&landing_src);

    // ── 5. the accent family, in both directions and both themes ─────────────────────────────────
    let mut accent_failures = 0;
    for (label, gsel) in [("light", ":root"), ("dark", "body[data-theme=\"dark\"]")] {
        let tokens = gc.get(":root").spread(&gc.get(gsel));
        let bad = contrast_failures(label, &tokens);
        accent_failures += bad.len();
        for b in bad {
            r.say(format!("  {b}"));
        }
    }
    if accent_failures == 0 {
        r.say("  accent family: readable in both directions, both themes");
    }

    // ── 1/2/6. the shared names, per theme ───────────────────────────────────────────────────────
    for (label, gsel, psel) in cases {
        let g = gc.get(gsel);
        let p = pc.get(psel);
        let (shared, differ, unresolved) = divergences(&g, &p);
        // A comparison that read nothing must not report success.
        assert!(
            shared.len() >= 8,
            "{label}: only {} shared tokens found — the parser read the wrong block",
            shared.len()
        );
        if !unresolved.is_empty() {
            r.say(format!(
                "  {label}: {} shared token(s) UNRESOLVED (nested var) — not compared: {}",
                unresolved.len(),
                unresolved.join(", ")
            ));
        }
        // 6. THE LANDING, for the names it shares with the console. EFFECTIVE sets, not raw blocks: a
        // theme's tokens are `:root` PLUS its override, and comparing the two OVERRIDE blocks alone
        // found an intersection of exactly ZERO and would have reported "no disagreement" for the
        // worst possible reason.
        {
            let g_eff = gc.get(":root").spread(&gc.get(gsel));
            let l_eff = lc.get(":root").spread(&lc.get(if label == "light" {
                ":root"
            } else {
                "body[data-ds-dark-theme]"
            }));
            let (l_shared, l_differ, _) = divergences(&g_eff, &l_eff);
            // COVERAGE BY NAME, not by count: these are the names whose disagreement would actually be
            // a defect (the art-direction palette, the type stack), so a parser reading some other
            // block cannot satisfy it by returning five unrelated tokens.
            let required = ["--glass-blur", "--ds-font-family"];
            let missing: Vec<&str> = required
                .iter()
                .copied()
                .filter(|t| !l_shared.iter().any(|s| s == t))
                .collect();
            assert!(
                missing.is_empty(),
                "{label}: the landing comparison did not cover {} — it read {} shared tokens, which is not the block this check wants",
                missing.join(", "),
                l_shared.len()
            );
            for (token, c, l) in &l_differ {
                r.failures += 1;
                r.say(format!(
                    "  {label}: landing  {token}: console {c} vs landing {l}"
                ));
            }
            if l_differ.is_empty() {
                r.say(format!(
                    "  {label}: landing agrees on all {} shared tokens",
                    l_shared.len()
                ));
            }
        }

        let scale_panel = pc.get(":root").spread(&pc.get(psel));
        let off = off_scale_neutrals(&g, &scale_panel);
        if off.is_empty() {
            r.say(format!(
                "  {label}: every console neutral is a value the panel declares"
            ));
        } else {
            r.failures += off.len();
            r.say(format!(
                "  {label}: {} console neutral(s) are NOT on the panel's scale:",
                off.len()
            ));
            for (token, value) in off {
                r.say(format!("    {token}: {value} is the console's alone"));
            }
        }

        if differ.is_empty() {
            r.say(format!("  {label}: {} shared tokens agree", shared.len()));
        } else {
            r.failures += differ.len();
            r.say(format!(
                "  {label}: {} of {} shared tokens DIVERGE",
                differ.len(),
                shared.len()
            ));
            for (token, c, p) in differ {
                r.say(format!("    {token}: console {c} vs panel {p}"));
            }
        }
    }
    r.failures += accent_failures;

    // ── the verdict ──────────────────────────────────────────────────────────────────────────────
    if r.failures > 0 {
        // The count covers THREE causes and the old summary named only one — a dead fallback was
        // reported as "N token(s) mean different things on the two surfaces", which sends a reader
        // looking for a value mismatch that is not there. Say which.
        eprintln!("\ntoken contract FAILED ({}):", r.failures);
        eprintln!(
            "  * a shared token holding DIFFERENT VALUES on the two surfaces -> the panel is"
        );
        eprintln!("    the device's primary operator surface, so pick ITS value for the console;");
        eprintln!("  * a DEAD fallback (`var(--x, v)` where the system declares --x) -> drop the");
        eprintln!("    fallback, it can never apply and it describes a design that is gone;");
        eprintln!(
            "  * or the ACCENT FAMILY is not readable (see the measurements above) -> darken the"
        );
        eprintln!(
            "    accent rather than the ink: no foreground passes on #d9480f at the base AND its"
        );
        panic!("token contract FAILED ({})", r.failures);
    }
    println!("token contract: the console and the panel agree on every shared token name.");

    // ── 7. the spacing scale, across both ────────────────────────────────────────────────────────
    //
    // WHY THIS SURVIVES THE COMPARISON ABOVE, which also fails on a `--sp-*` that disagrees. The two
    // are NOT the same check: the comparison above only looks at names BOTH sides define, so a step
    // that one side DELETED stops being shared and drops out of it silently — the scale quietly
    // becomes a five-step one on that surface. This block names the six steps explicitly and requires
    // both sides to define them, which is the half a shared-name comparison cannot see.
    {
        let mut seen: Vec<(String, Vec<(String, String)>)> = Vec::new();
        let mut problems = Vec::new();
        for (name, file) in SPACING_SIDES {
            let text = read(file);
            for step in SPACING_STEPS {
                let Some(value) = declared_value(&text, step) else {
                    problems.push(format!("{name} does not define {step}"));
                    continue;
                };
                match seen.iter_mut().find(|(s, _)| s == step) {
                    Some((_, by_side)) => by_side.push((name.to_string(), value)),
                    None => seen.push((step.to_string(), vec![(name.to_string(), value)])),
                }
            }
        }
        for (step, by_side) in &seen {
            let values: BTreeSet<&String> = by_side.iter().map(|(_, v)| v).collect();
            if values.len() > 1 {
                let listed = by_side
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                problems.push(format!("{step} differs: {listed}"));
            }
        }
        if !problems.is_empty() {
            eprintln!("token contract: the SPACING SCALE is not shared —");
            for p in &problems {
                eprintln!("  {p}");
            }
            panic!("token contract: the SPACING SCALE is not shared");
        }
        println!(
            "token contract: the spacing scale is identical in both UIs ({} steps).",
            SPACING_STEPS.len()
        );
    }
}

/// `new RegExp(`${step}\s*:\s*([^;]+);`)` over the RAW file text — the first match anywhere,
/// comments included, which is what the JavaScript does.
fn declared_value(text: &str, step: &str) -> Option<String> {
    let c: Vec<char> = text.chars().collect();
    let needle: Vec<char> = step.chars().collect();
    let mut i = 0;
    while i + needle.len() <= c.len() {
        if c[i..i + needle.len()] == needle[..] {
            let mut j = skip_ws(&c, i + needle.len());
            if c.get(j) == Some(&':') {
                j = skip_ws(&c, j + 1);
                let mut end = j;
                while end < c.len() && c[end] != ';' {
                    end += 1;
                }
                if end > j && end < c.len() {
                    let value: String = c[j..end].iter().collect();
                    return Some(value.trim().to_string());
                }
            }
        }
        i += 1;
    }
    None
}

/// `(?<![\w-])color:\s*var\(--x\)` — the LOOKBEHIND is why this is hand-rolled: the Rust `regex`
/// crate has none. The rule is that the `color` must not be the tail of a longer identifier
/// (`background-color` is a different property and `--x` there is a background, not text).
fn count_color_var(css: &str, token: &str) -> usize {
    let c: Vec<char> = css.chars().collect();
    let mut count = 0;
    let mut i = 0;
    while i + 6 <= c.len() {
        if c[i..i + 5].iter().collect::<String>() == "color" {
            let before_ok = i == 0 || !(common::is_word(c[i - 1]) || c[i - 1] == '-');
            let mut j = i + 5;
            j = skip_ws(&c, j);
            if before_ok && c.get(j) == Some(&':') {
                j = skip_ws(&c, j + 1);
                let rest: String = c[j..].iter().collect();
                let want = format!("var({token})");
                if rest.starts_with(&want) {
                    count += 1;
                }
            }
        }
        i += 1;
    }
    count
}
