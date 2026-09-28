//! ONE FACT, ONE DERIVATION: the panel may not map an ENDING to a state twice.
//!
//! WHY THIS EXISTS (round 78 of the standing goal). The spine of this objective is "one source of
//! truth per fact, with no surface computing its own version of the same fact" — and until this
//! gate, that rule was enforced by READING. Round 29 paid for it: `TrajectoryView` kept a private
//! derivation that mapped a BACKGROUNDED command to `warn` while the canonical one mapped it to
//! `bg`, so one backgrounded command wore two different states in the same view, and both looked
//! deliberate. The duplicate was deleted; nothing stopped the next one.
//!
//! WHAT IT CHECKS, and it is deliberately NARROW: the endings the device can report are listed in
//! `contract.gen.ts`, GENERATED from `agent/src/vocabulary.rs`. The one place allowed to turn an
//! ending into a `PathState` is `lib/path.ts` (`stateFromEnd`, and `cardState` as its two-line
//! adapter). Any OTHER production module that compares against one of those literals is a second
//! derivation, and this fails by file and line.
//!
//! WHAT IT DOES NOT CHECK: the mapping itself (that is the derivation's own tests), the shapes (the
//! sheet and rendered checks), or anything about the console — its device state is a different fact
//! with its own fixtures.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/one-derivation-check.mjs` → exit 0, "one-derivation: 139 panel module(s)
//!     scanned, and lib/path.ts is the only one that turns an ending (6 of them, plus the exited:
//!     prefix) into a state; 4 file(s) declared as sharing the WORDS without sharing the fact:
//!     components/EvidenceDrawer.tsx, components/UpdateCard.tsx, lib/evicted.ts,
//!     hooks/useCommandEvents.ts".
//!   * this file → the SAME sentence, digit for digit and name for name.
//!   * a planted `if (st === "timeout") { … }` in `components/MonitorsCard.tsx` → the JS gate exits
//!     1 naming `components/MonitorsCard.tsx:<line> compares against the ending "timeout"`, and this
//!     file fails with the identical file, LINE and word.
//!   * a planted `className="monitor-mark is-flapping"` in the same file → BOTH FAIL, and that is
//!     the clause's own recorded history: the first version of the pattern required a quote on both
//!     sides of the literal, so it passed the very mutation it was written for. The Rust scanner
//!     reproduces the FIXED rule (any string literal on the line).
//!   * a comment QUOTING the pattern (`// if (st === "timeout")`) → BOTH STAY GREEN, which is the
//!     honest measurement: comments are not derivations (round 136), and a gate that flags its own
//!     documentation is the kind that gets turned off.
//!
//! MUTATION: compare against an ending anywhere but `lib/path.ts`, or spell a mark state
//!           (`is-up`/`is-down`/`is-flapping`) outside `lib/monitorMark.ts`.
//! RESULT:   fails, printing the file, the LINE NUMBER and the literal, and the paragraph that says
//!           where an ending is allowed to become a state.

use std::fs;
use std::path::{Path, PathBuf};

/// `cargo test` runs from the CRATE root, not the repo root.
const CRATE: &str = env!("CARGO_MANIFEST_DIR");
const PANEL: &str = "agent/resources/panel-react/src";

fn repo() -> PathBuf {
    Path::new(CRATE)
        .parent()
        .expect("the crate lives one level below the repo root")
        .to_path_buf()
}

/// The one module allowed to derive a state from an ending.
const ALLOWED: [&str; 2] = ["lib/path.ts", "lib/contract.gen.ts"];

/// WORDS THAT ARE SHARED BUT FACTS THAT ARE NOT, declared per file with a reason. The first run of
/// this gate found seven comparisons; reading them took a minute and NONE was a second derivation —
/// they are four different vocabularies that happen to use the same words, plus the input side of the
/// derivation itself. That is the distinction the gate exists to make somebody write down, which is
/// why this is a list of reasons rather than a wider pattern.
const SHARED_WORDS: [(&str, &str); 4] = [
    ("components/EvidenceDrawer.tsx", "action verdicts (ok/timeout/err) — a browser action's state, not a command ending"),
    ("components/UpdateCard.tsx", "the update card's own phase machine"),
    ("lib/evicted.ts", "an eviction cause (idle/cap) — why a session was dropped"),
    ("hooks/useCommandEvents.ts", "UPSTREAM of the derivation: it turns a terminal marker into the reason stateFromEnd consumes"),
];

