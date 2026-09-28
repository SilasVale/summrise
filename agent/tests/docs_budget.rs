//! docs-budget-check — THE INSTRUCTION FILE HAS TO STAY SMALL ENOUGH TO BE READ
//! WHOLE, AND THE GLOSSARY HAS TO STAY A GLOSSARY.
//!
//! MEASURED, round 67: `AGENTS.md` reached 65,366 bytes, and the workspace harness
//! TRUNCATES an instruction file at 65,536 — it dropped the tail, which is the
//! Release and Agent-layout sections. Nothing failed, nothing warned except a
//! one-line note in a system reminder, and the operator's own instructions were
//! being silently cut off at the end of the file.
//!
//! SO THE RULE IS STRUCTURAL, not a size alone: `AGENTS.md` holds the OPERATIONAL
//! sections (build, test, which gate each mutation must fail, committing, release,
//! layout) and POINTERS; the reference tables live in `docs/agents/`. This check
//! fails if a narrative grows back into the instruction file, or if the file
//! approaches the budget again.
//!
//! MIGRATED FROM `scripts/test/docs-budget-check.mjs`, WHICH IS DELETED — a
//! migration that leaves both is two gates, not one. The operator asked for it
//! three times in one session (*"我不太喜欢js，你一直改js"*), and this gate is the
//! shape the rest will copy: PURE file-and-byte logic, no dependencies, and it runs
//! in `cargo test -p summrise-agent` — a command CI ALREADY runs, so the move costs
//! no new job, no new runner and no new workflow step.
//!
//! THE INPUTS, THE VERDICTS AND THE MESSAGES ARE UNCHANGED, and that was proved rather
//! than asserted: both implementations were run over the SAME tree on NINE inputs and
//! agreed on the verdict AND on the message text every time. The three that matter are a
//! file AT the ceiling (48,000 — both accept), a file OVER it (48,001 — both refuse with
//! the same sentence), and a `###` narrative section (both refuse, naming the section).
//! Also covered, because an edge nobody planted is an edge nobody checked: the same
//! `###` INSIDE a fenced block (both accept), `CONTEXT.md` over 12,000 and empty, and a
//! required `##` section renamed. The equivalence is what makes this a migration;
//! without it, it would be a second gate that happens to share a name.
//!
//! WHAT THIS DOES NOT DO, stated rather than implied: it is not a `.mjs` any more,
//! so `scripts/test/all-gates.bash` no longer runs it — that runner derives its list
//! from the `run:` lines of `ci.yml`, and its own header already says it runs the
//! GATE COMMANDS and not the per-directory cargo suites. The reach is the `agent`
//! job's `cargo test -p summrise-agent`, which is the command CI runs and therefore
//! the command a person runs by hand:
//!
//!     cd agent && cargo test -p summrise-agent --test docs_budget
//!
//! ── THE MUTATION THAT MUST FAIL THIS GATE ──
//! Read this when you change this file: the mutation is how you find out whether the
//! gate can still fail at all. A gate that cannot be broken is worse than no gate.
//!
//! MUTATION: append 8 bytes to `AGENTS.md`, putting it AT 48,001 against the 48,000 ceiling
//! RESULT:   exit 101 —
//!   docs-budget-check: AGENTS.md is 48001 bytes and the ceiling is 48000 — the workspace harness truncates at 65536, silently dropping the END of the file, which is where the release steps live (round 67)
//!
//! MUTATION: plant a `### Round 300` section OUTSIDE a fenced code block, trimming one
//!           sentence first so the file stays UNDER the ceiling and the narrative rule is
//!           the ONLY one that can fire
//! RESULT:   exit 101, at 47,941 bytes —
//!   docs-budget-check: "Round 300" is a narrative section in AGENTS.md — the instruction file holds what changes what you DO, and a narrative belongs in a commit message or an ADR
//!
//! AND THE MUTATION THAT MUST NOT: the same `### Round 300` INSIDE a fenced code block
//! leaves it GREEN (exit 0, at 47,942 bytes), because a heading in a code block is not a
//! heading. A gate that refused that would refuse its own documentation.
//!
//! THE TRIM IS PART OF THE PROOF RATHER THAN A CONVENIENCE, and the arithmetic is why:
//! `AGENTS.md` sits SEVEN bytes under the ceiling, so ANY planted text trips the ceiling
//! as well, and the narrative case would then refuse for two reasons while proving
//! neither. The differential harness asserts that margin before each case runs, so a case
//! that has quietly become a ceiling case fails instead of passing for the wrong reason.

use std::fs;
use std::path::PathBuf;

const INSTRUCTIONS: &str = "AGENTS.md";
const GLOSSARY: &str = "CONTEXT.md";
const GLOSSARY_CEILING: usize = 12_000;
const HARNESS_BUDGET: usize = 65_536;
const CEILING: usize = 48_000;

/// The operational sections that must stay in the instruction file. If one of these
/// moves out, the file stops being an instruction file — which is the failure this
/// gate exists to refuse, one step before the byte count notices.
const REQUIRED_SECTIONS: [&str; 7] = [
    "## Build",
    "## Test",
    "### Which gates have been PROVEN to bite",
    "## The vocabulary",
    "## Committing",
    "## Release",
    "## Agent layout",
];

