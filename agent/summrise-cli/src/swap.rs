//! THE STAGING AND THE SWAP — what `summrise update` DOES, after its guards have all passed.
//!
//! `setup` and `update` stage the SAME files and hand off to the SAME machinery, so they are staged
//! by the same functions here: a second copy is how the two come to disagree, and the disagreement
//! is invisible until a device is in the field.
//!
//! **WHY THE SWAP IS A POWERSHELL SCRIPT AT ALL, RATHER THAN WORK THIS BINARY DOES.** The agent
//! being replaced is RUNNING, and on Windows a running image cannot be overwritten. So the CLI
//! stages `*.new` beside the live files, writes a script that stops the service, copies with retry,
//! and starts it again — and launches that script through WMI (`Win32_Process.Create`) so the child
//! is parented by `WmiPrvSE` and survives both this CLI and the agent it kills. Node's detached
//! spawn does not (observed on the device), which is the whole reason for the WMI hop.
//!
//! **EVERY HARD-WON LINE OF THAT SCRIPT IS A MEASURED DEFECT, AND THE PORT CARRIES THEM ALL:**
//!
//! * the RESTART is in a `finally` — it used to be the 24th of ~24 steps with nothing guarding it,
//!   so any error in between left the device DARK. Measured 2026-09-28: an update answered 502 and
//!   the tunnel went from 502 to 530/1033 with no connector at all;
//! * the line that reports the restart carries `$rs`, because the old one read `task restarted`
//!   unconditionally — a log asserting a restart it had never verified;
//! * `.summrise-release` is written ONLY when `$ok`, because a failed swap keeps the device on the
//!   old exe and the marker is what `agent_update` compares;
//! * the LAUNCHER goes in before either task is re-registered, because both name it as their
//!   action and a task pointing at a program still called `.new` cannot start;
//! * the layout-v2 gate is FAIL-CLOSED and runs BEFORE the swap: the new agent reads only the v2
//!   homes, so aborting there leaves the old version running untouched.
//!
//! Ported from the `update` verb and `stageDesktopShell`/`ensureElectron` in
//! `agent/summrise-agent-npm/src/summrise.ts`.

use crate::components::{resolve_component_logged, write_boxed_versions};
use crate::config::agent_port;
use crate::dispatch::Outcome;
use crate::host::Host;
use crate::paths::{release_marker_path, win_join, Layout};
use crate::ps::psq;
use crate::psgen::{
    boot_task_ps, busy_marker_ps, desk_shortcut_repair_ps, ensure_desktop_ps, firewall_ps,
    migrate_layout_ps, playwright_probe_ps, start_desktop_ps, uninstall_version_ps,
    update_receipt_ps,
};
use crate::update::{
    await_release_marker, release_marker_verdict, AwaitOpts, ReleaseMarkerCheck, Verb,
};
use std::path::{Path, PathBuf};

/// The SIX sources `stageDesktopShell` ships, and the two icons beside them.
///
/// A LIST, because the swap script's own desktop loop copies the same files in the same order: the
/// staging and the swap have to agree about what "the desktop shell's files" means, and the only way
/// to make that structural is to have one list.
///
/// **IT IS SIX AND NOT FOUR SINCE LANDING 6b, AND THE REASON IS THE FAILURE THIS COMMENT WARNS ABOUT.**
/// The shell's `main.js` requires TWO policy glues now — `summrise_url_policy.js` (landing 6a) and
/// `summrise_shell_policy.js` (6b), each requiring its own `.wasm`. A file that is staged but never
/// copied into the shell's `src/` is a shell that dies on the device with
/// `Cannot find module './summrise_shell_policy'`, so the two names have to travel together.
pub const DESK_SHELL_SOURCES: [&str; 6] = [
    "main.js",
    "preload.js",
    "summrise_url_policy.js",
    "summrise_url_policy_bg.wasm",
    "summrise_shell_policy.js",
    "summrise_shell_policy_bg.wasm",
];

/// The two icons, which go NEXT TO `src/` (`electron` loads `../icon.png`, and the Windows tray
/// needs the `.ico`).
pub const DESK_SHELL_ICONS: [&str; 2] = ["icon.png", "icon.ico"];

/// The minimal Electron manifest `stageDesktopShell` writes.
///
/// WHY IT EXISTS: `start-desktop.ps1` launches the shell as `electron .`, which resolves its entry
/// ONLY through `package.json`'s `main`. The staging used to ship `src/*.js` and the icons and no
/// manifest at all — so `electron .` had nothing to load AND the installer's Electron step (gated on
/// `Test-Path package.json`) was skipped, leaving the desktop shell dead on every fresh box.
pub fn shell_manifest_json() -> String {
    // `JSON.stringify(obj, null, 2)` — two-space indent, keys in insertion order, no trailing
    // newline. Written as a literal so it cannot drift from what the TypeScript emitted.
    [
        "{",
        "  \"name\": \"summrise-desktop-electron\",",
        "  \"version\": \"0.2.0\",",
        "  \"main\": \"src/main.js\",",
        "  \"private\": true",
        "}",
    ]
    .join("\n")
}

