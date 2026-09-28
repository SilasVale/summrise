//! CARRY AND DETECT ARE TWO LISTS, AND NOTHING HELD THEM TOGETHER.
//!
//! `useSessions.ts` writes that sentence about itself:
//!
//!   AND THE LIST IS WHERE THE BUG WAS. It omitted `idleMs` and `commandRunning` while `wireFields` did
//!   too, so a refresh neither carried them nor noticed them — a row kept its discovery values for life
//!   and three consumers read them as live (`sessionActive`, the rail's `anyCommandRunning`,
//!   `idleSessions`' offer-to-close). CARRY AND DETECT ARE TWO LISTS; fixing the bug needed both.
//!
//! The list must stay explicit: `pendingApproval` is DERIVED at map time, so comparing the mapped objects
//! would report a change on every poll (the comment records that attempt and why it broke). So the two
//! lists cannot be merged — but they CAN be compared, and until now nothing did.
//!
//! This gate reads both and fails when DETECT stops covering CARRY.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28): the JS and
//! this file print the SAME one line, byte for byte — 1 carried field(s)… the exact counts are in the
//! commit. A planted `existing.ghost` inside `wireFieldsChanged` moves BOTH to the same two-line
//! failure body naming `ghost` in the same sentence, and a field named only in a COMMENT leaves both at
//! exit 0.
//!
//! MUTATION: drop a comparison from `wireFieldsChanged`, or carry a field no comparison names.
//! RESULT:   fails, naming the field and the direction: "wireFields carries it, wireFieldsChanged never
//!           compares it — a refresh would neither carry nor notice a change, which is the bug the
//!           comment above that list records."
//!
//! THE COMMENTS ARE STRIPPED FIRST, through the SHARED `common::decomment` — the same rule
//! `scripts/test/lib/decomment.mjs` holds for the JS, which imports it here too. A gate that read its
//! own documentation would report every field the comment above `wireFieldsChanged` names.

mod common;

use common::decomment;
use std::collections::BTreeSet;

const FILE: &str = "agent/resources/panel-react/src/hooks/useSessions.ts";

struct Streams {
    stdout: String,
    stderr: String,
    failed: bool,
}

/// The keys an object-returning arrow declares, ignoring spreads (those come from another list).
/// `/^\s{2}([A-Za-z_][A-Za-z0-9_]*)\s*:/` — EXACTLY two leading whitespace characters, which is what
/// keeps a nested object's keys and a continuation line out of the list.
fn keys_of(block: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in block.split('\n') {
        let c: Vec<char> = line.chars().collect();
        if c.len() < 3 || !c[0].is_whitespace() || !c[1].is_whitespace() || c[2].is_whitespace() {
            continue;
        }
        if !(c[2].is_ascii_alphabetic() || c[2] == '_') {
            continue;
        }
        let mut j = 2;
        while j < c.len() && (c[j].is_ascii_alphanumeric() || c[j] == '_') {
            j += 1;
        }
        let mut k = j;
        while k < c.len() && c[k].is_whitespace() {
            k += 1;
        }
        if k < c.len() && c[k] == ':' {
            out.insert(c[2..j].iter().collect());
        }
    }
    out
}

/// The block after `marker`, brace-walked to its own closer — the body of an object-returning arrow.
fn block_after(src: &str, marker: &str) -> Option<String> {
    let c: Vec<char> = src.chars().collect();
    let at = find(src, marker, 0)?;
    let open = src[at..].find('{')? + at;
    let mut depth = 0i64;
    let mut j = open;
    while j < c.len() {
        if c[j] == '{' {
            depth += 1;
        } else if c[j] == '}' {
            depth -= 1;
            if depth == 0 {
                return Some(c[open + 1..j].iter().collect());
            }
        }
        j += 1;
    }
    None
}

/// `wireFieldsChanged` is an EXPRESSION-bodied arrow — `(a, b): boolean => expr;` — so there is no brace
/// to walk. (The first version of the JS used the brace walker for all three and reported every field as
/// missing, because it had walked into an unrelated block further down.)
fn expression_after(src: &str, marker: &str) -> Option<String> {
    let at = find(src, marker, 0)?;
    let arrow = find(src, "=>", at)?;
    let end = src[arrow..].find(';')? + arrow;
    Some(src[arrow + 2..end].to_string())
}

