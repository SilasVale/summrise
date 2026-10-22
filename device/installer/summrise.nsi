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
  nsExec::Exec 'sc.exe stop ${SERVICE}'
  Pop $0
  ; `--install-service`/`--uninstall-service` always act on the launcher's default service name;
  ; only `service --service-name` selects another one for the SCM's own argv.
  nsExec::ExecToStack '"$INSTDIR\summrise-launch.exe" --uninstall-service'
  Pop $0
  Pop $1
FunctionEnd

Section "Summrise runtime" SecRuntime
  SectionIn RO
  SetShellVarContext all
  ; NSIS is a 32-bit process: without this, HKLM writes land under WOW6432Node and the
  ; "Apps & features" entry is the 32-bit one, not the machine's.
  SetRegView 64
  SetOutPath "$INSTDIR"
  File /oname=summrise-launch.exe "${SOURCE}\summrise-launch.exe"
  File /oname=summrise-device.exe "${SOURCE}\summrise-device.exe"
  File /oname=summrise-console.exe "${SOURCE}\summrise-console.exe"
  File /oname=summrise-device.sidecar.json "${SOURCE}\summrise-device.sidecar.json"
  WriteUninstaller "$INSTDIR\uninstall.exe"

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
  FileOpen $9 "$TEMP\summrise-install.log" w
  FileWrite $9 "summrise ${VERSION} install$\r$\n"
  FileWrite $9 "install: $INSTDIR$\r$\n"
  FileWrite $9 "data: ${DATA_DIR}$\r$\n"
  FileWrite $9 "port: ${PORT}$\r$\n"
  ; DIAGNOSTIC: the uninstall entry is written before this log opens; read it back so the log says
  ; whether the write landed and in which view.
  ReadRegStr $8 HKLM "${UNINST_KEY}" "DisplayVersion"
  FileWrite $9 "registry readback: '$8' (view 64)$\r$\n"

  nsExec::ExecToStack '"$INSTDIR\summrise-launch.exe" --install-service --replace-baseline --install "$INSTDIR" --data "${DATA_DIR}" --seed-sidecar "$INSTDIR\summrise-device.sidecar.json" --port ${PORT}'
  Pop $0
  Pop $1
  FileWrite $9 "install-service: exit $0$\r$\n$1$\r$\n"
  ${If} $0 != "0"
    FileClose $9
    ${If} ${Silent}
      SetErrorLevel 1
    ${Else}
      MessageBox MB_ICONSTOP "The device runtime could not be installed (exit $0).$\r$\n$\r$\n$1"
    ${EndIf}
    Abort
  ${EndIf}

  ; `--install-service` starts the service when it is not running; a second start then answers 1056
  ; ("already running"), which is success, not a warning.
  nsExec::ExecToStack 'sc.exe start ${SERVICE}'
  Pop $0
  Pop $1
  FileWrite $9 "sc start: exit $0$\r$\n$1$\r$\n"

  ; **THE INSTALL IS NOT DONE UNTIL THE DEVICE ANSWERS.** A registered service that cannot bind its
  ; port (the legacy agent holds 18080 by default) looks installed and serves nothing. Poll the
  ; health endpoint the app exposes; the installer's verdict is the device's own answer.
  StrCpy $7 "0"
  ${Do}
    nsExec::ExecToStack 'curl.exe -sf -m 2 http://127.0.0.1:${PORT}/api/health'
    Pop $0
    Pop $1
    ${If} $0 == "0"
      StrCpy $7 "1"
    ${Else}
      Sleep 1000
    ${EndIf}
  ${LoopUntil} $7 == "1"
  ${If} $7 != "1"
    FileWrite $9 "health: no answer on port ${PORT} after 10s$\r$\n"
    FileClose $9
    ${If} ${Silent}
      SetErrorLevel 1
    ${Else}
      MessageBox MB_ICONSTOP "The service is registered, but the device does not answer on port ${PORT}.$\r$\n$\r$\nAnother program may be using that port (the pre-1.3 agent holds 18080). Stop it or rebuild the installer with another port."
    ${EndIf}
    Abort
  ${EndIf}
  FileWrite $9 "health: ok on port ${PORT}$\r$\n"
  FileClose $9
SectionEnd

Section "Uninstall"
  SetShellVarContext all
  SetRegView 64
  nsExec::ExecToStack '"$INSTDIR\summrise-launch.exe" --uninstall-service'
  Pop $0
  Pop $1

  Delete "$INSTDIR\summrise-launch.exe"
  Delete "$INSTDIR\summrise-device.exe"
  Delete "$INSTDIR\summrise-console.exe"
  Delete "$INSTDIR\summrise-device.sidecar.json"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKLM "${UNINST_KEY}"

  ; Silent uninstall keeps the data: nobody can answer the question, and deleting state.json, the
  ; token and the identity behind an operator's back is the worse default.
  ${IfNot} ${Silent}
    MessageBox MB_YESNO|MB_ICONQUESTION "Delete the device data in ${DATA_DIR}?$\r$\n$\r$\nThat directory holds state.json, the install token and the device identity; keeping it lets a reinstall resume where this one stopped." IDNO +2
    RMDir /r "${DATA_DIR}"
  ${EndIf}
SectionEnd
