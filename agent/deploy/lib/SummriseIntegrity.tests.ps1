# SummriseIntegrity.tests.ps1 — plain asserts, no Pester. Exit 0 = all green.
# Run: pwsh -File agent/deploy/lib/SummriseIntegrity.tests.ps1
#
# These cover the two halves the installer's CDN fallback depends on: reading a
# digest out of the version manifest (where every wrong answer must be "") and
# deciding whether a file on disk is the artifact that manifest describes.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/SummriseIntegrity.ps1"

$PASS = 0
function Check([string]$desc, [string]$actual, [string]$expected) {
  if ($actual -ceq $expected) { $script:PASS++ }
  else {
    Write-Host "FAIL: $desc"
    Write-Host "  actual:   [$actual]"
    Write-Host "  expected: [$expected]"
    exit 1
  }
}

$sha = 'a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90'

# --- the manifest reader ----------------------------------------------------
Check "a manifest for THIS version yields its digest" `
  (Get-ManifestSha256 -ManifestJson "{`"version`":`"1.2.364`",`"sha256`":`"$sha`"}" -Version '1.2.364') $sha

Check "an UPPERCASE digest is normalised" `
  (Get-ManifestSha256 -ManifestJson "{`"version`":`"1.2.364`",`"sha256`":`"$($sha.ToUpper())`"}" -Version '1.2.364') $sha

# The three ways this check could be talked out of doing its job.
Check "a manifest for ANOTHER version is refused" `
  (Get-ManifestSha256 -ManifestJson "{`"version`":`"1.2.363`",`"sha256`":`"$sha`"}" -Version '1.2.364') ""
Check "a manifest with no digest is refused (not 'nothing to check')" `
  (Get-ManifestSha256 -ManifestJson '{"version":"1.2.364"}' -Version '1.2.364') ""
Check "a truncated digest is refused" `
  (Get-ManifestSha256 -ManifestJson '{"version":"1.2.364","sha256":"abc123"}' -Version '1.2.364') ""
Check "a non-hex digest is refused" `
  (Get-ManifestSha256 -ManifestJson '{"version":"1.2.364","sha256":"zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"}' -Version '1.2.364') ""
Check "malformed JSON is refused" `
  (Get-ManifestSha256 -ManifestJson 'not json at all' -Version '1.2.364') ""
Check "an empty body is refused" (Get-ManifestSha256 -ManifestJson '' -Version '1.2.364') ""
# A service returning the 503 envelope /api/version uses for a bad manifest
# (index.js: ver && sha required) must not read as a digest either.
Check "the worker's 503 envelope is refused" `
  (Get-ManifestSha256 -ManifestJson '{"error":"manifest unavailable"}' -Version '1.2.364') ""

# --- the COMPONENT pin reader (round 143) ------------------------------------
# The installer pre-stages cloudflared, playwright and electron into the npm package
# dir, and `summrise setup` then takes them BY PRESENCE (resolveComponent) — so these
# pins are the only check those bytes ever get. The acceptance used to be
# `Length -gt 1MB`, which is not a verdict about anything.
$comps = '{"version":"1.2.364","sha256":"' + $sha + '","components":{' +
  '"cloudflared":{"url":"https://x/cloudflared.exe","sha256":"' + $sha.ToUpper() + '"},' +
  '"playwright":{"url":"https://x/summrise-playwright.zip","sha256":"' + $sha + '"}}}'

Check "a named component yields its pin, normalised" `
  (Get-ComponentSha256 -ManifestJson $comps -Name 'cloudflared') $sha
Check "another named component yields its own" `
  (Get-ComponentSha256 -ManifestJson $comps -Name 'playwright') $sha

# Every way this check could be talked out of doing its job.
Check "a component the manifest does NOT carry is refused (not 'nothing to check')" `
  (Get-ComponentSha256 -ManifestJson $comps -Name 'electron') ""
Check "an empty name is refused" (Get-ComponentSha256 -ManifestJson $comps -Name '') ""
Check "a truncated component digest is refused" `
  (Get-ComponentSha256 -ManifestJson '{"components":{"cloudflared":{"sha256":"abc"}}}' -Name 'cloudflared') ""
Check "a non-hex component digest is refused" `
  (Get-ComponentSha256 -ManifestJson '{"components":{"cloudflared":{"sha256":"zzzz"}}}' -Name 'cloudflared') ""
Check "a component with no sha256 field is refused" `
  (Get-ComponentSha256 -ManifestJson '{"components":{"cloudflared":{"url":"https://x/y"}}}' -Name 'cloudflared') ""
Check "a manifest with NO components block is refused" `
  (Get-ComponentSha256 -ManifestJson '{"version":"1.2.364","sha256":"' + $sha + '"}' -Name 'cloudflared') ""
Check "malformed JSON is refused" (Get-ComponentSha256 -ManifestJson 'not json' -Name 'cloudflared') ""
Check "an empty body is refused" (Get-ComponentSha256 -ManifestJson '' -Name 'cloudflared') ""

# --- the file verdict -------------------------------------------------------
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("summrise-integrity-" + [Guid]::NewGuid().ToString('N') + ".bin")
[IO.File]::WriteAllText($tmp, "hello")
try {
  $real = (Get-FileHash -Algorithm SHA256 -Path $tmp).Hash.ToLower()
  Check "the file's own digest is computed" (Get-FileSha256 -Path $tmp) $real
  Check "a matching digest passes" (Test-FileSha256 -Path $tmp -Expected $real) "True"
  Check "an uppercase expectation still passes" (Test-FileSha256 -Path $tmp -Expected $real.ToUpper()) "True"
  Check "a different digest fails" (Test-FileSha256 -Path $tmp -Expected $sha) "False"
  Check "no expectation fails" (Test-FileSha256 -Path $tmp -Expected "") "False"
  Check "a missing file fails" (Test-FileSha256 -Path "$tmp.nope" -Expected $real) "False"
  Check "a missing file has no digest" (Get-FileSha256 -Path "$tmp.nope") ""
} finally {
  Remove-Item $tmp -ErrorAction SilentlyContinue
}

Write-Host "SummriseIntegrity: $PASS checks passed"