/// `stageDesktopShell(installDir, suffix)` — copy the shell's sources into the install tree.
///
/// Returns how many SOURCES were staged, which is the fact the caller's sentence depends on: it
/// printed "sources staged" regardless, so a package that did not ship them claimed they were there.
/// `suffix` is `""` for setup (in place) and `".new"` for update (the atomic swap).
pub fn stage_desktop_shell(
    host: &dyn Host,
    install_dir: &str,
    pkg_dir: &str,
    suffix: &str,
) -> usize {
    let shell_dir = format!("{install_dir}\\components\\summrise-desktop-electron");
    let src = format!("{pkg_dir}\\summrise-desktop-electron\\src");
    if !host.exists(Path::new(&src)) {
        return 0;
    }
    let dst = format!("{shell_dir}\\src");
    let _ = host.mkdirs(Path::new(&dst));
    let mut staged = 0;
    for f in DESK_SHELL_SOURCES {
        let from = format!("{src}\\{f}");
        if host.exists(Path::new(&from)) {
            let _ = host.copy_file(Path::new(&from), Path::new(&format!("{dst}\\{f}{suffix}")));
            staged += 1;
        }
    }
    for icon in DESK_SHELL_ICONS {
        let from = format!("{pkg_dir}\\summrise-desktop-electron\\{icon}");
        if host.exists(Path::new(&from)) {
            let _ = host.copy_file(Path::new(&from), Path::new(&format!("{shell_dir}\\{icon}")));
        }
    }
    // Best-effort: a failed write must not break staging.
    let _ = host.write_bytes(
        Path::new(&format!("{shell_dir}\\package.json")),
        shell_manifest_json().as_bytes(),
    );
    staged
}

/// `ensureElectron()` — the runtime the staged sources need, or `None` with the caller saying so.
///
/// Sources without a runtime is a shell that cannot start: no window, and nothing in the output
/// saying why. The zip's ROOT is the dist contents (measured through the route, not assumed), so it
/// expands into `dist`.
pub fn ensure_electron(host: &dyn Host, layout: &Layout, pkg_dir: &str) -> Option<String> {
    let el_dir = format!("{}\\node_modules\\electron", layout.desk_dir);
    let dist = format!("{el_dir}\\dist");
    if host.exists(Path::new(&format!("{dist}\\electron.exe"))) {
        return Some(dist);
    }
    let pkg_zip = win_join(pkg_dir, "electron-win32-x64.zip");
    let (zip, _log) = resolve_component_logged(host, "electron-win32-x64.zip", &pkg_zip);
    let zip = zip?;
    let _ = host.mkdirs(Path::new(&dist));
    let script = format!(
        "Expand-Archive -Force -Path '{}' -DestinationPath '{}'",
        psq(&zip.to_string_lossy()),
        psq(&dist)
    );
    let mut argv = vec!["powershell".to_string(), "-NoProfile".to_string()];
    argv.extend(crate::ps::ps_argv(&script));
    let _ = host.run(&argv, None);
    if !host.exists(Path::new(&format!("{dist}\\electron.exe"))) {
        return None;
    }
    // The electron package's own lookup file; the launcher does not need it, but anything that
    // resolves the package the npm way will.
    let _ = host.write_bytes(Path::new(&format!("{el_dir}\\path.txt")), b"electron.exe");
    Some(dist)
}

