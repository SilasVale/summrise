; Summrise Device Runtime — the Windows installer.
;
; Built by CI on windows-latest (NSIS ships there) and by hand with `makensis` elsewhere:
;   makensis -DVERSION=0.1.1 -DSOURCE=<dir with the binaries> -DPORT=18080 installer/summrise.nsi
;
; The wizard is MUI2, the same page set Tauri's Windows bundles use. The installer does NOT own the
; runtime's install logic: it copies the files and calls `summrise-launch --install-service`, which
; seeds the baseline from the signed sidecar, mints the install token, hardens the data directory
; and registers the service. One implementation, so an installer and a hand install cannot drift.

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
OutFile "SummriseSetup-${VERSION}.exe"
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
  FileOpen $6 "$TEMP\summrise-setup.log" w
  FileWrite $6 "onInit start (${VERSION}, port ${PORT}) install=$INSTDIR existing=$R9$\r$\n"
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
    FileWrite $6 "stop: ${SERVICE} did not stop; no image was replaced$\r$\n"
    FileClose $6
    SetErrorLevel 1
    ${IfNot} ${Silent}
      ${If} $R8 == "2"
        MessageBox MB_ICONSTOP "The ${SERVICE} service did not stop, so the installed files were not replaced.$\r$\n$\r$\n$INSTDIR\current.txt already names ${VERSION}; the process still serving is the version it booted. The pointer can be rolled back with one write.$\r$\n$\r$\nThe setup's log is $TEMP\summrise-setup.log."
      ${Else}
        MessageBox MB_ICONSTOP "The ${SERVICE} service did not stop, so the installed files were not replaced.$\r$\n$\r$\nThe setup's log is $TEMP\summrise-setup.log."
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

  ; A PERIODIC REPAIR, because losing the service is a state nothing inside the machine can leave:
  ; the process that would rebuild it IS the service. Two incidents made this: a failed install
  ; removed it once and left the device unreachable until someone walked to it (DESKTOP-CPH4KB2,
  ; 2026-10-28), and on 2026-10-29 the service sat stopped for four hours while the console answered
  ; `device reconnecting` — Windows restarts a service that FAILS three times and then stops trying,
  ; and a service that stopped without failing is not restarted at all. The task runs every five
  ; minutes: it starts the service when it is registered but not running, and re-registers it when
  ; it is gone.
  ;
  ; THE COST IS STATED WHERE IT IS FELT: a deliberate stop is undone within five minutes. Someone
  ; taking the machine down on purpose disables the task first —
  ; `schtasks /change /tn SummriseRepair /disable` — and re-enables it afterwards.
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
  FileWrite $9 "$\"$INSTDIR\summrise-launch.exe$\" --install-service --install $\"$INSTDIR$\" --data $\"${DATA_DIR}$\" --seed-sidecar $\"$INSTDIR\summrise-device.sidecar.json$\" --port ${PORT} >> $\"$INSTDIR\repair-task.log$\" 2>&1$\r\n"
  FileWrite $9 "set RC=%ERRORLEVEL%$\r$\n"
  FileWrite $9 "echo %DATE% %TIME% repair exit %RC% >> $\"$INSTDIR\repair-task.log$\"$\r$\n"
  FileWrite $9 "exit /b %RC%$\r$\n"
  FileClose $9
  nsExec::Exec 'schtasks /create /tn SummriseRepair /tr "$INSTDIR\repair.cmd" /sc minute /mo 5 /ru SYSTEM /rl HIGHEST /f'
  Pop $0
  ; No reopen here: `$6` is the section's log handle, still open. Reopening it overwrote the handle
  ; and the `FileClose` after the line closed the log for the rest of the section, which is why the
  ; install-service, install-version and health lines below never reached the file an operator reads.
  FileWrite $6 "repair task: exit $0$\r$\n"

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
    ; **THE DIALOG MUST NAME THE LAUNCHER'S FAILURE, NOT sc's.** The `sc start` below overwrote
    ; $0/$1, so a launcher that refused before registering the service read as "1060, the service is
    ; not installed" — the one sentence that cannot tell a missing runtime from a seed-sidecar
    ; refusal from an AV block (measured 2026-11-06 on a real install). Keep the launcher's exit and
    ; its own message, then append what sc answered.
    StrCpy $R0 $0
    StrCpy $R1 $1
    ; Leave the machine serving if it can: the registration is still there (the stop above does not
    ; remove it), so starting it runs whatever generation is consistent rather than nothing at all.
    nsExec::ExecToStack 'sc.exe start ${SERVICE}'
    Pop $0
    Pop $1
    FileWrite $6 "install-service failed; sc start says $0 $1$\r$\n"
    FileClose $6
    ${If} ${Silent}
      SetErrorLevel 1
    ${Else}
      MessageBox MB_ICONSTOP "The device runtime could not be installed (launcher exit $R0).$\r$\n$\r$\n$R1$\r$\n$\r$\nThe installer's log is $TEMP\summrise-setup.log.$\r$\n$\r$\n(sc start then answered $0: $1)"
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
  ; ("already running"), which is success, not a warning.
  nsExec::ExecToStack 'sc.exe start ${SERVICE}'
  Pop $0
  Pop $1
  FileWrite $6 "sc start: exit $0$\r$\n$1$\r$\n"

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
    FileWrite $6 "health: no ${VERSION} answer on port ${PORT} after $R5s; last answer: $4$\r$\n"
    FileClose $6
    ${If} ${Silent}
      SetErrorLevel 1
    ${Else}
      ${If} $R8 == "2"
        ; The pointer, the version directory it names and `previous.txt` are all still there, so the
        ; operator can roll back with one pointer write; the old version was never removed. Report,
        ; never repair behind the operator's back, and never call this install a success.
        MessageBox MB_ICONSTOP "Version ${VERSION} was made current, but the restarted service does not report it on /api/health.$\r$\n$\r$\nLast answer:$\r$\n$4$\r$\n$\r$\n$INSTDIR\current.txt names ${VERSION} and previous.txt still names the version that was running, so the pointer can be rolled back."
      ${Else}
        MessageBox MB_ICONSTOP "The service is registered, but the device does not answer on port ${PORT}.$\r$\n$\r$\nAnother program may be using that port (the pre-1.3 agent holds 18080). Stop it or rebuild the installer with another port."
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
  nsExec::ExecToStack 'schtasks /create /tn SummriseShellStart /tr "$INSTDIR\start-shell.cmd" /sc onstart /ru SYSTEM /rl HIGHEST /f'
  Pop $0
  Pop $1
  FileWrite $6 "shell task create: $0$\r$\n$1$\r$\n"
  nsExec::ExecToStack 'schtasks /run /tn SummriseShellStart'
  Pop $0
  Pop $1
  FileWrite $6 "shell task run: $0$\r$\n$1$\r$\n"
  Sleep 5000
  nsExec::Exec 'schtasks /delete /tn SummriseShellStart /f'
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
