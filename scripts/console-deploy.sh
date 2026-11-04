#!/usr/bin/env bash
# Build the console, swap it on the host, wait for it to answer.
#
#   deploy-console.sh <version> [--build-only]
#
# Version is the label baked into the binary and the name of the backup it creates, e.g. 0.3.272-ux37.
# Everything else is the measured procedure: cross-build for x86_64-pc-windows-gnu, fetch on the host
# through the tailnet, verify the digest, swap from a detached process (the swap stops the process that
# serves the shell it was started from), poll /health from here.
set -euo pipefail

VERSION="${1:?usage: deploy-console.sh <version> [--build-only]}"
MODE="${2:-}"
PRODUCT="$(cd "$(dirname "$0")/.." && pwd)"
DEVICE=$PRODUCT/device
HOST_DEVICE="${SUMMRISE_CONSOLE_HOST_DEVICE:-4c718d6e44c83388c7e07bd0d811fa0b7cf22e2fced85d35d0021dd26b11f687}"
STAGE=/tmp/console-deploy
URL=http://100.85.163.159:8099
export PATH="$HOME/.cargo/bin:$PATH"

# The page is embedded at compile time: a stale dist ships yesterday's UI under a new label.
"$PRODUCT/scripts/build-web.sh" --check >/dev/null || {
    echo "the committed page is stale: run scripts/build-web.sh and commit dist/index.html" >&2
    exit 1
}

cd "$DEVICE"
SUMMRISE_BUILD_VERSION="$VERSION" cargo build --release -p summrise-console --target x86_64-pc-windows-gnu 2>&1 | tail -1
SHA=$(sha256sum target/x86_64-pc-windows-gnu/release/summrise-console.exe | cut -d' ' -f1)
echo "built $VERSION sha=$SHA"

[ "$MODE" = "--build-only" ] && exit 0

mkdir -p "$STAGE"
cp target/x86_64-pc-windows-gnu/release/summrise-console.exe "$STAGE/summrise-console-$VERSION.exe"
( pgrep -f "http.server 8099 --bind 100.85.163.159" >/dev/null ) || ( cd "$STAGE" && setsid python3 -m http.server 8099 --bind 100.85.163.159 >/tmp/console-deploy-http.log 2>&1 & )
sleep 1

host() { python3 /tmp/host_call.py "$HOST_DEVICE" "$1" "$2"; }

host runs.exec "{\"command\":\"curl.exe\",\"args\":[\"-fsS\",\"--retry\",\"2\",\"-o\",\"C:\\\\SummriseConsole\\\\summrise-console.exe.new\",\"$URL/summrise-console-$VERSION.exe\"],\"timeout_ms\":120000}" >/dev/null
REMOTE=$(host runs.exec '{"command":"powershell.exe","args":["-NoProfile","-Command","(Get-FileHash C:\\SummriseConsole\\summrise-console.exe.new -Algorithm SHA256).Hash.ToLower()"]}' | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["content"][0]["text"].strip())')
[ "$REMOTE" = "$SHA" ] || { echo "digest mismatch: local $SHA remote $REMOTE" >&2; exit 1; }
echo "host fetched the same bytes"

SWAP=$(cat <<PS1
\$ErrorActionPreference = 'Continue'
\$dir = 'C:\\SummriseConsole'
\$live = Join-Path \$dir 'summrise-console.exe'
\$new  = Join-Path \$dir 'summrise-console.exe.new'
\$log  = Join-Path \$dir 'swap-$VERSION.log'
function Say(\$m) { Add-Content -Path \$log -Value ((Get-Date -Format o) + ' ' + \$m) }
Start-Sleep -Seconds 3
Say ('begin sha=$SHA')
schtasks /end /tn SummriseConsole | Out-Null
Start-Sleep -Seconds 2
Copy-Item -Path \$live -Destination (Join-Path \$dir 'summrise-console.exe.bak-live') -Force
Move-Item -Path \$new -Destination \$live -Force
schtasks /run /tn SummriseConsole | Out-Null
Start-Sleep -Seconds 8
try { \$h = Invoke-RestMethod -Uri 'http://127.0.0.1:7736/health' -TimeoutSec 5; Say ('health ' + (\$h | ConvertTo-Json -Compress)) } catch { Say ('health failed ' + \$_.Exception.Message) }
PS1
)
ENCODED=$(printf '%s' "$SWAP" | iconv -f utf-8 -t utf-16le | base64 -w0)
host runs.exec "{\"command\":\"powershell.exe\",\"args\":[\"-NoProfile\",\"-EncodedCommand\",\"$ENCODED\"],\"timeout_ms\":60000}" >/dev/null
echo "swap launched (detached)"

for _ in $(seq 1 30); do
  HEALTH=$(curl -sS --max-time 5 https://console.saisi.online/health 2>/dev/null || true)
  if echo "$HEALTH" | grep -q "$VERSION"; then
    echo "$HEALTH"
    rm -rf "$STAGE"
    exit 0
  fi
  sleep 3
done
echo "the console did not report $VERSION within 90s" >&2
exit 1
