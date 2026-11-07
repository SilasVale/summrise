#!/usr/bin/env bash
# Build the console, swap it on the host, wait for it to answer.
#
#   console-deploy.sh <version> [--build-only|--preflight-only]
#
# Version is the label baked into the binary and the name of the backup it creates, e.g. 0.3.272-ux37.
# Everything else is the measured procedure: cross-build for x86_64-pc-windows-gnu, fetch on the host
# through the tailnet, verify the digest, swap from a detached process (the swap stops the process that
# serves the shell it was started from), poll /health from here.
#
# THE DEPLOY GOES THROUGH THE HOST DEVICE'S SHELL. The console serves its page on its own, but the
# fetch and the swap are runs.exec calls proxied to the host device (DESKTOP-9VH92BJ); with that
# device's channel down the console answers and the deploy cannot move a byte. The preflight below
# asks the door before the build, so that state is named up front instead of surfacing as a bare
# "the console did not answer: HTTP Error 503" after a full build.
set -euo pipefail

VERSION="${1:?usage: console-deploy.sh <version> [--build-only|--preflight-only]}"
MODE="${2:-}"
case "$MODE" in
    "" | --build-only | --preflight-only) ;;
    *)
        echo "console-deploy: unknown mode $MODE (expected --build-only or --preflight-only)" >&2
        exit 2
        ;;
esac
PRODUCT="$(cd "$(dirname "$0")/.." && pwd)"
DEVICE=$PRODUCT/device
HOST_DEVICE="${SUMMRISE_CONSOLE_HOST_DEVICE:-4c718d6e44c83388c7e07bd0d811fa0b7cf22e2fced85d35d0021dd26b11f687}"
HOST_NAME="${SUMMRISE_CONSOLE_HOST_NAME:-DESKTOP-9VH92BJ}"
CONSOLE="${SUMMRISE_CONSOLE_URL:-https://console.saisi.online}"
HEALTH_FACTS="version and fleet unknown"
STAGE=/tmp/console-deploy
URL=http://100.85.163.159:8099
export PATH="$HOME/.cargo/bin:$PATH"

# THE CONSOLE'S /mcp IS THE DOOR: it proxies the call to the host device, so its own answer (`503
# device reconnecting` when the channel is down) is what a caller sees instead of the device's.
mcp_raw() { # mcp_raw <tool> <arguments json> — the body, then the HTTP status on the last line
    curl -sS --max-time 180 -X POST "$CONSOLE/mcp" \
        -H 'content-type: application/json' \
        -d "{\"device_id\":\"$HOST_DEVICE\",\"request\":{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"$1\",\"arguments\":$2}}}" \
        -w $'\n%{http_code}'
}

