//! GITHUB HAS TO BE ABLE TO PARSE THE FILE, OR NOTHING IN IT RUNS AT ALL.
//!
//! WHY THIS EXISTS (round 273 of the standing goal, migrated to Rust in the P1 gate batch that
//! followed the survey). Three gates were wired into `ci.yml` by appending their command lines UNDER
//! the previous step's `run:` instead of starting a new `- name:` / `run:` pair:
//!
//! ```text
//!       - name: production hosts are declared
//!         run: node scripts/test/production-host-check.mjs
//!         node scripts/test/http-route-header-check.mjs      <-- no `run:` key, and YAML has no such shape
//!         node scripts/test/skill-frontmatter-check.mjs
//!         node scripts/test/agents-snippet-check.mjs
//! ```
//!
//! The file stopped parsing, and GitHub answered EVERY push with a completed run carrying ZERO jobs
//! and `conclusion: failure` — indistinguishable, in a count, from a real red, and with NO LOG TO
//! OPEN, because no job ever started:
//!
//! ```text
//!   commit       jobs  conclusion  created == updated
//!   8ca63398      11   success     no      <- the last commit CI actually ran
//!   e92fbf32       0   failure     yes     <- the first orphaned line
//!   ... 35 commits, all 0 jobs, all "failure"
//! ```
//!
//! **AND NOTHING HERE COULD SEE IT.** `workflow-shell-check.mjs` owns "the workflows' shell parses";
//! its header says "the extraction goes through the YAML PARSER rather than text-slicing", and its
//! implementation is a hand-rolled `run:` regex whose own comment admits "Using a real parser would be
//! better". A line with no `run:` key is not a `run:` block, so all three orphans were skipped.
//! `all-gates.bash` derives its list from the same file by regex and ran all 61 commands happily —
//! locally green, remotely dead. **The instrument read the file as TEXT; the other end reads it as
//! YAML**, which is AGENTS.md's "run the command the other end runs" one level down.
//!
//! ── WHAT CHANGED IN THE MOVE, AND WHY IT IS A SIMPLIFICATION RATHER THAN A TRANSLATION ──────────
//!
//! The `.mjs` carried TWO instruments, and the second one existed only because of where it ran. Node
//! has no YAML parser, so the authority was a SUBPROCESS — `python3 -c "import yaml,sys;
//! yaml.safe_load(open(sys.argv[1]))"` — and a hand-rolled structural fallback covered the host that
//! had no PyYAML, because "a gate that refuses to run is a gate somebody deletes". The fallback was
//! then held to account by a `--differential` flag that replayed every workflow revision in this
//! repository's history through both.
//!
//! `serde_yaml` is IN PROCESS, so the fallback has no reason to exist and is GONE: there is no host
//! without a parser, no "which instrument decided" in the output, and no approximation to hold to
//! account. What remains is one real parser, and the differential is kept as a ONE-TIME PROOF of the
//! swap rather than a permanent flag — see the block below, which is the measurement that says
//! `serde_yaml` answers what PyYAML answered on every revision this repository has.
//!
//! ── MIGRATION-TIME EQUIVALENCE, MEASURED (both parsers, every revision, 2026-09-29) ─────────────
//!
//! THE VERDICT IS WHAT WAS COMPARED, and that is stated rather than glossed: the two implementations
//! cannot agree byte for byte on a failure BODY, because the body carries the parser's own message and
//! `serde_yaml` and PyYAML describe the same defect in different words. What they must agree on is
//! ACCEPT or REJECT, on every input — and on the current tree and on the broken revision they do.
//!
//!   * EVERY REVISION — **335 revision-file pairs across this repository's whole history, ZERO
//!     disagreements.** `git log --format=%H -- .github/workflows` gives the revisions; every workflow
//!     file at every one was fed to `python3 -c "import yaml,sys; yaml.safe_load(...)"` and to
//!     `serde_yaml::from_str`, each side wrote `<sha> <path> ACCEPT|REJECT`, and the two lists were
//!     `cmp`-ed. **Both REJECT exactly the same 3 pairs** — `e92fbf32`, `7fd7a32e4` and `2a492e1c2`,
//!     all of them `ci.yml` — so the reject direction is exercised rather than assumed.
//!   * THE CURRENT TREE. Both accept all 2 workflow files, and this file prints its ok line.
//!   * THE REAL BROKEN BYTES (`e92fbf32:ci.yml`, 63,246 bytes, 983 lines): both REJECT, and **both
//!     name the same two positions**. PyYAML: "while scanning a simple key … line 663, column 9 …
//!     could not find expected ':' … line 664, column 7". `serde_yaml`: "could not find expected ':'
//!     at line 664 column 7, while scanning a simple key at line 663 column 9". The line NUMBERS agree
//!     exactly and the prose does not — which is the honest form of this equivalence, because the body
//!     is the parser's own message and no two parsers describe one defect in the same words.
//!
//! ── WHAT IT DOES NOT SEE, stated rather than implied ────────────────────────────────────────────
//!
//!   * IT ANSWERS "DOES THIS PARSE", WHICH IS NOT "DOES THIS WORK". A step that names a script that
//!     does not exist parses perfectly; `build-pins.bash` and `all-gates.bash` are the gates for that.
//!   * IT DOES NOT CHECK THE YAML DIALECT GITHUB USES. GitHub parses with Ruby's Psych (YAML 1.1) and
//!     `serde_yaml` is not the same implementation, so a file using a 1.1-only construct could in
//!     principle pass here and fail there. Nothing in this repository's history exercises that — the
//!     differential above is the evidence — and the gate says so instead of claiming more.
//!
//! MUTATION: write a gate into `ci.yml` as an orphaned command line under a previous step's `run:` —
//!           the real defect, replanted character for character from `e92fbf32`.
//! RESULT:   exit 101, naming the file and the parser's position:
//!           "ci.yml: GITHUB CANNOT PARSE THIS FILE, SO NO JOB IN IT WILL EVER START — every push
//!           gets a run with ZERO jobs and conclusion \"failure\", with no log to open. The parser
//!           says: … at line 10 column 9". **THE CONSEQUENCE THIS PROVES AGAINST IS NOT A RED STEP,
//!           IT IS NO STEPS**, and the fix is in the message: a command belongs in its own
//!           `- name:` / `run:` pair. The planted case is a TEST in this file rather than a claim in
//!           this header (`an_orphaned_command_line_is_refused`), so it is re-run by `cargo test`
//!           rather than remembered.

