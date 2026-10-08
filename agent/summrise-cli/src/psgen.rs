//! THE POWERSHELL THIS CLI GENERATES, as pure functions.
//!
//! Ported 1:1 from `agent/summrise-agent-npm/src/summrise.ts`. Every function here returns the
//! LINES of a script; nothing here writes a file or spawns anything, which is why the whole
//! cluster is testable on Linux and why the oracle's cases for it are the strongest part of the
//! suite: they pin the exact text a SYSTEM/admin PowerShell will run.
//!
//! **THE ARGUMENT CONVENTION IS LOAD-BEARING.** The `_q`-suffixed parameters take a value ALREADY
//! escaped for a PowerShell single-quoted literal — the caller writes `psq(dir)`. The TypeScript
//! does exactly this, and a port that escaped twice would produce `a''''b` for a path with one
//! quote in it. The exceptions escape internally and are named in their own docs (the C#
//! here-string in [`desk_shortcut_repair_ps`], and the version strings, which are `\d+\.\d+\.\d+`
//! by [`crate::components::rollback_version_ok`] and contain nothing to escape).

/// The taskbar identity: ONE string, and it lives on BOTH sides of an association.
///
/// It is the AppUserModelID the desktop shell sets on its process and the value written into
/// `System.AppUserModel.ID` on the shortcuts below. Microsoft's lookup for the taskbar group's
/// icon takes the shortcut arm only when the window carries an explicit ID, and the window's arm
/// only names a shortcut carrying the SAME ID — so the two halves are one association and this is
/// the string that ties them.
///
/// WHY IT IS NOT `online.saisi.summrise.agent`, MEASURED on desktop-14rjcr8 one variable at a time:
/// the old ID had no shortcut anywhere carrying it, so the shell resolved it against the backing
/// executable — stock `electron.exe` — and kept the result. Deleting the Start Menu shortcut
/// reverted a running shell carrying the RIGHT ID to the Electron logo, which is what says the
/// identity has to live on the Start Menu link and not only on the desktop ones.
pub const DESKTOP_AUMID: &str = "online.saisi.summrise.desktop";

/// The two `IPropertyStore` values a `Summrise.lnk` must carry, as DATA.
///
/// Separated from the PowerShell that performs the write so the DECISION is testable where the COM
/// call is not. `icoPath` is the shell's own sunrise `.ico`; the `,0` is the icon index and is
/// REQUIRED even for a single-image `.ico`.
pub fn lnk_identity(ico_path: &str) -> (String, String) {
    (DESKTOP_AUMID.to_string(), format!("{ico_path},0"))
}

