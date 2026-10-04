//! **THE PANEL'S FIRST SCREEN, PINNED TO ONE COMMAND — BECAUSE THE CENSUS MIXED TWO.**
//!
//! ── THE MUTATION THAT MUST FAIL THIS GATE ───────────────────────────────────────────────────────
//! Read this when you change this file: the mutation is how you find out whether the check can still fail at
//! all. A gate that cannot be broken is worse than no gate.
//!
//! MUTATION: add a kilobyte of comments to `agent/resources/panel/panel.js` and rebuild nothing.
//! RESULT:   the `panel.js` row fails, naming the old and new gzip sizes and the total. (Measured on the
//!           port: the row is what catches it.)
//!
//! WHY IT EXISTS. The migration's P2 rows record three numbers per family — wasm gz, the glue's increment on
//! the first screen, and the first screen's own gz — and on 2026-10-03 the third was found to be **a mix of
//! two different commands**:
//!
//! ```text
//!     artifact                    gzip -9 -nc   gzip -9 -c   the census said
//!     panel.js                       190,406       190,415      190,406   <- the -n form
//!     panel.css                       84,390        84,400       84,400   <- the named form
//!     panel_logic_bg.wasm             84,196        84,216       84,216   <- the named form
//!     index.html                       1,360         1,371         ~700   <- neither
//! ```
//!
//! **`gzip` WRITES THE ORIGINAL FILENAME AND MTIME INTO ITS HEADER**, so `gzip -9 -c panel.js` is nine bytes
//! larger than the same bytes through a pipe — nine being `panel.js` plus its NUL. The census's total,
//! `~359,722`, is `190,406 + 84,400 + 84,216 + ~700`: one row from each form and one approximation. **It is
//! not reproducible, and this repository's rule is that a number whose command is not beside it is a number
//! that drifts.**
//!
//! `-n` IS THE FORM HERE, because it is the reproducible one: no filename, no mtime, the same answer from a
//! pipe and from a file. Every row below was measured with `gzip -9 -nc <file> | wc -c` on 2026-10-03.
//!
//! A TOLERANCE, AND WHY IT IS NOT A LOOPHOLE. DEFLATE output is deterministic for a given zlib, and a
//! DIFFERENT zlib can shift a large file by a few hundredths of a percent. The bound is ±0.5%, which is far
//! below any real change (the smallest artifact here is 1,360 bytes, where 0.5% is 7 bytes) and far above a
//! zlib revision. **A gate that fails on a zlib upgrade is a gate somebody disables.**

use std::process::Command;

/// One artifact of the first screen: its path, its measured gzip size, and what it is.
pub struct Artifact {
    pub path: &'static str,
    pub gz: u64,
    pub what: &'static str,
}

/// **THE PANEL'S FIRST SCREEN**, measured 2026-10-03 with `gzip -9 -nc <path> | wc -c`. The wasm and the
/// sheet are the two the panel preloads or links from `index.html`; the JS carries the wasm-bindgen glue
/// inside it, which is why its number is the largest.
pub const PANEL_FIRST_SCREEN: [Artifact; 4] = [
    Artifact {
        path: "agent/resources/panel/panel.js",
        gz: 190406,
        what: "the SPA bundle, with the wasm-bindgen glue inside it",
    },
    Artifact {
        path: "agent/resources/panel/panel.css",
        gz: 84390,
        what: "the built sheet",
    },
    Artifact {
        path: "agent/resources/panel/panel_logic_bg.wasm",
        gz: 84196,
        what: "the logic crate, preloaded by index.html",
    },
    Artifact {
        path: "agent/resources/panel/index.html",
        // **1,074, NOT 1,360 — AND THE FIRST VERSION OF THIS FILE PINNED THE WRONG FILE'S NUMBER.**
        // 1,360 is `gateway/public/index.html`, the CONSOLE's document; the panel's is 1,074. The gate
        // caught it on its first run, which is the same class of error as the census mixing two gzip
        // forms: a number that looks right because a neighbouring artifact has it.
        gz: 1074,
        what: "the document itself",
    },
];

