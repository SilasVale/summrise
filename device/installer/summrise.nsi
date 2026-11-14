; Summrise Device Runtime — the Windows installer.
;
; Built by CI on windows-latest (NSIS ships there) and by hand with `makensis` elsewhere:
;   makensis -DVERSION=0.1.1 -DSOURCE=<dir with the binaries> -DPORT=18080 installer/summrise.nsi
;
; The wizard is MUI2, the same page set Tauri's Windows bundles use. The installer does NOT own the
; runtime's install logic: it copies the files and calls `summrise-launch --install-service`, which
; seeds the baseline from the signed sidecar, mints the install token, hardens the data directory
; and registers the service. One implementation, so an installer and a hand install cannot drift.
;
; **WHAT THE EXIT CODES MEAN.**
;
;   0  the files are installed and ${SERVICE} is RUNNING (the final check before SectionEnd);
;   1  the install did not finish; the log in %TEMP%\summrise-setup.log and, in silent mode, the
;      exit code carry the failure;
;   3  the setup found itself running under the installed supervisor (`SUMMRISE_SUPERVISED=1`, the
;      mark the launcher puts on the app, and `runs.exec` commands inherit) and handed the same
;      silent install to the one-shot ${RELAY_TASK} task instead of stopping the service from a
;      process whose stop would kill it. The task's run appends to the same log, and the log's
;      final `service: ${SERVICE} is RUNNING at the end of the install; exit 0` (or its exit-1
;      counterpart) is the install's real verdict. See RelaySupervisedSetup.

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
; `HWND_BROADCAST` and `WM_SETTINGCHANGE` name the environment-change broadcast below.
!include "WinMessages.nsh"
; `/CONSOLE=` and `/CODE=` are read out of the command line: the optional auto-enrolment below.
!include "FileFunc.nsh"
; `StrStr` is how the upgrade proves the restarted app's `/api/health` reports its version, and how
; the install proves the delimited PATH entry absent; `UnStrRep` is how the uninstaller takes that
; entry back out.
!include "StrFunc.nsh"
${Using:StrFunc} StrStr
${Using:StrFunc} UnStrRep

!ifndef VERSION
  !error "VERSION is required (-DVERSION=0.1.1)"
!endif
!ifndef SOURCE
  !error "SOURCE is required (-DSOURCE=<dir with summrise-launch.exe and the signed sidecar>)"
!endif
!ifndef PORT
  !define PORT "18080"
!endif
!ifndef SERVICE
  !define SERVICE "SummriseLaunch"
!endif
; The one-shot task a supervised silent setup hands itself to. It runs the same setup with
; `/S /RELAYED`; the Task Scheduler starts it outside the supervisor's job, so the service stop it
; performs cannot kill it.
!ifndef RELAY_TASK
  !define RELAY_TASK "SummriseSetupRelay"
!endif
!ifndef CHANNEL
  !define CHANNEL "stable"
!endif

!define PRODUCT "Summrise Device Runtime"
!define COMPANY "Summrise"

; Windows' file properties and the wizard's icon come from the installer, not from the shell:
; a signed release is the artifact an operator double-clicks, and "unknown publisher / no icon"
; was the first impression it gave.
!define MUI_ICON "assets\summrise.ico"
!define MUI_UNICON "assets\summrise.ico"
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "CompanyName" "${COMPANY}"
VIAddVersionKey "FileDescription" "Summrise device installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Summrise"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Summrise"
; The machine environment block: `Path` here is what a prompt opened after the setup inherits.
!define MACHINE_ENV "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"
; With `SetShellVarContext all`, `$APPDATA` is the machine's ProgramData. NSIS has no
; `$COMMONAPPDATA` constant: writing one compiles to a literal path and puts the data
; directory in the wrong place.
!define DATA_DIR "$APPDATA\Summrise"

; `WM_SETTINGCHANGE` with `lParam = "Environment"` is how a running Explorer tells new processes
; that the machine environment changed; without it a prompt opened after the setup keeps the old
; Path until a sign-out. Best effort: the send carries a 5 s timeout and its result is not checked,
; because a hung window must not turn a successful install or uninstall into a failure.
!macro BroadcastEnvironmentChange
  System::Call 'user32::SendMessageTimeoutW(i ${HWND_BROADCAST}, i ${WM_SETTINGCHANGE}, i 0, w "Environment", i 0, i 5000, i 0)'
!macroend

Name "${PRODUCT} ${VERSION}"
OutFile "SummriseSetup-${VERSION}-${CHANNEL}.exe"
; The compile-time default is named so `.onInit` can tell "the wizard's default" from "the operator
; chose /D=": the registry's existing root replaces the former, never the latter.
!define DEFAULT_INSTDIR "$PROGRAMFILES64\Summrise"
InstallDir "${DEFAULT_INSTDIR}"
InstallDirRegKey HKLM "${UNINST_KEY}" "InstallLocation"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
BrandingText "${COMPANY}"

; The two optional auto-enrolment values, read from `/CONSOLE=` and `/CODE=` in the install section.
Var ConsoleUrl
Var EnrollCode

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
; A setup that found an install may not move it: the chosen root is checked on every leave of the
; directory page, before anything is written or stopped, and again at the section's first line for
; `/S` and `/D=` (which never draw a page).
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE CheckInstallRoot
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; The existing install's root, from the uninstall entry the runtime writes. A 32-bit NSIS reads the
; 32-bit view by default while the entry lives in the 64-bit one, so `SetRegView 64` before the read
; is not optional: without it this finds nothing and the guard passes on a machine that has an
; install.
;
; `$R9` is the existing root, or "" when there is none.
Function CheckInstallRoot
  SetRegView 64
  ReadRegStr $R9 HKLM "${UNINST_KEY}" "InstallLocation"
  ${If} $R9 == ""
    Return
  ${EndIf}
  ; Windows paths are case-insensitive (`StrCmp` is), and one spelling may carry a trailing
  ; backslash while the other does not. Each comparison that matches jumps past the refusal.
  StrCmp "$INSTDIR" "$R9" equal
  StrCmp "$INSTDIR\" "$R9" equal
  StrCmp "$INSTDIR" "$R9\" equal
  StrCmp "$INSTDIR\" "$R9\" equal
  ; **A DIFFERENT ROOT IS REFUSED BEFORE THE FIRST WRITE OR STOP.** The default is the existing
  ; root; a move is not supported. A silent setup has nobody to click OK, so it reports through the
  ; exit code instead of a box that would hang it.
  SetErrorLevel 1
  ${IfNot} ${Silent}
    MessageBox MB_ICONSTOP "The Summrise runtime is already installed in:$\r$\n$\r$\n$R9$\r$\n$\r$\nThis setup cannot install to a different directory:$\r$\n$\r$\n$INSTDIR$\r$\n$\r$\nAn upgrade installs into the existing directory; it is not moved. Run the setup again with the existing directory (the default)."
  ${EndIf}
  Quit
  equal:
FunctionEnd

