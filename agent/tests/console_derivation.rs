//! THE CONSOLE'S SIDE OF "ONE FACT, ONE DERIVATION".
//!
//! WHY THIS EXISTS (round 173 of the standing goal). The PANEL has had `one-derivation-check` since round 78: it
//! fails, by file and line, when a module other than `lib/path.ts` turns a command's ending into a state. The
//! CONSOLE — the other front end, with its own state vocabulary (`dot`, `sig-dot`, `prov-lane`) — had nothing of
//! the kind, and round 172 found what that costs: the rule "a provider prefix's trailing slash does not count" was
//! written out NINE times in `Models.tsx` and `lane.ts`, with two different regexes, so several of the nine already
//! disagreed about what a prefix IS.
//!
//! THE RULES, each with the round that paid for it:
//!
//!   1. `barePrefix` (the trailing-slash strip) is written ONCE, in `lib/lane.ts` (round 172).
//!   2. The update VERDICT is computed in ONE place, `views/DevicesPanel.tsx` (round 175). The device answers the
//!      question at `/api/update`, the console reads that answer, and its own comparison survives only as the
//!      guarded fallback that `device-verdict`'s structural clause holds in place (round 134). Mentions of a
//!      version are fine — `client.ts` declares the type, `Overview` displays it, and both were read before this
//!      rule was written; a COMPARISON is the verdict, and a second one is a second verdict.
//!
//! WHAT IT DOES NOT CHECK: the panel (its own gate), the Rust, or the shape of the console's marks
//! (`console-marks-check`).
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/console-derivation-check.mjs` → exit 0, stdout 162 bytes, one line:
//!     "console-derivation: 26 console module(s) scanned — the prefix rule lives only in lib/lane.ts, and the
//!     update verdict is compared only in views/DevicesPanel.tsx", newline-terminated; stderr 0 bytes.
//!   * this file → the SAME line, byte for byte, from the same `git ls-files` list. `cmp`: identical, 162/162.
//!     **THE LINE NAMES A DIFFERENT HOME SINCE 2026-09-29** — the prefix rule is in the wasm crate now, so
//!     the sentence says where it went and the .mjs it was compared against is gone. The equivalence above
//!     is the record of the port, not a claim about today's string.
//!   * a planted second trailing-slash strip in `lib/format.ts` AND a second version comparison in
//!     `views/Overview.tsx` → BOTH exit 1 with the FULL body, byte for byte: 479/479 bytes. The body carries
//!     both `file:line` offenders in `git ls-files` order and the whole two-line fix, not the first line.
//!   * THE DIFFERENTIAL PROBE — the strongest evidence here, because it tests the two regexes rather than the
//!     tree: a temporary tracked `lib/zz-probe.ts` of 37 adversarial lines (both rule-1 spellings and their
//!     near-misses; `!==`/`===`/`<`/`>` on both sides; `install?` with and without the `?`; zero, one and tab
//!     whitespace; `==`, `!=`, `=`, `??` and a mention-only line; a TRAILING comment quoting both patterns and
//!     a whole-line one) produced 23 offenders from BOTH implementations, and `cmp` on the two failure bodies
//!     was identical at 3654/3654 bytes. Both missed the same 14 lines, including `a !== b.lastVersion` and
//!     `lastVersion?? !== 3` — stated limits, agreed on rather than papered over.
//!   * THE CASE THAT MUST NOT BITE, in both, and it is THIS gate's own case rather than a generic one: the two
//!     homes are exempt BY NAME, and both of them really do carry the pattern — `lib/lane.ts:24` strips a
//!     trailing slash and `views/DevicesPanel.tsx:448` compares `d.lastVersion !== install.version` — so the
//!     clean-tree pass is a measured exemption, not an empty scan. And a line whose pattern sits in a COMMENT
//!     must not bite either: a planted `const KEEP = 1; // …replace(/\/+$/, "")… a !== b.lastVersion` left BOTH
//!     implementations green at 162/162, which is `decomment` doing its job on both sides (rounds 135-137).
//!
//! MUTATION: write the trailing-slash strip a second time, outside `lib/lane.ts` (planted in `lib/format.ts`).
//! RESULT:   fails, naming `lib/format.ts:<line> strips a trailing slash by hand — the bare provider prefix
//!           (round 172)` and printing the whole fix: "The console's derivations live in lib/, in ONE place each.
//!           Round 172 found this rule written nine times with two different behaviours; a tenth copy is how they
//!           drift again."
//!
//! THE FIX THIS PRINTS: the offending `file:line` and the reason that line is a second derivation — the JS's own
//! message, kept byte for byte, which is why this port adds no sentence of its own to the failure body.
//!
//! WHAT IT DOES NOT SEE, stated rather than implied:
//!   * a block comment that SPANS LINES is removed whole, newlines included, so every line number below it
//!     shifts. The JS has the same behaviour and this port did not widen or narrow it; the two agree because
//!     they share the strip's semantics, not because either is right about the line number.
//!   * the two patterns are the two spellings the console actually used. A third spelling of "strip the trailing
//!     slash" (a `slice(0, -1)`, a `substring`, a regex built at runtime) is invisible to both implementations.
//!   * a comparison written across TWO lines — the regex is per-line, so `d.lastVersion\n  !== install.version`
//!     is not seen by either.

