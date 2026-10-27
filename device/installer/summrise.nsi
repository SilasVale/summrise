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
; `/CONSOLE=` and `/CODE=` are read out of the command line: the optional auto-enrolment below.
!include "FileFunc.nsh"

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
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Summrise"
; With `SetShellVarContext all`, `$APPDATA` is the machine's ProgramData. NSIS has no
; `$COMMONAPPDATA` constant: writing one compiles to a literal path and puts the data
; directory in the wrong place.
!define DATA_DIR "$APPDATA\Summrise"

Name "${PRODUCT} ${VERSION}"
OutFile "SummriseSetup-${VERSION}.exe"
InstallDir "$PROGRAMFILES64\Summrise"
InstallDirRegKey HKLM "${UNINST_KEY}" "InstallLocation"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
BrandingText "${COMPANY}"

; The two optional auto-enrolment values, read from `/CONSOLE=` and `/CODE=` in the install section.
Var ConsoleUrl
Var EnrollCode

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; An upgrade must stop the old supervisor before its image is replaced: the service is
; uninstalled here and re-registered by the install section.
Function .onInit
  ; DIAGNOSTIC: a setup that exits 0 without touching anything is the hardest failure to see from
  ; the outside, so the init path leaves a trace next to the install log.
  FileOpen $6 "$TEMP\summrise-setup.log" w
  FileWrite $6 "onInit start (${VERSION}, port ${PORT}) install=$INSTDIR$\r$\n"
  nsExec::Exec 'sc.exe stop ${SERVICE}'
  Pop $0
  ; THE SERVICE STOPS ASYNCHRONOUSLY, AND THE FILES CANNOT BE REPLACED UNTIL IT HAS. `sc stop`
  ; returns when the control is accepted, not when the process is gone; copying over a running
  ; image fails with a sharing violation, which used to abort the install *after* the uninstall step
  ; and left the machine with neither the old service nor the new files. Wait for the launcher
  ; process to disappear, bounded, before touching anything.
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
  FileWrite $6 "onInit: launcher waited out after $2 attempt(s)$\r$\n"
  ; `--install-service`/`--uninstall-service` always act on the launcher's default service name;
  ; only `service --service-name` selects another one for the SCM's own argv.
  nsExec::ExecToStack '"$INSTDIR\summrise-launch.exe" --uninstall-service'
  Pop $0
  Pop $1
  FileWrite $6 "onInit: uninstall-service exit $0$\r$\n"
  ; The tray is a per-user process, not the service: close it before its image is replaced, and the
  ; login Run entry below starts the new one next time.
  nsExec::Exec 'taskkill /F /IM summrise-shell.exe'
  FileClose $6
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
  FileOpen $6 "$TEMP\summrise-setup.log" a
  FileWrite $6 "section: install to $INSTDIR$\r$\n"
  SetOutPath "$INSTDIR"
  File /oname=summrise-launch.exe "${SOURCE}\summrise-launch.exe"
  File /oname=summrise-device.exe "${SOURCE}\summrise-device.exe"
  File /oname=summrise-console.exe "${SOURCE}\summrise-console.exe"
  File /oname=summrise-shell.exe "${SOURCE}\summrise-shell.exe"
  File /oname=summrise-device.sidecar.json "${SOURCE}\summrise-device.sidecar.json"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; The runtime is a headless service; the tray is the visible half. HKLM Run starts it in the
  ; logged-in user's session at every login, with no elevation and no second credential.
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "SummriseShell" '"$INSTDIR\summrise-shell.exe"' 

  WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "${COMPANY}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\summrise-launch.exe"
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
    FileClose $6
    ${If} ${Silent}
      SetErrorLevel 1
    ${Else}
      MessageBox MB_ICONSTOP "The device runtime could not be installed (exit $0).$\r$\n$\r$\n$1"
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
  StrCpy $2 0
  StrCpy $3 "0"
  ${Do}
    IntOp $2 $2 + 1
    nsExec::ExecToStack 'curl.exe -sf -m 2 http://127.0.0.1:${PORT}/api/health'
    Pop $0
    Pop $1
    ${If} $0 == "0"
      StrCpy $3 "1"
      ${ExitDo}
    ${EndIf}
    ${If} $2 >= 10
      ${ExitDo}
    ${EndIf}
    Sleep 1000
  ${Loop}
  ${If} $3 != "1"
    FileWrite $6 "health: no answer on port ${PORT} after 10s$\r$\n"
    FileClose $6
    ${If} ${Silent}
      SetErrorLevel 1
    ${Else}
      MessageBox MB_ICONSTOP "The service is registered, but the device does not answer on port ${PORT}.$\r$\n$\r$\nAnother program may be using that port (the pre-1.3 agent holds 18080). Stop it or rebuild the installer with another port."
    ${EndIf}
    Abort
  ${EndIf}
  FileWrite $6 "health: ok on port ${PORT}$\r$\n"
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
  Delete "$INSTDIR\summrise-launch.exe"
  Delete "$INSTDIR\summrise-shell.exe"
  Delete "$INSTDIR\summrise-device.exe"
  Delete "$INSTDIR\summrise-console.exe"
  Delete "$INSTDIR\summrise-device.sidecar.json"
  Delete "$INSTDIR\uninstall.exe"
  ; The launcher writes these, so NSIS has no record of them: without this the two generations
  ; (~30 MB) and the repair record survive an uninstall, and a later install inherits a stale
  ; baseline. The device's own data directory is NOT here — it is kept above, on purpose.
  RMDir /r "$INSTDIR\baseline"
  Delete "$INSTDIR\repair.log"
  Delete "$INSTDIR\swap.request.json"
  Delete "$INSTDIR\swap.result.json"
  Delete "$INSTDIR\*.json.tmp"
  RMDir "$INSTDIR"

  DeleteRegKey HKLM "${UNINST_KEY}"

  ; Silent uninstall keeps the data: nobody can answer the question, and deleting state.json, the
  ; token and the identity behind an operator's back is the worse default.
  ${IfNot} ${Silent}
    MessageBox MB_YESNO|MB_ICONQUESTION "Delete the device data in ${DATA_DIR}?$\r$\n$\r$\nThat directory holds state.json, the install token and the device identity; keeping it lets a reinstall resume where this one stopped." IDNO +2
    RMDir /r "${DATA_DIR}"
  ${EndIf}
SectionEnd
