//! `summrise uninstall` — THE ONLY UNINSTALL PATH.
//!
//! The NSIS installer is retired and the npm CLI is the single install channel, so this is what a
//! device is taken apart with. Ported from the `uninstall` verb in
//! `agent/summrise-agent-npm/src/summrise.ts`.
//!
//! **IT REFUSES BEFORE IT DELETES, AND THAT IS THE FIRST THING IT DOES.** `SUMMRISE_AGENT_DIR` and
//! the registry key are both attacker- or accident-controlled, and everything below this guard is a
//! recursive delete: a value pointing at `C:\` used to be deleted from. Three markers are accepted
//! (the exe in the install root, the hostname file in either its v2 or its pre-v2 home) because a
//! half-migrated device has to remain uninstallable.
//!
//! **AND IT VERIFIES, BECAUSE `sh()` DISCARDED ITS RESULT AT EVERY CALL SITE.** A locked file, an AV
//! hold or a denied HKLM write produced "removed" with the thing still there — and exit 0. The
//! survivors list is what decides: the program dir, the registry key, and (under `--purge-data`)
//! the data dir. One failure class, one treatment: non-zero, with the sentence that says what is
//! still present.

use crate::dispatch::Outcome;
use crate::host::Host;
use crate::paths::{win_join, Layout, REG_KEY};
use crate::ps::psq;
use std::path::Path;

/// The two install dirs the retired channel left behind. Both were removable by an operator, and
/// neither is `Layout.dir` on a current device.
pub const LEGACY_DIRS: [&str; 2] = ["C:\\summrise-agent", "D:\\summrise-agent"];

/// `HKLM\SYSTEM\CurrentControlSet\Services\EventLog\Application\Cloudflared`, whose leftover
/// service registration makes the event log complain about a service that is gone.
pub const CLOUDFLARED_EVENTLOG: &str =
    "HKLM\\SYSTEM\\CurrentControlSet\\Services\\EventLog\\Application\\Cloudflared";

/// Does this look like a Summrise install dir at all?
///
/// Exposed as a predicate because the guard is the one thing here that must be impossible to get
/// wrong, and a test that drives it directly is the shape that proves it.
pub fn looks_like_an_install(host: &dyn Host, layout: &Layout) -> bool {
    host.exists(Path::new(&layout.exe_dst))
        || host.exists(Path::new(&layout.hostname_file))
        || host.exists(&win_join(&layout.dir, "summrise-agent.hostname"))
}

/// The PowerShell that stops every `node.exe` whose command line mentions a summrise playwright
/// bundle — the current install dir AND the legacy ones, because a fresh uninstall has to clear
/// both (`playwright` was under `D:\summrise-agent` before layout v2).
pub fn kill_playwright_nodes_ps() -> String {
    "Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'node.exe' -and ($_.CommandLine -like '*summrise-agent*playwright*' -or $_.CommandLine -like '*summrise-command*playwright*') } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }".to_string()
}

/// `Remove-Item -LiteralPath … -Recurse -Force`, run as a PowerShell FILE-less one-shot.
///
/// NOT `remove_tree`: `rmdir` is a cmd.exe BUILTIN, so a path handed to it is text cmd re-parses and
/// `%NAME%` expands even inside quotes. The install dir is operator-chosen, and `%` is a legal
/// character in an NTFS name.
pub fn remove_item_ps(path_q: &str) -> Vec<String> {
    vec![
        "powershell".to_string(),
        "-NoProfile".to_string(),
        "-Command".to_string(),
        format!(
            "Remove-Item -LiteralPath '{path_q}' -Recurse -Force -ErrorAction SilentlyContinue"
        ),
    ]
}

/// The legacy-dir removal: two attempts with a short wait between them, because a locked or
/// read-only file needs a beat before the retry can take it.
pub fn remove_legacy_ps(path_q: &str) -> String {
    format!(
        "Remove-Item -LiteralPath '{path_q}' -Recurse -Force -ErrorAction SilentlyContinue; \
         Start-Sleep -Milliseconds 500; \
         Remove-Item -LiteralPath '{path_q}' -Recurse -Force -ErrorAction SilentlyContinue"
    )
}