/// The swap script, as its list of lines — PURE, so the parity harness can compare it byte for byte.
///
/// `q` is the install dir and `qd` the data dir, each with its single quotes DOUBLED (the
/// TypeScript's `psq`), because both are operator-chosen paths interpolated into single-quoted
/// PowerShell literals.
pub fn swap_script(q: &str, qd: &str, rel_ver: &str, port: u16) -> Vec<String> {
    let log = format!("Out-File '{qd}\\logs\\summrise-update.log' -Append");
    let busy = busy_marker_ps();
    let mut s: Vec<String> = Vec::new();

    s.push(format!("\"[$(Get-Date -Format o)] update start\" | {log}"));
    // LAYOUT V2 FIRST: repoint the boot task at the explicit config path BEFORE touching anything.
    // Fail-closed — a config-path argument boots old AND new agents alike, so aborting here leaves
    // the old version running untouched. Without it the moved config strands the boot.
    s.push(format!(
        "try {{ {} }} catch {{ \"[$(Get-Date -Format o)] task repoint FAILED: $($_.Exception.Message)\" | {log}; try {{ Remove-Item -Force ({busy}) }} catch {{}}; exit 1 }}",
        boot_task_ps(&format!("{q}\\summrise-agent.exe"), &format!("{q}\\etc\\config.yaml"), false)
            .join("; ")
    ));
    s.push(format!(
        "\"[$(Get-Date -Format o)] task repointed at etc\\config.yaml\" | {log}"
    ));
    // Layout-v2 migration (ADR 0008): move pre-v2 root paths into their v2 homes. Best-effort per
    // item; the gate below is fail-closed.
    s.extend(migrate_layout_ps(q, qd));
    s.push(format!(
        "if ((-not (Test-Path '{q}\\etc\\config.yaml')) -or (-not (Test-Path '{q}\\etc\\summrise-agent.hostname'))) {{ \"[$(Get-Date -Format o)] migration gate FAILED (etc\\config.yaml/hostname missing) -- aborting, old version keeps running\" | {log}; try {{ Remove-Item -Force ({busy}) }} catch {{}}; exit 1 }}"
    ));
    // A running exe cannot be overwritten on Windows — stop the service first (task end + process
    // kill), THEN swap with retry. Everything from here to the `finally` is one `try`.
    s.push("try {".to_string());
    s.push("try { Stop-ScheduledTask SummriseAgent -ErrorAction Stop } catch {}".to_string());
    s.push("Get-Process summrise-agent -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue".to_string());
    s.push("Start-Sleep -Milliseconds 1500".to_string());
    s.push("$ok=$false".to_string());
    s.push(format!(
        "foreach($i in 1..12){{ try {{ Copy-Item -Force -ErrorAction Stop '{q}\\summrise-agent.new.exe' '{q}\\summrise-agent.exe'; $ok=$true; break }} catch {{ Start-Sleep -Milliseconds 800 }} }}"
    ));
    s.push(format!("\"[$(Get-Date -Format o)] copy ok=$ok\" | {log}"));
    // The launcher goes in BEFORE either task is re-registered below: both name it as their action,
    // so a swap that repointed them at a program still called `.new` would leave two tasks that
    // cannot start.
    s.push("$lok=$false".to_string());
    s.push(format!(
        "foreach($i in 1..12){{ try {{ Copy-Item -Force -ErrorAction Stop '{q}\\scripts\\summrise-launch.new.exe' '{q}\\scripts\\summrise-launch.exe'; $lok=$true; break }} catch {{ Start-Sleep -Milliseconds 800 }} }}"
    ));
    s.push(format!(
        "\"[$(Get-Date -Format o)] launcher ok=$lok\" | {log}"
    ));
    // `.summrise-release` is written ONLY when the copy PROVABLY completed. A failed swap keeps the
    // device on the OLD exe, and the marker is what `agent_update` compares — a marker that lied
    // would make every UI report a release the device is not running.
    s.push(format!(
        "if ($ok -and '{rel_ver}') {{ Set-Content -Path '{q}\\etc\\.summrise-release' -Value '{rel_ver}' -NoNewline -ErrorAction SilentlyContinue }}"
    ));
    // Add/Remove-Programs parity, on the same `$ok` gate: a failed swap must not move the displayed
    // version either.
    s.extend(uninstall_version_ps(q, rel_ver));
    s.push(format!(
        "Remove-Item -Force -ErrorAction SilentlyContinue '{q}\\summrise-agent.new.exe'"
    ));
    s.push(format!(
        "Remove-Item -Force -ErrorAction SilentlyContinue '{q}\\scripts\\summrise-launch.new.exe'"
    ));
    // Desktop shell sources, with retry — the running Electron may hold them briefly.
    //
    // **THE NAMES COME FROM `DESK_SHELL_SOURCES`, AND THAT IS A FIX RATHER THAN A TIDY-UP: this loop
    // used to spell them out a SECOND time, which made the constant's own comment ("the only way to
    // make that structural is to have one list") false for the half that matters.** Adding a source to
    // the constant without editing this literal stages a file the swap never copies into the shell's
    // `src/`, and the shell then dies with `Cannot find module` — the failure the constant's doc names.
    // The generated PowerShell is byte-identical: same names, same order, and `join(",")` with no space,
    // which is what the literal had.
    let desk_shell_list = DESK_SHELL_SOURCES
        .iter()
        .map(|f| format!("'{f}'"))
        .collect::<Vec<_>>()
        .join(",");
    s.push(format!(
        "foreach($df in @({desk_shell_list})){{ $ds='{q}\\components\\summrise-desktop-electron\\src\\'+$df+'.new'; if (Test-Path $ds) {{ $ok2=$false; foreach($i in 1..8){{ try {{ Copy-Item -Force -ErrorAction Stop $ds ('{q}\\components\\summrise-desktop-electron\\src\\'+$df); $ok2=$true; break }} catch {{ Start-Sleep -Milliseconds 500 }} }}; Remove-Item -Force -ErrorAction SilentlyContinue $ds; \"[$(Get-Date -Format o)] desk $df ok=$ok2\" | {log} }} }}"
    ));
    // NEVER LEAVE THE DEVICE DARK. `try`/`finally` WITHOUT `catch` is deliberate: the failure still
    // propagates to the exit code, and the restart happens regardless — PowerShell runs a `finally`
    // block for an uncaught exception and for `exit`.
    s.push("} finally {".to_string());
    s.push(
        "  $rs=$false; try { Start-ScheduledTask SummriseAgent -ErrorAction Stop; $rs=$true } catch { try { schtasks /Run /TN SummriseAgent; $rs=$true } catch {} }".to_string(),
    );
    s.push(format!(
        "\"[$(Get-Date -Format o)] task restart ok=$rs\" | {log}"
    ));
    s.push("}".to_string());
    s.push(format!("try {{ Remove-Item -Force ({busy}) }} catch {{}}"));
    // Custom-port installs: the firewall rule must track the configured bind port, baked here from
    // the live config.yaml — the swap itself runs from a static file and cannot read it.
    s.extend(firewall_ps(port));
    s.push(format!(
        "$deskDir = '{q}\\components\\summrise-desktop-electron'"
    ));
    s.push(format!("$deskStart = '{q}\\scripts\\start-desktop.ps1'"));
    // The shell supervisor is hardened HERE, and this is the path that reaches a machine already
    // installed: `setup` builds the same body, and a second hand-typed copy would silently overwrite
    // the instrumented one on every fleet device.
    s.push(format!("$en1 = '{q}\\scripts\\ensure-desktop.ps1'"));
    s.push(format!("$ln1 = '{q}\\scripts\\summrise-launch.exe'"));
    s.push(
        "$pwsh1 = Join-Path $env:SystemRoot 'System32\\WindowsPowerShell\\v1.0\\powershell.exe'"
            .to_string(),
    );
    s.push("$enBody = @'".to_string());
    s.extend(ensure_desktop_ps(&format!("{q}\\scripts"), qd));
    s.push("'@".to_string());
    s.push("Set-Content -Path $en1 -Value $enBody -Force".to_string());
    // The retired wrappers go with the action that used them. Best-effort: a `.vbs` nothing runs is
    // inert, and this line must not be able to abort a swap that is otherwise fine.
    s.push(format!(
        "foreach ($v in @('{q}\\scripts\\desktop-pulse.vbs','{q}\\desktop-pulse.vbs','{q}\\scripts\\run-hidden.vbs','{q}\\playwright\\run-hidden.vbs')) {{ if (Test-Path $v) {{ try {{ Remove-Item -Force -ErrorAction Stop $v }} catch {{}} }} }}"
    ));
    // The SAME launcher `setup` registers, and this is the path that migrates an installed fleet: a
    // device whose task still runs `wscript.exe <desktop-pulse.vbs>` is repointed here, which is what
    // removes both the missing-engine dialog and the dependency that caused it.
    s.push("if ((Test-Path $ln1) -and ($null -ne (Get-ScheduledTask -TaskName 'SummriseDesktop' -ErrorAction SilentlyContinue))) {".to_string());
    s.push(format!(
        "  $da = New-ScheduledTaskAction -Execute $ln1 -Argument ('\"' + $pwsh1 + '\" -NoProfile -ExecutionPolicy Bypass -File \"' + $en1 + '\"') -WorkingDirectory '{q}'"
    ));
    s.push("  $dt1 = New-ScheduledTaskTrigger -AtLogOn".to_string());
    s.push("  $dw1 = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(3) -RepetitionInterval (New-TimeSpan -Minutes 5)".to_string());
    // AND THE SETTINGS, WHICH IS THE HALF THAT REACHES A MACHINE ALREADY INSTALLED. This call used
    // to pass only -Action and -Trigger, and `Set-ScheduledTask` PRESERVES the components it is not
    // given — so an execution limit already on the task survived every update.
    s.push("  $dst = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 0) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -MultipleInstances IgnoreNew".to_string());
    s.push("  Set-ScheduledTask -TaskName 'SummriseDesktop' -Action $da -Trigger @($dt1, $dw1) -Settings $dst | Out-Null".to_string());
    s.push(format!("  \"[$(Get-Date -Format o)] desk: SummriseDesktop hardened (guarded 5-min pulse)\" | {log}"));
    s.push("}".to_string());
    s.push("if ((Test-Path $deskDir) -and (Test-Path $deskStart)) {".to_string());
    s.push(format!(
        "  \"[$(Get-Date -Format o)] desk: restarting electron shell\" | {log}"
    ));
    s.push("  Get-Process electron -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue".to_string());
    s.push("  Start-Sleep -Milliseconds 1500".to_string());
    s.push(
        "  $deskTask = Get-ScheduledTask -TaskName 'SummriseDesktop' -ErrorAction SilentlyContinue"
            .to_string(),
    );
    s.push("  if ($deskTask) { Start-ScheduledTask -TaskName 'SummriseDesktop' -ErrorAction SilentlyContinue }".to_string());
    s.push("  else { & cmd /c start /min \"\" powershell -NoProfile -ExecutionPolicy Bypass -File \"$deskStart\" }".to_string());
    s.push(format!(
        "  \"[$(Get-Date -Format o)] desk: electron restart initiated\" | {log}"
    ));
    s.push(format!(
        "}} else {{ \"[$(Get-Date -Format o)] desk: no electron shell (skipped)\" | {log} }}"
    ));
    // stage-brand: heal a stale desktop shortcut (Summrise.lnk -> the retired Tauri exe) and drop
    // the retired orphans. Repair-only, best-effort.
    s.extend(desk_shortcut_repair_ps(
        &format!("{q}\\scripts"),
        &format!("{q}\\components\\summrise-desktop-electron"),
        &log,
    ));
    // SummrisePlaywright is re-registered against the launcher so node.exe no longer allocates a
    // visible console — and so the task needs no VBScript engine to start at all.
    s.push(format!("$pwLn = '{q}\\scripts\\summrise-launch.exe'"));
    s.push(format!("$pwProbe = '{q}\\scripts\\playwright-probe.ps1'"));
    s.push("if ((Test-Path $pwLn) -and (Test-Path $pwProbe)) {".to_string());
    s.push(format!(
        "  $pwNode = '{q}\\components\\playwright\\node.exe'"
    ));
    s.push(format!(
        "  $pwCli  = '{q}\\components\\playwright\\node_modules\\@playwright\\mcp\\cli.js'"
    ));
    s.push("  if ((Test-Path $pwNode) -and (Test-Path $pwCli)) {".to_string());
    // Read the CURRENT task's UserId BEFORE unregistering: `$env:USERNAME` returns SYSTEM when
    // spawned via WMI, and Win32_ComputerSystem.UserName is empty from session 0. The existing
    // task's Principal is the most reliable source.
    s.push("    $oldTask = Get-ScheduledTask -TaskName 'SummrisePlaywright' -ErrorAction SilentlyContinue".to_string());
    s.push("    $pwUser = if ($oldTask) { $oldTask.Principal.UserId } else { (Get-CimInstance Win32_ComputerSystem -ErrorAction SilentlyContinue).UserName -replace '^.*\\\\', '' }".to_string());
    s.push("    try { Unregister-ScheduledTask -TaskName 'SummrisePlaywright' -Confirm:$false -ErrorAction SilentlyContinue } catch {}".to_string());
    s.push(
        "    $pwPs = Join-Path $env:SystemRoot 'System32\\WindowsPowerShell\\v1.0\\powershell.exe'"
            .to_string(),
    );
    // argv[0] IS THE PROGRAM — the launcher starts `$pwPs` with everything after it as its arguments,
    // so the wrapper's own path is not in this list. Every argument reaches CreateProcessW as one
    // argv entry, so a path with a space survives.
    s.push("    $pwArgs = '\"' + $pwPs + '\" -NoProfile -File \"' + $pwProbe + '\" \"' + $pwNode + '\" \"' + $pwCli + '\"'".to_string());
    s.push("    $pwAction = New-ScheduledTaskAction -Execute $pwLn -Argument $pwArgs".to_string());
    s.push("    $pwBoot = New-ScheduledTaskTrigger -AtLogOn".to_string());
    s.push("    $pwWatch = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(1) -RepetitionInterval (New-TimeSpan -Minutes 5)".to_string());
    s.push("    $pwSettings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 0) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable".to_string());
    s.push("    Register-ScheduledTask -TaskName 'SummrisePlaywright' -Action $pwAction -Trigger @($pwBoot, $pwWatch) -Principal (New-ScheduledTaskPrincipal -UserId $pwUser -LogonType Interactive -RunLevel Limited) -Settings $pwSettings -Force | Out-Null".to_string());
    s.push("    Start-ScheduledTask -TaskName 'SummrisePlaywright' | Out-Null".to_string());
    s.push(format!(
        "    \"[$(Get-Date -Format o)] SummrisePlaywright re-registered (probe launcher, user=$pwUser)\" | {log}"
    ));
    s.push("  }".to_string());
    s.push("}".to_string());
    // Layout v2: the swap script itself is transient — delete it LAST, because it is running.
    s.push(
        "try { Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction Stop } catch {}"
            .to_string(),
    );
    s
}

