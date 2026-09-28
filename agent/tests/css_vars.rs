//! EVERY `var(--token)` MUST RESOLVE. An undefined custom property does not fall back to
//! something sensible — it makes the WHOLE DECLARATION invalid at computed-value time,
//! silently. That is how the console's keyboard focus ring came to be invisible on every
//! control (round 137): a sheet styled `outline: 2px solid var(--focus-ring)` and
//! `box-shadow: 0 0 0 3px var(--focus-ring-soft)`, neither token was defined in that sheet, and
//! BOTH declarations were dropped. The CSS read as correct. The page looked fine. Only a
//! rendered measurement of the focus ring found it, and only because rounds 133-136 had just
//! repaired the instrument that could see it.
//!
//! A missing definition is not a thing to find by rendering. It is a string that has no
//! counterpart, which is exactly what a check is good at — so this gate makes the class
//! impossible to reintroduce.
//!
//! SCOPE: each UI is scanned as a WHOLE, because token definitions and their uses live in
//! different files (the panel's tokens.css defines what components.css and desktop.css consume).
//! Per-file scanning would report thousands of false positives.
//!
//! NOT A FALSE POSITIVE: a `var(--x, fallback)` needs no definition — the fallback is the
//! answer. Those are skipped deliberately rather than counted, and the count of skipped ones is
//! printed so a stylesheet that suddenly becomes all-fallbacks is visible.
//!
//! AND THE OTHER DIRECTION: A TOKEN NOTHING READS (round 20 of the standing goal). The check
//! above finds a reference with no definition. Nothing found a DEFINITION with no reference, and
//! there were twenty-one of them across the panel and the console — dead weight in the one block
//! a reader goes to in order to learn what the palette IS. A token may be read by a RULE, by
//! CODE, or by a name ASSEMBLED at runtime, and anything else is a name to delete.
//!
//! AND THE CODE THAT READS TOKENS AT RUNTIME (round 235). This covered stylesheets only, so
//! nothing noticed that BOTH particles.ts files still ask for the RETIRED aura palette
//! (`pick("--aura-1", 190)`, …). The aura tokens went in the rebrand; the READERS stayed, and
//! `pick` falls back when a token reads empty, so the particles quietly drew the three
//! PRE-REBRAND hues instead of the brand's.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/css-vars-check.mjs` → "all checks passed", panel 114 defined · 107
//!     referenced without fallback · 0 with a fallback; console 73 · 70 · 0; "0
//!     declared-but-unread across both UIs"; "runtime: 132 sources · 9 token reads (--accent
//!     --brand-grad-a --brand-grad-b --danger-on-soft --muted --state-fail-ink) · 0 waived · 0
//!     undefined".
//!   * this file → the SAME five numbers and the SAME token list, character for character.
//!   * a planted `--token-nobody-reads: 1px` in the console's globals.css → the JS gate exits 1
//!     ("console: every declared token is READ by something — 1 declared and never read") and
//!     this file fails with the identical two lines.
//!   * TWO BOUNDARY INPUTS WHERE BOTH STAY GREEN, which is why they were measured: renaming a
//!     definition to `--warning-text-RETIRED-PROBE`, and deleting ONE of two `--warning-text`
//!     declarations, both leave this gate green in BOTH implementations. The first survives
//!     because `R` is not a `[a-z0-9-]` character, so the token still counts as read; the second
//!     because the other declaration still defines it. A happy-path-only proof would have called
//!     either of those a divergence.
//!
//! MUTATION: delete a `--token` definition its own sheet references, OR a token the CODE reads
//!           at runtime, OR declare a token nothing reads (`--token-nobody-reads`).
//! RESULT:   fails either way. The runtime half was added in round 235 after both `particles.ts`
//!           files were found reading the retired `--aura-*` palette; its floors caught the
//!           first version of that scan reading ZERO references in 117 files. The other
//!           direction found twenty-one dead tokens across the panel and the console, and
//!           reports a planted `--token-nobody-reads`. Comments are stripped FIRST, because the
//!           first run in that direction reported two tokens that exist only as PROSE.

