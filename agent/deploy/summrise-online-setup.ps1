#Requires -Version 5.1
<#
  Summrise Agent 在线安装引导脚本（NSIS 安装包内嵌调用，也可手动运行）。
  只做最小引导，真正的安装复用 npm 通道本身：
    Node（复用现有的，缺失才下载便携版）→ npm i -g  pinned tgz →
    summrise setup → Electron → 桌面任务/快捷方式。
  更新通道不变：装完之后一律 `summrise update`。

  参数：
    -InstallDir  安装目录（默认 C:\Program Files\Summrise）
    -SummriseVersion 钉死的版本号（必填，如 1.2.306）
    -CdnBase    安装源（默认 https://agent.saisi.online）
    -RegKey     可选：网关注册码（自动登记设备）
    -Tunnel     可选：tunnel 主机名（summrise setup --tunnel）
    -ResultFile 安装结果回执（NSIS 完成页读取）
    -LocalTgz   可选：自带 tgz 路径（自包含安装包内嵌，优先用它，免下载）
    -ResultFile 安装结果回执（NSIS 完成页读取）
#>
param(
  [string]$InstallDir = "C:\Program Files\Summrise",
  [Parameter(Mandatory = $true)][string]$SummriseVersion,
  [string]$CdnBase = "https://agent.saisi.online",
  [string]$RegKey = "",
  [string]$Tunnel = "",
  [string]$ResultFile = "",
  [string]$DataDir = (Join-Path $env:ProgramData "Summrise"),
  [string]$LocalTgz = ""
)

$ErrorActionPreference = "Stop"
$ElectronVersion = "33.4.11"
$NodeFloorMajor = 18

function Say([string]$m) { Write-Host "[summrise-setup] $m" }

# --- admin 自检（NSIS 本来就要提权；手动跑脚本时给一句人话） ---
$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  Write-Host "[summrise-setup] 需要管理员权限：请右键“以管理员身份运行”。"
  exit 3
}
if (-not [Environment]::Is64BitOperatingSystem) {
  Write-Host "[summrise-setup] 仅支持 64 位 Windows。"
  exit 3
}
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $InstallDir "etc") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $InstallDir "components") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $InstallDir "scripts") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $DataDir "logs") | Out-Null
try { Start-Transcript -Path (Join-Path $DataDir "logs\installer.log") -Append | Out-Null } catch { }

# round-124: SHA-256 verification for the CDN fallback (shipped beside this file
# as lib\SummriseIntegrity.ps1; NSIS File + build-installer.sh both carry it).
. (Join-Path $PSScriptRoot "lib\SummriseIntegrity.ps1")

function Download-File([string]$url, [string]$dest, [string]$what) {
  # 主源一次 + 备用源一次；调用方决定失败是否致命。
  $mirrors = @($url)
  if ($url -like "https://nodejs.org/*") { $mirrors += $url -replace "https://nodejs.org", "https://npmmirror.com/mirrors/node" }
  if ($url -like "https://registry.npmjs.org/*") { $mirrors += $url -replace "https://registry.npmjs.org", "https://registry.npmmirror.com" }
  foreach ($u in $mirrors) {
    try {
      Say "下载 $what ..."
      Invoke-WebRequest -Uri $u -OutFile $dest -UseBasicParsing -TimeoutSec 120
      if ((Test-Path $dest) -and ((Get-Item $dest).Length -gt 0)) { return $true }
    } catch { Say "$what 下载失败（$u）：$($_.Exception.Message)" }
  }
  return $false
}