/// The CLI's pre-handoff receipt, in the swap's OWN log sink.
///
/// Written BEFORE anything irreversible, because everything past this point ends with the agent being
/// killed — which is the DOCUMENTED success signal and therefore indistinguishable from a transport
/// failure that never ran this command at all. Returns whether it took, so the caller can say that a
/// failed write costs the log its only way to tell those two apart.
pub fn write_receipt(host: &dyn Host, layout: &Layout, from: &str, to: &str) -> bool {
    let script = update_receipt_ps(&psq(&layout.data_dir), from, to).join("; ");
    let mut argv = vec!["powershell".to_string(), "-NoProfile".to_string()];
    argv.extend(crate::ps::ps_argv(&script));
    host.run(&argv, None).status == Some(0)
}

/// Write `scripts\summrise-update.ps1`, `start-desktop.ps1` and the probe the swap expects to find
/// beside them.
pub fn write_swap_files(host: &dyn Host, layout: &Layout) -> usize {
    let _ = host.mkdirs(Path::new(&layout.scripts_dir));
    let _ = host.write_bytes(
        &win_join(&layout.scripts_dir, "start-desktop.ps1"),
        start_desktop_ps(&psq(&layout.desk_dir), &psq(&layout.logs_dir))
            .join("\r\n")
            .as_bytes(),
    );
    // The probe launcher is written only when a playwright bundle is present — it exists to launch
    // THAT bundle, and on a device without one it is a script nothing calls.
    if host.exists(Path::new(&layout.pw_dir))
        || host.exists(Path::new(&format!("{}\\playwright", layout.dir)))
    {
        let _ = host.write_bytes(
            &win_join(&layout.scripts_dir, "playwright-probe.ps1"),
            playwright_probe_ps().join("\r\n").as_bytes(),
        );
    }
    let body = swap_script(
        &psq(&layout.dir),
        &psq(&layout.data_dir),
        &crate::version::package_version(),
        agent_port(host, &layout.etc_dir),
    );
    let ps1 = win_join(&layout.scripts_dir, "summrise-update.ps1");
    if host
        .write_bytes(&ps1, body.join("\r\n").as_bytes())
        .is_err()
    {
        return 1;
    }
    0
}

