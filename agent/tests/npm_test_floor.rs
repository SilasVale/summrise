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
//! ── THE SPAWN MOVED HERE TOO, AND THE `.mjs` IS DELETED ─────────────────────────────────────────
//!
//! `scripts/test/npm-test-floored.mjs` is gone: `count_of` above is its decision, and
//! `every_package_that_declares_a_test_script_reports_a_count` below is its spawn. The two were run on one
//! tree until they agreed, which is how every other `.mjs` in `scripts/test/` was retired.
//!
//! **THE SUBJECT IS DERIVED, NOT LISTED.** The `.mjs` was handed a working directory by the CI step that
//! invoked it; a Rust test has no such caller, so it asks the repository which packages declare a `test`
//! script (`git ls-files`, so a `node_modules` copy cannot answer) and floors EVERY one of them. That is
//! strictly wider than the six steps the `.mjs` replaced — it found a SEVENTH, `agent/summrise-workspace-dsh`,
//! whose suite nothing in CI runs — and it cannot go stale the way a list of directories would.
//!
//! **AND THE `n/a` BRANCH IS DELETED RATHER THAN PORTED, BECAUSE IT HAS NO CALLER.** The `.mjs` exited 2
//! with "this host cannot run me" when its cwd held no `package.json`: `all-gates.bash` runs every
//! `scripts/test/*` file from the REPOSITORY ROOT, where that is true. Nothing runs this file from a
//! directory it did not choose — it resolves each package from the repository root — so a branch for "I do
//! not know where I am" would be a branch nothing reaches. What replaced it is LOUDER: a package whose
//! suite cannot run at all fails this test with the command that fixes it, rather than declaring itself
//! inapplicable.

mod common;

use common::{git_ls_files_all, repo, Spawn};

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

/// **THE PACKAGES THIS FLOOR COVERS, AND THE RUNNERS IT DOES NOT NEED TO.** Every TRACKED `package.json`
/// whose `scripts.test` is a non-empty string — `git ls-files` rather than a walk, so a `node_modules` copy
/// of somebody else's manifest cannot answer, and the list cannot go stale.
///
/// **THE RUNNER DECIDES WHETHER THE FLOOR IS NEEDED, AND THAT IS MEASURED RATHER THAN ASSUMED.**
/// `node --test` prints `ℹ pass 0` and **EXITS 0** — the vacuity this gate exists for. `vitest` does not:
/// with no test files it prints `No test files found, exiting with code 1` and exits **1** (MEASURED in
/// `agent/resources/panel-react`, 2026-10-10), so its own runner is already the floor, and running a
/// 935-test vitest suite inside `cargo test` would buy nothing and cost a fork pool — MEASURED once
/// already: under a loaded box the panel's suite failed with `[vitest-pool]: Failed to start forks
/// worker`, which is a red this gate would have reported for a reason that is not the suite's.
///
/// AN UNKNOWN RUNNER IS A FAILURE, NOT A SKIP: this gate cannot claim a runner refuses an empty suite
/// without having measured that it does, so a package whose `test` script is something else must be
/// brought here and classified deliberately.
fn classify(script: &str) -> Runner {
    if script.starts_with("node --test") {
        Runner::NodeTest
    } else if script.starts_with("vitest") {
        Runner::Vitest
    } else {
        Runner::Unknown
    }
}

enum Runner {
    /// Prints a count and exits 0 even when it ran nothing — THE SUBJECT OF THIS GATE.
    NodeTest,
    /// Refuses an empty suite itself (measured), so it needs no floor from here.
    Vitest,
    /// Never seen. Named in the failure so the next reader classifies it rather than guessing.
    Unknown,
}

fn packages_with_a_test_script() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for f in git_ls_files_all() {
        if !f.ends_with("package.json") {
            continue;
        }
        let raw = match std::fs::read_to_string(repo().join(&f)) {
            Ok(t) => t,
            // An unreadable manifest is skipped rather than fatal: this gate's subject is the suites, and a
            // file that cannot be read is `js_boundary_inventory`'s business, not this one's.
            Err(_) => continue,
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        if let Some(t) = v["scripts"]["test"]
            .as_str()
            .filter(|t| !t.trim().is_empty())
        {
            out.push((
                f.trim_end_matches("/package.json").to_string(),
                t.to_string(),
            ));
        }
    }
    out.sort();
    out
}