door_failure() { # door_failure <what> <status> <body>
    local what=$1 status=$2 body=$3 reason=""
    reason=$(printf '%s' "$body" | python3 -c '
import json, sys
try:
    data = json.load(sys.stdin)
except Exception:
    raise SystemExit
parts = []
if data.get("error"):
    parts.append(str(data["error"]))
if data.get("retry_after_ms") is not None:
    parts.append("retry in %s ms" % data["retry_after_ms"])
print(", ".join(parts))
' 2>/dev/null || true)
    if [ "$status" = 503 ]; then
        echo "$what: a console deploy fetches and swaps through $HOST_NAME's shell, and both steps are runs.exec calls on it. The console itself answers ($HEALTH_FACTS), but its channel to the host is down: /mcp answered 503${reason:+ $reason}." >&2
    else
        echo "$what: a console deploy fetches and swaps through $HOST_NAME's shell, and both steps are runs.exec calls on it. The console itself answers ($HEALTH_FACTS), but /mcp answered HTTP $status${reason:+ $reason}." >&2
    fi
    exit 1
}

mcp() { # mcp <tool> <arguments json> <what failed> — the raw JSON-RPC envelope
    local raw status body
    raw=$(mcp_raw "$1" "$2") || {
        echo "the console did not answer at $CONSOLE/mcp while $3: the console host is unreachable" >&2
        exit 1
    }
    status=${raw##*$'\n'}
    body=${raw%$'\n'*}
    [ "$status" = 200 ] || door_failure "$3" "$status" "$body"
    printf '%s\n' "$body"
}

# THE SWAP STOPS THE CONSOLE THAT WOULD CARRY ITS OWN ANSWER. `runs.exec` waits for the child, the
# child stops the console three seconds in, and the reply arrives after the console is gone — so a
# lost answer or a 502/504 here is the expected shape of a launched swap, not proof it did not run.
# Only a 503 (nothing was written) still names the door and stops; the poll below is what decides.
launch() { # launch <tool> <arguments json> <what> — 0 answered, 1 outcome unknown
    local raw status body
    if ! raw=$(mcp_raw "$1" "$2"); then
        echo "$3: the console restarts as part of the swap, so its answer can be lost; polling for $VERSION" >&2
        return 1
    fi
    status=${raw##*$'\n'}
    body=${raw%$'\n'*}
    case "$status" in
        200) return 0 ;;
        502 | 504)
            echo "$3: /mcp answered HTTP $status (the request may have run); polling for $VERSION" >&2
            return 1
            ;;
        *) door_failure "$3" "$status" "$body" ;;
    esac
}

