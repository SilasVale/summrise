//! The installer downloads code and runs it. These pins read the scripts that
//! decide WHAT it installs and WHETHER the bytes were verified.
//!
//! WHY (round 124). The online installer's CDN fallback handed `npm install -g`
//! a tarball it had never hashed, while `/api/version` carried the digest and
//! every other consumer of those bytes refuses unverified input
//! (`plugins/update/tools.rs`: "refusing unverifiable install"). No PowerShell
//! runs on this box, so the division of labour is explicit:
//!
//!   * the LOGIC is tested on a runner — `deploy/lib/SummriseIntegrity.tests.ps1`
//!     (GitHub's ubuntu runners ship pwsh; the CI step is guarded on it);
//!   * the WIRING is pinned here, by reading the sources — the same instrument
//!     this repo already uses for the boot-task contract and the pre-v2 path rule.
//!
//! A pin that only reads text is weak evidence, and it is what is available for
//! a script that cannot execute here. It is still strictly stronger than the
//! previous state, where NOTHING checked either the logic or the wiring.

use std::fs;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn read(rel: &str) -> String {
    let path = format!("{ROOT}/{rel}");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
}

/// The CDN fallback must verify the download against the version manifest before
/// npm is allowed to install it — and the check must be wired in, not merely
/// present in a library nobody calls.
#[test]
fn the_cdn_fallback_verifies_before_it_installs() {
    let ps1 = read("deploy/summrise-online-setup.ps1");
    let dotted = ps1
        .find(". (Join-Path $PSScriptRoot")
        .expect("the installer must dot-source its integrity lib");
    let manifest = ps1
        .find("/api/version")
        .expect("...must read the version manifest");
    let verify = ps1
        .find("Test-FileSha256")
        .expect("...must call the verifier");
    let install = ps1
        .rfind("install -g --prefix $NpmGlobal $tgz")
        .expect("the npm install call must still be there");
    assert!(
        dotted < manifest && manifest < verify && verify < install,
        "order is the guarantee: dot-source ({dotted}) -> manifest ({manifest}) -> \
         verify ({verify}) -> install ({install})"
    );
    // ...and a bad verdict must actually stop the run.
    let refusal = ps1
        .find("拒绝安装")
        .expect("a failed verification must refuse, not warn");
    assert!(verify < refusal && refusal < install);
    // The bundled payload is exempt BY DESIGN (it arrives inside the signed
    // installer); if that ever stops being true, this test should be rewritten
    // rather than silently widened.
    assert!(
        ps1.contains("$tgzSource = \"bundled\"") && ps1.contains("$tgzSource = \"cdn\""),
        "the two payload paths must stay distinguishable"
    );
}

/// Something the installer dot-sources must be SHIPPED by the installer: a File
/// line in the NSIS script AND a copy in the staging step. Missing either breaks
/// the installed flow at runtime on the user's machine — the one failure mode no
/// test on this box can otherwise see.
#[test]
fn every_dot_sourced_deploy_script_is_packaged() {
    let ps1 = read("deploy/summrise-online-setup.ps1");
    let nsi = read("deploy/summrise-setup.nsi");
    let build = read("../scripts/build-installer.sh");
    let mut checked = 0;
    for line in ps1.lines().filter(|l| l.contains("$PSScriptRoot")) {
        let rel = line
            .split('"')
            .nth(1)
            .unwrap_or_else(|| panic!("no quoted path in: {line}"));
        let file = rel.rsplit('\\').next().expect("a file name").to_string();
        // THE ACT, NOT THE WORD. `nsi.contains(file)` was satisfied by a COMMENT
        // mentioning the file — my own explanatory comment, in the mutation that
        // proved it (round 124). A guard that a comment can satisfy is the same
        // defect class as an assertion that cannot fail, so these match a real
        // directive/copy line instead.
        assert!(
            nsi.lines()
                .any(|l| l.trim_start().starts_with("File ") && l.contains(&file)),
            "summrise-setup.nsi must ship {file} with a File directive, or the installer breaks at runtime"
        );
        assert!(
            build
                .lines()
                .any(|l| l.trim_start().starts_with("cp ") && l.contains(&file)),
            "build-installer.sh must `cp` {file} into the stage, or makensis never sees it"
        );
        // ...and it must exist. The first version of this line built the path and
        // then called `.contains(&file)` on it — true BY CONSTRUCTION — so only the
        // `metadata` half could ever fail. One real check, not two where one is
        // decoration.
        let path = format!("{ROOT}/deploy/{}", rel.replace('\\', "/"));
        assert!(
            fs::metadata(&path).is_ok(),
            "the dot-sourced file {rel} must exist in the repo ({path})"
        );
        checked += 1;
    }
    assert_eq!(
        checked, 1,
        "expected exactly one dot-sourced deploy script; found {checked} — update this pin"
    );
}
