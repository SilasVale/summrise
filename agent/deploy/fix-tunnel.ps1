# fix-tunnel.ps1 — repair the cloudflared tunnel config after a Summrise Agent
# migration. The old install used a tunnel named summrise-command-dN with the
# *.command.saisi.online hostname; the new install created summrise-agent-dN with
# *.agent.saisi.online. If the agent-owned tunnel.yml still references the
# old tunnel or hostname, rewrite it (single location — no user/systemprofile
# copies) and restart cloudflared.
#
# Idempotent: a config already on the new tunnel/hostname is untouched.
$ErrorActionPreference = "Stop"

# Find the new agent tunnel (summrise-agent-dN) by DNS probe of the device hostname.
$hostFile = "C:\summrise-agent\summrise-agent.hostname"
if (-not (Test-Path $hostFile)) { $hostFile = "D:\summrise-command\summrise-agent.hostname" }
if (-not (Test-Path $hostFile)) { $hostFile = "D:\summrise-agent\summrise-agent.hostname" }
$hostname = ""
if (Test-Path $hostFile) { $hostname = (Get-Content $hostFile -Raw).Trim() }
if (-not $hostname) { $hostname = "d1.agent.saisi.online" }
$tunnelName = "summrise-agent-" + ($hostname -split '\.')[0]

# cloudflared is BOXED — the agent spawns exactly one copy with --config tunnel.yml.
# WHERE it is boxed changed, and this script was looking in the old place: round 237
# found it deriving `$installDir` from the SCRIPT's own path and then appending
# `tools\`, so it searched `<InstallDir>\scripts\tools\cloudflared.exe` and exited 1
# unconditionally. `agent/summrise-agent-npm/bin/summrise.js:387` installs this script into
# `<InstallDir>\scripts\`, and `agent/src/paths.rs:228` (`cloudflared_bin()`) resolves the
# binary as `<InstallDir>\components\cloudflared.exe` — the flat `tools\` dir is the
# pre-layout-v2 location that ADR 0008 replaced, and `summrise.js` migrates out of it.
#
# Both locations are tried, NEW FIRST, so the script works on a migrated install and on
# one that has not been migrated yet. NOT VERIFIED ON A DEVICE — this is a static fix
# against the two sources above; the loop has not run it on d1.
$installDir = Split-Path -Parent $PSScriptRoot   # <InstallDir>\scripts\ -> <InstallDir>
$cloudflared = Join-Path $installDir "components\cloudflared.exe"
if (-not (Test-Path $cloudflared)) {
    $legacy = Join-Path $installDir "tools\cloudflared.exe"
    if (Test-Path $legacy) { $cloudflared = $legacy }
}
if (-not (Test-Path $cloudflared)) {
    Write-Host "!! cloudflared not found at $installDir\components\cloudflared.exe nor $installDir\tools\cloudflared.exe"
    exit 1
}
# Find the agent tunnel by NAME, not by the first UUID in `tunnel list` — the
# legacy summrise-command-dN tunnels still exist, and taking the FIRST UUID in
# `tunnel list` could pick one of them, writing the OLD tunnel into the config. (That is
# what the deleted Get-TunnelId did; this script matches by NAME instead.)
# EAP=Continue guard (round-66): cloudflared's stderr WRN/INF (e.g. a version
# notice) is a terminating NativeCommandError under EAP=Stop — the script
# aborted before any tunnel repair. Real failures still surface via exit codes.
$oldEAP3 = $ErrorActionPreference
$ErrorActionPreference = "Continue"
$list = & $cloudflared tunnel list 2>&1 | Out-String
$ErrorActionPreference = $oldEAP3
$newTunnel = ""
if ($list -match "([0-9a-fA-F]{8}-[0-9a-fA-F-]{27})\s+$([regex]::Escape($tunnelName))\s") {
    $newTunnel = $Matches[1]
}
if (-not $newTunnel) {
    Write-Host "!! tunnel $tunnelName not found in tunnel list — create it (cloudflared tunnel create $tunnelName)"
    exit 1
}

