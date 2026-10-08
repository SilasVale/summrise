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

// ── the character-space helpers the PATTERN gates share ────────────────────────────────────────────
//
// WHY THESE LIVE HERE AND NOT IN EACH GATE. Four gates read a pattern out of a source file
// (`device_verdict`, `proxy_cors_parity`, `proxy_timeout_parity`, `exports_check`) and each needs
// "find this literal, skip the whitespace a regex's `\s*` would skip, check a `\b`". Written per file
// that is four copies of one rule in the new language — the defect `decomment` was extracted to remove
// on the JS side — so it is one definition here, and the JS regex each helper mirrors is named on it.

/// The text as characters. JavaScript indexes strings by UTF-16 code unit and Rust by byte, so a
/// pattern gate works in CHARACTER space on both sides and the two agree.
pub fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

/// The first index at or after `from` where `pat` sits. Every needle these gates use is ASCII.
pub fn find_seq(c: &[char], pat: &str, from: usize) -> Option<usize> {
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

/// JavaScript's `\s`: the Unicode White_Space property PLUS U+FEFF, which `char::is_whitespace` does
/// not have. The difference is one character wide and this is where it would have been.
pub fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// What a regex's `\s*` consumes: every whitespace character at the cursor, newlines included.
pub fn skip_ws(c: &[char], mut i: usize) -> usize {
    while i < c.len() && is_js_space(c[i]) {
        i += 1;
    }
    i
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

/// **`//` TO END OF LINE, UNLESS IT IS INSIDE A JSON STRING** — the strip a JSONC config needs, and the
/// one thing `decomment` above cannot do for it.
///
/// `decomment_line` keeps a URL by looking at the character BEFORE the `//` (a colon), which is a
/// heuristic about prose. A JSON string can carry `//` with no colon anywhere near it, and a strip that
/// cuts there hands the parser half a value: measured on `"main": "worker//build/index.js"`, the naive
/// strip leaves `"worker` and the config stops parsing. Block comments are NOT stripped here: no config in
/// this repository uses one, and a config that did would fail to parse and be REPORTED rather than
/// silently mis-read.
pub fn strip_jsonc_comments(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for n in chars.by_ref() {
                if n == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Every tracked `*wrangler*.jsonc` as (repo-relative path, text), **WITH THE SOURCE VIEWER MIRROR
/// EXCLUDED**: `gateway/public/code/files/…` is a byte-for-byte copy of the console's sources served by
/// the /code/ viewer, so it contains a `wrangler.jsonc` that is not a deploy target — reading it would
/// make every config gate report the same file twice, once as a worker and once as its own mirror.
pub fn worker_configs() -> Vec<(String, String)> {
    let root = repo();
    git_ls_files_all()
        .into_iter()
        .filter(|f| f.ends_with(".jsonc") && f.contains("wrangler"))
        .filter(|f| !f.starts_with("gateway/public/code/files/"))
        .filter_map(|f| {
            let text = fs::read(root.join(&f)).ok()?;
            Some((f, String::from_utf8_lossy(&text).into_owned()))
        })
        .collect()
}

/// A config's directory plus its `main`, with `.` and `..` resolved — a config's `main` is relative to the
/// config, and the gates that ask "is this entry a TRACKED file?" need the repo-relative answer.
pub fn resolve(dir: &str, main: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in dir.split('/').chain(main.split('/')) {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
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

/// `git ls-files` with NO pathspec — every tracked path, in the index's own order, which is the list
/// `production-host-check` walks. Its own function rather than `git_ls_files("")`, because an empty
/// pathspec is a question about git's matching rules and this is a question about the index.
pub fn git_ls_files_all() -> Vec<String> {
    let out = std::process::Command::new("git")
        .args(["ls-files"])
        .current_dir(repo())
        .output()
        .unwrap_or_else(|e| panic!("cannot run git ls-files: {e}"));
    assert!(out.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&out.stdout)
        .split('\n')
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}
