//! **THE PANEL'S FIRST SCREEN, PINNED TO ONE COMMAND — BECAUSE THE CENSUS MIXED TWO.**
//!
//! ── THE MUTATION THAT MUST FAIL THIS GATE ───────────────────────────────────────────────────────
//! Read this when you change this file: the mutation is how you find out whether the check can still fail at
//! all. A gate that cannot be broken is worse than no gate.
//!
//! MUTATION: append **2 KB OF RANDOM BYTES** to `gateway/public/ui_logic_bg.wasm`:
//!           `head -c 2048 /dev/urandom >> gateway/public/ui_logic_bg.wasm`
//! RESULT:   `gateway/public/ui_logic_bg.wasm — census 15762 gz, measured 18216 gz …, tolerance ±78`, exit 1.
//!           (Measured 2026-10-03.)
//!
//! **AND THE FIRST TWO MUTATIONS TRIED DID NOT BITE, WHICH IS A LESSON ABOUT THIS GATE'S SUBJECT.** Its
//! subject is the COMPRESSED size, so a mutation has to change it:
//!
//!   * **one byte appended** — absorbed by the deflate stream; the gzip size did not move at all.
//!   * **1 KB of `'A'`** — a run of one repeated byte compresses to almost nothing, so 1,024 bytes of input
//!     changed the output by ~20.
//!   * **2 KB from `/dev/urandom`** — incompressible, and the row failed by 2,454 bytes against a ±78 bound.
//!
//! A mutation for a size gate must be INCOMPRESSIBLE. The earlier version of this header prescribed "a
//! kilobyte of comments", which is real text and compresses to a few hundred bytes — **below `panel.js`'s
//! ±952 bound, so it would have proved nothing.**
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

use std::io::Write;
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

/// **THE CONSOLE'S FIRST SCREEN**, measured 2026-10-03 with the same command. Its assets carry CONTENT HASHES
/// in their names, so the paths cannot be pinned — the files are found by shape, and a changed hash simply
/// means changed bytes, which the size assertion then judges.
///
/// **ITS ROW HAD THE SAME DISEASE AS THE PANEL'S**: `js` was measured the `-n` way and matched, while `html`,
/// `css` and the wasm were measured WITH the filename — and every difference was exactly the basename's
/// length plus one, which is the arithmetic that settles the cause:
///
/// ```text
///     index.html          1,371 - 1,360 = 11 = "index.html" (10) + NUL
///     index-B1XnjM77.css  7,090 - 7,071 = 19 = "index-B1XnjM77.css" (18) + NUL
///     ui_logic_bg.wasm   15,779 - 15,762 = 17 = "ui_logic_bg.wasm" (16) + NUL
///     index-CbfNO-ug.js 106,420 - 106,420 = 0  <- this one WAS measured with -n
/// ```
pub const CONSOLE_FIRST_SCREEN: [Artifact; 2] = [
    Artifact {
        path: "gateway/public/index.html",
        gz: 1360,
        what: "the document itself",
    },
    Artifact {
        path: "gateway/public/ui_logic_bg.wasm",
        gz: 15762,
        what: "the logic crate; index.html compiles it inline, so there is no wasm-bindgen glue",
    },
];

/// **THE SPA BUNDLE'S SIZE, PINNED — ITS NAME IS NOT.** A console edit re-hashes the file
/// (`index-CbfNO-ug.js` -> `index-DAMlHm4x.js` on 2026-10-06, for the two provider-form fields that round
/// added), and a pinned NAME turns every UI change into a red `agent` job whose message is "No such file or
/// directory" rather than a size. So the file is found BY SHAPE — the way `console_sheet_gz` already finds the
/// stylesheet — and what is asserted is the number the census is about: 106,420 -> 106,826 gz, measured with
/// `gzip -9 -nc <file> | wc -c`.
pub const CONSOLE_BUNDLE_GZ: u64 = 106826;

/// The bundle's hashed name, found by shape rather than pinned. Returns the gzip size.
fn console_bundle_gz(root: &std::path::Path) -> (String, u64) {
    let dir = root.join("gateway/public/assets");
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("index-") && n.ends_with(".js"))
        .collect();
    found.sort();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one hashed bundle under gateway/public/assets, found {found:?}"
    );
    let rel = format!("gateway/public/assets/{}", found[0]);
    let gz = gz_size(root, &rel);
    (rel, gz)
}

/// The sheet's hashed name, found by shape rather than pinned. Returns the gzip size.
fn console_sheet_gz(root: &std::path::Path) -> (String, u64) {
    let dir = root.join("gateway/public/assets");
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("index-") && n.ends_with(".css"))
        .collect();
    found.sort();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one hashed sheet under gateway/public/assets, found {found:?}"
    );
    let rel = format!("gateway/public/assets/{}", found[0]);
    let gz = gz_size(root, &rel);
    (rel, gz)
}

