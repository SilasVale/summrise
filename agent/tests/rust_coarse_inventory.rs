//! EVERY TRACKED RUST FILE OF 800 LINES OR MORE, AND WHETHER ITS TOP-LEVEL ITEMS ARE TOO COARSE.
//!
//! **THE RULE.** A tracked `.rs` file that is **>= 800 lines** AND **more than 120 lines per top-level item**
//! must be NAMED in `agent/tests/fixtures/rust-coarse-files.txt`, with a reason that says what its items ARE.
//! Anything over the line that is not named fails this gate, and so does a name whose file is no longer over
//! it — a list that only grows is a list nobody prunes.
//!
//! **WHY THIS GATE EXISTS, AND IT IS NOT "BIG FILES ARE BAD".** Nine surfaces were measured by hand in the
//! session recorded in `docs/agents/ideas.md` ("a calibrated yardstick", rounds 165-176), and exactly ONE was
//! real debt. The same measurement run over the WHOLE tree — every tracked `.rs` file of 800 lines or more —
//! named SEVEN files, five of which no hand-picked sample had ever opened. **A sample concludes "there is
//! nothing more" because it only contains what was picked**; this gate is the whole-tree scan, so that sentence
//! can never again be reached by looking at a list somebody chose.
//!
//! **THE ITEM PATTERN IS PINNED BELOW, AND PINNED BY A TEST, BECAUSE THE RATIO MOVES WITH IT.** It is
//! `docs/agents/ideas.md`'s pattern, character for character (see `ITEM_KEYWORDS`). `agent/src/web/mod.rs`
//! measures **121.5** lines per item under it and **156.3** under a `fn`-only reading — both measured on this
//! tree (9,844 lines; 81 items against 63). A gate whose pattern is implicit drifts one "improvement" at a time,
//! and every recorded number in the manifest moves with it, so the pattern is written down AS DATA and the test
//! below fails if it changes. **A number without its instrument is a number about somebody else's tree.**
//!
//! **AND THE 225 THE DESIGN SESSION RECORDED FOR THAT FILE DOES NOT REPRODUCE HERE, WHICH IS WHY IT IS NOT
//! RESTATED.** It implies 44 items at the 9,888 lines the file then had, and no reading of THIS tree answers 44:
//! 34 `async fn`, 63 `fn` at column 0, 223 `fn` at any indent, 71 with every visibility counted, and 43
//! `RouteId` variants are what the file actually holds. The wide figure reproduces (125 then, 121.5 now, on a
//! file that has since shrunk by 44 lines); the narrow one does not, and a number that does not reproduce is a
//! finding about the instrument rather than a fact about the file.
//!
//! **AND THE PATTERN HAS TWO BLIND SPOTS THAT THE REASONS HAVE TO SPEAK AROUND**, because they are properties of
//! the instrument rather than of the files: `^(pub )?` matches neither `pub(crate) fn` nor `pub(super) fn`, and
//! there is no `use` arm. `agent/src/evidence.rs` is on the list at 140.3 and measures **61.4** when the file's
//! own `pub(crate)` items are counted too, because eight of its twelve top-level fns are invisible to `^(pub )?`;
//! and the largest function in `agent/src/plugins/terminal/tools/exec.rs` — `tool_execute`, 823 lines — is
//! `pub(super) fn` and is INVISIBLE to the pattern the ratio was computed with. Both facts are in the reasons.
//! Fixing them here instead would change every measurement this manifest records, so the honest move is a pinned
//! pattern plus a reason that says where it cannot see.
//!
//! **THE MANIFEST'S PATTERNS ARE EXACT PATHS, UNLIKE ITS JAVASCRIPT SIBLING'S.** `js-boundaries.txt` is
//! per-directory with named exceptions because its list runs to hundreds of files that every UI change rewrites
//! (it prints its own count, which is the only number about it worth quoting); this list is seven files, and a
//! directory rule here would swallow the next file that crosses the line SILENTLY — which is the precise failure
//! this gate exists to end.
//!
//! MUTATION: delete one file's line from the manifest (here: `agent/src/spawn.rs`, the last entry).
//! RESULT:   exit 101 —
//!             these tracked Rust files are 800+ lines AND coarser than 120 lines per top-level item, and
//!             the manifest does not name them:
//!               agent/src/spawn.rs — 1693 lines / 14 items = 120.9
//!             FIX: name the file in agent/tests/fixtures/rust-coarse-files.txt with `path | lines | items |
//!             tests | reason`, where the reason says what its top-level items ARE — measured, not asserted —
//!             or make the file finer-grained (lines per item may only fall).
//!
//! MUTATION: plant a NEW coarse file — `agent/src/tmp_coarse_probe.rs`, 899 lines with 3 items — and
//!           `git add -N` it (the scan reads `git ls-files`, so a file that is not in the index is not in the
//!           repository as far as this gate is concerned, and intent-to-add is enough).
//! RESULT:   exit 101 — the same message, naming
//!             agent/src/tmp_coarse_probe.rs — 899 lines / 3 items = 299.7
//!           Removing the file and resetting the index returns it to green, and `git status --short` shows
//!           nothing of it.
//!
//! MUTATION: lower `MAX_LINES_PER_ITEM` to 40.
//! RESULT:   exit 101 — the FAILING list grows from one entry to THIRTY, every 800+ line file above the new
//!           ceiling, each with its own `lines / items = ratio`, because the ceiling is read from the constant
//!           rather than restated anywhere.
//!
//! MUTATION: append 20 comment lines to `agent/sweep-plan/src/tools/panel.rs` — an unlisted file, 821 lines /
//!           7 items = 117.3 — so it crosses at 841.
//! RESULT:   exit 101 —
//!             agent/sweep-plan/src/tools/panel.rs — 841 lines / 7 items = 120.1
//!           A file does not have to be NEW to cross: it only has to grow.
//!
//! MUTATION: raise one entry's recorded `lines` by one (`agent/src/spawn.rs`, 1693 -> 1694).
//! RESULT:   exit 101 —
//!             agent/tests/fixtures/rust-coarse-files.txt:80: `agent/src/spawn.rs` records a measurement the
//!             tree does not have: lines: manifest says 1694, measured 1693
//!             FIX: write the numbers this gate measured (1693 lines / 14 items = 120.9) — the reason's claims
//!             rest on them, so a stale number is a reason nobody re-read.
//!
//! MUTATION: "improve" the pattern — teach it `pub(crate) ` — which is the drift the header warns about and the
//!           one edit that would silently move every number in the manifest.
//! RESULT:   exit 101, TWO tests red: `the_item_pattern_is_the_one_that_was_measured` panics with
//!           `the pattern does not count `pub(crate) fn helper() {}``, and the manifest test fails on the
//!           recorded numbers, which no longer match the tree. The pattern has two independent guards.
//!
//! MUTATION: raise `MIN_RUST_FILES` to 10,000 — the floor that says the scan looked at the repository.
//! RESULT:   exit 101 —
//!             this scan saw only 378 tracked Rust files (floor 10000) — a gate that looked at almost nothing
//!             must not pass
//!
//! **THE NUMBERS IN THE MANIFEST ARE CHECKED, NOT DECORATION, AND THAT IS A DELIBERATE COST.** Every edit to a
//! named file re-measures it and the gate refuses until the entry is updated. The alternative is the defect
//! `js-boundaries.txt` recorded against itself: a reason that said "nine" where the file's whole history never
//! held more than eight — *"a number in a prose reason is enforced by nobody, so it goes stale instead of
//! failing"*. The recorded numbers are the tripwire that makes a reader look at the reason again.
//!
//! **WHAT THIS GATE DOES NOT DO, STATED RATHER THAN IMPLIED: it does not refuse a coarse file.** The operator's
//! rule is that such a file must be NAMED, with a reason, so a human decides — and the gate therefore PRINTS
//! which of the named files are coarse in their CODE and which are coarse because of their own test module
//! (a file whose ratio falls under the ceiling once its test items are removed is measured, not argued about).
//! A gate that refused the state the operator has not yet decided to change is a gate that gets switched off,
//! which is the failure `js_boundary_inventory.rs` already records twice.
//!
//! AND THE PRINT IS ON THE PASSING PATH ON PURPOSE: reading the measurement used to require breaking the gate,
//! and a measurement nobody can take without a deliberate break is one nobody takes. Test stdout is captured
//! unless it fails, so the ledger is visible under
//! `cargo test -p summrise-agent --test rust_coarse_inventory -- --nocapture`. That is one flag, not one mutation.

