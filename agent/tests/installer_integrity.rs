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
//!
//! THE SECOND PAYLOAD was in that previous state until now: the tgz NSIS EMBEDS
//! carried no digest at all, exempt on a "code signed" premise the build does not
//! provide. `the_bundled_payload_is_verified_not_exempted` pins the three links
//! that carry its digest from build to check.

use std::fs;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn read(rel: &str) -> String {
    let path = format!("{ROOT}/{rel}");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
}

/// The .ps1/.nsi text with its COMMENT LINES removed (`#` for PowerShell, `;` for NSIS).
///
/// EVERY ordering pin below runs on this, because a scan that reads comments can be
/// satisfied by prose. The two sibling tests above lost a round each to exactly that
/// (`find("Abort")` matched the comment that NAMED the Abort), and this one lost one
/// too: the first version scoped the CDN arm with `find("$tgzSource = \"cdn\"")`, which
/// sits ABOVE the if/else and so swallowed the bundled branch — and the bundled branch's
/// own explanatory comment mentions "/api/version", which made `manifest < verify` pass
/// while measuring the wrong arm. Whole-line comments only: a trailing comment after
/// code is not stripped, and that is the limit of this filter, stated rather than
/// implied.
fn without_comments(text: &str, marker: char) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with(marker))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The CDN fallback must verify the download against the version manifest before
/// npm is allowed to install it — and the check must be wired in, not merely
/// present in a library nobody calls.
#[test]
fn the_cdn_fallback_verifies_before_it_installs() {
    let ps1 = read("deploy/summrise-online-setup.ps1");
    let code = without_comments(&ps1, '#');
    let dotted = code
        .find(". (Join-Path $PSScriptRoot")
        .expect("the installer must dot-source its integrity lib");
    let install = code
        .rfind("install -g --prefix $NpmGlobal $tgz")
        .expect("the npm install call must still be there");
    // SCOPE EVERY LOOKUP TO THE ARM UNDER TEST. Both arms verify now, and the BUNDLED
    // arm comes first in the file, so an unscoped `find("Test-FileSha256")` measures the
    // wrong branch. The CDN arm is entered at its own download line ($dl) — the marker
    // `$tgzSource = "cdn"` is the DEFAULT set before the if/else and does not delimit it —
    // and it ends at the single npm install both arms share, so the indices below are
    // relative to that span and cannot be satisfied by the other branch.
    let cdn_start = code
        .find("$dl = Join-Path $env:TEMP")
        .expect("the CDN arm must still download to a temp file");
    let cdn = &code[cdn_start..install];
    let manifest = cdn
        .find("/api/version")
        .expect("...must read the version manifest");
    let verify = cdn
        .find("Test-FileSha256")
        .expect("...must call the verifier");
    assert!(
        dotted < cdn_start && manifest < verify,
        "order is the guarantee: dot-source ({dotted}) -> manifest ({manifest}) -> \
         verify ({verify}) -> install ({install})"
    );
    // ...and a bad verdict must actually stop the run.
    let refusal = cdn
        .find("拒绝安装")
        .expect("a failed verification must refuse, not warn");
    assert!(verify < refusal);
}