use std::collections::BTreeSet;
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

const SKIP_DIRS: [&str; 3] = ["node_modules", "dist", "build"];

fn is_name_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
}

/// Read a `--name` at `i` (the first `-`), returning it and the index past it.
fn name_at(s: &[char], i: usize) -> Option<(String, usize)> {
    if s.get(i) != Some(&'-') || s.get(i + 1) != Some(&'-') {
        return None;
    }
    let mut j = i + 2;
    while j < s.len() && is_name_char(s[j]) {
        j += 1;
    }
    if j == i + 2 {
        return None;
    }
    Some((s[i..j].iter().collect(), j))
}

/// Every file under `dir` whose name passes `keep`, without descending into build output.
fn walk(dir: &Path, keep: &dyn Fn(&str) -> bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let full = e.path();
        if full.is_dir() {
            walk(&full, keep, out);
        } else if keep(&name) {
            out.push(full);
        }
    }
}

fn read_all(files: &[PathBuf]) -> String {
    files
        .iter()
        .map(|f| {
            fs::read_to_string(f).unwrap_or_else(|e| panic!("cannot read {}: {e}", f.display()))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `/* … */` removed. Comments are not declarations: a comment that MENTIONS a token counted as
/// a declaration of it, and the unread-token check then reported two tokens that exist only as
/// prose in an explanation.
fn strip_block_comments(text: &str) -> String {
    let s: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == '/' && s.get(i + 1) == Some(&'*') {
            let mut j = i + 2;
            while j + 1 < s.len() && !(s[j] == '*' && s[j + 1] == '/') {
                j += 1;
            }
            i = if j + 1 < s.len() { j + 2 } else { s.len() };
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

/// `(--[a-z0-9-]+)\s*:` — a declaration.
fn definitions(text: &str) -> BTreeSet<String> {
    let s: Vec<char> = text.chars().collect();
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < s.len() {
        if let Some((name, j)) = name_at(&s, i) {
            let mut k = j;
            while k < s.len() && s[k].is_whitespace() {
                k += 1;
            }
            if s.get(k) == Some(&':') {
                out.insert(name);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// `var\(\s*(--[a-z0-9-]+)\s*([,)])` — a reference. Returns (bare references, fallback count).
fn references(text: &str) -> (BTreeSet<String>, usize) {
    let s: Vec<char> = text.chars().collect();
    let needle: Vec<char> = "var(".chars().collect();
    let mut bare = BTreeSet::new();
    let mut with_fallback = 0;
    let mut i = 0;
    while i + needle.len() <= s.len() {
        if s[i..i + needle.len()] != needle[..] {
            i += 1;
            continue;
        }
        let mut j = i + needle.len();
        while j < s.len() && s[j].is_whitespace() {
            j += 1;
        }
        let Some((name, k)) = name_at(&s, j) else {
            i += 1;
            continue;
        };
        let mut m = k;
        while m < s.len() && s[m].is_whitespace() {
            m += 1;
        }
        match s.get(m) {
            Some(',') => with_fallback += 1,
            Some(')') => {
                bare.insert(name);
            }
            _ => {}
        }
        i = m.max(i + 1);
    }
    (bare, with_fallback)
}

/// Occurrences of `token` not followed by another name character.
fn occurrences(text: &str, token: &str) -> usize {
    let s: Vec<char> = text.chars().collect();
    let t: Vec<char> = token.chars().collect();
    let mut n = 0;
    let mut i = 0;
    while i + t.len() <= s.len() {
        if s[i..i + t.len()] == t[..] {
            if !s.get(i + t.len()).is_some_and(|c| is_name_char(*c)) {
                n += 1;
            }
            i += t.len();
        } else {
            i += 1;
        }
    }
    n
}

/// Occurrences of `token` followed by optional whitespace and a colon — a WRITE, not a read.
fn writes(text: &str, token: &str) -> usize {
    let s: Vec<char> = text.chars().collect();
    let t: Vec<char> = token.chars().collect();
    let mut n = 0;
    let mut i = 0;
    while i + t.len() <= s.len() {
        if s[i..i + t.len()] == t[..] {
            let mut k = i + t.len();
            while k < s.len() && s[k].is_whitespace() {
                k += 1;
            }
            if s.get(k) == Some(&':') {
                n += 1;
            }
            i += t.len();
        } else {
            i += 1;
        }
    }
    n
}

/// THE TWO ENTRIES A NAIVE SCAN GETS WRONG, named with their reasons rather than filtered by a
/// pattern.
const UNREAD_OK: [(&str, &str); 2] = [
    ("--ds-neutral-", "a FAMILY, read assembled: themeContrast.test.ts builds `--ds-neutral-${step}` from a step number — the same false positive the dead-CSS tool documents for class names"),
    ("--glass-blur", "declared on BOTH UIs because the token contract REQUIRES it present for the landing comparison (the art-direction palette must be covered); the landing is the surface that reads it"),
];

/// NOT TOKENS, and each says why. These are the three a naive scan reported and a reader can
/// check in seconds.
const NOT_A_TOKEN: [(&str, &str); 2] = [
    (
        "--json",
        "a command-line flag in prose (the agent's CLI), not a custom property",
    ),
    (
        "--ds-neutral",
        "a PREFIX: the text is `--ds-neutral-*`, naming a family rather than one property",
    ),
];

/// WAIVED, WITH REASONS, the same way the sweeps waive what they cannot judge. EMPTY, and the
/// story of what left it is the point: `--aura-1/3/4` sat here because the particle field read a
/// palette the rebrand had retired and the replacement was a DESIGN decision. Round 14 carried
/// that decision out, so the waiver is gone and a regression fails here like any other
/// undefined token.
const WAIVED_RUNTIME: [&str; 0] = [];

/// The tokens that MEAN a state — not used here; the CSS sheets and the code that reads them.
struct Ui {
    name: &'static str,
    sheet_dirs: &'static [&'static str],
    code_dirs: &'static [&'static str],
}

const UIS: [Ui; 2] = [
    Ui {
        name: "panel",
        sheet_dirs: &["agent/resources/panel-react/src/styles"],
        code_dirs: &["agent/resources/panel-react/src"],
    },
    Ui {
        name: "console",
        sheet_dirs: &["gateway/ui/src/styles"],
        code_dirs: &["gateway/ui/src"],
    },
];

fn is_css(name: &str) -> bool {
    name.ends_with(".css")
}

fn is_code(name: &str) -> bool {
    (name.ends_with(".ts")
        || name.ends_with(".tsx")
        || name.ends_with(".js")
        || name.ends_with(".jsx")
        || name.ends_with(".mjs"))
        && !name.contains(".test.")
}

fn css_files(root: &Path, dirs: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in dirs {
        walk(&root.join(d), &is_css, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

/// The whole gate, returning the summary lines or the failure text. Split out so the planted
/// fixtures below can drive it without the real tree.
fn run(root: &Path) -> Result<Vec<String>, String> {
    let mut failures = Vec::new();
    let mut lines = Vec::new();
    let mut total_defs = 0usize;
    let mut total_refs = 0usize;
    let mut total_unread = 0usize;

    for ui in &UIS {
        let files = css_files(root, ui.sheet_dirs);
        if files.is_empty() {
            failures.push(format!("{}: no .css under {:?}", ui.name, ui.sheet_dirs));
            continue;
        }
        let text = strip_block_comments(&read_all(&files));
        let defined = definitions(&text);
        let (bare, with_fallback) = references(&text);
        total_defs += defined.len();
        total_refs += bare.len();
        let missing: Vec<&String> = bare.iter().filter(|v| !defined.contains(*v)).collect();
        if !missing.is_empty() {
            failures.push(format!(
                "{}: every var() reference resolves — {} undefined — each one silently drops its whole declaration:\n    {}",
                ui.name,
                missing.len(),
                missing
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("\n    ")
            ));
        }
        lines.push(format!(
            "    {} {} defined · {} referenced without fallback · {} with a fallback",
            ui.name,
            defined.len(),
            bare.len(),
            with_fallback
        ));

        let mut code_files = Vec::new();
        for d in ui.code_dirs {
            walk(&root.join(d), &is_code, &mut code_files);
        }
        let code = read_all(&code_files);
        let mut unread = Vec::new();
        for token in &defined {
            let sheet_reads = occurrences(&text, token).saturating_sub(writes(&text, token));
            let read = sheet_reads > 0 || occurrences(&code, token) > 0;
            if !read && !UNREAD_OK.iter().any(|(k, _)| token.starts_with(k)) {
                unread.push(token.clone());
            }
        }
        total_unread += unread.len();
        if !unread.is_empty() {
            failures.push(format!(
                "{}: every declared token is READ by something — {} declared and never read — a name a reader has \
                 to check for nothing:\n    {}\n  Delete it, or add it to UNREAD_OK with the reason it stays.",
                ui.name,
                unread.len(),
                unread.join("\n    ")
            ));
        }
    }

    // ── the SAME rule for the code that READS tokens at runtime (round 235) ────────────────
    let mut all_sheets = css_files(root, UIS[0].sheet_dirs);
    all_sheets.extend(css_files(root, UIS[1].sheet_dirs));
    let mut all_defined = BTreeSet::new();
    for f in &all_sheets {
        let raw =
            fs::read_to_string(f).unwrap_or_else(|e| panic!("cannot read {}: {e}", f.display()));
        all_defined.extend(definitions(&raw));
    }
    let mut runtime_sources = Vec::new();
    for ui in &UIS {
        for d in ui.code_dirs {
            walk(&root.join(d), &is_code, &mut runtime_sources);
        }
    }
    // THE LITERALS, NOT THE CALLS. The first version looked for a token name inside
    // `getPropertyValue("--x")` and found ZERO references in 117 files — because particles.ts
    // passes the name to a helper (`pick("--aura-1", 190)`) and the call site never sees it.
    // The floor below caught that immediately, which is what the floor is for.
    let mut dead = Vec::new();
    let mut found = BTreeSet::new();
    let mut seen = 0usize;
    for file in &runtime_sources {
        let text = fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        for token in quoted_tokens(&text) {
            seen += 1;
            found.insert(token.clone());
            if NOT_A_TOKEN.iter().any(|(t, _)| *t == token) {
                continue;
            }
            if !all_defined.contains(&token) && !WAIVED_RUNTIME.contains(&token.as_str()) {
                let rel = file
                    .strip_prefix(root)
                    .unwrap_or(file)
                    .to_string_lossy()
                    .to_string();
                dead.push(format!("{rel}: {token}"));
            }
        }
    }
    if runtime_sources.len() < 10 {
        failures.push(format!(
            "the runtime scan read real files — only {} sources",
            runtime_sources.len()
        ));
    }
    if seen < 6 {
        failures.push(format!(
            "the runtime scan found real references — only {seen} runtime references"
        ));
    }
    if !dead.is_empty() {
        failures.push(format!(
            "every token the CODE reads at runtime is defined somewhere — {} undefined — a reader of a removed \
             token falls back silently rather than failing:\n    {}",
            dead.len(),
            dead.join("\n    ")
        ));
    }
    lines.push(format!(
        "    {total_unread} declared-but-unread across both UIs"
    ));
    lines.push(format!(
        "    runtime: {} sources · {seen} token reads ({}) · {} waived · {} undefined",
        runtime_sources.len(),
        found.iter().cloned().collect::<Vec<_>>().join(" "),
        WAIVED_RUNTIME.len(),
        dead.len()
    ));

    // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN.
    if total_defs < 100 {
        failures.push(format!(
            "the scan read real stylesheets — only {total_defs} definitions found"
        ));
    }
    if total_refs < 80 {
        failures.push(format!(
            "the scan found real references — only {total_refs} references found"
        ));
    }

    if failures.is_empty() {
        Ok(lines)
    } else {
        Err(failures.join("\n"))
    }
}

/// `["'`](--[a-z0-9-]+)["'`]` — a string literal that LOOKS like a token name.
fn quoted_tokens(text: &str) -> Vec<String> {
    let s: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if matches!(s[i], '"' | '\'' | '`') {
            let quote = s[i];
            if let Some((name, j)) = name_at(&s, i + 1) {
                if s.get(j) == Some(&quote) {
                    out.push(name);
                    i = j + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

#[test]
fn every_var_reference_resolves_and_every_token_is_read() {
    match run(&repo()) {
        Ok(lines) => {
            for l in lines {
                println!("{l}");
            }
            println!("css-vars-check: all checks passed");
        }
        Err(e) => panic!("css-vars-check: FAILED\n{e}"),
    }
}

// ── the scanner's own proof, on planted input ───────────────────────────────────────────────
//
// The gate above reads whatever the tree holds today, so it cannot prove the scanner
// distinguishes a definition from a reference or a write from a read. These do.

#[test]
fn the_scanner_separates_a_definition_a_reference_and_a_fallback() {
    let text = ":root { --ok: #0f0; --warn:#fa0; }\n.a { color: var(--ok); background: var(--gone, red); }";
    let dec = definitions(text);
    assert!(dec.contains("--ok") && dec.contains("--warn"));
    let (bare, fb) = references(text);
    assert!(bare.contains("--ok"));
    assert!(
        !bare.contains("--gone"),
        "a fallback is not a bare reference"
    );
    assert_eq!(fb, 1);
    // `--ok` is written once and read once, so the READ count is one, not two.
    assert_eq!(occurrences(text, "--ok"), 2);
    assert_eq!(writes(text, "--ok"), 1);
    assert_eq!(occurrences(text, "--ok") - writes(text, "--ok"), 1);
}

#[test]
fn a_token_that_is_a_prefix_of_another_is_not_the_same_token() {
    // `--ds-neutral` must not be satisfied by `--ds-neutral-500`.
    let text = ":root { --ds-neutral-500: #333; }\n.a { color: var(--ds-neutral); }";
    let dec = definitions(text);
    assert!(dec.contains("--ds-neutral-500"));
    assert!(
        !dec.contains("--ds-neutral"),
        "the family is not the member"
    );
    let (bare, _) = references(text);
    assert!(bare.contains("--ds-neutral"));
    assert_eq!(
        occurrences(text, "--ds-neutral"),
        1,
        "the longer name does not count"
    );
}

#[test]
fn only_a_string_literal_that_looks_like_a_token_is_a_runtime_read() {
    let found =
        quoted_tokens("pick(\"--aura-1\", 190); getPropertyValue('--ok'); let x = `--also`;");
    assert_eq!(found, vec!["--aura-1", "--ok", "--also"]);
    // A bare `--json` in prose is a literal too — which is why NOT_A_TOKEN exists rather than a
    // narrower pattern.
    assert_eq!(quoted_tokens("run `--json`"), vec!["--json"]);
    // Not a literal: an unquoted token.
    assert!(quoted_tokens("var(--ok)").is_empty());
}

#[test]
fn a_parser_that_reads_nothing_is_not_a_pass() {
    let empty = std::env::temp_dir().join("summrise-css-vars-empty");
    let _ = fs::remove_dir_all(&empty);
    fs::create_dir_all(&empty).expect("temp dir");
    let err = run(&empty).expect_err("silence must not pass");
    assert!(err.contains("no .css"), "{err}");
    let _ = fs::remove_dir_all(&empty);
}