/// **THE FLOOR, OVER EVERY PACKAGE THAT DECLARES A SUITE.** Each one is run and its own count is read; the
/// failures are COLLECTED rather than returned on the first, so one red does not hide the other six.
///
/// THREE WAYS TO FAIL, and they are the original's three: the suite exited non-zero (its own failure, passed
/// through exactly as `execFileSync` passed it through); the suite exited 0 with NO COUNT in its output (the
/// vacuity the `.mjs` was written for); or the count was below 1 (`node --test` prints `ℹ pass 0` and EXITS
/// 0). The message names the package, the count, the rc and the command that fixes a suite that could not run.
///
/// MUTATION: a package's `test` script -> `node --test "test/**/*.nomatch"` — the glob stops matching, which
///           is the rename/never-matched case this floor was written for. MEASURED on `index/package.json`:
///           `npm test` prints `ℹ pass 0` and EXITS 0, and this test fails with
///           `index: the suite reported 0 test(s) and exited 0. A suite that tests nothing passes
///           everything — check that its test files still exist and that the runner still matches them.`
///
/// MUTATION: a package's `test` script -> `node --test test/does-not-exist.mjs` -> MEASURED: `npm test` exits
///           1 and this test fails with `index: \`npm test\` exited 1 — the suite failed, which is the
///           suite's own verdict and this floor passes it through.`
///
/// **AND THE OBVIOUS MUTATION IS NOT ONE — MEASURED, WHICH IS WHY IT IS WRITTEN DOWN HERE.** A first draft of
/// this block proposed `node --test --test-name-pattern=zzz-no-such-test` as "a suite that matches nothing".
/// It is not: Node still REPORTS every test it filtered out — `ℹ tests 8 / ℹ pass 8`, rc=0 — so the floor
/// correctly passes it, and a mutation that cannot fail proves nothing. **A FILTER IS NOT AN EMPTY SUITE**;
/// the count comes from the FILES the runner found, not from the tests it chose to run.
#[test]
fn every_package_that_declares_a_test_script_reports_a_count() {
    let packages = packages_with_a_test_script();
    // A FLOOR ON THE DERIVATION ITSELF: a scan that found no package would pass the loop below while
    // checking nothing, which is the same vacuity one level up.
    assert!(
        packages.len() >= 5,
        "only {} package(s) with a `test` script were found: {:?} — the derivation moved, so this proves \
         nothing",
        packages.len(),
        packages.iter().map(|(d, _)| d).collect::<Vec<_>>()
    );

    let mut failures: Vec<String> = Vec::new();
    let mut floored: Vec<String> = Vec::new();
    let mut self_floored: Vec<String> = Vec::new();
    for (dir, script) in &packages {
        match classify(script) {
            Runner::Vitest => {
                // Its own runner refuses an empty suite (measured), so it needs nothing from here.
                self_floored.push(format!("{dir} (vitest)"));
                continue;
            }
            Runner::Unknown => {
                failures.push(format!(
                    "{dir}: its `test` script is {script:?}, a runner this gate has NOT measured. Either it \
                     refuses an empty suite itself (say so here, with the measurement) or it needs the floor \
                     and belongs in the arm above."
                ));
                continue;
            }
            Runner::NodeTest => {}
        }
        let out = Spawn::new("npm").args(["test"]).cwd(repo().join(dir)).run();
        // **AND THE FAILING TESTS' NAMES, BECAUSE A TAIL OF 12 LINES DOES NOT REACH THEM.** MEASURED 2026-10-10:
        // CI reported `gateway: `npm test` exited 1 … 64 !== 73` and nothing else, so the one thing a reader needs
        // — WHICH test — was 300 lines above the tail. `node --test` prints them as `✖ <name> (<ms>)`, and this is
        // the same move `scripts/test/all-gates.bash` makes for cargo's `panicked at`: **a gate whose output does
        // not name the failure is a gate somebody re-runs hoping.**
        let failing: String = out
            .both()
            .lines()
            .filter(|l| l.trim_start().starts_with('✖'))
            .take(6)
            .collect::<Vec<_>>()
            .join("
");
        let tail: String = out
            .both()
            .lines()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        if !out.ok() {
            failures.push(format!(
                "{dir}: `npm test` exited {} — the suite failed, which is the suite's own verdict and this \
                 floor passes it through.\nFailing tests:\n{failing}\nOutput tail:\n{tail}",
                out.code()
            ));
            continue;
        }
        match count_of(&out.stdout) {
            None => failures.push(format!(
                "{dir}: the suite exited 0 but its output carried NO test count at all, so this proves \
                 nothing. Expected `ℹ pass N` (node --test, Node 24), `# pass N` (Node 20) or `Tests  N \
                 passed` (vitest). Output tail:\n{tail}"
            )),
            Some(0) => failures.push(format!(
                "{dir}: the suite reported 0 test(s) and exited 0. A suite that tests nothing passes \
                 everything — check that its test files still exist and that the runner still matches them. \
                 Output tail:\n{tail}"
            )),
            Some(n) => floored.push(format!("{dir} ({n})")),
        }
    }

    assert!(
        failures.is_empty(),
        "the floor refused {} of {} package(s) that declare a `test` script:\n  {}\n\
         FIX: the runner's own command is in that package's `test` script; a suite whose dependencies are \
         missing needs `npm ci` in its directory (CI's `agent` job installs them).",
        failures.len(),
        packages.len(),
        failures.join("\n  ")
    );
    // AND THE SELF-FLOORED ARM IS EXERCISED: if the vitest package ever leaves, the skip above becomes
    // dead code that a reader would take for coverage.
    assert!(
        !self_floored.is_empty(),
        "no package was classified as self-floored (vitest) — the arm that skips them is now unreachable, \
         so either a package changed runner or the derivation stopped finding it: {packages:?}"
    );
    println!(
        "npm-test-floored: {} package(s) floored here — {}\n                  {} self-floored (their runner \
         refuses an empty suite) — {}",
        floored.len(),
        floored.join(", "),
        self_floored.len(),
        self_floored.join(", ")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE CORPUS, AND WHERE EACH ROW CAME FROM.** The vitest rows are the ones the original admitted it
    /// had never been tested on; the first is a real capture rather than a plausible-looking line.
    const CORPUS: [(&str, &str, Option<u64>); 11] = [
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
            "index — CAPTURED 2026-10-03, a package with ZERO dependencies and no lockfile",
            "\u{2139} tests 45\n\u{2139} pass 45\n\u{2139} fail 0\n",
            Some(45),
        ),
        (
            "summrise-agent-npm — CAPTURED 2026-10-03, the other zero-dependency package",
            "\u{2139} tests 68\n\u{2139} pass 68\n\u{2139} fail 0\n",
            Some(68),
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
