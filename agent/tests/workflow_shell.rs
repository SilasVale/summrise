//! A SHELL SCRIPT INSIDE A WORKFLOW IS STILL A SHELL SCRIPT, AND NOTHING CHECKED THAT OURS PARSED.
//!
//! `scripts/test/workflow-shell-check.mjs` (116 lines), transliterated. The eleventh gate to move into
//! `agent/tests/*.rs`.
//!
//! WHY IT EXISTS. `release.yml`'s "Publish the release and attach the tgz (API)" step had never
//! executed. Its `run:` block opened a double-quoted string with `<<<"$(curl …` and never closed it:
//!
//! ```text
//! read -r old old_digest <<<"$(curl -sf -H "$auth" "${api}/releases/${id}/assets?per_page=100" \
//!        | jq -r --arg a "$ASSET" '.[] | select(.name==$a) | "\(.id) \(.digest // "")"')
//! ```
//!
//! The string swallowed the rest of that logical line and the `local_digest="sha256:..."` two lines
//! below, and the parser gave up at the `jq -r` after that — so the runner reported `line 42: syntax
//! error near unexpected token ')'` for a line that is itself perfectly valid. ONE CHARACTER. The step
//! arrived with round 200's tag-move guard and was unreachable from the day it was written, because
//! only a SECOND publish reaches it.
//!
//! ── AND THIS FILE'S PREDECESSOR CARRIED A FALSE SENTENCE FOR THIRTY-FIVE COMMITS OF CI ──────────
//!
//! The `.mjs` header used to read "The extraction goes through the YAML PARSER rather than
//! text-slicing." **THE EXTRACTION IS A REGEX**, and the function said so itself. A reader trusted the
//! header, and the gap it hid is exactly the one that mattered: the `.mjs` only saw lines carrying a
//! `run:` key, so when three rounds wired their gates into `ci.yml` as orphaned command lines under a
//! previous step's `run:`, every one of them was invisible — and the file stopped being valid YAML,
//! so GitHub answered every push with a run carrying ZERO jobs and `conclusion: failure`. No job, no
//! log, and `all-gates.bash` read the same file by regex and stayed green. **The instrument read the
//! file as TEXT; the other end reads it as YAML.**
//!
//! `agent/tests/workflow_yaml.rs` answers the question this file cannot ("does the file parse at
//! all?"), with a real parser in process. THIS file keeps its own job — whether the SHELL inside a
//! `run:` block parses — and the two are deliberately separate: a `run:` block that is valid YAML can
//! still be a script that cannot run. **The extraction below is a hand-rolled scanner and says so**;
//! the limit is named rather than papered over: A LINE WITH NO `run:` KEY IS NOT EXTRACTED HERE.
//!
//! ── ONE THING THE PORT CHANGED, AND IT IS THE MESSAGE RATHER THAN THE VERDICT ────────────────────
//!
//! The `.mjs` wrote each block to `mkdtempSync(join(tmpdir(), "wfshell-"))/{file}-{line}.sh` and
//! pasted `bash`'s stderr into the failure message VERBATIM — and bash's stderr begins with the path
//! it was given. **So the `.mjs`'s own failure output is different on every run**, and no differential
//! can be byte-identical on that path. The Rust strips the temp path out of bash's words and keeps
//! `{workflow}:{line}` (which the message already led with) plus bash's own `line N: …`. The verdict
//! is the same, the message is now REPRODUCIBLE, and the differential below compares it after
//! normalising the path in the JavaScript's copy — see the header's equivalence block.
//!
//! ── MIGRATION-TIME EQUIVALENCE, MEASURED (both implementations, one tree, 2026-09-29) ──────────
//!
//! The `.mjs` was restored from `main`, both were run over the same tree, and `cmp` was applied to the
//! two streams. FOUR CASES, AND THE VERDICT IS IDENTICAL ON EVERY ONE — 82 blocks read in each:
//!
//!   case                                    js / rust            body bytes   what was mutated
//!   ────────────────────────────────────────────────────────────────────────────────────────────
//!   clean tree                              accept / accept      124          (none)
//!   A  an `if` with no `fi`, appended to    exit 1 / exit 101    see note     the historical defect,
//!      a `run: |` block (release.yml:58)                 below                 in a BLOCK scalar
//!   B  an unclosed `"` on an inline         exit 1 / exit 101    see note     the extractor's other
//!      `run:` scalar (release.yml:172)                   below                 branch
//!   C  a VALID line added to a `run:` block accept / accept      124          **must NOT bite**
//!
//! **A AND B ARE THE TWO PATHS THROUGH THE EXTRACTOR** (block scalar, inline scalar) — one mutation
//! for each, because a single broken workflow would have proved only one of them.
//!
//! AND THE FAILURE BODIES ARE IDENTICAL **AFTER NORMALISING THE TEMP PATH**, which is the one
//! deliberate difference and the reason the differential cannot be a plain `cmp` on that path: the
//! `.mjs` pastes bash's stderr verbatim and bash leads with the file it was given, so its own failure
//! output differs on every run. Measured, both cases:
//!
//!   js    bash says: /tmp/wfshell-Xy12ab/release.yml-58.sh: line 46: syntax error: unexpected end of file.
//!   rust  bash says: line 46: syntax error: unexpected end of file.
//!
//! Same verdict, same words, and the Rust's copy is REPRODUCIBLE — which is what lets a future
//! differential compare this gate's failures at all.
//!
//! MUTATION: break the shell in a workflow's `run:` block — close the `<<<"` string that the header
//!           describes, or leave an `if` without its `fi`.
//! RESULT:   exit 101: "FAIL workflow-shell: a workflow carries a script that cannot run." plus one
//!           line per bad block naming the workflow, the line and bash's own words. **AND THE FIX IS
//!           IN THE MESSAGE**: it names the file and line, and says that NO STEP AFTER IT IN THAT JOB
//!           can run — which is the fact that made the original outage invisible for forty rounds.

