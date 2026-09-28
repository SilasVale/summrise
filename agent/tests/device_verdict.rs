//! THE UPDATE VERDICT CROSSES THREE FILES, AND ALL THREE MUST STILL BE IN THE CHAIN.
//!
//! WHY THIS EXISTS (round 79 of the standing goal). Round 29 fixed a defect this objective exists to
//! remove: the console decided for itself whether a device was behind, by comparing the version somebody
//! last reported against the version the CDN advertises — a second computation of a fact the DEVICE
//! already answers, and it got the answer BACKWARDS for two devices at once (the one that was behind,
//! and the one deliberately pinned). The fix routes the device's own verdict through the wire:
//! `/api/update` on the device → the worker's probe → `/api/plugins/status` → the client's type → the
//! devices view, which prefers it and keeps the old comparison only as the fallback for agents too old
//! to answer.
//!
//! THAT CHAIN IS FOUR LINKS IN THREE FILES, and a single deleted field anywhere in it silently returns
//! the console to computing the fact itself — with every suite still green, because nothing else asserts
//! that the console READS it. This gate is that assertion, and its failure message is the regression's
//! name.
//!
//! WHAT IT CHECKS: the worker still forwards the verdict, the client's type still declares it, and the
//! view still reads it. NOT the fallback's shape (that is the view's business and its own tests) and not
//! the device side (the Rust suite and the wire fixtures own that end).
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28): both print
//! the same four lines, byte for byte — three `ok   <file>: <what>` lines and the closing
//! "device-verdict: the device's own update verdict still crosses all three files". A planted
//! `const verdict = undefined;` in the view moves BOTH to the same failure body. The measurements are in
//! the commit.
//!
//! MUTATION: delete the verdict from any one link — the worker's forwarded field, the client's type, or
//!           the view's read of the probed status.
//! RESULT:   fails, naming the file and the sentence that says what the console would go back to doing.
//!
//! THE ONE PLACE THE TWO DIFFER, stated rather than glossed: the MISSING-FILE branch embeds the
//! runtime's own error text ("ENOENT: no such file or directory, open '<abs path>'" against Rust's
//! `io::Error`), so that line is the same SHAPE and not the same bytes. Every other line is byte-equal,
//! and the missing-file branch is not reachable on a tree where the three files exist.
//!
//! AND ONE THING THE JS CARRIES THAT IS DEAD TEXT: the third link's object literal declares `also`
//! TWICE (the two clauses are character-identical, so the second wins and nothing behaves differently).
//! One clause is ported; the duplicate is not a behaviour and is not reproduced.

mod common;

use std::fs;

const LINKS: [(&str, &str); 3] = [
    (
        "gateway/src/plugins/mcp.ts",
        "the worker FORWARDS the device's verdict into the console's payload",
    ),
    (
        "gateway/ui/src/api/client.ts",
        "the console's client TYPE declares the verdict",
    ),
    (
        "gateway/ui/src/views/DevicesPanel.tsx",
        "the devices view READS the verdict",
    ),
];

