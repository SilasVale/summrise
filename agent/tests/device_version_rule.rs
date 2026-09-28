//! ONE RULE, TWO PACKAGES, NO SHARED MODULE.
//!
//! "Which of the device's two version fields wins" is implemented on BOTH sides:
//!
//!   panel    agent/resources/panel-react/src/lib/agentVersion.ts   releaseVersion()
//!   gateway  gateway/src/plugins/mcp.ts                            wireVersion()
//!
//! They cannot share code — different packages, different deploy targets — and BOTH have been broken:
//! round-304 on the gateway side (the console read the frozen Cargo `version`, showed v1.0.145 forever,
//! and the outdated badge never cleared), and the panel's own comment records the other direction (its
//! desktop shell read v1.2.354 while Settings reported v1.0.145 for the SAME device). The panel names
//! the lesson: "Two copies of a rule is what let them disagree."
//!
//! This gate does not merge them. It holds them to ONE rule and requires that each side NAMES the
//! other, so a change on one side is visible from the other — which is the part neither had.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28): both print
//! the same closing line, byte for byte. A mutation that swaps the two `return`s in `wireVersion` moves
//! BOTH to the same failure body naming the branch that no longer tests `release`. The measurements are
//! in the commit.
//!
//! MUTATION: make either half read the frozen `version` first, or delete a cross-reference.
//! RESULT:   fails, naming the side and the reason: "its first version branch does not test `release`…"
//!
//! RAW FOR THE CROSS-REFERENCES, DECOMMENTED FOR THE PRECEDENCE, and the split is the whole point: the
//! names each side must carry live in COMMENTS, so decommenting first erases the thing being checked
//! (the first version of the JS did exactly that and reported each side as naming nothing), while the
//! precedence check reads CODE and a comment could mention either name in any order.

mod common;

use common::decomment;
use std::fs;

const PANEL: &str = "agent/resources/panel-react/src/lib/agentVersion.ts";
const GATEWAY: &str = "gateway/src/plugins/mcp.ts";

struct Streams {
    stdout: String,
    stderr: String,
    failed: bool,
}

/// `bodyOf(src, name)`: from `export function <name>(`, the next `{`, brace-walked to its own closer.
/// An empty string when either is missing — which is what makes a RENAMED function a problem rather
/// than a pass.
fn body_of(src: &str, name: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let marker = format!("export function {name}(");
    let Some(at) = find(src, &marker, 0) else {
        return String::new();
    };
    let at = src[..at].chars().count();
    let Some(open) = find(src, "{", src[..at].len()) else {
        return String::new();
    };
    let open = src[..open].chars().count();
    let mut depth = 0i64;
    let mut j = open;
    while j < c.len() {
        if c[j] == '{' {
            depth += 1;
        } else if c[j] == '}' {
            depth -= 1;
            if depth == 0 {
                return c[open + 1..j].iter().collect();
            }
        }
        j += 1;
    }
    String::new()
}

/// A byte index, because every needle here is ASCII.
fn find(hay: &str, needle: &str, from: usize) -> Option<usize> {
    hay.get(from..)?.find(needle).map(|i| i + from)
}

/// READ THE BRANCHES, NOT THE FIRST MENTION. The first version of the JS used `indexOf` over the whole
/// body, which found the TypeScript annotation (`as { release?: unknown; version?: unknown }`) — so it
/// measured DECLARATION order and stayed green when the two `return`s were swapped. A mutation proved
/// it: reversing the branches in `wireVersion` left the gate passing. `/if \(([^)]*)\)/g`, first branch
/// naming either field.
fn first_version_branch(body: &str) -> Option<String> {
    let c: Vec<char> = body.chars().collect();
    let mut from = 0;
    while let Some(i) = find(body, "if (", from) {
        let i = body[..i].chars().count();
        let start = i + "if (".chars().count();
        let mut j = start;
        while j < c.len() && c[j] != ')' {
            j += 1;
        }
        if j < c.len() {
            let branch: String = c[start..j].iter().collect();
            if branch.contains("release") || branch.contains("version") {
                return Some(branch);
            }
            // `matchAll` does not re-scan inside a match; the next `if (` starts after this one's `)`.
            from = byte_index_of_char(body, j + 1);
            continue;
        }
        break;
    }
    None
}

