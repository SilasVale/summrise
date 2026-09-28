//! THE TWO BACKENDS OF ONE MODULE MUST OFFER THE SAME SURFACE.
//!
//! WHY THIS EXISTS (round 148 of the standing goal). Round 146 found that the agent's DEFAULT feature
//! config had not compiled for a long time: `plugins/terminal/tools/exec.rs` calls `term_note_exit_code`
//! in three places, the REAL terminal backend defines it, and the STUB — the backend compiled when
//! the `terminal` feature is off — did not. Nothing feature-independent called it when it was added,
//! which is exactly why it went unnoticed until a caller appeared in a third place.
//!
//! A configuration nobody builds is a configuration that is broken, and a surface with a hole in it
//! breaks in ONE configuration only. This compares the two: every `pub async fn` the real backend
//! offers must be offered by the stub, or declared here with a reason.
//!
//! WHAT IT DOES NOT CHECK: the bodies (a stub answers "nothing here" on purpose), the other modules,
//! or whether a stub's answer is truthful — only that a caller cannot fail to COMPILE because it
//! picked the wrong backend.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/stub-surface-check.mjs` → exit 0, "stub-surface: 35 method(s) in the real
//!     backend, 34 in the stub, 1 declared exception(s) — a caller cannot pick the wrong backend".
//!   * this file → the SAME sentence with the SAME three numbers, character for character.
//!   * a planted `pub async fn not_in_the_stub(&self) {}` in `agent/src/tools/terminal/mod.rs` → the
//!     JS gate exits 1 naming `agent/src/tools/terminal/mod.rs offers \`not_in_the_stub\` and
//!     agent/src/tools/terminal/stub.rs does not — a caller that compiles with the feature on will
//!     fail to compile without it, in a configuration CI may not build. Mirror it, or declare it
//!     here with the reason it cannot be`, and this file fails with the identical sentence.
//!   * the same signature placed inside a COMMENT → BOTH STAY GREEN, which is the honest measurement
//!     rather than a divergence: the strip below is what makes an EPITAPH (a comment quoting the
//!     signature a change removed, which is the clearest way to write it) not read as a live method.
//!     Found 2026-09-24 when `touch` was deleted and its epitaph tripped this check.
//!
//! MUTATION: offer a `pub async fn` in the real backend that the stub does not mirror, without
//!           declaring it.
//! RESULT:   fails, naming both files, the method, and the sentence that says a caller compiling with
//!           the feature on will fail to compile without it.

mod common;

use common::read;
use std::collections::BTreeSet;

const REAL: &str = "agent/src/tools/terminal/mod.rs";
const STUB: &str = "agent/src/tools/terminal/stub.rs";

/// Names the stub deliberately does not mirror, each with the reason it cannot or should not.
const DECLARED: [(&str, &str); 1] = [(
    "sweep_idle",
    "its return type is defined inside the feature-gated module, so a stub mirror cannot name it without moving the type — and no feature-independent caller exists",
)];

