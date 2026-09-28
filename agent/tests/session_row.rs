//! A DEVICE FIELD IS READ IN ONE PLACE, OR THE SPINE IS BROKEN AGAIN.
//!
//! WHY THIS EXISTS (round 98 of the standing goal). Rounds 96 and 97 removed the same
//! duplication twice, in the same file: the mapping from a device row to a session row
//! appeared TWICE byte-identical, and the part of it that is the DEVICE's to say appeared
//! THREE times (the full row, the revive of a tombstone, and the sync of a hold that changed
//! without an event). Both were the spine's defect in its most literal form — one fact written
//! down more than once, free to drift the moment one copy gains a field. A field added to the
//! wire would have reached a NEW row and neither of the refreshed ones.
//!
//! THE RULE THAT KEEPS IT FIXED is absolute and mechanical: in `useSessions.ts` — the one
//! module that turns device rows into session rows — every snake_case field READ from a wire
//! row must sit inside `wireFields`, the single definition. A snake_case read anywhere else in
//! that file is a second definition, whoever wrote it and for whatever good reason.
//!
//! WHAT IT DOES NOT CHECK: the panel's other hooks (their fields are the wire-field gate's
//! business), the camelCase names the row uses afterwards, or whether `wireFields` is used
//! everywhere it should be — that is what the panel's own tests and the three call sites are
//! for.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/session-row-check.mjs` → exit 0, "session-row: N device field(s)
//!     read, all inside the single pair of row definitions".
//!   * this file → ok, with the SAME count N.
//!   * a planted `const x = s.last_exit_code;` BELOW the hook → both fail, both naming
//!     `useSessions.ts:<line> reads last_exit_code`.
//!
//! MUTATION: read a device field outside the mapping section — add
//!           `const late = s.last_exit_code;` to the body of `useSessions`.
//! RESULT:   fails, printing the file, the LINE NUMBER and the field name, and the sentence
//!           that says why a second read is a second definition.

use std::fs;
use std::path::{Path, PathBuf};

/// `cargo test` runs from the CRATE root, not the repo root.
const CRATE: &str = env!("CARGO_MANIFEST_DIR");
const FILE: &str = "agent/resources/panel-react/src/hooks/useSessions.ts";

fn repo() -> PathBuf {
    Path::new(CRATE)
        .parent()
        .expect("the crate lives one level below the repo root")
        .to_path_buf()
}