fn byte_index_of_char(s: &str, chars: usize) -> usize {
    s.char_indices()
        .nth(chars)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

fn check() -> Streams {
    let root = common::repo();
    let panel_raw = match fs::read_to_string(root.join(PANEL)) {
        Ok(t) => t,
        Err(e) => {
            return Streams {
                stdout: String::new(),
                stderr: format!("FAIL device-version-rule: cannot read {PANEL} ({e})\n"),
                failed: true,
            }
        }
    };
    let gateway_raw = match fs::read_to_string(root.join(GATEWAY)) {
        Ok(t) => t,
        Err(e) => {
            return Streams {
                stdout: String::new(),
                stderr: format!("FAIL device-version-rule: cannot read {GATEWAY} ({e})\n"),
                failed: true,
            }
        }
    };
    let panel = decomment(&panel_raw);
    let gateway = decomment(&gateway_raw);

    let mut problems: Vec<String> = Vec::new();

    // 1. Each side must still implement the rule, under a name, so a reader can find it.
    if !panel.contains("export function releaseVersion(") {
        problems.push(format!(
            "{PANEL}: `releaseVersion` is gone — the panel's half of the rule has no name."
        ));
    }
    if !gateway.contains("export function wireVersion(") {
        problems.push(format!(
            "{GATEWAY}: `wireVersion` is gone. This was two inline lines until round 236; without a name the \
             console's half of the rule cannot be pointed at, tested, or found by the other side."
        ));
    }

    // 2. Both must prefer `release`, and only then fall back to `version`. Order is the whole rule:
    //    `version` is the FROZEN Cargo protocol version, so reading it first is what showed v1.0.145
    //    forever.
    for (src, name, label) in [
        (&panel, "releaseVersion", PANEL),
        (&gateway, "wireVersion", GATEWAY),
    ] {
        let body = body_of(src, name);
        let first = first_version_branch(&body);
        if !first.is_some_and(|b| b.contains("release")) {
            problems.push(format!(
                "{label}: its first version branch does not test `release`. `version` is the frozen Cargo \
                 protocol version (1.0.x); reading it first is the round-304 defect that showed v1.0.145 forever."
            ));
        }
    }

    // 3. Each side must NAME the other. This is the part that was missing, and the only part that makes
    //    a change on one side visible from the other without a shared module.
    if !panel_raw.contains("gateway/src/plugins/mcp.ts") {
        problems.push(format!(
            "{PANEL}: no longer names `gateway/src/plugins/mcp.ts`. The console applies the same rule to the same \
             device; if this file does not say so, the next change here is invisible from there."
        ));
    }
    if !gateway_raw.contains("agent/resources/panel-react/src/lib/agentVersion.ts") {
        problems.push(format!(
            "{GATEWAY}: no longer names the panel's `agentVersion.ts`. Same reason, other direction."
        ));
    }

    if !problems.is_empty() {
        let mut stderr = String::from(
            "FAIL device-version-rule: the two halves of this rule have drifted apart.\n\n",
        );
        for p in &problems {
            stderr.push_str(&format!("  {p}\n"));
        }
        return Streams {
            stdout: String::new(),
            stderr,
            failed: true,
        };
    }
    Streams {
        stdout: String::from(
            "device-version-rule: both halves prefer `release` over the frozen `version`, and each names the other.\n",
        ),
        stderr: String::new(),
        failed: false,
    }
}

#[test]
fn both_halves_of_the_version_rule_still_agree() {
    let out = check();
    print!("{}", out.stdout);
    if out.failed {
        panic!("{}", out.stderr);
    }
    assert!(out.stdout.contains("each names the other"));
}

// ── the scanner's own proof ─────────────────────────────────────────────────────────────────────────

#[test]
fn the_branch_is_read_from_the_returns_not_from_the_annotation() {
    // THE MUTATION THE FIRST VERSION MISSED: the annotation mentions both names, so an `indexOf` over
    // the whole body measured DECLARATION order. The branches are what the rule lives in.
    let swapped = "  return as { release?: unknown; version?: unknown }\n    if (v.version) return v.version;\n    if (v.release) return v.release;";
    let first = first_version_branch(swapped).expect("a branch naming a field");
    assert_eq!(first.trim(), "v.version");
    assert!(
        !first.contains("release"),
        "the frozen version is read first"
    );
    let right = "  if (v.release) return v.release;\n  if (v.version) return v.version;";
    assert!(first_version_branch(right)
        .expect("a branch")
        .contains("release"));
    // A body with no version branch at all is a problem, not a pass.
    assert!(first_version_branch("  return 1;").is_none());
}

#[test]
fn a_renamed_function_has_no_body_and_is_reported() {
    assert!(body_of(
        "export function wireVersion(x) { if (x.release) return 1; }",
        "wireVersion"
    )
    .contains("x.release"));
    assert_eq!(body_of("export function other(x) { }", "wireVersion"), "");
    // The walk stops at the function's OWN closer, not the file's end.
    let two = "export function a() { if (x.release) return 1; }\nexport function b() { return 2; }";
    assert_eq!(body_of(two, "a").trim(), "if (x.release) return 1;");
}

#[test]
fn the_cross_references_are_read_raw_because_they_live_in_comments() {
    let raw = "// see agent/resources/panel-react/src/lib/agentVersion.ts for the other half\nconst x = 1;";
    assert!(raw.contains("agent/resources/panel-react/src/lib/agentVersion.ts"));
    assert!(
        !decomment(raw).contains("agent/resources/panel-react/src/lib/agentVersion.ts"),
        "decommenting first would erase the thing this half checks — the split is load-bearing"
    );
}