/// `gzip -9 -nc <path> | wc -c`, in Rust: no filename, no mtime, the reproducible form.
pub fn gz_size(root: &std::path::Path, rel: &str) -> u64 {
    let path = root.join(rel);
    let out = Command::new("gzip")
        .args(["-9", "-nc"])
        .arg(&path)
        .output()
        .unwrap_or_else(|e| panic!("gzip {}: {e}", path.display()));
    assert!(
        out.status.success(),
        "gzip -9 -nc {} failed: {}",
        path.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout.len() as u64
}

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the repository root")
        .to_path_buf()
}

/// The bound, in bytes, for a measured size: ±0.5%, and at least one byte so a tiny artifact is still pinned.
pub fn tolerance(gz: u64) -> u64 {
    (gz / 200).max(1)
}

#[test]
fn the_panel_first_screen_is_what_the_census_says() {
    let root = root();
    let mut total = 0u64;
    let mut failures: Vec<String> = Vec::new();
    for a in &PANEL_FIRST_SCREEN {
        let got = gz_size(&root, a.path);
        total += got;
        let tol = tolerance(a.gz);
        if got.abs_diff(a.gz) > tol {
            failures.push(format!(
                "  {} — census {} gz, measured {got} gz ({}), tolerance ±{tol}",
                a.path, a.gz, a.what
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "FAIL the panel's first screen moved:\n{}\n\
         Measure with: gzip -9 -nc <path> | wc -c   (the -n form: no filename, no mtime)",
        failures.join("\n")
    );
    // **AND THE TOTAL, BECAUSE THE CENSUS'S TOTAL IS THE NUMBER THAT WAS WRONG.** It is what the P2 row
    // publishes, and it was `190,406 + 84,400 + 84,216 + ~700` — one row from each gzip form and one
    // approximation.
    let want_total: u64 = PANEL_FIRST_SCREEN.iter().map(|a| a.gz).sum();
    let tol = tolerance(want_total);
    assert!(
        total.abs_diff(want_total) <= tol,
        "FAIL the panel's first screen totals {total} gz, and the census says {want_total} gz \
         (tolerance ±{tol}). The rows are pinned individually; this is the sum the P2 row publishes."
    );
    println!(
        "first-screen: {total} gz over {} artifact(s) — {}",
        PANEL_FIRST_SCREEN.len(),
        PANEL_FIRST_SCREEN
            .iter()
            .map(|a| format!("{} {}", a.path.rsplit('/').next().unwrap_or(a.path), a.gz))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

#[test]
fn the_measuring_form_is_the_reproducible_one() {
    // `-n` is the whole reason this file exists: with the filename in the header, the same bytes answer
    // differently through a pipe than from a file, and the census mixed the two.
    let root = root();
    let named = Command::new("gzip")
        .args(["-9", "-c"])
        .arg(root.join("agent/resources/panel/panel.js"))
        .output()
        .expect("gzip");
    let unnamed = gz_size(&root, "agent/resources/panel/panel.js");
    assert_eq!(
        named.stdout.len() as u64 - unnamed,
        9,
        "panel.js plus its NUL"
    );
    // And the bytes AFTER the header are identical, which is what makes the difference purely a header.
    // **COMPARED AS A DIGEST, NOT AS SLICES**: `assert_eq!` on two 190 KB vectors PRINTS BOTH, and a gate
    // whose failure output is 380 KB of binary is a gate nobody can read. Measured — the first version of
    // this did exactly that.
    let unnamed_out = Command::new("gzip")
        .args(["-9", "-nc"])
        .arg(root.join("agent/resources/panel/panel.js"))
        .output()
        .expect("gzip");
    assert_eq!(
        named.stdout.len(),
        unnamed_out.stdout.len() + 9,
        "the only difference between the two forms is the 9-byte header"
    );
    // **THE DEFLATE STREAM STARTS AT 19 IN THE NAMED FORM, NOT 10.** The gzip header is ten fixed bytes plus
    // an optional FNAME field — which is `panel.js` and its NUL, nine bytes — so the named form's stream
    // begins nine bytes later. Comparing from 10 on both sides compares different regions, which is what the
    // first version of this did, and it failed for that reason rather than for a real difference.
    assert!(
        named.stdout[19..] == unnamed_out.stdout[10..],
        "the deflate stream differs between the named and unnamed forms, so `-n` is not just a header"
    );
}
