//! THE POWERSHELL THAT RUNS AS ADMINISTRATOR ON A CUSTOMER'S MACHINE IS NEVER PARSED HERE.
//!
//! WHY THIS EXISTS (rounds 28, 31, 32). `agent/deploy/*.ps1` is the installer, the tunnel repair and
//! the integrity library — the code that runs AS ADMINISTRATOR on a customer's box. No `pwsh` exists
//! on this machine, so not one line of it is ever EXECUTED here. And `scripts/test/script-syntax.bash`
//! walks `git ls-files '*.sh' '*.bash'` — `.ps1` is NOT among them. What those rounds did instead was a
//! brace/paren census BY HAND against the previous version, which is exactly the manual check that
//! stops happening. So the defect that can reach a customer is the one nothing here can see: an edit
//! that leaves a brace, a paren or a bracket unbalanced, or a string unterminated — which `pwsh`
//! rejects in a second.
//!
//! WHAT IT IS: a structural scan, NOT a parser. It answers ONE question — do `{}`, `()` and `[]`
//! balance OUTSIDE single-quoted strings, single-quoted here-strings and comments, and INSIDE every
//! `$( … )` subexpression a DOUBLE-QUOTED thing carries — whether that thing is a `"…"` string or the
//! body of a `@" … "@` here-string, which PowerShell expands by the same rules and which is therefore
//! scanned by the same pass (`expanding_body`). The stripping is what makes it correct rather than a
//! false alarm: the installer writes launcher scripts whose bodies contain braces
//! (`'… { exit }; …'`), the integrity tests carry JSON in single-quoted strings (`'{"version":…}'`),
//! and the retired installer writes tunnel.yml into a `@" … "@` here-string — so a naive count fails
//! on correct code.
//!
//! THE SELF-TEST TABLE IS PART OF THE INSTRUMENT, not decoration: a scanner whose skip rules were
//! never themselves tested is the instrument this suite trusts least, and `SELF_TEST` below is where
//! those rules are stated as VERDICTS. `installer_integrity.rs` keeps its own pins; this is a
//! DIFFERENT instrument — syntax SHAPE, not wiring — and neither weakens nor duplicates the other.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/powershell-structure-check.mjs` → exit 0, stdout 279 bytes, the three-line
//!     "7 .ps1 file(s) … 13 scanner fixture(s) agree" report; this file → the SAME 279 bytes (`cmp`).
//!   * a `{` planted in `agent/deploy/fix-tunnel.ps1` → BOTH exit 1 with the same body: the FAIL
//!     header, the one-line "cannot be read" variant, and every problem line. Measured below.
//!   * THE CASE THAT MUST NOT BITE, measured in both: a NEW `.ps1` holding `'{"a":1} {'` — braces
//!     that are DATA — plus a balanced `$( … { … } … )` in a double-quoted string, stays at exit 0 in
//!     both implementations, with the file count moving 7 → 8 in both reports.
//!
//! MUTATION: open a `{` in a deploy `.ps1` and never close it.
//! RESULT:   fails, naming the file, the line and the character: "`{` is opened here and never
//!           closed — `pwsh` would reject the file", after the header that says this code runs as
//!           administrator and nothing on this box parses it.
//!
//! WHAT THIS CANNOT SEE, so the next reader does not mistake it for a parser:
//!   * TEXT, BY DESIGN. A brace inside a `'…'` string or a `@' … '@` here-string is DATA. A
//!     `${name}`/`$name` is the same kind of thing: its braces name a variable, they do not group.
//!   * ANYTHING SEMANTIC. A misspelled cmdlet, a wrong parameter, a missing `param` block, whether the
//!     code would RUN. `pwsh` answers those and is not installed here, which is the whole reason this
//!     check exists at the level it does.
//!   * THE WALK SORTS BY BYTES, where the JS sorts by UTF-16 code unit. Identical for every path that
//!     is ASCII, which is every path under `agent/deploy`; a non-ASCII filename would order the
//!     problem list differently, and the verdict — not the order — is what the two share.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

const DIR: &str = "agent/deploy";

/// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN, and this suite has paid for that lesson already
/// (round 170 measured it for `script-syntax.bash`: an empty file list printed "0 files parse" and
/// exited 0). 7 `.ps1` files live under `agent/deploy` today; the floor leaves room for a deliberate
/// removal and none for a collapse.
const FLOOR: usize = 5;

