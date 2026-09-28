//! A `var()` THAT RESOLVES TO NOTHING SILENTLY DELETES THE DECLARATION.
//!
//! This is not a warning — it is how CSS works. `color: var(--x)` where `--x` is
//! undefined makes the whole declaration INVALID AT COMPUTED-VALUE TIME, so the
//! property is dropped and the element inherits. Nothing errors, nothing logs, and
//! the page renders — just not the way the rule says.
//!
//! FOUND THE HARD WAY. Round 43's console Models page used three names that exist in
//! the PANEL's token set and not in the console's:
//!     --accent-soft      --accent-on-soft      --danger
//! so the AMD lane had NO lane stripe at all and the "current model" chip was not
//! highlighted — while every contrast sweep reported 0 under AA, because a dropped
//! `color` leaves an inherited colour that measures fine. A contrast probe cannot see
//! a declaration that never applied. It was invisible to the token contract too: that
//! check compares names BOTH frontends declare, and these were declared by neither.
//!
//! So: every custom property USED in a frontend must be DEFINED in that frontend.
//! A `var(--x, fallback)` is reported separately rather than failed — the fallback is
//! real and the property does apply — but it is still a name the stylesheet does not
//! own, which is how round 43 found 38 dead fallbacks in the panel.
//!
//! WHY IT IS RUST NOW (2026-09-28). The operator's instruction is that the gates move
//! into `cargo test`, which is already a CI job. The division of labour is the one
//! `agent/tests/installer_integrity.rs` states: a gate that reads SOURCES and asserts
//! invariants over them belongs here, where the types force "I did not read it" to be
//! handled rather than defaulted.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/custom-prop-check.mjs` → exit 0, and three counts:
//!     console 73 defined / 70 used, panel 115 / 107, landing 23 / 21.
//!   * this file → ok, with the SAME three counts, to the digit.
//!   * a planted `color: var(--accent-soft)` in `gateway/ui/src/styles/globals.css` — a
//!     name the panel owns and the console does not, round 43's exact defect → the JS
//!     gate exits 1 ("FAIL console (gateway/ui): 1 custom property is USED BUT NOT
//!     DEFINED … --accent-soft") and this file fails with the same sentence and the same
//!     name. Restoring the sheet turns both green again.
//!
//! MUTATION: add `color: var(--accent-soft);` to any rule in
//! `gateway/ui/src/styles/` (a property the panel defines and the console does not).
//! RESULT:   `cargo test -p summrise-agent custom_prop` fails with
//!           "console (gateway/ui): 1 custom property USED BUT NOT DEFINED ... --accent-soft",
//!           and the same edit makes the pre-migration JS gate exit 1 naming the same name.
//!           Restoring the sheet turns both green again.
//!
//! WHAT THIS DOES NOT SEE, stated rather than implied: a `var()` whose FALLBACK contains a
//! nested `)` (`var(--x, rgb(0 0 0))`) is not a bare use, so it is reported in the
//! fallback-only list rather than as a dangling reference — the JS gate could not see it at
//! all. The verdict is the same either way, because a fallback-only name is a note and not
//! a failure.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// `cargo test` runs from the CRATE root, not the repo root, so every path is resolved
/// from here — a gate that resolves a relative path passes locally and fails in CI.
const CRATE: &str = env!("CARGO_MANIFEST_DIR");

