#!/usr/bin/env bash
# Build summrise-console for Windows, stage it on the mirror, and write the deploy script.
#
#   scripts/stage-console-deploy.bash 0.3.153
#   scripts/stage-console-deploy.bash 0.3.153 --mirror-dir /tmp/canary-mirror \
#                                            --base-url http://100.85.163.159:18100
#
# After this, ONE command runs on the console host (it is printed at the end):
#
#   powershell -NoProfile -Command "Invoke-WebRequest -UseBasicParsing <base>/deploy-console-<version>.cmd
#       -OutFile C:\Windows\Temp\deploy.cmd; cmd /c C:\Windows\Temp\deploy.cmd"
#
# The host has no Rust toolchain (measured 2026-10-28: cargo, rustc and git are all absent), so the
# binary is built here and fetched by the host over the tailnet. The bytes never pass through an AI
# context, and the host opens nothing inbound.
#
# The address the host fetches from is the canary mirror the operator already runs for device
# artifacts; `--base-url` is the address that host can reach, not necessarily the one this host uses.
set -euo pipefail
cd "$(dirname "$0")/.."

version=""
mirror_dir="/tmp/canary-mirror"
base_url="http://100.85.163.159:18100"
target="x86_64-pc-windows-msvc"
previous=""

while [ $# -gt 0 ]; do
    case "$1" in
        --mirror-dir) mirror_dir="$2"; shift 2 ;;
        --base-url) base_url="${2%/}"; shift 2 ;;
        --target) target="$2"; shift 2 ;;
        --previous) previous="$2"; shift 2 ;;
        -h | --help) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        -*) echo "stage-console-deploy: unknown argument $1" >&2; exit 2 ;;
        *) version="$1"; shift ;;
    esac
done

[ -n "$version" ] || { echo "usage: scripts/stage-console-deploy.bash <version> [--mirror-dir DIR] [--base-url URL]" >&2; exit 2; }
case "$version" in
    *[!0-9.]* | "") echo "stage-console-deploy: version must be digits and dots: $version" >&2; exit 2 ;;
esac
command -v cargo >/dev/null 2>&1 || export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo-xwin >/dev/null 2>&1 || { echo "stage-console-deploy: cargo-xwin is not on PATH" >&2; exit 2; }
[ -d "$mirror_dir" ] || { echo "stage-console-deploy: mirror dir does not exist: $mirror_dir" >&2; exit 2; }

echo "stage-console-deploy: building $version for $target"
SUMMRISE_BUILD_VERSION="$version" cargo xwin build --manifest-path device/Cargo.toml \
    --release --target "$target" -p summrise-console

exe="device/target/$target/release/summrise-console.exe"
[ -f "$exe" ] || { echo "stage-console-deploy: the build produced no $exe" >&2; exit 2; }

# The version must be in the bytes, not only in the variable: a build that ignored
# SUMMRISE_BUILD_VERSION would deploy a binary that reports yesterday's release.
# `grep -c`, not `grep -q`: with `set -o pipefail`, an early-exiting `grep -q` gives `strings` a
# SIGPIPE and the pipeline reports failure on a match — the check would fail exactly when it should pass.
if [ "$(strings -n 4 "$exe" | grep -cF "$version" || true)" -eq 0 ]; then
    echo "stage-console-deploy: $exe does not contain the string $version — did the build ignore SUMMRISE_BUILD_VERSION?" >&2
    exit 2
fi

sha="$(sha256sum "$exe" | cut -d' ' -f1)"
staged="$mirror_dir/summrise-console-$version.exe"
deploy="$mirror_dir/deploy-console-$version.cmd"
cp "$exe" "$staged"

sed -e "s|__VERSION__|$version|g" \
    -e "s|__SHA256__|$sha|g" \
    -e "s|__BASE_URL__|$base_url|g" \
    scripts/deploy-console.cmd.template > "$deploy"

echo
echo "staged:"
echo "  $staged  ($(stat -c%s "$staged") bytes, sha256 $sha)"
echo "  $deploy"
echo
echo "run on the console host (DESKTOP-9VH92BJ, task SummriseConsole):"
echo "  powershell -NoProfile -Command \"Invoke-WebRequest -UseBasicParsing $base_url/deploy-console-$version.cmd -OutFile C:\\Windows\\Temp\\deploy.cmd; cmd /c C:\\Windows\\Temp\\deploy.cmd\""
echo
echo "rollback, if the health line does not say $version:"
echo "  schtasks /end /tn SummriseConsole && taskkill /F /IM summrise-console.exe"
echo "  copy /Y C:\\SummriseConsole\\summrise-console.exe.bak-<previous> C:\\SummriseConsole\\summrise-console.exe"
echo "  schtasks /run /tn SummriseConsole"