; **THE INIT PATH TOUCHES NOTHING THAT IS RUNNING.** It used to stop the service and kill the
; device here, before the directory page and before the setup had checked anything. A second-root
; attempt on a working machine therefore stopped the working service first, and a setup that then
; failed left it stopped. The only thing this function may do to an existing install is read its
; root (and seed the default from it): the stop belongs in the section, after the new generation is
; staged and proven.
Function .onInit
  SetRegView 64
  ; **THE RELAY'S OWN RUN TAKES ITS TRIGGER BACK FIRST.** This process IS the one-shot task; an
  ; onstart task left registered would run the install again at every boot. It is deleted before
  ; the root check, because a check that refuses this run must not leave the task behind either.
  ; A running task can be deleted (measured 2026-11-07: the instance keeps running).
  ${StrStr} $R3 "$CMDLINE" "/RELAYED"
  ${If} $R3 != ""
    nsExec::Exec 'schtasks /delete /tn ${RELAY_TASK} /f'
  ${EndIf}
  ReadRegStr $R9 HKLM "${UNINST_KEY}" "InstallLocation"
  ${If} $R9 != ""
    ; `InstallDirRegKey` reads the 32-bit view on a 32-bit NSIS and so does not seed a custom root
    ; on a 64-bit machine. Seed it here — but only while the wizard still holds the compile-time
    ; default: an explicit `/D=` is the operator's choice and the check below refuses it, rather
    ; than silently replacing it.
    ${If} $INSTDIR == "${DEFAULT_INSTDIR}"
      StrCpy $INSTDIR "$R9"
    ${EndIf}
  ${EndIf}
  Call CheckInstallRoot
  ; DIAGNOSTIC: a setup that exits 0 without touching anything is the hardest failure to see from
  ; the outside, so the init path leaves a trace next to the install log.
  ;
  ; **THE RELAYED RUN APPENDS TO THE FIRST RUN'S LOG.** One install, two processes: the supervised
  ; setup that handed itself off (exit 3) writes the header, and the task's run writes the stop, the
  ; copy, the start and the verdict. Truncating here would erase the hand-off that explains why the
  ; first process returned before anything was installed.
  ${If} $R3 == ""
    FileOpen $6 "$TEMP\summrise-setup.log" w
    FileWrite $6 "onInit start (${VERSION}, port ${PORT}) install=$INSTDIR existing=$R9 relayed=0$\r$\n"
  ${Else}
    ; The setup that handed itself off is still writing its last line and its caller is still
    ; collecting the answer: let it exit before this run appends to the same log and before
    ; anything here can stop the service. The hand-off's two log lines follow its `schtasks /run`,
    ; so this is milliseconds of real work behind a deliberately generous wait.
    Sleep 3000
    FileOpen $6 "$TEMP\summrise-setup.log" a
    FileWrite $6 "onInit start (${VERSION}, port ${PORT}) install=$INSTDIR existing=$R9 relayed=1$\r$\n"
  ${EndIf}
  FileClose $6
FunctionEnd

; Stop what is running so its images can be replaced. **CALLED ONLY AFTER THE NEW GENERATION IS
; STAGED AND PROVEN** (or on a first install, where there is nothing to stop): an upgrade that
; cannot prove its candidate must leave the old service serving.
;
; `$R6` is "0" when a registered service was asked to stop and did not — the caller aborts before
; replacing any image — else "1". `$6` must be the open setup log.
Function StopRuntime
  StrCpy $R6 "1"
  nsExec::ExecToStack 'sc.exe query ${SERVICE}'
  Pop $0
  Pop $1
  ${If} $0 == "0"
    FileWrite $6 "stop: ${SERVICE} is registered; stopping it$\r$\n"
    nsExec::Exec 'sc.exe stop ${SERVICE}'
    Pop $0
    FileWrite $6 "stop: sc stop: exit $0$\r$\n"
    ; THE SERVICE STOPS ASYNCHRONOUSLY, AND THE FILES CANNOT BE REPLACED UNTIL IT HAS. `sc stop`
    ; returns when the control is accepted, not when the process is gone; copying over a running
    ; image fails with a sharing violation. Wait for the launcher process to disappear, bounded.
    StrCpy $2 0
    StrCpy $7 "0"
    ${Do}
      IntOp $2 $2 + 1
      nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq summrise-launch.exe" | findstr /I summrise-launch'
      Pop $0
      Pop $1
      ${If} $0 != "0"
        StrCpy $7 "1"
      ${ElseIf} $2 >= 45
        StrCpy $7 "1"
      ${Else}
        Sleep 1000
      ${EndIf}
    ${LoopUntil} $7 == "1"
    FileWrite $6 "stop: launcher waited out after $2 attempt(s)$\r$\n"
    ; The control was accepted but the SCM may still be stopping it (or it may have failed to
    ; stop). Proving STOPPED is what makes the copy below safe.
    StrCpy $2 0
    StrCpy $3 "0"
    ${Do}
      IntOp $2 $2 + 1
      nsExec::ExecToStack 'cmd /c sc.exe query ${SERVICE} | findstr /I "STOPPED"'
      Pop $0
      Pop $1
      ${If} $0 == "0"
        StrCpy $3 "1"
        ${ExitDo}
      ${EndIf}
      ${If} $2 >= 30
        ${ExitDo}
      ${EndIf}
      Sleep 1000
    ${Loop}
    FileWrite $6 "stop: service stopped after $2 attempt(s)$\r$\n"
    ${If} $3 != "1"
      StrCpy $R6 "0"
    ${EndIf}
  ${Else}
    FileWrite $6 "stop: ${SERVICE} is not registered; nothing to stop$\r$\n"
  ${EndIf}
  ; THE APP IS A CHILD OF THE LAUNCHER, BUT ITS IMAGE IS THE ONE BEING REPLACED. A supervisor that
  ; exited does not guarantee the app did, and a running `summrise-device.exe` holds its image open.
  nsExec::Exec 'taskkill /F /IM summrise-device.exe'
  Pop $0
  StrCpy $2 0
  StrCpy $7 "0"
  ${Do}
    IntOp $2 $2 + 1
    nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq summrise-device.exe" | findstr /I summrise-device'
    Pop $0
    Pop $1
    ${If} $0 != "0"
      StrCpy $7 "1"
    ${ElseIf} $2 >= 15
      StrCpy $7 "1"
    ${Else}
      Sleep 1000
    ${EndIf}
  ${LoopUntil} $7 == "1"
  FileWrite $6 "stop: app waited out after $2 attempt(s)$\r$\n"
  ; The tray is a per-user process, not the service: close it before its image is replaced, and the
  ; login Run entry (plus the start-shell task at the end of the section) starts the new one.
  nsExec::Exec 'taskkill /F /IM summrise-shell.exe'
  Pop $0
FunctionEnd

; **THE SERVICE IS THE INSTALL'S VERDICT, NOT ITS SIDE EFFECT.** A setup that ends with
; `${SERVICE}` stopped has installed files and no product: `/S` has no window in which to notice,
; and its caller sees only the exit code. Start it when it is not running, wait for the SCM's
; observable state (bounded), and leave the answer in `$R7`:
;
;   $R7 = "1"  the service reported RUNNING before the wait ran out
;   $R7 = "0"  it did not; `$R2` is the number of start attempts and `$1` the last output
;
; Seeking RUNNING in `sc query`'s output rather than calling `sc start` and trusting its exit code
; is deliberate: `sc start` answers 1056 ("already running") for a service that is not serving, and
; a start it accepted can still fail in the SCM. `$6` must be the open setup log. This clobbers
; `$0`, `$1`, `$2`, `$R2` and `$R7`; a caller that needs `sc`'s answer must save it first.
Function EnsureServiceRunning
  StrCpy $R7 "0"
  StrCpy $R2 0
  ${Do}
    IntOp $R2 $R2 + 1
    nsExec::ExecToStack 'cmd /c sc.exe query ${SERVICE} | findstr /I "RUNNING"'
    Pop $0
    Pop $1
    ${If} $0 == "0"
      StrCpy $R7 "1"
      ${ExitDo}
    ${EndIf}
    ${If} $R2 >= 15
      ${ExitDo}
    ${EndIf}
    nsExec::ExecToStack 'sc.exe start ${SERVICE}'
    Pop $0
    Pop $1
    FileWrite $6 "service: start attempt $R2: exit $0 $1$\r$\n"
    Sleep 1000
  ${Loop}