# --- 1. Node：复用现有的，版本太旧或缺失才装便携版 ---
# Layout v2: portable node lives at components\node.
$NodeDir = Join-Path $InstallDir "components\node"
$nodeExe = ""
try {
  $found = (Get-Command node -ErrorAction SilentlyContinue).Source
  if ($found) {
    $v = (& $found --version) 2>$null
    if ($v -match "v(\d+)\.") {
      if ([int]$Matches[1] -ge $NodeFloorMajor) { $nodeExe = $found; Say "复用系统 Node $v ($found)" }
      else { Say "系统 Node $v 太旧（要 >= v$NodeFloorMajor），装便携版" }
    }
  }
} catch { }
if (-not $nodeExe) {
  # 解析最新 LTS（nodejs.org 主，npmmirror 备；都挂则本机无网，直接结束）
  $lts = ""
  foreach ($idx in @("https://nodejs.org/dist/index.json", "https://npmmirror.com/mirrors/node/index.json")) {
    try { $lts = (Invoke-RestMethod -Uri $idx -UseBasicParsing -TimeoutSec 30 | Where-Object { $_.lts } | Select-Object -First 1).version; if ($lts) { break } } catch { }
  }
  if (-not $lts) { Write-Host "[summrise-setup] 拿不到 Node 版本列表（网络不通？），退出。"; exit 5 }
  Say "最新 LTS Node $lts"
  $zip = Join-Path $env:TEMP "summrise-node.zip"
  $zipUrl = "https://nodejs.org/dist/$lts/node-$lts-win-x64.zip"
  if (-not (Download-File $zipUrl $zip "Node $lts")) { Write-Host "[summrise-setup] Node 下载失败，退出。"; exit 5 }
  if (Test-Path $NodeDir) { Remove-Item -Recurse -Force $NodeDir }
  Expand-Archive -Force -Path $zip -DestinationPath (Join-Path $InstallDir "components")
  Move-Item (Join-Path $InstallDir "components\node-$lts-win-x64") $NodeDir -Force
  Remove-Item -Force $zip -ErrorAction SilentlyContinue
  $nodeExe = Join-Path $NodeDir "node.exe"
  Say "便携 Node 就绪：$nodeExe"
}
$nodeBinDir = Split-Path $nodeExe -Parent
# Layout v2: the npm global prefix lives at components\npm-global (was tools\).
$NpmGlobal = Join-Path $InstallDir "components\npm-global"
# 本机 PATH（新进程生效；summrise setup 的 where node 也能找到它）
try {
  $mp = [Environment]::GetEnvironmentVariable("Path", "Machine")
  foreach ($p in @($nodeBinDir, $NpmGlobal)) {
    if ($mp -notlike "*$p*") { $mp = "$mp;$p" }
  }
  [Environment]::SetEnvironmentVariable("Path", $mp, "Machine")
} catch { Say "写 Machine PATH 失败（继续，当前会话 PATH 已就绪）" }
$env:Path = "$nodeBinDir;$NpmGlobal;$env:Path"
$npmCmd = Join-Path $nodeBinDir "npm.cmd"
if (-not (Test-Path $npmCmd)) { Write-Host "[summrise-setup] 找不到 npm.cmd（$nodeBinDir），退出。"; exit 5 }

# --- 2. npm 装 pinned 的 summrise-agent（这就是以后的更新通道） ---
# 自包含安装包优先用内嵌 tgz（LocalTgz，NSIS 打包时 File 进去）——无网络、
# 被 prune 的老版本照样能装；手动跑脚本时缺省走 CDN 下载（原逻辑不变）。
$tgz = "$CdnBase/summrise-agent/summrise-agent-$SummriseVersion.tgz"
$tgzSource = "cdn"
if ($LocalTgz -and (Test-Path $LocalTgz)) {
  $tgz = $LocalTgz
  $manifestJson = ""
$tgzSource = "bundled"
  Say "使用自带安装包 $SummriseVersion（免下载）..."
} else {
  if ($LocalTgz) { Say "自带包缺失（$LocalTgz），回退 CDN 下载..." }
  Say "安装 summrise-agent $SummriseVersion ..."

  # 校验后才装（round-124）。这条路从网络取的是**要执行的代码**，而 /api/version
  # 本来就带正确摘要，其它消费者（agent_update）也一律拒绝未校验的字节；这里以前
  # 什么都没查就交给 npm。自带包那条路不需要查（它来自已签名的安装包本身）。
  $dl = Join-Path $env:TEMP "summrise-agent-$SummriseVersion.tgz"
  if (-not (Download-File $tgz $dl "summrise-agent $SummriseVersion")) {
    Write-Host "[summrise-setup] 下载失败（$tgz），退出。"; exit 6
  }
  $manifestUrl = "$CdnBase/api/version"
  # Kept for the component blocks below, which run on BOTH branches and need the same pins.
  $expected = ""
  try {
    $manifest = (Invoke-WebRequest -Uri $manifestUrl -UseBasicParsing -TimeoutSec 60).Content
    $expected = Get-ManifestSha256 -ManifestJson $manifest -Version $SummriseVersion
    $manifestJson = $manifest
  } catch { Say "读取版本清单失败（$manifestUrl）：$($_.Exception.Message)" }
  if (-not (Test-FileSha256 -Path $dl -Expected $expected)) {
    Write-Host "[summrise-setup] 校验失败：下载的 tgz 与版本清单的 sha256 不符（或清单缺少该版本的摘要），拒绝安装。"
    Write-Host "  清单：$manifestUrl"
    Remove-Item $dl -ErrorAction SilentlyContinue
    exit 7
  }
  Say "sha256 校验通过（$expected）"
  $tgz = $dl
}
& $npmCmd install -g --prefix $NpmGlobal $tgz
if ($LASTEXITCODE -ne 0) { Write-Host "[summrise-setup] npm 安装失败，退出。"; exit 6 }
$summriseCmd = Join-Path $NpmGlobal "summrise.cmd"
if (-not (Test-Path $summriseCmd)) { Write-Host "[summrise-setup] summrise.cmd 没生成，退出。"; exit 6 }

