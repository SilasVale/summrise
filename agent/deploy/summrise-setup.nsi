; Summrise Agent 在线安装包（NSIS 3.x, Unicode）。
; 由 scripts/build-installer.sh 编译：
;   makensis /DSUMMRISE_VERSION=1.2.N /DSUMMRISE_CDN=https://agent.saisi.online summrise-setup.nsi
; 只做最小外壳：中文向导 + 管理员提权 + 卸载器；真正的安装由内嵌的
; summrise-online-setup.ps1 完成（Node 引导 → npm 通道 → summrise setup）。
; 自包含：pinned tgz（summrise-agent-${SUMMRISE_VERSION}.tgz）File 进包，
; ps1 优先用它（-LocalTgz），无网络/老版本被 prune 照样能装；
; 装完即删，不留死重。更新通道不变：装完一律 `summrise update`。
Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "nsDialogs.nsh"
!include "StrFunc.nsh"
${StrRep}

!ifndef SUMMRISE_VERSION
  !define SUMMRISE_VERSION "0.0.0-dev"
!endif
!ifndef SUMMRISE_CDN
  !define SUMMRISE_CDN "https://agent.saisi.online"
!endif

Name "Summrise Agent ${SUMMRISE_VERSION}"
OutFile "SummriseAgent-Setup-${SUMMRISE_VERSION}.exe"
InstallDir "$PROGRAMFILES\Summrise"
RequestExecutionLevel admin

; MUI2 owns the Icon call: a bare `Icon` gets overridden by MUI's default
; (modern-install.ico) at MUI_LANGUAGE time — MUI_ICON/MUI_UNICON are the
; supported hooks and must precede the page macros.
!define MUI_ICON "summrise-agent.ico"
!define MUI_UNICON "summrise-agent.ico"

; 品牌头图（scripts/render-installer-art.py 生成，日出主题）
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_BITMAP "res\header.bmp"
!define MUI_WELCOMEFINISHPAGE_BITMAP "res\welcome.bmp"
!define MUI_ABORTWARNING

Var RESULT_TEXT
Var PANEL_URL
Var PANEL_CHK

; StrFunc 函数落子（全局作用域；卸载节用 Un 变体）
${UnStrRep}

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY

!insertmacro MUI_PAGE_INSTFILES

; 完成页：读引导脚本写的回执（布局 v2 在 %ProgramData%\Summrise\logs\；
; ReadEnvStr 读进程环境，不受 ShellVarContext 影响；绝不含 token）
; 回执是 UTF-16LE（ps1 用 -Encoding Unicode 写），必须用 FileReadUTF16LE 读
; —— 之前用 ANSI 的 FileRead 去读 UTF-8(带 BOM) 的字节，中文全成乱码。
Page custom finishPage finishPageLeave
Function finishPage
  ReadEnvStr $3 "ProgramData"
  StrCpy $RESULT_TEXT "安装程序已退出。请用 summrise status 查看状态，或重新运行安装。"
  ${If} ${FileExists} "$3\Summrise\logs\install-result.txt"
    StrCpy $RESULT_TEXT ""
    FileOpen $4 "$3\Summrise\logs\install-result.txt" r
    ${If} $4 != ""
      ${Do}
        ClearErrors
        FileReadUTF16LE $4 $5
        ${If} ${Errors}
          ${Break}
        ${EndIf}
        StrCpy $RESULT_TEXT "$RESULT_TEXT$5$\r$\n"
      ${Loop}
      FileClose $4
    ${EndIf}
  ${EndIf}
  ; 纯 ASCII 的一行 URL（ps1 另写一个文件）——完成页据此提供"打开面板"。
  StrCpy $PANEL_URL ""
  ${If} ${FileExists} "$3\Summrise\logs\install-panel-url.txt"
    FileOpen $4 "$3\Summrise\logs\install-panel-url.txt" r
    ${If} $4 != ""
      FileRead $4 $PANEL_URL
      FileClose $4
    ${EndIf}
  ${EndIf}
  nsDialogs::Create 1018
  Pop $0
  ${NSD_CreateLabel} 0 0 100% 150u "$RESULT_TEXT"
  Pop $0
  StrCpy $PANEL_CHK ""
  ${If} $PANEL_URL != ""
    ${NSD_CreateCheckbox} 0 158u 100% 12u "安装完成后打开 Summrise 面板"
    Pop $PANEL_CHK
    ${NSD_SetState} $PANEL_CHK ${BST_CHECKED}
  ${EndIf}
  nsDialogs::Show
FunctionEnd

Function finishPageLeave
  ${If} $PANEL_CHK != ""
    ${NSD_GetState} $PANEL_CHK $0
    ${If} $0 == ${BST_CHECKED}
      ExecShell "open" "$PANEL_URL"
    ${EndIf}
  ${EndIf}
FunctionEnd

!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

