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

// ── THE SPAWNING GATES ─────────────────────────────────────────────────────────────────────────────
//
// FOUR GATES THAT CAME OUT OF `scripts/test/*.mjs` SPAWN A TOOLCHAIN — `npm run build`, `npm test`,
// `node <sweep> --emit`, `git status` — and each one needs the same three things: a child, its two
// streams captured SEPARATELY, and its exit code. `scripts/test/lib/` had no such module on the JS side
// because `execFileSync` is one line there; here it is four, and four copies of four lines is the defect
// `decomment` was extracted to remove.

/// A finished child process. Both streams are kept apart because the gates that came across read them
/// apart: `agent/tests/console_assets.rs` prints ONLY the build's stdout tail when it fails, and
/// `agent/tests/console_smoke.rs` falls back to stderr only when stdout named no error.
#[derive(Debug)]
pub struct Ran {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Ran {
    /// `e.status ?? 1`, which is what every ported message prints for a failed child.
    pub fn code(&self) -> i32 {
        self.status.unwrap_or(1)
    }

    pub fn ok(&self) -> bool {
        self.status == Some(0)
    }

    /// stdout then stderr — the concatenation `agent/tests/console_smoke.rs` makes when a smoke died
    /// before printing a failing line.
    pub fn both(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

/// A child process, described the way `execFileSync(program, args, { cwd, env })` describes one.
pub struct Spawn {
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
    envs: Vec<(String, String)>,
}

impl Spawn {
    pub fn new(program: &str) -> Self {
        Self {
            program: program.to_string(),
            args: Vec::new(),
            cwd: repo(),
            envs: Vec::new(),
        }
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_string()));
        self
    }

    /// A path is passed as a string, exactly as `execFileSync("node", [s, built])` passes it.
    pub fn arg_path(self, p: &Path) -> Self {
        self.args([p.to_string_lossy().to_string()])
    }

    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = dir.into();
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.envs.push((key.to_string(), value.to_string()));
        self
    }

    pub fn run(self) -> Ran {
        let out = std::process::Command::new(&self.program)
            .args(&self.args)
            .current_dir(&self.cwd)
            .envs(self.envs.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap_or_else(|e| {
                panic!(
                    "cannot run {} in {}: {e} — the gate needs it on PATH",
                    self.program,
                    self.cwd.display()
                )
            });
        Ran {
            status: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

/// `execFileSync("git", a, { cwd: ROOT, encoding: "utf8" }).trim()` — every ported shell-out to git
/// wants exactly this, and the trim is load-bearing: `git status --porcelain` on a clean tree answers
/// "\n" for an empty diff, and the gates compare that answer to "".
pub fn git(args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo())
        .output()
        .unwrap_or_else(|e| panic!("cannot run git {args:?}: {e}"));
    if !out.status.success() {
        panic!(
            "git {args:?} failed (rc={}): {}",
            out.status.code().unwrap_or(1),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A string's LAST `n` characters — JavaScript's `s.slice(-n)`, which the ported messages use to show
/// the tail of a failed build. Character-wise, because a byte slice would split a UTF-8 sequence and
/// panic on a build that printed a box-drawing character.
pub fn tail(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

/// Is a package's dependency tree installed? The question every ported toolchain gate asks before it
/// declares "this host cannot run me", and the one round 191 is about: `pack-chain` runs every gate
/// through `all-gates.bash` and installs only what it needs, so a missing `node_modules` is a fact
/// about the HOST and never about the thing being measured.
pub fn has_node_modules(rel: &str) -> bool {
    repo().join(rel).join("node_modules").is_dir()
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