/// A byte index, because every needle here is ASCII; `from` is a byte index too.
fn find(hay: &str, needle: &str, from: usize) -> Option<usize> {
    hay.get(from..)?.find(needle).map(|i| i + from)
}

/// DETECT = the fields `wireFieldsChanged` compares. It reads them off the mapped objects, so the left
/// operand names the field; `?.id` narrows to an identity without changing which field it is.
/// `/existing\.([A-Za-z_][A-Za-z0-9_]*)/g`.
fn detect_fields(block: &str) -> BTreeSet<String> {
    let c: Vec<char> = block.chars().collect();
    let needle: Vec<char> = "existing.".chars().collect();
    let mut out = BTreeSet::new();
    let mut p = 0;
    while p + needle.len() <= c.len() {
        if c[p..p + needle.len()] == needle[..] {
            let start = p + needle.len();
            let mut j = start;
            while j < c.len() && (c[j].is_ascii_alphanumeric() || c[j] == '_') {
                j += 1;
            }
            if j > start && (c[start].is_ascii_alphabetic() || c[start] == '_') {
                out.insert(c[start..j].iter().collect());
                p = j; // `matchAll` does not re-scan inside a match
                continue;
            }
        }
        p += 1;
    }
    out
}

fn compare(carry: &BTreeSet<String>, detect: &BTreeSet<String>) -> Streams {
    let missing: Vec<&String> = carry.difference(detect).collect();
    let extra: Vec<&String> = detect.difference(carry).collect();
    if missing.is_empty() && extra.is_empty() {
        return Streams {
            stdout: String::new(),
            stderr: String::new(),
            failed: false,
        };
    }
    let mut stderr = format!("FAIL {FILE}: CARRY and DETECT disagree about the session row.\n");
    for k in missing {
        stderr.push_str(&format!(
            "  {k}: wireFields carries it, wireFieldsChanged never compares it — a refresh would neither \
             carry nor notice a change, which is the bug the comment above that list records.\n"
        ));
    }
    for k in extra {
        stderr.push_str(&format!(
            "  {k}: wireFieldsChanged compares it, but no list carries it.\n"
        ));
    }
    stderr.push_str(
        "\nAdd it to `wireFieldsChanged` (and keep `pendingApproval` narrow — its deadline is derived).\n",
    );
    Streams {
        stdout: String::new(),
        stderr,
        failed: true,
    }
}

fn check() -> Streams {
    let src = decomment(&common::read(FILE));
    let (Some(live_block), Some(wire_block), Some(changed_block)) = (
        block_after(&src, "const liveFields = (s: any) => ({"),
        block_after(&src, "const wireFields = (s: any) => ({"),
        expression_after(&src, "const wireFieldsChanged = ("),
    ) else {
        return Streams {
            stdout: String::new(),
            stderr: format!(
                "FAIL {FILE}: could not read one of the three lists — this gate cannot measure.\n"
            ),
            failed: true,
        };
    };

    // CARRY = what liveFields produces plus what wireFields adds. A spread is the link between them.
    let live = keys_of(&live_block);
    let wire = keys_of(&wire_block);
    if !wire_block.contains("...liveFields(s)") {
        return Streams {
            stdout: String::new(),
            stderr: format!(
                "FAIL {FILE}: `wireFields` no longer spreads `liveFields` — the CARRY side is now two lists of \
                 its own, and this comparison cannot see half of it.\n"
            ),
            failed: true,
        };
    }
    let carry: BTreeSet<String> = live.union(&wire).cloned().collect();
    let detect = detect_fields(&changed_block);

    let mut out = compare(&carry, &detect);
    if !out.failed {
        out.stdout = format!(
            "session-carry-detect: {} carried field(s), all {} compared by wireFieldsChanged ({} from liveFields + {} from wireFields).\n",
            carry.len(),
            detect.len(),
            live.len(),
            wire.len()
        );
    }
    out
}