/// THE ONE `###` SECTION THIS FILE MAY CARRY — the gate table. Anything else is a
/// narrative, and a narrative in the instruction file is what this gate refuses.
const ALLOWED_SUBSECTION: &str = "Which gates have been PROVEN to bite";

fn repo_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("agent/ has a parent")
        .to_path_buf()
}

/// `/```[\s\S]*?```/g` — a heading inside a fenced code block is not a heading.
///
/// A HAND-ROLLED SCAN RATHER THAN A REGEX CRATE, because the crate would be a new
/// dependency for one non-greedy pattern. It reproduces the regex's two edges: it
/// matches the FIRST closing fence after an opening one, and an UNCLOSED fence is
/// left in the text (the regex cannot match there either, so it moves on).
fn without_fenced_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with("```") {
            match text[i + 3..].find("```") {
                Some(rel) => {
                    i += 3 + rel + 3;
                    continue;
                }
                None => {
                    out.push_str("```");
                    i += 3;
                    continue;
                }
            }
        }
        let ch = text[i..].chars().next().expect("i is on a char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `/^### (.+)$/gm`, over the text with its fenced blocks removed.
///
/// `(.+)` needs at least one character, and `.` matches no line terminator — so a
/// line whose tail carries `\r` never matches the pattern at all (multiline `$`
/// anchors before `\n` only). Both edges are kept: they are the difference between
/// this and a `starts_with("### ")`.
fn narrative_subsections(text: &str) -> Vec<String> {
    without_fenced_blocks(text)
        .split('\n')
        .filter_map(|line| {
            let rest = line.strip_prefix("### ")?;
            if rest.is_empty() || rest.contains(['\r', '\u{2028}', '\u{2029}']) {
                return None;
            }
            Some(rest.to_string())
        })
        .collect()
}

/// Every reason the two files break their budget, in the order the `.mjs` reported
/// them — a message that names the fix is the whole value of a gate, so the text is
/// carried across verbatim rather than paraphrased.
fn budget_failures(instructions: &str, glossary: &str) -> Vec<String> {
    let mut failures = Vec::new();

    // `String::len` IS the UTF-8 byte count — the same number `Buffer.byteLength`
    // answers. A `.chars().count()` here would count the em-dashes in this file's own
    // prose as one byte each and pass a file the harness truncates.
    let bytes = instructions.len();
    if bytes > CEILING {
        failures.push(format!(
            "{INSTRUCTIONS} is {bytes} bytes and the ceiling is {CEILING} — the workspace harness truncates at {HARNESS_BUDGET}, silently dropping the END of the file, which is where the release steps live (round 67)"
        ));
    }
    for must in REQUIRED_SECTIONS {
        if !instructions.contains(must) {
            failures.push(format!(
                "{INSTRUCTIONS} no longer has \"{must}\" — the operational half is what stays here"
            ));
        }
    }
    for section in narrative_subsections(instructions) {
        if !section.starts_with(ALLOWED_SUBSECTION) {
            failures.push(format!(
                "\"{section}\" is a narrative section in {INSTRUCTIONS} — the instruction file holds what changes what you DO, and a narrative belongs in a commit message or an ADR"
            ));
        }
    }

    // A MISSING GLOSSARY AND AN EMPTY ONE BOTH ANSWER "" — `existsSync` then
    // `readFileSync` in the `.mjs`, and `unwrap_or_default` here, are the same
    // instrument: both report "does not exist". The quirk is kept, because a
    // silently different edge case is a different gate.
    let glossary_bytes = glossary.len();
    if glossary.is_empty() {
        failures.push(format!(
            "{GLOSSARY} does not exist — the glossary was DELETED rather than moved, and every surface that names a thing now has nowhere to check the word"
        ));
    } else if glossary_bytes > GLOSSARY_CEILING {
        failures.push(format!(
            "{GLOSSARY} is {glossary_bytes} bytes and the ceiling is {GLOSSARY_CEILING} — it is a GLOSSARY: terms, not a rulebook, and not a place for implementation decisions"
        ));
    }

    failures
}

#[test]
fn docs_budget_check() {
    let root = repo_dir();
    let instructions = fs::read_to_string(root.join(INSTRUCTIONS)).unwrap_or_else(|e| {
        panic!(
            "docs-budget-check: cannot read {INSTRUCTIONS} at {}: {e}",
            root.display()
        )
    });
    let glossary = fs::read_to_string(root.join(GLOSSARY)).unwrap_or_default();

    let failures = budget_failures(&instructions, &glossary);
    let report = failures
        .iter()
        .map(|f| format!("docs-budget-check: {f}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(failures.is_empty(), "{report}");

    let bytes = instructions.len();
    let glossary_bytes = glossary.len();
    println!(
        "docs-budget-check: ok — the instruction file fits ({bytes} of {CEILING} bytes — the ENFORCED ceiling; the harness \
         truncates at {HARNESS_BUDGET}), the glossary is {glossary_bytes} B of {GLOSSARY_CEILING}, and the gates carry \
         their own proofs",
    );
}