/// C# interop for the two shortcut properties `WScript.Shell` cannot set.
///
/// `System.AppUserModel.ID` and `System.AppUserModel.RelaunchIconResource` are not fields of the
/// `.lnk` header — they live in the shortcut's `IPropertyStore`, and `WScript.Shell` exposes no way
/// to reach one. `Add-Type` compiles this with the C# compiler that ships IN the box, which is what
/// keeps a stock Windows machine (no Node, no Rust, no SDK) able to repair its own shortcut.
///
/// TWO THINGS THE DEVICE TAUGHT, both load-bearing: `IPersistFile::Load` must be given
/// STGM_READWRITE (2) — loading with 0 opens the link READ-ONLY and the later
/// `IPropertyStore::Commit` fails STG_E_ACCESSDENIED while reporting nothing the caller can see —
/// and the write is verified by READING THE PROPERTIES BACK, which is also what decides whether a
/// shortcut needs repairing at all.
pub const LNK_PROPS_CS: &str = r#"using System;
using System.Runtime.InteropServices;
public static class SummriseLnk {
  [StructLayout(LayoutKind.Sequential, Pack = 4)]
  struct PROPERTYKEY { public Guid fmtid; public uint pid; }
  [StructLayout(LayoutKind.Sequential)]
  struct PROPVARIANT { public ushort vt, r1, r2, r3; public IntPtr p, p2; }
  [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IPropertyStore {
    int GetCount(out uint c);
    int GetAt(uint i, out PROPERTYKEY k);
    int GetValue(ref PROPERTYKEY k, out PROPVARIANT v);
    int SetValue(ref PROPERTYKEY k, ref PROPVARIANT v);
    int Commit();
  }
  [ComImport, Guid("0000010b-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IPersistFile {
    void GetClassID(out Guid g);
    [PreserveSig] int IsDirty();
    void Load([MarshalAs(UnmanagedType.LPWStr)] string f, uint m);
    void Save([MarshalAs(UnmanagedType.LPWStr)] string f, [MarshalAs(UnmanagedType.Bool)] bool r);
    void SaveCompleted([MarshalAs(UnmanagedType.LPWStr)] string f);
    void GetCurFile([MarshalAs(UnmanagedType.LPWStr)] out string f);
  }
  static readonly Guid CLSID = new Guid("00021401-0000-0000-C000-000000000046");
  static readonly Guid FMT = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3");
  const uint RW = 2;
  static PROPERTYKEY K(uint pid) { PROPERTYKEY k; k.fmtid = FMT; k.pid = pid; return k; }
  static object New(string lnk, uint mode) {
    object o = Activator.CreateInstance(Type.GetTypeFromCLSID(CLSID));
    ((IPersistFile)o).Load(lnk, mode);
    return o;
  }
  static string Get(string lnk, uint pid, uint mode) {
    IPropertyStore s = (IPropertyStore)New(lnk, mode);
    PROPERTYKEY k = K(pid); PROPVARIANT v;
    if (s.GetValue(ref k, out v) != 0) return "";
    if (v.vt != 31 || v.p == IntPtr.Zero) return "";
    string r = Marshal.PtrToStringUni(v.p);
    Marshal.FreeCoTaskMem(v.p);
    return r;
  }
  static void Put(IPropertyStore s, uint pid, string val) {
    PROPERTYKEY k = K(pid); PROPVARIANT v = new PROPVARIANT();
    v.vt = 31; v.p = Marshal.StringToCoTaskMemUni(val);
    try { int hr = s.SetValue(ref k, ref v); if (hr != 0) Marshal.ThrowExceptionForHR(hr); }
    finally { Marshal.FreeCoTaskMem(v.p); }
  }
  public static string Read(string lnk) { return Get(lnk, 5, 0) + "|" + Get(lnk, 3, 0); }
  public static void Write(string lnk, string id, string icon) {
    object o = New(lnk, RW);
    IPropertyStore s = (IPropertyStore)o;
    Put(s, 5, id); Put(s, 3, icon);
    int hr = s.Commit();
    if (hr != 0) Marshal.ThrowExceptionForHR(hr);
    ((IPersistFile)o).Save(lnk, true);
  }
}"#;

/// The desktop shortcut repair: REPAIR ONLY, plus the taskbar identity.
///
/// A 2026-09-01 `Summrise.lnk` on devices points at the RETIRED Tauri `summrise-desktop.exe`
/// (embedded stale icon) — double-clicking it launches the dead app instead of the Electron shell.
/// The repair repoints at `start-desktop.ps1` with `IconLocation` pinned to the sunrise `icon.ico`,
/// removes the retired orphans, and writes the taskbar identity into the link's property store.
///
/// The `_q` arguments are single-quote-escaped by the caller. `sink` is a PS output pipe
/// (`Write-Host`, or the update log pipe).
pub fn desk_shortcut_repair_ps(scripts_q: &str, desk_dir_q: &str, sink: &str) -> Vec<String> {
    let (id, icon) = lnk_identity(&format!("{desk_dir_q}\\icon.ico"));

    // BOTH shortcuts get the identity, and the START MENU one is the load-bearing half: deleting it
    // from a running shell reverted the taskbar to the Electron logo with the ID still set on the
    // process and both desktop shortcuts still carrying it.
    let lnk = |p: &str, tag: &str| -> Vec<String> {
        vec![
            format!(
                "  try {{ $dWs{tag} = New-Object -ComObject WScript.Shell; $dSc{tag} = $dWs{tag}.CreateShortcut({p}); $dSc{tag}.TargetPath = Join-Path $env:SystemRoot 'System32\\WindowsPowerShell\\v1.0\\powershell.exe'; $dSc{tag}.Arguments = '-NoProfile -ExecutionPolicy Bypass -File \"' + $dPs1 + '\"'; $dSc{tag}.WorkingDirectory = '{desk_dir_q}'; $dSc{tag}.IconLocation = $dIco + ',0'; $dSc{tag}.Save(); 'desk: ' + {p} + ' repointed to electron shell' | {sink} }} catch {{ ('desk: ' + {p} + ' repair failed: ' + $_.Exception.Message) | {sink} }}"
            ),
            format!(
                "  if ($dSet) {{ try {{ [SummriseLnk]::Write({p}, $dId, $dRes); ('desk: ' + {p} + ' taskbar identity [' + [SummriseLnk]::Read({p}) + ']') | {sink} }} catch {{ ('desk: ' + {p} + ' identity write failed: ' + $_.Exception.Message) | {sink} }} }}"
            ),
        ]
    };

    let mut out: Vec<String> = vec![
        "$dLnk = Join-Path $env:PUBLIC 'Desktop\\Summrise.lnk'".to_string(),
        // ALL-USERS Start Menu: the shortcut the shell's app resolver actually reads, and it is
        // CREATED here when absent — a machine that never had one is exactly the machine whose
        // taskbar shows the Electron logo.
        "$dSm = Join-Path $env:ProgramData 'Microsoft\\Windows\\Start Menu\\Programs\\Summrise.lnk'"
            .to_string(),
        format!("$dIco = '{desk_dir_q}\\icon.ico'"),
        format!("$dPs1 = '{scripts_q}\\start-desktop.ps1'"),
        format!("$dId = '{id}'"),
        format!("$dRes = '{icon}'"),
        "$dSet = $false".to_string(),
        "$dShell = (Test-Path $dIco) -and (Test-Path $dPs1)".to_string(),
        // Compiled when there is a SHELL to point at, not when a desktop link happens to exist: the
        // Start Menu shortcut is created from nothing, so gating this on `$dLnk` would skip the
        // identity on precisely the installs that need it. The `-as [type]` guard keeps a second
        // call in one process from turning a "type already exists" throw into a silent skip.
        "if ($dShell) {".to_string(),
        format!("  $dCs = @'\n{LNK_PROPS_CS}\n'@"),
        format!(
            "  try {{ if (-not ('SummriseLnk' -as [type])) {{ Add-Type -TypeDefinition $dCs }}; $dSet = [bool]('SummriseLnk' -as [type]) }} catch {{ ('desk: lnk identity helper unavailable: ' + $_.Exception.Message) | {sink} }}"
        ),
        "}".to_string(),
        "$dNeed = $false".to_string(),
        "if (Test-Path $dLnk) {".to_string(),
        "  try { $dEx = (New-Object -ComObject WScript.Shell).CreateShortcut($dLnk); if (($dEx.TargetPath -like '*summrise-desktop.exe') -or ($dEx.TargetPath -like '*summrise-tray.exe') -or (-not (Test-Path $dEx.TargetPath))) { $dNeed = $true } } catch { $dNeed = $true }".to_string(),
        // ...and a link that is otherwise healthy still needs repairing when its identity is
        // missing or names a different ID/icon. `Read` returns "id|icon", so ONE comparison covers
        // both properties.
        "  if ($dSet) { try { if (([SummriseLnk]::Read($dLnk)) -ne ($dId + '|' + $dRes)) { $dNeed = $true } } catch { $dNeed = $true } }".to_string(),
        "}".to_string(),
        // The Start Menu link is created when ABSENT and rewritten when its identity is wrong — it
        // is never deleted, and a link that already matches is left alone.
        "$dSmNeed = $false".to_string(),
        "if ($dShell) {".to_string(),
        "  if (-not (Test-Path $dSm)) { $dSmNeed = $true }".to_string(),
        "  elseif (-not $dSet) { $dSmNeed = $true }".to_string(),
        "  else { try { if (([SummriseLnk]::Read($dSm)) -ne ($dId + '|' + $dRes)) { $dSmNeed = $true } } catch { $dSmNeed = $true } }".to_string(),
        "}".to_string(),
        "if ($dNeed -and $dShell) {".to_string(),
    ];
    out.extend(lnk("$dLnk", "1"));
    out.push("}".to_string());
    out.push("if ($dSmNeed -and $dShell) {".to_string());
    out.extend(lnk("$dSm", "2"));
    out.push("}".to_string());
    out.push(format!(
        "foreach ($dRx in @('summrise-desktop.exe','summrise-tray.exe')) {{ $dRp = '{desk_dir_q}\\' + $dRx; if (Test-Path $dRp) {{ try {{ Remove-Item -Force -ErrorAction Stop $dRp; ('desk: removed retired ' + $dRx) | {sink} }} catch {{ ('desk: retired ' + $dRx + ' locked, kept') | {sink} }} }} }}"
    ));
    out
}

/// The desktop shell's launcher — and the only place in the product that can see it EXIT.
///
/// `exit code N` is what tells a crash from a kill, and GETTING IT COST A MEASUREMENT: electron.exe
/// is a GUI-subsystem binary and Windows PowerShell 5.1 does NOT set `$LASTEXITCODE` for one.
/// Measured: `& cmd /c 'exit 7'` set it to 7, the next `& electron.exe --version` left it at 7
/// unchanged, and the first version of this file wrote `exit code ` with the number missing. The
/// launch therefore goes through `System.Diagnostics.Process` with `UseShellExecute = $false`: the
/// same CreateProcess the call operator uses, plus `WaitForExit()` and a real `.ExitCode`.
///
/// WHAT IT CANNOT SEE, and it is the case that happened: a Task Scheduler execution limit kills the
/// whole tree, so this PowerShell dies first and NO exit code is written at all. The comment block
/// inside the generated file says so, and [`ensure_desktop_ps`] reports that absence next pulse.
pub fn start_desktop_ps(desk_dir_q: &str, logs_q: &str) -> Vec<String> {
    let mut out = vec![
        "# written by `summrise setup` / `summrise desktop` / `summrise update` -- start the",
        "# Electron shell, and leave a record of the launch and of the exit. The comments stay",
        "# IN the file on purpose: this is what an operator reads when the window is gone.",
        "#",
        "# WHAT THIS CANNOT SEE. The launch below WAITS, so it returns with the shell's exit code",
        "# when the shell exits OR crashes. It does NOT return when the whole process tree is",
        "# killed from outside -- a Task Scheduler execution limit, a `taskkill /T`, a machine",
        "# going down -- because this PowerShell dies first, and then NO exit code is written at",
        "# all. ensure-desktop.ps1 reports that absence on the next pulse; a log that recorded",
        "# only exits would silently omit the one death that actually happens here.",
        "#",
        "# WHY NOT `& electron.exe .` AND $LASTEXITCODE (measured on desktop-14rjcr8, 2026-09-28):",
        "# electron.exe is a GUI-subsystem binary and Windows PowerShell 5.1 does not set",
        "# $LASTEXITCODE for one. `& cmd /c 'exit 7'` set it to 7; the next `& electron.exe",
        "# --version` left it at 7, unchanged. The first version of this file used `&` and wrote",
        "# `exit code ` with the number missing -- a field that looks like a value and is not one.",
        "# System.Diagnostics.Process is the same CreateProcess, and its ExitCode is real.",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<String>>();
    out.push(format!("$dir = '{desk_dir_q}'"));
    out.push(format!("$log = '{logs_q}\\startup.log'"));
    for line in [
        "New-Item -ItemType Directory -Force -Path (Split-Path $log) | Out-Null",
        "$E = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()",
        "Add-Content -Path $log -Value ('[' + (Get-Date -Format o) + '] desktop: electron.exe launching from ' + $dir + ' (start=' + $E + ')') -ErrorAction SilentlyContinue",
        "Set-Location $dir",
        "$code = $null",
        "try {",
        "  $si = New-Object System.Diagnostics.ProcessStartInfo",
        "  $si.FileName = \"$dir\\node_modules\\electron\\dist\\electron.exe\"",
        "  $si.Arguments = '.'",
        "  $si.WorkingDirectory = $dir",
        "  $si.UseShellExecute = $false",
        "  $p = [System.Diagnostics.Process]::Start($si)",
        "  $p.WaitForExit()",
        "  $code = $p.ExitCode",
        "} catch { $code = $null }",
        "$L = [int]([DateTimeOffset]::UtcNow.ToUnixTimeSeconds() - $E)",
        "if ($null -ne $code) {",
        "  Add-Content -Path $log -Value ('[' + (Get-Date -Format o) + '] desktop: electron.exe exited (start=' + $E + ' exit code ' + $code + ' lived=' + $L + 's)') -ErrorAction SilentlyContinue",
        "} else {",
        "  Add-Content -Path $log -Value ('[' + (Get-Date -Format o) + '] desktop: electron.exe DID NOT RUN (start=' + $E + ') -- the launcher could not start it, so there is no exit code') -ErrorAction SilentlyContinue",
        "}",
    ] {
        out.push(line.to_string());
    }
    out
}

/// The desktop shell's WATCHDOG: start the shell when none is running, and RECORD WHICH OF THE TWO
/// THINGS HAPPENED — in the words a person would search for.
///
/// THE HEALTHY CASE IS LOGGED ONCE AN HOUR, and both halves of that are deliberate. A line per
/// 5-minute pulse is 288 lines a day of "still fine" in `startup.log`, which is the AGENT's boot
/// record and rotates at 1 MB — that heartbeat would evict the history of a real incident. Writing
/// NOTHING is worse in the other direction: a watchdog that has stopped and one that is fine would
/// look identical, because both are SILENCE. So: one line an hour, deduplicated against the LOG
/// ITSELF rather than a state file (the record is the state, so nothing can go out of sync).
///
/// THE DECISION IS STATED AS THE OBSERVATION: "no electron shell is running" is what the pulse SAW.
/// When the shell that disappeared left no `exited (start=N …)` line, this says so in words — that
/// is the scheduler-kill signature, and it is exactly the case [`start_desktop_ps`] cannot record.
pub fn ensure_desktop_ps(scripts_q: &str, logs_q: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![
        "# written by `summrise setup` / `summrise desktop` / `summrise update` -- the desktop",
        "# shell's watchdog, run every 5 minutes by desktop-pulse.vbs under SummriseDesktop.",
        "# Start the shell if none is running, and RECORD WHICH OF THE TWO THINGS HAPPENED:",
        "# silence is not a status. The builder in src/summrise.ts carries the full reasoning,",
        "# including why the healthy case is one line an hour and not one per pulse.",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    out.push(format!("$log = '{logs_q}\\startup.log'"));
    for line in [
        "New-Item -ItemType Directory -Force -Path (Split-Path $log) | Out-Null",
        "$now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()",
        "$tail = @(Get-Content -Path $log -Tail 400 -ErrorAction SilentlyContinue)",
        "$p = @(Get-Process electron -ErrorAction SilentlyContinue)",
        "if ($p.Count -gt 0) {",
        "  $h = 'hour=' + (Get-Date -Format 'yyyy-MM-ddTHH')",
        "  if (-not ($tail -match ('desktop: electron shell alive .*' + $h))) {",
        "    $up = ($p | Sort-Object StartTime | Select-Object -First 1).StartTime",
        "    Add-Content -Path $log -Value ('[' + (Get-Date -Format o) + '] desktop: electron shell alive -- ' + $p.Count + ' process(es), oldest since ' + $up + ' ' + $h) -ErrorAction SilentlyContinue",
        "  }",
        "  exit",
        "}",
        "$was = 'no earlier launch is recorded in the log tail'",
        // THE PATTERN MUST MATCH THE LINE `startDesktopPs` ACTUALLY WRITES, and the first version
        // did NOT: it read `launching \(start=` while the launcher writes `launching from <dir>
        // (start=`. Every pulse therefore fell through to "no earlier launch is recorded" and the
        // previous shell's lifetime was never reported — measured on desktop-14rjcr8, where the
        // line it was looking for sat thirty lines above it in the same file.
        "$prev = $tail | Select-String -Pattern 'desktop: electron.exe launching .*\\(start=(\\d+)\\)' | Select-Object -Last 1",
        "if ($prev -and ($prev.Line -match '\\(start=(\\d+)\\)')) {",
        "  $E = [int]$Matches[1]",
        "  if ($tail -match ('desktop: electron\\.exe exited \\(start=' + $E + ' ')) { $was = 'the previous shell exited and its code is in the exited (start=' + $E + ') line' }",
        "  else { $was = 'the previous shell lived ' + ([int]$now - $E) + 's and left NO exit line -- it was KILLED, not exited: a Task Scheduler stop, a taskkill /T or a reboot leaves no exit code (start=' + $E + ')' }",
        "}",
        "Add-Content -Path $log -Value ('[' + (Get-Date -Format o) + '] desktop: no electron shell is running -- the watchdog is starting it; previous: ' + $was) -ErrorAction SilentlyContinue",
    ] {
        out.push(line.to_string());
    }
    out.push(format!(
        "& powershell -NoProfile -ExecutionPolicy Bypass -File \"{scripts_q}\\start-desktop.ps1\""
    ));
    out
}

/// The desktop shell's watchdog pair + its task, as ONE PowerShell script.
///
/// WHY IT IS A FILE AND NOT AN INLINE `-Command`: it has to write two other files whose contents
/// contain quotes, and every attempt to do that inside one command string turns into quoting hell.
/// [`ensure_desktop_ps`]'s body is carried in a single-quoted here-string, so it arrives byte for
/// byte and nothing in it can end the here-string early.
pub fn desktop_task_ps(install_q: &str, logs_q: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![
        "# written by `summrise setup` / `summrise desktop` -- the desktop shell's task + watchdog."
            .to_string(),
        "$ErrorActionPreference = 'Stop'".to_string(),
        format!("$q = '{install_q}'"),
        "$en = Join-Path $q 'scripts\\ensure-desktop.ps1'".to_string(),
        "$ln = Join-Path $q 'scripts\\summrise-launch.exe'".to_string(),
        // THE LAUNCHER'S FIRST ARGUMENT IS THE PROGRAM. An action written as
        // `-Argument '-NoProfile …'` asks it to start a program NAMED `-NoProfile`, which fails with
        // the launcher's own exit 126 and one line in launcher.log.
        "$ps = Join-Path $env:SystemRoot 'System32\\WindowsPowerShell\\v1.0\\powershell.exe'"
            .to_string(),
        // THE WATCHDOG'S BODY COMES FROM ITS OWN BUILDER, not from a second hand-typed copy: three
        // paths write this file, and a copy that drifts silently reverts the shell's record on
        // whichever path was not edited.
        "$enBody = @'".to_string(),
    ];
    out.extend(ensure_desktop_ps(&format!("{install_q}\\scripts"), logs_q));
    out.push("'@".to_string());
    out.push("Set-Content -Path $en -Value $enBody -Force".to_string());
    // THE TWO `.vbs` WRAPPERS ARE RETIRED, AND THIS IS WHERE THEY WERE BORN. Guarded on purpose:
    // `$ErrorActionPreference` is `Stop` here, so an unguarded Remove-Item that failed would abort
    // the registration BELOW and leave the OLD task in place.
    out.push(format!(
        "foreach ($v in @('{install_q}\\scripts\\desktop-pulse.vbs','{install_q}\\desktop-pulse.vbs','{install_q}\\scripts\\run-hidden.vbs','{install_q}\\playwright\\run-hidden.vbs')) {{ if (Test-Path $v) {{ try {{ Remove-Item -Force -ErrorAction Stop $v }} catch {{}} }} }}"
    ));
    // THE ACTION IS THE LAUNCHER, NOT `wscript.exe <desktop-pulse.vbs>`. The `.vbs` existed for ONE
    // property — a hidden window, without waiting — and it bought that with a dependency on a
    // Windows component Microsoft is retiring.
    out.push(
        "$da = New-ScheduledTaskAction -Execute $ln -Argument ('\"' + $ps + '\" -NoProfile -ExecutionPolicy Bypass -File \"' + $en + '\"') -WorkingDirectory $q".to_string(),
    );
    out.push("$dt1 = New-ScheduledTaskTrigger -AtLogOn".to_string());
    out.push("$dw1 = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(3) -RepetitionInterval (New-TimeSpan -Minutes 5)".to_string());
    out.push("$pr = New-ScheduledTaskPrincipal -UserId ('{0}\\{1}' -f $env:USERDOMAIN, $env:USERNAME) -LogonType Interactive -RunLevel Highest".to_string());
    // NO EXECUTION TIME LIMIT — the same value, in the same spelling, the agent's own task has
    // carried since round 118: `-ExecutionTimeLimit 0   never kill the running task`. It was
    // `-Minutes 10` here, and that WAS the defect: the launcher's PowerShell stays alive as the
    // shell's PARENT, and Task Scheduler enforces the limit on that whole tree, so the shell was
    // killed every ten minutes and the guarded pulse correctly revived it — which the operator
    // reported as "反复重启", four times.
    out.push("$st = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 0) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -MultipleInstances IgnoreNew".to_string());
    out.push("Register-ScheduledTask SummriseDesktop -Action $da -Trigger @($dt1,$dw1) -Principal $pr -Settings $st -Force | Out-Null".to_string());
    out.push("Start-ScheduledTask -TaskName SummriseDesktop".to_string());
    out
}

/// The desktop shell's START script, as a file — NOT a `-Command` string.
///
/// WHY A FILE, the third thing the device taught us: the CLI spawned with a shell, so the command
/// went through cmd.exe, and cmd treats `&` as a command separator. The PowerShell call operator
/// `&` therefore split the command in half, the `$t` assignment never ran, and it fell through to
/// re-registering the task — which then failed on the service account's principal.
///
/// Prints ONE WORD the caller can trust: `already-running`, `started`, or `not-started`.
pub fn desktop_start_ps(install_q: &str) -> Vec<String> {
    let mut out: Vec<String> = [
        "# written by `summrise desktop` -- ask for the window, or say it is already there.",
        "$ErrorActionPreference = 'Stop'",
        "if (Get-Process electron -ErrorAction SilentlyContinue) { Write-Output 'already-running'; exit 0 }",
        "# DO NOT RE-REGISTER THE TASK TO START IT. Register-ScheduledTask needs the interactive",
        "# user's principal as DOMAIN\\user, and a shell running as a service account has no such",
        "# mapping -- the device answered \"No mapping between account names and security IDs was",
        "# done ... UserId\" from a PTY running as systemprofile, while the task itself was Ready.",
        "# The task already carries the right principal -- `summrise setup` creates it -- so starting",
        "# it is all that is needed.",
        "$t = Get-ScheduledTask -TaskName SummriseDesktop -ErrorAction SilentlyContinue",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    out.push(format!(
        "if (-not $t) {{ & powershell -NoProfile -ExecutionPolicy Bypass -File \"{install_q}\\scripts\\register-desktop-task.ps1\" }}"
    ));
    for line in [
        "Start-ScheduledTask -TaskName SummriseDesktop -ErrorAction SilentlyContinue",
        "Start-Sleep -Seconds 3",
        "if (Get-Process electron -ErrorAction SilentlyContinue) { Write-Output 'started' } else { Write-Output 'not-started'; exit 1 }",
    ] {
        out.push(line.to_string());
    }
    out
}

/// The desktop task's REGISTRATION script — the one `summrise desktop` falls back to when the task
/// is absent.
///
/// Not a separate builder in the TypeScript: the fallback names `register-desktop-task.ps1` on
/// disk, and this is the text that file must carry. It is [`desktop_task_ps`] with the install's own
/// `logs` directory, so a device that never ran `setup` can still get a working task and the
/// 5-minute trigger never steals focus from a live shell.
pub fn register_desktop_task_ps(install_q: &str) -> Vec<String> {
    desktop_task_ps(install_q, &format!("{install_q}\\logs"))
}

/// Idempotent Windows firewall inbound rule for the agent port.
///
/// LAN clients are dropped at the firewall otherwise, even bound `0.0.0.0`. Prunes our OWN
/// stale-port rules by `DisplayName` and never touches a foreign one.
pub fn firewall_ps(port: u16) -> Vec<String> {
    vec![
        format!("$fwPort = {port};"),
        "foreach ($fr in @(Get-NetFirewallRule -DisplayName 'Summrise Agent' -ErrorAction SilentlyContinue)) { try { $fp = @(Get-NetFirewallPortFilter -AssociatedNetFirewallRule $fr | Select-Object -ExpandProperty LocalPort); if ($fp -notcontains \"$fwPort\") { Remove-NetFirewallRule -Name $fr.Name -Confirm:$false -ErrorAction SilentlyContinue } } catch {} }".to_string(),
        "if (-not (Get-NetFirewallRule -DisplayName 'Summrise Agent' -ErrorAction SilentlyContinue | Where-Object { @(Get-NetFirewallPortFilter -AssociatedNetFirewallRule $PSItem | Select-Object -ExpandProperty LocalPort) -contains \"$fwPort\" })) { New-NetFirewallRule -DisplayName 'Summrise Agent' -Direction Inbound -LocalPort $fwPort -Protocol TCP -Action Allow | Out-Null }".to_string(),
    ]
}

/// SummriseAgent boot-task registration (SYSTEM, hardened).
///
/// The task's `-Argument` is the EXPLICIT config path (layout v2: `etc\config.yaml`) — never the
/// exe path: the Rust agent takes `argv[1]` as the config FILE, so an exe path fails YAML parse and
/// quarantines the install. Shared by `setup` (fresh install) and the update swap (repoint,
/// fail-closed). `start` appends the kick for setup; the swap omits it.
pub fn boot_task_ps(exe_q: &str, cfg_q: &str, start: bool) -> Vec<String> {
    let mut lines = vec![
        format!("$action = New-ScheduledTaskAction -Execute '{exe_q}' -Argument ('\"' + '{cfg_q}' + '\"')"),
        "$boot = New-ScheduledTaskTrigger -AtStartup".to_string(),
        "$watch = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(3) -RepetitionInterval (New-TimeSpan -Minutes 5)".to_string(),
        "$principal = New-ScheduledTaskPrincipal -UserId SYSTEM -LogonType ServiceAccount -RunLevel Highest".to_string(),
        "$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 0) -RestartCount 8 -RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable".to_string(),
        "Register-ScheduledTask SummriseAgent -Action $action -Trigger @($boot,$watch) -Principal $principal -Settings $settings -Force | Out-Null".to_string(),
    ];
    if start {
        lines.push("Start-ScheduledTask SummriseAgent".to_string());
    }
    lines
}

/// Layout-v2 one-time migration (ADR 0008) for the setup/update paths.
///
/// Mirrors `paths.rs` migration_moves EXACTLY (same pairs — both sides are pinned by tests; drift
/// strands upgraded devices). Moves only when the target is missing (never clobbers staged `.new`
/// output); dirs merge children. The marker guard is precomputed into `$summriseMg` and every
/// statement carries it, because all statements stay SINGLE-LINE (setup joins them with `; `
/// through `-Command`).
pub fn migrate_layout_ps(q: &str, dq: &str) -> Vec<String> {
    let mvf = |old_rel: &str, new_abs: &str| {
        format!("if ($summriseMg -and (Test-Path '{q}\\{old_rel}') -and (-not (Test-Path '{new_abs}'))) {{ try {{ New-Item -ItemType Directory -Force -Path (Split-Path '{new_abs}') | Out-Null; Move-Item -Force -Path '{q}\\{old_rel}' -Destination '{new_abs}' -ErrorAction Stop }} catch {{}} }}")
    };
    let mvd = |old_rel: &str, new_abs: &str| {
        format!("if ($summriseMg -and (Test-Path '{q}\\{old_rel}')) {{ try {{ if (-not (Test-Path '{new_abs}')) {{ New-Item -ItemType Directory -Force -Path (Split-Path '{new_abs}') | Out-Null; Move-Item -Path '{q}\\{old_rel}' -Destination '{new_abs}' -ErrorAction Stop }} else {{ Get-ChildItem -Force '{q}\\{old_rel}' | ForEach-Object {{ if (-not (Test-Path (Join-Path '{new_abs}' $_.Name))) {{ Move-Item -Force -Path $_.FullName -Destination (Join-Path '{new_abs}' $_.Name) -ErrorAction SilentlyContinue }} }} }} }} catch {{}} }}")
    };
    let etc = format!("{q}\\etc");
    let comp = format!("{q}\\components");
    let scr = format!("{q}\\scripts");
    let logs = format!("{dq}\\logs");
    // (oldRel, newAbs, kind) — 'f' files move atomically, 'd' dirs merge children. **THE TWO `.vbs`
    // WRAPPERS ARE NOT HERE**: both scheduled tasks run `scripts\summrise-launch.exe` now, so
    // setup/update delete either copy instead of carrying it forward, and `paths.rs`'s plan asserts
    // the same absence. A port that kept the old pairs would migrate a file nothing runs.
    let moves: [(&str, String, char); 22] = [
        ("config.yaml", format!("{etc}\\config.yaml"), 'f'),
        (
            "summrise-agent.hostname",
            format!("{etc}\\summrise-agent.hostname"),
            'f',
        ),
        ("tunnel.yml", format!("{etc}\\tunnel.yml"), 'f'),
        (
            ".summrise-release",
            format!("{etc}\\.summrise-release"),
            'f',
        ),
        (
            "boxed-versions.json",
            format!("{etc}\\boxed-versions.json"),
            'f',
        ),
        ("tools\\node", format!("{comp}\\node"), 'd'),
        ("tools\\npm-global", format!("{comp}\\npm-global"), 'd'),
        (
            "tools\\cloudflared.exe",
            format!("{comp}\\cloudflared.exe"),
            'f',
        ),
        ("playwright", format!("{comp}\\playwright"), 'd'),
        (
            "summrise-desktop-electron",
            format!("{comp}\\summrise-desktop-electron"),
            'd',
        ),
        (
            "ensure-desktop.ps1",
            format!("{scr}\\ensure-desktop.ps1"),
            'f',
        ),
        (
            "start-desktop.ps1",
            format!("{scr}\\start-desktop.ps1"),
            'f',
        ),
        (
            "summrise-online-setup.ps1",
            format!("{scr}\\summrise-online-setup.ps1"),
            'f',
        ),
        ("fix-tunnel.ps1", format!("{scr}\\fix-tunnel.ps1"), 'f'),
        (
            "playwright\\playwright-probe.ps1",
            format!("{scr}\\playwright-probe.ps1"),
            'f',
        ),
        (
            "shell-integration",
            format!("{scr}\\shell-integration"),
            'd',
        ),
        ("installer.log", format!("{logs}\\installer.log"), 'f'),
        (
            "install-result.txt",
            format!("{logs}\\install-result.txt"),
            'f',
        ),
        (
            "summrise-update.log",
            format!("{logs}\\summrise-update.log"),
            'f',
        ),
        ("agent.log", format!("{logs}\\agent.log"), 'f'),
        ("startup.log", format!("{logs}\\startup.log"), 'f'),
        ("pwout", format!("{dq}\\pwout"), 'd'),
    ];
    let pending = moves
        .iter()
        .map(|(o, n, _)| format!("((Test-Path '{q}\\{o}') -and (-not (Test-Path '{n}')))"))
        .collect::<Vec<_>>()
        .join(" -or ");
    let mut out = vec![
        format!("$summriseMg = (-not (Test-Path '{etc}\\.layout-v2'))"),
        // A running boxed node locks the playwright tree — stop DIR-local ones first (setup
        // precedent; the updater itself runs from npm-global, which never matches this filter).
        format!("if ($summriseMg) {{ Get-CimInstance Win32_Process | Where-Object {{ $_.Name -eq 'node.exe' -and $_.CommandLine -like '*{q}*playwright*' }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }} }}"),
    ];
    for (o, n, k) in &moves {
        out.push(if *k == 'd' { mvd(o, n) } else { mvf(o, n) });
    }
    out.push(format!(
        "if ($summriseMg -and (-not ({pending}))) {{ try {{ New-Item -ItemType Directory -Force -Path '{etc}' | Out-Null; New-Item -ItemType File -Force -Path '{etc}\\.layout-v2' | Out-Null }} catch {{}} }}"
    ));
    out
}

/// Add/Remove-Programs entry body, shared by `setup` and the update swap.
///
/// Writes DisplayVersion/DisplayName/InstallLocation/Publisher always; writes UninstallString ONLY
/// when absent — an NSIS install owns its `$INSTDIR\uninstall.exe` value and it must never be
/// overwritten. The fallback relaunches `summrise.cmd` ELEVATED: the control panel does not elevate
/// for us, and without `RunAs` the uninstall dies on HKLM/schtasks with access denied. Empty
/// version = no-op. Best-effort throughout: a registry failure must never fail install/update.
pub fn uninstall_reg_body_ps(q: &str, ver: &str) -> Vec<String> {
    if ver.is_empty() {
        return Vec::new();
    }
    vec![
        "try {".to_string(),
        "  $rk = 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\SummriseAgent'"
            .to_string(),
        "  if (-not (Test-Path $rk)) { New-Item -Path $rk -Force | Out-Null }".to_string(),
        format!("  Set-ItemProperty -Path $rk -Name DisplayVersion -Value '{ver}' -ErrorAction Stop"),
        format!("  Set-ItemProperty -Path $rk -Name DisplayName -Value 'Summrise Agent {ver}' -ErrorAction Stop"),
        format!("  Set-ItemProperty -Path $rk -Name InstallLocation -Value '{q}' -ErrorAction Stop"),
        "  Set-ItemProperty -Path $rk -Name Publisher -Value 'Summrise' -ErrorAction Stop"
            .to_string(),
        format!("  $uv = '{q}\\components\\npm-global\\summrise.cmd'"),
        format!("  if (-not (Test-Path $uv)) {{ $uv = '{q}\\tools\\npm-global\\summrise.cmd' }}"),
        "  if ((Test-Path $uv) -and (-not (Get-ItemProperty -Path $rk -Name UninstallString -ErrorAction SilentlyContinue))) { Set-ItemProperty -Path $rk -Name UninstallString -Value ('powershell.exe -NoProfile -ExecutionPolicy Bypass -Command \"Start-Process -FilePath ''' + $uv + ''' -ArgumentList ''uninstall'' -Verb RunAs -Wait\"') -ErrorAction Stop }".to_string(),
        "} catch {}".to_string(),
    ]
}

/// The same body, gated on a PROVABLE swap (`$ok`) — spliced right after the release-marker write.
///
/// Same round-298 discipline as `.summrise-release`: a failed swap must not move the Add/Remove
/// version.
pub fn uninstall_version_ps(q: &str, ver: &str) -> Vec<String> {
    if ver.is_empty() {
        return Vec::new();
    }
    let mut out = vec![format!("if ($ok -and '{ver}') {{")];
    out.extend(uninstall_reg_body_ps(q, ver));
    out.push("}".to_string());
    out
}

/// The two scheduled tasks that ARE the autostart surface.
///
/// `summrise stop` only Ends the running instance and the 5-min watchdog revives it, so stop is NOT
/// opting out of autostart. This flips the task ENABLED flag itself. `/ENABLE|/DISABLE` need no
/// credentials, unlike trigger edits which prompt for the `/ru` password interactively and hang the
/// caller — never add `/RI` `/RU` `/RP` `/TR` here.
pub const BOOT_TASKS: [&str; 2] = ["SummriseAgent", "SummriseDesktop"];

/// `schtasks /Change /TN <task> /ENABLE|/DISABLE`.
///
/// `action` is `"on"` or `"off"`; anything else is treated as `"off"`, which is the TypeScript's
/// ternary. A Rust enum would be the deeper type, but the ORACLE drives this with the string the
/// dispatch actually receives from argv, and a signature that cannot accept the caller's value
/// would move the validation somewhere the case does not look.
pub fn autostart_argv(task: &str, action: &str) -> Vec<String> {
    vec![
        "schtasks".to_string(),
        "/Change".to_string(),
        "/TN".to_string(),
        task.to_string(),
        if action == "on" {
            "/ENABLE"
        } else {
            "/DISABLE"
        }
        .to_string(),
    ]
}

/// The SummrisePlaywright probe launcher (`playwright-probe.ps1`).
///
/// Probe order matches the agent's `preferred_cdp_endpoint()`: 9333 (the Electron DESKTOP embedded
/// view — what the user watches) when up, else private `--headless`. `--output-dir` pins MCP
/// screenshots where the Evidence drawer lists them. The retry loop is the boot race: the task can
/// fire before the desktop's CDP is up, and a single check then forks a private headless chromium
/// nobody sees (device-caught: detached 9229 serving about:blank while the user watched the view).
pub fn playwright_probe_ps() -> Vec<String> {
    [
        "param([string]$node, [string]$cli)",
        "$ErrorActionPreference = 'Continue'",
        "$pwout = Join-Path (Split-Path $node -Parent) '..\\pwout'",
        "if (!(Test-Path $pwout)) { New-Item -ItemType Directory -Path $pwout -Force | Out-Null }",
        "$ep = ''",
        "function Test-Port([int]$port) {",
        "  try {",
        "    $c = New-Object System.Net.Sockets.TcpClient",
        "    $iar = $c.BeginConnect('127.0.0.1', $port, $null, $null)",
        "    if ($iar.AsyncWaitHandle.WaitOne(1500)) { return $c.Connected }",
        "    $c.Close()",
        "  } catch { }",
        "  return $false",
        "}",
        "for ($i = 1; $i -le 12; $i++) { if (Test-Port 9333) { $ep = 'http://127.0.0.1:9333'; break }; Start-Sleep -Seconds 5 }",
        "if ($ep) {",
        "  & $node $cli --port 9229 --host 127.0.0.1 --cdp-endpoint $ep --output-dir $pwout --ignore-https-errors --allowed-hosts '127.0.0.1:9229,localhost:9229'",
        "} else {",
        "  & $node $cli --port 9229 --browser chromium --host 127.0.0.1 --headless --output-dir $pwout --ignore-https-errors --allowed-hosts '127.0.0.1:9229,localhost:9229'",
        "}",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// The CLI's pre-handoff receipt, appended to the swap's OWN log.
///
/// Takes the resolved DATA dir and rebuilds the same `<data>\logs\summrise-update.log` path the swap
/// script appends to — NOT a `..` relative guess, because the data dir can be a registry-remapped
/// location that is not the install dir, and a receipt written to a different file than the swap
/// writes is worse than none: it would look like the swap never started.
///
/// Deliberately NOT the swap's `update start` wording: the value of the receipt is that its
/// presence-without-`update start` proves the CLI ran and the swap did not.
pub fn update_receipt_ps(data_dir_q: &str, from_version: &str, to_version: &str) -> Vec<String> {
    let log = format!("Out-File '{data_dir_q}\\logs\\summrise-update.log' -Append");
    let line = format!(
        "\"[$(Get-Date -Format o)] update requested {from_version} -> {to_version} (CLI reached the device; the swap has not started yet)\""
    );
    vec![format!("{line} | {log}")]
}

/// The two components of the update mutual-exclusion marker's path, under `%ProgramData%`.
///
/// ONE owner, because three readers depend on it agreeing: the mutual-exclusion check in `setup`,
/// the guard in `update`, and [`crate::status::status_report`]'s "is a swap pending" line. A drift
/// means `status` confidently reports "none in flight" while an update is refusing to start because
/// of a marker it cannot see.
const BUSY_REL: [&str; 2] = ["SummriseAgent", "update-busy"];

/// The `%ProgramData%` root, or the same fallback the TypeScript uses.
pub fn program_data() -> String {
    std::env::var("ProgramData")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "C:\\ProgramData".to_string())
}

/// The update mutual-exclusion marker's absolute path.
pub fn update_busy_path() -> String {
    // `path.join(...)` in the TypeScript, and the port JOINs the same way rather than spelling the
    // separator: the two must give the same answer on the platform both run on (Windows, where both
    // are backslashes), and the parity check runs them on Linux, where `path.join` yields forward
    // slashes — a string built by concatenation would differ there for a reason that has nothing to
    // do with the decision.
    let mut p = std::path::PathBuf::from(program_data());
    for part in BUSY_REL {
        p.push(part);
    }
    p.to_string_lossy().to_string()
}

/// The SAME marker as PowerShell text, **DERIVED** from [`update_busy_path`] rather than written out
/// by hand — which is what the TypeScript did, three times, inside the generated swap script. The
/// agent fixed exactly this on its own side, and says why: a drift is invisible until an update
/// actually runs, and then the swap releases a file the agent never created, the marker survives,
/// and every later update is refused for up to an hour.
pub fn busy_marker_ps() -> String {
    format!("(Join-Path $env:ProgramData '{}')", BUSY_REL.join("\\"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    fn lines(v: &[String]) -> String {
        v.join("\n")
    }

    fn assert_match(hay: &str, pat: &str) {
        assert!(
            Regex::new(pat).unwrap().is_match(hay),
            "expected /{pat}/ to match:\n{hay}"
        );
    }

    /// Oracle: cli.test.mjs:261 — "desktopTaskPs / desktopStartPs: asking for the window, and
    /// answering with a FACT".
    ///
    /// The last four assertions of that case read the GENERATED `bin/summrise.js` to pin that the
    /// CLI spawns the start script as a FILE and not as a `-Command` string. That half does not
    /// port as an assertion: it pinned the wiring of a build artifact, and here the wiring is the
    /// call graph — `desktop_start_ps` returns a file body, and no method on
    /// [`crate::host::Host`] takes a command line, so a `-Command` string is unrepresentable
    /// rather than merely absent. The DECISION half below is ported in full.
    #[test]
    fn desktop_task_and_start_ask_for_the_window_and_answer_with_a_fact() {
        let logs = "C:\\ProgramData\\Summrise\\logs";
        let task = lines(&desktop_task_ps("C:\\Program Files\\Summrise", logs));
        assert_match(&task, r"New-ScheduledTaskTrigger -AtLogOn");
        assert_match(&task, r"RepetitionInterval \(New-TimeSpan -Minutes 5\)");
        assert_match(&task, r"Get-Process electron -ErrorAction SilentlyContinue");
        // THE ACTION IS THE LAUNCHER, AND `wscript`/`.vbs` MUST NOT COME BACK: the wrapper needed a
        // VBScript engine — a deprecated Feature-on-Demand that debloated images drop — and on a
        // machine without one the task popped a modal dialog every five minutes while the watchdog
        // never ran once (measured on desktop-14rjcr8, 2026-10-05).
        assert_match(&task, r"New-ScheduledTaskAction -Execute \$ln");
        assert!(
            !Regex::new(r"(?i)wscript").unwrap().is_match(&task),
            "the desktop task must not run wscript.exe"
        );
        let vbs = Regex::new(r"(?i)\.vbs").unwrap();
        for action in task
            .split('\n')
            .filter(|l| l.contains("New-ScheduledTaskAction"))
        {
            assert!(
                !vbs.is_match(action),
                "the action must name the launcher, not a wrapper: {action}"
            );
        }
        assert!(
            !Regex::new(r"Set-Content[^\n]*\.vbs")
                .unwrap()
                .is_match(&task),
            "this builder must not write a .vbs any more"
        );
        assert_match(&task, r"Register-ScheduledTask SummriseDesktop");
        assert_match(&task, r"Start-ScheduledTask -TaskName SummriseDesktop");
        // AND THE TASK MUST NOT BE ABLE TO KILL THE SHELL. `/ExecutionTimeLimit.*0/` reads as a pin
        // and is NOT one: `-Minutes 10` contains a "0" and satisfied it for as long as the value was
        // wrong. The whole argument is asserted.
        assert_match(&task, r"-ExecutionTimeLimit \(New-TimeSpan -Seconds 0\)");
        assert!(
            !Regex::new(r"-ExecutionTimeLimit \(New-TimeSpan -Minutes")
                .unwrap()
                .is_match(&task),
            "no finite execution limit: the shell is meant to outlive the task that launched it"
        );

        let start = lines(&desktop_start_ps("C:\\Summrise"));
        for word in ["already-running", "started", "not-started"] {
            assert!(
                start.contains(word),
                "the command must be able to say {word}"
            );
        }
        // ORDER IS THE PROPERTY: "already running" has to be decided BEFORE anything is started, or
        // asking for a window that is already there would launch a second shell and steal focus.
        assert!(
            start.find("already-running").unwrap() < start.find("Start-ScheduledTask").unwrap(),
            "the already-running check must come first"
        );
        assert!(
            start.contains("-File \"C:\\Summrise\\scripts\\register-desktop-task.ps1\""),
            "the register script must be passed with double quotes"
        );
        assert!(
            !start.contains("-Command"),
            "the start script must not be a -Command string"
        );
        assert!(
            start.contains("if (-not $t)"),
            "re-registration must be conditional on the task being absent"
        );
        assert!(start.contains("Start-ScheduledTask"));
    }

    /// Oracle: cli.test.mjs:406 — "deskShortcutRepairPs: stale-shortcut repair is repair-only +
    /// sunrise-pinned".
    #[test]
    fn desk_shortcut_repair_is_repair_only_and_sunrise_pinned() {
        let body = lines(&desk_shortcut_repair_ps(
            "D:\\Summrise\\scripts",
            "D:\\Summrise\\components\\summrise-desktop-electron",
            "Write-Host",
        ));
        assert_match(&body, r"Summrise\.lnk");
        assert_match(&body, r"summrise-desktop\.exe");
        assert_match(&body, r"summrise-tray\.exe");
        assert_match(&body, r"icon\.ico");
        assert_match(&body, r"start-desktop\.ps1");
        assert_match(&body, r"Write-Host");
        assert!(
            !body.contains("Remove-Item -Recurse"),
            "never deletes directories, files only"
        );
    }

    /// Oracle: cli.test.mjs:436 — "lnkIdentity: the taskbar identity is the same ID the shell sets,
    /// + the sunrise ico".
    #[test]
    fn lnk_identity_is_the_shell_id_and_the_sunrise_ico() {
        let (id, icon) =
            lnk_identity("D:\\Summrise\\components\\summrise-desktop-electron\\icon.ico");
        assert_eq!(id, "online.saisi.summrise.desktop");
        assert_eq!(id, DESKTOP_AUMID, "one string, exported once");
        assert_eq!(
            icon, "D:\\Summrise\\components\\summrise-desktop-electron\\icon.ico,0",
            "RelaunchIconResource is the shell's .ico AND a resource index"
        );
        assert!(!id.contains(' '), "AppUserModelIDs cannot contain spaces");
    }

    /// Oracle: cli.test.mjs:456 — "deskShortcutRepairPs: writes System.AppUserModel.ID +
    /// RelaunchIconResource".
    #[test]
    fn desk_shortcut_repair_writes_the_taskbar_identity() {
        let dir = "D:\\Summrise\\components\\summrise-desktop-electron";
        let body = lines(&desk_shortcut_repair_ps(
            "D:\\Summrise\\scripts",
            dir,
            "Write-Host",
        ));
        assert_match(&body, r"9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3");
        assert_match(&body, r"Get\(lnk, 5, 0\)");
        assert_match(&body, r"Get\(lnk, 3, 0\)");
        assert_match(&body, r"Put\(s, 5, id\); Put\(s, 3, icon\);");
        assert_match(&body, r"Add-Type -TypeDefinition");
        assert_match(&body, "\\$dCs = @'\n");
        assert_match(&body, r"\[SummriseLnk\]::Write\(\$dLnk, \$dId, \$dRes\)");
        assert_match(
            &body,
            r"\('desk: ' \+ \$dLnk \+ ' taskbar identity \[' \+ \[SummriseLnk\]::Read\(\$dLnk\) \+ '\]'\)",
        );
        assert_match(&body, r"\$dSc1\.IconLocation = \$dIco \+ ',0'");
        assert_match(
            &body,
            r"if \(\(\[SummriseLnk\]::Read\(\$dLnk\)\) -ne \(\$dId \+ '\|' \+ \$dRes\)\) \{ \$dNeed = \$true \}",
        );
        assert!(body.contains("if (Test-Path $dLnk) {"));
        assert_match(
            &body,
            r"ProgramData 'Microsoft\\Windows\\Start Menu\\Programs\\Summrise\.lnk'",
        );
        assert_match(
            &body,
            r"if \(-not \(Test-Path \$dSm\)\) \{ \$dSmNeed = \$true \}",
        );
        assert_match(&body, r"\[SummriseLnk\]::Write\(\$dSm, \$dId, \$dRes\)");
        assert_match(&body, r"if \(\$dShell\) \{");
        assert!(
            !Regex::new(r"throw\b").unwrap().is_match(&body) && !body.contains("exit 1"),
            "best-effort: the identity can never fail the caller"
        );
    }

    /// Oracle: cli.test.mjs:529 — "deskShortcutRepairPs: stays under cmd.exe's 8191-char command
    /// line".
    #[test]
    fn desk_shortcut_repair_stays_under_the_cmd_line_cap() {
        let body = desk_shortcut_repair_ps(
            "D:\\Summrise\\scripts",
            "D:\\Summrise\\components\\summrise-desktop-electron",
            "Write-Host",
        )
        .join("; ");
        assert!(
            body.len() < 8191,
            "the joined script is {} chars; cmd.exe caps a command line at 8191",
            body.len()
        );
    }

    /// Oracle: cli.test.mjs:548 — "playwrightProbePs: waits for desktop CDP before forking
    /// headless".
    #[test]
    fn playwright_probe_waits_for_desktop_cdp() {
        let body = lines(&playwright_probe_ps());
        assert_match(&body, r"Test-Port 9333");
        assert_match(&body, r"for \(\$i = 1");
        assert_match(&body, r"Start-Sleep -Seconds 5");
        assert_match(&body, r"--cdp-endpoint \$ep");
        assert_match(&body, r"--headless");
        assert_match(&body, r"127\.0\.0\.1:9229,localhost:9229");
        assert_match(&body, r"--output-dir \$pwout");
        assert!(
            !body.chars().any(|c| c as u32 > 127),
            "ASCII-only (system-locale PS)"
        );
    }

    /// Oracle: cli.test.mjs:621 — "firewallPs: idempotent Summrise-scoped rule for the port".
    #[test]
    fn firewall_ps_is_idempotent_and_scoped() {
        let body = lines(&firewall_ps(7740));
        assert_match(&body, r"LocalPort \$fwPort");
        assert_match(&body, r"\$fwPort = 7740");
        assert_match(&body, r"New-NetFirewallRule");
        assert_match(&body, r"Remove-NetFirewallRule");
        assert_match(&body, r"'Summrise Agent'");
        assert!(
            !body.chars().any(|c| c as u32 > 127),
            "ASCII-only (system-locale PS)"
        );
    }

    /// Oracle: cli.test.mjs:643 — "startDesktopPs: the electron launcher matches the migrated
    /// layout".
    #[test]
    fn start_desktop_matches_the_migrated_layout() {
        let body = lines(&start_desktop_ps(
            "D:\\Summrise\\components\\summrise-desktop-electron",
            "C:\\ProgramData\\Summrise\\logs",
        ));
        assert_match(
            &body,
            r"\$dir = 'D:\\Summrise\\components\\summrise-desktop-electron'",
        );
        assert_match(&body, r"Set-Location \$dir");
        assert_match(&body, r"node_modules\\electron\\dist\\electron\.exe");
        assert!(
            !Regex::new(r"(?i)Start-Process").unwrap().is_match(&body),
            "plain invocation (focus steal is the app's job)"
        );
        assert!(
            !body.chars().any(|c| c as u32 > 127),
            "ASCII-only (system-locale PS)"
        );
    }

    /// Oracle: cli.test.mjs:676 — "the desktop shell says what it did — and says what it CANNOT
    /// see".
    ///
    /// MEASURED on desktop-14rjcr8 (2026-09-28): the shell was killed every ten minutes by the
    /// task's `<ExecutionTimeLimit>PT10M</ExecutionTimeLimit>`, revived five minutes later by the
    /// guarded pulse, and killed again — and `logs\startup.log` held 4,015 lines that answered ZERO
    /// for `desktop`, ZERO for `electron` and ZERO for `ExecutionTimeLimit`. These assertions are
    /// the record that fix exists, and the case it is not allowed to pretend about.
    #[test]
    fn the_desktop_shell_says_what_it_did_and_what_it_cannot_see() {
        let logs = "C:\\ProgramData\\Summrise\\logs";
        let scripts = "D:\\Summrise\\scripts";
        let desk = "D:\\Summrise\\components\\summrise-desktop-electron";
        let ensure = lines(&ensure_desktop_ps(scripts, logs));
        let start = lines(&start_desktop_ps(desk, logs));

        assert!(
            ensure.contains(&format!("$log = '{logs}\\startup.log'")),
            "the watchdog must write to the log directory it was built with"
        );
        assert!(
            start.contains(&format!("$log = '{logs}\\startup.log'")),
            "and the launcher writes to the same file, not a second one nobody knows to open"
        );

        for (what, body) in [("the watchdog", &ensure), ("the launcher", &start)] {
            assert!(
                body.contains("desktop: "),
                "{what}: every line greppable by `desktop`"
            );
            assert!(body.contains("electron"), "{what} names electron");
            assert!(
                !body.chars().any(|c| c as u32 > 127),
                "ASCII-only (system-locale PS)"
            );
        }

        assert!(
            ensure.contains("desktop: no electron shell is running"),
            "the pulse says what it OBSERVED before it acts on it"
        );
        assert_match(&ensure, r"previous shell lived ' \+ \(\[int\]\$now - \$E\)");
        assert!(
            ensure.contains("left NO exit line -- it was KILLED, not exited"),
            "the absent exit code is reported as the kill it is"
        );

        let hb = ensure
            .split('\n')
            .find(|l| l.contains("Add-Content") && l.contains("desktop: electron shell alive"))
            .expect("the healthy case is recorded — one line, written to the log");
        assert!(
            ensure.contains("$h = 'hour='") && hb.contains("$h"),
            "and deduplicated by the hour, against the log itself"
        );
        assert_match(&ensure, r"-Tail 400");

        // THE TWO SCRIPTS MUST AGREE ON THE LINE FORMAT, AND THIS IS THE CHECK FOR IT — so it runs
        // the PULSE'S OWN pattern against the LAUNCHER'S OWN line, both captured from the device.
        let pat_re = Regex::new(r"Select-String -Pattern '([^']+)'").unwrap();
        let caps = pat_re
            .captures(&ensure)
            .expect("the pulse must look for the previous launch in the log tail");
        let pat = caps.get(1).unwrap().as_str();
        let real_launch = "[2026-09-28T18:06:20.6814912+08:00] desktop: electron.exe launching from D:\\Summrise\\components\\summrise-desktop-electron (start=1790589980)";
        let launch_re = Regex::new(&format!("(?i){pat}")).unwrap();
        let m = launch_re.captures(real_launch).unwrap_or_else(|| {
            panic!("the pulse's pattern must match the launcher's real line — got {pat}")
        });
        assert_eq!(
            m.get(1).unwrap().as_str(),
            "1790589980",
            "and it must capture the epoch the exit line is paired by"
        );

        // ...and the other half of the pair: the exit test must match the exit line the launcher
        // writes, or the pulse would call a clean exit a kill. Captured from the device at 18:06:47.
        let exit_re = Regex::new(r"'([^']*electron\\?\.exe exited[^']*)' \+ \$E").unwrap();
        let ecaps = exit_re
            .captures(&ensure)
            .expect("the pulse must look for the exit line that closes a start");
        let exit_pat = format!("(?i){}1790589980 ", ecaps.get(1).unwrap().as_str());
        let real_exit = "[2026-09-28T18:06:47.3402859+08:00] desktop: electron.exe exited (start=1790589980 exit code 1 lived=27s)";
        assert!(
            Regex::new(&exit_pat).unwrap().is_match(real_exit),
            "the exit test must match the real exit line — got {exit_pat}"
        );
        assert!(
            !Regex::new(&exit_pat).unwrap().is_match(
                "[2026-09-28T18:09:00.0000000+08:00] desktop: electron.exe exited (start=1790599999 exit code 1 lived=5s)"
            ),
            "and it must NOT match a different start — the epoch is what pairs them"
        );

        assert!(
            Regex::new(r"CANNOT SEE").unwrap().is_match(&start)
                && Regex::new(r"NO exit code").unwrap().is_match(&start),
            "the launcher states the blind spot in its own file"
        );
        assert!(ensure.contains("leaves no exit code"));
        assert_match(&start, r"\$p\.WaitForExit\(\)\n\s*\$code = \$p\.ExitCode");
        assert!(start.contains("exit code ' + $code"));

        // AND IT MUST NOT GO BACK TO `& electron.exe .` + `$LASTEXITCODE`: judged on the CODE, not
        // on the comments — the comment block above is where the measurement is recorded.
        let code: String = start
            .split('\n')
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains("$LASTEXITCODE"),
            "the call operator cannot report a GUI binary's exit code — do not go back to it"
        );
        assert!(code.contains("UseShellExecute = $false"));
        assert!(start.contains("GUI-subsystem binary"));
    }

    /// Oracle: cli.test.mjs:869 — "uninstallVersionPs: $ok-gated DisplayVersion parity,
    /// UninstallString only when absent".
    #[test]
    fn uninstall_version_ps_is_ok_gated() {
        let body = lines(&uninstall_version_ps(
            "C:\\Program Files\\Summrise",
            "1.2.307",
        ));
        assert_match(&body, r"\$ok -and '1\.2\.307'");
        assert_match(&body, r"DisplayVersion");
        assert_match(&body, r"DisplayName");
        assert_match(&body, r"InstallLocation");
        assert_match(&body, r"catch \{\}");
        assert_match(&body, r"Test-Path \$rk");
        assert!(
            !body.chars().any(|c| c as u32 > 127),
            "ASCII-only (system-locale PS)"
        );
    }

    /// Oracle: cli.test.mjs:899 — "uninstallRegBodyPs: shared setup/swap body, conditional elevated
    /// uninstall".
    #[test]
    fn uninstall_reg_body_is_shared_and_conditional() {
        let body = lines(&uninstall_reg_body_ps("D:\\Summrise", "1.2.307"));
        assert!(!body.contains("$ok"), "ungated body (setup has no $ok)");
        assert_match(&body, r"DisplayVersion");
        assert_match(&body, r"Get-ItemProperty -Path \$rk -Name UninstallString");
        let idx_read = body
            .find("Get-ItemProperty -Path $rk -Name UninstallString")
            .unwrap();
        let idx_write = body
            .find("-Name UninstallString -Value")
            .expect("the UninstallString write must exist");
        assert!(idx_write > idx_read, "write is guarded by the absence read");
        let idx_comp = body.find("components\\npm-global\\summrise.cmd").unwrap();
        let idx_tools = body.find("tools\\npm-global\\summrise.cmd").unwrap();
        assert!(
            idx_tools > idx_comp,
            "components first, tools legacy fallback"
        );
        assert_match(&body, r"-Verb RunAs");
        assert_match(&body, r"-Wait");
        assert_eq!(
            uninstall_reg_body_ps("D:\\Summrise", ""),
            Vec::<String>::new(),
            "empty ver writes nothing"
        );
        assert!(
            !body.chars().any(|c| c as u32 > 127),
            "ASCII-only (system-locale PS)"
        );
    }

    /// Oracle: cli.test.mjs:944 — "autostartArgv: ENABLE/DISABLE both boot tasks, no
    /// credential-prompt flags".
    #[test]
    fn autostart_argv_covers_both_tasks_without_credential_flags() {
        let mut sorted = BOOT_TASKS.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, vec!["SummriseAgent", "SummriseDesktop"]);
        for t in BOOT_TASKS {
            assert_eq!(
                autostart_argv(t, "off"),
                vec!["schtasks", "/Change", "/TN", t, "/DISABLE"]
            );
            assert_eq!(
                autostart_argv(t, "on"),
                vec!["schtasks", "/Change", "/TN", t, "/ENABLE"]
            );
        }
        let all = BOOT_TASKS
            .iter()
            .flat_map(|t| {
                [
                    autostart_argv(t, "on").join(" "),
                    autostart_argv(t, "off").join(" "),
                ]
            })
            .collect::<Vec<_>>()
            .join("\n");
        for banned in ["/RU", "/RP", "/RI", "/TR"] {
            assert!(
                !all.contains(banned),
                "{banned} must never appear (it prompts for the account password and hangs)"
            );
        }
    }

    /// Oracle: cli.test.mjs:979 — "bootTaskPs: explicit config argument, hardened SYSTEM task,
    /// optional kick".
    #[test]
    fn boot_task_ps_uses_the_config_argument_and_a_hardened_principal() {
        let reg = lines(&boot_task_ps(
            "C:\\V\\summrise-agent.exe",
            "C:\\V\\etc\\config.yaml",
            false,
        ));
        assert_match(
            &reg,
            r#"-Argument \('"'\s*\+\s*'C:\\V\\etc\\config\.yaml'\s*\+\s*'"'\)"#,
        );
        assert!(
            !reg.contains("summrise-agent.exe' + '\""),
            "the exe path must never be the argument"
        );
        assert_match(&reg, r"-UserId SYSTEM");
        assert_match(&reg, r"-ExecutionTimeLimit \(New-TimeSpan -Seconds 0\)");
        assert_match(&reg, r"Register-ScheduledTask SummriseAgent");
        assert!(
            !reg.contains("Start-ScheduledTask"),
            "no kick without start=true"
        );
        let kick = lines(&boot_task_ps(
            "C:\\V\\summrise-agent.exe",
            "C:\\V\\etc\\config.yaml",
            true,
        ));
        assert_match(&kick, r"Start-ScheduledTask SummriseAgent");
        for banned in ["/RU", "/RP", "/RI", "/TR"] {
            assert!(
                !reg.contains(&format!(" {banned}")),
                "{banned} must never appear"
            );
        }
        assert!(
            !reg.chars().any(|c| c as u32 > 127),
            "ASCII-only (system-locale PS)"
        );
    }

    /// Oracle: cli.test.mjs:1034 — "migrateLayoutPs: mirrors paths.rs pairs, never clobbers, kills
    /// boxed node first, marker-gated".
    #[test]
    fn migrate_layout_ps_mirrors_the_paths_pairs() {
        let body = lines(&migrate_layout_ps(
            "D:\\Summrise",
            "C:\\ProgramData\\Summrise",
        ));
        for pair in [
            [
                "D:\\Summrise\\config.yaml",
                "D:\\Summrise\\etc\\config.yaml",
            ],
            [
                "D:\\Summrise\\summrise-agent.hostname",
                "D:\\Summrise\\etc\\summrise-agent.hostname",
            ],
            [
                "D:\\Summrise\\.summrise-release",
                "D:\\Summrise\\etc\\.summrise-release",
            ],
            [
                "D:\\Summrise\\tools\\node",
                "D:\\Summrise\\components\\node",
            ],
            [
                "D:\\Summrise\\playwright",
                "D:\\Summrise\\components\\playwright",
            ],
            [
                "D:\\Summrise\\summrise-desktop-electron",
                "D:\\Summrise\\components\\summrise-desktop-electron",
            ],
            [
                "D:\\Summrise\\start-desktop.ps1",
                "D:\\Summrise\\scripts\\start-desktop.ps1",
            ],
            [
                "D:\\Summrise\\installer.log",
                "C:\\ProgramData\\Summrise\\logs\\installer.log",
            ],
            ["D:\\Summrise\\pwout", "C:\\ProgramData\\Summrise\\pwout"],
        ] {
            assert!(
                body.contains(pair[0]) && body.contains(pair[1]),
                "migration covers {} -> {}",
                pair[0],
                pair[1]
            );
        }
        assert_match(&body, r"-not \(Test-Path");
        assert_match(&body, r"CommandLine -like '\*.*playwright\*");
        assert!(
            body.contains("$summriseMg = (-not (Test-Path 'D:\\Summrise\\etc\\.layout-v2'))"),
            "marker short-circuits re-runs (single-line guard)"
        );
        // 22 move lines + the node-kill line + the marker write all carry the guard. **THE NUMBER
        // FELL FROM 26 TO 24 WHEN THE TWO `.vbs` WRAPPERS LEFT THE MAP**: both scheduled tasks run
        // `scripts\summrise-launch.exe`, so setup/update delete either copy instead of carrying it
        // forward (paths.rs's plan asserts the same absence). A count that silently kept its old
        // value here would be a gate measuring a migration that no longer happens.
        assert_eq!(
            body.matches("if ($summriseMg").count(),
            24,
            "every statement carries the marker guard"
        );
        assert!(
            body.contains("if ($summriseMg -and (-not (")
                && body.contains("(Test-Path 'D:\\Summrise\\config.yaml')"),
            "marker write gated on pending pairs"
        );
        assert!(
            body.contains("New-Item -ItemType File -Force -Path 'D:\\Summrise\\etc\\.layout-v2'")
        );
        assert!(
            !body.chars().any(|c| c as u32 > 127),
            "ASCII-only (system-locale PS)"
        );
    }

    /// Oracle: cli.test.mjs:1407 — "updateReceiptPs: appends the INTENT before the handoff, to the
    /// same log the swap writes".
    #[test]
    fn update_receipt_appends_the_intent() {
        let body = lines(&update_receipt_ps("D:\\Summrise", "1.2.321", "1.2.322"));
        assert_match(&body, r"summrise-update\.log");
        assert_match(&body, r"1\.2\.321");
        assert_match(&body, r"1\.2\.322");
        assert_match(&body, r"-Append");
        assert_match(&body, r"requested");
        assert!(
            !Regex::new(r"update start").unwrap().is_match(&body),
            "must not reuse the swap's 'update start' marker"
        );
    }

    /// Oracle: cli.test.mjs:1442 — "updateReceiptPs: the sink is byte-identical to the swap script's
    /// log sink".
    #[test]
    fn update_receipt_sink_is_byte_identical_to_the_swap_sink() {
        for data_dir in [
            "D:\\Summrise",
            "D:\\ProgramData\\Summrise",
            "C:\\Program Files\\Summrise",
        ] {
            // Mirrors the `const log = ...` line in update(), verbatim.
            let swap_log = format!(
                "Out-File '{}\\logs\\summrise-update.log' -Append",
                data_dir.replace('\'', "''")
            );
            let receipt = &update_receipt_ps(&crate::ps::psq(data_dir), "1.0.0", "1.0.1")[0];
            let receipt_log = &receipt[receipt.find("Out-File").unwrap()..];
            assert_eq!(receipt_log, swap_log, "sinks must agree for {data_dir}");
        }
        let quoted = &update_receipt_ps(&crate::ps::psq("D:\\it's\\Summrise"), "1.0.0", "1.0.1")[0];
        assert_match(quoted, r"it''s");
    }

    /// Oracle: cli.test.mjs:3117 — "busyMarkerPs names the same file as updateBusyPath".
    ///
    /// The agent has had this contract on its own side since it fixed the SAME defect there. The CLI
    /// claimed ONE owner for this path while its generated swap script carried three hand-written
    /// copies of it, and a drift is invisible until an update runs: the swap releases a file the
    /// agent never created, the marker survives, and every later update is refused for up to an hour.
    #[test]
    fn busy_marker_ps_names_the_same_file_as_update_busy_path() {
        let js = update_busy_path().replace('\\', "/");
        let ps = busy_marker_ps();
        assert_match(&ps, r"^\(Join-Path \$env:ProgramData '");
        let inner = &ps[ps.find('\'').unwrap() + 1..ps.rfind('\'').unwrap()];
        let rel = inner.replace('\\', "/");
        assert!(!rel.is_empty(), "the PS form must name a relative path");
        assert!(js.ends_with(&rel), "{ps} must name the same file as {js}");
    }
}