# Single location: the agent-owned tunnel.yml (main.rs spawns cloudflared
# with --config tunnel.yml). The hostname file is the only other thing that
# needs the same rewrite.
$files = @(
    (Join-Path $installDir "tunnel.yml"),
    $hostFile
)
$anyChanged = $false
foreach ($f in $files) {
    if (-not (Test-Path $f)) { continue }
    $c = Get-Content $f -Raw
    $changed = $false
    # Old tunnel ID (summrise-command-dN) → new agent tunnel ID
    if ($c -match 'tunnel:\s*[0-9a-fA-F-]{36}' -and $c -notmatch [regex]::Escape($newTunnel)) {
        $c = $c -replace 'tunnel:\s*[0-9a-fA-F-]{36}', "tunnel: $newTunnel"
        $changed = $true
    }
    # Old command hostname → agent hostname
    if ($c -match '\.command\.saisi\.online') {
        $c = $c -replace '\.command\.saisi\.online', '.agent.saisi.online'
        $changed = $true
    }
    if ($changed) {
        Set-Content $f $c -Encoding ascii
        Write-Host "  fixed: $f"
        $anyChanged = $true
    }
    # Diagnostic log — written next to the install dir so it can be fetched
    # via the panel/API instead of asking the user to read files.
    $logDir = Split-Path $hostFile -Parent
    if ($logDir) {
        $log = Join-Path $logDir "fix-tunnel.log"
        "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') file=$f changed=$changed newTunnel=$newTunnel hostname=$hostname" |
            Out-File -FilePath $log -Append -Encoding utf8
    }
}

# Also clean up legacy names in summrise-agent's config.yaml: `name` should be
# summrise-agent (not summrise-command). The token field is NOT touched — the new
# install already writes device_token, and rewriting a legacy auth_token could
# clobber the fresh token.
$cfgCandidates = @(
    (Join-Path $env:USERPROFILE ".summrise-agent\config.yaml"),
    "C:\summrise-agent\config.yaml",
    "C:\summrise-command\config.yaml",
    "D:\summrise-command\config.yaml",
    "D:\summrise-agent\config.yaml"
)
foreach ($cfg in $cfgCandidates) {
    if (-not (Test-Path $cfg)) { continue }
    $c = Get-Content $cfg -Raw
    if ($c -match 'name:\s*summrise-command') {
        $c = $c -replace 'name:\s*summrise-command', 'name: summrise-agent'
        Set-Content $cfg $c -Encoding ascii
        Write-Host "  fixed config: $cfg"
    }
}

# Ensure the tunnel has a real DNS route for the hostname (a bare CNAME is not
# a route — without this the tunnel 530s with error 1033). --overwrite replaces
# any pre-existing record with a proper tunnel route.
# NOTE: cloudflared logs its INF lines to stderr, which PS 5.1 turns into a
# terminating NativeCommandError under EAP=Stop even with 2>&1 — scope EAP to
# Continue and rely on the exit code, so a benign 'already configured' message
# cannot abort the script BEFORE the cloudflared restart below.
$oldEAP = $ErrorActionPreference
$ErrorActionPreference = "Continue"
& $cloudflared tunnel route dns $tunnelName $hostname 2>$null
$routeCode = $LASTEXITCODE
$ErrorActionPreference = $oldEAP
if ($routeCode -ne 0) {
    Write-Host "  !! route dns failed (exit $routeCode) — add the Public Hostname in the dashboard"
} else {
    Write-Host "  route dns ok: $hostname → $tunnelName"
}

# Restart cloudflared ONLY if a config file actually changed. A restart drops
# the tunnel for several seconds (error 1033 in the browser) — restarting on
# every boot, when nothing changed, made every agent start briefly offline.
if ($anyChanged) {
    $oldEAP2 = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    sc.exe stop cloudflared 2>$null | Out-Null
    Start-Sleep -Seconds 2
    sc.exe start cloudflared 2>$null | Out-Null
    $ErrorActionPreference = $oldEAP2
    Write-Host "  cloudflared restarted (config changed; tunnel $tunnelName → $newTunnel, hostname $hostname)"
} else {
    Write-Host "  cloudflared config unchanged — not restarted"
}
