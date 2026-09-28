//! A COMMENT IN A STYLESHEET MUST NOT LOOK LIKE CODE.
//!
//! WHY THIS EXISTS (round 87). The console's sheet carried this:
//!
//!     /* ── Model catalogue admin: add form, per-chip remove {
//!       display: flex;
//!       align-items: center;
//!       gap: 8px;
//!       flex-wrap: wrap;
//!     }
//!     .model-add .form-input {
//!       flex: 0 1 auto;
//!       min-width: 0;
//!     }
//!
//! Four `/*` and one `*/` in that region. A CSS comment runs to the NEXT `*/`, so the real rule
//! `.model-add .form-input` was inside a comment and had never applied — while every instrument that
//! reads the sheet as TEXT (the pair sweep, the dead-CSS pruner, both design contracts) saw it and
//! judged it as live. A browser and a regex disagreed, and only the browser was rendering.
//!
//! The panel had the other shape of the same problem: a comment reading "--muted on --surface-chip
//! measured 4.40 in the running app" contains `--muted: … ;` — declaration-shaped prose. Round 86
//! proved that pattern can fool a token reader (it made `--surface` resolve to a sentence). Comments
//! are documentation; they must not be parseable as rules.
//!
//! WHAT IT CHECKS, on every stylesheet the repo ships or serves:
//!   1. comments are BALANCED (a `/*` without its `*/` swallows whatever follows);
//!   2. no comment contains a rule-like selector line (`{`-terminated) or a declaration-shaped
//!      `--token: value;`;
//!   3. NO ORPHANED `*/` OUTSIDE a comment. The walk below finds `/*` and takes the NEXT `*/`, so a
//!      stray close — a comment that ended early, with prose after it — is skipped over as ordinary
//!      text and the sheet looks clean. THE BROWSER DOES NOT SKIP IT: `*/` in a selector prelude is a
//!      parse error, and the parser then discards everything up to the next `}`, so ONE stray close
//!      silently deleted a large region of the panel's sheet — the marks, the session rows and the
//!      marks' own sizes stopped applying — while every gate here stayed green and the page merely
//!      LOOKED wrong.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/stylesheet-hygiene.mjs` → exit 0, "stylesheet-hygiene: 2 sheet(s) clean —
//!     comments are balanced and none of them reads as code".
//!   * this file → the SAME sentence, character for character.
//!   * an ORPHANED `*/` planted at the top of the console's `globals.css` → the JS gate exits 1 with
//!     "ORPHANED '*/' outside any comment — the browser reads it as a parse error and drops rules
//!     until the next '}', which is how one stray close deleted a large region of this sheet", and
//!     this file fails with the identical sentence and the identical line number.
//!   * declaration-shaped prose inside a comment → BOTH FAIL on the same `--token: value;` clause.
//!   * a `/*` with no `*/` → BOTH FAIL with "UNTERMINATED comment — it swallows the rest of the file".
//!
//! MUTATION: plant an ORPHANED `*/` outside any comment, or a `/*` with no close, or a
//!           declaration-shaped `--token: value;` inside a comment.
//! RESULT:   fails, naming the file, the LINE and which of the three shapes it found.

mod common;

use common::repo;
use std::fs;

const SHEETS: [&str; 2] = [
    "agent/resources/panel/panel.css",
    "gateway/ui/src/styles/globals.css",
];