/// `summrise uninstall [--purge-data]`.
pub fn uninstall_decision(host: &dyn Host, layout: &Layout, args: &[String]) -> Outcome {
    let purge = args.iter().any(|a| a == "--purge-data");
    let mut out = Outcome::ok();

    if !looks_like_an_install(host, layout) {
        return Outcome::fail(
            1,
            format!(
                "uninstall: REFUSE — {} does not look like a Summrise install dir (no summrise-agent.exe/hostname). Set SUMMRISE_AGENT_DIR to the correct path.",
                layout.dir
            ),
        );
    }

    out = out.say("uninstall: stopping SummriseAgent...");
    // Every one of these is BEST-EFFORT and every one is argv: the answer is not the point, the
    // survivor check at the end is. `sc`, `reg` and `schtasks` all take argv, unlike `rmdir`.
    let run = |argv: &[&str]| {
        let v: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        let _ = host.run(&v, None);
    };
    run(&["schtasks", "/End", "/TN", "SummriseAgent"]);
    let _ = host.taskkill_image("summrise-agent.exe");
    let _ = host.taskkill_image("summrise-desktop.exe");
    // npm audit #11: electron survived uninstall (a dead SPA window), and the update-hardened
    // SummriseDesktop 5-minute pulse kept firing against the deleted dir.
    let _ = host.taskkill_image("electron.exe");
    run(&["schtasks", "/End", "/TN", "SummriseDesktop"]);
    run(&["schtasks", "/Delete", "/TN", "SummriseDesktop", "/F"]);
    let kill_nodes = kill_playwright_nodes_ps();
    let mut argv = vec![
        "powershell".to_string(),
        "-NoProfile".to_string(),
        "-Command".to_string(),
    ];
    argv.push(kill_nodes);
    let _ = host.run(&argv, None);
    let _ = host.taskkill_image("cloudflared.exe");
    run(&["schtasks", "/Delete", "/TN", "SummriseAgent", "/F"]);
    run(&["schtasks", "/Delete", "/TN", "SummrisePlaywright", "/F"]);
    // The legacy Windows service + its event-log source (installed by the retired setup.ps1; the
    // agent-supervised model installs no service at all).
    run(&["sc", "stop", "Cloudflared"]);
    run(&["sc", "delete", "Cloudflared"]);
    run(&["reg", "delete", CLOUDFLARED_EVENTLOG, "/f"]);

    let _ = host.run(&remove_item_ps(&psq(&layout.dir)), None);

    for legacy in LEGACY_DIRS {
        if legacy == layout.dir || !host.exists(Path::new(legacy)) {
            continue;
        }
        out = out.say(format!("uninstall: removing legacy install dir {legacy}"));
        let _ = host.run(&remove_item_ps(&remove_legacy_ps(&psq(legacy))), None);
        if host.exists(Path::new(legacy)) {
            out = out.say(format!(
                "uninstall: WARNING -- legacy dir still present: {legacy}"
            ));
        }
    }
    run(&["reg", "delete", REG_KEY, "/f"]);

    // ── THE SURVIVORS ARE THE VERDICT ─────────────────────────────────────────────────────────
    let mut survivors: Vec<String> = Vec::new();
    if host.exists(Path::new(&layout.dir)) {
        survivors.push(format!("install dir {}", layout.dir));
    }
    let reg_left = host.run(&["reg".into(), "query".into(), REG_KEY.into()], None);
    if reg_left.status == Some(0) {
        survivors.push(format!("registry key {REG_KEY}"));
    }
    if !survivors.is_empty() {
        // ONE FAILURE CLASS, ONE TREATMENT — the data dir's twin below exits 1 for exactly this,
        // and this branch used to WARN AND EXIT 0 while its own comment stated the rule it broke.
        out = out.warn(format!(
            "uninstall: FAILED -- still present after removal: {}",
            survivors.join(", ")
        ));
        return out
            .warn("uninstall: a locked file or a permission problem; re-run after stopping the agent.")
            .exit(1);
    }
    out = out.say("uninstall: program dir + registry removed");

    if purge {
        let _ = host.run(&remove_item_ps(&psq(&layout.data_dir)), None);
        // A failed removal used to print "data dir purged" anyway.
        if host.exists(Path::new(&layout.data_dir)) {
            return out
                .warn(format!(
                    "uninstall: FAILED to purge the data dir -- {} is still present",
                    layout.data_dir
                ))
                .exit(1);
        }
        out = out.say("uninstall: data dir purged");
    } else {
        out = out.say(format!(
            "uninstall: data kept at {} (pass --purge-data to delete)",
            layout.data_dir
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::RunResult;
    use crate::testing::FakeHost;

    fn layout() -> Layout {
        Layout::from_roots("D:\\Summrise", "C:\\ProgramData\\Summrise")
    }

    /// THE GUARD IS FIRST AND IT IS NOT ADVISORY. A directory that is not a Summrise install is
    /// refused before a single delete runs, and the sentence names the env var that fixes it.
    #[test]
    fn a_directory_that_is_not_an_install_is_refused_before_anything_is_deleted() {
        let host = FakeHost::new();
        let r = uninstall_decision(&host, &layout(), &[]);
        assert_eq!(r.exit, 1);
        assert!(r.err[0].contains("REFUSE"), "{:?}", r.err);
        assert!(
            host.runs().is_empty(),
            "nothing may run against a refused path: {:?}",
            host.runs()
        );
    }

    /// Any ONE of the three markers is enough — the pre-v2 home and the v2 home are both accepted,
    /// because a half-migrated device must remain uninstallable.
    #[test]
    fn any_one_of_the_three_markers_proves_it_is_an_install() {
        for marker in [
            "D:\\Summrise\\summrise-agent.exe",
            "D:\\Summrise\\etc\\summrise-agent.hostname",
            "D:\\Summrise\\summrise-agent.hostname",
        ] {
            let host = FakeHost::new().with_file(marker, "x");
            assert!(
                looks_like_an_install(&host, &layout()),
                "{marker} must be accepted"
            );
        }
        assert!(!looks_like_an_install(&FakeHost::new(), &layout()));
    }

    /// THE SURVIVOR CHECK IS WHAT DECIDES. A locked file leaves the dir behind and the command says
    /// so with a NON-ZERO exit — it used to warn and exit 0, the one false verdict this whole
    /// module exists to refuse.
    #[test]
    fn a_surviving_install_dir_is_a_failure_not_a_warning() {
        let host = FakeHost::new()
            .with_file("D:\\Summrise\\summrise-agent.exe", "MZ")
            // The fake does not delete on `run`, so the dir survives — which is the case.
            .script_runs(vec![RunResult {
                status: Some(0),
                ..Default::default()
            }]);
        let r = uninstall_decision(&host, &layout(), &[]);
        assert_eq!(r.exit, 1, "{r:?}");
        assert!(
            r.err
                .iter()
                .any(|e| e.contains("still present after removal")),
            "{:?}",
            r.err
        );
        assert!(
            r.err.iter().any(|e| e.contains("registry key")),
            "the registry key is checked too: {:?}",
            r.err
        );
    }

    /// ...and the DATA dir has the same treatment under `--purge-data`: a surviving data dir is a
    /// failure, and the data dir is NOT touched without the flag.
    #[test]
    fn the_data_dir_is_only_touched_with_the_flag_and_is_verified() {
        // Without the flag: kept, and named.
        let host = FakeHost::new()
            .with_file("D:\\Summrise\\summrise-agent.exe", "MZ")
            .script_runs(vec![RunResult {
                status: Some(1),
                ..Default::default()
            }]);
        let r = uninstall_decision(&host, &layout(), &[]);
        assert_eq!(r.exit, 0, "{r:?}");
        assert!(
            r.out.iter().any(|l| l.contains("data kept at")),
            "{:?}",
            r.out
        );
        assert!(
            !host
                .runs()
                .iter()
                .any(|a| a.iter().any(|x| x.contains("C:\\ProgramData\\Summrise"))),
            "the data dir must not be named to any command without --purge-data"
        );

        // With the flag: the purge is attempted and its failure is fatal.
        //
        // THE LOCK IS ON THE DATA DIR ALONE. The program dir's removal has to SUCCEED here, or the
        // survivor check fires on it first and this case never reaches the branch it is about — and
        // a REMOVAL THAT SUCCEEDS IS THE FAKE'S DEFAULT now that it honours the `Remove-Item` it is
        // asked to run. The old comment ("the fake does not delete on `run`") described a fake that
        // could not reach ANY survivor branch, which is why the fake was changed.
        let host = FakeHost::new()
            .with_file("D:\\Summrise\\summrise-agent.exe", "MZ")
            .with_file("C:\\ProgramData\\Summrise\\sessions\\s1", "x")
            .with_locked("C:\\ProgramData\\Summrise\\sessions\\s1")
            .script_runs(vec![RunResult {
                status: Some(1),
                ..Default::default()
            }]);
        let r = uninstall_decision(&host, &layout(), &["--purge-data".into()]);
        assert_eq!(r.exit, 1, "{r:?}");
        assert!(
            r.err
                .iter()
                .any(|e| e.contains("FAILED to purge the data dir")),
            "{:?}",
            r.err
        );
        // ...and the program dir was NOT reported as a survivor, so the two failures are told apart.
        assert!(
            !r.err.iter().any(|e| e.contains("install dir")),
            "the program dir came out clean: {:?}",
            r.err
        );
    }

    /// Every removal is ARGV. `rmdir` is a cmd builtin and a path handed to it is text cmd
    /// re-parses, so the recursive delete goes through PowerShell with the path as one argument.
    #[test]
    fn the_recursive_delete_is_powershell_with_the_path_as_one_argument() {
        let host = FakeHost::new()
            .with_file("D:\\Summrise\\summrise-agent.exe", "MZ")
            .script_runs(vec![RunResult {
                status: Some(1),
                ..Default::default()
            }]);
        let _ = uninstall_decision(&host, &layout(), &[]);
        let pw: Vec<Vec<String>> = host
            .runs()
            .into_iter()
            .filter(|a| a.first().map(|s| s.as_str()) == Some("powershell"))
            .collect();
        assert!(!pw.is_empty(), "powershell must be used for the delete");
        // One element carries `-LiteralPath '<dir>'` — the whole script, verbatim.
        assert!(
            pw.iter().any(|a| a
                .iter()
                .any(|x| x.contains("Remove-Item -LiteralPath 'D:\\Summrise'"))),
            "{pw:?}"
        );
    }

    /// The playwright node killer matches the LEGACY dirs too — the command line pattern is
    /// `*summrise-agent*playwright*`, not the current install dir, so a fresh uninstall clears the
    /// pre-v2 residue as well.
    #[test]
    fn the_playwright_node_killer_is_not_scoped_to_the_current_dir() {
        let script = kill_playwright_nodes_ps();
        assert!(script.contains("*summrise-agent*playwright*"));
        assert!(script.contains("*summrise-command*playwright*"));
        assert!(!script.contains("D:\\Summrise"));
    }
}
