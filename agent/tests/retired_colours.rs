//! A COLOUR THAT WAS REPLACED MAY NOT COME BACK ANYWHERE.
//!
//! WHY (round 236). The device's status page carried `#d9480f` long after the stylesheets
//! replaced it with `#bf3a0a`. Nothing caught it, because the palette lives in FOUR places —
//! the panel's CSS, the console's CSS, the extension's CSS, and a Rust string constant that
//! renders the status page — and the checks all looked at the first three. Measured on the
//! rendered pair: `#d9480f` on `#ffefe5` is 3.83 against the 4.5 AA needs for text, so the
//! code chips on that page were the only sub-AA text in the product.
//!
//! WHAT THIS IS: a list of values that were REPLACED, each with the reason it was replaced,
//! checked against every source file. It is deliberately not a palette check — the palettes
//! are allowed to differ, which is a brand question recorded in the operator's inbox. What
//! they are not allowed to do is carry a value that was retired for a measured reason.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/retired-colours-check.mjs` → exit 0, "retired colours: ok — N files
//!     · 1 retired value(s) · 3 known survivors, waived with reasons".
//!   * `cargo test -p summrise-agent --test retired_colours` → ok, the SAME file count.
//!   * the mutation below → the JS gate exits 1 naming the file and the reason, and this file
//!     fails with the same two lines.
//!
//! MUTATION: put a retired value back anywhere outside a comment — `const X: &str = "#d9480f";`
//!           in `agent/src/`.
//! RESULT:   fails naming the file and the measurement that retired it. **The strip runs
//!           FIRST**, because the gate's own first run failed on ten files that merely
//!           recorded the retirement — a gate that deletes its reasons is worse than no gate.

use std::fs;
use std::path::{Path, PathBuf};

/// `cargo test` runs from the CRATE root, not the repo root — a gate that resolves a
/// relative path passes locally and fails in CI.
const CRATE: &str = env!("CARGO_MANIFEST_DIR");

fn repo() -> PathBuf {
    Path::new(CRATE)
        .parent()
        .expect("the crate lives one level below the repo root")
        .to_path_buf()
}

/// RETIRED, with the measurement or decision that retired it.
const RETIRED: [(&str, &str); 1] = [(
    "#d9480f",
    "the accent before round 79: white on it measured 4.30 and it as text on #fafafa measured 4.12, \
     both under AA. Replaced by #bf3a0a (5.49 / 4.90).",
)];

const SCAN: [&str; 7] = [
    "agent/src",
    "agent/resources/panel-react/src",
    "agent/resources/panel-react/src/styles",
    "gateway/ui/src",
    "gateway/public",
    "extension",
    "index/public",
];

/// SURVIVORS, WAIVED WITH REASONS — the same way the sweeps waive what they cannot judge.
/// These are REAL uses of the retired value that the first run found, and they are not fixed
/// here because each needs a measurement across two themes before a replacement can be
/// chosen: `#d9480f` on `#ffefe5` is 3.83, under the 4.5 AA wants, but the same value on a
/// DARK soft surface may pass. Round 236 records them rather than guessing.
const WAIVED: [(&str, &str, &str); 3] = [
    ("tokens.css", "--accent-ink", "KEPT AFTER MEASUREMENT (round 237): every use is a GRAPHIC — hover fills, dot backgrounds, a border — where the need is 3:1, and it measures 4.12 light / 3.78 dark. Round 236 called this a text use; the sheets say otherwise and the numbers agree."),
    ("TerminalPane.tsx", "cursor", "KEPT AFTER MEASUREMENT (round 237): a cursor is a graphic on the terminal's own #131418, where #d9480f measures 4.28 and the current accent would measure 3.35 — the replacement would be WORSE on a dark background."),
    ("themeContrast.test.ts", "prose", "a template literal QUOTING the old measurements as history, not a use — the test documents what the retired value measured."),
];

fn skipped(p: &Path) -> bool {
    let s = p.to_string_lossy();
    ["node_modules", ".git", "target", "dist", "assets/index-"]
        .iter()
        .any(|k| s.contains(k))
}