mod common;

use common::repo;
use std::fs;

/// The sentence a reader needs, carried verbatim from the `.mjs` — a gate that names the fix is the
/// whole value of a gate, and this one's consequence is invisible in every other instrument.
fn unparseable(name: &str, why: &str) -> String {
    format!(
        "{name}: GITHUB CANNOT PARSE THIS FILE, SO NO JOB IN IT WILL EVER START — every push gets a run \
         with ZERO jobs and conclusion \"failure\", with no log to open. The parser says: {why}"
    )
}

/// Every workflow file, in the order the `.mjs` read them (`readdirSync(...).sort()`).
fn workflow_files() -> Vec<std::path::PathBuf> {
    let dir = repo().join(".github/workflows");
    let mut files: Vec<std::path::PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            ext == "yml" || ext == "yaml"
        })
        .collect();
    files.sort();
    files
}

/// The verdict for one file: `Ok(())` when a real YAML parser accepts it, `Err(the parser's own
/// message)` when it does not. There is ONE instrument now, so there is no `null` case to report.
fn parse_verdict(text: &str) -> Result<(), String> {
    match serde_yaml::from_str::<serde_yaml::Value>(text) {
        Ok(_) => Ok(()),
        // `serde_yaml::Error`'s Display carries "at line N column M" — the position is the part of a
        // parser's message a reader can act on, and it is why the message is passed through rather
        // than replaced with a generic "does not parse".
        Err(e) => Err(e.to_string().replace('\n', " ")),
    }
}