mod common;

use std::collections::BTreeMap;

/// The manifest. One entry per line: `path | lines | items | tests | reason`.
const MANIFEST: &str = "agent/tests/fixtures/rust-coarse-files.txt";

/// A file SHORTER than this is not measured at all: below it, lines per item is a statement about a small file.
const MIN_LINES: usize = 800;

/// The ceiling, and the rule is STRICTLY greater — 120.0 exactly is inside it.
const MAX_LINES_PER_ITEM: usize = 120;

/// Floors, not claims: the scan must see the repository that exists today. A gate that looked at almost nothing
/// must not pass, and `git ls-files` answering an empty list is exactly how that happens.
const MIN_RUST_FILES: usize = 300;
/// ...and the same floor on what it MEASURED, because "no file crossed the line" and "no file was read" are two
/// different facts and only one of them is good news. Measured 2026-10-15: 378 tracked `.rs` files, 52 of them
/// 800+ lines.
const MIN_MEASURED_FILES: usize = 40;

/// **THE KEYWORDS OF THE PINNED PATTERN, AND THE PATTERN IS `docs/agents/ideas.md`'s, VERBATIM:**
///
/// ```text
/// ^(pub )?(async )?(fn|struct|enum|impl|const|static|type|trait|mod)
/// ```
///
/// Two things about it are load-bearing and are NOT to be "improved" here:
///
///   * **NO `\b`.** `models = 3;` matches `mod`, exactly as the regex above does. Adding a boundary would move
///     every recorded number in the manifest, which is why `the_item_pattern_is_the_one_that_was_measured`
///     pins the quirk rather than tolerating it.
///   * **NO `pub(crate)` AND NO `use`.** `pub(crate) fn` and `pub(super) fn` are invisible; so is every import.
///     This is the blind spot the header names, and it is why one file on the list measures 140.3 here and 61.4
///     when its own `pub(crate)` items are counted (16 items rather than the 7 the pattern can see).
const ITEM_KEYWORDS: [&str; 9] = [
    "fn", "struct", "enum", "impl", "const", "static", "type", "trait", "mod",
];