fn repo(rel: &str) -> PathBuf {
    Path::new(CRATE)
        .parent()
        .expect("the crate lives one level below the repo root")
        .join(rel)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Every `.css` file in a directory, as one blob: tokens and consumers often live apart.
fn css_dir(rel: &str) -> String {
    let dir = repo(rel);
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .map(|e| e.expect("a readable directory entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "css"))
        .collect();
    // SORTED, because a `readdir` order is not a contract and a gate whose message moves
    // between runs is a gate nobody can diff.
    files.sort();
    files.iter().map(|p| read(p)).collect::<Vec<_>>().join("\n")
}

/// The landing page's CSS.
///
/// IT WAS A `<style>` BLOCK INSIDE A JS TEMPLATE LITERAL until the landing migrated to Rust
/// (2026-09-28), which is why the crop below existed; the sheet is now
/// `index/landing/assets/page.css`, embedded into the document with `include_str!`. The
/// migration left this function returning an EMPTY string, and the gate said so in the right
/// words — "landing (index/page.js): parsed only 0 definitions / 0 uses — the parser read the
/// wrong thing" — rather than passing on a sheet it had not read.
fn landing_css() -> String {
    read(&repo("index/landing/assets/page.css"))
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
}

/// Read a `--name` starting at `i` (which must be the first `-`), returning it and the
/// index just past it.
fn name_at(s: &[char], i: usize) -> Option<(String, usize)> {
    if s.get(i) != Some(&'-') || s.get(i + 1) != Some(&'-') {
        return None;
    }
    let mut j = i + 2;
    while j < s.len() && is_name_char(s[j]) {
        j += 1;
    }
    if j == i + 2 {
        return None;
    }
    Some((s[i..j].iter().collect(), j))
}

/// Defined custom properties: `--x` at a DECLARATION position (`(?:^|[;{\s])--x\s*:`).
fn defined(css: &str) -> BTreeSet<String> {
    let s: Vec<char> = css.chars().collect();
    let mut out = BTreeSet::new();
    for i in 0..s.len() {
        let boundary = i == 0 || matches!(s[i - 1], ';' | '{' | '}' | ' ' | '\t' | '\n' | '\r');
        if !boundary {
            continue;
        }
        if let Some((name, j)) = name_at(&s, i) {
            let mut k = j;
            while k < s.len() && s[k].is_whitespace() {
                k += 1;
            }
            if s.get(k) == Some(&':') {
                out.insert(name);
            }
        }
    }
    out
}

/// `var(--x)` without a fallback — the fatal form — and `var(--x, …)`, which applies but
/// names a property this stylesheet does not own.
fn var_uses(css: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let s: Vec<char> = css.chars().collect();
    let mut bare = BTreeSet::new();
    let mut fallback = BTreeSet::new();
    let needle: Vec<char> = "var(".chars().collect();
    for i in 0..s.len() {
        if s[i..].len() < needle.len() || s[i..i + needle.len()] != needle[..] {
            continue;
        }
        let mut j = i + needle.len();
        while j < s.len() && s[j].is_whitespace() {
            j += 1;
        }
        let Some((name, k)) = name_at(&s, j) else {
            continue;
        };
        let mut m = k;
        while m < s.len() && s[m].is_whitespace() {
            m += 1;
        }
        match s.get(m) {
            Some(')') => {
                bare.insert(name);
            }
            Some(',') => {
                fallback.insert(name);
            }
            _ => {}
        }
    }
    (bare, fallback)
}

struct Frontend {
    name: &'static str,
    css: fn() -> String,
    /// A parser that reads nothing must not report a pass (round 33's rule).
    min_defined: usize,
    min_used: usize,
}

const FRONTENDS: [Frontend; 3] = [
    Frontend {
        name: "console (gateway/ui)",
        css: || css_dir("gateway/ui/src/styles"),
        min_defined: 20,
        min_used: 5,
    },
    Frontend {
        name: "panel (panel-react)",
        css: || css_dir("agent/resources/panel-react/src/styles"),
        min_defined: 40,
        min_used: 5,
    },
    Frontend {
        name: "landing (index/landing/assets/page.css)",
        css: landing_css,
        min_defined: 20,
        min_used: 5,
    },
];

fn check(frontends: &[Frontend]) -> Result<Vec<String>, String> {
    let mut failures = Vec::new();
    let mut notes = Vec::new();
    for f in frontends {
        let css = (f.css)();
        let dec = defined(&css);
        let (bare, fb) = var_uses(&css);
        if dec.len() < f.min_defined || bare.len() < f.min_used {
            failures.push(format!(
                "{}: parsed only {} definitions / {} uses — the parser read the wrong thing",
                f.name,
                dec.len(),
                bare.len()
            ));
            continue;
        }
        let broken: Vec<&String> = bare.iter().filter(|v| !dec.contains(*v)).collect();
        if broken.is_empty() {
            notes.push(format!(
                "  {}: {} defined, {} used, 0 dangling",
                f.name,
                dec.len(),
                bare.len()
            ));
        } else {
            failures.push(format!(
                "{}: {} custom propert{} USED BUT NOT DEFINED.\n  Each one makes its declaration INVALID, so the property is DROPPED and the element inherits:\n{}",
                f.name,
                broken.len(),
                if broken.len() == 1 { "y is" } else { "ies are" },
                broken
                    .iter()
                    .map(|v| format!("    {v}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        let fb_only: Vec<&String> = fb.iter().filter(|v| !dec.contains(*v)).collect();
        if !fb_only.is_empty() {
            notes.push(format!(
                "  note {}: {} used only WITH a fallback ({}…)",
                f.name,
                fb_only.len(),
                fb_only
                    .iter()
                    .take(4)
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if failures.is_empty() {
        Ok(notes)
    } else {
        Err(failures.join("\n"))
    }
}

#[test]
fn every_var_is_defined_in_its_own_frontend() {
    match check(&FRONTENDS) {
        Ok(notes) => {
            println!("custom-property check: every var() used is defined in its own frontend");
            for n in notes {
                println!("{n}");
            }
        }
        Err(e) => panic!(
            "{e}\n\ncustom-property check: a `var()` that resolves to nothing deletes its declaration \
             silently. Define the name in the frontend that uses it, or give it a fallback."
        ),
    }
}

// ── the scanner's own proof: a passing case, a failing case, and the boundary ───────
//
// The gate above reads whatever the tree happens to hold TODAY, so it cannot prove the
// scanner distinguishes a definition from a use. These do, on planted input.

#[test]
fn the_scanner_tells_a_definition_from_a_use() {
    let css = ":root { --accent: #fff; --on-accent:#000; }\n.a { color: var(--accent); }";
    let dec = defined(css);
    assert!(
        dec.contains("--accent"),
        "a `--x:` declaration is a definition"
    );
    assert!(
        dec.contains("--on-accent"),
        "no space before the colon still counts"
    );
    let (bare, fb) = var_uses(css);
    assert_eq!(
        bare.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["--accent"]
    );
    assert!(fb.is_empty(), "`var(--x)` is not the fallback form");
}

#[test]
fn the_scanner_separates_a_bare_use_from_one_with_a_fallback() {
    let (bare, fb) = var_uses(".a{color:var(--gone);background:var(--also-gone, red)}");
    assert!(bare.contains("--gone"));
    assert!(!bare.contains("--also-gone"));
    assert!(fb.contains("--also-gone"));
}

#[test]
fn a_bare_use_of_an_undefined_name_is_the_failure() {
    let f = Frontend {
        name: "planted",
        css: || ":root{--here:1px}\n.a{color:var(--here);width:var(--absent)}".to_string(),
        min_defined: 1,
        min_used: 1,
    };
    let err = check(std::slice::from_ref(&f)).expect_err("a dangling var() must fail");
    assert!(
        err.contains("--absent"),
        "the message must name the name: {err}"
    );
    assert!(
        !err.contains("--here"),
        "a defined name is not a finding: {err}"
    );
}

#[test]
fn a_parser_that_reads_nothing_is_not_a_pass() {
    let f = Frontend {
        name: "empty",
        css: String::new,
        min_defined: 20,
        min_used: 5,
    };
    let err = check(std::slice::from_ref(&f)).expect_err("silence must not pass");
    assert!(err.contains("read the wrong thing"), "{err}");
}