FunctionEnd

; **A SETUP THAT RUNS UNDER THE SUPERVISOR MAY NOT STOP IT ITSELF.** The supervisor assigns the app
; to a kill-on-close job, so every process the app starts is in that job unless the job carries
; `SILENT_BREAKAWAY_OK`; `sc stop` makes the supervisor exit, closing the job, which terminates the
; app — and, on a launcher that predates the breakaway flag, the setup waiting in the stop loop with
; it. The job was created by the launcher that is *already installed*: an upgrade cannot be
; protected by flags in the launcher the upgrade is installing. Two things follow, and the
; measurement is unambiguous (DESKTOP-9VH92BJ, 2026-11-07, `SummriseSetup-0.3.288.exe /S` through
; `runs.exec`): the setup's log stopped at `stop: sc stop: exit 0` and the next line ("launcher
; waited out") was never written, because the job killed it there; the console's `runs.exec` call
; answered `{"error":"device disconnected"}` because the same stop kills the app that hosts it; and
; the service stayed stopped until an operator started it. A setup that does not stop the service
; cannot be killed by that stop, so a supervised silent upgrade hands the same silent install to a
; one-shot SYSTEM task — the Task Scheduler starts it outside the job — and returns exit 3.
;
; `$R8` must be the install mode (`"0"` is a first install: there is no service to stop and nothing
; to hand off) and `$6` the open setup log. The verdict is left in `$R3`:
;
;   $R3 = "1"   a task was created and started; the caller must exit 3
;   $R3 = "-1"  the task could not be created or started; nothing has been stopped or written
;   $R3 = "0"   no relay is needed; continue in this process
;
; This clobbers `$0`, `$1`, `$R1` and `$R3`.
Function RelaySupervisedSetup
  StrCpy $R3 "0"
  ; Silent is the case this exists for: `runs.exec` has no window to notice a stopped service, and
  ; the caller sees only the exit code. An interactive wizard started from the machine's own console
  ; is not under the supervisor at all; one started inside a device session keeps its old behaviour
  ; rather than vanishing after the welcome page.
  ${IfNot} ${Silent}
    Return
  ${EndIf}
  ; The launcher sets this for the app it supervises; a command the app runs inherits the app's
  ; environment, so a `runs.exec` PowerShell and the setup it starts both carry it (measured
  ; 2026-11-07: `SUPERVISED=[1]` in a runs.exec child).
  ReadEnvStr $0 "SUMMRISE_SUPERVISED"
  ${If} $0 != "1"
    Return
  ${EndIf}
  ; The relayed run is outside the job by construction and must not relay again.
  ${StrStr} $0 "$CMDLINE" "/RELAYED"
  ${If} $0 != ""
    Return
  ${EndIf}
  ; A first install has no SummriseLaunch to stop (StopRuntime skips an unregistered service), so
  ; the stop cannot kill anything and the install continues in this process.
  ${If} $R8 == "0"
    Return
  ${EndIf}
  ; The relay exists for the stop; a service that is not RUNNING will not be stopped by it. This is
  ; the same test StopRuntime makes with `sc query`, so the two agree about what there is to stop.
  nsExec::ExecToStack 'cmd /c sc.exe query ${SERVICE} | findstr /I "RUNNING"'
  Pop $0
  Pop $1
  ${If} $0 != "0"
    FileWrite $6 "relay: ${SERVICE} is not RUNNING; there is no stop to hand off; continuing in this process$\r$\n"
    Return
  ${EndIf}
  FileWrite $6 "relay: this setup is inside the supervisor's job (SUMMRISE_SUPERVISED=1); handing the stop-based install to ${RELAY_TASK}$\r$\n"
  ; The same command line the operator gave: silent, the marker, and the enrolment pair when both
  ; are present (the section makes the same check). There is no `/D=`: the install root comes from
  ; the uninstall entry's `InstallLocation`, which CheckInstallRoot has already proven equal.
  StrCpy $R1 " /S /RELAYED"
  ${If} $ConsoleUrl != ""
  ${AndIf} $EnrollCode != ""
    StrCpy $R1 "$R1 /CONSOLE=$ConsoleUrl /CODE=$EnrollCode"
  ${EndIf}
  ; **THE `/tr` VALUE NEEDS A LITERAL `\"` AROUND THE EXECUTABLE, AND NSIS NEEDS `\$\"` TO EMIT
  ; IT.** schtasks splits the task's command at the first space unless the executable part is
  ; quoted *inside* the `/tr` value; `/tr "$EXEDIR\$EXEFILE"` with a path that has a space is
  ; stored as `Command=C:\Program` + `Arguments=Files\...` and fails to start with 0x80070002
  ; (measured 2026-11-07). The value schtasks must receive is `"<path>" <args>`, so this command
  ; line must carry `\"<path>\" <args>`: NSIS turns `$\"` into one quote and `\$\"` into backslash
  ; + quote. The 0.3.289 script wrote `"$\"...$\""`, which expands to `""<path>""`; schtasks
  ; rejects that with "invalid argument/option" and no task is created (measured on the 84T's
  ; relayed `/S`: `repair task: exit -2147467259`, `shell task create: -2147467259`). This line
  ; survived only because the setup's `$EXEDIR` has no space, which made the extra quotes harmless.
  nsExec::ExecToStack 'schtasks /create /tn ${RELAY_TASK} /tr "\$\"$EXEDIR\$EXEFILE\$\"$R1" /sc onstart /ru SYSTEM /rl HIGHEST /f'
  Pop $0
  Pop $1
  FileWrite $6 "relay: schtasks create: exit $0 $1$\r$\n"
  ${If} $0 != "0"
    StrCpy $R3 "-1"
    Return
  ${EndIf}
  nsExec::ExecToStack 'schtasks /run /tn ${RELAY_TASK}'
  Pop $0
  Pop $1
  FileWrite $6 "relay: schtasks run: exit $0 $1$\r$\n"
  ${If} $0 != "0"
    ; A created but unstarted task would complete the install at the next boot, after this setup
    ; reported failure; take it back so the report is true.
    nsExec::Exec 'schtasks /delete /tn ${RELAY_TASK} /f'
    StrCpy $R3 "-1"
    Return
  ${EndIf}
  StrCpy $R3 "1"
  FileWrite $6 "relay: ${RELAY_TASK} is running; this setup exits 3 before the first write or stop. Its run appends to this log: the last 'service:' line it writes is the install's verdict$\r$\n"
FunctionEnd