Section "Install" SEC01
  SetOutPath "$INSTDIR"
  ; 引导脚本 + 版本钉死（装 pinned tgz，不装 latest，保证可复现）。
  ; 布局 v2：引导脚本进 scripts\（根目录只留 exe + 卸载器）；
  ; 回执/日志走 %ProgramData%\Summrise\logs\（与 ps1 的 -DataDir 默认一致）。
  ; 自包含：pinned tgz 一起 File 进来，ps1 优先用 -LocalTgz 装。
  SetOutPath "$INSTDIR\scripts"
  File "summrise-online-setup.ps1"
  ; round-124: the installer dot-sources this (SHA-256 verification for the CDN
  ; fallback), so it MUST ship beside it. Forgetting a File line here breaks the
  ; installed flow at RUNTIME on the user's machine, which no test on this box
  ; can see — which is why the coupling is pinned in agent/tests/.
  SetOutPath "$INSTDIR\scripts\lib"
  File "lib\SummriseIntegrity.ps1"
  SetOutPath "$INSTDIR\scripts"
  File "summrise-agent-${SUMMRISE_VERSION}.tgz"
  SetOutPath "$INSTDIR"
  ReadEnvStr $3 "ProgramData"
  ; THE FAILURE DIALOG NAMES THE LOG THE TRANSCRIPT ACTUALLY GOES TO (round 143). It said
  ; $INSTDIR\installer.log, which never exists: summrise-online-setup.ps1 writes its transcript to
  ; $DataDir\logs\installer.log, and $DataDir is %ProgramData%\Summrise. The path below is built
  ; from $3, which line 127 already reads for -ResultFile. Start-Transcript's failure is swallowed,
  ; so the wrong path also meant the operator could be sent to nothing at all.
  ; THE UNINSTALLER IS WRITTEN BEFORE THE STEP THAT CAN FAIL (round 147). It used to be
  ; written after the `${If} $0 != 0` / Abort below, so a FAILED install left everything the
  ; setup script had already done — Machine PATH among it — with NO uninstaller and no
  ; Add/Remove entry: a half-installed product with no supported way to remove it. The
  ; dialog's advice (re-run; it is idempotent) is true, but it is not the only thing an
  ; operator may want to do. The uninstall section tolerates a partial install, because
  ; every Delete/RMDir it performs is already tolerant of absence.
  ; ORDER IS THE GUARANTEE: installer_integrity.rs fails if this is ever moved back.
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\SummriseAgent" "DisplayName" "Summrise Agent ${SUMMRISE_VERSION}"
  WriteRegStr HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\SummriseAgent" "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\SummriseAgent" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\SummriseAgent" "DisplayVersion" "${SUMMRISE_VERSION}"
  WriteRegStr HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\SummriseAgent" "Publisher" "Summrise"
  WriteRegDWORD HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\SummriseAgent" "NoModify" 1
  WriteRegDWORD HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\SummriseAgent" "NoRepair" 1

  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\scripts\summrise-online-setup.ps1" -InstallDir "$INSTDIR" -SummriseVersion "${SUMMRISE_VERSION}" -CdnBase "${SUMMRISE_CDN}" -ResultFile "$3\Summrise\logs\install-result.txt" -LocalTgz "$INSTDIR\scripts\summrise-agent-${SUMMRISE_VERSION}.tgz"'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "安装失败（步骤退出码 $0）。$\r$\n看 $3\Summrise\logs\installer.log 找原因，修好后重跑安装包即可（幂等）。"
    Abort
  ${EndIf}

SectionEnd

Section "Uninstall"
  ; 正主：summrise uninstall（停任务、杀进程、删程序目录+注册表；数据默认保留）
  ; 布局 v2：summrise.cmd 在 components\npm-global\；tools\ 是迁移前残留，兼容。
  ${If} ${FileExists} "$INSTDIR\components\npm-global\summrise.cmd"
    nsExec::ExecToLog 'cmd /c "set SUMMRISE_AGENT_DIR=$INSTDIR && "$INSTDIR\components\npm-global\summrise.cmd" uninstall"'
  ${ElseIf} ${FileExists} "$INSTDIR\tools\npm-global\summrise.cmd"
    nsExec::ExecToLog 'cmd /c "set SUMMRISE_AGENT_DIR=$INSTDIR && "$INSTDIR\tools\npm-global\summrise.cmd" uninstall"'
  ${Else}
    nsExec::ExecToLog 'cmd /c "schtasks /End /TN SummriseAgent 2>NUL & schtasks /Delete /TN SummriseAgent /F 2>NUL & schtasks /End /TN SummriseDesktop 2>NUL & schtasks /Delete /TN SummriseDesktop /F 2>NUL & taskkill /F /IM summrise-agent.exe 2>NUL & taskkill /F /IM electron.exe 2>NUL"'
    nsExec::ExecToLog 'cmd /c "rmdir /s /q "$INSTDIR" 2>NUL & reg delete HKLM\SOFTWARE\Summrise\Agent /f 2>NUL"'
  ${EndIf}
  ; 快捷方式（安装时写了公共桌面 + 当前用户桌面）
  SetShellVarContext all
  Delete "$DESKTOP\Summrise.lnk"
  SetShellVarContext current
  Delete "$DESKTOP\Summrise.lnk"
  ; 清掉安装时加的 Machine PATH（便携 node + npm-global；系统自带的 node 不动）。
  ; 布局 v2 在 components\ 下；tools\ 条目是迁移前版本的残留，一并清掉。
  ReadRegStr $0 HKLM "SYSTEM\CurrentControlSet\Control\Session Manager\Environment" "Path"
  ${UnStrRep} $0 $0 "$INSTDIR\components\node;" ""
  ${UnStrRep} $0 $0 ";$INSTDIR\components\node" ""
  ${UnStrRep} $0 $0 "$INSTDIR\components\npm-global;" ""
  ${UnStrRep} $0 $0 ";$INSTDIR\components\npm-global" ""
  ${UnStrRep} $0 $0 "$INSTDIR\tools\node;" ""
  ${UnStrRep} $0 $0 ";$INSTDIR\tools\node" ""
  ${UnStrRep} $0 $0 "$INSTDIR\tools\npm-global;" ""
  ${UnStrRep} $0 $0 ";$INSTDIR\tools\npm-global" ""
  WriteRegExpandStr HKLM "SYSTEM\CurrentControlSet\Control\Session Manager\Environment" "Path" $0
  Delete "$INSTDIR\uninstall.exe"
  RMDir /r "$INSTDIR"
  DeleteRegKey HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\SummriseAgent"
SectionEnd
