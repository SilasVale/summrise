//! ONE DOCUMENTED NUMBER, SEVEN IMPLEMENTATIONS — and the documentation is the only thing holding them.
//!
//! `proxies/README.md:13` says "Upstream fetches carry a 30s timeout". Measured (round 122 of the
//! standing goal): that single sentence is implemented THREE ways in seven places —
//!
//!   * the relay's three TypeScript handlers read `SUMMRISE_RELAY_HEADER_TIMEOUT_MS ?? 30000`, so they
//!     are the only ones an operator can move;
//!   * the relay's two legacy JS handlers (`api/zen.js`, `api/proxy.js`) hardcode a module const;
//!   * BOTH zen proxies hardcode their own const, and they are SEPARATE WORKERS — nothing shares a
//!     module across a Worker boundary, so each is independently editable.
//!
//! THE RISK IS NOT THE TIDINESS. A change to one of these is invisible to the other six: the README
//! keeps stating one number, each surface keeps working, and two callers of the same upstream get
//! different budgets depending on which route they took. This gate reads all seven and fails when the
//! numbers stop agreeing, which is the check the comment never was.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28): both print
//! the same closing line, byte for byte, and one site moved to 20000 moves BOTH to the same
//! disagreement body. The measurements are in the commit.
//!
//! MUTATION: change one site's number, or rename a const so a pattern goes stale.
//! RESULT:   the first fails naming the file, the value it states and the documented one; the second
//!           fails the FLOOR — "read N of 7 timeout site(s) — the patterns are stale, so this proves
//!           nothing" — because a gate that reads nothing is worse than no gate.
//!
//! ONE DELIBERATE DIFFERENCE FROM THE JS, stated rather than glossed: the JS reads with paths relative
//! to the PROCESS's working directory, so it only works when run from the repo root. This resolves
//! against the repo root instead, because `cargo test`'s working directory is the crate.

mod common;

use common::{chars, find_seq, skip_ws};
use std::collections::BTreeSet;
use std::fs;

const CANONICAL: u64 = 30000;

/// [file, how to find the number, whether an env override is allowed here]
///
/// **THE TWO SATELLITE SITES ARE RUST FILES SINCE 2026-10-08**, when their JavaScript halves were
/// deleted: the consts did not move, they were PORTED, and the gate follows the implementation rather
/// than the language it used to be written in. Both crates still declare their OWN const — `zen-go`
/// re-exports `zen-us`'s CORS policy but not its timeout — so this is still seven independently
/// editable sites, which is the risk the gate exists for.
const SITES: [(&str, &str, bool); 7] = [
    ("proxies/api-relay/api/git.ts", "env", true),
    ("proxies/api-relay/api/github.ts", "env", true),
    ("proxies/api-relay/api/gform.ts", "env", true),
    ("proxies/api-relay/api/zen.js", "const", false),
    ("proxies/api-relay/api/proxy.js", "const", false),
    ("proxies/zen-go-proxy/worker/src/lib.rs", "rust", false),
    ("proxies/zen-us-proxy/worker/src/lib.rs", "rust", false),
];

struct Streams {
    stdout: String,
    stderr: String,
    failed: bool,
}

/// The digits at `i`, or `None` when there is not at least one — a regex's `(\d+)`.
fn digits_at(c: &[char], i: usize) -> Option<u64> {
    let mut j = i;
    let mut n: u64 = 0;
    while j < c.len() && c[j].is_ascii_digit() {
        n = n
            .saturating_mul(10)
            .saturating_add(c[j] as u64 - '0' as u64);
        j += 1;
    }
    (j > i).then_some(n)
}

/// `SUMMRISE_RELAY_HEADER_TIMEOUT_MS\s*\?\?\s*(\d+)` — the env form, and the only one an operator can
/// move.
fn env_timeout(src: &str) -> Option<u64> {
    let c = chars(src);
    let mut from = 0;
    while let Some(i) = find_seq(&c, "SUMMRISE_RELAY_HEADER_TIMEOUT_MS", from) {
        let mut j = skip_ws(&c, i + "SUMMRISE_RELAY_HEADER_TIMEOUT_MS".chars().count());
        if find_seq(&c, "??", j) == Some(j) {
            j = skip_ws(&c, j + 2);
            if let Some(n) = digits_at(&c, j) {
                return Some(n);
            }
        }
        from = i + 1;
    }
    None
}

/// `const HEADER_TIMEOUT_MS = (\d+);` — the hardcoded form, whose spacing is part of the pattern.
fn const_timeout(src: &str) -> Option<u64> {
    let c = chars(src);
    let at = find_seq(&c, "const HEADER_TIMEOUT_MS = ", 0)?;
    let j = at + "const HEADER_TIMEOUT_MS = ".chars().count();
    let n = digits_at(&c, j)?;
    let after = j + n.to_string().len();
    (c.get(after) == Some(&';')).then_some(n)
}

/// **`pub const HEADER_TIMEOUT_MS: u64 = (\d+);` — the Rust form, and the TYPE ANNOTATION is what makes
/// it a different pattern rather than the same one with a prefix.** The JS `const` matcher is anchored on
/// `const HEADER_TIMEOUT_MS = `, which the Rust line does not contain (`: u64` sits between), so reusing
/// it would read nothing from a ported file and the FLOOR below would be the only thing to notice.
fn rust_timeout(src: &str) -> Option<u64> {
    let c = chars(src);
    let at = find_seq(&c, "pub const HEADER_TIMEOUT_MS: u64 = ", 0)?;
    let j = at + "pub const HEADER_TIMEOUT_MS: u64 = ".chars().count();
    let n = digits_at(&c, j)?;
    let after = j + n.to_string().len();
    (c.get(after) == Some(&';')).then_some(n)
}