mod common;

const UI: &str = "gateway/ui/src";

/// The files that own a derivation. Everything else must ask them.
///
/// **EMPTY SINCE 2026-09-29, AND THE EMPTINESS IS THE POINT.** The one home it named — `lib/lane.ts`,
/// which stripped a provider prefix's trailing slash — is `gateway/ui-logic/src/lib.rs` now (block ③ of
/// the migration wired it), i.e. OUTSIDE the tree this scan walks. Nothing under `gateway/ui/src` may
/// strip a slash by hand, so there is nothing left to exempt, and the non-vacuity probe below follows
/// the rule to its new home instead of asserting against a file that no longer carries it.
const HOMES: [(&str, &str); 0] = [];

/// WHERE THE PREFIX RULE LIVES, in the message a reader gets when they write a second copy of it — one
/// string, so the sentence and the home cannot drift apart.
const PREFIX_RULE: &str =
    "the bare provider prefix (round 172) — `gateway/ui-logic/src/lib.rs` since 2026-09-29";

/// The rule-2 home, exempt inline in the JS rather than through `HOMES`, and mirrored that way so a reader
/// comparing the two finds the exemption in the same shape.
const VERDICT_HOME: &str = "views/DevicesPanel.tsx";

/// A floor, not a claim: the JS refuses to pass when it has read almost nothing. Below this the scan proves
/// nothing, which is the failure mode this repository has recorded seven times.
const MIN_SCANNED: usize = 15;

/// THE TWO STREAMS, SEPARATED, because the JS separates them. `stderr` carries NO trailing newline: `panic!`
/// supplies it and `console.error` supplies it on the other side, so the two bodies are byte-equal once the
/// panic framing is stripped.
struct Streams {
    stdout: String,
    stderr: String,
    failed: bool,
}

struct Scan {
    scanned: usize,
    offenders: Vec<String>,
}

/// `/replace\(\/\\\/\+\$|replace\(\/\\\/\$/.test(line)`.
///
/// Both alternatives are LITERAL — every metacharacter in them is escaped — so this is two substring searches
/// and not a regex engine. That is a claim, and it was measured rather than assumed: over the 5,824 lines of
/// the 26 modules `git ls-files` returns, the regex and these two `contains` calls disagree on ZERO lines.
fn strips_a_trailing_slash(line: &str) -> bool {
    line.contains("replace(/\\/+$") || line.contains("replace(/\\/$")
}

/// `(lastVersion|install\??\.version|update_available)\s*(!==|===|<|>)|(!==|===|<|>)\s*(lastVersion|install\??\.version|update_available)`
///
/// Hand-rolled because this crate has no regex dependency and adding one to a GATE would be a new dependency
/// for a twelve-character grammar. It is a search, not an anchored match: the JS's `.test` finds the pattern
/// anywhere in the line, so every start position is tried and the LEFT alternative is tried before the right
/// one at each — which is the order the JS's `|` gives.
///
/// MEASURED AGAINST THE JS RATHER THAN READ OFF IT: the 37-line differential probe described in the module
/// header produced the same 23 offenders from both implementations, misses included.
fn compares_a_version(line: &str) -> bool {
    let c = common::chars(line);
    (0..c.len()).any(|i| {
        version_side(&c, i)
            .map(|j| common::skip_ws(&c, j))
            .is_some_and(|j| comparison_side(&c, j).is_some())
            || comparison_side(&c, i)
                .map(|j| common::skip_ws(&c, j))
                .is_some_and(|j| version_side(&c, j).is_some())
    })
}

