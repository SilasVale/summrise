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
const SHARED_WORDS: [(&str, &str); 7] = [
    ("components/EvidenceDrawer.tsx", "action verdicts (ok/timeout/err) — a browser action's state, not a command ending"),
    ("components/UpdateCard.tsx", "the update card's own phase machine"),
    ("lib/evicted.ts", "an eviction cause (idle/cap) — why a session was dropped"),
    ("hooks/useCommandEvents.ts", "UPSTREAM of the derivation: it turns a terminal marker into the reason stateFromEnd consumes"),
    // AND THE TWO RUST HALVES OF THOSE LAST TWO, declared when the scan crossed the boundary into the
    // crate. They are the SAME facts with the same reasons — `events.rs` is where the terminal marker
    // becomes a reason now, and `evicted.rs` is where the cause is read — so they are listed rather
    // than silently exempted, which is the distinction this list exists to make somebody write down.
    ("events.rs", "UPSTREAM of the derivation — the Rust half of hooks/useCommandEvents.ts"),
    ("actions.rs", "action verdicts (ok/timeout/nocode/fail) — the Rust half of lib/browserAction.ts, whose \"timeout\" is a browser action's state and not a command ending"),
    ("evicted.rs", "an eviction cause (idle/cap) — the Rust half of lib/evicted.ts"),
];

/// The mark families' CSS states (round 129). The rule is absolute and cheap: a mark's CSS state
/// literal may appear ONLY in the module that derives it.
///
/// **AND THAT MODULE IS RUST NOW (block ②, 2026-09-30), SO THE SCAN FOLLOWS IT.** `lib/monitorMark.ts`
/// is a wrapper that calls `monitor_modifier`/`monitor_mark_class`, so the literals live in
/// `panel-logic/src/marks.rs` — and the panel's TypeScript may no longer spell them ANYWHERE, the
/// wrapper included. Both trees are scanned; the home is the crate-relative path.
const MARK_STATES: [(&str, &str); 3] = [
    ("is-flapping", "marks.rs"),
    ("is-up", "marks.rs"),
    ("is-down", "marks.rs"),
];

/// SHARED WORDS, DIFFERENT FACTS — `MonitorsCard`'s LOG ROWS spell `is-up`/`is-down` for a `<li>`,
/// not for a mark: the element is a transition entry and its own stylesheet rule is about log rows.
const MARK_STATE_EXCEPTIONS: [(&str, &str); 1] =
    [("components/MonitorsCard.tsx", "log rows, not marks")];

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    walk_where(dir, &[".ts", ".tsx"], out)
}