/// `^(pub )?(async )?(fn|struct|enum|impl|const|static|type|trait|mod)` — hand-rolled rather than a regex
/// crate, the way `docs_budget.rs` hand-rolls its own two: a new dependency for one anchored literal prefix is
/// not a trade this repository makes. The two optional groups are consumed in the regex's own order, and with
/// the regex's own greediness — `pub async fn` matches, `async pub fn` does not.
fn is_top_level_item(line: &str) -> bool {
    let rest = line.strip_prefix("pub ").unwrap_or(line);
    let rest = rest.strip_prefix("async ").unwrap_or(rest);
    ITEM_KEYWORDS.iter().any(|k| rest.starts_with(k))
}

/// **COUNT `#[test]` ATTRIBUTES, NEVER TEST FILES** — the rule `docs/agents/ideas.md` paid for: a
/// `find -path "*/tests/*"` count said `proxies/` had 3 tests where 11 files carry 93, and this repository puts
/// its tests IN the file. The line is taken trimmed, so `#[tokio::test(flavor = "multi_thread")]` counts (it is
/// one attribute) and a `#[test]` named inside a comment does not (it does not start the trimmed line).
fn is_test_attribute(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("#[test]") || t.starts_with("#[tokio::test]")
}

/// What the pinned pattern measures in one file.
#[derive(Debug)]
struct Shape {
    /// `str::lines()`'s count — the way `wc -l` counts a file that ends in a newline, which every tracked `.rs`
    /// here does. A file with no final newline would earn one more line, which is the safe direction for a scan
    /// that only measures files of 800 lines or more.
    lines: usize,
    items: usize,
    /// `#[test]` + `#[tokio::test]` attributes in the whole file.
    tests: usize,
    /// The lines covered by items whose span carries a test attribute, and how many such items. This is the
    /// file's own test module measured by CONTENT rather than by name, so a `mod workspace_registry_tests` is
    /// counted the same way a `mod tests` is.
    test_lines: usize,
    test_items: usize,
}