fn check() -> Streams {
    let root = common::repo();
    let mut bad: Vec<String> = Vec::new();
    let mut read = 0;
    for (file, form, env_ok) in SITES {
        let Ok(src) = fs::read_to_string(root.join(file)) else {
            bad.push(format!(
                "{file}: cannot be read — the site moved or was deleted"
            ));
            continue;
        };
        let value = match form {
            "env" => env_timeout(&src),
            "rust" => rust_timeout(&src),
            _ => const_timeout(&src),
        };
        let Some(value) = value else {
            // The third column is part of the sentence: a site an operator can move is read with the env
            // form, and a site that is hardcoded is read with its language's const form.
            bad.push(format!(
                "{file}: no header timeout found (expected {})",
                if env_ok {
                    "the env form"
                } else if form == "rust" {
                    "the Rust const"
                } else {
                    "a module const"
                }
            ));
            continue;
        };
        read += 1;
        if value != CANONICAL {
            bad.push(format!(
                "{file}: {value} ms, where the documented budget is {CANONICAL}"
            ));
        }
    }

    // A FLOOR, because a gate that reads nothing because its patterns went stale is worse than no gate:
    // the five `exports-check`-style scans in this repo have each failed this way exactly once.
    if read < SITES.len() {
        let mut stderr = format!(
            "  FAIL read {read} of {} timeout site(s) — the patterns are stale, so this proves nothing:\n",
            SITES.len()
        );
        for b in &bad {
            stderr.push_str(&format!("    {b}\n"));
        }
        return Streams {
            stdout: String::new(),
            stderr,
            failed: true,
        };
    }
    if !bad.is_empty() {
        let mut stderr = format!(
            "  FAIL the header budget disagrees with the documented {CANONICAL} ms, and one number is what the README states:\n"
        );
        for b in &bad {
            stderr.push_str(&format!("    {b}\n"));
        }
        return Streams {
            stdout: String::new(),
            stderr,
            failed: true,
        };
    }
    let units: BTreeSet<String> = SITES
        .iter()
        .map(|(f, _, _)| f.split('/').take(2).collect::<Vec<_>>().join("/"))
        .collect();
    Streams {
        stdout: format!(
            "  ok — {read} header-timeout site(s) across {} deployment unit(s) all state {CANONICAL} ms, the number proxies/README.md documents\n",
            units.len()
        ),
        stderr: String::new(),
        failed: false,
    }
}

#[test]
fn the_header_timeout_is_one_number_across_every_site() {
    let out = check();
    print!("{}", out.stdout);
    if out.failed {
        panic!("{}", out.stderr);
    }
    assert!(
        out.stdout
            .contains("7 header-timeout site(s) across 3 deployment unit(s)"),
        "{}",
        out.stdout
    );
}

// ── the two patterns' own proof: each must be able to say NO, and the floor must bite ───────────────

#[test]
fn both_forms_are_read_and_a_stale_pattern_is_not_a_pass() {
    assert_eq!(
        env_timeout("const t = SUMMRISE_RELAY_HEADER_TIMEOUT_MS ?? 30000;"),
        Some(30000)
    );
    assert_eq!(
        env_timeout("SUMMRISE_RELAY_HEADER_TIMEOUT_MS  ??\n  45000"),
        Some(45000)
    );
    assert_eq!(
        const_timeout("const HEADER_TIMEOUT_MS = 30000;"),
        Some(30000)
    );
    // A RENAMED CONST IS NOT A PASS: the pattern is the spacing and the name together.
    assert_eq!(const_timeout("const HEADER_TIMEOUT = 30000;"), None);
    assert_eq!(const_timeout("const HEADER_TIMEOUT_MS=30000;"), None);
    assert_eq!(
        env_timeout("SUMMRISE_RELAY_HEADER_TIMEOUT_MS ?? undefined"),
        None
    );
    // ...and the digit run stops where the regex's `(\d+)` stops.
    assert_eq!(
        const_timeout("const HEADER_TIMEOUT_MS = 30000;"),
        Some(30000)
    );
    assert_eq!(const_timeout("const HEADER_TIMEOUT_MS = 3000x;"), None);
    // **AND THE RUST FORM IS A DIFFERENT PATTERN RATHER THAN THE SAME ONE WITH A PREFIX** — the trap
    // this pair of assertions exists for. `const_timeout` cannot read the ported line (the type
    // annotation sits where its ` = ` is), so reusing it would read NOTHING from a Rust site and only the
    // floor would notice; and `rust_timeout` cannot read the JS line, so neither can stand in for the
    // other.
    assert_eq!(
        rust_timeout("pub const HEADER_TIMEOUT_MS: u64 = 30000;"),
        Some(30000)
    );
    assert_eq!(rust_timeout("const HEADER_TIMEOUT_MS = 30000;"), None);
    assert_eq!(
        const_timeout("pub const HEADER_TIMEOUT_MS: u64 = 30000;"),
        None
    );
    // A renamed or retyped const is not a pass either, and the digit run stops where `(\d+)` stops.
    assert_eq!(
        rust_timeout("pub const HEADER_TIMEOUT_MS: u32 = 30000;"),
        None
    );
    assert_eq!(
        rust_timeout("pub const HEADER_TIMEOUT_MS: u64 = 3000x;"),
        None
    );
}

#[test]
fn the_unit_count_is_derived_from_the_paths() {
    let units: BTreeSet<String> = SITES
        .iter()
        .map(|(f, _, _)| f.split('/').take(2).collect::<Vec<_>>().join("/"))
        .collect();
    assert_eq!(units.len(), 3, "three deployment units: {units:?}");
    assert!(units.contains("proxies/api-relay"));
    assert!(units.contains("proxies/zen-go-proxy"));
    assert!(units.contains("proxies/zen-us-proxy"));
}