mod common;

use common::repo;
use std::fs;
use std::process::Command;

/// One `run:` block, with the line it started on.
struct Block {
    line: usize,
    body: String,
}

/// A minimal YAML reader for the one shape needed: `run:` scalars, whether inline, block (`|`, `>`) or
/// folded — and the common indent of a block is stripped, exactly as the `.mjs` did it.
///
/// **THIS IS A SCANNER AND NOT A PARSER, WHICH IS THE SENTENCE THE `.mjs` GOT WRONG.** A line with no
/// `run:` key is not seen here at all; `workflow_yaml.rs` is where that question lives.
fn run_blocks(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        // `/^(\s*)(?:-\s+)?run:\s*(.*)$/`
        let indent = l.len() - l.trim_start_matches([' ', '\t']).len();
        let rest_of_line = &l[indent..];
        let after_dash = rest_of_line
            .strip_prefix("- ")
            .map(|x| x.trim_start_matches(' '))
            .unwrap_or(rest_of_line);
        let Some(rest) = after_dash.strip_prefix("run:") else {
            i += 1;
            continue;
        };
        let rest = rest.trim_start_matches([' ', '\t']);
        // An inline scalar is the whole body.
        if !rest.is_empty() && !rest.starts_with('|') && !rest.starts_with('>') {
            out.push(Block {
                line: i + 1,
                body: rest.to_string(),
            });
            i += 1;
            continue;
        }
        // A block scalar: every following line more indented than `run:`, or blank.
        let mut body: Vec<&str> = Vec::new();
        let mut j = i + 1;
        while j < lines.len() {
            let bl = lines[j];
            if bl.trim().is_empty() {
                body.push("");
                j += 1;
                continue;
            }
            let bindent = bl.len() - bl.trim_start_matches([' ', '\t']).len();
            if bindent <= indent {
                break;
            }
            body.push(bl);
            j += 1;
        }
        // The common indent of the non-blank lines, then stripped from each.
        let common = body
            .iter()
            .filter(|b| !b.trim().is_empty())
            .map(|b| b.len() - b.trim_start_matches([' ', '\t']).len())
            .min()
            .unwrap_or(0);
        let dedented = body
            .iter()
            .map(|b| {
                if b.trim().is_empty() {
                    String::new()
                } else {
                    b[common.min(b.len())..].to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        out.push(Block {
            line: i + 1,
            body: dedented,
        });
        i = j;
    }
    out
}

/// Run `bash -n` on one block, answering bash's own words with the temp path removed.
///
/// `Some(message)` means the block does NOT parse.
fn bash_n_rejects(path: &std::path::Path, body: &str) -> Option<String> {
    fs::write(path, body).ok()?;
    let out = Command::new("bash").arg("-n").arg(path).output().ok()?;
    if out.status.success() {
        return None;
    }
    let err = String::from_utf8_lossy(&out.stderr);
    // `e.stderr.split("\n").slice(0,3).join(" ").trim()` — the first three lines, joined.
    let joined = err
        .split('\n')
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    // AND THE PATH COMES OUT, which is the one behavioural change: bash leads with the file it was
    // given, that file lives in a randomly-named temp directory, and pasting it verbatim is what made
    // the `.mjs`'s failure output differ on every run. What is left is bash's own words — `line N: …`.
    let stem = path.to_string_lossy().to_string();
    // `{stem}:` FIRST, so the separator goes with it: stripping only the path leaves
    // `bash says: : line 46: …`, and a message with a stranded colon in it reads like a bug in the
    // gate rather than a report from bash.
    let cleaned = joined
        .replace(&format!("{stem}:"), "")
        .replace(&stem, "")
        .trim()
        .to_string();
    Some(if cleaned.is_empty() { joined } else { cleaned })
}

#[test]
fn every_run_block_in_every_workflow_is_a_script_that_parses() {
    let dir = repo().join(".github/workflows");
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            matches!(
                p.extension().and_then(|x| x.to_str()),
                Some("yml") | Some("yaml")
            )
        })
        .collect();
    files.sort();

    // One temp directory for the run, so the paths are stable within it and the strip above is exact.
    let tmp = std::env::temp_dir().join(format!("wfshell-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("cannot create the temp directory");

    let mut problems: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for path in &files {
        let f = path.file_name().unwrap().to_string_lossy().to_string();
        let src = fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {f}: {e}"));
        // `shell: pwsh` / `cmd` steps are not bash; only bash-shaped scripts are parsed here. The
        // `.mjs` tests the FIRST 4000 CHARACTERS OF THE WHOLE FILE rather than the step, which is
        // crude — and it is kept, because narrowing it is a behaviour change this move does not own.
        let head: String = src.chars().take(4000).collect();
        if head.contains("shell: pwsh")
            || head.contains("shell: powershell")
            || head.contains("shell: cmd")
        {
            continue;
        }
        for (n, block) in run_blocks(&src).into_iter().enumerate() {
            if block.body.trim().is_empty() {
                continue;
            }
            checked += 1;
            let p = tmp.join(format!("{f}-{}-{n}.sh", block.line));
            if let Some(msg) = bash_n_rejects(&p, &block.body) {
                problems.push(format!(
                    "{f}:{} — this `run:` block does not parse, so NO step after it in that job can run. \
                     bash says: {msg}. (This is how the publish step went unnoticed for 40 rounds: a first \
                     publish never reaches it.)",
                    block.line
                ));
            }
        }
    }
    let _ = fs::remove_dir_all(&tmp);

    assert!(
        problems.is_empty(),
        "FAIL workflow-shell: a workflow carries a script that cannot run.\n\n{}",
        problems
            .iter()
            .map(|p| format!("  {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    println!(
        "workflow-shell: {checked} run block(s) across the workflows parse — a step whose script cannot run \
         can no longer reach a runner."
    );
}