# THE MANIFEST IS READ HERE ON PURPOSE, not only on the CDN branch above (round 143).
# These two components are staged by PRESENCE into the npm package dir, and
# `summrise setup` takes them by presence in turn (resolveComponent), so THIS is the
# only place their pins can be consulted at all. The tgz was already verified;
# these — 54 MB and 31 MB of executable payload — were accepted on `Length -gt 1MB`.
# A component that fails verification is DELETED, not skipped in place: setup then
# fetches it through its own verified path, which is the better failure.
if (-not $manifestJson) {
  try { $manifestJson = (Invoke-WebRequest -Uri "$CdnBase/api/version" -UseBasicParsing -TimeoutSec 60).Content } catch { $manifestJson = "" }
}

# --- 3. cloudflared：能带上就带上（setup 会 stage 进 tools/）；失败不致命 ---
try {
  $pkgDir = Join-Path $NpmGlobal "node_modules\summrise-agent"
  $cfDest = Join-Path $pkgDir "cloudflared.exe"
  if (-not (Test-Path $cfDest)) {
    Download-File "$CdnBase/summrise-agent/cloudflared.exe" $cfDest "cloudflared" | Out-Null
  }
  if (Test-Path $cfDest) {
    # Verified whether or not we just downloaded it: a file already here came from a
    # previous run and setup takes it by presence (see the playwright note below).
    $wantCf = Get-ComponentSha256 -ManifestJson $manifestJson -Name "cloudflared"
    if (Test-FileSha256 -Path $cfDest -Expected $wantCf) {
      Say "cloudflared 已随包（sha256 校验通过）"
    } else {
      Remove-Item -Force $cfDest -ErrorAction SilentlyContinue
      Say "cloudflared 校验失败（清单缺少该组件的摘要，或字节不符）→ 已丢弃，安装后由 summrise setup 走校验过的路径取"
    }
  } else {
    Say "cloudflared 跳过（以后用 tunnel 时 agent 会自己拉）"
  }
} catch { Say "cloudflared 跳过：$($_.Exception.Message)" }

# --- 3b. playwright 浏览器工具包（约 30MB，走我们的 CDN；失败不致命） ---
# 装进 npm 包目录，随后 `summrise setup` 会 Expand-Archive 到 components\playwright
# 并启用 browser_* 工具。CDN 代理路由从 R2 取（30MB 超 Workers 静态资源 25MiB
# 上限，不能进 tgz）。npmjs 在很多设备上不可达，所以走我们自己这个源。
try {
  $pkgDir = Join-Path $NpmGlobal "node_modules\summrise-agent"
  $pwDest = Join-Path $pkgDir "summrise-playwright.zip"
  if (-not (Test-Path $pwDest)) {
    Say "下载 playwright 浏览器工具包（约 30MB，走我们的 CDN）..."
    try {
      $ProgressPreference = "SilentlyContinue"
      Invoke-WebRequest -Uri "$CdnBase/summrise-agent/summrise-playwright.zip" -OutFile $pwDest -UseBasicParsing -TimeoutSec 600
    } catch { Say "playwright 下载失败：$($_.Exception.Message)" }
  }
  # VERIFIED WHETHER OR NOT WE JUST DOWNLOADED IT, and that is the point rather than a
  # detail: a file already at this path was staged by a PREVIOUS run, possibly by the
  # installer from before this check existed, and `summrise setup` takes it by presence.
  # SIZE IS NOT A VERDICT: the acceptance used to be "> 1MB", which accepts any 31 MB.
  $wantPw = Get-ComponentSha256 -ManifestJson $manifestJson -Name "playwright"
  if ((Test-Path $pwDest) -and (Test-FileSha256 -Path $pwDest -Expected $wantPw)) {
    Say "playwright 已随包（sha256 校验通过；浏览器工具将启用）"
  } elseif (Test-Path $pwDest) {
    Remove-Item -Force $pwDest -ErrorAction SilentlyContinue
    Say "playwright 校验失败（清单缺少该组件的摘要，或字节不符）→ 已丢弃，浏览器工具改由 summrise setup 走校验过的路径取"
  } else {
    Say "playwright 跳过（浏览器工具不可用，可稍后补）"
  }
} catch { Say "playwright 跳过：$($_.Exception.Message)" }

