//! A COMMENT THAT ASSERTS PARITY IS NOT A COMPARISON.
//!
//! Four files carry the same CORS allowlist: the gateway — which EXPORTS it and lets config override it
//! — and three copies that restate it: the relay's two JS handlers, and **ONE** Rust copy that serves
//! both satellites. (It was two copies until 2026-10-08; the port collapsed them, because `zen-go`'s
//! crate depends on `zen-us`'s and re-exports `cors_headers`, and a rule that exists once cannot drift
//! from itself.) Every copy's comment says it matches the others ("the console
//! origins used in this repo"), and NOTHING compared them. The one test whose name claims to be the
//! guard (`proxies/api-relay/api/test/proxy-gate.test.mjs:53`, "CORS matrix on the autonomous copy
//! (drift guard vs gateway http.ts)") imports `../proxy.js` and no gateway file at all — it checks the
//! copy against itself, which is the twelfth instance of this repository's recurring shape: an
//! instrument whose name promises more than its body does (round 116 of the standing goal).
//!
//! WHY IT MATTERS BEYOND TIDINESS: the gateway's list can be EXTENDED BY CONFIGURATION, and the
//! proxies' cannot. So an origin added for production works on the console and is refused by every
//! proxy — a browser error in one place and not the other, with the four copies agreeing with each
//! other and with a list that has already moved. The gateway owns the fact; the proxies must not
//! disagree with it.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28): both print
//! the same closing line, byte for byte, and a planted origin in one copy moves BOTH to the same
//! missing/extra body. The measurements are in the commit.
//!
//! MUTATION: add an origin to one proxy's `ALLOWED_ORIGINS`, or take one out.
//! RESULT:   fails, naming the copy, the direction and the origin, after the paragraph that says the
//!           gateway's list can be extended by configuration and the copies cannot.
//!
//! ONE DELIBERATE DIFFERENCE FROM THE JS, stated rather than glossed: the JS reads its files with
//! paths relative to the PROCESS's working directory, so it only works when run from the repo root
//! (the CI step does). This resolves against the repo root instead, because `cargo test`'s working
//! directory is the crate — a port that copied the JS here would read nothing and pass its own floor
//! check into the bargain.
//!
//! NO COMMENT STRIP HERE, AND THAT IS A CORRECTION RATHER THAN AN OMISSION — the JS's own words, kept
//! because the port inherits the reasoning: the first version stripped `//…` from this slice, and THE
//! STRIP ATE THE DATA — every origin is `https://…`, so the strip deleted each URL and its closing
//! quote with it and the check could read nothing from any file. The slice already starts at
//! `new Set([` and ends at `])`, so no comment can be inside it.

mod common;

use common::{chars, find_seq};
use std::fs;

const OWNER: &str = "gateway/src/http.ts";

/// **(file, how to find its list).** The list SHRANK from four copies to three on 2026-10-08, when the
/// satellites' JavaScript halves were deleted — and it shrank by TWO while gaining one, which is the
/// part worth reading: `zen-go` and `zen-us` each carried their own JS copy (which this gate existed to
/// keep in step), and the port keeps **one**, because `zen-go`'s crate depends on `zen-us`'s and
/// re-exports `cors_headers`. ADR 0003 keeps the satellites autonomous as DEPLOYMENTS; a shared Rust
/// crate is not a shared deployment. **A rule that exists once cannot drift from itself**, so the
/// migration removed a copy rather than porting it.
const COPIES: [(&str, &str); 3] = [
    ("proxies/api-relay/api/zen.js", "js"),
    ("proxies/api-relay/api/proxy.js", "js"),
    ("proxies/zen-us-proxy/worker/src/lib.rs", "rust"),
];

struct Streams {
    stdout: String,
    stderr: String,
    failed: bool,
}

/// The quoted strings inside a slice, in order — shared by both readers, because the two languages
/// differ only in where the slice STARTS.
fn quoted(body: &[char]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(open) = find_seq(body, "\"", i) {
        let Some(close) = find_seq(body, "\"", open + 1) else {
            break;
        };
        if close > open + 1 {
            out.push(body[open + 1..close].iter().collect());
        }
        i = close + 1;
    }
    out
}