struct Streams {
    stdout: String,
    stderr: String,
    failed: bool,
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn find_seq(c: &[char], pat: &str, from: usize) -> Option<usize> {
    let p: Vec<char> = pat.chars().collect();
    let mut j = from;
    while j + p.len() <= c.len() {
        if c[j..j + p.len()] == p[..] {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// JavaScript's `\s`, which is the White_Space property plus U+FEFF.
fn is_ws(ch: char) -> bool {
    ch.is_whitespace() || ch == '\u{feff}'
}

fn skip_ws(c: &[char], mut i: usize) -> usize {
    while i < c.len() && is_ws(c[i]) {
        i += 1;
    }
    i
}

fn is_word(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// `state\.update\s*=\s*\{[\s\S]{0,300}update_available:` — the worker assembles the object field by
/// field (spread-guarded, so an absent value is ABSENT rather than empty); the first version of this
/// pattern looked for a shape the code never had and reported a link that is intact.
fn mcp_forwards(t: &str) -> bool {
    let c = chars(t);
    let mut from = 0;
    while let Some(i) = find_seq(&c, "state.update", from) {
        let mut j = skip_ws(&c, i + "state.update".chars().count());
        if c.get(j) == Some(&'=') {
            j = skip_ws(&c, j + 1);
            if c.get(j) == Some(&'{') {
                let start = j + 1;
                if let Some(k) = find_seq(&c, "update_available:", start) {
                    if k - start <= 300 {
                        return true;
                    }
                }
            }
        }
        from = i + 1;
    }
    false
}

/// `update\?:\s*\{[\s\S]{0,200}update_available` — the console's client type.
fn client_declares(t: &str) -> bool {
    let c = chars(t);
    let mut from = 0;
    while let Some(i) = find_seq(&c, "update?:", from) {
        let j = skip_ws(&c, i + "update?:".chars().count());
        if c.get(j) == Some(&'{') {
            let start = j + 1;
            if let Some(k) = find_seq(&c, "update_available", start) {
                if k - start <= 200 {
                    return true;
                }
            }
        }
        from = i + 1;
    }
    false
}

/// `\bverdict\s*=\s*st\?\.\s*update\b` — THE STRUCTURE, NOT THE WORDS. The first version asked whether
/// `update_available` and `verdict` appear anywhere in the file — and a mutation that made the view
/// ignore the device's verdict (`const verdict = undefined`) still passed, because both words survive in
/// the fallback and the variable. This asks for the READ: the verdict assigned FROM the probed status,
/// which is the link the mutation broke.
fn view_reads(t: &str) -> bool {
    let c = chars(t);
    let mut from = 0;
    while let Some(i) = find_seq(&c, "verdict", from) {
        let end = i + "verdict".chars().count();
        if i == 0 || !is_word(c[i - 1]) {
            let mut j = skip_ws(&c, end);
            if c.get(j) == Some(&'=') {
                j = skip_ws(&c, j + 1);
                if find_seq(&c, "st?.", j) == Some(j) {
                    let k = skip_ws(&c, j + "st?.".chars().count());
                    if find_seq(&c, "update", k) == Some(k) {
                        let after = k + "update".chars().count();
                        if after >= c.len() || !is_word(c[after]) {
                            return true;
                        }
                    }
                }
            }
        }
        from = i + 1;
    }
    false
}

/// `search(/verdict\s*\?\s*verdict\.update_available/)` — the guarded ternary's first index.
fn guarded_ternary(t: &str) -> Option<usize> {
    let c = chars(t);
    let mut from = 0;
    while let Some(i) = find_seq(&c, "verdict", from) {
        let mut j = skip_ws(&c, i + "verdict".chars().count());
        if c.get(j) == Some(&'?') {
            j = skip_ws(&c, j + 1);
            if find_seq(&c, "verdict.update_available", j) == Some(j) {
                return Some(i);
            }
        }
        from = i + 1;
    }
    None
}

/// `search(/lastVersion\s*!==\s*install/)` — the console's own comparison.
fn own_comparison(t: &str) -> Option<usize> {
    let c = chars(t);
    let mut from = 0;
    while let Some(i) = find_seq(&c, "lastVersion", from) {
        let mut j = skip_ws(&c, i + "lastVersion".chars().count());
        if find_seq(&c, "!==", j) == Some(j) {
            j = skip_ws(&c, j + 3);
            if find_seq(&c, "install", j) == Some(j) {
                return Some(i);
            }
        }
        from = i + 1;
    }
    None
}

/// AND THE OLD COMPARISON STAYS INSIDE THE FALLBACK (round 134). The clause above proves the device's
/// verdict is READ; it cannot see whether the console's own comparison is still GUARDED by it.
/// Hoisting it — `d.lastVersion !== install.version || verdict?.update_available` — passes every
/// pattern above while making the console answer for devices it cannot speak about, which is the
/// round-29 defect in a new costume. So: the comparison must appear AFTER the guarded ternary in the
/// file, which is where a fallback lives and a hoist cannot.
fn view_guards_the_fallback(t: &str) -> bool {
    match (guarded_ternary(t), own_comparison(t)) {
        (Some(g), Some(cmp)) => cmp > g,
        _ => false,
    }
}

fn test_link(i: usize, t: &str) -> bool {
    match i {
        0 => mcp_forwards(t),
        1 => client_declares(t),
        2 => view_reads(t),
        _ => unreachable!(),
    }
}

fn also_link(i: usize, t: &str) -> bool {
    match i {
        2 => view_guards_the_fallback(t),
        _ => true,
    }
}

fn check() -> Streams {
    let root = common::repo();
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut fail = 0;
    for (i, (file, what)) in LINKS.iter().enumerate() {
        let text = match fs::read_to_string(root.join(file)) {
            Ok(t) => t,
            Err(e) => {
                stderr.push_str(&format!(
                    "FAIL {file} is missing ({e}) — the verdict's chain is broken at this link\n"
                ));
                fail += 1;
                continue;
            }
        };
        if test_link(i, &text) && also_link(i, &text) {
            stdout.push_str(&format!("ok   {file}: {what}\n"));
        } else {
            stderr.push_str(&format!(
                "FAIL {file} no longer {what}. The console would be computing the update verdict ITSELF again — the defect \
                 round 29 fixed, where the comparison got the answer backwards for the device that was behind AND the one that \
                 was deliberately pinned. Restore the link, or change the design deliberately and update this gate's reason.\n"
            ));
            fail += 1;
        }
    }
    if fail > 0 {
        stderr.push_str(&format!(
            "\ndevice-verdict: {fail} broken link(s) in the verdict's chain\n"
        ));
        return Streams {
            stdout,
            stderr,
            failed: true,
        };
    }
    stdout.push_str(
        "\ndevice-verdict: the device's own update verdict still crosses all three files\n",
    );
    Streams {
        stdout,
        stderr,
        failed: false,
    }
}

#[test]
fn the_devices_verdict_still_crosses_the_wire() {
    let out = check();
    print!("{}", out.stdout);
    if out.failed {
        panic!("{}", out.stderr);
    }
    assert_eq!(out.stdout.matches("ok   ").count(), LINKS.len());
}

// ── the patterns' own proof: each one must be able to say NO ────────────────────────────────────────

#[test]
fn the_worker_pattern_wants_the_assembly_not_the_word() {
    assert!(mcp_forwards("state.update = {\n  update_available: u,\n};"));
    // THE MEASURED FAILURE OF THE FIRST VERSION: `update_available` merely mentioned nearby.
    assert!(!mcp_forwards(
        "state.update = {\n  other: 1,\n};\nconst update_available = 1;"
    ));
    // ...and the 300-character window is what makes "nearby" mean something.
    assert!(!mcp_forwards(&format!(
        "state.update = {{{}update_available:",
        "x".repeat(301)
    )));
    assert!(mcp_forwards(&format!(
        "state.update = {{{}update_available:",
        "x".repeat(300)
    )));
}

#[test]
fn the_view_pattern_wants_the_read_not_the_words() {
    assert!(view_reads("const verdict = st?.update;"));
    // The whitespace the regex allows is AFTER the `?.`, not between `st` and it.
    assert!(view_reads("const verdict = st?.\n  update;"));
    assert!(!view_reads("const verdict = st ?. update;"));
    // THE MUTATION THAT ESCAPED THE FIRST VERSION: both words survive, the READ is gone.
    assert!(!view_reads(
        "const verdict = undefined; // update_available"
    ));
    // A word boundary on both ends: `myverdict` is not `verdict`, and `updates` is not `update`.
    assert!(!view_reads("const myverdict = st?.update;"));
    assert!(!view_reads("const verdict = st?.updates;"));
}

#[test]
fn the_fallback_must_stay_after_the_guarded_ternary() {
    let read = "const verdict = st?.update;";
    let guarded =
        "const stale = verdict ? verdict.update_available : d.lastVersion !== install.version;";
    let hoisted =
        "const stale = d.lastVersion !== install.version || verdict ? verdict.update_available : false;";
    assert!(
        view_reads(read),
        "the read is what the clause above asks for"
    );
    assert!(view_guards_the_fallback(&format!("{read}\n{guarded}")));
    // THE HOIST: the comparison first, the device's answer second — the read above still passes.
    assert!(view_reads(read), "the read is still there in the hoist");
    assert!(
        !view_guards_the_fallback(&format!("{read}\n{hoisted}")),
        "and the console would answer for devices it cannot speak about"
    );
    assert!(!view_guards_the_fallback(&format!(
        "{read}\nconst stale = verdict ? verdict.update_available : false;"
    )));
}

#[test]
fn a_missing_link_is_a_failure_with_its_own_sentence() {
    // The branch is unreachable on a tree where the three files exist; what is pinned is that the shape
    // exists and that it counts as a broken link rather than a skip.
    let out = check();
    assert!(!out.stderr.contains("is missing"), "{}", out.stderr);
    assert!(out
        .stdout
        .contains("gateway/src/plugins/mcp.ts: the worker FORWARDS"));
}
