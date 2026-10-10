//! THE npm PACKAGE'S OWN SUITE, IN RUST — what npm would UPLOAD, checked against the two lists that
//! describe it.
//!
//! ── WHAT THIS REPLACES, AND WHY THE CLASS DEMANDED IT ─────────────────────────────────────────────
//!
//! `agent/summrise-agent-npm/test/packaging.test.mjs` held ten cases and was the last `LOGIC` file in
//! that package. It was classified LOGIC for a reason worth keeping in view: **every one of its cases
//! is a VALIDATION** — the `bin` target is one npm will pack, no `files[]` entry is a directory, the
//! required list and `files[]` agree, no component artefact is boxed — and the class's own rule is
//! "it computes, parses, derives, validates, decides or formats → **must become Rust**".
//!
//! **THE SPAWN STAYS A BOUNDARY AND THE CHECKS ARE HERE.** The one thing this file asks a tool is
//! `npm pack --dry-run --json`, which DECIDES the entry list — it is asked to PACK, never to judge.
//! Everything else is a Rust assertion over files this repository commits, so the division is the one
//! the boundary manifest describes rather than a compromise.
//!
//! ── WHY IT CAN SPAWN npm AT ALL, AND WHERE THE SPAWN RUNS ─────────────────────────────────────────
//!
//! The package has **zero dependencies and no lockfile** — `npm ci` there fails with `EUSAGE`, which
//! the agent job's own notes record — so `npm pack` needs no install step and works in a fresh
//! checkout. It runs in `agent/tests/`, which is the `summrise-agent` crate: a DEFAULT member whose
//! `cargo test -p summrise-agent --no-fail-fast` is already a step of ci.yml's `agent` job, and that
//! job already sets up node 24 for the gates that build and render the console. **No ci.yml change is
//! needed for this file to be tested** — which is the fact that decided where it lives, because a
//! workspace member that is not a default member is a crate no job tests unless it is named there.
//!
//! ── WHAT THE TARBALL'S ENTRY LIST IS COMPARED AGAINST, AND WHAT IS DELIBERATELY EXCLUDED ──────────
//!
//! Against **`required-in-tgz.txt`**, minus its three `*.exe` entries. Those three are cargo-xwin
//! output: gitignored, absent from a fresh checkout, and absent from CI's own pack job for exactly
//! that reason (its loop skips them by name). Comparing them here would be a test that fails wherever
//! the artifact has not been built — including every developer's first run — so the exclusion is the
//! same one ci.yml makes, made in the same direction.
//!
//! What is NOT excluded is everything else: this file is the only place that asks npm to enumerate the
//! tarball and then holds that list against the committed required list. The `.mjs` suite never
//! packed; it compared the two LISTS to each other. So the port adds a link rather than moving one.
//!
//! ── THE MUTATIONS, EACH RUN AND QUOTED ───────────────────────────────────────────────────────────
//!
//! MUTATION: `package.json`'s `bin.summrise` → `"bin/summrise.js"` →
//! RESULT: `the_cli_is_the_rust_binary_and_the_compiled_pair_stays_deleted` fails with "the npm bin
//! must be the Rust CLI" and `the_bin_target_is_one_npm_will_pack` fails with "files[] must declare
//! bin/summrise.js".
//!
//! MUTATION: `files[]` gains `"bin/"` (a DIRECTORY, the round-278 hazard) →
//! RESULT: `no_files_entry_is_a_directory` fails naming `["bin/"]`.
//!
//! MUTATION: `bin/summrise.exe` is deleted from `files[]` →
//! RESULT: `the_bin_target_is_one_npm_will_pack` and `every_required_in_tgz_entry_is_declared_in_files`
//! both fail, the second naming the entry.
//!
//! MUTATION: `required-in-tgz.txt` gains a line for a file that exists but is not in `files[]` →
//! RESULT: `every_required_in_tgz_entry_is_declared_in_files` fails with "npm pack cannot satisfy the
//! release gate".
//!
//! MUTATION: `required-in-tgz.txt` names `README.md` as `READMEE.md` →
//! RESULT: `the_packed_tarball_carries_every_required_file` fails with "npm would not pack it", which
//! is the case the `.mjs` suite could not make at all.
//!
//! MUTATION: `package.json`'s `os` → `["linux"]` →
//! RESULT: `the_package_is_windows_x64_only` fails with "the package ships Windows binaries".

mod common;

use common::{read, repo, Spawn};
use serde_json::Value;