/// A TABLE THAT LOST ITS ROWS IS NOT A TABLE THAT PASSED — the same rule as `FLOOR`, for the
/// instrument rather than the subject: the four verdicts this check is defined by are the minimum, and
/// an empty table would otherwise print "0 scanner fixture(s) agree" and exit 0.
const TABLE_FLOOR: usize = 4;

fn open_of(c: char) -> Option<char> {
    match c {
        '{' => Some('}'),
        '(' => Some(')'),
        '[' => Some(']'),
        _ => None,
    }
}

fn close_of(c: char) -> Option<char> {
    match c {
        '}' => Some('{'),
        ')' => Some('('),
        ']' => Some('['),
        _ => None,
    }
}

/// JavaScript's `\s`, which is what `/^[^\S\n]*\r?\n/` asks with: Rust's `char::is_whitespace` is the
/// White_Space property and JS's `\s` is that property PLUS U+FEFF, the byte-order mark. The
/// difference is one character wide and this is where it would have been.
fn js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// The two things `expanding_body` is asked to end on: a `"…"` string at a bare `"`, and a `@" … "@`
/// here-string at `"@` at the START of a line. On the JS side these are two closures passed to one
/// function; here they are one enum, because the only thing that differs IS the rule, and an enum
/// makes that visible at the call site.
#[derive(Clone, Copy, PartialEq)]
enum Term {
    Str,
    HereDoc,
}

struct Scanner {
    c: Vec<char>,
    file: String,
    i: usize,
    line: usize,
    stack: Vec<(char, usize)>,
    problems: Vec<String>,
}

impl Scanner {
    fn new(src: &str, file: &str) -> Self {
        Scanner {
            c: src.chars().collect(),
            file: file.to_string(),
            i: 0,
            line: 1,
            stack: Vec::new(),
            problems: Vec::new(),
        }
    }

    fn at(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }

    /// Advance to `j`, counting the newlines crossed. Every caller passes a `j` at or after the
    /// cursor, which is the only shape the JS uses.
    fn skip_to(&mut self, j: usize) {
        let j = j.min(self.c.len());
        for k in self.i..j {
            if self.c[k] == '\n' {
                self.line += 1;
            }
        }
        self.i = j;
    }

    fn fail(&mut self, at: usize, why: &str) {
        self.problems.push(format!("{}:{at} — {why}", self.file));
    }

    fn find_seq(&self, from: usize, pat: &[char]) -> Option<usize> {
        let mut j = from;
        while j + pat.len() <= self.c.len() {
            if self.c[j..j + pat.len()] == *pat {
                return Some(j);
            }
            j += 1;
        }
        None
    }

    /// A SINGLE-QUOTED STRING: `''` is an escaped quote, and everything else is literal — INCLUDING
    /// braces, which is how the installer writes launcher scripts and how the integrity tests carry
    /// JSON manifests. Returns false when it is never closed.
    fn single_quoted(&mut self) -> bool {
        let at = self.line;
        let n = self.c.len();
        let mut j = self.i + 1;
        let mut closed = false;
        while j < n {
            if self.c[j] == '\'' {
                if self.c.get(j + 1) == Some(&'\'') {
                    j += 2; // an escaped quote, still inside
                    continue;
                }
                closed = true;
                break;
            }
            j += 1;
        }
        if !closed {
            self.fail(
                at,
                "a single-quoted string is never closed — `pwsh` would reject the file",
            );
            return false;
        }
        self.skip_to(j + 1);
        true
    }

    /// Whether the body ends AT the cursor, consuming the terminator when it does.
    fn step(&mut self, kind: Term) -> bool {
        match kind {
            Term::Str => {
                if self.at(0) != Some('"') {
                    return false;
                }
                if self.at(1) == Some('"') {
                    self.skip_to(self.i + 2); // an escaped quote, still inside
                    return false;
                }
                self.skip_to(self.i + 1);
                true
            }
            Term::HereDoc => {
                // The terminator rule IS the body rule here: `"@` only ends the here-string at the
                // start of a line, and an earlier `"@` — or a lone `"` inside the body — is text.
                if self.at(0) != Some('\n') || self.at(1) != Some('"') || self.at(2) != Some('@') {
                    return false;
                }
                self.skip_to(self.i + 3);
                true
            }
        }
    }

