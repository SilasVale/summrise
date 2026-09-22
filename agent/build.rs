//! Build script — panel bundle content hash + panel-first staleness gate.
//!
//! Two jobs, both driven by the panel SPA sources (no new dependencies —
//! pure std, so the Windows xwin cross-compile needs nothing extra):
//!
//! 1. PANEL_BUNDLE_HASH: FNV-1a-64 over `resources/panel/panel.js` +
//!    `panel.css`, exposed as a compile-time env var. `web/panel.rs`
//!    stamps `?v=<hash>` on the bundle URLs, so every panel rebuild gets a
//!    distinct cache key. (The previous key was `env!("CARGO_PKG_VERSION")`
//!    = the Cargo crate version, frozen at 1.0.x while the npm release rides
//!    1.2.x — Cloudflare's 4h Browser-Cache-TTL override for .js/.css kept
//!    serving the PREVIOUS panel for hours after an update.)
//!    FNV-1a is a cache key, NOT an integrity proof — no security property
//!    rides on it, so a non-cryptographic hash is the right tool.
//!
//! 2. STALENESS GATE (panel-first enforcement): `resources/panel/` holds
//!    COMMITTED vite build output (`vite outDir` overwrites it in place),
//!    so a `panel-react/src` edit without a rebuild silently ships the stale
//!    bundle (panel.js is `include_str!`'d at compile time). If the newest
//!    file under `resources/panel-react/src` is newer than the built
//!    products, the build FAILS with the rebuild command — fail loud, not
//!    stale. The gate only fires on genuine drift (see STALENESS_GRACE_SECS),
//!    never on a clean checkout, so `cargo test` stays green by default.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Fresh-checkout skew grace: a clean `git clone` writes every file within
/// seconds, and checkout order across directories is not contractual, so a
/// zero threshold could fail a pristine tree (measured 2026-09-06: the
/// committed products lag the newest src file by only ~11s even when the
/// tree is CONSISTENT — the vite build itself spans ~90s). Real drift (an
/// src edit followed by a build that skipped the panel rebuild) is minutes
/// to days. 120s clears both without letting real drift through, except a
/// sub-2-minute edit→build sprint with no panel rebuild — documented,
/// accepted: the gate is a safety net, not a proof.
const STALENESS_GRACE_SECS: u64 = 120;

/// Products the gate + hash cover: the two `?v=`-stamped bundles. index.html
/// is the host page (rewritten with the hash at serve time); the vendor/
/// third-party files carry no `?v=` and change with the bundle rebuild.
const PRODUCT_FILES: [&str; 2] = ["panel.js", "panel.css"];

fn main() {
    let manifest = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo"),
    );
    let panel_dir = manifest.join("resources/panel");
    let src_dir = manifest.join("resources/panel-react/src");

    // Pin rebuild triggers: the default (no directive) reruns on ANY package
    // file change; these narrow it to what this script actually reads.
    println!("cargo:rerun-if-changed=resources/panel/panel.js");
    println!("cargo:rerun-if-changed=resources/panel/panel.css");
    println!("cargo:rerun-if-changed=resources/panel-react/src");

    // Reproducible-build pin: without /Brepro, lld stamps the PE header with the
    // BUILD TIME, so two builds of identical source never match byte for byte
    // (measured on 1.2.317: that timestamp was the first of the remaining
    // differences). /Brepro derives it from the output instead.
    //
    // MSVC-style flag => applied ONLY for the MSVC target: build.rs also runs
    // for host builds (cargo test / clippy), and a GNU host linker would reject
    // it. `rustc-link-arg-bins` additionally keeps it off test harnesses.
    if std::env::var("TARGET").as_deref() == Ok("x86_64-pc-windows-msvc") {
        println!("cargo:rustc-link-arg-bins=/Brepro");
        // /Brepro alone is NOT enough: lld still emits a PDB debug directory
        // whose RSDS GUID is freshly randomised per link, and /Brepro derives
        // the header timestamp from a hash of the output — so the two feed each
        // other and the binary changes on every build (measured: two identical
        // local builds differed by exactly the timestamp + that GUID, 20 bytes).
        // A release exe needs no PDB: drop the debug directory and the output
        // becomes a pure function of its inputs.
        println!("cargo:rustc-link-arg-bins=/DEBUG:NONE");
        embed_windows_icon(&manifest);
    }

    staleness_gate(&panel_dir, &src_dir);

    let mut hash: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a-64 offset basis
    for name in PRODUCT_FILES {
        let bytes = std::fs::read(panel_dir.join(name)).unwrap_or_else(|e| {
            panic!(
                "vale-agent build: resources/panel/{name} unreadable ({e}) — \
                 run the panel build first: `cd agent/resources/panel-react && npm run build` \
                 (or `./scripts/build.sh agent` from the repo root)"
            )
        });
        hash = fnv1a64(&bytes, hash);
    }
    println!("cargo:rustc-env=PANEL_BUNDLE_HASH={hash:016x}");
}