/// The WMI handoff: create the swap process OUTSIDE this CLI's job, then READ WHAT IT SAID.
///
/// `Win32_Process.Create` reports success through `ReturnValue`, and the old form printed the object
/// and checked only PowerShell's own exit code — so a `Create` that returned 9 (path not found) still
/// printed "swap launched" and the update was a silent no-op.
pub fn wmi_handoff(host: &dyn Host, layout: &Layout) -> Result<(), String> {
    // Flags matter on this path: children created via WMI with `-ExecutionPolicy Bypass` or
    // `-EncodedCommand` in their command line die silently before running anything (d1, no Defender
    // ASR events — cause unconfirmed). Plain `powershell -NoProfile -File` works, and that requires
    // script execution to be allowed, so lift `Restricted` here once.
    let mut lift = vec![
        "powershell".to_string(),
        "-NoProfile".to_string(),
        "-Command".to_string(),
    ];
    lift.push(
        "if((Get-ExecutionPolicy) -eq 'Restricted'){ Set-ExecutionPolicy RemoteSigned -Scope LocalMachine -Force }"
            .to_string(),
    );
    let _ = host.run(&lift, Some(30_000));

    let ps1 = win_join(&layout.scripts_dir, "summrise-update.ps1");
    // DIR can contain characters that break the inner double-quoted PS literal (backslash, quote):
    // escape for the inner `-File` argument, then the outer WMI literal.
    let ps1_safe = ps1
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let inner = format!("powershell -NoProfile -File \"{ps1_safe}\"");
    let wmi = format!(
        "Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{{CommandLine='{}'}} | ConvertTo-Json -Compress",
        inner.replace('\'', "''")
    );
    let r = host.run(
        &[
            "powershell".into(),
            "-NoProfile".into(),
            "-Command".into(),
            wmi,
        ],
        Some(20_000),
    );
    let retval = serde_json::from_str::<serde_json::Value>(r.stdout.trim())
        .ok()
        .and_then(|j| j.get("ReturnValue").and_then(|v| v.as_i64()));
    if r.status != Some(0) || retval != Some(0) {
        let why = if r.stderr.trim().is_empty() {
            String::new()
        } else {
            format!(" — {}", r.stderr.trim())
        };
        return Err(format!(
            "update: WMI handoff failed (ps status {}, ReturnValue {}){why}",
            r.status
                .map(|c| c.to_string())
                .unwrap_or_else(|| "?".into()),
            retval.map(|c| c.to_string()).unwrap_or_else(|| "?".into())
        ));
    }
    Ok(())
}