/// The mark families' CSS states (round 129). The rule is absolute and cheap: a mark's CSS state
/// literal may appear ONLY in the module that derives it.
const MARK_STATES: [(&str, &str); 3] = [
    ("is-flapping", "lib/monitorMark.ts"),
    ("is-up", "lib/monitorMark.ts"),
    ("is-down", "lib/monitorMark.ts"),
];

/// SHARED WORDS, DIFFERENT FACTS — `MonitorsCard`'s LOG ROWS spell `is-up`/`is-down` for a `<li>`,
/// not for a mark: the element is a transition entry and its own stylesheet rule is about log rows.
const MARK_STATE_EXCEPTIONS: [(&str, &str); 1] =
    [("components/MonitorsCard.tsx", "log rows, not marks")];

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for e in entries {
        let p = e.expect("a readable directory entry").path();
        if p.is_dir() {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if name == "__tests__" || name == "node_modules" {
                continue;
            }
            walk(&p, out);
        } else {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if name.ends_with(".ts") || name.ends_with(".tsx") {
                out.push(p);
            }
        }
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `[=!]==?\s*"LIT"` — `==`, `===`, `!=`, `!==`, whitespace, then the quoted literal.
fn compares_to(line: &str, lit: &str) -> bool {
    let c: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < c.len() {
        if c[i] != '=' && c[i] != '!' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if c.get(j) != Some(&'=') {
            i += 1;
            continue;
        }
        j += 1;
        if c.get(j) == Some(&'=') {
            j += 1;
        }
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        if c.get(j) == Some(&'"') {
            let rest: String = c[j + 1..].iter().collect();
            if rest.starts_with(lit) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// `case\s+"LIT"\s*:` — a switch arm.
fn case_arm(line: &str, lit: &str) -> bool {
    let c: Vec<char> = line.chars().collect();
    let kw: Vec<char> = "case".chars().collect();
    let mut i = 0;
    while i + kw.len() <= c.len() {
        if c[i..i + kw.len()] != kw[..] {
            i += 1;
            continue;
        }
        let mut j = i + kw.len();
        let ws = j;
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        if j == ws {
            i += 1;
            continue;
        }
        if c.get(j) != Some(&'"') {
            i += 1;
            continue;
        }
        let rest: String = c[j + 1..].iter().collect();
        if !rest.starts_with(lit) {
            i += 1;
            continue;
        }
        // `"LIT"` — the CLOSING quote is part of the pattern, then `\s*`, then `:`.
        let mut k = j + 2 + lit.chars().count();
        if c.get(k - 1) != Some(&'"') {
            i += 1;
            continue;
        }
        while k < c.len() && c[k].is_whitespace() {
            k += 1;
        }
        if c.get(k) == Some(&':') {
            return true;
        }
        i += 1;
    }
    false
}

/// `startsWith("PREFIX")` — a comparison against the prefix rather than against a whole ending.
fn starts_with_call(line: &str, prefix: &str) -> bool {
    let needle = format!("startsWith(\"{prefix}\")");
    line.contains(&needle)
}

/// `["'`][^"'`]*\bLIT\b[^"'`]*["'`]` — ANY string literal on the line, not just one that IS the
/// literal: the shape that regressed was `className="monitor-mark is-flapping"` — the state buried
/// inside a longer class list — and the first version of this pattern required a quote on both sides
/// of it, so it passed the very mutation it was written for.
fn spells_literal(line: &str, lit: &str) -> bool {
    let c: Vec<char> = line.chars().collect();
    let quote = |ch: char| ch == '"' || ch == '\'' || ch == '`';
    let mut i = 0;
    while i < c.len() {
        // A run of non-quote characters bounded by a quote on BOTH sides is a string literal.
        if quote(c[i]) {
            let start = i + 1;
            let mut end = start;
            while end < c.len() && !quote(c[end]) {
                end += 1;
            }
            if end < c.len() && end > start {
                let body: String = c[start..end].iter().collect();
                if word_bounded(&body, lit) {
                    return true;
                }
            }
            i = if end < c.len() { end } else { c.len() };
            continue;
        }
        i += 1;
    }
    false
}

/// `\bLIT\b` inside a run whose edges are quotes (non-word), so a literal at either edge counts.
fn word_bounded(body: &str, lit: &str) -> bool {
    let c: Vec<char> = body.chars().collect();
    let l: Vec<char> = lit.chars().collect();
    if l.is_empty() || l.len() > c.len() {
        return false;
    }
    for start in 0..=c.len() - l.len() {
        if c[start..start + l.len()] != l[..] {
            continue;
        }
        let before_ok = start == 0 || !is_word(c[start - 1]);
        let after = start + l.len();
        let after_ok = after == c.len() || !is_word(c[after]);
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

fn check(gen_src: &str, files: &[(String, String)]) -> Result<String, String> {
    // The endings, from the generated contract, so this check follows the vocabulary instead of
    // copying it.
    let reasons = parse_end_reasons(gen_src).ok_or_else(|| {
        "FAIL contract.gen.ts has no END_REASONS — this check would be scanning for nothing"
            .to_string()
    })?;
    let prefix = parse_exited_prefix(gen_src).ok_or_else(|| {
        "FAIL contract.gen.ts has no EXITED_PREFIX — this check would be scanning for nothing"
            .to_string()
    })?;

    let scanned = files.len();
    let mut offenders: Vec<String> = Vec::new();

    for (rel, src) in files {
        if ALLOWED.contains(&rel.as_str()) || SHARED_WORDS.iter().any(|(f, _)| f == rel) {
            continue;
        }
        for (i, line) in src.lines().enumerate() {
            // COMMENTS ARE NOT DERIVATIONS (round 136): without it, a comment that QUOTES the
            // pattern — which is how this file explains the rule — is reported as a second
            // derivation, and a gate that flags its own documentation is the kind that gets turned
            // off.
            if line.trim_start().starts_with("//") {
                continue;
            }
            for r in &reasons {
                if compares_to(line, r) || case_arm(line, r) {
                    offenders.push(format!(
                        "{rel}:{} compares against the ending \"{r}\"",
                        i + 1
                    ));
                }
            }
            if compares_to(line, &prefix) || starts_with_call(line, &prefix) {
                offenders.push(format!(
                    "{rel}:{} compares against the \"{prefix}\" prefix",
                    i + 1
                ));
            }
        }
    }

    for (rel, src) in files {
        if rel == "lib/monitorMark.ts" || (rel.starts_with("lib/") && rel.ends_with(".test.ts")) {
            continue;
        }
        if MARK_STATE_EXCEPTIONS.iter().any(|(f, _)| f == rel) {
            continue;
        }
        for (i, line) in src.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for (literal, home) in MARK_STATES {
                if spells_literal(line, literal) {
                    offenders.push(format!(
                        "{rel}:{} spells the mark state \"{literal}\", which {home} derives",
                        i + 1
                    ));
                }
            }
        }
    }

    if scanned < 60 {
        return Err(format!(
            "FAIL scanned only {scanned} panel module(s) — the tree moved, so this proves nothing"
        ));
    }
    if !offenders.is_empty() {
        return Err(format!(
            "one-derivation: {} second derivation(s) of an ending:\n  {}\n\nEndings become states in lib/path.ts ONLY \
             (stateFromEnd; cardState is its adapter). A second mapping is how one backgrounded command wore two states \
             in one view (round 29).",
            offenders.len(),
            offenders.join("\n  ")
        ));
    }
    let declared: Vec<&str> = SHARED_WORDS.iter().map(|(f, _)| *f).collect();
    Ok(format!(
        "one-derivation: {scanned} panel module(s) scanned, and lib/path.ts is the only one that turns an ending \
         ({} of them, plus the {prefix} prefix) into a state; {} file(s) declared as sharing the WORDS without \
         sharing the fact: {}",
        reasons.len(),
        declared.len(),
        declared.join(", ")
    ))
}

/// `export const END_REASONS = (\[[^\]]*\]) as const;` — the array literal, through `serde_json`
/// exactly as the JS gate puts it through `JSON.parse`. (The parentheses in that JS regex are GROUP
/// delimiters, not literal characters — reading them as literals is what the first draft of this
/// function did, and the floor is what caught it.)
fn parse_end_reasons(gen: &str) -> Option<Vec<String>> {
    const ANCHOR: &str = "export const END_REASONS = ";
    let rest = &gen[gen.find(ANCHOR)? + ANCHOR.len()..];
    let open = rest.find('[')?;
    let close = rest[open..].find(']')? + open;
    let json = &rest[open..=close];
    let v: Vec<String> = serde_json::from_str(json).ok()?;
    // ` as const;` must follow, or this matched some other array.
    if !rest[close + 1..].starts_with(" as const;") {
        return None;
    }
    Some(v)
}

/// `export const EXITED_PREFIX = "([^"]+)";`
fn parse_exited_prefix(gen: &str) -> Option<String> {
    const ANCHOR: &str = "export const EXITED_PREFIX = \"";
    let rest = &gen[gen.find(ANCHOR)? + ANCHOR.len()..];
    let end = rest.find('"')?;
    if end == 0 || !rest[end + 1..].starts_with(';') {
        return None;
    }
    Some(rest[..end].to_string())
}

fn panel_files() -> Vec<(String, String)> {
    let root = repo().join(PANEL);
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths
        .into_iter()
        .map(|p| {
            let rel = p
                .strip_prefix(&root)
                .expect("a walked path is under the panel root")
                .to_string_lossy()
                .replace('\\', "/");
            let src = fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()));
            (rel, src)
        })
        .collect()
}

fn read(rel: &str) -> String {
    let p = repo().join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

#[test]
fn lib_path_is_the_only_derivation_of_an_ending() {
    let msg = check(
        &read("agent/resources/panel-react/src/lib/contract.gen.ts"),
        &panel_files(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    println!("{msg}");
    // A FLOOR, NOT AN EQUALITY. This assertion used to pin the exact count ("139 panel
    // module(s) scanned"), and that number grows every time a panel module is added — so the
    // gate went red on the panel migration's two new files while the RULE it exists to check
    // was untouched: the scanner still reports the same four SHARED_WORDS and the same one
    // derivation. The gate already has a floor of its own (the scanner refuses below 60, "the
    // tree moved, so this proves nothing"), and this line was a second, stricter copy of it
    // that could only ever fail for the wrong reason.
    assert!(msg.contains("panel module(s) scanned"), "{msg}");
}

// ── the scanner's own proof: a parser that reads nothing must not pass ─────────────────────────

/// The gate's floor is checked BEFORE the offenders, exactly as the JS does — so a fixture that is
/// meant to exercise a RULE has to clear the floor first, or the floor's message is what comes back
/// and the fixture proves nothing about the rule.
fn panel_like(probe: &str) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = (0..60)
        .map(|i| {
            (
                format!("components/Filler{i}.tsx"),
                "export const x = 1;\n".to_string(),
            )
        })
        .collect();
    files.push(("components/Probe.tsx".to_string(), probe.to_string()));
    files
}

#[test]
fn a_second_derivation_is_reported_with_its_line() {
    let files = panel_like("const a = 1;\nconst b = 2;\nif (st === \"timeout\") { x(); }\n");
    let err = check(
        &read("agent/resources/panel-react/src/lib/contract.gen.ts"),
        &files,
    )
    .expect_err("a second derivation must fail");
    assert!(
        err.contains("components/Probe.tsx:3 compares against the ending \"timeout\""),
        "{err}"
    );
}

#[test]
fn a_comment_quoting_the_pattern_is_not_a_derivation() {
    let files = panel_like("// if (st === \"timeout\") { x(); }\n");
    let msg = check(
        &read("agent/resources/panel-react/src/lib/contract.gen.ts"),
        &files,
    )
    .expect("a comment is not a derivation, and the floor is cleared");
    assert!(msg.contains("61 panel module(s) scanned"), "{msg}");
    assert!(!msg.contains("compares against"), "{msg}");
}

#[test]
fn a_mark_state_spelled_outside_its_home_is_reported() {
    let files = panel_like("return <span className=\"monitor-mark is-flapping\" />;\n");
    let err = check(
        &read("agent/resources/panel-react/src/lib/contract.gen.ts"),
        &files,
    )
    .expect_err("a mark state outside its home must fail");
    assert!(
        err.contains("spells the mark state \"is-flapping\", which lib/monitorMark.ts derives"),
        "{err}"
    );
    // The word alone, with no quotes around it, is not a literal and must NOT bite.
    assert!(!spells_literal(
        "const isFlapping = compute();",
        "is-flapping"
    ));
    assert!(spells_literal("x = `a is-up b`;", "is-up"));
}

#[test]
fn a_missing_contract_array_is_not_a_pass() {
    let err = check("export const OTHER = [];\n", &[]).expect_err("no END_REASONS must fail");
    assert!(err.contains("no END_REASONS"), "{err}");
}

#[test]
fn the_case_arm_and_the_prefix_have_their_own_shapes() {
    assert!(case_arm("  case \"idle\":", "idle"));
    assert!(!case_arm("  case \"idle\"", "idle"));
    assert!(compares_to("if (r !== \"closed\") {", "closed"));
    assert!(starts_with_call(
        "if (k.startsWith(\"exited:\")) {",
        "exited:"
    ));
    assert!(compares_to("if (k === \"exited:\") {", "exited:"));
}