/// COMMENTS ARE STRIPPED FIRST, the lesson two sibling gates already record (`css-vars-check` and
/// `retired-colours-check` learned it the same way): this scan reads source TEXT, so without
/// stripping, a comment that DOCUMENTS a removal — quoting the signature it removed — reads as the
/// method still being offered. A gate that cannot see past a comment forbids documenting a removal,
/// and the next contributor documents it anyway and then declares a difference that does not exist.
///
/// NOTE THE RULE IS NARROWER THAN `common::decomment`, AND DELIBERATELY: this one drops a `//` line
/// only when the whole line is a comment (`^\s*\/\/`), so a TRAILING `// …` survives here while
/// `decomment` removes it. That is the JS's own split — `stub-surface-check.mjs` carries its own
/// `stripComments` rather than importing `lib/decomment.mjs` — and it is reproduced rather than
/// unified, because unifying it would change what this gate reads.
fn strip_comments(s: &str) -> String {
    let blocks = common::strip_block_comments(s);
    blocks
        .split('\n')
        .map(|line| match line.find("//") {
            // A whole-line comment goes entirely, whitespace included: `^\s*` is part of the match.
            Some(k) if line[..k].trim().is_empty() => String::new(),
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `pub async fn (\w+)` — the offered surface, in source order, de-duplicated.
fn offered(src: &str) -> Vec<String> {
    let needle = "pub async fn ";
    let c: Vec<char> = strip_comments(src).chars().collect();
    let n: Vec<char> = needle.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut i = 0;
    while i + n.len() <= c.len() {
        if c[i..i + n.len()] != n[..] {
            i += 1;
            continue;
        }
        let mut j = i + n.len();
        let start = j;
        while j < c.len() && common::is_word(c[j]) {
            j += 1;
        }
        if j > start {
            let name: String = c[start..j].iter().collect();
            if seen.insert(name.clone()) {
                out.push(name);
            }
        }
        i = j.max(i + 1);
    }
    out
}

fn check() -> Result<String, String> {
    let real = offered(&read(REAL));
    let stub = offered(&read(STUB));

    if real.len() < 20 {
        return Err(format!(
            "FAIL read only {} method(s) from the real backend — the tree moved, so this proves nothing",
            real.len()
        ));
    }

    let mut findings = Vec::new();
    for name in &real {
        if !stub.contains(name) && !DECLARED.iter().any(|(n, _)| n == name) {
            findings.push(format!(
                "{REAL} offers `{name}` and {STUB} does not — a caller that compiles with the feature on will fail to compile without it, in a configuration CI may not build. Mirror it, or declare it here with the reason it cannot be"
            ));
        }
    }
    for name in &stub {
        if !real.contains(name) {
            findings.push(format!(
                "{STUB} offers `{name}` and {REAL} does not — the two surfaces have drifted apart"
            ));
        }
    }
    if !findings.is_empty() {
        return Err(format!(
            "stub-surface: {} difference(s) between the backends:\n  {}",
            findings.len(),
            findings.join("\n  ")
        ));
    }
    Ok(format!(
        "stub-surface: {} method(s) in the real backend, {} in the stub, {} declared exception(s) — a caller cannot pick the wrong backend",
        real.len(),
        stub.len(),
        DECLARED.len()
    ))
}

#[test]
fn the_two_backends_offer_the_same_surface() {
    let msg = check().unwrap_or_else(|e| panic!("{e}"));
    println!("{msg}");
    assert!(
        msg.contains("35 method(s) in the real backend, 34 in the stub, 1 declared exception(s)"),
        "{msg}"
    );
}

// ── the scanner's own proof: a parser that reads nothing must not pass ─────────────────────────

#[test]
fn a_signature_in_a_comment_is_not_an_offered_method() {
    assert_eq!(
        offered("pub async fn live_one(&self) {}"),
        vec!["live_one".to_string()]
    );
    // The measured case: an EPITAPH quoting the signature a change removed.
    assert!(offered("// pub async fn touch(&self) {}").is_empty());
    assert!(offered("/* pub async fn touch(&self) {} */").is_empty());
    // A TRAILING `// …` survives this strip, which is the documented difference from `decomment`.
    assert_eq!(
        offered("pub async fn live_one(&self) {} // pub async fn ghost(&self) {}"),
        vec!["live_one".to_string(), "ghost".to_string()]
    );
    // `pub fn` is not `pub async fn`, and a duplicated name is one method.
    assert!(offered("pub fn sync_one(&self) {}").is_empty());
    assert_eq!(
        offered("pub async fn a(&self) {}\npub async fn a(&self) {}"),
        vec!["a".to_string()]
    );
}

#[test]
fn a_real_backend_that_reads_nothing_is_reported_rather_than_passing() {
    // The floor is what a moved tree trips, and it is checked before any finding.
    let err = check_declared(&[], &["a".to_string()]).expect_err("the floor must bite");
    assert!(
        err.contains("read only 0 method(s) from the real backend"),
        "{err}"
    );
}

/// The finding logic, without the file reads — so the floor can be proven on a fixture.
fn check_declared(real: &[String], stub: &[String]) -> Result<String, String> {
    if real.len() < 20 {
        return Err(format!(
            "FAIL read only {} method(s) from the real backend — the tree moved, so this proves nothing",
            real.len()
        ));
    }
    let mut findings = Vec::new();
    for name in real {
        if !stub.contains(name) && !DECLARED.iter().any(|(n, _)| n == name) {
            findings.push(format!(
                "{REAL} offers `{name}` and {STUB} does not — a caller that compiles with the feature on will fail to compile without it, in a configuration CI may not build. Mirror it, or declare it here with the reason it cannot be"
            ));
        }
    }
    for name in stub {
        if !real.contains(name) {
            findings.push(format!(
                "{STUB} offers `{name}` and {REAL} does not — the two surfaces have drifted apart"
            ));
        }
    }
    if !findings.is_empty() {
        return Err(format!(
            "stub-surface: {} difference(s) between the backends:\n  {}",
            findings.len(),
            findings.join("\n  ")
        ));
    }
    Ok(format!(
        "stub-surface: {} method(s) in the real backend, {} in the stub, {} declared exception(s) — a caller cannot pick the wrong backend",
        real.len(),
        stub.len(),
        DECLARED.len()
    ))
}

#[test]
fn a_declared_exception_is_not_a_finding_and_an_extra_stub_method_is() {
    let mut real: Vec<String> = (0..20).map(|i| format!("m{i}")).collect();
    real.push("sweep_idle".to_string());
    let stub: Vec<String> = (0..20).map(|i| format!("m{i}")).collect();
    let msg = check_declared(&real, &stub).expect("a declared exception is not a finding");
    assert!(
        msg.contains("21 method(s) in the real backend, 20 in the stub"),
        "{msg}"
    );
    // And the other direction: a stub method the real backend does not offer.
    let mut stub_extra = stub.clone();
    stub_extra.push("only_in_the_stub".to_string());
    let err = check_declared(&real, &stub_extra).expect_err("drift must fail");
    assert!(err.contains("offers `only_in_the_stub` and"), "{err}");
}
