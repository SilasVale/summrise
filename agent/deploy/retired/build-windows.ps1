# build-windows.ps1 — RETIRED (round 237). Kept for history; do not run.
#
# It built `--bin vale-command`, and that binary no longer exists: `cargo build
# --features terminal --bin vale-command` answers
#   error: no bin target named `vale-command` in default-run packages
#   help: available bin targets: vale-agent
# Its sibling `vale-command-setup.bat` was retired long ago; this script was not, and
# it sat in `agent/deploy/` as the only file no round had ever named that still
# referenced the retired binary.
#
# A SECOND REASON NOT TO KEEP IT IN SERVICE, recorded because it is this repository's
# own documented trap: the build exit code was never checked. The final branch was a
# `Test-Path`, so a FAILED cargo printed "Build finished but <out> not found" in yellow
# and the script exited 0 — the same shape as `vale update` returning 0 on the WMI
# handoff rather than on the swap. A build script whose failure mode is a yellow line
# and a zero exit is worse than no build script.
#
# The live build entry is `scripts/build.sh agent` (panel build + cargo xwin).

param([switch]$Release)

$feature = "terminal"   # serial/SSH/PTY. The `browser` feature is retired.

$config = if ($Release) { "--release" } else { "" }
$args = @("build", "--features", $feature) + @($config) + @("--bin", "vale-command")

Write-Host "cargo $($args -join ' ')"
cargo @args

$out = if ($Release) { "target\release\vale-command.exe" } else { "target\debug\vale-command.exe" }
if (Test-Path $out) {
    Write-Host "Built: $out"
} else {
    Write-Host "Build finished but $out not found." -ForegroundColor Yellow
}