/// The package under test, relative to the repository root.
const PKG: &str = "agent/summrise-agent-npm";

/// THE THREE EXES ARE CARGO-XWIN OUTPUT: gitignored, absent from a fresh checkout, and skipped by
/// ci.yml's own pack job for that reason. See the header.
const EXES: [&str; 3] = [
    "summrise-agent.exe",
    "summrise-launch.exe",
    "bin/summrise.exe",
];

fn manifest() -> Value {
    let raw = read(&format!("{PKG}/package.json"));
    serde_json::from_str(&raw).expect("package.json must parse")
}

fn files_list(m: &Value) -> Vec<String> {
    m["files"]
        .as_array()
        .expect("package.json must declare files[]")
        .iter()
        .map(|f| {
            f.as_str()
                .expect("every files[] entry is a string")
                .to_string()
        })
        .collect()
}

/// `required-in-tgz.txt`: one path per line, `#` comments and blanks ignored. The ONE owner of the
/// list — `scripts/publish-release.sh`, `release.yml` and CI's pack gate all read this file.
fn required_in_tgz() -> Vec<String> {
    read(&format!("{PKG}/required-in-tgz.txt"))
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// WHAT npm WOULD PACK, from npm itself — `npm pack --dry-run --json`, whose `files[].path` is the
/// entry list it would write and which writes nothing at all. THE BOUNDARY: npm is asked to pack, and
/// every judgement about the answer is made here.
fn packed_paths() -> Vec<String> {
    let out = Spawn::new("npm")
        .args(["pack", "--dry-run", "--json"])
        .cwd(repo().join(PKG))
        .run();
    assert!(
        out.ok(),
        "`npm pack --dry-run --json` must succeed in {PKG} (it needs no install: the package has zero \
         dependencies and no lockfile) — rc={} stdout={:?} stderr={:?}",
        out.code(),
        out.stdout,
        out.stderr
    );
    let parsed: Value = serde_json::from_str(&out.stdout)
        .unwrap_or_else(|e| panic!("npm pack --json must print JSON: {e}\n{}", out.stdout));
    let list = parsed
        .as_array()
        .and_then(|a| a.first())
        .unwrap_or_else(|| {
            panic!(
                "npm pack --json must print one entry per package: {}",
                out.stdout
            )
        });
    let paths: Vec<String> = list["files"]
        .as_array()
        .unwrap_or_else(|| panic!("npm pack --json must carry files[]: {}", out.stdout))
        .iter()
        .filter_map(|f| f["path"].as_str().map(str::to_string))
        .collect();
    assert!(
        !paths.is_empty(),
        "npm answered with an EMPTY entry list — a scan that read nothing must not pass: {}",
        out.stdout
    );
    paths
}

fn exists(rel: &str) -> bool {
    repo().join(rel).exists()
}

#[test]
fn the_cli_is_the_rust_binary_and_the_compiled_pair_stays_deleted() {
    let m = manifest();
    assert_eq!(
        m["bin"]["summrise"].as_str(),
        Some("bin/summrise.exe"),
        "the npm bin must be the Rust CLI — `cargo xwin build -p summrise-cli` output staged by \
         scripts/build.sh. MEASURED before that was chosen: npm's shim (cmd-shim 8.0.0, bundled with \
         npm 11.19.0) reads the bin target, finds no shebang, and generates a .cmd that runs the file \
         DIRECTLY — no `node` in between — so a native .exe is deliverable by npm."
    );
    // The TypeScript CLI, its tsc emit and its project file are gone with the cutover, and nothing
    // regenerates them: a stale copy left on a box would be a second, unbuilt CLI beside the one that
    // ships.
    for gone in ["src/summrise.ts", "bin/summrise.js", "tsconfig.json"] {
        assert!(
            !exists(&format!("{PKG}/{gone}")),
            "{gone} is back. It was deleted with landing 4b's cutover and nothing regenerates it — \
             delete it rather than leaving a second CLI in the package."
        );
    }
}

#[test]
fn the_bin_target_is_one_npm_will_pack() {
    let m = manifest();
    let files = files_list(&m);
    let bin = m["bin"]["summrise"]
        .as_str()
        .expect("bin.summrise must be a string");
    // `files[]` is an ALLOWLIST: a bin target outside it is a command npm links to a file the tarball
    // does not contain. The install still succeeds and prints `added 1 package`; `summrise` is simply
    // not there.
    assert!(
        files.iter().any(|f| f == bin),
        "files[] must declare {bin}, or the tarball ships a bin it does not contain: {files:?}"
    );
    assert!(
        bin.ends_with(".exe"),
        "a non-.exe bin target is the generated-JS arrangement the cutover removed: {bin}"
    );
}

#[test]
fn no_files_entry_is_a_directory() {
    let dirs: Vec<String> = files_list(&manifest())
        .into_iter()
        .filter(|f| f.ends_with('/'))
        .collect();
    // THE ROUND-278 LESSON, kept as a rule rather than as a comment: `files: ["bin/"]` passed a "does
    // the entry exist" check while the CLI itself was missing, because the directory existed.
    assert!(
        dirs.is_empty(),
        "these files[] entries are directories, so a gate over them can pass with the file inside \
         missing — name the files: {dirs:?}"
    );
}

#[test]
fn every_required_in_tgz_entry_is_declared_in_files() {
    let files = files_list(&manifest());
    let required = required_in_tgz();
    assert!(
        required.len() >= 5,
        "required-in-tgz.txt answered {} entries — a scan that read almost nothing must not pass",
        required.len()
    );
    for entry in &required {
        // THE DIRECTION `scripts/test/build-pins.bash` DOES NOT CHECK (it asserts the other one: every
        // files[] entry is named in required-in-tgz.txt). This one fails at a different moment: a file
        // required by the release gates but EXCLUDED from the pack is a tarball that cannot satisfy
        // its own release check, and the failure lands in the release job after the version is bumped.
        assert!(
            files.iter().any(|f| f == entry),
            "required-in-tgz.txt requires {entry} and files[] does not pack it — npm pack cannot \
             satisfy the release gate. Either add it to files[] or take it off the required list."
        );
    }
}

#[test]
fn the_packed_tarball_carries_every_required_file() {
    let packed = packed_paths();
    let required = required_in_tgz();
    let mut checked = 0;
    for entry in &required {
        // See EXES: the three cargo-xwin artefacts are absent here and in CI's pack job, which skips
        // them by name for the same reason.
        if EXES.contains(&entry.as_str()) {
            continue;
        }
        checked += 1;
        assert!(
            packed.iter().any(|p| p == entry),
            "npm would not pack {entry}, which required-in-tgz.txt requires — the tarball a device \
             installs would be missing it. npm's list: {packed:?}"
        );
    }
    assert!(
        checked >= 5,
        "only {checked} required entries were checkable against npm's list — the exclusion list has \
         grown past the exes, and this gate is no longer measuring the tarball"
    );
    // AND npm's list is not the only direction: every packed path must be DECLARED, so a file that
    // arrives by npm's defaults (package.json, README) rather than by files[] is visible here.
    let files = files_list(&manifest());
    for p in &packed {
        assert!(
            files.iter().any(|f| f == p) || p == "package.json",
            "{p} would be packed but files[] does not declare it — files[] is an allowlist and this \
             is the one path npm adds by itself (package.json). files[]: {files:?}"
        );
    }
}

#[test]
fn no_boxed_component_artefact_is_in_the_package() {
    // The package is ~6.7 MB and carries NO component: `setup` FETCHES cloudflared (54 MB), the
    // playwright bundle (31 MB) and the electron runtime, verifying each against the release
    // manifest's sha256 pin. A boxed copy here would be a package nobody can upload, and one whose
    // component is older than the pin that describes it.
    let files = files_list(&manifest());
    let required = required_in_tgz();
    for artefact in [
        "cloudflared.exe",
        "summrise-playwright.zip",
        "electron-win32-x64.zip",
    ] {
        assert!(
            !files.iter().any(|f| f == artefact),
            "{artefact} is a FETCHED component, not a packaged one — it must not be in files[]"
        );
        assert!(
            !required.iter().any(|f| f == artefact),
            "{artefact} is a FETCHED component — it must not be required in the tgz"
        );
    }
}

#[test]
fn the_launcher_and_the_agent_exe_are_both_shipped() {
    let files = files_list(&manifest());
    let required = required_in_tgz();
    // A scheduled task whose action is a program the package does not carry is Task Scheduler's
    // `0x2`, five minutes later, forever.
    for exe in ["summrise-agent.exe", "summrise-launch.exe"] {
        assert!(
            files.iter().any(|f| f == exe),
            "the package must ship {exe}: {files:?}"
        );
        assert!(
            required.iter().any(|f| f == exe),
            "required-in-tgz.txt must require {exe}, or the release gates cannot see it go missing"
        );
    }
}

#[test]
fn the_desktop_shells_sources_and_icons_are_shipped() {
    let files = files_list(&manifest());
    let required = required_in_tgz();
    // THE FILES ARE PAIRS, AND A PAIR IS WHY THIS IS NAMED RATHER THAN COUNTED: `main.js` requires
    // `./summrise_url_policy`, which requires `./summrise_url_policy_bg.wasm`, and the shell policy
    // has the same shape one module over. A tarball carrying the JavaScript and not the module starts
    // a shell that dies with `Cannot find module` — the defect landing 6 recorded, which is why the
    // four names of that era are six here and `summrise-cli`'s `swap.rs` stages the same set.
    //
    // NAMED AS WELL AS DERIVED (the loop below): naming is what fails when a file leaves BOTH lists,
    // and the derivation is what fails when a NEW shell file joins `files[]` and nobody adds it to the
    // required list. Neither alone covers the other's case.
    for f in [
        "summrise-desktop-electron/src/main.js",
        "summrise-desktop-electron/src/preload.js",
        "summrise-desktop-electron/src/summrise_url_policy.js",
        "summrise-desktop-electron/src/summrise_url_policy_bg.wasm",
        "summrise-desktop-electron/src/summrise_shell_policy.js",
        "summrise-desktop-electron/src/summrise_shell_policy_bg.wasm",
        "summrise-desktop-electron/icon.png",
        "summrise-desktop-electron/icon.ico",
    ] {
        assert!(files.iter().any(|x| x == f), "the package must ship {f}");
        assert!(
            required.iter().any(|x| x == f),
            "required-in-tgz.txt must require {f}"
        );
    }
    // Every shell file `files[]` packs must be required, whatever it is called: a name list is a pin,
    // and a pin that only knows yesterday's names is a pin that stops covering today's.
    for f in files
        .iter()
        .filter(|f| f.starts_with("summrise-desktop-electron/"))
    {
        assert!(
            required.iter().any(|x| x == f),
            "{f} is declared in files[] and NOT required by required-in-tgz.txt — a shell file the \
             release gates cannot see go missing. Add it to the required list."
        );
    }
}

#[test]
fn the_package_is_windows_x64_only() {
    let m = manifest();
    // It carries PE binaries and a `bin` whose shim story is measured on Windows. Installing it on a
    // platform it cannot run is the failure npm's `os`/`cpu` fields exist to refuse.
    assert_eq!(
        m["os"].as_array().map(|a| a.len()),
        Some(1),
        "the package ships Windows binaries and declares one platform"
    );
    assert_eq!(
        m["os"][0].as_str(),
        Some("win32"),
        "the package ships Windows binaries"
    );
    assert_eq!(m["cpu"][0].as_str(), Some("x64"), "and x64 ones");
}

#[test]
fn the_version_is_a_plain_semver() {
    // Load-bearing three ways: `release.yml` refuses a tag that disagrees with it, the CLI's own
    // `--version` acceptance check parses it, and `updateWouldNotMove` compares it against the
    // device's release marker. The CLI's version is COMPILED IN from this manifest, so a range or a
    // build suffix here is not a version any of those can read.
    let v = manifest()["version"]
        .as_str()
        .expect("package.json must carry a version")
        .to_string();
    // Hand-rolled rather than `regex`: this crate's tests do not link a regex engine, and a
    // dependency added for three lines is a dependency added for every build. The rule is the one the
    // CLI's own `version.rs` pins with a regex on the same manifest — three dot-separated runs of
    // digits, nothing else.
    let parts: Vec<&str> = v.split('.').collect();
    let ok = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    assert!(
        ok,
        "package.json's version must be a plain dotted triple, got {v:?}"
    );
}

#[test]
fn the_build_script_compiles_no_cli() {
    let build = manifest()["scripts"]["build"]
        .as_str()
        .expect("package.json must carry scripts.build")
        .to_string();
    // `-p tsconfig.json` AND NOT THE BARE NAME: the electron compile legitimately names
    // `../summrise-desktop-electron/tsconfig.json`, so a check for `tsconfig.json` matches that too.
    // The needle has to be the form the deleted compile used.
    for dead in ["bin/summrise.js", "summrise-fresh-bin", "-p tsconfig.json"] {
        assert!(
            !build.contains(dead),
            "the build script still names {dead}, which the cutover deleted: {build}"
        );
    }
    assert!(
        build.contains("summrise-desktop-electron"),
        "the build script's remaining job is the electron shell's emit: {build}"
    );
}