# --- 4. summrise setup（目录/注册表/任务/防火墙/注册全是它做） ---
$env:SUMMRISE_AGENT_DIR = $InstallDir
$setupArgs = @("setup")
if ($RegKey) { $setupArgs += @("--reg-key", $RegKey) }
if ($Tunnel) { $setupArgs += @("--tunnel", $Tunnel) }
Say "运行 summrise setup ..."
& $summriseCmd @setupArgs
if ($LASTEXITCODE -ne 0) { Write-Host "[summrise-setup] summrise setup 失败，退出。"; exit 7 }

# --- 5. Electron（桌面壳二进制；主源=我们的 CDN，备源=npm；失败只告警） ---
# Layout v2: the shell lives at components\summrise-desktop-electron.
# The shell is launched as `electron .` from node_modules\electron\dist.
$shellDir = Join-Path $InstallDir "components\summrise-desktop-electron"
$distDir = Join-Path $shellDir "node_modules\electron\dist"
$electronExe = Join-Path $distDir "electron.exe"
$electronOk = Test-Path $electronExe
# PRIMARY: pull the win32-x64 dist zip from our own CDN (index worker proxies
# it from GitHub). npmjs/npmmirror are unreachable on many device boxes — that
# is exactly why fresh installs shipped a desktop shell with no Electron.
if (-not $electronOk) {
  New-Item -ItemType Directory -Force -Path $distDir | Out-Null
  $zip = Join-Path $env:TEMP "summrise-electron-win32-x64.zip"
  Say "下载 Electron $ElectronVersion（约 115MB，走我们的 CDN）..."
  $got = $false
  try {
    $ProgressPreference = "SilentlyContinue"
    Invoke-WebRequest -Uri "$CdnBase/summrise-agent/electron-win32-x64.zip" -OutFile $zip -UseBasicParsing -TimeoutSec 900
    # VERIFIED, NOT MEASURED (round 143). The acceptance here was "> 1MB", and what follows
    # EXPANDS this zip and copies its contents into dist\\ — 115 MB of executable payload
    # installed on the strength of a file size. The manifest pins this component; ask it.
    $wantEl = Get-ComponentSha256 -ManifestJson $manifestJson -Name "electron"
    if ((Test-Path $zip) -and (Test-FileSha256 -Path $zip -Expected $wantEl)) {
      $got = $true
    } elseif (Test-Path $zip) {
      Say "Electron 校验失败（清单缺少该组件的摘要，或字节不符）→ 已丢弃，桌面外壳这一点不启用"
      Remove-Item -Force $zip -ErrorAction SilentlyContinue
    }
  } catch { Say "Electron CDN 下载失败：$($_.Exception.Message)" }
  if ($got) {
    try {
      $tmpx = Join-Path $env:TEMP "summrise-electron-x"
      if (Test-Path $tmpx) { Remove-Item -Recurse -Force $tmpx }
      Expand-Archive -Force -Path $zip -DestinationPath $tmpx
      # The zip root holds electron.exe + *.dll + resources\ + locales\ —
      # copy its CONTENTS into dist\ (so dist\electron.exe exists).
      Copy-Item -Path (Join-Path $tmpx "*") -Destination $distDir -Recurse -Force
      Remove-Item -Recurse -Force $tmpx -ErrorAction SilentlyContinue
      Remove-Item -Force $zip -ErrorAction SilentlyContinue
    } catch { Say "Electron 解压失败：$($_.Exception.Message)" }
  }
  $electronOk = Test-Path $electronExe
}
# FALLBACK: npm (only works where the registry is reachable). Needs the shell
# package.json (now written by `summrise setup`); kept for boxes that can reach
# npm but where the CDN zip somehow failed.
if (-not $electronOk -and (Test-Path (Join-Path $shellDir "package.json"))) {
  Push-Location $shellDir
  try {
    Say "回退：npm 安装 Electron $ElectronVersion ..."
    & $npmCmd install --no-save "electron@$ElectronVersion" 2>&1 | Select-Object -Last 2
    if (-not (Test-Path "node_modules\electron\dist\electron.exe")) {
      $env:ELECTRON_MIRROR = "https://npmmirror.com/mirrors/electron/"
      & $npmCmd install --no-save "electron@$ElectronVersion" --registry=https://registry.npmmirror.com 2>&1 | Select-Object -Last 2
    }
  } catch { Say "Electron 安装异常：$($_.Exception.Message)" }
  Pop-Location
  $electronOk = Test-Path $electronExe
}
if ($electronOk) { Say "Electron 就绪" } else { Say "警告：Electron 没装上（桌面壳跑不起来，agent 本体不受影响；可稍后手动 npm 装）" }