    /// A DOUBLE-QUOTED BODY — the inside of a `"…"` string AND the inside of a `@" … "@` here-string,
    /// which PowerShell EXPANDS by the same rules and so is scanned by the same pass. A backtick
    /// escapes the next character and `""` is an escaped quote.
    ///
    /// `$( … )` INSIDE IT IS NOT TEXT. PowerShell evaluates it, so its braces and parens belong to the
    /// file's structure exactly like brackets outside the string — and a launcher line is where they
    /// are written. The subexpression's `(` joins the SAME stack, its body is scanned as code, and it
    /// ends at the `)` that closes it. A plain `$name` — and `${name}`, whose braces are part of the
    /// NAME, not a group — is a variable reference and stays opaque.
    fn expanding_body(&mut self, kind: Term, at: usize, why: &str) -> bool {
        let n = self.c.len();
        while self.i < n {
            if self.step(kind) {
                return true;
            }
            let c = self.c[self.i];
            if c == '`' {
                self.skip_to((self.i + 2).min(n)); // the backtick escapes whatever follows
                continue;
            }
            if c == '$' && self.at(1) == Some('(') {
                self.stack.push(('(', self.line));
                self.skip_to(self.i + 2);
                let depth = self.stack.len();
                if !self.code(Some(depth)) {
                    return false; // the subexpression, ended by the `)` closing this `(`
                }
                continue;
            }
            if c == '\n' {
                self.line += 1;
            }
            self.i += 1;
        }
        self.fail(at, why);
        false
    }

    /// A DOUBLE-QUOTED STRING `"…"`: `""` is an escaped quote, and everything else is the body above.
    fn double_quoted(&mut self) -> bool {
        let at = self.line;
        self.skip_to(self.i + 1); // the opening quote
        self.expanding_body(
            Term::Str,
            at,
            "a double-quoted string is never closed — `pwsh` would reject the file",
        )
    }

    /// `@'` / `@"` followed by optional horizontal space and a newline — PowerShell's rule that the
    /// body starts on the NEXT line. `/^[^\S\n]*\r?\n/`.
    fn heredoc_opens(&self) -> bool {
        let n = self.c.len();
        let mut j = self.i + 2;
        while j < n && js_space(self.c[j]) && self.c[j] != '\n' {
            j += 1;
        }
        if j < n && self.c[j] == '\r' {
            j += 1;
        }
        j < n && self.c[j] == '\n'
    }

    /// CODE — outside every string, and inside a `$( … )` subexpression. `sub_depth` is the stack
    /// depth at which the subexpression's own `(` was pushed, or `None` outside one: a `)` arriving at
    /// that depth ends the subexpression and hands control back to the string that opened it. Returns
    /// false when a string or comment ran away, which stops the scan — `fail` has already said why.
    fn code(&mut self, sub_depth: Option<usize>) -> bool {
        let n = self.c.len();
        while self.i < n {
            let c = self.c[self.i];
            let at = self.line;

            // A COMMENT IS NOT CODE: `#` to end of line, and `<# … #>` — which the installer's own
            // header uses.
            if c == '<' && self.at(1) == Some('#') {
                match self.find_seq(self.i + 2, &['#', '>']) {
                    None => {
                        self.fail(at, "`<#` opens a block comment that is never closed with `#>`, so `pwsh` would reject the file");
                        return false;
                    }
                    Some(end) => {
                        self.skip_to(end + 2);
                        continue;
                    }
                }
            }
            if c == '#' {
                match self.find_seq(self.i, &['\n']) {
                    Some(end) => self.skip_to(end),
                    None => self.skip_to(n),
                }
                continue;
            }

            // A HERE-STRING IS OPENED AT THE END OF A LINE AND CLOSED BY `'@` / `"@` AT THE START OF
            // ONE (PowerShell 5.1's rule, and these scripts are `#Requires -Version 5.1`). A body
            // holding an odd number of quotes would otherwise desynchronize every line below it. THE
            // TWO FORMS ARE NOT THE SAME:
            //   * `@' … '@` expands NOTHING. Its body is RAW TEXT — an installer writing a config or a
            //     JSON manifest puts exactly that in one — so braces inside it are DATA, not groups,
            //     and it is skipped whole.
            //   * `@" … "@` expands variable references AND `$( … )` subexpressions, exactly as a `"…"`
            //     string does, so its body goes through `expanding_body` and the subexpressions'
            //     brackets land on the file's own stack.
            if c == '@' && matches!(self.at(1), Some('\'') | Some('"')) && self.heredoc_opens() {
                let q = self.at(1).expect("checked just above");
                if q == '"' {
                    self.skip_to(self.i + 2); // past the opening `@"`; the newline is part of the body
                    if !self.expanding_body(
                        Term::HereDoc,
                        at,
                        "`@\"` opens a here-string that is never closed by `\"@` at the start of a line",
                    ) {
                        return false;
                    }
                    continue;
                }
                match self.find_seq(self.i + 2, &['\n', q, '@']) {
                    None => {
                        self.fail(at, &format!("`@{q}` opens a here-string that is never closed by `{q}@` at the start of a line"));
                        return false;
                    }
                    Some(end) => {
                        self.skip_to(end + 3);
                        continue;
                    }
                }
            }

            if c == '\'' {
                if !self.single_quoted() {
                    return false;
                }
                continue;
            }
            if c == '"' {
                if !self.double_quoted() {
                    return false;
                }
                continue;
            }

            // OUTSIDE a string a backtick escapes the next character too: `` `{ `` is a literal brace,
            // not a group, and a backtick at the end of a line is a continuation.
            if c == '`' {
                self.skip_to((self.i + 2).min(n));
                continue;
            }

            if open_of(c).is_some() {
                self.stack.push((c, at));
                self.i += 1;
                continue;
            }
            if let Some(want) = close_of(c) {
                // THE SUBEXPRESSION ENDS HERE: its own `(` is on top, so it is consumed and the string
                // resumes.
                if c == ')' && sub_depth == Some(self.stack.len()) {
                    self.stack.pop();
                    self.i += 1;
                    return true;
                }
                match self.stack.pop() {
                    None => self.fail(
                        at,
                        &format!("`{c}` closes nothing — there is one closer too many"),
                    ),
                    Some((ch, ln)) => {
                        if ch != want {
                            self.fail(at, &format!("`{c}` closes the `{ch}` opened at line {ln}"));
                        }
                    }
                }
                self.i += 1;
                continue;
            }

            if c == '\n' {
                self.line += 1;
            }
            self.i += 1;
        }
        true
    }
}