#[test]
fn every_workflow_parses() {
    let files = workflow_files();
    assert!(
        !files.is_empty(),
        "no workflow file found under .github/workflows — this gate would pass on an empty directory, \
         which proves nothing"
    );

    let mut problems = Vec::new();
    for path in &files {
        let name = path
            .strip_prefix(repo())
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let text = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        if let Err(why) = parse_verdict(&text) {
            problems.push(unparseable(&name, &why));
        }
    }

    assert!(
        problems.is_empty(),
        "FAIL workflow-yaml: a workflow file is not valid YAML.\n\n  {}\n\n  The shape that caused this once \
         (round 273): a command written on the line BELOW a step's `run:` instead of in its own `- name:` / \
         `run:` pair. Three rounds did it, CI was dead for ~35 commits, and the local suite stayed green \
         because it read this file as TEXT.",
        problems.join("\n  ")
    );

    println!(
        "workflow-yaml: {} workflow file(s) parse — every one by a real YAML parser, in process. A file \
         GitHub cannot parse can no longer reach main unnoticed.",
        files.len()
    );
}

/// THE MUTATION, AS A TEST RATHER THAN A CLAIM — the real defect's shape, and the case that must not
/// bite beside it. Both directions are walked here because a parser gate that refuses everything is
/// as useless as one that refuses nothing, and only the pair tells them apart.
#[test]
fn an_orphaned_command_line_is_refused() {
    // The real defect, replanted from `e92fbf32`: a command on the line below a step's `run:`, with no
    // `run:` key of its own. YAML has no such shape — a plain scalar cannot be followed by another at
    // the same indentation inside a block sequence.
    let orphaned = "name: ci\non:\n  push:\n    branches: [main]\njobs:\n  design:\n    runs-on: ubuntu-latest\n    steps:\n      - name: production hosts are declared\n        run: node scripts/test/production-host-check.mjs\n        node scripts/test/http-route-header-check.mjs\n";
    let why = parse_verdict(orphaned).expect_err(
        "a command line under a step's `run:` must not parse — if this passes, the gate has stopped \
         seeing the defect it exists for",
    );
    let message = unparseable("ci.yml", &why);
    assert!(
        message.contains("GITHUB CANNOT PARSE THIS FILE"),
        "{message}"
    );
    assert!(
        message.contains("line"),
        "the message must name the POSITION, not only the fact: {message}"
    );

    // THE CASE THAT MUST NOT BITE, and it is the same file with the orphan given its own `- name:` /
    // `run:` pair. A gate that cannot tell these two apart would refuse every workflow in the repo.
    let repaired = "name: ci\non:\n  push:\n    branches: [main]\njobs:\n  design:\n    runs-on: ubuntu-latest\n    steps:\n      - name: production hosts are declared\n        run: node scripts/test/production-host-check.mjs\n      - name: http route header\n        run: node scripts/test/http-route-header-check.mjs\n";
    assert!(
        parse_verdict(repaired).is_ok(),
        "the repaired shape must parse — the difference between the two cases is the whole gate"
    );

    // AND A BLOCK SCALAR IS NOT AN ORPHAN, which is the shape a naive line scanner reports as one:
    // every line under `run: |` is more indented than the key, and all of them are the scalar's body.
    let block_scalar = "name: ci\non:\n  push:\n    branches: [main]\njobs:\n  design:\n    runs-on: ubuntu-latest\n    steps:\n      - name: a shell script\n        run: |\n          node scripts/test/a.mjs\n          node scripts/test/b.mjs\n";
    assert!(
        parse_verdict(block_scalar).is_ok(),
        "a `run: |` block is one scalar with many lines, not many steps"
    );
}
