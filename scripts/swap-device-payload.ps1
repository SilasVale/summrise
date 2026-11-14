# Swap the installed device payload (binary + signed sidecar) with a health-checked rollback.
#
# THE LESSON THIS FILE EXISTS FOR: a swap that only checks `sc start`'s exit code left a machine with
# no device and no tray icon, because the copy had raced the stopping process and the launcher refused
# the mismatched pair. This script waits for the old process to be gone, verifies the bytes it wrote,
# and puts the old pair + pointer back when the new one does not answer /api/health.
param(
  [Parameter(Mandatory = $true)] [string] $Install,
  [Parameter(Mandatory = $true)] [string] $Artifact,
  [Parameter(Mandatory = $true)] [string] $Sidecar,
  [string] $Version = "2.0.0",
  [int] $Port = 18090
)
$ErrorActionPreference = "Stop"
$versionDir = Join-Path $Install ("versions\" + $Version)
$currentPath = Join-Path $Install "current.txt"
$devicePath = Join-Path $versionDir "summrise-device.exe"
$sidecarPath = Join-Path $versionDir "summrise-device.json"

New-Item -ItemType Directory -Force $versionDir | Out-Null
$hadDevice = Test-Path $devicePath
$hadSidecar = Test-Path $sidecarPath
$oldVersion = if (Test-Path $currentPath) { (Get-Content $currentPath -Raw).Trim() } else { "" }
if ($hadDevice) { Copy-Item $devicePath "$devicePath.rollback" -Force }
if ($hadSidecar) { Copy-Item $sidecarPath "$sidecarPath.rollback" -Force }
Set-Content $currentPath -Value $Version -NoNewline

function Wait-DeviceGone {
  for ($i = 0; $i -lt 60; $i++) {
    if (-not (Get-Process summrise-device -ErrorAction SilentlyContinue)) { return }
    Start-Sleep -Seconds 1
  }
  Get-Process summrise-device -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep -Seconds 2
}

function Restart-Launcher {
  # A running launcher with a dead device answers `sc start` with 1056 and starts nothing — the
  # failure mode this script exists for. Stop first, wait for the process, then start.
  sc.exe stop SummriseLaunch | Out-Null
  Wait-DeviceGone
  sc.exe start SummriseLaunch | Out-Null
}

function Restore-Old {
  if ($hadDevice) { Copy-Item "$devicePath.rollback" $devicePath -Force }
  if ($hadSidecar) { Copy-Item "$sidecarPath.rollback" $sidecarPath -Force }
  if ($oldVersion) { Set-Content $currentPath -Value $oldVersion -NoNewline }
  Restart-Launcher
}

sc.exe stop SummriseLaunch | Out-Null
Wait-DeviceGone
Copy-Item $Artifact $devicePath -Force
Copy-Item $Sidecar $sidecarPath -Force
if ((Get-FileHash $Artifact).Hash -ne (Get-FileHash $devicePath).Hash) { Restore-Old; throw "the copied binary does not match its source" }
if ((Get-FileHash $Sidecar).Hash -ne (Get-FileHash $sidecarPath).Hash) { Restore-Old; throw "the copied sidecar does not match its source" }

Restart-Launcher
$healthy = $false
for ($i = 0; $i -lt 90; $i++) {
  Start-Sleep -Seconds 2
  try {
    $health = Invoke-RestMethod "http://127.0.0.1:$Port/api/health" -TimeoutSec 3
    if ($health.status -eq "ok") { $healthy = $true; break }
  } catch { }
}
if (-not $healthy) { Restore-Old; throw "the new payload never answered /api/health; the old pair and pointer are back" }
"swapped $Version and healthy on port $Port"