/// The problems found in one source, each naming the line it starts on.
pub fn scan(src: &str, file: &str) -> Vec<String> {
    let mut s = Scanner::new(src, file);
    s.code(None);
    let stack = std::mem::take(&mut s.stack);
    for (ch, line) in stack {
        s.fail(
            line,
            &format!("`{ch}` is opened here and never closed — `pwsh` would reject the file"),
        );
    }
    s.problems
}

// ── THE SCANNER'S OWN FIXTURES. pwsh is not installed here, so these snippets are the only place the
// rules above are stated as VERDICTS — and the instrument had never been tested, only its subject. A
// skip rule one character too greedy reports "balanced" over precisely the edit this check exists to
// catch. The rows that carry the weight are the pairs: the SAME brace must FAIL inside a `$( … )` and
// PASS inside a single-quoted string, in a here-string exactly as in a plain string — because the
// installer writes launcher scripts and JSON manifests that hold braces, and a scanner that cannot
// tell those apart fails correct code and gets reverted within a week.
const SELF_TEST: [(&str, &str, bool); 13] = [
    ("a `{` opened inside a `$( … )` in a double-quoted string", "\"$(Get-X { )\"", false),
    ("braces that are TEXT — the JSON the integrity tests carry in `'…'`", "'{\"a\":1}'", true),
    ("`${name}` is a variable NAME, not a group", "\"${name}\"", true),
    ("a balanced `$( … )` carrying a group", "\"$(Get-X -A { 1 })\"", true),
    ("a `$( … )` nested inside another one", "\"$(Get-X -A \"$(Get-Y)\")\"", true),
    ("a backtick still escapes inside the string: `` `$( `` is literal", "\"`$(Get-X { )\"", true),
    ("a `)` inside a single-quoted string does not end the subexpression", "\"$(Get-X -A ')')\"", true),
    ("a `#` comment's braces are not code", "# { and (\n( )\n", true),
    // THE HERE-STRING ROWS. A `@" … "@` body is EXPANDED by PowerShell, so it is the same region as
    // the rows above and must get the same verdicts; a `@' … '@` body is not, and must stay opaque.
    ("a `{` opened inside a `$( … )` in a DOUBLE-QUOTED here-string", "@\"\n$(Get-X { )\n\"@\n", false),
    ("the same braces are TEXT inside a SINGLE-quoted here-string", "@'\n{ $(Get-X )\n'@\n", true),
    ("a balanced `$( … )` carrying a group, inside `@\" … \"@`", "@\"\n$(Get-X -A { 1 })\n\"@\n", true),
    ("`${name}` inside a here-string is a variable NAME, not a group", "@\"\n${name}\n\"@\n", true),
    (
        "an INDENTED `\"@` does not end the here-string: the body runs on to the real `\"@` at column 0",
        "@\"\n  \"@ is text\n\"@\n",
        true,
    ),
];

