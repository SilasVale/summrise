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

/// THE THREE PRE-STAGED COMPONENTS ARE VERIFIED TOO (round 143).
///
/// The installer downloads cloudflared (54 MB), the playwright bundle (31 MB) and the
/// electron runtime (115 MB) into the npm package directory, and `summrise setup` then
/// takes them BY PRESENCE — so these pins are the only check those bytes get on this
/// path. The acceptance used to be `Length -gt 1MB`. A component that fails is
/// DELETED, so setup fetches it through its own verified route instead.
#[test]
fn every_prestaged_component_is_verified() {
    let ps1 = read("deploy/summrise-online-setup.ps1");
    // COMMENTS ARE STRIPPED FIRST, because this test records the old acceptance by name
    // in the comment above the fix and a naive scan reads its own explanation as the
    // defect. That is the same lesson retired-colours-check records from its first run,
    // and the first version of THIS assertion failed on exactly that line.
    let code: String = ps1
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code.contains("Length -gt 1MB"),
        "a file size is not a verdict — that check accepted any 115 MB of anything"
    );
    // Each component asks the manifest for its pin, by the name the manifest uses.
    let ask = ps1.matches("Get-ComponentSha256").count();
    assert!(
        ask >= 3,
        "all three pre-staged components must ask for their pin, found {ask}"
    );
    assert!(
        ps1.contains("-Name \"cloudflared\""),
        "cloudflared asks for its pin"
    );
    assert!(
        ps1.contains("-Name \"playwright\""),
        "playwright asks for its pin"
    );
    assert!(
        ps1.contains("-Name \"electron\""),
        "electron asks for its pin"
    );
    // AND THE VERIFICATION MUST NOT SIT INSIDE THE DOWNLOAD GUARD: a file already on
    // disk — including one staged by the installer from before this check existed —
    // would never be looked at, which is the population this change is for.
    let download = ps1
        .find("summrise-playwright.zip\" -OutFile")
        .expect("the playwright download must still be there");
    let verdict = ps1
        .find("-Name \"playwright\"")
        .expect("the playwright pin check");
    assert!(
        download < verdict,
        "the pin check must FOLLOW the download and still run when it is skipped"
    );
    // The library must define what the installer calls, not merely be called.
    let lib = read("deploy/lib/SummriseIntegrity.ps1");
    assert!(
        lib.contains("function Get-ComponentSha256"),
        "the installer must ship the function it calls"
    );
}

/// THE LOGON TASK HAS TWO OWNERS AND THEY MUST SAY THE SAME THING (round 143).
///
/// `summrise setup` registers SummriseDesktop with an Interactive/Highest principal
/// and a settings set (10-minute limit, IgnoreNew). The NSIS installer's setup script
/// registers it TOO, and its step runs AFTER setup — so its definition is the one that
/// survives, and it used to pass neither, overwriting the hardened one on every
/// install. Both ends now carry both, and this fails if either drifts.
#[test]
fn the_task_has_one_definition() {
    let ps1 = read("deploy/summrise-online-setup.ps1");
    let cli = read("summrise-agent-npm/src/summrise.ts");
    // The two facts that were missing from the weaker end.
    for (needle, who) in [
        ("New-ScheduledTaskPrincipal", "the -Principal argument"),
        ("New-ScheduledTaskSettingsSet", "the -Settings argument"),
    ] {
        assert!(
            ps1.contains(needle),
            "the installer registers SummriseDesktop without {who}, and its step runs \
             last — so its definition is the one that survives"
        );
        assert!(cli.contains(needle), "the CLI must keep {who}");
    }
    // And the registration line itself must pass them.
    let reg = ps1
        .lines()
        .find(|l| l.contains("Register-ScheduledTask SummriseDesktop"))
        .expect("the installer must still register the task");
    assert!(
        reg.contains("-Principal $pr"),
        "the installer must pass the principal: {reg}"
    );
    assert!(
        reg.contains("-Settings $st"),
        "the installer must pass the settings: {reg}"
    );
    // The two definitions must agree on the values, not merely on the argument names.
    assert!(
        ps1.contains("-RunLevel Highest") && cli.contains("-RunLevel Highest"),
        "both ends must run the desktop shell at the logged-on user's level"
    );
    assert!(
        ps1.contains("IgnoreNew") && cli.contains("IgnoreNew"),
        "both ends must refuse a second instance"
    );
}

/// A FAILED INSTALL MUST STILL BE REMOVABLE (round 147).
///
/// The NSIS script runs the setup script and, on a non-zero exit, shows a dialog and
/// Aborts. `WriteUninstaller` and the Add/Remove registration used to come AFTER that
/// Abort, so a failure left whatever the setup script had already done — Machine PATH
/// included — with no uninstaller and no entry in Add/Remove Programs. Order is the
/// guarantee here, exactly as it is for the dot-source check above.
#[test]
fn the_uninstaller_precedes_the_step_that_can_fail() {
    // COMMENTS ARE STRIPPED FIRST. This is the FOURTH time this stretch that a scan read its own
    // explanation as the defect: the comment above the reorder NAMES the `${If} $0 != 0` branch and the
    // Abort, so `find("Abort")` matched the prose rather than the instruction and the order assertion
    // failed on a file that was already correct. The sibling test above strips for the same reason.
    let nsi: String = read("deploy/summrise-setup.nsi")
        .lines()
        .filter(|l| !l.trim_start().starts_with(';'))
        .collect::<Vec<_>>()
        .join("\n");
    let run = nsi
        .find("nsExec::ExecToLog")
        .expect("the setup script must still run");
    let uninstaller = nsi
        .find("WriteUninstaller")
        .expect("an uninstaller must be written");
    let entry = nsi
        .find("Uninstall\\SummriseAgent")
        .expect("and the Add/Remove entry registered");
    assert!(
        uninstaller < run && entry < run,
        "the uninstaller and its Add/Remove entry must be written BEFORE the setup run, or a failed install cannot be removed (uninstaller={uninstaller}, entry={entry}, run={run})"
    );
    // The Abort must still be there: this changes what a failure LEAVES, not whether it stops.
    let abort = nsi
        .find("Abort")
        .expect("a failed step must still abort the install");
    assert!(run < abort, "the abort must follow the run it judges");
}

/// A RE-RUN MUST NOT DELETE AND RE-DOWNLOAD THE PORTABLE NODE (round 149).
///
/// The setup script probes `Get-Command node` — which sees only the CURRENT session's PATH —
/// and extends that PATH much later, in the Machine-PATH block. So a repair run in a fresh
/// session found no node, removed the working components\node and downloaded ~30 MB again.
/// The install directory is checked before the network now, and order is the guarantee.
#[test]
fn the_portable_node_is_reused_before_it_is_downloaded() {
    let ps1 = read("deploy/summrise-online-setup.ps1");
    let installed = ps1
        .find("Join-Path $NodeDir \"node.exe\"")
        .expect("the probe must ask the install directory for its own portable node");
    let download = ps1
        .find("node-$lts-win-x64.zip")
        .expect("the download arm must still exist");
    assert!(
        installed < download,
        "the installed portable node must be checked BEFORE the download (installed={installed}, download={download}) — otherwise every re-run pays ~30 MB and deletes a working install"
    );
    // Nor may it delete before it looks: the Remove-Item that precedes the extract must come
    // after the reuse check, or the reuse is pointless.
    let remove = ps1
        .find("Remove-Item -Recurse -Force $NodeDir")
        .expect("the extract still clears the old directory");
    assert!(
        installed < remove,
        "the reuse check must precede the delete"
    );
}