Section "Summrise runtime" SecRuntime
  ; Optional auto-enrolment: `/CONSOLE=<console-url> /CODE=<one-time-code>` makes the installer register
  ; this machine with the console, so "install" is the whole onboarding. Both are absent by default and
  ; a failure to enrol never fails the install — a device that is installed but not yet enrolled is a
  ; useful device; an install that aborts because a code expired is not.
  ${GetOptions} $CMDLINE "/CONSOLE=" $ConsoleUrl
  ${GetOptions} $CMDLINE "/CODE=" $EnrollCode
  ${If} $ConsoleUrl != ""
  ${AndIf} $EnrollCode != ""
    StrCpy $9 ""
    StrCpy $8 $ConsoleUrl 7
    ${If} $8 == "http://"
      StrCpy $9 "--allow-plaintext"
    ${EndIf}
  ${EndIf}
  SectionIn RO
  SetShellVarContext all
  ; NSIS is a 32-bit process: without this, HKLM writes land under WOW6432Node and the
  ; "Apps & features" entry is the 32-bit one, not the machine's.
  SetRegView 64
  ; The page's leave check covers the wizard; this covers `/S` and `/D=`, and it is the first thing
  ; the section does, before the first write or stop.
  Call CheckInstallRoot
  FileOpen $6 "$TEMP\summrise-setup.log" a
  FileWrite $6 "section: install to $INSTDIR$\r$\n"

  ; **WHAT THIS SETUP FOUND.** The uninstall entry is the product's "an install is here" fact; the
  ; tree is checked too because a registry entry whose files are gone is a first install, not an
  ; upgrade. Mode 0 = first install, 1 = pre-versioned install, 2 = versioned install.
  ReadRegStr $R9 HKLM "${UNINST_KEY}" "InstallLocation"
  StrCpy $R8 "0"
  ${If} $R9 != ""
  ${AndIf} ${FileExists} "$INSTDIR\summrise-launch.exe"
    StrCpy $R8 "1"
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\current.txt"
    StrCpy $R8 "2"
  ${EndIf}
  FileWrite $6 "found: mode $R8 (0 first install, 1 pre-versioned install, 2 versioned install)$\r$\n"

  ; **A PERIODIC REPAIR, because losing the service is a state nothing inside the machine can leave:
  ; the process that would rebuild it IS the service.** Two incidents made this: a failed install
  ; removed it once and left the device unreachable until someone walked to it (DESKTOP-CPH4KB2,
  ; 2026-10-28), and on 2026-10-29 the service sat stopped for four hours while the console answered
  ; `device reconnecting` — Windows restarts a service that FAILS three times and then stops trying,
  ; and a service that stopped without failing is not restarted at all. The task runs every five
  ; minutes: it starts the service when it is registered but not running, and re-registers it when
  ; it is gone.
  ;
  ; **IT IS REGISTERED BEFORE ANYTHING THAT CAN END THIS SETUP, AND THAT ORDER IS THE FIX.** The
  ; registration used to sit after the copy and the uninstall entry. An existing install whose setup
  ; ended earlier therefore kept its previous registration: the 9VH's 0.3.288 `/S` log ends at
  ; `stop: sc stop: exit 0` (the supervised stop killed the setup before its next line), and the
  ; 0.3.279 registration — `Command=C:\Program` + `Arguments=Files\Summrise\repair.cmd`, last
  ; result 0x80070002 — survived two releases on DESKTOP-9VH92BJ. **A REPAIR TASK THAT CANNOT START
  ; IS WORSE THAN NONE**: it is the safety net the five-minute repair exists for, so it has to be
  ; right the moment the stop begins, not after the last step that might not run. The script and
  ; the task now precede the hand-off, the stop and every abort those can take; a relayed run
  ; registers it twice, which `schtasks /f` makes harmless.
  ;
  ; THE COST IS STATED WHERE IT IS FELT: a deliberate stop is undone within five minutes. Someone
  ; taking the machine down on purpose disables the task first —
  ; `schtasks /change /tn SummriseRepair /disable` — and re-enables it afterwards.
  ;
  ; `FileOpen` does not create its directory, and a first install has no `$INSTDIR` yet. Creating it
  ; here writes nothing a running product owns; the images are copied later, after the stop.
  CreateDirectory "$INSTDIR"
  FileOpen $9 "$INSTDIR\repair.cmd" w
  FileWrite $9 "@echo off$\r$\n"
  FileWrite $9 "rem Created by the Summrise installer: keep the ${SERVICE} service registered and running.$\r$\n"
  FileWrite $9 "rem Runs every five minutes. A deliberate stop is undone by it: disable the task first$\r$\n"
  FileWrite $9 "rem (schtasks /change /tn SummriseRepair /disable) to keep this machine down on purpose.$\r$\n"
  FileWrite $9 "sc query ${SERVICE} | find $\"RUNNING$\" >nul 2>&1$\r$\n"
  FileWrite $9 "if not errorlevel 1 exit /b 0$\r$\n"
  FileWrite $9 "sc query ${SERVICE} >nul 2>&1$\r$\n"
  FileWrite $9 "if errorlevel 1 goto missing$\r$\n"
  ; ITS OWN LOG, NOT `repair.log`: the launcher writes that file as a JSON repair journal of the
  ; generations it restored, and appending human lines to it corrupts a record an operator reads.
  FileWrite $9 "echo %DATE% %TIME% the ${SERVICE} service is not running; starting it >> $\"$INSTDIR\repair-task.log$\"$\r$\n"
  FileWrite $9 "sc start ${SERVICE} >> $\"$INSTDIR\repair-task.log$\" 2>&1$\r$\n"
  FileWrite $9 "set RC=%ERRORLEVEL%$\r$\n"
  FileWrite $9 "echo %DATE% %TIME% start exit %RC% >> $\"$INSTDIR\repair-task.log$\"$\r$\n"
  FileWrite $9 "exit /b %RC%$\r$\n"
  FileWrite $9 ":missing$\r$\n"
  FileWrite $9 "echo %DATE% %TIME% the ${SERVICE} service is missing; re-registering >> $\"$INSTDIR\repair-task.log$\"$\r$\n"
  FileWrite $9 "$\"$INSTDIR\summrise-launch.exe$\" --install-service --install $\"$INSTDIR$\" --data $\"${DATA_DIR}$\" --seed-sidecar $\"$INSTDIR\summrise-device.sidecar.json$\" --port ${PORT} >> $\"$INSTDIR\repair-task.log$\" 2>&1$\r$\n"
  FileWrite $9 "set RC=%ERRORLEVEL%$\r$\n"
  FileWrite $9 "echo %DATE% %TIME% repair exit %RC% >> $\"$INSTDIR\repair-task.log$\"$\r$\n"
  FileWrite $9 "exit /b %RC%$\r$\n"
  FileClose $9
  ; **THE `/tr` VALUE NEEDS ESCAPED QUOTES AROUND THE PATH.** schtasks splits the task's command at
  ; the first space unless the executable part is quoted; with an install root that has a space
  ; (`E:\Program Files\Summrise` on the 84T) the plain `/tr "$INSTDIR\repair.cmd"` is stored as
  ; `Command=E:\Program` + `Arguments=Files\Summrise\repair.cmd` and the task cannot start. That is
  ; not a theory: on DESKTOP-9VH92BJ the repair task created by the 0.3.279 install had exactly that
  ; pair, last result 0x80070002, and `repair-task.log` was never written — the five-minute repair
  ; that was supposed to start the service after the 2026-11-07 `/S` install never ran. The value
  ; schtasks must receive is `"<path>"`; NSIS emits the required `\"` from `\$\"`, not from `"$\"`
  ; (which gives `""<path>""` and is rejected outright). The `"\$\"<path>\$\""` form stores
  ; `"<path>"` as the command and starts (measured 2026-11-07 with a probe task).
  nsExec::Exec 'schtasks /create /tn SummriseRepair /tr "\$\"$INSTDIR\repair.cmd\$\"" /sc minute /mo 5 /ru SYSTEM /rl HIGHEST /f'
  Pop $0
  ; No reopen here: `$6` is the section's log handle, still open. Reopening it overwrote the handle
  ; and the `FileClose` after the line closed the log for the rest of the section, which is why the
  ; install-service, install-version and health lines below never reached the file an operator reads.
  FileWrite $6 "repair task: exit $0$\r$\n"

  ; **THE HAND-OFF COMES BEFORE THE FIRST IMAGE REPLACEMENT OR STOP.** A silent setup started under
  ; the running supervisor cannot stop that supervisor: the stop kills the app that hosts the
  ; caller's `runs.exec` (its answer is lost) and, on a launcher without the child-job breakaway,
  ; this setup too (the service is then left stopped). The task it starts is outside the job and
  ; appends its own install to the same log. Apart from the new `repair.cmd` written above (a file
  ; no running process holds), nothing above this line has replaced an image or touched the
  ; service; a failed hand-off therefore reports and aborts with the running product untouched.
  Call RelaySupervisedSetup
  ${If} $R3 == "1"
    FileClose $6
    SetErrorLevel 3
    Quit
  ${EndIf}
  ${If} $R3 == "-1"
    FileWrite $6 "relay: ${RELAY_TASK} could not be started; no image was written and ${SERVICE} was not stopped$\r$\n"
    FileClose $6
    SetErrorLevel 1
    Abort
  ${EndIf}

  ; **STAGE AND PROVE BEFORE ANY STOP.** An existing install's new generation goes into its own
  ; `versions\<version>\` directory — a new path, so nothing running is touched — and
  ; `--install-version` starts it on a scratch port with a copy of the data directory, requires two
  ; matching `/api/health` answers, and only then moves `current.txt`. A failure here leaves the old
  ; generation serving and the pointer untouched.
  ${If} $R8 != "0"
    FileWrite $6 "upgrade: staging versions\${VERSION}$\r$\n"
    ; A version directory is immutable once named. Copy only what is missing: an existing binary is
    ; never overwritten (the launcher refuses different bytes), and a half-written directory left by
    ; a crash can still be completed.
    CreateDirectory "$INSTDIR\versions\${VERSION}"
    ${IfNot} ${FileExists} "$INSTDIR\versions\${VERSION}\summrise-device.exe"
      SetOutPath "$INSTDIR\versions\${VERSION}"
      File /oname=summrise-device.exe "${SOURCE}\summrise-device.exe"
      SetOutPath "$INSTDIR"
    ${EndIf}
    ${IfNot} ${FileExists} "$INSTDIR\versions\${VERSION}\summrise-device.json"
      SetOutPath "$INSTDIR\versions\${VERSION}"
      File /oname=summrise-device.json "${SOURCE}\summrise-device.sidecar.json"
      SetOutPath "$INSTDIR"
    ${EndIf}
    ; **PROVE WITH THE NEW LAUNCHER, NOT THE INSTALLED ONE.** An install from 0.3.275/0.3.276 has a
    ; launcher whose `--install-version` accepts only the release key; the setup's sidecar is the
    ; first generation's, signed with the launcher key, so the installed launcher would refuse its
    ; own upgrade. The new launcher is copied beside the install instead: creating a new image is
    ; safe while the service runs, overwriting the running one is not. It is removed once the
    ; pointer has moved (and on failure).
    SetOutPath "$INSTDIR"
    File /oname=summrise-launch-${VERSION}.exe "${SOURCE}\summrise-launch.exe"
    SetOutPath "$INSTDIR"
    nsExec::ExecToStack '"$INSTDIR\summrise-launch-${VERSION}.exe" --install-version "$INSTDIR\versions\${VERSION}" "$INSTDIR\versions\${VERSION}\summrise-device.json" --install "$INSTDIR" --data "${DATA_DIR}" --port ${PORT}'
    Pop $0
    Pop $1
    FileWrite $6 "install-version ${VERSION}: exit $0$\r$\n$1$\r$\n"
    ${If} $0 != "0"
      ; The launcher's contract: a failed probe or pointer move leaves `current.txt` exactly as it
      ; was, and nothing here has stopped the service. Report and abort; do not repair.
      FileWrite $6 "install-version failed; the service was not stopped and $INSTDIR\current.txt is untouched$\r$\n"
      Delete "$INSTDIR\summrise-launch-${VERSION}.exe"
      FileClose $6
      SetErrorLevel 1
      ${IfNot} ${Silent}
        MessageBox MB_ICONSTOP "Version ${VERSION} could not be staged and proven (launcher exit $0).$\r$\n$\r$\nThe running service was not stopped and keeps serving the version it booted.$\r$\n$\r$\n$1$\r$\n$\r$\nThe installer's log is $TEMP\summrise-setup.log."
      ${EndIf}
      Abort
    ${EndIf}
    Delete "$INSTDIR\summrise-launch-${VERSION}.exe"
    FileWrite $6 "install-version ${VERSION}: proven; $INSTDIR\current.txt names it$\r$\n"
    ${If} $R8 == "1"
      ; The pointer exists now, so the one-time migration below is not needed: this install is
      ; versioned and proven, and the stop below replaces its images.
      StrCpy $R8 "2"
    ${EndIf}
  ${EndIf}

  ; **THE STOP COMES AFTER THE PROOF, AND ONLY THEN.** A first install finds no service and the
  ; stop is skipped; a failed proof above never reaches this line.
  Call StopRuntime
  ${If} $R6 == "0"
    ; The setup did stop it (or is still doing so) but could not prove STOPPED, so it is aborting
    ; before replacing images. **PUT THE SERVICE BACK.** A failed install must not also take the
    ; running product down; if it had reached STOPPED, this starts it again, and if it is still
    ; stopping the wait covers that. Silent mode has no dialog, so the log and exit code carry it.
    Call EnsureServiceRunning
    ${If} $R7 == "1"
      StrCpy $R4 "The ${SERVICE} service is running again."
    ${Else}
      StrCpy $R4 "The ${SERVICE} service is NOT running and $R2 start attempt(s) did not make it answer RUNNING."
    ${EndIf}
    FileWrite $6 "stop: ${SERVICE} did not stop; no image was replaced. $R4$\r$\n"
    FileClose $6
    SetErrorLevel 1
    ${IfNot} ${Silent}
      ${If} $R8 == "2"
        MessageBox MB_ICONSTOP "The ${SERVICE} service did not stop, so the installed files were not replaced.$\r$\n$\r$\n$INSTDIR\current.txt already names ${VERSION}; the process still serving is the version it booted. The pointer can be rolled back with one write.$\r$\n$\r$\n$R4$\r$\n$\r$\nThe setup's log is $TEMP\summrise-setup.log."
      ${Else}
        MessageBox MB_ICONSTOP "The ${SERVICE} service did not stop, so the installed files were not replaced.$\r$\n$\r$\n$R4$\r$\n$\r$\nThe setup's log is $TEMP\summrise-setup.log."
      ${EndIf}
    ${EndIf}
    Abort
  ${EndIf}

  ; The flat-layout images, replaced only once the supervisor is down.
  SetOutPath "$INSTDIR"
  File /oname=summrise-launch.exe "${SOURCE}\summrise-launch.exe"
  File /oname=summrise-device.exe "${SOURCE}\summrise-device.exe"
  File /oname=summrise-console.exe "${SOURCE}\summrise-console.exe"
  File /oname=summrise-shell.exe "${SOURCE}\summrise-shell.exe"
  File /oname=summrise-device.sidecar.json "${SOURCE}\summrise-device.sidecar.json"
  ; The browser is a product asset, not a machine prerequisite: a clean Windows machine with only
  ; Edge must still get the headless Chromium the browser capability drives. `current` is stable
  ; across upgrades; the pack's manifest names its version and the installer's digest covers it.
  !ifdef BROWSER_SOURCE
    SetOutPath "$INSTDIR\browser\current"
    File /r "${BROWSER_SOURCE}\*"
    SetOutPath "$INSTDIR"
  !endif
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; The runtime is a headless service; the tray is the visible half. HKLM Run starts it in the
  ; logged-in user's session at every login, with no elevation and no second credential.
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "SummriseShell" '"$INSTDIR\summrise-shell.exe"' 

  WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "${COMPANY}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1

  ; The one install implementation: seed the baseline, mint the token, harden the data directory,
  ; register the service. A failure aborts the install rather than leaving a half-installed tree.
  ;
  ; A SILENT INSTALL MUST NEVER SHOW A BOX: `/S` has nobody to click OK, so a MessageBox is an
  ; invisible hang (it held a canary install for minutes with the process alive and no output).
  ; Silent mode reports through the exit code and the log instead.
  FileWrite $6 "section: files written, registry $INSTDIR$\r$\n"
  FileWrite $6 "install: $INSTDIR$\r$\n"
  FileWrite $6 "data: ${DATA_DIR}$\r$\n"
  FileWrite $6 "port: ${PORT}$\r$\n"
  ; DIAGNOSTIC: the uninstall entry is written before this log opens; read it back so the log says
  ; whether the write landed and in which view.
  ReadRegStr $8 HKLM "${UNINST_KEY}" "DisplayVersion"
  FileWrite $6 "registry readback: '$8' (view 64)$\r$\n"

  nsExec::ExecToStack '"$INSTDIR\summrise-launch.exe" --install-service --replace-baseline --install "$INSTDIR" --data "${DATA_DIR}" --seed-sidecar "$INSTDIR\summrise-device.sidecar.json" --port ${PORT}'
  Pop $0
  Pop $1
  FileWrite $6 "install-service: exit $0$\r$\n$1$\r$\n"
  ${If} $0 != "0"
    ; **THE DIALOG MUST NAME THE LAUNCHER'S FAILURE, NOT sc's.** `EnsureServiceRunning` overwrites
    ; $0/$1 with the start it tried, so a launcher that refused before registering the service read
    ; as "1060, the service is not installed" — the one sentence that cannot tell a missing runtime
    ; from a seed-sidecar refusal from an AV block (measured 2026-11-06 on a real install). Keep the
    ; launcher's exit and its own message in registers the check does not touch.
    StrCpy $R4 $0
    StrCpy $R5 $1
    ; Leave the machine serving if it can: the registration is still there (the stop above does not
    ; remove it), so starting it runs whatever generation is consistent rather than nothing at all.
    ; A start that does not reach RUNNING is reported here, never swallowed.
    Call EnsureServiceRunning
    ${If} $R7 == "1"
      StrCpy $R0 "The ${SERVICE} service is RUNNING; it is serving the generation it booted."
    ${Else}
      StrCpy $R0 "The ${SERVICE} service is NOT running; $R2 start attempt(s) did not make it answer RUNNING."
    ${EndIf}
    FileWrite $6 "install-service failed; $R0$\r$\n"
    FileClose $6
    ${If} ${Silent}
      SetErrorLevel 1
    ${Else}
      MessageBox MB_ICONSTOP "The device runtime could not be installed (launcher exit $R4).$\r$\n$\r$\n$R5$\r$\n$\r$\n$R0$\r$\n$\r$\nThe installer's log is $TEMP\summrise-setup.log."
    ${EndIf}
    Abort
  ${EndIf}

  ${If} $ConsoleUrl != ""
  ${AndIf} $EnrollCode != ""
    ; The service is installed but not started yet, which is exactly when enrolling is safe: no
    ; channel is open and no token exists to rotate.
    nsExec::ExecToStack '"$INSTDIR\summrise-device.exe" --enroll "$ConsoleUrl" "$EnrollCode" $9 --data "${DATA_DIR}" --install "$INSTDIR"'
    Pop $0
    Pop $1
    FileWrite $6 "enroll: exit $0$\r$\n$1$\r$\n"
  ${EndIf}

  ; `--install-service` starts the service when it is not running; a second start then answers 1056
  ; ("already running"), which is success, not a warning. **THE START IS PROVEN, NOT ASSUMED.** The
  ; exit code is logged, then the SCM is asked for the state; a service the installer cannot get to
  ; RUNNING is an install failure even though every file is on disk, and silent mode says so through
  ; the exit code and the log instead of a dialog nobody can click.
  nsExec::ExecToStack 'sc.exe start ${SERVICE}'
  Pop $0
  Pop $1
  FileWrite $6 "sc start: exit $0$\r$\n$1$\r$\n"
  Call EnsureServiceRunning
  ${If} $R7 != "1"
    FileWrite $6 "service: ${SERVICE} is NOT running after install-service ($R2 start attempt(s))$\r$\n"
    FileClose $6
    SetErrorLevel 1
    ${IfNot} ${Silent}
      MessageBox MB_ICONSTOP "The device files are installed, but the ${SERVICE} service is not running.$\r$\n$\r$\nThe setup started it $R2 time(s) and the SCM never reported RUNNING.$\r$\n$\r$\nThe setup's log is $TEMP\summrise-setup.log."
    ${EndIf}
    Abort
  ${EndIf}
  FileWrite $6 "service: ${SERVICE} is RUNNING$\r$\n"

  ; **THE INSTALL IS NOT DONE UNTIL THE DEVICE ANSWERS.** A registered service that cannot bind its
  ; port (the legacy agent holds 18080 by default) looks installed and serves nothing. Poll the
  ; health endpoint the app exposes; the installer's verdict is the device's own answer.
  ; BOUNDED. The first version looped until the device answered, which on a port held by another
  ; program meant a setup that never returned: invisible in /S, holding its own file, and every later
  ; install died with "the file is in use by another process".
  ; An upgrade has one more question: the answer must name the version `current.txt` names, or the
  ; setup would recreate the exact symptom the upgrade exists to remove — a pointer naming one
  ; version while another answers.
  ${If} $R8 == "2"
    StrCpy $R5 30
  ${Else}
    StrCpy $R5 10
  ${EndIf}
  StrCpy $2 0
  StrCpy $3 "0"
  StrCpy $4 ""
  ${Do}
    IntOp $2 $2 + 1
    nsExec::ExecToStack 'curl.exe -sf -m 2 http://127.0.0.1:${PORT}/api/health'
    Pop $0
    Pop $1
    ${If} $0 == "0"
      StrCpy $4 "$1"
      ${If} $R8 == "2"
        ${StrStr} $5 "$1" "$\"version$\":$\"${VERSION}$\""
        ${If} $5 != ""
          StrCpy $3 "1"
          ${ExitDo}
        ${EndIf}
      ${Else}
        StrCpy $3 "1"
        ${ExitDo}
      ${EndIf}
    ${EndIf}
    ${If} $2 >= $R5
      ${ExitDo}
    ${EndIf}
    Sleep 1000
  ${Loop}
  ${If} $3 != "1"
    ; The health answer is the device's verdict, but the service is the thing this setup owns and
    ; this is the last place to leave it running. Ask the SCM, start it if the app's failure took it
    ; down, and put that state in the log and the dialog; a silent install still exits non-zero.
    Call EnsureServiceRunning
    ${If} $R7 == "1"
      StrCpy $R4 "The ${SERVICE} service is RUNNING; the app behind it did not answer."
    ${Else}
      StrCpy $R4 "The ${SERVICE} service is NOT running and $R2 start attempt(s) did not make it answer RUNNING."
    ${EndIf}
    FileWrite $6 "health: no ${VERSION} answer on port ${PORT} after $R5s; last answer: $4. $R4$\r$\n"
    FileClose $6
    ${If} ${Silent}
      SetErrorLevel 1
    ${Else}
      ${If} $R8 == "2"
        ; The pointer, the version directory it names and `previous.txt` are all still there, so the
        ; operator can roll back with one pointer write; the old version was never removed. Report,
        ; never repair behind the operator's back, and never call this install a success.
        MessageBox MB_ICONSTOP "Version ${VERSION} was made current, but the restarted service does not report it on /api/health.$\r$\n$\r$\nLast answer:$\r$\n$4$\r$\n$\r$\n$R4$\r$\n$\r$\n$INSTDIR\current.txt names ${VERSION} and previous.txt still names the version that was running, so the pointer can be rolled back."
      ${Else}
        MessageBox MB_ICONSTOP "The service is registered, but the device does not answer on port ${PORT}.$\r$\n$\r$\n$R4$\r$\n$\r$\nAnother program may be using that port (the pre-1.3 agent holds 18080). Stop it or rebuild the installer with another port."
      ${EndIf}
    ${EndIf}
    Abort
  ${EndIf}
  FileWrite $6 "health: ok on port ${PORT}$\r$\n"
  ; **THE PRODUCT'S OWN COMMAND IS A BARE NAME, SO THE MACHINE PATH MUST CARRY THE ROOT.** The
  ; console's enrollment sentence names the install folder, but a prompt opened anywhere else finds
  ; `summrise-device.exe` only when its directory is on the machine Path. This runs after the health
  ; check: a setup that could not prove a working device has no business putting its directory on
  ; the machine's Path. An entry already present is left untouched; otherwise it is appended once,
  ; delimited by `;`.
  ;
  ; `ReadRegStr` returns the stored REG_EXPAND_SZ text without expanding it, so other entries'
  ; `%SystemRoot%` spellings survive the write below. It sets the error flag and leaves the variable
  ; empty for a value longer than this setup's 1024-character buffer, or a missing/non-string value;
  ; that is "unreadable", not "empty", and is left alone rather than overwritten.
  ClearErrors
  ReadRegStr $0 HKLM "${MACHINE_ENV}" "Path"
  ${If} ${Errors}
    FileWrite $6 "path: machine Path is unreadable (missing, non-string, or longer than this setup's 1024-character buffer); not written$\r$\n"
  ${Else}
    ; Delimited membership: wrapping the value in `;` turns an entry at either end into a middle
    ; entry, so one search proves `;$INSTDIR;` and never matches a longer path that merely starts or
    ; ends with the same text.
    StrCpy $1 ";$0;"
    ${StrStr} $2 "$1" ";$INSTDIR;"
    ${If} $2 != ""
      FileWrite $6 "path: $INSTDIR is already present; not written$\r$\n"
    ${Else}
      ; Exactly one separator: none before an empty value, none after one that already ends in `;`.
      StrCpy $3 ";"
      ${If} $0 == ""
        StrCpy $3 ""
      ${Else}
        StrCpy $4 $0 1 -1
        ${If} $4 == ";"
          StrCpy $3 ""
        ${EndIf}
      ${EndIf}
      StrLen $5 "$0"
      StrLen $R0 "$INSTDIR"
      IntOp $R0 $5 + $R0
      StrLen $R1 "$3"
      IntOp $R0 $R0 + $R1
      ; 2000 is the ceiling this entry may cost the machine Path. 1023 is the real ceiling of the
      ; setup's own string buffer: `WriteRegExpandStr` writes that buffer, and a longer value would
      ; be silently cut mid-entry. Either way the Path is left alone and the install still succeeds;
      ; the console sentence still tells the operator the folder.
      ${If} $R0 > 2000
        FileWrite $6 "path: not written; the machine Path would be $R0 characters (over 2000)$\r$\n"
      ${ElseIf} $R0 > 1023
        FileWrite $6 "path: not written; the machine Path would be $R0 characters, over this setup's 1024-character string buffer$\r$\n"
      ${Else}
        StrCpy $R1 "$0$3$INSTDIR"
        WriteRegExpandStr HKLM "${MACHINE_ENV}" "Path" "$R1"
        FileWrite $6 "path: appended $INSTDIR; the machine Path is now $R0 characters$\r$\n"
        !insertmacro BroadcastEnvironmentChange
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ; **AN UPGRADE IS NOT A MIGRATION.** `--migrate-layout` is the one-time move from the flat layout:
  ; once `current.txt` exists it answers "already on the versioned layout" and changes nothing. An
  ; existing install was already staged as `versions\<version>\`, proven, and its pointer moved
  ; above; only a first install (mode 0) reaches this one-time move.
  ${If} $R8 != "2"
    ; THE ONE-TIME MOVE to versions\<name>\ + current.txt, after the flat install has answered
    ; health. The launcher owns the transaction and its fallback: a migration that cannot serve
    ; removes current.txt again and restarts the flat layout, so a failure here is reported, never
    ; fatal.
    nsExec::ExecToStack '"$INSTDIR\summrise-launch.exe" --migrate-layout --install "$INSTDIR" --data "${DATA_DIR}" --port ${PORT}'
    Pop $0
    Pop $1
    FileWrite $6 "migrate-layout: exit $0$\r$\n$1$\r$\n"
    ${If} $0 != "0"
      FileWrite $6 "migrate-layout failed; the flat layout remains in charge$\r$\n"
      ${IfNot} ${Silent}
        MessageBox MB_ICONEXCLAMATION "The device was installed, but the one-time migration to the versioned layout failed (exit $0).$\r$\n$\r$\nIt is still running the flat layout.$\r$\n$\r$\n$1"
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ; The stop above killed the tray so its image could be replaced. The login Run entry only covers
  ; the next sign-in; start it now in the logged-on user's session so an upgrade does not look like a
  ; vanished product. The one-shot task only supplies SYSTEM's SeTcb privilege: the launcher itself
  ; starts the shell with the user's token, environment and `winsta0\\default`. A failure here is
  ; logged, not fatal: the next sign-in still starts it.
  FileOpen $9 "$INSTDIR\start-shell.cmd" w
  FileWrite $9 "@echo off$\r$\n"
  FileWrite $9 "$\"$INSTDIR\summrise-launch.exe$\" --start-shell --install $\"$INSTDIR$\"$\r$\n"
  FileClose $9
  ; The same `/tr` rule as the repair task above: a path with a space needs the escaped quotes
  ; inside the `/tr` value, or the task is registered as `Command=$INSTDIR` split at the first
  ; space (`E:\Program`) and never starts. `\$\"` is what emits that `\"`; the 0.3.289 form
  ; `"$\"...$\""` emitted `""<path>""`, which schtasks rejected before creating any task (measured
  ; on the 84T's relayed `/S`: `shell task create: -2147467259`, then the misleading
  ; `shell task run: 1` / "cannot find the file" of a `/run` with no task). This one is one-shot and
  ; deleted below, so a broken registration only costs the tray after the upgrade — which the login
  ; Run entry covers at the next sign-in — but it is the same defect and it is fixed here too.
  nsExec::ExecToStack 'schtasks /create /tn SummriseShellStart /tr "\$\"$INSTDIR\start-shell.cmd\$\"" /sc onstart /ru SYSTEM /rl HIGHEST /f'
  Pop $0
  Pop $1
  FileWrite $6 "shell task create: $0$\r$\n$1$\r$\n"
  ${If} $0 == "0"
    nsExec::ExecToStack 'schtasks /run /tn SummriseShellStart'
    Pop $0
    Pop $1
    FileWrite $6 "shell task run: $0$\r$\n$1$\r$\n"
    Sleep 5000
    nsExec::Exec 'schtasks /delete /tn SummriseShellStart /f'
  ${Else}
    ; A failed create leaves no task to run; calling `/run` anyway logged a second, misleading
    ; "cannot find the file" instead of surfacing this create line as the cause.
    FileWrite $6 "shell task not created; the tray waits for the login Run entry$\r$\n"
  ${EndIf}

  ; **THE EXIT CODE IS THE SERVICE'S STATE.** The health check proves the device answered; this
  ; proves the thing the setup stopped, replaced and restarted is still there at the end. Exit 0
  ; therefore means "files installed and ${SERVICE} RUNNING"; a stopped service is exit 1 and, in
  ; silent mode, a log line — the only two channels `/S` has. If it stopped during the steps above,
  ; this starts it again before making that judgement.
  Call EnsureServiceRunning
  ${If} $R7 != "1"
    FileWrite $6 "service: ${SERVICE} is NOT running at the end of the install ($R2 start attempt(s)); exit 1$\r$\n"
    FileClose $6
    SetErrorLevel 1
    ${IfNot} ${Silent}
      MessageBox MB_ICONSTOP "The device files are installed, but the ${SERVICE} service is not running.$\r$\n$\r$\nThe setup made $R2 start attempt(s) and the SCM never reported RUNNING. The machine will not serve until it is started:$\r$\n$\r$\n    sc start ${SERVICE}$\r$\n$\r$\nThe setup's log is $TEMP\summrise-setup.log."
    ${EndIf}
    Abort
  ${EndIf}
  FileWrite $6 "service: ${SERVICE} is RUNNING at the end of the install; exit 0$\r$\n"
  FileClose $6