impl Shape {
    fn measure(text: &str) -> Self {
        let lines: Vec<&str> = text.lines().collect();
        let starts: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| is_top_level_item(l))
            .map(|(i, _)| i)
            .collect();
        let mut test_lines = 0;
        let mut test_items = 0;
        for (k, &start) in starts.iter().enumerate() {
            let end = starts.get(k + 1).copied().unwrap_or(lines.len());
            if lines[start..end].iter().any(|l| is_test_attribute(l)) {
                test_items += 1;
                test_lines += end - start;
            }
        }
        Self {
            lines: lines.len(),
            items: starts.len(),
            tests: lines.iter().filter(|l| is_test_attribute(l)).count(),
            test_lines,
            test_items,
        }
    }

    /// `lines / items`. An item-less file answers INFINITY rather than panicking: a file of 900 lines with no
    /// top-level item is as coarse as a file can be, and a gate must not divide by zero to say so.
    fn ratio(&self) -> f64 {
        if self.items == 0 {
            f64::INFINITY
        } else {
            self.lines as f64 / self.items as f64
        }
    }

    /// The rule, in integers: strictly more than `MAX_LINES_PER_ITEM` lines per item.
    fn is_over(&self) -> bool {
        self.lines > MAX_LINES_PER_ITEM * self.items
    }

    /// The same ratio with the file's own test items taken out — the number that separates "this file is a
    /// coarse program" from "this file is a program plus a large suite". `None` when nothing but tests is left.
    fn ratio_without_tests(&self) -> Option<f64> {
        let items = self.items - self.test_items;
        if items == 0 {
            None
        } else {
            Some((self.lines - self.test_lines) as f64 / items as f64)
        }
    }

    fn describe(&self) -> String {
        format!(
            "{} lines / {} items = {}",
            self.lines,
            self.items,
            ratio_text(self.ratio())
        )
    }
}

/// A float for a report: one decimal, or `inf` where the arithmetic has no answer.
fn ratio_text(r: f64) -> String {
    if r.is_finite() {
        format!("{r:.1}")
    } else {
        "inf".to_string()
    }
}

/// One named file: the path, the three numbers this gate re-measures, and the reason a reader checks.
#[derive(Debug)]
struct Entry {
    path: String,
    lines: usize,
    items: usize,
    tests: usize,
    reason: String,
    line: usize,
}

fn manifest() -> Vec<Entry> {
    let text = common::read(MANIFEST);
    let mut entries: Vec<Entry> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('|').map(str::trim).collect();
        assert!(
            parts.len() >= 5,
            "{MANIFEST}:{}: an entry needs `path | lines | items | tests | reason`, got: {line}",
            i + 1
        );
        let number = |field: &str, which: &str| -> usize {
            field.parse().unwrap_or_else(|_| {
                panic!(
                    "{MANIFEST}:{}: `{field}` is not a number for {which}: {line}",
                    i + 1
                )
            })
        };
        entries.push(Entry {
            path: parts[0].to_string(),
            lines: number(parts[1], "lines"),
            items: number(parts[2], "items"),
            tests: number(parts[3], "tests"),
            // A reason may carry a `|` — it is prose, and prose here quotes code. The last field is joined
            // back together rather than truncated, so an entry cannot be silently mis-parsed.
            reason: parts[4..].join(" | "),
            line: i + 1,
        });
    }
    entries
}

/// Every tracked `.rs` file, measured; and the subset the rule applies to.
fn scan() -> (Vec<String>, Vec<(String, Shape)>) {
    let files: Vec<String> = common::git_ls_files_all()
        .into_iter()
        .filter(|f| f.ends_with(".rs"))
        .collect();
    assert!(
        files.len() >= MIN_RUST_FILES,
        "this scan saw only {} tracked Rust files (floor {MIN_RUST_FILES}) — a gate that looked at almost \
         nothing must not pass",
        files.len()
    );
    let measured: Vec<(String, Shape)> = files
        .iter()
        .map(|f| (f.clone(), Shape::measure(&common::read(f))))
        .filter(|(_, s)| s.lines >= MIN_LINES)
        .collect();
    assert!(
        measured.len() >= MIN_MEASURED_FILES,
        "only {} tracked Rust files reached {MIN_LINES} lines (floor {MIN_MEASURED_FILES}) — either the tree \
         lost its large files or this scan stopped reading them, and the two are told apart by looking",
        measured.len()
    );
    (files, measured)
}