#[test]
fn the_console_first_screen_is_what_the_census_says() {
    let root = root();
    let mut total = 0u64;
    let mut failures: Vec<String> = Vec::new();
    for a in &CONSOLE_FIRST_SCREEN {
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
    // The sheet, by shape.
    let (sheet, sheet_gz) = console_sheet_gz(&root);
    total += sheet_gz;
    // The bundle, by shape — its SIZE is what this census pins.
    let (bundle, bundle_gz) = console_bundle_gz(&root);
    total += bundle_gz;
    let bundle_tol = tolerance(CONSOLE_BUNDLE_GZ);
    if bundle_gz.abs_diff(CONSOLE_BUNDLE_GZ) > bundle_tol {
        failures.push(format!(
            "  {bundle} — census {CONSOLE_BUNDLE_GZ} gz, measured {bundle_gz} gz (the SPA bundle), tolerance ±{bundle_tol}"
        ));
    }
    // **THE SHEET'S NUMBER IS NOT PINNED, BECAUSE ITS NAME IS NOT STABLE** — a rebuilt sheet has a new hash
    // and a new size, and both are legitimate. What is pinned is that it is THERE and plausible: a console
    // sheet is tens of kilobytes, not zero and not a megabyte.
    assert!(
        (1000..200_000).contains(&sheet_gz),
        "FAIL {sheet} gzips to {sheet_gz} bytes, which is not a plausible console sheet"
    );
    assert!(
        failures.is_empty(),
        "FAIL the console's first screen moved:\n{}\n\
         Measure with: gzip -9 -nc <path> | wc -c   (the -n form: no filename, no mtime)",
        failures.join("\n")
    );
    println!(
        "console-first-screen: {total} gz over {} artifact(s) + the sheet — {}",
        CONSOLE_FIRST_SCREEN.len(),
        CONSOLE_FIRST_SCREEN
            .iter()
            .map(|a| format!("{} {}", a.path.rsplit('/').next().unwrap_or(a.path), a.gz))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// **THE GLUE'S INCREMENT, BOTH ENDS.** The P2 row publishes `+2,428 gz` for the panel — "what the wasm cost
/// the first screen" — and it is the difference between `panel.js` before the wasm landed and now. **The
/// second end is pinned above; this pins the first**, by reading the file out of the commit before the
/// migration touched it. The command is the same one, and `git` is a tool every checkout has — which is the
/// same reading that let `main-shape-check` come across.
///
/// **WHY IT MATTERS THAT BOTH ENDS ARE PINNED**: an increment is a claim about two numbers, and a gate that
/// pins one of them pins half a claim. Measured 2026-10-03: `git show 462cfc89^:agent/resources/panel/panel.js
/// | gzip -9 -n | wc -c` -> 187,978, and HEAD -> 190,406, which is the row's +2,428.
const PANEL_JS_BEFORE_THE_WASM: (&str, u64) = ("462cfc89^", 187978);

/// `Some(size)` when the object is in this clone, `None` when it is not.
///
/// **A SHALLOW CLONE DOES NOT HAVE IT, AND THAT IS NOT A DEFECT IN THE TREE.** `actions/checkout@v4` defaults
/// to `fetch-depth: 1`; the `agent` job is given `fetch-depth: 0` precisely so this gate can run there, but
/// the umask loop in `pack-chain` runs `all-gates.bash`, which runs `cargo test`, in a clone that is still
/// shallow. **So this returns `None` rather than panicking — and the caller SAYS SO LOUDLY**, because a gate
/// that silently passes is worse than one that fails.
fn gz_size_of_blob(rev_path: &str) -> Option<u64> {
    let root = root();
    let git = Command::new("git")
        .args(["show", rev_path])
        .current_dir(&root)
        .output()
        .unwrap_or_else(|e| panic!("git show {rev_path}: {e}"));
    if !git.status.success() {
        return None;
    }
    let mut child = Command::new("gzip")
        .args(["-9", "-n"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("gzip");
    child
        .stdin
        .as_mut()
        .expect("gzip stdin")
        .write_all(&git.stdout)
        .expect("write to gzip");
    let out = child.wait_with_output().expect("gzip output");
    assert!(out.status.success(), "gzip failed");
    Some(out.stdout.len() as u64)
}

#[test]
fn the_glue_increment_has_both_its_ends() {
    let (rev, before) = PANEL_JS_BEFORE_THE_WASM;
    let Some(got_before) = gz_size_of_blob(&format!("{rev}:agent/resources/panel/panel.js")) else {
        // **LOUD, NOT SILENT.** Measured in CI on 2026-10-03: this gate passed every local run and failed the
        // `agent` job, because `462cfc89^` is not in a `fetch-depth: 1` clone. The job fetches the history
        // now; this branch is for the clones that do not, and it says which measurement it could not make.
        println!(
            "glue-increment: NOT MEASURED — {rev} is not in this clone (a shallow checkout). \
             The `agent` job fetches the history; run this from a full clone to check the increment."
        );
        return;
    };
    let got_now = gz_size(&root(), "agent/resources/panel/panel.js");
    assert!(
        got_before.abs_diff(before) <= tolerance(before),
        "FAIL the panel.js from {rev} gzips to {got_before}, and the row's increment starts from {before}"
    );
    // **THE INCREMENT ITSELF**, which is the number the P2 row publishes.
    let increment = got_now - got_before;
    assert_eq!(
        increment, 2428,
        "FAIL the panel's glue increment is {increment} gz ({before} -> {got_now}), and the P2 row says 2,428. \
         Measure both ends with: git show <rev>:agent/resources/panel/panel.js | gzip -9 -n | wc -c"
    );
    println!("glue-increment: {increment} gz ({before} -> {got_now})");
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
