//! A MODULE'S PUBLIC SURFACE SHOULD BE WHAT SOMEBODY USES.
//!
//! WHY THIS EXISTS (round 21 of the standing goal). The objective says prune whatever stops earning its
//! place, and the previous round pruned eighteen tokens nothing read. The same question was never asked
//! of the CODE. Measured: 305 exports in the panel and 43 in the console, of which 67 + 9 were
//! referenced NOWHERE in the other files and 34 + 0 were referenced only by tests — and triaging the 67
//! found the distinction that makes this check worth having:
//!
//!   * 24 of them were used INSIDE their own file — the `export` keyword was the only thing that made
//!     them public. Nothing breaks when it goes, and the module stops promising something it never
//!     offered. (A constant like `URGENT_MS` is the file's business; exporting it invites a second copy
//!     elsewhere.)
//!   * 3 were used nowhere at all: the panel's `hasTransport()`, the console's `<StatusChip>` and its
//!     `getLang()`.
//!
//! `<StatusChip>` is the reason this file exists rather than the deletion alone: its CSS family stayed
//! behind, 35 lines of `.chip` / `.chip .dot` / four tone rules that nothing could render — INCLUDING a
//! `.chip.off .dot` using the very `--text-faint` ink the console's marks gate had already caught at
//! 2.46:1 on the live `.sig-dot.off`. The defect existed twice and only the live copy was fixed,
//! because nothing connected "this component is gone" to "these rules can never match".
//!
//! TESTS COUNT AS USE. An export only a test reads is a deliberate testing seam on this codebase (34 of
//! them in the panel), not dead weight, and a check that called them dead would be turned off within a
//! round.
//!
//! WHAT IT CANNOT SEE: a member reached dynamically (`mod[name]`, a string-built import). Nothing in
//! either UI does that today; if something starts, name it in `ALLOWED` with the reason.
//!
//! MUTATION: export a name and use it only inside its own file (`export const URGENT_MS` in
//!           ApprovalGate), or export one used nowhere at all.
//! RESULT:   exit 1 either way: "is exported but used only inside its own file — drop the `export`", or
//!           "is exported and used NOWHERE — delete it". THE MUTATION BLOCK IS THE JS'S OWN, moved here
//!           from the ledger table in landing 4b — this is the one gate of the batch that already
//!           carried a proof, and the proof is reproduced rather than replaced.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28): both print
//! the same closing line with the same export count, byte for byte; a planted `export const
//! PLANTED_UNUSED` in the panel moves BOTH to the same failure body naming it. The measurements are in
//! the commit.
//!
//! COMMENTS ARE STRIPPED FIRST, and this gate needed it for a reason of its own: it counts a bare
//! `\bname\b` ANYWHERE as a use, so PROSE kept dead exports alive. `useSessionEvents` survived six
//! audits on two comments that merely mentioned it, and `evicted.ts`'s `humanIdle` was matched by an
//! unrelated same-named local in another file. Stripping also stops a commented-out `export` being
//! counted as a declaration. NOTE THE STRIP IS NOT `decomment`: this one removes block comments and
//! WHOLE-LINE `//` comments only, because a trailing `//` here would eat the `https://` in a fixture —
//! it is the JS's own narrower rule, reproduced rather than unified.

mod common;

use common::strip_block_comments;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const UIS: [(&str, &str); 2] = [
    ("panel", "agent/resources/panel-react/src"),
    ("console", "gateway/ui/src"),
];

/// Deliberately public despite no importer. Empty, and each entry would need its reason.
const ALLOWED: [&str; 0] = [];

struct Streams {
    stdout: String,
    stderr: String,
    failed: bool,
}

