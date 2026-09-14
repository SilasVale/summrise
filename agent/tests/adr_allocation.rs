//! ADR numbers are identifiers, and two files share one — twice.
//!
//! `docs/adr/README.md:3` says "Numbered files are adopted decisions", so the NUMBER is
//! the adoption marker; `:28` says "Do not renumber existing records to close the gap."
//! Those two rules together mean a collision cannot be closed by renumbering — and the
//! directory has two of them: `0009` and `0010` each name TWO different decisions.
//!
//! Nothing noticed. The ledger's ADR index enumerates by NUMBER, so "… 0009 … 0010 …"
//! silently represents four files as two, and a reader following it to `0009` finds two
//! documents with no way to tell which the index meant. A citation "per ADR 0009" is
//! ambiguous, and that is the whole cost.
//!
//! This pins the allocation that EXISTS rather than the allocation that would be tidy:
//! the two known pairs are named as exceptions, each with the reason, and ANY THIRD
//! collision fails. That is the same shape the repo already uses for an unreconciled
//! publish — a debt you cannot close today, made explicit so the next change must
//! confront it instead of adding to it.

use std::collections::BTreeMap;
use std::path::PathBuf;

fn adr_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("agent/ has a parent")
        .join("docs/adr")
}

/// number -> the file stems carrying it, sorted.
fn allocation() -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in std::fs::read_dir(adr_dir()).expect("docs/adr/ is readable") {
        let name = entry.expect("dir entry").file_name();
        let name = name.to_string_lossy().to_string();
        if !name.ends_with(".md") {
            continue;
        }
        let stem = name.trim_end_matches(".md");
        let Some((num, _)) = stem.split_once('-') else {
            continue;
        };
        if num.len() != 4 || !num.chars().all(|c| c.is_ascii_digit()) {
            continue; // proposal-* and README are unnumbered on purpose
        }
        out.entry(num.to_string())
            .or_default()
            .push(stem.to_string());
    }
    for v in out.values_mut() {
        v.sort();
    }
    out
}

/// The collisions that exist, each with the file the index means and why it stays.
const KNOWN_COLLISIONS: &[(&str, &[&str])] = &[
    (
        "0009",
        &[
            "0009-self-contained-installer",
            "0009-unreconciled-publish-is-a-debt",
        ],
    ),
    (
        "0010",
        &[
            "0010-linkify-is-off-by-default",
            "0010-two-products-one-repository",
        ],
    ),
];

#[test]
fn no_adr_number_is_shared_beyond_the_two_recorded_collisions() {
    let alloc = allocation();
    let mut new_collisions = Vec::new();
    for (num, files) in &alloc {
        if files.len() < 2 {
            continue;
        }
        let known = KNOWN_COLLISIONS.iter().any(|(n, f)| {
            n == num && {
                let mut want: Vec<String> = f.iter().map(|s| s.to_string()).collect();
                want.sort();
                &want == files
            }
        });
        if !known {
            new_collisions.push(format!("{num} -> {}", files.join(", ")));
        }
    }
    assert!(
        new_collisions.is_empty(),
        "A NEW ADR number collision appeared: {}. An ADR number is an identifier — \
         `docs/adr/README.md` calls a numbered file an ADOPTED decision — so two files \
         under one number make every citation of it ambiguous. Take the next free number \
         instead. The two EXISTING collisions (0009, 0010) are recorded as exceptions \
         here because `docs/adr/README.md:28` forbids renumbering existing records.",
        new_collisions.join("; "),
    );
}

#[test]
fn the_recorded_collisions_still_exist_and_still_name_those_files() {
    let alloc = allocation();
    for (num, files) in KNOWN_COLLISIONS {
        let got = alloc.get(*num).map(|v| v.as_slice()).unwrap_or(&[]);
        let mut want: Vec<String> = files.iter().map(|s| s.to_string()).collect();
        want.sort();
        assert_eq!(
            got,
            want.as_slice(),
            "the recorded exception for ADR {num} no longer matches the directory. If a \
             collision was CLOSED, delete its entry here in the same commit and say which \
             file moved and why — a stale exception is how an exception becomes a licence.",
        );
    }
}