/// Fail the build when the React sources are newer than the committed build
/// output (drift = a rebuild was skipped). Skips silently when the src tree
/// is absent (sparse checkout — nothing to judge); missing products are left
/// to the hash step / `include_str!` errors above, which already name the fix.
fn staleness_gate(panel_dir: &Path, src_dir: &Path) {
    let Ok((newest_src_path, newest_src_mtime)) = newest_mtime(src_dir) else {
        println!("cargo:warning=vale-agent build: resources/panel-react/src not found — skipping panel staleness gate");
        return;
    };
    let mut newest_product_mtime: Option<SystemTime> = None;
    for name in PRODUCT_FILES {
        match std::fs::metadata(panel_dir.join(name)).and_then(|m| m.modified()) {
            Ok(t) => {
                newest_product_mtime =
                    Some(newest_product_mtime.map_or(t, |prev: SystemTime| prev.max(t)))
            }
            Err(_) => return, // missing product: the hash step fails with the fix
        }
    }
    let newest_product_mtime = match newest_product_mtime {
        Some(t) => t,
        None => return,
    };
    let drift = newest_src_mtime
        .duration_since(newest_product_mtime)
        .unwrap_or_default();
    if drift.as_secs() > STALENESS_GRACE_SECS {
        panic!(
            "vale-agent build: STALE panel bundle — resources/panel-react/src is newer than \
             resources/panel/ by {}s (newest source: {}, products predate it). \
             panel.js is embedded at compile time, so this build would ship the OLD panel. \
             Rebuild first: `cd agent/resources/panel-react && npm run build` \
             (or `./scripts/build.sh agent` from the repo root), then rerun cargo.",
            drift.as_secs(),
            newest_src_path.display()
        );
    }
}

/// Newest mtime under `dir` (recursive), with the winning path for the error
/// message. Err when the dir cannot be walked at all.
fn newest_mtime(dir: &Path) -> std::io::Result<(PathBuf, SystemTime)> {
    let mut best: Option<(PathBuf, SystemTime)> = None;
    let mut stack = vec![dir.to_path_buf()];
    let mut seen_any = false;
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            seen_any = true;
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                let mtime = entry.metadata()?.modified()?;
                let replace = best.as_ref().is_none_or(|(_, t)| mtime > *t);
                if replace {
                    best = Some((path, mtime));
                }
            }
        }
    }
    if !seen_any {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "empty src dir",
        ));
    }
    best.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no files"))
}

/// FNV-1a-64 fold (chained across files by threading the state through).
fn fnv1a64(bytes: &[u8], mut hash: u64) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01B3;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