/// The two streams, separated the way the JS separates them: the report goes to stdout, every failure
/// to stderr.
struct Streams {
    stdout: String,
    stderr: String,
    failed: bool,
}

fn ok(stdout: String) -> Streams {
    Streams {
        stdout,
        stderr: String::new(),
        failed: false,
    }
}

fn err(stderr: String) -> Streams {
    Streams {
        stdout: String::new(),
        stderr,
        failed: true,
    }
}

/// A table that lost its rows is not a table that passed.
fn table_floor(n: usize) -> Option<String> {
    (n < TABLE_FLOOR).then(|| {
        format!(
            "FAIL powershell-structure: only {n} scanner fixture(s) — the table has been emptied, so nothing states what the skip rules mean any more\n"
        )
    })
}

/// The self-test pass, as a stream — the instrument's verdict on itself.
fn fixture_report() -> Streams {
    if let Some(e) = table_floor(SELF_TEST.len()) {
        return err(e);
    }
    let mut stderr = String::new();
    let mut failures = 0;
    for (what, snippet, clean) in SELF_TEST {
        let got = scan(snippet, "<fixture>");
        if got.is_empty() != clean {
            failures += 1;
            stderr.push_str(&format!(
                "FAIL powershell-structure self-test: {what} — expected {}, got {}{}\n",
                if clean { "PASS" } else { "FAIL" },
                if clean { "FAIL" } else { "PASS" },
                if got.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", got.join("; "))
                }
            ));
        }
    }
    if failures > 0 {
        return err(format!(
            "{stderr}FAIL powershell-structure: {failures} of {} fixture(s) disagree — the scanner no longer\n\
             follows the rules this file states, so its verdict on the deploy .ps1 files proves nothing.\n",
            SELF_TEST.len()
        ));
    }
    ok(String::new())
}

/// PROBLEMS BEFORE THE FLOOR, and the order is load-bearing: when the walk found files but could not
/// READ them, every one of them landed in `problems` while `checked` stayed 0 — and the floor's "read
/// only 0 file(s)" fired first and buried the reason. A loud failure that hides its own cause is half
/// a loud failure.
fn report(problems: &[String], checked: usize) -> Streams {
    if !problems.is_empty() {
        let mut stderr = format!(
            "FAIL powershell-structure: {} structural problem(s) in {checked} file(s). This code runs AS ADMINISTRATOR on a\n\
             customer's machine, no `pwsh` on this box ever parses it, and `script-syntax.bash` does not walk `.ps1`:\n\n",
            problems.len()
        );
        for p in problems {
            stderr.push_str(&format!("  {p}\n"));
        }
        return err(stderr);
    }
    if checked < FLOOR {
        return err(format!(
            "FAIL powershell-structure: read only {checked} .ps1 file(s) under {DIR}, expected at least {FLOOR} — the scan is reading the wrong thing\n"
        ));
    }
    ok(format!(
        "powershell-structure: {checked} .ps1 file(s) under {DIR} balance {{}} () [] outside single-quoted strings,\n\
         single-quoted here-strings and comments — and inside the `$( … )` subexpressions a double-quoted string or a\n\
         `@\" … \"@` here-string carries. {} scanner fixture(s) agree.\n",
        SELF_TEST.len()
    ))
}

