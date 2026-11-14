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

function Wait-ServiceStopped {
  for ($i = 0; $i -lt 90; $i++) {
    $state = (sc.exe query SummriseLaunch | Out-String)
    if ($state -match "STOPPED") { return $true }
    Start-Sleep -Seconds 1
  }
  return $false
}

function Test-Health {
  try {
    $health = Invoke-RestMethod "http://127.0.0.1:$Port/api/health" -TimeoutSec 3
    return ($health.status -eq "ok")
  } catch { return $false }
}

function Restart-Launcher {
  # A launcher that gave up after three consecutive child failures stays alive with no device, and
  # `sc start` then answers 1056 without starting anything. The stop has to reach STOPPED, not just be
  # requested, and the start has to be followed by a health answer; otherwise retry with a fresh stop.
  for ($attempt = 1; $attempt -le 3; $attempt++) {
    sc.exe stop SummriseLaunch | Out-Null
    if (-not (Wait-ServiceStopped)) {
      Add-Content $logPath "attempt $attempt: the service did not reach STOPPED"
      continue
    }
    Wait-DeviceGone
    sc.exe start SummriseLaunch | Out-Null
    for ($i = 0; $i -lt 60; $i++) {
      Start-Sleep -Seconds 1
      if (Test-Health) { Add-Content $logPath "attempt $attempt: healthy"; return $true }
    }
    Add-Content $logPath "attempt $attempt: started but never healthy"
  }
  return $false
}

function Restore-Old {
  if ($hadDevice) { Copy-Item "$devicePath.rollback" $devicePath -Force }
  if ($hadSidecar) { Copy-Item "$sidecarPath.rollback" $sidecarPath -Force }
  if ($oldVersion) { Set-Content $currentPath -Value $oldVersion -NoNewline }
  if (-not (Restart-Launcher)) {
    Add-Content $logPath "the old pair is back but the service will not serve it; a reboot is required"
    throw "the old pair is back but the service will not serve it; a reboot is required"
  }
}

$logPath = Join-Path $env:TEMP "summrise-swap.log"
Add-Content $logPath "swapping $Version from $Artifact"
sc.exe stop SummriseLaunch | Out-Null
if (-not (Wait-ServiceStopped)) { Add-Content $logPath "the service did not reach STOPPED before the copy"; }
Wait-DeviceGone
Copy-Item $Artifact $devicePath -Force
Copy-Item $Sidecar $sidecarPath -Force
if ((Get-FileHash $Artifact).Hash -ne (Get-FileHash $devicePath).Hash) { Restore-Old; throw "the copied binary does not match its source" }
if ((Get-FileHash $Sidecar).Hash -ne (Get-FileHash $sidecarPath).Hash) { Restore-Old; throw "the copied sidecar does not match its source" }

if (-not (Restart-Launcher)) {
  Add-Content $logPath "the new payload never answered /api/health; restoring the old pair"
  Restore-Old
  throw "the new payload never answered /api/health; the old pair and pointer are back"
}
"swapped $Version and healthy on port $Port"
