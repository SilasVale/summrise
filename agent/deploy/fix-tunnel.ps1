# fix-tunnel.ps1 — repair the cloudflared tunnel config after a Summrise Agent
# migration. The old install used a tunnel named summrise-command-dN with the
# *.command.saisi.online hostname; the new install created summrise-agent-dN with
# *.agent.saisi.online. If the agent-owned tunnel.yml still references the
# old tunnel or hostname, rewrite it (single location — no user/systemprofile
# copies) and restart cloudflared.
#
# Idempotent: a config already on the new tunnel/hostname is untouched.
$ErrorActionPreference = "Stop"

# <InstallDir>\scripts\ -> <InstallDir> (the CLI installs this script into
# <InstallDir>\scripts\). Computed FIRST because every file this script repairs is
# now resolved from it — the cloudflared block below records the round-237 half of
# that story.
$installDir = Split-Path -Parent $PSScriptRoot

# Find the agent tunnel (summrise-agent-dN) from the device hostname. WHERE that file
# lives changed in layout v2: `agent/src/paths.rs` hostname_file() is
# `etc_dir().join("summrise-agent.hostname")` = <InstallDir>\etc\summrise-agent.hostname,
# and the CLI's migration table (`agent/summrise-agent-npm/src/summrise.ts`, the
# ["summrise-agent.hostname", etc\..., "f"] pair) MOVES the pre-v2 root copy there. So
# etc\ is tried FIRST, then the pre-v2 roots the migration moves FROM — still needed on
# an install the CLI has not migrated, or has only half-migrated.
#
# THE LITERAL "d1.agent.saisi.online" FALLBACK IS DELETED, and it was the destructive
# half of this defect. The old code looked ONLY at the three pre-v2 absolute roots, so
# on a v2 install it found nothing, called every device d1, and then ran
# `cloudflared tunnel route dns summrise-agent-d1 d1.agent.saisi.online` — from whatever
# box it happened to be on. On a half-migrated install (which `migration_pending` in
# paths.rs explicitly tolerates) <InstallDir>\tunnel.yml still exists, so the rewrite
# below pointed a NON-d1 device's tunnel config at d1's tunnel UUID: the exact failure
# the tunnel-list comment further down says this script exists to prevent. An absent
# hostname means "this device has not been named" — the agent refuses in that state
# ("cannot provision: summrise-agent.hostname missing or invalid", agent/src/tunnel.rs) —
# and this script now refuses the same way instead of inventing a device name.
$hostFile = ""
$hostname = ""
foreach ($candidate in @(
        (Join-Path $installDir "etc\summrise-agent.hostname"),
        (Join-Path $installDir "summrise-agent.hostname"),
        "C:\summrise-agent\summrise-agent.hostname",
        "D:\summrise-command\summrise-agent.hostname",
        "D:\summrise-agent\summrise-agent.hostname"
    )) {
    if (Test-Path $candidate) {
        # `Get-Content -Raw` is $null for a zero-byte file in PS 5.1, and `.Trim()` on
        # $null throws under EAP=Stop — read first, then trim the truthy value.
        $raw = Get-Content $candidate -Raw
        if ($raw) {
            $hostname = $raw.Trim()
            if ($hostname) { $hostFile = $candidate; break }
        }
    }
}
# The same charset the agent enforces before a hostname reaches cloudflared's ARGV (the
# `host_ok` closure in agent/src/tunnel.rs): a value starting with "-" would be read as a
# FLAG, and anything outside [A-Za-z0-9._-] is not a DNS name. Missing and invalid are the
# same refusal here, exactly as they are there.
$hostOk = ($hostname -ne "") -and ($hostname.Length -le 253) -and (-not $hostname.StartsWith("-")) -and ($hostname -match '^[A-Za-z0-9._-]+$')
if (-not $hostOk) {
    Write-Host "!! no usable device hostname: nothing found at any of these"
    Write-Host "     $installDir\etc\summrise-agent.hostname   (layout v2)"
    Write-Host "     $installDir\summrise-agent.hostname        (pre-v2 install root)"
    Write-Host "     C:\summrise-agent\, D:\summrise-command\, D:\summrise-agent\   (pre-v2 roots)"
    Write-Host "   Refusing to guess one: a guessed name would point ANOTHER device's hostname"
    Write-Host "   at this device's tunnel, which is worse than not repairing anything."
    Write-Host "   Name this device first:  summrise setup --hostname <sub>"
    exit 1
}
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
#
# $installDir itself is computed once at the top of the script: the hostname block above
# needs it too, and a second derivation of the same path is a second thing to keep right.
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
#
# tunnel.yml is resolved the same way the hostname was, and for the same reason:
# `agent/src/paths.rs` tunnel_file() is `etc_dir().join("tunnel.yml")`, and the CLI's
# migration table moves the pre-v2 root copy into etc\. So: etc\ first, then the install
# root (which IS the pre-v2 location when the install dir is a legacy one). This file is
# the one whose rewrite REPOINTS THE TUNNEL, so a stale copy read from the wrong place is
# the expensive mistake here.
$files = @(
    (Join-Path $installDir "etc\tunnel.yml"),
    (Join-Path $installDir "tunnel.yml"),
    $hostFile
)
# Diagnostic log — under <InstallDir> (etc\ when it exists, else the install root), NOT
# the pre-v2 absolute root this used to derive from $hostFile: that directory can be gone
# on a migrated install, and Out-File THROWS under $ErrorActionPreference = "Stop", so the
# log write aborted the run BEFORE the config.yaml cleanup, the `route dns` and the
# cloudflared restart below. A log is evidence, not a step: its failure must not stop the
# repair, hence the try/catch on the write itself.
$logDir = Join-Path $installDir "etc"
if (-not (Test-Path $logDir)) { $logDir = $installDir }
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
    try {
        "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') file=$f changed=$changed newTunnel=$newTunnel hostname=$hostname" |
            Out-File -FilePath (Join-Path $logDir "fix-tunnel.log") -Append -Encoding utf8 -ErrorAction Stop
    } catch {
        Write-Host "  (fix-tunnel.log not written: $($_.Exception.Message))"
    }
}

# Also clean up legacy names in summrise-agent's config.yaml: `name` should be
# summrise-agent (not summrise-command). The token field is NOT touched — the new
# install already writes device_token, and rewriting a legacy auth_token could
# clobber the fresh token.
#
# RESOLVED THE SAME WAY (etc\ first), and here too the old list was all pre-v2 paths:
# `agent/src/paths.rs` config_file() is `etc_dir().join("config.yaml")`. The five paths
# that follow the two $installDir ones are kept: a pre-v2 or half-migrated install still
# keeps the file there — the CLI's migration table moves it into etc\, and the move is
# skipped when the target already exists, which leaves the source behind.
$cfgCandidates = @(
    (Join-Path $installDir "etc\config.yaml"),
    (Join-Path $installDir "config.yaml"),
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