/// `/^\s*\/\/.*$/gm` — a WHOLE-LINE comment goes, a trailing one does not (see the header).
fn strip_whole_line_comments(s: &str) -> String {
    s.split('\n')
        .map(|line| {
            if line.trim_start().starts_with("//") {
                String::new()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_comments(s: &str) -> String {
    strip_whole_line_comments(&strip_block_comments(s))
}

/// Every `.ts`/`.tsx` under `dir`, excluding `.d.ts`, `node_modules`, `dist` and `build` — in the
/// ORDER the filesystem returns them, which is what the JS's `readdirSync` walk produces too.
fn source_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name == "node_modules" || name == "dist" || name == "build" {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if (name.ends_with(".ts") || name.ends_with(".tsx")) && !name.ends_with(".d.ts")
            {
                out.push(p);
            }
        }
    }
    out
}

/// `/^export\s+(?:async\s+)?(?:function|const|class|enum|type|interface)\s+([A-Za-z_][\w]*)/gm` — the
/// declaration, at a LINE START, in source order.
fn declared_exports(text: &str) -> Vec<String> {
    let c = common::chars(text);
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if i > 0 && c[i - 1] != '\n' {
            i += 1;
            continue;
        }
        let mut j = i;
        let mut ok = true;
        for kw in ["export"] {
            if common::find_seq(&c, kw, j) != Some(j) {
                ok = false;
                break;
            }
            j += kw.chars().count();
        }
        if ok {
            let after_ws = common::skip_ws(&c, j);
            if after_ws == j {
                ok = false;
            } else {
                j = after_ws;
                if common::find_seq(&c, "async", j) == Some(j) {
                    let k = common::skip_ws(&c, j + 5);
                    if k == j + 5 {
                        ok = false;
                    } else {
                        j = k;
                    }
                }
            }
        }
        if ok {
            let mut matched = None;
            for kw in ["function", "const", "class", "enum", "type", "interface"] {
                if common::find_seq(&c, kw, j) == Some(j) {
                    matched = Some(kw.chars().count());
                    break;
                }
            }
            match matched {
                None => ok = false,
                Some(len) => {
                    let k = common::skip_ws(&c, j + len);
                    if k == j + len {
                        ok = false;
                    } else {
                        j = k;
                    }
                }
            }
        }
        if ok
            && c.get(j)
                .is_some_and(|ch| ch.is_ascii_alphabetic() || *ch == '_')
        {
            let start = j;
            let mut k = j;
            while k < c.len() && common::is_word(c[k]) {
                k += 1;
            }
            out.push(c[start..k].iter().collect());
            i = k;
            continue;
        }
        i += 1;
    }
    out
}

/// `\bname\b`, counted — non-overlapping, left to right, which is what a `/g` regex does.
fn count_word(text: &str, name: &str) -> usize {
    let c = common::chars(text);
    let n = common::chars(name);
    let mut count = 0;
    let mut i = 0;
    while i + n.len() <= c.len() {
        if c[i..i + n.len()] == n[..]
            && (i == 0 || !common::is_word(c[i - 1]))
            && (i + n.len() >= c.len() || !common::is_word(c[i + n.len()]))
        {
            count += 1;
            i += n.len();
            continue;
        }
        i += 1;
    }
    count
}

fn check() -> Streams {
    let root = common::repo();
    let mut failures: Vec<String> = Vec::new();
    let mut exports_seen = 0usize;

    for (ui, dir) in UIS {
        let files = source_files(&root.join(dir));
        let texts: Vec<(PathBuf, String)> = files
            .iter()
            .map(|f| {
                let raw = fs::read_to_string(f)
                    .unwrap_or_else(|e| panic!("cannot read {}: {e}", f.display()));
                (f.clone(), strip_comments(&raw))
            })
            .collect();
        let mut seen = 0usize;

        for (file, text) in &texts {
            if file.to_string_lossy().contains(".test.") {
                continue;
            }
            for name in declared_exports(text) {
                if ALLOWED.contains(&name.as_str()) {
                    continue;
                }
                seen += 1;
                let mut elsewhere = 0usize;
                let mut own = 0usize;
                for (other, body) in &texts {
                    let found = count_word(body, &name);
                    if other == file {
                        own += found.saturating_sub(1); // minus the declaration itself
                    } else {
                        elsewhere += found;
                    }
                }
                let where_ = file
                    .strip_prefix(&root)
                    .unwrap_or(file)
                    .to_string_lossy()
                    .replace('\\', "/");
                if elsewhere == 0 && own == 0 {
                    failures.push(format!(
                        "{ui}: {name} ({where_}) is exported and used NOWHERE — delete it"
                    ));
                } else if elsewhere == 0 {
                    failures.push(format!(
                        "{ui}: {name} ({where_}) is exported but used only inside its own file — drop the `export`"
                    ));
                }
            }
        }

        // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN: the two UIs had 305 and 43 exports when this
        // was written.
        if seen < 20 {
            return Streams {
                stdout: String::new(),
                stderr: format!(
                    "exports-check: FAILED — the {ui} scan found {seen} exports, which is too few to be reading it\n"
                ),
                failed: true,
            };
        }
        exports_seen += seen;
    }

    if !failures.is_empty() {
        let mut stderr = String::from("exports-check: FAILED — public surface nothing uses:\n");
        for f in &failures {
            stderr.push_str(&format!("  {f}\n"));
        }
        stderr.push_str(
            "\n  An export is a PROMISE that somebody outside needs this. If nobody does, it is either the file's\n  \
             own business (drop the keyword) or nobody's (delete it). A test-only use is a seam and counts as use.\n",
        );
        return Streams {
            stdout: String::new(),
            stderr,
            failed: true,
        };
    }
    Streams {
        stdout: format!(
            "exports-check: ok — {exports_seen} exports across both UIs, every one used outside the file that declares it\n"
        ),
        stderr: String::new(),
        failed: false,
    }
}

#[test]
fn every_export_is_used_outside_the_file_that_declares_it() {
    let out = check();
    print!("{}", out.stdout);
    if out.failed {
        panic!("{}", out.stderr);
    }
    // A FLOOR, not a claim: the export count is the tree's business. What must not happen is this scan
    // reading almost nothing and reporting that every one of them is used.
    let seen: usize = out
        .stdout
        .split_once("exports-check: ok — ")
        .and_then(|(_, rest)| rest.split(' ').next())
        .and_then(|n| n.parse().ok())
        .expect("the line names the export count");
    assert!(
        seen >= 40,
        "read only {seen} exports — the walk is looking at the wrong thing"
    );
    assert!(out.stdout.contains("across both UIs"));
}

// ── the scanner's own proof: the declaration, the count, and the prose that must not count ──────────

#[test]
fn a_declaration_is_read_at_a_line_start_and_only_in_its_six_shapes() {
    assert_eq!(
        declared_exports("export const A = 1;\nexport async function B() {}\nexport type C = 1;"),
        vec!["A".to_string(), "B".to_string(), "C".to_string()]
    );
    // A commented-out declaration is not a declaration — the strip is what stops it counting.
    assert!(declared_exports(&strip_comments("// export const GONE = 1;")).is_empty());
    // ...and neither is an indented one, or a name that is not a name.
    assert!(declared_exports("  export const INDENTED = 1;").is_empty());
    assert!(declared_exports("export default thing;").is_empty());
    assert!(declared_exports("exported const X = 1;").is_empty());
    // `export {}` with no keyword is not a declaration either.
    assert!(declared_exports("export { a, b };").is_empty());
}

#[test]
fn a_use_is_a_word_and_prose_is_not_a_use() {
    assert_eq!(
        count_word("const x = useSessionEvents();", "useSessionEvents"),
        1
    );
    assert_eq!(
        count_word("useSessionEventsX", "useSessionEvents"),
        0,
        "a word, not a prefix"
    );
    assert_eq!(count_word("myUseSessionEvents", "useSessionEvents"), 0);
    assert_eq!(
        count_word(
            "a useSessionEvents b useSessionEvents c",
            "useSessionEvents"
        ),
        2
    );
    // THE MEASURED DEFECT THIS STRIP EXISTS FOR: `useSessionEvents` survived six audits on two comments
    // that merely mentioned it.
    let src = "// useSessionEvents is mentioned here and nowhere else\nconst x = 1;";
    assert_eq!(count_word(&strip_comments(src), "useSessionEvents"), 0);
    assert_eq!(
        count_word(src, "useSessionEvents"),
        1,
        "without the strip it is a use"
    );
}

#[test]
fn the_walk_skips_the_three_directories_and_the_declaration_files() {
    let names: Vec<String> = source_files(&common::repo().join("agent/resources/panel-react/src"))
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    assert!(
        names.iter().any(|n| n.ends_with(".tsx")),
        "the panel has components"
    );
    assert!(!names.iter().any(|n| n.contains("/node_modules/")));
    assert!(!names.iter().any(|n| n.ends_with(".d.ts")));
    let set: BTreeSet<&String> = names.iter().collect();
    assert_eq!(set.len(), names.len(), "a file is visited once");
}