/// THE BUNDLED PAYLOAD IS VERIFIED TOO — the exemption is gone, and its premise was
/// false. It read "the bundled tgz needs no check: it arrives inside the SIGNED
/// installer", but nothing signs that installer: every reference to
/// `SUMMRISE_SIGN_CRT` / `SUMMRISE_SIGN_KEY` is inside scripts/build-installer.sh —
/// none in `.github/workflows`, none at either caller — so `sign_exe` prints
/// "code signing skipped" and returns 0 in every automated build. The bundled arm is
/// also the arm that runs with NO network, so it has no `/api/version` manifest to fall
/// back on: the digest from the build is the only check those bytes can get.
///
/// Three links carry it, and a break in any ONE leaves the check armed with "" — which
/// is why "no digest" must be a refusal rather than a pass. All three are pinned here,
/// because a check that reads `""` looks exactly like a check that passed.
#[test]
fn the_bundled_payload_is_verified_not_exempted() {
    let ps1 = read("deploy/summrise-online-setup.ps1");
    let code = without_comments(&ps1, '#');
    let nsi = without_comments(&read("deploy/summrise-setup.nsi"), ';');
    let build = read("../scripts/build-installer.sh");

    // 1. The BUILD computes the digest of the tgz it embeds, before it invokes NSIS...
    let computed = build
        .find("TGZ_SHA256=$(sha256sum \"$STAGE/summrise-agent-$VER.tgz\"")
        .expect("build-installer.sh must sha256sum the tgz it embeds");
    let passed = build
        .find("-DSUMMRISE_TGZ_SHA256=$TGZ_SHA256")
        .expect("...and hand that digest to makensis");
    assert!(
        computed < passed,
        "the digest must be computed BEFORE NSIS runs (computed={computed}, passed={passed})"
    );

    // 2. NSIS declares it — with an empty default, so a caller that does not pass one
    //    gets a refusal rather than a silent pass — and passes it on the run line.
    assert!(
        nsi.contains("!ifndef SUMMRISE_TGZ_SHA256"),
        "the NSIS script must declare the digest it interpolates"
    );
    let run = nsi
        .lines()
        .find(|l| l.contains("nsExec::ExecToLog") && l.contains("summrise-online-setup.ps1"))
        .expect("the setup script must still run");
    assert!(
        run.contains("-LocalTgzSha256 \"${SUMMRISE_TGZ_SHA256}\""),
        "the run line must carry the digest to the script: {run}"
    );

    // 3. The SCRIPT takes that parameter and checks the bundled file with it, inside the
    //    bundled branch — the span from its own marker to the shared npm install, so the
    //    CDN arm's check cannot satisfy this, and (comments being stripped above) neither
    //    can a sentence describing the check.
    assert!(
        code.contains("[string]$LocalTgzSha256"),
        "the installer must accept the digest NSIS passes"
    );
    let bundled = code
        .find("$tgzSource = \"bundled\"")
        .expect("the two payload paths must stay distinguishable");
    // The span ENDS at the other arm's first line, not at the shared npm install: the
    // install sits AFTER the if/else, so `[bundled..install]` would swallow the CDN arm
    // and its own Test-FileSha256 call. MEASURED — that is how the first version of this
    // pin passed against a bundled arm whose check had been deleted (mutation: the
    // Test-FileSha256 call replaced by a bare Test-Path).
    let cdn_start = code
        .find("$dl = Join-Path $env:TEMP")
        .expect("the CDN arm must still download to a temp file");
    assert!(
        bundled < cdn_start,
        "the bundled arm must precede the CDN arm"
    );
    let arm = &code[bundled..cdn_start];
    let check = arm
        .find("Test-FileSha256")
        .expect("the bundled arm must digest-check the payload it is about to install");
    assert!(
        arm[check..].contains("exit "),
        "and the check must EXIT before npm is handed the file, not warn"
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
    // THE ANCHOR IS THE DOWNLOAD ITSELF, NOT THE URL IT USES. It used to pin the literal
    // `summrise-playwright.zip" -OutFile`, which was the hardcoded path; the URL now comes from the
    // release manifest (`Get-ComponentUrl … -Name "playwright"`), so that literal is gone and the
    // assertion failed for a reason that has nothing to do with what it protects — that the PIN CHECK
    // follows the download and still runs when the file is already on disk. Anchor on the playwright
    // `-OutFile` write instead, whatever expression supplies the Uri.
    let download = ps1
        .find("-OutFile $pwDest")
        .expect("the playwright download must still be there");
    // AND THE VERDICT ANCHORS ON THE CHECK, NOT ON THE NAME: `-Name "playwright"` now appears INSIDE the
    // download's own `Get-ComponentUrl` call as well, so anchoring the verdict on the name would make the
    // two anchors the same occurrence and the ordering assertion vacuous (it failed exactly that way).
    let verdict = ps1
        .find("Get-ComponentSha256 -ManifestJson $manifestJson -Name \"playwright\"")
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

/// The line with its single-quoted spans blanked out (SAME BYTE LENGTH, so offsets still line up),
/// because this script WRITES another script whose text contains `{ exit }` — that one is not this
/// script's control flow. The limit, stated rather than implied: PowerShell escapes a quote inside
/// a single-quoted string by doubling it (`''`), and a simple toggle does not know that, so such a
/// span would flip the state early. No line carrying an `exit` here has a doubled quote in it, and
/// a maintainer who writes one gets a failure with the line printed, never silence.
fn unquoted(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut inside = false;
    for ch in line.chars() {
        if ch == '\'' {
            inside = !inside;
            out.push(' ');
        } else if inside {
            for _ in 0..ch.len_utf8() {
                out.push(' ');
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// The byte index of the `exit` KEYWORD in a line, if it has one. Case-sensitive and word-bounded
/// on purpose: `$LASTEXITCODE` sits on two of the exit lines and is not an exit, and neither is the
/// `exits` of prose (the header comment is not a `#` line, so it survives `without_comments`).
fn exit_keyword(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(rel) = line[from..].find("exit") {
        let at = from + rel;
        let before_ok = at == 0 || {
            let c = bytes[at - 1];
            !(c.is_ascii_alphanumeric() || c == b'_' || c == b'$')
        };
        let end = at + 4;
        let after_ok = end >= bytes.len() || {
            let c = bytes[end];
            !(c.is_ascii_alphanumeric() || c == b'_')
        };
        if before_ok && after_ok {
            return Some(at);
        }
        from = end;
    }
    None
}

/// THE INSTALL LOG IS PART OF THE FAILURE REPORT, SO EVERY EXIT CLOSES IT FIRST.
///
/// WHY. On any non-zero exit from the setup script, `summrise-setup.nsi` shows the user:
///
/// ```text
/// 安装失败（步骤退出码 $0）。
/// 看 $3\Summrise\logs\installer.log 找原因，修好后重跑安装包即可（幂等）。
/// ```
///
/// The dialog names that transcript as the place to look, and the only reader the log ever has is
/// someone whose install has just failed. A transcript is appended progressively, so an `exit`
/// USUALLY leaves a usable file — "usually" is the whole defect, and this pin is what holds the fix:
/// a log that stops mid-line, or that is missing the very failure the dialog was shown for, is worse
/// than no log at all, because the dialog promised it.
///
/// THE SHAPE, and why it is not a `try/finally`. One `try { <whole body> } finally { … }` would
/// cover future exits for free, and it is NOT what this file does, because the guarantee would rest
/// on `exit` unwinding through `finally` — which the language spec does not say: §8.7 promises the
/// finally block for "normal execution … `break`, `continue`, or `return` … or an exception being
/// thrown out of the `try` statement" and never names `exit`, and about_Try_Catch_Finally adds only
/// "an Exit keyword stops the script from within a Catch block". No pwsh runs on this box, so that
/// arm is untestable here; and wrapping the trailing `exit 0` in it would put the SUCCESS code
/// behind the same unverified rule, which NSIS reads (`$0 != 0` shows the failure dialog). So the
/// stop is explicit at each exit, and this pin is what makes the explicitness checkable.
///
/// WHAT IS PINNED, honestly: that every `exit` is preceded by `Stop-InstallLog` — on its own line
/// (`Stop-InstallLog; exit 7`) or on the line immediately above it. That is a TEXTUAL property, and
/// for "a call before each exit site" it is the form the property actually has; it is judged on
/// comment-stripped and single-quote-stripped code, so neither this prose nor the launcher text the
/// script writes can satisfy it or trip it. Removing the call from ONE exit fails it — measured, not
/// assumed.
#[test]
fn every_exit_closes_the_install_log() {
    let ps1 = read("deploy/summrise-online-setup.ps1");
    let code = without_comments(&ps1, '#');

    // ONE mechanism. A second `Stop-Transcript` beside an exit is a second path that nothing guards
    // (and the guard is the whole point: it must be safe when no transcript was ever started).
    let stops = code.matches("Stop-Transcript").count();
    assert_eq!(
        stops, 1,
        "the transcript must be closed in exactly ONE place — Stop-InstallLog; found {stops} \
         Stop-Transcript calls, so one of them is a second mechanism"
    );

    // ...and that one place is guarded TWICE: by the flag Start-Transcript sets only when it
    // returned, and by a catch. Under `$ErrorActionPreference = "Stop"` a Stop-Transcript with no
    // transcript running is a TERMINATING error, and closing a log may never change an exit code.
    let def = code
        .find("function Stop-InstallLog")
        .expect("the installer must define the one stop");
    let body_end = def
        + code[def..]
            .find("\n}")
            .expect("the function body must close at column 0");
    let body = &code[def..body_end];
    assert!(
        body.contains("Stop-Transcript"),
        "Stop-InstallLog must be the function that stops it"
    );
    assert!(
        body.contains("$script:TranscriptOn"),
        "the stop must be GUARDED by the flag, or a host where Start-Transcript threw gets an \
         error from Stop-Transcript instead of an exit code"
    );
    assert!(
        body.contains("catch"),
        "and it must be CAUGHT: under $ErrorActionPreference = \"Stop\" an unguarded failure to \
         close the log would turn a successful install into a failed one"
    );

    // The flag is armed only after Start-Transcript returns, inside the same `try`: the two exits
    // above that call (admin / 64-bit checks) and any host where it threw must take the no-op path.
    let start = code
        .find("Start-Transcript")
        .expect("the installer must still start a transcript");
    let armed = code
        .find("$script:TranscriptOn = $true")
        .expect("...and arm the guard flag only when it returned");
    assert!(
        start < armed,
        "the flag must be set AFTER Start-Transcript ({start} < {armed}), never before"
    );

    // EVERY exit, one at a time. This is the assertion the fix is for.
    let lines: Vec<String> = code.lines().map(unquoted).collect();
    let mut judged = 0;
    for (i, line) in lines.iter().enumerate() {
        let Some(at) = exit_keyword(line) else {
            continue;
        };
        judged += 1;
        let on_this_line = line.find("Stop-InstallLog").is_some_and(|s| s < at);
        let on_the_line_above = lines[..i]
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty())
            .is_some_and(|l| l.contains("Stop-InstallLog"));
        assert!(
            on_this_line || on_the_line_above,
            "this exit is not preceded by Stop-InstallLog, so its transcript is left open — and \
             the failure dialog sends the user to that log: {}",
            line.trim()
        );
    }
    // A FLOOR, not a claim: the installer has 13 exit sites today and the number may grow. What
    // must not happen is this scan judging nothing and passing because it looked at nothing.
    assert!(
        judged >= 10,
        "expected the installer's exit paths to be judged; found {judged} — if the exits really \
         moved elsewhere, this pin must be moved with them"
    );
}

/// THE BOM IS LOAD-BEARING, and nothing else in the suite can see it.
///
/// PowerShell 5.1 decodes a `.ps1` with no byte-order mark as ANSI, and this file is UTF-8 carrying
/// Chinese in every user-facing line — so a lost BOM shows the user mojibake instead of the
/// installer's own messages. MEASURED, while writing the pin above: an editor that rewrote this
/// file dropped the BOM silently, and the whole suite stayed green (the file still parses as UTF-8
/// on this box, which is why the defect is invisible here). Same hazard the result file's comment
/// records from the other end, where the completion page showed 鈥?… .
#[test]
fn the_setup_script_keeps_its_utf8_bom() {
    let path = format!("{ROOT}/deploy/summrise-online-setup.ps1");
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    assert!(
        bytes.starts_with(&[0xEF, 0xBB, 0xBF]),
        "deploy/summrise-online-setup.ps1 lost its UTF-8 BOM: PowerShell 5.1 would read its \
         Chinese messages as ANSI bytes and show the user mojibake"
    );
}
