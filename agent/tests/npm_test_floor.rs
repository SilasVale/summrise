//! **A TEST STEP THAT RAN ZERO TESTS IS NOT A PASSING TEST STEP — THE DECISION, MOVED OUT OF
//! `scripts/test/npm-test-floored.mjs`.**
//!
//! ── THE MUTATION THAT MUST FAIL THIS GATE ───────────────────────────────────────────────────────
//! Read this when you change this file: the mutation is how you find out whether the check can still fail at
//! all. A gate that cannot be broken is worse than no gate.
//!
//! MUTATION: in `count_of`, return `Some(0)` for the `"Tests  N passed"` shape instead of the parsed number —
//!           that is, stop reading vitest's count.
//! RESULT:   `the_vitest_reporter_is_read` fails naming the count it got, and the corpus's floor assertion
//!           fails too. (Measured on the port: the vitest row is what catches it.)
//!
//! AND THE MUTATION THE ORIGINAL COULD NOT SURVIVE: delete the `tests` fallback and the `ℹ tests 0` case is
//! still refused, but delete the `counted < 1` refusal and `a_suite_that_ran_nothing_is_refused` fails —
//! **the whole reason this gate exists**, because `node --test` prints `ℹ pass 0` and EXITS 0.
//!
//! WHY IT MOVED, AND WHAT THE ORIGINAL ADMITTED. `npm-test-floored.mjs` spawns `npm test` — and **`npm` is a
//! tool every checkout with a package has, not a dependency**, which is the same reading that let
//! `main-shape-check` come across (it spawns `git`). But the reason to move the DECISION first is the
//! sentence the original carries about itself:
//!
//! > This file was tested on `node --test` in two packages and **never on the vitest reporter it claims to
//! > read**.
//!
//! **A GATE THAT ADMITS ONE OF ITS THREE CASES WAS NEVER EXERCISED IS A GATE WITH A HOLE IN IT**, and the
//! hole is exactly what a corpus fixes. The vitest line in this file's corpus is not invented: it is
//! `      Tests  935 passed (935)`, captured from `npm test` in `agent/resources/panel-react` on 2026-10-03,
//! six leading spaces and two spaces after `Tests` included. The colourised variant is the one round 203
//! recorded, because vitest DOES colourise in some environments and the strip has to survive it.
//!
//! THE SPAWN STAYS IN THE `.mjs` FOR NOW. This is the decision only — the reporter parsing and the two
//! refusals — which is the same split `contrast-probe-check` used when thirteen of its assertions moved and
//! five stayed. The retirement is its own step, with its own proof.