fn is_source(name: &str) -> bool {
    matches!(
        name.rsplit_once('.').map(|(_, e)| e),
        Some("rs" | "ts" | "tsx" | "js" | "jsx" | "css" | "html" | "json" | "md")
    )
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let full = e.path();
        if skipped(&full) {
            continue;
        }
        if full.is_dir() {
            walk(&full, out);
        } else if full
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(is_source)
        {
            out.push(full);
        }
    }
}

/// COMMENTS ARE NOT USES. The first version of this failed on ten files — and every one of
/// them mentioned the value in a comment RECORDING its retirement ("was #d9480f — white on it
/// measured 4.30"). That history is exactly what should stay; a gate that deletes its own
/// reasons is worse than no gate. So the scan reads the code with comments removed: a comment
/// is evidence, a value in a rule or a string is a use.
fn strip_comments(text: &str) -> String {
    let s: Vec<char> = text.chars().collect();
    let mut blocked = String::with_capacity(text.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == '/' && s.get(i + 1) == Some(&'*') {
            let mut j = i + 2;
            while j + 1 < s.len() && !(s[j] == '*' && s[j + 1] == '/') {
                j += 1;
            }
            i = if j + 1 < s.len() { j + 2 } else { s.len() };
            blocked.push(' ');
        } else {
            blocked.push(s[i]);
            i += 1;
        }
    }
    // js/ts/rust line comments, then rust doc-comment bodies (`^ *\*`), in that order —
    // the order the JS gate uses, because `//` removal can expose a leading `*`.
    blocked
        .lines()
        .map(|line| {
            let cut = match line.find("//") {
                Some(k) => &line[..k],
                None => line,
            };
            if cut.trim_start().starts_with('*') {
                String::new()
            } else {
                cut.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn waived(rel: &str, text: &str) -> bool {
    WAIVED
        .iter()
        .any(|(file, token, _)| rel.ends_with(file) && (*token == "prose" || text.contains(*token)))
}

fn scan(root: &Path) -> (usize, Vec<(String, String, String)>) {
    let mut files = Vec::new();
    for d in SCAN {
        walk(&root.join(d), &mut files);
    }
    let mut hits = Vec::new();
    for f in &files {
        // The gate's own source is not a use — and it is in `agent/tests/`, outside SCAN,
        // which this guard states rather than relies on.
        if f.to_string_lossy().ends_with("retired_colours.rs") {
            continue;
        }
        let Ok(raw) = fs::read_to_string(f) else {
            continue;
        };
        let text = strip_comments(&raw);
        let rel = f
            .strip_prefix(root)
            .unwrap_or(f)
            .to_string_lossy()
            .to_string();
        for (value, why) in RETIRED {
            if text.to_lowercase().contains(&value.to_lowercase()) && !waived(&rel, &text) {
                hits.push((rel.clone(), value.to_string(), why.to_string()));
            }
        }
    }
    (files.len(), hits)
}

#[test]
fn no_retired_colour_is_back_in_the_source() {
    let root = repo();
    let (files, hits) = scan(&root);
    // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN.
    assert!(
        files >= 100,
        "retired colours: FAILED — the scan read only {files} files, so it proves nothing"
    );
    assert!(
        hits.is_empty(),
        "retired colours: FAILED — a value that was replaced for a measured reason is back:\n{}\n\n\
         Either restore the current value, or retire the new one with the measurement that \
         decided it and add it to RETIRED.",
        hits.iter()
            .map(|(f, v, w)| format!("  {f}  {v}\n      retired because {w}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    println!(
        "retired colours: ok — {files} files · {} retired value(s) · {} known survivors, waived with reasons",
        RETIRED.len(),
        WAIVED.len()
    );
}

#[test]
fn the_strip_keeps_a_comment_that_records_the_retirement() {
    // The gate's own reason for existing: ten files mentioned the value in a comment and the
    // first version failed all ten. A comment is evidence; a string is a use.
    let css = "/* was #d9480f — white on it measured 4.30 */\n.a { color: #bf3a0a; }";
    assert!(!strip_comments(css).contains("#d9480f"));
    let rust = "// retired: #d9480f\nconst X: &str = \"#d9480f\";";
    let stripped = strip_comments(rust);
    assert!(!stripped.contains("// retired"));
    assert!(
        stripped.contains("#d9480f"),
        "a value in a string is a use, not history"
    );
}