#[test]
fn carry_and_detect_agree_about_the_session_row() {
    let out = check();
    print!("{}", out.stdout);
    if out.failed {
        panic!("{}", out.stderr);
    }
    // A FLOOR, not a claim: the two lists are the tree's business and may grow. What must not happen is
    // this comparison reading almost nothing and reporting agreement over it.
    let carried: usize = out
        .stdout
        .split_once("session-carry-detect: ")
        .and_then(|(_, rest)| rest.split(' ').next())
        .and_then(|n| n.parse().ok())
        .expect("the line names the carried count");
    assert!(
        carried >= 8,
        "read only {carried} carried field(s) — the lists moved, so this proves nothing"
    );
    assert!(
        out.stdout.contains("compared by wireFieldsChanged"),
        "{}",
        out.stdout
    );
}

// ── the scanner's own proof: what it must catch, and the comment it must not ────────────────────────

#[test]
fn a_field_named_only_in_a_comment_is_not_compared() {
    // THE CASE THAT MUST NOT BITE, and the reason `decomment` is shared rather than re-written: the
    // comment above `wireFieldsChanged` names every field it explains, so a gate that read prose would
    // report the whole list as missing.
    let block = "  // existing.ghost is mentioned here\n  existing.live !== fresh.live";
    let detect = detect_fields(&decomment(block));
    assert_eq!(
        detect.iter().cloned().collect::<Vec<_>>(),
        vec!["live".to_string()]
    );
    // A whole-line comment goes entirely, and a trailing one goes with it.
    assert!(detect_fields(&decomment("// existing.ghost")).is_empty());
    assert_eq!(
        detect_fields(&decomment(
            "existing.live !== fresh.live; // existing.ghost"
        ))
        .iter()
        .cloned()
        .collect::<Vec<_>>(),
        vec!["live".to_string()]
    );
    // AND `//` IN A URL IS NOT A COMMENT — the conservative half of the shared rule.
    assert_eq!(
        detect_fields(&decomment("existing.x // https://a.b/c"))
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["x".to_string()]
    );
}

#[test]
fn a_spread_is_not_a_key_and_the_indentation_is_exact() {
    let keys = keys_of(
        "  ...liveFields(s),\n  heldByHuman: !!s.held_by_human,\n    nested: 1,\n  noColon\n",
    );
    assert_eq!(
        keys.iter().cloned().collect::<Vec<_>>(),
        vec!["heldByHuman".to_string()],
        "a spread comes from another list, a four-space line is nested, and a line without a colon is not a key"
    );
}

#[test]
fn the_two_directions_are_named_differently() {
    let carry: BTreeSet<String> = ["carried", "both"].iter().map(|s| s.to_string()).collect();
    let detect: BTreeSet<String> = ["both", "extra"].iter().map(|s| s.to_string()).collect();
    let out = compare(&carry, &detect);
    assert!(out.failed);
    assert!(out.stdout.is_empty(), "a failure prints nothing on stdout");
    assert!(
        out.stderr.contains(
            "FAIL agent/resources/panel-react/src/hooks/useSessions.ts: CARRY and DETECT disagree"
        ),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr
            .contains("  carried: wireFields carries it, wireFieldsChanged never compares it"),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr
            .contains("  extra: wireFieldsChanged compares it, but no list carries it."),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr
            .ends_with("(and keep `pendingApproval` narrow — its deadline is derived).\n"),
        "{}",
        out.stderr
    );
    // ...and agreement is silent on stderr.
    let same = compare(&carry, &carry);
    assert!(!same.failed && same.stderr.is_empty() && same.stdout.is_empty());
}

#[test]
fn the_three_markers_are_read_from_the_real_file() {
    let src = decomment(&common::read(FILE));
    let live = block_after(&src, "const liveFields = (s: any) => ({").expect("liveFields");
    let wire = block_after(&src, "const wireFields = (s: any) => ({").expect("wireFields");
    let changed = expression_after(&src, "const wireFieldsChanged = (").expect("wireFieldsChanged");
    assert!(wire.contains("...liveFields(s)"), "the spread is the link");
    assert!(
        !live.trim_end().ends_with("});"),
        "the walk stops at the closer"
    );
    assert!(
        changed.contains("existing."),
        "the comparison reads the mapped objects"
    );
    // A marker that is not there is `None`, never an empty block that would agree with everything.
    assert!(block_after(&src, "const notThere = (").is_none());
    assert!(expression_after(&src, "const notThere = (").is_none());
}