/// The string literals inside the first `new Set([...])` that follows `ALLOWED_ORIGINS`, sorted.
fn origins_of(src: &str) -> Option<Vec<String>> {
    let c = chars(src);
    let at = find_seq(&c, "ALLOWED_ORIGINS", 0)?;
    let set = find_seq(&c, "new Set([", at)?;
    let end = find_seq(&c, "])", set)?;
    let mut out = quoted(&c[set..end]);
    out.sort();
    Some(out)
}

/// **THE RUST COPY, AND ITS SHAPE IS NOT THE JS's** — `pub const ALLOWED_ORIGINS: [&str; 2] = ["…"];`.
/// The anchor is `= [`, not the first `[`: the TYPE's bracket comes first in a Rust const, and anchoring
/// on it would slice `&str; 2` and read zero origins from a file that has two. `]` ends the slice (there
/// is no `])` in Rust), and no comment can be inside it.
fn origins_of_rust(src: &str) -> Option<Vec<String>> {
    let c = chars(src);
    let at = find_seq(&c, "ALLOWED_ORIGINS", 0)?;
    let eq = find_seq(&c, "= [", at)?;
    let end = find_seq(&c, "]", eq)?;
    let mut out = quoted(&c[eq..end]);
    out.sort();
    Some(out)
}

/// `None` means the ANCHORS are missing (`ALLOWED_ORIGINS`, `new Set([`, `])`) or the file cannot be
/// read; `Some(vec![])` means an allowlist was FOUND and is empty. The JS draws that line too —
/// `originsOf` returns `null` for the first and `[]` for the second, and `[]` is truthy — so an
/// emptied Set is reported as "disagrees with the owner, missing: …" rather than as a renamed copy.
/// (The JS has no try/catch around its reads, so a MISSING copy file is an uncaught ENOENT there and a
/// finding here; that difference is stated in the header.)
fn read_origins(rel: &str, form: &str) -> Option<Vec<String>> {
    let text = fs::read_to_string(common::repo().join(rel)).ok()?;
    if form == "rust" {
        origins_of_rust(&text)
    } else {
        origins_of(&text)
    }
}

fn check() -> Streams {
    let owner = match read_origins(OWNER, "js") {
        Some(o) if o.len() >= 2 => o,
        _ => {
            return Streams {
                stdout: String::new(),
                stderr: format!(
                    "  FAIL could not read an allowlist from {OWNER} — this proves nothing\n"
                ),
                failed: true,
            }
        }
    };

    let mut stderr = String::new();
    let mut bad = 0;
    for (f, form) in COPIES {
        let got = match read_origins(f, form) {
            Some(g) => g,
            None => {
                stderr.push_str(&format!(
                    "  FAIL {f}: no ALLOWED_ORIGINS found — the copy moved or was renamed\n"
                ));
                bad += 1;
                continue;
            }
        };
        let missing: Vec<&String> = owner.iter().filter(|o| !got.contains(o)).collect();
        let extra: Vec<&String> = got.iter().filter(|o| !owner.contains(o)).collect();
        if !missing.is_empty() || !extra.is_empty() {
            stderr.push_str(&format!("  FAIL {f} disagrees with {OWNER}:\n"));
            for m in missing {
                stderr.push_str(&format!("    missing: {m}\n"));
            }
            for e in extra {
                stderr.push_str(&format!("    extra:   {e}\n"));
            }
            bad += 1;
        }
    }

    if bad > 0 {
        stderr.push_str(&format!(
            "\n  {bad} CORS allowlist copy/copies disagree with the owner. The gateway's list can be extended by\n  \
             configuration and the copies cannot, so a disagreement is a browser error on one surface and not\n  \
             the other. Fix the copy, or move the origin into the owner and re-run.\n"
        ));
        return Streams {
            stdout: String::new(),
            stderr,
            failed: true,
        };
    }
    Streams {
        stdout: format!(
            "  ok — {} console origin(s) in {OWNER}, and all {} copies agree with it\n",
            owner.len(),
            COPIES.len()
        ),
        stderr,
        failed: false,
    }
}