// ── THE PROGRAM'S OWN ICON, INSIDE THE PROGRAM (round 265) ────────────────────────────────────────────────
//
// THE DEFECT: `vale-agent.exe` carried NO resource of any kind, so Windows had nothing to draw and Task
// Manager showed the generic process glyph for the agent the whole product is about. Nothing in the
// repository could catch it: no test reads a PE resource table, the icon existed in `brand/` (rendered by
// `scripts/render-brand-icon.py` for exactly this purpose) and was wired into the desktop app and the
// installer — and the SERVICE binary, the one Task Manager actually lists, was never given it.
//
// HOW: one `ICON` + one `VERSIONINFO` statement in a generated .rc, compiled to a `.res` by LLVM's resource
// compiler and handed to the linker. No new crate: this build script stays pure std, which is what lets the
// Windows cross-compile run with nothing installed but the toolchain cargo-xwin already sets up.
//
// WHERE THE COMPILER COMES FROM: `find_resource_compiler` below searches every place these environments were
// MEASURED to put `llvm-rc` — PATH (versioned and not), the `~/.cache/cargo-xwin` symlink release.yml creates,
// Ubuntu's `/usr/lib/llvm-*/bin` from the apt package the `xwin check` job installs, and MSVC's `rc.exe` last.
// The first version of this asked for `llvm-windres` instead and died in that CI job, whose LLVM has no such
// binary — a build that was byte-perfect here and unbuildable there, which is the whole reason the search is
// written down rather than assumed.
fn embed_windows_icon(manifest: &Path) {
    // THE BRAND MARK IS THE REPO'S, not a copy made for the exe: one source, so the taskbar, the installer
    // and the landing cannot drift apart. Its 16/24/32/48 frames are the set the renderer deliberately
    // produces (it refuses a 256px frame: Pillow would PNG-compress it, which Chromium's ICO parser choked
    // on — Windows itself is happy to scale 48 up for the large view).
    let ico = manifest.join("..").join("brand").join("icon.ico");
    if !ico.is_file() {
        panic!(
            "vale-agent build: brand/icon.ico is missing ({}), so the exe would ship with NO icon and \
             Task Manager would show a generic glyph — restore it with scripts/render-brand-icon.py",
            ico.display()
        );
    }
    println!("cargo:rerun-if-changed={}", ico.display());

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR set by cargo"));
    let rc = out_dir.join("vale-agent.rc");
    // RC STRING PATHS: forward slashes, and the version resource below is the one Task Manager's Name column
    // reads — a process with no FileDescription is listed as `vale-agent.exe`, which is what the operator saw.
    let ico_rc = ico.to_string_lossy().replace('\\', "/");
    let version = product_version(manifest);
    std::fs::write(
        &rc,
        format!(
            "1 ICON \"{ico_rc}\"\n\
             \n\
             1 VERSIONINFO\n\
             FILEVERSION {v}\n\
             PRODUCTVERSION {v}\n\
             FILEFLAGSMASK 0x3fL\n\
             FILEFLAGS 0x0L\n\
             FILEOS 0x40004L\n\
             FILETYPE 0x1L\n\
             FILESUBTYPE 0x0L\n\
             BEGIN\n\
             \x20   BLOCK \"StringFileInfo\"\n\
             \x20   BEGIN\n\
             \x20       BLOCK \"040904b0\"\n\
             \x20       BEGIN\n\
             \x20           VALUE \"CompanyName\", \"Vale\"\n\
             \x20           VALUE \"FileDescription\", \"Vale Agent\"\n\
             \x20           VALUE \"FileVersion\", \"{ver}\"\n\
             \x20           VALUE \"InternalName\", \"vale-agent\"\n\
             \x20           VALUE \"OriginalFilename\", \"vale-agent.exe\"\n\
             \x20           VALUE \"ProductName\", \"Vale Agent\"\n\
             \x20           VALUE \"ProductVersion\", \"{ver}\"\n\
             \x20       END\n\
             \x20   END\n\
             \x20   BLOCK \"VarFileInfo\"\n\
             \x20   BEGIN\n\
             \x20       VALUE \"Translation\", 0x409, 1200\n\
             \x20   END\n\
             END\n",
            v = version.replace('.', ","),
            ver = version,
        ),
    )
    .unwrap_or_else(|e| panic!("vale-agent build: cannot write {}: {e}", rc.display()));

    // ONE TOOL, ONE OUTPUT, ONE LINK ARG — AND IT IS `llvm-rc`, NOT `llvm-windres` (round 265, measured the hard
    // way). The first version of this asked for windres because it can emit a COFF object directly; the CI job that
    // checks the Windows target (`agent (xwin check windows-msvc)`) installs apt's `llvm`, which ships `llvm-rc`
    // and NOT `llvm-windres`, so the build died there with "cannot run llvm-windres" on a commit whose local build
    // was byte-perfect. `llvm-rc` writes a `.res`, and lld-link accepts a `.res` as an input file directly (measured
    // before this rewrite: a .res handed to lld-link lands in the image as RT_GROUP_ICON + RT_ICON) — so the tool
    // every environment already has is also the simpler one. MSVC's `rc.exe` takes the SAME `/fo` argv, which is
    // why there is one code path below rather than two.
    let tool = find_resource_compiler();
    let res = out_dir.join("vale-agent.res");
    let out = std::process::Command::new(&tool)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res)
        .arg(&rc)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "vale-agent build: cannot run {} ({e}) — it is the resource compiler that turns brand/icon.ico \
                 into something the linker can attach. Point VALE_LLVM_RC at LLVM's llvm-rc, or put llvm-rc (or \
                 MSVC's rc.exe) on PATH",
                tool.display()
            )
        });
    if !out.status.success() {
        panic!(
            "vale-agent build: {} failed on {}: {}{}",
            tool.display(),
            rc.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
    // WITHOUT THIS LINE THE RESOURCE IS COMPILED AND THROWN AWAY — the shape of "the fix is in the build script
    // but not in the binary" this repository keeps finding.
    println!("cargo:rustc-link-arg-bins={}", res.display());
}

/// THE VERSION THE EXE REPORTS. It is the npm package's, not `CARGO_PKG_VERSION`: the crate's own version is
/// an internal 1.0.x that no user has ever seen, while the release flow bumps
/// `vale-agent-npm/package.json` to 1.2.N BEFORE it builds — so at release time that file IS this binary's
/// version, and the Properties dialog and Task Manager agree with `vale status` instead of contradicting it.
/// A dev build reports the last released version, which is the honest answer to "which release line is this".
fn product_version(manifest: &Path) -> String {
    let pkg = manifest.join("vale-agent-npm").join("package.json");
    println!("cargo:rerun-if-changed={}", pkg.display());
    let fallback = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let Ok(text) = std::fs::read_to_string(&pkg) else {
        return fallback;
    };
    // No serde in a build script: the first `"version": "x.y.z"` is the package's own, and a version that
    // does not look like one is not worth guessing at — the fallback is a real number either way.
    let needle = "\"version\":";
    let Some(at) = text.find(needle) else {
        return fallback;
    };
    let rest = &text[at + needle.len()..];
    let Some(open) = rest.find('"') else {
        return fallback;
    };
    let Some(close) = rest[open + 1..].find('"') else {
        return fallback;
    };
    let v = &rest[open + 1..open + 1 + close];
    let numeric = !v.is_empty()
        && v.split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
    if numeric && v.matches('.').count() >= 2 {
        v.to_string()
    } else {
        fallback
    }
}

/// FIND `llvm-rc`, IN EVERY PLACE THESE ENVIRONMENTS PUT IT (round 265). Written after a build that was
/// byte-perfect locally died in CI on `cannot run llvm-windres`: the tool this job has is the one apt's `llvm`
/// package ships, and the first version looked only on PATH and beside the cargo-xwin symlink. The order below is
/// most-specific first, and every candidate is a place a real environment was measured to have it:
///
///   1. `VALE_LLVM_RC` — an explicit override for a machine nobody has thought of yet;
///   2. PATH, unversioned then versioned (`llvm-rc-18` … `llvm-rc-14`): Debian and Ubuntu install the versioned
///      names, and a CI image that has one does not always have the other;
///   3. `~/.cache/cargo-xwin/llvm-rc` — the symlink `release.yml` creates, plus its RESOLVED directory, because
///      LLVM's whole toolchain lives in one `bin/`;
///   4. `/usr/lib/llvm-*/bin/llvm-rc` — Ubuntu's layout for the apt package, which is what the `xwin check` job
///      installs. Highest version wins;
///   5. MSVC's `rc.exe`, which takes the same `/fo` argv, for a native Windows build.
///
/// If none of them exists the build FAILS, naming the override — because a missing icon is precisely the defect
/// this code exists to prevent, and "warned and continued" is how it went unnoticed for the product's whole life.
fn find_resource_compiler() -> PathBuf {
    if let Ok(p) = std::env::var("VALE_LLVM_RC") {
        return PathBuf::from(p);
    }
    let mut names = vec!["llvm-rc".to_string()];
    for v in (14..=20).rev() {
        names.push(format!("llvm-rc-{v}"));
    }
    for name in &names {
        let p = PathBuf::from(name);
        if std::process::Command::new(&p)
            .arg("--version")
            .output()
            .is_ok()
        {
            return p;
        }
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let cached = PathBuf::from(&home)
        .join(".cache")
        .join("cargo-xwin")
        .join("llvm-rc");
    if std::process::Command::new(&cached)
        .arg("--version")
        .output()
        .is_ok()
    {
        return cached;
    }
    if let Ok(real) = std::fs::canonicalize(&cached) {
        if let Some(dir) = real.parent() {
            let sibling = dir.join("llvm-rc");
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    // UBUNTU'S PACKAGE LAYOUT, scanned rather than guessed: /usr/lib/llvm-<n>/bin/llvm-rc, highest n first.
    let mut versions: Vec<(u32, PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/usr/lib") {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(v) = name.strip_prefix("llvm-") else {
                continue;
            };
            let Ok(n) = v.split('.').next().unwrap_or("").parse::<u32>() else {
                continue;
            };
            let cand = e.path().join("bin").join("llvm-rc");
            if cand.is_file() {
                versions.push((n, cand));
            }
        }
    }
    versions.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    if let Some((_, p)) = versions.into_iter().next() {
        return p;
    }
    let msrc = PathBuf::from("rc.exe");
    if std::process::Command::new(&msrc).arg("/?").output().is_ok() {
        return msrc;
    }
    // Last resort: the bare name, so the failure names the TOOL rather than a path that never existed.
    PathBuf::from("llvm-rc")
}