/// `summrise update`'s effect body: stage, write the swap, hand off, and ASK THE DEVICE.
///
/// The caller ([`crate::dispatch::update_decision`]) has already run every guard and written the
/// receipt plus the deliberate-stop mark; this is everything past that point, and it is the half
/// that used to answer `exit 3` with "NOT PORTED".
pub fn update_swap(
    host: &dyn Host,
    layout: &Layout,
    pkg_dir: &str,
    from_version: &str,
    to_version: &str,
    timeout_ms: i64,
    interval_ms: i64,
) -> Outcome {
    let busy = crate::psgen::update_busy_path();
    let busy_path = Path::new(&busy);
    let release = || {
        let _ = host.remove_file(busy_path);
    };

    // ── STAGING, INSIDE A GUARD, BECAUSE A THROW HERE STRANDS A LOCK ───────────────────────────
    // A full disk, an AV lock or a permission error used to throw straight out to the top level,
    // leaving the in-progress marker behind — and the NEXT update then refused for ten minutes
    // citing an update that never started, while the operator saw only a stack trace.
    let exe_src = win_join(pkg_dir, "summrise-agent.exe");
    if !host.exists(&exe_src) {
        release();
        return Outcome::fail(
            1,
            format!(
                "exe missing from package: {}\nupdate: staging failed before the swap (exe missing from package)\nupdate: nothing was swapped and the in-progress marker was released -- safe to re-run",
                exe_src.display()
            ),
        );
    }
    let _ = host.mkdirs(Path::new(&layout.dir));
    if host
        .copy_file(&exe_src, &win_join(&layout.dir, "summrise-agent.new.exe"))
        .is_err()
    {
        release();
        return staging_failed("could not stage summrise-agent.new.exe");
    }
    // THE LAUNCHER TRAVELS WITH THE UPDATE: a device updating from a version that registered
    // `wscript.exe <desktop-pulse.vbs>` has no launcher, and the swap repoints both tasks at one. A
    // task naming a program that is not there is Task Scheduler's `0x2` with no explanation.
    let launcher_src = win_join(pkg_dir, "summrise-launch.exe");
    if !host.exists(&launcher_src) {
        release();
        return Outcome::fail(
            1,
            format!(
                "launcher missing from package: {}\nupdate: staging failed before the swap (launcher missing from package)\nupdate: nothing was swapped and the in-progress marker was released -- safe to re-run",
                launcher_src.display()
            ),
        );
    }
    let _ = host.mkdirs(Path::new(&layout.scripts_dir));
    if host
        .copy_file(
            &launcher_src,
            &win_join(&layout.scripts_dir, "summrise-launch.new.exe"),
        )
        .is_err()
    {
        release();
        return staging_failed("could not stage summrise-launch.new.exe");
    }
    let _ = stage_desktop_shell(host, &layout.dir, pkg_dir, ".new");

    // Best-effort, never fail-closed: the manifest is an echo of what was staged, not a gate.
    write_boxed_versions(host, &layout.dir, pkg_dir);

    if write_swap_files(host, layout) != 0 {
        release();
        return staging_failed("could not write scripts\\summrise-update.ps1");
    }
    let mut out = Outcome::ok();

    if let Err(e) = wmi_handoff(host, layout) {
        release();
        return Outcome::fail(1, e);
    }
    // ASK THE DEVICE, DO NOT TRUST THE HANDOFF. `ReturnValue=0` means a process was created; every
    // decision that matters happens after, in a WmiPrvSE-parented script whose exit code nobody
    // reads. `rollback` already solved this by reading the release marker back; `update` reported the
    // HANDOFF instead, and that is the only reason the two commands disagreed about whether an update
    // took. The read is a FILE, not the network, so the dropped connection does not matter.
    out = out.say("update: swap launched -- waiting for the device to confirm");
    let marker = release_marker_path(&layout.dir);
    let opts = AwaitOpts {
        want: to_version.to_string(),
        timeout_ms,
        interval_ms,
    };
    let mut read = || host.read_string(&marker).ok().map(|s| s.trim().to_string());
    let now = || host.now_ms();
    let mut slept = 0i64;
    let check: ReleaseMarkerCheck = await_release_marker(
        &opts,
        &mut read,
        |ms| {
            slept += ms;
            host.sleep_ms(ms.max(0) as u64);
        },
        &now,
    );
    let _ = slept;
    let verdict = release_marker_verdict(&check, to_version, Verb::Update, Some(from_version));
    if verdict.write_pin {
        out = out.say(format!(
            "update: {} -> {to_version} COMPLETE (the device reported the new release)",
            if from_version.is_empty() {
                "?"
            } else {
                from_version
            }
        ));
        return out;
    }
    // Not a pin decision here — `update` never writes one — but the same verdict logic answers
    // "did it take", which is the question the operator asked.
    out.warn(verdict.message).exit(1)
}

fn staging_failed(why: &str) -> Outcome {
    Outcome::fail(
        1,
        format!(
            "update: staging failed before the swap ({why})\nupdate: nothing was swapped and the in-progress marker was released -- safe to re-run"
        ),
    )
}

/// The `summrise run` verb: the agent in the FOREGROUND.
///
/// It is the diagnostic path — an operator who wants to watch the agent's own output rather than
/// read a log — so it inherits the console and returns the child's exit code.
pub fn run_decision(host: &dyn Host, layout: &Layout, args: &[String]) -> Outcome {
    // EXE_DST is always truthy in the TypeScript, so the old `EXE_DST || EXE_SRC` was dead code and
    // there was no existence check: ENOENT yielded `status: null`, `?? 0` reported that as a clean
    // run, and the banner printed either way.
    if !host.exists(Path::new(&layout.exe_dst)) {
        return Outcome::fail(
            1,
            format!(
                "summrise run: agent binary not found at {} -- run 'summrise setup' first",
                layout.exe_dst
            ),
        );
    }
    let mut argv = vec![layout.exe_dst.clone()];
    argv.extend(args.iter().cloned());
    let r = host.run(&argv, None);
    let mut out = Outcome::ok().say("running summrise-agent (foreground, Ctrl+C to stop)");
    match r.status {
        Some(code) => {
            out.exit = code;
            out
        }
        None => out
            .warn("summrise run: could not start the agent (no exit status)")
            .exit(1),
    }
}