# --- 6. SummriseDesktop 登录任务（总是重建 = 顺带修复旧版指向；形态抄 update 流的 hardened 版） ---
# Layout v2: supervisor scripts live in scripts\. The three launcher files
# are written UNCONDITIONALLY (idempotent refresh on re-run — the old code
# only wrote them inside the create-task branch, so a repair run left stale
# or missing launchers): start-desktop.ps1 (electron entry), the guarded
# ensure-desktop.ps1 pulse, and its console-less VBS wrapper.
try {
  $sdPath = Join-Path $InstallDir "scripts\start-desktop.ps1"
  $sdLines = @(
    ("`$dir = '" + $shellDir.Replace("'","''") + "'"),
    'Set-Location $dir',
    '& "$dir\node_modules\electron\dist\electron.exe" .'
  )
  Set-Content -Path $sdPath -Value $sdLines -Encoding ASCII
  $en1 = Join-Path $InstallDir "scripts\ensure-desktop.ps1"
  $vb1 = Join-Path $InstallDir "scripts\desktop-pulse.vbs"
  Set-Content -Path $en1 -Value ('if (Get-Process electron -ErrorAction SilentlyContinue) { exit }; & powershell -NoProfile -ExecutionPolicy Bypass -File "' + (Join-Path $InstallDir "scripts\start-desktop.ps1") + '"') -Force
  Set-Content -Path $vb1 -Value ('CreateObject("WScript.Shell").Run "powershell -NoProfile -ExecutionPolicy Bypass -File " & Chr(34) & "' + (Join-Path $InstallDir "scripts\ensure-desktop.ps1") + '" & Chr(34), 0, False') -Force
  # ALWAYS re-register (Register -Force overwrites an existing definition).
  # The old create-if-absent logic left a pre-layout-v2 task pointing at the
  # retired root "D:\Summrise\desktop-pulse.vbs" — it fired every 5 min and popped
  # "Windows Script Host: 无法找到脚本文件". Re-registering heals the action to
  # the scripts\ path on every install/repair.
  $da = New-ScheduledTaskAction -Execute "wscript.exe" -Argument ('"' + $vb1 + '"') -WorkingDirectory $InstallDir
  $dt1 = New-ScheduledTaskTrigger -AtLogOn
  $dw1 = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(3) -RepetitionInterval (New-TimeSpan -Minutes 5)
  # THE PRINCIPAL AND THE SETTINGS THE COMMENT ABOVE ALREADY CLAIMED (round 143).
  # It said the shape was "copied from the update flow's hardened version" and the body
  # passed NEITHER: no -Principal (the CLI writes LogonType Interactive, RunLevel
  # Highest) and no -Settings (a 10-minute ExecutionTimeLimit and IgnoreNew).
  # ORDER IS WHY THIS MATTERS: step 6 runs AFTER `summrise setup`, so on the NSIS path
  # THIS registration is the one that survives — the weaker definition was overwriting
  # the hardened one on every install. Same shape as agent/summrise-agent-npm's register
  # step, and installer_integrity.rs fails if the two ever diverge again.
  $pr = New-ScheduledTaskPrincipal -UserId ('{0}\{1}' -f $env:USERDOMAIN, $env:USERNAME) -LogonType Interactive -RunLevel Highest
  $st = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 10) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -MultipleInstances IgnoreNew
  Register-ScheduledTask SummriseDesktop -Action $da -Trigger @($dt1, $dw1) -Principal $pr -Settings $st -Force | Out-Null
  Say "SummriseDesktop 登录任务已就绪（含修复旧版指向）"
  Start-ScheduledTask -TaskName "SummriseDesktop" -ErrorAction SilentlyContinue
} catch { Say "SummriseDesktop 任务跳过（行 $($_.InvocationInfo.ScriptLineNumber)）：$($_.Exception.Message)" }