/// `css.slice(0, at).split("\n").length` — the 1-based line an offset sits on.
///
/// `at` IS A CHAR INDEX, NOT A BYTE ONE, and the first version of this treated it as bytes: these
/// sheets carry `─` (3 bytes, 1 char), so the slice panicked on a boundary inside a box-drawing
/// character. The JS indexes by UTF-16 unit; indexing by `char` agrees on every sheet here.
fn line_of(c: &[char], at: usize) -> usize {
    c[..at].iter().filter(|&&ch| ch == '\n').count() + 1
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `^[ \t]*[.#][A-Za-z][\w.\- ]*\{` — a rule-like line inside a comment.
fn rule_like_lines(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in body.lines() {
        let t = line.trim_start_matches([' ', '\t']);
        let mut it = t.chars();
        let (Some(first), Some(second)) = (it.next(), it.next()) else {
            continue;
        };
        if (first != '.' && first != '#') || !second.is_ascii_alphabetic() {
            continue;
        }
        // `[\w.\- ]*` then `{`.
        let run: String = t
            .chars()
            .skip(2)
            .take_while(|&c| is_word(c) || c == '.' || c == '-' || c == ' ')
            .collect();
        // `.`/`#` and the letter are ASCII, so 2 is a valid BYTE offset, and `run.len()` is one too.
        let after = 2 + run.len();
        if t[after..].starts_with('{') {
            // The match starts at the line start, so it carries the leading `[ \t]*`.
            out.push(format!(
                "{}{}",
                &line[..line.len() - t.len()],
                &t[..after + 1]
            ));
        }
    }
    out
}

/// `^[^\n]*\{[ \t]*\n[ \t]*[a-z-]+\s*:\s*[^;\n]+;` — a comment that has swallowed a rule's SELECTOR.
///
/// The selector-shaped check above needs a leading `.` or `#`, so it missed real damage a previous
/// prune left in the console's sheet: a selector line became `is not a dot {` — no dot, no hash — and
/// the comment went on holding `margin-bottom: 14px;` and its closing brace for as long as nobody read
/// it. `.models-card` lost its margin and the sheet stopped parsing there. WHAT IS UNAMBIGUOUS IS THE
/// SHAPE, not the name.
fn swallowed_rule(body: &str) -> Option<String> {
    let c: Vec<char> = body.chars().collect();
    let space = |ch: char| ch == ' ' || ch == '\t';
    let any_space = |ch: char| ch.is_whitespace();
    // `^` positions, in order; `[^\n]*` is greedy, so the LAST `{` on the line is tried first.
    let mut line_start = 0usize;
    while line_start <= c.len() {
        let line_end = c[line_start..]
            .iter()
            .position(|&ch| ch == '\n')
            .map(|k| line_start + k)
            .unwrap_or(c.len());
        let braces: Vec<usize> = (line_start..line_end).filter(|&k| c[k] == '{').collect();
        for &b in braces.iter().rev() {
            let mut j = b + 1;
            while j < c.len() && space(c[j]) {
                j += 1;
            }
            if c.get(j) != Some(&'\n') {
                continue;
            }
            j += 1;
            while j < c.len() && space(c[j]) {
                j += 1;
            }
            let sel = j;
            while j < c.len() && (c[j].is_ascii_lowercase() || c[j] == '-') {
                j += 1;
            }
            if j == sel {
                continue;
            }
            while j < c.len() && any_space(c[j]) {
                j += 1;
            }
            if c.get(j) != Some(&':') {
                continue;
            }
            j += 1;
            while j < c.len() && any_space(c[j]) {
                j += 1;
            }
            let val = j;
            while j < c.len() && c[j] != ';' && c[j] != '\n' {
                j += 1;
            }
            if j == val || c.get(j) != Some(&';') {
                continue;
            }
            return Some(c[line_start..=j].iter().collect());
        }
        if line_end >= c.len() {
            break;
        }
        line_start = line_end + 1;
    }
    None
}

/// `--[a-z0-9-]+\s*:\s*[^;*\n]+;` — declaration-shaped prose a token reader can mistake for a value.
fn declaration_shaped(body: &str) -> Option<String> {
    let c: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i + 1 < c.len() {
        if c[i] != '-' || c[i + 1] != '-' {
            i += 1;
            continue;
        }
        let mut j = i + 2;
        let name = j;
        while j < c.len() && (c[j].is_ascii_lowercase() || c[j].is_ascii_digit() || c[j] == '-') {
            j += 1;
        }
        if j == name {
            i += 1;
            continue;
        }
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        if c.get(j) != Some(&':') {
            i += 1;
            continue;
        }
        j += 1;
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        let val = j;
        while j < c.len() && c[j] != ';' && c[j] != '*' && c[j] != '\n' {
            j += 1;
        }
        if j == val || c.get(j) != Some(&';') {
            i += 1;
            continue;
        }
        return Some(c[i..=j].iter().collect());
    }
    None
}

/// JS `slice(0, 40)` counts UTF-16 units; every stylesheet here is BMP, so counting `char`s agrees.
fn first40(s: &str) -> String {
    s.chars().take(40).collect()
}

fn check(sheets: &[(String, Option<String>)]) -> Result<String, String> {
    let mut problems: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for (rel, body) in sheets {
        // A sheet this check expects to read is GONE: that is a problem, and it does not count as
        // one of the sheets that were read.
        let Some(css) = body else {
            problems.push(format!(
                "{rel}: missing — a sheet this check expects to read is gone"
            ));
            continue;
        };
        checked += 1;

        // 1. Walk it the way a parser does: each comment ends at the NEXT `*/`.
        let c: Vec<char> = css.chars().collect();
        let mut spans: Vec<(usize, usize)> = Vec::new();
        let mut pos = 0usize;
        while let Some(a) = find_from(&c, pos, &['/', '*']) {
            match find_from(&c, a + 2, &['*', '/']) {
                Some(b) => {
                    spans.push((a, b + 2));
                    pos = b + 2;
                }
                None => {
                    spans.push((a, c.len()));
                    problems.push(format!(
                        "{rel}:{}: UNTERMINATED comment — it swallows the rest of the file",
                        line_of(&c, a)
                    ));
                    break;
                }
            }
        }

        // 3. AN ORPHANED CLOSE IS A PARSE ERROR, NOT TEXT — checked in the GAPS between the spans,
        // because that is exactly where the walk above cannot see one.
        let mut cursor = 0usize;
        let mut orphan_lines: Vec<usize> = Vec::new();
        for &(a, b) in &spans {
            if let Some(at) = find_from(&c, cursor, &['*', '/']).filter(|&k| k < a) {
                orphan_lines.push(line_of(&c, cursor + at));
            }
            cursor = b;
        }
        if let Some(at) = find_from(&c, cursor, &['*', '/']) {
            orphan_lines.push(line_of(&c, cursor + at));
        }
        for line in orphan_lines {
            problems.push(format!(
                "{rel}:{line}: ORPHANED '*/' outside any comment — the browser reads it as a parse error and drops rules until the next '}}', which is how one stray close deleted a large region of this sheet"
            ));
        }

        for &(a, b) in &spans {
            let body: String = c[a..b].iter().collect();
            let line = line_of(&c, a);
            let selectors = rule_like_lines(&body);
            if !selectors.is_empty() {
                problems.push(format!(
                    "{rel}:{line}: comment contains {} rule-like line(s), e.g. \"{}\" — a comment must not hold a rule",
                    selectors.len(),
                    first40(selectors[0].trim())
                ));
            }
            if let Some(s) = swallowed_rule(&body) {
                let head = s.split('\n').next().unwrap_or("").trim();
                problems.push(format!(
                    "{rel}:{line}: a comment has swallowed a rule — \"{}\" is followed by a declaration, so a selector was lost (in a sheet, that rule no longer exists)",
                    first40(head)
                ));
            }
            if let Some(d) = declaration_shaped(&body) {
                problems.push(format!(
                    "{rel}:{line}: comment contains a declaration-shaped token \"{}\" — a token reader can mistake prose for a value (round 86)",
                    first40(&d)
                ));
            }
        }
    }

    // A check that reads nothing must not report success.
    // "A check that reads nothing must not report success." The JS compares against its module
    // constant; here it is the LIST PASSED IN, which is the same thing on the real call (the real
    // call passes exactly `SHEETS`) and is what lets a fixture prove the floor.
    if checked != sheets.len() {
        return Err(format!(
            "expected to read {} stylesheets, read {checked}",
            sheets.len()
        ));
    }
    if !problems.is_empty() {
        let mut out = format!("{} stylesheet hygiene problem(s):", problems.len());
        for p in &problems {
            out.push_str(&format!("\n  {p}"));
        }
        return Err(out);
    }
    Ok(format!(
        "stylesheet-hygiene: {checked} sheet(s) clean — comments are balanced and none of them reads as code"
    ))
}

/// The index of the two-character needle starting at or after `from`.
fn find_from(c: &[char], from: usize, needle: &[char; 2]) -> Option<usize> {
    let mut i = from;
    while i + 1 < c.len() {
        if c[i] == needle[0] && c[i + 1] == needle[1] {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// The sheets as (relative path, contents) — a missing sheet is a problem, not a panic.
fn sheets() -> Vec<(String, Option<String>)> {
    SHEETS
        .iter()
        .map(|rel| {
            let full = repo().join(rel);
            let body = if full.exists() {
                Some(
                    fs::read_to_string(&full)
                        .unwrap_or_else(|e| panic!("cannot read {}: {e}", full.display())),
                )
            } else {
                None
            };
            (rel.to_string(), body)
        })
        .collect()
}

#[test]
fn every_sheet_has_balanced_comments_that_do_not_read_as_code() {
    let loaded = sheets();
    for (rel, body) in &loaded {
        assert!(
            body.is_some(),
            "{rel}: missing — a sheet this check expects to read is gone"
        );
    }
    let msg = check(&loaded).unwrap_or_else(|e| panic!("{e}"));
    println!("{msg}");
    assert!(msg.contains("2 sheet(s) clean"), "{msg}");
}

// ── the scanner's own proof: a parser that reads nothing must not pass ─────────────────────────

#[test]
fn an_orphaned_close_outside_a_comment_is_a_problem() {
    let err = check(&[(
        "x.css".to_string(),
        Some("a */ b { color: red; }\n".to_string()),
    )])
    .expect_err("an orphaned close must fail");
    assert!(
        err.contains("x.css:1: ORPHANED '*/' outside any comment"),
        "{err}"
    );
    // Inside a comment it is fine.
    let msg = check(&[(
        "x.css".to_string(),
        Some("/* a */ b { color: red; }\n".to_string()),
    )])
    .expect("a closed comment is clean");
    assert!(msg.contains("1 sheet(s) clean"), "{msg}");
}

#[test]
fn an_unterminated_comment_swallows_the_rest_of_the_file() {
    let err = check(&[(
        "x.css".to_string(),
        Some("a { color: red; }\n/* never closed\n".to_string()),
    )])
    .expect_err("an unterminated comment must fail");
    assert!(err.contains("x.css:2: UNTERMINATED comment"), "{err}");
}

#[test]
fn the_three_shapes_inside_a_comment_are_each_reported() {
    // A rule-like selector line — the round-87 shape, where the SELECTOR sits on its OWN line
    // inside the comment. A `/* .model-add {` on the comment's first line is not this clause's
    // business (the line starts with `/`, not `.` or `#`), in either implementation.
    let err = check(&[(
        "x.css".to_string(),
        Some("/* prose\n.model-add .form-input {\n  flex: 0 1 auto;\n*/\n".to_string()),
    )])
    .expect_err("a rule-like line must fail");
    assert!(
        err.contains("comment contains 1 rule-like line(s)"),
        "{err}"
    );
    assert!(err.contains("e.g. \".model-add .form-input {\""), "{err}");
    // Declaration-shaped prose.
    let err = check(&[(
        "x.css".to_string(),
        Some("/* --muted: 4.40 measured; */\n".to_string()),
    )])
    .expect_err("declaration-shaped prose must fail");
    assert!(
        err.contains("declaration-shaped token \"--muted: 4.40 measured;\""),
        "{err}"
    );
    // A swallowed selector with no dot or hash — the shape the selector clause cannot see.
    let err = check(&[(
        "x.css".to_string(),
        Some("/* is not a dot {\n  margin-bottom: 14px;\n*/\n".to_string()),
    )])
    .expect_err("a swallowed selector must fail");
    assert!(err.contains("a comment has swallowed a rule"), "{err}");
    // A `#id` selector and a `.class` selector are both rule-like — the clause anchors at the LINE
    // START, so the selector must open the line (indentation is allowed).
    assert_eq!(rule_like_lines("  #a {").len(), 1);
    assert_eq!(rule_like_lines("\t.a-b {").len(), 1);
    // A line that opens with `/*` is not this clause's business, in either implementation.
    assert!(rule_like_lines("/* .a-b { */").is_empty());
    // Prose that merely mentions a name is not.
    assert!(rule_like_lines("/* the --muted token */").is_empty());
}

#[test]
fn a_comment_that_holds_no_rule_is_not_a_finding() {
    let msg = check(&[(
        "x.css".to_string(),
        Some(
            "/* --muted on --surface-chip measured 4.40 in the running app */\n.a { color: red; }\n"
                .to_string(),
        ),
    )])
    .expect("prose without a declaration shape is clean");
    assert!(msg.contains("1 sheet(s) clean"), "{msg}");
}