/// **THE WALKER FILTERS BY EXTENSION, AND THAT IS HOW THE CRATE SCAN WAS A NO-OP.** The first version
/// of the mark-state scan reused this function for `panel-logic/src`, collected ZERO files, and passed —
/// a scan that reads nothing certifies nothing, which is the failure this repository has recorded more
/// than once. It was caught by trying to make the gate REFUSE something (a planted literal in another
/// crate module) and watching it pass instead.
fn walk_where(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
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
            if name == "__tests__" || name == "node_modules" || name == "target" {
                continue;
            }
            walk_where(&p, extensions, out);
        } else {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if extensions.iter().any(|ext| name.ends_with(ext)) {
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

/// THE SAME TWO READERS, OVER THE WASM CRATE'S COPY — because the vocabulary MOVED (2026-09-29, P2).
///
/// `lib/path.ts`'s `stateFromEnd` was the only reader of `END_REASONS` and `EXITED_PREFIX`, and it is
/// `panel-logic/src/path.rs` now, so the generator stopped emitting them and `panel-logic/src/
/// vocabulary.rs` carries the list instead. **A check whose subject moved is not a check that was
/// broken** — and this gate would otherwise have refused itself with "this check would be scanning
/// for nothing", which is its own vacuity guard doing the right thing for the wrong reason.
fn parse_end_reasons_rust(vocab: &str) -> Option<Vec<String>> {
    const ANCHOR: &str = "pub const END_REASONS";
    let rest = &vocab[vocab.find(ANCHOR)? + ANCHOR.len()..];
    // THE TYPE COMES FIRST — `pub const END_REASONS: [&str; 6] = [` — and a reader that took the
    // first `[` got `[&str; 6]`, which is the bug this function's own first run had. The array is the
    // bracket that FOLLOWS the `=`.
    let eq = rest.find('=')?;
    let open = rest[eq..].find('[')? + eq;
    let close = rest[open..].find(']')? + open;
    let mut out = Vec::new();
    for line in rest[open..=close].lines() {
        // `"marker",` — the TRAILING COMMA is what the first version forgot, and `strip_suffix('"')`
        // then refused every line and the reader reported "no END_REASONS" against a file that has
        // six of them. A reader that finds nothing where the thing demonstrably is should say so.
        let t = line.trim().trim_end_matches(',');
        if let Some(inner) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            out.push(inner.to_string());
        }
    }
    (!out.is_empty()).then_some(out)
}

/// `pub const EXITED_PREFIX: &str = "exited:";`
fn parse_exited_prefix_rust(vocab: &str) -> Option<String> {
    const ANCHOR: &str = "EXITED_PREFIX: &str = \"";
    let rest = &vocab[vocab.find(ANCHOR)? + ANCHOR.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn check(vocab_src: &str, files: &[(String, String)]) -> Result<String, String> {
    // The endings, from the file the vocabulary now lives in, so this check follows it instead of
    // copying it.
    let reasons = parse_end_reasons_rust(vocab_src).ok_or_else(|| {
        "FAIL panel-logic/src/vocabulary.rs has no END_REASONS — this check would be scanning for \
         nothing"
            .to_string()
    })?;
    let prefix = parse_exited_prefix_rust(vocab_src).ok_or_else(|| {
        "FAIL panel-logic/src/vocabulary.rs has no EXITED_PREFIX — this check would be scanning for \
         nothing"
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
        // NO EXEMPTION FOR THE WRAPPER ANY MORE: `lib/monitorMark.ts` calls the crate, so a mark-state
        // literal in it would be a SECOND spelling — exactly what this rule exists to catch.
        if rel.starts_with("lib/") && rel.ends_with(".test.ts") {
            continue;
        }
        // AND NONE FOR WHAT WASM-PACK GENERATES. `wasm/panel_logic.d.ts` and its `.js` twin carry the
        // CRATE'S OWN DOC COMMENTS, so a module whose header quotes the state it derives would be
        // reported as a second spelling of it. The directory is a build artifact: nothing in it is
        // written by hand, and `agent/tests/panel_sheet_freshness.rs` is what proves it matches the source.
        if rel.starts_with("wasm/") {
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
                // THE HOME IS EXEMPT, and it is the same tuple that names it in the message: the rule is
                // "only the module that DERIVES the state may spell it", so that module must be allowed
                // to. (Removing the old by-name skip for the TypeScript wrapper took this exemption with
                // it, and the first run after the walker fix reported `marks.rs` as an offender three
                // times — which is what a gate that cannot see its own home looks like.)
                if rel == home {
                    continue;
                }
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

// THE TWO TYPESCRIPT READERS ARE GONE WITH THE EXPORT THEY READ. `parse_end_reasons` and
// `parse_exited_prefix` parsed `contract.gen.ts`, and the generator stopped emitting those two
// constants when the derivation that was their only reader moved into the wasm crate — so they had
// no caller and `cargo clippy -D warnings` named them, which is the same rule `exports-check` applies
// to a TypeScript export: an export nothing imports is a promise nobody asked for. The Rust readers
// above replaced them, and their own two unit cases moved with them.
//
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
    let mut files = panel_files();
    files.extend(crate_files());
    let msg = check(
        &read("agent/resources/panel-logic/src/vocabulary.rs"),
        &files,
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
/// THE CRATE'S SOURCES, with the file name as `rel` — so `marks.rs` is the home the table above names.
/// The panel's rule and this one are the same rule, and a scan that stopped at the boundary would
/// certify half of it.
fn crate_files() -> Vec<(String, String)> {
    let root = repo().join("agent/resources/panel-logic/src");
    let mut paths = Vec::new();
    walk_where(&root, &[".rs"], &mut paths);
    paths
        .into_iter()
        .map(|p| {
            let rel = p
                .strip_prefix(&root)
                .expect("a walked path is under the crate root")
                .to_string_lossy()
                .replace('\\', "/");
            let src = fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()));
            (rel, src)
        })
        .collect()
}

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
        &read("agent/resources/panel-logic/src/vocabulary.rs"),
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
        &read("agent/resources/panel-logic/src/vocabulary.rs"),
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
        &read("agent/resources/panel-logic/src/vocabulary.rs"),
        &files,
    )
    .expect_err("a mark state outside its home must fail");
    assert!(
        err.contains("spells the mark state \"is-flapping\", which marks.rs derives"),
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
    let err =
        check("pub const OTHER: [&str; 0] = [];\n", &[]).expect_err("no END_REASONS must fail");
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
