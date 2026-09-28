//! HELPERS SHARED BY THE MIGRATED GATES — ONE DEFINITION, the way `scripts/test/lib/decomment.mjs`
//! is one definition on the JS side.
//!
//! WHY THIS MODULE EXISTS. `scripts/test/lib/decomment.mjs` was extracted because rounds 135 and 136
//! each wrote the comment strip into the gate that needed it, "which made three copies of one rule —
//! the exact defect this objective spends its rounds removing, committed by the gates that enforce
//! it". Moving the gates to Rust reproduces that hazard in a new language unless the strip is shared
//! here, so `decomment` has exactly one Rust definition and `wire_fields.rs` and
//! `contract_vocabulary.rs` both call it.
//!
//! `cargo test` does NOT treat a subdirectory of `tests/` as a test target (only `tests/*.rs` and
//! `tests/<name>/main.rs` are targets), so nothing here runs on its own; each gate that wants it says
//! `mod common;`. `dead_code` is allowed because a helper used by one gate is unused in the other's
//! binary, and that is not a defect to report on every build.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

/// `cargo test` runs from the CRATE root, not the repo root.
pub const CRATE: &str = env!("CARGO_MANIFEST_DIR");

pub fn repo() -> PathBuf {
    Path::new(CRATE)
        .parent()
        .expect("the crate lives one level below the repo root")
        .to_path_buf()
}

pub fn read(rel: &str) -> String {
    let p = repo().join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

pub fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `/\/\*[\s\S]*?\*\//g` — block comments, non-greedy to the FIRST `*/`. With no `*/` the regex does
/// not match at all and the `/*` stays in the text, which is what the JS does.
pub fn strip_block_comments(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '/' && c.get(i + 1) == Some(&'*') {
            let mut j = i + 2;
            let mut close = None;
            while j + 1 < c.len() {
                if c[j] == '*' && c[j + 1] == '/' {
                    close = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(k) = close {
                i = k + 2;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// The per-line half of `decomment.mjs`: a whole-line `//` comment goes entirely; a trailing `// …`
/// goes only when the character before it is not a colon, because `//` also opens a URL.
pub fn decomment_line(line: &str) -> String {
    if line.trim_start().starts_with("//") {
        return String::new();
    }
    let c: Vec<char> = line.chars().collect();
    let mut p = 0;
    while p + 1 < c.len() {
        if c[p] == '/' && c[p + 1] == '/' && (p == 0 || c[p - 1] != ':') {
            return c[..p].iter().collect();
        }
        p += 1;
    }
    line.to_string()
}

/// A COMMENT IS NOT A PRODUCER, NOR A DERIVATION (rounds 135-137).
///
/// For the field gates a comment that satisfied the scan was a FALSE NEGATIVE — a requirement met by
/// prose, so a deleted producer could be kept alive by a comment. For `one-derivation` it was a FALSE
/// POSITIVE — a gate flagging its own documentation. The direction of error differs; the strip does
/// not.
pub fn decomment(text: &str) -> String {
    strip_block_comments(text)
        .split('\n')
        .map(decomment_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Recursive file walk. `readdir` order is the OS's, which is what Node's `readdirSync` returns too —
/// neither sorts, so the two implementations see the same order.
pub fn walk(dir: &Path, test: &dyn Fn(&str) -> bool, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for e in entries {
        let p = e.expect("a readable directory entry").path();
        if p.is_dir() {
            walk(&p, test, out);
        } else {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if test(&name) {
                out.push(p);
            }
        }
    }
}

pub fn files_under(dir: &str, test: &dyn Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(&repo().join(dir), test, &mut out);
    out
}

pub fn decommented_corpus(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| {
            decomment(
                &fs::read_to_string(p)
                    .unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display())),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn rel_to_repo(p: &Path) -> String {
    p.strip_prefix(repo())
        .expect("a walked path is under the repo root")
        .to_string_lossy()
        .replace('\\', "/")
}

/// `git ls-files <dir>`, split and filtered — the authoritative, ordered file list that
/// `production-host-check` and `contract-vocabulary-check` both use. A hand-rolled walk recursed
/// until the stack blew (a symlink, most likely); `git ls-files` cannot loop.
pub fn git_ls_files(dir: &str) -> Vec<String> {
    let out = std::process::Command::new("git")
        .args(["ls-files", dir])
        .current_dir(repo())
        .output()
        .unwrap_or_else(|e| panic!("cannot run git ls-files {dir}: {e}"));
    assert!(out.status.success(), "git ls-files {dir} failed");
    String::from_utf8_lossy(&out.stdout)
        .split('\n')
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}