/// `(lastVersion|install\??\.version|update_available)` — returns the index just past the match.
///
/// The three alternatives are mutually exclusive by their first character (`l`, `i`, `u`), so the JS's
/// left-to-right order cannot change the outcome here; they are written in the JS's order anyway.
fn version_side(c: &[char], i: usize) -> Option<usize> {
    for alt in ["lastVersion", "update_available"] {
        if common::find_seq(c, alt, i) == Some(i) {
            return Some(i + alt.chars().count());
        }
    }
    // `install\??\.version` — `\??` is GREEDY, so the `?` is taken when it is there and skipped when it is
    // not. A `?` that is present but not followed by `.version` cannot fall back to the shorter form, because
    // the shorter form needs `\.` where the `?` sits.
    if common::find_seq(c, "install", i) == Some(i) {
        let mut j = i + "install".len();
        if c.get(j) == Some(&'?') {
            j += 1;
        }
        if common::find_seq(c, ".version", j) == Some(j) {
            return Some(j + ".version".len());
        }
    }
    None
}

/// `(!==|===|<|>)` — returns the index just past the match.
fn comparison_side(c: &[char], i: usize) -> Option<usize> {
    for op in ["!==", "==="] {
        if common::find_seq(c, op, i) == Some(i) {
            return Some(i + op.len());
        }
    }
    match c.get(i) {
        Some('<') | Some('>') => Some(i + 1),
        _ => None,
    }
}

/// `relative(UI, rel)` — every path `git ls-files gateway/ui/src` returns is under `UI`, so this is the strip
/// `path.relative` performs, without resolving either side against a working directory.
fn short_of(rel: &str) -> String {
    rel.strip_prefix(&format!("{UI}/"))
        .unwrap_or(rel)
        .to_string()
}

fn scan() -> Scan {
    let files: Vec<String> = common::git_ls_files(UI)
        .into_iter()
        .filter(|f| f.ends_with(".ts") || f.ends_with(".tsx"))
        .filter(|f| !f.contains(".test."))
        .collect();

    let mut offenders = Vec::new();
    let mut scanned = 0usize;
    for rel in &files {
        let short = short_of(rel);
        scanned += 1;
        // A HOME IS SKIPPED ENTIRELY — both rules — which is what the JS's `continue` does.
        if HOMES.iter().any(|(h, _)| *h == short) {
            continue;
        }
        let text = common::decomment(&common::read(rel));
        for (i, line) in text.split('\n').enumerate() {
            if strips_a_trailing_slash(line) {
                offenders.push(format!(
                    "{short}:{} strips a trailing slash by hand — {}",
                    i + 1,
                    PREFIX_RULE
                ));
            }
            if compares_a_version(line) && short != VERDICT_HOME {
                offenders.push(format!(
                    "{short}:{} compares a version outside {VERDICT_HOME} — the device ANSWERS this question, \
                     and a second comparison is a second verdict (round 175)",
                    i + 1
                ));
            }
        }
    }
    Scan { scanned, offenders }
}

fn report(s: &Scan) -> Streams {
    if s.scanned < MIN_SCANNED {
        return Streams {
            stdout: String::new(),
            stderr: format!(
                "FAIL scanned only {} console module(s) — the tree moved, so this proves nothing",
                s.scanned
            ),
            failed: true,
        };
    }
    if !s.offenders.is_empty() {
        return Streams {
            stdout: String::new(),
            stderr: format!(
                "console-derivation: {} second derivation(s):\n  {}\n\n\
                 The console's derivations live in lib/, in ONE place each. Round 172 found this rule written \
                 nine times with two\ndifferent behaviours; a tenth copy is how they drift again.",
                s.offenders.len(),
                s.offenders.join("\n  ")
            ),
            failed: true,
        };
    }
    Streams {
        stdout: format!(
            "console-derivation: {} console module(s) scanned — the prefix rule lives only in \
             gateway/ui-logic/src/lib.rs, and the update verdict is compared only in \
             views/DevicesPanel.tsx\n",
            s.scanned
        ),
        stderr: String::new(),
        failed: false,
    }
}

#[test]
fn the_consoles_derivations_live_in_one_place_each() {
    let out = report(&scan());
    print!("{}", out.stdout);
    if out.failed {
        panic!("{}", out.stderr);
    }
}

// ── the unit cases: this gate's own edges, each measured rather than assumed ──────────────────────────