# --- 7. 桌面快捷方式（公共桌面 + 当前用户桌面；目标早已不是那个删掉的旧壳） ---
try {
  $ws = New-Object -ComObject WScript.Shell
  $ico = Join-Path $shellDir "icon.ico"
  foreach ($desk in @([Environment]::GetFolderPath("CommonDesktopDirectory"), [Environment]::GetFolderPath("Desktop"))) {
    if (-not $desk) { continue }
    $lnk = Join-Path $desk "Summrise.lnk"
    $s = $ws.CreateShortcut($lnk)
    $s.TargetPath = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"
    $s.Arguments = '-NoProfile -ExecutionPolicy Bypass -File "' + (Join-Path $InstallDir "scripts\start-desktop.ps1") + '"'
    $s.WorkingDirectory = $shellDir
    if (Test-Path $ico) { $s.IconLocation = "$ico,0" }
    $s.Save()
  }
  Say "桌面快捷方式就绪"
} catch { Say "快捷方式跳过：$($_.Exception.Message)" }

# --- 8. 回执（NSIS 完成页读这个；绝不写 token） ---
# Layout v2: config in etc\, receipt in DataDir\logs\.
$port = "18080"
try { $p = Select-String -Path (Join-Path $InstallDir "etc\config.yaml") -Pattern "^\s*port:\s*(\d+)" | Select-Object -First 1; if ($p -and $p.Matches.Groups[1].Value) { $port = $p.Matches.Groups[1].Value } } catch { }
$lines = @(
  "DONE Summrise Agent $SummriseVersion 安装完成",
  "面板： http://127.0.0.1:$port/desktop/",
  "目录： $InstallDir",
  ("桌面壳 Electron：" + ($(if ($electronOk) { "就绪" } else { "未装上（见上方警告）" })))
)
if (-not $ResultFile) { $ResultFile = Join-Path $DataDir "logs\install-result.txt" }
# UTF-16LE (PS 5.1 spells it -Encoding Unicode) because the NSIS finish page
# reads this file with FileReadUTF16LE (which skips the BOM). It used to be
# -Encoding UTF8: PS 5.1 writes UTF-8 WITH a BOM, NSIS's plain ANSI FileRead
# then decoded those bytes as the system codepage (GBK) and the completion
# page showed mojibake (鈥?…).
$lines | Set-Content -Path $ResultFile -Encoding Unicode
# Companion ASCII one-liner holding ONLY the panel URL: the finish page offers
# "open the panel" and reads it with plain FileRead — pure ASCII, so there is
# no encoding question at all.
try {
  Set-Content -Path (Join-Path (Split-Path $ResultFile) "install-panel-url.txt") -Value "http://127.0.0.1:$port/desktop/" -Encoding ASCII
} catch { }
$lines | ForEach-Object { Say $_ }
# 自包含包的内嵌 tgz 用完即删（~6MB 死重；重跑安装包会重新解压出来）。
# 只删"自带的那一份"（LocalTgz 指向的文件），不动用户手里的东西。
if (($tgzSource -eq "bundled") -and $LocalTgz) {
  try { Remove-Item -Force -ErrorAction Stop $LocalTgz; Say "内嵌安装包已清理" } catch { Say "内嵌安装包清理跳过（不影响使用）" }
}
try { Stop-Transcript | Out-Null } catch { }
exit 0