/// Every `.ps1` under `dir`, recursively, sorted. A directory WALK rather than `git ls-files`, so a
/// file that cannot be read is a FAILURE here rather than an absence — the one thing this check must
/// never do is pass quietly over something it did not look at.
fn ps1_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d)? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.to_string_lossy().ends_with(".ps1") {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn run() -> Streams {
    let fixtures = fixture_report();
    if fixtures.failed {
        return fixtures;
    }

    let root = common::repo();
    let found = match ps1_files(&root.join(DIR)) {
        Ok(f) => f,
        Err(e) => {
            return err(format!(
                "FAIL powershell-structure: cannot walk {DIR} ({}) — a scan that cannot read its subject proves nothing\n",
                e.kind()
            ))
        }
    };

    let mut problems: Vec<String> = Vec::new();
    let mut checked = 0;
    for abs in &found {
        let rel = abs
            .strip_prefix(&root)
            .unwrap_or(abs)
            .to_string_lossy()
            .replace('\\', "/");
        match fs::read(abs) {
            // LOUD, NOT SILENT: a file this check cannot read is a FAILURE, never an absence from the
            // count. Bytes that are not UTF-8 are decoded LOSSILY, which is what `readFileSync(f,
            // "utf8")` does — `read_to_string` would ERROR and take the walk down with it.
            Err(e) => problems.push(format!(
                "{rel} — CANNOT BE READ ({}); a file this check cannot read must never count as a pass",
                e.kind()
            )),
            Ok(bytes) => {
                checked += 1;
                problems.extend(scan(&String::from_utf8_lossy(&bytes), &rel));
            }
        }
    }
    report(&problems, checked)
}

#[test]
fn the_deploy_powershell_is_structurally_sound() {
    let out = run();
    print!("{}", out.stdout);
    if out.failed {
        panic!("{}", out.stderr);
    }
    // A FLOOR, not a claim: the count itself is the tree's business. What must not happen is the walk
    // reading almost nothing and reporting balance over it.
    let checked: usize = out
        .stdout
        .split_once("powershell-structure: ")
        .and_then(|(_, rest)| rest.split(' ').next())
        .and_then(|n| n.parse().ok())
        .expect("the report names the file count");
    assert!(
        checked >= FLOOR,
        "read only {checked} .ps1 file(s) — the walk is looking at the wrong thing"
    );
    assert!(
        out.stdout
            .contains(&format!("{} scanner fixture(s) agree.", SELF_TEST.len())),
        "{}",
        out.stdout
    );
}

// ── the scanner's own proof, beyond the table: the instrument's rules on fixtures, and the two
//    failure paths that are about the SCAN rather than the subject ─────────────────────────────────

#[test]
fn every_self_test_row_gets_the_verdict_it_states() {
    for (what, snippet, clean) in SELF_TEST {
        let got = scan(snippet, "<fixture>");
        assert_eq!(
            got.is_empty(),
            clean,
            "{what} — expected {}, got {} ({got:?})",
            if clean { "PASS" } else { "FAIL" },
            if got.is_empty() { "PASS" } else { "FAIL" }
        );
    }
    assert!(fixture_report().stdout.is_empty(), "the table agrees today");
    assert!(!fixture_report().failed);
}

#[test]
fn a_table_that_lost_its_rows_is_reported_rather_than_passing() {
    let e = table_floor(3).expect("three rows is below the floor");
    assert!(e.contains("only 3 scanner fixture(s)"), "{e}");
    assert!(table_floor(TABLE_FLOOR).is_none());
}

#[test]
fn a_problem_is_reported_before_the_floor_can_hide_it() {
    // The measured shape: the walk found files, could not READ any of them, so `checked` is 0 and the
    // problem carries the reason. The floor must not fire first.
    let problems = vec!["agent/deploy/x.ps1 — CANNOT BE READ (permission denied); a file this check cannot read must never count as a pass".to_string()];
    let out = report(&problems, 0);
    assert!(out.failed);
    assert!(
        out.stderr.contains("1 structural problem(s) in 0 file(s)"),
        "{}",
        out.stderr
    );
    assert!(out.stderr.contains("CANNOT BE READ"), "{}", out.stderr);
    assert!(
        !out.stderr.contains("read only 0"),
        "the floor must not bury the reason: {}",
        out.stderr
    );
    // ...and with nothing wrong, the floor is what refuses a scan that read almost nothing.
    let floor = report(&[], 3);
    assert!(floor.failed);
    assert!(
        floor.stderr.contains("read only 3 .ps1 file(s)"),
        "{}",
        floor.stderr
    );
    assert!(report(&[], FLOOR).stdout.contains("balance {} () []"));
}

#[test]
fn a_file_that_cannot_be_read_is_a_failure_not_an_absence() {
    let out = run();
    assert!(
        !out.stderr.contains("CANNOT BE READ"),
        "every deploy .ps1 is readable today: {}",
        out.stderr
    );
    assert!(
        out.stdout.contains(".ps1 file(s) under agent/deploy"),
        "{}",
        out.stdout
    );
}