/// The same conservative strip the field gates use (round 137): a comment naming a read is
/// neither a read nor a floor. `//` also opens a URL, so a whole-line comment goes and a
/// trailing one goes — this file has no URLs, and the rule is copied rather than re-derived.
fn decomment(line: &str) -> String {
    let cut = match line.find("//") {
        Some(k) => &line[..k],
        None => line,
    };
    cut.to_string()
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `\bs\??\.[a-z][a-z0-9]*(?:_[a-z0-9]+)+\b` — a snake_case field read off a wire row `s`.
fn snake_reads(line: &str) -> Vec<String> {
    let s: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] != 's' || (i > 0 && is_word(s[i - 1])) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if s.get(j) == Some(&'?') {
            j += 1;
        }
        if s.get(j) != Some(&'.') {
            i += 1;
            continue;
        }
        j += 1;
        let start = j;
        while j < s.len() && is_word(s[j]) {
            j += 1;
        }
        let ident: String = s[start..j].iter().collect();
        // `[a-z][a-z0-9]*` then one or more `_[a-z0-9]+`: all lowercase, and at least one
        // underscore with a non-empty group on both sides.
        let ok = ident.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            && ident
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && match ident.split_once('_') {
                Some((head, tail)) => {
                    !head.is_empty()
                        && !tail.is_empty()
                        && tail
                            .split('_')
                            .all(|g| !g.is_empty() && g.chars().all(|c| c.is_ascii_alphanumeric()))
                }
                None => false,
            };
        if ok {
            out.push(ident);
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn check(src: &str) -> Result<(usize, String), String> {
    let lines: Vec<&str> = src.lines().collect();
    let hook = lines
        .iter()
        .position(|l| l.starts_with("export function useSessions"))
        .ok_or_else(|| {
            format!("FAIL {FILE} has no useSessions — this check cannot tell the mapping section from the hook")
        })?;
    if !lines
        .iter()
        .any(|l| l.contains("const wireFields = (s: any) =>"))
    {
        return Err(format!(
            "FAIL {FILE} has no wireFields definition — the single place a device field may be read is gone, so \
             this check would be scanning for nothing. Rounds 96-97 extracted it; restore it or explain the new \
             design in this gate."
        ));
    }
    // THE ALLOWED REGION IS THE MAPPING SECTION — everything above the hook: the small `map*`
    // helpers, `wireFields` and `mapRow`. Two attempts narrowed it further and each was
    // corrected by what it found: `wireFields` alone flagged `mapRow`'s own identity reads, and
    // `wireFields` + `mapRow` flagged `mapGrants`/`mapPending`, which are the helpers the
    // definition calls. That IS the shape of one definition — a section, not a line.
    let end = hook - 1;
    let mut inside = 0;
    let mut outside = 0;
    let mut offenders = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let reads = snake_reads(&decomment(line));
        if reads.is_empty() {
            continue;
        }
        if i <= end {
            inside += reads.len();
        } else {
            outside += reads.len();
            offenders.push(format!("{FILE}:{} reads {}", i + 1, reads.join(", ")));
        }
    }
    if inside < 6 {
        return Err(format!(
            "FAIL read only {inside} device field(s) inside the row definitions — they moved, so this proves nothing"
        ));
    }
    if outside > 0 {
        return Err(format!(
            "session-row: {outside} device field read(s) outside the one definition:\n  {}\n\nEvery device field is \
             read in `wireFields` ONLY (rounds 96-97 extracted it after the same duplication appeared twice: a \
             byte-identical row mapper, and the device-owned half of it written three times). A read anywhere else \
             is a second definition, and a field added to the wire reaches one copy and not the others.",
            offenders.join("\n  ")
        ));
    }
    Ok((
        inside,
        format!("session-row: {inside} device field(s) read, all inside the single pair of row definitions (wireFields + mapRow)"),
    ))
}

#[test]
fn every_device_field_is_read_in_the_one_definition() {
    let path = repo().join(FILE);
    let src =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    match check(&src) {
        Ok((_, msg)) => println!("{msg}"),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn a_snake_case_read_below_the_hook_is_the_failure() {
    // The mapping section must clear the gate's own floor first, or the floor's message is
    // what comes back and this proves nothing about the rule.
    let src = "const wireFields = (s: any) => ({ a: s.one_two, b: s.three_four, c: s.five_six, d: s.seven_eight, e: s.eleven_twelve });\n\
               const mapRow = (s: any) => wireFields(s);\n\
               const mapGrants = (s: any) => ({ g: s.nine_ten });\n\
               export function useSessions() {\n  const late = s.last_exit_code;\n}\n";
    let err = check(src).expect_err("a read outside the section must fail");
    assert!(err.contains("last_exit_code"), "{err}");
    assert!(err.contains(":5"), "the message must carry the line: {err}");
}

#[test]
fn a_camel_case_read_is_not_a_device_field_read() {
    // The row's own camelCase names are the gate's stated blind spot, not a finding.
    assert!(snake_reads("const x = row.lastExitCode;").is_empty());
    assert_eq!(
        snake_reads("const x = s.last_exit_code;"),
        vec!["last_exit_code".to_string()]
    );
    assert_eq!(
        snake_reads("const x = s?.last_exit_code;"),
        vec!["last_exit_code".to_string()]
    );
    // `s` as a SUFFIX of a longer name is not the wire row.
    assert!(snake_reads("const x = items.last_exit_code;").is_empty());
}