# The door is a deploy step. `--build-only` builds a binary for a later deploy and has no host call
# to fail, so it does not require the host device; every other mode asks the door before cargo.
if [ "$MODE" != "--build-only" ]; then
    # `/health` is unauthenticated and tells the console apart from its devices: if it does not
    # answer, the console itself is down and there is no door to ask. It also carries the facts a
    # failure quotes.
    HEALTH=$(curl -fsS --max-time 10 "$CONSOLE/health" 2>/dev/null) || {
        echo "the console at $CONSOLE/health did not answer: a console deploy cannot begin while the console is down" >&2
        exit 1
    }
    HEALTH_FACTS=$(printf '%s' "$HEALTH" | python3 -c '
import json, sys
try:
    data = json.load(sys.stdin)
except Exception:
    raise SystemExit
print("%s; %s of %s devices online" % (
    data.get("version", "?"), data.get("online", "?"), data.get("devices", "?")))
' 2>/dev/null || true)
    [ -n "$HEALTH_FACTS" ] || HEALTH_FACTS="version and fleet unknown"

    # state.get is read-only and cheap; it answers with the host's own state when the door is open
    # and never executes anything on the host. A closed door stops here, before cargo and the fetch.
    DOOR=$(mcp_raw state.get '{}') || {
        echo "the console did not answer at $CONSOLE/mcp: a console deploy cannot begin without the console" >&2
        exit 1
    }
    DOOR_STATUS=${DOOR##*$'\n'}
    DOOR_BODY=${DOOR%$'\n'*}
    [ "$DOOR_STATUS" = 200 ] ||
        door_failure "the host device's door is closed; nothing was built or fetched" "$DOOR_STATUS" "$DOOR_BODY"
    echo "the host device's door is open: $HOST_NAME is connected; the fetch and the swap will run through its shell ($HEALTH_FACTS)"
fi

[ "$MODE" = "--preflight-only" ] && exit 0

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

# Every host call parses the raw JSON-RPC envelope, so `mcp` returns it whole. The door preflight
# above uses the same function, which is why a 503 during the fetch or the swap is reported in the
# same words as the preflight instead of as a bare HTTP error.
mcp runs.exec "{\"command\":\"curl.exe\",\"args\":[\"-fsS\",\"--retry\",\"2\",\"-o\",\"C:\\\\SummriseConsole\\\\summrise-console.exe.new\",\"$URL/summrise-console-$VERSION.exe\"],\"timeout_ms\":120000}" "the fetch did not reach the host device" >/dev/null
REMOTE=$(mcp runs.exec '{"command":"powershell.exe","args":["-NoProfile","-Command","(Get-FileHash C:\\SummriseConsole\\summrise-console.exe.new -Algorithm SHA256).Hash.ToLower()"]}' "the digest read did not reach the host device" | python3 -c 'import json,sys; outer=json.load(sys.stdin); inner=json.loads(outer["result"]["content"][0]["text"]); print(inner["stdout"].strip())')
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
# THE END RETURNS WHEN THE CONTROL IS ACCEPTED, NOT WHEN THE PROCESS IS GONE. Moving the new binary
# over one still mapped fails with "Cannot create a file when that file already exists" (measured
# 2026-11-06); wait for the image to leave, and stop it if it is still there after 10 s.
for (\$i = 0; \$i -lt 20; \$i++) {
  if (-not (Get-Process summrise-console -ErrorAction SilentlyContinue)) { break }
  Start-Sleep -Milliseconds 500
}
\$p = Get-Process summrise-console -ErrorAction SilentlyContinue
if (\$p) { Say ('process ' + \$p.Id + ' did not stop; stopping it'); Stop-Process -Id \$p.Id -Force; Start-Sleep -Seconds 2 }
Copy-Item -Path \$live -Destination (Join-Path \$dir 'summrise-console.exe.bak-live') -Force
Remove-Item -Path \$live -Force -ErrorAction SilentlyContinue
Move-Item -Path \$new -Destination \$live -Force
if (-not (Test-Path \$live)) { Copy-Item -Path (Join-Path \$dir 'summrise-console.exe.bak-live') -Destination \$live -Force; Say 'move failed; the live image was restored from bak-live' }
schtasks /run /tn SummriseConsole | Out-Null
Start-Sleep -Seconds 8
try { \$h = Invoke-RestMethod -Uri 'http://127.0.0.1:7736/health' -TimeoutSec 5; Say ('health ' + (\$h | ConvertTo-Json -Compress)) } catch { Say ('health failed ' + \$_.Exception.Message) }
PS1
)
ENCODED=$(printf '%s' "$SWAP" | iconv -f utf-8 -t utf-16le | base64 -w0)
if launch runs.exec "{\"command\":\"powershell.exe\",\"args\":[\"-NoProfile\",\"-EncodedCommand\",\"$ENCODED\"],\"timeout_ms\":60000}" "the swap call" >/dev/null; then
    echo "swap launched (detached)"
else
    echo "the swap's answer was lost in the restart; polling for $VERSION"
fi

for _ in $(seq 1 30); do
  LIVE=$(curl -sS --max-time 5 "$CONSOLE/health" 2>/dev/null || true)
  if echo "$LIVE" | grep -q "$VERSION"; then
    echo "$LIVE"
    rm -rf "$STAGE"
    exit 0
  fi
  sleep 3
done
echo "the console did not report $VERSION within 90s" >&2
# The swap's own log is on the host, not here. Read its tail through the same door so the failure
# names what the host did; if the door closed meanwhile, that read is the line that says so.
SWAP_LOG=$(mcp runs.exec "{\"command\":\"powershell.exe\",\"args\":[\"-NoProfile\",\"-Command\",\"Get-Content -LiteralPath 'C:\\\\SummriseConsole\\\\swap-$VERSION.log' -Tail 20\"],\"timeout_ms\":30000}" "the swap log read did not reach the host device" | python3 -c '
import json, sys
try:
    outer = json.load(sys.stdin)
    inner = json.loads(outer["result"]["content"][0]["text"])
except Exception:
    raise SystemExit
print(inner.get("stdout", "").strip())
' 2>/dev/null || true)
if [ -n "$SWAP_LOG" ]; then
  echo "the host's swap log:" >&2
  printf '%s\n' "$SWAP_LOG" >&2
else
  echo "the host's swap log could not be read; read C:\\SummriseConsole\\swap-$VERSION.log through the host's shell" >&2
fi
exit 1