/// THE CASE THAT MUST NOT BITE, and it is this gate's own: the rules are LOAD-BEARING, not decorative.
/// A green run must be a measured scan rather than an empty one, which is the difference between this gate
/// working and this gate passing because nothing matched.
///
/// **THE PROBE FOLLOWS THE RULE (2026-09-29).** It used to assert that `lib/lane.ts` still stripped a
/// trailing slash, which made the exemption non-vacuous. The strip is in `gateway/ui-logic/src/lib.rs`
/// now, so THAT is what has to carry it — and `lib/lane.ts` has to be the thin wrapper, because a
/// `lane.ts` that had grown its own copy back is exactly the tenth derivation this gate exists to catch.
#[test]
fn both_homes_really_do_carry_the_pattern_they_are_exempt_from() {
    // THE RUST SPELLING IS NOT THE JS ONE, and the predicate above is deliberately JS-only: it is what
    // the SCAN matches, and the scan walks TypeScript. The strip's Rust form is `trim_end_matches('/')`,
    // which is what this asserts — the rule, in the language it moved to.
    let rust = common::decomment(&common::read("gateway/ui-logic/src/lib.rs"));
    assert!(
        rust.contains("trim_end_matches('/')"),
        "gateway/ui-logic/src/lib.rs must be where the trailing-slash strip lives (trim_end_matches('/')) \
         — if it is not, this gate is green for the wrong reason"
    );
    let lane = common::decomment(&common::read("gateway/ui/src/lib/lane.ts"));
    assert!(
        !lane.split('\n').any(strips_a_trailing_slash),
        "lib/lane.ts must be the WRAPPER now — a strip here is a second derivation"
    );
    let panel = common::decomment(&common::read("gateway/ui/src/views/DevicesPanel.tsx"));
    assert!(
        panel.split('\n').any(compares_a_version),
        "views/DevicesPanel.tsx must still compare the version — it is the ONE place allowed to"
    );
}

/// A COMMENT IS NOT A DERIVATION (rounds 135-137). A line documenting the rule must not be read as a copy of
/// it, and this is the direction that matters here: the gate flagging its own documentation.
#[test]
fn a_derivation_quoted_in_a_comment_does_not_bite() {
    let doc =
        "// the strip is prefix.replace(/\\/+$/, \"\") and the verdict is a !== b.lastVersion\n";
    assert!(strips_a_trailing_slash(doc), "the raw line does match");
    assert!(
        !common::decomment(doc)
            .split('\n')
            .any(strips_a_trailing_slash),
        "...and after the strip it does not, which is why `decomment` runs before the scan"
    );
}

/// The two spellings, and the two sides of the version comparison, each on its own.
#[test]
fn the_two_spellings_and_the_two_sides_are_each_recognised() {
    assert!(strips_a_trailing_slash("p.replace(/\\/+$/, \"\")"));
    assert!(strips_a_trailing_slash("p.replace(/\\/$/, \"\")"));
    assert!(!strips_a_trailing_slash("p.replace(/\\+$/, \"\")"));

    for line in [
        "d.lastVersion !== install.version",
        "install?.version === d.lastVersion",
        "update_available < 3",
        "3 > update_available",
        "!!install.version && install.version !== lastVersion",
    ] {
        assert!(compares_a_version(line), "{line} must be a comparison");
    }
    // A MENTION is not a comparison: the type declaration, the display, and the presence test.
    for line in [
        "lastVersion: string;",
        "install?.version ?? \"—\"",
        "!!d.lastVersion && !!install?.version",
    ] {
        assert!(
            !compares_a_version(line),
            "{line} is a mention, not a comparison"
        );
    }
    // ...and the whitespace between the two sides is `\s*`, so none is allowed too.
    assert!(compares_a_version("lastVersion!==install.version"));
}

/// The failure BODY, not the verdict: the count, the `file:line`, the reason, and the two-line fix.
#[test]
fn the_failure_body_names_the_line_and_the_one_place_it_belongs() {
    let s = Scan {
        scanned: 26,
        offenders: vec![
            "lib/format.ts:12 strips a trailing slash by hand — the bare provider prefix (round 172)"
                .to_string(),
        ],
    };
    let out = report(&s);
    assert!(out.failed);
    assert_eq!(
        out.stderr,
        "console-derivation: 1 second derivation(s):\n  \
         lib/format.ts:12 strips a trailing slash by hand — the bare provider prefix (round 172)\n\n\
         The console's derivations live in lib/, in ONE place each. Round 172 found this rule written nine \
         times with two\ndifferent behaviours; a tenth copy is how they drift again."
    );
    assert!(out.stdout.is_empty(), "the failure body is stderr only");
}

/// A scan that reads almost nothing refuses rather than passing — the floor, with its own message.
#[test]
fn a_scan_that_reads_almost_nothing_refuses_rather_than_passing() {
    let out = report(&Scan {
        scanned: 3,
        offenders: Vec::new(),
    });
    assert!(out.failed);
    assert_eq!(
        out.stderr,
        "FAIL scanned only 3 console module(s) — the tree moved, so this proves nothing"
    );
}