SectionEnd

Section "Uninstall"
  SetShellVarContext all
  SetRegView 64
  nsExec::Exec 'taskkill /F /IM summrise-shell.exe'
  nsExec::ExecToStack '"$INSTDIR\summrise-launch.exe" --uninstall-service'
  Pop $0
  Pop $1

  DeleteRegValue HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "SummriseShell"
  ; The repair task would re-register the service the operator just uninstalled.
  nsExec::Exec 'schtasks /delete /tn SummriseRepair /f'
  ; A supervised silent install hands itself to this task; an uninstall in the gap between the
  ; hand-off and the task's run must not leave a boot-time install behind.
  nsExec::Exec 'schtasks /delete /tn ${RELAY_TASK} /f'
  Delete "$INSTDIR\repair.cmd"
  Delete "$INSTDIR\summrise-launch.exe"
  Delete "$INSTDIR\summrise-shell.exe"
  Delete "$INSTDIR\summrise-device.exe"
  Delete "$INSTDIR\summrise-console.exe"
  Delete "$INSTDIR\summrise-device.sidecar.json"
  Delete "$INSTDIR\start-shell.cmd"
  Delete "$INSTDIR\uninstall.exe"
  ; The launcher writes these, so NSIS has no record of them: without this the two generations
  ; (~30 MB) and the repair record survive an uninstall, and a later install inherits a stale
  ; baseline. The device's own data directory is NOT here — it is kept above, on purpose.
  RMDir /r "$INSTDIR\baseline"
  RMDir /r "$INSTDIR\browser"
  Delete "$INSTDIR\repair.log"
  Delete "$INSTDIR\swap.request.json"
  Delete "$INSTDIR\swap.result.json"
  Delete "$INSTDIR\*.json.tmp"
  RMDir "$INSTDIR"

  ; **TAKE EXACTLY ITS OWN DELIMITED ENTRY BACK OUT.** The install appends `$INSTDIR` as a
  ; `;`-delimited entry; an entry that merely contains the same text — a sibling directory or a
  ; longer path — must survive, so every match here is bounded by `;` or by the value's end.
  ; This reads only the registry: the tree above is already gone, and a stale Path entry must not
  ; depend on a file that no longer exists.
  ;
  ; A read that fails (missing, non-string, or longer than this setup's 1024-character buffer) is
  ; left alone: the uninstaller cannot safely rewrite a value it could not read, and a dead entry in
  ; the Path breaks nothing.
  ClearErrors
  ReadRegStr $0 HKLM "${MACHINE_ENV}" "Path"
  ${IfNot} ${Errors}
    StrCpy $1 "$0"
    ; middle: ";$INSTDIR;" -> ";"
    ${UnStrRep} $1 "$1" ";$INSTDIR;" ";"
    StrLen $2 "$INSTDIR"
    IntOp $3 $2 + 1
    ; first: "$INSTDIR;" at the start
    StrCpy $4 "$1" $3
    ${If} $4 == "$INSTDIR;"
      StrCpy $1 "$1" "" $3
    ${EndIf}
    ; last: ";$INSTDIR" at the end
    IntOp $4 $3 * -1
    StrCpy $5 "$1" "" $4
    ${If} $5 == ";$INSTDIR"
      StrLen $5 "$1"
      IntOp $2 $5 - $3
      StrCpy $1 "$1" $2
    ${EndIf}
    ; the whole value was the entry
    ${If} $1 == "$INSTDIR"
      StrCpy $1 ""
    ${EndIf}
    ${If} $1 != "$0"
      WriteRegExpandStr HKLM "${MACHINE_ENV}" "Path" "$1"
      !insertmacro BroadcastEnvironmentChange
    ${EndIf}
  ${EndIf}

  DeleteRegKey HKLM "${UNINST_KEY}"

  ; Silent uninstall keeps the data: nobody can answer the question, and deleting state.json, the
  ; token and the identity behind an operator's back is the worse default.
  ${IfNot} ${Silent}
    MessageBox MB_YESNO|MB_ICONQUESTION "Delete the device data in ${DATA_DIR}?$\r$\n$\r$\nThat directory holds state.json, the install token and the device identity; keeping it lets a reinstall resume where this one stopped." IDNO +2
    RMDir /r "${DATA_DIR}"
  ${EndIf}
SectionEnd