/// `\u001b\[[0-9;]*m` — vitest colourises its summary, and an anchored match then never fires.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '\u{1b}' && i + 1 < bytes.len() && bytes[i + 1] == '[' {
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == ';') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == 'm' {
                i = j + 1;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// The digits at the start of `line` after `prefix`, or `None`. The three shapes are matched by hand rather
/// than with a regex crate: this crate is SHIPPED, and three anchored patterns do not justify a dependency.
fn digits_after(line: &str, prefix: &str) -> Option<u64> {
    let rest = line.strip_prefix(prefix)?;
    // **`\s+`, NOT A SINGLE SPACE, AND THE REAL CAPTURE IS WHAT SAID SO.** vitest prints
    // `      Tests  935 passed (935)` — TWO spaces after `Tests` — so the first version of this returned
    // `None` for the one reporter the original admitted it had never tested. The corpus row is a real
    // capture rather than a plausible-looking line, which is exactly why it caught this.
    let rest = rest.trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// **THE DECISION, PURE.** `None` means the output carried no test count at all, which is its own refusal.
///
/// The precedence is the original's: `pass` first, then vitest's `Tests N passed`, then `tests` — and it is
/// load-bearing, because `node --test` prints BOTH `ℹ tests N` and `ℹ pass N` and only one of them is the
/// count of tests that ran.
pub fn count_of(raw: &str) -> Option<u64> {
    let clean = strip_ansi(raw);
    let mut pass = None;
    let mut vitest = None;
    let mut tests = None;
    for line in clean.lines() {
        // `ℹ pass N` (node --test, Node 24) and `# pass N` (Node 20) — the same line shape, two markers.
        let marked = line.strip_prefix('ℹ').or_else(|| line.strip_prefix('#'));
        if let Some(rest) = marked {
            let rest = rest.trim_start();
            if pass.is_none() {
                pass = digits_after(rest, "pass ");
                if pass.is_some() {
                    continue;
                }
            }
            if tests.is_none() {
                tests = digits_after(rest, "tests ");
            }
            continue;
        }
        // `      Tests  935 passed (935)` — vitest, six leading spaces in the captured line.
        if vitest.is_none() {
            let rest = line.trim_start();
            if let Some(n) = digits_after(rest, "Tests ") {
                // The word `passed` has to follow, or it is not this reporter's summary line.
                if rest.contains(" passed") {
                    vitest = Some(n);
                }
            }
        }
    }
    pass.or(vitest).or(tests)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE CORPUS, AND WHERE EACH ROW CAME FROM.** The vitest rows are the ones the original admitted it
    /// had never been tested on; the first is a real capture rather than a plausible-looking line.
    const CORPUS: [(&str, &str, Option<u64>); 9] = [
        (
            "node --test, Node 24 (`ℹ pass N`)",
            "ℹ tests 12\nℹ pass 12\nℹ fail 0\n",
            Some(12),
        ),
        (
            "node --test, Node 20 (`# pass N`)",
            "# tests 12\n# pass 12\n# fail 0\n",
            Some(12),
        ),
        (
            "vitest, CAPTURED 2026-10-03 from agent/resources/panel-react",
            " Test Files  116 passed (116)\n      Tests  935 passed (935)\n",
            Some(935),
        ),
        (
            "vitest, colourised — the shape round 203 recorded",
            "\u{1b}[2m Tests \u{1b}[22m \u{1b}[1m \u{1b}[32m832 passed\u{1b}[39m\n",
            Some(832),
        ),
        (
            "a suite that ran nothing, and EXITED 0 — the reason this gate exists",
            "ℹ tests 0\nℹ pass 0\nℹ fail 0\n",
            Some(0),
        ),
        ("no count at all", "all good!\n", None),
        (
            "`pass` wins over `tests` when both are present",
            "ℹ tests 99\nℹ pass 7\n",
            Some(7),
        ),
        (
            "vitest's own summary, with the file count above it",
            " Test Files  1 passed (1)\n      Tests  1 passed (1)\n",
            Some(1),
        ),
        (
            "MUST NOT BITE: a large real-shaped node --test run",
            "ℹ tests 935\nℹ suites 116\nℹ pass 935\nℹ fail 0\n",
            Some(935),
        ),
    ];

    #[test]
    fn the_corpus_holds() {
        for (what, output, want) in CORPUS {
            assert_eq!(count_of(output), want, "{what}");
        }
    }

    #[test]
    fn the_vitest_reporter_is_read() {
        // The row the original's own comment says was never exercised. Stated separately so a failure names
        // the reporter rather than a corpus index.
        assert_eq!(
            count_of(" Test Files  116 passed (116)\n      Tests  935 passed (935)\n"),
            Some(935),
            "vitest's `Tests  N passed` line"
        );
        assert_eq!(
            count_of("\u{1b}[2m Tests \u{1b}[22m \u{1b}[1m \u{1b}[32m832 passed\u{1b}[39m\n"),
            Some(832),
            "vitest, colourised"
        );
    }

    #[test]
    fn a_suite_that_ran_nothing_is_refused() {
        // `node --test` prints this and EXITS 0. The gate's whole subject.
        assert_eq!(count_of("ℹ tests 0\nℹ pass 0\n"), Some(0));
        // And the refusal is `< 1`, not `== 0`, so a count that parsed to nothing is refused too.
        assert!(count_of("all good!\n").is_none());
    }

    /// **THE TWO COPIES CANNOT DRIFT SILENTLY.** The `.mjs` keeps the spawn and its own copy of these three
    /// shapes until the retirement; this reads that file and fails if any of them moved. It is the same
    /// cross-check `tests/two_relays_agree.rs` uses, for the same reason: **a claim nobody checks is a claim
    /// that drifts**, and here the claim is that two implementations of one decision still agree.
    #[test]
    fn the_javascript_still_carries_the_same_shapes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the repository root")
            .join("scripts/test/npm-test-floored.mjs");
        let js =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        for (what, fragment) in [
            // the ANSI strip, which round 203 is why exists
            ("the ANSI strip", r#"replace(/\u001b\[[0-9;]*m/g, "")"#),
            // the three reporters
            ("node --test's `tests`", r"match(/^[ℹ#]\s*tests\s+(\d+)/m)"),
            ("node --test's `pass`", r"match(/^[ℹ#]\s*pass\s+(\d+)/m)"),
            ("vitest's summary", r"match(/^\s*Tests\s+(\d+)\s+passed/m)"),
            // the precedence, which is load-bearing: node --test prints both `tests` and `pass`
            ("the precedence", "pass ? Number(pass[1]) : vitest ? Number(vitest[1]) : tests ? Number(tests[1]) : null"),
            // the two refusals
            ("the no-count refusal", "counted === null"),
            ("the zero refusal", "counted < 1"),
            // and the thing this file does NOT take over yet
            ("the spawn", r#"execFileSync("npm", ["test"]"#),
        ] {
            assert!(
                js.contains(fragment),
                "scripts/test/npm-test-floored.mjs no longer carries {what} — it is {fragment:?}, and the \
                 Rust port's corpus is written against it"
            );
        }
    }

    #[test]
    fn the_ansi_strip_does_not_eat_ordinary_text() {
        // A matcher that stripped too much would pass the colourised row and corrupt every other one.
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[39m"), "red");
        assert_eq!(strip_ansi("a\u{1b}[0mb"), "ab");
        // An ESCAPE THAT IS NOT A COLOUR CODE STAYS, because the original's pattern is `[0-9;]*m` only.
        assert_eq!(strip_ansi("a\u{1b}[2Jb"), "a\u{1b}[2Jb");
    }
}