#[test]
fn the_cors_allowlist_copies_agree_with_their_owner() {
    let out = check();
    print!("{}", out.stdout);
    if out.failed {
        panic!("{}", out.stderr);
    }
    assert!(
        out.stdout.contains("copies agree with it"),
        "{}",
        out.stdout
    );
}

// ── the reader's own proof: it must read the SET, and it must not eat the URLs ──────────────────────

#[test]
fn the_slice_is_the_set_and_the_urls_survive_it() {
    let src = "const ALLOWED_ORIGINS = new Set([\"https://a.example\", \"https://b.example\"]);";
    assert_eq!(
        origins_of(src).expect("an allowlist"),
        vec![
            "https://a.example".to_string(),
            "https://b.example".to_string()
        ]
    );
    // THE CORRECTION THE JS RECORDS, measured: a `//…`-to-end-of-line strip over the set's body leaves
    // `"https:` — an unterminated quote — so the check would read NOTHING from any file. That is why
    // there is no strip here, and it is not an omission.
    let body = "\"https://a.example\", \"https://b.example\"";
    let naive: String = body
        .split('\n')
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(naive, "\"https:");
    // The anchors are still there, so this is a FOUND-and-empty allowlist, not a renamed copy — which is
    // the distinction the JS draws with `null` against a truthy `[]`.
    assert_eq!(
        origins_of(&format!("ALLOWED_ORIGINS new Set([{naive}])")),
        Some(vec![])
    );
    assert!(origins_of("const x = 1;").is_none());
    // The slice ENDS at `])`, so a comment after the set cannot contribute an origin.
    let after = "const ALLOWED_ORIGINS = new Set([\"https://a.example\", \"https://b.example\"]);\n// https://comment.example\n";
    assert_eq!(origins_of(after).expect("an allowlist").len(), 2);
    // A file that does not mention the name at all is `None`, not an empty allowlist.
    assert!(origins_of("const x = 1;").is_none());
}

/// **THE RUST READER'S OWN PROOF, AND THE TRAP IT IS WRITTEN AGAINST.** A Rust const's FIRST bracket is
/// the TYPE's (`[&str; 2]`), so an anchor on `[` slices `&str; 2` and answers `Some(vec![])` — an EMPTY
/// allowlist, which this gate reports as a disagreement rather than as a reader that cannot read. The
/// first assertion is what fails if that anchor is ever "simplified".
#[test]
fn the_rust_reader_anchors_on_the_value_and_not_on_the_type() {
    let src =
        "pub const ALLOWED_ORIGINS: [&str; 2] = [\"https://a.example\", \"https://b.example\"];";
    assert_eq!(
        origins_of_rust(src).expect("an allowlist"),
        vec![
            "https://a.example".to_string(),
            "https://b.example".to_string()
        ]
    );
    // The slice ends at `]`, so a comment after the const cannot contribute an origin.
    let after = "pub const ALLOWED_ORIGINS: [&str; 1] = [\"https://a.example\"];\n// https://comment.example\n";
    assert_eq!(origins_of_rust(after).expect("an allowlist").len(), 1);
    // A file that does not mention the name is `None`, not an empty allowlist.
    assert!(origins_of_rust("pub const ALLOWED: [&str; 1] = [\"https://a.example\"];").is_none());
    assert!(origins_of_rust("const x = 1;").is_none());
}

#[test]
fn the_two_directions_are_named_and_a_disagreement_is_not_a_pass() {
    // The finding logic, without the file reads — so a disagreement can be proven on a fixture.
    let owner = [
        "https://a.example".to_string(),
        "https://b.example".to_string(),
    ];
    let got = [
        "https://a.example".to_string(),
        "https://c.example".to_string(),
    ];
    let missing: Vec<&String> = owner.iter().filter(|o| !got.contains(o)).collect();
    let extra: Vec<&String> = got.iter().filter(|o| !owner.contains(o)).collect();
    assert_eq!(missing, vec![&"https://b.example".to_string()]);
    assert_eq!(extra, vec![&"https://c.example".to_string()]);
    assert!(!missing.is_empty() && !extra.is_empty());
}