#[test]
fn every_coarse_rust_file_is_named_and_every_name_is_still_coarse() {
    let entries = manifest();
    let (files, measured) = scan();

    let mut failures: Vec<String> = Vec::new();

    // ── ONE NAME PER FILE, AND THE FILE EXISTS ──────────────────────────────────────────────────────────────
    let mut named: BTreeMap<&str, &Entry> = BTreeMap::new();
    for e in &entries {
        if let Some(previous) = named.insert(e.path.as_str(), e) {
            failures.push(format!(
                "{MANIFEST}:{}: `{}` is named twice (first at line {}) — one file, one reason",
                e.line, e.path, previous.line
            ));
        }
        if !files.iter().any(|f| f == &e.path) {
            failures.push(format!(
                "{MANIFEST}:{}: `{}` is not a tracked Rust file",
                e.line, e.path
            ));
        }
        if e.reason.is_empty() {
            failures.push(format!(
                "{MANIFEST}:{}: `{}` is named with no reason — the name is the gate's demand and the reason is \
                 its whole value",
                e.line, e.path
            ));
        }
    }

    // ── THE RULE: OVER THE LINE AND UNNAMED ────────────────────────────────────────────────────────────────
    let unnamed: Vec<String> = measured
        .iter()
        .filter(|(_, s)| s.is_over())
        .filter(|(f, _)| !named.contains_key(f.as_str()))
        .map(|(f, s)| format!("  {f} — {}", s.describe()))
        .collect();
    if !unnamed.is_empty() {
        failures.push(format!(
            "these tracked Rust files are {MIN_LINES}+ lines AND coarser than {MAX_LINES_PER_ITEM} lines per \
             top-level item, and the manifest does not name them:\n{}\n\
             FIX: name the file in {MANIFEST} with `path | lines | items | tests | reason`, where the reason \
             says what its top-level items ARE — measured, not asserted — or make the file finer-grained \
             (lines per item may only fall).",
            unnamed.join("\n")
        ));
    }

    // ── AND THE OTHER DIRECTION: A NAME THAT IS NO LONGER OVER THE LINE IS FINISHED WORK WEARING A BUDGET ──
    for e in &entries {
        // A duplicate path was reported above; its numbers are checked once, against the first entry.
        if named.get(e.path.as_str()).map(|n| n.line) != Some(e.line) {
            continue;
        }
        let Some((_, shape)) = measured.iter().find(|(f, _)| f == &e.path) else {
            failures.push(format!(
                "{MANIFEST}:{}: `{}` is named but is no longer {MIN_LINES} lines — below that the ratio is a \
                 statement about a small file",
                e.line, e.path
            ));
            continue;
        };
        if !shape.is_over() {
            failures.push(format!(
                "{MANIFEST}:{}: `{}` is named but is no longer over the line — it measures {}. A list that only \
                 grows is a list nobody prunes.\n\
                 FIX: delete the entry in this commit, or the reason it carried is decoration.",
                e.line,
                e.path,
                shape.describe()
            ));
            continue;
        }
        // ── THE RECORDED NUMBERS ARE CHECKED, SO A REASON CANNOT GO STALE SILENTLY ──────────────────────────
        let mut drift: Vec<String> = Vec::new();
        if e.lines != shape.lines {
            drift.push(format!(
                "lines: manifest says {}, measured {}",
                e.lines, shape.lines
            ));
        }
        if e.items != shape.items {
            drift.push(format!(
                "items: manifest says {}, measured {}",
                e.items, shape.items
            ));
        }
        if e.tests != shape.tests {
            drift.push(format!(
                "tests: manifest says {}, measured {}",
                e.tests, shape.tests
            ));
        }
        if !drift.is_empty() {
            failures.push(format!(
                "{MANIFEST}:{}: `{}` records a measurement the tree does not have: {}\n\
                 FIX: write the numbers this gate measured ({}) — the reason's claims rest on them, so a stale \
                 number is a reason nobody re-read.",
                e.line,
                e.path,
                drift.join(", "),
                shape.describe()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "rust-coarse-inventory: {} finding(s):\n{}",
        failures.len(),
        failures.join("\n")
    );

    // ── THE LEDGER, ON THE PASSING PATH: WHICH OF THE NAMES ARE COARSE CODE AND WHICH ARE A TEST MODULE ─────
    let mut over: Vec<&(String, Shape)> = measured.iter().filter(|(_, s)| s.is_over()).collect();
    // Worst first, so the ledger is a ranking rather than the index order of `git ls-files`.
    over.sort_by(|a, b| {
        b.1.ratio()
            .partial_cmp(&a.1.ratio())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    println!(
        "rust-coarse-inventory: {} tracked Rust files, {} of them {MIN_LINES}+ lines, {} over \
         {MAX_LINES_PER_ITEM} lines per top-level item (the manifest names {})",
        files.len(),
        measured.len(),
        over.len(),
        entries.len()
    );
    for (path, shape) in &over {
        let without = match shape.ratio_without_tests() {
            Some(r) if r > MAX_LINES_PER_ITEM as f64 => {
                format!("STILL {} WITHOUT ITS TESTS", ratio_text(r))
            }
            Some(r) => format!("{} without its tests", ratio_text(r)),
            None => "nothing but tests".to_string(),
        };
        println!(
            "  {:5} lines  {:3} items  {:>6}/item  {:3} test attrs  — {}  [{without}]",
            shape.lines,
            shape.items,
            ratio_text(shape.ratio()),
            shape.tests,
            path
        );
    }
    for e in &entries {
        println!("  {}:{}\n      {}", MANIFEST, e.line, e.reason);
    }
    // THE AGGREGATE THE TEST-ATTRIBUTE FLOOR RESTS ON, printed rather than left in a comment: this is the share
    // of the named files that a deletion could remove without making anything finer-grained.
    let (lines, test_lines) = over
        .iter()
        .fold((0, 0), |(l, t), (_, s)| (l + s.lines, t + s.test_lines));
    if lines > 0 {
        println!(
            "  {} of the {lines} lines in those files are inside their own test modules ({:.0}%) — the share \
             that says a ratio is measuring a suite rather than a program",
            test_lines,
            100.0 * test_lines as f64 / lines as f64
        );
    }
}

/// **THE TEST-ATTRIBUTE FLOOR, AND WHY IT IS A FLOOR RATHER THAN ONE MORE CAP.**
///
/// The ratio above has a cheapest fix that is not a fix: DELETE THE TESTS. Measured on the tree that carries
/// this gate, the manifest's seven files hold **11,642 of their 22,774 lines inside their own test modules** —
/// 6,052 of `web/mod.rs`'s 9,844, 1,764 of `terminal/mod.rs`'s 3,779, 1,260 of `session_log.rs`'s 2,768, 1,156
/// of `spawn.rs`'s 1,693, 598 of `evidence.rs`'s 982, 455 of `exec.rs`'s 1,956, 357 of `report.rs`'s 1,752 — so a
/// file can be brought under a lines-per-item ceiling by deleting the tests that made it big, and a gate that
/// cannot see that has a hole in the shape of its own subject. **The gate prints this share on its passing
/// path**, because a number that lives only in a comment is a number nothing enforces.
///
/// **IN ATTRIBUTES, NOT FILES**, because that is the instrument this repository paid for: a file count said
/// `proxies/` had 3 tests where 11 files carry 93. **WHOLE-TREE, NOT PER-FILE**, because tests MOVE here (the
/// e2e judge, the packaging suite) and a per-file floor would call a legitimate extraction a deletion. **AND
/// THE DIRECTION IS `>=`, NOT EQUALITY**: the JavaScript inventory's `MAX_LOGIC` is two-way because it is a
/// migration budget that may only fall, while test attributes may only legitimately GROW — an upper bound on
/// tests would be a gate that fails on good work. Its weakness is stated rather than hidden: a targeted
/// deletion of one file's suite can hide under a tree-wide floor that other commits raise, which is why the
/// manifest also records each named file's own attribute count and refuses a stale one.
///
/// MUTATION: raise this constant by one above the measured count — the same failure a deleted `mod tests`
///           produces, one attribute at a time.
/// RESULT:   exit 101 —
///             test attributes across the tracked Rust tree: 1999, and the floor is 2000 — this gate measures
///             lines per top-level item, and a file's own test module is the largest thing a deletion can
///             remove from it.
///             FIX: restore the tests, or if the suite really did shrink, lower MIN_TEST_ATTRIBUTES in a commit
///             that names the file whose tests left — a floor lowered in silence is a floor that no longer
///             means anything.
///           Restoring the tests returns it to green.
const MIN_TEST_ATTRIBUTES: usize = 1999;

#[test]
fn the_test_attribute_floor_holds() {
    let files = common::git_ls_files_all();
    let rust: Vec<&String> = files.iter().filter(|f| f.ends_with(".rs")).collect();
    assert!(
        rust.len() >= MIN_RUST_FILES,
        "the scan saw only {} Rust files — this proves nothing",
        rust.len()
    );
    let attributes: usize = rust
        .iter()
        .map(|f| {
            common::read(f)
                .lines()
                .filter(|l| is_test_attribute(l))
                .count()
        })
        .sum();
    println!(
        "rust-coarse-inventory: {attributes} `#[test]`/`#[tokio::test]` attributes across {} tracked Rust files \
         (floor {MIN_TEST_ATTRIBUTES})",
        rust.len()
    );
    assert!(
        attributes >= MIN_TEST_ATTRIBUTES,
        "test attributes across the tracked Rust tree: {attributes}, and the floor is {MIN_TEST_ATTRIBUTES} — \
         this gate measures lines per top-level item, and a file's own test module is the largest thing a \
         deletion can remove from it.\n\
         FIX: restore the tests, or if the suite really did shrink, lower MIN_TEST_ATTRIBUTES in a commit that \
         names the file whose tests left — a floor lowered in silence is a floor that no longer means anything."
    );
}

/// **THE PATTERN'S OWN PROOF, AND IT IS THE GUARD AGAINST THE DRIFT THE HEADER WARNS ABOUT.** Each case below
/// is a line the measured pattern accepts or refuses, and every ratio in the manifest moves if one of them
/// flips. It is deliberately NOT a test of "a nicer pattern": `pub(crate) fn` and `use` are refused here
/// because they are refused by `docs/agents/ideas.md`'s regex, and changing that is a re-measurement of the
/// whole tree rather than an improvement to this file.
#[test]
fn the_item_pattern_is_the_one_that_was_measured() {
    for line in [
        "fn helper() {}",
        "pub fn helper() {}",
        "async fn handler() {}",
        "pub async fn handler() {}",
        "struct S {",
        "pub struct S {",
        "enum E {",
        "impl S {",
        "impl<'a> S<'a> {",
        "const N: usize = 1;",
        "static S: usize = 1;",
        "type T = u8;",
        "trait Tr {",
        "mod tests {",
    ] {
        assert!(is_top_level_item(line), "the pattern counts `{line}`");
    }
    for line in [
        "// fn in a comment",
        "    fn indented_is_not_top_level() {}",
        "use std::path::Path;",
        "pub(crate) fn helper() {}",
        "pub(super) fn helper() {}",
        "pub(crate) async fn handler() {}",
        "unsafe fn helper() {}",
        "async pub fn not_the_patterns_order() {}",
        "let fn_like = 1;",
    ] {
        assert!(
            !is_top_level_item(line),
            "the pattern does not count `{line}`"
        );
    }
    // NO `\b` IN THE MEASURED PATTERN, and this is the quirk that is pinned rather than fixed: `models`
    // matches `mod`, `static_assert` matches `static`. It changes no count on today's tree, and a gate that
    // "improved" it would silently move every recorded number.
    assert!(is_top_level_item("models = 3;"));
    assert!(is_top_level_item("statics_of(x);"));
    assert!(!is_top_level_item("functional() {}"));

    // THE CEILING IS STRICTLY GREATER, and the edge is the whole rule: 120.0 exactly is inside it.
    // `items` one-line items, padded with blank lines to exactly `lines` lines.
    let file = |items: usize, lines: usize| -> String {
        let mut text = String::new();
        for i in 0..items {
            text.push_str(&format!("fn f{i}() {{}}\n"));
        }
        while text.lines().count() < lines {
            text.push('\n');
        }
        text
    };
    let at_the_line = Shape::measure(&file(6, 6 * 120));
    assert_eq!(at_the_line.items, 6);
    assert_eq!(at_the_line.lines, 720);
    assert_eq!(at_the_line.ratio(), 120.0);
    assert!(
        !at_the_line.is_over(),
        "120.0 lines per item exactly is INSIDE the ceiling — the rule is `more than`"
    );
    // One line more, and the same shape is over.
    let one_more = Shape::measure(&file(6, 6 * 120 + 1));
    assert_eq!(one_more.lines, 721);
    assert!(one_more.is_over());
    // ...and an item-less file is over rather than a division by zero.
    let no_items = Shape::measure("\n\n\n");
    assert_eq!(no_items.items, 0);
    assert!(no_items.ratio().is_infinite());
    assert!(no_items.is_over());
}
