# SummriseIntegrity.ps1 — what the online installer checks before it installs anything.
#
# WHY THIS FILE EXISTS (round 124). The installer's CDN fallback ran
# `npm install -g <url>` on a tarball it had never hashed, while `/api/version`
# carried the correct digest and every other consumer of the same bytes refused
# unverified input (`agent/src/plugins/update/tools.rs`: "refusing unverifiable
# install", and a sha mismatch skips the install). The fallback is not an exotic
# branch — it is the one the script takes deliberately whenever the
# NSIS-embedded payload is missing on disk (AV quarantine, a partial extract),
# i.e. exactly when the machine is already in a state worth distrusting.
#
# Pure functions only — strings in, verdicts out — so the logic can be tested on
# a runner with no Windows box: `agent/deploy/lib/SummriseIntegrity.tests.ps1`.

function Get-ManifestSha256 {
  # The tgz digest the version manifest advertises, or "" when it does not
  # advertise one FOR THIS VERSION.
  #
  # A VERSION MISMATCH IS A FAILURE, NOT A FALLBACK. A manifest describing
  # another release must never be the thing that validates this download, and
  # "no digest" must never be read as "nothing to check".
  param([string]$ManifestJson, [string]$Version)
  if (-not $ManifestJson) { return "" }
  try { $m = $ManifestJson | ConvertFrom-Json } catch { return "" }
  if (-not $m) { return "" }
  if ($m.version -and ("$($m.version)" -ne $Version)) { return "" }
  $sha = "$($m.sha256)"
  if ($sha -notmatch '^[0-9a-fA-F]{64}$') { return "" }
  return $sha.ToLower()
}

function Get-FileSha256 {
  # The file's digest in lowercase hex, or "" when it cannot be read.
  param([string]$Path)
  if (-not $Path -or -not (Test-Path $Path)) { return "" }
  try { return (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLower() } catch { return "" }
}

function Test-FileSha256 {
  # $true ONLY when the file's digest equals the expected one. Every other
  # outcome — unreadable file, empty expectation, mismatch — is $false, because
  # what the caller does with $true is install code.
  param([string]$Path, [string]$Expected)
  if (-not $Expected) { return $false }
  $got = Get-FileSha256 -Path $Path
  if (-not $got) { return $false }
  return ($got -eq $Expected.ToLower())
}

function Get-ComponentSha256 {
  # The digest the manifest advertises for a NAMED component, or "" when it does
  # not advertise one — and "" must never be read as "nothing to check", the same
  # contract Get-ManifestSha256 carries one function up.
  #
  # WHY THIS EXISTS (round 143). The installer PRE-STAGES cloudflared and the
  # playwright bundle into the npm package dir, and `summrise setup` then takes
  # them by PRESENCE (resolveComponent), so the pins those components carry in the
  # manifest were never consulted on this path at all. The ps1 accepted them on
  # `Length -gt 1MB`. The tgz above was already verified; these two — 54 MB and
  # 31 MB of executable payload — were not.
  param([string]$ManifestJson, [string]$Name)
  if (-not $ManifestJson -or -not $Name) { return "" }
  try { $m = $ManifestJson | ConvertFrom-Json } catch { return "" }
  if (-not $m -or -not $m.components) { return "" }
  $c = $m.components.$Name
  if (-not $c) { return "" }
  $sha = "$($c.sha256)"
  if ($sha -notmatch '^[0-9a-fA-F]{64}$') { return "" }
  return $sha.ToLower()
}

function Get-ComponentUrl {
  # The ADDRESS the manifest publishes for a NAMED component, or $Fallback when it
  # publishes none — the mirror image of Get-ComponentSha256's contract, and the
  # fallback is what keeps "" from ever meaning "fetch nothing".
  #
  # WHY THIS EXISTS (2026-09-25). version.json has published a `url` beside each
  # component's sha256 for as long as the pins have existed, and this installer
  # read the DIGEST from it while RETYPING the matching path three blocks below
  # (`$CdnBase/summrise-agent/<file>`) — one fact with two authors, which is how a
  # renamed component comes to be published at one address and fetched from
  # another. index/src/index.js rebuilds the published url against the ORIGIN OF
  # THE REQUEST that asked for /api/version, so the address it hands back is
  # exactly the one this script derives from -CdnBase: they agree today, which is
  # why reading the manifest is a DRIFT GUARD and not a fix, and why a mirror set
  # with -CdnBase keeps fetching from the mirror.
  #
  # A url that is NOT an absolute http(s) address is refused in favour of the
  # fallback rather than fetched: what the caller does with the answer is download
  # and install it.
  #
  # AND SO IS EVERY SPELLING OF -CdnBase THAT IS NOT A BARE ORIGIN ($BaseUrl, when
  # the caller passes it). The published url is rebuilt against the ORIGIN of the
  # /api/version request, so it cannot carry a mirror's path prefix: for
  # `https://host/mirror` — or `https://host/`, which the derived URLs spell with a
  # double slash today — this arm keeps the path it built itself, which is the URL
  # it used before the manifest was read at all.
  param([string]$ManifestJson, [string]$Name, [string]$Fallback, [string]$BaseUrl = "")
  if (-not $ManifestJson -or -not $Name) { return $Fallback }
  try { $m = $ManifestJson | ConvertFrom-Json } catch { return $Fallback }
  if (-not $m -or -not $m.components) { return $Fallback }
  $c = $m.components.$Name
  if (-not $c) { return $Fallback }
  $u = "$($c.url)"
  if ($u -notmatch '^https?://[^\s]+$') { return $Fallback }
  if ($BaseUrl -and $BaseUrl -notmatch '^https?://[^/]+$') { return $Fallback }
  return $u
}