/// A path helper kept beside its caller so the package's own tree is spelled one way.
pub fn package_file(pkg_dir: &str, name: &str) -> PathBuf {
    win_join(pkg_dir, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::RunResult;
    use crate::testing::FakeHost;

    fn layout() -> Layout {
        Layout::from_roots("D:\\Summrise", "C:\\ProgramData\\Summrise")
    }

    /// THE RESTART IS IN A `finally`, AND THE LINE THAT REPORTS IT CARRIES THE OUTCOME.
    ///
    /// Two separate defects, and both are shape rather than content: the restart used to be the LAST
    /// statement of a ~24-step sequence with nothing guarding it (an error in between left the device
    /// dark), and the line that reported it read `"task restarted"` unconditionally — a log asserting
    /// a restart it had never verified.
    #[test]
    fn the_restart_is_in_a_finally_and_reports_what_it_got() {
        let s = swap_script(
            "D:\\Summrise",
            "C:\\ProgramData\\Summrise",
            "1.2.400",
            18080,
        );
        let joined = s.join("\r\n");
        let try_at = joined.find("try {").expect("the swap is guarded");
        let finally_at = joined.find("} finally {").expect("a finally block");
        let start_at = joined
            .find("Start-ScheduledTask SummriseAgent")
            .expect("the restart");
        assert!(finally_at > try_at);
        assert!(
            start_at > finally_at,
            "the restart must be INSIDE the finally, or an error anywhere above leaves the device dark"
        );
        assert!(
            joined.contains("task restart ok=$rs"),
            "the restart line must carry the outcome it observed"
        );
        assert!(
            !joined.contains("\"task restarted\""),
            "the unverifiable sentence must not come back"
        );
    }

    /// THE MARKER IS GATED ON THE COPY. A failed swap keeps the device on the old exe, and
    /// `.summrise-release` is what `agent_update` and every UI compare against.
    #[test]
    fn the_release_marker_is_written_only_when_the_copy_proved_itself() {
        let s = swap_script(
            "D:\\Summrise",
            "C:\\ProgramData\\Summrise",
            "1.2.400",
            18080,
        );
        let joined = s.join("\n");
        assert!(
            joined.contains("if ($ok -and '1.2.400') { Set-Content -Path 'D:\\Summrise\\etc\\.summrise-release'"),
            "the marker write must be guarded by $ok"
        );
        // ...and it comes AFTER the copy loop that sets $ok.
        //
        // ONE backslash per separator, in the source and in the assertion: the generator
        // interpolates the already-`psq`'d install dir into a PowerShell single-quoted literal, so
        // the script carries `'D:\Summrise\summrise-agent.new.exe'`. The first version of these
        // three cases wrote `\\\\` (a value of `\\`) and asserted a script PowerShell would read as a
        // UNC-ish path — the parity harness is what settles which side is wrong, and it compares
        // `swap_script` against the TypeScript byte for byte.
        let copy = joined
            .find(
                "foreach($i in 1..12){ try { Copy-Item -Force -ErrorAction Stop 'D:\\Summrise\\summrise-agent.new.exe'",
            )
            .unwrap();
        // THE WRITE, not the first mention of the file: the layout-v2 migration ABOVE the copy loop
        // MOVES `<install>\.summrise-release` into `etc\`, so a bare `find(".summrise-release")`
        // lands on the migration and the case could never pass — while asserting nothing about the
        // write it is named for.
        let mark = joined
            .find("Set-Content -Path 'D:\\Summrise\\etc\\.summrise-release'")
            .expect("the marker write");
        assert!(mark > copy, "the marker follows the copy it attests to");
    }

    /// THE LAUNCHER IS INSTALLED BEFORE EITHER TASK IS RE-REGISTERED. Both tasks name it as their
    /// action, so a swap that repointed them at a program still called `.new` would leave two tasks
    /// that cannot start.
    #[test]
    fn the_launcher_lands_before_the_tasks_that_name_it() {
        let joined = swap_script(
            "D:\\Summrise",
            "C:\\ProgramData\\Summrise",
            "1.2.400",
            18080,
        )
        .join("\n");
        let launcher = joined
            .find(
                "Copy-Item -Force -ErrorAction Stop 'D:\\Summrise\\scripts\\summrise-launch.new.exe'",
            )
            .expect("the launcher copy");
        let desk_task = joined
            .find("New-ScheduledTaskAction -Execute $ln1")
            .expect("the desktop task");
        let pw_task = joined
            .find("New-ScheduledTaskAction -Execute $pwLn")
            .expect("the playwright task");
        assert!(launcher < desk_task, "the launcher must be in place first");
        assert!(launcher < pw_task);
    }

    /// The layout-v2 gate is FAIL-CLOSED and runs BEFORE the swap: aborting there leaves the old
    /// version running untouched, which is only true if nothing was copied yet.
    #[test]
    fn the_migration_gate_aborts_before_anything_is_swapped() {
        let joined = swap_script(
            "D:\\Summrise",
            "C:\\ProgramData\\Summrise",
            "1.2.400",
            18080,
        )
        .join("\n");
        let gate = joined
            .find("migration gate FAILED")
            .expect("the fail-closed gate");
        let copy = joined
            .find("summrise-agent.new.exe' 'D:\\Summrise\\summrise-agent.exe'")
            .expect("the swap copy");
        assert!(gate < copy, "the gate must precede the swap");
    }

    /// THE STAGING IS INSIDE A GUARD: a failure there RELEASES the in-progress marker, or the next
    /// update refuses for ten minutes citing an update that never started.
    #[test]
    fn a_staging_failure_releases_the_marker() {
        let host = FakeHost::new().with_file(&crate::psgen::update_busy_path(), "1");
        let pkg = "C:\\npm\\node_modules\\summrise-agent";
        let r = update_swap(&host, &layout(), pkg, "1.2.300", "1.2.400", 100, 10);
        assert_eq!(r.exit, 1);
        let text = r.err.join("\n");
        assert!(text.contains("staging failed before the swap"), "{text}");
        assert!(text.contains("safe to re-run"), "{text}");
        assert!(
            !host.exists(std::path::Path::new(&crate::psgen::update_busy_path())),
            "the marker must be released"
        );
    }

    /// A MISSING LAUNCHER IS FATAL, unlike the desktop shell's sources: both scheduled tasks name it,
    /// so a device without it has two tasks that cannot start anything.
    #[test]
    fn a_missing_launcher_aborts_the_update() {
        let pkg_dir = "C:\\npm\\node_modules\\summrise-agent";
        let host = FakeHost::new().with_file(&format!("{pkg_dir}\\summrise-agent.exe"), "MZ");
        let r = update_swap(&host, &layout(), pkg_dir, "1.2.300", "1.2.400", 100, 10);
        assert_eq!(r.exit, 1);
        assert!(
            r.err.join("\n").contains("launcher missing from package"),
            "{:?}",
            r.err
        );
    }

    /// The desktop shell is staged as `.new` — never in place, because the running Electron holds
    /// the live files.
    #[test]
    fn the_desktop_shell_is_staged_beside_the_live_files() {
        let pkg_dir = "C:\\npm\\node_modules\\summrise-agent";
        let host = FakeHost::new()
            .with_file(
                &format!("{pkg_dir}\\summrise-desktop-electron\\src\\main.js"),
                "// main",
            )
            .with_file(
                &format!("{pkg_dir}\\summrise-desktop-electron\\src\\preload.js"),
                "// preload",
            )
            .with_file(
                &format!("{pkg_dir}\\summrise-desktop-electron\\icon.ico"),
                "ICO",
            );
        let staged = stage_desktop_shell(&host, "D:\\Summrise", pkg_dir, ".new");
        assert_eq!(staged, 2);
        assert_eq!(
            host.file("D:\\Summrise\\components\\summrise-desktop-electron\\src\\main.js.new")
                .map(|b| String::from_utf8(b).unwrap())
                .as_deref(),
            Some("// main")
        );
        // The icons go NEXT TO src/ (electron loads ../icon.png), and NOT with the suffix.
        assert!(host
            .file("D:\\Summrise\\components\\summrise-desktop-electron\\icon.ico")
            .is_some());
        // The minimal manifest `electron .` needs to resolve its entry at all.
        let manifest = String::from_utf8(
            host.file("D:\\Summrise\\components\\summrise-desktop-electron\\package.json")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(manifest, shell_manifest_json());
        assert!(manifest.contains("\"main\": \"src/main.js\""));
    }

    /// A package that ships no shell sources stages ZERO, which is the fact the caller's sentence
    /// depends on — it used to claim "sources staged" regardless.
    #[test]
    fn a_package_without_shell_sources_stages_zero() {
        let host = FakeHost::new();
        assert_eq!(
            stage_desktop_shell(&host, "D:\\Summrise", "C:\\nope", ""),
            0
        );
    }

    /// The WMI handoff READS `ReturnValue`. A `Create` that returned 9 (path not found) still
    /// printed "swap launched" under the old form, and the update was a silent no-op.
    #[test]
    fn the_handoff_reads_the_return_value_and_refuses_a_nonzero_one() {
        let host = FakeHost::new().script_runs(vec![
            RunResult {
                status: Some(0),
                ..Default::default()
            }, // the execution-policy lift
            RunResult {
                status: Some(0),
                stdout: "{\"ReturnValue\":9,\"ProcessId\":0}".into(),
                ..Default::default()
            },
        ]);
        let e = wmi_handoff(&host, &layout()).unwrap_err();
        assert!(e.contains("ReturnValue 9"), "{e}");

        let host = FakeHost::new().script_runs(vec![
            RunResult {
                status: Some(0),
                ..Default::default()
            },
            RunResult {
                status: Some(0),
                stdout: "{\"ReturnValue\":0,\"ProcessId\":4321}".into(),
                ..Default::default()
            },
        ]);
        assert!(wmi_handoff(&host, &layout()).is_ok());
    }

    /// `run` refuses a missing binary instead of reporting ENOENT as a clean exit.
    #[test]
    fn run_refuses_a_missing_agent_binary() {
        let host = FakeHost::new();
        let r = run_decision(&host, &layout(), &[]);
        assert_eq!(r.exit, 1);
        assert!(r.err[0].contains("agent binary not found"), "{:?}", r.err);
        assert!(host.runs().is_empty(), "and it must not try to start it");

        // ...and a started agent's exit code is the command's exit code.
        let host = FakeHost::new()
            .with_file("D:\\Summrise\\summrise-agent.exe", "MZ")
            .script_runs(vec![RunResult {
                status: Some(3),
                ..Default::default()
            }]);
        let r = run_decision(&host, &layout(), &["--port".into(), "1".into()]);
        assert_eq!(r.exit, 3);
        assert_eq!(host.runs()[0][0], "D:\\Summrise\\summrise-agent.exe");
        assert_eq!(host.runs()[0].len(), 3, "the agent's own argv is forwarded");
    }
}
